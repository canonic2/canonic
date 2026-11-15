//! Canonical project MCP tools with connection-owned diagnostic and paging evidence.
mod apply;
mod confirmation;
mod connection_review;
mod diagnostics;
mod documentation;
mod elicitation;
mod inspection;
mod operator;
mod pack_info;
mod schemas;
mod suggestions;
#[cfg(test)]
mod tests;
mod views;
use super::{CapacityKind, Checkpoint, Context, Error, approvals, changes, history, project};
use super::{packs, registries};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::sync::Mutex;

#[cfg(test)]
pub(crate) use elicitation::PackQuestion;
pub(crate) use elicitation::Question;
pub(super) use operator::{Diagnostic, diagnostic as operator_diagnostic, docs as operator_docs};
pub(in crate::review) fn captured_summary(
    candidate: &Value,
    reviewed_files: bool,
    args: &Value,
) -> String {
    confirmation::captured_summary(candidate, reviewed_files, args)
}

pub(crate) fn question_error(context: &Context, error: &Error) -> Value {
    failure(context, error)
}

/// Validate exact retained review bytes before showing a host confirmation.
pub(crate) fn apply_confirmation(
    context: &Context,
    connection: &Connection,
    args: &Value,
) -> Result<Option<String>, Error> {
    let normalized = runtime_context(context)?;
    let context = &normalized;
    if crate::agents::review::for_context(context).confirmation()
        == crate::agents::review::Confirmation::ToolPermission
        && connection
            .views
            .lock()
            .map_err(|_| Error::Locked)?
            .connection_review(args)
            == Some(false)
    {
        return Ok(None);
    }
    let (state, id) = apply::validate(context, args)?;
    let Some(proposal) = state
        .proposals
        .iter()
        .find(|p| p.id == id && p.status == "pending")
    else {
        return Ok(None);
    };
    let changed = super::publication::mcp_changed(&proposal.base_candidate, &proposal.candidate)?;
    if !changed
        && crate::agents::review::for_context(context).confirmation()
            != crate::agents::review::Confirmation::Form
    {
        return Ok(None);
    }
    let mut message = confirmation::message(proposal, &proposal.apply_arguments());
    if changed {
        message.push('\n');
        message.push_str(super::integrations::HOST_PATH_WARNING);
        // Local settings are deliberately absent from public changes and tool
        // arguments. This human form compares the retained values privately,
        // then labels masked additions, edits and removals without exposing them.
        let review = connection_review::projection(&proposal.base_candidate, &proposal.candidate)?;
        message.push_str("\n\nMCP connection settings (before and after):\n");
        message.push_str(
            &serde_json::to_string_pretty(&review)
                .map_err(|_| Error::Invalid("connection review encoding"))?,
        );
        message.push_str("\nThis approves the connection configuration and publication of its owned connection entries for registered agents. Unrelated settings and native hooks are preserved. Interrupted publication resumes automatically; a conflicting owned edit requires a fresh in-session review.");
    }
    if message.len() > 48 * 1024 {
        return Err(Error::Invalid(
            "policy review exceeds form bound; prepare a smaller change for in-session approval",
        ));
    }
    Ok(Some(message))
}

/// Present the exact stored request whose pending decision or unused grant is revoked.
pub(crate) fn deny_confirmation(context: &Context, args: &Value) -> Result<String, Error> {
    fields(args, &["request_id"])?;
    let (_, request) = approvals::request_view(context, required(args, "request_id")?)?;
    if request["terminal"] == true {
        return Err(Error::Invalid("request is terminal"));
    }
    let message = format!(
        "Deny this pending request or revoke its unused grant?\n{}",
        serde_json::to_string_pretty(&request)
            .map_err(|_| Error::Invalid("request review encoding"))?
    );
    if message.len() > 48 * 1024 {
        return Err(Error::Invalid(
            "request review exceeds the in-session form bound; reduce the reviewed group",
        ));
    }
    Ok(message)
}

#[cfg(test)]
pub(crate) fn initialize_fixture(
    root: &std::path::Path,
    bundle: &crate::configuration::Bundle,
) -> Context {
    let candidates = std::collections::BTreeMap::from([("project".into(), json!(bundle))]);
    let revision = crate::configuration::Evaluator::new(super::snapshots(&candidates).unwrap())
        .unwrap()
        .revision()
        .to_owned();
    super::storage::Store::initialize_active(
        root,
        candidates,
        &revision,
        json!({"sources":{},"default":null,"installed":[],"projection":null}),
    )
    .unwrap();
    super::open(root).unwrap()
}

#[cfg(test)]
pub(crate) fn register_fixture(context: &Context, source: &std::path::Path) {
    super::settings::registries_fixture(context, |registries| {
        registries.sources.insert(
            "fixture".into(),
            canonic_registry_kit::source::Source::local(source).unwrap(),
        );
    });
}

#[cfg(test)]
pub(crate) fn interrupted_setup_fixture(context: &Context, agent: &str) {
    let root = std::path::Path::new(&context.project_root);
    let metadata =
        super::project::metadata(&super::project::checkpoint(context).expect("fixture checkpoint"));
    let plan = super::integrations::setup::prepare(
        root,
        &metadata,
        super::integrations::IntegrationChoice::agent(agent),
    )
    .expect("fixture setup")
    .expect("setup plan");
    super::storage::Store::open(root)
        .expect("fixture store")
        .update_checkpoint(context, &|| Ok(()), |state| {
            state.operator["integration_pending"] = json!(plan);
            Ok(json!({}))
        })
        .expect("confirmed pending setup");
    super::integrations::publish(root, &plan).expect("partial shared publication");
}

/// An MCP refusal whose recovery differs from the shared review error's.
struct Refusal {
    code: &'static str,
    message: &'static str,
    route: Option<&'static str>,
}

/// Only the proposing connection may withdraw; review belongs to the operator.
const FOREIGN_WITHDRAWAL: Refusal = Refusal {
    code: "operator_review_required",
    message: "Only the proposing interaction can withdraw this live review. Inspect it with policy_show; if still needed, prepare a fresh change in this session. Do not send the user to a proposal queue",
    route: Some("policy_show"),
};

/// `request_approve` reached without the person's answer: the host showed no form.
const REQUEST_CONFIRMATION: Refusal = Refusal {
    code: "operator_review_required",
    message: "The user must accept Shield's confirmation form for this request, and this host did not show it. Ask the user to enable confirmation prompts for this session, then call request_approve again with the same arguments",
    route: None,
};

const SESSION_CONFIRMATION: Refusal = Refusal {
    code: "operator_review_required",
    message: "The user must accept Shield's session-grant confirmation form. Enable confirmation prompts in this session and retry while the candidate remains current. If the host cannot show the form, report the missing capability; do not send the user to a terminal",
    route: None,
};

