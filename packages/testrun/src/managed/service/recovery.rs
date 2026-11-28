//! The control subset every manager serves whatever its protocol. The
//! library answers identity, retire-if-empty and drain; Testrun's control
//! extensions add the pool and blocker inventory, release retry and
//! operation lookup. These are admitted outside any device queue; a release
//! itself runs under the device's existing resource owner.
use super::{Kind, Service};
use crate::{
    error::{Error, Result},
    managed::{Caller, storage},
};
use canonic_process_kit::core::Hold;
use serde_json::{Value, json};
use std::{path::PathBuf, sync::Arc, time::Duration};

impl Service {
    /// `hold` keeps a hosted manager running for work that outlasts the
    /// reply, such as a retry past its budget.
    pub(crate) async fn control(
        self: &Arc<Self>,
        kind: &str,
        request: &Value,
        hold: Option<Hold>,
    ) -> Result<Value> {
        match kind {
            "pool" => Ok(self.pool()),
            "blockers" => Ok(self.blockers()),
            "retry" => Ok(self.retry(request, hold).await),
            "operation" => self.operation(request),
            _ => Err(Error::invalid("Unsupported control request")),
        }
    }

    fn pool(&self) -> Value {
        json!({"live":true,"owner":self.identity(),"pool":self.pool_summary()})
    }

    /// Everything that keeps a device or the manager from being released,
    /// with its cause, recovery action and expiry where known.
    pub(super) fn blockers(&self) -> Value {
        let mut blockers = Vec::new();
        let pending: Vec<(String, super::registry::Release)> = self
            .registry()
            .pool()
            .map(|pool| {
                pool.targets
                    .iter()
                    .filter_map(|(target, held)| Some((target.clone(), held.release.clone()?)))
                    .collect()
            })
            .unwrap_or_default();
        for (target, release) in pending {
            let caller = Caller::new(None, self.root.clone(), Kind::Cli).with_target(&target);
            let directory = caller.directory().ok();
            blockers.push(json!({
                "kind":"release","targetId":target,"cause":release.error,
                "attempts":release.attempts,"retrying":true,
                "recoveryAction":"automatic; testrun manager retry runs it now",
                "expiry":directory.as_deref().map_or(json!({"unknown":true}), expiry),
                "cleanup":directory.and_then(|directory| caller.cleanup_inventory(&directory).ok()),
            }));
        }
        for actor in self.supervisor.inventory() {
            if actor["state"] == "quarantined" {
                blockers.push(json!({"kind":"quarantine","targetId":actor["targetId"],"generation":actor["generation"],"cause":actor["cause"],"recoveryAction":"testrun manager retry","expiry":{"unknown":true}}));
            }
        }
        blockers.extend(
            self.lost
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .iter()
                .cloned(),
        );
        json!({"blockers":blockers,"owner":self.identity()})
    }

    /// Run unfinished releases now instead of at their next automatic
    /// attempt: one target, or every blocker. Completed stages are kept; an
    /// attempt over budget returns a pending handle.
    async fn retry(self: &Arc<Self>, request: &Value, hold: Option<Hold>) -> Value {
        let operation = Operation::begin(&self.root, "retry", request);
        let service = self.clone();
        let named = request["targetId"].as_str().map(str::to_owned);
        let targets = match &named {
            Some(target) => vec![self.held_target(target).unwrap_or_else(|| target.clone())],
            None => self.retry_targets(),
        };
        let durable = if named.is_none() {
            super::super::transition::unfinished(&self.root)
        } else {
            Vec::new()
        };
        if targets.is_empty() && durable.is_empty() && named.is_none() {
            super::super::transition::settle(service).await;
            return operation.finish(Ok(json!({"recovered":true,"retried":[]})));
        }
        let attempt = async move {
            let mut failures = Vec::new();
            for target in &targets {
                if let Err(error) = service.release(target, Kind::Cli, None).await {
                    failures.push(json!({"targetId":target,"error":error}));
                }
            }
            let pooled = service.registry().pool().map(|pool| pool.directory.clone());
            for directory in durable {
                // The live pool's actors are released above, never settled here.
                if pooled.as_ref() == Some(&directory) {
                    continue;
                }
                let caller = Caller::new(Some(service.clone()), service.root.clone(), Kind::Cli);
                if let Err(error) = caller.settle_stopped(&directory).await {
                    failures.push(json!({"directory":directory,"error":error}));
                }
            }
            if named.is_none() {
                // Refresh the lost-manager report from what remains.
                super::super::transition::settle(service.clone()).await;
            }
            // The attempt may outlast its reply; it held the manager until
            // here and hands over to the busy hold if a device remains.
            service.keep(hold.as_ref());
            drop(hold);
            if failures.is_empty() {
                Ok(json!({"recovered":true,"retried":targets}))
            } else {
                Err(
                    Error::new("CLEANUP_FAILED", "Recovery left unfinished releases")
                        .details(json!({"failures":failures})),
                )
            }
        };
        let budget = self.recovery_budget;
        operation.run(attempt, budget).await
    }

