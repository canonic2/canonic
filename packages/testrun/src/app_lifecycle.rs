//! App launch, restart and observed readiness: stage records and readiness rules.
//!
//! Restart is terminate then launch. Each stage is recorded as it finishes, and
//! a failure reports the stages that completed; nothing here repeats a stage.
//! Readiness is an observed condition (the app's window in the foreground and,
//! optionally, a matching accessibility element), never process creation.
use crate::{
    error::{Error, Result},
    ui::Query,
};
use serde_json::{Map, Value, json};
use std::{future::Future, time::Duration};
use tokio::time::Instant;

mod android;
mod device;
mod simulator;
mod termination;
pub(crate) use android::perform as android;
pub(crate) use device::perform as device;
pub(crate) use simulator::perform as simulator;

const DEFAULT_READINESS: Duration = Duration::from_secs(10);
const POLL: Duration = Duration::from_millis(250);

/// A caller's request to observe readiness after launch.
pub(crate) struct Readiness {
    element: Option<Query>,
    timeout: Duration,
}

impl Readiness {
    /// Parses the optional `readiness` argument; absence means no observation.
    pub(crate) fn parse(args: &Value) -> Result<Option<Self>> {
        let Some(value) = args.get("readiness") else {
            return Ok(None);
        };
        let object = value
            .as_object()
            .ok_or_else(|| Error::invalid("readiness must be an object"))?;
        if let Some(key) = object
            .keys()
            .find(|key| !matches!(key.as_str(), "element" | "timeoutMs"))
        {
            return Err(Error::invalid(format!(
                "readiness.{key} is not supported; use element and timeoutMs"
            )));
        }
        let timeout = match object.get("timeoutMs") {
            None => DEFAULT_READINESS,
            Some(value) => Duration::from_millis(
                value
                    .as_u64()
                    .filter(|ms| (100..=30_000).contains(ms))
                    .ok_or_else(|| Error::invalid("readiness.timeoutMs must be 100..30000"))?,
            ),
        };
        let element = object
            .get("element")
            .map(|query| {
                let query: Query = serde_json::from_value(query.clone())
                    .map_err(|_| Error::invalid("Invalid readiness.element query"))?;
                query.validate()?;
                Ok::<_, Error>(query)
            })
            .transpose()?;
        Ok(Some(Self { element, timeout }))
    }
    fn condition(&self) -> &'static str {
        if self.element.is_some() {
            "foreground_element"
        } else {
            "foreground"
        }
    }
    /// Whether observations must include the accessibility tree.
    pub(crate) fn needs_tree(&self) -> bool {
        self.element.is_some()
    }
}

/// One readiness observation supplied by a platform adapter.
pub(crate) struct Observed {
    /// The launched app owns the foreground window.
    pub(crate) foreground: bool,
    /// Platform evidence for the foreground decision, such as the focused package.
    pub(crate) evidence: Value,
    /// The accessibility tree, present when [`Readiness::needs_tree`] asked for it.
    pub(crate) root: Option<Value>,
}

/// Polls `observe` until the condition holds or the readiness deadline passes.
/// Observation errors are retried within the deadline and reported if it expires.
pub(crate) async fn wait<F, Fut>(readiness: &Readiness, mut observe: F) -> Result<Value>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<Observed>>,
{
    let result = poll(readiness.timeout, || {
        let future = observe();
        async move {
            let observed = future.await?;
            let element = match (&readiness.element, &observed.root) {
                (None, _) => Some(Value::Null),
                (Some(query), Some(root)) if observed.foreground => {
                    query.find(root, 1)?["matches"].get(0).cloned()
                }
                _ => None,
            };
            let satisfied = observed.foreground && element.is_some();
            let mut evidence = json!({"observed":observed.evidence});
            if let Some(element) = element.filter(|element| !element.is_null()) {
                evidence["element"] = element;
            }
            Ok((satisfied, evidence))
        }
    })
    .await;
    match result {
        Ok(mut report) => {
            report["state"] = json!("ready");
            report["condition"] = json!(readiness.condition());
            let evidence = report["evidence"].take();
            report["observed"] = evidence["observed"].clone();
            if let Some(element) = evidence.get("element") {
                report["element"] = element.clone();
            }
            Ok(report)
        }
        Err(error) => {
            let mut report = error.details.unwrap_or_default();
            report["state"] = json!("not_ready");
            report["condition"] = json!(readiness.condition());
            report["lastObserved"] = report["lastObserved"]["observed"].clone();
            Err(Error::new(
                "APP_NOT_READY",
                "The app did not reach the requested readiness condition before its deadline",
            )
            .details(json!({"readiness":report})))
        }
    }
}

