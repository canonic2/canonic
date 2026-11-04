//! Example consumer-owned durable records. I/O stays outside MCP Kit's protocol core.
//!
//! The private directory is one access scope; IDs are bearer credentials inside
//! that explicitly selected local scope. A kernel lock admits one store owner.
//! Mutation reservations reject concurrent writers rather than holding map locks
//! during I/O. Recovery never replays a possibly executed report operation.

use canonic_mcp_kit::{
    Content, Decision, RequestContext, RpcError, ToolResult, json,
    task::{
        self, InputHistory, Metadata, Question, Snapshot, State, TaskId, TaskOptions, TaskRequest,
        Timestamp,
    },
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::Mutex,
};

const RECORD_BYTES: usize = 2 * task::MAX_SNAPSHOT_BYTES;

#[derive(Debug, Clone, PartialEq, Serialize)]
struct Record {
    snapshot: Snapshot,
    history: InputHistory,
    scope: String,
    operation: String,
    decision: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Stored {
    snapshot: Value,
    history: Vec<String>,
    scope: String,
    operation: String,
    decision: Option<String>,
}

#[derive(Default)]
struct Records {
    entries: BTreeMap<String, Record>,
    writing: BTreeSet<String>,
    unavailable: bool,
}

pub(crate) struct Store {
    root: PathBuf,
    _lease: File,
    policy: TaskOptions,
    records: Mutex<Records>,
}

fn storage(error: impl std::fmt::Display) -> RpcError {
    eprintln!("Acme task storage: {error}");
    RpcError::new(-32603, "Acme task storage is unavailable.")
}

/// The example always reserves a full bounded result slot and failure bookkeeping.
pub(crate) fn policy() -> TaskOptions {
    TaskOptions {
        max_records: 8,
        max_bytes: 8 * RECORD_BYTES,
        max_running: 8,
        max_input_rounds: 1,
        execution_timeout: std::time::Duration::from_mins(1),
        input_timeout: std::time::Duration::from_mins(10),
    }
}

impl Store {
    /// Opens a private, explicitly selected example directory and reconciles
    /// interrupted execution before serving any request.
    pub(crate) fn open(root: &Path, now: &Timestamp) -> io::Result<Self> {
        fs::create_dir_all(root)?;
        if !fs::symlink_metadata(root)?.is_dir() {
            return Err(io::Error::other("Acme store must be a plain directory"));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            // The caller selects a dedicated store directory. Tighten existing
            // directories too: records contain bearer IDs and must remain private.
            fs::set_permissions(root, fs::Permissions::from_mode(0o700))?;
        }
        let lease = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(root.join("owner.lock"))?;
        lease.try_lock().map_err(io::Error::other)?;
        let policy = policy();
        let mut records = Records::default();
        for entry in fs::read_dir(root)? {
            let entry = entry?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                return Err(io::Error::other("Invalid Acme store entry"));
            };
            if name == "owner.lock" {
                continue;
            }
            if !entry.file_type()?.is_file() {
                return Err(io::Error::other("Invalid Acme record type"));
            }
            if let Some(id) = name.strip_suffix(".pending")
                && hex_id(id)
            {
                fs::remove_file(entry.path())?;
                continue;
            }
            let Some(id) = name.strip_suffix(".json").filter(|id| hex_id(id)) else {
                return Err(io::Error::other("Invalid Acme record name"));
            };
            let record = read_record(&entry.path(), &policy)?;
            if record.snapshot.metadata().id().as_str() != id {
                return Err(io::Error::other("Acme record identity mismatch"));
            }
            // Clock regression is an error, not grounds to delete a live record.
            match record.snapshot.metadata().ensure_live(now) {
                Ok(()) => {}
                Err(task::TaskError::Expired) => {
                    fs::remove_file(entry.path())?;
                    continue;
                }
                Err(_) => return Err(io::Error::other("Acme store clock regressed")),
            }
            if records.entries.insert(id.into(), record).is_some() {
                return Err(io::Error::other("Duplicate Acme record"));
            }
            policy
                .admit(
                    records.entries.len(),
                    records.entries.len() * RECORD_BYTES,
                    records.entries.len(),
                )
                .map_err(io::Error::other)?;
        }
        sync_directory(root)?;
        let store = Self {
            root: root.into(),
            _lease: lease,
            policy,
            records: Mutex::new(records),
        };
        // Records awaiting user input are recoverable. A crash after input was
        // consumed may have interrupted effects, so working records fail once.
        let interrupted: Vec<_> = store
            .records
            .lock()
            .map_err(|_| io::Error::other("Acme lock poisoned"))?
            .entries
            .values()
            .filter(|record| record.snapshot.status() == "working")
            .cloned()
            .collect();
        for record in interrupted {
            let mut failed = record.clone();
            failed.snapshot = Snapshot::new(record.snapshot.metadata().clone().updated(now.clone(),record.snapshot.metadata().ttl_ms()).map_err(io::Error::other)?,State::Failed(RpcError::new(-32603,"Execution was interrupted; effects may have occurred. Review before starting again."))).map_err(io::Error::other)?;
            store
                .commit(Some(&record), failed, now)
                .map_err(|error| io::Error::other(error.message))?;
        }
        Ok(store)
    }

