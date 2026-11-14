//! The read-only Git process that answers a [`Query`]: the inspection Git
//! with its fixed settings, a scrubbed environment and a short deadline. It
//! takes no optional locks and runs no external diff, text conversion, hook
//! or filesystem monitor. Hosts without a fixed Git, such as Windows, answer
//! nothing, so the user decides.
use super::tree::Query;
use std::path::Path;

/// Whether the query finds a change in the folder Git runs in; None when Git
/// cannot answer, fails or exceeds the deadline.
pub(super) fn run(base: &Path, query: &Query) -> Option<bool> {
    #[cfg(not(windows))]
    return answer(base, query);
    #[cfg(windows)]
    {
        let _ = (base, query);
        None
    }
}

#[cfg(not(windows))]
fn answer(base: &Path, query: &Query) -> Option<bool> {
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};
    const DEADLINE: Duration = Duration::from_secs(2);
    let mut command = Command::new(crate::execution::GIT_EXECUTABLE);
    command
        .current_dir(base)
        .env_clear()
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("LC_ALL", "C")
        .args(crate::execution::GIT_SETTINGS)
        .stdin(Stdio::null())
        .stderr(Stdio::null());
    let listing = match query {
        Query::Diff { revisions, paths } => {
            command
                .args([
                    "diff",
                    "--quiet",
                    "--no-ext-diff",
                    "--no-textconv",
                    "--ignore-submodules=all",
                    "--end-of-options",
                ])
                .args(revisions)
                .arg("--")
                .args(paths)
                .stdout(Stdio::null());
            false
        }
        Query::Untracked { paths } => {
            command
                .args(["ls-files", "--others", "--exclude-standard", "-z", "--"])
                .args(paths)
                .stdout(Stdio::piped());
            true
        }
    };
    let mut child = command.spawn().ok()?;
    // Draining the listing on its own thread keeps a full pipe from holding
    // Git past the deadline.
    let reader = child.stdout.take().map(|mut output| {
        std::thread::spawn(move || std::io::copy(&mut output, &mut std::io::sink()).ok())
    });
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if started.elapsed() < DEADLINE => {
                std::thread::sleep(Duration::from_millis(2));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    let printed = reader.and_then(|reader| reader.join().ok().flatten());
    let status = status?;
    if listing {
        return if status.success() {
            printed.map(|bytes| bytes > 0)
        } else {
            None
        };
    }
    match status.code() {
        Some(0) => Some(false),
        Some(1) => Some(true),
        _ => None,
    }
}
