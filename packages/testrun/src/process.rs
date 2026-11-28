//! Bounded platform processes. No shell expansion and no child survives cancellation.
use crate::error::{Error, Result};
use std::{path::Path, process::Stdio, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::Command,
};

pub struct Output {
    pub bytes: Vec<u8>,
}

/// Cooperative cancellation for bounded filesystem work owned by a build.
#[derive(Clone, Default)]
pub(crate) struct Cancellation(Option<tokio::sync::watch::Receiver<bool>>);
impl Cancellation {
    pub(crate) fn new(receiver: tokio::sync::watch::Receiver<bool>) -> Self {
        Self(Some(receiver))
    }
    pub(crate) fn check(&self) -> Result<()> {
        if self.0.as_ref().is_some_and(|receiver| *receiver.borrow()) {
            Err(Error::new(
                "CANCELLED",
                "Build artifact processing cancelled",
            ))
        } else {
            Ok(())
        }
    }
}

/// A build's explicit working directory/environment, with cancellation settled
/// before the caller releases its project lock. Output stays private to the caller.
#[cfg(test)]
pub(crate) async fn build(
    argv: &[String],
    cwd: &Path,
    env: &std::collections::BTreeMap<String, String>,
    deadline: tokio::time::Instant,
    cancelled: tokio::sync::watch::Receiver<bool>,
) -> Result<Output> {
    build_output(argv, cwd, env, deadline, cancelled, None).await
}

pub(crate) async fn build_logged(
    argv: &[String],
    cwd: &Path,
    env: &std::collections::BTreeMap<String, String>,
    deadline: tokio::time::Instant,
    cancelled: tokio::sync::watch::Receiver<bool>,
    log: &Path,
) -> Result<Output> {
    let file = tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log)
        .await
        .map_err(|_| Error::new("STATE_IO", "Cannot open private operation log"))?;
    let size = file
        .metadata()
        .await
        .map_err(|_| Error::new("STATE_IO", "Cannot inspect operation log"))?
        .len();
    build_output(
        argv,
        cwd,
        env,
        deadline,
        cancelled,
        Some(std::sync::Arc::new(tokio::sync::Mutex::new((
            file,
            size,
            log.with_extension("process.json"),
        )))),
    )
    .await
}
type BuildLog = std::sync::Arc<tokio::sync::Mutex<(tokio::fs::File, u64, std::path::PathBuf)>>;
async fn build_output(
    argv: &[String],
    cwd: &Path,
    env: &std::collections::BTreeMap<String, String>,
    deadline: tokio::time::Instant,
    mut cancelled: tokio::sync::watch::Receiver<bool>,
    log: Option<BuildLog>,
) -> Result<Output> {
    if *cancelled.borrow() {
        return Err(Error::new("CANCELLED", "Build cancelled before dispatch"));
    }
    if tokio::time::Instant::now() >= deadline {
        return Err(Error::new(
            "TIMEOUT",
            "Build deadline expired before dispatch",
        ));
    }
    let mut command = Command::new(&argv[0]);
    command
        .args(&argv[1..])
        .current_dir(cwd)
        .env_clear()
        .envs(env)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(unix)]
    command.process_group(0);
    let mut child = command.spawn().map_err(|_| {
        Error::new(
            "PREREQUISITE_MISSING",
            "Cannot start build tool; install it and check the selected PATH",
        )
    })?;
    #[cfg(unix)]
    let mut group = OwnedGroup {
        id: child
            .id()
            .ok_or_else(|| Error::new("BUILD_IO", "Missing build process identity"))?,
        active: true,
    };
    let mut witness = serde_json::json!({"pid":child.id(),"startTime":child.id().and_then(start_time),"startedAtMs":crate::project::now(),"reaped":false});
    if let Some(log) = &log {
        crate::managed::storage::write(&log.lock().await.2, &witness)?;
    }
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let mut stdout_bytes = Vec::new();
    let mut stderr_bytes = Vec::new();
    let execution = async {
        let drain = async {
            tokio::try_join!(
                build_stream(stdout, &mut stdout_bytes, log.clone()),
                build_stream(stderr, &mut stderr_bytes, log.clone())
            )
        };
        let (_, status) = tokio::try_join!(drain, async {
            child
                .wait()
                .await
                .map_err(|_| Error::new("BUILD_IO", "Cannot reap build process"))
        })?;
        if stdout_bytes.len() + stderr_bytes.len() > 8 * 1024 * 1024 {
            return Err(Error::new("OUTPUT_LIMIT", "Build logs exceed 8 MiB"));
        }
        Ok(status)
    };
    let result = tokio::select! {
        biased;
        _ = async { if !*cancelled.borrow() { let _ = cancelled.changed().await; } } => Err(Error::new("CANCELLED","Build cancelled")),
        _ = tokio::time::sleep_until(deadline) => Err(Error::new("TIMEOUT","Build deadline expired")),
        result = execution => result,
    };
    #[cfg(unix)]
    group.stop();
    if result.is_err() {
        let _ = child.start_kill();
        child
            .wait()
            .await
            .map_err(|_| Error::new("CLEANUP_FAILED", "Cannot reap stopped build process"))?;
    }
    witness["reaped"] = serde_json::json!(true);
    witness["finishedAtMs"] = serde_json::json!(crate::project::now());
    if let Some(log) = &log {
        crate::managed::storage::write(&log.lock().await.2, &witness)?;
    }
    let mut bytes = stdout_bytes;
    if !stderr_bytes.is_empty() {
        bytes.extend_from_slice(b"\n");
        bytes.extend_from_slice(&stderr_bytes);
    }
    bytes.truncate(8 * 1024 * 1024);
    let status = result.map_err(|error| {
        error.details(serde_json::json!({"privateOutput":String::from_utf8_lossy(&bytes)}))
    })?;
    if !status.success() {
        // Private diagnostics are saved by the build workflow, never echoed in tool errors.
        return Err(Error::new("BUILD_FAILED", "Build tool failed; inspect the private build log")
            .details(serde_json::json!({"exitCode":status.code(),"privateOutput":String::from_utf8_lossy(&bytes)})));
    }
    Ok(Output { bytes })
}