/// A shared review error, or an MCP refusal with its own recovery.
enum Failure {
    Review(Error),
    Refused(&'static Refusal),
    /// Deferrals survive reconnects, so the bound refuses instead of forgetting one.
    DeferralsFull,
    Documentation(canonic_mcp_kit::docs::DocsError),
}
impl From<Error> for Failure {
    fn from(error: Error) -> Self {
        Self::Review(error)
    }
}
impl From<crate::policy::Error> for Failure {
    fn from(error: crate::policy::Error) -> Self {
        Self::Review(error.into())
    }
}

pub(crate) struct Connection {
    owner: Mutex<Option<super::proposal_lifecycle::Lease>>,
    views: Mutex<views::Views>,
    admin: Mutex<
        std::collections::BTreeMap<u64, (std::time::Instant, String, super::admin::Prepared)>,
    >,
    admin_sequence: std::sync::atomic::AtomicU64,
}
pub(crate) fn needs_runtime(name: &str, args: &Value) -> bool {
    schemas::mutates(name)
        || name == "policy_evaluate" && args.get("candidate_id").is_none_or(Value::is_null)
}
/// A connection's context for a call that uses the project's runtime: the
/// folder's current review state, converted from an earlier format first. A
/// folder is its files, so the connection follows whatever state the folder
/// holds now rather than the one it found when it started.
pub(crate) fn runtime_context(context: &Context) -> Result<Context, Error> {
    let root = std::path::Path::new(&context.project_root);
    // Read-only when the state is current and is the one this connection has;
    // only a conversion or recreated state opens it for writing.
    let (state, pending) = super::storage::Store::inspect(root)?.read_diagnostic_checkpoint()?;
    if !pending && state.project_id == context.project_id {
        state.check(context)?;
        state.check_operations()?;
        return Ok(context.clone());
    }
    let opened = super::open(root)?;
    if opened.project_root != context.project_root {
        return Err(Error::Invalid("project folder changed; reconnect Shield"));
    }
    let mut current = context.clone();
    current.project_id = opened.project_id;
    Ok(current)
}

impl Connection {
    pub(crate) fn enrolled(context: &Context) -> Result<Self, Error> {
        let views = views::Views::new(
            context
                .connection_id
                .clone()
                .ok_or(Error::Invalid("connection identity required"))?,
        );
        let (state, _) =
            super::storage::Store::inspect(std::path::Path::new(&context.project_root))?
                .read_diagnostic_checkpoint()?;
        state.check(context)?;
        Ok(Self {
            owner: Mutex::new(None),
            views: Mutex::new(views),
            admin: Mutex::new(BTreeMap::default()),
            admin_sequence: std::sync::atomic::AtomicU64::new(0),
        })
    }
    #[cfg(test)]
    pub(crate) fn new(seed: String) -> Self {
        Self {
            owner: Mutex::new(None),
            views: Mutex::new(views::Views::new(seed)),
            admin: Mutex::new(BTreeMap::default()),
            admin_sequence: std::sync::atomic::AtomicU64::new(0),
        }
    }
    fn ensure_owner(&self, context: &Context) -> Result<(), Error> {
        let Some(id) = &context.connection_id else {
            return Ok(());
        };
        let mut owner = self.owner.lock().map_err(|_| Error::Locked)?;
        if let Some(owner) = owner.as_ref() {
            if owner.root != context.project_root || owner.connection != *id {
                return Err(Error::Invalid(
                    "connection ownership differs from registration",
                ));
            }
        } else {
            *owner = Some(super::proposal_lifecycle::Lease::new(context)?);
        }
        Ok(())
    }
}

fn admin_key(name: &str, args: &Value) -> String {
    crate::policy::fingerprint(json!([name, args]).to_string().as_bytes())
}

/// Mutating calls resume only a previously human-confirmed plan.
/// Explicit conflict review/cancellation must inspect that plan before writes.
pub(crate) fn resume_setup(context: &Context, name: &str, args: &Value) -> Result<bool, Error> {
    if !schemas::mutates(name)
        || (name == "mcp_setup"
            && (!args["pending"].is_null()
                || args["dry_run"] == true
                || !args["project"].is_null()))
    {
        return Ok(false);
    }
    super::integrations::setup::resume_for(context, setup_resume_only(name, args))
}

pub(crate) fn setup_resume_only(name: &str, args: &Value) -> bool {
    name == "mcp_setup" && args == &json!({})
}

pub(crate) fn setup_resume_status(
    context: &Context,
    connection: &Connection,
    error: Option<&Error>,
    checkpoint: Checkpoint<'_>,
) -> Value {
    let mut value = call_with_bound_confirmation(
        context,
        connection,
        "status",
        &json!({}),
        checkpoint,
        false,
        None,
    );
    if let Some(error) = error {
        value["data"]["setup_error"] = json!(error.to_string());
    }
    value
}

pub(crate) fn discard_admin(connection: &Connection, binding: u64) {
    if let Ok(mut pending) = connection.admin.lock() {
        pending.remove(&binding);
    }
}

/// Retain the exact administrative plan while the host obtains the person's answer.
pub(crate) fn admin_confirmation(
    context: &Context,
    connection: &Connection,
    name: &str,
    args: &Value,
    checkpoint: Checkpoint<'_>,
) -> Result<Option<(String, u64)>, Error> {
    if !super::admin::supports(name) {
        return Ok(None);
    }
    let normalized = runtime_context(context)?;
    let context = &normalized;
    if (name == "mcp_setup" && args["dry_run"] == true)
        || crate::agents::review::for_context(context).confirmation()
            == crate::agents::review::Confirmation::Terminal
    {
        return Ok(None);
    }
    checkpoint()?;
    {
        let mut pending = connection.admin.lock().map_err(|_| Error::Locked)?;
        pending.retain(|_, (created, _, _)| created.elapsed() < std::time::Duration::from_mins(5));
        if pending.len() >= 8 {
            return Err(Error::Overloaded);
        }
    }
    let prepared = super::admin::prepare(context, name, args, checkpoint)?;
    let reconciled = prepared.committed_result().is_some();
    checkpoint()?;
    let key = admin_key(name, args);
    let mut pending = connection
        .admin
        .lock()
        .map_err(|_| Error::Invalid("administrative review state invalid"))?;
    pending.retain(|_, (created, _, _)| created.elapsed() < std::time::Duration::from_mins(5));
    if pending.len() >= 8 {
        return Err(Error::Overloaded);
    }
    let message = if reconciled {
        String::new()
    } else {
        format!(
            "Confirm this Shield administrative change?\n{}",
            serde_json::to_string_pretty(&prepared.review())
                .map_err(|_| Error::Invalid("administrative review encoding"))?
        )
    };
    if message.len() > 48 * 1024 {
        return Err(Error::Invalid(
            "administrative review exceeds the in-session form bound; prepare a smaller change",
        ));
    }
    let id = connection
        .admin_sequence
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    pending.insert(id, (std::time::Instant::now(), key, prepared));
    Ok((!reconciled).then_some((message, id)))
}

fn admin_call(
    context: &Context,
    connection: &Connection,
    name: &str,
    args: &Value,
    checkpoint: Checkpoint<'_>,
    confirmed: bool,
    binding: Option<u64>,
) -> Result<Value, Error> {
    let retained = if let Some(binding) = binding {
        connection
            .admin
            .lock()
            .map_err(|_| Error::Invalid("administrative review state invalid"))?
            .remove(&binding)
    } else {
        let mut pending = connection.admin.lock().map_err(|_| Error::Locked)?;
        let key = admin_key(name, args);
        let completed = pending.iter().find_map(|(id, (_, candidate, prepared))| {
            (candidate == &key && prepared.committed_result().is_some()).then_some(*id)
        });
        completed.and_then(|id| pending.remove(&id))
    };
    if let Some((created, key, prepared)) = retained {
        if !confirmed && prepared.committed_result().is_none() {
            return Err(Error::ApprovalRequired);
        }
        if key != admin_key(name, args) {
            return Err(Error::StaleSnapshot);
        }
        if created.elapsed() >= std::time::Duration::from_mins(5) {
            return Err(Error::StaleSnapshot);
        }
        return prepared.commit(context, checkpoint);
    }
    if name == "mcp_setup" && args["dry_run"] == true {
        return Ok(super::admin::prepare(context, name, args, checkpoint)?.public_review());
    }
    Err(if confirmed {
        Error::StaleSnapshot
    } else {
        Error::ApprovalRequired
    })
}
#[cfg(test)]
pub(crate) fn call(
    context: &Context,
    connection: &Connection,
    name: &str,
    args: &Value,
    checkpoint: Checkpoint<'_>,
) -> Value {
    call_with_confirmation(context, connection, name, args, checkpoint, false)
}

#[cfg(test)]
pub(crate) fn call_with_confirmation(
    context: &Context,
    connection: &Connection,
    name: &str,
    args: &Value,
    checkpoint: Checkpoint<'_>,
    confirmed: bool,
) -> Value {
    call_with_bound_confirmation(context, connection, name, args, checkpoint, confirmed, None)
}

pub(crate) fn call_with_bound_confirmation(
    context: &Context,
    connection: &Connection,
    name: &str,
    args: &Value,
    checkpoint: Checkpoint<'_>,
    confirmed: bool,
    binding: Option<u64>,
) -> Value {
    let normalized = if documentation::handles(name) {
        None
    } else if needs_runtime(name, args) {
        match runtime_context(context) {
            Ok(context) => Some(context),
            Err(error) => return failure(context, &error),
        }
    } else {
        None
    };
    let context = normalized.as_ref().unwrap_or(context);
    let result = (|| -> Result<ToolData, Failure> {
        checkpoint()?;
        // Documentation never waits for connection state held by other calls.
        if documentation::handles(name) {
            return documentation::call(name, args, checkpoint)
                .map(ToolData::Raw)
                .map_err(Failure::Documentation);
        }
        if schemas::mutates(name) {
            connection.ensure_owner(context)?;
        }
        if super::admin::supports(name) {
            return Ok(ToolData::Raw(admin_call(
                context, connection, name, args, checkpoint, confirmed, binding,
            )?));
        }
        let mut views = connection
            .views
            .lock()
            .map_err(|_| Error::Invalid("connection state invalid"))?;
        if name == "policy_evaluate" {
            return Ok(ToolData::Projected(diagnostics::evaluate(
                context, &mut views, args, checkpoint,
            )?));
        }
        if name == "policy_compare" {
            return Ok(ToolData::Projected(diagnostics::compare(
                context, &mut views, args, checkpoint,
            )?));
        }
        let mut data = dispatch(context, &mut views, name, args, checkpoint, confirmed)?;
        if !matches!(
            name,
            "policy_propose"
                | "policy_show"
                | "pack_update"
                | "pack_remove"
                | "pack_configure"
                | "rule_add"
                | "rule_edit"
                | "rule_remove"
        ) && !name.starts_with("session_grant_")
        {
            apply::present(context, &mut data)?;
        }
        if [
            "policy_show",
            "policy_propose",
            "pack_update",
            "pack_remove",
            "pack_configure",
            "policy_withdraw",
        ]
        .contains(&name)
        {
            bound_policy_result(&mut data)?;
        }
        Ok(ToolData::Raw(data))
    })();
    if name == "policy_apply"
        && let Err(Failure::Review(error)) = &result
        && let Some(outcome) = settle_apply_failure(context, args, error)
    {
        return outcome;
    }
    result_envelope_data(context, name, result)
}

/// Pre-form failures and post-confirmation failures close through the same
/// writer-locked reconciliation, including a commit whose reply was lost.
pub(crate) fn settle_apply_failure(
    context: &Context,
    args: &Value,
    error: &Error,
) -> Option<Value> {
    let unsupported = crate::agents::review::for_context(context).confirmation()
        == crate::agents::review::Confirmation::Terminal;
    if matches!(error, Error::Missing | Error::Locked | Error::Io(_))
        || matches!(error, Error::ApprovalRequired) && !unsupported
    {
        return None;
    }
    let (status, reason) = match error {
        Error::StaleSnapshot => ("stale", "base_changed"),
        Error::ApprovalRequired => ("withdrawn", "unsupported_host"),
        Error::Cancelled | Error::Policy(crate::policy::Error::Interrupted) => {
            ("withdrawn", "cancelled")
        }
        _ => ("failed", "apply_failed"),
    };
    super::proposal_lifecycle::settle(context, args, status, reason)
        .ok()
        .filter(|outcome| {
            outcome["status"] == "applied"
                && !matches!(error, Error::Invalid(_) | Error::StateRecreated)
        })
        .map(|outcome| result_envelope(context, "policy_apply", Ok(outcome)))
}

pub(crate) fn policy_outcome(context: &Context, args: &Value, code: &str) -> Value {
    let status = if code == "approval_declined" {
        "rejected"
    } else {
        "withdrawn"
    };
    let outcome = super::proposal_lifecycle::settle(context, args, status, code);
    let mut value = result_envelope(context, "policy_apply", outcome.map_err(Failure::from));
    if value["error"].is_null() && value["data"]["status"] != "applied" {
        let message = if code == "approval_unavailable" {
            "This approval attempt is closed. Enable host confirmation prompts in this session (Codex: /permissions), then prepare fresh with a new idempotency_key if still authorized. Do not change host settings yourself."
        } else {
            "This approval attempt is closed without applying. Continue without it; do not repeat a declined offer unless the user requests it."
        };
        value["error"] = json!({"code":code,"message":message,"operator_route":null});
    }
    value
}

enum ToolData {
    Raw(Value),
    Projected(diagnostics::Projected),
}

fn result_envelope(context: &Context, name: &str, result: Result<Value, Failure>) -> Value {
    result_envelope_data(context, name, result.map(ToolData::Raw))
}

fn result_envelope_data(context: &Context, name: &str, result: Result<ToolData, Failure>) -> Value {
    match result {
        Ok(data) => {
            let (mut data, projected) = match data {
                ToolData::Raw(data) => (data, None),
                ToolData::Projected(data) => {
                    let (value, redacted) = data.into_parts();
                    (value, Some(redacted))
                }
            };
            super::reports::attach(context, name, &mut data);
            // Bundled public docs contain no runtime input. Redacting example
            // flags would corrupt their syntax and Markdown line structure.
            let redacted = if let Some(redacted) = projected {
                redacted
            } else if ["history_list", "history_show"].contains(&name) {
                super::history::redact_result(&mut data)
            } else {
                !documentation::handles(name) && super::model::redact(&mut data)
            };
            let redacted = redacted || data["redacted"] == true;
            // A reader that did not follow the files knows no revision.
            let revision = data
                .get("policy_revision")
                .filter(|revision| revision.as_str() != Some(""))
                .cloned()
                .unwrap_or(Value::Null);
            let missing = data.get("missing").cloned().unwrap_or_else(|| json!([]));
            let truncated = data
                .get("truncated")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let mut envelope = json!({"schema_version":1,"project_id":context.project_id,"policy_revision":revision,"data":null,"missing":missing,"truncated":truncated,"redacted":redacted,"error":null});
            envelope["data"] = data;
            envelope
        }
        Err(Failure::Review(error)) => {
            let mut value = admin_error(context, name, &error);
            if name.starts_with("session_grant_") && matches!(error, Error::StaleSnapshot) {
                value["error"]["operator_route"] = json!("session_grant_show");
                value["error"]["message"] = json!(
                    "This session candidate is no longer pending or current. Inspect session_grant_show; obtain fresh confirmation only while the task remains authorized. Do not retry a declined offer automatically. Only if the person explicitly asks to reconsider a still-current decline, use session_grant_reconsider and obtain fresh human confirmation."
                );
            }
            value
        }
        Err(Failure::Documentation(error)) => {
            let (code, message) = documentation::error_fields(&error);
            refused(context, code, &message, None)
        }
        Err(Failure::Refused(refusal)) => {
            refused(context, refusal.code, refusal.message, refusal.route)
        }
        Err(Failure::DeferralsFull) => refused(
            context,
            "capacity_reached",
            &format!(
                "Capacity reached for review deferrals: {max} retained, limit {max}. Deferral is advisory; continue without deferring these reviews and do not retry review_defer",
                max = suggestions::MAX_DEFERRALS
            ),
            None,
        ),
    }
}

/// The error object of a cancelled call, for results built outside a call.
pub(crate) fn cancelled_error() -> Value {
    let (code, message) = error_fields(&Error::Cancelled);
    json!({"code":code,"message":message,"operator_route":null})
}

/// The evidence for arguments that do not match the tool's input schema,
/// naming where they differ so the agent can correct them.
pub(crate) fn invalid_arguments(context: &Context, tool: &str, path: &str, detail: &str) -> Value {
    let mut value = failure(
        context,
        &Error::Invalid("tool arguments do not match the input schema"),
    );
    let detail: String = detail
        .chars()
        .filter(|c| !c.is_control())
        .take(512)
        .collect();
    let hint = if tool == "policy_propose" && path.ends_with(".cwd.exact") {
        ". cwd.exact takes a path-reference object. For project-root-only scope, use \"cwd\":{\"exact\":{\"root\":\"project\"}}. Preserve the intended path and exact operator; within also matches subdirectories."
    } else {
        ""
    };
    value["error"]["message"] = json!(format!(
        "Tool arguments do not match the input schema: {detail}{hint}"
    ));
    value
}

pub(crate) fn admin_route(name: &str) -> Option<&'static str> {
    Some(match name {
        "enable" => "shield enable",
        "disable" => "shield disable",
        "protection_set" => "shield protection set",
        "registry_add" => "shield registry add",
        "registry_remove" => "shield registry remove",
        "registry_default" => "shield registry default",
        "registry_trust" => "shield registry trust",
        "policy_reject" => "shield policy reject",
        "history_configure" => "shield history configure",
        "mcp_setup" => "shield mcp setup",
        _ => return None,
    })
}

