//! Manager-owned diagnostic sessions, bounded collectors and retained private output.
pub(crate) mod access;
pub(crate) mod analysis;
pub(crate) mod android;
pub(crate) mod apple;
pub(crate) mod dart;
mod embedded;
mod guidance;
mod host;
mod inspector;
mod model;
pub(crate) mod native_trace;
mod observability;
mod paging;
mod privacy;
mod projection;
pub(crate) mod react_native;
mod reproduction;
mod source;
mod surface;
#[cfg(test)]
mod tests;
use crate::{
    artifacts, builds,
    error::{Error, Result},
    managed::storage,
};
use model::{Binding, Collector, Investigation, Limits, Phase, Source};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs::OpenOptions,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
pub(crate) use surface::{definition, tools};
use tokio::sync::watch;

pub(crate) fn handles(name: &str) -> bool {
    surface::NAMES.contains(&name)
}
pub(crate) fn inspects(name: &str) -> bool {
    matches!(
        name,
        "diagnostics_status"
            | "diagnostics_read"
            | "diagnostics_stop"
            | "diagnostics_analysis_status"
            | "diagnostics_analysis_cancel"
            | "diagnostics_query"
            | "diagnostics_compare"
            | "diagnostics_project"
    )
}
#[derive(Default)]
struct State {
    records: HashMap<PathBuf, Investigation>,
    running: HashMap<PathBuf, watch::Sender<Option<String>>>,
    analysis_indexes: HashMap<PathBuf, (String, Arc<analysis::model::Index>)>,
}
#[derive(Clone)]
pub(crate) struct Owners {
    state: Arc<Mutex<State>>,
    verified_outputs: Arc<Mutex<HashMap<PathBuf, String>>>,
    incarnation: String,
    offline: bool,
}
impl Default for Owners {
    fn default() -> Self {
        let mut owner = Self::new(uuid::Uuid::new_v4().to_string());
        owner.offline = true;
        owner
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Open {
    submission_id: String,
    question: String,
    #[serde(default)]
    binding: Binding,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Start {
    investigation_id: String,
    submission_id: String,
    source: Source,
    #[serde(default)]
    limits: Limits,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Select {
    investigation_id: String,
    collector_id: Option<String>,
    cursor: Option<String>,
    limit: Option<u64>,
}
fn parse<T: serde::de::DeserializeOwned>(value: &Value) -> Result<T> {
    serde_json::from_value(value.clone())
        .map_err(|_| Error::invalid("Invalid diagnostic arguments"))
}
fn digest(value: &[u8]) -> String {
    format!("{:x}", Sha256::digest(value))
}
fn id(prefix: &str, submission: &str) -> Result<String> {
    model::label(submission, 128)?;
    Ok(format!("{prefix}{}", &digest(submission.as_bytes())[..32]))
}
fn validate_id(value: &str, prefix: &str) -> Result<()> {
    if value.len() != prefix.len() + 32
        || !value.starts_with(prefix)
        || !value[prefix.len()..].bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err(Error::invalid("Invalid diagnostic reference"));
    }
    Ok(())
}
fn directory(root: &Path, workspace: &str, investigation: &str) -> Result<PathBuf> {
    validate_id(investigation, "diag-")?;
    Ok(root
        .join("diagnostics")
        .join(digest(workspace.as_bytes()))
        .join(investigation))
}
fn persist(dir: &Path, record: &Investigation) -> Result<()> {
    storage::write(&dir.join("session.json"), record)
}
fn final_phase(reason: &str, bytes: usize) -> Phase {
    if reason == "source_complete" {
        Phase::Complete
    } else if bytes > 0
        || matches!(
            reason,
            "stopped"
                | "duration_limit"
                | "capture_limit"
                | "target_released"
                | "manager_stopping"
                | "manager_draining"
                | "owner_lost"
        )
    {
        Phase::Partial
    } else {
        Phase::Failed
    }
}
fn public(record: &Investigation) -> Value {
    let collectors: Vec<_> = record.collectors.iter().map(|c| json!({
        "collectorId":c.id,"sourceKind":c.source.kind(),
        "sourceIdentity":c.source_identity,"phase":c.phase,"phases":c.phases,"settled":c.phase.settled(),
        "bytes":c.bytes,"events":c.events,"eventUnit":"read-chunk","sourceOffset":c.source_offset,
        "reason":c.reason,"sha256":c.sha256,"limits":c.limits,"private":true,
        "format":c.source.format(),"sourceReport":guidance::source_report(&c.source_report),"nextOperation":guidance::collector_next(&record.id,&c.id,c.phase.settled()),
        "scope":{"coverage":"selected_source; may include other processes","appIsolation":"not_verified"},"cost":c.cost
    })).collect();
    privacy::ordinary(
        json!({"investigationId":record.id,"question":record.question,"binding":record.binding,"bindingProvenance":"caller_declared",
        "closed":record.closed,"settled":record.collectors.iter().all(|c|c.phase.settled()),"collectors":collectors,"private":true,"nextOperation":if record.collectors.is_empty(){json!({"tool":"diagnostics_start","arguments":{"investigationId":record.id},"required":"select a source from diagnostics_capabilities and a stable submissionId"})}else{json!({"tool":"diagnostics_status","arguments":{"investigationId":record.id},"purpose":"inspect collector nextOperation; large content requires diagnostics_read/query"})}}),
    )
}
impl Owners {
    pub(crate) fn new(incarnation: String) -> Self {
        Self {
            state: Arc::default(),
            verified_outputs: Arc::default(),
            incarnation,
            offline: false,
        }
    }
    pub(crate) fn active(&self) -> bool {
        !self.state.lock().unwrap().running.is_empty()
    }
    pub(crate) fn active_count(&self) -> usize {
        self.state
            .lock()
            .unwrap()
            .running
            .keys()
            .filter(|key| {
                key.file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .starts_with("collector-")
            })
            .count()
    }
    pub(crate) fn active_analysis_count(&self) -> usize {
        self.state
            .lock()
            .unwrap()
            .running
            .keys()
            .filter(|key| {
                key.file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .starts_with("analysis-")
            })
            .count()
    }
    fn load<'a>(&self, state: &'a mut State, dir: &Path) -> Result<&'a mut Investigation> {
        if !state.records.contains_key(dir) {
            if !dir.exists() {
                return Err(Error::new(
                    "DIAGNOSTIC_NOT_FOUND",
                    "Investigation does not exist in this workspace",
                ));
            }
            let mut record: Investigation =
                storage::read_versioned(&dir.join("session.json"), "diagnostic session", 1)?
                    .ok_or_else(|| {
                        Error::new(
                            "DIAGNOSTIC_NOT_FOUND",
                            "Investigation does not exist in this workspace",
                        )
                    })?;
            if record.id != dir.file_name().unwrap_or_default().to_string_lossy()
                || record.collectors.len() > 16
                || record.binding.validate().is_err()
                || record.collectors.iter().any(|c| {
                    validate_id(&c.id, "collector-").is_err()
                        || c.limits.validate().is_err()
                        || c.bytes > c.limits.max_bytes
                })
            {
                return Err(Error::new(
                    "STATE_IO",
                    "Diagnostic record has invalid identity or bounds",
                ));
            }
            if record.owner != self.incarnation {
                for c in &mut record.collectors {
                    if !c.phase.settled() {
                        c.phase(Phase::Partial);
                        c.reason = Some("owner_lost".into());
                        // The retained bytes survive even if the last progress publication did not.
                        let bytes = read_data(dir, &c.id)?;
                        c.bytes = bytes.len() as u64;
                        c.sha256 = Some(digest(&bytes));
                    }
                }
                record.owner.clone_from(&self.incarnation);
                if !self.offline {
                    persist(dir, &record)?;
                }
            }
            if state.records.len() >= 256 {
                let busy: Vec<_> = state
                    .running
                    .keys()
                    .filter_map(|key| key.parent().map(Path::to_path_buf))
                    .collect();
                state.records.retain(|dir, r| {
                    busy.contains(dir) || r.collectors.iter().any(|c| !c.phase.settled())
                });
            }
            state.records.insert(dir.to_owned(), record);
        }
        Ok(state.records.get_mut(dir).unwrap())
    }
    pub(crate) fn inspect(
        &self,
        root: &Path,
        workspace: &str,
        name: &str,
        args: &Value,
    ) -> Result<Value> {
        if matches!(
            name,
            "diagnostics_analysis_status"
                | "diagnostics_analysis_cancel"
                | "diagnostics_query"
                | "diagnostics_compare"
                | "diagnostics_project"
                | "diagnostics_export"
        ) {
            return self.inspect_analysis(root, workspace, name, args);
        }
        let select: Select = parse(args)?;
        let dir = directory(root, workspace, &select.investigation_id)?;
        let mut state = self.state.lock().unwrap();
        let running: Vec<_> = state.running.keys().cloned().collect();
        let record = self.load(&mut state, &dir)?;
        if !self.offline {
            recover_collector_cleanup(record, &dir, &running)?;
        }
        if name == "diagnostics_remove" {
            remove_investigation(&dir, record, &running)?;
            state.records.remove(&dir);
            state
                .analysis_indexes
                .retain(|key, _| key.parent() != Some(dir.as_path()));
            return Ok(json!({"investigationId":select.investigation_id,"removed":true}));
        }
        if name == "diagnostics_read" {
            let collector = select
                .collector_id
                .as_deref()
                .ok_or_else(|| Error::invalid("collectorId is required"))?;
            let c = record
                .collectors
                .iter()
                .find(|c| c.id == collector)
                .ok_or_else(|| {
                    Error::new(
                        "DIAGNOSTIC_NOT_FOUND",
                        "Collector does not belong to this investigation",
                    )
                })?;
            let identity = (record.id.clone(), collector.to_owned());
            let size = c.bytes;
            let settled = c.phase.settled();
            let expected = c.sha256.clone();
            let path = dir.join(format!("{collector}.bin"));
            drop(state);
            artifacts::check_private_directory(&dir)?;
            self.verify_output(&dir, &identity.1, expected.as_deref(), size)?;
            let request = canonic_mcp_kit::page::arguments(select.cursor.as_deref(), select.limit);
            let mut page = crate::pagination::stream(
                "diagnostics_read",
                &(root, workspace, &identity),
                &request,
                &path,
                Some(size),
                !settled,
                true,
            )?;
            self.verify_output(&dir, &identity.1, expected.as_deref(), size)?;
            page["investigationId"] = json!(identity.0);
            page["collectorId"] = json!(identity.1);
            page["settled"] = json!(settled);
            page["private"] = json!(true);
            return Ok(page);
        }
        if matches!(name, "diagnostics_stop" | "diagnostics_close") {
            let ids = stop_collectors(
                record,
                &select,
                name == "diagnostics_close",
                self.offline,
                &dir,
            )?;
            for id in ids {
                if let Some(sender) = state.running.get(&dir.join(id)) {
                    sender.send_replace(Some("stopped".into()));
                }
            }
            for key in &running {
                if select.collector_id.is_none()
                    && key.parent() == Some(dir.as_path())
                    && key
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .starts_with("analysis-")
                    && let Some(sender) = state.running.get(key)
                {
                    sender.send_replace(Some("stopped".into()));
                }
            }
        }
        let value = public(self.load(&mut state, &dir)?);
        Ok(with_active_analyses(value, &state, &dir))
    }
    pub(crate) fn open(&self, root: &Path, workspace: &str, args: &Value) -> Result<Value> {
        let open: Open = parse(args)?;
        model::label(&open.question, 1024)?;
        open.binding.validate()?;
        let investigation = id("diag-", &open.submission_id)?;
        let signature = digest(&serde_json::to_vec(args).unwrap());
        let dir = directory(root, workspace, &investigation)?;
        let mut state = self.state.lock().unwrap();
        if dir.exists() {
            let record = self.load(&mut state, &dir)?;
            if record.signature != signature {
                return Err(Error::new(
                    "REQUEST_CONFLICT",
                    "Investigation submissionId has different inputs",
                ));
            }
            return Ok(public(record));
        }
        for path in [
            root.to_owned(),
            root.join("diagnostics"),
            dir.parent().unwrap().to_owned(),
        ] {
            storage::directory(&path)?;
        }
        if std::fs::read_dir(dir.parent().unwrap())
            .map_err(|_| Error::new("STATE_IO", "Cannot inspect diagnostic capacity"))?
            .take(257)
            .count()
            >= 256
        {
            return Err(Error::new(
                "DIAGNOSTIC_CAPACITY",
                "Workspace retains 256 investigations; remove a closed investigation",
            ));
        }
        storage::directory(&dir)?;
        let record = Investigation {
            version: 1,
            id: investigation,
            signature,
            owner: self.incarnation.clone(),
            question: open.question,
            binding: open.binding,
            closed: false,
            collectors: vec![],
        };
        persist(&dir, &record)?;
        let value = public(&record);
        state.records.insert(dir, record);
        Ok(value)
    }
    #[cfg(test)]
    pub(crate) fn start(
        &self,
        root: &Path,
        workspace: &str,
        args: &Value,
        builds: &builds::Owners,
    ) -> Result<(Value, Option<Work>)> {
        self.start_with_browser(root, workspace, args, builds, None)
    }
    pub(crate) fn start_with_browser(
        &self,
        root: &Path,
        workspace: &str,
        args: &Value,
        builds: &builds::Owners,
        browsers: Option<(&crate::browser::runtime::Owners, &str)>,
    ) -> Result<(Value, Option<Work>)> {
        let mut start: Start = parse(args)?;
        if matches!(start.source, Source::NativeTrace { .. })
            && args["limits"].get("maxBytes").is_none()
        {
            start.limits.max_bytes = native_trace::MAX_ENVELOPE as u64;
        }
        start.limits.validate()?;
        let dir = directory(root, workspace, &start.investigation_id)?;
        let collector_id = id("collector-", &start.submission_id)?;
        let signature = digest(&serde_json::to_vec(args).unwrap());
        let mut state = self.state.lock().unwrap();
        let record = self.load(&mut state, &dir)?;
        if let Some(c) = record.collectors.iter().find(|c| c.id == collector_id) {
            if c.signature != signature {
                return Err(Error::new(
                    "REQUEST_CONFLICT",
                    "Collector submissionId has different inputs",
                ));
            }
            return Ok((public(record), None));
        }
        if record.closed {
            return Err(Error::new(
                "DIAGNOSTIC_CLOSED",
                "Open a new investigation; this one is closed",
            ));
        }
        validate_source_binding(&start.source, &record.binding)?;
        if let Source::NativeTrace { options } = &start.source {
            options.validate()?;
            options.validate_retention_budget(start.limits.max_bytes)?;
        }
        let binding = record.binding.clone();
        if record.collectors.len() >= 16 || state.running.len() >= 16 {
            return Err(Error::new(
                "DIAGNOSTIC_CAPACITY",
                "At most 16 collectors per investigation and 16 active collectors per manager",
            ));
        }
        let (mut reader, source_identity) =
            source::Reader::open(&start.source, builds, root, workspace, browsers, &binding)?;
        reader.context(&binding);
        let record = self.load(&mut state, &dir)?;
        let mut c = Collector {
            id: collector_id.clone(),
            signature,
            source: start.source,
            source_identity,
            limits: start.limits,
            phase: Phase::Admitted,
            phases: vec![Phase::Admitted],
            bytes: 0,
            events: 0,
            source_offset: 0,
            reason: None,
            sha256: None,
            cost: None,
            source_report: None,
        };
        artifacts::write(&dir.join(format!("{collector_id}.bin")), &[])?;
        c.phase(Phase::Preparing);
        let mut next = record.clone();
        next.collectors.push(c);
        if let Err(error) = persist(&dir, &next) {
            let _ = std::fs::remove_file(dir.join(format!("{collector_id}.bin")));
            return Err(error);
        }
        *record = next;
        let result = public(record);
        let (sender, cancelled) = watch::channel(None);
        state.running.insert(dir.join(&collector_id), sender);
        Ok((
            result,
            Some(Work {
                dir,
                collector: collector_id,
                reader: Some(reader),
                cancelled,
                source_read_calls: 0,
            }),
        ))
    }
    pub(crate) fn stop_all(&self, target: Option<&str>, reason: &str) {
        let state = self.state.lock().unwrap();
        for (dir, record) in &state.records {
            if target.is_none_or(|target| record.binding.target_id.as_deref() == Some(target)) {
                for (key, sender) in &state.running {
                    if key.parent() == Some(dir.as_path()) {
                        sender.send_replace(Some(reason.into()));
                    }
                }
            }
        }
    }
    fn update<T>(&self, work: &Work, change: impl FnOnce(&mut Collector) -> T) -> Result<T> {
        let mut state = self.state.lock().unwrap();
        let record = self.load(&mut state, &work.dir)?;
        let c = record
            .collectors
            .iter_mut()
            .find(|c| c.id == work.collector)
            .unwrap();
        let result = change(c);
        persist(&work.dir, record)?;
        Ok(result)
    }
    pub(crate) async fn run(
        &self,
        mut work: Work,
        root: PathBuf,
        workspace: String,
        builds: builds::Owners,
    ) {
        use futures_util::FutureExt;
        let began = Instant::now();
        let result =
            std::panic::AssertUnwindSafe(self.collect(&mut work, &root, &workspace, &builds))
                .catch_unwind()
                .await
                .unwrap_or_else(|_| {
                    Err(Error::new(
                        "COLLECTOR_STOPPED",
                        "Collector stopped unexpectedly",
                    ))
                });
        let reason = match &result {
            Ok(reason) => reason.clone(),
            Err(error) => error.code.clone(),
        };
        if let Some(reader) = work.reader.as_mut()
            && let Ok(Some(mut summary)) = std::panic::AssertUnwindSafe(reader.settle())
                .catch_unwind()
                .await
        {
            if let Err(error) = &result
                && error.code == "NATIVE_TRACE_BOUNDS"
            {
                summary["retentionError"] = json!(error);
            }
            let _ = self.update(&work, |c| c.source_report = Some(summary));
        }
        // No terminal receipt is visible while a source descriptor is still owned.
        drop(work.reader.take());
        let finish = self
            .update(&work, |c| {
                c.phase(Phase::Finalizing);
                c.reason = Some(reason.clone());
            })
            .and_then(|()| read_data(&work.dir, &work.collector))
            .map(|bytes| (bytes.len() as u64, digest(&bytes)));
        let mut state = self.state.lock().unwrap();
        let record = state.records.get_mut(&work.dir).unwrap();
        let c = record
            .collectors
            .iter_mut()
            .find(|c| c.id == work.collector)
            .unwrap();
        c.cost = Some(model::CollectionCost {
            run_wall_ms: began.elapsed().as_secs_f64() * 1000.0,
            source_read_calls: work.source_read_calls,
        });
        match finish {
            Ok((bytes, sha256)) => {
                c.bytes = bytes;
                c.sha256 = Some(sha256);
                c.phase(final_phase(&reason, bytes as usize));
                c.reason = Some(reason);
            }
            Err(_) => c.phase(Phase::CleanupPending),
        }
        if persist(&work.dir, record).is_err() {
            record
                .collectors
                .iter_mut()
                .find(|c| c.id == work.collector)
                .unwrap()
                .phase(Phase::CleanupPending);
            let _ = persist(&work.dir, record);
        }
        // Publication and releasing the manager's obligation are one state transaction.
        state.running.remove(&work.dir.join(&work.collector));
    }
    async fn collect(
        &self,
        work: &mut Work,
        root: &Path,
        workspace: &str,
        builds: &builds::Owners,
    ) -> Result<String> {
        if let Some(reason) = work.cancelled.borrow().clone() {
            return Ok(reason);
        }
        let limits = self.update(work, |collector| {
            collector.phase(Phase::Collecting);
            collector.limits.clone()
        })?;
        let began = Instant::now();
        let mut bytes = 0;
        let mut events = 0;
        let mut offset = 0;
        let native_trace = work.reader.as_ref().unwrap().native_trace();
        let mut file = (!native_trace)
            .then(|| collector_output(work))
            .transpose()?;
        loop {
            if let Some(reason) = work.cancelled.borrow().clone() {
                return Ok(reason);
            }
            if began.elapsed() >= Duration::from_millis(limits.duration_ms) {
                return Ok("duration_limit".into());
            }
            work.source_read_calls += 1;
            let native = work.reader.as_ref().unwrap().native();
            let remaining =
                Duration::from_millis(limits.duration_ms).saturating_sub(began.elapsed());
            let reading = work.reader.as_mut().unwrap().read(
                builds,
                root,
                workspace,
                offset,
                remaining,
                work.cancelled.clone(),
            );
            let result = if native {
                reading.await
            } else {
                tokio::select! {
                    biased;
                    _=work.cancelled.changed()=>return Ok(work.cancelled.borrow().clone().unwrap_or_else(||"stopped".into())),
                    _=tokio::time::sleep(Duration::from_millis(limits.duration_ms).saturating_sub(began.elapsed()))=>return Ok("duration_limit".into()),
                    result=reading=>result,
                }
            };
            let mut chunk = match result {
                Ok(chunk) => chunk,
                Err(e) if e.code == "CANCELLED" => {
                    return Ok(work
                        .cancelled
                        .borrow()
                        .clone()
                        .unwrap_or_else(|| "stopped".into()));
                }
                Err(e)
                    if e.code == "TIMEOUT"
                        && began.elapsed() >= Duration::from_millis(limits.duration_ms) =>
                {
                    return Ok("duration_limit".into());
                }
                Err(e) => return Err(e),
            };
            if native_trace {
                return self.retain_native_trace(work, &limits, chunk);
            }
            if !chunk.bytes.is_empty() {
                let clipped = chunk.bytes.len() as u64 > limits.max_bytes - bytes;
                chunk.bytes.truncate((limits.max_bytes - bytes) as usize);
                if clipped {
                    chunk.complete = false;
                }
                let file = file.as_mut().unwrap();
                file.write_all(&chunk.bytes)
                    .and_then(|()| artifacts::sync(file))
                    .map_err(|_| Error::new("EVIDENCE_IO", "Cannot retain collector output"))?;
                bytes += chunk.bytes.len() as u64;
                events += 1;
                offset = chunk.offset;
                self.update(work, |c| {
                    c.bytes = bytes;
                    c.events = events;
                    c.source_offset = offset;
                    if let Some(summary) = chunk.summary.clone() {
                        c.source_report = Some(summary);
                    }
                })?;
            }
            if chunk.complete {
                return Ok(chunk.reason.unwrap_or_else(|| "source_complete".into()));
            }
            if bytes >= limits.max_bytes || events >= limits.max_events {
                return Ok("capture_limit".into());
            }
            if chunk.bytes.is_empty() {
                tokio::select! { _=work.cancelled.changed()=>{}, _=tokio::time::sleep(Duration::from_millis(100).min(Duration::from_millis(limits.duration_ms).saturating_sub(began.elapsed())))=>{} }
            }
        }
    }

    fn retain_native_trace(
        &self,
        work: &Work,
        limits: &Limits,
        chunk: source::Chunk,
    ) -> Result<String> {
        if let Some(reason) = work.cancelled.borrow().clone() {
            return Ok(reason);
        }
        let bytes = chunk.bytes.len() as u64;
        if bytes > limits.max_bytes {
            return Err(Error::new(
                "NATIVE_TRACE_BOUNDS",
                "Native trace exceeds limits.maxBytes; increase it or select a smaller recording",
            )
            .details(json!({"requiredBytes":bytes,"maxBytes":limits.max_bytes})));
        }
        drop(collector_output(work)?);
        storage::write_bytes(
            &work.dir.join(format!("{}.bin", work.collector)),
            &chunk.bytes,
        )?;
        self.update(work, |c| {
            c.bytes = bytes;
            c.events = 1;
            c.source_offset = chunk.offset;
            c.source_report = chunk.summary;
        })?;
        Ok(chunk.reason.unwrap_or_else(|| "source_complete".into()))
    }
}
pub(crate) struct Work {
    dir: PathBuf,
    collector: String,
    reader: Option<source::Reader>,
    cancelled: watch::Receiver<Option<String>>,
    source_read_calls: u64,
}
impl Work {
    pub(crate) fn perfetto_start(&mut self) -> Option<tokio::sync::oneshot::Receiver<Instant>> {
        self.reader
            .as_mut()
            .and_then(source::Reader::perfetto_start)
    }
}
fn read_data(dir: &Path, collector: &str) -> Result<Vec<u8>> {
    validate_id(collector, "collector-")?;
    artifacts::check_private_directory(dir)?;
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options
        .open(dir.join(format!("{collector}.bin")))
        .map_err(|_| Error::new("EVIDENCE_IO", "Collector output unavailable"))?;
    if !file
        .metadata()
        .is_ok_and(|m| m.is_file() && m.len() <= 8 * 1024 * 1024)
    {
        return Err(Error::new("EVIDENCE_BOUNDS", "Invalid collector output"));
    }
    let mut bytes = Vec::new();
    file.take(8 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Error::new("EVIDENCE_IO", "Cannot read collector output"))?;
    Ok(bytes)
}

fn validate_source_binding(source: &Source, binding: &Binding) -> Result<()> {
    if let Source::Android {
        target_id, app_id, ..
    }
    | Source::AndroidState {
        target_id, app_id, ..
    }
    | Source::Ios {
        target_id, app_id, ..
    }
    | Source::ReactNative {
        target_id, app_id, ..
    } = source
        && (binding.target_id.as_ref() != Some(target_id)
            || binding.app_id.as_ref() != Some(app_id))
    {
        return Err(Error::new(
            "DIAGNOSTIC_BINDING_REQUIRED",
            "Open the investigation with the same targetId and appId before native/runtime collection",
        ));
    }
    if let Source::HostProcess { options } = source
        && (binding.target_id.as_ref() != Some(&options.target_id)
            || binding.app_id.as_ref() != Some(&options.service_id)
            || binding
                .process_id
                .is_some_and(|pid| pid != options.process_id)
            || binding
                .process_generation
                .as_ref()
                .is_some_and(|g| options.expected_generation.as_ref() != Some(g)))
    {
        return Err(Error::new(
            "DIAGNOSTIC_BINDING_REQUIRED",
            "Bind matching targetId/appId (serviceId), process ID and optional expectedGeneration before host reads",
        ));
    }
    if let Source::BoxLogs { options } = source
        && (binding.target_id.as_ref() != Some(&options.target_id)
            || binding.app_id.as_ref() != Some(&options.service_id))
    {
        return Err(Error::new(
            "DIAGNOSTIC_BINDING_REQUIRED",
            "Bind matching targetId and appId (serviceId) before reading Box logs",
        ));
    }
    if let Source::EmbeddedCdp { options } = source
        && (binding.target_id.as_ref() != Some(&options.target_id)
            || binding.app_id.as_ref() != Some(&options.app_id))
    {
        return Err(Error::new(
            "DIAGNOSTIC_BINDING_REQUIRED",
            "Open the investigation with matching targetId/appId before embedded observation",
        ));
    }
    if let Source::NativeTrace { options } = source
        && (binding.target_id.as_ref() != Some(&options.target_id)
            || binding.app_id.as_ref() != Some(&options.app_id))
    {
        return Err(Error::new(
            "DIAGNOSTIC_BINDING_REQUIRED",
            "Open the investigation with matching targetId/appId before native trace collection",
        ));
    }
    if let Source::Dart { options } = source
        && (binding.target_id.as_ref() != Some(&options.target_id)
            || binding.app_id.as_ref() != Some(&options.app_id))
    {
        return Err(Error::new(
            "DIAGNOSTIC_BINDING_REQUIRED",
            "Open the investigation with matching targetId and appId before Dart collection",
        ));
    }
    if let Source::BrowserOperation { target_id, .. } = source
        && binding.target_id.as_ref() != Some(target_id)
    {
        return Err(Error::new(
            "DIAGNOSTIC_BINDING_REQUIRED",
            "Open the investigation with the same explicit browser targetId",
        ));
    }
    Ok(())
}

fn remove_investigation(dir: &Path, record: &Investigation, running: &[PathBuf]) -> Result<()> {
    if !record.closed || record.collectors.iter().any(|c| !c.phase.settled()) {
        return Err(Error::new(
            "DIAGNOSTIC_BUSY",
            "Close the investigation and wait until diagnostics_status is settled before removal",
        ));
    }
    let mut expected = Vec::new();
    for collector in &record.collectors {
        expected.push(format!("{}.bin", collector.id));
        if matches!(collector.source, Source::NativeTrace { .. }) {
            expected.push(format!("{}.bin.partial", collector.id));
        }
    }
    expected.extend(analysis::owned_analysis_files(dir)?);
    if running.iter().any(|key| key.parent() == Some(dir)) {
        return Err(Error::new(
            "DIAGNOSTIC_BUSY",
            "Wait for diagnostic analysis cleanup before removal",
        ));
    }
    expected.push("session.json".into());
    for entry in std::fs::read_dir(dir)
        .map_err(|_| Error::new("STATE_IO", "Cannot inspect owned evidence"))?
    {
        let entry = entry.map_err(|_| Error::new("STATE_IO", "Cannot inspect owned evidence"))?;
        if !expected
            .iter()
            .any(|name| entry.file_name() == std::ffi::OsStr::new(name))
        {
            return Err(Error::new(
                "CLEANUP_PENDING",
                "Investigation contains an unowned file; preserve it before retrying removal",
            ));
        }
    }
    // Delete the record last; a failed deletion can be retried without recollection.
    for name in expected
        .iter()
        .filter(|name| name.as_str() != "session.json")
        .chain(std::iter::once(&expected[expected.len() - 1]))
    {
        match std::fs::remove_file(dir.join(name)) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => {
                return Err(Error::new(
                    "CLEANUP_PENDING",
                    "Owned evidence removal failed; retry diagnostics_remove",
                ));
            }
        }
    }
    std::fs::remove_dir(dir)
        .map_err(|_| Error::new("CLEANUP_PENDING", "Owned directory removal failed"))?;
    Ok(())
}

