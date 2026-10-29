//! One bounded atomic project-state owner; locks are never stolen from writers.
use super::{Checkpoint, Context, Error, model::State};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
const MAX_STATE: usize = 16 * 1024 * 1024;

#[cfg(test)]
mod tests;

pub(crate) struct Store {
    directory: PathBuf,
    root: String,
}
struct Lock {
    path: Option<PathBuf>,
}
impl Drop for Lock {
    fn drop(&mut self) {
        if let Some(path) = self.path.take() {
            let _ = fs::remove_file(path);
        }
    }
}
impl Lock {
    fn release(mut self) -> Result<(), std::io::Error> {
        if let Some(path) = self.path.take() {
            fs::remove_file(path)?;
        }
        Ok(())
    }
}
struct Temporary {
    path: PathBuf,
}
impl Drop for Temporary {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

impl Store {
    fn location(project: &Path) -> Result<Self, Error> {
        let root = project.canonicalize()?;
        if !root.is_dir() {
            return Err(Error::Invalid("project must be a directory"));
        }
        let root_text = root
            .to_str()
            .ok_or(Error::Invalid("project must be UTF-8"))?
            .to_owned();
        let directory = root.join(".shield").join("review");
        for p in [root.join(".shield"), directory.clone()] {
            match fs::symlink_metadata(&p) {
                Ok(m) if m.file_type().is_symlink() || !m.is_dir() => {
                    return Err(Error::Invalid(
                        "review directories cannot be links or files",
                    ));
                }
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
            }
        }
        Ok(Self {
            directory,
            root: root_text,
        })
    }
    pub(crate) fn open(project: &Path) -> Result<Self, Error> {
        let store = Self::location(project)?;
        if !store.directory.is_dir() {
            return Err(Error::Missing);
        }
        Ok(store)
    }
    #[cfg(test)]
    pub(crate) fn initialize(
        project: &Path,
        candidates: BTreeMap<String, Value>,
        revision: &str,
    ) -> Result<Value, Error> {
        Self::initialize_with(
            project,
            candidates,
            revision,
            serde_json::json!({"sources":{},"default":null,"installed":[],"projection":null}),
            false,
            BTreeMap::default(),
        )
    }
    #[cfg(test)]
    pub(crate) fn initialize_active(
        project: &Path,
        candidates: BTreeMap<String, Value>,
        revision: &str,
        operator: Value,
    ) -> Result<Value, Error> {
        Self::initialize_with(
            project,
            candidates,
            revision,
            operator,
            true,
            BTreeMap::default(),
        )
    }
    pub(crate) fn initialize_verified(
        project: &Path,
        candidates: BTreeMap<String, Value>,
        revision: &str,
        operator: Value,
        checkpoints: BTreeMap<String, Value>,
    ) -> Result<Value, Error> {
        Self::initialize_with(project, candidates, revision, operator, true, checkpoints)
    }
    fn initialize_with(
        project: &Path,
        candidates: BTreeMap<String, Value>,
        revision: &str,
        operator: Value,
        activate: bool,
        registry_checkpoints: BTreeMap<String, Value>,
    ) -> Result<Value, Error> {
        let store = Self::location(project)?;
        fs::create_dir_all(&store.directory)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&store.directory, fs::Permissions::from_mode(0o700))?;
        }
        if store.directory.join("state.json").exists() {
            return Err(Error::Invalid("review project already enrolled"));
        }
        store.ignore_local_state();
        let epoch = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| Error::Invalid("clock before epoch"))?
            .as_nanos();
        let project_id = crate::policy::fingerprint(
            format!("{}:{epoch}:{}", store.root, std::process::id()).as_bytes(),
        );
        let mut state = State {
            history_generation: history::generation()?,
            history_available: true,
            pending_history: vec![],
            history_rewrite: None,
            history_control: false,
            history_evicted: false,
            operator,
            schema_version: 1,
            project_id: project_id.clone(),
            project_root: store.root.clone(),
            sequence: 0,
            policy_revision: revision.into(),
            candidates,
            registry_checkpoints,
            active: None,
            read_requests: vec![],
            run_requests: vec![],
            proposals: vec![],
            dismissed_reviews: vec![],
            events: vec![],
            authorization_events: vec![],
            history_enabled: true,
            history_limit: if activate { 5000 } else { 500 },
        };
        if activate {
            let context = Context::new(state.project_id.clone(), state.project_root.clone(), "cli");
            let compiled = super::cache::build(&state, None)?;
            super::runtime::publish(&mut state, &context, None, compiled);
        }
        let lock = store.lock(&|| Ok(()))?;
        if store.directory.join("state.json").exists() {
            return Err(Error::Invalid("review project already enrolled"));
        }
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        private(&mut options);
        let ignore_path = store.directory.join(".gitignore");
        match options.open(&ignore_path) {
            Ok(mut ignore) => ignore.write_all(b"*\n")?,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                let metadata = fs::symlink_metadata(&ignore_path)?;
                if !metadata.is_file() || metadata.file_type().is_symlink() {
                    return Err(Error::Invalid("review ignore file cannot be a link"));
                }
                if fs::read_to_string(&ignore_path)?.trim() != "*" {
                    return Err(Error::Invalid(
                        "review ignore file must exclude local state",
                    ));
                }
            }
            Err(error) => return Err(error.into()),
        }
        store.write(&state)?;
        lock.release()?;
        Ok(
            json!({"schema_version":1,"project_id":project_id,"project_root":store.root,"policy_revision":revision,"mode":"local-review","enforcement":"not-installed"}),
        )
    }
    pub(crate) fn read(&self) -> Result<State, Error> {
        self.read_evaluated().map(|(state, _)| state)
    }

    /// Keep the validated active plan for workflows that also need its prefixes.
    pub(in crate::review) fn read_evaluated(
        &self,
    ) -> Result<(State, crate::configuration::Evaluator), Error> {
        let state = self.read_unprepared()?;
        let evaluator = crate::configuration::Evaluator::new(super::snapshots(&state.candidates)?)?;
        if evaluator.revision() != state.policy_revision {
            return Err(Error::Invalid(
                "review revision differs from retained policy",
            ));
        }
        if let Some(active) = &state.active {
            active.validate_evaluated(&state, evaluator.revision())?;
        }
        Ok((state, evaluator))
    }

    // Diagnostic handles prepare their own immutable active candidate and check its
    // revision. This path checks storage identity and bindings without rebuilding
    // unrelated review policies. It must not be used as mutation authorization.
    pub(in crate::review) fn read_unprepared(&self) -> Result<State, Error> {
        let state = self.read_history()?;
        let mut value =
            serde_json::to_value(state).map_err(|_| Error::Invalid("state encoding"))?;
        sources::checkpoint(self, &mut value, true)?;
        serde_json::from_value(value).map_err(|_| Error::Invalid("source checkpoint decoding"))
    }
    pub(in crate::review) fn captured_candidate(&self, reference: &Value) -> Result<Value, Error> {
        let mut value = reference.clone();
        sources::hydrate(self, &mut value)?;
        Ok(value)
    }
    pub(in crate::review) fn retain_candidates(
        &self,
        candidates: &BTreeMap<String, Value>,
    ) -> Result<BTreeMap<String, Value>, Error> {
        let mut value = json!(candidates);
        sources::compact(self, &mut value)?;
        serde_json::from_value(value).map_err(|_| Error::Invalid("retained source map invalid"))
    }

    // Current enrollment, authorization and source identities, without preparing policy.
    pub(in crate::review) fn read_checkpoint(&self) -> Result<State, Error> {
        let file = self.directory.join("state.json");
        let metadata = fs::symlink_metadata(&file)?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(Error::Invalid("review state must be a regular file"));
        }
        let mut bytes = vec![];
        fs::File::open(file)?
            .take((MAX_STATE + 1) as u64)
            .read_to_end(&mut bytes)?;
        if bytes.len() > MAX_STATE {
            return Err(crate::policy::Error::Limit.into());
        }
        let state: State = serde_json::from_slice(&bytes)
            .map_err(|_| Error::Invalid("corrupt review state; preserve it for recovery"))?;
        if state.project_id.len() != 64
            || !state.project_id.bytes().all(|b| b.is_ascii_hexdigit())
            || state.sequence > u64::MAX - 1024
        {
            return Err(Error::Invalid("invalid state identity"));
        }
        state.check(&Context::new(
            state.project_id.clone(),
            self.root.clone(),
            "cli",
        ))?;
        if let Some(active) = &state.active {
            active.validate_binding(&state)?;
        }
        super::read_approval::validate(&state)?;
        super::run_approval::validate(&state)?;
        Ok(state)
    }
    /// Captures the history view under the lock, then verifies, correlates and
    /// retains it after releasing the lock.
    pub(in crate::review) fn read_history(&self) -> Result<State, Error> {
        let (mut state, view) = {
            let _guard = history::lock(self)?;
            let state = self.read_checkpoint()?;
            let view = history::capture(self, &state);
            (state, view)
        };
        history::finish(view, &mut state);
        Ok(state)
    }
    /// Appends one callback's observations; no earlier record is read.
    pub(in crate::review) fn observe(
        &self,
        context: &Context,
        operation: impl FnOnce(&mut State) -> Result<Value, Error>,
    ) -> Result<Value, Error> {
        let _guard = history::lock(self)?;
        let mut state = self.read_checkpoint()?;
        state.check(context)?;
        if !state.history_enabled {
            return Ok(json!({"status":"disabled","request_id":null,"event_id":null}));
        }
        let result = operation(&mut state)?;
        if let Err(error) = history::publish(self, &state) {
            history::mark_failure(self, &state);
            return Err(error);
        }
        Ok(result)
    }
    fn lock(&self, checkpoint: Checkpoint<'_>) -> Result<Lock, Error> {
        let path = self.directory.join("state.lock");
        for _ in 0..20 {
            checkpoint()?;
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            private(&mut options);
            match options.open(&path) {
                Ok(mut file) => {
                    let lock = Lock { path: Some(path) };
                    file.write_all(format!("{}\n", std::process::id()).as_bytes())?;
                    return Ok(lock);
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    thread::sleep(Duration::from_millis(25));
                }
                Err(e) => return Err(e.into()),
            }
        }
        Err(Error::Locked)
    }
    pub(crate) fn update(
        &self,
        context: &Context,
        checkpoint: Checkpoint<'_>,
        operation: impl FnOnce(&mut State) -> Result<Value, Error>,
    ) -> Result<Value, Error> {
        self.update_inner(context, checkpoint, operation, true)
    }
    pub(in crate::review) fn update_checkpoint(
        &self,
        context: &Context,
        checkpoint: Checkpoint<'_>,
        operation: impl FnOnce(&mut State) -> Result<Value, Error>,
    ) -> Result<Value, Error> {
        self.update_inner(context, checkpoint, operation, false)
    }
    fn update_inner(
        &self,
        context: &Context,
        checkpoint: Checkpoint<'_>,
        operation: impl FnOnce(&mut State) -> Result<Value, Error>,
        full: bool,
    ) -> Result<Value, Error> {
        let lock = self.lock(checkpoint)?;
        let mut state = if full {
            self.read()?
        } else {
            self.read_checkpoint()?
        };
        state.check(context)?;
        state.compact_authorization();
        checkpoint()?;
        let result = operation(&mut state)?;
        if serde_json::to_vec(&result)
            .map_err(|_| Error::Invalid("response encoding"))?
            .len()
            > 1_048_576
        {
            return Err(crate::policy::Error::Limit.into());
        }
        checkpoint()?;
        state.compact_authorization();
        self.write(&state)?;
        lock.release()?;
        self.ignore_local_state();
        Ok(result)
    }
    /// Keeps review state and the in-project cache out of version control, so
    /// committing .shield shares only its policy files. Never replaces a file.
    fn ignore_local_state(&self) {
        let Some(path) = self
            .directory
            .parent()
            .map(|shield| shield.join(".gitignore"))
        else {
            return;
        };
        match fs::symlink_metadata(&path) {
            Err(_) => {
                let _ = fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&path)
                    .and_then(|mut file| std::io::Write::write_all(&mut file, LOCAL_STATE_IGNORE));
            }
            // An existing regular file keeps its lines; missing entries are appended.
            Ok(metadata) if metadata.is_file() => {
                let Ok(existing) = fs::read_to_string(&path) else {
                    return;
                };
                let missing: String = String::from_utf8_lossy(LOCAL_STATE_IGNORE)
                    .lines()
                    .filter(|line| !existing.lines().any(|l| l.trim() == *line))
                    .flat_map(|line| [line, "\n"])
                    .collect();
                if !missing.is_empty() {
                    let separator = if existing.is_empty() || existing.ends_with('\n') {
                        ""
                    } else {
                        "\n"
                    };
                    let _ = fs::OpenOptions::new()
                        .append(true)
                        .open(&path)
                        .and_then(|mut file| {
                            std::io::Write::write_all(
                                &mut file,
                                format!("{separator}{missing}").as_bytes(),
                            )
                        });
                }
            }
            Ok(_) => {}
        }
    }
    pub(crate) fn inspect_locked<T>(
        &self,
        context: &Context,
        checkpoint: Checkpoint<'_>,
        operation: impl FnOnce(&State) -> Result<T, Error>,
    ) -> Result<T, Error> {
        self.inspect_inner(context, checkpoint, operation, true)
    }
    pub(in crate::review) fn inspect_checkpoint<T>(
        &self,
        context: &Context,
        checkpoint: Checkpoint<'_>,
        operation: impl FnOnce(&State) -> Result<T, Error>,
    ) -> Result<T, Error> {
        self.inspect_inner(context, checkpoint, operation, false)
    }
    fn inspect_inner<T>(
        &self,
        context: &Context,
        checkpoint: Checkpoint<'_>,
        operation: impl FnOnce(&State) -> Result<T, Error>,
        full: bool,
    ) -> Result<T, Error> {
        let lock = self.lock(checkpoint)?;
        let state = if full {
            self.read()?
        } else {
            self.read_checkpoint()?
        };
        state.check(context)?;
        checkpoint()?;
        let result = operation(&state)?;
        lock.release()?;
        Ok(result)
    }
    /// Publish minimum authority before the short callback, without releasing the
    /// activation lock. A publication/release failure never restores a grant.
    #[cfg(target_os = "macos")]
    pub(crate) fn transition_handoff<T>(
        &self,
        context: &Context,
        prepare: impl FnOnce(&mut State) -> Result<(Value, bool), Error>,
        handoff: impl FnOnce(&Value) -> T,
    ) -> Result<(Value, T), Error> {
        let lock = self.lock(&|| Ok(()))?;
        let mut state = self.read_checkpoint()?;
        state.check(context)?;
        let (decision, changed) = prepare(&mut state)?;
        if changed {
            self.write(&state)?;
        }
        let result = handoff(&decision);
        lock.release()?;
        Ok((decision, result))
    }
    fn write(&self, state: &State) -> Result<(), Error> {
        let history_guard = match history::lock(self) {
            Ok(guard) => Some(guard),
            Err(error) if state.history_control => return Err(error),
            Err(_) => None,
        };
        if state.authorization_events.len() > 1024 {
            return Err(crate::policy::Error::Limit.into());
        }
        let bytes = history::encode(self, state, history_guard.is_some())?;
        if bytes.len() > MAX_STATE {
            return Err(crate::policy::Error::Limit.into());
        }
        let tick = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let temporary = Temporary {
            path: self
                .directory
                .join(format!(".state-{}-{tick}.tmp", std::process::id())),
        };
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        private(&mut options);
        let mut file = options.open(&temporary.path)?;
        #[cfg(test)]
        tests::observe(tests::Stage::Created);
        file.write_all(&bytes)?;
        #[cfg(test)]
        tests::observe(tests::Stage::Written);
        sync(&file)?;
        #[cfg(test)]
        tests::observe(tests::Stage::Synced);
        drop(file);
        fs::rename(&temporary.path, self.directory.join("state.json"))?;
        #[cfg(test)]
        tests::observe(tests::Stage::Renamed);
        #[cfg(unix)]
        sync(&fs::File::open(&self.directory)?)?;
        #[cfg(test)]
        tests::observe(tests::Stage::Complete);
        // Every state writer holds the writer lock, so no other `.state-*.tmp`
        // is still being written.
        remove_temporaries(&self.directory, |name| state_temporary(name).is_some());
        // Optional evidence cannot roll back an already durable authority
        // transition. Without pending observations nothing was lost, so a busy
        // history lock is not a failure.
        let pending = state.history_enabled && !state.pending_history.is_empty();
        if pending && (history_guard.is_none() || history::publish(self, state).is_err()) {
            history::mark_failure(self, state);
        }
        if history_guard.is_some() {
            history::cleanup(self, state);
        }
        Ok(())
    }
}