pub(crate) fn admin_error(context: &Context, name: &str, error: &Error) -> Value {
    let mut value = failure(context, error);
    if admin_route(name).is_some() {
        if matches!(error, Error::ApprovalRequired) {
            value["error"]["operator_route"] = Value::Null;
            value["error"]["message"] = json!(
                "This administrative change requires supported in-session confirmation. Report the missing capability; do not send the user to a terminal or perform an unreviewed change."
            );
        } else if matches!(error, Error::StaleSnapshot) {
            value["error"]["operator_route"] = json!(name);
            value["error"]["message"] = json!(format!(
                "This administrative review expired or its inputs changed. Call {name} again to prepare a fresh review; the person must confirm it."
            ));
        } else if name == "mcp_setup"
            && matches!(context.agent.as_deref(), Some("claude" | "codex"))
            && matches!(
                error,
                Error::Invalid(
                    "Shield hook group is unowned or manually changed"
                        | "unowned Codex Shield MCP table"
                        | "Codex Shield MCP table was manually changed"
                        | "shield MCP entry is unowned or manually changed; resolve the conflict before setup"
                )
            )
        {
            value["error"]["operator_route"] = json!("mcp_setup");
            value["error"]["retry_arguments"] = json!({"agents":[context.agent],"repair":true});
            value["error"]["message"] = json!(
                "Shield's native registration conflicts with the current files. Call mcp_setup with the returned retry_arguments to review an exact reconciliation in this session. The user must approve it; unrelated hooks and servers are preserved."
            );
        }
    }
    value
}

