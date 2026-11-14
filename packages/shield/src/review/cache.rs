//! Disposable external plans, authenticated by the durable activation's exact digest.
use super::{
    Context, Error,
    model::State,
    snapshots,
    storage::{self, Store},
};
use crate::{configuration::Evaluator, policy};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};
pub(in crate::review) mod inputs;
mod ownership;
mod preparation;
mod retention;

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Artifact {
    #[serde(skip)]
    _pins: Option<storage::objects::Pins>,
    pub(super) directory: PathBuf,
    project_key: String,
    plan_key: String,
    pub(super) digest: String,
    length: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    inspection: Option<InspectionRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    input_hints: Option<Value>,
}

/// Derived metadata never owns activation validity. Unsupported JSON remains
/// preserved through ordinary authority writes rather than being discarded.
#[derive(Clone, Deserialize, Serialize)]
#[serde(untagged)]
enum InspectionRecord {
    Known(Inspection),
    Opaque(Value),
}

/// Nonauthorizing coverage facts captured while the evaluator is already owned.
/// Oversized descriptions are omitted rather than growing the authority checkpoint.
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(in crate::review) struct Inspection {
    schema_version: u8,
    candidate_digest: String,
    policy_revision: String,
    prefixes: Vec<policy::agent_outputs::CodexPrefix>,
    connections: crate::mcp_connections::Connections,
    automatic_mcp: bool,
}
const INSPECTION_LIMIT: usize = 256 * 1024;

impl Inspection {
    fn capture(source: Source<'_>, evaluator: &Evaluator) -> Option<Self> {
        let descriptor = Self {
            schema_version: 1,
            candidate_digest: storage::sources::map_digest(source.candidates),
            policy_revision: source.policy_revision.into(),
            prefixes: evaluator.codex_prefixes().to_vec(),
            connections: evaluator.effective().mcp_connections().clone(),
            automatic_mcp: evaluator.has_mcp_integrations(),
        };
        crate::json::fits(&descriptor, INSPECTION_LIMIT).then_some(descriptor)
    }
    pub(in crate::review) fn matches(&self, candidate: &str, revision: &str) -> bool {
        self.schema_version == 1
            && self.candidate_digest == candidate
            && self.policy_revision == revision
            && crate::json::fits(self, INSPECTION_LIMIT)
    }
    pub(in crate::review) fn codex_prefixes(&self) -> &[policy::agent_outputs::CodexPrefix] {
        &self.prefixes
    }
    pub(in crate::review) fn mcp_connections(&self) -> &crate::mcp_connections::Connections {
        &self.connections
    }
    pub(in crate::review) fn mcp_connection(
        &self,
        agent: &str,
        alias: &str,
    ) -> Option<&crate::mcp_connections::Connection> {
        self.connections.values().find(|connection| {
            connection.enabled
                && connection
                    .agents
                    .get(agent)
                    .is_some_and(|name| name == alias)
        })
    }
    pub(in crate::review) fn has_mcp_integrations(&self) -> bool {
        self.automatic_mcp
    }
}
/// The operator command that prepares this project's compiled cache.
pub(super) fn repair_route(project_root: &str) -> String {
    crate::agent_message::cache_repair(project_root)
        .unwrap_or_else(|| "shield cache prepare".into())
}

/// The plan format this engine writes and loads.
const PLAN_FORMAT: &str = crate::policy::compiled::FORMAT;

/// The plan identity for captured source and revision under this engine and format.
fn plan_key(candidate_digest: &str, revision: &str) -> String {
    policy::fingerprint(
        json!([
            "shield-plan-cache-v1",
            candidate_digest,
            revision,
            env!("CARGO_PKG_VERSION"),
            PLAN_FORMAT
        ])
        .to_string()
        .as_bytes(),
    )
}