pub(in crate::review) mod history;
pub(in crate::review) mod sources;
const LOCAL_STATE_IGNORE: &[u8] =
    b"review/\ncache/\nconfig.local.yml\nrules.local.yml\nshield.local.lock\n";

fn private_directory(path: &Path) -> Result<(), Error> {
    match fs::create_dir(path) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e.into()),
    }
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(Error::Invalid("private directory cannot be a link or file"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}
/// Flushes `file`, or a directory opened as a file, before or after a rename.
///
/// macOS uses plain `fsync`: `File::sync_all` issues `F_FULLFSYNC` there,
/// which also empties the drive's write cache and serializes behind every
/// other process's flush. Other platforms use `File::sync_all`.
pub(in crate::review) fn sync(file: &fs::File) -> std::io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        nix::unistd::fsync(file).map_err(std::io::Error::from)
    }
    #[cfg(not(target_os = "macos"))]
    {
        file.sync_all()
    }
}
/// The writer PID of a `.state-<pid>-<nanoseconds>.tmp` name.
fn state_temporary(name: &str) -> Option<u32> {
    stamp_pid(name.strip_prefix(".state-")?.strip_suffix(".tmp")?, 2)
}
/// The writer PID of a temporary-name stamp `<pid>-<n>…` with exactly `fields`
/// decimal fields, as Shield's writers name their temporaries.
pub(in crate::review) fn stamp_pid(stamp: &str, fields: usize) -> Option<u32> {
    let parts: Vec<_> = stamp.split('-').collect();
    if parts.len() != fields
        || parts
            .iter()
            .any(|part| part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()))
    {
        return None;
    }
    parts[0].parse().ok()
}
/// Removes the regular files in `directory` whose names `stale` selects. A file
/// that cannot be removed stays for a later writer.
pub(in crate::review) fn remove_temporaries(directory: &Path, stale: impl Fn(&str) -> bool) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        if entry.file_type().is_ok_and(|kind| kind.is_file())
            && entry.file_name().to_str().is_some_and(&stale)
        {
            let _ = fs::remove_file(entry.path());
        }
    }
}
/// Removes temporaries in `directory` whose writer, the PID `writer` reads from
/// a Shield temporary name, is no longer running.
pub(in crate::review) fn remove_dead_temporaries(
    directory: &Path,
    writer: impl Fn(&str) -> Option<u32>,
) {
    remove_temporaries(directory, |name| {
        writer(name).is_some_and(|pid| !writer_running(pid))
    });
}
/// Whether the process that named a temporary may still be writing it. This
/// process, and a PID whose exit cannot be established, count as running, so a
/// live writer's temporary is never removed. A reused PID only keeps a leftover.
fn writer_running(pid: u32) -> bool {
    if pid == std::process::id() {
        return true;
    }
    #[cfg(unix)]
    {
        i32::try_from(pid)
            .ok()
            .and_then(rustix::process::Pid::from_raw)
            .is_none_or(|pid| {
                rustix::process::test_kill_process(pid) != Err(rustix::io::Errno::SRCH)
            })
    }
    #[cfg(not(unix))]
    {
        true
    }
}
/// A PID whose process has exited and been reaped, for leftover temporaries.
#[cfg(all(test, unix))]
pub(in crate::review) fn exited_pid() -> u32 {
    let mut child = std::process::Command::new("true")
        .spawn()
        .expect("exiting child");
    let pid = child.id();
    child.wait().expect("reaped child");
    pid
}
fn private(options: &mut OpenOptions) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    #[cfg(not(unix))]
    let _ = options;
}
