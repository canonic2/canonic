//! Parent-side commands that bound and isolate Apple worker processes.
use crate::{
    error::{Error, Result},
    process,
};
use std::time::Duration;

async fn worker(args: &[&str], deadline: Duration, limit: usize) -> Result<process::Output> {
    let executable = std::env::current_exe()
        .map_err(|_| Error::new("INTERNAL", "Cannot resolve native worker"))?;
    // Inherit every variable unchanged, including non-UTF-8 ones; only the set is replaced.
    let mut environment: std::collections::BTreeMap<_, _> = std::env::vars_os().collect();
    crate::simulator::device_set::DeviceSet::current()?.apply_environment(&mut environment);
    process::run_environment(executable, args, &environment, deadline, limit).await
}

pub async fn developer_directory() -> Result<String> {
    let output = process::run(
        "/usr/bin/xcode-select",
        &["-p"],
        Duration::from_secs(5),
        4096,
    )
    .await?;
    Ok(std::str::from_utf8(&output.bytes)
        .map_err(|_| Error::new("PLATFORM_PROTOCOL", "Invalid developer directory"))?
        .trim()
        .to_owned())
}
pub async fn snapshot(uuid: &str) -> Result<serde_json::Value> {
    let set = crate::simulator::device_set::DeviceSet::current()?;
    let developer = developer_directory().await?;
    let output = worker(
        &[
            "native-accessibility",
            "--developer",
            &developer,
            "--id",
            uuid,
        ],
        Duration::from_secs(5),
        2 * 1024 * 1024,
    )
    .await?;
    if crate::simulator::device_set::DeviceSet::current()?.identity != set.identity {
        return Err(Error::new(
            "STALE_TARGET",
            "Simulator device set changed during accessibility observation",
        ));
    }
    serde_json::from_slice(&output.bytes)
        .map_err(|_| Error::new("PLATFORM_PROTOCOL", "Invalid native accessibility response"))
}

pub(crate) async fn incarnation(uuid: &str) -> Result<crate::simulator::incarnation::Incarnation> {
    let developer = developer_directory().await?;
    let output = worker(
        &["native-identity", "--developer", &developer, "--id", uuid],
        Duration::from_secs(5),
        65536,
    )
    .await?;
    let reply = crate::simulator::ipc::read(&mut output.bytes.as_slice()).await?;
    if reply.metadata["event"] == "error" {
        return Err(crate::simulator::ipc::failure(&reply.metadata["error"]));
    }
    if reply.metadata["event"] != "identity" || !reply.payload.is_empty() {
        return Err(Error::new(
            "PLATFORM_PROTOCOL",
            "Invalid simulator identity response",
        ));
    }
    serde_json::from_value(reply.metadata["identity"].clone())
        .map_err(|_| Error::new("PLATFORM_PROTOCOL", "Invalid simulator identity witness"))
}
