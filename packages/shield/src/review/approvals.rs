//! Shared inspection and dispatch for stored action requests.
use super::{Context, Error};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
mod confirmation;
pub(crate) use confirmation::{approve, confirmation};
#[cfg(all(test, any(target_os = "macos", target_os = "linux", windows)))]
pub(crate) mod tests;

/// Native resource grants keep terminal compatibility; host-issued grants also
/// require the issuing MCP agent and session to match the captured consumer.
pub(super) fn issued_by_operator(
    event: &Value,
    agent: Option<&str>,
    session: Option<&str>,
) -> bool {
    event["operator"]["mechanism"] == "operator-cli"
        || (event["operator"]["mechanism"] == "host-elicitation"
            && mcp_origin(&event["provenance"], agent, session))
}

/// The SDK read consumer uses the registered Claude human form channel. This
/// bridge does not give an SDK label native agent authority.
pub(super) fn read_operator_agent(agent: Option<&str>) -> Option<&str> {
    if agent == Some("claude-sdk") {
        Some("claude")
    } else {
        agent
    }
}

pub(super) fn mcp_origin(provenance: &Value, agent: Option<&str>, session: Option<&str>) -> bool {
    agent.is_some()
        && provenance["surface"] == "mcp"
        && provenance["agent"].as_str() == agent
        && provenance["session_id"] == json!(session)
}

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
    let state = super::project::authorization(context)?;
    let time = super::clock::now_ms()?;
    let mut requests = Vec::new();
    for (kind, mut value) in [
        (
            "read",
            super::read_approval::inspect_from(&state, None, time)?,
        ),
        (
            "execution",
            super::run_approval::inspect_from(&state, None, time)?,
        ),
        (
            "hook",
            super::hook_approval::inspect_from(&state, context, None, time)?,
        ),
    ] {
        let Value::Array(rows) = value["requests"].take() else {
            continue;
        };
        for mut row in rows {
            let live = match row["status"].as_str() {
                Some("pending") => true,
                Some("approved") => row["grant_expires_at_ms"]
                    .as_u64()
                    .is_some_and(|expiry| time < expiry),
                _ => false,
            };
            if live {
                row["type"] = json!(kind);
                requests.push(row);
            }
        }
    }
    Ok(json!({"schema_version":1,"project_id":context.project_id,"requests":requests}))
}

/// Read one bounded checkpoint and build public views only for the page.
pub(super) fn list_page(context: &Context, args: &Value) -> Result<Value, Error> {
    let state = super::project::authorization(context)?;
    let time = super::clock::now_ms()?;
    let query = super::page::query(context, "request_list", args);
    let request =
        canonic_mcp_kit::page::Request::from_mcp(args, canonic_mcp_kit::page::Limits::STANDARD)
            .map_err(super::page::error)?;
    let after = super::page::after(&super::page::scope("request_list", &query), &request)?;
    let mut records = Vec::new();
    for row in &state.read_requests {
        if let Some(id) = row.live_id(time) {
            records.push((super::page::key(&json!(id)), "read", id));
        }
    }
    for row in &state.run_requests {
        if let Some(id) = row.live_id(time) {
            records.push((super::page::key(&json!(id)), "execution", id));
        }
    }
    for row in &state.hook_requests {
        if let Some(id) = row.listed_id(&state, time) {
            records.push((super::page::key(&json!(id)), "hook", id));
        }
    }
    records.sort_by(|a, b| a.0.cmp(&b.0));
    let total = records.len();
    let redacted = std::cell::Cell::new(false);
    let rows = records
        .into_iter()
        .filter(|(key, _, _)| after.as_ref().is_none_or(|last| key > last))
        .map(|(key, kind, id)| {
            let mut row = match kind {
                "read" => state
                    .read_requests
                    .iter()
                    .find(|row| row.id() == id)
                    .expect("selected retained read")
                    .list_view(),
                "execution" => state
                    .run_requests
                    .iter()
                    .find(|row| row.id() == id)
                    .expect("selected retained execution")
                    .list_view(),
                _ => super::hook_approval::inspect_from(&state, context, Some(id), time)?,
            };
            if super::model::redact(&mut row) || row["redacted"] == true {
                redacted.set(true);
            }
            row["type"] = json!(kind);
            Ok((key, row))
        });
    let page = super::page::collect_results("request_list", &query, rows, "request_show")?;
    Ok(
        json!({"schema_version":1,"project_id":context.project_id,"requests":page.items,"next_cursor":page.next_cursor,"total":total,"redacted":redacted.get()}),
    )
}

