//! Private durable preparation records; no task can change the policy files.
mod execution;
mod files;
use super::{
    Checkpoint, Context, Error, clock,
    storage::{self, objects::Directory},
};
pub(crate) use execution::eligible;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    path::{Path, PathBuf},
    sync::Mutex,
    thread::JoinHandle,
};

pub(crate) const MAX_RECORDS: usize = 32;
pub(crate) const CAPTURE_BYTES: usize = 16 * 1024 * 1024;
pub(crate) const RESULT_BYTES: usize = 1024 * 1024;
pub(crate) const TTL_MS: u64 = 24 * 60 * 60 * 1000;
const META_BYTES: usize = 16 * 1024;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Record {
    pub(crate) schema: u8,
    pub(crate) tool: String,
    pub(crate) id: String,
    pub(crate) project: String,
    pub(crate) engine: String,
    pub(crate) key: String,
    pub(crate) request_digest: String,
    pub(crate) base_binding: String,
    pub(crate) created: u64,
    pub(crate) ttl_ms: u64,
    pub(crate) updated: u64,
    pub(crate) status: String,
    pub(crate) message: String,
    pub(crate) cancel_requested: bool,
    pub(crate) capture_digest: Option<String>,
    pub(crate) result_digest: Option<String>,
}
impl Record {
    fn new(
        project: &str,
        id: String,
        key: String,
        request_digest: String,
        base_binding: String,
        now: u64,
    ) -> Self {
        Self {
            schema: 1,
            tool: "policy_propose".into(),
            id,
            project: project.into(),
            engine: env!("CARGO_PKG_VERSION").into(),
            key,
            request_digest,
            base_binding,
            created: now,
            ttl_ms: TTL_MS,
            updated: now,
            status: "working".into(),
            message: "Preparing inactive policy capture".into(),
            cancel_requested: false,
            capture_digest: None,
            result_digest: None,
        }
    }
    fn retained(&self, now: u64) -> bool {
        now < self.updated.saturating_add(self.ttl_ms)
    }
}

#[derive(Clone)]
pub(crate) struct Store {
    root: PathBuf,
    project: String,
    project_root: String,
}

pub(crate) struct Runtime {
    store: Store,
    recovered: Mutex<bool>,
    workers: Mutex<Vec<JoinHandle<()>>>,
}

pub(crate) struct Observation {
    pub(crate) record: Record,
    pub(crate) result: Option<Value>,
}

impl Runtime {
    pub(crate) fn new(context: &Context) -> Self {
        Self {
            store: Store::new(context),
            recovered: Mutex::new(false),
            workers: Mutex::new(Vec::new()),
        }
    }
    #[cfg(test)]
    pub(crate) fn store(&self) -> Result<Store, Error> {
        self.store_controlled(&|| Ok(()))
    }
    pub(crate) fn store_controlled(&self, checkpoint: Checkpoint<'_>) -> Result<Store, Error> {
        let mut recovered = self.recovered.lock().map_err(|_| Error::Locked)?;
        checkpoint()?;
        if self.store.root.exists() {
            self.store.authority()?;
            files::writer_directory(&self.store.root, false)?;
            storage::objects::initialize_mutex(&self.store.root)?;
            if !*recovered {
                self.store.recover(checkpoint)?;
            }
        }
        *recovered = true;
        Ok(self.store.clone())
    }
    /// The store for inspecting recorded task results; it never recovers
    /// workers or creates storage.
    pub(crate) fn observation_store(&self) -> &Store {
        &self.store
    }
    #[cfg(test)]
    pub(crate) fn join(&self) {
        for worker in self.workers.lock().unwrap().drain(..) {
            worker.join().unwrap();
        }
    }
}

