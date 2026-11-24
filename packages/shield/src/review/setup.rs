//! The project setup defaults shared by the CLI and MCP.
use super::{Error, integrations};
use serde_json::{Value, json};
use std::path::Path;

pub(crate) const TOOL: &str = "setup";
pub(crate) const DESCRIPTION: &str = "Set up or repair Shield for this connection's project. Reuse saved integration choices or detect the project's publisher or agent. New projects use the Recommended baseline by default. Changes require human confirmation; unchanged setup does not.";

pub(crate) fn schema() -> Value {
    let ids: Vec<_> = integrations::host_definition::HOSTS
        .iter()
        .map(|host| host.id)
        .chain(crate::agents::ADAPTERS.iter().map(|a| a.spec().id))
        .collect();
    json!({"type":"object","additionalProperties":false,"properties":{
        "agents":{"type":"array","minItems":1,"maxItems":16,"uniqueItems":true,"items":{"type":"string","enum":ids}},
        "profile":{"type":"string","enum":["loose","recommended","strict"],"description":"Baseline profile for a new project."}
    }})
}

pub(crate) fn first_run_target_allowed(project: &Path, home: Option<&Path>) -> bool {
    project.parent().is_some()
        && home.is_none_or(|home| {
            home.canonicalize()
                .is_ok_and(|home| !home.starts_with(project))
        })
}

pub(crate) fn operator_action(root: &Path, arguments: &Value) -> Value {
    let mut argv = vec![json!("--project"), json!(root), json!("setup")];
    for agent in arguments["agents"].as_array().into_iter().flatten() {
        argv.extend([json!("--agent"), agent.clone()]);
    }
    if let Some(profile) = arguments.get("profile") {
        argv.extend([json!("--profile"), profile.clone()]);
    }
    json!({"command":"shield","argv":argv})
}

pub(crate) fn agents(
    root: &Path,
    metadata: &Value,
    requested: &[String],
    connected: Option<&str>,
) -> Result<Vec<String>, Error> {
    if !requested.is_empty() {
        return Ok(requested.to_vec());
    }
    let saved = saved_agents(metadata)?;
    if saved.iter().any(|agent| agent == "hub") {
        // OpenCode can keep its separately owned plugin alongside Hub. Repeat
        // the publisher route without submitting a competing mixed selection.
        return Ok(vec!["hub".into()]);
    }
    if !saved.is_empty() {
        return Ok(saved);
    }
    if let Some(host) = integrations::host_definition::project_host(root) {
        return Ok(vec![host.id.into()]);
    }
    if let Some(agent) = connected.filter(|id| crate::agents::find(id).is_some()) {
        return Ok(vec![agent.into()]);
    }
    let installed = crate::agents::detection::installed();
    choose_detected(&installed)
}

fn choose_detected(installed: &[&str]) -> Result<Vec<String>, Error> {
    match installed {
        [agent] => Ok(vec![(*agent).into()]),
        [] => Err(Error::Invalid(
            "no supported agent detected; specify --agent for shield setup or agents for the setup tool",
        )),
        agents => Err(Error::Ambiguous {
            reason: "several agents detected; choose a target with --agent for shield setup or agents for the setup tool",
            alternatives: agents.iter().map(|id| (*id).into()).collect(),
            total: agents.len(),
        }),
    }
}

pub(in crate::review) fn saved_agents(metadata: &Value) -> Result<Vec<String>, Error> {
    Ok(
        integrations::registration(metadata)?.map_or_default(|registration| {
            registration
                .definition
                .as_ref()
                .map(|definition| definition.host().id.to_owned())
                .into_iter()
                .chain(registration.agents.into_keys())
                .collect()
        }),
    )
}

pub(crate) fn receipt(root: &Path, mut status: Value, outcome: &str) -> Value {
    if status["setup_error"].is_string() {
        if status["pending_setup"].is_object() {
            status["next_action"] = json!(
                "Run setup again to resume the approved changes. If the conflict persists, use the returned pending setup review or cancel action."
            );
        }
        return status;
    }
    let coverage = &status["coverage"];
    let hub = coverage["host_integration"]["agent"] == "hub";
    let hub_hooks = coverage["installed_integrations"]
        .as_array()
        .is_some_and(|rows| {
            rows.iter()
                .any(|row| row["managed_by"] == "hub" && row["status"] == "installed")
        });
    let (next_action, next) = if hub
        && (outcome != "unchanged"
            || !hub_hooks
            || coverage["host_integration"]["status"] == "degraded")
    {
        (
            "Publish the Shield definition with Hub setup in this project.",
            json!({"server":"hub","tool":"setup","arguments":{"project":root},"command":"hub","argv":["--project",root,"setup"]}),
        )
    } else if coverage["host_integration"]["agent"] == "box" && outcome != "unchanged" {
        (
            "Run Box setup or Box config sync to publish the Shield definition in this project.",
            json!({"server":"box","tool":"config_sync","arguments":{"project":root,"servers":["shield"]}}),
        )
    } else if status["enabled"] == false {
        (
            "Shield is paused. Use enable to review resuming enforcement.",
            json!({"server":"shield","tool":"enable","arguments":{},"command":"shield","argv":["--project",root,"enable"]}),
        )
    } else if outcome != "unchanged" {
        (
            "Reload the affected agents to load Shield, then check Shield doctor.",
            json!({"server":"shield","tool":"doctor","arguments":{}}),
        )
    } else {
        ("", Value::Null)
    };
    json!({"setup_status":outcome,"project":root,"project_id":status["project_id"],"policy_revision":status["policy_revision"],"enabled":status["enabled"],"enforcement":status["enforcement"],"cache_location":status["cache_location"],"next_action":next_action,"next":next})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn setup_infers_one_detected_agent_and_refuses_ambiguous_or_missing_targets() {
        assert_eq!(choose_detected(&["codex"]).unwrap(), ["codex"]);
        assert!(
            matches!(choose_detected(&["claude", "codex"]), Err(Error::Ambiguous { alternatives, total: 2, .. }) if alternatives == ["claude", "codex"])
        );
        assert!(matches!(choose_detected(&[]), Err(Error::Invalid(_))));
    }
}
