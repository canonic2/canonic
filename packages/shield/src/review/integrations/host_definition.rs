//! Shield's MCP definition for an agent manager, Hub or Box: the shared
//! `<folder>/mcp/shield.yml` the manager publishes to the agents each
//! developer chose, and the private `<folder>/local/mcp/shield.yml` that pins
//! this Shield binary. The shared file names the bare `shield` command and its
//! hooks name `{command}`, which the manager expands to the pinned binary, so
//! it renders the same hook groups Shield writes directly. Hub and Box share
//! the definition format; only their folders and commands differ.
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

/// One agent manager Shield can write a definition for. Messages are static
/// because review errors and diagnostics carry static text.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Host {
    /// The integration ID `shield mcp setup --agent` accepts.
    pub(crate) id: &'static str,
    pub(crate) label: &'static str,
    /// The shared, portable definition.
    pub(crate) path: &'static str,
    /// The private local layer, which replaces the shared connection with the
    /// pinned binary for this checkout.
    pub(crate) local_path: &'static str,
    /// The project configuration whose presence makes a folder this host's project.
    config: &'static str,
    /// How the host publishes the definition to agents.
    pub(crate) publish: &'static str,
    /// The setup review line for the shared file.
    pub(crate) write_note: &'static str,
    /// The post-setup notice.
    pub(crate) next_step: &'static str,
    edited_removal: &'static str,
    edited_switch: &'static str,
    missing_config: &'static str,
    unowned: &'static str,
    missing: &'static str,
    changed: &'static str,
    config_missing: &'static str,
}

macro_rules! host {
    ($id:literal, $label:literal, $folder:literal, $init:literal, $publish:literal) => {
        Host {
            id: $id,
            label: $label,
            path: concat!($folder, "/mcp/shield.yml"),
            local_path: concat!($folder, "/local/mcp/shield.yml"),
            config: concat!($folder, "/config.yml"),
            publish: $publish,
            write_note: concat!(
                "Write ",
                $folder,
                "/mcp/shield.yml; ",
                $label,
                " publishes it to each developer's agents"
            ),
            next_step: concat!(
                "Shield's ",
                $label,
                " MCP definition is in ",
                $folder,
                "/mcp/shield.yml. Run ",
                $publish,
                " so ",
                $label,
                " publishes it to your agents."
            ),
            edited_removal: concat!(
                "the ",
                $label,
                " MCP definition (",
                $folder,
                "/mcp/shield.yml or ",
                $folder,
                "/local/mcp/shield.yml) was edited; restore it with shield mcp setup --agent ",
                $id,
                ", or delete it yourself, before removal"
            ),
            edited_switch: concat!(
                "the ",
                $label,
                " MCP definition (",
                $folder,
                "/mcp/shield.yml or ",
                $folder,
                "/local/mcp/shield.yml) was edited, so Shield cannot replace it with the other host's; restore it with shield mcp setup --agent ",
                $id,
                ", or delete it yourself, before switching hosts"
            ),
            missing_config: concat!(
                "this project has no ",
                $folder,
                "/config.yml; run ",
                $init,
                " first, then shield mcp setup --agent ",
                $id
            ),
            unowned: concat!(
                "a ",
                $label,
                " MCP definition for shield exists that Shield did not write; move ",
                $folder,
                "/mcp/shield.yml and ",
                $folder,
                "/local/mcp/shield.yml aside before setup"
            ),
            missing: concat!("owned ", $label, " MCP definition is missing"),
            changed: concat!("owned ", $label, " MCP definition was changed"),
            config_missing: concat!(
                $label,
                " project configuration ",
                $folder,
                "/config.yml is missing"
            ),
        }
    };
}

pub(crate) const HUB: Host = host!("hub", "Hub", ".hub", "hub setup", "hub setup");
pub(crate) const BOX: Host = host!(
    "box",
    "Box",
    ".box",
    "box config init",
    "box setup (first time) or box config sync"
);

/// Every host in selection order: Hub, Box's successor, comes first and is the
/// default where both are configured.
pub(crate) const HOSTS: [&Host; 2] = [&HUB, &BOX];

/// The host an integration ID names.
pub(crate) fn find(id: &str) -> Option<&'static Host> {
    HOSTS.into_iter().find(|host| host.id == id)
}

/// Every host's shared and private definition paths.
pub(crate) fn paths() -> impl Iterator<Item = &'static str> {
    HOSTS
        .into_iter()
        .flat_map(|host| [host.path, host.local_path])
}

/// The host whose definition files include `path`.
pub(crate) fn owning(path: &str) -> Option<&'static Host> {
    HOSTS
        .into_iter()
        .find(|host| host.path == path || host.local_path == path)
}