fn stop_collectors(
    record: &mut Investigation,
    select: &Select,
    close: bool,
    offline: bool,
    dir: &Path,
) -> Result<Vec<String>> {
    if select
        .collector_id
        .as_ref()
        .is_some_and(|id| !record.collectors.iter().any(|c| &c.id == id))
    {
        return Err(Error::new(
            "DIAGNOSTIC_NOT_FOUND",
            "Collector does not belong to this investigation",
        ));
    }
    if close {
        record.closed = true;
    }
    let ids: Vec<_> = record
        .collectors
        .iter()
        .filter(|c| select.collector_id.as_ref().is_none_or(|id| &c.id == id) && !c.phase.settled())
        .map(|c| c.id.clone())
        .collect();
    for c in &mut record.collectors {
        if ids.contains(&c.id) {
            c.phase(Phase::Stopping);
            c.reason = Some("stopped".into());
        }
    }
    if !offline {
        persist(dir, record)?;
    }
    Ok(ids)
}

fn collector_output(work: &Work) -> Result<std::fs::File> {
    let mut options = OpenOptions::new();
    options.append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options
        .open(work.dir.join(format!("{}.bin", work.collector)))
        .map_err(|_| Error::new("EVIDENCE_IO", "Cannot open owned collector output"))?;
    if !file.metadata().is_ok_and(|m| m.is_file() && m.len() == 0) {
        return Err(Error::new(
            "EVIDENCE_CHANGED",
            "Collector output changed before collection",
        ));
    }
    Ok(file)
}

fn recover_collector_cleanup(
    record: &mut Investigation,
    dir: &Path,
    running: &[PathBuf],
) -> Result<()> {
    let mut repaired = false;
    for c in &mut record.collectors {
        if c.phase == Phase::CleanupPending && !running.contains(&dir.join(&c.id)) {
            let bytes = read_data(dir, &c.id)?;
            c.bytes = bytes.len() as u64;
            c.sha256 = Some(digest(&bytes));
            c.phase(final_phase(
                c.reason.as_deref().unwrap_or("owner_lost"),
                bytes.len(),
            ));
            repaired = true;
        }
    }
    if repaired {
        persist(dir, record)?;
    }
    Ok(())
}

fn with_active_analyses(mut value: Value, state: &State, dir: &Path) -> Value {
    let active = state
        .running
        .keys()
        .filter(|key| {
            key.parent() == Some(dir)
                && key
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .starts_with("analysis-")
        })
        .count();
    value["activeAnalyses"] = json!(active);
    if active > 0 {
        value["settled"] = json!(false);
    }
    value
}
