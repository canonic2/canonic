//! Enrolled project loading and noninteractive policy/status inspection.
use super::integrations;
use super::{Context, Error, model::State, storage::Store};
use crate::review;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
mod doctor;
mod outcome;
pub(super) use doctor::report as doctor;
pub(super) use outcome::describe;

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

/// Enrollment and authority metadata, brought up to the policy files as the
/// writer that later commits a review will see them; preparation is explicit.
/// Hooks read the checkpoint alone through the store.
pub(super) fn checkpoint(context: &Context) -> Result<State, Error> {
    let state = Store::open(Path::new(&context.project_root))?.read_following_files()?;
    state.check(context)?;
    Ok(state)
}

/// Current authorization and settings without following or preparing policy.
/// Exact native reviews must remain usable when those files need repair.
pub(super) fn authorization(context: &Context) -> Result<State, Error> {
    let state = Store::open(Path::new(&context.project_root))?.read_checkpoint()?;
    state.check(context)?;
    state.check_operations()?;
    Ok(state)
}

pub(super) fn captured(context: &Context) -> Result<State, Error> {
    let state = Store::open(Path::new(&context.project_root))?.read_following_files()?;
    state.check(context)?;
    Ok(state)
}

/// Immutable record inspection retains source references. Consumers resolve
/// only selected fields against their canonical lock, without preparing policy.
pub(super) fn captured_inspection(context: &Context) -> Result<State, Error> {
    let store = Store::inspect(Path::new(&context.project_root))?;
    let state = store.read_inspection_following_files()?;
    state.check_inspection(context)?;
    Ok(state)
}

pub(super) fn status(context: &Context) -> Result<Value, Error> {
    let (state, pending) =
        Store::inspect(Path::new(&context.project_root))?.read_diagnostic_checkpoint()?;
    state.check_inspection(context)?;
    let mut value = status_from(context, &state);
    value["pack_updates"] = super::pack_updates::inspect(context, &state, None);
    value["policy_files"] = policy_files(root(context).as_path(), &state.operator);
    value["checkpoint_upgrade_pending"] = json!(pending);
    describe(&mut value, false);
    Ok(value)
}

fn policy_files(root: &Path, metadata: &Value) -> Value {
    let cache = super::files::cache_for(metadata).ok();
    match super::files::check(root, cache.as_deref()) {
        Ok(digest) => json!({"digest":digest}),
        Err(error) => {
            let code = match &error {
                Error::PolicyFileConflict(_) => "conflict",
                Error::Policy(crate::policy::Error::Contract(message))
                    if *message == integrations::project_sources::MISSING_PACK =>
                {
                    "missing_pack"
                }
                _ => "unreadable",
            };
            json!({"error":error.to_string(),"code":code})
        }
    }
}

pub(super) fn restore_packs(
    root: &Path,
    fetcher: &canonic_registry_kit::acquisition::Fetcher,
    checkpoint: super::Checkpoint<'_>,
) -> Result<Value, Error> {
    let retained = Store::inspect(root).and_then(|store| store.read_diagnostic_checkpoint());
    let (settings, metadata) = match retained {
        Ok((state, _)) => (state.settings, state.operator),
        Err(Error::Missing) => (
            super::settings::current(root, &super::settings::Legacy::default()),
            Value::Null,
        ),
        Err(error) => return Err(error),
    };
    // A plan recorded under the files' key proves they read cleanly, so the
    // conflict check skips validation on the common, healthy path.
    let cache = super::files::cache_for(&metadata).ok();
    if let Err(error @ Error::PolicyFileConflict(_)) = super::files::check(root, cache.as_deref()) {
        return Err(error);
    }
    let restored = super::files::fetch_missing(root, &settings, fetcher, checkpoint)?;
    let failures: Vec<_> = restored
        .failures
        .into_iter()
        .map(|(release, error)| json!({"release":release,"error":error}))
        .collect();
    let mut value = json!({"project_root":root,"fetched":restored.releases,
        "fetch_failures":failures,"policy_files":policy_files(root, &metadata)});
    if value["policy_files"]["error"].is_string() {
        value["next"] = outcome::shield_action(&value, "doctor", &json!({}), &["doctor"]);
        value["next_action"] = json!("Run Shield doctor to inspect the remaining problems.");
    }
    Ok(value)
}

/// A policy checkout can be inspected before it creates private runtime state.
pub(super) fn source_status(root: &Path, doctor: bool) -> Value {
    let settings = super::settings::current(root, &super::settings::Legacy::default());
    let mut value = json!({"project_root":root,"configuration":configuration_selection(root),
        "project_id":null,"runtime_state":"absent","enabled":settings.enabled(),"enforcement":"unchecked",
        "coverage":{"live_host_verification":"unchecked","tool_interception":"unchecked"},
        "history":{"available":false},"installed_packs":null,
        "policy_files":policy_files(root, &Value::Null),"checks":[]});
    describe(&mut value, doctor);
    value
}

