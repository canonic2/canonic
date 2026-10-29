//! One-time native-run requests and grants; minimum authority is independent of history.
use super::clock::now_ms as now;
use super::{Context, Error, model::State, storage::Store};
use crate::{
    policy::{self, Decision, Request},
    runtime::EvaluatedRequest,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::Path;

#[cfg(all(test, target_os = "macos"))]
mod tests;

/// Native-run requests a project retains in every status.
const MAX_REQUESTS: usize = 128;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Origin {
    agent: Option<String>,
    session_id: Option<String>,
    tool_use_id: Option<String>,
    consumer_id: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RunRequest {
    request_id: String,
    created_at_ms: u64,
    pending_expires_at_ms: u64,
    project_id: String,
    activation_id: String,
    policy_revision: String,
    candidate_digest: String,
    request: Request,
    resource_digest: String,
    resource: Value,
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
    operator_provenance: Value,
    binding: String,
}

fn hash(value: &Value) -> String {
    policy::fingerprint(value.to_string().as_bytes())
}
impl RunRequest {
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
        hash(&json!([
            self.request_id,
            self.created_at_ms,
            self.pending_expires_at_ms,
            self.project_id,
            self.activation_id,
            self.policy_revision,
            self.candidate_digest,
            self.request,
            self.resource_digest,
            self.resource,
            self.origin,
            self.provenance
        ]))
    }
    fn grant_binding(&self, grant: &Grant) -> String {
        hash(&json!([
            self.binding,
            grant.approval_id,
            grant.issued_at_ms,
            grant.expires_at_ms,
            grant.operator_provenance
        ]))
    }
    fn matches(&self, evidence: &EvaluatedRequest, resource: &Value, origin: &Origin) -> bool {
        self.project_id == evidence.identity().project_id()
            && self.activation_id == evidence.identity().activation_id()
            && self.policy_revision == evidence.identity().policy_revision()
            && self.candidate_digest == evidence.identity().candidate_digest()
            && json!(self.request) == json!(evidence.request())
            && self.resource_digest == hash(resource)
            && json!(self.origin) == json!(origin)
    }
}

pub(super) fn validate(state: &State) -> Result<(), Error> {
    let mut ids = std::collections::BTreeSet::new();
    for row in &state.run_requests {
        row.request.validate()?;
        if row.project_id != state.project_id
            || row.request.project != state.project_root
            || !crate::execution::valid_approval_binding(&row.request, &row.resource)
            || row.resource_digest != hash(&row.resource)
            || row.origin.consumer_id != "shield-native-run-v1"
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
            return Err(Error::Invalid("run request binding differs"));
        }
        let issued = state.authorization_events.iter().any(|event| {
            event["kind"] == "run_grant_issued" && event["links"]["request_id"] == row.request_id
        });
        if issued != row.grant.is_some() {
            return Err(Error::Invalid("run grant issuance differs"));
        }
        if let Some(grant) = &row.grant {
            if grant.binding != row.grant_binding(grant)
                || grant.operator_provenance["surface"] != "cli"
                || grant.operator_provenance["project_id"] != state.project_id
                || grant.operator_provenance["project_root"] != state.project_root
                || grant
                    .expires_at_ms
                    .checked_sub(grant.issued_at_ms)
                    .is_none_or(|ttl| !(1000..=3_600_000).contains(&ttl))
                || grant.issued_at_ms < row.created_at_ms
                || grant.issued_at_ms >= row.pending_expires_at_ms
                || !state.authorization_events.iter().any(|event| {
                    event["kind"] == "run_grant_issued"
                        && event["links"]["approval_id"] == grant.approval_id
                        && event["details"]["binding"] == grant.binding
                        && event["links"]["request_id"] == row.request_id
                        && event["details"]["request_binding"] == row.binding
                        && event["details"]["operator_binding"] == hash(&grant.operator_provenance)
                        && event["operator"]["mechanism"] == "operator-cli"
                })
            {
                return Err(Error::Invalid("run grant authorization differs"));
            }
        } else if matches!(row.status.as_str(), "approved" | "consumed") {
            return Err(Error::Invalid("run grant is missing"));
        }
        for (status, kind) in [
            ("consumed", "run_grant_consumed"),
            ("revoked", "run_grant_revoked"),
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
                return Err(Error::Invalid("run grant terminal evidence differs"));
            }
        }
        if row.status == "pending" && row.grant.is_some() {
            return Err(Error::Invalid("pending grant is invalid"));
        }
    }
    Ok(())
}

