//! Per-user authority keyed by exact content, never by folder: consent for
//! administration settings, agent registrations and active policy, and the
//! single-use ledger for one-time grants. A project's files and its ignored
//! checkpoint are state; only these records, under the user's home, let that
//! state grant anything. The same content in any folder carries the same
//! consent for this user.
use super::{Error, State, objects, private_files};
use serde_json::{Value, json};
use std::{
    fs,
    io::Write as _,
    path::{Path, PathBuf},
};

const LIMIT: usize = 512 * 1024;
/// Most expired ledger entries one claim removes, so a claim's cost stays
/// bounded however many grants were consumed before.
const PRUNE: usize = 64;

/// The kinds of reviewed content a record can cover.
#[derive(Clone, Copy)]
pub(in crate::review) enum Kind {
    Administration,
    Registration,
    Policy,
}

impl Kind {
    fn folder(self) -> &'static str {
        match self {
            Self::Administration => "administration",
            Self::Registration => "registration",
            Self::Policy => "policy",
        }
    }
}

fn base(create: bool) -> Result<PathBuf, Error> {
    // Production keeps every per-user store beside `reviewed-policies` in
    // `~/.shield`. Tests use a named fixture user, or one store shared by the
    // test binary's processes, as one user's processes share their home.
    #[cfg(test)]
    let path = match super::portability::consent::fixture_directory() {
        Some(reviewed) => reviewed.with_extension("authority"),
        None => std::env::temp_dir().canonicalize()?.join(format!(
            "shield-authority-{}",
            crate::policy::fingerprint(std::env::current_exe()?.to_string_lossy().as_bytes())
        )),
    };
    #[cfg(not(test))]
    let path = super::portability::directory()?.with_file_name("authority");
    if create && !path.exists() {
        if let Some(parent) = path.parent()
            && !parent.exists()
        {
            super::private_directory(parent)?;
        }
        private_files::directory(&path, true)?;
    }
    private_files::directory(&path, false)?;
    Ok(path)
}

fn folder(name: &str, create: bool) -> Result<PathBuf, Error> {
    let path = base(create)?.join(name);
    private_files::directory(&path, create)?;
    Ok(path)
}

fn valid(digest: &str) -> bool {
    digest.len() == 64
        && digest
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn fingerprint(value: &Value) -> Result<String, Error> {
    crate::json::fingerprint(value).map_err(|_| Error::Invalid("authority content encoding"))
}

/// Whether this user consented to this exact content. Unavailable storage
/// grants nothing.
pub(in crate::review) fn consented(kind: Kind, digest: &str) -> bool {
    valid(digest)
        && folder(kind.folder(), false)
            .is_ok_and(|path| objects::read_file(&path.join(format!("{digest}.json"))).is_ok())
}

/// Record this user's consent to this exact content. The body is kept for
/// inspection and, for registrations, for adoption by an identical folder.
fn consent(kind: Kind, digest: &str, body: &Value) -> Result<(), Error> {
    if !valid(digest) {
        return Err(Error::Invalid("invalid authority digest"));
    }
    let directory = folder(kind.folder(), true)?;
    let path = directory.join(format!("{digest}.json"));
    if objects::read_file(&path).is_ok() {
        return Ok(());
    }
    let _guard = objects::Directory::enter(&directory)?;
    if objects::read_file(&path).is_ok() {
        return Ok(());
    }
    private_files::write(&path, body, LIMIT)
}

/// The administration settings this checkout asserts, or `None` for the
/// defaults, which need no consent: native interception enabled and the
/// default protected-path choice.
pub(in crate::review) fn administration(state: &State) -> Option<Value> {
    let value =
        json!({"enabled":state.enforcement.enabled,"path_access":state.operator["path_access"]});
    (!state.enforcement.enabled || !state.operator["path_access"].is_null()).then_some(value)
}

fn administration_digest(value: &Value) -> Result<String, Error> {
    fingerprint(&json!(["shield-administration-v1", value]))
}

/// The reviewed content of a registration: what Shield owns in agent files,
/// its tool approvals and Ask fallbacks. The installed binary's location,
/// digest and version and per-connection launch tokens are excluded, so an
/// upgrade or another installation path keeps the consent.
fn registration_content(integration: &Value) -> Value {
    let mut content = integration.clone();
    if let Some(fields) = content.as_object_mut() {
        for field in ["binary", "binary_digest", "version"] {
            fields.remove(field);
        }
        if let Some(connections) = fields.get_mut("connections").and_then(Value::as_object_mut) {
            for owned in connections.values_mut() {
                if let Some(owned) = owned.as_object_mut() {
                    owned.remove("launches");
                }
            }
        }
    }
    content
}

/// Registration consent covers its content together with the reviewed policy
/// it was set up under, so identical agent entries alone do not register
/// Shield in a folder whose policy this user never reviewed with them.
fn registration_digest(integration: &Value, policy_revision: &str) -> Result<String, Error> {
    fingerprint(&json!([
        "shield-registration-v1",
        registration_content(integration),
        policy_revision
    ]))
}

/// The active policy's exact retained content identity.
fn policy_digest(state: &State) -> Result<Option<String>, Error> {
    let Some(active) = &state.active else {
        return Ok(None);
    };
    let candidate = active.project_candidate()?;
    fingerprint(&json!([
        "shield-policy-v1",
        super::sources::candidate_identity(candidate),
        super::sources::map_digest(&active.candidates),
        active.revision()
    ]))
    .map(Some)
}

fn policy_index(revision: &str, create: bool) -> Result<PathBuf, Error> {
    if !valid(revision) {
        return Err(Error::Invalid("invalid policy revision"));
    }
    let registrations = folder(Kind::Registration.folder(), create)?;
    let path = registrations.join(revision);
    private_files::directory(&path, create)?;
    Ok(path)
}

/// Record consent for the authority this state asserts. Called by the project
/// writer before the checkpoint is published: state reaches a writer only
/// through [`admit`] or a reviewed change, so it never launders a copy.
pub(in crate::review) fn record(state: &State) -> Result<(), Error> {
    record_administration(state)?;
    record_registration(state)?;
    if let Some(digest) = policy_digest(state)? {
        consent(Kind::Policy, &digest, &json!({"schema_version":1}))?;
    }
    Ok(())
}

fn record_administration(state: &State) -> Result<(), Error> {
    if let Some(value) = administration(state) {
        consent(
            Kind::Administration,
            &administration_digest(&value)?,
            &json!({"schema_version":1,"administration":value}),
        )?;
    }
    Ok(())
}

fn record_registration(state: &State) -> Result<(), Error> {
    if !state.operator["integration"].is_null() {
        let digest = registration_digest(&state.operator["integration"], &state.policy_revision)?;
        // When this content was first reviewed: picks the newer of two
        // equally covering registrations that both match a folder's files.
        let recorded_at = crate::review::clock::now_ms().unwrap_or(0);
        let body = json!({"schema_version":1,"policy_revision":state.policy_revision,
            "integration":state.operator["integration"],"recorded_at_ms":recorded_at});
        consent(Kind::Registration, &digest, &body)?;
        // Index by policy so a folder with only committed files can find the
        // registrations reviewed for its policy and check them against its files.
        if valid(&state.policy_revision) {
            let index = policy_index(&state.policy_revision, true)?;
            let path = index.join(format!("{digest}.json"));
            if objects::read_file(&path).is_err() {
                let _guard = objects::Directory::enter(&index)?;
                if objects::read_file(&path).is_err() {
                    private_files::write(&path, &body, LIMIT)?;
                }
            }
        }
    }
    Ok(())
}

/// What the earlier format's own evidence proves about a checkpoint.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::review) enum Legacy {
    /// Not a legacy checkpoint, or its evidence proves nothing.
    Unproven,
    /// A committed administration receipt matched exactly its settings and
    /// registration. The receipt binds no policy, so it proves only those.
    Administration,
    /// The installation the earlier format wrote, upgraded in place.
    Installation,
}

