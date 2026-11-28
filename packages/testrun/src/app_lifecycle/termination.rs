//! Physical app termination by installed executable identity, with observed absence.
use crate::{
    error::{Error, Result},
    process,
};
use serde_json::{Value, json};
use std::time::Duration;

pub(super) async fn perform(args: &Value, serial: &str, timeout: Duration) -> Result<Value> {
    let bundle = super::bundle(args)?;
    tokio::time::timeout(timeout, async {
        let executable = crate::applications::physical_executable(serial, bundle).await?;
        let processes = command(serial, &["info", "processes"], timeout).await?;
        let Some(pid) = matching_pid(&processes, &executable)? else {
            return Ok(json!({"terminated":true,"wasRunning":false,"backend":"coredevice"}));
        };
        // Resolve again before dispatch: an app replacement must not inherit the
        // process authority established for the earlier installed executable.
        if crate::applications::physical_executable(serial, bundle).await? != executable {
            return Err(Error::new(
                "STALE_APP",
                "Installed app changed before termination",
            ));
        }
        let current = command(serial, &["info", "processes"], timeout).await?;
        if matching_pid(&current, &executable)? != Some(pid) {
            return Err(Error::new(
                "STALE_APP",
                "App process changed before termination",
            ));
        }
        command(
            serial,
            &["process", "terminate", "--pid", &pid.to_string()],
            timeout,
        )
        .await?;
        super::poll(timeout, || async {
            let processes = command(serial, &["info", "processes"], timeout).await?;
            Ok((
                matching_pid(&processes, &executable)?.is_none(),
                json!({"backend":"coredevice"}),
            ))
        })
        .await?;
        Ok(json!({"terminated":true,"wasRunning":true,"backend":"coredevice"}))
    })
    .await
    .map_err(|_| {
        Error::new(
            "TIMEOUT",
            "Physical app termination did not complete within its deadline",
        )
    })?
}

async fn command(serial: &str, arguments: &[&str], timeout: Duration) -> Result<Value> {
    let directory =
        std::env::temp_dir().join(format!("testrun-terminate-{}", uuid::Uuid::new_v4()));
    crate::artifacts::directory(&directory)?;
    let output = directory.join("result.json");
    let result = async {
        let output = output
            .to_str()
            .ok_or_else(|| Error::invalid("Native result path is not UTF-8"))?;
        let mut args = vec!["devicectl", "device"];
        args.extend(arguments);
        args.extend(["--device", serial, "--json-output", output, "--quiet"]);
        process::run_diagnostic("/usr/bin/xcrun", &args, timeout, 64 * 1024).await?;
        let reply: Value = serde_json::from_slice(
            &std::fs::read(output)
                .map_err(|_| Error::new("PLATFORM_PROTOCOL", "CoreDevice omitted its result"))?,
        )
        .map_err(|_| Error::new("PLATFORM_PROTOCOL", "CoreDevice result is not JSON"))?;
        if reply["info"]["outcome"] != "success" {
            return Err(Error::new(
                "PLATFORM_FAILED",
                "CoreDevice did not report success",
            ));
        }
        Ok(reply)
    }
    .await;
    let _ = std::fs::remove_dir_all(directory);
    result
}

fn matching_pid(reply: &Value, path: &str) -> Result<Option<u64>> {
    let processes = reply["result"]["runningProcesses"]
        .as_array()
        .ok_or_else(|| {
            Error::new(
                "PLATFORM_PROTOCOL",
                "CoreDevice omitted the process inventory",
            )
        })?;
    if !processes
        .iter()
        .any(|process| process["processIdentifier"] == 1)
        || processes.iter().any(|process| {
            process["processIdentifier"]
                .as_u64()
                .is_none_or(|pid| pid == 0)
        })
    {
        return Err(Error::new(
            "PLATFORM_PROTOCOL",
            "CoreDevice process inventory is incomplete",
        ));
    }
    let mut found = None;
    // CoreDevice reports file URLs. Encode path bytes rather than matching a
    // basename, which could select another app's executable or extension.
    let mut expected = String::from("file://");
    for byte in path.bytes() {
        if byte.is_ascii_alphanumeric() || b"/-._~".contains(&byte) {
            expected.push(char::from(byte));
        } else {
            use std::fmt::Write;
            write!(expected, "%{byte:02X}").unwrap();
        }
    }
    for process in processes {
        let executable = process["executable"].as_str().ok_or_else(|| {
            Error::new(
                "PLATFORM_PROTOCOL",
                "Process inventory omitted executable identity",
            )
        })?;
        if executable != expected {
            continue;
        }
        let pid = process["processIdentifier"]
            .as_u64()
            .filter(|pid| *pid > 0)
            .ok_or_else(|| Error::new("PLATFORM_PROTOCOL", "App process has no valid identity"))?;
        if found.replace(pid).is_some() {
            return Err(Error::new(
                "AMBIGUOUS_APP_PROCESS",
                "Multiple processes match the installed app executable",
            ));
        }
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn process_selection_requires_exact_installed_path_and_unique_valid_pid() {
        let path = "/private/var/Acme App.app/Fixture";
        let mut reply = json!({"result":{"runningProcesses":[
            {"executable":"file:///sbin/launchd","processIdentifier":1},
            {"executable":"file:///private/var/Other.app/Fixture","processIdentifier":4},
            {"executable":"file:///private/var/Acme%20App.app/Fixture","processIdentifier":5}
        ]}});
        assert_eq!(matching_pid(&reply, path).unwrap(), Some(5));
        assert_eq!(matching_pid(&reply, "/different/Fixture").unwrap(), None);
        let duplicate = reply["result"]["runningProcesses"][2].clone();
        reply["result"]["runningProcesses"]
            .as_array_mut()
            .unwrap()
            .push(duplicate);
        assert_eq!(
            matching_pid(&reply, path).unwrap_err().code,
            "AMBIGUOUS_APP_PROCESS"
        );
        assert!(matching_pid(&json!({}), path).is_err());
        assert!(matching_pid(&json!({"result":{"runningProcesses":[]}}), path).is_err());
    }
}