fn failure(context: &Context, error: &Error) -> Value {
    let (code, mut message) = error_fields(error);
    let route: Option<String> = match error {
        Error::ApprovalRequired => {
            // The adapter's guidance is the recovery in both structured and text results.
            let adapter = crate::agents::review::for_context(context);
            message = adapter.guidance().required.into();
            if adapter.confirmation() == crate::agents::review::Confirmation::Terminal {
                None
            } else {
                Some("policy_apply".into())
            }
        }
        Error::StaleSnapshot => Some("policy_propose".into()),
        Error::Io(_) => Some("doctor".into()),
        Error::Capacity { kind, .. } => kind.operator_route().map(Into::into),
        Error::CacheUnavailable => {
            message = "Shield cannot use its compiled policy folder. Call doctor and ask the user to fix the folder or choose another with shield mcp setup --cache-dir, then retry the original request.".into();
            Some("doctor".into())
        }
        Error::Dependency(canonic_registry_kit::resolution::Error::Unenrolled(_)) => {
            Some("registry_add".into())
        }
        _ => None,
    };
    let mut value = refused(context, code, &message, route.as_deref());
    match error {
        Error::ApprovalFormTooLong { retry, remaining } => {
            value["error"]["retry_arguments"] = json!({"request_ids":retry});
            value["error"]["remaining_request_ids"] = json!(remaining);
        }
        Error::Ambiguous {
            alternatives,
            total,
            ..
        } => {
            value["error"]["alternatives"] = json!(alternatives);
            value["error"]["alternatives_total"] = json!(total);
        }
        Error::RequiredPack { pack, dependents } => {
            value["error"]["pack"] = json!(pack);
            value["error"]["dependents"] = json!(
                dependents
                    .iter()
                    .take(super::MAX_ALTERNATIVES)
                    .collect::<Vec<_>>()
            );
            value["error"]["dependents_total"] = json!(dependents.len());
        }
        _ => {}
    }
    value
}

