//! Canonical project MCP tools with connection-owned diagnostic and paging evidence.
mod apply;
mod confirmation;
mod diagnostics;
mod documentation;
mod inspection;
mod pack_info;
mod schemas;
mod suggestions;
#[cfg(test)]
mod tests;
mod views;
use super::{CapacityKind, Checkpoint, Context, Error, approvals, changes, history, project};
use super::{packs, registries};
use serde_json::{Value, json};
use std::sync::Mutex;

/// Validate exact retained review bytes before showing a host confirmation.
pub(crate) fn apply_confirmation(context: &Context, args: &Value) -> Result<Option<String>, Error> {
    let (state, id) = apply::validate(context, args)?;
    Ok(state
        .proposals
        .iter()
        .find(|p| p.id == id && p.status == "pending")
        .map(|proposal| confirmation::message(proposal, &proposal.apply_arguments())))
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

/// An MCP refusal whose recovery differs from the shared review error's.
struct Refusal {
    code: &'static str,
    message: &'static str,
    route: Option<&'static str>,
}

/// Only the proposing connection may withdraw; review belongs to the operator.
const FOREIGN_WITHDRAWAL: Refusal = Refusal {
    code: "operator_review_required",
    message: "Only the proposing connection can withdraw this proposal. If it is no longer needed, ask the user to reject it with shield policy review in their terminal",
    route: Some("shield policy review"),
};

/// A failed proposal has no receipt and cannot be applied again.
const FAILED_PROPOSAL: Refusal = Refusal {
    code: "operator_review_required",
    message: "This proposal failed to apply and has no receipt. Ask the user to review it with shield policy review in their terminal; do not call policy_apply for it",
    route: Some("shield policy review"),
};

/// A shared review error, or an MCP refusal with its own recovery.
enum Failure {
    Review(Error),
    Refused(&'static Refusal),
    /// Deferrals survive reconnects, so the bound refuses instead of forgetting one.
    DeferralsFull,
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
    views: Mutex<views::Views>,
}
impl Connection {
    pub(crate) fn enrolled(context: &Context) -> Result<Self, Error> {
        let mut views = views::Views::new(
            context
                .connection_id
                .clone()
                .ok_or(Error::Invalid("connection identity required"))?,
        );
        views.active(context)?;
        Ok(Self {
            views: Mutex::new(views),
        })
    }
    #[cfg(test)]
    pub(crate) fn new(seed: String) -> Self {
        Self {
            views: Mutex::new(views::Views::new(seed)),
        }
    }
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

pub(crate) fn call_with_confirmation(
    context: &Context,
    connection: &Connection,
    name: &str,
    args: &Value,
    checkpoint: Checkpoint<'_>,
    confirmed: bool,
) -> Value {
    let result = (|| -> Result<Value, Failure> {
        checkpoint()?;
        if matches!(
            name,
            "shield_docs_list" | "shield_docs_search" | "shield_docs_read"
        ) {
            return Ok(documentation::call(name, args, checkpoint)?);
        }
        let mut views = connection
            .views
            .lock()
            .map_err(|_| Error::Invalid("connection state invalid"))?;
        let mut data = dispatch(context, &mut views, name, args, checkpoint, confirmed)?;
        if name != "policy_propose" {
            apply::present(context, &mut data)?;
        }
        if [
            "policy_show",
            "policy_propose",
            "policy_withdraw",
            "policy_receipt",
        ]
        .contains(&name)
        {
            bound_policy_result(&mut data)?;
        }
        Ok(data)
    })();
    match result {
        Ok(mut data) => {
            // Bundled public docs contain no runtime input. Redacting example
            // flags would corrupt their syntax and Markdown line structure.
            let redacted = if matches!(
                name,
                "shield_docs_list" | "shield_docs_search" | "shield_docs_read"
            ) {
                false
            } else {
                super::model::redact(&mut data)
            };
            let revision = data.get("policy_revision").cloned().unwrap_or(Value::Null);
            let missing = data.get("missing").cloned().unwrap_or_else(|| json!([]));
            let truncated = data
                .get("truncated")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            json!({"schema_version":1,"project_id":context.project_id,"policy_revision":revision,"data":data,"missing":missing,"truncated":truncated,"redacted":redacted,"error":null})
        }
        Err(Failure::Review(Error::Missing))
            if matches!(name, "shield_docs_read" | "shield_docs_search") =>
        {
            let mut value = failure(context, &Error::Missing);
            value["error"]["message"] = json!("Unknown documentation page; call shield_docs_list.");
            value
        }
        Err(Failure::Review(error)) => failure(context, &error),
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

fn failure(context: &Context, error: &Error) -> Value {
    let (code, mut message) = error_fields(error);
    let route = match error {
        Error::McpSetupRequired => Some("shield policy review".into()),
        Error::ApprovalRequired => {
            // The adapter's guidance is the recovery in both structured and text results.
            let adapter = crate::agents::review::resolve(context.agent.as_deref());
            message = adapter.guidance().required.into();
            if adapter.confirmation() == crate::agents::review::Confirmation::Terminal {
                Some("shield policy review".into())
            } else {
                Some("policy_apply".into())
            }
        }
        Error::StaleSnapshot => Some("policy_propose".into()),
        Error::Io(_) => Some("shield doctor".into()),
        Error::Capacity { kind, .. } => kind.operator_route().map(Into::into),
        Error::CacheUnavailable => crate::agent_message::cache_repair(&context.project_root),
        Error::Dependency(canonic_registry::resolution::Error::Unenrolled(_)) => {
            Some("shield registry add".into())
        }
        _ => None,
    };
    let mut value = refused(context, code, &message, route.as_deref());
    match error {
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
        if data.to_string().len() <= 1024 * 1024 {
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
    if data.to_string().len() > 1024 * 1024 {
        return Err(crate::policy::Error::Limit.into());
    }
    Ok(())
}
/// The code and recovery-bearing message of a shared review error.
fn error_fields(error: &Error) -> (&'static str, String) {
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
        Error::Dependency(canonic_registry::resolution::Error::Unenrolled(source)) => (
            "invalid_input",
            format!(
                "A dependency needs registry {}, which this project has not added. Ask the user to add it with shield registry add, then retry",
                source.chars().take(256).collect::<String>()
            ),
        ),
        Error::Ambiguous { .. } => (
            "ambiguous_reference",
            format!("Ambiguous reference: {error}"),
        ),
        _ => ("invalid_input", invalid_input_message(error)),
    }
}

fn capacity_message(kind: CapacityKind, pending: usize, retained: usize, limit: usize) -> String {
    let next = match kind {
        CapacityKind::Proposals => {
            "ask the user to review or reject them with shield policy review, then retry"
        }
        CapacityKind::RetainedProposals => {
            "retained proposal storage is full and cannot be cleared yet. Continue without proposing; tell the user that new policy proposals cannot be saved"
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
        Error::Policy(error @ crate::policy::Error::McpDefaults { .. }) => {
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
        Error::McpSetupRequired => (
            "mcp_setup_required",
            "Connection settings changed. Review the complete candidate with shield policy review in an operator terminal, then run shield mcp setup; policy_apply cannot activate this change",
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
        // Replaced by the resolved adapter's guidance in `failure`.
        Error::ApprovalRequired => (
            "operator_review_required",
            "User approval is required to activate this proposal",
        ),
        Error::StaleSnapshot => (
            "stale_revision",
            "Policy or review context changed. Call status for fresh handles, reassess and re-propose any still-needed change",
        ),
        Error::Cancelled | Error::Policy(crate::policy::Error::Interrupted) => (
            "cancelled",
            "The request was cancelled. Inspect current state before retrying a proposal or mutation",
        ),
        Error::Policy(crate::policy::Error::Limit)
        | Error::Dependency(canonic_registry::resolution::Error::Limit) => (
            "budget_exceeded",
            "This request exceeded Shield's limits. Narrow the query or input before retrying",
        ),
        Error::Io(_) => (
            "io_failure",
            "Project or registry I/O failed. Inspect doctor and ask the user to resolve the reported problem before retrying",
        ),
        Error::CacheUnavailable => (
            "cache_unavailable",
            "Compiled policy unavailable; ask the user to run shield cache prepare, then retry",
        ),
        _ => return None,
    })
}
/// Status, bound to the active captured policy, or the doctor report.
fn status(context: &Context, views: &mut views::Views, doctor: bool) -> Result<Value, Error> {
    if doctor {
        return project::doctor(context);
    }
    let mut data = project::status(context)?;
    let id = views.active(context)?;
    let captured = views.get(&id)?;
    if data["active"]["policy_revision"] != captured.revision()? {
        return Err(Error::StaleSnapshot);
    }
    data["policy_revision"] = json!(captured.revision()?);
    data["active_candidate_id"] = json!(id);
    Ok(data)
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
        "policy_withdraw" | "policy_receipt" => {
            fields(args, &["proposal_id"])?;
            inspection::transition(context, name, required(args, "proposal_id")?, checkpoint)
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
        "policy_apply" => apply::apply(context, args, checkpoint, confirmed),
        "policy_propose" => propose(context, views, args, checkpoint),
        "policy_evaluate" => diagnostics::evaluate(context, views, args, checkpoint),
        "policy_compare" => diagnostics::compare(context, views, args, checkpoint),
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
        "request_list" => {
            fields(args, &["request_id", "cursor", "limit"])?;
            selector(args, "request_id")?;
            let data = approvals::inspect(context, text(args, "request_id")?)?;
            views.page(name, args, data, "requests")
        }
        "history_list" => {
            fields(
                args,
                &["entry_id", "denied", "approved", "agent", "cursor", "limit"],
            )?;
            selector(args, "entry_id")?;
            let data = history::inspect(
                context,
                &history::Query {
                    entry: text(args, "entry_id")?,
                    denied: boolean(args, "denied")?,
                    approved: boolean(args, "approved")?,
                    agent: text(args, "agent")?,
                },
            )?;
            views.page(name, args, data, "entries")
        }
        "suggestion_list" => {
            fields(args, &["session_id", "cursor", "limit"])?;
            let session = text(args, "session_id")?.or(context.session_id.as_deref());
            views.page(name, args, suggestions::list(context, session), "requests")
        }
        _ => Err(Error::Invalid("unknown tool")),
    }
}
fn cursor_fields(name: &str, args: &Value) -> Result<(), Error> {
    fields(
        args,
        match name {
            "pack_list" | "request_list" => &["cursor", "limit"],
            "rule_list" => &["candidate_id", "cursor", "limit"],
            "pack_search" => &["query", "cursor", "limit"],
            "pack_info" => &["reference", "view", "query", "cursor", "limit"],
            "policy_show" => &["agent", "session_id", "status", "cursor", "limit"],
            "history_list" => &["denied", "approved", "agent", "cursor", "limit"],
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
    let input: changes::Input = serde_json::from_value(args.clone())
        .map_err(|_| Error::Invalid("invalid proposal arguments"))?;
    let old = views.clone();
    let mut handles = None;
    let result = changes::propose(
        context,
        &input,
        checkpoint,
        &mut |base, base_meta, after, meta, plans| {
            let (base_plan, next_plan) =
                plans.map_or((None, None), |(base, next)| (Some(base), Some(next)));
            handles = Some((
                views.retain_prepared(base, base_meta, base_plan)?,
                views.retain_prepared(after, meta, next_plan)?,
            ));
            Ok(())
        },
        &|proposal| {
            let mut data = apply::proposal_result(context, proposal);
            skipped_pinned(&mut data, &input, &proposal.base_metadata);
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
    data["before_candidate_id"] = json!(before);
    data["after_candidate_id"] = json!(after);
    Ok(data)
}
/// Report the pinned roots each `pack_update` leaves at their exact versions,
/// as `skipped_pinned` rows of selector, id and version.
fn skipped_pinned(data: &mut Value, input: &changes::Input, base: &Value) {
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
        for row in packs::update_skipped(base, reference).unwrap_or_default() {
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
    schemas::tools(agent)
}

/// Each tool's name and description, for the setup permissions page.
pub(crate) fn tool_summaries() -> Vec<(String, String)> {
    tools_for(None)["tools"]
        .as_array()
        .expect("static tool inventory")
        .iter()
        .map(|tool| {
            (
                tool["name"].as_str().unwrap_or_default().to_owned(),
                tool["description"].as_str().unwrap_or_default().to_owned(),
            )
        })
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
