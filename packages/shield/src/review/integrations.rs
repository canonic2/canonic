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
mod reporting;
pub(super) use reporting::enforcement;

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

/// Administration path candidates supplied by adapters and core state.
/// The historical interceptor folder additionally requires retained ownership.
pub(crate) fn protected_files() -> &'static [&'static str] {
    static PATHS: std::sync::OnceLock<Vec<&'static str>> = std::sync::OnceLock::new();
    PATHS.get_or_init(|| {
        [
            ".shield",
            ".mcp.json",
            ".opencode/plugins",
            ".opencode/tool",
            ".opencode/tools",
            ".github/hooks",
        ]
        .into_iter()
        .chain(agents::ADAPTERS.iter().flat_map(|a| {
            std::iter::once(a.spec().hooks_path)
                .chain(a.spec().mcp_path)
                .chain(a.spec().prefix_rules_path)
                .chain(a.spec().settings_paths.iter().copied())
        }))
        .chain([
            box_definition::PATH,
            box_definition::LOCAL_PATH,
            legacy::BASE,
        ])
        .collect()
    })
}
pub(crate) mod box_definition;
mod choice;
pub(crate) use choice::IntegrationChoice;
pub(in crate::review) mod activation;
mod connections;
pub(crate) use activation::resume_launch as resume_mcp_launch;
mod legacy;
pub(crate) use legacy::BASE as LEGACY_INTERCEPTOR_PATH;
pub(in crate::review) mod project_sources;
pub(in crate::review) mod setup;
pub(crate) use connections::{mcp_projection, resolve_mcp_token};
pub(super) mod files;
mod maintenance;
pub(super) mod native;
pub(in crate::review) mod portable;
pub(super) mod prefix_rules;
mod privacy;
mod rebind;
pub(in crate::review) use rebind::resume as rebind_launchers;
pub(in crate::review) const HOST_PATH_WARNING: &str = "Shield project entries use the installed shield command and the host's project working directory. Connector settings you supply may contain machine-specific paths. Host files may be tracked; review their diff before sharing. Private connector settings remain in Shield's private state.";
mod reconcile;
pub(in crate::review) mod recovery;
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
    /// Exact assets retained by the earliest installed Claude mod integration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(in crate::review) interceptor: Option<legacy::Installation>,
    // Native registrations predate the fallback selection. Absence retains
    // the original Block behavior; it never implies permission to Allow Ask.
    #[serde(default)]
    pub(in crate::review) ask_fallback: std::collections::BTreeMap<String, AskFallback>,
    /// Tools the operator approved to run without asking; `None` until chosen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(in crate::review) approved_tools: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub(in crate::review) connections: std::collections::BTreeMap<String, Value>,
    /// The exact Box definition files Shield wrote, when Box is set up.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(in crate::review) box_definition: Option<box_definition::Definition>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::review) struct Plan {
    /// Absent on retained plans which reviewed the complete registration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(in crate::review) targets: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(in crate::review) scope_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(in crate::review) policy_revision: Option<String>,
    pub(in crate::review) before: Option<String>,
    pub(in crate::review) after: String,
    pub(in crate::review) previous: Option<Registration>,
    pub(in crate::review) desired: Option<Registration>,
    pub(in crate::review) files: Vec<files::Change>,
    /// Direct installations whose records the plan drops while their native
    /// files stay in place for Box to adopt.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(in crate::review) handed_over: Vec<String>,
}

