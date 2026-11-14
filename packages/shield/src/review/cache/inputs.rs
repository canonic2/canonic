//! Optional metadata hints for already verified projections; never policy authority.
use super::{Artifact, Error, digest};
use crate::{
    policy,
    review::{Context, model::State, storage},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Read,
    path::Path,
};

const MAX_INPUTS: usize = 128;
const MAX_PARENTS: usize = 512;
const MAX_BYTES: usize = 512 * 1024;
const REF_BYTES: usize = 1024;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Reference {
    schema_version: u8,
    activation: String,
    candidate: String,
    revision: String,
    compiled: String,
    object: String,
    length: u64,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(in crate::review) struct Manifest {
    schema_version: u8,
    inputs: BTreeMap<String, Option<Stamp>>,
    parents: BTreeMap<String, Stamp>,
}
#[derive(PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Stamp {
    device: u64,
    inode: u64,
    length: u64,
    modified: (i64, i64),
    changed: (i64, i64),
    mode: u32,
}
impl Stamp {
    fn capture(metadata: &fs::Metadata) -> Option<Self> {
        if metadata.file_type().is_symlink() || (!metadata.is_file() && !metadata.is_dir()) {
            return None;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            Some(Self {
                device: metadata.dev(),
                inode: metadata.ino(),
                length: metadata.len(),
                modified: (metadata.mtime(), metadata.mtime_nsec()),
                changed: (metadata.ctime(), metadata.ctime_nsec()),
                mode: metadata.mode(),
            })
        }
        #[cfg(not(unix))]
        {
            None
        }
    }
}

fn version(path: &Path) -> Result<Option<Stamp>, Error> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => Stamp::capture(&metadata)
            .map(Some)
            .ok_or(Error::Invalid("input metadata unavailable")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

/// Replaces the existing post-publication content verification, using that same
/// opened file for byte verification and before/after native version capture.
pub(in crate::review) fn verify(
    context: &Context,
    state: &State,
    trusted_files: &BTreeMap<String, String>,
) -> Result<(bool, Option<Manifest>), Error> {
    let Some(expected) = state.operator["projection_file_hashes"]
        .as_object()
        .filter(|rows| rows.len() <= MAX_INPUTS.saturating_sub(2))
    else {
        return Ok((
            crate::review::publication::projections::projection_changed(context, state)?,
            None,
        ));
    };
    if !cfg!(unix) {
        return Ok((
            crate::review::publication::projections::projection_changed(context, state)?,
            None,
        ));
    }
    // The publication owner supplies the canonical closure it generated from
    // the reviewed candidate, rather than trusting paths appointed by a hint.
    if !expected.keys().eq(trusted_files.keys()) {
        return Ok((true, None));
    }
    let root = Path::new(&context.project_root).join(".shield");
    let mut inputs = BTreeMap::new();
    let mut parents = BTreeSet::from([String::new()]);
    let mut changed = false;
    for (name, hash) in expected {
        crate::configuration::portable_path(name)?;
        let mut parent = Path::new(name).parent();
        while let Some(path) = parent.filter(|path| !path.as_os_str().is_empty()) {
            parents.insert(
                path.to_str()
                    .ok_or(Error::Invalid("input parent encoding"))?
                    .into(),
            );
            parent = path.parent();
        }
        if parents.len() > MAX_PARENTS {
            return Ok((
                crate::review::publication::projections::projection_changed(context, state)?,
                None,
            ));
        }
        let path = root.join(name);
        let before = version(&path)?;
        let Some(before) = before else {
            changed = true;
            continue;
        };
        let file = fs::File::open(&path)?;
        if !file.metadata()?.is_file() {
            return Err(Error::Invalid("projection must be a regular file"));
        }
        let opened = Stamp::capture(&file.metadata()?);
        let mut bytes = Vec::new();
        (&file)
            .take((policy::MAX_DOCUMENT_BYTES + 1) as u64)
            .read_to_end(&mut bytes)?;
        if bytes.len() > policy::MAX_DOCUMENT_BYTES {
            return Err(policy::Error::Limit.into());
        }
        let stable = opened.as_ref() == Some(&before)
            && Stamp::capture(&file.metadata()?).as_ref() == Some(&before)
            && version(&path)?.as_ref() == Some(&before);
        let approved = policy::fingerprint(trusted_files[name].as_bytes());
        changed |= !stable || hash != &json!(approved) || policy::fingerprint(&bytes) != approved;
        inputs.insert(name.clone(), Some(before));
    }
    for name in ["config.local.yml", "rules.local.yml"] {
        if !expected.contains_key(name) {
            let actual = version(&root.join(name))?;
            changed |= actual.is_some();
            inputs.insert(name.into(), actual);
        }
    }
    if changed {
        return Ok((true, None));
    }
    let mut parent_versions = BTreeMap::new();
    for name in parents {
        let path = root.join(&name);
        let metadata = fs::symlink_metadata(&path)?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(Error::Invalid("input parent cannot be a link"));
        }
        parent_versions.insert(
            name,
            Stamp::capture(&metadata).ok_or(Error::Invalid("input metadata unavailable"))?,
        );
    }
    // A swapped parent or file during capture cannot become a clean baseline.
    for (name, stamp) in &inputs {
        if version(&root.join(name))?.as_ref() != stamp.as_ref() {
            return Ok((true, None));
        }
    }
    let manifest = Manifest {
        schema_version: 1,
        inputs,
        parents: parent_versions,
    };
    Ok((
        false,
        crate::json::fits(&manifest, MAX_BYTES).then_some(manifest),
    ))
}

