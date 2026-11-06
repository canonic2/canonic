//! Bounded execution of the consumer's trusted read-only catalog check.
use crate::{Context, Error, Result, storage};
use std::{
    io::{Read, Seek, SeekFrom},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

const TIMEOUT: Duration = Duration::from_secs(30);

struct OwnedChild(Child);

impl Drop for OwnedChild {
    fn drop(&mut self) {
        self.stop();
    }
}

impl OwnedChild {
    fn stop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

/// Validate the trusted installed catalog with the shared deadline and output bound.
///
/// # Errors
/// Returns `CatalogCheck` on spawn, timeout, output, exit or JSON validation failure.
pub fn check(context: &Context) -> Result<()> {
    let mut command = Command::new(&context.program);
    command
        .args(&context.arguments)
        .args(&context.check_arguments)
        .current_dir(&context.root);
    run(&mut command, TIMEOUT)
        .map_err(|error| Error::CatalogCheck(format!("{error}; no agent configuration changed")))
}

fn run(command: &mut Command, timeout: Duration) -> std::result::Result<(), String> {
    // A private temporary file avoids pipe-reader threads that can hang when a
    // descendant inherits stdout. Never expose catalog stderr on protocol stdout.
    let mut stdout = tempfile::tempfile().map_err(|e| e.to_string())?;
    let output = stdout.try_clone().map_err(|e| e.to_string())?;
    let deadline = Instant::now() + timeout;
    let mut child = OwnedChild(
        command
            .stdin(Stdio::null())
            .stdout(output)
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("Installed MCP catalog check could not run: {e}"))?,
    );
    let status = loop {
        if stdout.metadata().map_err(|e| e.to_string())?.len() > storage::LIMIT {
            return Err("Installed MCP catalog check exceeded its stdout limit".into());
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(format!(
                "Installed MCP catalog check timed out after {} ms",
                timeout.as_millis()
            ));
        }
        if let Some(status) = child.0.try_wait().map_err(|e| e.to_string())? {
            break status;
        }
        std::thread::sleep(remaining.min(Duration::from_millis(10)));
    };
    stdout.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    stdout
        .take(storage::LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > storage::LIMIT {
        return Err("Installed MCP catalog check exceeded its stdout limit".into());
    }
    if !status.success() || serde_json::from_slice::<serde_json::Value>(&bytes).is_err() {
        return Err("Installed MCP catalog check failed or returned invalid JSON".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_deadline_kills_and_reaps_the_owned_child() {
        const CHILD: &str = "CANONIC_AGENT_KIT_PROBE_FIXTURE";
        if std::env::var_os(CHILD).is_some() {
            loop {
                std::thread::park();
            }
        }
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "preflight::tests::catalog_deadline_kills_and_reaps_the_owned_child",
                "--nocapture",
            ])
            .env(CHILD, "1");
        let error = run(&mut command, Duration::from_millis(100)).unwrap_err();
        assert!(error.contains("timed out"), "{error}");
        let mut child = OwnedChild(command.spawn().unwrap());
        child.stop();
        assert!(child.0.try_wait().unwrap().is_some());
    }

    #[test]
    fn catalog_stdout_limit_refuses_excessive_output() {
        const CHILD: &str = "CANONIC_AGENT_KIT_OUTPUT_FIXTURE";
        if std::env::var_os(CHILD).is_some() {
            use std::io::Write;
            std::io::stdout()
                .lock()
                .write_all(&vec![b'x'; usize::try_from(storage::LIMIT).unwrap() + 1])
                .unwrap();
            return;
        }
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "preflight::tests::catalog_stdout_limit_refuses_excessive_output",
                "--nocapture",
            ])
            .env(CHILD, "1");
        let error = run(&mut command, Duration::from_secs(2)).unwrap_err();
        assert!(error.contains("stdout limit"), "{error}");
    }

    #[cfg(unix)]
    #[test]
    fn catalog_accepts_json_and_refuses_invalid_output_and_exit_failure() {
        let mut valid = Command::new("/usr/bin/printf");
        valid.arg("{}");
        run(&mut valid, Duration::from_secs(2)).unwrap();
        let mut invalid = Command::new("/usr/bin/printf");
        invalid.arg("invalid JSON");
        assert!(run(&mut invalid, Duration::from_secs(2)).is_err());
        let mut failure = Command::new("/usr/bin/false");
        assert!(run(&mut failure, Duration::from_secs(2)).is_err());
    }
}