fn refused(context: &Context, code: &str, message: &str, route: Option<&str>) -> Value {
    json!({"schema_version":1,"project_id":context.project_id,"policy_revision":null,"data":null,"missing":[],"truncated":false,"error":{"code":code,"message":message,"operator_route":route}})
}
fn bound_policy_result(data: &mut Value) -> Result<(), Error> {
    for field in [
        "rules",
        "comparison",
        "authorization_events",
        "project_rules",
    ] {
        if crate::json::fits(data, 1024 * 1024) {
            return Ok(());
        }
        if data
            .as_object_mut()
            .is_some_and(|fields| fields.remove(field).is_some())
        {
            if !data["missing"].is_array() {
                data["missing"] = json!([]);
            }
            data["missing"]
                .as_array_mut()
                .ok_or(Error::Invalid("missing evidence metadata"))?
                .push(json!(field));
            data["truncated"] = json!(true);
        }
    }
    if !crate::json::fits(data, 1024 * 1024) {
        return Err(crate::policy::Error::Limit.into());
    }
    Ok(())
}
/// The code and recovery-bearing message of a shared review error.
fn error_fields(error: &Error) -> (&'static str, String) {
    if let Error::Invalid(message) = error {
        for code in [
            "session_unavailable",
            "unsupported_scope",
            "idempotency_conflict",
            "capacity_exceeded",
            "operator_review_required",
            "authorization_unavailable",
        ] {
            if let Some(detail) = message
                .strip_prefix(code)
                .and_then(|m| m.strip_prefix(": "))
            {
                return (code, detail.into());
            }
        }
    }
    if let Some((code, message)) = static_fields(error) {
        return (code, message.to_owned());
    }
    match error {
        Error::RequiredPack { pack, dependents } => (
            "dependency_required",
            format!(
                "Pack {pack} is required by {}. Keep it, or first remove the packs that depend on it; inspect them with pack_list",
                if dependents.is_empty() {
                    "other installed packs".into()
                } else {
                    let shown = &dependents[..dependents.len().min(super::MAX_ALTERNATIVES)];
                    super::alternatives_text(shown, dependents.len())
                }
            ),
        ),
        Error::Capacity {
            kind,
            pending,
            retained,
            limit,
        } => (
            "capacity_reached",
            capacity_message(*kind, *pending, *retained, *limit),
        ),
        Error::UnsupportedReleases(pack) => (
            "unsupported_version",
            format!(
                "No release of {pack} supports this Shield version. Ask the user to update Shield before retrying; do not retry with another reference to the same pack"
            ),
        ),
        Error::Dependency(canonic_registry_kit::resolution::Error::Unenrolled(source)) => (
            "invalid_input",
            format!(
                "A dependency needs registry {}, which this project has not added. Call registry_add with its alias and source location to obtain the person's trust confirmation in this session, then retry the original pack request. Ask for the location if it is unknown; discovery does not grant trust",
                source.chars().take(256).collect::<String>()
            ),
        ),
        Error::Ambiguous { .. } => (
            "ambiguous_reference",
            format!("Ambiguous reference: {error}"),
        ),
        // Its recovery is in the message; it names no submitted text.
        Error::Grouped { .. } | Error::ApprovalFormTooLong { .. } => {
            ("invalid_input", format!("Shield: {error}"))
        }
        _ => ("invalid_input", invalid_input_message(error)),
    }
}

fn capacity_message(kind: CapacityKind, pending: usize, retained: usize, limit: usize) -> String {
    let next = match kind {
        CapacityKind::Proposals => {
            "other live interactions hold the project capacity. Finish or withdraw your own interaction in this session, or wait for those interactions to finish or expire"
        }
        CapacityKind::RetainedProposals => {
            "the retained outcome recovery window is full. Continue without proposing and retry after older closed outcomes leave their 24-hour recovery window; do not ask the user to clear a queue"
        }
        CapacityKind::ReadRequests | CapacityKind::RunRequests => {
            "ask the user to review them with shield request list (or deny with shield request deny), then retry"
        }
    };
    format!(
        "Capacity reached for {}s: {pending} pending, {retained} retained, limit {limit}; {next}",
        kind.noun()
    )
}

/// Invalid-input messages; each names what to correct without submitted values.
fn invalid_input_message(error: &Error) -> String {
    match error {
        Error::StateFormat { .. } | Error::StateCorrupt { .. } => error.to_string(),
        Error::Policy(error @ (crate::policy::Error::McpDefaults { .. } | crate::policy::Error::Resolution { .. })) => {
            format!("Invalid input: {error}")
        }
        // Static reasons name what to correct; they carry no submitted text.
        Error::Invalid(reason) | Error::Policy(crate::policy::Error::Contract(reason)) => {
            format!("Invalid input: {reason}")
        }
        // The path in the message is one the caller submitted. Directory
        // conflicts name a bounded option, tool file and sources; no machine paths.
        Error::NotPortable(message)
        | Error::Policy(crate::policy::Error::DirectoryOptionConflict(message)) => {
            format!("Invalid input: {message}")
        }
        // Bounded selection and variable identifiers and a static reason; never a value.
        Error::Policy(crate::policy::Error::Binding {
            selection,
            variable,
            reason,
        }) => {
            let pack = if selection.is_empty() {
                String::new()
            } else {
                format!("pack {selection} ")
            };
            format!("Invalid input: {pack}variable {variable}: {reason}")
        }
        Error::Dependency(_) => "Pack dependencies could not be resolved under the requested versions, profiles and enrolled registries. Check them with pack_info and pack_list".into(),
        _ => "Request or retained state is invalid".into(),
    }
}