async fn build_stream(
    mut stream: impl tokio::io::AsyncRead + Unpin,
    bytes: &mut Vec<u8>,
    log: Option<BuildLog>,
) -> Result<()> {
    let mut buffer = [0; 8192];
    loop {
        let size = stream
            .read(&mut buffer)
            .await
            .map_err(|_| Error::new("BUILD_IO", "Cannot read build output"))?;
        if size == 0 {
            return Ok(());
        }
        if let Some(log) = &log {
            let mut writer = log.lock().await;
            let remaining = (8 * 1024 * 1024_u64).saturating_sub(writer.1) as usize;
            let retained = size.min(remaining);
            writer
                .0
                .write_all(&buffer[..retained])
                .await
                .map_err(|_| Error::new("STATE_IO", "Cannot append private operation log"))?;
            writer.1 += retained as u64;
            if retained < size {
                return Err(Error::new("OUTPUT_LIMIT", "Operation log exceeds 8 MiB"));
            }
        }
        let remaining = (8 * 1024 * 1024_usize).saturating_sub(bytes.len());
        bytes.extend_from_slice(&buffer[..size.min(remaining)]);
        if size > remaining {
            return Err(Error::new("OUTPUT_LIMIT", "Build logs exceed 8 MiB"));
        }
    }
}

#[cfg(unix)]
struct OwnedGroup {
    id: u32,
    active: bool,
}
#[cfg(unix)]
impl OwnedGroup {
    fn stop(&mut self) {
        if self.active {
            // SAFETY: the negative identity addresses only our newly created group.
            unsafe {
                libc::kill(-(self.id as i32), libc::SIGKILL);
            }
            self.active = false;
        }
    }
}
#[cfg(unix)]
impl Drop for OwnedGroup {
    fn drop(&mut self) {
        self.stop();
    }
}

