//! Vendor-specific paths and schemas; availability is explicit per scope.
use super::{Context, Error, Result, Scope, Tool};
use serde_json::{Value, json};
use std::path::PathBuf;

/// Every setup choice with its CLI discovery names: the configuration hosts
/// first, then each agent surface; configuration hosts, editor surfaces and
/// ambiguous names have none.
pub(crate) const CATALOG: &[(&str, &[&str])] = &[
    (HUB.id, &[]),
    (BOX.id, &[]),
    ("claude", &["claude"]),
    ("codex", &["codex"]),
    ("copilot-vscode", &[]),
    ("copilot-cli", &[]), // Also names an unrelated infrastructure CLI.
    ("cursor", &[]),
    ("gemini", &["gemini"]),
    ("kilo", &[]),
    ("kiro", &[]),
    ("kimi", &["kimi"]),
    ("opencode", &["opencode"]),
    ("qwen", &["qwen"]),
    ("amp", &["amp"]),
    ("droid", &["droid"]),
    ("windsurf", &[]),
    ("cline", &[]),
    ("cline-cli", &["cline"]),
    ("augment", &[]),
    ("augment-cli", &["auggie"]),
    ("goose", &[]), // Also names an unrelated database migration CLI.
];

/// Ordered host identifiers and their unambiguous CLI discovery names.
pub fn catalog() -> impl Iterator<Item = (&'static str, &'static [&'static str])> {
    CATALOG.iter().copied()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    Json,
    Jsonc,
    Toml,
    Yaml,
}

pub(crate) struct Destination {
    pub path: PathBuf,
    pub format: Format,
    pub key: &'static str,
    pub entry: Value,
    pub surfaces: Vec<String>,
}

pub(crate) fn destination(context: &Context, id: &str, scope: Scope) -> Result<Destination> {
    if context.root.to_str().is_none() {
        return Err(Error::invalid(
            "Project path is not Unicode; agent configuration cannot represent it",
        ));
    }
    let project = scope == Scope::Project;
    // From home (or above it), several agents' project files are their user
    // files; project scope must never write user configuration.
    if project && context.home.starts_with(&context.root) {
        return Err(Error::selection(format!(
            "{} is your home folder or contains it, not a project; run setup from inside a project, or pass --scope user to configure this OS user's agents",
            context.root.display()
        )));
    }
    let base = if project {
        &context.root
    } else {
        &context.home
    };
    let (relative, key, mut format) = match id {
        "claude" | "copilot-cli" if project => (".mcp.json", "mcpServers", Format::Json),
        "claude" => (".claude.json", "mcpServers", Format::Json),
        "codex" => (".codex/config.toml", "mcp_servers", Format::Toml),
        "copilot-vscode" if project => (".vscode/mcp.json", "servers", Format::Jsonc),
        "copilot-vscode" => {
            return Err(Error::selection(
                "Select the VS Code profile's MCP configuration manually; automatic user scope is unavailable",
            ));
        }
        "copilot-cli" => (".copilot/mcp-config.json", "mcpServers", Format::Json),
        "cursor" => (".cursor/mcp.json", "mcpServers", Format::Json),
        "gemini" => (".gemini/settings.json", "mcpServers", Format::Json),
        "qwen" => (".qwen/settings.json", "mcpServers", Format::Json),
        "kimi" => (".kimi-code/mcp.json", "mcpServers", Format::Json),
        "kiro" => (".kiro/settings/mcp.json", "mcpServers", Format::Json),
        "kilo" if project => (".kilo/kilo.jsonc", "mcp", Format::Json),
        "kilo" => (".config/kilo/kilo.jsonc", "mcp", Format::Json),
        "opencode" if project => ("opencode.json", "mcp", Format::Json),
        "opencode" => (".config/opencode/opencode.json", "mcp", Format::Json),
        "amp" => (".amp/settings.json", "amp.mcpServers", Format::Json),
        "droid" => (".factory/mcp.json", "mcpServers", Format::Json),
        "windsurf" if !project => (".config/devin/mcp_config.json", "mcpServers", Format::Json),
        "windsurf" => {
            return Err(Error::selection(
                "Windsurf/Devin project MCP registration is unverified; user scope must be selected explicitly",
            ));
        }
        "cline-cli" if !project => (
            ".cline/data/settings/cline_mcp_settings.json",
            "mcpServers",
            Format::Json,
        ),
        "augment-cli" if !project => (".augment/settings.json", "mcpServers", Format::Json),
        "goose" if !project => (".config/goose/config.yaml", "extensions", Format::Yaml),
        "cline" | "cline-cli" | "augment" | "augment-cli" | "goose" => {
            return Err(Error::selection(format!(
                "{id}: automatic registration for this scope is unverified; configure this host manually"
            )));
        }
        _ => {
            return Err(Error::selection(format!(
                "Unknown agent {id}; choose one of {}",
                CATALOG
                    .iter()
                    .map(|(id, _)| *id)
                    .collect::<Vec<_>>()
                    .join(", ")
            )));
        }
    };
    let mut path = base.join(relative);
    if !project {
        path = user_path(context, id, path)?;
    }
    let path = existing_alternate(context, id, project, path)?;
    if path.to_str().is_none() {
        return Err(Error::invalid("Agent configuration path is not Unicode"));
    }
    // Project files are often committed, so they name the command for PATH
    // lookup instead of this user's install path; user files keep the path.
    let program = if let Some(command) = &context.launch_command {
        command.as_str()
    } else if project {
        context.product.as_str()
    } else {
        context
            .program
            .to_str()
            .ok_or_else(|| Error::invalid("Executable path is not Unicode"))?
    };
    if path.extension().is_some_and(|ext| ext == "jsonc") {
        format = Format::Jsonc;
    }
    Ok(Destination {
        path,
        key,
        format,
        entry: entry(context, id, program),
        surfaces: if project && matches!(id, "claude" | "copilot-cli") {
            vec!["claude".into(), "copilot-cli".into()]
        } else {
            vec![id.into()]
        },
    })
}

/// A user-scope path after the agent's native home and XDG overrides, so setup
/// never writes a default file the agent ignores.
fn user_path(context: &Context, id: &str, mut path: PathBuf) -> Result<PathBuf> {
    if let Some(directory) = context.agent_homes.get(id) {
        if !directory.is_absolute() {
            return Err(Error::invalid("Agent home override must be absolute"));
        }
        path = if id == "cline-cli" {
            directory.clone()
        } else {
            directory.join(
                path.file_name()
                    .ok_or_else(|| Error::invalid("Agent configuration path has no file name"))?,
            )
        };
    }
    if id == "amp" {
        path = context.home.join(".config/amp/settings.json");
    }
    if matches!(id, "opencode" | "kilo" | "goose")
        && let Some(config) = context.agent_homes.get("xdg").filter(|p| p.is_absolute())
    {
        path = config.join(match id {
            "opencode" => "opencode/opencode.json",
            "kilo" => "kilo/kilo.jsonc",
            _ => "goose/config.yaml",
        });
    }
    #[cfg(windows)]
    if id == "goose" {
        path = context
            .agent_homes
            .get("roaming")
            .cloned()
            .unwrap_or_else(|| context.home.join("AppData/Roaming"))
            .join("Block/goose/config/config.yaml");
    }
    Ok(path)
}

/// Prefers an existing supported alternative file, and refuses ambiguous ones.
fn existing_alternate(
    context: &Context,
    id: &str,
    project: bool,
    mut path: PathBuf,
) -> Result<PathBuf> {
    if matches!(id, "opencode" | "amp") {
        let alternate = path.with_extension("jsonc");
        if path.exists() && alternate.exists() {
            return Err(Error::invalid(format!(
                "Both {} and {} exist; choose one host configuration first",
                path.display(),
                alternate.display()
            )));
        }
        if alternate.exists() {
            path = alternate;
        }
    }
    if id == "kilo" {
        let candidates = if project {
            vec![
                context.root.join(".kilo/kilo.jsonc"),
                context.root.join(".kilo/kilo.json"),
                context.root.join("kilo.jsonc"),
                context.root.join("kilo.json"),
            ]
        } else {
            vec![path.clone(), path.with_extension("json")]
        };
        let existing: Vec<_> = candidates.into_iter().filter(|p| p.exists()).collect();
        if existing.len() > 1 {
            return Err(Error::invalid(
                "Multiple Kilo project configurations exist; resolve their precedence before setup",
            ));
        }
        if let Some(existing) = existing.into_iter().next() {
            path = existing;
        }
    }
    Ok(path)
}

/// The server entry in the agent's own schema.
fn entry(context: &Context, id: &str, program: &str) -> Value {
    let mut entry = json!({"command":program,"args":context.arguments});
    if id == "goose" {
        entry = json!({"type":"stdio","name":context.product,"enabled":true,"cmd":program,"args":context.arguments,"envs":{},"env_keys":[],"timeout":300});
    } else if matches!(id, "opencode" | "kilo") {
        entry = json!({"type":"local","command":std::iter::once(program.to_string()).chain(context.arguments.clone()).collect::<Vec<_>>(),"enabled":true});
    } else if matches!(id, "claude" | "copilot-vscode" | "copilot-cli" | "droid") {
        entry["type"] = json!("stdio");
    }
    entry
}

/// A choice that writes a server definition into an agent manager's folder
/// instead of an agent's own file; the manager then publishes it to every
/// agent each developer chose. Hub and Box share the definition format.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct ConfigHost {
    pub(crate) id: &'static str,
    pub(crate) name: &'static str,
    /// The folder in the project or home folder: `.hub` or `.box`.
    folder: &'static str,
    /// The project folder the host reads instead when `folder` has no
    /// `config.yml`; user scope always uses `folder`.
    grouped: Option<&'static str>,
    /// The command that creates the configuration marker.
    init: &'static str,
    /// The command that republishes a changed definition, when it is not
    /// `<id> setup` again.
    sync: Option<&'static str>,
}

