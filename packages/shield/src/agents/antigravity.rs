//! Antigravity CLI 1.5.1 native protocol. Hook events are pinned by installation;
//! the vendor payload does not contain an event name or a tool invocation ID.
use super::{AgentAdapter, AgentSpec, Decision, Event, Reply, Request, text};
use crate::{enforcement::protocol::NativeApproval, review::Error};
use serde_json::{Value, json};
use std::path::Path;

mod artifacts;
mod setup;
pub(super) struct Antigravity;
const HTTP: super::json_mcp::Http = super::json_mcp::Http {
    key: "serverUrl",
    legacy: Some("url"),
    discriminator: None,
};
static SPEC: AgentSpec = AgentSpec {
    id: "agy",
    label: "Antigravity CLI",
    events: &[
        ("PreToolUse", Event::Before),
        ("PostToolUse", Event::Completed),
    ],
    hooks_path: ".agents/hooks.json",
    mcp_path: Some(".agents/mcp_config.json"),
    settings_paths: &[],
    prefix_rules_path: None,
    needs_ask_fallback: false,
    approval_notes: false,
    approval_action: "Allow this request?",
};

impl AgentAdapter for Antigravity {
    fn native_forms_unavailable(&self, version: Option<&str>) -> bool {
        version == Some("1.5.1")
    }
    fn supports_box_hooks(&self) -> bool {
        true
    }
    fn box_hook(&self, command: &str) -> Option<Value> {
        Some(json!({"matcher":"*","command":command,"timeout":10,
            "name":"shield-project-enforcement"}))
    }
    fn mcp_session<'a>(&self, metadata: &'a serde_json::Map<String, Value>) -> Option<&'a str> {
        let session = text(metadata.get("antigravity.google/conversation_id")?, 128)?;
        session
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
            .then_some(session)
    }
    fn review(&self) -> &'static dyn super::review::ReviewAdapter {
        &super::review::FORM
    }
    fn invalid_input_guidance(&self) -> Option<&'static str> {
        Some(
            "Check the tool's required path fields and use an absolute Cwd when selecting a shell directory. Shield cannot infer a reused terminal's working directory. Revise malformed tool arguments; do not propose a broader policy rule to work around missing host context.",
        )
    }
    fn merge_connections(
        &self,
        source: &str,
        previous: &Value,
        desired: &std::collections::BTreeMap<String, crate::mcp_connections::Transport>,
    ) -> Result<super::SettingsUpdate, Error> {
        super::json_mcp::connections(source, previous, desired, HTTP)
    }
    fn connection(
        &self,
        source: &str,
        alias: &str,
    ) -> Result<Option<crate::mcp_connections::Transport>, Error> {
        super::json_mcp::connection(source, alias, HTTP)
    }
    fn check_mcp_scopes(&self, root: &Path, cwd: &Path, alias: &str) -> Result<(), Error> {
        let home = std::env::home_dir().ok_or(Error::Invalid(
            "cannot resolve Antigravity configuration scope",
        ))?;
        super::json_mcp::check_scope(&home.join(".gemini/config/mcp_config.json"), alias)?;
        for layer in super::mcp_scopes::project_layers(root, cwd)?.iter().skip(1) {
            super::json_mcp::check_scope(&layer.join(".agents/mcp_config.json"), alias)?;
        }
        Ok(())
    }
    fn spec(&self) -> &'static AgentSpec {
        &SPEC
    }
    fn is_installed(&self, paths: &[std::path::PathBuf]) -> bool {
        super::detection::on_path("agy", paths)
    }
    fn input_event_for(&self, host: &Value, expected: Event) -> Option<Event> {
        // Do not accept an injected event discriminator from another host.
        if host.get("hook_event_name").is_some() {
            return None;
        }
        matches!(expected, Event::Before | Event::Completed).then_some(expected)
    }
    fn request<'a>(&self, host: &'a Value) -> Option<Request<'a>> {
        self.request_for_event(host, Event::Before)
    }
    fn request_for_event<'a>(&self, host: &'a Value, event: Event) -> Option<Request<'a>> {
        self.input_event_for(host, event)?;
        let paths = host["workspacePaths"].as_array()?;
        if paths.is_empty() || paths.len() > 64 || paths.iter().any(|p| absolute(p).is_none()) {
            return None;
        }
        host["stepIdx"].as_u64()?;
        let input = &host["toolCall"]["args"];
        input.as_object()?;
        let tool = text(&host["toolCall"]["name"], 128)?;
        let complete_shell = input.get("RequestedTerminalID").is_none()
            && (input.get("Cwd").is_some() || paths.len() == 1);
        let ambiguous_file = paths.len() > 1
            && matches!(tool, "read_file" | "edit_file" | "write_file")
            && absolute(&input["Path"]).is_none();
        let cwd = if tool == "run_command" {
            match input.get("Cwd") {
                Some(value) => absolute(value)?,
                // Verified on CLI 1.5.1: new commands start in the sole
                // workspace even after a prior command changed directory.
                None => absolute(&paths[0])?,
            }
        } else {
            absolute(&paths[0])?
        };
        let mcp = if tool == "call_mcp_tool" {
            let alias = text(&input["ServerName"], 128)?;
            (alias == "shield" || crate::mcp_connections::valid_name(alias)).then_some(())?;
            let name = text(&input["ToolName"], 128)?;
            input["Arguments"].as_object()?;
            Some((alias, name))
        } else {
            None
        };
        let shield_tool = mcp
            .filter(|(alias, name)| *alias == "shield" && crate::review::mcp::is_tool(name))
            .map(|(_, name)| name);
        if mcp.is_some_and(|(alias, _)| alias == "shield") && shield_tool.is_none() {
            return None;
        }
        let operation = if shield_tool.is_some() {
            None
        } else if let Some((alias, name)) = mcp {
            Some(json!({"kind":"mcp","alias":alias,"tool":name,"arguments":input["Arguments"]}))
        } else {
            let operation = operation_at(tool, input, cwd)?.into_operation();
            if (tool == "run_command" && !complete_shell) || ambiguous_file {
                operation.map(
                    |operation| json!({"kind":"batch","operations":[operation],"unmodeled":true}),
                )
            } else {
                operation
            }
        };
        if event == Event::Completed && host.get("error").is_some_and(|e| !e.is_string()) {
            return None;
        }
        // A trajectory index is not promised to be a stable before/after call ID.
        // Retain absent correlation rather than inventing one.
        Some(Request {
            event: if event == Event::Completed
                && host["error"].as_str().is_some_and(|s| !s.is_empty())
            {
                Event::Failed
            } else {
                event
            },
            event_name: if event == Event::Before {
                "PreToolUse"
            } else {
                "PostToolUse"
            },
            tool,
            input: if shield_tool.is_some() {
                &input["Arguments"]
            } else {
                input
            },
            tool_id: None,
            shield_tool,
            session: text(&host["conversationId"], 4096)?,
            cwd: cwd.into(),
            operation,
            context_complete: !ambiguous_file
                && if tool == "run_command" {
                    complete_shell
                } else {
                    paths.len() == 1
                        || input["Cwd"]
                            .as_str()
                            .is_some_and(|p| Path::new(p).is_absolute())
                        || input["Path"]
                            .as_str()
                            .is_some_and(|p| Path::new(p).is_absolute())
                },
            working_files: None,
            harmless: false,
        })
    }
    fn working_file_categories(&self) -> &'static [&'static str] {
        &["temporary", "skills", "session_tool_results"]
    }
    fn working_files(&self, host: &Value, project: &Path) -> crate::location::WorkingFiles {
        let config = std::env::home_dir().map(|home| home.join(".gemini"));
        let mut scope = super::working_files::temporary(config.as_deref());
        let mut candidates = vec![
            (project.to_path_buf(), ".agents/skills"),
            (project.to_path_buf(), ".agent/skills"),
        ];
        if let Some(config) = config {
            if let Some(root) = artifacts::root(host, project, &config) {
                scope.read.push(root);
            }
            candidates.extend([
                (config.clone(), "config/skills"),
                (config.clone(), "skills"),
                (config, "antigravity-cli/skills"),
            ]);
        }
        scope
            .skills
            .extend(super::working_files::skill_directories(candidates));
        scope
    }
    fn approval(&self, _: Event) -> NativeApproval {
        NativeApproval::Prompt
    }
    fn reply(&self, _: Event, reply: &Reply) -> Value {
        let reason = [Some(reply.reason.as_str()), reply.agent_context.as_deref()]
            .into_iter()
            .flatten()
            .filter(|s| !s.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        json!({"decision":match reply.decision {Decision::Allow=>"auto_approve",Decision::Ask=>"force_ask",Decision::Deny=>"deny"},"reason":reason})
    }
    fn completion(&self, _: &str, _: Option<&str>) -> Value {
        json!({})
    }
    fn pack_offer_instruction(&self) -> Option<&'static str> {
        None
    }
    fn hook_group(&self, command: &str) -> Value {
        json!({"matcher":"*","hooks":[{"type":"command","command":command,"timeout":10}]})
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
    fn mcp_entry(&self, binary: &Path, root: &Path, _: &[String]) -> String {
        json!({"command":binary,"args":["mcp","serve","--project",root,"--agent",SPEC.id]})
            .to_string()
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
        setup::diagnose(source, owned, diagnostics)
    }
}

