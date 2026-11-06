//! Gemini CLI v0.67.0 native protocol and project configuration.
//! Submitted paths use the best available host context, with missing context recorded.
use super::{AgentAdapter, AgentSpec, Decision, Event, Reply, Request, text};
use crate::{enforcement::protocol::NativeApproval, review::Error};
use serde_json::{Value, json};
use std::path::Path;
mod session;
mod tools;
mod web;

pub(super) struct Gemini;
const HTTP: super::json_mcp::Http = super::json_mcp::Http {
    key: "url",
    legacy: Some("httpUrl"),
    discriminator: Some(("type", "http")),
};
static SPEC: AgentSpec = AgentSpec {
    id: "gemini",
    label: "Gemini CLI",
    events: &[
        ("BeforeTool", Event::Before),
        ("AfterTool", Event::Completed),
    ],
    hooks_path: ".gemini/settings.json",
    mcp_path: Some(".gemini/settings.json"),
    settings_paths: &[],
    prefix_rules_path: None,
    needs_ask_fallback: false,
    approval_notes: false,
    approval_action: "Allow this request?",
};

impl AgentAdapter for Gemini {
    fn native_forms_unavailable(&self, version: Option<&str>) -> bool {
        version == Some("0.67.0")
    }
    fn supports_box_hooks(&self) -> bool {
        true
    }
    fn box_hook(&self, command: &str) -> Option<Value> {
        Some(json!({"matcher":".*","command":command,"timeout":10,
            "name":"shield-project-enforcement","description":"Shield project enforcement"}))
    }
    fn review(&self) -> &'static dyn super::review::ReviewAdapter {
        &super::review::FORM
    }
    fn invalid_input_guidance(&self) -> Option<&'static str> {
        Some(
            "Check the tool's required fields and use a narrower file pattern or explicit directory when a search footprint cannot be represented. Revise the tool arguments; this does not need a broader policy rule.",
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
        super::json_mcp::check_scope(
            &system_settings_path(std::env::var_os("GEMINI_CLI_SYSTEM_SETTINGS_PATH"))?,
            alias,
        )?;
        let base = super::working_files::configuration("GEMINI_CLI_HOME", "")
            .filter(|p| p.is_absolute())
            .ok_or(Error::Invalid("cannot resolve Gemini configuration scope"))?;
        super::json_mcp::check_scope(&base.join(".gemini/settings.json"), alias)?;
        for layer in super::mcp_scopes::project_layers(root, cwd)?.iter().skip(1) {
            super::json_mcp::check_scope(&layer.join(".gemini/settings.json"), alias)?;
        }
        Ok(())
    }
    fn spec(&self) -> &'static AgentSpec {
        &SPEC
    }
    fn is_installed(&self, paths: &[std::path::PathBuf]) -> bool {
        super::detection::on_path("gemini", paths)
    }
    fn request<'a>(&self, host: &'a Value) -> Option<Request<'a>> {
        normalize(host, None, false)
    }
    fn request_for_project<'a>(
        &self,
        host: &'a Value,
        _event: Event,
        project: &Path,
    ) -> Option<Request<'a>> {
        if let Some(runtime) = runtime()
            && session::directory(host, project, &runtime).is_some()
        {
            normalize(host, Some(project.to_str()?), true)
        } else {
            self.request(host)
        }
    }
    fn working_file_categories(&self) -> &'static [&'static str] {
        &["temporary", "skills", "session_tool_results"]
    }
    fn working_files(&self, host: &Value, project: &Path) -> crate::location::WorkingFiles {
        let base = super::working_files::configuration("GEMINI_CLI_HOME", "");
        let config = base.as_ref().map(|base| base.join(".gemini"));
        let mut scope = super::working_files::temporary(config.as_deref());
        let mut candidates = vec![
            (project.to_path_buf(), ".gemini/skills"),
            (project.to_path_buf(), ".agents/skills"),
        ];
        if let Some(base) = base {
            scope.protected.push(base.join(".agents"));
            candidates.extend([(base.clone(), ".gemini/skills"), (base, ".agents/skills")]);
        }
        scope
            .skills
            .extend(super::working_files::skill_directories(candidates));
        if let Some(runtime) = runtime() {
            scope.protected.push(runtime.clone());
            scope.read.extend(session::outputs(host, project, &runtime));
        }
        scope
    }
    fn approval(&self, _: Event) -> NativeApproval {
        NativeApproval::Prompt
    }
    fn reply(&self, _: Event, reply: &Reply) -> Value {
        before_reply(reply)
    }
    fn completion(&self, _: &str, note: Option<&str>) -> Value {
        completion(note)
    }
    fn hook_group(&self, command: &str) -> Value {
        hook_group(command)
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
        old: &super::setup::HookGroups,
        new: &super::setup::HookGroups,
    ) -> Result<String, Error> {
        super::setup::merge(source, old, new)
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
        let config = super::setup::diagnose(source, owned, diagnostics)?;
        if config["hooksConfig"]["enabled"] == false {
            diagnostics.push("native hooks are disabled in project settings");
        }
        if config["hooksConfig"]["disabled"]
            .as_array()
            .is_some_and(|names| {
                names
                    .iter()
                    .any(|name| name == "shield-project-enforcement")
            })
        {
            diagnostics.push("Shield native hook is disabled in project settings");
        }
        Ok(())
    }
}