/// Errors whose message is fixed. Each carries the recovery, since hosts may
/// show the model only the structured result.
fn static_fields(error: &Error) -> Option<(&'static str, &'static str)> {
    Some(match error {
        Error::Policy(crate::policy::Error::ArtifactMismatch) => (
            "candidate_error",
            "Shield could not build a consistent pack candidate. The policy files were not changed. Continue without this proposal and report the failure; changing tool arguments is not a recovery for this error",
        ),
        Error::CandidateUnavailable => (
            "candidate_unavailable",
            "This handle is unavailable in this connection. Call status for current context, then request a fresh view",
        ),
        Error::CursorUnavailable => (
            "cursor_unavailable",
            "This cursor is unavailable for this query or connection. Restart the list without cursor; continue by copying next_cursor into cursor with the same tool, filters and limit, including omitted fields",
        ),
        Error::Overloaded | Error::Locked => (
            "overloaded",
            "Shield is busy or at capacity. Wait for the current operation to finish, then retry or reconnect",
        ),
        Error::Missing => (
            "not_found",
            "The requested record is unavailable. List current records before selecting another ID",
        ),
        Error::AgentNotConnected => ("agent_not_connected", super::hook_approval::NOT_CONNECTED),
        // Replaced by the resolved adapter's guidance in `failure`.
        Error::ApprovalRequired => (
            "operator_review_required",
            "User approval is required to apply this proposal",
        ),
        Error::StaleSnapshot => (
            "stale_revision",
            "Policy or review context changed. Call status for fresh handles, reassess and re-propose any still-needed change. For a preparation task, submit the original concrete changes again instead of the old preparation_task_id; if you supplied an idempotency_key, use a new key for fresh preparation",
        ),
        Error::StateRecreated => (
            "state_recreated",
            "This project's Shield state was recreated after this connection started. Ask the user to restart this agent session so Shield reconnects, then retry",
        ),
        Error::Cancelled | Error::Policy(crate::policy::Error::Interrupted) => (
            "cancelled",
            "The request was cancelled. Inspect current state before retrying a proposal or mutation",
        ),
        Error::Policy(crate::policy::Error::Limit)
        | Error::Dependency(canonic_registry_kit::resolution::Error::Limit) => (
            "budget_exceeded",
            "This request exceeded Shield's limits. Narrow the query or input before retrying",
        ),
        Error::Io(_) => (
            "io_failure",
            "Project or registry I/O failed. Inspect doctor and ask the user to resolve the reported problem before retrying",
        ),
        Error::CacheUnavailable => (
            "cache_unavailable",
            "Compiled policy folder unusable; inspect doctor and ask the user to fix the folder, then retry",
        ),
        _ => return None,
    })
}
/// Status, bound to the active captured policy, or the doctor report.
fn status(context: &Context, views: &mut views::Views, doctor: bool) -> Result<Value, Error> {
    if doctor {
        let mut data = project::doctor(context)?;
        integration_recovery(context, &mut data);
        return Ok(data);
    }
    let mut data = project::status(context)?;
    integration_recovery(context, &mut data);
    if data["checkpoint_upgrade_pending"] == true {
        data["policy_candidate_id"] = Value::Null;
        return Ok(data);
    }
    let id = match views.active_readonly(context) {
        Ok(id) => id,
        Err(error) => {
            data["policy_candidate_id"] = Value::Null;
            data["diagnostic_snapshot"] = json!({"status":"unavailable","detail":error.to_string(),"next_action":"Retry an explicit policy inspection or evaluation to capture the active policy."});
            return Ok(data);
        }
    };
    // The handle is the policy the files hold, which requests follow.
    let captured = views.get(&id)?;
    data["policy_revision"] = json!(captured.revision());
    data["policy_candidate_id"] = json!(id);
    Ok(data)
}

fn integration_recovery(context: &Context, data: &mut Value) {
    if crate::agents::review::for_context(context).confirmation()
        == crate::agents::review::Confirmation::Terminal
        || !data["repair_route"]
            .as_str()
            .is_some_and(|route| route.starts_with("shield mcp setup"))
    {
        return;
    }
    let resume = data["coverage"]["diagnostics"]
        .as_array()
        .is_some_and(|rows| {
            rows.iter()
                .any(|row| row == "confirmed setup requires repair")
        });
    let args = if resume {
        json!({"pending":"review"})
    } else {
        let mut affected = ["installed_integrations", "registered_integrations"]
            .into_iter()
            .flat_map(|key| data["coverage"][key].as_array().into_iter().flatten())
            .filter(|row| row["status"] == "degraded")
            .filter_map(|row| row["agent"].as_str())
            .filter(|agent| crate::agents::find(agent).is_some())
            .collect::<std::collections::BTreeSet<_>>();
        let host = &data["coverage"]["host_integration"];
        let host_status = host["status"].as_str();
        if let Some(id) = host["agent"].as_str().filter(|_| {
            matches!(host_status, Some("degraded" | "missing" | "changed"))
                || data["coverage"]["diagnostics"]
                    .as_array()
                    .is_some_and(|rows| {
                        rows.iter().any(|row| {
                            row.as_str()
                                .is_some_and(|text| text.contains("Hub ") || text.contains("Box "))
                        })
                    })
        }) {
            affected.insert(id);
        }
        if data["coverage"]["legacy_interceptor"]["status"] == "degraded" {
            affected.insert("claude");
        }
        if affected.is_empty() {
            return;
        }
        json!({"agents":affected})
    };
    data["repair_route"] = json!("mcp_setup");
    data["repair_arguments"] = args;
    if resume {
        data["pending_setup"] = json!({"review":{"tool":"mcp_setup","arguments":{"pending":"review"}},"cancel":{"tool":"mcp_setup","arguments":{"pending":"cancel"}},"authority":"Both actions require a fresh human confirmation; unrelated settings are preserved."});
    }
}

/// Tools with MCP-specific refusals first, then the shared review tools.
fn dispatch(
    context: &Context,
    views: &mut views::Views,
    name: &str,
    args: &Value,
    checkpoint: Checkpoint<'_>,
    confirmed: bool,
) -> Result<Value, Failure> {
    match name {
        "policy_defer" => {
            fields(
                args,
                &["intent", "constraints", "proposal_id", "idempotency_key"],
            )?;
            Ok(super::proposal_lifecycle::drafts::save(context, args)?)
        }
        "policy_draft_remove" => {
            fields(args, &["draft_id"])?;
            Ok(super::proposal_lifecycle::drafts::remove(
                context,
                required(args, "draft_id")?,
            )?)
        }
        "session_grant_show" | "session_grant_list" => {
            Ok(super::session_grants::show(context, args)?)
        }
        "session_grant_propose" => Ok(super::session_grants::propose(context, args)?),
        "session_grant_reconsider" => Ok(super::session_grants::reconsider(context, args)?),
        "session_grant_revoke" => Ok(super::session_grants::revoke(context, args)?),
        "session_grant_approve" => {
            super::session_grants::apply(context, args, confirmed).map_err(|error| match error {
                Error::ApprovalRequired => Failure::Refused(&SESSION_CONFIRMATION),
                error => error.into(),
            })
        }
        "policy_withdraw" => {
            fields(args, &["proposal_id"])?;
            inspection::withdraw(context, required(args, "proposal_id")?, checkpoint)
        }
        "review_defer" => {
            fields(args, &["session_id", "review_ids"])?;
            // Only the retained-deferral bound reports Overloaded here.
            suggestions::dismiss(
                context,
                Some(required(args, "session_id")?),
                &args["review_ids"],
                checkpoint,
            )
            .map_err(|error| match error {
                Error::Overloaded => Failure::DeferralsFull,
                error => error.into(),
            })?;
            Ok(json!({"status":"deferred","policy_changed":false,"review_ids":args["review_ids"]}))
        }
        "request_approve" => {
            super::approvals::approve(context, args, confirmed).map_err(|error| match error {
                Error::ApprovalRequired => Failure::Refused(&REQUEST_CONFIRMATION),
                error => error.into(),
            })
        }
        "request_deny" => {
            fields(args, &["request_id"])?;
            Ok(approvals::deny(
                context,
                required(args, "request_id")?,
                confirmed,
            )?)
        }
        _ => Ok(review_tool(
            context, views, name, args, checkpoint, confirmed,
        )?),
    }
}

