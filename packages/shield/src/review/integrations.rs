//! Noninteractive integration ownership, protected settings, file reconciliation and repair.
use crate::{
    agents, policy,
    review::{self, Context, Error, model::State, storage::Store},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

pub(in crate::review) const MAX_CONFIG: u64 = 1024 * 1024;

pub(crate) fn verify_mcp_connection(
    root: &Path,
    agent: &str,
    alias: &str,
    expected: &crate::mcp_connections::Transport,
) -> Result<(), Error> {
    verify_mcp_connection_at(root, root, agent, alias, expected)
}

pub(crate) fn verify_mcp_connection_at(
    root: &Path,
    cwd: &Path,
    agent: &str,
    alias: &str,
    expected: &crate::mcp_connections::Transport,
) -> Result<(), Error> {
    let adapter = agents::find(agent).ok_or(Error::Invalid("unsupported MCP agent"))?;
    adapter.check_mcp_scopes(root, cwd, alias)?;
    let source = match adapter.spec().mcp_path {
        Some(path) => files::read(root, path)?,
        None => read_file(root)?,
    }
    .ok_or(Error::Invalid("bound MCP configuration is missing"))?;
    if adapter.connection(&source, alias)?.as_ref() != Some(expected) {
        return Err(Error::Invalid(
            "bound MCP connection changed; review setup in a terminal",
        ));
    }
    Ok(())
}

/// Administration paths supplied by all registered adapters, plus core state.
pub(crate) fn protected_files() -> &'static [&'static str] {
    static PATHS: std::sync::OnceLock<Vec<&'static str>> = std::sync::OnceLock::new();
    PATHS.get_or_init(|| {
        [".shield", ".mcp.json"]
            .into_iter()
            .chain(agents::ADAPTERS.iter().flat_map(|a| {
                std::iter::once(a.spec().hooks_path)
                    .chain(a.spec().mcp_path)
                    .chain(a.spec().prefix_rules_path)
                    .chain(a.spec().settings_paths.iter().copied())
            }))
            .collect()
    })
}
mod connections;
pub(crate) use connections::{mcp_projection, resolve_mcp_token};
mod privacy;
pub(crate) use privacy::private_mcp_destination;
pub(super) mod files;
pub(super) mod native;
pub(super) mod prefix_rules;
pub(crate) use prefix_rules::drifted as prefix_rules_drifted;
pub(in crate::review) use prefix_rules::{
    preview as preview_prefix_rules, sync as sync_prefix_rules,
};

#[derive(Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum AskFallback {
    #[default]
    Block,
    Allow,
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::review) struct Registration {
    pub(in crate::review) binary: PathBuf,
    pub(in crate::review) binary_digest: String,
    pub(in crate::review) version: String,
    pub(in crate::review) entry: Value,
    pub(in crate::review) agents: std::collections::BTreeMap<String, native::Installation>,
    pub(in crate::review) ask_fallback: std::collections::BTreeMap<String, AskFallback>,
    /// Tools the operator approved to run without asking; `None` until chosen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(in crate::review) approved_tools: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub(in crate::review) connections: std::collections::BTreeMap<String, Value>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::review) struct Plan {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(in crate::review) policy_revision: Option<String>,
    pub(in crate::review) before: Option<String>,
    pub(in crate::review) after: String,
    pub(in crate::review) previous: Option<Registration>,
    pub(in crate::review) desired: Option<Registration>,
    pub(in crate::review) files: Vec<files::Change>,
}

pub(in crate::review) fn registration(meta: &Value) -> Result<Option<Registration>, Error> {
    if meta["integration"].is_null() {
        return Ok(None);
    }
    let registration: Registration = serde_json::from_value(meta["integration"].clone())
        .map_err(|_| Error::Invalid("invalid integration ownership record"))?;
    if registration
        .agents
        .keys()
        .any(|id| agents::find(id).is_none())
    {
        return Err(Error::Invalid("unknown owned agent adapter"));
    }
    Ok(Some(registration))
}
pub(in crate::review) fn pending(meta: &Value) -> Result<Option<Plan>, Error> {
    if meta["integration_pending"].is_null() {
        return Ok(None);
    }
    serde_json::from_value(meta["integration_pending"].clone())
        .map(Some)
        .map_err(|_| Error::Invalid("invalid integration repair plan"))
}