impl ConfigHost {
    /// How the manager publishes a written definition in `scope`; the
    /// commands carry `--scope user` because they default to project scope.
    pub(crate) fn publish(&self, scope: Scope) -> String {
        let (agents, flag) = match scope {
            Scope::Project => ("each developer's agents", ""),
            Scope::User => ("your agents", " --scope user"),
        };
        let after = match self.sync {
            Some(sync) => format!(", or `{sync}{flag}` after a change"),
            None => " now and after each change".into(),
        };
        format!(
            "{} publishes it to {agents}: run `{} setup{flag}`{after}",
            self.name, self.id
        )
    }
}

pub(crate) const HUB: ConfigHost = ConfigHost {
    id: "hub",
    name: "Hub",
    folder: ".hub",
    grouped: Some(".config/hub"),
    init: "hub setup",
    sync: None,
};

pub(crate) const BOX: ConfigHost = ConfigHost {
    id: "box",
    name: "Box",
    folder: ".box",
    grouped: None,
    init: "box config init",
    sync: Some("box config sync"),
};

/// The configuration hosts in catalog and default-selection order.
pub(crate) const CONFIG_HOSTS: &[&ConfigHost] = &[&HUB, &BOX];

/// The configuration host an agent ID names, if any.
pub(crate) fn config_host(id: &str) -> Option<&'static ConfigHost> {
    CONFIG_HOSTS.iter().copied().find(|host| host.id == id)
}

