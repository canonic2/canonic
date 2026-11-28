//! Android launch and restart through adb, with focused-window readiness.
use super::{Observed, Readiness, Stages};
use crate::{
    android::Android,
    error::{Error, Result},
};
use serde_json::{Value, json};
use std::time::Duration;

/// Runs `app_launch` or `app_restart` for one adb serial.
pub(crate) async fn perform(
    name: &str,
    args: &Value,
    serial: &str,
    timeout: Duration,
) -> Result<Value> {
    let package = args
        .get("appId")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::invalid("Required appId is missing"))?;
    let readiness = Readiness::parse(args)?;
    let restart = name == "app_restart";
    let adapter = Android::default();
    let mut stages = Stages::new(restart, readiness.is_some());
    if restart {
        stages.record(
            "terminate",
            adapter.terminate(serial, package, timeout).await,
        )?;
    }
    stages.record("launch", adapter.launch(serial, package, timeout).await)?;
    if let Some(readiness) = readiness {
        let observed = super::wait(&readiness, || {
            observe(&adapter, serial, package, &readiness)
        })
        .await;
        stages.record("readiness", observed)?;
    }
    Ok(stages.finish())
}

async fn observe(
    adapter: &Android,
    serial: &str,
    package: &str,
    readiness: &Readiness,
) -> Result<Observed> {
    let focused = adapter.focused_package(serial).await?;
    let foreground = focused.as_deref() == Some(package);
    let root = if foreground && readiness.needs_tree() {
        let mut snapshot = adapter.snapshot(serial).await?;
        Some(snapshot["root"].take())
    } else {
        None
    };
    Ok(Observed {
        foreground,
        evidence: json!({"focusedPackage":focused}),
        root,
    })
}
