//! Noninteractive inspection of text-read and native execution approval requests.
use super::{Context, Error};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// The policy decision a request was captured under, for operator review only:
/// the decision, the resolver's reason code and up to three decisive rules.
/// It holds no request operands and is not part of any grant binding.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PolicyBasis {
    decision: String,
    reason: String,
    rules: Vec<BasisRule>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BasisRule {
    source: String,
    rule_id: String,
    reason: String,
}

fn bounded(value: &Value, limit: usize) -> String {
    let text = value.as_str().unwrap_or_default();
    let mut short: String = text.chars().take(limit).collect();
    if short.len() < text.len() {
        short.push('…');
    }
    short
}

impl PolicyBasis {
    pub(super) fn of(evaluation: &crate::configuration::CandidateEvaluation) -> Self {
        // Serialize only the resolver's evaluation, not the effective policy.
        let value = serde_json::to_value(evaluation.evaluation()).unwrap_or_default();
        let value = &value;
        let applied = || {
            value["matches"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|m| m["applied"] == true)
        };
        // Rules that produced the decision come first; other applied matches follow.
        let rules = applied()
            .filter(|m| m["decision"] == value["decision"])
            .chain(applied().filter(|m| m["decision"] != value["decision"]))
            .take(3)
            .map(|m| BasisRule {
                source: bounded(&m["source"], 128),
                rule_id: bounded(&m["rule_id"], 128),
                reason: bounded(&m["reason"], 256),
            })
            .collect();
        Self {
            decision: bounded(&value["decision"], 16),
            reason: bounded(&value["reason"], 64),
            rules,
        }
    }
}

pub(super) fn inspect(context: &Context, id: Option<&str>) -> Result<Value, Error> {
    if let Some(request) = id {
        return request_view(context, request).map(|(_, v)| v);
    }
    let mut requests = Vec::new();
    for (kind, value) in [
        ("read", super::read_approval::inspect(context, None)?),
        ("execution", super::run_approval::inspect(context, None)?),
    ] {
        for mut row in value["requests"].as_array().cloned().unwrap_or_default() {
            let time = super::clock::best_effort_ms();
            let expiry = if row["status"] == "approved" {
                row["grant_expires_at_ms"].as_u64()
            } else {
                row["pending_expires_at_ms"].as_u64()
            };
            if matches!(row["status"].as_str(), Some("pending" | "approved"))
                && expiry.is_some_and(|expiry| time < expiry)
            {
                row["type"] = json!(kind);
                requests.push(row);
            }
        }
    }
    Ok(json!({"schema_version":1,"project_id":context.project_id,"requests":requests}))
}

/// Count unexpired read and execution requests that wait for an operator decision.
pub(super) fn pending_count(state: &super::model::State, time: u64) -> usize {
    state
        .read_requests
        .iter()
        .filter(|r| r.awaits_decision(time))
        .count()
        + state
            .run_requests
            .iter()
            .filter(|r| r.awaits_decision(time))
            .count()
}

pub(super) fn request_view(context: &Context, id: &str) -> Result<(bool, Value), Error> {
    match super::read_approval::inspect(context, Some(id)) {
        Ok(value) => Ok((true, value)),
        Err(Error::Missing) => super::run_approval::inspect(context, Some(id)).map(|v| (false, v)),
        Err(error) => Err(error),
    }
}