/// `<folder>/mcp/<product>.yml` in the project, or in the home folder for user
/// scope, refusing a folder the host does not configure. A project whose
/// `folder` has no marker uses the host's grouped folder when that has one.
pub(crate) fn definition_path(
    host: &ConfigHost,
    context: &Context,
    scope: Scope,
    remove: bool,
) -> Result<PathBuf> {
    let project = scope == Scope::Project;
    if project && context.home.starts_with(&context.root) {
        return Err(Error::selection(format!(
            "{} is your home folder or contains it, not a project; pass --scope user for your personal {} configuration",
            context.root.display(),
            host.name
        )));
    }
    let base = if project {
        &context.root
    } else {
        &context.home
    };
    let grouped = host.grouped.filter(|_| project);
    let mut folder = base.join(host.folder);
    if let Some(grouped) = grouped
        && !folder.join("config.yml").is_file()
        && base.join(grouped).join("config.yml").is_file()
    {
        folder = base.join(grouped);
    }
    if !remove && !folder.join("config.yml").is_file() {
        return Err(Error::selection(format!(
            "{} has no {} configuration ({}/config.yml{}); run `{}{}` first",
            base.display(),
            host.name,
            host.folder,
            grouped.map_or(String::new(), |grouped| format!(" or {grouped}/config.yml")),
            host.init,
            if project { "" } else { " --scope user" }
        )));
    }
    Ok(folder.join("mcp").join(format!("{}.yml", context.product)))
}

pub(crate) fn project_directory(entry: &mut Value, id: &str, context: &Context) {
    if matches!(
        id,
        "codex" | "copilot-vscode" | "gemini" | "qwen" | "kimi" | "opencode" | "kilo"
    ) {
        entry["cwd"] = json!(context.root);
    }
}

pub(crate) fn launch_command(context: &Context, scope: Scope) -> Result<&str> {
    if let Some(command) = &context.launch_command {
        return Ok(command);
    }
    if scope == Scope::Project {
        return Ok(&context.product);
    }
    context
        .program
        .to_str()
        .ok_or_else(|| Error::invalid("Executable path is not Unicode"))
}

