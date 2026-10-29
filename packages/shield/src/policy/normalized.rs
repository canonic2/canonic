//! Strict normalized input and project rule conditions, resolved by the shared policy resolver.
use super::{Error, Request};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Context {
    pub(crate) cwd: String,
    pub(crate) environment: Option<BTreeMap<String, String>>,
    /// Supplied only by the native adapter, never deserialized from tool input.
    #[serde(skip)]
    pub(crate) working_files: Option<crate::location::WorkingFiles>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
pub(crate) enum Input {
    Argv {
        executable: String,
        arguments: Vec<String>,
        original_input: Value,
    },
    Shell {
        source: String,
        dialect: String,
        dialect_version: Option<String>,
        original_input: Value,
    },
    File {
        action: String,
        sources: Vec<String>,
        destination: Option<String>,
        recursive: bool,
        original_input: Value,
    },
    Web {
        action: String,
        query: Option<String>,
        url: Option<String>,
        method: Option<String>,
        conditions: Value,
        original_input: Value,
    },
    Mcp {
        server: String,
        tool: String,
        arguments: Value,
        /// Trusted bound launch settings, never accepted from model input.
        #[serde(default, skip_deserializing, skip_serializing_if = "Option::is_none")]
        connection: Option<Value>,
        original_input: Value,
    },
    Unknown {
        host_tool: String,
        original_input: Value,
    },
}

impl Input {
    pub(crate) fn validate(&self, context: &Context, project: &str) -> Result<(), Error> {
        let value = json!(self);
        // Normalized fields and the complete vendor input can retain the same
        // source twice; their envelope bound must preserve the source capacity.
        if value.to_string().len() > 262_144
            || !value["original_input"].is_object()
            || crate::json::contains_nul(&value)
        {
            return Err(Error::Contract(
                "original_input must be a complete object without NULs",
            ));
        }
        Request {
            executable: "context".into(),
            arguments: vec![],
            cwd: context.cwd.clone(),
            project: project.into(),
        }
        .validate()?;
        if crate::json::contains_nul(&json!(context)) {
            return Err(Error::Contract("NUL in context"));
        }
        if context.environment.as_ref().is_some_and(|e| {
            e.len() > 256
                || e.iter()
                    .any(|(k, v)| k.is_empty() || k.len() > 4096 || v.len() > 4096)
        }) {
            return Err(Error::Limit);
        }
        match self {
            Self::Argv {
                executable,
                arguments,
                ..
            } => Request {
                executable: executable.clone(),
                arguments: arguments.clone(),
                cwd: context.cwd.clone(),
                project: project.into(),
            }
            .validate(),
            Self::Shell {
                source, dialect, ..
            } if source.len() <= 65536
                && ["bash", "posix", "powershell", "cmd"].contains(&dialect.as_str()) =>
            {
                Ok(())
            }
            Self::File {
                action,
                sources,
                destination,
                ..
            } if ["read", "write", "copy", "move", "remove"].contains(&action.as_str())
                && sources.len() <= 256
                && (action == "write" || !sources.is_empty())
                && (action != "write" || sources.is_empty())
                && (!["read", "remove"].contains(&action.as_str()) || destination.is_none())
                && sources
                    .iter()
                    .chain(destination.iter())
                    .all(|p| !p.is_empty() && p.len() <= 4096)
                && (!["write", "copy", "move"].contains(&action.as_str())
                    || destination.is_some()) =>
            {
                Ok(())
            }
            Self::Web {
                action,
                query,
                url,
                conditions,
                ..
            } if conditions.is_object()
                && ((action == "fetch" && url.is_some())
                    || (action == "search" && query.is_some())) =>
            {
                Ok(())
            }
            Self::Mcp {
                server,
                tool,
                arguments,
                ..
            } if !server.is_empty() && !tool.is_empty() && arguments.is_object() => Ok(()),
            Self::Unknown { host_tool, .. } if !host_tool.is_empty() => Ok(()),
            _ => Err(Error::Contract("invalid normalized input")),
        }
    }
}

pub(crate) use super::rules::Rule;