/// How a checkpoint's authority is admitted when loaded.
pub(in crate::review) struct Admission {
    /// What the earlier format proves. Proven authority is recorded as this
    /// user's consent once.
    pub(in crate::review) legacy: Legacy,
    /// Record consent for proven legacy authority; diagnostic readers only
    /// treat it as consented in memory.
    pub(in crate::review) seed: bool,
}

/// Keep only the authority this user consented to, by exact content. Settings
/// without consent read as defaults, a registration without consent is
/// absent, and an active policy without consent is inactive. Nothing is
/// written to the checkpoint here.
pub(in crate::review) fn admit(
    store: &super::Store,
    state: &mut State,
    admission: &Admission,
) -> Result<(), Error> {
    let root = Path::new(store.root());
    let installation = admission.legacy == Legacy::Installation;
    let receipt = admission.legacy == Legacy::Administration;
    if installation && admission.seed {
        record(state)?;
    }
    let trusted = |kind, digest: &str| installation || consented(kind, digest);
    if let Some(value) = administration(state)
        && !receipt
        && !trusted(Kind::Administration, &administration_digest(&value)?)
    {
        state.enforcement = crate::review::mode::Mode::default();
        if let Some(fields) = state.operator.as_object_mut() {
            fields.remove("path_access");
        }
    }
    // An activation whose own binding fails is corrupt, not unreviewed; it
    // stays so validation refuses it instead of quietly dropping it. An
    // administration receipt never vouches for policy.
    let had_policy = state.active.is_some();
    if let Some(digest) = policy_digest(state)?
        && state
            .active
            .as_ref()
            .is_some_and(|active| active.validate_binding(state).is_ok())
        && !trusted(Kind::Policy, &digest)
        && !privately_reviewed(store, state)
    {
        state.active = None;
    }
    // The receipt vouches for the registration only under policy this user
    // reviewed, since registration consent is keyed by that policy too.
    let receipt_registration = receipt && (!had_policy || state.active.is_some());
    if !state.operator["integration"].is_null()
        && crate::review::integrations::registration(&state.operator).is_ok()
        // A malformed record is corrupt, not unreviewed; it stays so its
        // readers refuse it instead of quietly treating the agent as unset.
        && !receipt_registration
        && !trusted(
            Kind::Registration,
            &registration_digest(&state.operator["integration"], &state.policy_revision)?,
        )
        && let Some(fields) = state.operator.as_object_mut()
    {
        fields.remove("integration");
    }
    if receipt && admission.seed {
        record_administration(state)?;
        if receipt_registration {
            record_registration(state)?;
        }
    }
    // An absent or dropped registration can come back from agent files that
    // match one this user reviewed; a setup in progress owns those files.
    if state.operator["integration"].is_null()
        && state.operator["integration_pending"].is_null()
        && let Some(integration) = adoptable(root, state)?
    {
        state.operator["integration"] = integration;
    }
    Ok(())
}