pub(in crate::review) fn registration(meta: &Value) -> Result<Option<Registration>, Error> {
    if meta["integration"].is_null() {
        return Ok(None);
    }
    let mut source = meta["integration"].clone();
    legacy::normalize_registration(&mut source);
    let registration: Registration = serde_json::from_value(source)
        .map_err(|_| Error::Invalid("invalid integration ownership record"))?;
    if registration
        .agents
        .keys()
        .any(|id| agents::find(id).is_none())
    {
        return Err(Error::Invalid("unknown owned agent adapter"));
    }
    if let Some(interceptor) = &registration.interceptor {
        interceptor.validate()?;
    }
    Ok(Some(registration))
}
pub(in crate::review) fn pending(meta: &Value) -> Result<Option<Plan>, Error> {
    if meta["integration_pending"].is_null() {
        return Ok(None);
    }
    let mut source = meta["integration_pending"].clone();
    if !source.is_object() {
        return Err(Error::Invalid("invalid integration repair plan"));
    }
    if source.get("files").is_none() && legacy::historical_plan(&source) {
        source["files"] = json!([]);
    }
    for field in ["previous", "desired"] {
        legacy::normalize_registration(&mut source[field]);
    }
    let plan: Plan = serde_json::from_value(source)
        .map_err(|_| Error::Invalid("invalid integration repair plan"))?;
    maintenance::validate(&plan)?;
    if [plan.previous.as_ref(), plan.desired.as_ref()]
        .into_iter()
        .flatten()
        .any(|registration| {
            registration
                .agents
                .keys()
                .any(|id| agents::find(id).is_none())
        })
    {
        return Err(Error::Invalid("unknown owned agent adapter"));
    }
    legacy::validate_plan(&plan)?;
    Ok(Some(plan))
}

/// Initial adoption may carry provisional file ownership before any registered
/// integration exists. A later plan must still name the committed predecessor.
pub(super) fn validate_pending_ownership(meta: &Value) -> Result<(), Error> {
    if let Some(plan) = pending(meta)? {
        let committed = registration(meta)?;
        if (committed.is_some() || plan.policy_revision.is_some()) && committed != plan.previous {
            return Err(Error::Invalid(
                "pending setup no longer matches committed integration ownership; review cancellation in this session",
            ));
        }
    }
    Ok(())
}

/// Old mod registration requests precede host attribution. Retained ownership
/// selects their receipt representation, never permission to read or execute.
pub(crate) fn has_legacy_interceptor(root: &Path) -> bool {
    review::open(root)
        .and_then(|context| review::project::checkpoint(&context))
        .and_then(|state| registration(&state.operator))
        .ok()
        .flatten()
        .is_some_and(|registration| registration.interceptor.is_some())
}

/// A reserved spelling alone does not make an ordinary skill an owned plugin.
pub(crate) fn protects_legacy_path(path: &Path) -> bool {
    let parts: Vec<_> = path.components().collect();
    let owned: Vec<_> = Path::new(legacy::BASE).components().collect();
    parts
        .windows(owned.len())
        .enumerate()
        .any(|(start, window)| {
            window.iter().zip(&owned).all(|(left, right)| {
                crate::location::native_name(left.as_os_str(), right.as_os_str())
            }) && legacy_path_owned(&parts[..start].iter().collect::<std::path::PathBuf>())
        })
}

fn legacy_path_owned(root: &Path) -> bool {
    match review::storage::Store::open(root) {
        Ok(store) => store
            .read_diagnostic_checkpoint()
            .and_then(|(state, _)| registration(&state.operator))
            .map_or(true, |registered| {
                registered.is_some_and(|r| r.interceptor.is_some())
            }),
        Err(Error::Missing) => false,
        Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(_) => true,
    }
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
    repair: bool,
) -> Result<String, Error> {
    let shared = |r: &Registration| {
        r.interceptor.is_some()
            || r.agents
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
        if !repair && previous.is_none_or(|r| &r.entry != existing) {
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
        .filter(|r| r.covers(agent))
        .and_then(|r| r.ask_fallback.get(agent).copied()))
}

impl Registration {
    /// Whether this record installs `agent`'s hooks, directly or through the
    /// Box definition, for adapters its publisher supports.
    pub(in crate::review) fn covers(&self, agent: &str) -> bool {
        self.native_covers(agent) || (agent == "claude" && self.interceptor.is_some())
    }

    pub(in crate::review) fn native_covers(&self, agent: &str) -> bool {
        self.agents.contains_key(agent)
            || (self.box_definition.is_some()
                && agents::find(agent).is_some_and(agents::AgentAdapter::supports_box_hooks))
    }
}

pub(crate) fn fallback(meta: &Value, agent: &str) -> Result<AskFallback, Error> {
    let registered = registration(meta)?;
    let repairing = pending(meta)?.is_some_and(|plan| plan.affects(agent));
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
        .filter(|r| r.covers(agent))
        .and_then(|r| r.ask_fallback.get(agent))
        .copied()
        .unwrap_or_default())
}