fn review_tool(
    context: &Context,
    views: &mut views::Views,
    name: &str,
    args: &Value,
    checkpoint: Checkpoint<'_>,
    confirmed: bool,
) -> Result<Value, Error> {
    let mut normalized;
    let args = if name == "suggestion_list" && args.get("limit").is_none() {
        normalized = args.clone();
        normalized["limit"] = json!(16);
        &normalized
    } else {
        args
    };
    if args.get("cursor").is_some() {
        cursor_fields(name, args)?;
        if name == "pack_info" {
            return pack_info::resume(views, args);
        }
        return views.resume(name, args);
    }

    match name {
        "policy_apply" => apply::apply(context, args, checkpoint, confirmed, views),
        "policy_propose" => propose(context, views, args, checkpoint),
        "pack_update" | "pack_remove" | "pack_configure" | "rule_add" | "rule_edit"
        | "rule_remove" => {
            fields(
                args,
                if name.starts_with("rule_") {
                    &[
                        "rule",
                        "rule_id",
                        "layer",
                        "idempotency_key",
                        "agent_reason",
                    ]
                } else if name == "pack_configure" {
                    &[
                        "reference",
                        "set",
                        "unset",
                        "override",
                        "clear_override",
                        "layer",
                        "idempotency_key",
                        "agent_reason",
                    ]
                } else {
                    &["reference", "idempotency_key", "agent_reason"]
                },
            )?;
            if !name.starts_with("rule_") && name != "pack_update" {
                required(args, "reference")?;
            } else if name == "pack_update" {
                text(args, "reference")?;
            }
            if name == "pack_configure"
                && !["set", "unset", "override", "clear_override"]
                    .iter()
                    .any(|key| args.get(key).is_some())
            {
                return Err(Error::Invalid(
                    "supply variables for a settings form, or set, unset, override or clear_override",
                ));
            }
            let mut change = args.clone();
            let mut proposal = json!({});
            for key in ["idempotency_key", "agent_reason"] {
                if let Some(value) = change.as_object_mut().and_then(|fields| fields.remove(key)) {
                    proposal[key] = value;
                }
            }
            change["kind"] = json!(name);
            proposal["change"] = change;
            propose(context, views, &proposal, checkpoint)
        }
        "pack_search" | "pack_info" => discovery(context, views, name, args),
        _ => inspection_tool(context, views, name, args),
    }
}

/// Read-only listing and inspection tools.
fn inspection_tool(
    context: &Context,
    views: &mut views::Views,
    name: &str,
    args: &Value,
) -> Result<Value, Error> {
    match name {
        "policy_draft_list" => {
            fields(args, &["draft_id", "cursor", "limit"])?;
            let data = super::proposal_lifecycle::drafts::list(context, text(args, "draft_id")?)?;
            if args.get("draft_id").is_some() {
                return Ok(data);
            }
            views.page(name, args, data, "drafts")
        }
        "status" | "doctor" => {
            fields(args, &[])?;
            status(context, views, name == "doctor")
        }
        "pack_list" => {
            fields(args, &["reference", "cursor", "limit"])?;
            selector(args, "reference")?;
            let data = match text(args, "reference")? {
                Some(reference) => packs::show(context, reference)?,
                None => packs::list(context)?,
            };
            views.page(name, args, data, "packs")
        }
        "pack_show" => {
            fields(args, &["reference"])?;
            packs::show(context, required(args, "reference")?)
        }
        "registry_list" => {
            fields(args, &["scope"])?;
            registries::list(match args.get("scope").and_then(Value::as_str) {
                None | Some("project") => Some(context),
                Some("user") => None,
                Some(_) => return Err(Error::Invalid("scope must be project or user")),
            })
        }
        "rule_list" => {
            fields(args, &["rule_ref", "candidate_id", "cursor", "limit"])?;
            if args.get("rule_ref").is_some() {
                fields(args, &["rule_ref", "candidate_id"])?;
            }
            let data = inspection::rules(
                context,
                views,
                args.get("rule_ref"),
                text(args, "candidate_id")?,
            )?;
            views.page(name, args, data, "rules")
        }
        "policy_show" => {
            fields(
                args,
                &[
                    "proposal_id",
                    "agent",
                    "session_id",
                    "status",
                    "cursor",
                    "limit",
                ],
            )?;
            selector(args, "proposal_id")?;
            inspection::policy(context, views, args)
        }
        "request_list" | "request_show" => {
            fields(args, &["request_id", "cursor", "limit"])?;
            if name == "request_show" {
                fields(args, &["request_id"])?;
                required(args, "request_id")?;
            }
            selector(args, "request_id")?;
            let data = approvals::inspect(context, text(args, "request_id")?)?;
            views.page(name, args, data, "requests")
        }
        "history_list" | "history_show" => history_tool(context, views, name, args),
        "suggestion_list" => {
            fields(args, &["session_id", "cursor", "limit"])?;
            let session = text(args, "session_id")?.or(context.session_id.as_deref());
            views.page(name, args, suggestions::list(context, session), "requests")
        }
        _ => Err(Error::Invalid("unknown tool")),
    }
}