fn absolute(value: &Value) -> Option<&str> {
    let value = text(value, 4096)?;
    Path::new(value).is_absolute().then_some(value)
}

#[cfg(test)]
fn operation(tool: &str, input: &Value) -> Option<super::ToolMapping> {
    operation_at(tool, input, "")
}
fn operation_at(tool: &str, input: &Value, base: &str) -> Option<super::ToolMapping> {
    let file = |key, write, recursive| {
        Some(super::ToolMapping::Operation(
            json!({"kind":"file","action":if write {"write"} else {"read"},"path":absolute(&input[key])?,"recursive":recursive}),
        ))
    };
    match tool {
        "run_command" => Some(super::ToolMapping::Operation(
            json!({"kind":"shell","source":text(&input["CommandLine"],65536)?,"dialect":"bash"}),
        )),
        "read_file" | "edit_file" | "write_file" => {
            let value = text(&input["Path"], 4096)?;
            let path = if Path::new(value).is_absolute() {
                Path::new(value).to_path_buf()
            } else {
                if !Path::new(base).is_absolute() {
                    return None;
                }
                Path::new(base).join(value)
            };
            Some(super::ToolMapping::Operation(
                json!({"kind":"file","action":if tool=="read_file" {"read"} else {"write"},"path":path,"recursive":false}),
            ))
        }
        "view_file" | "view_file_outline" => file("AbsolutePath", false, false),
        "view_code_item" => file("File", false, false),
        // Input to an existing terminal can execute code in an unknown shell/cwd.
        "send_command_input" => None,
        "write_to_file" | "replace_file_content" | "multi_replace_file_content" => {
            file("TargetFile", true, false)
        }
        "list_dir" => file("DirectoryPath", false, true),
        "grep_search" => file("SearchPath", false, true),
        // Name search patterns can traverse outside SearchDirectory; reject that
        // ambiguity rather than authorizing just the starting tree.
        "find_by_name" => {
            let pattern = text(&input["Pattern"], 4096)?;
            if Path::new(pattern).is_absolute() || pattern.split(['/', '\\']).any(|p| p == "..") {
                return None;
            }
            file("SearchDirectory", false, true)
        }
        "search_web" => Some(super::ToolMapping::Operation(
            json!({"kind":"web","action":"search","query":text(&input["query"],8192)?,"url":null}),
        )),
        "read_url_content" => Some(super::ToolMapping::Operation(
            json!({"kind":"web","action":"fetch","query":null,"url":text(&input["Url"],8192)?}),
        )),
        _ if tool.starts_with("mcp_") => None,
        _ => Some(super::ToolMapping::Unmapped),
    }
}

#[cfg(test)]
mod tests;
