//! Copilot CLI's camelCase hooks. Native events are fixed by installation;
//! neither an event discriminator nor a tool invocation ID is in the payload.
use super::{AgentAdapter, AgentSpec, Decision, Event, Reply, Request, text};
use crate::{enforcement::protocol::NativeApproval, review::Error};
use serde_json::{Value, json};
use std::path::Path;

mod setup;
#[cfg(test)]
mod tests;
mod tools;

pub(super) struct Copilot;
fn native_absolute(value: &Value) -> Option<&str> {
    let path = text(value, 4096)?;
    Path::new(path).is_absolute().then_some(path)
}
static SPEC: AgentSpec = AgentSpec {
    id: "copilot",
    label: "Copilot CLI",
    events: &[
        ("preToolUse", Event::Before),
        ("postToolUse", Event::Completed),
        ("postToolUseFailure", Event::Failed),
    ],
    hooks_path: ".github/hooks/shield.json",
    mcp_path: Some(".mcp.json"),
    settings_paths: &[
        ".github/copilot/settings.json",
        ".github/copilot/settings.local.json",
        ".github/mcp.json",
    ],
    prefix_rules_path: None,
    needs_ask_fallback: false,
    approval_notes: false,
    approval_action: "Allow this request?",
};

impl AgentAdapter for Copilot {
    fn check_native_setup(
        &self,
        root: &Path,
        pending: &dyn Fn(&str) -> Option<Option<String>>,
    ) -> Result<(), Error> {
        setup::check_compatible(root, pending)
    }
    fn spec(&self) -> &'static AgentSpec {
        &SPEC
    }
    // Explicit selection only: discovering the CLI cannot establish that its
    // imported Claude/profile callbacks are compatible with this integration.
    fn input_event_for(&self, host: &Value, expected: Event) -> Option<Event> {
        if host.get("hook_event_name").is_some() || host.get("tool_name").is_some() {
            return None;
        }
        matches!(expected, Event::Before | Event::Completed | Event::Failed).then_some(expected)
    }
    fn request<'a>(&self, host: &'a Value) -> Option<Request<'a>> {
        self.request_for_event(host, Event::Before)
    }
    fn request_for_event<'a>(&self, host: &'a Value, event: Event) -> Option<Request<'a>> {
        self.input_event_for(host, event)?;
        host.as_object()?;
        host["timestamp"].as_u64()?;
        let input = &host["toolArgs"];
        input.as_object()?;
        let tool = text(&host["toolName"], 256)?;
        let cwd = native_absolute(&host["cwd"])?;
        let shield_tool = tool
            .strip_prefix("shield-")
            .filter(|name| crate::review::mcp::is_tool(name));
        // The host flattens MCP alias/tool identity with '-'. Do not guess a
        // bound connection from that lossy delimiter or reuse another adapter's
        // namespace. Shield's own canonical tools retain their confirmation.
        let operation = if shield_tool.is_some() {
            None
        } else {
            tools::operation(tool, input, cwd)?.into_operation()
        };
        match event {
            Event::Completed if host["toolResult"]["resultType"] != "success" => return None,
            Event::Failed => {
                text(&host["error"], 65_536)?;
            }
            _ => {}
        }
        Some(Request {
            event,
            event_name: match event {
                Event::Before => "preToolUse",
                Event::Completed => "postToolUse",
                Event::Failed => "postToolUseFailure",
                _ => return None,
            },
            tool,
            input,
            tool_id: None,
            shield_tool,
            session: text(&host["sessionId"], 4096)?,
            cwd: cwd.into(),
            operation,
            // Reused shell IDs can retain an earlier start directory. The
            // envelope gives the session cwd, not that shell's directory.
            context_complete: !matches!(tool, "bash" | "powershell")
                || input.get("shellId").is_none(),
            working_files: None,
            harmless: false,
        })
    }
    fn working_file_categories(&self) -> &'static [&'static str] {
        &["temporary", "skills"]
    }
    fn working_files(&self, _: &Value, project: &Path) -> crate::location::WorkingFiles {
        let config = super::working_files::configuration("COPILOT_HOME", ".copilot");
        let mut scope = super::working_files::temporary(config.as_deref());
        let mut candidates = vec![
            (project.to_path_buf(), ".github/skills"),
            (project.to_path_buf(), ".agents/skills"),
        ];
        candidates.extend(config.map(|config| (config, "skills")));
        scope
            .skills
            .extend(super::working_files::skill_directories(candidates));
        scope
    }
    fn approval(&self, _: Event) -> NativeApproval {
        NativeApproval::Prompt
    }
    fn reply(&self, _: Event, reply: &Reply) -> Value {
        let mut value = json!({"permissionDecision":match reply.decision {
            Decision::Allow => "allow", Decision::Ask => "ask", Decision::Deny => "deny"
        }});
        if !reply.reason.is_empty() {
            value["permissionDecisionReason"] = json!(reply.reason);
        }
        if let Some(context) = &reply.agent_context {
            // preToolUse's documented output has no separate context channel.
            let reason = value["permissionDecisionReason"]
                .as_str()
                .unwrap_or_default();
            value["permissionDecisionReason"] = json!(if reason.is_empty() {
                context.clone()
            } else {
                format!("{reason}\n{context}")
            });
        }
        value
    }
    fn completion(&self, _: &str, note: Option<&str>) -> Value {
        note.map_or_else(|| json!({}), |note| json!({"additionalContext":note}))
    }
    fn hook_group(&self, command: &str) -> Value {
        json!({"type":"command","command":command,"timeoutSec":10})
    }
    fn direct_hook_group(&self, _: &Path, _: &Path, event: &str) -> Option<Value> {
        Some(
            json!({"type":"command","exec":"shield","args":["__internal","native-hook","--project",".","--agent",SPEC.id,"--event",event],"timeoutSec":10}),
        )
    }
    fn capture_hooks(
        &self,
        source: Option<&str>,
        expected: &super::setup::HookGroups,
    ) -> Result<super::setup::HookGroups, Error> {
        setup::capture(source, expected)
    }
    fn merge_hooks(
        &self,
        source: Option<&str>,
        old: &super::setup::HookGroups,
        new: &super::setup::HookGroups,
    ) -> Result<String, Error> {
        setup::merge(source, old, new, false)
    }
    fn repair_hooks(
        &self,
        source: Option<&str>,
        desired: &super::setup::HookGroups,
    ) -> Result<String, Error> {
        setup::merge(
            source,
            &std::collections::BTreeMap::default(),
            desired,
            true,
        )
    }
    fn mcp_entry(&self, _: &Path, root: &Path, _: &[String]) -> String {
        json!({"type":"stdio","command":"shield","args":super::setup::serve_args(root, SPEC.id),"tools":["*"]}).to_string()
    }
    fn has_mcp_entry(&self, source: &str) -> Result<bool, Error> {
        super::json_mcp::present(source)
    }
    fn merge_mcp(
        &self,
        source: Option<&str>,
        old: Option<&str>,
        new: Option<&str>,
    ) -> Result<String, Error> {
        super::json_mcp::merge(source, old, new)
    }
    fn diagnose(
        &self,
        source: Option<&str>,
        owned: &super::setup::HookGroups,
        _: Option<&str>,
        diagnostics: &mut Vec<&'static str>,
    ) -> Result<(), Error> {
        if setup::capture(source, owned)? != *owned {
            diagnostics.push("Shield native hooks are missing");
        }
        Ok(())
    }
}
