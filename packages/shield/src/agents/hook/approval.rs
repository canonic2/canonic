//! In-session approval at the native hook: which refusals the person may allow
//! once, consuming their grant on the identical retry, and the route the agent
//! follows otherwise. Fixed protected state never qualifies.
use super::{AgentAdapter, Decision, Event, Reply, Request};
use crate::review::hook_approval::{self, Identity, Key, Outcome, Summary, one_line};
use serde_json::{Value, json};
use std::path::Path;

#[cfg(test)]
mod tests;

/// Refusal codes a person can allow once. Rewrites, invalid input, the
/// operator CLI and protection failures each have their own recovery.
const APPROVABLE: &[&str] = &[
    "policy-deny",
    "approval-required",
    "protected-access",
    "sensitive-read",
    "unsupported-operation",
    "policy-file-unknown",
    "policy-files-invalid",
];
/// What the person reads when a refusal offers in-session approval.
pub(in crate::agents) const OFFER: &str = " The agent can ask you to allow it once.";

/// The receipt fields in-session approval needs.
pub(super) fn refusal(receipt: &Value) -> Value {
    json!({"code":receipt["code"],"fixed_protection":receipt["fixed_protection"],
        "identity":receipt["identity"],"history":receipt["history"],"protected_path":receipt["protected_path"],
        "protected_group":receipt["protected_group"],"policy_change":receipt["policy_change"],"git_restoration":receipt["git_restoration"],"policy_unavailable":receipt["policy_unavailable"],"deny_rules":receipt["deny_rules"],"mcp_match":receipt["mcp_match"],"_mcp_binding":receipt["_mcp_binding"]})
}

fn approvable(refusal: &Value) -> bool {
    refusal["fixed_protection"] == false
        && refusal["code"]
            .as_str()
            .is_some_and(|code| APPROVABLE.contains(&code))
}

/// Allow the request once with a live grant, or add the route to ask for one.
pub(super) fn in_session(
    root: &Path,
    adapter: &dyn AgentAdapter,
    request: &Request<'_>,
    refusals: &[Value],
    reply: &mut Reply,
) -> Option<Reply> {
    let first = refusals.first()?;
    if !refusals.iter().all(approvable)
        || refusals.iter().any(|r| r["identity"] != first["identity"])
    {
        return None;
    }
    let identity = Identity {
        activation_id: first["identity"]["activation_id"].as_str()?,
        policy_revision: first["identity"]["policy_revision"].as_str()?,
        candidate_digest: first["identity"]["candidate_digest"].as_str()?,
    };
    let cwd = hook_approval::relative_cwd(root, request.cwd.as_ref());
    let input_digest = input_digest(request, refusals)?;
    let key = Key {
        agent: adapter.spec().id,
        session: request.session,
        cwd: &cwd,
        tool: request.tool,
        input_digest,
    };
    let decision_requests: Vec<&str> = refusals
        .iter()
        .filter_map(|receipt| receipt["history"]["request_id"].as_str())
        .collect();
    match request.event {
        Event::Before => {
            // A failed read is not a missing registration; recording the
            // request below reports it with a retry.
            if enrolled(root, key.agent) == Some(false) {
                if refusals.iter().any(|r| r["policy_unavailable"] == true) {
                    reply.reason.push_str(" In-session approval is unavailable in this checkout. Repair the policy files from your terminal first, then run `shield mcp setup` in this project and restart the agent session.");
                    append(&mut reply.agent_context, INVALID_POLICY_NOT_CONNECTED);
                } else {
                    reply.reason.push_str(" Shield's approval connection is not registered in this checkout; repair Shield setup to restore in-session approval.");
                    append(&mut reply.agent_context, NOT_CONNECTED);
                }
                return None;
            }
            let Some(summary) = summary(request, refusals) else {
                append(
                    &mut reply.agent_context,
                    "This request is too long to show the user in full, so it cannot be approved as it is; split it into smaller calls.",
                );
                return None;
            };
            match hook_approval::refused_observed(
                root,
                &key,
                &identity,
                &summary,
                &decision_requests,
            ) {
                Ok(Outcome::Allowed) => Some(allowed()),
                Ok(Outcome::Pending(id, others)) => {
                    route(reply, &id, others);
                    None
                }
                Err(_) => {
                    append(
                        &mut reply.agent_context,
                        "Shield could not record this request for approval; retry the identical call once.",
                    );
                    None
                }
            }
        }
        // A later callback of an attempt whose grant was just consumed.
        Event::Permission => hook_approval::followup_observed(root, &key, &decision_requests)
            .ok()
            .filter(|covered| *covered)
            .map(|_| allowed()),
        Event::Completed | Event::Failed | Event::Ended => None,
    }
}