impl Store {
    fn new(context: &Context) -> Self {
        Self {
            root: crate::layout::state(Path::new(&context.project_root)).join("preparation-tasks"),
            project: context.project_id.clone(),
            project_root: context.project_root.clone(),
        }
    }
    fn authority(&self) -> Result<(), Error> {
        let state = storage::Store::open(Path::new(&self.project_root))?.read_checkpoint()?;
        state.check(&Context::new(
            self.project.clone(),
            self.project_root.clone(),
            "mcp",
        ))?;
        state.check_operations()
    }
    fn observation_authority(&self) -> Result<(), Error> {
        let (state, pending) =
            storage::Store::inspect(Path::new(&self.project_root))?.read_diagnostic_checkpoint()?;
        if pending {
            return Err(Error::Missing);
        }
        state
            .check(&Context::new(
                self.project.clone(),
                self.project_root.clone(),
                "mcp",
            ))
            .map_err(|_| Error::Missing)?;
        state.check_operations().map_err(|_| Error::Missing)
    }
    fn directory(&self, id: &str) -> Result<PathBuf, Error> {
        if id.len() != 64
            || !id
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        {
            return Err(Error::Missing);
        }
        Ok(self.root.join(id))
    }
    fn record(&self, id: &str) -> Result<Record, Error> {
        files::directory(&self.root, false)?;
        let directory = self.directory(id)?;
        files::directory(&directory, false)?;
        let record: Record =
            serde_json::from_value(files::read(&directory.join("record.json"), META_BYTES)?)
                .map_err(|_| Error::Invalid("preparation record invalid"))?;
        Self::validate_record(&record, id)?;
        Ok(record)
    }
    fn validate_record(record: &Record, id: &str) -> Result<(), Error> {
        if record.schema != 1
            || record.tool != "policy_propose"
            || record.ttl_ms == 0
            || record.ttl_ms > TTL_MS
            || record.id != id
            || !matches!(
                record.status.as_str(),
                "working" | "completed" | "failed" | "cancelled"
            )
            || record.updated < record.created
        {
            return Err(Error::Invalid("preparation record identity invalid"));
        }
        Ok(())
    }
    pub(crate) fn get(&self, id: &str) -> Result<Record, Error> {
        self.authority()?;
        self.control(id)
    }
    fn control(&self, id: &str) -> Result<Record, Error> {
        let record = match self.record(id) {
            Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(Error::Missing);
            }
            result => result?,
        };
        let now = clock::now_ms()?;
        if record.project != self.project {
            return Err(Error::Missing);
        }
        if now < record.created || now < record.updated {
            return Err(Error::Invalid("preparation clock regressed"));
        }
        if record.status != "working" && !record.retained(now) {
            return Err(Error::Missing);
        }
        Ok(record)
    }
    fn save(&self, record: &Record) -> Result<(), Error> {
        files::write(
            &self.directory(&record.id)?.join("record.json"),
            &serde_json::to_value(record).map_err(|_| Error::Invalid("task encoding"))?,
            META_BYTES,
        )
    }
    fn read_result(&self, record: &Record) -> Result<Value, Error> {
        let value = files::read(
            &self.directory(&record.id)?.join("result.json"),
            RESULT_BYTES,
        )?;
        if record.result_digest.as_deref() != Some(files::digest(&value)?.as_str()) {
            return Err(Error::Invalid("preparation result changed"));
        }
        Ok(value)
    }
    pub(in crate::review) fn capture(
        &self,
        id: &str,
    ) -> Result<super::changes::preparation::Capture, Error> {
        let record = self.get(id)?;
        if record.status != "completed" || record.engine != env!("CARGO_PKG_VERSION") {
            return Err(Error::Invalid(
                "preparation is incomplete or needs fresh preparation with this engine",
            ));
        }
        let value = files::read(&self.directory(id)?.join("capture.json"), CAPTURE_BYTES)?;
        if record.capture_digest.as_deref() != Some(files::digest(&value)?.as_str()) {
            return Err(Error::Invalid("preparation capture changed"));
        }
        serde_json::from_value(value).map_err(|_| Error::Invalid("preparation capture invalid"))
    }
    pub(crate) fn cancel(&self, id: &str, checkpoint: Checkpoint<'_>) -> Result<(), Error> {
        self.directory(id)?;
        let _lock = match storage::objects::write_lease(&self.root, checkpoint) {
            Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(Error::Missing);
            }
            result => result?,
        };
        checkpoint()?;
        self.authority()?;
        let mut record = self.control(id)?;
        self.reconcile(&mut record)?;
        if record.status == "working" && !record.cancel_requested {
            record.cancel_requested = true;
            record.updated = clock::now_ms()?.max(record.updated);
            record.message = "Cancellation requested; preparation may already be finishing".into();
            self.save(&record)?;
        }
        Ok(())
    }
    pub(crate) fn observe(
        &self,
        id: &str,
        include_result: bool,
        checkpoint: Checkpoint<'_>,
    ) -> Result<Observation, Error> {
        self.directory(id)?;
        match files::directory(&self.root, false) {
            Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(Error::Missing);
            }
            result => result?,
        }
        let namespace = match storage::objects::read_lease(&self.root, Some(checkpoint)) {
            Ok(lock) => Some(lock),
            Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error),
        };
        checkpoint()?;
        self.observation_authority()?;
        let record = self.observed_record(id, namespace.is_some())?;
        let result = if include_result && record.status == "completed" {
            Some(self.read_result(&record)?)
        } else {
            None
        };
        checkpoint()?;
        Ok(Observation { record, result })
    }
    fn observed_record(&self, id: &str, check_owner: bool) -> Result<Record, Error> {
        let mut record = self.control(id)?;
        if record.status == "working" {
            let owner = if check_owner {
                // Execution holds an exclusive lease. Other polls can share
                // this probe without looking like a running worker.
                match storage::objects::read_lease(&self.directory(id)?, None) {
                    Ok(owner) => Some(owner),
                    Err(Error::Locked) => return Ok(record),
                    Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => None,
                    Err(error) => return Err(error),
                }
            } else {
                None
            };
            if owner.is_some() {
                record.status = if record.cancel_requested {
                    "cancelled"
                } else {
                    "failed"
                }
                .into();
                record.message = "Preparation owner stopped; the policy files were not changed. Observed without changing the retained record; prepare with a new idempotency_key if still needed".into();
            } else {
                record.message = "Stored preparation status; owner liveness is unchecked because its existing mutex is unavailable".into();
            }
        }
        Ok(record)
    }
    fn reconcile(&self, record: &mut Record) -> Result<(), Error> {
        if record.status != "working" {
            return Ok(());
        }
        let _owner = match Directory::try_enter(&self.directory(&record.id)?) {
            Ok(owner) => owner,
            Err(Error::Locked) => return Ok(()),
            Err(error) => return Err(error),
        };
        record.status = if record.cancel_requested {
            "cancelled"
        } else {
            "failed"
        }
        .into();
        record.updated = clock::now_ms()?.max(record.updated);
        record.message = "Preparation owner stopped; the policy files were not changed. If still needed, prepare with a new idempotency_key".into();
        self.save(record)
    }
    fn entries(&self) -> Result<Vec<String>, Error> {
        let mut ids = Vec::new();
        let mut bytes = 0_u64;
        for (index, entry) in std::fs::read_dir(&self.root)?
            .take(MAX_RECORDS + 66)
            .enumerate()
        {
            if index >= MAX_RECORDS + 65 {
                return Err(Error::Invalid("preparation namespace exceeds scan bound"));
            }
            let entry = entry?;
            if entry.file_name() == ".objects.lock" {
                continue;
            }
            let id = entry.file_name().to_string_lossy().into_owned();
            let metadata = std::fs::symlink_metadata(entry.path())?;
            if self.directory(&id).is_err()
                || !metadata.is_dir()
                || metadata.file_type().is_symlink()
            {
                bytes = bytes.saturating_add(metadata.len());
                if bytes > 640 * 1024 * 1024 {
                    return Err(Error::Invalid("preparation storage exceeds its byte bound"));
                }
                continue;
            }
            files::directory(&entry.path(), false)?;
            bytes = bytes.saturating_add(files::inventory(&entry.path())?);
            if bytes > 640 * 1024 * 1024 {
                return Err(Error::Invalid("preparation storage exceeds its byte bound"));
            }
            ids.push(id);
        }
        if ids.len() > MAX_RECORDS {
            return Err(Error::Invalid(
                "preparation task directory exceeds its bound",
            ));
        }
        Ok(ids)
    }
    /// The namespace is already private and exclusively owned by this writer.
    /// Lost record-directory privacy is repaired only with a valid owned codec.
    fn normalize_record_directories(&self) -> Result<(), Error> {
        for (index, entry) in std::fs::read_dir(&self.root)?
            .take(MAX_RECORDS + 66)
            .enumerate()
        {
            if index >= MAX_RECORDS + 65 {
                return Err(Error::Invalid("preparation directory exceeds scan bound"));
            }
            let entry = entry?;
            let Some(id) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if self.directory(&id).is_err() {
                continue;
            }
            let metadata = std::fs::symlink_metadata(entry.path())?;
            if !metadata.is_dir() || metadata.file_type().is_symlink() {
                continue;
            }
            if files::directory(&entry.path(), false).is_ok() {
                continue;
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt as _;
                if metadata.uid() != std::fs::symlink_metadata(&self.root)?.uid() {
                    return Err(Error::Invalid("task record directory owner differs"));
                }
            }
            let record: Record =
                serde_json::from_value(files::read(&entry.path().join("record.json"), META_BYTES)?)
                    .map_err(|_| Error::Invalid("preparation record invalid"))?;
            Self::validate_record(&record, &id)?;
            files::writer_directory(&entry.path(), false)?;
        }
        Ok(())
    }
    fn recover(&self, checkpoint: Checkpoint<'_>) -> Result<(), Error> {
        let _lock = storage::objects::write_lease(&self.root, checkpoint)?;
        checkpoint()?;
        self.authority()?;
        self.normalize_record_directories()?;
        for id in self.entries()? {
            let Some(mut record) = self.record_or_recover(&id)? else {
                continue;
            };
            if record.status != "working" {
                continue;
            }
            let _owner = match Directory::try_enter(&self.directory(&id)?) {
                Ok(owner) => owner,
                Err(Error::Locked) => continue,
                Err(error) => return Err(error),
            };
            record.status = if record.cancel_requested {
                "cancelled"
            } else {
                "failed"
            }
            .into();
            record.updated = clock::now_ms()?.max(record.updated);
            record.message = "Preparation owner stopped before settlement; the policy files were not changed. If still needed, prepare with a new idempotency_key".into();
            self.save(&record)?;
        }
        Ok(())
    }
    fn record_or_recover(&self, id: &str) -> Result<Option<Record>, Error> {
        Ok(Some(match self.record(id) {
            Ok(record) => record,
            Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
                let _owner = match Directory::try_enter(&self.directory(id)?) {
                    Ok(owner) => owner,
                    Err(Error::Locked) => return Ok(None),
                    Err(error) => return Err(error),
                };
                files::remove(&self.directory(id)?)?;
                return Ok(None);
            }
            Err(error) => return Err(error),
        }))
    }
}

pub(in crate::review) fn review_input(
    context: &Context,
    id: &str,
) -> Result<super::changes::Input, Error> {
    let capture = Store::new(context).capture(id)?;
    if let Some(expected) = &capture.global_sources
        && expected != &serde_json::json!(super::registries::global()?)
    {
        return Err(Error::StaleSnapshot);
    }
    if capture.engine != env!("CARGO_PKG_VERSION") {
        return Err(Error::StaleSnapshot);
    }
    let mut input: super::changes::Input = serde_json::from_value(capture.input)
        .map_err(|_| Error::Invalid("preparation input invalid"))?;
    input.prepared = Some(capture.prepared);
    input.expected_policy_revision = Some(capture.revision);
    input.idempotency_key = Some(format!("preparation-{id}"));
    Ok(input)
}