/// Count requests that wait for an operator decision.
pub(super) fn pending_count(state: &super::model::State) -> usize {
    state
        .read_requests
        .iter()
        .filter(|r| r.awaits_decision())
        .count()
        + state
            .run_requests
            .iter()
            .filter(|r| r.awaits_decision())
            .count()
        + super::hook_approval::pending_count(state)
}

pub(super) enum RequestKind {
    Read,
    Execution,
    Hook,
}

pub(super) fn request_view(context: &Context, id: &str) -> Result<(RequestKind, Value), Error> {
    let state = super::project::authorization(context)?;
    let time = super::clock::now_ms()?;
    if let Some((kind, value)) =
        super::hook_approval::terminal_registry(&state, time).inspect(context, id, time)
    {
        let kind = match kind {
            super::authorization_receipts::Kind::Read => RequestKind::Read,
            super::authorization_receipts::Kind::Execution => RequestKind::Execution,
            super::authorization_receipts::Kind::Hook => RequestKind::Hook,
        };
        return Ok((kind, value));
    }
    match super::read_approval::inspect_from(&state, Some(id), time) {
        Ok(value) => Ok((RequestKind::Read, value)),
        Err(Error::Missing) => match super::run_approval::inspect_from(&state, Some(id), time) {
            Ok(value) => Ok((RequestKind::Execution, value)),
            Err(Error::Missing) => {
                super::hook_approval::inspect_from(&state, context, Some(id), time)
                    .map(|v| (RequestKind::Hook, v))
            }
            Err(error) => Err(error),
        },
        Err(error) => Err(error),
    }
}

/// The terminal has confirmed this exact request; native issuance still needs
/// its host's explicit refusal/override review rather than the Ask-only CLI.
pub(super) fn authorize(context: &Context, id: &str, deny: bool) -> Result<Value, Error> {
    let (kind, view) = request_view(context, id)?;
    if !deny && view["terminal"] == true {
        super::project::authorization(context)?.check_operations()?;
        return super::hook_approval::terminal_registry(
            &super::project::authorization(context)?,
            super::clock::now_ms()?,
        )
        .replay_operator(super::authorization_receipts::OperatorReplay {
            context,
            id,
            decision: super::authorization_receipts::Decision::Approve,
            ttl_seconds: Some(300),
            reason: None,
            at: super::clock::now_ms()?,
        })?
        .ok_or(Error::Missing);
    }
    match kind {
        RequestKind::Read => super::read_approval::authorize(context, id, 300, None, deny),
        RequestKind::Execution => super::run_approval::authorize(context, id, 300, None, deny),
        RequestKind::Hook if deny => super::hook_approval::deny(context, id, true),
        RequestKind::Hook => Err(Error::Invalid(
            "request host confirmation through request_approve",
        )),
    }
}

/// Human-confirmed MCP revocation never issues or broadens an approval.
pub(super) fn deny(context: &Context, id: &str, confirmed: bool) -> Result<Value, Error> {
    match request_view(context, id)?.0 {
        RequestKind::Read => super::read_approval::deny(context, id, confirmed),
        RequestKind::Execution => super::run_approval::deny(context, id, confirmed),
        RequestKind::Hook => super::hook_approval::deny(context, id, confirmed),
    }
}

pub(super) fn owns_request(context: &Context, agent: Option<&str>, session: Option<&str>) -> bool {
    context.surface == "cli"
        || (context.surface == "mcp"
            && agent.is_some()
            && context.agent.as_deref() == agent
            && session.is_none_or(|session| context.session_id.as_deref() == Some(session)))
}