/// What the agent reads when this project records no integration for it:
/// its MCP connection cannot ask the person, so no request is stored.
const NOT_CONNECTED: &str = "Shield cannot ask the user to allow this here: this agent is not connected to Shield in this project. If it is needed, ask the user to run `shield mcp setup` in this project and restart this agent session, then repeat this exact call.";
const INVALID_POLICY_NOT_CONNECTED: &str = "Shield cannot ask the user to allow this here: this agent is not connected to Shield in this project, and policy is invalid. The user must first repair the policy files themselves, for example by resolving their Git conflicts from their own terminal. Setup also needs valid policy; after repair, ask the user to run `shield mcp setup` in this project and restart the agent session. Do not edit protected files or substitute an automatic host approval.";

/// Whether the project records an integration for this agent, the
/// condition its MCP connection needs to ask the person; `None` when the
/// project could not be read.
fn enrolled(root: &Path, agent: &str) -> Option<bool> {
    let context = crate::review::open_authorization(root).ok()?;
    crate::review::cli::registered(&context, agent).ok()
}

fn allowed() -> Reply {
    Reply {
        decision: Decision::Allow,
        reason: "Shield: allowed once; the user approved this exact request in this session."
            .into(),
        agent_context: None,
    }
}

fn append(text: &mut Option<String>, addition: &str) {
    *text = Some(match text.take() {
        Some(existing) if !existing.is_empty() => format!("{existing} {addition}"),
        _ => addition.to_owned(),
    });
}

fn route(reply: &mut Reply, id: &str, others: bool) {
    append(
        &mut reply.agent_context,
        &format!(
            "To do exactly this anyway, call request_approve with request_id {id}; Shield asks the user. After it returns approved, repeat this exact call once. Do not reach the same effect another way, and do not ask in chat instead: only Shield's form grants permission."
        ),
    );
    // Shield cannot tell which pending requests serve the same task, so the
    // hint names no other ID; the agent chooses the group.
    if others {
        append(
            &mut reply.agent_context,
            "For this task, approve only requests still needed. Pass up to eight pending requests from this session as request_ids, subject to the 12,000-byte confirmation limit; one request_id is always an option. If a group is too long, use the returned retry_arguments and review the remaining_request_ids separately.",
        );
    }
    if reply.decision != Decision::Allow {
        reply.reason.push_str(OFFER);
    }
}

/// Input fields that only explain a request to a person and never change how
/// it runs. Codex's `PermissionRequest` adds `description` to the same
/// attempt; `workdir` is bound through the decided operation's folder.
const EXPLANATORY: &[&str] = &["description", "justification", "workdir"];

/// The tool input that decides how the request runs: everything except
/// explanatory text, so a changed option such as a sandbox override needs a
/// new approval.
fn bound_input(request: &Request<'_>) -> Value {
    let mut input = request.input.clone();
    if let Some(fields) = input.as_object_mut() {
        fields.retain(|name, _| !EXPLANATORY.contains(&name.as_str()));
    }
    input
}

/// Binds execution input and every reviewed MCP or Git consequence, retaining
/// the existing nesting and labels so stored requests keep their exact binding.
fn input_digest(request: &Request<'_>, refusals: &[Value]) -> Option<String> {
    let input_digest = crate::policy::fingerprint(
        json!([request.operation, bound_input(request)])
            .to_string()
            .as_bytes(),
    );
    let input_digest = if refusals.iter().any(|r| !r["mcp_match"].is_null()) {
        crate::json::fingerprint(&(
            input_digest,
            refusals
                .iter()
                .map(|r| (&r["mcp_match"], &r["_mcp_binding"]))
                .collect::<Vec<_>>(),
        ))
        .ok()?
    } else {
        input_digest
    };
    // An older deny override may have been reviewed without the Git-policy
    // warning. Require a fresh review that includes this consequence, while
    // leaving every other saved grant's request binding unchanged.
    let input_digest = if refusals.iter().any(|r| r["policy_change"] == true) {
        crate::json::fingerprint(&(input_digest, "git-policy-change")).ok()?
    } else {
        input_digest
    };
    if refusals.iter().any(|r| r["git_restoration"].is_object()) {
        crate::json::fingerprint(&(
            input_digest,
            "git-restoration",
            refusals
                .iter()
                .map(|r| &r["git_restoration"])
                .collect::<Vec<_>>(),
        ))
        .ok()
    } else {
        Some(input_digest)
    }
}

