//! User-private immutable registry security metadata and monotonic head CAS.
use super::{Error, objects, private_files};
use canonic_registry_kit::{
    acquisition::VerifiedUpdate,
    checkpoint::Checkpoints,
    source::Source,
    trust::{Revocations, TrustedRoot},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeSet, VecDeque},
    fs::{self, File},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
};

const HEAD_LIMIT: usize = 32 * 1024;
const OBJECT_LIMIT: usize = 4 * 1024 * 1024;
const STORAGE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_SOURCES: usize = 256;
const MAX_FILES: usize = MAX_SOURCES * 3 + 4;
const MEMO_BYTES: usize = 64 * 1024 * 1024;
const MEMO_ENTRIES: usize = 64;

#[cfg(test)]
thread_local! {
    pub(in crate::review) static INITIAL_ADOPTION_STAGED: std::cell::RefCell<Option<Box<dyn FnOnce()>>> = const { std::cell::RefCell::new(None) };
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Head {
    schema_version: u32,
    generation: u64,
    intent: String,
    source: Source,
    revision: String,
    object: String,
    bytes: usize,
    root_version: u64,
    revocation_sequence: u64,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Mode {
    schema_version: u32,
    #[serde(rename = "mode")]
    kind: String,
    source: Source,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    approved_initial: Option<Head>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Object {
    schema_version: u32,
    root_checkpoint: String,
    revocation_payload: String,
    revocation_signatures: String,
    catalog_checkpoints: Value,
}

pub(in crate::review) struct Loaded {
    pub(in crate::review) root: TrustedRoot,
    pub(in crate::review) revocations: Revocations,
    pub(in crate::review) checkpoints: Checkpoints,
    pub(in crate::review) revision: String,
    pub(in crate::review) root_revision: String,
    pub(in crate::review) revocation_revision: String,
    pub(in crate::review) object: String,
}
struct Slot {
    cost: usize,
    value: OnceLock<Result<Arc<Loaded>, String>>,
}
#[derive(Clone)]
pub(in crate::review) struct Retained {
    pub(in crate::review) generation: u64,
    _slot: Arc<Slot>,
    loaded: Arc<Loaded>,
}
impl std::ops::Deref for Retained {
    type Target = Loaded;
    fn deref(&self) -> &Loaded {
        &self.loaded
    }
}
static MEMO: Mutex<VecDeque<(PathBuf, Arc<Slot>)>> = Mutex::new(VecDeque::new());

#[derive(Clone)]
pub(in crate::review) struct Namespace {
    directory: PathBuf,
}
pub(in crate::review) struct Published {
    pub(in crate::review) generation: u64,
    pub(in crate::review) revision: String,
    pub(in crate::review) object: String,
    pub(in crate::review) replayed: bool,
    pub(in crate::review) durable: bool,
    pub(in crate::review) repair_needed: bool,
}
impl Namespace {
    #[cfg(test)]
    pub(in crate::review) fn exclusive_fixture(&self) -> Result<objects::Directory, Error> {
        self.initialize()?;
        objects::Directory::try_enter(&self.directory)
    }
    pub(in crate::review) fn user() -> Result<Self, Error> {
        #[cfg(test)]
        let base = if let Some(home) = std::env::var_os("SHIELD_ISOLATED_HOME") {
            PathBuf::from(home).join(".shield").join("registry-trust")
        } else {
            std::env::temp_dir()
                .canonicalize()?
                .join(format!("shield-registry-trust-{}", std::process::id()))
        };
        #[cfg(not(test))]
        let base = {
            #[cfg(windows)]
            let home = std::env::var_os("LOCALAPPDATA");
            #[cfg(not(windows))]
            let home = std::env::var_os("HOME");
            home.map(PathBuf::from)
                .filter(|path| path.is_absolute())
                .ok_or(Error::Invalid("user registry trust storage unavailable"))?
                .join(".shield")
                .join("registry-trust")
        };
        Ok(Self { directory: base })
    }
    fn initialize(&self) -> Result<(), Error> {
        let parent = self
            .directory
            .parent()
            .ok_or(Error::Invalid("registry trust parent missing"))?;
        if !parent.exists() {
            super::private_directory(parent)?;
        }
        private_files::directory(&self.directory, true)?;
        if !self.directory.join(".objects.lock").exists() {
            objects::initialize_mutex(&self.directory)?;
        }
        Ok(())
    }
    /// Shared guard linearizes final policy use with exclusive head publication.
    pub(in crate::review) fn guard(
        &self,
        checkpoint: super::Checkpoint<'_>,
    ) -> Result<File, Error> {
        self.initialize()?;
        objects::read_lease(&self.directory, Some(checkpoint))
    }
    pub(in crate::review) fn try_guard(&self) -> Result<File, Error> {
        self.initialize()?;
        objects::read_lease(&self.directory, None)
    }

    /// Inspect staged adoption only after the active publisher has released
    /// its lease. A marker that survives that wait needs exact recovery.
    pub(in crate::review) fn guard_recovering(
        &self,
        sources: &BTreeSet<&str>,
        checkpoint: super::Checkpoint<'_>,
    ) -> Result<File, Error> {
        loop {
            let guard = self.guard(checkpoint)?;
            let mut pending = Vec::new();
            for &identity in sources {
                if self
                    .mode(identity)?
                    .is_some_and(|mode| mode.approved_initial.is_some())
                {
                    pending.push(identity);
                }
            }
            if pending.is_empty() {
                return Ok(guard);
            }

            // Release the shared lease before queueing for exclusive access.
            // Recovery reads the marker again under that lease, so it can
            // never replay a transition another writer has already finished.
            drop(guard);
            let _writer = objects::write_lease(&self.directory, checkpoint)?;
            for identity in pending {
                checkpoint()?;
                self.recover_locked(identity)?;
            }
        }
    }

    pub(in crate::review) fn read(&self, identity: &str) -> Result<Option<Retained>, Error> {
        let _guard = self.try_guard()?;
        self.read_guarded(identity)
    }
    /// Only actual policy use, acquisition or confirmed publication may finish
    /// an already approved adoption. Inspection must continue to use `read`.
    pub(in crate::review) fn recover(&self, identity: &str) -> Result<(), Error> {
        self.initialize()?;
        if self
            .mode(identity)?
            .is_some_and(|mode| mode.approved_initial.is_some())
        {
            let _lease = objects::Directory::try_enter(&self.directory)?;
            self.recover_locked(identity)?;
        }
        Ok(())
    }
    fn recover_locked(&self, identity: &str) -> Result<(), Error> {
        let Some(mut mode) = self.mode(identity)? else {
            return Ok(());
        };
        let Some(target) = mode.approved_initial.as_ref() else {
            return Ok(());
        };
        // Never rediscover an update or delegate a different key on recovery.
        // The retained target is the exact confirmed generation-one intent.
        let expected_intent =
            crate::json::fingerprint(&json!([identity, Value::Null, Value::Null, &target.object]))
                .map_err(|_| Error::Invalid("registry trust intent encoding"))?;
        if target.generation != 1
            || target.intent != expected_intent
            || target.source != mode.source
        {
            return Err(Error::Invalid("invalid approved registry trust target"));
        }
        load(&self.directory, target)?;
        if let Some(current) = self.head_raw(identity)? {
            if json!(current) != json!(target) {
                return Err(Error::StaleSnapshot);
            }
        } else {
            private_files::write(&self.head_path(identity), &json!(target), HEAD_LIMIT)?;
        }
        // A failed or unconfirmed clear must stop every higher-head publication.
        // The immutable approved target remains available for the next retry.
        mode.approved_initial = None;
        private_files::write(&self.mode_path(identity), &json!(mode), HEAD_LIMIT)
    }
    fn head_path(&self, identity: &str) -> PathBuf {
        self.directory
            .join(format!("{}.head.json", source_key(identity)))
    }
    fn mode_path(&self, identity: &str) -> PathBuf {
        self.directory
            .join(format!("{}.mode.json", source_key(identity)))
    }
    pub(in crate::review) fn read_guarded(
        &self,
        identity: &str,
    ) -> Result<Option<Retained>, Error> {
        let Some(head) = self.head(identity)? else {
            return Ok(None);
        };
        memo(&self.directory, &head).map(Some)
    }
    fn head(&self, identity: &str) -> Result<Option<Head>, Error> {
        let head = self.head_raw(identity)?;
        let mode = self.mode(identity)?;
        if mode
            .as_ref()
            .is_some_and(|mode| mode.approved_initial.is_some())
        {
            return Err(Error::Invalid(
                "approved registry trust adoption requires recovery",
            ));
        }
        if head.is_none() && mode.is_some() {
            return Err(Error::Invalid("adopted registry trust head unavailable"));
        }
        Ok(head)
    }
    fn mode(&self, identity: &str) -> Result<Option<Mode>, Error> {
        let path = self.mode_path(identity);
        let value = match private_files::read(&path, HEAD_LIMIT) {
            Ok(value) => value,
            Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(None);
            }
            Err(error) => return Err(error),
        };
        let mode: Mode = serde_json::from_value(value)
            .map_err(|_| Error::Invalid("invalid registry trust mode"))?;
        if mode.schema_version != 1
            || mode.kind != "versioned-v1"
            || mode.source.identity() != identity
            || mode
                .approved_initial
                .as_ref()
                .is_some_and(|head| !valid_head(head, identity))
        {
            return Err(Error::Invalid("invalid registry trust mode"));
        }
        Ok(Some(mode))
    }
    fn head_raw(&self, identity: &str) -> Result<Option<Head>, Error> {
        let path = self.head_path(identity);
        let value = match private_files::read(&path, HEAD_LIMIT) {
            Ok(value) => value,
            Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(None);
            }
            Err(error) => return Err(error),
        };
        let head: Head = serde_json::from_value(value)
            .map_err(|_| Error::Invalid("invalid registry trust head"))?;
        if !valid_head(&head, identity) {
            return Err(Error::Invalid("invalid registry trust head"));
        }
        Ok(Some(head))
    }
    /// An authenticated update changes global security knowledge only. CAS and
    /// publication hold no project writer; callers update project refs afterward.
    pub(in crate::review) fn publish(
        &self,
        update: &VerifiedUpdate,
        expected: Option<&Retained>,
    ) -> Result<Published, Error> {
        self.initialize()?;
        let source = update.root().source();
        let (mut next, value) = captured_head(update, expected)?;
        let _lease = objects::Directory::try_enter(&self.directory)?;
        self.recover_locked(source.identity())?;
        let current = self.head_raw(source.identity())?;
        if current.is_none() && self.mode(source.identity())?.is_some() {
            return Err(Error::Invalid("adopted registry trust head unavailable"));
        }
        if current.is_some() && self.mode(source.identity())?.is_none() {
            return Err(Error::Invalid("adopted registry trust mode unavailable"));
        }
        if let Some(current) = &current {
            let same = current.revision == next.revision && current.object == next.object;
            if same
                && (current.intent == next.intent
                    || expected.is_some_and(|expected| {
                        expected.generation == current.generation
                            && expected.object == current.object
                    }))
            {
                return Ok(Published {
                    generation: current.generation,
                    revision: current.revision.clone(),
                    object: current.object.clone(),
                    replayed: true,
                    durable: false,
                    repair_needed: false,
                });
            }
        }
        if current.as_ref().map(|head| head.revision.as_str()) != update.expected_head_revision()
            || current
                .as_ref()
                .map(|head| (head.generation, head.object.as_str()))
                != expected.map(|head| (head.generation, head.object.as_str()))
        {
            return Err(Error::StaleSnapshot);
        }
        next.generation = current
            .as_ref()
            .map_or(Some(1), |head| head.generation.checked_add(1))
            .ok_or(crate::policy::Error::Limit)?;
        // Reclaim only provably unreferenced objects before admission. Missing
        // adopted heads preserve their entire namespace for repair instead.
        let _ = self.collect();
        self.admit(&next)?;
        let object_path = self.directory.join(format!("{}.object.json", next.object));
        if object_path.exists() {
            if private_files::read(&object_path, OBJECT_LIMIT)? != value {
                return Err(Error::Invalid("registry trust object replaced"));
            }
        } else {
            private_files::write(&object_path, &value, OBJECT_LIMIT)?;
        }
        if let Some(mode) = self.mode(source.identity())? {
            if mode.source != *source {
                return Err(Error::Invalid("registry trust anchor changed"));
            }
        } else {
            private_files::write(
                &self
                    .directory
                    .join(format!("{}.mode.json", source_key(source.identity()))),
                &json!(Mode {
                    schema_version: 1,
                    kind: "versioned-v1".into(),
                    source: source.clone(),
                    approved_initial: Some(next.clone())
                }),
                HEAD_LIMIT,
            )?;
            #[cfg(test)]
            if let Some(staged) = INITIAL_ADOPTION_STAGED.with(|slot| slot.borrow_mut().take()) {
                staged();
            }
        }
        let written = private_files::write(
            &self
                .directory
                .join(format!("{}.head.json", source_key(source.identity()))),
            &json!(next),
            HEAD_LIMIT,
        );
        let mut durable = written.is_ok();
        if let Err(error) = written
            && !self
                .head_raw(source.identity())?
                .is_some_and(|head| head.revision == next.revision && head.object == next.object)
        {
            return Err(error);
        }
        // The selected head is committed even if clearing its recovery marker
        // has unconfirmed durability. Keep the marker and report repair needed.
        durable &= self.recover_locked(source.identity()).is_ok();
        let repair_needed = self.collect().is_err() || !durable;
        Ok(Published {
            generation: next.generation,
            revision: next.revision,
            object: next.object,
            replayed: false,
            durable,
            repair_needed,
        })
    }
    fn entries(&self) -> Result<Vec<PathBuf>, Error> {
        let entries: Vec<_> = fs::read_dir(&self.directory)?
            .take(MAX_FILES + 1)
            .map(|row| row.map(|entry| entry.path()))
            .collect::<Result<_, _>>()?;
        if entries.len() > MAX_FILES {
            return Err(crate::policy::Error::Limit.into());
        }
        Ok(entries)
    }
    fn heads(&self) -> Result<Vec<Head>, Error> {
        let mut heads = Vec::new();
        for path in self.entries()? {
            if path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with(".head.json"))
            {
                let value = private_files::read(&path, HEAD_LIMIT)?;
                let head: Head = serde_json::from_value(value)
                    .map_err(|_| Error::Invalid("invalid registry trust head"))?;
                if path.file_name().and_then(|name| name.to_str())
                    != Some(format!("{}.head.json", source_key(head.source.identity())).as_str())
                {
                    return Err(Error::Invalid("registry trust head identity differs"));
                }
                self.head(head.source.identity())?
                    .ok_or(Error::Invalid("registry trust head missing"))?;
                heads.push(head);
            }
        }
        if heads.len() > MAX_SOURCES {
            return Err(crate::policy::Error::Limit.into());
        }
        Ok(heads)
    }
    fn admit(&self, next: &Head) -> Result<(), Error> {
        let modes = self
            .entries()?
            .iter()
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.ends_with(".mode.json"))
            })
            .count();
        if modes >= MAX_SOURCES && self.mode(next.source.identity())?.is_none() {
            return Err(crate::policy::Error::Limit.into());
        }
        let mut physical = 0u64;
        let next_name = format!("{}.object.json", next.object);
        let mut present = false;
        for path in self.entries()? {
            if let Some(name) = path
                .file_name()
                .and_then(|name| name.to_str())
                .filter(|name| name.ends_with(".object.json"))
            {
                present |= name == next_name;
                physical = physical
                    .checked_add(fs::symlink_metadata(&path)?.len())
                    .ok_or(crate::policy::Error::Limit)?;
            }
        }
        if !present {
            physical = physical
                .checked_add(next.bytes as u64)
                .ok_or(crate::policy::Error::Limit)?;
        }
        if physical > STORAGE_BYTES + OBJECT_LIMIT as u64 {
            return Err(crate::policy::Error::Limit.into());
        }
        let retired = self
            .head_raw(next.source.identity())?
            .filter(|head| head.object != next.object)
            .map_or(0, |head| head.bytes as u64);
        if physical.saturating_sub(retired) > STORAGE_BYTES {
            return Err(crate::policy::Error::Limit.into());
        }
        Ok(())
    }
    fn collect(&self) -> Result<(), Error> {
        let heads = self.heads()?;
        let sources: BTreeSet<_> = heads
            .iter()
            .map(|head| source_key(head.source.identity()))
            .collect();
        for path in self.entries()? {
            if let Some(name) = path
                .file_name()
                .and_then(|name| name.to_str())
                .and_then(|name| name.strip_suffix(".mode.json"))
                && !sources.contains(name)
            {
                return Err(Error::Invalid(
                    "adopted registry trust head unavailable; security objects preserved",
                ));
            }
        }
        let live: BTreeSet<_> = heads
            .into_iter()
            .map(|head| format!("{}.object.json", head.object))
            .collect();
        for path in self.entries()? {
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if name.strip_suffix(".object.json").is_some_and(hex) && !live.contains(name) {
                drop(objects::read_file(&path)?);
                fs::remove_file(path)?;
            }
        }
        Ok(())
    }
}

