//! In-session approval of refused native hook requests: the hook stores an
//! approvable refusal, the person accepts Shield's MCP form for it, and the
//! identical retry consumes the one-time grant. Minimum authority is
//! independent of history.
use super::clock::now_ms as now;
use super::{Context, Error, model::State, path_access, storage::Store};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::Path;

#[cfg(test)]
mod tests;

/// In-session requests a project retains in every status.
const MAX_REQUESTS: usize = 128;
/// How long a refused request waits for the person's answer.
const PENDING_MS: u64 = 600_000;
/// How long an accepted request waits for its identical retry.
const GRANT_MS: u64 = 300_000;
/// How long a consumed grant covers the same attempt's later permission
/// callback, such as Codex's `PermissionRequest` after an escalated retry.
const FOLLOWUP_MS: u64 = 60_000;
const ID_KIND: &str = "hook-request";
/// Longest form Shield shows, within the host confirmation's message limit.
const MAX_FORM: usize = 12_000;

/// What identifies one exact native request across its refusal and retry.
pub(crate) struct Key<'a> {
    pub(crate) agent: &'a str,
    pub(crate) session: &'a str,
    pub(crate) cwd: &'a str,
    pub(crate) tool: &'a str,
    /// Digest of the decided operation, plus the full tool input when the
    /// operation omits content such as a patch body.
    pub(crate) input_digest: String,
}

/// The active policy generation a refusal was decided against.
pub(crate) struct Identity<'a> {
    pub(crate) activation_id: &'a str,
    pub(crate) policy_revision: &'a str,
    pub(crate) candidate_digest: &'a str,
}