/// A copied checkpoint whose exact policy this user reviewed elsewhere keeps
/// it active, as a config-only clone of the same files would.
fn privately_reviewed(store: &super::Store, state: &State) -> bool {
    state
        .active
        .as_ref()
        .and_then(|active| active.project_candidate().ok())
        .and_then(|reference| store.captured_candidate(reference).ok())
        .is_some_and(|candidate| super::portability::consent::presence_for(&candidate))
}

/// A registration this user reviewed for this exact policy whose agent files
/// in this folder match it exactly, such as in a worktree or clone created
/// with the same committed or copied agent settings.
fn adoptable(root: &Path, state: &State) -> Result<Option<Value>, Error> {
    if !valid(&state.policy_revision) {
        return Ok(None);
    }
    let Ok(index) = policy_index(&state.policy_revision, false) else {
        return Ok(None);
    };
    let mut found: Vec<(Value, Vec<&'static str>, u64)> = Vec::new();
    for entry in fs::read_dir(&index)?.take(64) {
        let path = entry?.path();
        if path.extension().is_none_or(|extension| extension != "json") {
            continue;
        }
        let Ok(body) = private_files::read(&path, LIMIT) else {
            continue;
        };
        let integration = &body["integration"];
        if body["schema_version"] != 1
            || body["policy_revision"] != state.policy_revision.as_str()
            || !consented(
                Kind::Registration,
                &registration_digest(integration, &state.policy_revision)?,
            )
        {
            continue;
        }
        let mut operator = state.operator.clone();
        operator["integration"] = integration.clone();
        if crate::review::integrations::matches_files(root, &operator)
            && let Ok(Some(registered)) = crate::review::integrations::registration(&operator)
        {
            let covered = crate::agents::ADAPTERS
                .iter()
                .map(|adapter| adapter.spec().id)
                .filter(|id| registered.covers(id))
                .collect();
            let recorded = body["recorded_at_ms"].as_u64().unwrap_or(0);
            found.push((integration.clone(), covered, recorded));
        }
    }
    Ok(choose(&found))
}

/// The registration to adopt among those whose files all match: the one
/// covering every agent the others cover, and among equals the most recently
/// reviewed. Anything else is ambiguous and adopts nothing.
fn choose(found: &[(Value, Vec<&'static str>, u64)]) -> Option<Value> {
    let covers_all = |covered: &[&str]| {
        found
            .iter()
            .all(|(_, other, _)| other.iter().all(|id| covered.contains(id)))
    };
    let mut maximal: Vec<_> = found
        .iter()
        .filter(|(_, covered, _)| covers_all(covered))
        .collect();
    maximal.sort_by_key(|(_, _, recorded)| std::cmp::Reverse(*recorded));
    match maximal.as_slice() {
        [only] => Some(only.0.clone()),
        [newest, next, ..] if newest.2 > next.2 => Some(newest.0.clone()),
        _ => None,
    }
}

/// Spend a one-time grant once for this user, whichever folder holds it.
/// Returns false when it was already spent. A crash after creating the entry
/// leaves it spent, never reusable.
pub(in crate::review) fn claim(grant: &str, expires_at_ms: u64) -> Result<bool, Error> {
    let digest = fingerprint(&json!(["shield-grant-v1", grant]))?;
    let directory = folder("consumed", true)?;
    prune(&directory);
    let path = directory.join(format!("{digest}.json"));
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    super::private(&mut options);
    match options.open(&path) {
        Ok(mut file) => {
            file.write_all(
                &serde_json::to_vec(&json!({"expires_at_ms":expires_at_ms}))
                    .map_err(|_| Error::Invalid("ledger encoding"))?,
            )?;
            super::sync(&file)?;
            Ok(true)
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
        Err(error) => Err(error.into()),
    }
}

/// Remove a bounded number of entries whose grants have expired. An entry is
/// needed only while its grant could still be presented.
fn prune(directory: &Path) {
    let Ok(now) = crate::review::clock::now_ms() else {
        return;
    };
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    let mut removed = 0;
    for entry in entries.flatten() {
        if removed == PRUNE {
            break;
        }
        let path = entry.path();
        let expired = fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
            .and_then(|value| value["expires_at_ms"].as_u64())
            .is_some_and(|expires| expires < now);
        if expired && fs::remove_file(&path).is_ok() {
            removed += 1;
        }
    }
}

#[cfg(test)]
mod tests;
