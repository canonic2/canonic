//! Configuration inspection and historical activity are separate evidence.
use super::{Registration, State, agents, json, native};
use serde_json::Value;
use std::{collections::BTreeMap, path::Path};

pub(super) fn box_hooks(
    root: &Path,
    registered: Option<&Registration>,
    metadata: &Value,
    diagnostics: &mut Vec<&'static str>,
) -> Vec<Value> {
    let Some(registration) = registered.filter(|r| r.box_definition.is_some()) else {
        return Vec::new();
    };
    let previous_roots: Vec<_> = metadata["portable_launch_roots"]
        .as_array()
        .into_iter()
        .flatten()
        .take(2)
        .filter_map(Value::as_str)
        .map(Path::new)
        .collect();
    agents::ADAPTERS
        .iter()
        .filter_map(|adapter| {
            let agent = adapter.spec().id;
            if !adapter.supports_box_hooks() || registration.agents.contains_key(agent) {
                return None;
            }
            let status =
                match native::published_hooks(root, agent, &registration.binary, &previous_roots) {
                    Ok(None) => return None,
                    Ok(Some(true)) => "installed",
                    Ok(Some(false)) => {
                        diagnostics.push(
                        "Box-published native hooks or agent settings are incomplete or disabled",
                    );
                        "degraded"
                    }
                    Err(_) => {
                        diagnostics.push(
                            "Box-published native hooks are unreadable, conflicting or changed",
                        );
                        "degraded"
                    }
                };
            Some(
                json!({"agent":agent,"surface":"native-hooks","managed_by":"box","status":status,
                "ask_fallback":registration.ask_fallback.get(agent)}),
            )
        })
        .collect()
}

pub(in crate::review) fn enforcement(coverage: &Value) -> &'static str {
    if coverage["repair_needed"] == true {
        "degraded"
    } else if coverage["installed_integrations"]
        .as_array()
        .is_some_and(|rows| !rows.is_empty())
        || !coverage["legacy_interceptor"].is_null()
    {
        "installed"
    } else if !coverage["box_integration"].is_null() {
        "defined-for-box"
    } else {
        "not-installed"
    }
}

pub(super) fn verification(state: &State) -> Value {
    let mut latest = BTreeMap::<&str, Value>::new();
    if state.history_available {
        for event in &state.events {
            let p = &event["provenance"];
            if p["surface"] != "hook"
                || p["project_id"] != state.project_id
                || !matches!(event["kind"].as_str(), Some("decision" | "completion"))
            {
                continue;
            }
            let Some(agent) = p["agent"]
                .as_str()
                .filter(|agent| agents::find(agent).is_some())
            else {
                continue;
            };
            let Some(recorded_at) = event["recorded_at"].as_str() else {
                continue;
            };
            if latest.get(agent).is_none_or(|row| {
                row["recorded_at"]
                    .as_str()
                    .is_none_or(|at| at < recorded_at)
            }) {
                latest.insert(agent, json!({"agent":agent,"recorded_at":recorded_at,
                    "event":p["host_event_name"],"kind":event["kind"],"event_id":event["event_id"]}));
            }
        }
    }
    json!({"state":if latest.is_empty() {"unverified"} else {"observed"},
        "basis":"retained-hook-history","history_available":state.history_available,
        "agents":latest.into_values().collect::<Vec<_>>(),
        "scope":"Recorded hook activity only; current host loading and future interception are not attested. Missing history does not prove hooks are inactive.",
        "inspect":"shield history list"})
}

/// Combine inspected registrations with separately attributed activity evidence.
pub(super) fn complete(report: &mut Value, state: &State, published: Vec<Value>) {
    report["installed_integrations"]
        .as_array_mut()
        .expect("integration rows")
        .extend(published);
    if !report["installed_integrations"]
        .as_array()
        .expect("integration rows")
        .is_empty()
    {
        report["tool_interception"] = json!(if report["repair_needed"] == true {
            "native hook configuration needs repair; see diagnostics"
        } else {
            "native hooks installed; see verification for recorded activity"
        });
    }
    report["verification"] = verification(state);
    if report["verification"]["state"] == "observed" {
        report["live_host_verification"] = json!("observed");
    }
}
