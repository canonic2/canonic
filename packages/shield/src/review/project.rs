//! Enrolled project loading and noninteractive policy/status inspection.
use super::{Context, Error, model::State, storage::Store};
use super::{integrations, path_access};
use crate::review;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
mod doctor;
pub(super) use doctor::report as doctor;

pub(super) fn metadata(state: &State) -> Value {
    state.operator.clone()
}

pub(super) fn load(context: &Context) -> Result<State, Error> {
    load_prepared(context).map(|(state, _)| state)
}

pub(super) fn load_prepared(
    context: &Context,
) -> Result<(State, crate::configuration::Evaluator), Error> {
    let (state, evaluator) =
        Store::open(Path::new(&context.project_root))?.read_current_evaluated()?;
    state.check(context)?;
    if state.candidates.len() != 1 {
        return Err(Error::Invalid(
            "operator workflow requires project-only policy",
        ));
    }
    Ok((state, evaluator))
}

/// Enrollment and authority metadata; source inspection and preparation are explicit.
pub(super) fn checkpoint(context: &Context) -> Result<State, Error> {
    let state = Store::open(Path::new(&context.project_root))?.read_checkpoint()?;
    state.check(context)?;
    Ok(state)
}

pub(super) fn captured(context: &Context) -> Result<State, Error> {
    let state = Store::open(Path::new(&context.project_root))?.read_current()?;
    state.check(context)?;
    Ok(state)
}

/// Immutable record inspection retains source references. Consumers resolve
/// only selected fields against their canonical lock, without preparing policy.
pub(super) fn captured_inspection(context: &Context) -> Result<State, Error> {
    let store = Store::inspect(Path::new(&context.project_root))?;
    let (state, _) = store.read_diagnostic_checkpoint()?;
    state.check_inspection(context)?;
    if state.legacy_preview.is_some() {
        return Err(Error::Invalid(
            "historical policy display is unavailable before runtime conversion; status and doctor can inspect its recorded proof without preparing policy",
        ));
    }
    Ok(state)
}

pub(super) fn status(context: &Context) -> Result<Value, Error> {
    let (state, pending) =
        Store::inspect(Path::new(&context.project_root))?.read_diagnostic_checkpoint()?;
    state.check_inspection(context)?;
    let mut value = status_from(context, &state);
    if state.legacy_preview.is_none() {
        value["pack_updates"] = super::pack_updates::inspect(context, &state, None);
    }
    value["checkpoint_upgrade_pending"] = json!(pending);
    Ok(value)
}

