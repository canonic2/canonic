//! Physical iOS launch and restart through CoreDevice's `devicectl`, without XCTest.
//!
//! Restart is one `--terminate-existing` launch, so its terminate and launch
//! stages complete or fail together. Readiness is not observable here.
use super::{Stages, bundle, launch_arguments};
use crate::{
    error::{Error, Result},
    process,
};
use serde_json::{Value, json};
use std::time::Duration;

/// Runs `app_launch` or `app_restart` on one physical device UDID.
pub(crate) async fn perform(
    name: &str,
    args: &Value,
    udid: &str,
    timeout: Duration,
) -> Result<Value> {
    if name == "app_terminate" {
        return super::termination::perform(args, udid, timeout).await;
    }
    let bundle = bundle(args)?;
    let arguments = launch_arguments(args)?;
    let restart = name == "app_restart";
    let mut stages = Stages::new(restart, false);
    let directory = std::env::temp_dir().join(format!("testrun-launch-{}", uuid::Uuid::new_v4()));
    crate::artifacts::directory(&directory)?;
    let output = directory.join("launch.json");
    let launched = async {
        let output_path = output
            .to_str()
            .ok_or_else(|| Error::invalid("Private launch path is not UTF-8"))?;
        let environment=super::launch_environment(args)?;
        let command = command(udid, bundle, &arguments, &environment, restart, output_path);
        let values: Vec<&str> = command.iter().map(String::as_str).collect();
        let host = host_environment(std::env::vars_os());
        process::run_diagnostic_environment("/usr/bin/xcrun", &values, &host, timeout, 64 * 1024).await.map_err(|error|if environment.is_empty(){error}else{Error::new(&error.code,"iOS app launch failed with private environment values; inspect app/device readiness")})?;
        let bytes = std::fs::read(&output)
            .map_err(|_| Error::new("PLATFORM_PROTOCOL", "devicectl wrote no launch result"))?;
        launched(&bytes)
    }
    .await;
    let _ = std::fs::remove_dir_all(&directory);
    if restart {
        let terminate = match &launched {
            Ok(_) => Ok(json!({"method":"terminate_existing"})),
            Err(error) => Err(error.clone()),
        };
        stages.record("terminate", terminate)?;
    }
    stages.record("launch", launched)?;
    Ok(stages.finish())
}

/// App bindings come only from the request, even when devicectl's JSON flag is absent.
fn host_environment(
    inherited: impl IntoIterator<Item = (std::ffi::OsString, std::ffi::OsString)>,
) -> std::collections::BTreeMap<std::ffi::OsString, std::ffi::OsString> {
    inherited
        .into_iter()
        .filter(|(key, _)| !key.as_encoded_bytes().starts_with(b"DEVICECTL_CHILD_"))
        .collect()
}

/// devicectl parses arguments after the bundle ID as its own options unless a
/// `--` separates them, and then passes that separator on to the app.
fn command(
    udid: &str,
    bundle: &str,
    arguments: &[String],
    environment: &std::collections::BTreeMap<String, String>,
    restart: bool,
    output: &str,
) -> Vec<String> {
    let mut command: Vec<String> = ["devicectl", "device", "process", "launch", "--device", udid]
        .map(String::from)
        .to_vec();
    if restart {
        command.push("--terminate-existing".into());
    }
    // devicectl rejects an empty JSON dictionary with a decoding error.
    if !environment.is_empty() {
        command.extend([
            "--environment-variables".into(),
            serde_json::to_string(environment).unwrap(),
        ]);
    }
    command.extend(["--json-output".into(), output.into(), bundle.into()]);
    if !arguments.is_empty() {
        command.push("--".into());
        command.extend(arguments.iter().cloned());
    }
    command
}

