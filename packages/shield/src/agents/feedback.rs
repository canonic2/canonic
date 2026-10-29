//! Shared native feedback, proposals and approval notes, independent of vendor JSON.
use super::{AgentAdapter, Decision, Event, Reply};
use crate::enforcement::protocol::NativeApproval;
use serde_json::{Value, json};
use std::path::Path;
mod packs;
mod presentation;
// Agents change policy only by proposal; the user reviews and activates it.
const PROPOSE: &str = "If the user requests an allowance, read the rule grammar with shield_docs_search and shield_docs_read, then propose a bounded reusable rule or outside-folder access with policy_propose; copy the returned apply_arguments object unchanged into policy_apply for approval.";
// Stable marker for protected-access feedback across groups.
const PROTECTED_ACCESS: &str = "Shield: protected path.";

pub(super) fn deny(code: &str) -> Reply {
    presentation::denial(raw_deny(code), &serde_json::json!({"code":code}))
}

fn raw_deny(code: &str) -> Reply {
    let reason = match code {
        "policy-deny" => format!("Shield: denied by active policy. Do not work around this; inspect the matching rule with rule_list if useful. {PROPOSE}"),
        // A rule cannot cover a tool Shield does not model, so no proposal helps.
        "unsupported-operation" => "Shield: unsupported tool; blocked. Continue without it, or explain to the user why it is needed.".to_owned(),
        "operator-cli" => "Shield: inspect with the status or rule_list tools; propose changes with policy_propose and copy the returned apply_arguments object unchanged into policy_apply for approval.".to_owned(),
        "protected-access" => format!("{PROTECTED_ACCESS} Never read or change .shield/review or the cache. Propose policy changes with policy_propose; direct policy/settings edits require the user's shield protection set choice. Do not bypass protection."),
        "sensitive-read" => "Shield: settings may contain credentials; this callback cannot request read approval. Continue without them unless essential; then ask the user to inspect them in a terminal.".to_owned(),
        "search-footprint-unavailable" => "Shield: the files this search reaches could not be verified. Search a narrower path with fewer entries. In a shell rg, --no-ignore without glob filters keeps hidden files out.".to_owned(),
        "protected-search" => "Shield: this search could read protected files. Search a narrower path that excludes .shield and agent settings. In a shell rg, --files lists names only, and --no-ignore without glob filters keeps a content search to visible files.".to_owned(),
        "invalid-host-input" | "invalid-host-event" | "invalid-input" => "Shield: invalid request. Correct the tool input before retrying.".to_owned(),
        "invalid-context" => "Shield: the session's working directory is not an existing folder inside this Shield project. Work from the project folder, or ask the user to start the session there.".to_owned(),
        "runtime-busy" => "Shield: Shield state is briefly locked; retry the request.".to_owned(),
        "handoff-failed" | "project-changed" => "Shield: current authorization could not be verified. Inspect the status tool and retry only after the user resolves any protection problem.".to_owned(),
        "compiled-cache-unavailable" => "Shield: compiled policy is unavailable. Ask the user to run `shield cache prepare` in their terminal, then retry.".to_owned(),
        _ => format!("Shield blocked this tool ({code}). Ask the user to run `shield doctor` before retrying."),
    };
    Reply {
        decision: Decision::Deny,
        reason,
        agent_context: None,
    }
}

fn approval(adapter: &dyn AgentAdapter) -> Reply {
    Reply {
        decision: Decision::Ask,
        agent_context: None,
        reason: format!(
            "Shield: approval required. {}",
            adapter.spec().approval_action
        ),
    }
}

// An Ask explained only by source Shield cannot inspect goes back to the agent
// to rewrite; the revised request then reaches its actual decision.
fn rewrite(receipt: &Value) -> Option<Reply> {
    crate::agent_message::rewrite_detail(&receipt["diagnostics"]).map(|detail| Reply {
        decision: Decision::Deny,
        agent_context: None,
        reason: format!("Shield: rewrite this request before it can be decided. {detail}"),
    })
}

