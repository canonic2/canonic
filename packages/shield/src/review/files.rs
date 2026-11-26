//! Owns reading the project's policy files, which are its enforced policy, and
//! loading their compiled plan from a per-user cache keyed by their content.
use super::{Error, integrations::project_sources};
#[cfg(test)]
use crate::configuration::Bundle;
use crate::{application, configuration::Evaluator, policy};
use std::{
    collections::{HashMap, HashSet},
    fs,
    io::Write,
    path::{Path, PathBuf},
};

/// The project's policy as its files say now.
pub(in crate::review) struct Policy {
    /// The digest of the files' bytes, which keys their compiled plan.
    pub(in crate::review) digest: String,
    pub(in crate::review) evaluator: Evaluator,
}

/// Read the policy files under `root` and load their plan. Only a cache miss
/// validates the files and compiles them; a hit reads and hashes bytes.
pub(in crate::review) fn load(root: &Path) -> Result<Policy, Error> {
    load_in(root, cache_of(root).ok().as_deref())
}

/// [`load`] with the project's cache folder already known; `None` compiles
/// without a cache.
pub(in crate::review) fn load_in(root: &Path, cache: Option<&Path>) -> Result<Policy, Error> {
    let _trace = crate::diagnostics::trace::span("policy.files");
    let invalid = |cause| diagnose(root, error(cause));
    let sources =
        project_sources::read_retaining(&crate::layout::config_file(root)).map_err(invalid)?;
    let digest = key(&sources);
    // Without a cache folder every load compiles; the files still decide.
    let path = cache.map(|cache| plan_path(cache, &digest));
    let evaluator =
        if let Some(evaluator) = path.as_deref().and_then(|path| read_plan(path, &digest)) {
            crate::diagnostics::trace::count("policy.plan_hits", 1);
            evaluator
        } else {
            crate::diagnostics::trace::count("policy.plan_misses", 1);
            let (_, snapshot) = sources.validated().map_err(invalid)?;
            let evaluator = Evaluator::new(vec![(policy::Scope::Project, snapshot)])?;
            // The cache is disposable: a failed write only costs the next compile.
            if let Some(path) = &path {
                let _ = write_plan(path, &digest, evaluator.compiled_bytes());
            }
            evaluator
        };
    Ok(Policy { digest, evaluator })
}

/// The digest of the policy files under `root` after confirming they are valid,
/// without compiling them: a plan cached under their exact key was compiled
/// from them; otherwise their sources are validated and composed.
pub(in crate::review) fn check(root: &Path, cache: Option<&Path>) -> Result<String, Error> {
    let invalid = |cause| diagnose(root, error(cause));
    let sources = project_sources::read(&crate::layout::config_file(root)).map_err(invalid)?;
    let digest = key(&sources);
    if cache.is_some_and(|cache| plan_recorded(&plan_path(cache, &digest), &digest)) {
        return Ok(digest);
    }
    let (_, snapshot) = sources.validated().map_err(invalid)?;
    Evaluator::revision_of(&[(policy::Scope::Project, snapshot)])?;
    Ok(digest)
}

/// Whether a plan file starts with this key, without reading the plan.
fn plan_recorded(path: &Path, key: &str) -> bool {
    use std::io::Read;
    let mut recorded = vec![0; key.len()];
    fs::File::open(path)
        .and_then(|mut file| file.read_exact(&mut recorded))
        .is_ok_and(|()| recorded == key.as_bytes())
}

/// The digest of the policy files' exact bytes under `root`, settings
/// included, without validating or compiling them.
pub(in crate::review) fn digest(root: &Path) -> Result<String, Error> {
    let sources = project_sources::read(&crate::layout::config_file(root)).map_err(error)?;
    Ok(bytes_key(&sources))
}

/// Compare the sources to the captured plan key without loading or compiling a plan.
pub(in crate::review) fn plan_key(root: &Path) -> Result<String, Error> {
    let sources = project_sources::read(&crate::layout::config_file(root)).map_err(error)?;
    Ok(key(&sources))
}

