//! One-time text-read requests and grants; minimum authority is independent of history.
use super::clock::now_ms as now;
use super::{Context, Error, model::State, storage::Store};
use crate::{
    policy::{self, Decision, Request},
    runtime::EvaluatedRequest,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::Path;

mod legacy;
mod receipts;
pub(crate) mod runtime;
#[cfg(test)]
pub(crate) mod tests;

/// Text-read requests a project retains in every status.
const MAX_REQUESTS: usize = 128;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Origin {
    agent: Option<String>,
    session_id: Option<String>,
    consumer_id: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReadRequest {
    request_id: String,
    created_at_ms: u64,
    pending_expires_at_ms: u64,
    project_id: String,
    activation_id: String,
    policy_revision: String,
    candidate_digest: String,
    request: Request,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    resource: Option<super::runtime::consumer::ResourceRequest>,
    resource_digest: String,
    /// General-runtime rows require their private, admitted consumer context.
    /// Absence retains the exact legacy binding and consumption contract.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    runtime_context: Option<String>,
    origin: Origin,
    provenance: Value,
    binding: String,
    status: String,
    grant: Option<Grant>,
    /// Review context only; outside the request and grant bindings.
    #[serde(default)]
    policy: super::approvals::PolicyBasis,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Grant {
    approval_id: String,
    issued_at_ms: u64,
    expires_at_ms: u64,
    binding: String,
}

fn hash(value: &Value) -> String {
    policy::fingerprint(value.to_string().as_bytes())
}
impl ReadRequest {
    pub(super) fn id(&self) -> &str {
        &self.request_id
    }
    pub(super) fn session_id(&self) -> Option<&str> {
        self.origin.session_id.as_deref()
    }
    /// Whether the request still waits for an operator decision.
    pub(super) fn awaits_decision(&self, time: u64) -> bool {
        self.status == "pending" && time < self.pending_expires_at_ms
    }
    pub(super) fn live_id(&self, time: u64) -> Option<&str> {
        let live = match self.status.as_str() {
            "pending" => time < self.pending_expires_at_ms,
            "approved" => self.grant.as_ref().is_some_and(|g| time < g.expires_at_ms),
            _ => false,
        };
        live.then_some(self.request_id.as_str())
    }
    fn binding(&self) -> String {
        let mut binding = json!([
            self.request_id,
            self.created_at_ms,
            self.pending_expires_at_ms,
            self.project_id,
            self.activation_id,
            self.policy_revision,
            self.candidate_digest,
            self.request,
            self.resource_digest,
            self.origin,
            self.provenance
        ]);
        if let Some(resource) = &self.resource {
            binding
                .as_array_mut()
                .expect("binding array")
                .push(json!(resource));
        }
        if let Some(context) = &self.runtime_context {
            binding
                .as_array_mut()
                .expect("binding array")
                .push(json!({"runtime_context":context}));
        }
        hash(&binding)
    }
    fn grant_binding(&self, grant: &Grant) -> String {
        hash(&json!([
            self.binding,
            grant.approval_id,
            grant.issued_at_ms,
            grant.expires_at_ms
        ]))
    }
    fn matches(&self, evidence: &EvaluatedRequest, resource: &str, origin: &Origin) -> bool {
        self.project_id == evidence.identity().project_id()
            && self.activation_id == evidence.identity().activation_id()
            && self.policy_revision == evidence.identity().policy_revision()
            && self.candidate_digest == evidence.identity().candidate_digest()
            && json!(self.request) == json!(evidence.request())
            && evidence.resource().is_some_and(|resource| {
                self.resource.as_ref().map_or_else(
                    || legacy::matches(&self.request, resource),
                    |bound| json!(bound) == json!(resource),
                )
            })
            && self.resource_digest == resource
            && json!(self.origin) == json!(origin)
    }
}

pub(super) fn validate(state: &State) -> Result<(), Error> {
    let mut ids = std::collections::BTreeSet::new();
    for row in &state.read_requests {
        row.request.validate()?;
        if let Some(resource) = &row.resource {
            resource
                .input
                .validate(&resource.context, &row.request.project)?;
            if resource.context.cwd != row.request.cwd
                || !matches!(&resource.input, policy::normalized::Input::File { action, sources, destination: None, recursive: false, .. } if action == "read" && Some(sources.as_slice()) == row.request.arguments.get(..1))
            {
                return Err(Error::Invalid("invalid bound read resource"));
            }
        } else {
            legacy::resource(&row.request)?;
        }
        if row.project_id != state.project_id
            || row
                .runtime_context
                .as_ref()
                .is_some_and(|value| !canonic_registry_kit::digest::is_sha256_hex(value))
            || row.request.project != state.project_root
            || !crate::enforcement::read_text::valid_request(&row.request)
            || row.binding != row.binding()
            || !ids.insert(&row.request_id)
            || row.request_id.is_empty()
            || row.pending_expires_at_ms.checked_sub(row.created_at_ms) != Some(600_000)
            || row.origin.consumer_id.is_empty()
            || row.origin.consumer_id.len() > 128
            || !matches!(
                row.status.as_str(),
                "pending" | "approved" | "consumed" | "revoked"
            )
        {
            return Err(Error::Invalid("read request binding differs"));
        }
        let issued = state.authorization_events.iter().any(|event| {
            event["kind"] == "read_grant_issued" && event["links"]["request_id"] == row.request_id
        });
        if issued != row.grant.is_some() {
            return Err(Error::Invalid("read grant issuance differs"));
        }
        if let Some(grant) = &row.grant {
            if grant.binding != row.grant_binding(grant)
                || grant
                    .expires_at_ms
                    .checked_sub(grant.issued_at_ms)
                    .is_none_or(|ttl| !(1000..=3_600_000).contains(&ttl))
                || grant.issued_at_ms < row.created_at_ms
                || grant.issued_at_ms >= row.pending_expires_at_ms
                || !state.authorization_events.iter().any(|event| {
                    event["kind"] == "read_grant_issued"
                        && event["links"]["approval_id"] == grant.approval_id
                        && event["details"]["binding"] == grant.binding
                        && event["links"]["request_id"] == row.request_id
                        && event["details"]["request_binding"] == row.binding
                        && super::approvals::issued_by_operator(
                            event,
                            super::approvals::read_operator_agent(row.origin.agent.as_deref()),
                            row.origin.session_id.as_deref(),
                        )
                })
            {
                return Err(Error::Invalid("read grant authorization differs"));
            }
        } else if matches!(row.status.as_str(), "approved" | "consumed") {
            return Err(Error::Invalid("read grant is missing"));
        }
        for (status, kind) in [
            ("consumed", "read_grant_consumed"),
            ("revoked", "read_grant_revoked"),
        ] {
            let terminal = state.authorization_events.iter().any(|event| {
                event["kind"] == kind
                    && event["links"]["request_id"] == row.request_id
                    && event["details"]["request_binding"] == row.binding
                    && (status != "consumed"
                        || row.grant.as_ref().is_some_and(|grant| {
                            event["links"]["approval_id"] == grant.approval_id
                                && event["details"]["grant_binding"] == grant.binding
                        }))
            });
            if (row.status == status) != terminal {
                return Err(Error::Invalid("read grant terminal evidence differs"));
            }
        }
        if row.status == "pending" && row.grant.is_some() {
            return Err(Error::Invalid("pending grant is invalid"));
        }
    }
    Ok(())
}

/// Project an already checked authority snapshot at one inspection timestamp.
pub(super) fn inspect_from(state: &State, id: Option<&str>, time: u64) -> Result<Value, Error> {
    let mut value = match id {
        Some(id) => json!(
            state
                .read_requests
                .iter()
                .find(|r| r.request_id == id && r.live_id(time).is_some())
                .ok_or(Error::Missing)?
        ),
        None => {
            json!({"schema_version":1,"requests":state.read_requests.iter().filter(|r| r.live_id(time).is_some()).map(|r| json!({"request_id":r.request_id,
            "status":r.status,"request":r.request,"created_at_ms":r.created_at_ms,"pending_expires_at_ms":r.pending_expires_at_ms,
            "provenance":r.provenance,"policy":r.policy,"grant_expires_at_ms":r.grant.as_ref().map(|g|g.expires_at_ms)})).collect::<Vec<_>>()})
        }
    };
    let redacted = super::model::redact(&mut value);
    value["redacted"] = json!(redacted);
    Ok(value)
}

pub(crate) fn gate(
    context: &Context,
    consumer: &str,
    evidence: &EvaluatedRequest,
    resource: &str,
    reference: Option<&str>,
    verify: impl FnOnce() -> bool,
) -> Result<Value, Error> {
    if evidence.identity().project_id() != context.project_id
        || evidence.resource().is_none()
        || !crate::enforcement::read_text::valid_request(evidence.request())
        || evidence.request().project != context.project_root
        || consumer.is_empty()
        || consumer.len() > 128
        || consumer.contains('\0')
    {
        return Err(Error::Invalid("unsupported approval binding"));
    }
    let origin = Origin {
        agent: context.agent.clone(),
        session_id: context.session_id.clone(),
        consumer_id: consumer.into(),
    };
    let authority = evidence.authorization(context, &|| Ok(()))?;
    Store::open(Path::new(&context.project_root))?.update_checkpoint(context, &|| Ok(()), |state| {
        if reference.is_some() || evidence.evaluation().decision == Decision::Ask {
            state.check_operations()?;
        }
        authority.check_locked(state, evidence, &|| Ok(()))?;
        if !verify() {
            return Ok(result("read-changed", false, None, "none", None));
        }
        if let Some(reference) = reference
            && let Some(receipt) =
                receipts::replay(state, &origin, evidence, resource, reference, now()?)
        {
            return Ok(receipt);
        }
        match evidence.evaluation().decision {
            Decision::Deny => Ok(result("policy-deny", false, None, "none", None)),
            Decision::Allow if reference.is_none() => {
                Ok(result("allowed", true, None, "none", None))
            }
            Decision::Allow => Err(Error::Invalid("approval requires current Ask policy")),
            Decision::Ask => ask(
                state,
                context,
                &origin,
                evidence,
                resource,
                reference,
                now()?,
            ),
        }
    })
}

fn result(
    code: &str,
    allowed: bool,
    request: Option<&str>,
    status: &str,
    approval: Option<&str>,
) -> Value {
    json!({"code":code,"allowed":allowed,"approval":{"request_id":request,"status":status,"approval_id":approval}})
}

fn ask(
    state: &mut State,
    context: &Context,
    origin: &Origin,
    evidence: &EvaluatedRequest,
    resource: &str,
    reference: Option<&str>,
    time: u64,
) -> Result<Value, Error> {
    if let Some(reference) = reference {
        return consume(
            state, context, origin, evidence, resource, reference, time, None,
        );
    }
    capture(state, context, origin, evidence, resource, time)
}

#[expect(
    clippy::too_many_arguments,
    reason = "the private runtime context extends the existing exact read claim without changing legacy rows"
)]
fn consume(
    state: &mut State,
    context: &Context,
    origin: &Origin,
    evidence: &EvaluatedRequest,
    resource: &str,
    reference: &str,
    time: u64,
    runtime_context: Option<&str>,
) -> Result<Value, Error> {
    state.check_operations()?;
    let Some(index) = state
        .read_requests
        .iter()
        .position(|r| r.request_id == reference)
    else {
        if !reference.starts_with("read-request-") {
            return Err(Error::Missing);
        }
        return Ok(result(
            "approval-unavailable",
            false,
            None,
            "unavailable",
            None,
        ));
    };
    let row = &state.read_requests[index];
    if row.runtime_context.as_deref() != runtime_context {
        return Ok(result(
            "approval-unavailable",
            false,
            None,
            "unavailable",
            None,
        ));
    }
    if !row.matches(evidence, resource, origin) {
        return Ok(result(
            "approval-stale",
            false,
            Some(reference),
            "stale",
            None,
        ));
    }
    if let Some((code, status)) = match row.status.as_str() {
        "consumed" => Some(("approval-consumed", "consumed")),
        "revoked" => Some(("approval-revoked", "revoked")),
        _ => None,
    } {
        return Ok(result(code, false, Some(reference), status, None));
    }
    if let Some(grant) = &row.grant {
        if time < grant.issued_at_ms || time >= grant.expires_at_ms {
            return Ok(result(
                "approval-expired",
                false,
                Some(reference),
                "expired",
                None,
            ));
        }
        let approval = grant.approval_id.clone();
        let grant_binding = grant.binding.clone();
        let binding = row.binding.clone();
        receipts::capture(
            state,
            index,
            super::authorization_receipts::Status::Consumed,
            time,
            None,
        )?;
        state.read_requests[index].status = "consumed".into();
        let event = state.event("read_grant_consumed", context, json!({"request_id":reference,"approval_id":approval}), json!({}),
                json!({"request_binding":binding,"grant_binding":grant_binding,"consumed_at_ms":time,"execution":"unobserved"}));
        state.record(event);
        return Ok(result(
            "allowed-once",
            true,
            Some(reference),
            "consumed",
            Some(&approval),
        ));
    }
    if time < row.created_at_ms || time >= row.pending_expires_at_ms {
        return Ok(result(
            "approval-expired",
            false,
            Some(reference),
            "expired",
            None,
        ));
    }
    Ok(result(
        "approval-required",
        false,
        Some(reference),
        "pending",
        None,
    ))
}

fn capture(
    state: &mut State,
    context: &Context,
    origin: &Origin,
    evidence: &EvaluatedRequest,
    resource: &str,
    time: u64,
) -> Result<Value, Error> {
    capture_context(state, context, origin, evidence, resource, time, None)
}

fn capture_context(
    state: &mut State,
    context: &Context,
    origin: &Origin,
    evidence: &EvaluatedRequest,
    resource: &str,
    time: u64,
    runtime_context: Option<&str>,
) -> Result<Value, Error> {
    state.compact_authorization_at(time);
    if let Some(row) = state.read_requests.iter().find(|r| {
        r.status == "pending"
            && r.runtime_context.as_deref() == runtime_context
            && time >= r.created_at_ms
            && time < r.pending_expires_at_ms
            && r.matches(evidence, resource, origin)
    }) {
        return Ok(result(
            "approval-required",
            false,
            Some(&row.request_id),
            "pending",
            None,
        ));
    }
    if state.read_requests.len() >= MAX_REQUESTS {
        return Err(Error::Capacity {
            kind: super::CapacityKind::ReadRequests,
            pending: state
                .read_requests
                .iter()
                .filter(|r| r.status == "pending")
                .count(),
            retained: state.read_requests.len(),
            limit: MAX_REQUESTS,
        });
    }
    let mut row = ReadRequest {
        request_id: state.id("read-request"),
        created_at_ms: time,
        pending_expires_at_ms: time + 600_000,
        project_id: state.project_id.clone(),
        activation_id: evidence.identity().activation_id().into(),
        policy_revision: evidence.identity().policy_revision().into(),
        candidate_digest: evidence.identity().candidate_digest().into(),
        request: evidence.request().clone(),
        resource: Some(
            evidence
                .resource()
                .cloned()
                .ok_or(Error::Invalid("read resource missing"))?,
        ),
        resource_digest: resource.into(),
        runtime_context: runtime_context.map(str::to_owned),
        origin: origin.clone(),
        provenance: context.provenance(),
        binding: String::new(),
        status: "pending".into(),
        grant: None,
        policy: super::approvals::PolicyBasis::of(evidence.evaluation()),
    };
    row.binding = row.binding();
    let value = result(
        "approval-required",
        false,
        Some(&row.request_id),
        "pending",
        None,
    );
    state.read_requests.push(row);
    Ok(value)
}

/// Explicit denial-only entry point; MCP cannot use it to issue a grant.
pub(super) fn deny(context: &Context, id: &str, confirmed: bool) -> Result<Value, Error> {
    if let Some(value) = receipts::operator(
        context,
        id,
        super::authorization_receipts::Decision::Deny,
        None,
        None,
        now()?,
    )? {
        return Ok(value);
    }
    Store::open(Path::new(&context.project_root))?.update_checkpoint(context, &|| Ok(()), |state| {
        let index = state
            .read_requests
            .iter()
            .position(|row| row.request_id == id)
            .ok_or(Error::Missing)?;
        let row = &state.read_requests[index];
        if !super::approvals::owns_request(
            context,
            super::approvals::read_operator_agent(row.origin.agent.as_deref()),
            row.origin.session_id.as_deref(),
        ) {
            return Err(Error::Missing);
        }
        if !confirmed {
            return Err(Error::ApprovalRequired);
        }
        if row.status == "consumed" {
            return Err(Error::Invalid("request is terminal"));
        }
        if row.status == "revoked" {
            return Ok(json!({"request_id":id,"status":"revoked"}));
        }
        revoke_request(state, context, index, None, now()?)
    })
}

pub(crate) fn authorize(
    context: &Context,
    id: &str,
    ttl: u64,
    reason: Option<&str>,
    revoke: bool,
) -> Result<Value, Error> {
    authorize_with_clock(context, id, ttl, reason, revoke, &now)
}

fn authorize_with_clock(
    context: &Context,
    id: &str,
    ttl: u64,
    reason: Option<&str>,
    revoke: bool,
    clock: &dyn Fn() -> Result<u64, Error>,
) -> Result<Value, Error> {
    if context.surface != "cli" {
        return Err(Error::ApprovalRequired);
    }
    if !(1..=3600).contains(&ttl) || reason.is_some_and(|r| r.len() > 4096) {
        return Err(Error::Invalid("invalid grant lifetime or reason"));
    }
    if let Some(value) = receipts::operator(
        context,
        id,
        if revoke {
            super::authorization_receipts::Decision::Deny
        } else {
            super::authorization_receipts::Decision::Approve
        },
        (!revoke).then_some(ttl),
        reason,
        clock()?,
    )? {
        return Ok(value);
    }
    let snapshot = if revoke {
        None
    } else {
        Some(super::runtime::consumer::load(context, &|| Ok(()))?)
    };
    Store::open(Path::new(&context.project_root))?.update_checkpoint(context, &|| Ok(()), |state| {
        if revoke {
            let index = state
                .read_requests
                .iter()
                .position(|r| r.request_id == id)
                .ok_or(Error::Missing)?;
            if matches!(
                state.read_requests[index].status.as_str(),
                "consumed" | "revoked"
            ) {
                return Err(Error::Invalid("request is terminal"));
            }
            return revoke_request(state, context, index, reason, clock()?);
        }
        issue_checked(
            state,
            context,
            id,
            ttl,
            reason,
            snapshot.as_ref().ok_or(Error::StaleSnapshot)?,
            clock()?,
        )
    })
}

/// Validate the exact policy generation and consumer-owned resources without issuing a grant.
fn check(
    state: &State,
    id: &str,
    snapshot: &super::runtime::consumer::Snapshot,
    time: u64,
) -> Result<usize, Error> {
    state.check_operations()?;
    let index = state
        .read_requests
        .iter()
        .position(|r| r.request_id == id)
        .ok_or(Error::Missing)?;
    let row = &state.read_requests[index];
    if row.status == "consumed" || row.status == "revoked" {
        return Err(Error::Invalid("request is terminal"));
    }
    if time < row.created_at_ms || (row.grant.is_none() && time >= row.pending_expires_at_ms) {
        return Err(Error::Invalid("pending request expired"));
    }
    if state
        .active
        .as_ref()
        .is_none_or(|active| !active.matches(snapshot.identity()))
    {
        return Err(Error::StaleSnapshot);
    }
    let resource = row
        .resource
        .clone()
        .map_or_else(|| legacy::resource(&row.request), Ok)?;
    let evidence = snapshot
        .evaluate_resource(row.request.clone(), resource)
        .map_err(|_| Error::Invalid("request cannot be evaluated"))?;
    if !row.matches(&evidence, &row.resource_digest, &row.origin)
        || evidence.evaluation().decision != Decision::Ask
    {
        return Err(Error::StaleSnapshot);
    }
    crate::enforcement::read_text::verify_approval_read(
        &row.request,
        &row.resource_digest,
        snapshot.access_paths(),
    )?;
    Ok(index)
}

fn issue_checked(
    state: &mut State,
    context: &Context,
    id: &str,
    ttl: u64,
    reason: Option<&str>,
    snapshot: &super::runtime::consumer::Snapshot,
    time: u64,
) -> Result<Value, Error> {
    let index = check(state, id, snapshot, time)?;
    let row = &state.read_requests[index];
    if let Some(grant) = &row.grant {
        if time < grant.issued_at_ms
            || time >= grant.expires_at_ms
            || grant.expires_at_ms - grant.issued_at_ms != ttl * 1000
        {
            return Err(Error::Invalid("grant cannot be renewed or changed"));
        }
        return Ok(
            json!({"request_id":id,"status":"approved","approval_id":grant.approval_id,"expires_at_ms":grant.expires_at_ms}),
        );
    }
    let approval = state.id("read-approval");
    let row = &mut state.read_requests[index];
    let mut grant = Grant {
        approval_id: approval.clone(),
        issued_at_ms: time,
        expires_at_ms: time + ttl * 1000,
        binding: String::new(),
    };
    grant.binding = row.grant_binding(&grant);
    let binding = grant.binding.clone();
    row.grant = Some(grant);
    row.status = "approved".into();
    let request_binding = row.binding.clone();
    let event = operator_event(
        state,
        context,
        "read_grant_issued",
        id,
        Some(&approval),
        reason,
        json!({"binding":binding,"request_binding":request_binding,"expires_at_ms":time+ttl*1000}),
    );
    state.record(event);
    Ok(
        json!({"request_id":id,"status":"approved","approval_id":approval,"expires_at_ms":time+ttl*1000}),
    )
}

fn mcp_index(state: &State, context: &Context, id: &str) -> Result<usize, Error> {
    let index = state
        .read_requests
        .iter()
        .position(|r| r.request_id == id)
        .ok_or(Error::Missing)?;
    let row = &state.read_requests[index];
    if context.surface != "mcp"
        || row.origin.agent.is_none()
        || context.agent.as_deref()
            != super::approvals::read_operator_agent(row.origin.agent.as_deref())
        || context.session_id != row.origin.session_id
    {
        return Err(Error::Missing);
    }
    if row.status != "pending" || row.grant.is_some() {
        return Err(Error::Invalid(
            "this request was already answered; retry an approved request once unchanged",
        ));
    }
    Ok(index)
}

pub(super) fn review_in(
    state: &State,
    context: &Context,
    id: &str,
    snapshot: &super::runtime::consumer::Snapshot,
    time: u64,
) -> Result<String, Error> {
    let index = mcp_index(state, context, id)?;
    check(state, id, snapshot, time)?;
    let row = &state.read_requests[index];
    let details = serde_json::to_string_pretty(&json!({"request":row.request,
        "consumer":row.origin.consumer_id,"session":row.origin.session_id,"policy":row.policy}))
    .map_err(|_| Error::Invalid("request review encoding"))?;
    Ok(format!(
        "Allow this exact SDK text read once?\n{details}\nIts resources are revalidated before approval and retry. Permission expires in five minutes; policy is unchanged."
    ))
}

pub(super) fn approve_in(
    state: &mut State,
    context: &Context,
    id: &str,
    snapshot: &super::runtime::consumer::Snapshot,
    time: u64,
) -> Result<Value, Error> {
    mcp_index(state, context, id)?;
    let mut value = issue_checked(state, context, id, 300, None, snapshot, time)?;
    value["next_action"] = json!(
        "Retry this identical read through the same factory and session, with the same window. Approval does not execute it."
    );
    Ok(value)
}

fn revoke_request(
    state: &mut State,
    context: &Context,
    index: usize,
    reason: Option<&str>,
    time: u64,
) -> Result<Value, Error> {
    receipts::capture(
        state,
        index,
        super::authorization_receipts::Status::Revoked,
        time,
        reason,
    )?;
    let row = &mut state.read_requests[index];
    row.status = "revoked".into();
    let id = row.request_id.clone();
    let approval = row.grant.as_ref().map(|g| g.approval_id.clone());
    let binding = row.binding.clone();
    let event = operator_event(
        state,
        context,
        "read_grant_revoked",
        &id,
        approval.as_deref(),
        reason,
        json!({"request_binding":binding}),
    );
    state.record(event);
    Ok(json!({"request_id":id,"status":"revoked"}))
}
fn operator_event(
    state: &mut State,
    context: &Context,
    kind: &str,
    id: &str,
    approval: Option<&str>,
    reason: Option<&str>,
    details: Value,
) -> Value {
    let mut event = state.event(
        kind,
        context,
        json!({"request_id":id,"approval_id":approval}),
        json!({"operator_reason":reason}),
        details,
    );
    event["operator"] = json!({"operator_id":null,"display_name":null,"mechanism":if context.surface == "cli" {"operator-cli"} else {"host-elicitation"}});
    event
}