pub(crate) fn inspect(context: &Context, id: Option<&str>) -> Result<Value, Error> {
    let state = Store::open(Path::new(&context.project_root))?.read_checkpoint()?;
    state.check(context)?;
    let time = now()?;
    let mut value = match id {
        Some(id) => json!(
            state
                .run_requests
                .iter()
                .find(|r| r.request_id == id && r.live_id(time).is_some())
                .ok_or(Error::Missing)?
        ),
        None => {
            json!({"schema_version":1,"requests":state.run_requests.iter().filter(|r| r.live_id(time).is_some()).map(|r| json!({"request_id":r.request_id,
            "status":r.status,"request":r.request,"created_at_ms":r.created_at_ms,"pending_expires_at_ms":r.pending_expires_at_ms,
            "provenance":r.provenance,"policy":r.policy,"grant_expires_at_ms":r.grant.as_ref().map(|g|g.expires_at_ms)})).collect::<Vec<_>>()})
        }
    };
    let redacted = super::model::redact(&mut value);
    value["redacted"] = json!(redacted);
    Ok(value)
}

/// Private native consumer only. Verification occurs before consumption; the
/// store publishes consumption before invoking handoff, all under one writer lock.
#[cfg(target_os = "macos")]
pub(crate) fn gate<T>(
    context: &Context,
    evidence: &EvaluatedRequest,
    resource: &Value,
    reference: Option<&str>,
    verify: impl FnOnce() -> bool,
    handoff: impl FnOnce(&Value) -> T,
) -> Result<(Value, Option<T>), Error> {
    if context.surface != "run"
        || evidence.identity().project_id() != context.project_id
        || !crate::execution::valid_approval_binding(evidence.request(), resource)
        || evidence.request().project != context.project_root
        || reference.is_some_and(|id| id.is_empty() || id.len() > 128 || id.contains('\0'))
    {
        return Err(Error::Invalid("unsupported native approval binding"));
    }
    let origin = Origin {
        agent: context.agent.clone(),
        session_id: context.session_id.clone(),
        tool_use_id: context.tool_use_id.clone(),
        consumer_id: "shield-native-run-v1".into(),
    };
    Store::open(Path::new(&context.project_root))?.transition_handoff(
        context,
        |state| {
            if !state
                .active
                .as_ref()
                .is_some_and(|a| a.matches(evidence.identity()))
            {
                return Err(Error::StaleSnapshot);
            }
            if !verify() {
                return Err(Error::Invalid(
                    "native binding changed or request interrupted",
                ));
            }
            let decision = match evidence.evaluation().decision {
                Decision::Deny => result("policy-deny", false, None, "none", None),
                Decision::Allow if reference.is_none() => {
                    result("allowed", true, None, "none", None)
                }
                // An explicit reference must never silently fall back to Allow.
                Decision::Allow => {
                    return Err(Error::Invalid("approval requires current Ask policy"));
                }
                Decision::Ask => ask(
                    state,
                    context,
                    &origin,
                    evidence,
                    resource,
                    reference,
                    now()?,
                )?,
            };
            Ok((decision, evidence.evaluation().decision == Decision::Ask))
        },
        |decision| {
            if decision["allowed"] == true {
                Some(handoff(decision))
            } else {
                None
            }
        },
    )
}

#[cfg(target_os = "macos")]
fn result(
    code: &str,
    allowed: bool,
    request: Option<&str>,
    status: &str,
    approval: Option<&str>,
) -> Value {
    json!({"code":code,"allowed":allowed,"approval":{"request_id":request,"status":status,"approval_id":approval}})
}

#[cfg(target_os = "macos")]
fn ask(
    state: &mut State,
    context: &Context,
    origin: &Origin,
    evidence: &EvaluatedRequest,
    resource: &Value,
    reference: Option<&str>,
    time: u64,
) -> Result<Value, Error> {
    if let Some(reference) = reference {
        return consume(state, context, origin, evidence, resource, reference, time);
    }
    capture(state, context, origin, evidence, resource, time)
}

