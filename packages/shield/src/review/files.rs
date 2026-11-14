//! Owns reading the project's policy files, which are its enforced policy, and
//! loading their compiled plan from a per-user cache keyed by their content.
use super::{Error, integrations::project_sources};
#[cfg(test)]
use crate::configuration::Bundle;
use crate::{application, configuration::Evaluator, policy};
use std::{
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
    let sources = project_sources::read(&root.join(".shield/config.yml")).map_err(error)?;
    let digest = key(&sources);
    // Without a cache folder every load compiles; the files still decide.
    let path = cache.map(|cache| plan_path(cache, &digest));
    let evaluator =
        if let Some(evaluator) = path.as_deref().and_then(|path| read_plan(path, &digest)) {
            crate::diagnostics::trace::count("policy.plan_hits", 1);
            evaluator
        } else {
            crate::diagnostics::trace::count("policy.plan_misses", 1);
            let (_, snapshot) = sources.validated().map_err(error)?;
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
    let sources = project_sources::read(&root.join(".shield/config.yml")).map_err(error)?;
    let digest = key(&sources);
    if cache.is_some_and(|cache| plan_recorded(&plan_path(cache, &digest), &digest)) {
        return Ok(digest);
    }
    let (_, snapshot) = sources.validated().map_err(error)?;
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

/// The digest of the policy files under `root`, the one requests are decided
/// by, without validating or compiling them.
pub(in crate::review) fn digest(root: &Path) -> Result<String, Error> {
    let sources = project_sources::read(&root.join(".shield/config.yml")).map_err(error)?;
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
    if !root.join(".shield/config.yml").is_file() {
        return Ok(false);
    }
    let invalid = |error: Error| Error::PolicyFiles(error.to_string());
    let digest = digest(root).map_err(invalid)?;
    if state.followed_files.as_deref() == Some(digest.as_str()) {
        return Ok(false);
    }
    // Adoption compares with the stored candidate's own files; the stored
    // form of the candidate stays as it is unless the content changes.
    let Some(stored) = state.candidates.get("project").cloned() else {
        return Ok(false);
    };
    let capture = super::storage::Store::open(root)
        .and_then(|store| store.captured_candidate(&stored))
        .and_then(|hydrated| {
            state.candidates.insert("project".into(), hydrated);
            super::changes::project_files::capture(context, state)
        });
    state.candidates.insert("project".into(), stored);
    let capture = capture.map_err(invalid)?;
    let candidates = std::collections::BTreeMap::from([("project".into(), capture.candidate)]);
    let revision = super::snapshots(&candidates)
        .and_then(|snapshots| Evaluator::revision_of(&snapshots).map_err(Error::from))
        .map_err(invalid)?;
    let hashes: serde_json::Map<String, serde_json::Value> = capture
        .paths
        .into_iter()
        .filter_map(|(name, hash)| hash.map(|hash| (name, serde_json::Value::String(hash))))
        .collect();
    // Files that say what the state already holds change only the record of
    // which files it followed; operator metadata and proposals stay as they are.
    if revision != state.policy_revision
        || capture.metadata["installed"] != state.operator["installed"]
    {
        state.operator = capture.metadata;
        state.operator["projection_file_hashes"] = hashes.into();
        state.candidates = candidates;
        state.policy_revision = revision;
    }
    state.followed_files = Some(digest);
    Ok(true)
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
    meta: &serde_json::Value,
) -> Result<Fetched, Error> {
    let shield = root.join(".shield");
    let store = artifact_store()?;
    let mut missing = Vec::new();
    for lock in ["shield.lock", "shield.local.lock"] {
        let Ok(text) = fs::read_to_string(shield.join(lock)) else {
            continue;
        };
        let locked: serde_json::Value = serde_json::from_str(&text)
            .map_err(|_| Error::Invalid("project lock file is not valid JSON"))?;
        for row in locked["packs"].as_array().into_iter().flatten() {
            let Some(digest) = row["digest"]
                .as_str()
                .filter(|d| d.len() == 64 && d.bytes().all(|b| b.is_ascii_hexdigit()))
            else {
                continue;
            };
            let name = format!("{digest}.yml");
            if !store.join(&name).exists() && !shield.join("cache").join(&name).exists() {
                missing.push(row.clone());
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
    let sources = super::registries::effective_from_metadata(meta)?;
    let client = super::registries::fetcher();
    for row in missing {
        let release = format!(
            "{}@{}",
            row["id"].as_str().unwrap_or("?"),
            row["version"].as_str().unwrap_or("?")
        );
        match fetch_release(&row, &sources, &client, &store) {
            Ok(()) => outcome.releases.push(release),
            Err(error) => outcome.failures.push((release, error.to_string())),
        }
    }
    Ok(outcome)
}

fn fetch_release(
    row: &serde_json::Value,
    sources: &canonic_registry_kit::registrations::Effective,
    fetcher: &canonic_registry_kit::acquisition::Fetcher,
    store: &Path,
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
    write_private(&store.join(format!("{digest}.yml")), text.as_bytes())
}

/// The compiled plan of the policy files under `root` if one is cached; never
/// compiles.
pub(in crate::review) fn cached(root: &Path) -> Option<Evaluator> {
    let sources = project_sources::read(&root.join(".shield/config.yml")).ok()?;
    let digest = key(&sources);
    read_plan(&plan_path(&cache_of(root).ok()?, &digest), &digest)
}

/// Capture and validate the policy files under `root`.
#[cfg(test)]
pub(in crate::review) fn capture(root: &Path) -> Result<Bundle, Error> {
    project_sources::capture(&root.join(".shield/config.yml")).map_err(error)
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

fn key(sources: &project_sources::Sources) -> String {
    let mut hasher = blake3::Hasher::new();
    let header = [
        "shield-files-plan-v1",
        env!("CARGO_PKG_VERSION"),
        policy::compiled::FORMAT,
    ];
    for part in header.into_iter().map(Some).chain(sources.parts()) {
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
    let bytes = fs::read(path).ok()?;
    let (recorded, rest) = bytes.split_at_checked(key.len())?;
    let (digest, plan) = rest.split_at_checked(32)?;
    if recorded != key.as_bytes()
        || blake3::hash(plan).as_bytes() != digest
        || plan.len() > policy::compiled::MAX_BYTES
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

#[cfg(test)]
mod tests;
