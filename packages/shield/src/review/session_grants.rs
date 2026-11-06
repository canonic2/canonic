//! Private reusable conversation authorization, distinct from policy and exact grants.
mod native;
mod scope;
#[cfg(test)]
mod tests;
mod time;
mod view;
#[cfg(test)]
pub(crate) use tests::fixture as native_fixture;
#[cfg(test)]
pub(crate) use tests::fixture_for_agent as native_fixture_for_agent;

use super::{Context, Error, model::State, storage::Store};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path};

pub(crate) use native::handoff;
pub(crate) use view::{confirmation, show};

const PENDING: u64 = 600_000;
const RETAIN: u64 = 86_400_000;
const MAX_FORM: usize = 12_000;

#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Registry {
    contexts: BTreeMap<String, Conversation>,
    records: BTreeMap<String, Record>,
    /// Only issued grants are indexed; inspection/preparation never uses this as authority.
    sessions: BTreeMap<String, Vec<String>>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Conversation {
    reference: String,
    agent: String,
    session_id: String,
    registration: String,
    created: u64,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    id: String,
    session_key: String,
    session: Conversation,
    owner: String,
    key: String,
    created: u64,
    duration: u64,
    scope: Value,
    overrides: Value,
    override_evidence: Value,
    reason: Option<String>,
    activation: Value,
    protection: Value,
    provenance: Value,
    digest: String,
    status: String,
    grant: Option<Grant>,
    terminal: Option<u64>,
    terminal_reason: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Grant {
    id: String,
    approval: String,
    issued: u64,
    expires: u64,
    clock: time::Stamp,
    mechanism: String,
    binding: String,
    revoked: Option<u64>,
}

fn hash(value: &Value) -> String {
    crate::policy::fingerprint(value.to_string().as_bytes())
}

fn store(context: &Context) -> Result<Store, Error> {
    Store::open(Path::new(&context.project_root))
}

fn registration(state: &State, agent: &str) -> Result<String, Error> {
    if crate::agents::find(agent).is_none() {
        return Err(Error::Invalid(
            "session_unavailable: registered native agent required",
        ));
    }
    let registration = super::integrations::registration(&state.operator)?.ok_or(
        Error::Invalid("session_unavailable: integration is not enrolled"),
    )?;
    if !registration.covers(agent) {
        return Err(Error::Invalid(
            "session_unavailable: integration is not enrolled",
        ));
    }
    Ok(hash(&json!(registration)))
}

fn protection(state: &State) -> Result<Value, Error> {
    Ok(json!(super::path_access::load(&state.operator)?))
}

fn activation(state: &State) -> Result<Value, Error> {
    let active = state.active.as_ref().ok_or(Error::StaleSnapshot)?;
    Ok(json!([active.activation_id(), state.policy_revision]))
}

/// Shared authority is stable throughout one inspection/compaction pass.
struct Authority {
    activation: Option<Value>,
    protection: Option<Value>,
    registrations: BTreeMap<String, String>,
}
impl Authority {
    fn capture(state: &State) -> Self {
        let registration = super::integrations::registration(&state.operator)
            .ok()
            .flatten();
        let fingerprint = registration.as_ref().map(|r| hash(&json!(r)));
        Self {
            activation: activation(state).ok(),
            protection: protection(state).ok(),
            registrations: crate::agents::ADAPTERS
                .iter()
                .filter_map(|adapter| {
                    let agent = adapter.spec().id;
                    registration
                        .as_ref()
                        .filter(|r| r.covers(agent))
                        .and_then(|_| fingerprint.clone())
                        .map(|r| (agent.into(), r))
                })
                .collect(),
        }
    }
}

impl Record {
    fn digest(&self) -> String {
        hash(&json!([
            self.id,
            self.session_key,
            self.session,
            self.owner,
            self.key,
            self.created,
            self.duration,
            self.scope,
            self.overrides,
            self.override_evidence,
            self.reason,
            self.activation,
            self.protection,
            self.provenance
        ]))
    }
    fn current_with(&self, authority: &Authority) -> bool {
        authority.activation.as_ref() == Some(&self.activation)
            && authority.protection.as_ref() == Some(&self.protection)
            && authority.registrations.get(&self.session.agent) == Some(&self.session.registration)
    }
    fn effective(&self, state: &State, now: u64) -> &'static str {
        self.effective_with(&Authority::capture(state), now)
    }
    fn effective_with(&self, authority: &Authority, now: u64) -> &'static str {
        if let Some(reason) = &self.terminal_reason {
            return match reason.as_str() {
                "expired" => "expired",
                "revoked" => "revoked",
                "declined" => "declined",
                "withdrawn" => "withdrawn",
                _ => "stale",
            };
        }
        if let Some(grant) = &self.grant {
            if grant.revoked.is_some() {
                "revoked"
            } else if !self.current_with(authority) {
                "stale"
            } else {
                match grant.clock.valid(self.duration * 1000) {
                    Err(_) => "suspended",
                    Ok(false) => "expired",
                    Ok(true) if now >= grant.expires => "expired",
                    Ok(true) => "active",
                }
            }
        } else if self.status != "pending" {
            match self.status.as_str() {
                "declined" => "declined",
                "withdrawn" => "withdrawn",
                _ => "stale",
            }
        } else if !self.current_with(authority) {
            "stale"
        } else if now >= self.created.saturating_add(PENDING) {
            "expired"
        } else {
            "pending"
        }
    }
}