pub async fn run(
    program: impl AsRef<Path>,
    args: &[&str],
    deadline: Duration,
    max_bytes: usize,
) -> Result<Output> {
    run_input(program, args, &[], deadline, max_bytes).await
}
/// Run one native command with an explicitly constructed environment.
pub(crate) async fn run_environment(
    program: impl AsRef<Path>,
    args: &[&str],
    environment: &std::collections::BTreeMap<std::ffi::OsString, std::ffi::OsString>,
    deadline: Duration,
    max_bytes: usize,
) -> Result<Output> {
    run_owned(
        program,
        args,
        &[],
        deadline,
        max_bytes,
        Sink::Pipe { diagnostics: false },
        Some(environment),
    )
    .await
}
pub async fn run_input(
    program: impl AsRef<Path>,
    args: &[&str],
    input: &[u8],
    deadline: Duration,
    max_bytes: usize,
) -> Result<Output> {
    run_owned(
        program,
        args,
        input,
        deadline,
        max_bytes,
        Sink::Pipe { diagnostics: false },
        None,
    )
    .await
}

/// Run a command whose stderr carries a single NUL-byte start acknowledgement
/// alongside text diagnostics. Keep stdout available for its native payload.
pub(crate) async fn run_input_with_start_ack(
    program: impl AsRef<Path>,
    args: &[&str],
    input: &[u8],
    deadline: Duration,
    max_bytes: usize,
    on_start: impl FnOnce() + Send + 'static,
) -> Result<Output> {
    run_owned(
        program,
        args,
        input,
        deadline,
        max_bytes,
        Sink::StartAck(Box::new(on_start)),
        None,
    )
    .await
}
/// Opt-in bounded diagnostics for app tools; existing callers keep stderr private.
pub(crate) async fn run_diagnostic(
    program: impl AsRef<Path>,
    args: &[&str],
    deadline: Duration,
    max_bytes: usize,
) -> Result<Output> {
    run_owned(
        program,
        args,
        &[],
        deadline,
        max_bytes,
        Sink::Pipe { diagnostics: true },
        None,
    )
    .await
}
/// Bounded app-tool diagnostics with an explicitly constructed environment.
pub(crate) async fn run_diagnostic_environment(
    program: impl AsRef<Path>,
    args: &[&str],
    environment: &std::collections::BTreeMap<std::ffi::OsString, std::ffi::OsString>,
    deadline: Duration,
    max_bytes: usize,
) -> Result<Output> {
    run_owned(
        program,
        args,
        &[],
        deadline,
        max_bytes,
        Sink::Pipe { diagnostics: true },
        Some(environment),
    )
    .await
}
/// A deliberately finite sample of a streaming source. Retains a bounded prefix;
/// deadline/output-limit settlement uses the same owned group/child reaper.
pub(crate) async fn sample(
    program: impl AsRef<Path>,
    args: &[&str],
    deadline: Duration,
    max_bytes: usize,
) -> Result<Output> {
    run_owned(program, args, &[], deadline, max_bytes, Sink::Sample, None).await
}
/// Writes combined stdout and stderr to a new private `log` file. A non-zero
/// exit returns `PLATFORM_FAILED` whose `diagnostic` summarizes the log's error
/// lines, or its last lines when none mention an error.
pub(crate) async fn run_logged(
    program: impl AsRef<Path>,
    args: &[&str],
    deadline: Duration,
    log: &Path,
) -> Result<()> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options
        .open(log)
        .map_err(|_| Error::new("HOST_IO", "Cannot create a private platform log"))?;
    let errors = file
        .try_clone()
        .map_err(|_| Error::new("HOST_IO", "Cannot share a private platform log"))?;
    let result = run_owned(
        program,
        args,
        &[],
        deadline,
        0,
        Sink::File {
            stdout: file,
            stderr: errors,
        },
        None,
    )
    .await;
    match result {
        Err(mut error) if error.code == "PLATFORM_FAILED" => {
            let text = std::fs::read(log).unwrap_or_default();
            let mut details = error.details.take().unwrap_or(serde_json::json!({}));
            details["diagnostic"] = serde_json::json!(summary(&String::from_utf8_lossy(&text)));
            Err(error.details(details))
        }
        other => other.map(|_| ()),
    }
}
/// The lines of a tool log that explain a failure, bounded for error details.
pub(crate) fn summary(text: &str) -> String {
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    let mut selected: Vec<&str> = Vec::new();
    for line in &lines {
        let lower = line.to_ascii_lowercase();
        if (lower.contains("error") || lower.contains("failed") || lower.contains("reason"))
            && !selected.contains(line)
        {
            selected.push(line);
        }
    }
    // Tools also print harmless warnings with "error" in them, so the last
    // lines always follow the matches; they usually hold the final failure.
    selected.truncate(10);
    let tail = &lines[lines
        .len()
        .saturating_sub(if selected.is_empty() { 20 } else { 10 })..];
    for line in tail {
        if !selected.contains(line) {
            selected.push(line);
        }
    }
    let mut text = selected.join("\n");
    if text.len() > 4096 {
        let mut end = 4096;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
    }
    text
}
enum Sink {
    Sample,
    StartAck(Box<dyn FnOnce() + Send>),
    Pipe {
        diagnostics: bool,
    },
    File {
        stdout: std::fs::File,
        stderr: std::fs::File,
    },
}
#[expect(
    clippy::too_many_lines,
    reason = "Concurrent bounded standard streams share one owned process group and deadline"
)]
async fn run_owned(
    program: impl AsRef<Path>,
    args: &[&str],
    input: &[u8],
    deadline: Duration,
    max_bytes: usize,
    sink: Sink,
    environment: Option<&std::collections::BTreeMap<std::ffi::OsString, std::ffi::OsString>>,
) -> Result<Output> {
    let mut command = Command::new(program.as_ref());
    command.args(args).stdin(Stdio::piped()).kill_on_drop(true);
    if let Some(environment) = environment {
        command.env_clear().envs(environment);
    }
    let sampling = matches!(&sink, Sink::Sample);
    let mut start_ack = None;
    let diagnostics = match sink {
        Sink::Sample => {
            command.stdout(Stdio::piped()).stderr(Stdio::piped());
            true
        }
        Sink::Pipe { diagnostics } => {
            command.stdout(Stdio::piped()).stderr(if diagnostics {
                Stdio::piped()
            } else {
                Stdio::null()
            });
            diagnostics
        }
        Sink::StartAck(sender) => {
            command.stdout(Stdio::piped()).stderr(Stdio::piped());
            start_ack = Some(sender);
            false
        }
        Sink::File { stdout, stderr } => {
            command.stdout(stdout).stderr(stderr);
            false
        }
    };
    #[cfg(unix)]
    command.process_group(0);
    let mut child = command.spawn().map_err(|error| {
        Error::new(
            "PREREQUISITE_MISSING",
            "Could not start the selected platform tool",
        )
        .details(
            serde_json::json!({"osCode":error.raw_os_error(),"kind":format!("{:?}",error.kind())}),
        )
    })?;
    #[cfg(unix)]
    let mut group = OwnedGroup {
        id: child
            .id()
            .ok_or_else(|| Error::new("INTERNAL", "Owned process has no identity"))?,
        active: true,
    };
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| Error::new("INTERNAL", "Missing process input pipe"))?;
    // A file sink leaves no stdout pipe; reading then only waits for exit.
    let mut stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let mut sampled_bytes = Vec::new();
    let mut sample_stopped = false;
    let operation = async {
        let writing = async {
            stdin.write_all(input).await.map_err(|_| {
                Error::new("PLATFORM_IO", "Could not deliver private platform input")
            })?;
            drop(stdin);
            Ok::<_, Error>(())
        };
        let reading = async {
            let bytes = &mut sampled_bytes;
            if let Some(stdout) = stdout.as_mut() {
                stdout
                    .take(max_bytes as u64 + 1)
                    .read_to_end(bytes)
                    .await
                    .map_err(|_| Error::new("PLATFORM_IO", "Could not read platform result"))?;
            }
            if bytes.len() > max_bytes {
                #[cfg(unix)]
                group.stop();
                child.kill().await.map_err(|_| {
                    Error::new(
                        "CLEANUP_FAILED",
                        "Could not stop an oversized platform response",
                    )
                })?;
                if sampling {
                    sample_stopped = true;
                    bytes.truncate(max_bytes);
                    let status = child
                        .wait()
                        .await
                        .map_err(|_| Error::new("CLEANUP_FAILED", "Cannot reap bounded sample"))?;
                    return Ok((status, std::mem::take(bytes)));
                }
                return Err(Error::new(
                    "OUTPUT_LIMIT",
                    "Platform result exceeded its bounded output limit",
                ));
            }
            let status = child
                .wait()
                .await
                .map_err(|_| Error::new("PLATFORM_IO", "Could not reap platform process"))?;
            #[cfg(unix)]
            {
                group.stop();
            }
            Ok((status, std::mem::take(bytes)))
        };
        let errors = async {
            let mut bytes = Vec::new();
            if let Some(stderr) = stderr {
                let mut stderr = stderr.take(16385);
                let mut buffer = [0; 1024];
                loop {
                    let count = stderr.read(&mut buffer).await.map_err(|_| {
                        Error::new("PLATFORM_IO", "Cannot read platform diagnostic")
                    })?;
                    if count == 0 {
                        break;
                    }
                    let received = &buffer[..count];
                    if let Some(notify) = start_ack.take_if(|_| received.contains(&0)) {
                        notify();
                    }
                    bytes.extend_from_slice(received);
                }
                if bytes.len() > 16384 {
                    return Err(Error::new(
                        "OUTPUT_LIMIT",
                        "Platform diagnostic exceeded its limit",
                    ));
                }
            }
            Ok::<_, Error>(bytes)
        };
        let (_, (status, bytes), errors) = tokio::try_join!(writing, reading, errors)?;
        if !status.success() && !sample_stopped {
            let mut details = serde_json::json!({"exitCode":status.code()});
            if diagnostics {
                details["diagnostic"] = serde_json::json!(summary(&format!(
                    "{}\n{}",
                    String::from_utf8_lossy(&bytes),
                    String::from_utf8_lossy(&errors)
                )));
            }
            return Err(Error::new(
                "PLATFORM_FAILED",
                "Platform operation failed; verify target state and prerequisites",
            )
            .details(details));
        }
        Ok(Output { bytes })
    };
    match tokio::time::timeout(deadline, operation).await {
        Ok(result) => result,
        Err(_) => {
            #[cfg(unix)]
            group.stop();
            child.kill().await.map_err(|_| {
                Error::new(
                    "CLEANUP_FAILED",
                    "Could not stop timed-out platform process",
                )
            })?;
            if sampling {
                sampled_bytes.truncate(max_bytes);
                return Ok(Output {
                    bytes: sampled_bytes,
                });
            }
            Err(Error::new(
                "TIMEOUT",
                "Platform operation exceeded its deadline",
            ))
        }
    }
}