/// The server definition the host publishes: the same launch as the agents'
/// own entries, with the approved tools allowed and every other tool asking.
pub(crate) fn definition(
    host: &ConfigHost,
    context: &Context,
    scope: Scope,
    approved: &[String],
) -> Result<String> {
    let command = launch_command(context, scope)?;
    // JSON strings are YAML double-quoted scalars, so values need no YAML codec.
    let quote = |text: &str| serde_json::to_string(text).expect("serializing a string cannot fail");
    let mut text = format!(
        "# Written by `{product} mcp setup --agent {id}`; rerun it to change this file.\nschema_version: 1\nconnection:\n  transport: stdio\n  command: {}\n  args: [{}]\n",
        quote(command),
        context
            .arguments
            .iter()
            .map(|arg| quote(arg))
            .collect::<Vec<_>>()
            .join(", "),
        product = context.product,
        id = host.id,
    );
    if !approved.is_empty() {
        text.push_str("permissions:\n  tools:\n");
        for tool in approved {
            text.push_str("    ");
            text.push_str(&quote(tool));
            text.push_str(": allow\n");
        }
    }
    if let Some(group) = &context.prompt_hook {
        let hooks = prompt_handlers(group)?;
        let mut portable = Vec::new();
        for hook in hooks {
            let mut entry = json!({"agents":["claude","codex"],"events":["user_prompt_submit"],"command":hook["command"]});
            for (native, key) in [("timeout", "timeout"), ("statusMessage", "status_message")] {
                if let Some(value) = hook.get(native) {
                    entry[key] = value.clone();
                }
            }
            if let Some(matcher) = group.get("matcher") {
                entry["matcher"] = matcher.clone();
            }
            portable.push(entry);
        }
        text.push_str("hooks: ");
        text.push_str(
            &serde_json::to_string(&portable).map_err(|e| Error::invalid(e.to_string()))?,
        );
        text.push('\n');
    }
    Ok(text)
}

const HOST_HOOK_TOKENS: &[&str] = &["{project}", "{agent}", "{event}", "{command}"];

/// The native command-hook subset that both direct hosts and configuration
/// hosts can publish.
pub(crate) fn prompt_handlers(group: &Value) -> Result<&Vec<Value>> {
    let valid_group = group.as_object().is_some_and(|object| {
        object
            .keys()
            .all(|key| matches!(key.as_str(), "hooks" | "matcher"))
            && object.get("matcher").is_none_or(Value::is_string)
    });
    let handlers = group["hooks"].as_array().filter(|hooks| !hooks.is_empty());
    let valid_handlers = handlers.is_some_and(|hooks| {
        hooks.iter().all(|hook| {
            hook.as_object().is_some_and(|object| {
                object.keys().all(|key| {
                    matches!(
                        key.as_str(),
                        "type" | "command" | "timeout" | "statusMessage"
                    )
                })
            }) && hook["type"] == "command"
                && hook["command"]
                    .as_str()
                    .is_some_and(|command| !command.trim().is_empty())
                && hook.get("timeout").is_none_or(|timeout| {
                    timeout
                        .as_u64()
                        .is_some_and(|n| n > 0 && u32::try_from(n).is_ok())
                })
                && hook.get("statusMessage").is_none_or(Value::is_string)
        })
    });
    if !valid_group || !valid_handlers {
        return Err(Error::invalid(
            "Prompt hook requires command handlers with optional timeout, statusMessage and group matcher",
        ));
    }
    // Hub and Box expand these tokens in hook commands and native hosts do
    // not, so one group would run different commands depending on the route.
    if handlers.into_iter().flatten().any(|hook| {
        hook["command"]
            .as_str()
            .is_some_and(|command| HOST_HOOK_TOKENS.iter().any(|token| command.contains(token)))
    }) {
        return Err(Error::invalid(
            "Prompt hook commands cannot contain {project}, {agent}, {event} or {command}; Hub and Box expand them and native hosts do not",
        ));
    }
    handlers.ok_or_else(|| Error::invalid("Prompt hook must contain command hooks"))
}

/// Agents whose tool approvals setup writes: Codex in its server entry, Claude
/// Code as `permissions.allow` rules in a separate settings file, and Hub and
/// Box in the definition each publishes.
pub(crate) const APPROVALS: &[&str] = &[HUB.id, BOX.id, "claude", "codex"];

/// Codex asks before every tool except those approved here.
pub(crate) fn codex_approvals(entry: &mut Value, tools: &[Tool], approved: &[String]) {
    entry["default_tools_approval_mode"] = json!("prompt");
    entry["tools"] = tools
        .iter()
        .map(|tool| {
            let mode = if approved.contains(&tool.name) {
                "approve"
            } else {
                "prompt"
            };
            (tool.name.clone(), json!({"approval_mode":mode}))
        })
        .collect::<serde_json::Map<_, _>>()
        .into();
}

/// Claude Code's settings file for permission rules. Project rules go in the
/// personal, uncommitted local settings, like the absolute launch path.
pub(crate) fn claude_settings(context: &Context, scope: Scope) -> PathBuf {
    match scope {
        Scope::Project => context.root.join(".claude/settings.local.json"),
        Scope::User => context.home.join(".claude/settings.json"),
    }
}

/// Claude Code's permission rule for one tool of this product's server.
pub(crate) fn claude_rule(product: &str, tool: &str) -> String {
    format!("mcp__{product}__{tool}")
}
