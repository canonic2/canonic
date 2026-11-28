//! Build/install/start ownership and rollback for the signed XCTest runner.
use super::rpc::Runner;
use crate::{
    artifacts,
    error::{Error, Result},
    process,
};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};
use uuid::Uuid;

pub(super) struct PreparedRunner {
    pub(super) runner: Runner,
    pub(super) configuration_dir: PathBuf,
    pub(super) process: std::sync::Arc<super::host::Host>,
    pub(super) heartbeat: Option<tokio::task::JoinHandle<()>>,
    pub(super) stopped: bool,
    pub(super) serial: String,
    pub(super) native_pid: Option<u32>,
    pub(super) signing: Value,
    pub(super) _cache: Option<crate::lease::ControlLease>,
}
impl Drop for PreparedRunner {
    fn drop(&mut self) {
        if let Some(task) = self.heartbeat.take() {
            task.abort();
        }
        let _ = std::fs::remove_dir_all(&self.configuration_dir);
    }
}
const XCRUN: &str = "/usr/bin/xcrun";
/// The runner app's bundle ID; Testrun owns this app and may replace it.
pub(super) const RUNNER_BUNDLE: &str = "com.canonic.testrun.RunnerTests.xctrunner";

/// Private per-step logs of one preparation. They are deleted after success
/// and kept after a failure, whose error names the step and the log.
pub(super) struct StepLogs {
    directory: PathBuf,
    keep: std::sync::atomic::AtomicBool,
}
impl StepLogs {
    fn new() -> Result<Self> {
        let directory =
            std::env::temp_dir().join(format!("testrun-runner-logs-{}", Uuid::new_v4()));
        artifacts::directory(&directory)?;
        Ok(Self {
            directory,
            keep: std::sync::atomic::AtomicBool::new(false),
        })
    }
    pub(super) async fn run(
        &self,
        step: &'static str,
        program: &str,
        args: &[&str],
        deadline: Duration,
    ) -> Result<()> {
        let log = self.directory.join(format!(
            "{step}-{}.log",
            std::fs::read_dir(&self.directory).map_or(0, Iterator::count)
        ));
        process::run_logged(program, args, deadline, &log)
            .await
            .map_err(|error| {
                self.keep.store(true, std::sync::atomic::Ordering::SeqCst);
                step_error(step, &log, error)
            })
    }
}
impl Drop for StepLogs {
    fn drop(&mut self) {
        if !self.keep.load(std::sync::atomic::Ordering::SeqCst) {
            let _ = std::fs::remove_dir_all(&self.directory);
        }
    }
}
/// Names the failed step and keeps the platform cause, diagnostic and log.
fn step_error(step: &str, log: &Path, cause: Error) -> Error {
    let code = match step {
        "build" => "RUNNER_BUILD_FAILED",
        "configure" => "RUNNER_CONFIGURE_FAILED",
        "verify" => "RUNNER_SIGNATURE_INVALID",
        _ => "RUNNER_INSTALL_FAILED",
    };
    let mut details = cause.details.clone().unwrap_or_else(|| json!({}));
    details["step"] = json!(step);
    details["cause"] = json!(cause.code);
    details["log"] = json!(log);
    let message = match step {
        "build" => "Xcode could not build and sign the device runner",
        "configure" => "The runner's private session configuration could not be copied",
        "verify" => "The runner's signature or signing certificate could not be verified",
        _ => "The signed runner could not be installed on the device",
    };
    let mut error = Error::new(code, message).details(details);
    error.retryable = cause.retryable;
    error
}
/// iOS refuses to upgrade an app signed by another team in place.
fn signed_by_other_team(error: &Error) -> bool {
    error.details.as_ref().is_some_and(|details| {
        details["diagnostic"]
            .as_str()
            .is_some_and(|text| text.contains("MismatchedApplicationIdentifierEntitlement"))
    })
}
struct PreparationGuard(Option<PathBuf>);
impl Drop for PreparationGuard {
    fn drop(&mut self) {
        if let Some(path) = self.0.take() {
            let _ = std::fs::remove_dir_all(path);
        }
    }
}
impl PreparedRunner {
    #[expect(
        clippy::too_many_lines,
        reason = "One guarded build/install/start transaction owns unpublished resources"
    )]
    pub(super) async fn start(
        serial: &str,
        device_id: u32,
        team: &str,
        source: &Path,
    ) -> Result<Self> {
        let directory = std::env::temp_dir().join(format!("testrun-device-{}", Uuid::new_v4()));
        artifacts::directory(&directory)?;
        let mut guard = PreparationGuard(Some(directory.clone()));
        let logs = StepLogs::new()?;
        let products = super::cache::acquire(source, serial, team, &logs).await?;
        let runner_app = products.app;
        let xctestrun = products.descriptor;
        let install = [
            "devicectl",
            "device",
            "install",
            "app",
            "--device",
            serial,
            utf8(&runner_app)?,
        ];
        let timeout = Duration::from_secs(60);
        match logs.run("install", XCRUN, &install, timeout).await {
            Err(error) if signed_by_other_team(&error) => {
                // Only Testrun's own runner app is removed; iOS refuses to
                // upgrade it in place when another team signed it.
                logs.run(
                    "uninstall",
                    XCRUN,
                    &[
                        "devicectl",
                        "device",
                        "uninstall",
                        "app",
                        "--device",
                        serial,
                        RUNNER_BUNDLE,
                    ],
                    timeout,
                )
                .await?;
                logs.run("install", XCRUN, &install, timeout).await?;
                logs.keep.store(false, std::sync::atomic::Ordering::SeqCst);
            }
            other => other?,
        }
        let token = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        let session_id = Uuid::new_v4().to_string();
        let port = 8101;
        let configuration = directory.join("testrun-session.json");
        let expires_at = (time::OffsetDateTime::now_utc() + time::Duration::hours(8))
            .format(&time::format_description::well_known::Rfc3339)
            .map_err(|_| Error::new("INTERNAL", "Cannot format runner expiry"))?;
        artifacts::write(
            &configuration,
            &serde_json::to_vec(
                &json!({"sessionId":session_id,"token":token,"port":port,"expiresAt":expires_at}),
            )
            .map_err(|_| Error::new("INTERNAL", "Cannot encode private runner configuration"))?,
        )?;
        logs.run(
            "configure",
            XCRUN,
            &[
                "devicectl",
                "device",
                "copy",
                "to",
                "--device",
                serial,
                "--source",
                utf8(&configuration)?,
                "--destination",
                "Documents/testrun-session.json",
                "--domain-type",
                "appDataContainer",
                "--domain-identifier",
                RUNNER_BUNDLE,
            ],
            Duration::from_secs(30),
        )
        .await?;
        std::fs::remove_file(&configuration).map_err(|_| {
            Error::new(
                "CLEANUP_FAILED",
                "Cannot remove private host runner configuration",
            )
        })?;
        let host_log = host_log(&directory)?;
        let host_errors = host_log
            .try_clone()
            .map_err(|_| Error::new("HOST_IO", "Cannot share the XCTest host log"))?;
        let process = tokio::process::Command::new("/usr/bin/xcodebuild")
            .args([
                "test-without-building",
                "-xctestrun",
                utf8(&xctestrun)?,
                "-destination",
                &format!("id={serial}"),
                "-only-testing:TestrunRunnerTests/TestrunRunnerTests/testServeOwnedSession",
            ])
            .stdin(std::process::Stdio::null())
            .stdout(host_log)
            .stderr(host_errors)
            .kill_on_drop(true)
            .spawn()
            .map_err(|_| {
                Error::new(
                    "RUNNER_START_FAILED",
                    "Cannot launch the owned XCTest runner",
                )
            })?;
        let process = std::sync::Arc::new(super::host::Host::new(process));
        let prepared = Self {
            runner: Runner {
                device_id,
                session_id,
                token,
                port,
                host: Some(std::sync::Arc::downgrade(&process)),
            },
            configuration_dir: directory,
            process,
            heartbeat: None,
            stopped: false,
            serial: serial.to_owned(),
            native_pid: None,
            signing: products.signing,
            _cache: Some(products.lease),
        };
        logs.keep.store(false, std::sync::atomic::Ordering::SeqCst);
        guard.0.take();
        Ok(prepared)
    }
}
const HOST_LOG: &str = "xctest-host.log";
fn host_log(directory: &Path) -> Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
        .open(directory.join(HOST_LOG))
        .map_err(|_| Error::new("HOST_IO", "Cannot create the XCTest host log"))
}
/// Adds the XCTest host's summarized output to a readiness failure while the
/// owned preparation directory still exists.
pub(super) fn with_host_diagnostic(directory: &Path, mut error: Error) -> Error {
    let text = std::fs::read(directory.join(HOST_LOG)).unwrap_or_default();
    let diagnostic = process::summary(&String::from_utf8_lossy(&text));
    let cause = error.code.clone();
    let (code, message) = startup_failure(&cause, &String::from_utf8_lossy(&text));
    error.code = code.into();
    error.message = message.into();
    let mut details = error.details.take().unwrap_or_else(|| json!({}));
    details["step"] = json!("start");
    details["cause"] = json!(cause);
    details["diagnostic"] = json!(diagnostic);
    if code == "UNTRUSTED_DEVELOPER" {
        details["settingsPath"] =
            json!("Settings > General > VPN & Device Management > Developer App");
        details["manualAction"] =
            json!("Trust the runner's developer on the selected iPhone, then run setup again");
        details["warning"] = json!(super::signing::PERSONAL_WARNING);
    }
    error.details(details)
}