impl Registry {
    fn compact(&mut self, now: u64) {
        self.contexts
            .retain(|_, c| now < c.created.saturating_add(RETAIN));
        self.records.retain(|_, r| {
            let end = r
                .terminal
                .or_else(|| r.grant.as_ref().map(|g| g.revoked.unwrap_or(g.expires)))
                .unwrap_or(r.created.saturating_add(PENDING));
            now < end.saturating_add(RETAIN)
        });
        self.sessions.clear();
        for row in self.records.values().filter(|r| {
            r.terminal_reason.is_none()
                && r.grant
                    .as_ref()
                    .is_some_and(|g| g.revoked.is_none() && now < g.expires)
        }) {
            self.sessions
                .entry(row.session_key.clone())
                .or_default()
                .push(row.id.clone());
        }
    }
}

/// Integrity checks run with the shared required authorization checkpoint.
pub(super) fn validate(state: &State) -> Result<(), Error> {
    let registry = &state.session_grants;
    if registry.contexts.len() > 256
        || registry.records.len() > 1024
        || registry.sessions.len() > 64
    {
        return Err(Error::Invalid("session grant authorization exceeds bounds"));
    }
    for (key, session) in &registry.contexts {
        if key != &hash(&json!([session.agent, session.session_id]))
            || session.reference.len() > 128
            || session.reference.is_empty()
            || session.session_id.is_empty()
            || session.session_id.len() > 4096
        {
            return Err(Error::Invalid("session context binding differs"));
        }
    }
    for (id, row) in &registry.records {
        if id != &row.id
            || row.digest != row.digest()
            || !(60..=28_800).contains(&row.duration)
            || !["pending", "issued", "declined", "withdrawn"].contains(&row.status.as_str())
            || (row.status == "issued") != row.grant.is_some()
            || row.session_key != hash(&json!([row.session.agent, row.session.session_id]))
        {
            return Err(Error::Invalid("session grant candidate binding differs"));
        }
        if let Some(grant) = &row.grant
            && (grant.expires.checked_sub(grant.issued) != Some(row.duration * 1000)
                || !["host-elicitation", "operator-terminal"].contains(&grant.mechanism.as_str())
                || grant.binding
                    != hash(&json!([
                        row.digest,
                        grant.id,
                        grant.approval,
                        grant.issued,
                        grant.expires,
                        grant.clock,
                        grant.mechanism
                    ])))
        {
            return Err(Error::Invalid("session grant issuance binding differs"));
        }
    }
    for (key, ids) in &registry.sessions {
        if ids.len() > 16
            || ids.iter().any(|id| {
                registry
                    .records
                    .get(id)
                    .is_none_or(|r| r.session_key != *key || r.grant.is_none())
            })
        {
            return Err(Error::Invalid("session grant index differs"));
        }
    }
    Ok(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProposalInput {
    session_ref: String,
    idempotency_key: String,
    scope: Value,
    #[serde(default = "default_duration")]
    duration_seconds: u64,
    reason: Option<String>,
    #[serde(default = "empty_array")]
    override_denies: Value,
}
fn default_duration() -> u64 {
    7200
}
fn empty_array() -> Value {
    json!([])
}

/// Capture an immutable candidate. Neither persistence nor a reference is approval.
pub(crate) fn propose(context: &Context, args: &Value) -> Result<Value, Error> {
    let input = proposal_input(args)?;
    propose_input(context, &input)
}

fn proposal_input(args: &Value) -> Result<ProposalInput, Error> {
    let input: ProposalInput = serde_json::from_value(args.clone())
        .map_err(|_| Error::Invalid("invalid session grant arguments"))?;
    if input.idempotency_key.is_empty()
        || input.idempotency_key.len() > 128
        || !(60..=28_800).contains(&input.duration_seconds)
        || input
            .reason
            .as_ref()
            .is_some_and(|r| r.len() > 1024 || r.contains('\0'))
    {
        return Err(Error::Invalid(
            "invalid session grant lifetime, reason or idempotency key",
        ));
    }
    scope::prepare(&input.scope)?;
    Ok(input)
}

fn propose_input(context: &Context, input: &ProposalInput) -> Result<Value, Error> {
    let rules = super::inspection::rules(context, None)?;
    let override_evidence = scope::overrides(&input.override_denies, &rules)?;
    let now = super::clock::now_ms()?;
    store(context)?.update_checkpoint(context, &|| Ok(()), |state| {
        let authority = Authority::capture(state);
        if rules["policy_revision"] != state.policy_revision {
            return Err(Error::StaleSnapshot);
        }
        state.session_grants.compact(now);
        let (session_key, session) = state
            .session_grants
            .contexts
            .iter()
            .find(|(_, s)| s.reference == input.session_ref)
            .map(|(k, s)| (k.clone(), s.clone()))
            .ok_or(Error::Invalid(
                "session_unavailable: run a native request to obtain session_ref",
            ))?;
        if registration(state, &session.agent)? != session.registration
            || (context.surface != "cli"
                && (context.agent.as_deref().is_none_or(|a| a != session.agent)
                    || context
                        .session_id
                        .as_ref()
                        .is_some_and(|s| *s != session.session_id)))
        {
            return Err(Error::Invalid(
                "session_unavailable: native session binding differs",
            ));
        }
        let owner = context.retry_owner();
        if let Some(row) = state.session_grants.records.values().find(|r| {
            r.owner == owner
                && r.session.reference == input.session_ref
                && r.key == input.idempotency_key
        }) {
            if row.scope != input.scope
                || row.duration != input.duration_seconds
                || row.overrides != input.override_denies
                || row.reason != input.reason
            {
                return Err(Error::Invalid(
                    "idempotency_conflict: use a new key for a changed candidate",
                ));
            }
            return Ok(view::public(row, state, now));
        }
        // A new retry key must not create duplicate forms or reopen a declined
        // offer. Changed scope or a new policy/session requires a new review.
        if let Some(row) = state.session_grants.records.values().find(|r| {
            r.session.reference == input.session_ref
                && r.scope == input.scope
                && r.duration == input.duration_seconds
                && r.overrides == input.override_denies
                && r.current_with(&authority)
                && ["pending", "active", "declined", "suspended"]
                    .contains(&r.effective_with(&authority, now))
        }) {
            return Ok(view::public(row, state, now));
        }
        if state.session_grants.records.len() >= 1024
            || state
                .session_grants
                .records
                .values()
                .filter(|r| r.effective_with(&authority, now) == "pending")
                .count()
                >= 64
        {
            return Err(Error::Invalid(
                "capacity_exceeded: inspect session_grant_show before proposing more",
            ));
        }
        let mut row = Record {
            id: state.id("session-proposal"),
            session_key,
            session,
            owner,
            key: input.idempotency_key.clone(),
            created: now,
            duration: input.duration_seconds,
            scope: input.scope.clone(),
            overrides: input.override_denies.clone(),
            override_evidence: override_evidence.clone(),
            reason: input.reason.clone(),
            activation: activation(state)?,
            protection: protection(state)?,
            provenance: context.provenance(),
            digest: String::new(),
            status: "pending".into(),
            grant: None,
            terminal: None,
            terminal_reason: None,
        };
        row.digest = row.digest();
        view::message(&row, &context.project_root, now)?;
        let result = view::public(&row, state, now);
        event(state, context, "session_grant_proposed", &row);
        state.session_grants.records.insert(row.id.clone(), row);
        Ok(result)
    })
}

fn event(state: &mut State, context: &Context, kind: &str, row: &Record) {
    let event = state.event(kind,context,json!({"proposal_id":row.id,"approval_id":row.grant.as_ref().map(|g| &g.approval)}),
        json!({"agent_reason":row.reason}),json!({"grant_id":row.grant.as_ref().map(|g| &g.id),"scope_digest":row.digest,"status":row.status,"terminal_reason":row.terminal_reason}));
    state.record(event);
}

/// Commit only after a direct human answer, revalidating under the writer lock.
pub(crate) fn apply(context: &Context, args: &Value, confirmed: bool) -> Result<Value, Error> {
    transition(context, args, confirmed, false)
}

pub(crate) fn decline(context: &Context, args: &Value) -> Result<Value, Error> {
    transition(context, args, true, true)
}

fn transition(
    context: &Context,
    args: &Value,
    confirmed: bool,
    decline: bool,
) -> Result<Value, Error> {
    let (id, digest) = view::apply_fields(args)?;
    let now = super::clock::now_ms()?;
    store(context)?.update_checkpoint(context, &|| Ok(()), |state| {
        let authority = Authority::capture(state);
        let row = state.session_grants.records.get(id).ok_or(Error::Missing)?;
        if row.digest != digest {
            return Err(Error::Invalid(
                "candidate digest differs; inspect session_grant_show",
            ));
        }
        if row.status == "issued" {
            return Ok(view::public(row, state, now));
        }
        if row.effective(state, now) != "pending" {
            return Err(Error::StaleSnapshot);
        }
        if !confirmed {
            return Err(Error::ApprovalRequired);
        }
        let mut row = row.clone();
        if decline {
            row.status = "declined".into();
            row.terminal = Some(now);
            row.terminal_reason = Some("declined".into());
        } else {
            let active =
                state.session_grants.records.values().filter(|r| {
                    ["active", "suspended"].contains(&r.effective_with(&authority, now))
                });
            if active.clone().count() >= 64
                || active.filter(|r| r.session_key == row.session_key).count() >= 16
            {
                return Err(Error::Invalid(
                    "capacity_exceeded: revoke an unused session grant",
                ));
            }
            let mut grant = Grant {
                id: state.id("session-grant"),
                approval: state.id("approval"),
                issued: now,
                expires: now
                    .checked_add(row.duration * 1000)
                    .ok_or(Error::Invalid("clock out of range"))?,
                clock: time::Stamp::capture()?,
                mechanism: if context.surface == "cli" {
                    "operator-terminal"
                } else {
                    "host-elicitation"
                }
                .into(),
                binding: String::new(),
                revoked: None,
            };
            grant.binding = hash(&json!([
                row.digest,
                grant.id,
                grant.approval,
                grant.issued,
                grant.expires,
                grant.clock,
                grant.mechanism
            ]));
            row.grant = Some(grant);
            row.status = "issued".into();
        }
        event(
            state,
            context,
            if decline {
                "session_grant_declined"
            } else {
                "session_grant_issued"
            },
            &row,
        );
        state
            .session_grants
            .records
            .insert(row.id.clone(), row.clone());
        state.session_grants.compact(now);
        Ok(view::public(&row, state, now))
    })
}

/// A caller may reduce only its own authority; the operator can revoke any record.
pub(crate) fn revoke(context: &Context, args: &Value) -> Result<Value, Error> {
    let fields = args
        .as_object()
        .ok_or(Error::Invalid("invalid session revoke arguments"))?;
    if fields.len() != 1 {
        return Err(Error::Invalid("select exactly one grant_id or proposal_id"));
    }
    let id = fields
        .get("grant_id")
        .or_else(|| fields.get("proposal_id"))
        .and_then(Value::as_str)
        .ok_or(Error::Invalid("select grant_id or proposal_id"))?;
    let now = super::clock::now_ms()?;
    store(context)?.update_checkpoint(context, &|| Ok(()), |state| {
        let row = state
            .session_grants
            .records
            .values()
            .find(|r| r.id == id || r.grant.as_ref().is_some_and(|g| g.id == id))
            .ok_or(Error::Missing)?;
        if context.surface != "cli"
            && row.owner != context.retry_owner()
            && !(context.agent.as_ref() == Some(&row.session.agent)
                && context.session_id.as_ref() == Some(&row.session.session_id))
        {
            return Err(Error::Invalid(
                "operator_review_required: ask the user to revoke this grant in their terminal",
            ));
        }
        if !["active", "pending", "suspended"].contains(&row.effective(state, now)) {
            return Ok(view::public(row, state, now));
        }
        let mut row = row.clone();
        row.terminal = Some(now);
        row.terminal_reason = Some(
            if row.grant.is_some() {
                "revoked"
            } else {
                "withdrawn"
            }
            .into(),
        );
        if let Some(g) = &mut row.grant {
            g.revoked = Some(now);
        } else {
            row.status = "withdrawn".into();
        }
        event(state, context, "session_grant_revoked", &row);
        state
            .session_grants
            .records
            .insert(row.id.clone(), row.clone());
        state.session_grants.compact(now);
        Ok(view::public(&row, state, now))
    })
}

/// Every writer closes observed stale/expired authority irreversibly, even if
/// the operator later restores the same settings. Native reuse remains read-only.
pub(super) fn compact(state: &mut State, now: u64) {
    let authority = Authority::capture(state);
    let terminals: Vec<_> = state
        .session_grants
        .records
        .iter()
        .filter_map(|(id, row)| {
            let status = row.effective_with(&authority, now);
            (row.terminal.is_none() && ["stale", "expired"].contains(&status))
                .then(|| (id.clone(), status.to_owned()))
        })
        .collect();
    for (id, status) in terminals {
        if let Some(row) = state.session_grants.records.get_mut(&id) {
            row.terminal = Some(now);
            row.terminal_reason = Some(status);
        }
    }
    state.session_grants.compact(now);
    let mut index = BTreeMap::<String, Vec<String>>::new();
    for row in state
        .session_grants
        .records
        .values()
        .filter(|r| ["active", "suspended"].contains(&r.effective_with(&authority, now)))
    {
        index
            .entry(row.session_key.clone())
            .or_default()
            .push(row.id.clone());
    }
    state.session_grants.sessions = index;
}

pub(super) fn summary(state: &State) -> Value {
    let now = super::clock::best_effort_ms();
    let authority = Authority::capture(state);
    let count = |status: &str| {
        state
            .session_grants
            .records
            .values()
            .filter(|r| r.effective_with(&authority, now) == status)
            .count()
    };
    json!({"active":count("active"),"pending":count("pending"),"inspect":"session_grant_show"})
}