/// What the person is shown before accepting; bound into the request.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Summary {
    /// The exact action, shown whole or not offered.
    pub(crate) action: String,
    /// Why Shield refused it, in plain language.
    pub(crate) reason: String,
    pub(crate) overrides_deny: bool,
    /// The IDs of the rules that deny it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) deny_rules: Vec<String>,
    pub(crate) settings_change: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct HookRequest {
    request_id: String,
    created_at_ms: u64,
    pending_expires_at_ms: u64,
    agent: String,
    session_id: String,
    cwd: String,
    tool: String,
    input_digest: String,
    activation_id: String,
    policy_revision: String,
    candidate_digest: String,
    path_access: Value,
    summary: Summary,
    binding: String,
    status: String,
    grant: Option<Grant>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Grant {
    approval_id: String,
    issued_at_ms: u64,
    expires_at_ms: u64,
    binding: String,
    consumed_at_ms: Option<u64>,
    followup_used: bool,
}

/// What the hook does with an approvable refusal.
pub(crate) enum Outcome {
    /// A live grant for this exact request was consumed; allow it once.
    Allowed,
    /// The refusal is stored; the agent asks the person with this ID.
    Pending(String),
}

fn hash(value: &Value) -> String {
    crate::policy::fingerprint(value.to_string().as_bytes())
}

impl HookRequest {
    pub(super) fn live_id(&self, time: u64) -> Option<&str> {
        let live = match (self.status.as_str(), &self.grant) {
            ("pending", None) => time < self.pending_expires_at_ms,
            ("approved", Some(grant)) => time < grant.expires_at_ms,
            ("consumed", Some(grant)) => {
                !grant.followup_used
                    && grant
                        .consumed_at_ms
                        .is_some_and(|at| time < at.saturating_add(FOLLOWUP_MS))
            }
            _ => false,
        };
        live.then_some(self.request_id.as_str())
    }
    fn binding(&self) -> String {
        hash(&json!([
            self.request_id,
            self.created_at_ms,
            self.pending_expires_at_ms,
            self.agent,
            self.session_id,
            self.cwd,
            self.tool,
            self.input_digest,
            self.activation_id,
            self.policy_revision,
            self.candidate_digest,
            self.path_access,
            self.summary,
        ]))
    }
    fn grant_binding(&self, grant: &Grant) -> String {
        hash(&json!([
            self.binding,
            grant.approval_id,
            grant.issued_at_ms,
            grant.expires_at_ms
        ]))
    }
    fn matches(&self, key: &Key<'_>, identity: &Identity<'_>, access: &Value) -> bool {
        self.agent == key.agent
            && self.session_id == key.session
            && self.cwd == key.cwd
            && self.tool == key.tool
            && self.input_digest == key.input_digest
            && self.activation_id == identity.activation_id
            && self.policy_revision == identity.policy_revision
            && self.candidate_digest == identity.candidate_digest
            && &self.path_access == access
    }
    /// The active generation and protection choice are those it was refused under.
    fn current(&self, state: &State) -> Result<bool, Error> {
        Ok(state.active.as_ref().is_some_and(|active| {
            active.matches_parts(
                &self.activation_id,
                &self.policy_revision,
                &self.candidate_digest,
            )
        }) && self.path_access == access(state)?)
    }
}

fn access(state: &State) -> Result<Value, Error> {
    Ok(json!(path_access::load(&state.operator)?))
}

pub(super) fn validate(state: &State) -> Result<(), Error> {
    let mut ids = std::collections::BTreeSet::new();
    for row in &state.hook_requests {
        let invalid = row.request_id.is_empty()
            || !ids.insert(&row.request_id)
            || row.binding != row.binding()
            || row.pending_expires_at_ms.checked_sub(row.created_at_ms) != Some(PENDING_MS)
            || [&row.agent, &row.session_id, &row.cwd, &row.tool]
                .iter()
                .any(|field| field.is_empty() || field.len() > 4096)
            || !matches!(row.status.as_str(), "pending" | "approved" | "consumed")
            || (row.status == "pending") != row.grant.is_none();
        if invalid {
            return Err(Error::Invalid("in-session request binding differs"));
        }
        let Some(grant) = &row.grant else { continue };
        let issued = state.authorization_events.iter().any(|event| {
            event["kind"] == "hook_grant_issued"
                && event["links"]["request_id"] == row.request_id
                && event["links"]["approval_id"] == grant.approval_id
                && event["details"]["binding"] == grant.binding
                && event["details"]["request_binding"] == row.binding
                && event["operator"]["mechanism"] == "host-elicitation"
        });
        let consumed = state.authorization_events.iter().any(|event| {
            event["kind"] == "hook_grant_consumed"
                && event["links"]["request_id"] == row.request_id
                && event["links"]["approval_id"] == grant.approval_id
                && event["details"]["grant_binding"] == grant.binding
        });
        if !issued
            || grant.binding != row.grant_binding(grant)
            || grant.expires_at_ms.checked_sub(grant.issued_at_ms) != Some(GRANT_MS)
            || grant.issued_at_ms < row.created_at_ms
            || grant.issued_at_ms >= row.pending_expires_at_ms
            || (row.status == "consumed") != grant.consumed_at_ms.is_some()
            || (row.status == "consumed") != consumed
            || (grant.followup_used && row.status != "consumed")
        {
            return Err(Error::Invalid("in-session grant authorization differs"));
        }
    }
    Ok(())
}

/// What state already holds for one exact refused request.
enum Found {
    /// The index of a live grant for it.
    Grant(usize),
    /// The ID of its live pending request.
    Pending(String),
}

/// Whether the generation a hook decided under is still the active one.
fn active(state: &State, identity: &Identity<'_>) -> bool {
    state.active.as_ref().is_some_and(|active| {
        active.matches_parts(
            identity.activation_id,
            identity.policy_revision,
            identity.candidate_digest,
        )
    })
}

/// The live grant or pending request for this exact request under the current
/// generation and protection choice.
fn lookup(
    state: &State,
    key: &Key<'_>,
    identity: &Identity<'_>,
    time: u64,
) -> Result<Option<Found>, Error> {
    if !active(state, identity) {
        return Err(Error::StaleSnapshot);
    }
    let access = access(state)?;
    if let Some(index) = state.hook_requests.iter().position(|row| {
        row.status == "approved"
            && row.matches(key, identity, &access)
            && row
                .grant
                .as_ref()
                .is_some_and(|grant| grant.issued_at_ms <= time && time < grant.expires_at_ms)
    }) {
        return Ok(Some(Found::Grant(index)));
    }
    Ok(state
        .hook_requests
        .iter()
        .find(|row| {
            row.status == "pending"
                && time < row.pending_expires_at_ms
                && row.matches(key, identity, &access)
        })
        .map(|row| Found::Pending(row.request_id.clone())))
}

/// Consume a live grant for this exact request, or store the refusal for the
/// person to answer. A repeated refusal reuses its pending request without
/// writing; consuming or storing is one writer transaction.
pub(crate) fn refused(
    root: &Path,
    key: &Key<'_>,
    identity: &Identity<'_>,
    summary: &Summary,
) -> Result<Outcome, Error> {
    let context = super::open(root)?;
    let store = Store::open(root)?;
    let found = store.inspect_checkpoint(&context, &|| Ok(()), |state| {
        lookup(state, key, identity, now()?)
    })?;
    if let Some(Found::Pending(id)) = found {
        return Ok(Outcome::Pending(id));
    }
    let value = store.update_checkpoint(&context, &|| Ok(()), |state| {
        let time = now()?;
        state.compact_authorization_at(time);
        match lookup(state, key, identity, time)? {
            Some(Found::Grant(index)) => {
                consume(state, &context, index, time);
                return Ok(json!({"allowed":true}));
            }
            Some(Found::Pending(id)) => return Ok(json!({"request_id":id})),
            None => {}
        }
        let access = access(state)?;
        if state.hook_requests.len() >= MAX_REQUESTS {
            // A pending request holds no authority; the oldest makes room.
            let Some(oldest) = state
                .hook_requests
                .iter()
                .position(|r| r.status == "pending")
            else {
                return Err(Error::Invalid(
                    "in-session approvals are at capacity; wait for live grants to expire",
                ));
            };
            state.hook_requests.remove(oldest);
        }
        let mut row = HookRequest {
            request_id: state.id(ID_KIND),
            created_at_ms: time,
            pending_expires_at_ms: time + PENDING_MS,
            agent: key.agent.into(),
            session_id: key.session.into(),
            cwd: key.cwd.into(),
            tool: key.tool.into(),
            input_digest: key.input_digest.clone(),
            activation_id: identity.activation_id.into(),
            policy_revision: identity.policy_revision.into(),
            candidate_digest: identity.candidate_digest.into(),
            path_access: access,
            summary: summary.clone(),
            binding: String::new(),
            status: "pending".into(),
            grant: None,
        };
        row.binding = row.binding();
        let id = row.request_id.clone();
        state.hook_requests.push(row);
        Ok(json!({"request_id":id}))
    })?;
    Ok(match value["request_id"].as_str() {
        Some(id) => Outcome::Pending(id.to_owned()),
        None => Outcome::Allowed,
    })
}

fn consume(state: &mut State, context: &Context, index: usize, time: u64) {
    let row = &mut state.hook_requests[index];
    row.status = "consumed".into();
    let grant = row.grant.as_mut().expect("approved requests hold a grant");
    grant.consumed_at_ms = Some(time);
    let links = json!({"request_id":row.request_id,"approval_id":grant.approval_id});
    let details = json!({"request_binding":row.binding,"grant_binding":grant.binding,
        "consumed_at_ms":time,"execution":"unobserved"});
    let event = state.event("hook_grant_consumed", context, links, json!({}), details);
    state.record(event);
}

/// Whether a grant consumed moments ago covers this exact request's later
/// permission callback in the same attempt. Each grant covers one; a callback
/// with nothing to cover writes nothing.
pub(crate) fn followup(root: &Path, key: &Key<'_>, identity: &Identity<'_>) -> Result<bool, Error> {
    // Rechecked under the writer lock: a policy change since the hook decided
    // ends the grant's cover.
    let covering = |state: &State, time: u64| -> Result<Option<usize>, Error> {
        if !active(state, identity) {
            return Ok(None);
        }
        let access = access(state)?;
        Ok(state.hook_requests.iter().position(|row| {
            row.status == "consumed"
                && row.matches(key, identity, &access)
                && row.live_id(time).is_some()
        }))
    };
    let context = super::open(root)?;
    let store = Store::open(root)?;
    if store
        .inspect_checkpoint(&context, &|| Ok(()), |state| covering(state, now()?))?
        .is_none()
    {
        return Ok(false);
    }
    let value = store.update_checkpoint(&context, &|| Ok(()), |state| {
        let Some(index) = covering(state, now()?)? else {
            return Ok(json!(false));
        };
        let row = &mut state.hook_requests[index];
        if let Some(grant) = row.grant.as_mut() {
            grant.followup_used = true;
        }
        Ok(json!(true))
    })?;
    Ok(value == json!(true))
}

fn agent_matches(context: &Context, row: &HookRequest) -> bool {
    context.agent.as_deref() == Some(row.agent.as_str())
}

/// The pending request an MCP call selects, if it can still be approved.
fn selected<'a>(
    state: &'a State,
    context: &Context,
    args: &Value,
) -> Result<&'a HookRequest, Error> {
    if context.surface != "mcp" {
        return Err(Error::ApprovalRequired);
    }
    let fields = args
        .as_object()
        .ok_or(Error::Invalid("arguments must be an object"))?;
    if fields.len() != 1 {
        return Err(Error::Invalid("request_approve takes only request_id"));
    }
    let id = args["request_id"]
        .as_str()
        .ok_or(Error::Invalid("request_id is required"))?;
    let time = now()?;
    let row = state
        .hook_requests
        .iter()
        .find(|row| row.request_id == id && agent_matches(context, row))
        .ok_or(Error::Missing)?;
    if row.status != "pending" {
        return Err(Error::Invalid(
            "this request was already answered; repeat the identical call once if it was approved, otherwise continue without it",
        ));
    }
    if time >= row.pending_expires_at_ms {
        return Err(Error::Invalid(
            "this request expired; repeat the call to get a new request_id",
        ));
    }
    if !row.current(state)? {
        return Err(Error::Invalid(
            "policy or protection settings changed since this request; repeat the call to get a new request_id",
        ));
    }
    Ok(row)
}