/// Render a checked history capture, also used by doctor's additional probes.
pub(super) fn status_from(context: &Context, state: &State) -> Value {
    let mut value = review::runtime::status_from(state);
    value["policy_revision"] = json!(state.policy_revision);
    value["enabled"] = json!(state.enforcement.enabled);
    if state.legacy_preview.is_some() {
        value["installed_packs"] = crate::review::packs::summaries(&metadata(state));
        value["history"] = json!({"enabled":state.history_enabled,"limit":state.history_limit,"available":null,"integrity":"unchecked","inspect":"shield history list"});
        value["policy_validation"] =
            json!({"source":"unchecked","model":"unchecked","compiled_integrity":"unchecked"});
        value["project_file_inspection"] = json!("unchecked");
        value["pending_approvals"] = json!(0);
        value["session_grants"] =
            json!({"available":false,"reason":"historical record inspection"});
        value["action_authority"] = json!(
            "unavailable until actual runtime validates and converts the historical checkpoint"
        );
        value["operation_readiness"] =
            json!({"available":false,"reason":"historical checkpoint upgrade pending"});
        value["operation_epoch_pending"] = json!(true);
        value["coverage"] = json!({"registered_integrations":[],"installed_integrations":[],"tool_interception":"unchecked","live_host_verification":"unchecked","verification":{"state":"unchecked","basis":"historical-record-only-inspection"},"policy_inspection":{"availability":"unavailable","source_validation":"not-performed"}});
        value["repair_needed"] = json!(false);
        value["repair_assessment"] =
            json!({"known_integration_repairs":false,"project_files":"unchecked","complete":false});
        value["repair_route"] = json!(
            "The next actual runtime operation validates and converts this historical checkpoint; no new policy consent is required solely for its format."
        );
        value["runtime_relocation_pending"] = json!(state.project_root != context.project_root);
        value["retained_project_root"] = json!(state.project_root);
        value["project_root"] = json!(context.project_root);
        return value;
    }
    value["installed_packs"] = crate::review::packs::summaries(&metadata(state));
    value["pending_configuration"] = json!(
        state
            .proposals
            .iter()
            .filter(|p| p.status == "pending")
            .map(crate::review::model::Proposal::summary)
            .collect::<Vec<_>>()
    );
    value["pending_capture_freshness"] = json!("unchecked; apply revalidates exact reviewed files");
    value["history"] = history_metadata(context, state);
    value["reviewed_policy_consent"] = super::storage::policy_consent_presence(state);
    value["pending_approvals"] = json!(super::approvals::pending_count(
        state,
        super::clock::best_effort_ms()
    ));
    value["session_grants"] = super::session_grants::summary(state);
    value["coverage"] = integrations::status(context, state);
    value["coverage"]["verification"] = json!({"state":"unchecked","basis":"record-only-inspection","history_available":value["history"]["available"],"agents":[],"inspect":"shield history list"});
    value["coverage"]["live_host_verification"] = json!("unchecked");
    value["enforcement"] = json!(integrations::enforcement(&value["coverage"]));
    value["path_access"] = path_access::load(&metadata(state)).map_or_else(
        |error| json!({"error":error.to_string()}),
        |access| json!(access),
    );
    // Exact projection hashing belongs to explicit review/mutation. Even a
    // recorded digest does not make scanning all pack source a metadata probe.
    value["project_file_inspection"] = json!("unchecked");
    value["project_input_metadata"] = state.active.as_ref().map_or_else(
        || json!({"availability":"unavailable","probe":"not-run","integrity":"unchecked","authorizes":false}),
        |active| super::cache::inputs::availability(&active.compiled),
    );
    value["repair_needed"] = json!(value["coverage"]["repair_needed"] == true);
    value["repair_assessment"] = json!({"known_integration_repairs":value["repair_needed"],"project_files":"unchecked","complete":false});
    value["repair_route"] = json!(if value["coverage"]["repair_needed"] == true {
        integration_route(&value["coverage"])
    } else {
        "shield policy review".into()
    });
    inspection_location(context, state, &mut value);
    value
}

/// One known generation marker is a bounded probe; the journal is not opened,
/// listed or decoded to render status. Absence does not establish completeness.
fn history_metadata(context: &Context, state: &State) -> Value {
    let digest = state
        .history_generation
        .strip_prefix("journal-")
        .filter(|value| value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()));
    let incomplete = digest.is_some_and(|digest| {
        let path = Path::new(&context.project_root)
            .join(".shield/review")
            .join(format!("history-{digest}.error"));
        super::storage::objects::read_file(&path)
            .ok()
            .and_then(|file| file.metadata().ok())
            .is_some_and(|metadata| metadata.len() <= 1024 * 1024)
    });
    json!({"enabled":state.history_enabled,"limit":state.history_limit,
        "available":if incomplete {json!(false)}else{Value::Null},
        "integrity":if incomplete {"incomplete"}else{"unchecked"},"inspect":"shield history list"})
}

/// A copied checkpoint is evidence of its old epoch, not usable action authority.
pub(super) fn inspection_location(context: &Context, state: &State, value: &mut Value) {
    let pending = state.project_root != context.project_root;
    value["runtime_relocation_pending"] = json!(pending);
    let readiness = super::storage::incarnation::readiness(state, Path::new(&context.project_root));
    let operations_pending = readiness["available"] != true;
    value["operation_readiness"] = readiness;
    value["operation_epoch_pending"] = json!(operations_pending);
    value["portable_administration"] = if state.operator["portable_admin_unverified"] == true {
        json!({"integrity":"unverified","effective":"safe defaults","next_action":"Review administrative preferences and agent registration; policy content does not require another review"})
    } else {
        json!({"integrity":"unchecked"})
    };
    if state.operator["portable_lineage_unverified"] == true {
        value["portable_lineage"] = json!({"origin":"unverified","effective":"fresh lineage","reason":"The private exact-content record belongs to another recorded lineage"});
    }
    if pending || operations_pending {
        value["retained_policy_authority"] = json!(
            "unverified until native continuity or private reviewed-content consent is established"
        );
        value["retained_administration"] =
            json!({"enabled":state.enforcement.enabled,"integrity":"unverified"});
        value["retained_project_root"] = json!(state.project_root);
        value["project_root"] = json!(context.project_root);
        if value.get("retained_pending_approvals").is_none() {
            value["retained_pending_approvals"] = value["pending_approvals"].clone();
        }
        value["pending_approvals"] = json!(0);
        value["session_grants"] =
            json!({"available":false,"reason":"operation continuity unavailable"});
        value["action_authority"] =
            json!("unavailable until runtime operation continuity is verified");
    }
}