/// Encode only authenticated retained values before taking the publication lock.
fn captured_head(
    update: &VerifiedUpdate,
    expected: Option<&Retained>,
) -> Result<(Head, Value), Error> {
    let source = update.root().source();
    let root_checkpoint = String::from_utf8(update.root().checkpoint().map_err(trust_error)?)
        .map_err(|_| Error::Invalid("registry trust encoding"))?;
    let (payload, signatures) = update.revocations().signed_bytes();
    let checkpoints: Value = json!(update.checkpoints());
    let value = json!(Object {
        schema_version: 1,
        root_checkpoint,
        revocation_payload: std::str::from_utf8(payload)
            .map_err(|_| Error::Invalid("registry trust encoding"))?
            .into(),
        revocation_signatures: std::str::from_utf8(signatures)
            .map_err(|_| Error::Invalid("registry trust encoding"))?
            .into(),
        catalog_checkpoints: json!({source.identity():checkpoints[source.identity()]})
    });
    let bytes =
        serde_json::to_vec(&value).map_err(|_| Error::Invalid("registry trust encoding"))?;
    if bytes.len() > OBJECT_LIMIT {
        return Err(crate::policy::Error::Limit.into());
    }
    let object = crate::policy::fingerprint(&bytes);
    let intent = crate::json::fingerprint(&json!([
        source.identity(),
        expected.map(|head| head.generation),
        expected.map(|head| &head.object),
        &object
    ]))
    .map_err(|_| Error::Invalid("registry trust intent encoding"))?;
    Ok((
        Head {
            schema_version: 1,
            generation: 0,
            intent,
            source: source.clone(),
            revision: update.head_revision(),
            object,
            bytes: bytes.len(),
            root_version: update.root().version(),
            revocation_sequence: update.revocations().sequence(),
        },
        value,
    ))
}