/// The persisted host of a definition; records written before Hub existed
/// name none and are Box's.
#[derive(Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(in crate::review) enum HostId {
    Hub,
    #[default]
    Box,
}

impl HostId {
    #[expect(
        clippy::trivially_copy_pass_by_ref,
        reason = "serde's skip_serializing_if passes a reference"
    )]
    fn is_box(&self) -> bool {
        *self == Self::Box
    }

    pub(in crate::review) fn host(self) -> &'static Host {
        match self {
            Self::Hub => &HUB,
            Self::Box => &BOX,
        }
    }

    fn of(host: &Host) -> Self {
        if host.id == HUB.id {
            Self::Hub
        } else {
            Self::Box
        }
    }
}

/// The exact texts Shield wrote, owned and diagnosed together.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::review) struct Definition {
    /// Omitted for Box, so Box records keep their earlier encoding.
    #[serde(default, skip_serializing_if = "HostId::is_box")]
    pub(in crate::review) host: HostId,
    pub(in crate::review) shared: String,
    pub(in crate::review) local: String,
}

impl Definition {
    fn texts(&self) -> [&str; 2] {
        [&self.shared, &self.local]
    }

    pub(in crate::review) fn host(&self) -> &'static Host {
        self.host.host()
    }

    /// The pin has its own exact generated codec. Direct installations can
    /// retain a different historical binary path in the shared registration.
    pub(in crate::review) fn portable_codec(&self, approved: &[String]) -> bool {
        let Ok(local) = crate::policy::parse::<serde_json::Value>(&self.local) else {
            return false;
        };
        let Some(command) = local["connection"]["command"].as_str() else {
            return false;
        };
        let binary = Path::new(command);
        binary.is_absolute()
            && render(self.host(), binary, approved).is_ok_and(|expected| expected == *self)
    }

    pub(in crate::review) fn pinned_to(&self, binary: &Path) -> bool {
        crate::policy::parse::<serde_json::Value>(&self.local)
            .is_ok_and(|local| local["connection"]["command"].as_str() == binary.to_str())
    }
}

/// Whether `root` is this host's project, without reading its configuration.
pub(in crate::review) fn configured(host: &Host, root: &Path) -> bool {
    std::fs::metadata(root.join(host.config)).is_ok_and(|m| m.is_file())
}

/// The first configured host for `root`, in [`HOSTS`] order.
pub(in crate::review) fn project_host(root: &Path) -> Option<&'static Host> {
    HOSTS.into_iter().find(|host| configured(host, root))
}

/// The portable name for a native event: `PreToolUse` becomes `pre_tool_use`.
#[cfg(not(windows))]
fn portable(event: &str) -> Result<String, Error> {
    if event == "BeforeTool" {
        return Ok("pre_tool_use".into());
    }
    if event == "AfterTool" {
        return Ok("post_tool_use".into());
    }
    let mut name = String::new();
    for (index, c) in event.chars().enumerate() {
        if c.is_ascii_uppercase() && index > 0 {
            name.push('_');
        }
        name.push(c.to_ascii_lowercase());
    }
    if !EVENTS.contains(&name.as_str()) {
        return Err(Error::Invalid(
            "an agent hook event has no Hub or Box equivalent; set the agent up directly",
        ));
    }
    Ok(name)
}

/// The command shared files name, found on `PATH`.
#[cfg(not(windows))]
const COMMAND: &str = "shield";
/// The launch arguments the host expands with the project root and agent ID.
#[cfg(not(windows))]
const ARGS: [&str; 6] = ["mcp", "serve", "--project", ".", "--agent", "{agent}"];
/// The portable hook event names Hub and Box accept.
#[cfg(not(windows))]
const EVENTS: &[&str] = &[
    "pre_tool_use",
    "post_tool_use",
    "post_tool_use_failure",
    "permission_request",
    "user_prompt_submit",
    "session_start",
    "stop",
    "session_end",
    "interrupt",
];
/// Tools that ask for every agent: Codex always prompts before applying policy.
#[cfg(not(windows))]
const ALWAYS_ASK: &str = "policy_apply";

