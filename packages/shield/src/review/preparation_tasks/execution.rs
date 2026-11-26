//! Bounded execution owns a kernel lease; RPC contexts never enter workers.
use super::{
    CAPTURE_BYTES, Context, Directory, Error, MAX_RECORDS, Path, RESULT_BYTES, Record, Runtime,
    Store, Value, clock, files, storage,
};
use ring::rand::{SecureRandom as _, SystemRandom};
use serde_json::json;

type Publish = Box<dyn Fn(&Record, Value) -> Result<Value, Error> + Send + 'static>;

pub(crate) fn eligible(arguments: &Value) -> Result<bool, Error> {
    super::super::changes::preparation::eligible(arguments)
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod process_tests;

impl Runtime {
    pub(crate) fn start(
        &self,
        context: &Context,
        arguments: &Value,
        checkpoint: super::super::Checkpoint<'_>,
        publish: Publish,
    ) -> Result<Option<Record>, Error> {
        checkpoint()?;
        let store = match self.store_controlled(checkpoint) {
            Ok(store) => store,
            Err(error @ Error::Policy(crate::policy::Error::Interrupted)) => return Err(error),
            Err(_) => return Ok(None),
        };
        let reservation = match store.reserve(arguments, checkpoint) {
            Ok(reservation) => reservation,
            Err(
                error @ (Error::Policy(crate::policy::Error::Interrupted)
                | Error::Invalid("preparation idempotency key reused with changed input")),
            ) => return Err(error),
            Err(_) => return Ok(None),
        };
        let Some((record, owner)) = reservation else {
            return Ok(None);
        };
        let Some(owner) = owner else {
            return Ok(Some(record));
        };
        let mut workers = self.workers.lock().map_err(|_| Error::Locked)?;
        let mut retained = Vec::new();
        for worker in workers.drain(..) {
            if worker.is_finished() {
                let _ = worker.join();
            } else {
                retained.push(worker);
            }
        }
        *workers = retained;
        let worker_store = store.clone();
        let id = record.id.clone();
        let mut operation = context.clone();
        // A task has its own lifetime and status sink, never the creation RPC's.
        operation.connection_id = None;
        let status_store = store.clone();
        let status_id = id.clone();
        operation.progress = super::super::progress::Progress::new(move |_, stage| {
            let _ = status_store.stage(&status_id, stage.message());
        });
        let arguments = arguments.clone();
        let trace = crate::diagnostics::trace::child("preparation");
        let worker_trace = trace.clone();
        if let Ok(worker) = std::thread::Builder::new()
            .name("shield-preparation".into())
            .spawn(move || {
                let _owner = owner;
                let _scope = worker_trace
                    .as_ref()
                    .map(crate::diagnostics::trace::Request::enter);
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    worker_store.run(&id, &operation, &arguments, &publish)
                }));
                let outcome = outcome.unwrap_or_else(|_| {
                    worker_store
                        .settle(
                            &id,
                            "failed",
                            "Preparation worker stopped; the policy files were not changed",
                            None,
                        )
                        .unwrap_or("settlement_failed")
                });
                if let Some(trace) = worker_trace {
                    trace.finish(outcome);
                }
            })
        {
            workers.push(worker);
        } else {
            let settlement = store.settle(
                &record.id,
                "failed",
                "Preparation could not start; the policy files were not changed",
                None,
            );
            if let Some(trace) = trace {
                trace.finish(
                    settlement
                        .as_ref()
                        .map_or("settlement_failed", |status| *status),
                );
            }
            settlement?;
        }
        Ok(Some(record))
    }
}