/// Make the review state's copy of the policy follow the policy files, so CLI
/// and MCP workflows read, compare and publish against the files as they are.
/// Runs under the writer lock, or in memory for readers. Returns whether the
/// state changed. Files that cannot be followed are an error, never a silent
/// fall back to the older copy: hooks would decide by the files while CLI and
/// MCP showed something else.
pub(in crate::review) fn sync(
    context: &super::Context,
    state: &mut super::model::State,
) -> Result<bool, Error> {
    let root = Path::new(&context.project_root);
    let invalid = |error: Error| match diagnose(root, error) {
        conflict @ Error::PolicyFileConflict(_) => conflict,
        error => Error::PolicyFiles(error.to_string()),
    };
    // A selected file that cannot be read refuses; only a project with no
    // configuration in either default location has nothing to follow.
    let selection = crate::layout::select(root).map_err(|error| invalid(error.into()))?;
    if selection.source == crate::layout::Source::Absent {
        return Ok(false);
    }
    let sources = project_sources::read(&crate::layout::config_file(root))
        .map_err(|cause| invalid(error(cause)))?;
    let digest = bytes_key(&sources);
    if state.followed_files.as_deref() == Some(digest.as_str())
        && state.candidates.contains_key("project")
    {
        return Ok(false);
    }
    // A fresh read of the same files and metadata captures what the last one
    // did; a process keeps that one capture instead of repeating it.
    let key = (!state.candidates.contains_key("project"))
        .then(|| crate::json::fingerprint(&state.operator).ok())
        .flatten()
        .map(|operator| (root.to_path_buf(), digest.clone(), operator));
    let remembered = key.as_ref().and_then(|key| {
        let last = LAST_CAPTURE
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        last.as_ref()
            .filter(|(last, _)| last == key)
            .map(|(_, capture)| capture.clone())
    });
    let Captured {
        candidate,
        metadata,
        revision,
        hashes,
    } = if let Some(capture) = remembered {
        drop(sources);
        capture
    } else {
        let capture = follow(context, state, sources).map_err(invalid)?;
        if let Some(key) = key {
            *LAST_CAPTURE
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some((key, capture.clone()));
        }
        capture
    };
    let candidates = std::collections::BTreeMap::from([("project".into(), candidate)]);
    // The copy follows every change to the files' bytes, including edits that
    // keep the policy the same, such as a comment or a setting: a later write
    // renders the files from it and must not drop them. The checkpoint keeps
    // only operator metadata, which follows a change to what it records, so
    // settings and formatting edits leave it, and the reviews bound to it, as
    // they are. Only that change is reported: the rest lives in memory.
    // The capture records the files' packs; no packs is the same as none
    // recorded.
    let packs = |metadata: &serde_json::Value, key: &str| match &metadata[key] {
        serde_json::Value::Null => serde_json::json!([]),
        rows => rows.clone(),
    };
    let changed = ["installed", "disabled", "shared_installed"]
        .iter()
        .any(|key| packs(&metadata, key) != packs(&state.operator, key));
    if changed {
        state.operator = metadata;
        state.operator["projection_file_hashes"] = hashes.into();
    }
    state.candidates = candidates;
    state.policy_revision = revision;
    state.followed_files = Some(digest);
    Ok(changed)
}

/// What following one version of the files gives review state.
#[derive(Clone)]
struct Captured {
    candidate: serde_json::Value,
    metadata: serde_json::Value,
    revision: String,
    hashes: serde_json::Map<String, serde_json::Value>,
}

/// The last capture: its folder, files digest and the metadata it started from.
type CaptureKey = (PathBuf, String, String);
static LAST_CAPTURE: std::sync::Mutex<Option<(CaptureKey, Captured)>> = std::sync::Mutex::new(None);

/// Capture the policy files under `root` into review state's terms. A
/// checkpoint holds no copy of the policy: a load starts from the files' own
/// capture, and the metadata capture then reconciles pack records.
fn follow(
    context: &super::Context,
    state: &mut super::model::State,
    sources: project_sources::Sources,
) -> Result<Captured, Error> {
    crate::diagnostics::trace::count("policy.captures", 1);
    if !state.candidates.contains_key("project") {
        let fresh = sources.bundle().map_err(error)?;
        state
            .candidates
            .insert("project".into(), serde_json::json!(fresh));
    }
    drop(sources);
    let capture = super::changes::project_files::capture(context, state)?;
    let revision = Evaluator::revision_of(&[(policy::Scope::Project, capture.snapshot)])?;
    let hashes = capture
        .paths
        .into_iter()
        .filter_map(|(name, hash)| hash.map(|hash| (name, serde_json::Value::String(hash))))
        .collect();
    Ok(Captured {
        candidate: capture.candidate,
        metadata: capture.metadata,
        revision,
        hashes,
    })
}

/// Releases a fetch stored, and those it could not, each with its reason.
pub(in crate::review) struct Fetched {
    pub(in crate::review) releases: Vec<String>,
    pub(in crate::review) failures: Vec<(String, String)>,
}