    pub(crate) fn get(
        &self,
        id: &TaskId,
        scope: &str,
        now: &Timestamp,
    ) -> Result<Snapshot, RpcError> {
        Ok(self.record(id, scope, now)?.snapshot)
    }

    fn record(&self, id: &TaskId, scope: &str, now: &Timestamp) -> Result<Record, RpcError> {
        let records = self.records.lock().map_err(storage)?;
        if records.unavailable {
            return Err(storage("Previous write was uncertain; reopen the store."));
        }
        let record = records
            .entries
            .get(id.as_str())
            .filter(|record| record.scope == scope)
            .ok_or_else(task::not_found)?;
        record
            .snapshot
            .metadata()
            .ensure_live(now)
            .map_err(|_| task::not_found())?;
        Ok(record.clone())
    }

    pub(crate) fn create(
        &self,
        context: &RequestContext,
        scope: &str,
        now: &Timestamp,
    ) -> Result<Snapshot, RpcError> {
        if !context.task_eligible() {
            context.require_tasks()?;
            return Err(RpcError::new(
                -32603,
                "Only tool calls create report tasks.",
            ));
        }
        let question = Question::new(context, "report-approval", "Generate the Acme report?")?;
        self.prune(now)?;
        let mut random = [0_u8; 16];
        getrandom::fill(&mut random).map_err(storage)?;
        let id = TaskId::new(
            random
                .iter()
                .flat_map(|byte| {
                    const HEX: &[u8; 16] = b"0123456789abcdef";
                    [
                        char::from(HEX[usize::from(byte >> 4)]),
                        char::from(HEX[usize::from(byte & 15)]),
                    ]
                })
                .collect::<String>(),
        )
        .map_err(|error| error.invalid_params())?;
        let metadata = Metadata::new(id, now.clone(), 600_000)
            .and_then(|m| m.with_poll_interval(1000))
            .map_err(|error| error.invalid_params())?;
        let snapshot =
            Snapshot::new(metadata, State::InputRequired(question.clone())).map_err(storage)?;
        let mut history = InputHistory::default();
        history
            .issue(question.key(), &self.policy)
            .map_err(storage)?;
        let record = Record {
            snapshot: snapshot.clone(),
            history,
            scope: scope.into(),
            operation: "acme_report".into(),
            decision: None,
        };
        self.commit(None, record, now)?;
        Ok(snapshot)
    }

    /// Commits input consumption before any report work runs. The returned
    /// decision is informational; durable state determines whether execution may run.
    pub(crate) fn accept(
        &self,
        request: &TaskRequest,
        context: &RequestContext,
        scope: &str,
        now: &Timestamp,
    ) -> Result<Option<Decision>, RpcError> {
        let record = self.record(request.id(), scope, now)?;
        let Some(decision) = record.snapshot.input_decision(request, context, now)? else {
            return Ok(None);
        };
        let mut next = record.clone();
        next.snapshot = Snapshot::new(
            record
                .snapshot
                .metadata()
                .clone()
                .updated(now.clone(), record.snapshot.metadata().ttl_ms())
                .map_err(storage)?,
            State::Working,
        )
        .map_err(storage)?;
        next.decision = Some(
            if decision == Decision::Accepted {
                "accept"
            } else {
                "decline"
            }
            .into(),
        );
        self.commit(Some(&record), next, now)?;
        Ok(Some(decision))
    }

    /// Tiny consumer executor. No driver context or original RPC is retained.
    pub(crate) fn finish_report(
        &self,
        id: &TaskId,
        scope: &str,
        now: &Timestamp,
    ) -> Result<(), RpcError> {
        let record = self.record(id, scope, now)?;
        if record.snapshot.status() != "working" {
            return Ok(());
        }
        let result = match record.decision.as_deref() {
            Some("accept") => ToolResult::success(vec![Content::text("Acme report generated.")]),
            Some("decline") => {
                ToolResult::error(vec![Content::text("Report generation was declined.")])
            }
            _ => return Err(storage("Missing persisted report decision")),
        };
        let mut next = record.clone();
        next.snapshot = Snapshot::new(
            record
                .snapshot
                .metadata()
                .clone()
                .updated(now.clone(), record.snapshot.metadata().ttl_ms())
                .map_err(storage)?,
            if now
                .elapsed_millis_since(record.snapshot.metadata().updated_at())
                .map_err(storage)?
                >= self.policy.execution_timeout.as_millis()
            {
                State::Failed(RpcError::new(
                    -32001,
                    "Report execution deadline exceeded; review its state before retrying.",
                ))
            } else {
                State::Completed(result)
            },
        )
        .map_err(storage)?;
        self.commit(Some(&record), next, now)
    }

