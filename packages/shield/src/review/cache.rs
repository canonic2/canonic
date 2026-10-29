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

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Artifact {
    pub(super) directory: PathBuf,
    project_key: String,
    plan_key: String,
    pub(super) digest: String,
    length: u64,
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
    fn path(&self) -> PathBuf {
        self.directory
            .join("compiled")
            .join(&self.project_key)
            .join(format!("{}.bin", self.plan_key))
    }
    pub(super) fn load(&self, root: &str, project_id: &str) -> Result<Evaluator, Error> {
        if !self.directory.is_absolute()
            || self.project_key != project_key(root, project_id)
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
fn project_key(root: &str, project_id: &str) -> String {
    policy::fingerprint(
        json!(["shield-project-cache-v1", root, project_id])
            .to_string()
            .as_bytes(),
    )
}

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
    )
}
pub(super) fn build_candidate(
    context: &Context,
    candidates: &std::collections::BTreeMap<String, Value>,
    revision: &str,
    operator: &Value,
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
    )
}
#[derive(Clone, Copy)]
struct Source<'a> {
    project_root: &'a str,
    project_id: &'a str,
    candidates: &'a std::collections::BTreeMap<String, Value>,
    policy_revision: &'a str,
    operator: &'a Value,
}
fn build_source(state: Source<'_>, directory: Option<&Path>) -> Result<Artifact, Error> {
    let explicit = directory.is_some() || state.operator["compiled_cache_root"].is_string();
    let directory = directory
        .map(Path::to_path_buf)
        .or_else(|| {
            state.operator["compiled_cache_root"]
                .as_str()
                .map(PathBuf::from)
        })
        .map_or_else(default_directory, Ok)?;
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
    let project_key = project_key(state.project_root, state.project_id);
    let candidate_digest = super::storage::sources::map_digest(state.candidates);
    let plan_key = plan_key(&candidate_digest, state.policy_revision);
    let evaluator = Evaluator::new(snapshots(state.candidates)?)?;
    if evaluator.revision() != state.policy_revision {
        return Err(Error::StaleSnapshot);
    }
    let bytes = evaluator.compiled_bytes();
    let artifact = Artifact {
        directory,
        project_key,
        plan_key,
        digest: compiled_digest(bytes),
        length: bytes.len() as u64,
    };
    let partition = artifact
        .directory
        .join("compiled")
        .join(&artifact.project_key);
    prepare_directory(&artifact.directory.join("compiled"), owner)?;
    prepare_directory(&partition, owner)?;
    // Preparers share the partition without a lock, so only a temporary whose
    // writer has exited is removed.
    storage::remove_dead_temporaries(&partition, temporary_writer);
    atomic_write(&artifact.path(), bytes)?;
    let manifest = json!({"schema_version":1,"kind":"compiled-inputs","project_key":artifact.project_key,
        "plan_key":artifact.plan_key,"policy_revision":state.policy_revision,
        "candidate_digest":candidate_digest,
        "compiled_digest":artifact.digest,"engine":env!("CARGO_PKG_VERSION"),"format":PLAN_FORMAT});
    atomic_write(
        &partition.join(format!("{}.inputs.json", artifact.plan_key)),
        manifest.to_string().as_bytes(),
    )?;
    Ok(artifact)
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
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    match builder.create(directory) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e.into()),
    }
    check_directory(directory, true, owner)
}
fn check_directory(directory: &Path, private: bool, owner: Option<u32>) -> Result<(), Error> {
    let metadata = fs::symlink_metadata(directory).map_err(|_| unavailable())?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(unavailable());
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
/// The writer PID of a `<plan>[.inputs].<pid>-<sequence>-<nanoseconds>.tmp`
/// partition temporary.
fn temporary_writer(name: &str) -> Option<u32> {
    let (plan, stamp) = name.strip_suffix(".tmp")?.rsplit_once('.')?;
    let plan = plan.strip_suffix(".inputs").unwrap_or(plan);
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
        fs::rename(&temporary, path)?;
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

/// Reconstruct disposable data from the committed source, never editable projections.
pub(super) fn recover(context: &Context, checkpoint: super::Checkpoint<'_>) -> Result<(), Error> {
    rebuild(context, None, checkpoint, "derived-cache").map(|_| ())
}

fn rebuild(
    context: &Context,
    directory: Option<&Path>,
    checkpoint: super::Checkpoint<'_>,
    mechanism: &str,
) -> Result<Value, Error> {
    checkpoint()?;
    let store = Store::open(Path::new(&context.project_root))?;
    let mut state = store.read_checkpoint()?;
    state.check(context)?;
    let active = state
        .active
        .as_ref()
        .ok_or(Error::Invalid("no committed policy to prepare"))?;
    let activation = active.activation_id().to_owned();
    // Direct edits and pending review candidates cannot enter an active cache.
    state.candidates = std::collections::BTreeMap::from([(
        "project".into(),
        store.captured_candidate(active.project_candidate()?)?,
    )]);
    state.policy_revision = active.revision().into();
    // Recovery must keep the active cache location, even if projections changed.
    let directory = directory.unwrap_or(&active.compiled.directory);
    checkpoint()?;
    let artifact = build(&state, Some(directory))?;
    store.update_checkpoint(context, checkpoint, |current| {
        let active = current.active.as_mut().ok_or(Error::StaleSnapshot)?;
        if active.activation_id() != activation {
            return Err(Error::StaleSnapshot);
        }
        active.compiled = artifact.clone();
        current.operator["compiled_cache_root"] = json!(artifact.directory);
        if !current.authorization_events.iter().any(|event| {
            event["links"]["activation_id"] == activation
                && event["details"]["compiled_digest"] == artifact.digest
                && (event["operator"]["mechanism"] == "operator-cli"
                    || (event["kind"] == "compiled_cache_prepared"
                        && event["operator"]["mechanism"] == "derived-cache"))
        }) {
            let mut event = current.event(
                "compiled_cache_prepared",
                context,
                json!({"activation_id":activation}),
                json!({}),
                json!({"compiled_digest":artifact.digest}),
            );
            event["operator"] = json!({"mechanism":mechanism});
            current.record(event);
        }
        Ok(
            json!({"schema_version":1,"prepared":true,"policy_revision":state.policy_revision,
            "cache_directory":artifact.directory,"compiled_digest":artifact.digest}),
        )
    })
}

#[cfg(test)]
mod tests;
