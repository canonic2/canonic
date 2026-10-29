//! Vendor native-hook formats, tool normalization and setup configuration.
use super::{AgentAdapter, AgentSpec, Decision, Event, Reply, Request, absolute, envelope, text};
use crate::{enforcement::protocol::NativeApproval, review::Error};
use serde_json::{Value, json};
use std::path::Path;

mod artifacts;
mod connections;
mod mcp_scopes;
mod review;
mod setup;

pub(super) struct ClaudeCode;
static SPEC: AgentSpec = AgentSpec {
    id: "claude",
    label: "Claude Code",
    history_agent: "claude-code",
    events: &[
        ("PreToolUse", Event::Before),
        ("PostToolUse", Event::Completed),
        ("PostToolUseFailure", Event::Failed),
    ],
    hooks_path: ".claude/settings.local.json",
    mcp_path: None,
    // Shared project settings can set `disableAllHooks` or replace hooks.
    settings_paths: &[".claude/settings.json"],
    prefix_rules_path: None,
    needs_ask_fallback: false,
    approval_notes: true,
    approval_action: "Allow this request?",
};
impl AgentAdapter for ClaudeCode {
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
    fn connection_settings(
        &self,
        source: &str,
        previous: &Value,
        aliases: &[String],
    ) -> Result<super::SettingsUpdate, Error> {
        connections::settings(source, previous, aliases)
    }
    fn connection(
        &self,
        source: &str,
        alias: &str,
    ) -> Result<Option<crate::mcp_connections::Transport>, Error> {
        let document = crate::json::parse(source.as_bytes())
            .map_err(|_| Error::Invalid("invalid MCP JSON"))?;
        super::unambiguous_mcp_alias(
            alias,
            document["mcpServers"]
                .as_object()
                .into_iter()
                .flatten()
                .map(|(key, _)| key.as_str()),
        )?;
        let Some(entry) = document.get("mcpServers").and_then(|s| s.get(alias)) else {
            return Ok(None);
        };
        let mut entry = entry.clone();
        if entry.get("type").is_none() {
            entry["type"] = json!("stdio");
        }
        serde_json::from_value(entry)
            .map(Some)
            .map_err(|_| Error::Invalid("unsupported MCP connection settings"))
    }
    fn is_installed(&self, search_path: &[std::path::PathBuf]) -> bool {
        super::detection::on_path("claude", search_path)
    }
    fn review(&self) -> &'static dyn crate::agents::review::ReviewAdapter {
        &ClaudeCode
    }
    fn working_file_categories(&self) -> &'static [&'static str] {
        &["temporary", "session_tool_results"]
    }
    fn working_files(&self, host: &Value, project: &Path) -> crate::location::WorkingFiles {
        let config = super::working_files::configuration("CLAUDE_CONFIG_DIR", ".claude");
        let mut scope = super::working_files::temporary(config.as_deref());
        if let Some(config) = config
            && let Some(root) = artifacts::root(host, project, &config)
        {
            scope.read.push(root);
        }
        scope
    }
    fn spec(&self) -> &'static AgentSpec {
        &SPEC
    }
    fn approval(&self, _: Event) -> NativeApproval {
        NativeApproval::Prompt
    }
    fn request<'a>(&self, host: &'a Value) -> Option<Request<'a>> {
        let mut r = envelope(host, self)?;
        let tool = text(&host["tool_name"], 128)?;
        if SESSION_TOOLS.contains(&tool) {
            return Some(r);
        }
        r.operation = Some(operation(host)?);
        Some(r)
    }
    fn reply(&self, _: Event, r: &Reply) -> Value {
        let d = match r.decision {
            Decision::Allow => "allow",
            Decision::Ask => "ask",
            Decision::Deny => "deny",
        };
        let mut value = json!({"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":d,"permissionDecisionReason":r.reason}});
        if let Some(context) = &r.agent_context
            && !context.trim().is_empty()
        {
            value["hookSpecificOutput"]["additionalContext"] = json!(context);
        }
        if r.decision == Decision::Allow {
            value["hookSpecificOutput"]["permissionDecisionReason"] = json!("Shield: allowed.");
            let context = [Some(r.reason.as_str()), r.agent_context.as_deref()]
                .into_iter()
                .flatten()
                .filter(|part| !part.trim().is_empty())
                .collect::<Vec<_>>()
                .join(" ");
            if !context.is_empty() {
                value["hookSpecificOutput"]["additionalContext"] = json!(context);
            }
        }
        value
    }
    fn completion(&self, event: &str, note: Option<&str>) -> Value {
        note.map_or_else(
            || json!({}),
            |n| json!({"hookSpecificOutput":{"hookEventName":event,"additionalContext":n}}),
        )
    }
    fn hook_group(&self, command: &str) -> Value {
        json!({"matcher":".*","hooks":[{"type":"command","command":command,"timeout":10,"statusMessage":"Shield project enforcement"}]})
    }
    fn mcp_entry(&self, binary: &Path, root: &Path, _approved: &[String]) -> String {
        json!({"type":"stdio","command":binary,"args":["mcp","serve","--project",root,"--agent",SPEC.history_agent]}).to_string()
    }
    fn has_mcp_entry(&self, source: &str) -> Result<bool, Error> {
        let config = crate::json::parse(source.as_bytes())
            .map_err(|_| Error::Invalid("invalid MCP configuration JSON"))?;
        if !config.is_object() || config.get("mcpServers").is_some_and(|v| !v.is_object()) {
            return Err(Error::Invalid("invalid MCP configuration shape"));
        }
        Ok(config["mcpServers"]
            .as_object()
            .is_some_and(|servers| servers.contains_key("shield")))
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
        _: Option<&str>,
        _: Option<&str>,
    ) -> Result<String, Error> {
        Ok(before.unwrap_or_default().into())
    }
    fn merge_settings(
        &self,
        source: Option<&str>,
        owned: Option<&Value>,
        approved: Option<&[String]>,
    ) -> Result<super::SettingsUpdate, Error> {
        setup::merge(source, owned, approved)
    }
    fn diagnose(
        &self,
        hooks: Option<&str>,
        owned: &super::setup::HookGroups,
        _: Option<&str>,
        d: &mut Vec<&'static str>,
    ) -> Result<(), Error> {
        let config = super::setup::diagnose(hooks, owned, d)?;
        setup::diagnose(&config, d);
        if config["disableAllHooks"] == true {
            d.push("native hooks are disabled in project settings");
        }
        Ok(())
    }
}
/// Native tools that act only within the Claude Code session: planning, task
/// lists, scheduling, questions, subagents, agent listing, background task
/// control and MCP resource listing. Resource reads return another server's
/// content, so they are unsupported like that server's tools. `SendMessage`
/// can reach other sessions that this project's hooks do not check, so it is
/// unsupported too. A local subagent's or scheduled prompt's own tool calls
/// reach the hook separately.
const SESSION_TOOLS: &[&str] = &[
    "Agent",
    "AskUserQuestion",
    "CronCreate",
    "CronDelete",
    "CronList",
    "EndConversation",
    "EnterPlanMode",
    "ExitPlanMode",
    "ListAgents",
    "ListMcpResourcesTool",
    "ReportFindings",
    "ScheduleWakeup",
    "SendFeedback",
    "Skill",
    "SubagentHandback",
    "Task",
    "TaskCreate",
    "TaskGet",
    "TaskList",
    "TaskOutput",
    "TaskStop",
    "TaskUpdate",
    "TodoWrite",
    "ToolSearch",
    "WaitForMcpServers",
    "Workflow",
];

