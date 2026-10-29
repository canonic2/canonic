//! Declarative project MCP connections and explicit pack associations.
//! No declaration starts a process, writes host settings or grants permission.
use crate::policy::{self, Error};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
pub(crate) mod launcher;

pub(crate) type Connections = BTreeMap<String, Connection>;

fn enabled() -> bool {
    true
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Connection {
    #[serde(default = "enabled")]
    pub(crate) enabled: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub(crate) local: bool,
    pub(crate) transport: Transport,
    /// Agent ID to server alias. This binds a connection independently of packs.
    #[serde(default)]
    pub(crate) agents: BTreeMap<String, String>,
    /// Exact selected pack identity to its declared integration name.
    #[serde(default)]
    pub(crate) packs: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) command_fields: BTreeMap<String, CommandField>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) effective_fields: BTreeMap<String, EffectiveField>,
}

impl Connection {
    pub(crate) fn facts(&self, arguments: &Value) -> Value {
        let transport = serde_json::to_value(&self.transport).expect("transport serialization");
        let mut facts = serde_json::json!({"transport":transport,"source":if self.local {"local"}else{"project"},"effective":{},"evidence_sources":{}});
        for (name, mapping) in &self.effective_fields {
            let (value, source, pointer) = match arguments.pointer(&mapping.argument) {
                Some(value) => (Some(value), "request", &mapping.argument),
                None => (
                    transport.pointer(&mapping.default),
                    "reviewed-connection",
                    &mapping.default,
                ),
            };
            if let Some(value) = value {
                facts["effective"][name] = value.clone();
            }
            facts["evidence_sources"][name] =
                serde_json::json!({"source":source,"pointer":pointer,"available":value.is_some()});
        }
        facts
    }
    pub(crate) fn projected(
        &self,
        binary: &std::path::Path,
        root: &std::path::Path,
        token: Option<&str>,
    ) -> Result<Transport, Error> {
        if !self.local {
            return Ok(self.transport.clone());
        }
        if !matches!(self.transport, Transport::Stdio { .. }) {
            return Err(Error::Contract(
                "private MCP projection currently requires stdio; no host files were changed",
            ));
        }
        let token = token
            .filter(|token| launcher::valid_token(token))
            .ok_or(Error::Contract(
                "private MCP launcher requires a reviewed random token",
            ))?;
        let command = binary
            .to_str()
            .ok_or(Error::Contract("MCP launcher path must be UTF-8"))?
            .to_owned();
        let root = root
            .to_str()
            .ok_or(Error::Contract("MCP project path must be UTF-8"))?;
        Ok(Transport::Stdio {
            command,
            args: vec![
                "__internal".into(),
                "mcp-connect".into(),
                "--project".into(),
                root.into(),
                "--token".into(),
                token.into(),
            ],
            env: BTreeMap::new(),
        })
    }
    pub(crate) fn launch_digest(&self) -> String {
        policy::fingerprint(
            serde_json::to_string(&self.transport)
                .expect("transport serialization")
                .as_bytes(),
        )
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]
pub(crate) enum Transport {
    Stdio {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        env: BTreeMap<String, String>,
    },
    Http {
        url: String,
    },
    /// Resolve one selected pack's declarative defaults during capture.
    Defaults {
        pack: String,
        integration: String,
    },
}

/// Pack-side connection matching and optional setup defaults. Match predicates
/// use the ordinary typed JSON field map over the reviewed transport object.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Integration {
    #[serde(skip)]
    pub(crate) default_variables: std::collections::BTreeSet<String>,
    pub(crate) transport: String,
    pub(crate) fields: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) defaults: Option<Value>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) command_fields: BTreeMap<String, CommandField>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) effective_fields: BTreeMap<String, EffectiveField>,
}

/// Reviewed mapping semantics: an explicit argument overrides a connection
/// setting. Packs must bind this declaration to a verified tool/connector shape.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct EffectiveField {
    pub(crate) argument: String,
    pub(crate) default: String,
}

