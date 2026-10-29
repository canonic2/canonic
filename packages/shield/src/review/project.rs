//! Enrolled project loading and noninteractive policy/status inspection.
use super::publication::projections::projection_changed;
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
    let state = Store::open(Path::new(&context.project_root))?.read()?;
    state.check(context)?;
    if state.candidates.len() != 1 {
        return Err(Error::Invalid(
            "operator workflow requires project-only policy",
        ));
    }
    Ok(state)
}

pub(super) fn status(context: &Context) -> Result<Value, Error> {
    let state = Store::open(Path::new(&context.project_root))?.read_history()?;
    state.check(context)?;
    let mut value = review::runtime::status(context)?;
    if value["review"]["policy_revision"] != state.policy_revision {
        return Err(Error::StaleSnapshot);
    }
    value["policy_revision"] = json!(state.policy_revision);
    value["installed_packs"] = crate::review::packs::summaries(&metadata(&state));
    value["pending_configuration"] = json!(
        state
            .proposals
            .iter()
            .filter(|p| p.status == "pending")
            .map(crate::review::model::Proposal::summary)
            .collect::<Vec<_>>()
    );
    value["history"] = json!({"enabled":state.history_enabled,"limit":state.history_limit,"available":state.history_available});
    value["pending_approvals"] = json!(super::approvals::pending_count(
        &state,
        super::clock::best_effort_ms()
    ));
    value["coverage"] = integrations::status(context, &state);
    value["path_access"] = path_access::load(&metadata(&state)).map_or_else(
        |error| json!({"error":error.to_string()}),
        |access| json!(access),
    );
    value["repair_needed"] =
        json!(projection_changed(context, &state)? || value["coverage"]["repair_needed"] == true);
    value["repair_route"] = json!(if value["coverage"]["repair_needed"] == true {
        integration_route(&value["coverage"])
    } else {
        "shield policy review".into()
    });
    Ok(value)
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
    let agents: Vec<_> = coverage["installed_integrations"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|row| row["agent"].as_str())
        .collect();
    match agents.as_slice() {
        [agent] => format!("shield mcp setup --agent {agent}"),
        _ => "shield mcp setup".into(),
    }
}

pub(super) fn policy(context: &Context) -> Result<Value, Error> {
    let mut value = review::inspect(context)?;
    let pending = review::proposals(context, None, false)?;
    value["pending_configuration"] = json!(
        pending["proposals"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|row| row["status"] == "pending")
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
