//! Owner-retained asynchronous builds; polling and cancellation never replay work.
use super::*;
use crate::managed::storage;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use tokio::sync::watch;

#[derive(Default, Clone)]
pub(crate) struct Owners {
    entries: Arc<Mutex<HashMap<String, Entry>>>,
}
struct Entry {
    signature: String,
    stop: watch::Sender<bool>,
    record: Value,
    directory: PathBuf,
    active: bool,
}
pub(crate) struct Ticket {
    key: String,
    pub id: String,
    pub directory: PathBuf,
    pub cancelled: watch::Receiver<bool>,
}
impl Owners {
    pub(crate) fn active_project(&self, session: &str, project: &str) -> Value {
        let prefix = format!("{session}/");
        let records: Vec<_> = self
            .entries
            .lock()
            .unwrap()
            .iter()
            .filter(|(key, entry)| {
                key.starts_with(&prefix)
                    && entry.active
                    && entry
                        .record
                        .get("applicationRoot")
                        .unwrap_or(&entry.record["project"])
                        == project
            })
            .map(|(_, entry)| entry.record.clone())
            .collect();
        json!(records.into_iter().map(inspection).collect::<Vec<_>>())
    }
    pub(crate) fn recent_project(&self, session: &str, project: &str) -> Value {
        let mut entries: Vec<_> = self
            .entries
            .lock()
            .unwrap()
            .iter()
            .filter(|(key, entry)| {
                key.starts_with(&format!("{session}/"))
                    && entry
                        .record
                        .get("applicationRoot")
                        .unwrap_or(&entry.record["project"])
                        == project
            })
            .map(|(_, entry)| entry.record.clone())
            .collect();
        entries.sort_by_key(|entry| {
            std::cmp::Reverse(entry["createdAtMs"].as_u64().unwrap_or_default())
        });
        entries.truncate(20);
        json!(entries.into_iter().map(inspection).collect::<Vec<_>>())
    }
    pub(crate) fn log(&self, root: &Path, session: &str, args: &Value) -> Result<Value> {
        let id = args["operationId"]
            .as_str()
            .ok_or_else(|| Error::invalid("operationId is required"))?;
        let record = self.status(root, session, id, false)?;
        let path = Self::directory(root, session, id)?.join("build.log");
        let mut page = crate::pagination::stream(
            "build_log",
            &(root, session, id),
            args,
            &path,
            None,
            record["settled"] != true,
            false,
        )?;
        page["operationId"] = json!(id);
        page["settled"] = record["settled"].clone();
        page["private"] = json!(true);
        Ok(page)
    }
    pub(crate) fn active(&self) -> bool {
        self.entries
            .lock()
            .unwrap()
            .values()
            .any(|entry| entry.active)
    }
    pub(crate) fn identify(&self, ticket: &Ticket, prepared: &Prepared) -> Result<Value> {
        let mut entries = self.entries.lock().unwrap();
        let entry = entries
            .get_mut(&ticket.key)
            .ok_or_else(|| Error::new("BUILD_IO", "Missing retained build"))?;
        entry.record["applicationRoot"] = json!(prepared.root);
        if prepared.source.get("configurationRoot").is_some() {
            entry.record["configurationRoot"] = prepared.source["configurationRoot"].clone();
            entry.record["projectId"] = prepared.source["project"].clone();
        }
        storage::write(&entry.directory.join("operation.json"), &entry.record)?;
        Ok(public(entry.record.clone()))
    }
    fn directory(root: &Path, session: &str, id: &str) -> Result<PathBuf> {
        if !id.starts_with("build-")
            || id.len() != 38
            || !id[6..].bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(Error::invalid("Invalid build operationId"));
        }
        Ok(root
            .join("builds")
            .join(format!("{:x}", Sha256::digest(session.as_bytes())))
            .join(id))
    }
    pub(crate) fn begin(
        &self,
        root: &Path,
        session: &str,
        request_id: &str,
        request: &Request,
    ) -> Result<(Value, Option<Ticket>)> {
        let id = request.operation_id(request_id)?;
        let directory = Self::directory(root, session, &id)?;
        let key = format!("{session}/{id}");
        let signature = request.signature();
        let mut entries = self.entries.lock().unwrap();
        if let Some(entry) = entries.get(&key) {
            if entry.signature != signature {
                return Err(Error::new(
                    "REQUEST_CONFLICT",
                    "Build request identity was reused with different inputs",
                ));
            }
            return Ok((public(entry.record.clone()), None));
        }
        if directory.exists()
            && let Some(record) = storage::read::<Value>(&directory.join("operation.json"))?
        {
            if record["signature"] != signature {
                return Err(Error::new(
                    "REQUEST_CONFLICT",
                    "Build request identity was reused with different inputs",
                ));
            }
            return Ok((offline(record), None));
        }
        if entries.values().filter(|e| e.active).count() >= 4 {
            return Err(Error::new("OVERLOADED", "Four builds are already active"));
        }
        if entries.len() >= 256 {
            entries.retain(|_, entry| entry.active);
        }
        if !root.exists() {
            storage::directory(root)?;
        }
        if !root.join("builds").exists() {
            storage::directory(&root.join("builds"))?;
        }
        storage::directory(directory.parent().unwrap())?;
        storage::directory(&directory)?;
        let (stop, cancelled) = watch::channel(false);
        let mut record = json!({"operationId":id,"submissionId":request.submission_id,"project":request.project.canonicalize().unwrap_or_else(|_|request.project.clone()),"profile":request.profile,"task":request.task.as_deref().unwrap_or("build"),"check":request.check,"createdAtMs":std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis(),"settled":false,"state":"preparation","signature":signature,"stages":{
            "preparation":{"state":"running"},"build":{"state":"not_started"},"artifact":{"state":"not_started"},
            "connect":{"state":if request.install {"not_started"} else {"not_requested"}},
            "install":{"state":if request.install {"not_started"} else {"not_requested"}},
            "launch":{"state":if request.launch {"not_started"} else {"not_requested"}}
        },"logPath":directory.join("build.log")});
        if request.root.is_some() {
            record["project"] = json!(request.project);
        }
        if request.task.as_deref().is_some_and(|task| task != "build") {
            record["stages"]["build"]["state"] = json!("not_requested");
            record["stages"]["artifact"]["state"] = json!("not_requested");
        }
        storage::write(&directory.join("operation.json"), &record)?;
        entries.insert(
            key.clone(),
            Entry {
                signature,
                stop,
                record: record.clone(),
                directory: directory.clone(),
                active: true,
            },
        );
        Ok((
            public(record),
            Some(Ticket {
                key,
                id,
                directory,
                cancelled,
            }),
        ))
    }
    /// Recover a previous delivery without reading project files or resolving tools.
    pub(crate) fn replay(
        &self,
        root: &Path,
        session: &str,
        transport: &str,
        request: &Request,
    ) -> Result<Option<Value>> {
        let id = request.operation_id(transport)?;
        let signature = request.signature();
        if let Some(entry) = self.entries.lock().unwrap().get(&format!("{session}/{id}")) {
            if entry.signature != signature {
                return Err(Error::new(
                    "REQUEST_CONFLICT",
                    "Build submission identity already has different inputs",
                ));
            }
            return Ok(Some(public(entry.record.clone())));
        }
        let directory = Self::directory(root, session, &id)?;
        if !directory.exists() {
            return Ok(None);
        }
        let record = storage::read::<Value>(&directory.join("operation.json"))?;
        record
            .map(|record| {
                if record["signature"] == signature {
                    Ok(offline(record))
                } else {
                    Err(Error::new(
                        "REQUEST_CONFLICT",
                        "Build submission identity already has different inputs",
                    ))
                }
            })
            .transpose()
    }
    pub(crate) fn query(
        &self,
        root: &Path,
        session: &str,
        args: &Value,
        cancel: bool,
    ) -> Result<Value> {
        let operation = args["operationId"].as_str();
        let submission = args["submissionId"].as_str();
        if operation.is_some() && submission.is_some() {
            return Err(Error::invalid("Select operationId or submissionId"));
        }
        let id = if let Some(id) = operation {
            Some(id.to_owned())
        } else if let Some(key) = submission {
            if key.is_empty() || key.len() > 128 || key.chars().any(char::is_control) {
                return Err(Error::invalid("Invalid submissionId"));
            }
            Some(format!(
                "build-{}",
                &format!("{:x}", Sha256::digest(key.as_bytes()))[..32]
            ))
        } else {
            None
        };
        if let Some(id) = id {
            let page = canonic_mcp_kit::page::Request::from_mcp(
                args,
                canonic_mcp_kit::page::Limits::STANDARD,
            )?;
            if page.cursor.is_some() {
                return Err(canonic_mcp_kit::page::Error::InvalidCursor.into());
            }
            return self.status(root, session, &id, cancel);
        }
        if cancel {
            return Err(Error::invalid(
                "Cancellation requires operationId or submissionId",
            ));
        }
        let directory = root
            .join("builds")
            .join(format!("{:x}", Sha256::digest(session.as_bytes())));
        let scope = crate::pagination::scope("build_status", &(root, session))?;
        let (_, after) = crate::pagination::request(args, &scope)?;
        let mut last = after
            .map(String::from_utf8)
            .transpose()
            .map_err(|_| canonic_mcp_kit::page::Error::InvalidCursor)?
            .unwrap_or_default();
        let mut batch = std::collections::VecDeque::new();
        let mut failure = None;
        let rows = std::iter::from_fn(|| {
            while batch.is_empty() {
                match crate::pagination::directory_keys(&directory, &last) {
                    Ok(keys) => {
                        if keys.is_empty() {
                            return None;
                        }
                        last.clone_from(keys.last().expect("nonempty window"));
                        batch.extend(keys.into_iter().filter(|id| {
                            id.starts_with("build-")
                                && id.len() == 38
                                && id[6..].bytes().all(|b| b.is_ascii_hexdigit())
                        }));
                    }
                    Err(error) => {
                        failure = Some(error);
                        return None;
                    }
                }
            }
            let id = batch.pop_front()?;
            last.clone_from(&id);
            Some((id.as_bytes().to_vec(), id))
        });
        let page = crate::pagination::collect(
            "build_status",
            &(root, session),
            args,
            rows,
            |id| {
                let record = self.status(root, session, &id, false)?;
                Ok(
                    json!({"operationId":record["operationId"],"submissionId":record["submissionId"],"createdAtMs":record["createdAtMs"],"state":record["state"],"settled":record["settled"]}),
                )
            },
            "build_status with operationId",
        )?;
        if let Some(error) = failure {
            return Err(error);
        }
        Ok(json!({"operations":page.items,"nextCursor":page.next_cursor}))
    }
    pub(crate) fn status(
        &self,
        root: &Path,
        session: &str,
        id: &str,
        cancel: bool,
    ) -> Result<Value> {
        let key = format!("{session}/{id}");
        if let Some(entry) = self.entries.lock().unwrap().get(&key) {
            if cancel && entry.active {
                entry.stop.send_replace(true);
            }
            let mut record = inspection(entry.record.clone());
            record["cancelRequested"] = json!(*entry.stop.borrow());
            return Ok(record);
        }
        let path = Self::directory(root, session, id)?.join("operation.json");
        if !path.parent().unwrap().exists() {
            return Err(Error::new(
                "BUILD_NOT_FOUND",
                "Build operation does not exist in this session",
            ));
        }
        let record = storage::read::<Value>(&path)?.ok_or_else(|| {
            Error::new(
                "BUILD_NOT_FOUND",
                "Build operation does not exist in this session",
            )
        })?;
        Ok(offline(record))
    }
    pub(crate) fn update(&self, ticket: &Ticket, stage: &str, result: Value) -> Result<()> {
        let mut entries = self.entries.lock().unwrap();
        let entry = entries
            .get_mut(&ticket.key)
            .ok_or_else(|| Error::new("BUILD_NOT_FOUND", "Build operation disappeared"))?;
        entry.record["state"] = json!(stage);
        entry.record["stages"][stage] = result;
        storage::write(&entry.directory.join("operation.json"), &entry.record)
    }
    pub(crate) fn finish(&self, ticket: &Ticket, result: Result<Value>) {
        let mut entries = self.entries.lock().unwrap();
        let Some(entry) = entries.get_mut(&ticket.key) else {
            return;
        };
        match result {
            Ok(value) => {
                entry.record["state"] = json!("completed");
                entry.record["result"] = value;
            }
            Err(mut error) => {
                if let Some(details) = error.details.as_mut().and_then(Value::as_object_mut) {
                    details.remove("privateOutput");
                }
                if let Some(stages) = entry.record["stages"].as_object_mut() {
                    for stage in stages.values_mut() {
                        if stage["state"] == "running" {
                            stage["state"] = json!(if error.code == "CANCELLED" {
                                "cancelled"
                            } else {
                                "failed"
                            });
                            stage["error"] = json!({"code":error.code,"message":error.message});
                        }
                    }
                }
                entry.record["state"] = json!(if error.code == "CANCELLED" {
                    "cancelled"
                } else {
                    "failed"
                });
                entry.record["error"] = json!(error);
            }
        }
        entry.record["settled"] = json!(true);
        entry.record["finishedAtMs"] = json!(crate::project::now());
        if storage::write(&entry.directory.join("operation.json"), &entry.record).is_err() {
            entry.record["error"] = json!(Error::new(
                "STATE_IO",
                "Build finished but its terminal record could not be saved"
            ));
            entry.record["state"] = json!("failed");
        }
        entry.active = false;
    }
    #[expect(
        clippy::too_many_lines,
        reason = "A build holds its project lease through command execution, private diagnostics and immutable artifact publication"
    )]
    pub(crate) async fn build(&self, ticket: &Ticket, prepared: &Prepared) -> Result<Value> {
        let _lock = crate::lease::ControlLease::acquire(&format!(
            "{}:{}",
            if prepared.task == "server" {
                "project-server"
            } else {
                "build-project"
            },
            prepared.root.display()
        ))
        .map_err(|e| {
            if e.code == "TARGET_BUSY" {
                let namespace = ticket.key.rsplit_once('/').map(|v| v.0).unwrap_or_default();
                let prefix = format!("{namespace}/");
                let existing = self
                    .entries
                    .lock()
                    .unwrap()
                    .iter()
                    .find(|(key, entry)| {
                        key.starts_with(&prefix)
                            && entry.active
                            && entry.record["operationId"] != ticket.id
                            && entry
                                .record
                                .get("applicationRoot")
                                .unwrap_or(&entry.record["project"])
                                == prepared.root.to_string_lossy().as_ref()
                            && (entry.record["task"] == "server") == (prepared.task == "server")
                    })
                    .map(|(_, entry)| entry.record["operationId"].clone());
                Error::new(
                    "BUILD_BUSY",
                    "Another build holds this project output; wait for it to settle",
                )
                .details(json!({"operationId":existing,"nextAction":"build_status"}))
            } else {
                e
            }
        })?;
        if *ticket.cancelled.borrow() {
            return Err(Error::new("CANCELLED", "Build cancelled before dispatch"));
        }
        let deadline = tokio::time::Instant::now()
            + std::time::Duration::from_millis(prepared.profile.timeout_ms);
        self.update(ticket,"preparation",json!({"state":"completed","source":prepared.source,"task":prepared.task,"timeoutMs":prepared.profile.timeout_ms}))?;
        if prepared.task != "build" {
            let development = prepared.profile.development.as_ref().unwrap();
            if prepared.task == "server" {
                crate::project::admit_server(development, &prepared.profile.platform).await?;
            }
            self.project_task(
                ticket,
                prepared,
                &prepared.task,
                development.task(&prepared.task, prepared.check.as_deref())?,
            )
            .await?;
            return Ok(json!({"task":prepared.task,"check":prepared.check,"completed":true}));
        }
        if prepared.prepare {
            let development = prepared.profile.development.as_ref().unwrap();
            self.project_task(
                ticket,
                prepared,
                "prepare",
                development.task("prepare", None)?,
            )
            .await?;
            if let Some(task) = &development.preflight {
                self.project_task(ticket, prepared, "preflight", task)
                    .await?;
            }
        }
        if !prepared.cwd.is_dir() {
            return Err(Error::new(
                "CONFIG_PATH",
                "The build working directory is unavailable after preparation; inspect the declared task and its file effects",
            ));
        }
        let output = ticket.directory.join("output");
        let cancel = crate::process::Cancellation::new(ticket.cancelled.clone());
        storage::directory(&output)?;
        let plan = adapters::plan(prepared, &output)?;
        let previous = if prepared.profile.builder == "custom" {
            artifacts::select_checked(&plan.cwd, &plan.selector, &prepared.root, &output, &cancel)
                .ok()
                .map(|path| artifacts::stamp_checked(&path, &cancel))
                .transpose()?
        } else {
            None
        };
        let mut effective = serde_json::to_value(&prepared.profile).unwrap();
        crate::configuration::redact(&mut effective);
        self.update(ticket,"preparation",json!({"state":"completed","source":prepared.source,"effective":effective,"builder":prepared.profile.builder,"platform":prepared.profile.platform,"targetKind":prepared.profile.target_kind,"envKeys":prepared.env.keys().collect::<Vec<_>>()}))?;
        self.update(ticket, "build", json!({"state":"running","startedAtMs":crate::project::now(),"timeoutMs":prepared.profile.timeout_ms}))?;
        for command in &plan.commands {
            match crate::process::build_logged(
                command,
                &plan.cwd,
                &prepared.env,
                deadline,
                ticket.cancelled.clone(),
                &ticket.directory.join("build.log"),
            )
            .await
            {
                Ok(_) => {}
                Err(mut error) => {
                    error = crate::project::diagnostic(error);
                    if let Some(details) = error.details.as_mut().and_then(|d| d.as_object_mut()) {
                        details.remove("privateOutput");
                    }
                    return Err(error);
                }
            }
        }
        self.update(ticket, "build", json!({"state":"completed"}))?;
        if *ticket.cancelled.borrow() {
            return Err(Error::new(
                "CANCELLED",
                "Build cancelled before artifact handoff",
            ));
        }
        self.update(ticket, "artifact", json!({"state":"running"}))?;
        let mut path =
            artifacts::select_checked(&plan.cwd, &plan.selector, &prepared.root, &output, &cancel)?;
        if let Some(before) = &previous
            && &artifacts::stamp_checked(&path, &cancel)? == before
        {
            return Err(Error::new(
                "BUILD_ARTIFACT_STALE",
                "Custom command left the selected artifact unchanged; produce or explicitly refresh its output",
            ));
        }
        if plan.archive {
            path = artifacts::unpack_checked(&path, &output, &cancel)?;
        }
        let mut artifact = crate::applications::retain_build_checked(
            &path,
            &ticket.directory.join("artifact"),
            &prepared.profile.platform,
            prepared.profile.target_kind == "simulator",
            &prepared.env,
            cancel,
        )
        .await?;
        if let Some(development) = &prepared.profile.development
            && artifact["identity"]["appId"] != development.app_id
        {
            return Err(Error::new(
                "BUILD_APP_MISMATCH",
                "Built app identity differs from development.app_id; no device installation was dispatched",
            ));
        }
        artifact["reference"] = json!(ticket.id);
        artifact["platform"] = json!(prepared.profile.platform);
        artifact["targetKind"] = json!(prepared.profile.target_kind);
        if let Some(diagnostics) = &prepared.profile.diagnostics {
            artifact["diagnostics"] = json!({"declared":diagnostics,"verification":"profile_claims_not_binary_verified","automaticInstrumentation":false});
        }
        artifact["retention"] = json!(
            "Retained in private build storage until explicit operator cleanup; missing files require a new build or explicit path submission"
        );
        self.update(
            ticket,
            "artifact",
            json!({"state":"completed","artifact":artifact}),
        )?;
        Ok(artifact)
    }
    async fn project_task(
        &self,
        ticket: &Ticket,
        prepared: &Prepared,
        stage: &str,
        task: &crate::project::Task,
    ) -> Result<()> {
        let cwd = prepared
            .root
            .join(&task.cwd)
            .canonicalize()
            .map_err(|_| Error::new("CONFIG_PATH", "Development task cwd is unavailable"))?;
        if !cwd.is_dir() {
            return Err(Error::new(
                "CONFIG_PATH",
                "Development task cwd is not a directory",
            ));
        }
        self.update(ticket,stage,json!({"state":"running","cwd":cwd,"effects":task.effects,"startedAtMs":crate::project::now(),"timeoutMs":task.timeout_ms,"owner":"testrun_operation"}))?;
        let before = crate::project::files::capture(&prepared.root, &task.effects);
        let result = crate::process::build_logged(
            &task.command,
            &cwd,
            &prepared.env,
            tokio::time::Instant::now() + std::time::Duration::from_millis(task.timeout_ms),
            ticket.cancelled.clone(),
            &ticket.directory.join("build.log"),
        )
        .await;
        let changes = crate::project::files::changes(
            &before,
            &crate::project::files::capture(&prepared.root, &task.effects),
        );
        self.update(
            ticket,
            stage,
            json!({"state":"running","cwd":cwd,"effects":task.effects,"changes":changes}),
        )?;
        match result {
            Ok(output) => {
                let actual = String::from_utf8_lossy(&output.bytes);
                if let Some(expected) = &task.expected_output
                    && actual.trim() != expected
                {
                    return Err(Error::new(
                        "PROJECT_CHECK_MISMATCH",
                        "Task output does not match its declared expectation; inspect build_log",
                    )
                    .details(json!({"stage":stage})));
                }
                self.update(ticket,stage,json!({"state":"completed","cwd":cwd,"effects":task.effects,"changes":changes,"expectedOutputMatched":task.expected_output.is_some()}))?;
                Ok(())
            }
            Err(mut error) => {
                error = crate::project::diagnostic(error);
                let mut details = error.details.take().unwrap_or(json!({}));
                details.as_object_mut().unwrap().remove("privateOutput");
                details["stage"] = json!(stage);
                details["cwd"] = json!(cwd);
                details["nextAction"] = json!("build_log");
                Err(error.details(details))
            }
        }
    }
    pub(crate) fn artifact(&self, root: &Path, session: &str, id: &str) -> Result<Value> {
        let record = self.status(root, session, id, false)?;
        let artifact = record["stages"]["artifact"]["artifact"].clone();
        if artifact["path"]
            .as_str()
            .is_none_or(|path| !Path::new(path).exists())
        {
            return Err(Error::new(
                "BUILD_ARTIFACT_MISSING",
                "Artifact reference is unavailable; submit an explicit path or build again",
            ));
        }
        Ok(artifact)
    }
}
fn public(mut record: Value) -> Value {
    record.as_object_mut().unwrap().remove("signature");
    record
}
fn inspection(record: Value) -> Value {
    let mut record = public(record);
    record["elapsedMs"] = json!(
        record["finishedAtMs"]
            .as_u64()
            .map(u128::from)
            .unwrap_or_else(crate::project::now)
            .saturating_sub(record["createdAtMs"].as_u64().map(u128::from).unwrap_or(0))
    );
    if let Some(path) = record["logPath"].as_str().map(PathBuf::from) {
        if let Ok(Some(process)) = storage::read::<Value>(&path.with_extension("process.json")) {
            record["process"] = process;
        }
        if let Ok(metadata) = std::fs::metadata(path) {
            record["logBytes"] = json!(metadata.len());
            record["lastOutputAtMs"] = json!(
                metadata
                    .modified()
                    .ok()
                    .and_then(|v| v.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|v| v.as_millis())
            );
        }
    }
    record
}
fn offline(mut record: Value) -> Value {
    if record["settled"] != true {
        record["state"] = json!("outcome_unknown");
        record["settled"] = json!(true);
        record["error"] = json!(Error::new(
            "OUTCOME_UNKNOWN",
            "Build owner stopped before a terminal record; inspect target state before another mutation"
        ));
    }
    inspection(record)
}