fn reference(artifact: &Artifact) -> Option<Reference> {
    let value = artifact.input_hints.as_ref()?;
    if !crate::json::fits(value, REF_BYTES) {
        return None;
    }
    let reference: Reference = serde_json::from_value(value.clone()).ok()?;
    (reference.schema_version == 1
        && digest(&reference.object)
        && reference.length <= MAX_BYTES as u64
        && reference.compiled == artifact.digest)
        .then_some(reference)
}
fn authenticated_reference(state: &State) -> Result<Reference, Error> {
    let active = state.active.as_ref().ok_or(Error::Missing)?;
    let reference = reference(&active.compiled).ok_or(Error::Missing)?;
    if !active.matches_parts(
        &reference.activation,
        &reference.revision,
        &reference.candidate,
    ) {
        return Err(Error::StaleSnapshot);
    }
    let fingerprint =
        crate::json::fingerprint(active.compiled.input_hints.as_ref().ok_or(Error::Missing)?)
            .map_err(|_| Error::Missing)?;
    state
        .authorization_events
        .iter()
        .any(|event| {
            event["kind"] == "compiled_cache_prepared"
                && event["operator"]["mechanism"] == "derived-cache"
                && event["links"]["activation_id"] == reference.activation
                && event["details"]["purpose"] == "projection-input-metadata"
                && event["details"]["candidate_digest"] == reference.candidate
                && event["details"]["policy_revision"] == reference.revision
                && event["details"]["compiled_digest"] == reference.compiled
                && event["details"]["input_hint_digest"] == fingerprint
        })
        .then_some(reference)
        .ok_or(Error::Missing)
}
pub(super) fn object_name(artifact: &Value) -> Option<String> {
    let reference = &artifact["input_hints"];
    (reference["schema_version"] == 1).then_some(())?;
    let object = reference["object"].as_str().filter(|value| digest(value))?;
    Some(format!("{object}.probe.json"))
}
pub(in crate::review) fn availability(artifact: &Artifact) -> Value {
    json!({"availability":if reference(artifact).is_some(){"recorded"}else{"unavailable"},
        "probe":"not-run","integrity":"unchecked","authorizes":false,
        "native_change_time":if cfg!(unix){"supported"}else{"unavailable"}})
}

/// Optional maintenance after exact source verification. Failure loses only a
/// hint, never the committed activation or its checked source/compiled binding.
pub(in crate::review) fn bind(
    context: &Context,
    expected: &State,
    manifest: &Manifest,
) -> Result<(), Error> {
    let active = expected.active.as_ref().ok_or(Error::Missing)?;
    let bytes =
        serde_json::to_vec(manifest).map_err(|_| Error::Invalid("input metadata encoding"))?;
    if bytes.len() > MAX_BYTES {
        return Err(Error::Overloaded);
    }
    let object = policy::fingerprint(&bytes);
    let name = format!("{object}.probe.json");
    let _pin =
        super::retention::publish_probe(&context.project_root, &active.compiled, &name, &bytes)?;
    let reference = serde_json::to_value(Reference {
        schema_version: 1,
        activation: active.activation_id().into(),
        candidate: active.metadata()["candidate_digest"]
            .as_str()
            .ok_or(Error::Missing)?
            .into(),
        revision: expected.policy_revision.clone(),
        compiled: active.compiled.digest.clone(),
        object,
        length: bytes.len() as u64,
    })
    .map_err(|_| Error::Invalid("input metadata reference"))?;
    storage::Store::open(Path::new(&context.project_root))?.update_checkpoint(
        context,
        &|| Ok(()),
        |state| {
            {
                let current = state.active.as_mut().ok_or(Error::StaleSnapshot)?;
                if current.activation_id() != active.activation_id()
                    || current.compiled.digest != active.compiled.digest {
                    return Err(Error::StaleSnapshot);
                }
                current.compiled.input_hints = Some(reference.clone());
            }
            if authenticated_reference(state).is_ok() { return Ok(json!({})); }
            // Retain one current auxiliary proof, independent of the history
            // journal. Never discard the actual activation/cache authorization.
            state.authorization_events.retain(|event| !(event["kind"] == "compiled_cache_prepared"
                && event["details"]["purpose"] == "projection-input-metadata"
                && event["links"]["activation_id"] == active.activation_id()));
            let mut event = state.event("compiled_cache_prepared", context,
                json!({"activation_id":active.activation_id()}), json!({}),
                json!({"purpose":"projection-input-metadata","candidate_digest":reference["candidate"],
                    "policy_revision":reference["revision"],"compiled_digest":reference["compiled"],
                    "input_hint_digest":crate::json::fingerprint(&reference).map_err(|_|Error::Invalid("input metadata digest"))?}));
            event["operator"] = json!({"mechanism":"derived-cache"});
            state.record(event);
            Ok(json!({}))
        },
    )?;
    Ok(())
}

