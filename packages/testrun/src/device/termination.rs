//! Read-only native exit verification; a vanished host alone cannot release a runner.
use crate::{
    artifacts,
    error::{Error, Result},
    process,
};
use serde_json::Value;
use std::time::Duration;

pub(super) async fn verify_absent(serial: &str, pid: Option<u32>) -> Result<()> {
    let pid = pid.ok_or_else(|| {
        Error::new(
            "CLEANUP_FAILED",
            "No authenticated native runner identity is available to verify exit",
        )
    })?;
    let directory =
        std::env::temp_dir().join(format!("testrun-runner-exit-{}", uuid::Uuid::new_v4()));
    artifacts::directory(&directory)?;
    let path = directory.join("processes.json");
    let result = async {
        let output = path
            .to_str()
            .ok_or_else(|| Error::invalid("Invalid private process inventory path"))?;
        process::run_diagnostic(
            "/usr/bin/xcrun",
            &[
                "devicectl",
                "device",
                "info",
                "processes",
                "--device",
                serial,
                "--json-output",
                output,
                "--quiet",
                "--timeout",
                "10",
            ],
            Duration::from_secs(12),
            64 * 1024,
        )
        .await?;
        let bytes = std::fs::read(&path).map_err(|_| {
            Error::new(
                "PLATFORM_PROTOCOL",
                "No device process inventory was written",
            )
        })?;
        absent(&bytes, pid)
    }
    .await;
    let _ = std::fs::remove_dir_all(directory);
    result
}

fn absent(bytes: &[u8], pid: u32) -> Result<()> {
    let reply: Value = serde_json::from_slice(bytes)
        .map_err(|_| Error::new("PLATFORM_PROTOCOL", "Invalid device process inventory"))?;
    let processes = reply["result"]["runningProcesses"]
        .as_array()
        .filter(|_| reply["info"]["outcome"] == "success")
        .ok_or_else(|| {
            Error::new(
                "PLATFORM_PROTOCOL",
                "Device process inventory did not establish success",
            )
        })?;
    // Empty or malformed inventories do not prove release. A healthy iPhone
    // inventory includes launchd and positive process identifiers.
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
            "Device process inventory is incomplete",
        ));
    }
    if processes
        .iter()
        .any(|process| process["processIdentifier"] == pid)
    {
        return Err(Error::new(
            "RUNNER_BUSY",
            "The recorded native runner process is still present; cleanup remains unfinished",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn only_a_successful_complete_inventory_can_prove_native_exit() {
        for processes in [
            json!([]),
            json!([{"processIdentifier":0}]),
            json!([{"processIdentifier":1},{}]),
        ] {
            let reply =
                json!({"info":{"outcome":"success"},"result":{"runningProcesses":processes}});
            assert_eq!(
                absent(&serde_json::to_vec(&reply).unwrap(), 42)
                    .unwrap_err()
                    .code,
                "PLATFORM_PROTOCOL"
            );
        }
        let mut reply = json!({"info":{"outcome":"success"},"result":{"runningProcesses":[{"processIdentifier":1},{"processIdentifier":42}]}});
        assert_eq!(
            absent(&serde_json::to_vec(&reply).unwrap(), 42)
                .unwrap_err()
                .code,
            "RUNNER_BUSY"
        );
        absent(&serde_json::to_vec(&reply).unwrap(), 43).unwrap();
        reply["info"]["outcome"] = json!("failed");
        assert_eq!(
            absent(&serde_json::to_vec(&reply).unwrap(), 43)
                .unwrap_err()
                .code,
            "PLATFORM_PROTOCOL"
        );
        assert_eq!(
            absent(b"not json", 42).unwrap_err().code,
            "PLATFORM_PROTOCOL"
        );
    }
}