impl Store {
    fn reserve(
        &self,
        arguments: &Value,
        checkpoint: super::super::Checkpoint<'_>,
    ) -> Result<Option<(Record, Option<Directory>)>, Error> {
        self.authority()?;
        let base = storage::Store::open(Path::new(&self.project_root))?.read_following_files()?;
        base.check(&Context::new(
            self.project.clone(),
            self.project_root.clone(),
            "mcp",
        ))?;
        base.check_operations()?;
        let base_binding = super::super::changes::prepared_binding(&base);
        let request_digest = files::digest(arguments)?;
        let key = match arguments.get("idempotency_key") {
            Some(key) => files::digest(key)?,
            None => files::digest(&json!([
                request_digest,
                base_binding,
                env!("CARGO_PKG_VERSION")
            ]))?,
        };
        // Loading policy and preparing the binding must not hold up polls or
        // terminal writes. The worker revalidates the captured base before work.
        files::writer_directory(&self.root, true)?;
        storage::objects::initialize_mutex(&self.root)?;
        let _lock = storage::objects::write_lease(&self.root, checkpoint)?;
        checkpoint()?;
        self.normalize_record_directories()?;
        let now = clock::now_ms()?;
        let mut retained = 0;
        let mut running = 0;
        for id in self.entries()? {
            let Some(mut record) = self.record_or_recover(&id)? else {
                if self.directory(&id)?.exists() {
                    retained += 1;
                    running += 1;
                }
                continue;
            };
            if now < record.created || now < record.updated {
                return Err(Error::Invalid("preparation clock regressed"));
            }
            let owner = match Directory::try_enter(&self.directory(&id)?) {
                Ok(owner) => Some(owner),
                Err(Error::Locked) => None,
                Err(error) => return Err(error),
            };
            let same_project = record.project == self.project;
            if (!same_project || !record.retained(now)) && owner.is_some() {
                files::remove(&self.directory(&id)?)?;
                continue;
            }
            if record.status == "working" {
                if owner.is_none() {
                    running += 1;
                } else {
                    record.status = if record.cancel_requested {
                        "cancelled"
                    } else {
                        "failed"
                    }
                    .into();
                    record.updated = now.max(record.updated);
                    record.message =
                        "Preparation owner stopped; if still needed, prepare with a new idempotency_key"
                            .into();
                    self.save(&record)?;
                }
            }
            if same_project && record.key == key {
                if record.request_digest != request_digest {
                    return Err(Error::Invalid(
                        "preparation idempotency key reused with changed input",
                    ));
                }
                return Ok(Some((record, None)));
            }
            retained += 1;
        }
        if retained >= MAX_RECORDS || running >= 2 {
            return Ok(None);
        }
        let (record, owner) = self.create_record(
            arguments,
            key,
            request_digest,
            base_binding,
            now,
            checkpoint,
        )?;
        Ok(Some((record, Some(owner))))
    }

    /// Publish initial task state while admission holds the namespace lease.
    fn create_record(
        &self,
        arguments: &Value,
        key: String,
        request_digest: String,
        base_binding: String,
        now: u64,
        checkpoint: super::super::Checkpoint<'_>,
    ) -> Result<(Record, Directory), Error> {
        checkpoint()?;
        let mut random = [0_u8; 32];
        SystemRandom::new()
            .fill(&mut random)
            .map_err(|_| Error::Invalid("task randomness unavailable"))?;
        let id = crate::policy::fingerprint(&random);
        let directory = self.directory(&id)?;
        if directory.exists() {
            return Err(Error::Invalid("task identity collision"));
        }
        files::directory(&directory, true)?;
        let owner = Directory::enter(&directory)?;
        let record = Record::new(&self.project, id, key, request_digest, base_binding, now);
        let result = (|| {
            files::write(&directory.join("input.json"), arguments, 524_288)?;
            checkpoint()?;
            self.save(&record)?;
            #[cfg(unix)]
            storage::sync(&std::fs::File::open(&self.root)?)?;
            Ok::<_, Error>(())
        })();
        if let Err(error) = result {
            files::remove(&directory)?;
            return Err(error);
        }
        Ok((record, owner))
    }

    fn stage(&self, id: &str, message: &str) -> Result<(), Error> {
        let checkpoint = || self.check_working(id);
        let _lock = storage::objects::write_lease(&self.root, &checkpoint)?;
        let mut record = self.get(id)?;
        if record.status == "working" && !record.cancel_requested && record.message != message {
            record.message = message.into();
            record.updated = clock::now_ms()?.max(record.updated);
            self.save(&record)?;
        }
        Ok(())
    }