fn operation(host: &Value) -> Option<Value> {
    let tool = text(&host["tool_name"], 128)?;
    let input = host["tool_input"].as_object()?;
    if let Some(operation) = super::mcp_operation(tool, &host["tool_input"]) {
        return Some(operation);
    }
    match tool {
        // A command watch runs its command like Bash; a WebSocket watch is unsupported.
        "Monitor"
            if input.contains_key("command")
                && input
                    .keys()
                    .all(|k| ["command", "description", "timeout_ms"].contains(&k.as_str())) =>
        {
            let source = text(&input["command"], 65_536)?;
            Some(json!({"kind":"shell","source":source,"dialect":"bash"}))
        }
        "Bash" => {
            if input.keys().any(|k| {
                ![
                    "command",
                    "description",
                    "timeout",
                    "run_in_background",
                    "dangerouslyDisableSandbox",
                ]
                .contains(&k.as_str())
            }) {
                return None;
            }
            let source = text(&input["command"], 65_536)?;
            Some(json!({"kind":"shell","source":source,"dialect":"bash"}))
        }
        "Read" => {
            if input
                .keys()
                .any(|k| !["file_path", "offset", "limit"].contains(&k.as_str()))
            {
                return None;
            }
            // Claude reads the file itself, so any file type is decided by its path.
            let path = text(&host["tool_input"]["file_path"], 4096)?;
            Some(
                json!({"kind":"file","action":"read","path":absolute(path, host)?,"recursive":false}),
            )
        }
        // Claude file tools are decided by the file they change or the tree they read.
        "Write" | "Edit" | "MultiEdit" | "NotebookEdit" => {
            let key = if tool == "NotebookEdit" {
                "notebook_path"
            } else {
                "file_path"
            };
            let path = text(&host["tool_input"][key], 4096)?;
            Some(
                json!({"kind":"file","action":"write","path":absolute(path, host)?,"recursive":false}),
            )
        }
        "Glob" | "Grep" => {
            let path = match host["tool_input"].get("path") {
                Some(value) => text(value, 4096)?,
                None => text(&host["cwd"], 4096)?,
            };
            // Glob lists names matching its pattern; Grep reads the contents of
            // the files its glob and type select.
            let selection = |key: &str| match input.get(key) {
                Some(value) => text(value, 4096).map(|s| vec![s.to_owned()]),
                None => Some(vec![]),
            };
            let search = if tool == "Glob" {
                json!({"names_only":true,"globs":selection("pattern")?,"types":[]})
            } else {
                json!({"names_only":false,"globs":selection("glob")?,"types":selection("type")?})
            };
            Some(
                json!({"kind":"file","action":"read","path":absolute(path, host)?,"recursive":true,"search":search}),
            )
        }
        // Domain lists only narrow where a search looks; the query is decided.
        "WebSearch" => {
            if input
                .keys()
                .any(|k| !["query", "allowed_domains", "blocked_domains"].contains(&k.as_str()))
            {
                return None;
            }
            let query = text(&host["tool_input"]["query"], 8192)?;
            Some(json!({"kind":"web","action":"search","query":query,"url":null}))
        }
        "WebFetch" => {
            if input
                .keys()
                .any(|k| !["url", "prompt"].contains(&k.as_str()))
            {
                return None;
            }
            let url = text(&host["tool_input"]["url"], 8192)?;
            Some(json!({"kind":"web","action":"fetch","query":null,"url":url}))
        }
        _ => Some(json!({"kind":"unsupported","name":tool})),
    }
}