fn permitted(reason: &str) -> Reply {
    Reply {
        decision: Decision::Allow,
        reason: reason.into(),
        agent_context: None,
    }
}

/// One sentence naming the protected path, what the request does to it and
/// what the agent can do instead. Policy files and agent settings are blocked
/// only for changes, since reading them is ordinary policy or asks the user;
/// any command Shield cannot model counts as a change.
fn protected_access(path: &str, group: Option<&str>) -> String {
    let path = format!("`{path}`");
    match group {
        Some("working-root") => format!(
            "{PROTECTED_ACCESS} This request changes {path}, a working-folder root. Use files and subdirectories inside it instead."
        ),
        Some("state") => format!(
            "{PROTECTED_ACCESS} {path} is protected review state or cache. Never read or change it; continue without it."
        ),
        Some("folder") => format!(
            "{PROTECTED_ACCESS} This request is treated as a change to {path}, which holds protected review state. Continue without changing it."
        ),
        Some("ignore") => format!(
            "{PROTECTED_ACCESS} This request is treated as a change to {path}, which keeps review state out of Git. Continue without changing it."
        ),
        Some("policy") => format!(
            "{PROTECTED_ACCESS} This command counts as a change to {path}, a policy file Shield protects. Propose policy changes with policy_propose, or ask the user to allow direct edits with `shield protection set`."
        ),
        _ => format!(
            "{PROTECTED_ACCESS} This request is treated as an edit to {path}, which can disable agent protection. Ask the user to edit it or permit edits with `shield protection set`."
        ),
    }
}

fn unsupported(adapter: &dyn AgentAdapter, event: Event, operation: &Value) -> Reply {
    // Tool names are host data. Render only bounded identifiers, never inputs,
    // URLs or control/markup characters in routine permission messages.
    let tool = operation["name"].as_str().filter(|name| {
        !name.is_empty()
            && name.len() <= 128
            && name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"_-.:".contains(&byte))
    });
    let name = tool.map_or_else(|| "this tool".to_owned(), |name| format!("`{name}`"));
    if adapter.approval(event) == NativeApproval::Prompt {
        return Reply {
            decision: Decision::Ask,
            agent_context: None,
            reason: format!(
                "Shield: approval required for {name}. Shield cannot check what this tool does. {}",
                adapter.spec().approval_action
            ),
        };
    }
    let help = tool.and_then(|tool| adapter.unsupported_help(tool)).unwrap_or(
        "Policy changes cannot add tool support. Continue without it, or tell the user what you needed it for.",
    );
    Reply {
        decision: Decision::Deny,
        agent_context: None,
        reason: format!("Shield: blocked {name}: unsupported request form. {help}"),
    }
}

/// Completion evidence is not proof of individual operator approval.
pub(super) fn approval_note(
    root: &Path,
    adapter: &dyn AgentAdapter,
    session: &str,
    id: &str,
) -> Option<String> {
    if !adapter.spec().approval_notes {
        return None;
    }
    let explanation = crate::review::observations::asked_explanation(root, session, id)?;
    let mut note = "Shield: this Ask call completed through the host; that alone does not prove individual approval.".to_owned();
    if let Some(reason) = explanation["guidance"][0]["message"].as_str() {
        note.push_str(" It asked because ");
        note.push_str(reason.trim_end_matches('.'));
        note.push('.');
    }
    note.push_str(" The same request will ask again unless policy changes. After the task, inspect suggestion_list for eligible recurring reviews.");
    Some(note)
}