/// What the form shows, or nothing when it cannot show the action whole.
fn summary(request: &Request<'_>, refusals: &[Value]) -> Option<Summary> {
    let mut reasons: Vec<String> = Vec::new();
    let mut deny_rules: Vec<String> = Vec::new();
    for refusal in refusals {
        if refusal["policy_unavailable"] == true {
            let reason = "the current policy files are invalid; Shield cannot apply policy, so only your exact one-time approval can authorize this request".to_owned();
            if !reasons.contains(&reason) {
                reasons.push(reason);
            }
        }
        let reason = reason(refusal);
        if !reasons.contains(&reason) {
            reasons.push(reason);
        }
        for rule in refusal["deny_rules"].as_array().into_iter().flatten() {
            let rule = one_line(rule.as_str().unwrap_or_default());
            if !rule.is_empty() && !deny_rules.contains(&rule) {
                deny_rules.push(rule);
            }
        }
    }
    let settings = |r: &Value| {
        r["git_restoration"]["settings_change"] == true
            || r["code"] == "protected-access"
                && !["policy"].contains(&r["protected_group"].as_str().unwrap_or("settings"))
    };
    let worktrees: std::collections::BTreeSet<_> = refusals
        .iter()
        .flat_map(|r| {
            r["git_restoration"]["worktrees"]
                .as_array()
                .into_iter()
                .flatten()
        })
        .filter_map(Value::as_str)
        .map(one_line)
        .collect();
    let mut action = action(request);
    if !worktrees.is_empty() {
        action.push_str("\n\nGit target worktrees:\n");
        action.push_str(&worktrees.into_iter().collect::<Vec<_>>().join("\n"));
        let calls: std::collections::BTreeSet<_> = refusals
            .iter()
            .flat_map(|r| {
                r["git_restoration"]["calls"]
                    .as_array()
                    .into_iter()
                    .flatten()
            })
            .map(Value::to_string)
            .collect();
        action.push_str(
            "\n\nGit restoration calls (arguments, working folder and target worktree):\n",
        );
        action.push_str(&calls.into_iter().collect::<Vec<_>>().join("\n"));
    }
    let summary = Summary {
        action,
        reason: reasons.join("; "),
        overrides_deny: refusals.iter().any(|r| r["code"] == "policy-deny"),
        deny_rules,
        settings_change: refusals.iter().any(settings),
        policy_change: refusals.iter().any(|r| r["policy_change"] == true),
    };
    hook_approval::fits(&summary, request.cwd.as_ref()).then_some(summary)
}

fn reason(refusal: &Value) -> String {
    // Paths come from the agent's request; they must not break the form's lines.
    let path = refusal["protected_path"].as_str().map(one_line);
    let path = path.as_deref();
    match (refusal["code"].as_str().unwrap_or_default(), path) {
        ("policy-deny", _) => "the active policy denies it".into(),
        ("approval-required", _) => {
            "the active policy requires your approval".into()
        }
        ("policy-file-unknown", _) => {
            "this Git command can replace Shield's policy files, or its writes to them cannot be determined".into()
        }
        ("policy-files-invalid", _) => "the current policy files are invalid".into(),
        ("protected-access", Some(path)) if refusal["protected_group"] == "policy" => {
            format!("it changes `{path}`, a Shield policy file")
        }
        ("protected-access", Some(path)) => {
            format!("it changes `{path}`, which holds the agent's settings")
        }
        ("protected-access", None) => "it changes protected agent settings or policy files".into(),
        ("sensitive-read", Some(path)) => format!("it reads `{path}`, which may hold credentials"),
        ("sensitive-read", None) => "it reads agent settings, which may hold credentials".into(),
        _ => "Shield cannot check what this tool does".into(),
    }
}

/// The exact action in plain words; the person approves what they read here.
fn action(request: &Request<'_>) -> String {
    let Some(operation) = &request.operation else {
        return format!("Use the {} tool with {}", request.tool, request.input);
    };
    let path = |op: &Value| op["path"].as_str().unwrap_or_default().to_owned();
    match operation["kind"].as_str().unwrap_or_default() {
        "shell" => {
            let mut text = format!(
                "Run this command:\n{}",
                operation["source"].as_str().unwrap_or_default()
            );
            // The grant binds these too, such as a sandbox override.
            let mut options = bound_input(request);
            if let Some(fields) = options.as_object_mut() {
                fields.remove("command");
                if !fields.is_empty() {
                    text.push_str("\n\nWith these options:\n");
                    text.push_str(&serde_json::to_string_pretty(&options).unwrap_or_default());
                }
            }
            text
        }
        // The grant binds the whole input, such as the content a write puts there.
        "file" if operation["action"] == "read" => format!("Read {}", path(operation)),
        "file" => format!(
            "{} {} with this tool input:\n{}",
            verb(&operation["action"]),
            path(operation),
            serde_json::to_string_pretty(request.input).unwrap_or_default()
        ),
        "batch" => {
            let files: Vec<_> = operation["operations"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|op| format!("{} {}", verb(&op["action"]), path(op)))
                .collect();
            match request.input["command"].as_str() {
                Some(body) => format!("Change these files:\n{}\n\n{body}", files.join("\n")),
                None => format!("Change these files:\n{}", files.join("\n")),
            }
        }
        "mcp" => format!(
            "Call the {} tool of MCP server {} with {}",
            operation["tool"].as_str().unwrap_or_default(),
            operation["alias"].as_str().unwrap_or_default(),
            operation["arguments"]
        ),
        _ => format!("Use the {} tool with {}", request.tool, request.input),
    }
}

fn verb(action: &Value) -> &'static str {
    match action.as_str() {
        Some("read") => "Read",
        Some("remove") => "Delete",
        _ => "Write",
    }
}