    /// Pooled targets with a failed release or a quarantined device actor.
    fn retry_targets(&self) -> Vec<String> {
        let targets: Vec<(String, bool)> = self
            .registry()
            .pool()
            .map(|pool| {
                pool.targets
                    .iter()
                    .map(|(target, held)| (target.clone(), held.release.is_some()))
                    .collect()
            })
            .unwrap_or_default();
        targets
            .into_iter()
            .filter(|(target, pending)| *pending || self.supervisor.quarantined(target).is_some())
            .map(|(target, _)| target)
            .collect()
    }

    fn operation(&self, request: &Value) -> Result<Value> {
        let id = request["operationId"]
            .as_str()
            .filter(|id| id.chars().all(|c| c.is_ascii_hexdigit() || c == '-'))
            .ok_or_else(|| Error::invalid("Name an operationId"))?;
        storage::read::<Value>(&self.root.join("operations").join(format!("{id}.json")))?
            .ok_or_else(|| Error::new("NOT_FOUND", "No such operation"))
    }
}

/// When a blocker is expected to clear: a physical iPhone's remote media
/// expires about 22 seconds after loss was detected; other blockers have no
/// platform estimate. An elapsed estimate never proves release.
fn expiry(directory: &std::path::Path) -> Value {
    let detected = std::iter::once(directory.to_path_buf())
        .chain(crate::managed::selection::target_directories(directory))
        .flat_map(|parent| std::fs::read_dir(parent).into_iter().flatten().flatten())
        .filter_map(|entry| {
            storage::read::<u64>(&entry.path().join("recovery.json"))
                .ok()
                .flatten()
        })
        .max();
    match detected {
        Some(time) => json!({"estimatedAt":(time + 22) * 1000}),
        None => json!({"unknown":true}),
    }
}

/// A durable handle for a recovery attempt.
struct Operation {
    path: PathBuf,
    record: Value,
}
impl Operation {
    fn begin(root: &std::path::Path, kind: &str, request: &Value) -> Self {
        let id = uuid::Uuid::new_v4().to_string();
        let directory = root.join("operations");
        let _ = storage::directory(&directory);
        // Durable as pending until the attempt settles, so a lost reply or a
        // manager loss never reads as completed.
        let record = json!({
            "operationId":id,"kind":kind,"state":"pending","targetId":request["targetId"],
            "cause":"The attempt has not finished","expiry":{"unknown":true},
            "started":super::super::now(),"updated":super::super::now(),
        });
        let path = directory.join(format!("{id}.json"));
        let _ = storage::write(&path, &record);
        Self { path, record }
    }
    fn finish(mut self, result: Result<Value>) -> Value {
        if let Some(record) = self.record.as_object_mut() {
            record.remove("cause");
            record.remove("expiry");
        }
        match result {
            Ok(value) => {
                self.record["state"] = json!("completed");
                self.record["result"] = value;
            }
            Err(error) => {
                self.record["state"] = json!("failed");
                self.record["error"] = json!(error);
            }
        }
        self.record["updated"] = json!(super::super::now());
        let _ = storage::write(&self.path, &self.record);
        self.record
    }
    /// Run `attempt` in its own task. Answer with its outcome within
    /// `budget`, or with a durable pending record the task completes later.
    async fn run(
        self,
        attempt: impl std::future::Future<Output = Result<Value>> + Send + 'static,
        budget: Duration,
    ) -> Value {
        let pending = self.record.clone();
        let (sender, receiver) = tokio::sync::oneshot::channel();
        super::detached(move || async move {
            let record = self.finish(attempt.await);
            let _ = sender.send(record);
        });
        match tokio::time::timeout(budget, receiver).await {
            Ok(Ok(record)) => record,
            _ => pending,
        }
    }
}