/// Native permission prompts address the user. Alternatives and proposal payloads
/// belong in agent feedback, including the post-approval note above.
fn approval_prompt(
    root: &Path,
    adapter: &dyn AgentAdapter,
    receipt: &Value,
    operation: &Value,
) -> Reply {
    let mut value = approval(adapter);
    let guidance = &receipt["diagnostics"]["guidance"][0];
    if let Some(detail) = guidance["message"].as_str() {
        value.reason = format!(
            "Shield: approval required. {detail} {}",
            adapter.spec().approval_action
        );
    }
    let offer = draft(
        root,
        adapter,
        guidance,
        operation,
        None,
        receipt["history"]["request_id"].as_str(),
    );
    let detail = receipt["diagnostics"]["message"]
        .as_str()
        .unwrap_or_default();
    if !detail.is_empty() || offer.is_some() {
        let offer = offer.unwrap_or_default();
        value.agent_context = Some(format!(
            "Shield: policy asks for approval for this request. {detail} {offer}"
        ));
    }
    value
}

/// Agent-facing Ask feedback when the host cannot open a permission prompt.
fn asked(root: &Path, adapter: &dyn AgentAdapter, receipt: &Value, operation: &Value) -> Reply {
    let guidance = &receipt["diagnostics"]["guidance"][0];
    let entry = receipt["history"]["request_id"].as_str();
    let mut value = approval(adapter);
    // The same review route is available to native agents on the initial Ask.
    let offer = draft(root, adapter, guidance, operation, None, entry);
    let mut detail = receipt["diagnostics"]["message"].as_str();
    // The offer replaces generic advice to propose a change.
    let generic = guidance["alternatives"].as_array().is_none_or(|items| {
        items
            .iter()
            .all(|item| item.as_str().is_some_and(|s| s.contains("policy_propose")))
    });
    if offer.is_some() && generic {
        detail = guidance["message"].as_str();
    }
    if detail.is_some() || offer.is_some() {
        let action = offer.map_or_else(
            || adapter.spec().approval_action.to_owned(),
            |offer| format!("{} {offer}", adapter.spec().approval_action),
        );
        let detail = detail.map_or_else(String::new, |detail| format!("{detail} "));
        value.reason = format!("Shield: approval required. {detail}{action}");
    }
    if receipt["ask_fallback_reason"] == "codex-prefix-rules" {
        value.reason = value.reason.replacen(
            "Shield: approval required. ",
            "Shield: approval required. Codex may run this outside its sandbox without asking, so the Ask fallback does not apply. ",
            1,
        );
    }
    value
}

/// A candidate change for operator review: a folder, web request, relevant
/// registry pack, or guidance for drafting a reusable command rule.
/// Pack relevance is not permission.
fn draft(
    root: &Path,
    adapter: &dyn AgentAdapter,
    guidance: &Value,
    operation: &Value,
    outside: Option<&Value>,
    entry: Option<&str>,
) -> Option<String> {
    use crate::proposal_draft;
    if outside.is_none()
        && let Some(offer) = packs::offer(root, adapter, guidance, entry)
    {
        return Some(offer);
    }
    if outside.is_none() {
        // Blocked web access retains its immediate operator-review route.
        // Fallback-allowed requests use the later recurrence queue instead.
        let web = if operation["kind"] == "web" {
            if operation["action"] == "search" {
                Some(proposal_draft::web_search())
            } else {
                operation["url"]
                    .as_str()
                    .and_then(proposal_draft::host_web_fetch)
            }
        } else {
            guidance["fetch_url"]
                .as_str()
                .and_then(proposal_draft::web_fetch)
        };
        if let Some(change) = web {
            return Some(proposal_draft::offer(&proposal_draft::arguments(
                &change, entry,
            )));
        }
        let source = entry.map_or_else(String::new, |id| format!(" source_entry_id {id}."));
        return Some(format!(
            "After the task, inspect suggestion_list for eligible recurring reviews and canonical authoring examples. No permission is granted.{source}"
        ));
    }
    let outside = outside?;
    let change = proposal_draft::folder(
        outside["access"].as_str()?,
        Path::new(outside["folder"].as_str()?),
    )?;
    Some(proposal_draft::offer(&proposal_draft::arguments(
        &change, entry,
    )))
}

