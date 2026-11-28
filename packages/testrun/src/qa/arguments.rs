//! Pure QA request models and argument validation, independent of transport and platform I/O.
use crate::{
    error::{Error, Result},
    targets::TargetRef,
};
use serde::Deserialize;
use serde_json::Value;
use std::time::Duration;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct BeginRun {
    #[serde(default)]
    pub(super) goal_ids: Vec<String>,
    pub(super) label: Option<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct BeginGoal {
    pub(super) run_id: String,
    pub(super) goal_id: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct OpenSession {
    pub(super) android_capture: Option<Value>,
    pub(super) runner_team_id: Option<String>,
    pub(super) control_path: Option<String>,
    pub(super) run_id: String,
    pub(super) goal_id: String,
    pub(super) target: TargetRef,
    pub(super) mode: String,
    #[serde(default)]
    pub(super) require_capabilities: Vec<String>,
    #[serde(default)]
    pub(super) boot: bool,
    /// Show the window of an emulator this session starts; headless otherwise.
    #[serde(default)]
    pub(super) window: bool,
    #[serde(default)]
    pub(super) setup: bool,
    pub(super) timeout_ms: Option<u64>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct SessionRef {
    pub(super) run_id: String,
    pub(super) goal_id: String,
    pub(super) session_id: String,
}

pub(super) fn parse<T: serde::de::DeserializeOwned>(value: Value) -> Result<T> {
    serde_json::from_value(value)
        .map_err(|_| Error::invalid("Arguments do not match the tool schema"))
}
pub(super) fn field<'a>(args: &'a Value, name: &str) -> Result<&'a str> {
    args.get(name)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| Error::invalid(format!("Missing or invalid {name}")))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Empty {}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct EndRun {
    pub(super) run_id: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ListTargets {
    #[serde(default)]
    pub(super) probe: bool,
    pub(super) kind: Option<String>,
    pub(super) state: Option<String>,
    pub(super) cursor: Option<String>,
    pub(super) limit: Option<usize>,
}
pub(super) fn deadline(value: Option<u64>) -> Result<Duration> {
    let ms = value.unwrap_or(30_000);
    if !(1..=900_000).contains(&ms) {
        return Err(Error::invalid("timeoutMs must be 1..900000"));
    }
    Ok(Duration::from_millis(ms))
}
pub(super) fn session_ref(args: &Value) -> Result<SessionRef> {
    Ok(SessionRef {
        run_id: field(args, "runId")?.into(),
        goal_id: field(args, "goalId")?.into(),
        session_id: field(args, "sessionId")?.into(),
    })
}
pub(super) fn only_fields(args: &Value, allowed: &[&str]) -> Result<()> {
    let object = args
        .as_object()
        .ok_or_else(|| Error::invalid("Arguments must be an object"))?;
    if object.keys().any(|key| !allowed.contains(&key.as_str())) {
        return Err(Error::invalid("Unknown argument field"));
    }
    Ok(())
}