impl Artifact {
    /// The descriptor retained by a handle is additional to its compiled bytes.
    pub(in crate::review) fn inspection_byte_len(&self) -> Result<usize, Error> {
        // Opaque data is bounded by the checkpoint cap and charged in full,
        // even though it cannot supply usable coverage facts.
        let inspection = self
            .inspection
            .as_ref()
            .map_or(Ok(0), crate::json::encoded_len)
            .map_err(|_| Error::Overloaded)?;
        let inputs = self
            .input_hints
            .as_ref()
            .map_or(Ok(0), crate::json::encoded_len)
            .map_err(|_| Error::Overloaded)?;
        inspection.checked_add(inputs).ok_or(Error::Overloaded)
    }
    pub(super) fn inspection_digest(&self) -> Option<String> {
        self.inspection
            .as_ref()
            .map(|metadata| crate::json::fingerprint(metadata).expect("inspection serialization"))
    }
    pub(super) fn inspection(&self) -> Option<&Inspection> {
        match self.inspection.as_ref()? {
            InspectionRecord::Known(metadata) => Some(metadata),
            InspectionRecord::Opaque(_) => None,
        }
    }
    /// Report the artifact without reading it; compatibility compares the stored
    /// plan key with the one this engine derives for the active source and revision.
    pub(super) fn diagnostics(
        &self,
        project_root: &str,
        candidate_digest: &str,
        revision: &str,
    ) -> Value {
        let presence = match fs::symlink_metadata(self.path()) {
            Ok(m) if m.is_file() && !m.file_type().is_symlink() && m.len() == self.length => {
                "present"
            }
            Ok(_) => "invalid",
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => "missing",
            Err(_) => "unavailable",
        };
        let compatibility = if self.plan_key == plan_key(candidate_digest, revision) {
            "current"
        } else {
            "rebuild-required"
        };
        json!({"directory":self.directory,"project_key":self.project_key,"plan_key":self.plan_key,
            "running_format":PLAN_FORMAT,"running_engine":env!("CARGO_PKG_VERSION"),
            "presence":presence,"compatibility":compatibility,
            "integrity":"checked-on-load","encoded_bytes":self.length,"repair_route":repair_route(project_root)})
    }
    pub(super) fn byte_len(&self) -> u64 {
        self.length
    }
    pub(super) fn path(&self) -> PathBuf {
        self.directory
            .join("compiled")
            .join(&self.project_key)
            .join(format!("{}.bin", self.plan_key))
    }
    pub(super) fn load(&self, root: &str, project_id: &str) -> Result<Evaluator, Error> {
        let _trace = crate::diagnostics::trace::span("cache.load");
        if !self.directory.is_absolute()
            || (self.project_key != project_key(root, project_id)
                && self.project_key != legacy_project_key(root, project_id))
            || [&self.project_key, &self.plan_key]
                .iter()
                .any(|s| !digest(s))
            || self
                .digest
                .strip_prefix("blake3-v1:")
                .is_none_or(|value| !digest(value))
            || self.length > policy::compiled::MAX_BYTES as u64
        {
            return Err(Error::Invalid("invalid compiled cache binding"));
        }
        let owner = cache_owner(root)?;
        check_directory(&self.directory, true, owner)?;
        check_directory(&self.directory.join("compiled"), true, owner)?;
        check_directory(
            &self.directory.join("compiled").join(&self.project_key),
            true,
            owner,
        )?;
        let path = self.path();
        #[cfg(windows)]
        crate::windows_security::check_private(&path).map_err(|_| unavailable())?;
        let metadata = fs::symlink_metadata(&path).map_err(|_| unavailable())?;
        if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() != self.length
        {
            return Err(unavailable());
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if metadata.mode() & 0o077 != 0 || owner != Some(metadata.uid()) {
                return Err(unavailable());
            }
        }
        let mut bytes =
            Vec::with_capacity(usize::try_from(self.length + 1).map_err(|_| unavailable())?);
        fs::File::open(path)
            .and_then(|file| file.take(self.length + 1).read_to_end(&mut bytes))
            .map_err(|_| unavailable())?;
        if bytes.len() as u64 != self.length || compiled_digest(&bytes) != self.digest {
            return Err(unavailable());
        }
        let evaluator = Evaluator::from_authenticated_compiled(bytes).map_err(|_| unavailable())?;
        Ok(evaluator)
    }
}
fn compiled_digest(bytes: &[u8]) -> String {
    format!("blake3-v1:{}", blake3::hash(bytes).to_hex())
}
fn unavailable() -> Error {
    Error::CacheUnavailable
}
fn digest(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}
/// The cache namespace of a project, by its ID alone: a moved, renamed or
/// copied folder keeps its cache. Plans inside are keyed by content and
/// verified by digest on load, so copies sharing a namespace cannot confuse
/// each other. `_root` is accepted for the earlier key below.
fn project_key(_root: &str, project_id: &str) -> String {
    policy::fingerprint(
        json!(["shield-project-cache-v2", project_id])
            .to_string()
            .as_bytes(),
    )
}

