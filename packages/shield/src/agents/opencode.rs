//! `OpenCode` V1's owned plugin transport, separate from its V2 plugin API.
use super::{AgentAdapter, AgentSpec, Decision, Event, Reply, Request, text};
use crate::{enforcement::protocol::NativeApproval, review::Error};
use serde_json::{Value, json};
use std::path::Path;

mod setup;
#[cfg(test)]
mod tests;
mod tools;

pub(super) struct OpenCode;
static SPEC: AgentSpec = AgentSpec {
    id: "opencode",
    label: "OpenCode V1",
    events: &[
        ("tool.execute.before", Event::Before),
        ("tool.execute.after", Event::Completed),
    ],
    hooks_path: ".opencode/plugins/shield.js",
    mcp_path: Some("opencode.json"),
    settings_paths: &[
        "opencode.jsonc",
        ".opencode/opencode.json",
        ".opencode/opencode.jsonc",
    ],
    prefix_rules_path: None,
    needs_ask_fallback: true,
    approval_notes: false,
    approval_action: "OpenCode V1 cannot prompt from its tool hook. Propose a narrow policy rule for recurring requests.",
};

impl AgentAdapter for OpenCode {
    fn spec(&self) -> &'static AgentSpec {
        &SPEC
    }
    fn default_ask_fallback(&self) -> crate::review::integrations::AskFallback {
        crate::review::integrations::AskFallback::Block
    }
    fn request<'a>(&self, host: &'a Value) -> Option<Request<'a>> {
        if host["protocol_version"] != 1 {
            return None;
        }
        let event_name = text(&host["hook_event_name"], 128)?;
        let event = self.event(event_name)?;
        let tool = text(&host["tool"], 256)?;
        let input = &host["arguments"];
        input.as_object()?;
        let base = text(&host["cwd"], 4096)?;
        Path::new(base).is_absolute().then_some(())?;
        let cwd = if tool == "bash" {
            match input.get("workdir") {
                None => base.to_owned(),
                Some(value) => {
                    let path = Path::new(text(value, 4096)?);
                    if path.is_absolute() {
                        path.to_str()?.into()
                    } else {
                        Path::new(base).join(path).to_str()?.into()
                    }
                }
            }
        } else {
            base.to_owned()
        };
        let shell = host.get("shell").and_then(Value::as_str);
        let shield_tool = tool
            .strip_prefix("shield_")
            .filter(|name| crate::review::mcp::is_tool(name));
        let operation = if shield_tool.is_some() {
            None
        } else {
            Some(tools::operation(tool, input, &cwd, shell)?)
        };
        Some(Request {
            event,
            event_name,
            tool,
            input,
            tool_id: Some(text(&host["callID"], 4096)?),
            shield_tool,
            session: text(&host["sessionID"], 4096)?,
            cwd: cwd.into(),
            operation,
            // Mutable plugin configuration and shell startup files do not
            // establish the actual launch cwd, dialect or command resolution.
            context_complete: tool != "bash",
            working_files: None,
            harmless: false,
        })
    }
    fn working_file_categories(&self) -> &'static [&'static str] {
        &["temporary"]
    }
    fn working_files(&self, _: &Value, _: &Path) -> crate::location::WorkingFiles {
        super::working_files::temporary(None)
    }
    fn approval(&self, _: Event) -> NativeApproval {
        NativeApproval::Fallback
    }
    fn reply(&self, event: Event, reply: &Reply) -> Value {
        let reason = reply.agent_context.as_ref().map_or_else(
            || reply.reason.clone(),
            |context| format!("{}\n{context}", reply.reason),
        );
        json!({"protocol_version":1,"event":if event==Event::Before {"tool.execute.before"}else{"tool.execute.after"},"decision":match reply.decision {Decision::Allow=>"allow",Decision::Ask=>"ask",Decision::Deny=>"deny"},"reason":reason})
    }
    fn completion(&self, _: &str, note: Option<&str>) -> Value {
        json!({"protocol_version":1,"event":"tool.execute.after","decision":"observed","reason":note.unwrap_or_default()})
    }
    fn hook_group(&self, _: &str) -> Value {
        json!({"protocol_version":1,"source":setup::plugin()})
    }
    fn direct_hook_group(&self, _: &Path, _: &Path, _: &str) -> Option<Value> {
        Some(self.hook_group(""))
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
        setup::merge(source, old, new)
    }
    fn mcp_entry(&self, _: &Path, root: &Path, _: &[String]) -> String {
        let command: Vec<String> = std::iter::once("shield".to_owned())
            .chain(super::setup::serve_args(root, SPEC.id))
            .collect();
        json!({"type":"local","command":command,"enabled":true}).to_string()
    }
    fn has_mcp_entry(&self, source: &str) -> Result<bool, Error> {
        setup::has_mcp(source)
    }
    fn borrow_mcp_entry(
        &self,
        source: &str,
        binary: &Path,
        root: &Path,
    ) -> Result<Option<String>, Error> {
        setup::borrow_mcp(source, binary, root)
    }
    fn merge_mcp(
        &self,
        source: Option<&str>,
        old: Option<&str>,
        new: Option<&str>,
    ) -> Result<String, Error> {
        setup::merge_mcp(source, old, new)
    }
    fn diagnose(
        &self,
        source: Option<&str>,
        owned: &super::setup::HookGroups,
        _: Option<&str>,
        diagnostics: &mut Vec<&'static str>,
    ) -> Result<(), Error> {
        if setup::capture(source, owned)? != *owned {
            diagnostics.push("Shield OpenCode V1 plugin is missing");
        }
        Ok(())
    }
}