/// Fetch the pack releases the lock files name but this machine lacks, each
/// from its registry and checked against the lock's digest, into the per-user
/// artifact store. Each release is fetched independently: one failure does not
/// stop the others. CLI and MCP call this; hooks never use the network.
pub(in crate::review) fn fetch_missing(
    root: &Path,
    settings: &crate::configuration::settings::Values,
    client: &canonic_registry_kit::acquisition::Fetcher,
    checkpoint: super::Checkpoint<'_>,
) -> Result<Fetched, Error> {
    let shield = crate::layout::bundle(root);
    let store = artifact_store()?;
    let mut missing: Vec<Vec<serde_json::Value>> = Vec::new();
    let mut positions: HashMap<String, usize> = HashMap::new();
    for lock in ["shield.lock", "shield.local.lock"] {
        let path = shield.join(lock);
        let path = match fs::symlink_metadata(&path) {
            Ok(_) => project_sources::regular_file(&path).map_err(error)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.into()),
        };
        let text = application::read(&path).map_err(error)?;
        let mut locked: serde_json::Value = serde_json::from_str(&text)
            .map_err(|_| Error::Invalid("project lock file is not valid JSON"))?;
        let Some(serde_json::Value::Array(rows)) =
            locked.get_mut("packs").map(serde_json::Value::take)
        else {
            continue;
        };
        for row in rows {
            let Some(digest) = row["digest"]
                .as_str()
                .filter(|d| d.len() == 64 && d.bytes().all(|b| b.is_ascii_hexdigit()))
            else {
                continue;
            };
            if let Some(&index) = positions.get(digest) {
                missing[index].push(row);
            } else {
                let name = format!("{digest}.yml");
                if project_sources::stored_artifact(&shield, &name).is_none()
                    && !shield.join("cache").join(&name).exists()
                {
                    positions.insert(digest.to_owned(), missing.len());
                    missing.push(vec![row]);
                }
            }
        }
    }
    let mut outcome = Fetched {
        releases: Vec::new(),
        failures: Vec::new(),
    };
    if missing.is_empty() {
        return Ok(outcome);
    }
    let sources = super::registries::effective_from(settings)?;
    for rows in missing {
        let mut attempted = HashSet::new();
        let mut failure = None;
        for row in &rows {
            let attempt = (
                row["source_identity"].as_str(),
                row["id"].as_str(),
                row["version"].as_str(),
            );
            if !attempted.insert(attempt) {
                continue;
            }
            checkpoint()?;
            let release = format!("{}@{}", attempt.1.unwrap_or("?"), attempt.2.unwrap_or("?"));
            match fetch_release(row, &sources, client, &store, checkpoint) {
                Ok(()) => {
                    outcome.releases.push(release);
                    failure = None;
                    break;
                }
                Err(error) => failure = Some((release, error.to_string())),
            }
        }
        outcome.failures.extend(failure);
    }
    Ok(outcome)
}

fn fetch_release(
    row: &serde_json::Value,
    sources: &canonic_registry_kit::registrations::Effective,
    fetcher: &canonic_registry_kit::acquisition::Fetcher,
    store: &Path,
    checkpoint: super::Checkpoint<'_>,
) -> Result<(), Error> {
    let source = sources
        .iter()
        .find(|(_, source, _)| row["source_identity"] == source.identity())
        .map(|(_, source, _)| source.clone())
        .ok_or(Error::Invalid(
            "its registry is not configured; add it with shield registry add",
        ))?;
    let id: canonic_registry_kit::names::PackId = row["id"]
        .as_str()
        .and_then(|id| id.parse().ok())
        .ok_or(Error::Invalid("its pack identity is invalid"))?;
    let mut catalog = super::registries::catalog(fetcher, &source)?;
    super::registries::history(fetcher, &source, &mut catalog, &id)?;
    let entry = catalog
        .entries()
        .iter()
        .find(|entry| entry.id == id && row["version"] == entry.version.to_string())
        .ok_or(Error::Invalid("the registry no longer offers this release"))?;
    let text = super::registries::artifact(fetcher, &source, entry)?;
    let digest = crate::policy::fingerprint(text.as_bytes());
    if row["digest"] != digest.as_str() {
        return Err(Error::Invalid(
            "the registry's bytes differ from the lock's digest",
        ));
    }
    checkpoint()?;
    write_private(&store.join(format!("{digest}.yml")), text.as_bytes())
}