/// Hands off under the state lock when the activation and the protected path
/// access the request was checked against are both still current.
pub(crate) fn with_fallback<T>(
    root: &Path,
    agent: &str,
    checked: super::path_access::PathAccess,
    handoff: impl FnOnce(AskFallback) -> T,
) -> Result<T, Error> {
    let context = review::open(root)?;
    Store::open(root)?.inspect_checkpoint(&context, &|| Ok(()), |state| {
        if super::path_access::load(&state.operator)? != checked {
            return Err(Error::StaleSnapshot);
        }
        Ok(handoff(fallback(&state.operator, agent)?))
    })
}

#[cfg(test)]
pub(in crate::review) fn apply(
    context: &Context,
    expected: &Value,
    plan: &Plan,
) -> Result<(), Error> {
    apply_owned(context, expected, plan, &|| Ok(()), None)
}

#[cfg(test)]
pub(in crate::review) fn apply_owned(
    context: &Context,
    expected: &Value,
    plan: &Plan,
    checkpoint: super::Checkpoint<'_>,
    activation: Option<&str>,
) -> Result<(), Error> {
    super::project::checkpoint(context)?;
    let publication =
        super::publication::ProjectionLease::acquire(Path::new(&context.project_root))?;
    apply_owned_guarded(
        context,
        expected,
        plan,
        checkpoint,
        activation,
        &publication,
    )
}