pub(in crate::review) fn read_file(root: &Path) -> Result<Option<String>, Error> {
    let path = root.join(".mcp.json");
    match fs::symlink_metadata(&path) {
        Ok(m) if m.is_file() && !m.file_type().is_symlink() && m.len() <= MAX_CONFIG => {
            let mut bytes = String::new();
            fs::File::open(path)?
                .take(MAX_CONFIG + 1)
                .read_to_string(&mut bytes)?;
            if bytes.len() as u64 > MAX_CONFIG {
                return Err(Error::Invalid("MCP configuration exceeds limit"));
            }
            Ok(Some(bytes))
        }
        Ok(_) => Err(Error::Invalid(
            "MCP configuration must be a bounded regular file",
        )),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

pub(in crate::review) fn binary_digest(binary: &Path) -> Result<String, Error> {
    let mut bytes = Vec::new();
    fs::File::open(binary)?
        .take(128 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 128 * 1024 * 1024 {
        return Err(Error::Invalid("Shield binary exceeds setup limit"));
    }
    Ok(policy::fingerprint(&bytes))
}

pub(in crate::review) fn mcp_after(
    before: Option<&str>,
    previous: Option<&Registration>,
    desired: Option<&Registration>,
) -> Result<String, Error> {
    let shared = |r: &Registration| {
        r.agents
            .keys()
            .any(|id| agents::find(id).is_some_and(|a| a.spec().mcp_path.is_none()))
    };
    let touches = previous.is_some_and(shared) || desired.is_some_and(shared);
    if !touches {
        return Ok(before.unwrap_or_default().into());
    }
    let mut config = crate::json::parse(before.unwrap_or("{}").as_bytes())
        .map_err(|_| Error::Invalid("invalid existing MCP JSON"))?;
    let object = config
        .as_object_mut()
        .ok_or(Error::Invalid("MCP configuration must be an object"))?;
    let servers = object
        .entry("mcpServers")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or(Error::Invalid("mcpServers must be an object"))?;
    if let Some(existing) = servers.get("shield") {
        if previous.is_none_or(|r| &r.entry != existing) {
            return Err(Error::Invalid(
                "shield MCP entry is unowned or manually changed; resolve the conflict before setup",
            ));
        }
        servers.remove("shield");
    }
    if let Some(r) = desired
        && shared(r)
    {
        servers.insert("shield".into(), r.entry.clone());
    }
    Ok(format!(
        "{}\n",
        serde_json::to_string_pretty(&config).map_err(|_| Error::Invalid("setup encoding"))?
    ))
}

/// The operator's saved fallback for an installed agent, if one was confirmed.
pub(in crate::review) fn confirmed_fallback(
    meta: &Value,
    agent: &str,
) -> Result<Option<AskFallback>, Error> {
    fallback(meta, agent)?;
    Ok(registration(meta)?
        .filter(|r| r.agents.contains_key(agent))
        .and_then(|r| r.ask_fallback.get(agent).copied()))
}

pub(crate) fn fallback(meta: &Value, agent: &str) -> Result<AskFallback, Error> {
    let registered = registration(meta)?;
    let repairing = pending(meta)?.is_some();
    if registered.as_ref().is_some_and(|r| {
        r.ask_fallback
            .keys()
            .any(|k| agents::find(k).is_none_or(|a| !a.spec().needs_ask_fallback))
    }) {
        return Err(Error::Invalid("invalid Ask fallback integration"));
    }
    if repairing {
        return Ok(AskFallback::Block);
    }
    Ok(registered
        .as_ref()
        .filter(|r| r.agents.contains_key(agent))
        .and_then(|r| r.ask_fallback.get(agent))
        .copied()
        .unwrap_or_default())
}

/// Hands off under the state lock when the activation and the protected path
/// access the request was checked against are both still current.
pub(crate) fn with_fallback<T>(
    root: &Path,
    activation: &str,
    agent: &str,
    checked: super::path_access::PathAccess,
    handoff: impl FnOnce(AskFallback) -> T,
) -> Result<T, Error> {
    let context = review::open(root)?;
    Store::open(root)?.inspect_checkpoint(&context, &|| Ok(()), |state| {
        if state
            .active
            .as_ref()
            .is_none_or(|a| a.activation_id() != activation)
            || super::path_access::load(&state.operator)? != checked
        {
            return Err(Error::StaleSnapshot);
        }
        Ok(handoff(fallback(&state.operator, agent)?))
    })
}

pub(in crate::review) fn apply(
    context: &Context,
    expected: &Value,
    plan: &Plan,
) -> Result<(), Error> {
    privacy::validate(Path::new(&context.project_root), plan)?;
    let store = Store::open(Path::new(&context.project_root))?;
    store.update(context, &|| Ok(()), |state| {
        let current = crate::review::project::metadata(state);
        if pending(&current)?.is_none()
            && plan
                .policy_revision
                .as_ref()
                .is_some_and(|revision| revision != &state.policy_revision)
        {
            return Err(Error::Invalid(
                "active MCP configuration changed during setup; review again",
            ));
        }
        if current["integration"] != expected["integration"]
            || current["integration_pending"] != expected["integration_pending"]
        {
            return Err(Error::Invalid(
                "integration changed during confirmation; review again",
            ));
        }
        files::validate_recovery(Path::new(&context.project_root), &plan.files)?;
        if pending(&current)?.is_none() {
            files::validate_before(Path::new(&context.project_root), &plan.files)?;
        }
        if read_file(Path::new(&context.project_root))? != plan.before
            && pending(&current)?.is_none()
        {
            return Err(Error::Invalid(
                "MCP file changed during confirmation; review again",
            ));
        }
        state.operator["integration_pending"] = json!(plan);
        Ok(json!({}))
    })?;
    privacy::validate(Path::new(&context.project_root), plan)?;
    store.update(context, &|| Ok(()), |state| {
        if state.operator["integration_pending"] != json!(plan) {
            return Err(Error::Invalid("integration repair plan changed"));
        }
        publish(Path::new(&context.project_root), plan)?;
        files::publish(Path::new(&context.project_root), &plan.files)?;
        if let Some(metadata) = state.operator.as_object_mut() {
            metadata.remove("integration_verification");
        }
        state.operator["integration"] = json!(plan.desired);
        state.operator["integration_pending"] = Value::Null;
        Ok(json!({}))
    })?;
    Ok(())
}

pub(in crate::review) fn publish(root: &Path, plan: &Plan) -> Result<(), Error> {
    let current = read_file(root)?;
    if current.is_none() && plan.before.is_none() && plan.after.is_empty() {
        return Ok(());
    }
    if current.as_deref() == Some(&plan.after) {
        return Ok(());
    }
    if current != plan.before {
        return Err(Error::Invalid(
            "MCP file changed; confirmed repair preserved",
        ));
    }
    let temporary = root.join(format!(".shield-mcp-{}.tmp", std::process::id()));
    let mut file = files::create_like(&temporary, &root.join(".mcp.json"))?;
    let result = (|| {
        file.write_all(plan.after.as_bytes())?;
        review::storage::sync(&file)?;
        if read_file(root)? != plan.before {
            return Err(Error::Invalid(
                "MCP file changed during publication; confirmed repair preserved",
            ));
        }
        fs::rename(&temporary, root.join(".mcp.json"))?;
        #[cfg(unix)]
        review::storage::sync(&fs::File::open(root)?)?;
        Ok::<(), Error>(())
    })();
    drop(file);
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result?;
    Ok(())
}

pub(in crate::review) fn status(context: &Context, state: &State) -> Value {
    let root = Path::new(&context.project_root);
    let meta = crate::review::project::metadata(state);
    let inspected = (|| {
        let registered = registration(&meta)?;
        let mut diagnostics = Vec::new();
        if pending(&meta)?.is_some() {
            diagnostics.push("confirmed setup requires repair");
        }
        let bytes = read_file(root)?;
        let config = crate::json::parse(bytes.as_deref().unwrap_or("{}").as_bytes())
            .map_err(|_| Error::Invalid("invalid existing MCP JSON"))?;
        if !config.is_object() || config.get("mcpServers").is_some_and(|v| !v.is_object()) {
            return Err(Error::Invalid("invalid MCP configuration shape"));
        }
        if registered.is_none() && !config["mcpServers"]["shield"].is_null() {
            diagnostics.push("shield MCP entry is not owned by this enrollment");
        }
        let mut agents = Vec::new();
        if let Some(r) = &registered {
            for (agent, installation) in &r.agents {
                fallback(&meta, agent)?;
                native::diagnose(root, agent, installation, &mut diagnostics)?;
                agents.push(agent.as_str());
            }
            if r.agents
                .keys()
                .any(|id| agents::find(id).is_some_and(|a| a.spec().mcp_path.is_none()))
                && config["mcpServers"]["shield"] != r.entry
            {
                diagnostics.push("owned MCP entry missing or changed");
            }
            if binary_digest(&r.binary).ok().as_deref() != Some(&r.binary_digest) {
                diagnostics.push("registered binary missing or changed");
            }
            if r.version != env!("CARGO_PKG_VERSION") {
                diagnostics.push("registered binary version differs from this CLI");
            }
        }
        let prefixed = registered.as_ref().is_some_and(|r| {
            r.agents
                .keys()
                .any(|id| agents::find(id).is_some_and(|a| a.spec().prefix_rules_path.is_some()))
        });
        // Without a readable activation there is nothing to compare against.
        if let Some(prefixes) = if prefixed {
            prefix_rules::active(root).ok()
        } else {
            Some(Vec::new())
        } {
            prefix_rules::diagnose(root, &meta, &prefixes, &mut diagnostics);
        }
        let connections = connections::diagnose(context, registered.as_ref(), &mut diagnostics);
        let healthy = diagnostics.is_empty();
        Ok::<_, Error>(json!({
            "mcp_connections":connections,
            "registered_integrations":agents.iter().map(|agent|json!({"agent":agent,"surface":"mcp","status":if healthy {"registered"} else {"degraded"}})).collect::<Vec<_>>(),
            "installed_integrations":registered.as_ref().map(|r|r.agents.keys().map(|agent| {
                let adapter=agents::find(agent);
                let categories=adapter.map(crate::agents::AgentAdapter::working_file_categories).unwrap_or_default();
                json!({"agent":agent,"surface":"native-hooks","status":if healthy {"installed"} else {"degraded"},
                    "working_files":{"categories":categories,"session_discovery":"request-time","runtime_assets":if categories.contains(&"runtime_assets") {"supported"} else {"unsupported"},"non_temporary_scratch":if categories.contains(&"non_temporary_scratch") {"supported"} else {"unsupported"}},
                    "native_approval":adapter.is_some_and(|a|a.approval(agents::Event::Before)==crate::enforcement::protocol::NativeApproval::Prompt),
                    "permission_request":adapter.is_some_and(|a|a.spec().events.iter().any(|(_,e)|*e==agents::Event::Permission)),
                    "ask_fallback":if adapter.is_some_and(|a|a.spec().needs_ask_fallback) { json!(fallback(&meta,agent).unwrap_or_default()) } else { Value::Null }})
            }).collect::<Vec<_>>()).unwrap_or_default(),
            "diagnostics":diagnostics,"repair_needed":!healthy,
            "tool_interception":if agents.is_empty() {"not installed"} else {"native hooks installed; host loading is unverified"},
            "live_host_verification":"unverified","verification":{"state":"unverified","coverage":"Claude text reads and submitted shell command policy; native grant retries pending"},
            "discovery":"restart host; trust project, review native hooks and approve MCP",
            "operator_protection":"native hooks can be skipped or fail open on host timeout/startup failure; no same-user OS isolation"
        }))
    })();
    inspected.unwrap_or_else(|e| json!({"registered_integrations":[],"installed_integrations":[],"diagnostics":[e.to_string()],"repair_needed":true,"tool_interception":"unknown","live_host_verification":"unverified","verification":{"state":"unverified"}}))
}