    pub(crate) fn cancel(&self, id: &TaskId, scope: &str, now: &Timestamp) -> Result<(), RpcError> {
        let record = self.record(id, scope, now)?;
        if record.snapshot.terminal() {
            return Ok(());
        }
        let mut next = record.clone();
        next.snapshot = Snapshot::new(
            record
                .snapshot
                .metadata()
                .clone()
                .updated(now.clone(), record.snapshot.metadata().ttl_ms())
                .map_err(storage)?,
            State::Cancelled,
        )
        .map_err(storage)?;
        self.commit(Some(&record), next, now)
    }

    fn commit(&self, old: Option<&Record>, next: Record, now: &Timestamp) -> Result<(), RpcError> {
        let id = next.snapshot.metadata().id().as_str().to_owned();
        if let Some(old) = old {
            old.snapshot
                .validate_successor(&next.snapshot, now)
                .map_err(storage)?;
        }
        let bytes = json::encode(&next, RECORD_BYTES).map_err(storage)?;
        {
            let mut records = self.records.lock().map_err(storage)?;
            if records.unavailable {
                return Err(storage("Store requires reconciliation"));
            }
            if records.writing.contains(&id) {
                return Err(RpcError::new(
                    -32000,
                    "Task is being updated; retry after reading its state.",
                ));
            }
            if records.entries.get(&id) != old {
                return Err(RpcError::new(
                    -32000,
                    "Task changed; read its current state.",
                ));
            }
            let total = records.entries.len()
                + records
                    .writing
                    .iter()
                    .filter(|id| !records.entries.contains_key(*id))
                    .count()
                + usize::from(old.is_none());
            self.policy
                .admit(total, total * RECORD_BYTES, total)
                .map_err(|_| RpcError::new(-32000, "Acme task capacity is full."))?;
            records.writing.insert(id.clone());
        }
        let written = write_record(&self.root, &id, &bytes);
        let mut records = self.records.lock().map_err(storage)?;
        records.writing.remove(&id);
        if let Err(error) = written {
            records.unavailable = true;
            return Err(storage(error));
        }
        records.entries.insert(id, next);
        Ok(())
    }

    fn prune(&self, now: &Timestamp) -> Result<(), RpcError> {
        // Bounded by the fixed retained-record quota, never by profile age.
        let expired: Vec<_> = {
            let records = self.records.lock().map_err(storage)?;
            records
                .entries
                .iter()
                .filter(|(id, record)| {
                    !records.writing.contains(*id)
                        && matches!(
                            record.snapshot.metadata().ensure_live(now),
                            Err(task::TaskError::Expired)
                        )
                })
                .map(|(id, _)| id.clone())
                .collect()
        };
        for id in expired {
            // This example's one mutation lane serializes prune with commits.
            fs::remove_file(self.root.join(format!("{id}.json"))).map_err(storage)?;
            sync_directory(&self.root).map_err(storage)?;
            self.records.lock().map_err(storage)?.entries.remove(&id);
        }
        Ok(())
    }
}

fn hex_id(id: &str) -> bool {
    id.len() == 32 && id.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn read_record(path: &Path, policy: &TaskOptions) -> io::Result<Record> {
    let mut bytes = Vec::new();
    File::open(path)?
        .take(RECORD_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    let value = json::parse(&bytes, RECORD_BYTES).map_err(io::Error::other)?;
    let stored: Stored = serde_json::from_value(value).map_err(io::Error::other)?;
    if stored.operation != "acme_report"
        || stored.scope.len() > 128
        || !matches!(
            stored.decision.as_deref(),
            None | Some("accept" | "decline")
        )
    {
        return Err(io::Error::other("Invalid stored report operation"));
    }
    let snapshot = Snapshot::restore(&stored.snapshot).map_err(io::Error::other)?;
    let history = InputHistory::restore(stored.history, policy).map_err(io::Error::other)?;
    Ok(Record {
        snapshot,
        history,
        scope: stored.scope,
        operation: stored.operation,
        decision: stored.decision,
    })
}

fn write_record(root: &Path, id: &str, bytes: &[u8]) -> io::Result<()> {
    let pending = root.join(format!("{id}.pending"));
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&pending)?;
    file.write_all(bytes)?;
    sync(&file)?;
    drop(file);
    fs::rename(pending, root.join(format!("{id}.json")))?;
    sync_directory(root)
}

fn sync(file: &File) -> io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        rustix::fs::fsync(file).map_err(io::Error::from)
    }
    #[cfg(not(target_os = "macos"))]
    {
        file.sync_all()
    }
}

fn sync_directory(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        sync(&File::open(path)?)
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Err(io::Error::other(
            "The durable Acme example requires Unix directory flushes",
        ))
    }
}