/// Reads devicectl's JSON reply; managed results hide the process ID.
fn launched(bytes: &[u8]) -> Result<Value> {
    let reply: Value = serde_json::from_slice(bytes)
        .map_err(|_| Error::new("PLATFORM_PROTOCOL", "devicectl launch result is not JSON"))?;
    if reply["info"]["outcome"] != "success" {
        return Err(Error::new(
            "PLATFORM_FAILED",
            "devicectl did not report a successful launch",
        ));
    }
    let pid = reply["result"]["process"]["processIdentifier"]
        .as_i64()
        .filter(|pid| *pid > 0)
        .ok_or_else(|| {
            Error::new(
                "PLATFORM_PROTOCOL",
                "devicectl did not report the launched process",
            )
        })?;
    Ok(json!({"pid":pid,"backend":"coredevice"}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[tokio::test]
    async fn launch_and_restart_subprocesses_exclude_inherited_app_bindings() {
        let host = host_environment([
            ("DEVICECTL_CHILD_ACME_LEAK".into(), "foreign-project".into()),
            ("ACME_HOST_VALUE".into(), "retained".into()),
            ("X_DEVICECTL_CHILD_ACME".into(), "lookalike-retained".into()),
        ]);
        for restart in [false, true] {
            for environment in [
                Default::default(),
                std::collections::BTreeMap::from([("ACME_EXPLICIT".into(), "requested".into())]),
            ] {
                let command = command(
                    "UDID",
                    "com.example.acme",
                    &[],
                    &environment,
                    restart,
                    "/tmp/out.json",
                );
                // A real child prints its environment and the devicectl arguments.
                // All inherited values are synthetic; no process-global mutation.
                let mut values = vec!["-c", "/usr/bin/env; printf '%s\\n' \"$@\"", "fixture"];
                values.extend(command.iter().map(String::as_str));
                let output = process::run_diagnostic_environment(
                    "/bin/sh",
                    &values,
                    &host,
                    Duration::from_secs(3),
                    65536,
                )
                .await
                .unwrap();
                let output = String::from_utf8(output.bytes).unwrap();
                let lines: Vec<_> = output.lines().collect();
                assert!(
                    !lines
                        .iter()
                        .any(|line| line.starts_with("DEVICECTL_CHILD_"))
                );
                assert!(lines.contains(&"ACME_HOST_VALUE=retained"));
                assert!(lines.contains(&"X_DEVICECTL_CHILD_ACME=lookalike-retained"));
                assert_eq!(lines.contains(&"--terminate-existing"), restart);
                assert_eq!(
                    lines.contains(&"--environment-variables"),
                    !environment.is_empty()
                );
                if !environment.is_empty() {
                    assert!(lines.contains(&serde_json::to_string(&environment).unwrap().as_str()));
                }
            }
        }
        let error = process::run_diagnostic_environment(
            "/bin/sh",
            &["-c", "printf 'error: fixture failure' >&2; exit 7"],
            &host,
            Duration::from_secs(3),
            65536,
        )
        .await
        .err()
        .unwrap();
        assert_eq!(error.code, "PLATFORM_FAILED");
        assert_eq!(
            error.details.unwrap()["diagnostic"],
            "error: fixture failure"
        );
    }

    #[test]
    fn launch_arguments_follow_a_separator_so_devicectl_cannot_read_them_as_options() {
        let arguments = vec!["--terminate-existing".to_owned(), "-AcmeFlag".to_owned()];
        let launch = command(
            "UDID",
            "com.example.acme",
            &arguments,
            &Default::default(),
            false,
            "/tmp/out.json",
        );
        let bundle = launch.iter().position(|v| v == "com.example.acme").unwrap();
        assert_eq!(launch[bundle + 1], "--");
        assert_eq!(&launch[bundle + 2..], arguments.as_slice());
        assert!(!launch[..bundle].contains(&"--terminate-existing".to_owned()));
        let plain = command(
            "UDID",
            "com.example.acme",
            &[],
            &Default::default(),
            false,
            "/tmp/out.json",
        );
        assert_eq!(plain.last().unwrap(), "com.example.acme");
        assert!(!plain.iter().any(|value| value == "--environment-variables"));
    }

    #[test]
    fn restart_terminates_existing_instances_in_the_same_launch() {
        let restart = command(
            "UDID",
            "com.example.acme",
            &[],
            &Default::default(),
            true,
            "/tmp/out.json",
        );
        let flag = restart.iter().position(|v| v == "--terminate-existing");
        let bundle = restart.iter().position(|v| v == "com.example.acme");
        assert!(flag.unwrap() < bundle.unwrap());
        assert!(
            !restart
                .iter()
                .any(|value| value == "--environment-variables")
        );
    }

    #[test]
    fn nonempty_environment_is_a_string_dictionary_before_the_bundle() {
        let environment =
            super::super::launch_environment(&json!({"env":{"ACME_VALUE":"quoted \"value\"\n雪"}}))
                .unwrap();
        for restart in [false, true] {
            let launch = command(
                "UDID",
                "com.example.acme",
                &[],
                &environment,
                restart,
                "/tmp/out.json",
            );
            let flag = launch
                .iter()
                .position(|value| value == "--environment-variables")
                .unwrap();
            let decoded: std::collections::BTreeMap<String, String> =
                serde_json::from_str(&launch[flag + 1]).unwrap();
            assert_eq!(decoded, environment);
            assert!(
                flag + 1
                    < launch
                        .iter()
                        .position(|value| value == "com.example.acme")
                        .unwrap()
            );
        }
    }

    #[test]
    fn launch_result_requires_success_and_a_process() {
        let ok =
            br#"{"info":{"outcome":"success"},"result":{"process":{"processIdentifier":1509}}}"#;
        assert_eq!(launched(ok).unwrap()["pid"], 1509);
        let failed = br#"{"info":{"outcome":"failed"},"result":{}}"#;
        assert_eq!(launched(failed).err().unwrap().code, "PLATFORM_FAILED");
        let missing = br#"{"info":{"outcome":"success"},"result":{}}"#;
        assert_eq!(launched(missing).err().unwrap().code, "PLATFORM_PROTOCOL");
        assert_eq!(
            launched(b"not json").err().unwrap().code,
            "PLATFORM_PROTOCOL"
        );
    }
}