/// Shared bounded observation loop; it never owns or repeats a mutation.
pub(crate) async fn poll<F, Fut>(timeout: Duration, mut observe: F) -> Result<Value>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<(bool, Value)>>,
{
    let start = Instant::now();
    let deadline = start + timeout;
    let mut attempts = 0u32;
    let mut last = Value::Null;
    let mut last_error = None;
    loop {
        attempts += 1;
        match tokio::time::timeout_at(deadline, observe()).await {
            Ok(Ok((satisfied, evidence))) => {
                last_error = None;
                if satisfied {
                    return Ok(
                        json!({"state":"satisfied","elapsedMs":start.elapsed().as_millis(),"attempts":attempts,"evidence":evidence}),
                    );
                }
                last = evidence;
            }
            Ok(Err(error)) => last_error = Some(error),
            Err(_) => break,
        }
        if Instant::now() + POLL >= deadline {
            break;
        }
        tokio::time::sleep(POLL).await;
    }
    let mut report = json!({"state":"unsatisfied","elapsedMs":start.elapsed().as_millis(),"attempts":attempts,"lastObserved":last});
    if let Some(error) = last_error {
        report["lastError"] = json!(error);
    }
    Err(Error::new(
        "TIMEOUT",
        "The requested condition was not established before its deadline",
    )
    .details(report))
}

/// Ordered stage outcomes of one launch or restart command.
pub(crate) struct Stages {
    operation: &'static str,
    stages: Map<String, Value>,
}

impl Stages {
    /// Restart records terminate, launch and readiness; launch omits terminate.
    pub(crate) fn new(restart: bool, readiness: bool) -> Self {
        let mut stages = Map::new();
        if restart {
            stages.insert("terminate".into(), json!({"state":"not_started"}));
        }
        stages.insert("launch".into(), json!({"state":"not_started"}));
        stages.insert(
            "readiness".into(),
            json!({"state":if readiness {"not_started"} else {"not_requested"}}),
        );
        Self {
            operation: if restart { "restart" } else { "launch" },
            stages,
        }
    }
    /// Records a finished stage. A failure keeps the earlier stages, marks this
    /// one failed and returns the cause's code with every stage in its details.
    pub(crate) fn record(&mut self, stage: &str, result: Result<Value>) -> Result<()> {
        match result {
            Ok(value) => {
                let mut entry = json!({"state":"completed"});
                if let Some(object) = value.as_object() {
                    for (key, value) in object {
                        entry[key] = value.clone();
                    }
                }
                if stage == "readiness" {
                    entry["state"] = json!("ready");
                }
                self.stages.insert(stage.into(), entry);
                Ok(())
            }
            Err(error) => {
                let entry = if stage == "readiness" {
                    error
                        .details
                        .as_ref()
                        .map_or_else(|| json!({"state":"not_ready"}), |d| d["readiness"].clone())
                } else {
                    json!({"state":"failed","error":{"code":error.code,"message":error.message}})
                };
                self.stages.insert(stage.into(), entry);
                let completed: Vec<_> = self
                    .stages
                    .iter()
                    .filter(|(_, value)| value["state"] == "completed")
                    .map(|(name, _)| name.as_str())
                    .collect();
                let mut details = json!({"stages":self.stages,"replayed":false});
                if stage != "readiness"
                    && let Some(cause) = &error.details
                {
                    details["cause"] = cause.clone();
                }
                Err(Error {
                    message: format!(
                        "App {} stopped at its {stage} stage (completed: {}); no stage was repeated. {}",
                        self.operation,
                        if completed.is_empty() {
                            "none".into()
                        } else {
                            completed.join(", ")
                        },
                        error.message
                    ),
                    details: Some(details),
                    ..error
                })
            }
        }
    }
    pub(crate) fn finish(self) -> Value {
        Value::Object(self.stages)
    }
}

/// Checks launch and restart arguments before any receipt or platform command.
pub(crate) fn validate(args: &Value) -> Result<()> {
    bundle(args)?;
    launch_arguments(args)?;
    launch_environment(args)?;
    Readiness::parse(args)?;
    Ok(())
}

/// Per-process launch values, never applied to device-wide settings.
pub(crate) fn launch_environment(
    args: &Value,
) -> Result<std::collections::BTreeMap<String, String>> {
    let values: std::collections::BTreeMap<String, String> =
        serde_json::from_value(args.get("env").cloned().unwrap_or(json!({})))
            .map_err(|_| Error::invalid("env must map names to strings"))?;
    if values.len() > 128 {
        return Err(Error::invalid("At most 128 launch environment entries"));
    }
    for (key, value) in &values {
        if !crate::configuration::variable(key) {
            return Err(Error::invalid("Invalid launch environment name"));
        }
        crate::configuration::literal(value)?;
    }
    Ok(values)
}

/// Launch arguments are data passed to the app, at most 64 of 4096 bytes each.
fn launch_arguments(args: &Value) -> Result<Vec<String>> {
    let values: Vec<String> = match args.get("args") {
        Some(values) => serde_json::from_value(values.clone())
            .map_err(|_| Error::invalid("args must be an array of strings"))?,
        None => Vec::new(),
    };
    if values.len() > 64
        || values
            .iter()
            .any(|value| value.len() > 4096 || value.contains('\0'))
    {
        return Err(Error::invalid("Launch argument limit exceeded"));
    }
    Ok(values)
}

/// Validates a reverse-DNS app identifier shared by simulator commands.
pub(crate) fn bundle(args: &Value) -> Result<&str> {
    let bundle = args
        .get("appId")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty() && value.len() <= 256)
        .ok_or_else(|| Error::invalid("Required appId is missing"))?;
    if !bundle
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
    {
        return Err(Error::invalid("Invalid appId"));
    }
    Ok(bundle)
}

#[cfg(test)]
mod tests;