fn validate_effective(fields: &BTreeMap<String, EffectiveField>) -> Result<(), Error> {
    if fields.len() > 64 {
        return Err(Error::Limit);
    }
    for (name, mapping) in fields {
        if !valid_name(name)
            || !mapping.argument.starts_with('/')
            || !mapping.default.starts_with('/')
        {
            return Err(Error::Contract("invalid effective MCP field mapping"));
        }
        policy::validate_mcp_fields(
            &serde_json::json!({&mapping.argument:{"exact":null},&mapping.default:{"exact":null}}),
        )?;
    }
    Ok(())
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct CommandField {
    pub(crate) executable: String,
    pub(crate) pointer: String,
    /// `argv-v1` or `posix-literal-v1`; the pack asserts this tool's semantics.
    pub(crate) encoding: String,
}

impl CommandField {
    fn validate(&self) -> Result<(), Error> {
        if !valid_name(&self.executable)
            || !self.pointer.starts_with('/')
            || self.pointer.len() > 1024
            || !["argv-v1", "posix-literal-v1"].contains(&self.encoding.as_str())
        {
            return Err(Error::Contract("invalid declared MCP command field"));
        }
        policy::validate_mcp_fields(&serde_json::json!({&self.pointer:{"exact":null}}))
    }
    pub(crate) fn arguments(&self, input: &Value) -> Result<Vec<String>, Error> {
        let value = input
            .pointer(&self.pointer)
            .ok_or(Error::Contract("mapped MCP command field is missing"))?;
        let mut argv = if self.encoding == "argv-v1" {
            value
                .as_array()
                .ok_or(Error::Syntax)?
                .iter()
                .map(|v| v.as_str().map(str::to_owned).ok_or(Error::Syntax))
                .collect::<Result<Vec<_>, _>>()?
        } else {
            literal_words(value.as_str().ok_or(Error::Syntax)?)?
        };
        if argv.first() != Some(&self.executable)
            || argv.len() > 257
            || argv.iter().any(|s| s.len() > 4096 || s.contains('\0'))
        {
            return Err(Error::Contract(
                "mapped MCP command must name its fixed executable",
            ));
        }
        argv.remove(0);
        Ok(argv)
    }
}

fn literal_words(source: &str) -> Result<Vec<String>, Error> {
    if source.len() > 65536 {
        return Err(Error::Limit);
    }
    let mut words = Vec::new();
    let mut word = String::new();
    let mut started = false;
    let mut quote = None;
    let mut chars = source.chars();
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some('\''), '\'') | (Some('"'), '"') => quote = None,
            (Some('\''), _) => {
                word.push(c);
                started = true;
            }
            (_, '\\') => {
                let escaped = chars.next().ok_or(Error::Syntax)?;
                if escaped.is_control()
                    || (quote == Some('"') && !['"', '\\', '$', '`'].contains(&escaped))
                {
                    return Err(Error::Syntax);
                }
                word.push(escaped);
                started = true;
            }
            (_, '$' | '`') => {
                return Err(Error::Contract(
                    "MCP literal commands cannot expand expressions",
                ));
            }
            (None, '\'' | '"') => {
                quote = Some(c);
                started = true;
            }
            (
                None,
                ';' | '&' | '|' | '<' | '>' | '(' | ')' | '{' | '}' | '*' | '?' | '[' | ']' | '~',
            ) => return Err(Error::Contract("unsupported syntax in MCP literal command")),
            (_, c) if c.is_control() && !matches!(c, '\t') => return Err(Error::Syntax),
            (None, c) if c.is_whitespace() => {
                if started {
                    words.push(std::mem::take(&mut word));
                    started = false;
                }
            }
            _ => {
                word.push(c);
                started = true;
            }
        }
    }
    if quote.is_some() {
        return Err(Error::Syntax);
    }
    if started {
        words.push(word);
    }
    Ok(words)
}

fn validate_commands(commands: &BTreeMap<String, CommandField>) -> Result<(), Error> {
    if commands.len() > 64 {
        return Err(Error::Limit);
    }
    for (tool, mapping) in commands {
        if !valid_name(tool) {
            return Err(Error::Contract("invalid mapped MCP tool name"));
        }
        mapping.validate()?;
    }
    Ok(())
}

pub(crate) fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
        && name != "shield"
}