pub(in crate::review) fn apply_owned_guarded(
    context: &Context,
    expected: &Value,
    plan: &Plan,
    checkpoint: super::Checkpoint<'_>,
    activation: Option<&str>,
    publication: &super::publication::ProjectionLease,
) -> Result<(), Error> {
    publication.check(context)?;
    checkpoint()?;
    maintenance::validate(plan)?;
    privacy::validate(Path::new(&context.project_root), plan)?;
    let root = Path::new(&context.project_root);
    // The connections reviewed came from the policy files; refuse files that
    // changed since. Without files there are no connections to compare.
    let files_revision = if root.join(".shield/config.yml").exists() {
        Some(super::files::load(root)?.evaluator.revision().to_owned())
    } else {
        None
    };
    let store = Store::open(root)?;
    store.update_checkpoint(context, checkpoint, |state| {
        activation::check_owner(state, expected, activation)?;
        let current = crate::review::project::metadata(state);
        validate_pending_ownership(&current)?;
        if pending(&current)?.is_none()
            && plan.policy_revision.is_some()
            && plan.policy_revision != files_revision
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
        files::validate_plan_recovery(Path::new(&context.project_root), plan)?;
        if plan.touches_shared() {
            reconcile::reconcile(
                ".mcp.json",
                plan.before.as_deref(),
                shared_after(plan),
                read_file(Path::new(&context.project_root))?.as_deref(),
            )?;
        }
        if pending(&current)?.is_none() {
            files::validate_plan_before(Path::new(&context.project_root), plan)?;
        }
        if plan.touches_shared()
            && read_file(Path::new(&context.project_root))? != plan.before
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
    // The committed repair plan owns these bytes. All competing output
    // publishers wait on the projection lease, leaving request authority free.
    let current = store.read_checkpoint()?;
    current.check(context)?;
    activation::check_owner(&current, expected, activation)?;
    if current.operator["integration_pending"] != json!(plan)
        || current.operator["integration"] != expected["integration"]
    {
        return Err(Error::StaleSnapshot);
    }
    files::validate_plan_recovery(Path::new(&context.project_root), plan)?;
    publish(Path::new(&context.project_root), plan)?;
    files::publish_plan(Path::new(&context.project_root), plan)?;
    store.update_checkpoint(context, &|| Ok(()), |state| {
        activation::check_owner(state, expected, activation)?;
        if state.operator["integration_pending"] != json!(plan)
            || state.operator["integration"] != expected["integration"]
        {
            return Err(Error::Invalid("integration repair plan changed"));
        }
        validate_pending_ownership(&crate::review::project::metadata(state))?;
        files::validate_plan_recovery(Path::new(&context.project_root), plan)?;
        let previous = registration(&state.operator)?;
        super::session_grants::preserve_integration(
            state,
            previous.as_ref(),
            plan.desired.as_ref(),
        )?;
        if let Some(metadata) = state.operator.as_object_mut() {
            metadata.remove("integration_verification");
            metadata.remove("integration_publication");
        }
        let changed = state.operator["integration"] != json!(plan.desired);
        state.operator["integration"] = json!(plan.desired);
        state.operator["integration_pending"] = Value::Null;
        if changed {
            super::configuration_changes::changed(state, context, "agent registrations");
        }
        Ok(json!({}))
    })?;
    Ok(())
}

fn shared_after(plan: &Plan) -> Option<&str> {
    if plan.before.is_none() && plan.after.is_empty() {
        None
    } else {
        Some(&plan.after)
    }
}

pub(in crate::review) fn publish(root: &Path, plan: &Plan) -> Result<(), Error> {
    if !plan.touches_shared() {
        return Ok(());
    }
    let current = read_file(root)?;
    let after = reconcile::reconcile(
        ".mcp.json",
        plan.before.as_deref(),
        shared_after(plan),
        current.as_deref(),
    )?;
    if current == after {
        return Ok(());
    }
    let after = after.ok_or(Error::Invalid("invalid shared MCP removal"))?;
    let temporary = root.join(format!(".shield-mcp-{}.tmp", std::process::id()));
    let mut file = files::create_like(&temporary, &root.join(".mcp.json"))?;
    let result = (|| {
        file.write_all(after.as_bytes())?;
        review::storage::sync(&file)?;
        if read_file(root)? != current {
            return Err(Error::Invalid(
                "MCP file changed during publication; confirmed repair preserved",
            ));
        }
        super::storage::replace(&temporary, &root.join(".mcp.json"))?;
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

/// The recorded Box definition's state and the Ask fallbacks it carries.
fn box_status(
    root: &Path,
    meta: &Value,
    owned: &box_definition::Definition,
    diagnostics: &mut Vec<&'static str>,
) -> Value {
    let mut state = box_definition::diagnose(root, owned, diagnostics);
    // Ownership compares text only; Box also refuses a private pin others can
    // read, which `shield mcp setup --agent box` repairs in place.
    if state == "installed" && files::private_exposed(root) {
        diagnostics
            .push("Box's private Shield definition is readable by other users, so Box refuses it");
        state = "changed";
    }
    let fallbacks: serde_json::Map<String, Value> = agents::ADAPTERS
        .iter()
        .filter(|a| a.spec().needs_ask_fallback)
        .map(|a| {
            let id = a.spec().id;
            (id.to_owned(), json!(fallback(meta, id).unwrap_or_default()))
        })
        .collect();
    json!({"agent":box_definition::ID,"path":box_definition::PATH,
        "local_path":box_definition::LOCAL_PATH,"status":state,
        "ask_fallback":fallbacks,"publish":"box setup (first time) or box config sync"})
}

fn diagnose_binary(
    registration: &Registration,
    metadata: &Value,
    diagnostics: &mut Vec<&'static str>,
) -> Result<(), Error> {
    // Replacing the installed executable at its registered path is an ordinary
    // upgrade. Old setup bytes/version do not establish policy authority.
    let running = std::env::current_exe()?.canonicalize()?;
    if current_executable_receipt(registration, metadata, &running) {
        return Ok(());
    }
    if registration.binary.canonicalize().ok().as_ref() != Some(&running) {
        match std::fs::metadata(&registration.binary) {
            Ok(metadata) if metadata.is_file() => diagnostics.push(
                "registered binary path differs from this CLI; content identity and compatibility are unchecked",
            ),
            Ok(_) => diagnostics.push("registered binary path is not a regular file"),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                diagnostics.push("registered binary is missing");
            }
            Err(_) => diagnostics.push("registered binary presence is unavailable"),
        }
        if registration.version != env!("CARGO_PKG_VERSION") {
            diagnostics.push(
                "recorded registered binary version differs; current compatibility is unchecked",
            );
        }
    }
    Ok(())
}

/// Derived location evidence only; neither presence nor a saved digest verifies
/// today's payload. Native handoff and administrative review retain their pins.
fn current_executable_receipt(
    registration: &Registration,
    metadata: &Value,
    running: &Path,
) -> bool {
    let receipt = &metadata["current_executable"];
    receipt.as_object().is_some_and(|fields| fields.len() == 3)
        && receipt["schema_version"] == 1
        && receipt["digest"] == registration.binary_digest
        && registration.binary_digest.len() == 64
        && registration
            .binary_digest
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
        && receipt["path"].as_str().is_some_and(|path| {
            let path = Path::new(path);
            path.is_absolute()
                && path.canonicalize().ok().as_deref() == Some(running)
                && std::fs::metadata(path).is_ok_and(|metadata| metadata.is_file())
        })
}

fn checked_mcp_config(
    root: &Path,
    has_registration: bool,
    diagnostics: &mut Vec<&'static str>,
) -> Result<Value, Error> {
    let bytes = read_file(root)?;
    let config = crate::json::parse(bytes.as_deref().unwrap_or("{}").as_bytes())
        .map_err(|_| Error::Invalid("invalid existing MCP JSON"))?;
    if !config.is_object() || config.get("mcpServers").is_some_and(|v| !v.is_object()) {
        return Err(Error::Invalid("invalid MCP configuration shape"));
    }
    if !has_registration && !config["mcpServers"]["shield"].is_null() {
        diagnostics.push("shield MCP entry is not owned by this enrollment");
    }
    Ok(config)
}

fn diagnose_registered<'a>(
    root: &Path,
    meta: &Value,
    r: &'a Registration,
    config: &Value,
    unsettled: Option<&Plan>,
    diagnostics: &mut Vec<&'static str>,
    degraded: &mut std::collections::BTreeSet<&'a str>,
) -> (Vec<&'a str>, Value, Value) {
    let mut agents = Vec::new();
    for (agent, installation) in &r.agents {
        let before = diagnostics.len();
        if fallback(meta, agent).is_err() {
            diagnostics.push("invalid Ask fallback integration");
        }
        if native::diagnose(root, agent, &r.binary, installation, diagnostics).is_err() {
            diagnostics.push("agent native configuration is unreadable");
        }
        if diagnostics.len() != before || unsettled.is_some_and(|p| p.affects(agent)) {
            degraded.insert(agent.as_str());
        }
        agents.push(agent.as_str());
    }
    let mut legacy_interceptor = Value::Null;
    if let Some(interceptor) = &r.interceptor {
        let before = diagnostics.len();
        if legacy::diagnose(root, interceptor, diagnostics).is_err() {
            diagnostics.push("legacy interceptor configuration is unreadable");
        }
        if diagnostics.len() != before || unsettled.is_some_and(|p| p.affects("claude")) {
            degraded.insert("claude");
        }
        legacy_interceptor = json!({"agent":"claude","surface":"legacy-text-read-interceptor",
            "status":if degraded.contains("claude") {"degraded"} else {"installed"}});
        if !agents.contains(&"claude") {
            agents.push("claude");
        }
    }
    if (r.interceptor.is_some()
        || r.agents
            .keys()
            .any(|id| agents::find(id).is_some_and(|a| a.spec().mcp_path.is_none())))
        && config["mcpServers"]["shield"] != r.entry
    {
        diagnostics.push("owned MCP entry missing or changed");
        degraded.extend(
            agents
                .iter()
                .copied()
                .filter(|id| agents::find(id).is_some_and(|a| a.spec().mcp_path.is_none())),
        );
    }
    let mut box_integration = r.box_definition.as_ref().map_or(Value::Null, |owned| {
        box_status(root, meta, owned, diagnostics)
    });
    let before = diagnostics.len();
    if diagnose_binary(r, meta, diagnostics).is_err() {
        diagnostics.push("registered binary is unreadable");
    }
    if diagnostics.len() != before {
        degraded.extend(agents.iter().copied());
    }
    if !box_integration.is_null()
        && (diagnostics.len() != before || unsettled.is_some_and(|p| p.affects(box_definition::ID)))
    {
        box_integration["definition_status"] = box_integration["status"].clone();
        box_integration["status"] = json!("degraded");
    }
    (agents, box_integration, legacy_interceptor)
}

/// Whether this folder's agent files hold exactly what the registration in
/// `meta` owns. The installed binary's location is not part of the match.
pub(in crate::review) fn matches_files(root: &Path, meta: &Value) -> bool {
    let Ok(Some(registered)) = registration(meta) else {
        return false;
    };
    let mut diagnostics = Vec::new();
    let Ok(config) = checked_mcp_config(root, true, &mut diagnostics) else {
        return false;
    };
    let mut degraded = std::collections::BTreeSet::new();
    let (agents, _, _) = diagnose_registered(
        root,
        meta,
        &registered,
        &config,
        None,
        &mut diagnostics,
        &mut degraded,
    );
    !agents.is_empty()
        && diagnostics.iter().all(|reason| {
            reason.starts_with("registered binary")
                || reason.starts_with("recorded registered binary")
        })
}

fn agent_output_drift(
    root: &Path,
    state: &State,
    snapshot: Option<&review::cache::Inspection>,
    r: &Registration,
    agent: &str,
    connections: &[Value],
) -> bool {
    let prefix_drift = agents::find(agent)
        .and_then(|a| a.spec().prefix_rules_path.map(|path| (a, path)))
        .is_some_and(|(adapter, path)| {
            snapshot.is_some_and(|snapshot| {
                let prefixes = if state.enforcement.enabled {
                    snapshot.codex_prefixes()
                } else {
                    &[]
                };
                match files::read(root, path) {
                    Ok(text) => text != adapter.prefix_rules(prefixes),
                    Err(_) => true,
                }
            })
        });
    let stale_projection = r.connections.get(agent).is_some_and(|owned| {
        owned["entries"]
            .as_object()
            .into_iter()
            .flatten()
            .any(|(alias, _)| snapshot.is_some_and(|s| s.mcp_connection(agent, alias).is_none()))
    });
    prefix_drift
        || stale_projection
        || connections.iter().any(|row| {
            row["agent"] == agent
                && matches!(
                    row["configuration"].as_str(),
                    Some(
                        "missing or changed"
                            | "hooks missing"
                            | "discovery unavailable"
                            | "ambiguous or unsupported configuration"
                    )
                )
        })
}

fn diagnose_prefixes(
    root: &Path,
    meta: &Value,
    registered: Option<&Registration>,
    snapshot: Option<&review::cache::Inspection>,
    enabled: bool,
    diagnostics: &mut Vec<&'static str>,
) {
    let prefixed = registered.is_some_and(|r| {
        agents::ADAPTERS
            .iter()
            .any(|a| r.covers(a.spec().id) && a.spec().prefix_rules_path.is_some())
    });
    // Without a readable activation there is nothing to compare against.
    if let Some(prefixes) = if prefixed {
        snapshot.map(super::cache::Inspection::codex_prefixes)
    } else {
        Some([].as_slice())
    } {
        prefix_rules::diagnose(root, meta, prefixes, enabled, diagnostics);
    }
}

pub(in crate::review) fn status(context: &Context, state: &State) -> Value {
    let root = Path::new(&context.project_root);
    let meta = crate::review::project::metadata(state);
    let inspected = (|| {
        let registered = registration(&meta)?;
        let mut diagnostics = Vec::new();
        let unsettled = pending(&meta)?;
        let mut degraded = std::collections::BTreeSet::new();
        if unsettled.is_some() {
            diagnostics.push("confirmed setup requires repair");
        }
        let config = checked_mcp_config(root, registered.is_some(), &mut diagnostics)
            .unwrap_or_else(|_| {
                diagnostics.push("shared MCP configuration is unreadable or invalid");
                Value::Null
            });
        let (agents, mut box_integration, legacy_interceptor) = registered.as_ref().map_or_else(
            || (Vec::new(), Value::Null, Value::Null),
            |r| {
                diagnose_registered(
                    root,
                    &meta,
                    r,
                    &config,
                    unsettled.as_ref(),
                    &mut diagnostics,
                    &mut degraded,
                )
            },
        );
        // Coverage reads only activation-bound descriptions. It never loads a
        // plan, rebuilds a cache or prepares retained source for presentation.
        let snapshot = state
            .active
            .as_ref()
            .and_then(|active| active.inspection(state));
        diagnose_prefixes(
            root,
            &meta,
            registered.as_ref(),
            snapshot,
            state.enforcement.enabled,
            &mut diagnostics,
        );
        let connections =
            connections::diagnose(root, snapshot, registered.as_ref(), &mut diagnostics);
        if let Some(r) = &registered {
            for agent in &agents {
                if agent_output_drift(root, state, snapshot, r, agent, &connections) {
                    degraded.insert(*agent);
                }
            }
            if r.box_definition.is_some()
                && agents::ADAPTERS.iter().any(|adapter| {
                    let agent = adapter.spec().id;
                    adapter.supports_box_hooks()
                        && !r.agents.contains_key(agent)
                        && agent_output_drift(root, state, snapshot, r, agent, &connections)
                })
            {
                if box_integration.get("definition_status").is_none() {
                    box_integration["definition_status"] = box_integration["status"].clone();
                }
                box_integration["status"] = json!("degraded");
            }
        }
        let published =
            reporting::box_hooks(root, registered.as_ref(), &state.operator, &mut diagnostics);
        let healthy = diagnostics.is_empty();
        let mut report = json!({
            "binary_inspection":registered.as_ref().map(|registration| json!({
                "current_location_recorded":std::env::current_exe().ok().and_then(|path|path.canonicalize().ok()).is_some_and(|running|current_executable_receipt(registration,&meta,&running)),
                "payload_integrity":"unchecked","compatibility":"unchecked"
            })),
            "policy_inspection":{"availability":if snapshot.is_some() {"recorded"}else{"unavailable"},"source_validation":"not-performed","automatic_matching":"checked-on-interception"},
            "box_integration":box_integration,
            "legacy_interceptor":legacy_interceptor,
            "mcp_connections":connections,
            "registered_integrations":agents.iter().map(|agent|json!({"agent":agent,"surface":"mcp","status":if degraded.contains(agent) {"degraded"} else {"registered"}})).collect::<Vec<_>>(),
            "installed_integrations":registered.as_ref().map(|r|r.agents.keys().map(|agent| {
                let adapter=agents::find(agent);
                let categories=adapter.map(crate::agents::AgentAdapter::working_file_categories).unwrap_or_default();
                json!({"agent":agent,"surface":"native-hooks","status":if degraded.contains(agent.as_str()) {"degraded"} else {"installed"},
                    "events":r.agents[agent].groups.keys().collect::<Vec<_>>(),
                    "working_files":{"categories":categories,"session_discovery":"request-time","runtime_assets":if categories.contains(&"runtime_assets") {"supported"} else {"unsupported"},"non_temporary_scratch":if categories.contains(&"non_temporary_scratch") {"supported"} else {"unsupported"}},
                    "native_approval":adapter.is_some_and(|a|a.approval(agents::Event::Before)==crate::enforcement::protocol::NativeApproval::Prompt),
                    "permission_request":adapter.is_some_and(|a|a.spec().events.iter().any(|(event,e)|*e==agents::Event::Permission && r.agents[agent].groups.contains_key(*event))),
                    "ask_fallback":if adapter.is_some_and(|a|a.spec().needs_ask_fallback) { json!(fallback(&meta,agent).unwrap_or_default()) } else { Value::Null }})
            }).collect::<Vec<_>>()).unwrap_or_default(),
            "diagnostics":diagnostics,"repair_needed":!healthy,
            "tool_interception":if registered.as_ref().is_some_and(|r|!r.agents.is_empty()) {"native hooks installed; host loading is unverified"} else if !legacy_interceptor.is_null() {"legacy text-read interceptor installed; host loading is unverified"} else if box_integration.is_null() {"not installed"} else {"native hooks defined for Box to publish; host loading is unverified"},
            "live_host_verification":"unverified",
            "discovery":"restart host; trust project, review native hooks and approve MCP",
            "operator_protection":"native hooks can be skipped or fail open on host timeout/startup failure; no same-user OS isolation"
        });
        reporting::complete(&mut report, state, published);
        Ok::<_, Error>(report)
    })();
    inspected.unwrap_or_else(|e| json!({"registered_integrations":[],"installed_integrations":[],"diagnostics":[e.to_string()],"repair_needed":true,"tool_interception":"unknown","live_host_verification":"unverified","verification":{"state":"unverified"}}))
}