/// The compiled plan of the policy files under `root` if one is cached in
/// `cache`; never compiles.
pub(in crate::review) fn cached_in(root: &Path, cache: &Path) -> Option<Evaluator> {
    let sources = project_sources::read(&crate::layout::config_file(root)).ok()?;
    let digest = key(&sources);
    read_plan(&plan_path(cache, &digest), &digest)
}

/// Capture and validate the policy files under `root`.
#[cfg(test)]
pub(in crate::review) fn capture(root: &Path) -> Result<Bundle, Error> {
    project_sources::capture(&crate::layout::config_file(root)).map_err(error)
}

fn error(error: application::Error) -> Error {
    match error {
        application::Error::Read(error) => Error::Io(error),
        application::Error::Policy(error) => Error::Policy(error),
        application::Error::Encoding | application::Error::Context => {
            Error::Invalid("policy files are unreadable")
        }
    }
}

/// Inspect the policy files only after a load fails. Gathering all
/// marker blocks in one report still works when config cannot name its rules
/// or a conflicted lock cannot name the packs that would be fetched.
fn diagnose(root: &Path, error: Error) -> Error {
    let files = project_sources::conflicted_files(&crate::layout::config_file(root));
    if files.is_empty() {
        return error;
    }
    let names = files
        .iter()
        .map(|(file, line)| {
            let path = file.strip_prefix(root).unwrap_or(file).display();
            format!("{path} (line {line})")
        })
        .collect::<Vec<_>>()
        .join(", ");
    Error::PolicyFileConflict(names)
}

/// The key requests are decided by: the policy files without their settings,
/// which compile into nothing, so a settings change keeps the compiled plan,
/// the snapshot identity and approvals bound to it.
fn key(sources: &project_sources::Sources) -> String {
    let (shared, local) = sources.config_layers();
    let shared = without_settings(shared).unwrap_or_else(|| shared.to_owned());
    let local = local.and_then(|local| match without_settings(local) {
        // A layer holding only settings adds no policy, like an absent one.
        Some(policy) if policy == SETTINGS_ONLY => None,
        Some(policy) => Some(policy),
        None => Some(local.to_owned()),
    });
    let mut rest = sources.parts();
    let _ = (rest.next(), rest.next());
    hash(
        "shield-files-plan-v2",
        [Some(shared.as_str()), local.as_deref()]
            .into_iter()
            .chain(rest),
    )
}

/// A configuration document emptied of settings, as `key` sees it.
const SETTINGS_ONLY: &str = "{\"schema_version\":1}";

/// The configuration text without its settings keys, or `None` for text that
/// is not a configuration document, which validation refuses separately.
fn without_settings(text: &str) -> Option<String> {
    let mut document: serde_json::Value = policy::parse(text).ok()?;
    let fields = document.as_object_mut()?;
    for key in crate::configuration::settings::KEYS {
        fields.remove(key);
    }
    Some(document.to_string())
}

/// The digest of the policy files' exact bytes, settings included, so a
/// reader notices any edit to them.
fn bytes_key(sources: &project_sources::Sources) -> String {
    hash("shield-files-bytes-v1", sources.parts())
}

fn hash<'a>(label: &'a str, parts: impl Iterator<Item = Option<&'a str>>) -> String {
    let mut hasher = blake3::Hasher::new();
    let header = [
        label,
        env!("CARGO_PKG_VERSION"),
        policy::resolution::compiled::FORMAT,
    ];
    for part in header.into_iter().map(Some).chain(parts) {
        match part {
            Some(bytes) => {
                hasher.update(&[1]);
                hasher.update(&(bytes.len() as u64).to_le_bytes());
                hasher.update(bytes.as_bytes());
            }
            None => {
                hasher.update(&[0]);
            }
        }
    }
    hasher.finalize().to_hex().to_string()
}

/// The folder that holds this project's compiled plans: the operator's chosen
/// cache location, or the per-user default. The native path guard protects
/// exactly this folder, so the two cannot disagree.
pub(in crate::review) fn cache_for(operator: &serde_json::Value) -> Result<PathBuf, Error> {
    match operator["compiled_cache_root"].as_str() {
        Some(directory) => Ok(PathBuf::from(directory)),
        None => super::cache::default_directory(),
    }
}

