//! Simulator launch and restart through simctl, with frontmost-process readiness.
use super::{Observed, Readiness, Stages, bundle, launch_arguments};
use crate::{
    error::{Error, Result},
    process,
    simulator::device_set::{DeviceSet, executable},
};
use serde_json::{Value, json};
use std::time::Duration;

/// Runs `app_launch` or `app_restart` on one simulator UUID.
pub(crate) async fn perform(
    name: &str,
    args: &Value,
    uuid: &str,
    timeout: Duration,
) -> Result<Value> {
    let bundle = bundle(args)?;
    let arguments = launch_arguments(args)?;
    let readiness = Readiness::parse(args)?;
    let restart = name == "app_restart";
    let mut stages = Stages::new(restart, readiness.is_some());
    if restart {
        stages.record("terminate", terminate(uuid, bundle, timeout).await)?;
    }
    let launched = launch(
        uuid,
        bundle,
        &arguments,
        &super::launch_environment(args)?,
        timeout,
    )
    .await;
    let pid = launched
        .as_ref()
        .ok()
        .and_then(|value| value["pid"].as_i64());
    stages.record("launch", launched)?;
    if let Some(readiness) = readiness {
        let observed = super::wait(&readiness, || observe(uuid, pid)).await;
        stages.record("readiness", observed)?;
    }
    Ok(stages.finish())
}

/// Terminates a running app; an app that is not running already satisfies
/// restart's first stage and is reported with `wasRunning: false`.
async fn terminate(uuid: &str, bundle: &str, timeout: Duration) -> Result<Value> {
    let args = DeviceSet::current()?.arguments(&["terminate", uuid, bundle]);
    let values: Vec<_> = args.iter().map(String::as_str).collect();
    match process::run_diagnostic(executable(), &values, timeout, 64 * 1024).await {
        Ok(_) => Ok(json!({"wasRunning":true})),
        Err(error) if not_running(&error) => Ok(json!({"wasRunning":false})),
        Err(error) => Err(error),
    }
}

fn not_running(error: &Error) -> bool {
    error.code == "PLATFORM_FAILED"
        && error
            .details
            .as_ref()
            .and_then(|details| details["diagnostic"].as_str())
            .is_some_and(|text| text.contains("found nothing to terminate"))
}

async fn launch(
    uuid: &str,
    bundle: &str,
    arguments: &[String],
    environment: &std::collections::BTreeMap<String, String>,
    timeout: Duration,
) -> Result<Value> {
    let mut args = DeviceSet::current()?.arguments(&["launch", uuid, bundle]);
    args.extend(arguments.iter().cloned());
    let values: Vec<_> = args.iter().map(String::as_str).collect();
    let mut env: std::collections::BTreeMap<std::ffi::OsString, std::ffi::OsString> =
        std::env::vars_os()
            .filter(|(key, _)| !key.as_encoded_bytes().starts_with(b"SIMCTL_CHILD_"))
            .collect();
    env.extend(
        environment
            .iter()
            .map(|(key, value)| (format!("SIMCTL_CHILD_{key}").into(), value.into())),
    );
    let output = process::run_environment(executable(), &values, &env, timeout, 64 * 1024).await?;
    let mut result = json!({});
    if let Some(pid) = launched_pid(&String::from_utf8_lossy(&output.bytes), bundle) {
        result["pid"] = json!(pid);
    }
    Ok(result)
}

/// Parses simctl's `bundle: pid` launch reply.
fn launched_pid(text: &str, bundle: &str) -> Option<i64> {
    text.lines()
        .find_map(|line| line.trim().strip_prefix(bundle)?.strip_prefix(':'))
        .and_then(|pid| pid.trim().parse().ok())
        .filter(|pid| *pid > 0)
}

/// The frontmost app is the launched process and exposes at least one element.
async fn observe(uuid: &str, pid: Option<i64>) -> Result<Observed> {
    let mut snapshot = crate::native::snapshot(uuid).await?;
    let frontmost = snapshot["pid"].as_i64();
    let rendered = snapshot["root"]["children"]
        .as_array()
        .is_some_and(|children| !children.is_empty());
    let foreground = pid.is_some() && frontmost == pid && rendered;
    Ok(Observed {
        foreground,
        // Managed results hide process IDs; report only whether they match.
        evidence: json!({"frontmostIsLaunched":pid.is_some() && frontmost == pid,"rendered":rendered}),
        root: Some(snapshot["root"].take()),
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn launch_reply_pid_belongs_to_the_requested_bundle() {
        assert_eq!(
            super::launched_pid("com.example.acme: 4242\n", "com.example.acme"),
            Some(4242)
        );
        assert_eq!(
            super::launched_pid("com.example.acme.beta: 4242\n", "com.example.acme"),
            None
        );
        assert_eq!(super::launched_pid("", "com.example.acme"), None);
    }
}