/// One adapter's portable hook settings, preserving its native metadata and
/// timeout units through the host's projection. Direct executable hooks cannot
/// be represented by this POSIX command entry.
#[cfg(not(windows))]
fn hook(adapter: &dyn AgentAdapter, binary: &Path) -> Result<Value, Error> {
    let spec = adapter.spec();
    if spec.events.iter().any(|(event, _)| {
        adapter
            .direct_hook_group(binary, Path::new("/"), event)
            .is_some()
    }) {
        return Err(Error::Invalid(
            "an agent's direct executable hooks have no Hub or Box equivalent; set the agent up directly",
        ));
    }
    let command = super::native::posix_hook_command("{command}", ".", "{agent}", "{event}");
    let mut settings = adapter.box_hook(&command).ok_or(Error::Invalid(
        "an agent's hook settings have no Hub or Box equivalent; set the agent up directly",
    ))?;
    let events = spec
        .events
        .iter()
        .map(|(event, _)| portable(event))
        .collect::<Result<Vec<_>, _>>()?;
    settings["agents"] = json!([spec.id]);
    settings["events"] = json!(events);
    Ok(settings)
}

/// The definition texts: the shared file with Shield's server for each agent,
/// every tool's decision (`approved` tools allow, the rest ask) and every
/// adapter's hooks, and the local file pinning `binary`. Hub and Box receive
/// identical texts.
pub(in crate::review) fn render(
    host: &Host,
    binary: &Path,
    approved: &[String],
) -> Result<Definition, Error> {
    #[cfg(windows)]
    {
        let _ = (host, binary, approved);
        Err(Error::Invalid(
            "the Hub and Box integrations need POSIX hook commands and are not available on Windows; set up claude or codex directly",
        ))
    }
    #[cfg(not(windows))]
    {
        let text = binary
            .to_str()
            .ok_or(Error::Invalid("integration path is not UTF-8"))?;
        // The host quotes the pinned command for hooks; it must be one valid word.
        super::native::quoted(binary)?;
        let hooks = agents::ADAPTERS
            .iter()
            .filter(|adapter| adapter.supports_box_hooks())
            .map(|adapter| hook(*adapter, binary))
            .collect::<Result<Vec<_>, _>>()?;
        let mut shared = json!({"schema_version":1,
            "connection":{"transport":"stdio","command":COMMAND,"args":ARGS},
            "hooks":hooks});
        // Every tool has a decision, as a direct Codex installation records
        // one for each, so the host renders the identical entry it can adopt.
        let tools: Map<String, Value> = crate::review::mcp::tool_summaries()
            .into_iter()
            .map(|(name, _)| {
                let allow = name != ALWAYS_ASK && approved.contains(&name);
                (name, json!(if allow { "allow" } else { "ask" }))
            })
            .collect();
        let permission_agents: Vec<_> = agents::ADAPTERS
            .iter()
            .filter(|adapter| adapter.supports_box_permissions())
            .map(|adapter| adapter.spec().id)
            .collect();
        shared["permissions"] = json!({"agents":permission_agents,"tools": tools});
        let local = json!({"schema_version":1,
            "connection":{"transport":"stdio","command":text,"args":ARGS}});
        let encode = |value: &Value| {
            crate::policy_files::render(value)
                .map_err(|_| Error::Invalid("MCP definition encoding"))
        };
        Ok(Definition {
            host: HostId::of(host),
            shared: encode(&shared)?,
            local: encode(&local)?,
        })
    }
}

/// The committed shared definition when it is exactly what `binary` renders,
/// with the approved tools it names. The private pin may be absent, as in a
/// fresh checkout, but an existing pin must be the one `binary` renders.
pub(in crate::review) fn committed(
    host: &Host,
    root: &Path,
    binary: &Path,
) -> Result<Option<(Definition, Vec<String>)>, Error> {
    let Some(shared) = files::read(root, host.path)? else {
        return Ok(None);
    };
    let Ok(parsed) = crate::policy::parse::<serde_json::Value>(&shared) else {
        return Ok(None);
    };
    let approved: Vec<String> = parsed["permissions"]["tools"]
        .as_object()
        .into_iter()
        .flatten()
        .filter(|(_, decision)| decision.as_str() == Some("allow"))
        .map(|(name, _)| name.clone())
        .collect();
    let Ok(rendered) = render(host, binary, &approved) else {
        return Ok(None);
    };
    if rendered.shared != shared
        || files::read(root, host.local_path)?.is_some_and(|local| local != rendered.local)
    {
        return Ok(None);
    }
    Ok(Some((rendered, approved)))
}

