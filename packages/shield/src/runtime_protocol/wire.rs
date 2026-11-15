//! Closed general envelopes. Payload claims never select registration authority.
use crate::policy::resolution::normalized;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[cfg(test)]
mod tests;

pub(super) const MAX_INPUT: usize = 512 * 1024;

pub(super) struct Envelope {
    pub correlation_id: String,
    pub name: String,
    pub operation: Operation,
}

pub(super) enum Operation {
    Evaluate(Evaluate),
    Consume(Consume),
    Observe(Observe),
    Unsupported,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Provenance {
    pub session_id: Option<String>,
    pub tool_use_id: Option<String>,
    pub event: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Evaluate {
    pub input: normalized::Input,
    pub context: normalized::Context,
    pub provenance: Provenance,
    pub retry_ref: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Consume {
    pub request_id: String,
    pub input: normalized::Input,
    pub context: normalized::Context,
    pub provenance: Provenance,
    pub retry_ref: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Observe {
    pub delivery_id: String,
    pub request_id: Option<String>,
    pub observation: Observation,
    pub provenance: Provenance,
    // Read-only mapping extension: exact operation evidence for deferred joins.
    pub input: normalized::Input,
    pub context: normalized::Context,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Observation {
    pub kind: String,
    pub occurred_at: Option<String>,
    pub run_id: Option<String>,
    pub exit_code: Option<i32>,
    pub signal: Option<String>,
    pub host_approval: Option<HostApproval>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct HostApproval {
    pub mechanism: String,
    pub operator_id: Option<String>,
    pub display_name: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Error {
    InvalidInput,
    UnsupportedVersion,
    UnsupportedOperation,
}

fn exact(value: &Value, keys: &[&str]) -> bool {
    value.as_object().is_some_and(|fields| {
        fields.len() == keys.len() && keys.iter().all(|key| fields.contains_key(*key))
    })
}

fn bounded(value: &Value, depth: usize) -> bool {
    if depth > 64 {
        return false;
    }
    match value {
        Value::Array(values) => values.iter().all(|value| bounded(value, depth + 1)),
        Value::Object(fields) => fields.values().all(|value| bounded(value, depth + 1)),
        _ => true,
    }
}

pub(crate) fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._:-".contains(&byte))
}

fn nullable_identifier(value: &Value) -> bool {
    value.is_null() || value.as_str().is_some_and(identifier)
}

fn evidence(payload: &Value) -> bool {
    exact(&payload["context"], &["cwd", "environment"])
        && payload["context"]["environment"].is_null()
        && exact(
            &payload["provenance"],
            &["session_id", "tool_use_id", "event"],
        )
        && ["session_id", "tool_use_id"]
            .iter()
            .all(|key| nullable_identifier(&payload["provenance"][key]))
        && payload["provenance"]["event"]
            .as_str()
            .is_some_and(identifier)
}

impl Envelope {
    pub(super) fn parse(bytes: &[u8]) -> Result<Self, Error> {
        if bytes.len() > MAX_INPUT {
            return Err(Error::InvalidInput);
        }
        let value = crate::json::parse(bytes).map_err(|_| Error::InvalidInput)?;
        if !exact(
            &value,
            &["protocol_version", "correlation_id", "operation", "payload"],
        ) || crate::json::contains_nul(&value)
            || !bounded(&value, 0)
        {
            return Err(Error::InvalidInput);
        }
        let version = value["protocol_version"]
            .as_u64()
            .ok_or(Error::InvalidInput)?;
        if version != 1 {
            return Err(Error::UnsupportedVersion);
        }
        let correlation_id = value["correlation_id"]
            .as_str()
            .filter(|id| identifier(id))
            .ok_or(Error::InvalidInput)?
            .to_owned();
        let name = value["operation"].as_str().ok_or(Error::InvalidInput)?;
        let payload = &value["payload"];
        let operation = match name {
            "runtime.evaluate"
                if exact(payload, &["input", "context", "provenance", "retry_ref"])
                    && evidence(payload)
                    && nullable_identifier(&payload["retry_ref"]) =>
            {
                Operation::Evaluate(
                    serde_json::from_value(payload.clone()).map_err(|_| Error::InvalidInput)?,
                )
            }
            "runtime.consume"
                if exact(
                    payload,
                    &["request_id", "input", "context", "provenance", "retry_ref"],
                ) && evidence(payload)
                    && payload["request_id"].as_str().is_some_and(identifier)
                    && payload["retry_ref"].as_str().is_some_and(identifier) =>
            {
                Operation::Consume(
                    serde_json::from_value(payload.clone()).map_err(|_| Error::InvalidInput)?,
                )
            }
            "history.observe" if valid_observation(payload) => Operation::Observe(
                serde_json::from_value(payload.clone()).map_err(|_| Error::InvalidInput)?,
            ),
            "diagnostic.evaluate" | "diagnostic.compare" => Operation::Unsupported,
            "runtime.evaluate" | "runtime.consume" | "history.observe" => {
                return Err(Error::InvalidInput);
            }
            _ => return Err(Error::UnsupportedOperation),
        };
        Ok(Self {
            correlation_id,
            name: name.into(),
            operation,
        })
    }
}

fn valid_observation(payload: &Value) -> bool {
    let observation = &payload["observation"];
    exact(
        payload,
        &[
            "delivery_id",
            "request_id",
            "observation",
            "provenance",
            "input",
            "context",
        ],
    ) && evidence(payload)
        && payload["delivery_id"].as_str().is_some_and(identifier)
        && nullable_identifier(&payload["request_id"])
        && exact(
            observation,
            &[
                "kind",
                "occurred_at",
                "run_id",
                "exit_code",
                "signal",
                "host_approval",
            ],
        )
        && observation["kind"]
            .as_str()
            .is_some_and(|kind| ["launched", "completed", "failed", "cancelled"].contains(&kind))
        && nullable_identifier(&observation["run_id"])
        && nullable_identifier(&observation["signal"])
        && (observation["occurred_at"].is_null()
            || observation["occurred_at"].as_str().is_some_and(|time| {
                time.len() <= 64
                    && time.ends_with('Z')
                    && canonic_registry_kit::timestamp::parse(time).is_some()
            }))
        && (observation["host_approval"].is_null() || {
            let approval = &observation["host_approval"];
            exact(approval, &["mechanism", "operator_id", "display_name"])
                && approval["mechanism"].as_str().is_some_and(identifier)
                && ["operator_id", "display_name"].iter().all(|key| {
                    approval[key].is_null()
                        || approval[key]
                            .as_str()
                            .is_some_and(|text| !text.is_empty() && text.len() <= 128)
                })
        })
}