/// The earlier namespace, which also bound the folder's absolute root. An
/// artifact recorded under it still loads in place.
fn legacy_project_key(root: &str, project_id: &str) -> String {
    policy::fingerprint(
        json!(["shield-project-cache-v1", root, project_id])
            .to_string()
            .as_bytes(),
    )
}

#[cfg_attr(
    not(unix),
    expect(
        clippy::unnecessary_wraps,
        reason = "Unix ownership queries are fallible; cache callers use the same interface on every platform"
    )
)]
fn cache_owner(root: &str) -> Result<Option<u32>, Error> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Ok(Some(
            fs::metadata(Path::new(root).join(".shield/review"))?.uid(),
        ))
    }
    #[cfg(not(unix))]
    {
        let _ = root;
        Ok(None)
    }
}

pub(super) fn default_directory() -> Result<PathBuf, Error> {
    #[cfg(test)]
    return Ok(std::env::temp_dir()
        .canonicalize()?
        .join(format!("shield-unit-cache-{}", std::process::id())));
    #[cfg(not(test))]
    {
        #[cfg(target_os = "windows")]
        return std::env::var_os("LOCALAPPDATA")
            .map(|home| PathBuf::from(home).join("Shield/Cache"))
            .filter(|p| p.is_absolute())
            .ok_or(Error::Invalid("Local AppData unavailable; use --cache-dir"));
        #[cfg(not(target_os = "windows"))]
        {
            let home = std::env::var_os("HOME")
                .map(PathBuf::from)
                .filter(|p| p.is_absolute())
                .ok_or(Error::Invalid(
                    "home directory unavailable; use --cache-dir",
                ))?;
            #[cfg(target_os = "macos")]
            return Ok(home.join("Library/Caches/Shield"));
            #[cfg(not(target_os = "macos"))]
            return Ok(std::env::var_os("XDG_CACHE_HOME")
                .map(PathBuf::from)
                .filter(|p| p.is_absolute())
                .unwrap_or_else(|| home.join(".cache"))
                .join("shield"));
        }
    }
}

pub(super) fn build(state: &State, directory: Option<&Path>) -> Result<Artifact, Error> {
    build_source(
        Source {
            project_root: &state.project_root,
            project_id: &state.project_id,
            candidates: &state.candidates,
            policy_revision: &state.policy_revision,
            operator: &state.operator,
        },
        directory,
        None,
    )
}
pub(super) fn build_candidate(
    context: &Context,
    candidates: &std::collections::BTreeMap<String, Value>,
    revision: &str,
    operator: &Value,
    evaluator: &Evaluator,
) -> Result<Artifact, Error> {
    build_source(
        Source {
            project_root: &context.project_root,
            project_id: &context.project_id,
            candidates,
            policy_revision: revision,
            operator,
        },
        None,
        Some(evaluator),
    )
}

/// Bind validated compiled bytes without accessing their disposable destination.
pub(super) fn describe_candidate(
    context: &Context,
    candidates: &std::collections::BTreeMap<String, Value>,
    revision: &str,
    operator: &Value,
    evaluator: &Evaluator,
) -> Result<Artifact, Error> {
    let directory = selected_directory(operator, None)?;
    let directory = if directory.is_absolute() {
        directory
    } else {
        std::env::current_dir()?.join(directory)
    };
    if !operator["compiled_cache_root"].is_string() && directory.starts_with(&context.project_root)
    {
        return Err(Error::Invalid(
            "compiled cache must be an absolute directory outside the project",
        ));
    }
    describe(
        Source {
            project_root: &context.project_root,
            project_id: &context.project_id,
            candidates,
            policy_revision: revision,
            operator,
        },
        directory,
        evaluator,
    )
}

