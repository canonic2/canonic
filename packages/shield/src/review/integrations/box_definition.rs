//! Shield's Box MCP definition: the shared `.box/mcp/shield.yml` Box publishes
//! to the agents each developer chose, and the private `.box/local/mcp/shield.yml`
//! that pins this Shield binary. The shared file names the bare `shield`
//! command and its hooks name `{command}`, which Box expands to the pinned
//! binary, so Box renders the same hook groups Shield writes directly.
use super::{
    Error,
    files::{self, Change},
};
#[cfg(not(windows))]
use crate::agents::{self, AgentAdapter};
use serde::{Deserialize, Serialize};
#[cfg(not(windows))]
use serde_json::{Map, Value, json};
use std::path::Path;

/// The integration ID `shield mcp setup --agent` accepts for Box.
pub(crate) const ID: &str = "box";
pub(crate) const LABEL: &str = "Box";
/// The shared, portable definition.
pub(crate) const PATH: &str = ".box/mcp/shield.yml";
/// Box's private local layer, which replaces the shared connection with the
/// pinned binary for this checkout.
pub(crate) const LOCAL_PATH: &str = ".box/local/mcp/shield.yml";
/// Box's project configuration; its presence makes a folder a Box project.
const CONFIG: &str = ".box/config.yml";
/// The command shared files name, found on `PATH`.
#[cfg(not(windows))]
const COMMAND: &str = "shield";
/// The launch arguments Box expands with the project root and agent ID.
#[cfg(not(windows))]
const ARGS: [&str; 6] = [
    "mcp",
    "serve",
    "--project",
    "{project}",
    "--agent",
    "{agent}",
];
/// The portable hook event names Box accepts.
#[cfg(not(windows))]
const EVENTS: &[&str] = &[
    "pre_tool_use",
    "post_tool_use",
    "post_tool_use_failure",
    "permission_request",
    "user_prompt_submit",
    "session_start",
    "stop",
];
/// Tools that ask for every agent: Codex always prompts before applying policy.
#[cfg(not(windows))]
const ALWAYS_ASK: &str = "policy_apply";

/// The exact texts Shield wrote, owned and diagnosed together.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::review) struct Definition {
    pub(in crate::review) shared: String,
    pub(in crate::review) local: String,
}

impl Definition {
    fn texts(&self) -> [&str; 2] {
        [&self.shared, &self.local]
    }
}

/// Whether `root` is a Box project, without reading Box's configuration.
pub(in crate::review) fn box_project(root: &Path) -> bool {
    std::fs::metadata(root.join(CONFIG)).is_ok_and(|m| m.is_file())
}

/// Box's portable name for a native event: `PreToolUse` becomes `pre_tool_use`.
#[cfg(not(windows))]
fn portable(event: &str) -> Result<String, Error> {
    let mut name = String::new();
    for (index, c) in event.chars().enumerate() {
        if c.is_ascii_uppercase() && index > 0 {
            name.push('_');
        }
        name.push(c.to_ascii_lowercase());
    }
    if !EVENTS.contains(&name.as_str()) {
        return Err(Error::Invalid(
            "an agent hook event has no Box equivalent; set the agent up directly",
        ));
    }
    Ok(name)
}

/// One adapter's hooks as a Box hook entry. Box publishes each event as
/// `{"matcher", "hooks": [{"type": "command", "command", "timeout",
/// "statusMessage"}]}`; an adapter group with any other shape is refused.
#[cfg(not(windows))]
fn hook(adapter: &dyn AgentAdapter, binary: &Path) -> Result<Value, Error> {
    let spec = adapter.spec();
    let command = super::native::posix_hook_command("{command}", "{project}", "{agent}", "{event}");
    let group = adapter.hook_group(&command);
    let (matcher, settings) = (&group["matcher"], &group["hooks"][0]);
    let expressible = matcher.is_string()
        && settings["timeout"].is_u64()
        && settings["statusMessage"].is_string()
        && group
            == json!({"matcher":matcher,"hooks":[{"type":"command","command":command,
                "timeout":settings["timeout"],"statusMessage":settings["statusMessage"]}]})
        && spec.events.iter().all(|(event, _)| {
            adapter
                .direct_hook_group(binary, Path::new("/"), event)
                .is_none()
        });
    if !expressible {
        return Err(Error::Invalid(
            "an agent's hook settings have no Box equivalent; set the agent up directly",
        ));
    }
    let events = spec
        .events
        .iter()
        .map(|(event, _)| portable(event))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(
        json!({"agents":[spec.id],"events":events,"matcher":matcher,"command":command,
        "timeout":settings["timeout"],"status_message":settings["statusMessage"]}),
    )
}