fn startup_failure<'a>(cause: &'a str, diagnostic: &str) -> (&'a str, &'static str) {
    let lower = diagnostic.to_ascii_lowercase();
    if lower.contains("untrusted developer")
        || lower.contains("verify that the developer app certificate")
        || lower.contains("code signature") && lower.contains("not trusted")
    {
        return (
            "UNTRUSTED_DEVELOPER",
            "Trust the runner's developer on the iPhone in Settings > General > VPN & Device Management > Developer App, then run setup again. Free Personal Team provisioning profiles expire 7 days after issuance",
        );
    }
    if diagnostic.contains("RUNNER_RUNTIME_UNSUPPORTED") {
        return (
            "RUNNER_RUNTIME_UNSUPPORTED",
            "This XCTest runtime is incompatible with bounded runner automation; runner setup did not become ready",
        );
    }
    if diagnostic.contains("Failed to initiate daemon session") {
        return (
            "RUNNER_START_FAILED",
            "XCTest could not establish its test-manager session. Start the runner through connect with setup, not by opening its app icon; inspect the preserved startup diagnostic",
        );
    }
    if cause == "RUNNER_EXITED" {
        return (
            "RUNNER_START_FAILED",
            "The XCTest host exited before the runner became ready; inspect the startup diagnostic before reconnecting with setup",
        );
    }
    (
        cause,
        "The runner was installed but did not answer. Inspect the startup diagnostic and phone. If the phone shows an XCTest UI Automation passcode prompt, the user must approve it before connecting with setup again",
    )
}
pub(super) async fn wait_ready(runner: &Runner) -> Result<Value> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    loop {
        match runner
            .call("hello", json!({}), None, Duration::from_secs(3))
            .await
        {
            Ok(reply) => {
                return Ok(reply.data);
            }
            Err(error) if error.code == "RUNNER_EXITED" => return Err(error),
            Err(_) if tokio::time::Instant::now() < deadline => {
                tokio::time::sleep(Duration::from_millis(250)).await
            }
            Err(error) => return Err(error),
        }
    }
}
impl PreparedRunner {
    pub(super) async fn stop(&mut self) -> Result<()> {
        let runner = self.runner.clone();
        let serial = self.serial.clone();
        let native_pid = self.native_pid;
        self.stop_with(
            async move {
                // The runner may wait up to its 15-second input grace before
                // ending a wedged gesture itself.
                runner
                    .call_remote("stop", json!({}), None, Duration::from_secs(20))
                    .await
                    .map(|_| ())
            },
            async move { super::termination::verify_absent(&serial, native_pid).await },
        )
        .await
    }
    /// A refusal must retain the live runner, its heartbeat and preparation.
    /// After acknowledgement, retries finish only local cleanup.
    pub(super) async fn stop_with(
        &mut self,
        remote: impl std::future::Future<Output = Result<()>>,
        verify_exit: impl std::future::Future<Output = Result<()>>,
    ) -> Result<()> {
        if !self.stopped {
            if let Err(error) = remote.await {
                // Busy is positive evidence that this authenticated runner is
                // still serving. Do not kill it or replace its input session.
                if error.code == "RUNNER_BUSY" {
                    return Err(error);
                }
                verify_exit.await.map_err(|cause| {
                    Error::new(
                        "CLEANUP_FAILED",
                        "Runner stop was not acknowledged and its exit could not be verified",
                    )
                    .details(json!({"stop":error,"verification":cause}))
                })?;
            }
            self.stopped = true;
        }
        if let Some(task) = self.heartbeat.take() {
            task.abort();
        }
        self.process.finish().await?;
        match std::fs::remove_dir_all(&self.configuration_dir) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => {
                return Err(Error::new(
                    "CLEANUP_FAILED",
                    "Cannot remove owned runner preparation files",
                ));
            }
        }
        Ok(())
    }
}
pub(super) fn utf8(path: &Path) -> Result<&str> {
    path.to_str()
        .ok_or_else(|| Error::invalid("Owned runner path is not UTF-8"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_reports_runtime_and_daemon_failures_without_an_unlock_diagnosis() {
        for (cause, diagnostic, code) in [
            (
                "RUNNER_EXITED",
                "Failed to initiate daemon session: com.apple.testmanagerd: No such process",
                "RUNNER_START_FAILED",
            ),
            (
                "RUNNER_EXITED",
                "RUNNER_RUNTIME_UNSUPPORTED",
                "RUNNER_RUNTIME_UNSUPPORTED",
            ),
            ("RUNNER_EXITED", "XCTest host exited", "RUNNER_START_FAILED"),
        ] {
            let (actual, message) = startup_failure(cause, diagnostic);
            assert_eq!(actual, code);
            assert!(!message.contains("passcode"));
        }
    }

    #[test]
    fn untrusted_developer_has_distinct_recovery_without_misclassifying_expiry() {
        for diagnostic in [
            "Untrusted Developer",
            "Verify that the Developer App certificate for your account is trusted on your device",
            "The code signature is not trusted",
        ] {
            let (code, message) = startup_failure("RUNNER_EXITED", diagnostic);
            assert_eq!(code, "UNTRUSTED_DEVELOPER");
            assert!(message.contains("Settings > General > VPN & Device Management"));
            assert!(message.contains("7 days"));
        }
        for diagnostic in [
            "Provisioning profile has expired",
            "Code signature is invalid",
            "Developer Mode is disabled",
        ] {
            assert_ne!(
                startup_failure("RUNNER_EXITED", diagnostic).0,
                "UNTRUSTED_DEVELOPER"
            );
        }
    }

    /// devicectl's refusal when the installed runner has another team's signature.
    const OTHER_TEAM: &str = "ERROR: Failed to install the app on the device. (com.apple.dt.CoreDeviceError error 3002 (0xBBA))
Upgrade's application-identifier entitlement string (ACME000002.com.canonic.testrun.RunnerTests.xctrunner) does not match installed application's application-identifier string (ACME000001.com.canonic.testrun.RunnerTests.xctrunner); rejecting upgrade. (MIInstallerErrorDomain error 64 (0x40))
LegacyErrorString = MismatchedApplicationIdentifierEntitlement";

    #[test]
    fn step_failures_name_the_step_and_keep_the_platform_diagnostic() {
        let cause = Error::new("PLATFORM_FAILED", "Platform operation failed")
            .details(json!({"exitCode":1,"diagnostic":process::summary(OTHER_TEAM)}));
        assert!(signed_by_other_team(&cause));
        let error = step_error("install", Path::new("/tmp/acme/install-0.log"), cause);
        assert_eq!(error.code, "RUNNER_INSTALL_FAILED");
        let details = error.details.unwrap();
        assert_eq!(details["step"], "install");
        assert_eq!(details["cause"], "PLATFORM_FAILED");
        assert_eq!(details["log"], "/tmp/acme/install-0.log");
        assert!(
            details["diagnostic"]
                .as_str()
                .unwrap()
                .contains("does not match installed")
        );
        let build = step_error(
            "build",
            Path::new("/tmp/b.log"),
            Error::new("TIMEOUT", "late"),
        );
        assert_eq!(build.code, "RUNNER_BUILD_FAILED");
        assert_eq!(build.details.unwrap()["cause"], "TIMEOUT");
        let unrelated = Error::new("PLATFORM_FAILED", "failed")
            .details(json!({"diagnostic":"ERROR: The device is locked"}));
        assert!(!signed_by_other_team(&unrelated));
    }

    #[tokio::test]
    async fn step_logs_are_removed_after_success_and_kept_after_failure() {
        let logs = StepLogs::new().unwrap();
        let directory = logs.directory.clone();
        logs.run(
            "build",
            "/bin/sh",
            &["-c", "echo ok"],
            Duration::from_secs(5),
        )
        .await
        .unwrap();
        drop(logs);
        assert!(!directory.exists());
        let logs = StepLogs::new().unwrap();
        let directory = logs.directory.clone();
        let error = logs
            .run(
                "configure",
                "/bin/sh",
                &["-c", "echo 'error: copy refused' >&2; exit 1"],
                Duration::from_secs(5),
            )
            .await
            .unwrap_err();
        assert_eq!(error.code, "RUNNER_CONFIGURE_FAILED");
        drop(logs);
        let log = std::path::PathBuf::from(error.details.unwrap()["log"].as_str().unwrap());
        assert!(log.starts_with(&directory));
        assert!(
            std::fs::read_to_string(&log)
                .unwrap()
                .contains("copy refused")
        );
        std::fs::remove_dir_all(directory).unwrap();
    }
}