/// The plain-language question for the person, after validating the request.
pub(crate) fn confirmation(context: &Context, args: &Value) -> Result<String, Error> {
    let state = Store::open(Path::new(&context.project_root))?.read_checkpoint()?;
    state.check(context)?;
    let row = selected(&state, context, args)?;
    Ok(message(&row.summary, &row.cwd))
}

/// Whether the form can show this request whole; a longer one is not offered.
pub(crate) fn fits(summary: &Summary, cwd: &str) -> bool {
    message(summary, cwd).len() <= MAX_FORM
}

/// Shield's own lines come first; the agent's request follows as an indented
/// block with control characters escaped, so it cannot pose as Shield's lines.
fn message(summary: &Summary, cwd: &str) -> String {
    let mut text = format!(
        "Allow the agent to do this once?\n\nShield blocked it because {}.",
        summary.reason.trim_end_matches('.')
    );
    if summary.overrides_deny {
        match summary.deny_rules.as_slice() {
            [] => text.push_str("\nAllowing it overrides a rule that denies it."),
            rules => {
                text.push_str("\nAllowing it overrides the rule that denies it: ");
                text.push_str(&rules.join(", "));
                text.push('.');
            }
        }
    }
    if summary.settings_change {
        text.push_str(
            "\nChanging agent settings can turn off Shield's protection or the confirmation this question relies on.",
        );
    }
    text.push_str("\nFolder: ");
    text.push_str(&one_line(cwd));
    text.push_str("\n\nThe agent's request, exactly:\n");
    for line in summary.action.split('\n') {
        text.push_str("    ");
        text.push_str(&one_line(line));
        text.push('\n');
    }
    text.push_str(
        "\nThis allows only this exact request, once, within five minutes. It does not change your policy.",
    );
    text
}