// Map a completed evaluation receipt to the host's permission response.
pub(super) fn respond(
    root: &Path,
    adapter: &dyn AgentAdapter,
    event: Event,
    receipt: &Value,
    operation: &Value,
    session: &str,
) -> Option<Reply> {
    let reply = response(root, adapter, event, receipt, operation, session)?;
    Some(if reply.decision == Decision::Deny {
        presentation::denial(reply, receipt)
    } else {
        reply
    })
}

fn response(
    root: &Path,
    adapter: &dyn AgentAdapter,
    event: Event,
    receipt: &Value,
    operation: &Value,
    session: &str,
) -> Option<Reply> {
    if receipt["code"] == "request-rewrite" {
        return rewrite(receipt);
    }
    if let Some(reply) = existing_prompt(adapter, event, receipt) {
        return Some(reply);
    }
    if receipt["code"] == "unsupported-operation" {
        return Some(unsupported(adapter, event, operation));
    }
    let guidance = &receipt["diagnostics"]["guidance"][0];
    let entry = receipt["history"]["request_id"].as_str();
    if receipt["disposition"] == "allowed" && receipt["ask_fallback"] == "allow" {
        return Some(fallback_reply(root, adapter, receipt, operation, session));
    }
    let path = receipt["protected_path"]
        .as_str()
        .map_or_else(String::new, |path| format!(" (`{path}`)"));
    if let Some(reply) = sensitive_prompt(adapter, event, receipt, &path) {
        return Some(reply);
    }
    if ["sensitive-read", "protected-access", "protected-search"]
        .contains(&receipt["code"].as_str()?)
    {
        let mut value = raw_deny(receipt["code"].as_str()?);
        if receipt["code"] == "protected-access"
            && let Some(path) = receipt["protected_path"].as_str()
        {
            value.reason = protected_access(path, receipt["protected_group"].as_str());
        } else if let Some(reason) =
            Some(value.reason.as_str()).and_then(|reason| reason.strip_prefix("Shield: "))
            && !path.is_empty()
        {
            value.reason = format!("Shield: blocked by a protected path{path}. {reason}");
        }
        return Some(value);
    }
    if receipt["decision"] == "ask" || receipt["code"] == "approval-required" {
        return rewrite(receipt).or_else(|| {
            Some(if adapter.approval(event) == NativeApproval::Prompt {
                approval_prompt(root, adapter, receipt, operation)
            } else {
                asked(root, adapter, receipt, operation)
            })
        });
    }
    if receipt["decision"] != "allow" {
        if receipt["code"] == "policy-deny"
            && let Some(value) = rewrite(receipt)
        {
            return Some(value);
        }
        let mut value = raw_deny(receipt["code"].as_str()?);
        if receipt["code"] == "policy-deny"
            && let Some(detail) = receipt["diagnostics"]["message"].as_str()
        {
            // An unmet requirement is fixed in the request, not by a new rule.
            let unmet = receipt["diagnostics"]["requirement_checks"]
                .as_array()
                .is_some_and(|checks| checks.iter().any(|check| check["status"] != "met"));
            // A path outside the approved folders is named; when its folder is
            // narrow, adding that folder is offered as a complete proposal.
            let outside = receipt["outside_path"]["path"].as_str();
            let folder = outside.and_then(|_| {
                draft(
                    root,
                    adapter,
                    guidance,
                    operation,
                    Some(&receipt["outside_path"]),
                    entry,
                )
            });
            // The offer below replaces a rule's own advice about folders.
            let why = guidance["message"]
                .as_str()
                .unwrap_or(detail)
                .trim_end_matches('.');
            value.reason = match (outside, folder) {
                (Some(path), Some(offer)) => format!(
                    "Shield: blocked because `{path}` is outside the folders agents may use. {why}. {offer}"
                ),
                (Some(path), None) => format!(
                    "Shield: blocked because `{path}` is outside the folders agents may use. {why}. Its folder would open far more than this request needs, so do not propose it; work inside the project or ask the user."
                ),
                (None, _) if unmet => {
                    format!("Shield: blocked in this form; a revised request can pass. {detail}")
                }
                (None, _) => format!(
                    "Shield: denied. {detail} {} {PROPOSE}",
                    crate::agent_message::denial_detail(&receipt["diagnostics"])
                        .unwrap_or_default()
                ),
            };
        }
        return Some(value);
    }
    Some(permitted(if adapter.spec().id == "claude" {
        ""
    } else {
        "Shield active project policy permits this request."
    }))
}