/// Render a checked history capture, also used by doctor's additional probes.
pub(super) fn status_from(context: &Context, state: &State) -> Value {
    let mut value = review::runtime::status_from(state);
    // The one conversion from an earlier release's state, while recent.
    if let Some(notice) = super::storage::recent_upgrade(&state.operator) {
        value["upgrade"] = notice;
    }
    // A checkpoint read holds no policy; the files' digest identifies it.
    if !state.policy_revision.is_empty() {
        value["policy_revision"] = json!(state.policy_revision);
    }
    value["enabled"] = json!(state.settings.enabled());
    value["installed_packs"] = crate::review::packs::summaries(&metadata(state));
    super::page::preview(
        &mut value["installed_packs"],
        "packs",
        json!({"tool":"pack_list"}),
    );
    value["pending_configuration_total"] = json!(
        state
            .proposals
            .iter()
            .filter(|p| p.status == "pending")
            .count()
    );
    value["pending_configuration_list"] = json!({"tool":"policy_show","status":"pending"});
    value["pending_configuration"] = json!(
        state
            .proposals
            .iter()
            .filter(|p| p.status == "pending")
            .take(16)
            .map(crate::review::model::Proposal::summary)
            .collect::<Vec<_>>()
    );
    value["pending_capture_freshness"] = json!("unchecked; apply revalidates exact reviewed files");
    value["history"] = history_metadata(context, state);
    value["pending_approvals"] = json!(super::approvals::pending_count(state));
    value["session_grants"] = super::session_grants::summary(state);
    value["coverage"] = integrations::status(context, state);
    value["coverage"]["verification"] = json!({"state":"unchecked","basis":"record-only-inspection","history_available":value["history"]["available"],"agents":[],"inspect":"shield history list"});
    value["coverage"]["live_host_verification"] = json!("unchecked");
    value["enforcement"] = json!(if state.settings.enabled() {
        integrations::enforcement(&value["coverage"])
    } else {
        "disabled"
    });
    value["path_access"] = json!(super::settings::path_access(&state.settings));
    // Exact projection hashing belongs to explicit review/mutation. Even a
    // recorded digest does not make scanning all pack source a metadata probe.
    value["project_file_inspection"] = json!("unchecked");
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
        let path = crate::layout::state(Path::new(&context.project_root))
            .join(format!("history-{digest}.error"));
        super::storage::objects::read_file(&path)
            .ok()
            .and_then(|file| file.metadata().ok())
            .is_some_and(|metadata| metadata.len() <= 1024 * 1024)
    });
    json!({"enabled":state.settings.history_enabled,"limit":state.settings.history_limit,
        "available":if incomplete {json!(false)}else{Value::Null},
        "integrity":if incomplete {"incomplete"}else{"unchecked"},"inspect":"shield history list"})
}

/// The folder being inspected, as discovered now. Checkpoints record no
/// location, so a moved, renamed or copied project needs no follow-up.
pub(super) fn inspection_location(context: &Context, _state: &State, value: &mut Value) {
    value["project_root"] = json!(context.project_root);
    value["configuration"] = configuration_selection(Path::new(&context.project_root));
}

/// The selected shared configuration file, where it came from and the default
/// files it shadows, relative to the project when inside it.
pub(crate) fn configuration_selection(root: &Path) -> Value {
    let canonical = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let shown = |path: &Path| {
        path.strip_prefix(&canonical)
            .or_else(|_| path.strip_prefix(root))
            .map_or_else(
                |_| path.display().to_string(),
                |relative| relative.display().to_string(),
            )
    };
    match crate::layout::select(root) {
        Ok(selection) => json!({"file":shown(&selection.file),"source":selection.source.label(),
            "shadowed":selection.shadowed.iter().map(|path| shown(path)).collect::<Vec<_>>()}),
        Err(error) => json!({"source":"explicit","error":error.to_string()}),
    }
}

/// The command that repairs integration coverage diagnostics. `shield mcp
/// setup` resumes a confirmed plan; any other drift, such as agent prefix
/// rules, a changed binary, version, MCP entry or hooks, needs the agent set
/// up again, which also resynchronizes its prefix rules.
fn integration_route(coverage: &Value) -> String {
    if super::integrations::setup_pending(coverage) {
        "shield mcp setup".into()
    } else {
        agent_setup_route(coverage)
    }
}

/// Setting up the installed agent again, or guided setup for several or none.
pub(super) fn agent_setup_route(coverage: &Value) -> String {
    let agents: Vec<_> = coverage["host_integration"]["agent"]
        .as_str()
        .into_iter()
        .chain(
            coverage["installed_integrations"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|row| row["managed_by"].is_null())
                .filter_map(|row| row["agent"].as_str()),
        )
        .collect();
    match agents.as_slice() {
        [agent] => format!("shield mcp setup --agent {agent}"),
        _ => "shield mcp setup".into(),
    }
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
