//! Vendor native-hook formats, tool normalization and setup configuration.
use super::{AgentAdapter, AgentSpec, Decision, Event, Reply, Request, absolute, envelope, text};
use crate::{enforcement::protocol::NativeApproval, review::Error};
use serde_json::{Value, json};
use std::path::Path;

mod connections;
mod mcp_scopes;
mod review;
mod setup;
mod web;
pub(super) struct Codex;
static SPEC: AgentSpec = AgentSpec {
    id: "codex",
    label: "Codex",
    history_agent: "codex",
    events: &[
        ("PreToolUse", Event::Before),
        ("PermissionRequest", Event::Permission),
        ("PostToolUse", Event::Completed),
    ],
    hooks_path: ".codex/hooks.json",
    mcp_path: Some(".codex/config.toml"),
    // Codex reads hooks only from hooks.json and config.toml, both owned above.
    settings_paths: &[],
    prefix_rules_path: Some(".codex/rules/shield.rules"),
    approval_notes: false,
    needs_ask_fallback: true,
    approval_action: "Codex cannot prompt here. Submit a revised request, or propose a narrow rule with policy_propose for the user to review with shield policy review.",
};
impl AgentAdapter for Codex {
    fn check_mcp_scopes(&self, root: &Path, cwd: &Path, alias: &str) -> Result<(), Error> {
        mcp_scopes::check(root, cwd, alias)
    }
    fn merge_connections(
        &self,
        source: &str,
        previous: &Value,
        desired: &std::collections::BTreeMap<String, crate::mcp_connections::Transport>,
    ) -> Result<super::SettingsUpdate, Error> {
        connections::merge(source, previous, desired)
    }
    fn connection(
        &self,
        source: &str,
        alias: &str,
    ) -> Result<Option<crate::mcp_connections::Transport>, Error> {
        setup::connection(source, alias)
    }
    fn is_installed(&self, search_path: &[std::path::PathBuf]) -> bool {
        super::detection::on_path("codex", search_path)
    }
    fn review(&self) -> &'static dyn crate::agents::review::ReviewAdapter {
        &Codex
    }
    fn working_file_categories(&self) -> &'static [&'static str] {
        &["temporary"]
    }
    fn working_files(&self, _: &Value, _: &Path) -> crate::location::WorkingFiles {
        let config = super::working_files::configuration("CODEX_HOME", ".codex");
        super::working_files::temporary(config.as_deref())
    }
    fn spec(&self) -> &'static AgentSpec {
        &SPEC
    }
    fn approval(&self, event: Event) -> NativeApproval {
        if event == Event::Permission {
            NativeApproval::ExistingPrompt
        } else {
            NativeApproval::Fallback
        }
    }
    fn unsupported_help(&self, tool: &str) -> Option<&'static str> {
        (tool == "webrun").then_some(
            "Use search_query or open with direct HTTP(S) URLs; batches are supported. Result handles and other actions are unsupported. Use a direct URL when available; policy still applies.",
        )
    }
    fn request<'a>(&self, host: &'a Value) -> Option<Request<'a>> {
        let mut r = envelope(host, self)?;
        let tool = text(&host["tool_name"], 128)?;
        if SESSION_TOOLS.contains(&tool) {
            return Some(r);
        }
        // Reading or interrupting a command that already ran with approval.
        if tool == "write_stdin" && signal_only(host) {
            r.harmless = true;
            return Some(r);
        }
        if tool == "Bash" && r.event == Event::Permission {
            r.context_complete = host["tool_input"].get("workdir").is_some();
        }
        if tool == "Bash"
            && let Some(w) = host["tool_input"].get("workdir")
        {
            r.cwd = text(w, 4096)?;
            Path::new(r.cwd).is_absolute().then_some(())?;
        }
        r.operation = Some(operation(host)?);
        Some(r)
    }
    fn reply(&self, event: Event, r: &Reply) -> Value {
        if event == Event::Permission {
            return match r.decision {
                Decision::Ask => json!({}),
                Decision::Allow => {
                    json!({"hookSpecificOutput":{"hookEventName":"PermissionRequest","decision":{"behavior":"allow"}}})
                }
                Decision::Deny => {
                    json!({"hookSpecificOutput":{"hookEventName":"PermissionRequest","decision":{"behavior":"deny","message":r.reason}}})
                }
            };
        }
        if r.decision == Decision::Allow {
            json!({"hookSpecificOutput":{"hookEventName":"PreToolUse","additionalContext":r.reason}})
        } else {
            let mut value = json!({"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"deny","permissionDecisionReason":r.reason}});
            if let Some(context) = &r.agent_context {
                value["hookSpecificOutput"]["additionalContext"] = json!(context);
            }
            if r.decision == Decision::Ask {
                value["hookSpecificOutput"]["permissionDecisionReason"] = json!(
                    "Shield: approval required. Codex cannot request approval here. A policy change requires your approval."
                );
                let context = r
                    .agent_context
                    .as_deref()
                    .filter(|context| !context.is_empty())
                    .map_or_else(
                        || r.reason.clone(),
                        |context| format!("{}\n{context}", r.reason),
                    );
                value["hookSpecificOutput"]["additionalContext"] = json!(context);
            }
            value
        }
    }
    fn completion(&self, _: &str, _: Option<&str>) -> Value {
        json!({})
    }
    fn hook_group(&self, command: &str) -> Value {
        json!({"matcher":".*","hooks":[{"type":"command","command":command,"timeout":10,"statusMessage":"Shield project enforcement"}]})
    }
    fn mcp_entry(&self, binary: &Path, root: &Path, approved: &[String]) -> String {
        setup::mcp(binary, root, approved)
    }
    fn has_mcp_entry(&self, source: &str) -> Result<bool, Error> {
        setup::has_mcp_entry(source)
    }
    fn prefix_rules(
        &self,
        prefixes: &[crate::policy::agent_outputs::CodexPrefix],
    ) -> Option<String> {
        setup::prefix_rules(prefixes)
    }
    fn owns_prefix_rules(&self, source: &str) -> bool {
        source.starts_with(setup::PREFIX_RULES_HEADER)
    }
    fn capture_hooks(
        &self,
        source: Option<&str>,
        expected: &super::setup::HookGroups,
    ) -> Result<super::setup::HookGroups, Error> {
        super::setup::capture(source, expected)
    }
    fn merge_hooks(
        &self,
        source: Option<&str>,
        previous: &super::setup::HookGroups,
        desired: &super::setup::HookGroups,
    ) -> Result<String, Error> {
        super::setup::merge(source, previous, desired)
    }
    fn merge_mcp(
        &self,
        before: Option<&str>,
        old: Option<&str>,
        new: Option<&str>,
    ) -> Result<String, Error> {
        setup::merge_mcp(before, old, new)
    }
    fn diagnose(
        &self,
        hooks: Option<&str>,
        owned: &super::setup::HookGroups,
        extra: Option<&str>,
        d: &mut Vec<&'static str>,
    ) -> Result<(), Error> {
        super::setup::diagnose(hooks, owned, d)?;
        setup::diagnose(extra, d)
    }
}
/// Native local function tools that act only within the Codex session:
/// planning, questions and permission requests the user answers, tool search,
/// timing and context, subagents and MCP resource listing. Resource reads
/// return another server's content, so they block like that server's tools.
/// A subagent's own tool calls reach the hook separately.
const SESSION_TOOLS: &[&str] = &[
    "close_agent",
    "current_time",
    "followup_task",
    "get_context_remaining",
    "interrupt_agent",
    "list_agents",
    "list_available_plugins_to_install",
    "list_mcp_resource_templates",
    "list_mcp_resources",
    "new_context_window",
    "request_permissions",
    "request_user_input",
    "request_user_input_async",
    "resume_agent",
    "send_input",
    "send_message",
    "sleep",
    "spawn_agent",
    "tool_search",
    "update_plan",
    "wait_agent",
    "wait_for_environment",
];

/// Input to an already running command that only reads its output (none) or
/// interrupts or ends it (Ctrl-C, Ctrl-D). Typed text could run commands in an
/// interactive program, so any other input stays unsupported.
fn signal_only(host: &Value) -> bool {
    host["tool_input"]["chars"]
        .as_str()
        .is_some_and(|chars| chars.chars().all(|c| matches!(c, '\u{3}' | '\u{4}')))
}

fn operation(host: &Value) -> Option<Value> {
    let tool = text(&host["tool_name"], 128)?;
    let input = host["tool_input"].as_object()?;
    if let Some(operation) = super::mcp_operation(tool, &host["tool_input"]) {
        return Some(operation);
    }
    match tool {
        // Codex reads the image file itself, so the read is decided by its path.
        "view_image" => {
            if input
                .keys()
                .any(|k| !["path", "detail", "environment_id"].contains(&k.as_str()))
            {
                return None;
            }
            // A named environment resolves the path against its own cwd and
            // filesystem, which may be remote; only the primary one is modeled.
            if !input.get("environment_id").is_none_or(Value::is_null) {
                return Some(json!({"kind":"unsupported","name":tool}));
            }
            let path = text(&input["path"], 4096)?;
            Some(
                json!({"kind":"file","action":"read","path":absolute(path, host)?,"recursive":false}),
            )
        }
        "Bash" => {
            if input.keys().any(|k| {
                ![
                    "command",
                    "description",
                    "timeout_ms",
                    "workdir",
                    "yield_time_ms",
                    "max_output_tokens",
                    "shell",
                    "login",
                    "tty",
                    "sandbox_permissions",
                    "justification",
                    "prefix_rule",
                ]
                .contains(&k.as_str())
            }) {
                return None;
            }
            let source = text(&input["command"], 65_536)?;
            let dialect = if input.contains_key("shell") {
                "cmd"
            } else {
                "posix"
            };
            Some(json!({"kind":"shell","source":source,"dialect":dialect}))
        }
        "apply_patch" => {
            if input.keys().any(|k| k != "command") {
                return None;
            }
            let changes = patch_changes(text(&host["tool_input"]["command"], 1_048_576)?, host)?;
            Some(json!({"kind":"batch","operations":changes}))
        }
        "webrun" => web::operation(&host["tool_input"]),
        _ => Some(json!({"kind":"unsupported","name":tool})),
    }
}
/// The files a Codex patch adds, updates, moves or deletes. Text outside the
/// patch grammar's headers is refused rather than guessed.
fn patch_changes(source: &str, host: &Value) -> Option<Vec<Value>> {
    let lines: Vec<&str> = source.trim().lines().collect();
    let (first, rest) = lines.split_first()?;
    let (last, body) = rest.split_last()?;
    if first.trim_end() != "*** Begin Patch" || last.trim_end() != "*** End Patch" {
        return None;
    }
    let mut changes: Vec<Value> = Vec::new();
    let mut updating = false;
    // Content lines start with a space, + or -; only headers start with ***.
    for header in body.iter().filter_map(|line| line.strip_prefix("*** ")) {
        let header = header.trim_end();
        let (action, path) = if let Some(path) = header.strip_prefix("Add File: ") {
            ("write", path)
        } else if let Some(path) = header.strip_prefix("Update File: ") {
            ("write", path)
        } else if let Some(path) = header.strip_prefix("Delete File: ") {
            ("remove", path)
        } else if let Some(path) = header.strip_prefix("Move to: ") {
            // A moved file leaves its source path.
            if !updating {
                return None;
            }
            changes.last_mut()?["action"] = json!("remove");
            ("write", path)
        } else if header == "End of File" {
            continue;
        } else {
            return None;
        };
        updating = header.starts_with("Update File: ");
        let path = path.trim();
        if path.is_empty() {
            return None;
        }
        changes.push(
            json!({"kind":"file","action":action,"path":absolute(path, host)?,"recursive":false}),
        );
    }
    (!changes.is_empty() && changes.len() <= 256).then_some(changes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_ask_preserves_both_the_reason_and_agent_context() {
        let adapter = crate::agents::find("codex").unwrap();
        for context in [None, Some("Acme instructions to propose a narrow change.")] {
            let reply = Reply {
                decision: Decision::Ask,
                reason: "Acme request needs approval.".into(),
                agent_context: context.map(str::to_owned),
            };
            let value = adapter.reply(Event::Before, &reply);
            assert_eq!(value["hookSpecificOutput"]["permissionDecision"], "deny");
            let instructions = value["hookSpecificOutput"]["additionalContext"]
                .as_str()
                .unwrap();
            assert!(instructions.contains(&reply.reason));
            if let Some(context) = context {
                assert!(instructions.contains(context));
            }
        }
    }
}