fn history_tool(
    context: &Context,
    views: &mut views::Views,
    name: &str,
    args: &Value,
) -> Result<Value, Error> {
    if name == "history_show" {
        fields(args, &["entry_id"])?;
        required(args, "entry_id")?;
    }
    fields(
        args,
        &[
            "entry_id",
            "denied",
            "approved",
            "agent",
            "authorization_kind",
            "grant_id",
            "cursor",
            "limit",
        ],
    )?;
    selector(args, "entry_id")?;
    let data = history::inspect_filtered(
        context,
        &history::Query {
            entry: text(args, "entry_id")?,
            denied: boolean(args, "denied")?,
            approved: boolean(args, "approved")?,
            agent: text(args, "agent")?,
        },
        text(args, "authorization_kind")?,
        text(args, "grant_id")?,
    )?;
    views.page(name, args, data, "entries")
}
fn cursor_fields(name: &str, args: &Value) -> Result<(), Error> {
    fields(
        args,
        match name {
            "pack_list" | "request_list" | "policy_draft_list" => &["cursor", "limit"],
            "rule_list" => &["candidate_id", "cursor", "limit"],
            "pack_search" => &["query", "cursor", "limit"],
            "pack_info" => &["reference", "view", "query", "cursor", "limit"],
            "policy_show" => &["agent", "session_id", "status", "cursor", "limit"],
            "history_list" => &[
                "denied",
                "approved",
                "agent",
                "authorization_kind",
                "grant_id",
                "cursor",
                "limit",
            ],
            "suggestion_list" => &["session_id", "cursor", "limit"],
            _ => return Err(Error::Invalid("tool does not support paging")),
        },
    )
}
fn discovery(
    context: &Context,
    views: &mut views::Views,
    name: &str,
    args: &Value,
) -> Result<Value, Error> {
    if name == "pack_info" {
        return pack_info::inspect(context, views, args);
    }
    match name {
        "pack_search" => {
            fields(args, &["query", "cursor", "limit"])?;
            let data = packs::search(context, required(args, "query")?)?;
            views.page(name, args, data, "packs")
        }
        _ => Err(Error::Invalid("unknown discovery command")),
    }
}
fn propose(
    context: &Context,
    views: &mut views::Views,
    args: &Value,
    checkpoint: Checkpoint<'_>,
) -> Result<Value, Error> {
    if args.get("preparation_task_id").is_some() {
        fields(args, &["preparation_task_id"])?;
        let input = super::preparation_tasks::review_input(
            context,
            required(args, "preparation_task_id")?,
        )?;
        return propose_input(context, views, &input, checkpoint);
    }
    let input: changes::Input = serde_json::from_value(args.clone())
        .map_err(|_| Error::Invalid("invalid proposal arguments"))?;
    propose_input(context, views, &input, checkpoint)
}

fn propose_input(
    context: &Context,
    views: &mut views::Views,
    input: &changes::Input,
    checkpoint: Checkpoint<'_>,
) -> Result<Value, Error> {
    let old = views.clone();
    let mut handles = None;
    let presentation = std::cell::RefCell::new(None);
    // Pinned-root reporting resolves aliases through the project's registries.
    let settings = super::settings::current(
        std::path::Path::new(&context.project_root),
        &super::settings::Legacy::default(),
    );
    let result = changes::propose(
        context,
        input,
        checkpoint,
        &mut |base, base_meta, after, meta, plans| {
            handles = Some(views.retain_pair(context, (base, base_meta), (after, meta), plans)?);
            Ok(())
        },
        &|proposal| {
            let mut data = apply::proposal_result(context, proposal);
            presentation.replace(Some((
                apply::argument_binding(context, proposal),
                data["apply_arguments"].clone(),
                super::publication::mcp_changed(&proposal.base_candidate, &proposal.candidate)
                    .unwrap_or(true),
            )));
            skipped_pinned(&mut data, input, &proposal.base_metadata, &settings);
            data
        },
    );
    let mut data = match result {
        Ok(data) => data,
        Err(error) => {
            *views = old;
            return Err(error);
        }
    };
    // The reservation already captured these exact candidates before
    // persistence. Reopening the store here rehydrates every proposal
    // and pack after the write, risking a deadline without its receipt.
    let (before, after) = handles.ok_or(Error::CandidateUnavailable)?;
    if let Some((binding, arguments, connection_review)) = presentation.into_inner() {
        views.remember_approval(binding, arguments, connection_review);
    }
    data["before_candidate_id"] = json!(before);
    data["after_candidate_id"] = json!(after);
    Ok(data)
}
/// Report the pinned roots each `pack_update` leaves at their exact versions,
/// as `skipped_pinned` rows of selector, id and version.
fn skipped_pinned(
    data: &mut Value,
    input: &changes::Input,
    base: &Value,
    settings: &crate::configuration::settings::Values,
) {
    let updates = input
        .change
        .iter()
        .chain(input.changes.iter().flatten())
        .filter_map(|change| match change {
            changes::Change::PackUpdate { reference } => Some(reference.as_deref()),
            _ => None,
        });
    let mut skipped: Vec<Value> = Vec::new();
    for reference in updates {
        // Preparation already resolved the reference against this base; a
        // reference that names a pack an earlier batch change adds has none.
        for row in packs::update_skipped(base, settings, reference).unwrap_or_default() {
            if !skipped.contains(&row) {
                skipped.push(row);
            }
        }
    }
    if !skipped.is_empty() {
        data["skipped_pinned"] = json!(skipped);
    }
}
pub(super) fn fields(value: &Value, allowed: &[&str]) -> Result<(), Error> {
    if value
        .as_object()
        .is_none_or(|o| o.keys().any(|k| !allowed.contains(&k.as_str())))
    {
        Err(Error::Invalid("unknown tool arguments"))
    } else {
        Ok(())
    }
}
pub(super) fn text<'a>(value: &'a Value, key: &str) -> Result<Option<&'a str>, Error> {
    match value.get(key) {
        None => Ok(None),
        Some(Value::String(s)) if !s.is_empty() && s.len() <= 4096 && !s.contains('\0') => {
            Ok(Some(s))
        }
        _ => Err(Error::Invalid("invalid text field")),
    }
}
pub(super) fn required<'a>(value: &'a Value, key: &str) -> Result<&'a str, Error> {
    text(value, key)?.ok_or(Error::Invalid("required field missing"))
}
fn boolean(value: &Value, key: &str) -> Result<bool, Error> {
    match value.get(key) {
        None => Ok(false),
        Some(Value::Bool(b)) => Ok(*b),
        _ => Err(Error::Invalid("invalid boolean")),
    }
}
fn selector(args: &Value, key: &str) -> Result<(), Error> {
    if args.get(key).is_some() && args.as_object().is_some_and(|o| o.len() != 1) {
        Err(Error::Invalid(
            "selector cannot coexist with filters or paging",
        ))
    } else {
        Ok(())
    }
}
#[cfg(test)]
pub(crate) fn tools() -> Value {
    tools_for(None)
}

pub(crate) fn tools_for(agent: Option<&str>) -> Value {
    crate::diagnostics::trace::count("mcp.schema_builds", 1);
    schemas::tools(agent)
}

pub(crate) fn is_tool(name: &str) -> bool {
    schemas::metadata().any(|(known, _)| known == name) || documentation::handles(name)
}

/// Each tool's name and description, for the setup permissions page.
pub(crate) fn tool_summaries() -> Vec<(String, String)> {
    schemas::metadata()
        .map(|(name, description)| (name.to_owned(), description.to_owned()))
        .chain(documentation::tools().iter().map(|tool| {
            (
                tool["name"].as_str().unwrap_or_default().to_owned(),
                tool["description"].as_str().unwrap_or_default().to_owned(),
            )
        }))
        .collect()
}

/// Tools agents may call without asking unless the operator chooses otherwise:
/// every tool except applying a policy change.
pub(crate) fn default_approved() -> Vec<String> {
    tool_summaries()
        .into_iter()
        .map(|(name, _)| name)
        .filter(|name| name != "policy_apply")
        .collect()
}
