//! Shared status and doctor outcomes with project-bound recovery.
use serde_json::{Value, json};

pub(super) fn shield_action(value: &Value, tool: &str, arguments: &Value, argv: &[&str]) -> Value {
    let project = &value["project_root"];
    let mut command = vec![json!("--project"), project.clone()];
    command.extend(argv.iter().map(|arg| json!(arg)));
    json!({"server":"shield","tool":tool,"project":project,"arguments":arguments,
        "command":"shield","argv":command})
}

pub(in crate::review) fn describe(value: &mut Value, doctor: bool) {
    let coverage = &value["coverage"];
    let published = coverage["installed_integrations"]
        .as_array()
        .is_some_and(|rows| rows.iter().any(|row| row["status"] == "installed"))
        || coverage["legacy_interceptor"]["status"] == "installed";
    let action = |tool, argv: &[&str]| shield_action(value, tool, &json!({}), argv);
    let pending =
        value["pending_setup"].is_object() || crate::review::integrations::setup_pending(coverage);
    let (summary, guidance, next) = if value["policy_files"]["error"].is_string() {
        match value["policy_files"]["code"].as_str() {
            Some("missing_pack") => (
                "Shield policy is unavailable because a locked pack is missing.",
                "Restore the locked pack artifacts.",
                action("pack_restore", &["pack", "restore"]),
            ),
            _ => (
                "Shield policy is unavailable.",
                if doctor {
                    "Resolve the reported policy file problem, then rerun doctor."
                } else {
                    "Use Shield doctor to inspect the policy file problem."
                },
                action("doctor", &["doctor"]),
            ),
        }
    } else if value["enabled"] == false {
        (
            "Shield enforcement is paused. Protected controls remain guarded.",
            "Use enable to review resuming enforcement.",
            action("enable", &["enable"]),
        )
    } else if pending {
        (
            "Previously approved Shield setup needs attention.",
            "Review the interrupted setup with mcp_setup.",
            shield_action(
                value,
                "mcp_setup",
                &json!({"pending":"review"}),
                &["mcp", "setup", "--pending", "review"],
            ),
        )
    } else if let Some(notice) = publisher_notice(value) {
        notice
    } else if value["repair_needed"] == true {
        if doctor {
            (
                "Shield agent configuration needs attention.",
                "Review the named integration repair.",
                native_repair_action(value),
            )
        } else {
            (
                "Shield agent configuration needs attention.",
                "Use Shield doctor for the repair action.",
                action("doctor", &["doctor"]),
            )
        }
    } else if value["runtime_state"] == "absent" {
        (
            "Shield has no local runtime state. Agent integration is unverified.",
            "Use Shield setup to check and connect agents.",
            action("setup", &["setup"]),
        )
    } else if value["enforcement"] == "not-installed" {
        (
            "No Shield agent integration is configured.",
            "Use Shield setup to connect an agent.",
            action("setup", &["setup"]),
        )
    } else {
        (
            "Shield policy is enabled and agent hooks are configured.",
            "",
            Value::Null,
        )
    };
    value["summary"] = json!(summary);
    value["next_action"] = json!(guidance);
    value["next"] = next;
    value["enforcement_evidence"] = json!({
        "saved_policy":if value["policy_files"]["error"].is_string() {"unavailable"}
            else if value["enabled"] == false {"paused"} else {"enabled"},
        "published_hooks":if value["repair_needed"] == true {"needs_attention"}
            else if published {"configured"}
            else if value["runtime_state"] == "absent" {"unchecked"} else {"absent"},
        "host_loading":"unchecked",
        "interception":"unchecked"
    });
}

fn native_repair_action(value: &Value) -> Value {
    let mut arguments = value["repair_arguments"].clone();
    if !arguments.is_object() {
        let route = value["repair_route"].as_str().unwrap_or_default();
        arguments = route
            .strip_prefix("shield mcp setup --agent ")
            .map_or_else(|| json!({}), |agent| json!({"agents":[agent]}));
    }
    let mut argv = vec!["mcp", "setup"];
    for agent in arguments["agents"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        argv.extend(["--agent", agent]);
    }
    if arguments["repair"] == true {
        argv.push("--repair");
    }
    shield_action(value, "mcp_setup", &arguments, &argv)
}

