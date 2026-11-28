//! Explicit diagnostic driver process ownership beneath the browser binding.
//!
//! This adapter does not acquire dependencies. Its caller deliberately selects
//! trusted absolute paths. Production verified private preparation is pending.
//! Unix process groups and Windows jobs supply process ownership.

use super::{
    connection::{self, Connection},
    events::Events,
    processes::LaunchScope,
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    process::{ExitStatus, Stdio},
    sync::{Arc, Mutex},
};
#[cfg(unix)]
use tokio::process::Child;
use tokio::{io::AsyncReadExt, process::Command, task::JoinHandle, time::Instant};

const NODE_VERSION: &str = "v24.12.0";
const STDERR_BYTES: usize = 64 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("The diagnostic driver process adapter is unavailable on this host")]
    UnsupportedHost,
    #[error("Diagnostic driver paths must be absolute existing files/directories")]
    Paths,
    #[cfg(windows)]
    #[error("The Windows driver requires a Testrun-owned scratch directory")]
    PrivateScratchRequired,
    #[error("Selected Playwright package differs from the pinned binding")]
    Package,
    #[error("Diagnostic Node must match the selected development baseline")]
    Runtime,
    #[error("Driver preparation/start deadline expired")]
    Timeout,
    #[error("Owned driver release is incomplete; retain the driver and retry termination")]
    CleanupPending,
    #[error("Private driver I/O failed")]
    Io(#[from] std::io::Error),
    #[error("Diagnostic preparation failed during {stage}")]
    Preparation {
        stage: &'static str,
        #[source]
        cause: Box<Error>,
    },
    #[error(transparent)]
    Connection(#[from] connection::Error),
    #[error("Driver bootstrap failed; diagnostic bytes remain private")]
    Bootstrap {
        #[source]
        cause: Box<connection::Error>,
        diagnostics: Diagnostics,
    },
}

#[derive(Clone, Debug)]
pub struct Identity {
    pub driver_version: String,
    pub node_version: String,
    pub node_sha256: String,
    pub core_sha256: String,
    pub cli_sha256: String,
    pub client_language: &'static str,
}

/// An explicit development override, not a release bundle verification token.
/// Source hashes identify inspected bytes; acquisition trust is the caller's
/// selected installation. No ambient Node or global Playwright cache is used.
pub struct DiagnosticBundle {
    node: PathBuf,
    package: PathBuf,
    browsers: PathBuf,
    identity: Identity,
    #[cfg(all(test, feature = "acceptance-browser-public", target_os = "macos"))]
    child_observations: Option<(PathBuf, PathBuf)>,
}

impl DiagnosticBundle {
    #[cfg(all(test, feature = "acceptance-browser-public", target_os = "macos"))]
    pub(crate) fn observe_children(mut self, preload: PathBuf, report: PathBuf) -> Self {
        self.child_observations = Some((preload, report));
        self
    }

    pub async fn inspect(
        node: &Path,
        package: &Path,
        browsers: &Path,
        deadline: Instant,
    ) -> Result<Self, Error> {
        if !cfg!(unix) && !super::processes::windows_bundle_host() {
            return Err(Error::UnsupportedHost);
        }
        if !node.is_absolute() || !package.is_absolute() || !browsers.is_absolute() {
            return Err(Error::Paths);
        }
        tokio::time::timeout_at(deadline, async {
            let node = tokio::fs::canonicalize(node).await?;
            let package = tokio::fs::canonicalize(package).await?;
            let browsers = tokio::fs::canonicalize(browsers).await?;
            if !tokio::fs::metadata(&node).await?.is_file()
                || !tokio::fs::metadata(&browsers).await?.is_dir()
            {
                return Err(Error::Paths);
            }
            let mut bytes = Vec::new();
            tokio::fs::File::open(package.join("package.json"))
                .await?
                .take(65537)
                .read_to_end(&mut bytes)
                .await?;
            let metadata: Value = serde_json::from_slice(&bytes).map_err(|_| Error::Package)?;
            if bytes.len() > 65536
                || metadata["name"] != "playwright-core"
                || metadata["version"] != super::DRIVER_VERSION
            {
                return Err(Error::Package);
            }
            let core_sha256 = hash(&package.join("lib/coreBundle.js"))
                .await
                .map_err(|cause| preparation("driver hash", cause))?;
            let schema: Value = serde_json::from_str(include_str!("protocol-schema.json"))
                .expect("Generated schema metadata");
            let cli_sha256 = hash(&package.join("cli.js"))
                .await
                .map_err(|cause| preparation("CLI hash", cause))?;
            if schema["sourceSha256"] != core_sha256
                || schema["sourceFilesSha256"]["cli.js"] != cli_sha256
            {
                return Err(Error::Package);
            }
            let node_sha256 = hash(&node)
                .await
                .map_err(|cause| preparation("Node hash", cause))?;
            let node_version = probe_node(&node, &package, deadline)
                .await
                .map_err(|cause| preparation("Node version probe", cause))?;
            if node_version != NODE_VERSION {
                return Err(Error::Runtime);
            }
            Ok(Self {
                node,
                package,
                browsers,
                #[cfg(all(test, feature = "acceptance-browser-public", target_os = "macos"))]
                child_observations: None,
                identity: Identity {
                    driver_version: super::DRIVER_VERSION.into(),
                    node_version,
                    node_sha256,
                    core_sha256,
                    cli_sha256,
                    client_language: "rust",
                },
            })
        })
        .await
        .map_err(|_| Error::Timeout)?
    }

    pub fn identity(&self) -> &Identity {
        &self.identity
    }
}

fn preparation(stage: &'static str, cause: Error) -> Error {
    Error::Preparation {
        stage,
        cause: Box::new(cause),
    }
}

async fn hash(path: &Path) -> Result<String, Error> {
    let mut input = tokio::fs::File::open(path).await?;
    let mut buffer = [0; 65536];
    let mut hash = Sha256::new();
    loop {
        let read = input.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        hash.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

fn command(bundle_node: &Path, directory: &Path, scope: &LaunchScope) -> Command {
    let mut command = scope.command(bundle_node);
    command
        .current_dir(directory)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // Driver code never inherits NODE_OPTIONS, host credentials or arbitrary
    // application environment. Display/OS prerequisites are explicit passthrough.
    for key in ["SystemRoot", "WINDIR", "TMPDIR", "TMP", "TEMP"] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    display_environment(&mut command);
    #[cfg(unix)]
    command.env("PATH", "/usr/bin:/bin");
    #[cfg(target_os = "linux")]
    // SAFETY: umask is async-signal-safe and changes only the forked child.
    unsafe {
        command.pre_exec(|| {
            libc::umask(0o077);
            Ok(())
        });
    }
    command
}

pub(super) fn display_environment(command: &mut Command) {
    for key in ["DISPLAY", "WAYLAND_DISPLAY", "XAUTHORITY"] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    #[cfg(target_os = "linux")]
    if let Some(runtime) = std::env::var_os("XDG_RUNTIME_DIR") {
        // Wayland locates its user-session socket below this directory.
        command.env("XDG_RUNTIME_DIR", runtime);
    }
}

#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows::OwnedChild;

#[cfg(unix)]
pub(crate) struct OwnedChild {
    pub(crate) child: Child,
    group: u32,
    signalled: bool,
    forced: bool,
    pub(super) scope: LaunchScope,
}

#[cfg(unix)]
impl OwnedChild {
    pub(crate) fn spawn(command: &mut Command) -> Result<Self, Error> {
        Self::spawn_scoped(command, LaunchScope::default())
    }

    pub(crate) fn spawn_scoped(command: &mut Command, scope: LaunchScope) -> Result<Self, Error> {
        command.kill_on_drop(true);
        command.process_group(0);
        let child = command.spawn()?;
        let group = child.id().ok_or(Error::Paths)?;
        Ok(Self {
            child,
            group,
            signalled: false,
            forced: false,
            scope,
        })
    }

    pub(crate) fn signal(&mut self) -> Result<bool, Error> {
        self.scope.end();
        // The private Child has not been reaped: its kernel PID reservation
        // anchors this group identity even after leader exit. No saved PID,
        // process-name search or caller-supplied process ID is accepted.
        if self.signalled || self.child.id() != Some(self.group) {
            return Ok(false);
        }
        let group = i32::try_from(self.group).map_err(|_| Error::CleanupPending)?;
        // SAFETY: this group was created by spawn and its unreaped leader's
        // Child remains exclusively owned here, preventing PID reuse.
        if unsafe { libc::kill(-group, libc::SIGKILL) } == 0 {
            self.signalled = true;
            self.forced = true;
            return Ok(true);
        }
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ESRCH) {
            self.signalled = true;
            return Ok(false);
        }
        Err(error.into())
    }

    async fn release(&mut self, deadline: Instant) -> std::io::Result<bool> {
        self.scope.release(deadline).await
    }
}

#[cfg(unix)]
impl Drop for OwnedChild {
    fn drop(&mut self) {
        let _ = self.signal();
    }
}

async fn probe_node(node: &Path, directory: &Path, deadline: Instant) -> Result<String, Error> {
    let mut command = command(node, directory, &LaunchScope::default());
    command
        .arg("--version")
        .stdin(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(unix)]
    let spawned = OwnedChild::spawn(&mut command);
    #[cfg(windows)]
    let spawned = OwnedChild::spawn(command);
    let mut owned = spawned.map_err(|cause| preparation("Node version process launch", cause))?;
    #[cfg(windows)]
    {
        owned.child.stdin.take();
        owned.child.stderr.take();
    }
    let mut output = owned.child.stdout.take().ok_or(Error::Paths)?;
    let bytes = tokio::time::timeout_at(deadline, async {
        let mut bytes = Vec::new();
        (&mut output).take(129).read_to_end(&mut bytes).await?;
        Ok::<_, std::io::Error>(bytes)
    })
    .await
    .map_err(|_| Error::Timeout)??;
    // A completed --version probe is not a browser owner. Reap its natural
    // exit; the capsule still force-cleans on timeout/cancellation.
    let status = tokio::time::timeout_at(deadline, owned.child.wait())
        .await
        .map_err(|_| Error::CleanupPending)??;
    if bytes.len() > 128 || !status.success() {
        return Err(Error::Runtime);
    }
    Ok(String::from_utf8(bytes)
        .map_err(|_| Error::Runtime)?
        .trim()
        .into())
}

#[derive(Clone, Debug, Default)]
pub struct Diagnostics {
    pub stderr_tail: VecDeque<u8>,
    pub omitted_bytes: u64,
    pub read_failed: bool,
    pub stderr_closed: bool,
    // Keep the public bootstrap error compact without changing its fields.
    lifecycle: Box<lifecycle::Lifecycle>,
    process_wait: waiting::Samples,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Exit {
    pub code: Option<i32>,
    /// Termination fences the connection and force-cleans its owned group.
    /// Browser/trace/recording finalization must happen before this boundary.
    pub forced: bool,
}

pub struct Driver {
    owned: OwnedChild,
    connection: Connection,
    events: Option<Events>,
    identity: Identity,
    diagnostics: Arc<Mutex<Diagnostics>>,
    stderr: Option<JoinHandle<()>>,
    exit: Option<Exit>,
    witnesses: super::processes::Tracker,
    observation: Option<JoinHandle<std::io::Result<super::processes::Tracker>>>,
    /// Descendants could not be inspected before teardown: their release is
    /// unknown, so shutdown keeps reporting cleanup pending.
    uninspected: bool,
    /// Payload-free timings of the latest `shutdown` attempt's steps.
    shutdown_steps: Vec<(&'static str, u64)>,
    shutdown_resumed: bool,
}

impl Driver {
    pub async fn start(bundle: DiagnosticBundle, deadline: Instant) -> Result<Self, Error> {
        Self::start_in(bundle, deadline, None).await
    }

    pub(super) async fn start_in(
        bundle: DiagnosticBundle,
        deadline: Instant,
        temporary: Option<&Path>,
    ) -> Result<Self, Error> {
        if Instant::now() >= deadline {
            return Err(Error::Timeout);
        }
        if !cfg!(unix) && !super::processes::windows_bundle_host() {
            return Err(Error::UnsupportedHost);
        }
        let scope = LaunchScope::prepare(deadline).await?;
        let mut command = command(&bundle.node, &bundle.package, &scope);
        #[cfg(unix)]
        let _ = temporary;
        #[cfg(windows)]
        {
            let temporary = temporary.ok_or(Error::PrivateScratchRequired)?;
            for key in [
                "HOME",
                "USERPROFILE",
                "APPDATA",
                "LOCALAPPDATA",
                "TEMP",
                "TMP",
            ] {
                command.env(key, temporary);
            }
        }
        #[cfg(all(test, feature = "acceptance-browser-public", target_os = "macos"))]
        if let Some((preload, report)) = &bundle.child_observations {
            command
                .arg("--require")
                .arg(preload)
                .env("TESTRUN_CHILD_OBSERVATIONS", report);
        }
        // Fixed owned-process lifecycle logging, never ambient DEBUG. Raw bytes
        // remain in the bounded private tail; receipts expose allowlisted phases.
        command.env("DEBUG", "pw:browser").env("DEBUG_COLORS", "0");
        // The child starts in the verified package directory. Node's module
        // loader cannot resolve Windows verbatim drive paths from canonicalize.
        command
            .arg("-e")
            .arg(format!(
                "{}.run(require('./lib/coreBundle.js'), process, {});",
                include_str!("driver/bootstrap.js"),
                super::transport::MAX_FRAME_BYTES
            ))
            .env("PW_LANG_NAME", "javascript")
            // Owner decision: headed debugging is API-only. The pinned headed
            // pause hook would otherwise launch an Inspector app whose lifetime
            // races context closure; this is the SDK's own opt-out.
            .env("PW_CODEGEN_NO_INSPECTOR", "1")
            .env("PLAYWRIGHT_BROWSERS_PATH", &bundle.browsers);
        #[cfg(unix)]
        let mut owned = OwnedChild::spawn_scoped(&mut command, scope)?;
        #[cfg(windows)]
        let mut owned = OwnedChild::spawn(command)?;
        let input = owned.child.stdin.take().ok_or(Error::Paths)?;
        let output = owned.child.stdout.take().ok_or(Error::Paths)?;
        let mut errors = owned.child.stderr.take().ok_or(Error::Paths)?;
        let diagnostics = Arc::new(Mutex::new(Diagnostics::default()));
        let sink = diagnostics.clone();
        // Owned even while bootstrap is pending; cancellation aborts the reader
        // along with the retained process capsule instead of detaching a task.
        let mut stderr = AbortTask(Some(tokio::spawn(async move {
            let mut bytes = [0; 4096];
            loop {
                let read = match errors.read(&mut bytes).await {
                    Ok(0) => {
                        sink.lock().expect("Private diagnostics").stderr_closed = true;
                        break;
                    }
                    Ok(read) => read,
                    Err(_) => {
                        sink.lock().expect("Private diagnostics").read_failed = true;
                        break;
                    }
                };
                let mut state = sink.lock().expect("Private diagnostics");
                state.lifecycle.feed(&bytes[..read]);
                for byte in &bytes[..read] {
                    if state.stderr_tail.len() == STDERR_BYTES {
                        state.stderr_tail.pop_front();
                        state.omitted_bytes = state.omitted_bytes.saturating_add(1);
                    }
                    state.stderr_tail.push_back(*byte);
                }
            }
        })));
        let (connection, events) =
            Connection::open(output, input, &bundle.identity.driver_version, deadline)
                .await
                .map_err(|cause| Error::Bootstrap {
                    cause: Box::new(cause),
                    diagnostics: diagnostics.lock().expect("Private diagnostics").clone(),
                })?;
        Ok(Self {
            owned,
            connection,
            events: Some(events),
            identity: bundle.identity,
            diagnostics,
            stderr: stderr.0.take(),
            exit: None,
            witnesses: super::processes::Tracker::default(),
            observation: None,
            uninspected: false,
            shutdown_steps: Vec::new(),
            shutdown_resumed: false,
        })
    }

    pub fn connection(&self) -> &Connection {
        &self.connection
    }
    /// Private process observation for retained ownership and acceptance.
    pub(crate) fn pid(&self) -> Option<u32> {
        self.owned.child.id()
    }
    pub fn identity(&self) -> &Identity {
        &self.identity
    }
    pub fn take_events(&mut self) -> Option<Events> {
        self.events.take()
    }
    /// Raw stderr remains private; public diagnostics need redaction/artifact ownership.
    pub fn diagnostics(&self) -> Diagnostics {
        self.diagnostics
            .lock()
            .expect("Private diagnostics")
            .clone()
    }

    /// Close the pipe and wait for the unchanged driver's orderly browser
    /// finalizers. A timeout keeps the same process and stderr reader for retry;
    /// it never force-kills a driver that may still own browser processes, and
    /// ends one whose witnessed browser processes are all gone and that a fresh
    /// observation finds without live descendants (`forced`).
    /// A failed descendant inspection still closes the pipe, so browsers
    /// finalize, but their release stays unknown and cleanup stays pending.
    pub async fn shutdown(&mut self, deadline: Instant) -> Result<Exit, Error> {
        self.shutdown_steps.clear();
        self.shutdown_resumed = self.exit.is_some();
        let mut started = Instant::now();
        let mut step = |steps: &mut Vec<(&'static str, u64)>, name: &'static str| {
            steps.push((
                name,
                u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            ));
            started = Instant::now();
        };
        if self.exit.is_none() {
            if self.observe_before_close(deadline).await.is_err() {
                self.uninspected = true;
            }
            step(&mut self.shutdown_steps, "observe_descendants");
            tokio::time::timeout_at(deadline, self.connection.close())
                .await
                .map_err(|_| Error::CleanupPending)??;
            step(&mut self.shutdown_steps, "close_pipe");
            #[cfg(unix)]
            let status = waiting::exit(
                &mut self.owned,
                &mut self.witnesses,
                !self.uninspected,
                &self.diagnostics,
                deadline,
            )
            .await?;
            #[cfg(windows)]
            let status = self.owned.exit_after_eof(deadline).await?;
            step(&mut self.shutdown_steps, "driver_exit");
            self.exit = Some(exit(status, self.owned.forced));
        }
        let scope_released = self
            .owned
            .release(deadline)
            .await
            .map_err(|_| Error::CleanupPending)?;
        self.drain_stderr(deadline).await?;
        step(&mut self.shutdown_steps, "drain_stderr");
        if self.uninspected && !scope_released {
            return Err(Error::CleanupPending);
        }
        while !scope_released
            && !self
                .witnesses
                .released()
                .map_err(|_| Error::CleanupPending)?
        {
            if Instant::now() >= deadline {
                return Err(Error::CleanupPending);
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        step(&mut self.shutdown_steps, "descendants_released");
        Ok(self.exit.as_ref().expect("Reaped driver").clone())
    }
    /// The latest shutdown's completed steps, the step it stopped in, and how
    /// many witnessed descendants were still alive: names and counts only.
    pub(crate) fn shutdown_report(&self) -> Value {
        let diagnostics = self.diagnostics.lock().expect("Private diagnostics");
        const STEPS: [&str; 5] = [
            "observe_descendants",
            "close_pipe",
            "driver_exit",
            "drain_stderr",
            "descendants_released",
        ];
        // The step after the last completed one; a retry after the driver was
        // reaped starts at `drain_stderr`.
        let next = match self.shutdown_steps.last() {
            Some((last, _)) => STEPS
                .iter()
                .position(|step| step == last)
                .map(|index| index + 1),
            None if self.shutdown_resumed => Some(3),
            None => Some(0),
        };
        #[cfg(unix)]
        let alive = self.witnesses.remaining().ok();
        #[cfg(windows)]
        let alive = self
            .owned
            .snapshot()
            .ok()
            .map(|snapshot| snapshot.descendants_alive);
        serde_json::json!({
            "completedSteps":self.shutdown_steps.iter().map(|(name,elapsed)|serde_json::json!({"step":name,"elapsedMs":elapsed})).collect::<Vec<_>>(),
            "unfinishedStep":next.and_then(|index| STEPS.get(index)).copied(),
            "driverExited":self.exit.is_some(),
            "descendantsInspected":!self.uninspected,
            "witnessedProcesses":self.witnesses.witnessed(),
            "aliveProcesses":alive,
            "lifecycle":diagnostics.lifecycle.snapshot(),
            "stderrClosed":diagnostics.stderr_closed,
            "stderrReadFailed":diagnostics.read_failed,
            "processWait":diagnostics.process_wait.snapshot(),
        })
    }

    pub(crate) async fn observe_descendants(&mut self) -> Result<(), Error> {
        #[cfg(windows)]
        {
            let observed = self.owned.snapshot();
            let mut diagnostics = self.diagnostics.lock().expect("Private driver evidence");
            let elapsed = diagnostics.lifecycle.elapsed_ms();
            let completed = observed.is_ok();
            diagnostics.process_wait.record(elapsed, observed);
            if completed {
                Ok(())
            } else {
                Err(Error::CleanupPending)
            }
        }
        #[cfg(unix)]
        {
            if self.observation.is_none() {
                let Some(root) = self.owned.child.id() else {
                    return Ok(());
                };
                let mut tracker = self.witnesses.clone();
                self.observation = Some(tokio::task::spawn_blocking(move || {
                    tracker.observe(root)?;
                    Ok::<_, std::io::Error>(tracker)
                }));
            }
            let result = self
                .observation
                .as_mut()
                .expect("Owned witness query")
                .await;
            self.observation.take();
            self.witnesses = result.map_err(|_| Error::CleanupPending)??;
            Ok(())
        }
    }
    pub(crate) async fn observe_before_close(&mut self, deadline: Instant) -> Result<(), Error> {
        // Interruption retains the query. The caller records uncertainty at the
        // actual teardown boundary, so a cancelled read can recover before it.
        let refresh = async {
            if self.observation.is_some() {
                self.observe_descendants().await?;
            }
            // An adopted earlier query is not a fresh observation at this
            // boundary. Extend it with a scan started by this refresh.
            self.observe_descendants().await
        };
        match tokio::time::timeout_at(deadline, refresh).await {
            Ok(Ok(())) => Ok(()),
            _ => Err(Error::CleanupPending),
        }
    }
    pub(crate) fn note_teardown_inspection(&mut self, complete: bool) {
        // Once teardown proceeds through an unknown observation window, a later
        // successful scan cannot prove which descendants left that window.
        self.uninspected |= !complete;
    }
    #[cfg(all(test, feature = "acceptance-browser-lifecycle"))]
    pub(crate) fn witnessed_processes(&self) -> usize {
        self.witnesses.count()
    }

    async fn drain_stderr(&mut self, deadline: Instant) -> Result<(), Error> {
        if let Some(reader) = self.stderr.as_mut() {
            match tokio::time::timeout_at(deadline, reader).await {
                Err(_) => return Err(Error::CleanupPending),
                Ok(Err(_)) => {
                    self.diagnostics
                        .lock()
                        .expect("Private diagnostics")
                        .read_failed = true
                }
                Ok(Ok(())) => {}
            }
            self.stderr = None;
        }
        Ok(())
    }

    /// Explicit forced process cleanup, not a browser workflow disconnect or
    /// finalizer. The workflow must stop admission/finalize evidence first.
    /// Cancellation retains the child/reader on self for an explicit retry.
    pub async fn terminate(&mut self, deadline: Instant) -> Result<Exit, Error> {
        if self.exit.is_none() {
            if Instant::now() >= deadline {
                return Err(Error::CleanupPending);
            }
            // Signal before stdin EOF can make the leader exit. The capsule
            // retains the unreaped Child and remembers a successful group kill.
            self.owned.signal()?;
            tokio::time::timeout_at(deadline, self.connection.close())
                .await
                .map_err(|_| Error::CleanupPending)??;
            let status = tokio::time::timeout_at(deadline, self.owned.child.wait())
                .await
                .map_err(|_| Error::CleanupPending)??;
            self.exit = Some(exit(status, self.owned.forced));
        }
        self.owned
            .release(deadline)
            .await
            .map_err(|_| Error::CleanupPending)?;
        self.drain_stderr(deadline).await?;
        Ok(self.exit.as_ref().expect("Reaped driver").clone())
    }
}

fn exit(status: ExitStatus, forced: bool) -> Exit {
    Exit {
        code: status.code(),
        forced,
    }
}

impl Drop for Driver {
    fn drop(&mut self) {
        if let Some(task) = self.observation.take() {
            task.abort();
        }
        if let Some(reader) = self.stderr.take() {
            reader.abort();
        }
    }
}

struct AbortTask(Option<JoinHandle<()>>);
impl Drop for AbortTask {
    fn drop(&mut self) {
        if let Some(task) = &self.0 {
            task.abort();
        }
    }
}

mod lifecycle;
mod waiting;

#[cfg(all(test, feature = "acceptance-browser-public", target_os = "macos"))]
mod close_diagnostic;

#[cfg(all(test, feature = "acceptance-browser-public", target_os = "macos"))]
mod paused_close;

#[cfg(all(test, feature = "acceptance-browser-driver", unix))]
mod tests;

#[cfg(all(test, target_os = "linux"))]
mod linux_tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[tokio::test]
    async fn linux_driver_child_creates_private_files() {
        let directory = tempfile::tempdir().unwrap();
        let status = command(
            Path::new("/bin/sh"),
            directory.path(),
            &LaunchScope::default(),
        )
        .args(["-c", ": > marker"])
        .status()
        .await
        .unwrap();
        assert!(status.success());
        let mode = std::fs::metadata(directory.path().join("marker"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}