/// The command that repairs integration coverage diagnostics. `shield mcp
/// setup` resumes a confirmed plan; any other drift, such as agent prefix
/// rules, a changed binary, version, MCP entry or hooks, needs the agent set
/// up again, which also resynchronizes its prefix rules.
fn integration_route(coverage: &Value) -> String {
    let pending = coverage["diagnostics"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|d| d == "confirmed setup requires repair");
    if pending {
        "shield mcp setup".into()
    } else {
        agent_setup_route(coverage)
    }
}

/// Setting up the installed agent again, or guided setup for several or none.
pub(super) fn agent_setup_route(coverage: &Value) -> String {
    let agents: Vec<_> = coverage["box_integration"]["agent"]
        .as_str()
        .into_iter()
        .chain(
            coverage["installed_integrations"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|row| row["managed_by"] != "box")
                .filter_map(|row| row["agent"].as_str()),
        )
        .collect();
    match agents.as_slice() {
        [agent] => format!("shield mcp setup --agent {agent}"),
        _ => "shield mcp setup".into(),
    }
}

pub(super) fn policy(context: &Context) -> Result<Value, Error> {
    let mut state = captured_inspection(context)?;
    state.candidates.insert(
        "project".into(),
        super::inspection::source::policy(
            context,
            &state.candidates["project"],
            &state.policy_revision,
        )?,
    );
    let mut value = review::inspect_recorded(context, &state)?;
    value["pending_configuration"] = json!(
        state
            .proposals
            .iter()
            .filter(|row| row.status == "pending")
            .map(crate::review::model::Proposal::summary)
            .collect::<Vec<_>>()
    );
    inspection_location(context, &state, &mut value);
    Ok(value)
}

pub(super) fn policy_prepared(
    context: &Context,
    state: &State,
    evaluator: &crate::configuration::Evaluator,
) -> Result<Value, Error> {
    let mut value = review::inspect_prepared(context, state, evaluator)?;
    value["pending_configuration"] = json!(
        state
            .proposals
            .iter()
            .filter(|row| row.status == "pending")
            .map(crate::review::model::Proposal::summary)
            .collect::<Vec<_>>()
    );
    Ok(value)
}
pub(super) fn root(context: &Context) -> PathBuf {
    PathBuf::from(&context.project_root)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn coverage(agents: &[&str], diagnostics: &[&str]) -> Value {
        json!({"installed_integrations":agents.iter().map(|agent| json!({"agent":agent})).collect::<Vec<_>>(),
            "diagnostics":diagnostics,"repair_needed":!diagnostics.is_empty()})
    }

    #[test]
    fn integration_repairs_route_to_the_command_that_changes_them() {
        let prefix = "agent prefix rules differ from the active policy; run shield mcp setup";
        let pending = "confirmed setup requires repair";
        let cases: [(&[&str], &[&str], &str); 8] = [
            (&["codex"], &[prefix], "shield mcp setup --agent codex"),
            (
                &["codex"],
                &["agent prefix rules file is unreadable"],
                "shield mcp setup --agent codex",
            ),
            (
                &["claude"],
                &[pending, "owned MCP entry missing or changed"],
                "shield mcp setup",
            ),
            (
                &["claude"],
                &["registered binary missing or changed"],
                "shield mcp setup --agent claude",
            ),
            (
                &["codex"],
                &[prefix, "registered binary version differs from this CLI"],
                "shield mcp setup --agent codex",
            ),
            (
                &["claude"],
                &["owned native hook configuration is missing"],
                "shield mcp setup --agent claude",
            ),
            (
                &["claude", "codex"],
                &["owned MCP entry missing or changed"],
                "shield mcp setup",
            ),
            (
                &[],
                &["shield MCP entry is not owned by this enrollment"],
                "shield mcp setup",
            ),
        ];
        for (agents, diagnostics, route) in cases {
            assert_eq!(
                integration_route(&coverage(agents, diagnostics)),
                route,
                "{diagnostics:?}"
            );
        }
    }
}