fn publisher_notice(value: &Value) -> Option<(&'static str, &'static str, Value)> {
    let coverage = &value["coverage"];
    let host = &coverage["host_integration"];
    let project = &value["project_root"];
    let managed_drift: Vec<_> = coverage["installed_integrations"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|row| row["managed_by"] == host["agent"] && row["status"] == "degraded")
        .filter_map(|row| row["agent"].as_str())
        .collect();
    Some(
        if host["agent"] == "hub"
            && (value["enforcement"] == "defined-for-hub"
                || host["definition_status"] == "installed" && host["status"] == "degraded")
        {
            (
                "Hub publication pending.",
                "Run Hub setup for this project.",
                json!({"server":"hub","tool":"setup","project":project,"arguments":{"project":project},
                "command":"hub","argv":["--project",project,"setup"]}),
            )
        } else if host["agent"] == "hub"
            && host["status"] == "installed"
            && !managed_drift.is_empty()
        {
            let mut argv = vec![
                json!("--project"),
                project.clone(),
                json!("setup"),
                json!("--repair-hooks"),
            ];
            for agent in &managed_drift {
                argv.extend([json!("--agent"), json!(agent)]);
            }
            (
                "Hub-published Shield hooks need repair.",
                "Review hook repair through Hub setup.",
                json!({"server":"hub","tool":"setup","project":project,
                "arguments":{"project":project,"repair_hooks":true,"agents":managed_drift},
                "command":"hub","argv":argv}),
            )
        } else if value["enforcement"] == "defined-for-box" {
            (
                "Box publication pending.",
                "Run Box config sync for this project.",
                json!({"server":"box","tool":"config_sync","project":project,
                "arguments":{"project":project,"servers":["shield"]}}),
            )
        } else if host["agent"] == "box"
            && host["status"] == "installed"
            && !managed_drift.is_empty()
        {
            (
                "Box-published Shield hooks need repair.",
                "Review hook repair through Box config sync.",
                json!({"server":"box","tool":"config_sync","project":project,
                "arguments":{"project":project,"repair_hooks":true,"agents":managed_drift}}),
            )
        } else {
            return None;
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_only_status_keeps_registration_unverified() {
        use assert_fs::prelude::*;

        let root = assert_fs::TempDir::new().unwrap();
        root.child(".shield/config.yml")
            .write_str("schema_version: 1\npacks: []\n")
            .unwrap();
        let status = super::super::source_status(root.path(), false);
        assert_eq!(status["runtime_state"], "absent");
        assert_eq!(status["coverage"]["tool_interception"], "unchecked");
        assert_eq!(
            status["enforcement_evidence"]["published_hooks"],
            "unchecked"
        );
        assert_eq!(status["next"]["tool"], "setup");
        assert!(!root.path().join(".shield/state").exists());
    }

    #[test]
    fn diagnostics_route_unpublished_hub_and_distinguish_live_evidence() {
        let mut report = json!({"project_root":"/srv/acme","enabled":true,
            "policy_files":{"digest":"acme"},"enforcement":"defined-for-hub",
            "coverage":{"host_integration":{"agent":"hub","status":"installed"}}});
        describe(&mut report, false);
        assert_eq!(report["next"]["server"], "hub");
        assert_eq!(report["next"]["arguments"], json!({"project":"/srv/acme"}));
        assert_eq!(report["enforcement_evidence"]["published_hooks"], "absent");

        report["enforcement"] = json!("installed");
        report["coverage"]["installed_integrations"] =
            json!([{"agent":"codex","managed_by":"hub","status":"installed"}]);
        describe(&mut report, true);
        assert!(report["next"].is_null());
        assert_eq!(
            report["enforcement_evidence"],
            json!({"saved_policy":"enabled",
            "published_hooks":"configured","host_loading":"unchecked","interception":"unchecked"})
        );

        report["enabled"] = json!(false);
        report["enforcement"] = json!("disabled");
        describe(&mut report, false);
        assert_eq!(report["enforcement_evidence"]["saved_policy"], "paused");
        assert_eq!(
            report["enforcement_evidence"]["published_hooks"],
            "configured"
        );
        assert_eq!(report["next"]["tool"], "enable");
        assert_eq!(
            report["next"]["argv"],
            json!(["--project", "/srv/acme", "enable"])
        );
    }
}