fn system_settings_path(
    override_path: Option<std::ffi::OsString>,
) -> Result<std::path::PathBuf, Error> {
    let path = if let Some(value) = override_path.filter(|value| !value.is_empty()) {
        std::path::PathBuf::from(value)
    } else {
        #[cfg(target_os = "macos")]
        let default = "/Library/Application Support/GeminiCli/settings.json";
        #[cfg(windows)]
        let default = r"C:\ProgramData\gemini-cli\settings.json";
        #[cfg(not(any(target_os = "macos", windows)))]
        let default = "/etc/gemini-cli/settings.json";
        std::path::PathBuf::from(default)
    };
    // Relative overrides depend on Gemini's startup directory, not the tool cwd.
    if !path.is_absolute() {
        return Err(Error::Invalid(
            "Gemini system settings path must be absolute to verify MCP scope",
        ));
    }
    Ok(path)
}

fn runtime() -> Option<std::path::PathBuf> {
    let base = super::working_files::configuration("GEMINI_CLI_HOME", "")?;
    Some(
        if std::env::var_os("SANDBOX").is_some_and(|v| v == "sandbox-exec") {
            base.join(".cache/.gemini")
        } else {
            base.join(".gemini")
        },
    )
}

fn normalize<'a>(host: &'a Value, base: Option<&str>, verified: bool) -> Option<Request<'a>> {
    let envelope = envelope(host)?;
    let base = base.unwrap_or(envelope.cwd);
    let shield_tool = envelope
        .mcp
        .filter(|(server, name)| *server == "shield" && crate::review::mcp::is_tool(name))
        .map(|(_, name)| name);
    if envelope.mcp.is_some_and(|(server, _)| server == "shield") && shield_tool.is_none() {
        return None;
    }
    let operation = if shield_tool.is_some() {
        None
    } else if let Some((alias, name)) = envelope.mcp {
        Some(json!({"kind":"mcp","alias":alias,"tool":name,"arguments":envelope.input}))
    } else {
        tools::operation(envelope.tool, envelope.input, base)?.into_operation()
    };
    let cwd = if envelope.mcp.is_none() && envelope.tool == "run_shell_command" {
        match envelope.input.get("dir_path") {
            Some(value) => tools::absolute(value, base)?,
            None => base.to_owned(),
        }
    } else {
        base.to_owned()
    };
    Some(Request {
        event: if envelope.event == Event::Completed && host["tool_response"]["error"].is_object() {
            Event::Failed
        } else {
            envelope.event
        },
        event_name: text(&host["hook_event_name"], 128)?,
        tool: envelope.tool,
        input: envelope.input,
        tool_id: None,
        shield_tool,
        session: envelope.session,
        cwd: cwd.into(),
        operation,
        context_complete: verified
            || envelope.mcp.is_some()
            || (envelope.tool == "run_shell_command"
                && envelope.input["dir_path"]
                    .as_str()
                    .is_some_and(|s| Path::new(s).is_absolute())),
        working_files: None,
        harmless: false,
    })
}

struct Envelope<'a> {
    event: Event,
    session: &'a str,
    cwd: &'a str,
    tool: &'a str,
    input: &'a Value,
    mcp: Option<(&'a str, &'a str)>,
}

/// Gemini supplies session identity but no tool-call ID. Keep the vendor
/// envelope separate from the shared Claude/Codex envelope, which requires one.
/// The session working directory is fallback context, not execution attestation.
fn envelope(host: &Value) -> Option<Envelope<'_>> {
    let event = match text(&host["hook_event_name"], 128)? {
        "BeforeTool" => Event::Before,
        "AfterTool" => Event::Completed,
        _ => return None,
    };
    let cwd = text(&host["cwd"], 4096)?;
    Path::new(cwd).is_absolute().then_some(())?;
    let tool = text(&host["tool_name"], 128)?;
    host["tool_input"].as_object()?;
    if event == Event::Completed {
        host["tool_response"].as_object()?;
    }
    let mcp = match host.get("mcp_context") {
        Some(context) => {
            let server = text(&context["server_name"], 128)?;
            (server == "shield" || crate::mcp_connections::valid_name(server)).then_some(())?;
            Some((server, text(&context["tool_name"], 128)?))
        }
        // Underscores in server and tool names make splitting this name unsafe.
        None if tool.starts_with("mcp_") => return None,
        None => None,
    };
    Some(Envelope {
        event,
        session: text(&host["session_id"], 4096)?,
        cwd,
        tool,
        input: &host["tool_input"],
        mcp,
    })
}