fn existing_prompt(adapter: &dyn AgentAdapter, event: Event, receipt: &Value) -> Option<Reply> {
    (adapter.approval(event) == NativeApproval::ExistingPrompt
        && receipt["code"] == "approval-required"
        && receipt["disposition"] == "approval_required")
        .then(|| Reply {
            decision: Decision::Ask,
            reason: String::new(),
            agent_context: None,
        })
}

fn fallback_reply(
    root: &Path,
    adapter: &dyn AgentAdapter,
    receipt: &Value,
    operation: &Value,
    session: &str,
) -> Reply {
    let guidance = &receipt["diagnostics"]["guidance"][0];
    let entry = receipt["history"]["request_id"].as_str();
    let mut reason =
        "Shield: policy asks for approval; your configured Ask fallback allows this request."
            .to_owned();
    if let Some(offer) = packs::offer(root, adapter, guidance, entry) {
        reason.push(' ');
        reason.push_str(&offer);
        return permitted(&reason);
    }
    if let Some(detail) = receipt["diagnostics"]["message"].as_str() {
        reason.push(' ');
        reason.push_str(detail);
    }
    // Asked commands are collected per session; after the task the agent
    // drafts generic rules for the harmless ones and offers them together,
    // where a question cannot interrupt the work. Whether a pack asks for a
    // project rule travels with each suggestion.
    let _ = operation;
    permitted(&format!(
        "{reason} After the task, call suggestion_list with session_id {}. Offer one grouped custom-rule review only for eligible recurring requests or an explicit user request.",
        json!(session)
    ))
}