/// Escapes characters that break or rewrite a displayed line.
pub(crate) fn one_line(text: &str) -> String {
    let mut shown = String::with_capacity(text.len());
    for c in text.chars() {
        if (c.is_control() && c != '\t') || matches!(c, '\u{2028}' | '\u{2029}') {
            shown.extend(c.escape_default());
        } else {
            shown.push(c);
        }
    }
    shown
}

/// Issue the one-time grant after the person accepted the form.
pub(crate) fn approve(context: &Context, args: &Value, confirmed: bool) -> Result<Value, Error> {
    if !confirmed {
        // A request that cannot be approved reports why; a valid one still
        // needs the person's answer.
        confirmation(context, args)?;
        return Err(Error::ApprovalRequired);
    }
    Store::open(Path::new(&context.project_root))?.update_checkpoint(context, &|| Ok(()), |state| {
        let id = selected(state, context, args)?.request_id.clone();
        let time = now()?;
        let approval = state.id("hook-approval");
        let index = state
            .hook_requests
            .iter()
            .position(|row| row.request_id == id)
            .ok_or(Error::Missing)?;
        let row = &mut state.hook_requests[index];
        let mut grant = Grant {
            approval_id: approval.clone(),
            issued_at_ms: time,
            expires_at_ms: time + GRANT_MS,
            binding: String::new(),
            consumed_at_ms: None,
            followup_used: false,
        };
        grant.binding = row.grant_binding(&grant);
        let details = json!({"binding":grant.binding,"request_binding":row.binding,"expires_at_ms":grant.expires_at_ms});
        row.grant = Some(grant);
        row.status = "approved".into();
        let mut event = state.event(
            "hook_grant_issued",
            context,
            json!({"request_id":id,"approval_id":approval}),
            json!({}),
            details,
        );
        event["operator"] =
            json!({"operator_id":null,"display_name":null,"mechanism":"host-elicitation"});
        state.record(event);
        Ok(json!({"request_id":id,"status":"approved","expires_at_ms":time + GRANT_MS,
            "next_action":"Repeat the identical tool call once now. Any change to it needs a new approval."}))
    })
}
