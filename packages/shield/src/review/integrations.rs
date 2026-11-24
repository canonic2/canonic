//! Noninteractive integration ownership, protected settings, file reconciliation and repair.
use crate::configuration::settings::Values;
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
            crate::layout::SHIELD,
            crate::layout::GROUPED_BUNDLE,
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
        .chain(host_definition::paths())
        .chain([legacy::BASE])
        .collect()
    })
}
mod choice;
pub(crate) mod host_definition;
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

/// What a host that cannot ask does with a request policy asks about: the
/// `ask_fallback` setting of the policy files.
pub(crate) use crate::configuration::settings::Fallback as AskFallback;

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
    /// The Ask fallbacks a setup plan chooses for the agents it covers,
    /// written to `config.local.yml` when the setup commits. A record never
    /// holds them; earlier records that did are read, and their values move
    /// to the policy files ([`super::settings::Legacy`]).
    #[serde(default, skip_serializing)]
    pub(in crate::review) ask_fallback: std::collections::BTreeMap<String, AskFallback>,
    /// Tools the operator approved to run without asking; `None` until chosen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(in crate::review) approved_tools: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub(in crate::review) connections: std::collections::BTreeMap<String, Value>,
    /// The exact Hub or Box definition files Shield wrote, when one is set up.
    /// The record key predates Hub and stays `box_definition`.
    #[serde(
        default,
        rename = "box_definition",
        skip_serializing_if = "Option::is_none"
    )]
    pub(in crate::review) definition: Option<host_definition::Definition>,
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
    /// files stay in place for Hub or Box to adopt.
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

/// Digests this process already computed, keyed by the file's identity. A
/// process opens state many times and each open may compare its executable,
/// so an unchanged file is hashed once. Any later write, replacement or rename
/// changes the inode or the change time, so it is hashed again; a file that
/// changed too recently for that to hold is not remembered (see [`settled`]).
static DIGESTS: std::sync::Mutex<Vec<(FileIdentity, String)>> = std::sync::Mutex::new(Vec::new());

/// Device, inode, size, and modification and change times in seconds and
/// nanoseconds.
type FileIdentity = (u64, u64, u64, i64, i64, i64, i64);

/// The identity of the open file itself, so it names the bytes read from it;
/// `None` where the platform has none, which leaves the file unmemoized.
#[cfg(unix)]
fn file_identity(file: &fs::File) -> std::io::Result<Option<FileIdentity>> {
    use std::os::unix::fs::MetadataExt;
    let metadata = file.metadata()?;
    Ok(Some((
        metadata.dev(),
        metadata.ino(),
        metadata.size(),
        metadata.mtime(),
        metadata.mtime_nsec(),
        metadata.ctime(),
        metadata.ctime_nsec(),
    )))
}

#[cfg(not(unix))]
fn file_identity(_: &fs::File) -> std::io::Result<Option<FileIdentity>> {
    Ok(None)
}

/// Whether the file last changed at least a second before hashing started.
/// File systems with coarse timestamps, such as Linux before 6.13, give writes
/// within one clock tick the same change time, so only a file that settled
/// before the read is sure to get a new identity from any later write. This is
/// Git's rule for "racily clean" index entries.
fn settled(identity: &FileIdentity, started: std::time::SystemTime) -> bool {
    let changed = identity.5;
    started
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|since| i64::try_from(since.as_secs()).ok())
        .is_some_and(|started| changed < started - 1)
}

pub(in crate::review) fn binary_digest(binary: &Path) -> Result<String, Error> {
    binary_digest_started(binary, std::time::SystemTime::now())
}