fn sensitive_prompt(
    adapter: &dyn AgentAdapter,
    event: Event,
    receipt: &Value,
    path: &str,
) -> Option<Reply> {
    if receipt["code"] == "sensitive-read" && adapter.approval(event) == NativeApproval::Prompt {
        return Some(Reply {
            decision: Decision::Ask,
            agent_context: None,
            reason: format!(
                "Shield: approval required. This reads .mcp.json or agent hook settings{path}, which may hold credentials. {}",
                adapter.spec().approval_action
            ),
        });
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protected_access_marker_survives_every_group_and_native_adapter() {
        let root = assert_fs::TempDir::new().unwrap();
        for agent in ["claude", "codex"] {
            let adapter = crate::agents::find(agent).unwrap();
            for group in [
                None,
                Some("state"),
                Some("folder"),
                Some("ignore"),
                Some("policy"),
                Some("settings"),
                Some("working-root"),
            ] {
                let mut receipt = json!({"decision":"deny","code":"protected-access"});
                if let Some(group) = group {
                    receipt["protected_path"] = json!("acme.txt");
                    receipt["protected_group"] = json!(group);
                }
                let reply = respond(
                    root.path(),
                    adapter,
                    Event::Before,
                    &receipt,
                    &json!({}),
                    "acme-session",
                )
                .unwrap();
                let wire = adapter.reply(Event::Before, &reply);
                let output = &wire["hookSpecificOutput"];
                assert_eq!(output["permissionDecision"], "deny");
                let context = output["additionalContext"].as_str().unwrap();
                assert!(
                    context.starts_with("Shield: protected path."),
                    "{agent} {group:?}: {wire}"
                );
                if group.is_some() {
                    assert!(context.contains("`acme.txt`"), "{wire}");
                }
                // The marker identifies agent guidance, not the human summary.
                assert!(
                    !output["permissionDecisionReason"]
                        .as_str()
                        .unwrap()
                        .contains("Shield: protected path."),
                    "{wire}"
                );
            }
        }
    }

    #[test]
    fn approval_prompt_omits_agent_diagnostics_without_a_user_explanation() {
        let root = assert_fs::TempDir::new().unwrap();
        let adapter = crate::agents::find("claude").unwrap();
        let receipt = serde_json::json!({
            "decision":"ask", "code":"approval-required",
            "diagnostics":{"message":"Call policy_propose with agent_reason."}
        });
        let reply = respond(
            root.path(),
            adapter,
            Event::Before,
            &receipt,
            &serde_json::json!({"kind":"shell"}),
            "acme-session",
        )
        .unwrap();
        let wire = adapter.reply(Event::Before, &reply);
        assert_eq!(wire["hookSpecificOutput"]["permissionDecision"], "ask");
        assert_eq!(
            wire["hookSpecificOutput"]["permissionDecisionReason"],
            "Shield: approval required. Allow this request?"
        );
        assert!(
            wire["hookSpecificOutput"]["additionalContext"]
                .as_str()
                .unwrap()
                .contains("Call policy_propose with agent_reason.")
        );
    }

    #[test]
    fn unsupported_feedback_does_not_echo_unsafe_tool_names_or_inputs() {
        let adapter = crate::agents::find("codex").unwrap();
        for name in ["Acme\nTool", "Acme`Tool", "https://example.com/private", ""] {
            let reply = unsupported(
                adapter,
                Event::Before,
                &serde_json::json!({"kind":"unsupported","name":name,"private":"ACME_PRIVATE_INPUT"}),
            );
            assert!(reply.reason.starts_with("Shield: blocked this tool:"));
            assert!(!reply.reason.contains("ACME_PRIVATE_INPUT"));
            assert!(!reply.reason.contains("search_query"));
            assert!(!reply.reason.contains('\n'));
        }
    }

    #[test]
    fn automatic_fallback_retains_initial_project_review_guidance() {
        let root = assert_fs::TempDir::new().unwrap();
        let receipt = serde_json::json!({"diagnostics":{
            "message":"Inspect script definitions and propose a narrow rule with policy_propose for user review.",
            "guidance":[{"source":"decisive","request":{"executable":"pnpm","arguments":["run","acme-check"]}}]}});
        let reply = fallback_reply(
            root.path(),
            crate::agents::find("codex").unwrap(),
            &receipt,
            &serde_json::json!({"kind":"shell"}),
            "acme-session",
        );
        assert!(reply.decision == Decision::Allow);
        assert!(reply.reason.contains("configured Ask fallback"));
        assert!(reply.reason.contains("Inspect script definitions"));
        assert!(!reply.reason.contains("user approved"));
    }

    #[test]
    fn fallback_collects_every_asked_command_for_the_session_suggestions() {
        let root = assert_fs::TempDir::new().unwrap();
        let adapter = crate::agents::find("codex").unwrap();
        let receipt = |rule_ref: Value, alternative: &str| {
            serde_json::json!({"diagnostics":{"guidance":[
                {"source":"decisive","request":{"executable":"npm","arguments":["run","dev"]},
                    "alternatives":[alternative]},
                {"rule_ref":rule_ref,"alternatives":[alternative]}]}})
        };
        let invites = receipt(
            serde_json::json!({"pack_selector":"acme/node","rule_id":"npm-scripts","scope":"pack"}),
            "Inspect script definitions. If appropriate, propose a narrow project rule with policy_propose.",
        );
        let shell = serde_json::json!({"kind":"shell"});
        // Every asked command is collected for the end of the task, whether
        // or not a pack rule invites a rule.
        let generic = receipt(
            Value::Null,
            "If it should run without asking, propose a pack or rule that covers it with policy_propose.",
        );
        for receipt in [&invites, &generic] {
            let collected = fallback_reply(root.path(), adapter, receipt, &shell, "acme-session");
            assert!(collected.decision == Decision::Allow);
            assert!(
                collected
                    .reason
                    .contains(r#"call suggestion_list with session_id "acme-session""#),
                "{}",
                collected.reason
            );
            assert!(!collected.reason.contains("policy_propose with change"));
        }
    }
}