/// The definition texts: the shared file with Shield's server for each agent,
/// every tool's decision (`approved` tools allow, the rest ask) and every
/// adapter's hooks, and the local file pinning `binary`.
pub(in crate::review) fn render(binary: &Path, approved: &[String]) -> Result<Definition, Error> {
    #[cfg(windows)]
    {
        let _ = (binary, approved);
        Err(Error::Invalid(
            "the Box integration needs POSIX hook commands and is not available on Windows; set up claude or codex directly",
        ))
    }
    #[cfg(not(windows))]
    {
        let text = binary
            .to_str()
            .ok_or(Error::Invalid("integration path is not UTF-8"))?;
        // Box quotes the pinned command for hooks; it must be one valid word.
        super::native::quoted(binary)?;
        let hooks = agents::ADAPTERS
            .iter()
            .map(|adapter| hook(*adapter, binary))
            .collect::<Result<Vec<_>, _>>()?;
        let mut shared = json!({"schema_version":1,
            "connection":{"transport":"stdio","command":COMMAND,"args":ARGS},
            "hooks":hooks});
        // Every tool has a decision, as a direct Codex installation records
        // one for each, so Box renders the identical entry it can adopt.
        let tools: Map<String, Value> = crate::review::mcp::tool_summaries()
            .into_iter()
            .map(|(name, _)| {
                let allow = name != ALWAYS_ASK && approved.contains(&name);
                (name, json!(if allow { "allow" } else { "ask" }))
            })
            .collect();
        shared["permissions"] = json!({"tools": tools});
        let local = json!({"schema_version":1,
            "connection":{"transport":"stdio","command":text,"args":ARGS}});
        let encode = |value: &Value| {
            crate::policy_files::render(value)
                .map_err(|_| Error::Invalid("Box MCP definition encoding"))
        };
        Ok(Definition {
            shared: encode(&shared)?,
            local: encode(&local)?,
        })
    }
}

/// Plan both definition files and return the texts Shield then owns.
///
/// `binary` is `None` when removing. Owned files are deleted only while they
/// still hold the recorded content, and edited ones are rewritten only when
/// Box is `selected`; a file Shield never wrote is never replaced.
pub(in crate::review) fn prepare(
    root: &Path,
    previous: Option<&Definition>,
    binary: Option<&Path>,
    selected: bool,
    approved: &[String],
    changes: &mut Vec<Change>,
) -> Result<Option<Definition>, Error> {
    if previous.is_none() && binary.is_none() {
        return Ok(None);
    }
    let paths = [PATH, LOCAL_PATH];
    let current = [files::read(root, PATH)?, files::read(root, LOCAL_PATH)?];
    let owned = previous.map(Definition::texts);
    let Some(binary) = binary else {
        if current
            .iter()
            .enumerate()
            .any(|(i, text)| text.is_some() && text.as_deref() != owned.map(|o| o[i]))
        {
            return Err(Error::Invalid(
                "the Box MCP definition (.box/mcp/shield.yml or .box/local/mcp/shield.yml) was edited; restore it with shield mcp setup --agent box, or delete it yourself, before removal",
            ));
        }
        for (path, text) in paths.into_iter().zip(current) {
            if text.is_some() {
                changes.push(Change {
                    path: path.into(),
                    before: text,
                    after: None,
                });
            }
        }
        return Ok(None);
    };
    let unchanged = (0..2).all(|i| current[i].as_deref() == owned.map(|o| o[i]));
    // Setting up other agents keeps an edited or orphaned definition as recorded.
    if !selected && (!unchanged || !box_project(root)) {
        return Ok(previous.cloned());
    }
    if !box_project(root) {
        return Err(Error::Invalid(
            "this project has no .box/config.yml; run box config init first, then shield mcp setup --agent box",
        ));
    }
    let desired = render(binary, approved)?;
    let texts = desired.texts();
    if previous.is_none()
        && (0..2).any(|i| current[i].as_deref().is_some_and(|text| text != texts[i]))
    {
        return Err(Error::Invalid(
            "a Box MCP definition for shield exists that Shield did not write; move .box/mcp/shield.yml and .box/local/mcp/shield.yml aside before setup",
        ));
    }
    for ((path, before), after) in paths.into_iter().zip(current).zip(texts) {
        changes.push(Change {
            path: path.into(),
            before,
            after: Some(after.to_owned()),
        });
    }
    Ok(Some(desired))
}

/// The owned definition's state: `installed`, `missing` or `changed`.
pub(in crate::review) fn diagnose(
    root: &Path,
    owned: &Definition,
    diagnostics: &mut Vec<&'static str>,
) -> &'static str {
    let read = [files::read(root, PATH), files::read(root, LOCAL_PATH)];
    let texts = owned.texts();
    let state = if read
        .iter()
        .zip(texts)
        .all(|(text, owned)| matches!(text, Ok(Some(text)) if text == owned))
    {
        "installed"
    } else if read.iter().zip(texts).all(|(text, owned)| {
        matches!(text, Ok(None)) || matches!(text, Ok(Some(text)) if text == owned)
    }) {
        diagnostics.push("owned Box MCP definition is missing");
        "missing"
    } else {
        diagnostics.push("owned Box MCP definition was changed");
        "changed"
    };
    if !box_project(root) {
        diagnostics.push("Box project configuration .box/config.yml is missing");
    }
    state
}

#[cfg(all(test, not(windows)))]
mod tests {
    use super::*;

    #[test]
    fn native_events_map_to_box_portable_names() {
        for (native, portable_name) in [
            ("PreToolUse", "pre_tool_use"),
            ("PostToolUse", "post_tool_use"),
            ("PostToolUseFailure", "post_tool_use_failure"),
            ("PermissionRequest", "permission_request"),
            ("UserPromptSubmit", "user_prompt_submit"),
            ("SessionStart", "session_start"),
            ("Stop", "stop"),
        ] {
            assert_eq!(portable(native).unwrap(), portable_name);
        }
        assert!(portable("Notification").is_err());
    }
}
