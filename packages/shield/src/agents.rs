//! Native agent protocol adapters and their shared lifecycle contracts.
use crate::enforcement::protocol::NativeApproval;
use serde_json::Value;
use std::path::Path;

mod claude_code;
mod codex;
pub(crate) mod detection;
mod feedback;
mod hook;
mod mcp_scopes;
pub(crate) mod review;
pub(crate) mod setup;
mod working_files;
pub(crate) use hook::run;

pub(crate) static ADAPTERS: &[&dyn AgentAdapter] = &[&claude_code::ClaudeCode, &codex::Codex];

pub(crate) fn find(name: &str) -> Option<&'static dyn AgentAdapter> {
    ADAPTERS.iter().copied().find(|a| a.spec().id == name)
}

/// The folders a portable rule must not name, discovered the way every adapter
/// discovers its working files, with no agent session.
pub(crate) fn machine_roots(project: &Path) -> Vec<crate::policy::portability::Root> {
    let scopes: Vec<_> = ADAPTERS
        .iter()
        .map(|adapter| adapter.working_files(&Value::Null, project))
        .collect();
    working_files::machine_roots(&scopes, project)
}

pub(crate) fn names() -> clap::builder::PossibleValuesParser {
    clap::builder::PossibleValuesParser::new(ADAPTERS.iter().map(|a| a.spec().id))
}

pub(crate) fn label(name: &str) -> &str {
    ADAPTERS
        .iter()
        .find(|a| a.spec().id == name || a.spec().history_agent == name)
        .map_or(name, |a| a.spec().label)
}

/// Whether a recorded agent name and a filter name the same agent, accepting
/// either an adapter's CLI id or its history name; unknown names match exactly.
pub(crate) fn same_agent(filter: &str, recorded: &str) -> bool {
    let canonical = |name: &str| {
        ADAPTERS
            .iter()
            .find(|a| a.spec().id == name || a.spec().history_agent == name)
            .map(|a| a.spec().id)
    };
    match (canonical(filter), canonical(recorded)) {
        (Some(a), Some(b)) => a == b,
        _ => filter == recorded,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Event {
    Before,
    Permission,
    Completed,
    Failed,
}
impl Event {
    fn post(self) -> bool {
        matches!(self, Self::Completed | Self::Failed)
    }
}

pub(crate) struct AgentSpec {
    pub id: &'static str,
    pub label: &'static str,
    pub history_agent: &'static str,
    pub events: &'static [(&'static str, Event)],
    pub hooks_path: &'static str,
    /// None uses the shared project JSON MCP file.
    pub mcp_path: Option<&'static str>,
    /// Other project settings files the agent reads that can disable or
    /// replace its hooks; protected like the files Shield writes.
    pub settings_paths: &'static [&'static str],
    /// The Shield-owned file of prefix rules the agent loads, written from the
    /// active policy's Codex prefixes.
    pub prefix_rules_path: Option<&'static str>,
    /// The host needs a standing fallback because it cannot ask before execution.
    pub needs_ask_fallback: bool,
    pub approval_action: &'static str,
    pub approval_notes: bool,
}

/// A resolved host reply, kept semantic until all operations have been decided.
pub(crate) struct Reply {
    pub decision: Decision,
    pub reason: String,
    /// Guidance for the agent, separate from a user-facing permission reason.
    pub agent_context: Option<String>,
}
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Decision {
    Allow,
    Ask,
    Deny,
}

pub(crate) struct Request<'a> {
    pub event: Event,
    pub event_name: &'a str,
    pub tool: &'a str,
    pub input: &'a Value,
    pub tool_id: Option<&'a str>,
    pub session: &'a str,
    pub cwd: &'a str,
    pub operation: Option<Value>,
    pub context_complete: bool,
    pub working_files: Option<crate::location::WorkingFiles>,
    /// The adapter found no file, process or network effect of its own, so a
    /// host permission request for it is answered allow instead of prompting.
    pub harmless: bool,
}