fn load(directory: &Path, head: &Head) -> Result<Loaded, Error> {
    let path = directory.join(format!("{}.object.json", head.object));
    if objects::read_file(&path)?.metadata()?.len() != head.bytes as u64 {
        return Err(Error::Invalid("registry trust object size differs"));
    }
    let value = private_files::read(&path, OBJECT_LIMIT)?;
    if crate::json::fingerprint(&value).map_err(|_| Error::Invalid("registry trust encoding"))?
        != head.object
    {
        return Err(Error::Invalid("registry trust object digest differs"));
    }
    let object: Object = serde_json::from_value(value)
        .map_err(|_| Error::Invalid("invalid registry trust object"))?;
    if object.schema_version != 1 {
        return Err(Error::Invalid("invalid registry trust object"));
    }
    let root = TrustedRoot::restore_checkpoint(&head.source, object.root_checkpoint.as_bytes())
        .map_err(trust_error)?;
    let revocations = Revocations::restore_checkpoint(
        &root,
        object.revocation_payload.as_bytes(),
        object.revocation_signatures.as_bytes(),
    )
    .map_err(trust_error)?;
    let checkpoints: Checkpoints = serde_json::from_value(object.catalog_checkpoints)
        .map_err(|_| Error::Invalid("invalid registry trust catalog checkpoint"))?;
    let root_revision = root.revision();
    let revocation_revision = revocations.revision();
    if checkpoints.len() != 1
        || checkpoints.get(head.source.identity()).is_none()
        || root.version() != head.root_version
        || revocations.sequence() != head.revocation_sequence
        || crate::policy::fingerprint(format!("{root_revision}:{revocation_revision}").as_bytes())
            != head.revision
    {
        return Err(Error::Invalid("registry trust head binding differs"));
    }
    Ok(Loaded {
        root,
        revocations,
        checkpoints,
        revision: head.revision.clone(),
        root_revision,
        revocation_revision,
        object: head.object.clone(),
    })
}