fn selected_directory(operator: &Value, directory: Option<&Path>) -> Result<PathBuf, Error> {
    directory
        .map(Path::to_path_buf)
        .or_else(|| operator["compiled_cache_root"].as_str().map(PathBuf::from))
        .map_or_else(default_directory, Ok)
}

fn describe(
    state: Source<'_>,
    directory: PathBuf,
    evaluator: &Evaluator,
) -> Result<Artifact, Error> {
    if evaluator.revision() != state.policy_revision {
        return Err(Error::StaleSnapshot);
    }
    let bytes = evaluator.compiled_bytes();
    Ok(Artifact {
        _pins: None,
        directory,
        project_key: project_key(state.project_root, state.project_id),
        plan_key: plan_key(
            &super::storage::sources::map_digest(state.candidates),
            state.policy_revision,
        ),
        digest: compiled_digest(bytes),
        length: bytes.len() as u64,
        inspection: Inspection::capture(state, evaluator).map(InspectionRecord::Known),
        input_hints: None,
    })
}

#[derive(Clone, Copy)]
struct Source<'a> {
    project_root: &'a str,
    project_id: &'a str,
    candidates: &'a std::collections::BTreeMap<String, Value>,
    policy_revision: &'a str,
    operator: &'a Value,
}
fn build_source(
    state: Source<'_>,
    directory: Option<&Path>,
    prepared: Option<&Evaluator>,
) -> Result<Artifact, Error> {
    let _trace = crate::diagnostics::trace::span("cache.build");
    let explicit = directory.is_some() || state.operator["compiled_cache_root"].is_string();
    let directory = selected_directory(state.operator, directory)?;
    let directory = normalize(&directory)?;
    if !explicit && directory.starts_with(state.project_root) {
        return Err(Error::Invalid(
            "compiled cache must be an absolute directory outside the project",
        ));
    }
    let owner = cache_owner(state.project_root)?;
    prepare_directory(&directory, owner)?;
    if directory.starts_with(state.project_root) {
        atomic_write(&directory.join(".gitignore"), b"*\n!.gitignore\n")?;
    }
    let candidate_digest = super::storage::sources::map_digest(state.candidates);
    let built_evaluator;
    let evaluator = if let Some(evaluator) = prepared {
        evaluator
    } else {
        built_evaluator =
            Evaluator::new_for_revision(snapshots(state.candidates)?, state.policy_revision)?;
        &built_evaluator
    };
    let artifact = describe(state, directory, evaluator)?;
    let bytes = evaluator.compiled_bytes();
    let partition = artifact
        .directory
        .join("compiled")
        .join(&artifact.project_key);
    prepare_directory(&artifact.directory.join("compiled"), owner)?;
    prepare_directory(&partition, owner)?;
    let manifest = json!({"schema_version":1,"kind":"compiled-inputs","project_key":artifact.project_key,
        "plan_key":artifact.plan_key,"policy_revision":state.policy_revision,
        "candidate_digest":candidate_digest,
        "compiled_digest":artifact.digest,"engine":env!("CARGO_PKG_VERSION"),"format":PLAN_FORMAT});
    let manifest = manifest.to_string();
    let pins = retention::publish(
        state.project_root,
        &artifact,
        bytes,
        manifest.as_bytes(),
        prepared.is_none(),
    )?;
    if ownership::register(state, &artifact).is_err() {
        crate::diagnostics::trace::count("cache.ownership_untracked", 1);
    }
    if ownership::maintain(
        &artifact.directory,
        &artifact.project_key,
        owner,
        state.project_root,
    )
    .is_err()
    {
        crate::diagnostics::trace::count("cache.orphan_cleanup_incomplete", 1);
    }
    Ok(Artifact {
        _pins: Some(pins),
        ..artifact
    })
}

fn normalize(directory: &Path) -> Result<PathBuf, Error> {
    let absolute = if directory.is_absolute() {
        directory.to_path_buf()
    } else {
        std::env::current_dir()?.join(directory)
    };
    if fs::symlink_metadata(&absolute).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err(unavailable());
    }
    let mut ancestor = absolute.as_path();
    let mut suffix = Vec::new();
    while !ancestor.exists() {
        suffix.push(
            ancestor
                .file_name()
                .ok_or(Error::Invalid("cache directory name missing"))?,
        );
        ancestor = ancestor
            .parent()
            .ok_or(Error::Invalid("cache parent missing"))?;
    }
    let mut resolved = ancestor.canonicalize()?;
    for part in suffix.into_iter().rev() {
        resolved.push(part);
    }
    Ok(resolved)
}