    fn check_working(&self, id: &str) -> Result<(), crate::policy::Error> {
        if self
            .control(id)
            .is_ok_and(|record| record.status == "working" && !record.cancel_requested)
        {
            Ok(())
        } else {
            Err(crate::policy::Error::Interrupted)
        }
    }

    fn run(
        &self,
        id: &str,
        context: &Context,
        arguments: &Value,
        publish: &Publish,
    ) -> &'static str {
        let checkpoint = || self.check_working(id);
        let result = (|| {
            let record = self.get(id)?;
            let base =
                storage::Store::open(Path::new(&self.project_root))?.read_following_files()?;
            base.check(context)?;
            base.check_operations()?;
            if super::super::changes::prepared_binding(&base) != record.base_binding {
                return Err(Error::StaleSnapshot);
            }
            let capture =
                super::super::changes::preparation::capture(context, arguments, &checkpoint)?;
            if capture.prepared.binding != record.base_binding {
                return Err(Error::StaleSnapshot);
            }
            checkpoint()?;
            let revision = capture.revision.clone();
            let candidate_digest = crate::review::candidate_identity(&capture.prepared.candidate);
            let capture =
                serde_json::to_value(capture).map_err(|_| Error::Invalid("capture encoding"))?;
            let directory = self.directory(id)?;
            self.authority()?;
            files::write(&directory.join("capture.json"), &capture, CAPTURE_BYTES)?;
            let data = json!({"schema_version":1,"project_id":self.project,"policy_revision":revision,
                "data":{"preparation_task_id":id,"candidate_digest":candidate_digest,"policy_revision":revision,
                    "next_action":"Preparation captured; call policy_propose with preparation_task_id to create a fresh review in this connection, then obtain separate human approval using its unchanged apply_arguments.",
                    "review_arguments":{"preparation_task_id":id},"policy_changed":false},
                "missing":[],"truncated":false,"redacted":false,"error":null});
            let result = publish(&self.get(id)?, data)?;
            files::write(&directory.join("result.json"), &result, RESULT_BYTES)?;
            checkpoint()?;
            self.settle(
                id,
                "completed",
                "Preparation captured; fresh review and human approval are still required",
                Some((files::digest(&capture)?, files::digest(&result)?)),
            )
        })();
        result.unwrap_or_else(|error| {
            let message = if matches!(error, Error::StaleSnapshot) {
                "Policy base changed; request fresh preparation with a new idempotency_key if still needed"
            } else if matches!(error, Error::Policy(crate::policy::Error::Limit)) {
                "Preparation exceeded its durable capture/result bound; no proposal was applied"
            } else {
                "Preparation interrupted or failed; no proposal was applied. If still needed, prepare with a new idempotency_key"
            };
            self.settle(id, "failed", message, None).unwrap_or("settlement_failed")
        })
    }

    fn settle(
        &self,
        id: &str,
        status: &'static str,
        message: &str,
        result: Option<(String, String)>,
    ) -> Result<&'static str, Error> {
        // Settlement outlives the creation RPC.
        // Keep the worker's owner lease until its terminal record is durable.
        files::directory(&self.root, false)?;
        let _lock = storage::objects::write_lease(&self.root, &|| Ok(()))?;
        self.authority()?;
        let mut record = self.control(id)?;
        if record.status != "working" {
            return Ok("settled_elsewhere");
        }
        let outcome = if record.cancel_requested {
            "cancelled"
        } else {
            status
        };
        record.status = outcome.into();
        record.message = if record.cancel_requested {
            "Preparation cancelled; no proposal was applied"
        } else {
            message
        }
        .into();
        record.updated = clock::now_ms()?.max(record.updated);
        if record.status == "completed"
            && let Some((capture, result)) = result
        {
            record.capture_digest = Some(capture);
            record.result_digest = Some(result);
        }
        self.save(&record)?;
        Ok(outcome)
    }
}