/// Native creation witness of a live process. macOS uses Unix microseconds,
/// Linux uses boot ticks, and Windows uses the kernel FILETIME. Compare only
/// on the same host. `None` means the process cannot be inspected as live.
pub(crate) fn start_time(pid: u32) -> Option<u64> {
    #[cfg(unix)]
    let pid = i32::try_from(pid).ok().filter(|pid| *pid > 0)?;
    #[cfg(target_os = "macos")]
    {
        const ZOMBIE: u32 = 5;
        let mut info = std::mem::MaybeUninit::<libc::proc_bsdinfo>::zeroed();
        let size = i32::try_from(std::mem::size_of::<libc::proc_bsdinfo>()).ok()?;
        // SAFETY: the buffer is a zeroed proc_bsdinfo of the size passed.
        let written = unsafe {
            libc::proc_pidinfo(
                pid,
                libc::PROC_PIDTBSDINFO,
                0,
                info.as_mut_ptr().cast(),
                size,
            )
        };
        if written != size {
            return None;
        }
        // SAFETY: proc_pidinfo filled the whole structure.
        let info = unsafe { info.assume_init() };
        (info.pbi_status != ZOMBIE)
            .then(|| info.pbi_start_tvsec * 1_000_000 + info.pbi_start_tvusec)
    }
    #[cfg(target_os = "linux")]
    {
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        // Fields after the parenthesized command: state is first, start time 20th.
        let mut fields = stat.rsplit_once(')')?.1.split_whitespace();
        if fields.next()? == "Z" {
            return None;
        }
        fields.nth(18)?.parse().ok()
    }
    #[cfg(windows)]
    {
        windows_start_time(pid)
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
    {
        let _ = pid;
        None
    }
}

#[cfg(windows)]
fn windows_start_time(pid: u32) -> Option<u64> {
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use windows_sys::Win32::{
        Foundation::{FILETIME, WAIT_TIMEOUT},
        System::Threading::{
            GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
            WaitForSingleObject,
        },
    };

    if pid == 0 {
        return None;
    }
    // SAFETY: only query and synchronization access are requested, with no
    // inheritance. A non-null result is a process handle owned by this call.
    let handle = unsafe {
        OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
            0,
            pid,
        )
    };
    if handle.is_null() {
        return None;
    }
    // SAFETY: ownership was acquired above and is transferred exactly once.
    let process = unsafe { OwnedHandle::from_raw_handle(handle) };
    let raw = process.as_raw_handle();
    let [mut creation, mut exit, mut kernel, mut user] = [FILETIME::default(); 4];
    // SAFETY: the owned handle and all output buffers stay valid for these
    // synchronous calls. A zero-duration wait checks whether it is still live.
    if unsafe {
        GetProcessTimes(raw, &mut creation, &mut exit, &mut kernel, &mut user) == 0
            || WaitForSingleObject(raw, 0) != WAIT_TIMEOUT
    } {
        return None;
    }
    Some(u64::from(creation.dwHighDateTime) << 32 | u64::from(creation.dwLowDateTime))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    #[tokio::test]
    async fn sampled_streams_retain_a_prefix_reap_owned_clients_and_preserve_real_failures() {
        let out = sample(
            "/bin/sh",
            &["-c", "echo $$; printf retained; exec sleep 30"],
            Duration::from_millis(200),
            65536,
        )
        .await
        .unwrap();
        let text = String::from_utf8(out.bytes).unwrap();
        let (pid, body) = text.split_once('\n').unwrap();
        assert_eq!(body, "retained");
        assert!(start_time(pid.parse().unwrap()).is_none());
        let out = sample(
            "/bin/sh",
            &["-c", "printf 123456789; exec sleep 30"],
            Duration::from_secs(1),
            4,
        )
        .await
        .unwrap();
        assert_eq!(out.bytes, b"1234");
        assert_eq!(
            sample(
                "/bin/sh",
                &["-c", "echo 'Device is locked' >&2; exit 7"],
                Duration::from_secs(1),
                1024
            )
            .await
            .err()
            .unwrap()
            .code,
            "PLATFORM_FAILED"
        );
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn logged_failures_keep_the_full_log_and_summarize_its_errors() {
        let directory = tempfile::tempdir().unwrap();
        let log = directory.path().join("step.log");
        let script = "i=0; while [ $i -lt 3000 ]; do echo \"compile line $i\"; i=$((i+1)); done; \
                      echo 'error: No profiles for com.example.acme were found' >&2; exit 65";
        let error = run_logged("/bin/sh", &["-c", script], Duration::from_secs(10), &log)
            .await
            .err()
            .unwrap();
        assert_eq!(error.code, "PLATFORM_FAILED");
        let details = error.details.unwrap();
        assert_eq!(details["exitCode"], 65);
        let diagnostic = details["diagnostic"].as_str().unwrap();
        assert!(diagnostic.starts_with("error: No profiles for com.example.acme were found"));
        assert!(diagnostic.contains("compile line 2999") && !diagnostic.contains("line 2000\n"));
        let text = std::fs::read_to_string(&log).unwrap();
        assert!(text.contains("compile line 2999") && text.contains("No profiles"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&log).unwrap().permissions().mode();
            assert_eq!(mode & 0o077, 0);
        }
        let ok = directory.path().join("ok.log");
        run_logged("/bin/sh", &["-c", "echo fine"], Duration::from_secs(5), &ok)
            .await
            .unwrap();
        assert!(
            run_logged("/bin/sh", &["-c", "true"], Duration::from_secs(5), &ok)
                .await
                .is_err(),
            "an existing log is never overwritten"
        );
    }
    #[test]
    fn summaries_fall_back_to_the_last_lines_and_stay_bounded() {
        let text: String = (0..50).map(|i| format!("step {i}\n")).collect();
        let summary = summary(&text);
        assert!(summary.starts_with("step 30") && summary.ends_with("step 49"));
        let long = format!("error: {}\n", "é".repeat(5000));
        assert!(super::summary(&long).len() <= 4096);
    }
    #[cfg(unix)]
    #[test]
    fn start_time_identifies_live_processes_and_refuses_exited_ones() {
        let own = start_time(std::process::id()).expect("own process is inspectable");
        assert_eq!(start_time(std::process::id()), Some(own));
        let mut child = std::process::Command::new("/bin/sh")
            .args(["-c", "exit 0"])
            .spawn()
            .unwrap();
        let pid = child.id();
        // Unreaped, the exited child is a zombie and has no live identity.
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while start_time(pid).is_some() && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(start_time(pid), None);
        child.wait().unwrap();
        assert_eq!(start_time(0), None);
        assert_eq!(start_time(u32::MAX), None);
    }
    #[cfg(windows)]
    #[test]
    fn windows_start_time_distinguishes_current_and_exited_processes() {
        let pid = std::process::id();
        let witness = start_time(pid).unwrap();
        assert!(witness > 0);
        assert_eq!(start_time(pid), Some(witness));
        let command = std::path::PathBuf::from(std::env::var_os("SystemRoot").unwrap())
            .join("System32")
            .join("cmd.exe");
        let mut child = std::process::Command::new(command)
            .args(["/d", "/c", "exit", "0"])
            .spawn()
            .unwrap();
        let child_pid = child.id();
        assert!(child.wait().unwrap().success());
        assert_eq!(start_time(child_pid), None);
        assert_eq!(start_time(0), None);
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn deployment_diagnostics_preserve_failure_reasons_and_bound_stderr() {
        for script in [
            "printf 'signature mismatch' >&2; exit 7",
            "printf 'Failure [VERSION_DOWNGRADE]'; exit 7",
        ] {
            let error = run_diagnostic("/bin/sh", &["-c", script], Duration::from_secs(3), 65536)
                .await
                .err()
                .unwrap();
            assert_eq!(error.code, "PLATFORM_FAILED");
            let details = error.details.unwrap();
            assert_eq!(details["exitCode"], 7);
            assert!(!details["diagnostic"].as_str().unwrap().is_empty());
        }
        let error = run_diagnostic(
            "/bin/sh",
            &["-c", "dd if=/dev/zero bs=20000 count=1 >&2"],
            Duration::from_secs(3),
            65536,
        )
        .await
        .err()
        .unwrap();
        assert_eq!(error.code, "OUTPUT_LIMIT");
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn stdout_install_refusal_survives_stderr_transfer_progress() {
        let error = run_diagnostic(
            "/bin/sh",
            &["-c", "printf 'Failure [INSTALL_FAILED_VERSION_DOWNGRADE]\\n'; printf '1 file pushed\\n' >&2; exit 1"],
            Duration::from_secs(3), 65536,
        ).await.err().unwrap();
        let details = error.details.unwrap();
        let diagnostic = details["diagnostic"].as_str().unwrap();
        assert!(diagnostic.contains("INSTALL_FAILED_VERSION_DOWNGRADE"));
        assert!(diagnostic.contains("1 file pushed"));
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn completed_children_do_not_leave_owned_descendants() {
        for exit in ["0", "7"] {
            let directory = tempfile::tempdir().unwrap();
            let pidfile = directory.path().join("child.pid");
            let result = run(
                "/bin/sh",
                &[
                    "-c",
                    "sleep 60 </dev/null >/dev/null 2>&1 & printf '%s' $! > \"$1\"; exit \"$2\"",
                    "fixture",
                    pidfile.to_str().unwrap(),
                    exit,
                ],
                Duration::from_secs(2),
                1024,
            )
            .await;
            let pid: i32 = std::fs::read_to_string(pidfile).unwrap().parse().unwrap();
            let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
            // SAFETY: signal zero observes only this fixture's recorded descendant.
            while unsafe { libc::kill(pid, 0) } == 0 {
                if tokio::time::Instant::now() >= deadline {
                    // SAFETY: clean up the explicitly owned fixture on regression.
                    unsafe { libc::kill(pid, libc::SIGKILL) };
                    panic!("Owned descendant survived parent exit {exit}");
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            if exit == "0" {
                assert!(result.is_ok());
            } else {
                assert_eq!(result.err().unwrap().code, "PLATFORM_FAILED");
            }
        }
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn cancellation_stops_owned_descendant_processes() {
        let directory = tempfile::tempdir().unwrap();
        let pidfile = directory.path().join("child.pid");
        let task_path = pidfile.to_str().unwrap().to_owned();
        let task = tokio::spawn(async move {
            run(
                "/bin/sh",
                &[
                    "-c",
                    "sleep 60 & child=$!; printf '%s' \"$child\" > \"$1\"; wait",
                    "fixture",
                    &task_path,
                ],
                Duration::from_secs(60),
                1024,
            )
            .await
        });
        let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
        let pid = loop {
            if let Ok(text) = std::fs::read_to_string(&pidfile)
                && let Ok(pid) = text.parse::<i32>()
            {
                break pid;
            }
            assert!(tokio::time::Instant::now() < deadline);
            tokio::time::sleep(Duration::from_millis(10)).await;
        };
        task.abort();
        assert!(task.await.err().unwrap().is_cancelled());
        let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
        loop {
            // SAFETY: signal zero observes this explicitly owned fixture process only.
            if unsafe { libc::kill(pid, 0) } != 0 {
                break;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "Owned descendant survived cancellation"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
    #[tokio::test]
    async fn bounds_output_and_deadline() {
        let input = b"private input with $() and quotes";
        assert_eq!(
            run_input("/bin/cat", &[], input, Duration::from_secs(1), 1024)
                .await
                .unwrap()
                .bytes,
            input
        );
        let output = run(
            "/usr/bin/printf",
            &["%s", "$(not executed)"],
            Duration::from_secs(1),
            100,
        )
        .await
        .unwrap();
        assert_eq!(output.bytes, b"$(not executed)");
        assert_eq!(
            run("/usr/bin/printf", &["abcdef"], Duration::from_secs(1), 3)
                .await
                .err()
                .unwrap()
                .code,
            "OUTPUT_LIMIT"
        );
        assert_eq!(
            run("/bin/sleep", &["10"], Duration::from_millis(20), 20)
                .await
                .err()
                .unwrap()
                .code,
            "TIMEOUT"
        );
    }
}