/// Adapter-owned settings changes and the metadata needed to undo only additions.
pub(crate) struct SettingsUpdate {
    pub source: String,
    pub ownership: Option<Value>,
}

/// Vendor formats and presentation. Policy, authority, aggregation and history stay shared.
pub(crate) trait AgentAdapter: Sync {
    fn merge_connections(
        &self,
        source: &str,
        previous: &Value,
        desired: &std::collections::BTreeMap<String, crate::mcp_connections::Transport>,
    ) -> Result<SettingsUpdate, crate::review::Error> {
        if !desired.is_empty() || previous.as_object().is_some_and(|p| !p.is_empty()) {
            return Err(crate::review::Error::Invalid(
                "MCP connection projection unsupported",
            ));
        }
        Ok(SettingsUpdate {
            source: source.into(),
            ownership: None,
        })
    }
    fn connection_settings(
        &self,
        source: &str,
        _previous: &Value,
        _aliases: &[String],
    ) -> Result<SettingsUpdate, crate::review::Error> {
        Ok(SettingsUpdate {
            source: source.into(),
            ownership: None,
        })
    }
    /// Decode one host connection without reading files or inferring identity.
    fn connection(
        &self,
        _source: &str,
        _alias: &str,
    ) -> Result<Option<crate::mcp_connections::Transport>, crate::review::Error> {
        Err(crate::review::Error::Invalid(
            "MCP connection mapping unsupported",
        ))
    }
    /// Reject known competing MCP aliases without modifying other host scopes.
    fn check_mcp_scopes(
        &self,
        _root: &Path,
        _cwd: &Path,
        _alias: &str,
    ) -> Result<(), crate::review::Error> {
        Err(crate::review::Error::Invalid(
            "MCP scope verification unsupported",
        ))
    }

    /// Best-effort CLI discovery for init defaults; never executes the agent.
    /// Adapters without local CLI discovery remain unselected.
    fn is_installed(&self, _search_path: &[std::path::PathBuf]) -> bool {
        false
    }
    fn review(&self) -> &'static dyn review::ReviewAdapter {
        review::resolve(None)
    }
    fn spec(&self) -> &'static AgentSpec;
    fn request<'a>(&self, host: &'a Value) -> Option<Request<'a>>;
    fn working_files(&self, host: &Value, project: &Path) -> crate::location::WorkingFiles;
    fn working_file_categories(&self) -> &'static [&'static str] {
        &[]
    }
    fn approval(&self, event: Event) -> NativeApproval;
    /// Extra agent guidance for communicating one relevant uninstalled pack.
    /// Shared feedback selects the candidate and retains proposal authority.
    fn pack_offer_instruction(&self) -> Option<&'static str> {
        Some(
            "Briefly offer this rule pack once per conversation, after the task and before your final reply, even if the command already ran. The apply prompt is the installation question; do not first ask in text or with a question tool. Wait before the next proposal. Declining or cancelling leaves the pack uninstalled; do not retry.",
        )
    }
    /// Supported request forms to mention when this vendor tool is unmodeled.
    fn unsupported_help(&self, _tool: &str) -> Option<&'static str> {
        None
    }
    fn reply(&self, event: Event, reply: &Reply) -> Value;
    fn completion(&self, event: &str, note: Option<&str>) -> Value;
    fn hook_group(&self, command: &str) -> Value;
    fn capture_hooks(
        &self,
        source: Option<&str>,
        expected: &setup::HookGroups,
    ) -> Result<setup::HookGroups, crate::review::Error>;
    fn merge_hooks(
        &self,
        source: Option<&str>,
        previous: &setup::HookGroups,
        desired: &setup::HookGroups,
    ) -> Result<String, crate::review::Error>;
    /// Reconcile adapter settings; `approved` lists the Shield tools allowed
    /// without asking when installing, and is `None` when removing.
    fn merge_settings(
        &self,
        source: Option<&str>,
        _owned: Option<&Value>,
        _approved: Option<&[String]>,
    ) -> Result<SettingsUpdate, crate::review::Error> {
        Ok(SettingsUpdate {
            source: source.unwrap_or_default().into(),
            ownership: None,
        })
    }
    /// The MCP server entry, approving `approved` tools where the entry holds approvals.
    fn mcp_entry(&self, binary: &Path, root: &Path, approved: &[String]) -> String;
    /// Inspect configuration without merging or claiming ownership; malformed
    /// input is an error, distinct from an absent Shield entry.
    fn has_mcp_entry(&self, source: &str) -> Result<bool, crate::review::Error>;
    /// The prefix rules file for `prefixes`, None when there are none to write.
    fn prefix_rules(
        &self,
        _prefixes: &[crate::policy::agent_outputs::CodexPrefix],
    ) -> Option<String> {
        None
    }
    /// Whether an existing prefix rules file carries Shield's generated header.
    fn owns_prefix_rules(&self, _source: &str) -> bool {
        false
    }
    fn merge_mcp(
        &self,
        before: Option<&str>,
        old: Option<&str>,
        new: Option<&str>,
    ) -> Result<String, crate::review::Error>;
    fn diagnose(
        &self,
        hooks: Option<&str>,
        owned: &setup::HookGroups,
        extra: Option<&str>,
        diagnostics: &mut Vec<&'static str>,
    ) -> Result<(), crate::review::Error>;

    fn event(&self, name: &str) -> Option<Event> {
        self.spec()
            .events
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, e)| *e)
    }

    fn input_event(&self, host: &Value) -> Option<Event> {
        self.event(text(&host["hook_event_name"], 128)?)
    }
}