fn binary_digest_started(binary: &Path, started: std::time::SystemTime) -> Result<String, Error> {
    let _trace = crate::diagnostics::trace::span("integration.binary_digest");
    let file = fs::File::open(binary)?;
    let identity = file_identity(&file)?;
    let digests = || {
        DIGESTS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    };
    let known = identity.and_then(|identity| {
        digests()
            .iter()
            .find(|(key, _)| *key == identity)
            .map(|(_, digest)| digest.clone())
    });
    if let Some(digest) = known {
        crate::diagnostics::trace::count("integration.binary_hash_reuses", 1);
        return Ok(digest);
    }
    crate::diagnostics::trace::count("integration.binary_hashes", 1);
    let mut bytes = Vec::new();
    (&file)
        .take(128 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 128 * 1024 * 1024 {
        return Err(Error::Invalid("Shield binary exceeds setup limit"));
    }
    let digest = policy::fingerprint(&bytes);
    // Remember it only if nothing changed the file while it was read.
    let unchanged = identity.filter(|identity| {
        settled(identity, started) && file_identity(&file).ok().flatten() == Some(*identity)
    });
    if let Some(identity) = unchanged {
        let mut known = digests();
        // A process compares a handful of executables; keep the newest few.
        if !known.iter().any(|(key, _)| *key == identity) {
            if known.len() == 8 {
                known.remove(0);
            }
            known.push((identity, digest.clone()));
        }
    }
    Ok(digest)
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

impl Registration {
    /// Whether this record installs `agent`'s hooks, directly or through the
    /// Hub or Box definition, for adapters its publisher supports.
    pub(in crate::review) fn covers(&self, agent: &str) -> bool {
        self.native_covers(agent) || (agent == "claude" && self.interceptor.is_some())
    }

    pub(in crate::review) fn native_covers(&self, agent: &str) -> bool {
        self.agents.contains_key(agent)
            || (self.definition.is_some()
                && agents::find(agent).is_some_and(agents::AgentAdapter::supports_box_hooks))
    }
}

/// The Ask fallback in effect for `agent`: the policy files' setting for an
/// agent setup covers, and Block while a repair affects it or setup does not
/// cover it.
pub(crate) fn fallback(meta: &Value, settings: &Values, agent: &str) -> Result<AskFallback, Error> {
    if pending(meta)?.is_some_and(|plan| plan.affects(agent))
        || !registration(meta)?.is_some_and(|r| r.covers(agent))
    {
        return Ok(AskFallback::Block);
    }
    Ok(settings.fallback(agent))
}

/// The Ask fallback changes a committed setup makes: each covered agent's
/// confirmed fallback, and Block for agents it stops covering whose files
/// name another. A confirmed fallback is written even when it is the default,
/// so the next setup offers it again.
fn fallback_changes(
    plan: &Plan,
    settings: &Values,
) -> std::collections::BTreeMap<String, AskFallback> {
    let mut changes = std::collections::BTreeMap::new();
    for adapter in agents::ADAPTERS {
        let id = adapter.spec().id;
        if !adapter.spec().needs_ask_fallback || !plan.affects(id) {
            continue;
        }
        let named = settings.ask_fallback.get(id);
        let chosen = match plan.desired.as_ref().filter(|r| r.covers(id)) {
            Some(desired) => desired.ask_fallback.get(id).copied(),
            None => named.map(|_| AskFallback::Block),
        };
        if let Some(chosen) = chosen.filter(|chosen| named != Some(chosen)) {
            changes.insert(id.to_owned(), chosen);
        }
    }
    changes
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
        if super::settings::path_access(&state.settings) != checked {
            return Err(Error::StaleSnapshot);
        }
        Ok(handoff(fallback(&state.operator, &state.settings, agent)?))
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
    let files_revision = if crate::layout::configured(root) {
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
        // Revocations take effect before publication, so an interrupted
        // setup never leaves a fallback it revoked in effect.
        let mut revoked = fallback_changes(plan, &state.settings);
        revoked.retain(|_, fallback| *fallback == AskFallback::Block);
        write_fallbacks(context, state, revoked)?;
        Ok(json!({}))
    })?;
    privacy::validate(Path::new(&context.project_root), plan)?;
    // The committed repair plan owns these bytes. All competing output
    // publishers wait on the projection lease, leaving request authority free.
    let current = store.read_following_files()?;
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
        state.operator["integration"] = json!(plan.desired);
        state.operator["integration_pending"] = Value::Null;
        write_fallbacks(context, state, fallback_changes(plan, &state.settings))?;
        Ok(json!({}))
    })?;
    Ok(())
}