#[cfg(target_os = "macos")]
fn consume(
    state: &mut State,
    context: &Context,
    origin: &Origin,
    evidence: &EvaluatedRequest,
    resource: &Value,
    reference: &str,
    time: u64,
) -> Result<Value, Error> {
    let Some(index) = state
        .run_requests
        .iter()
        .position(|r| r.request_id == reference)
    else {
        if !reference.starts_with("run-request-") {
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
    let row = &state.run_requests[index];
    if !row.matches(evidence, resource, origin) {
        return Ok(result(
            "approval-stale",
            false,
            Some(reference),
            "stale",
            None,
        ));
    }
    if row.status == "consumed" {
        return Ok(result(
            "approval-consumed",
            false,
            Some(reference),
            "consumed",
            None,
        ));
    }
    if row.status == "revoked" {
        return Ok(result(
            "approval-revoked",
            false,
            Some(reference),
            "revoked",
            None,
        ));
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
        state.run_requests[index].status = "consumed".into();
        let event = state.event("run_grant_consumed", context, json!({"request_id":reference,"approval_id":approval}), json!({}),
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

#[cfg(target_os = "macos")]
fn capture(
    state: &mut State,
    context: &Context,
    origin: &Origin,
    evidence: &EvaluatedRequest,
    resource: &Value,
    time: u64,
) -> Result<Value, Error> {
    state.compact_authorization_at(time);
    if let Some(row) = state.run_requests.iter().find(|r| {
        r.status == "pending"
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
    if state.run_requests.len() >= MAX_REQUESTS {
        return Err(Error::Capacity {
            kind: super::CapacityKind::RunRequests,
            pending: state
                .run_requests
                .iter()
                .filter(|r| r.status == "pending")
                .count(),
            retained: state.run_requests.len(),
            limit: MAX_REQUESTS,
        });
    }
    let mut row = RunRequest {
        request_id: state.id("run-request"),
        created_at_ms: time,
        pending_expires_at_ms: time + 600_000,
        project_id: state.project_id.clone(),
        activation_id: evidence.identity().activation_id().into(),
        policy_revision: evidence.identity().policy_revision().into(),
        candidate_digest: evidence.identity().candidate_digest().into(),
        request: evidence.request().clone(),
        resource_digest: hash(resource),
        resource: resource.clone(),
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
    state.run_requests.push(row);
    Ok(value)
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
    Store::open(Path::new(&context.project_root))?.update(context, &||Ok(()), |state| {
        let index = state.run_requests.iter().position(|r|r.request_id == id).ok_or(Error::Missing)?;
        let row = &state.run_requests[index];
        if row.status == "consumed" || row.status == "revoked" { return Err(Error::Invalid("request is terminal")); }
        if revoke { return Ok(revoke_request(state, context, index, reason)); }
        let time = clock()?;
        if time < row.created_at_ms || (row.grant.is_none() && time >= row.pending_expires_at_ms) { return Err(Error::Invalid("pending request expired")); }
        let snapshot = super::runtime::consumer::load(context, &||Ok(()))?;
        let evidence = snapshot.evaluate(row.request.clone()).map_err(|_|Error::Invalid("request cannot be evaluated"))?;
        if !row.matches(&evidence, &row.resource, &row.origin) || evidence.evaluation().decision != Decision::Ask {
            return Err(Error::StaleSnapshot);
        }
        crate::execution::verify_approval_binding(&row.request, &row.resource)?;
        if let Some(grant) = &row.grant {
            if time < grant.issued_at_ms || time >= grant.expires_at_ms || grant.expires_at_ms-grant.issued_at_ms != ttl*1000 { return Err(Error::Invalid("grant cannot be renewed or changed")); }
            return Ok(json!({"request_id":id,"status":"approved","approval_id":grant.approval_id,"expires_at_ms":grant.expires_at_ms}));
        }
        let approval = state.id("run-approval");
        let row = &mut state.run_requests[index];
        let mut grant = Grant { approval_id: approval.clone(), issued_at_ms: time, expires_at_ms: time + ttl * 1000, operator_provenance: context.provenance(), binding: String::new() };
        grant.binding = row.grant_binding(&grant);
        let binding = grant.binding.clone();
        let operator_binding = hash(&grant.operator_provenance);
        row.grant = Some(grant);
        row.status = "approved".into();
        let request_binding = row.binding.clone();
        let event = operator_event(state, context, "run_grant_issued", id, Some(&approval), reason,
            json!({"binding":binding,"request_binding":request_binding,"operator_binding":operator_binding,"expires_at_ms":time+ttl*1000}));
        state.record(event);
        Ok(json!({"request_id":id,"status":"approved","approval_id":approval,"expires_at_ms":time+ttl*1000}))
    })
}

fn revoke_request(
    state: &mut State,
    context: &Context,
    index: usize,
    reason: Option<&str>,
) -> Value {
    let row = &mut state.run_requests[index];
    row.status = "revoked".into();
    let id = row.request_id.clone();
    let approval = row.grant.as_ref().map(|g| g.approval_id.clone());
    let binding = row.binding.clone();
    let event = operator_event(
        state,
        context,
        "run_grant_revoked",
        &id,
        approval.as_deref(),
        reason,
        json!({"request_binding":binding}),
    );
    state.record(event);
    json!({"request_id":id,"status":"revoked"})
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
    event["operator"] = json!({"operator_id":null,"display_name":null,"mechanism":"operator-cli"});
    event
}