/// `BeforeTool` consumes top-level decisions. Denial feedback must be in reason:
/// its scheduler does not consume `BeforeTool` `additionalContext` as agent guidance.
fn before_reply(reply: &Reply) -> Value {
    match reply.decision {
        Decision::Allow => json!({"decision":"allow"}),
        Decision::Ask => {
            json!({"decision":"ask","reason":reply.reason,"systemMessage":reply.reason})
        }
        Decision::Deny => {
            let reason = [Some(reply.reason.as_str()), reply.agent_context.as_deref()]
                .into_iter()
                .flatten()
                .filter(|text| !text.trim().is_empty())
                .collect::<Vec<_>>()
                .join("\n");
            json!({"decision":"deny","reason":reason,"systemMessage":reply.reason})
        }
    }
}

fn completion(note: Option<&str>) -> Value {
    note.filter(|note| !note.trim().is_empty()).map_or_else(
        || json!({}),
        |note| json!({"hookSpecificOutput":{"hookEventName":"AfterTool","additionalContext":note}}),
    )
}

fn hook_group(command: &str) -> Value {
    // Gemini measures hook timeouts in milliseconds, unlike Claude's seconds.
    json!({"matcher":".*","hooks":[{"name":"shield-project-enforcement",
        "type":"command","command":command,"timeout":10000,
        "description":"Shield project enforcement"}]})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gemini_system_settings_override_requires_absolute_path() {
        let path = std::env::temp_dir().join("acme-system-settings.json");
        assert_eq!(
            system_settings_path(Some(path.clone().into_os_string())).unwrap(),
            path
        );
        assert!(system_settings_path(Some("relative/settings.json".into())).is_err());
        let default = system_settings_path(None).unwrap();
        assert!(default.is_absolute());
        assert_eq!(system_settings_path(Some("".into())).unwrap(), default);
    }

    fn input() -> Value {
        json!({"hook_event_name":"BeforeTool","session_id":"acme-session",
            "cwd":std::env::temp_dir(),"transcript_path":"","timestamp":"2026-11-05T00:00:00Z",
            "tool_name":"run_shell_command","tool_input":{"command":"pwd","dir_path":std::env::temp_dir()}})
    }

    #[test]
    fn gemini_requests_preserve_available_context_and_mcp_authority() {
        let mut host = input();
        let request = Gemini.request(&host).unwrap();
        assert_eq!(
            request.cwd.as_ref(),
            host["tool_input"]["dir_path"].as_str().unwrap()
        );
        assert!(request.tool_id.is_none());
        host["tool_input"]
            .as_object_mut()
            .unwrap()
            .remove("dir_path");
        assert!(!Gemini.request(&host).unwrap().context_complete);
        host["tool_input"]["dir_path"] = json!("relative");
        assert_eq!(
            Gemini.request(&host).unwrap().cwd.as_ref(),
            Path::new(host["cwd"].as_str().unwrap())
                .join("relative")
                .to_str()
                .unwrap()
        );
        host["tool_name"] = json!("read_file");
        host["tool_input"] = json!({"file_path":std::env::temp_dir().join("acme.txt")});
        assert_eq!(
            Gemini.request(&host).unwrap().operation.unwrap()["action"],
            "read"
        );
        host["tool_input"]["file_path"] = json!("relative.txt");
        assert_eq!(
            Gemini.request(&host).unwrap().operation.unwrap()["path"],
            std::env::temp_dir().join("relative.txt").to_str().unwrap()
        );
        host["tool_name"] = json!("mcp_acme_tools_read_file");
        host["mcp_context"] = json!({"server_name":"acme_tools","tool_name":"read_file"});
        let mapped = Gemini.request(&host).unwrap().operation.unwrap();
        assert_eq!(mapped["alias"], "acme_tools");
        assert_eq!(mapped["arguments"], host["tool_input"]);
        host["mcp_context"] = json!({"server_name":"shield","tool_name":"policy_show"});
        let request = Gemini.request(&host).unwrap();
        assert_eq!(request.shield_tool, Some("policy_show"));
        assert!(request.operation.is_none());
        assert!(
            tools::operation(
                "glob",
                &json!({"dir_path":std::env::temp_dir(),"pattern":"**/../outside"}),
                host["cwd"].as_str().unwrap()
            )
            .is_none()
        );
    }

    #[test]
    fn gemini_envelope_preserves_host_facts_without_fabricating_a_tool_call_id() {
        let host = input();
        assert!(host.get("tool_use_id").is_none());
        let parsed = envelope(&host).unwrap();
        assert!(parsed.event == Event::Before);
        assert_eq!(parsed.session, "acme-session");
        assert_eq!(parsed.cwd, host["cwd"]);
        assert_eq!(parsed.tool, "run_shell_command");
        assert_eq!(parsed.input, &host["tool_input"]);
        assert!(parsed.mcp.is_none());
        let mut post = host;
        post["hook_event_name"] = json!("AfterTool");
        assert!(envelope(&post).is_none());
        post["tool_response"] = json!({"llmContent":"Acme output","returnDisplay":"Acme"});
        assert!(envelope(&post).unwrap().event == Event::Completed);
    }

    #[test]
    fn gemini_mcp_identity_uses_host_metadata_instead_of_splitting_underscores() {
        let mut host = input();
        host["tool_name"] = json!("mcp_acme_tools_read_file");
        assert!(envelope(&host).is_none());
        host["mcp_context"] =
            json!({"server_name":"acme_tools","tool_name":"read_file","command":"acme-mcp"});
        assert_eq!(
            envelope(&host).unwrap().mcp,
            Some(("acme_tools", "read_file"))
        );
        host["tool_name"] = json!("mcp_shield_policy_show");
        host["mcp_context"] = json!({"server_name":"shield","tool_name":"policy_show"});
        assert_eq!(
            envelope(&host).unwrap().mcp,
            Some(("shield", "policy_show"))
        );
        for invalid in [
            Value::Null,
            json!({}),
            json!({"server_name":"../acme","tool_name":"read"}),
        ] {
            host["mcp_context"] = invalid;
            assert!(envelope(&host).is_none());
        }
    }

    #[test]
    fn gemini_envelope_rejects_malformed_host_fields() {
        for (field, value) in [
            ("hook_event_name", json!("PreToolUse")),
            ("session_id", json!("")),
            ("cwd", json!("relative")),
            ("tool_name", json!("read\0file")),
            ("tool_input", json!([])),
        ] {
            let mut host = input();
            host[field] = value;
            assert!(envelope(&host).is_none(), "{field}: {host}");
        }
    }

    #[test]
    fn gemini_decisions_preserve_approval_and_deliver_denial_recovery_in_reason() {
        for (decision, wire) in [
            (Decision::Allow, "allow"),
            (Decision::Ask, "ask"),
            (Decision::Deny, "deny"),
        ] {
            let output = before_reply(&Reply {
                decision,
                reason: "Shield: Acme decision.".into(),
                agent_context: Some("Search the existing skill folder and retry.".into()),
            });
            assert_eq!(output["decision"], wire);
            assert!(output.get("continue").is_none());
            assert!(output.get("hookSpecificOutput").is_none());
            if decision == Decision::Deny {
                assert!(
                    output["reason"]
                        .as_str()
                        .unwrap()
                        .contains("skill folder and retry")
                );
            }
            if decision == Decision::Ask {
                assert_eq!(output["systemMessage"], "Shield: Acme decision.");
            }
        }
        assert_eq!(completion(None), json!({}));
        assert_eq!(completion(Some("  ")), json!({}));
        assert_eq!(
            completion(Some("Acme note"))["hookSpecificOutput"]["additionalContext"],
            "Acme note"
        );
    }

    #[test]
    fn gemini_hook_codec_preserves_unowned_settings_and_uses_millisecond_timeouts() {
        let command =
            "shield __internal native-hook --project /srv/acme --agent gemini --event BeforeTool";
        let group = hook_group(command);
        assert_eq!(group["hooks"][0]["timeout"], 10000);
        assert!(group["hooks"][0].get("statusMessage").is_none());
        let before = json!({"hooks":{"enabled":true,"BeforeTool":[{"matcher":"read_file",
            "hooks":[{"name":"acme-audit","type":"command","command":"acme-audit"}]}]},
            "mcpServers":{"acme":{"command":"acme-mcp"}},"ui":{"theme":"acme"}})
        .to_string();
        let empty = super::super::setup::HookGroups::new();
        let desired = [("BeforeTool".into(), group)].into_iter().collect();
        let merged = super::super::setup::merge(Some(&before), &empty, &desired).unwrap();
        let captured = super::super::setup::capture(Some(&merged), &desired).unwrap();
        assert_eq!(captured, desired);
        let removed = super::super::setup::merge(Some(&merged), &captured, &empty).unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&removed).unwrap(),
            serde_json::from_str::<Value>(&before).unwrap()
        );
        let edited = merged.replace(command, "shield-edited __internal native-hook");
        assert!(super::super::setup::merge(Some(&edited), &captured, &desired).is_err());
    }
}