fn mcp_operation(tool: &str, input: &Value) -> Option<Value> {
    let (alias, name) = tool.strip_prefix("mcp__")?.split_once("__")?;
    if !crate::mcp_connections::valid_name(alias)
        || name.is_empty()
        || name.len() > 128
        || !input.is_object()
    {
        return None;
    }
    Some(serde_json::json!({"kind":"mcp","alias":alias,"tool":name,"arguments":input}))
}

/// Refuse aliases that can collapse into the same encoded tool namespace or
/// consume the `__` delimiter. Never pick a connection by a similar name.
fn unambiguous_mcp_alias<'a>(
    alias: &str,
    aliases: impl Iterator<Item = &'a str>,
) -> Result<(), crate::review::Error> {
    for other in aliases.filter(|other| *other != alias) {
        let normalized = other
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '_' {
                    c.to_ascii_lowercase()
                } else {
                    '_'
                }
            })
            .collect::<String>();
        if [other, normalized.as_str()].into_iter().any(|other| {
            other == alias
                || other == format!("{alias}_")
                || other.starts_with(&format!("{alias}__"))
        }) {
            return Err(crate::review::Error::Invalid(
                "ambiguous MCP server aliases; resolve the conflicting host namespace before setup",
            ));
        }
    }
    Ok(())
}

fn text(value: &Value, max: usize) -> Option<&str> {
    value
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= max && !s.contains('\0'))
}

fn envelope<'a>(host: &'a Value, adapter: &dyn AgentAdapter) -> Option<Request<'a>> {
    let event_name = text(&host["hook_event_name"], 128)?;
    let event = adapter.event(event_name)?;
    let tool_id = match host.get("tool_use_id") {
        Some(value) => Some(text(value, 128)?),
        None if event == Event::Permission => None,
        None => return None,
    };
    let cwd = text(&host["cwd"], 4096)?;
    Path::new(cwd).is_absolute().then_some(())?;
    Some(Request {
        event,
        event_name,
        tool: text(&host["tool_name"], 128)?,
        input: &host["tool_input"],
        tool_id,
        session: text(&host["session_id"], 4096)?,
        cwd,
        operation: None,
        context_complete: true,
        working_files: None,
        harmless: false,
    })
}

fn absolute(path: &str, host: &Value) -> Option<String> {
    if Path::new(path).is_absolute() {
        return Some(path.to_owned());
    }
    Path::new(text(&host["cwd"], 4096)?)
        .join(path)
        .to_str()
        .map(str::to_owned)
}