fn memo(directory: &Path, head: &Head) -> Result<Retained, Error> {
    if objects::read_file(&directory.join(format!("{}.object.json", head.object)))?
        .metadata()?
        .len()
        != head.bytes as u64
    {
        return Err(Error::Invalid(
            "registry trust object unavailable or size differs",
        ));
    }
    let key = directory.join(&head.object);
    let slot = {
        let mut memo = MEMO
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(index) = memo.iter().position(|(path, _)| path == &key) {
            let row = memo
                .remove(index)
                .ok_or(Error::Invalid("registry trust memo missing"))?;
            let slot = row.1.clone();
            memo.push_back(row);
            slot
        } else {
            let cost = head
                .bytes
                .checked_mul(8)
                .ok_or(crate::policy::Error::Limit)?;
            while memo.len() >= MEMO_ENTRIES
                || memo
                    .iter()
                    .map(|(_, slot)| slot.cost)
                    .sum::<usize>()
                    .saturating_add(cost)
                    > MEMO_BYTES
            {
                let index = memo
                    .iter()
                    .position(|(_, slot)| Arc::strong_count(slot) == 1)
                    .ok_or(crate::policy::Error::Limit)?;
                memo.remove(index);
            }
            let slot = Arc::new(Slot {
                cost,
                value: OnceLock::new(),
            });
            memo.push_back((key, slot.clone()));
            slot
        }
    };
    let loaded = if let Ok(loaded) = slot
        .value
        .get_or_init(|| {
            load(directory, head)
                .map(Arc::new)
                .map_err(|error| error.to_string())
        })
        .as_ref()
    {
        loaded.clone()
    } else {
        MEMO.lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .retain(|(_, retained)| !Arc::ptr_eq(retained, &slot));
        return Err(Error::Invalid(
            "registry trust metadata unavailable or invalid",
        ));
    };
    if loaded.root.source() != &head.source
        || loaded.revision != head.revision
        || loaded.root.version() != head.root_version
        || loaded.revocations.sequence() != head.revocation_sequence
    {
        return Err(Error::Invalid("registry trust memo binding differs"));
    }
    Ok(Retained {
        generation: head.generation,
        _slot: slot,
        loaded,
    })
}
fn source_key(identity: &str) -> String {
    crate::policy::fingerprint(identity.as_bytes())
}
fn valid_head(head: &Head, identity: &str) -> bool {
    head.schema_version == 1
        && head.generation != 0
        && hex(&head.intent)
        && head.source.identity() == identity
        && hex(&head.object)
        && hex(&head.revision)
        && head.bytes != 0
        && head.bytes <= OBJECT_LIMIT
        && head.root_version != 0
        && head.revocation_sequence != 0
}
fn hex(value: &str) -> bool {
    canonic_registry_kit::digest::is_sha256_hex(value)
}
pub(in crate::review) fn trust_error(_: canonic_registry_kit::trust::TrustError) -> Error {
    Error::Invalid("versioned registry trust refused")
}