/// Plan both definition files for `host` and return the texts Shield then owns.
///
/// `binary` is `None` when removing. Owned files are deleted only while they
/// still hold the recorded content, and edited ones are rewritten only when
/// the host is `selected`; a file Shield never wrote is never replaced. A
/// `previous` definition for the other host is removed when `host` is written.
pub(in crate::review) fn prepare(
    host: &'static Host,
    root: &Path,
    previous: Option<&Definition>,
    binary: Option<&Path>,
    selected: bool,
    approved: &[String],
    changes: &mut Vec<Change>,
) -> Result<Option<Definition>, Error> {
    if let Some(old) = previous.filter(|old| old.host() != host) {
        // Switching hosts removes the other host's owned files first, and an
        // edited one stops the switch rather than a removal the operator never asked for.
        prepare(old.host(), root, Some(old), None, false, approved, changes).map_err(|error| {
            match error {
                Error::Invalid(text) if text == old.host().edited_removal => {
                    Error::Invalid(old.host().edited_switch)
                }
                error => error,
            }
        })?;
        return prepare(host, root, None, binary, selected, approved, changes);
    }
    if previous.is_none() && binary.is_none() {
        return Ok(None);
    }
    let paths = [host.path, host.local_path];
    let current = [
        files::read(root, host.path)?,
        files::read(root, host.local_path)?,
    ];
    let owned = previous.map(Definition::texts);
    let Some(binary) = binary else {
        if current
            .iter()
            .enumerate()
            .any(|(i, text)| text.is_some() && text.as_deref() != owned.map(|o| o[i]))
        {
            return Err(Error::Invalid(host.edited_removal));
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
    if !crate::layout::launch_args(root)?.is_empty() {
        return Err(Error::Invalid(
            "the Hub or Box definition selects the default configuration; move it to .config/shield/ instead of --config, or set the agents up directly",
        ));
    }
    let unchanged = (0..2).all(|i| current[i].as_deref() == owned.map(|o| o[i]));
    // An unselected refresh, the portable re-pin, keeps an edited or orphaned
    // definition as recorded.
    if !selected && (!unchanged || !configured(host, root)) {
        return Ok(previous.cloned());
    }
    if !configured(host, root) {
        return Err(Error::Invalid(host.missing_config));
    }
    let desired = render(host, binary, approved)?;
    let texts = desired.texts();
    if previous.is_none()
        && (0..2).any(|i| current[i].as_deref().is_some_and(|text| text != texts[i]))
    {
        return Err(Error::Invalid(host.unowned));
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
    let host = owned.host();
    let read = [
        files::read(root, host.path),
        files::read(root, host.local_path),
    ];
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
        diagnostics.push(host.missing);
        "missing"
    } else {
        diagnostics.push(host.changed);
        "changed"
    };
    if !configured(host, root) {
        diagnostics.push(host.config_missing);
    }
    state
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(not(windows))]
    #[test]
    fn native_events_map_to_portable_names() {
        for (native, portable_name) in [
            ("PreToolUse", "pre_tool_use"),
            ("PostToolUse", "post_tool_use"),
            ("PostToolUseFailure", "post_tool_use_failure"),
            ("PermissionRequest", "permission_request"),
            ("UserPromptSubmit", "user_prompt_submit"),
            ("SessionStart", "session_start"),
            ("Stop", "stop"),
            ("SessionEnd", "session_end"),
            ("Interrupt", "interrupt"),
        ] {
            assert_eq!(portable(native).unwrap(), portable_name);
        }
        assert!(portable("Notification").is_err());
    }

    #[test]
    fn hosts_have_their_own_folders_and_box_records_keep_their_encoding() {
        assert_eq!(HUB.path, ".hub/mcp/shield.yml");
        assert_eq!(HUB.local_path, ".hub/local/mcp/shield.yml");
        assert_eq!(BOX.path, ".box/mcp/shield.yml");
        assert_eq!(find("hub"), Some(&HUB));
        assert_eq!(owning(".box/local/mcp/shield.yml"), Some(&BOX));
        let root = assert_fs::TempDir::new().unwrap();
        assert!(project_host(root.path()).is_none());
        for folder in [".box", ".hub"] {
            std::fs::create_dir_all(root.path().join(folder)).unwrap();
            std::fs::write(root.path().join(folder).join("config.yml"), "").unwrap();
        }
        // Hub wins where both are configured.
        assert_eq!(project_host(root.path()), Some(&HUB));
        // An earlier Box record has no host and reads as Box; Box records
        // serialize without it, Hub records with it.
        let earlier: Definition = serde_json::from_str(r#"{"shared":"a","local":"b"}"#).unwrap();
        assert_eq!(earlier.host(), &BOX);
        assert_eq!(
            serde_json::to_string(&earlier).unwrap(),
            r#"{"shared":"a","local":"b"}"#
        );
        let hub = Definition {
            host: HostId::Hub,
            ..earlier
        };
        assert!(
            serde_json::to_string(&hub)
                .unwrap()
                .contains(r#""host":"hub""#)
        );
    }
}