/// Keep this project's compiled plans in `directory` from now on. Plans
/// compile there on the next request that misses one; nothing is rebuilt now.
pub(in crate::review) fn relocate_cache(
    context: &super::Context,
    directory: &Path,
) -> Result<serde_json::Value, Error> {
    if !directory.is_absolute() {
        return Err(Error::Invalid("--cache-dir must be an absolute directory"));
    }
    let location = serde_json::json!(directory);
    let store = super::storage::Store::open(Path::new(&context.project_root))?;
    // The same location changes nothing, so nothing is written.
    if store.read_checkpoint()?.operator["compiled_cache_root"] == location {
        return Ok(serde_json::json!({"schema_version":1,"cache_directory":directory}));
    }
    store.update_checkpoint_if_changed(context, &|| Ok(()), |state| {
        let changed = state.operator["compiled_cache_root"] != location;
        state.operator["compiled_cache_root"] = location.clone();
        Ok(changed)
    })?;
    Ok(serde_json::json!({"schema_version":1,"cache_directory":directory}))
}

/// [`cache_for`] the project at `root`; a folder without review state uses the
/// default.
pub(in crate::review) fn cache_of(root: &Path) -> Result<PathBuf, Error> {
    match super::storage::Store::open(root).and_then(|store| store.read_checkpoint()) {
        Ok(state) => cache_for(&state.operator),
        Err(_) => super::cache::default_directory(),
    }
}

pub(in crate::review) fn plans(cache: &Path) -> PathBuf {
    cache.join("plans")
}

fn plan_path(cache: &Path, digest: &str) -> PathBuf {
    plans(cache).join(format!("{digest}.bin"))
}

/// Pack artifacts are stored per user under their SHA-256 digest, so any
/// checkout of any project can use an artifact another one acquired.
pub(in crate::review) fn artifact_store() -> Result<PathBuf, Error> {
    Ok(super::cache::default_directory()?.join("packs"))
}

/// A plan file is the content key it was compiled for, the BLAKE3 digest of
/// the plan, then the plan. A plan copied under another key is refused, so
/// only the files' own plan can decide their requests.
fn read_plan(path: &Path, key: &str) -> Option<Evaluator> {
    if !path.parent().is_some_and(private) || !private(path) {
        return None;
    }
    let _trace = crate::diagnostics::trace::span("cache.load");
    let bytes = fs::read(path).ok()?;
    crate::diagnostics::trace::count("cache.plan_bytes_read", bytes.len() as u64);
    let (recorded, rest) = bytes.split_at_checked(key.len())?;
    let (digest, plan) = rest.split_at_checked(32)?;
    if recorded != key.as_bytes()
        || blake3::hash(plan).as_bytes() != digest
        || plan.len() > policy::resolution::compiled::MAX_BYTES
    {
        return None;
    }
    Evaluator::from_authenticated_compiled(plan.to_vec()).ok()
}

/// Whether `path` is this user's own private entry and not a link: a plan in a
/// shared, linked or foreign location is not used.
fn private(path: &Path) -> bool {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return false;
    };
    if metadata.file_type().is_symlink() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.mode() & 0o077 != 0 || metadata.uid() != rustix::process::geteuid().as_raw() {
            return false;
        }
    }
    #[cfg(windows)]
    if crate::windows_security::check_private(path).is_err() {
        return false;
    }
    true
}

fn write_plan(path: &Path, key: &str, plan: &[u8]) -> Result<(), Error> {
    let mut bytes = key.as_bytes().to_vec();
    bytes.extend_from_slice(blake3::hash(plan).as_bytes());
    bytes.extend_from_slice(plan);
    write_private(path, &bytes)
}

/// Distinguishes temporaries written by threads of one process.
static SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Write `bytes` to `path` through a private temporary, in a private directory.
pub(in crate::review) fn write_private(path: &Path, bytes: &[u8]) -> Result<(), Error> {
    let directory = path
        .parent()
        .ok_or(Error::Invalid("cache path has no parent"))?;
    let cache = directory
        .parent()
        .ok_or(Error::Invalid("cache path has no parent"))?;
    // Create only Shield's own folders: a missing parent, such as an absent
    // home folder, means there is no cache, never that one is created.
    for folder in [cache, directory] {
        if !folder.exists() {
            let mut builder = fs::DirBuilder::new();
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            builder.create(folder)?;
            #[cfg(windows)]
            crate::windows_security::make_private(folder)?;
        }
    }
    if !private(directory) {
        return Err(Error::Invalid("cache folder is not private to this user"));
    }
    let temporary = directory.join(format!(
        ".{}.{}-{}.tmp",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("plan"),
        std::process::id(),
        SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let result = options
        .open(&temporary)
        .and_then(|mut file| file.write_all(bytes))
        .and_then(|()| fs::rename(&temporary, path));
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    Ok(result?)
}

pub(in crate::review) mod write;

#[cfg(test)]
mod tests;