fn prepare_directory(directory: &Path, owner: Option<u32>) -> Result<(), Error> {
    if directory.exists() {
        return check_directory(directory, true, owner);
    }
    let parent = directory
        .parent()
        .ok_or(Error::Invalid("cache directory has no parent"))?;
    if parent.exists() {
        check_directory(parent, false, None)?;
    } else {
        fs::create_dir_all(parent)?;
        check_directory(parent, false, None)?;
    }
    let builder = fs::DirBuilder::new();
    #[cfg(unix)]
    let builder = {
        use std::os::unix::fs::DirBuilderExt;
        let mut builder = builder;
        builder.mode(0o700);
        builder
    };
    match builder.create(directory) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e.into()),
    }
    #[cfg(windows)]
    crate::windows_security::make_private(directory)?;
    check_directory(directory, true, owner)
}
fn check_directory(directory: &Path, private: bool, owner: Option<u32>) -> Result<(), Error> {
    let metadata = fs::symlink_metadata(directory).map_err(|_| unavailable())?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(unavailable());
    }
    #[cfg(windows)]
    if private {
        crate::windows_security::check_private(directory).map_err(|_| unavailable())?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        if private
            && (metadata.permissions().mode() & 0o077 != 0
                || metadata.permissions().mode() & 0o100 == 0
                || owner != Some(metadata.uid()))
        {
            return Err(unavailable());
        }
    }
    #[cfg(not(unix))]
    let _ = (private, owner);
    Ok(())
}
/// The writer PID of a `<digest>[.inputs|.probe].<pid>-<sequence>-<nanoseconds>.tmp`
/// partition temporary.
fn temporary_writer(name: &str) -> Option<u32> {
    let (plan, stamp) = name.strip_suffix(".tmp")?.rsplit_once('.')?;
    let plan = plan
        .strip_suffix(".inputs")
        .or_else(|| plan.strip_suffix(".probe"))
        .unwrap_or(plan);
    if !digest(plan) {
        return None;
    }
    storage::stamp_pid(stamp, 3)
}
fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), Error> {
    static SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let temporary = path.with_extension(format!(
        "{}-{}-{}.tmp",
        std::process::id(),
        SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    let result = (|| -> Result<(), Error> {
        if fs::symlink_metadata(path).is_ok_and(|m| !m.is_file() || m.file_type().is_symlink()) {
            return Err(unavailable());
        }
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary)?;
        file.write_all(bytes)?;
        storage::sync(&file)?;
        drop(file);
        super::storage::replace(&temporary, path)?;
        #[cfg(unix)]
        storage::sync(&fs::File::open(
            path.parent()
                .ok_or(Error::Invalid("cache parent missing"))?,
        )?)?;
        Ok(())
    })();
    let _ = fs::remove_file(temporary);
    result
}

pub(super) fn prepare(context: &Context, directory: Option<&Path>) -> Result<Value, Error> {
    if context.surface != "cli" {
        return Err(Error::ApprovalRequired);
    }
    rebuild(context, directory, &|| Ok(()), "operator-cli")
}

/// Rebuild the enrolled project's committed cache in its already approved
/// location. This restores disposable data without changing policy authority.
pub(super) fn prepare_committed(
    context: &Context,
    checkpoint: super::Checkpoint<'_>,
) -> Result<Value, Error> {
    rebuild(context, None, checkpoint, "derived-cache")
}

/// An in-session reviewed location change can recover an unusable cache root.
/// Source and activation still come exclusively from the committed checkpoint.
pub(super) fn relocate_committed(
    context: &Context,
    directory: &Path,
    checkpoint: super::Checkpoint<'_>,
) -> Result<Value, Error> {
    rebuild(context, Some(directory), checkpoint, "derived-cache")
}

fn reusable_current_plan(active: &super::runtime::Active, context: &Context) -> Option<Evaluator> {
    if active.compiled.plan_key
        != plan_key(
            &storage::sources::map_digest(&active.candidates),
            active.revision(),
        )
    {
        return None;
    }
    active
        .compiled
        .load(&context.project_root, &context.project_id)
        .ok()
        .filter(|evaluator| evaluator.revision() == active.revision())
}

fn rebuild(
    context: &Context,
    directory: Option<&Path>,
    checkpoint: super::Checkpoint<'_>,
    mechanism: &str,
) -> Result<Value, Error> {
    checkpoint()?;
    let store = Store::open(Path::new(&context.project_root))?;
    let initial = store.read_checkpoint()?;
    initial.check(context)?;
    let expected = initial
        .active
        .as_ref()
        .ok_or(Error::Invalid("no committed policy to prepare"))?
        .activation_id()
        .to_owned();
    drop(initial);
    let preparation = preparation::Lease::acquire_cancellable(&context.project_root, checkpoint)?;
    checkpoint()?;
    let (mut state, _source_pins) = store.read_pinned_checkpoint()?;
    state.check(context)?;
    let active = state
        .active
        .as_ref()
        .ok_or(Error::Invalid("no committed policy to prepare"))?;
    let activation = active.activation_id().to_owned();
    if activation != expected {
        return Err(Error::StaleSnapshot);
    }
    let cached = reusable_current_plan(active, context);
    // Direct edits and pending review candidates cannot enter an active cache.
    state.candidates = std::collections::BTreeMap::from([(
        "project".into(),
        if cached.is_some() {
            active.project_candidate()?.clone()
        } else {
            store.captured_candidate(active.project_candidate()?)?
        },
    )]);
    state.policy_revision = active.revision().into();
    // Recovery must keep the active cache location, even if projections changed.
    let explicit_directory = directory.is_some();
    let directory = directory.unwrap_or(&active.compiled.directory);
    checkpoint()?;
    let artifact = (if let Some(evaluator) = &cached {
        build_source(
            Source {
                project_root: &state.project_root,
                project_id: &state.project_id,
                candidates: &state.candidates,
                policy_revision: &state.policy_revision,
                operator: &state.operator,
            },
            Some(directory),
            Some(evaluator),
        )
    } else {
        build(&state, Some(directory))
    })
    .map_err(|error| match error {
        // Only this build touches the disposable cache. Authoritative source
        // reads above and checkpoint publication below keep their own errors.
        Error::Io(_) => Error::CacheUnavailable,
        other => other,
    })?;
    preparation.check()?;
    store.update_checkpoint(context, checkpoint, |current| {
        preparation.check()?;
        let active = current.active.as_mut().ok_or(Error::StaleSnapshot)?;
        if active.activation_id() != activation {
            return Err(Error::StaleSnapshot);
        }
        let location_changed = explicit_directory && active.compiled.directory != artifact.directory;
        active.compiled = artifact.clone();
        // An absent root selects the default location. Recovery into the active
        // location records nothing, so pending reviews keep their base metadata.
        if explicit_directory || current.operator["compiled_cache_root"].is_string() {
            current.operator["compiled_cache_root"] = json!(artifact.directory);
        }
        if !current.authorization_events.iter().any(|event| {
            event["links"]["activation_id"] == activation
                && event["details"]["compiled_digest"] == artifact.digest
                && event["details"]["inspection_digest"] == json!(artifact.inspection_digest())
                && (event["operator"]["mechanism"] == "operator-cli"
                    || (event["kind"] == "compiled_cache_prepared"
                        && event["operator"]["mechanism"] == "derived-cache"))
        }) {
            let mut event = current.event(
                "compiled_cache_prepared",
                context,
                json!({"activation_id":activation}),
                json!({}),
                json!({"compiled_digest":artifact.digest,"inspection_digest":artifact.inspection_digest()}),
            );
            event["operator"] = json!({"mechanism":mechanism});
            current.record(event);
        }
        if location_changed { super::configuration_changes::changed(current, context, "compiled cache location"); }
        Ok(
            json!({"schema_version":1,"prepared":true,"policy_revision":state.policy_revision,
            "cache_directory":artifact.directory,"compiled_digest":artifact.digest}),
        )
    })
}

#[cfg(test)]
mod tests;