impl Transport {
    pub(crate) fn validate(&self) -> Result<(), Error> {
        let value = serde_json::to_value(self).map_err(|_| Error::Syntax)?;
        if crate::json::contains_nul(&value)
            || crate::evidence::redact(&mut value.clone())
            || value.to_string().contains("${")
        {
            return Err(Error::Contract(
                "MCP settings cannot contain credentials, NULs or host environment placeholders",
            ));
        }
        match self {
            Self::Defaults { pack, integration } => {
                if pack.is_empty() || pack.len() > 256 || !valid_name(integration) {
                    return Err(Error::Contract("invalid MCP defaults reference"));
                }
            }
            Self::Stdio { command, args, env } => {
                if command.is_empty()
                    || command.len() > 4096
                    || args.len() > 256
                    || env.len() > 256
                    || command.chars().any(char::is_control)
                    || args.iter().any(|a| a.len() > 4096)
                    || env.iter().any(|(k, v)| !valid_name(k) || v.len() > 4096)
                {
                    return Err(Error::Contract("invalid MCP stdio settings"));
                }
            }
            Self::Http { url } => {
                let parsed =
                    url::Url::parse(url).map_err(|_| Error::Contract("invalid MCP endpoint"))?;
                if !matches!(parsed.scheme(), "http" | "https")
                    || parsed.host_str().is_none()
                    || !parsed.username().is_empty()
                    || parsed.password().is_some()
                    || parsed.fragment().is_some()
                    || url.len() > 4096
                {
                    return Err(Error::Contract(
                        "MCP endpoint requires HTTP(S) without credentials or fragment",
                    ));
                }
            }
        }
        Ok(())
    }
}

pub(crate) fn validate(connections: &Connections) -> Result<(), Error> {
    if connections.len() > 64 {
        return Err(Error::Limit);
    }
    let mut aliases = std::collections::BTreeSet::new();
    for (id, connection) in connections {
        if !valid_name(id) {
            return Err(Error::Contract("invalid project MCP connection identity"));
        }
        connection.transport.validate()?;
        validate_commands(&connection.command_fields)?;
        validate_effective(&connection.effective_fields)?;
        if connection.agents.len() > 2 || connection.packs.len() > 64 {
            return Err(Error::Limit);
        }
        for (agent, alias) in &connection.agents {
            if !["claude", "codex"].contains(&agent.as_str())
                || !valid_name(alias)
                || alias.len() > 64
                || !alias.as_bytes()[0].is_ascii_lowercase()
                || alias.ends_with('_')
                || !alias
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
                || alias.contains("__")
                || !aliases.insert((agent, alias))
            {
                return Err(Error::Contract(
                    "MCP host bindings require unique supported agent aliases",
                ));
            }
        }
        for integration in connection.packs.values() {
            if !valid_name(integration) {
                return Err(Error::Contract("invalid pack MCP integration identity"));
            }
        }
    }
    Ok(())
}

impl Integration {
    /// Validate the typed shape independently of sample string contents.
    pub(crate) fn defaults_transport(&self, name: &str) -> Result<Transport, Error> {
        let defaults = self
            .defaults
            .as_ref()
            .ok_or(Error::Contract("MCP integration declares no defaults"))?;
        let error = |reason| Error::mcp_defaults(name, &self.default_variables, reason);
        let transport: Transport = serde_json::from_value(defaults.clone()).map_err(|_| {
            error("expected a transport with string command/URL, string-array args and string-map env")
        })?;
        if matches!(transport, Transport::Defaults { .. }) {
            return Err(error("defaults cannot reference other defaults"));
        }
        if defaults["type"] != self.transport {
            return Err(error(
                "transport type does not match the declared integration",
            ));
        }
        Ok(transport)
    }

    pub(crate) fn validate(&self, name: &str) -> Result<(), Error> {
        validate_commands(&self.command_fields)?;
        validate_effective(&self.effective_fields)?;
        if !["stdio", "http"].contains(&self.transport.as_str()) {
            return Err(Error::Contract("invalid pack MCP transport"));
        }
        policy::validate_mcp_fields(&self.fields)?;
        if let Some(defaults) = &self.defaults {
            self.defaults_transport(name)?
                .validate()
                .map_err(|error| match error {
                    Error::Contract(reason) => {
                        Error::mcp_defaults(name, &self.default_variables, reason)
                    }
                    other => other,
                })?;
            if !policy::matches_mcp_fields(&self.fields, defaults)? {
                return Err(Error::mcp_defaults(
                    name,
                    &self.default_variables,
                    "defaults do not match their declared integration",
                ));
            }
        }
        Ok(())
    }
    pub(crate) fn matches(&self, connection: &Connection) -> Result<bool, Error> {
        let transport = serde_json::to_value(&connection.transport).map_err(|_| Error::Syntax)?;
        if transport["type"] != self.transport {
            return Ok(false);
        }
        policy::matches_mcp_fields(&self.fields, &transport)
    }
}

#[cfg(test)]
mod tests;