/// Explicit publication/repair may prepare a missing optional hint. Callers
/// already own projection publication; ordinary decisions must never call this.
pub(in crate::review) fn refresh(
    context: &Context,
    trusted_files: &BTreeMap<String, String>,
) -> Result<(), Error> {
    let state = storage::Store::open(Path::new(&context.project_root))?.read_checkpoint()?;
    state.check(context)?;
    if let (false, Some(manifest)) = verify(context, &state, trusted_files)? {
        bind(context, &state, &manifest)?;
    }
    Ok(())
}

// Runtime loading reads the policy files directly; only this module's tests
// still probe the edited-input hints, until publication stops recording them.
#[cfg(test)]
pub(in crate::review) fn probe(state: &State) -> Value {
    let _span = crate::diagnostics::trace::span("cache.input_probe");
    if let Ok(changed) = probe_inner(state) {
        crate::diagnostics::trace::count(
            if changed.is_empty() {
                "cache.inputs_metadata_match"
            } else {
                "cache.inputs_metadata_changed"
            },
            1,
        );
        json!({"status":if changed.is_empty(){"metadata-match"}else{"changed"},"changed_inputs":changed,
                "integrity":"unchecked","authorizes":false,"next_action":if changed.is_empty(){Value::Null}else{json!("shield policy review")}})
    } else {
        crate::diagnostics::trace::count("cache.inputs_unchecked", 1);
        json!({"status":"unchecked","integrity":"unchecked","authorizes":false})
    }
}
#[cfg(test)]
fn probe_inner(state: &State) -> Result<Vec<String>, Error> {
    let active = state.active.as_ref().ok_or(Error::Missing)?;
    let artifact = &active.compiled;
    // Authenticate before opening any descriptor-appointed file or stat target.
    let reference = authenticated_reference(state)?;
    let partition = artifact
        .path()
        .parent()
        .ok_or(Error::Missing)?
        .to_path_buf();
    let directory = storage::objects::Directory::inspect(&partition)?;
    let name = format!("{}.probe.json", reference.object);
    let _pin = directory.pins(&BTreeSet::from([name.clone()]))?;
    let file = storage::objects::read_file(&partition.join(name))?;
    if file.metadata()?.len() != reference.length {
        return Err(Error::Missing);
    }
    let mut bytes = Vec::new();
    file.take(reference.length + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 != reference.length || policy::fingerprint(&bytes) != reference.object {
        return Err(Error::Missing);
    }
    let manifest: Manifest =
        serde_json::from_slice(&bytes).map_err(|_| Error::Invalid("input metadata shape"))?;
    if manifest.schema_version != 1
        || manifest.inputs.len() > MAX_INPUTS
        || manifest.parents.len() > MAX_PARENTS
    {
        return Err(Error::Missing);
    }
    let root = Path::new(&state.project_root).join(".shield");
    let mut changed = Vec::new();
    for (name, stamp) in &manifest.parents {
        crate::diagnostics::trace::count("cache.input_metadata_probes", 1);
        if !name.is_empty() {
            crate::configuration::portable_path(name)?;
        }
        let path = root.join(name);
        if !fs::symlink_metadata(&path).is_ok_and(|metadata| {
            metadata.is_dir() && Stamp::capture(&metadata).as_ref() == Some(stamp)
        }) {
            changed.push(if name.is_empty() {
                ".shield".into()
            } else {
                name.clone()
            });
        }
    }
    for (name, expected) in manifest.inputs {
        crate::configuration::portable_path(&name)?;
        crate::diagnostics::trace::count("cache.input_metadata_probes", 1);
        if version(&root.join(&name))?.as_ref() != expected.as_ref() {
            changed.push(name);
        }
    }
    Ok(changed)
}

#[cfg(test)]
mod tests;