/// Write Ask fallback changes into `config.local.yml`. A folder without
/// policy files keeps every fallback at Block.
fn write_fallbacks(
    context: &Context,
    state: &mut State,
    changes: std::collections::BTreeMap<String, AskFallback>,
) -> Result<(), Error> {
    if changes.is_empty() || !crate::layout::configured(Path::new(&context.project_root)) {
        return Ok(());
    }
    super::settings::write(context, &super::settings::fallbacks(changes.clone()))?;
    state.settings.ask_fallback.extend(changes);
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

/// The recorded Hub or Box definition's state and the Ask fallbacks it carries.
fn host_status(
    root: &Path,
    project: &State,
    owned: &host_definition::Definition,
    diagnostics: &mut Vec<&'static str>,
) -> Value {
    let host = owned.host();
    let mut state = host_definition::diagnose(root, owned, diagnostics);
    // Ownership compares text only; the host also refuses a private pin others
    // can read, which `shield mcp setup --agent <host>` repairs in place.
    if state == "installed" && files::private_exposed(root, host) {
        diagnostics.push(
            "the private Shield definition is readable by other users, so Hub and Box refuse it",
        );
        state = "changed";
    }
    let fallbacks: serde_json::Map<String, Value> = agents::ADAPTERS
        .iter()
        .filter(|a| a.spec().needs_ask_fallback)
        .map(|a| {
            let id = a.spec().id;
            (
                id.to_owned(),
                json!(fallback(&project.operator, &project.settings, id).unwrap_or_default()),
            )
        })
        .collect();
    json!({"agent":host.id,"path":host.path,
        "local_path":host.local_path,"status":state,
        "ask_fallback":fallbacks,"publish":host.publish})
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
    state: &State,
    r: &'a Registration,
    config: &Value,
    unsettled: Option<&Plan>,
    diagnostics: &mut Vec<&'static str>,
    degraded: &mut std::collections::BTreeSet<&'a str>,
) -> (Vec<&'a str>, Value, Value) {
    let mut agents = Vec::new();
    for (agent, installation) in &r.agents {
        let before = diagnostics.len();
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
    let mut host_integration = r.definition.as_ref().map_or(Value::Null, |owned| {
        host_status(root, state, owned, diagnostics)
    });
    let before = diagnostics.len();
    if diagnose_binary(r, &state.operator, diagnostics).is_err() {
        diagnostics.push("registered binary is unreadable");
    }
    if diagnostics.len() != before {
        degraded.extend(agents.iter().copied());
    }
    if let Some(owned) = &r.definition
        && (diagnostics.len() != before || unsettled.is_some_and(|p| p.affects(owned.host().id)))
    {
        host_integration["definition_status"] = host_integration["status"].clone();
        host_integration["status"] = json!("degraded");
    }
    (agents, host_integration, legacy_interceptor)
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
                let prefixes = if state.settings.enabled() {
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
    // Without loadable policy files there is nothing to compare against.
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
        let (agents, mut host_integration, legacy_interceptor) = registered.as_ref().map_or_else(
            || (Vec::new(), Value::Null, Value::Null),
            |r| {
                diagnose_registered(
                    root,
                    state,
                    r,
                    &config,
                    unsettled.as_ref(),
                    &mut diagnostics,
                    &mut degraded,
                )
            },
        );
        // Coverage compares the agents' files with the policy files as they
        // are now, hand edits included, using their plan only when one is
        // already compiled. Status never compiles: without a cached plan the
        // comparison is reported unchecked.
        let inspection = review::files::cache_for(&state.operator)
            .ok()
            .and_then(|cache| review::files::cached_in(root, &cache))
            .map(|evaluator| review::cache::Inspection::of(&evaluator));
        let snapshot = inspection.as_ref();
        diagnose_prefixes(
            root,
            &meta,
            registered.as_ref(),
            snapshot,
            state.settings.enabled(),
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
            if r.definition.is_some()
                && agents::ADAPTERS.iter().any(|adapter| {
                    let agent = adapter.spec().id;
                    adapter.supports_box_hooks()
                        && !r.agents.contains_key(agent)
                        && agent_output_drift(root, state, snapshot, r, agent, &connections)
                })
            {
                if host_integration.get("definition_status").is_none() {
                    host_integration["definition_status"] = host_integration["status"].clone();
                }
                host_integration["status"] = json!("degraded");
            }
        }
        let published = reporting::host_hooks(root, registered.as_ref(), state, &mut diagnostics);
        let mcp_only = reporting::mcp_only(root, registered.as_ref(), &mut diagnostics);
        let healthy = diagnostics.is_empty();
        let mut report = json!({
            "binary_inspection":registered.as_ref().map(|registration| json!({
                "current_location_recorded":std::env::current_exe().ok().and_then(|path|path.canonicalize().ok()).is_some_and(|running|current_executable_receipt(registration,&meta,&running)),
                "payload_integrity":"unchecked","compatibility":"unchecked"
            })),
            "policy_inspection":{"availability":if snapshot.is_some() {"current"}else{"unchecked"},"source_validation":"not-performed","automatic_matching":"checked-on-interception"},
            "host_integration":host_integration,
            "mcp_only_integrations":mcp_only,
            "legacy_interceptor":legacy_interceptor,
            "mcp_connections":connections,
            "registered_integrations":agents.iter().map(|agent|json!({"agent":agent,"surface":"mcp","status":if degraded.contains(agent) {"degraded"} else {"registered"}})).collect::<Vec<_>>(),
            "installed_integrations":registered.as_ref().map_or_default(|r|r.agents.keys().map(|agent| {
                let adapter=agents::find(agent);
                let categories=adapter.map_or_default(crate::agents::AgentAdapter::working_file_categories);
                json!({"agent":agent,"surface":"native-hooks","status":if degraded.contains(agent.as_str()) {"degraded"} else {"installed"},
                    "events":r.agents[agent].groups.keys().collect::<Vec<_>>(),
                    "working_files":{"categories":categories,"session_discovery":"request-time","runtime_assets":if categories.contains(&"runtime_assets") {"supported"} else {"unsupported"},"non_temporary_scratch":if categories.contains(&"non_temporary_scratch") {"supported"} else {"unsupported"}},
                    "native_approval":adapter.is_some_and(|a|a.approval(agents::Event::Before)==crate::enforcement::protocol::NativeApproval::Prompt),
                    "permission_request":adapter.is_some_and(|a|a.spec().events.iter().any(|(event,e)|*e==agents::Event::Permission && r.agents[agent].groups.contains_key(*event))),
                    "ask_fallback":if adapter.is_some_and(|a|a.spec().needs_ask_fallback) { json!(fallback(&meta,&state.settings,agent).unwrap_or_default()) } else { Value::Null }})
            }).collect::<Vec<_>>()),
            "diagnostics":diagnostics,"repair_needed":!healthy,
            "tool_interception":if registered.as_ref().is_some_and(|r|!r.agents.is_empty()) {"native hooks installed; host loading is unverified"} else if !legacy_interceptor.is_null() {"legacy text-read interceptor installed; host loading is unverified"} else if host_integration.is_null() {"not installed"} else if host_integration["agent"] == host_definition::HUB.id {"native hooks defined for Hub to publish; host loading is unverified"} else {"native hooks defined for Box to publish; host loading is unverified"},
            "live_host_verification":"unverified",
            "discovery":"restart host; trust project, review native hooks and approve MCP",
            "operator_protection":"native hooks can be skipped or fail open on host timeout/startup failure; no same-user OS isolation"
        });
        reporting::complete(&mut report, state, published);
        Ok::<_, Error>(report)
    })();
    inspected.unwrap_or_else(|e| json!({"registered_integrations":[],"installed_integrations":[],"diagnostics":[e.to_string()],"repair_needed":true,"tool_interception":"unknown","live_host_verification":"unverified","verification":{"state":"unverified"}}))
}

#[cfg(all(test, unix))]
mod digest_tests {
    use super::*;

    #[test]
    fn a_settled_binary_is_hashed_once_and_a_recent_change_is_never_remembered() {
        let root = assert_fs::TempDir::new().unwrap();
        let logs = assert_fs::TempDir::new().unwrap();
        let cli = <crate::cli::Cli as clap::Parser>::parse_from([
            "shield",
            "--debug-log-dir",
            logs.path().to_str().unwrap(),
            "status",
        ]);
        let log = std::sync::Arc::new(crate::diagnostics::Log::open(&cli));
        // Digest, real hashes and cache hits for one call.
        let digest = |path: &Path, started| {
            let request = log.request("command", "status", None, None).unwrap();
            let digest = {
                let _scope = request.enter();
                binary_digest_started(path, started).unwrap()
            };
            let counts = (
                request.recorded_count("integration.binary_hashes"),
                request.recorded_count("integration.binary_hash_reuses"),
            );
            request.finish("ok");
            (digest, counts)
        };
        let path = root.path().join("shield");
        let now = std::time::SystemTime::now;
        let one = policy::fingerprint(b"acme binary one");
        let two = policy::fingerprint(b"acme binary two");

        // Just written: a same-size rewrite in the same clock tick could keep
        // its identity, so the digest is hashed each time and never remembered.
        fs::write(&path, b"acme binary one").unwrap();
        assert_eq!(digest(&path, now()), (one.clone(), (1, 0)));
        assert_eq!(digest(&path, now()), (one, (1, 0)));
        fs::write(&path, b"acme binary two").unwrap();
        assert_eq!(digest(&path, now()), (two.clone(), (1, 0)));

        // Hashing well after the last change remembers the digest.
        let later = now() + std::time::Duration::from_hours(1);
        assert_eq!(digest(&path, later), (two.clone(), (1, 0)));
        assert_eq!(digest(&path, later), (two, (0, 1)));

        // A replacement through rename changes the inode.
        let other = root.path().join("replacement");
        fs::write(&other, b"acme binary 333").unwrap();
        fs::rename(&other, &path).unwrap();
        assert_eq!(
            digest(&path, later),
            (policy::fingerprint(b"acme binary 333"), (1, 0))
        );
    }
}
