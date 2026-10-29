//! Human refusal summaries; actionable diagnostics stay in the agent channel.
use super::{Reply, Value};

pub(super) fn denial(mut reply: Reply, receipt: &Value) -> Reply {
    let code = receipt["code"].as_str().unwrap_or_default();
    let explanation = match code {
        _ if crate::agent_message::rewrite_detail(&receipt["diagnostics"]).is_some() => "The command could not be checked in this form. The agent has instructions to revise it.".into(),
        "policy-deny" if receipt["outside_path"]["path"].is_string() => format!(
            "`{}` is outside the approved folders. Additional folder access requires your approval.",
            receipt["outside_path"]["path"].as_str().unwrap_or_default()
        ),
        "policy-deny" if receipt["diagnostics"]["requirement_checks"]
            .as_array()
            .is_some_and(|checks| checks.first().is_some_and(|check| check["status"] != "met")) => format!(
            "{} The agent has instructions to revise the request.",
            crate::agent_message::failure(&receipt["diagnostics"]).unwrap_or_default()
        ),
        "policy-deny" => crate::agent_message::denial_detail(&receipt["diagnostics"]).unwrap_or_else(|| "The active policy does not allow this request. Inspect the matching rule before proposing a policy change.".into()),
        "request-rewrite" => "The command could not be checked in this form. The agent has instructions to revise it.".into(),
        "protected-access" if receipt["protected_path"].as_str().is_none() => "This request reaches protected files or directories.".into(),
        "protected-access" => {
            let path = receipt["protected_path"]
                .as_str()
                .unwrap_or("this path");
            let why = match receipt["protected_group"].as_str() {
                Some("state" | "folder" | "ignore") => "it protects Shield's review history and cache",
                Some("policy") => "it protects the active policy",
                Some("working-root") => "it is a temporary or session working-folder root",
                _ => "it protects the agent's security settings",
            };
            format!("Access to `{path}` is restricted because {why}.")
        }
        "protected-search" => "The search could read protected files. The agent has instructions to narrow it.".into(),
        "search-footprint-unavailable" => "The files affected by this search could not be verified. The agent has instructions to narrow it.".into(),
        "sensitive-read" => "These settings may contain credentials. This host cannot request permission to read them here.".into(),
        "unsupported-operation" => "This tool or request form is not supported.".into(),
        "operator-cli" => "Agents must use Shield's policy tools to request changes. Any change requires your approval.".into(),
        "invalid-host-input" | "invalid-host-event" | "invalid-input" => "The tool input is invalid. The agent must correct the request.".into(),
        "invalid-context" => "The session's working directory is not an existing folder inside this Shield project.".into(),
        "runtime-busy" => "Shield state was briefly locked by another update. Retrying the request can succeed.".into(),
        "handoff-failed" | "project-changed" => "The project's current authorization could not be verified. Check Shield's project status before retrying.".into(),
        "compiled-cache-unavailable" => "The policy cache is unavailable. Run `shield cache prepare` in your terminal before retrying.".into(),
        _ => "Authorization could not be verified. Run `shield doctor` in your terminal before retrying.".into(),
    };
    let context = reply.agent_context.take();
    reply.agent_context = Some(context.map_or_else(
        || reply.reason.clone(),
        |context| format!("{} {context}", reply.reason),
    ));
    reply.reason = format!("Shield: blocked. {explanation}");
    reply
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::{Decision, Event};
    use serde_json::json;

    #[test]
    fn denial_channels_keep_proposal_payloads_out_of_user_messages() {
        let receipt =
            json!({"code":"policy-deny", "outside_path":{"path":"/srv/acme/tasks/job.output"}});
        let instructions = "Call policy_propose with change {\"kind\":\"access_path_add\",\"path\":\"/srv/acme/tasks\"} and source_entry_id request-acme; submit apply_arguments to policy_apply.";
        for agent in ["claude", "codex"] {
            let reply = denial(
                Reply {
                    decision: Decision::Deny,
                    reason: instructions.into(),
                    agent_context: None,
                },
                &receipt,
            );
            let wire = crate::agents::find(agent)
                .unwrap()
                .reply(Event::Before, &reply);
            let user = wire["hookSpecificOutput"]["permissionDecisionReason"]
                .as_str()
                .unwrap();
            assert!(user.starts_with("Shield: blocked."));
            assert!(user.contains("/srv/acme/tasks/job.output"));
            for private in [
                "{",
                "source_entry_id",
                "policy_propose",
                "policy_apply",
                "apply_arguments",
                "request-acme",
            ] {
                assert!(!user.contains(private), "{user}");
            }
            assert_eq!(
                wire["hookSpecificOutput"]["additionalContext"],
                instructions
            );
        }
    }

    #[test]
    fn native_failure_messages_keep_diagnostics_in_agent_context() {
        for code in [
            "policy-deny",
            "unsupported-operation",
            "operator-cli",
            "protected-access",
            "sensitive-read",
            "search-footprint-unavailable",
            "protected-search",
            "invalid-input",
            "handoff-failed",
            "compiled-cache-unavailable",
            "runtime-busy",
            "invalid-context",
            "protection-failure",
        ] {
            let reply = super::super::deny(code);
            assert!(reply.reason.starts_with("Shield: blocked."));
            assert!(!reply.reason.contains("policy_propose"));
            assert!(!reply.reason.contains("policy_apply"));
            assert!(!reply.reason.contains("{\""));
            assert!(reply.agent_context.is_some());
        }
    }

    #[test]
    fn transient_locks_retry_and_unusable_working_directories_name_the_folder() {
        let busy = super::super::deny("runtime-busy");
        let context = busy.agent_context.unwrap();
        assert!(context.contains("retry the request"), "{context}");
        assert!(!context.contains("doctor") && !busy.reason.contains("doctor"));
        let cwd = super::super::deny("invalid-context");
        let context = cwd.agent_context.unwrap();
        assert!(context.contains("working directory"), "{context}");
        assert!(!context.contains("tool input") && !cwd.reason.contains("tool input"));
    }

    #[test]
    fn claude_feedback_omits_empty_context_and_keeps_meaningful_guidance() {
        let adapter = crate::agents::find("claude").unwrap();
        for decision in [Decision::Allow, Decision::Ask, Decision::Deny] {
            for context in [None, Some(""), Some(" \n ")] {
                let wire = adapter.reply(
                    Event::Before,
                    &Reply {
                        decision,
                        reason: String::new(),
                        agent_context: context.map(str::to_owned),
                    },
                );
                assert!(
                    wire["hookSpecificOutput"]
                        .get("additionalContext")
                        .is_none()
                );
            }
        }
        let wire = adapter.reply(
            Event::Before,
            &Reply {
                decision: Decision::Allow,
                reason: String::new(),
                agent_context: Some("Inspect the script before proposing a rule.".into()),
            },
        );
        assert_eq!(
            wire["hookSpecificOutput"]["additionalContext"],
            "Inspect the script before proposing a rule."
        );
    }

    #[test]
    fn allowed_claude_feedback_keeps_automatic_proposal_guidance_in_context() {
        let instructions = "The Ask fallback allows this request. Propose {\"kind\":\"pack_add\"} with source_entry_id request-acme.";
        let reply = Reply {
            decision: Decision::Allow,
            reason: instructions.into(),
            agent_context: None,
        };
        let wire = crate::agents::find("claude")
            .unwrap()
            .reply(Event::Before, &reply);
        assert_eq!(wire["hookSpecificOutput"]["permissionDecision"], "allow");
        assert_eq!(
            wire["hookSpecificOutput"]["permissionDecisionReason"],
            "Shield: allowed."
        );
        assert_eq!(
            wire["hookSpecificOutput"]["additionalContext"],
            instructions
        );
    }
}
