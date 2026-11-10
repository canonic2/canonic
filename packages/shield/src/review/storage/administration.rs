//! Private current administration receipts; copied checkpoints cannot mint consent.
use super::{Error, State, Store, incarnation, objects, portability, private_files};
use ring::rand::{SecureRandom as _, SystemRandom};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::PathBuf;

const LIMIT: usize = 512 * 1024;

#[derive(Clone, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Record {
    schema_version: u8,
    token: String,
    project: String,
    lineage: String,
    administration: Value,
    committed: bool,
    previous: Option<Value>,
}

/// Request-local proof, never deserialized from a checkpoint or tool input.
pub(in crate::review) struct CopyProof {
    origin: String,
    record: Record,
    target: Value,
}

fn directory(create: bool) -> Result<PathBuf, Error> {
    #[cfg(not(test))]
    let parent = portability::directory()?;
    #[cfg(test)]
    let parent = if let Some(path) = portability::consent::fixture_directory() {
        // Optional consent failure injection must not erase the mandatory
        // administration store as a side effect. Both still share host scope.
        path.with_extension("administration")
    } else {
        let executable = std::env::current_exe()?;
        std::env::temp_dir().canonicalize()?.join(format!(
            "shield-administration-{}",
            crate::policy::fingerprint(executable.to_string_lossy().as_bytes())
        ))
    };
    if create && !parent.exists() {
        if let Some(base) = parent.parent() {
            super::private_directory(base)?;
        }
        private_files::directory(&parent, true)?;
    }
    private_files::directory(&parent, false)?;
    let path = parent.join("administration");
    private_files::directory(&path, create)?;
    Ok(path)
}

fn digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn administration(state: &State) -> Value {
    json!({"enabled":state.enforcement.enabled,"path_access":state.operator["path_access"],
        "integration":state.operator["integration"],"pending":!state.operator["integration_pending"].is_null()})
}

fn read(directory: &std::path::Path, origin: &str) -> Result<Record, Error> {
    if !digest(origin) {
        return Err(Error::Invalid("invalid administration origin"));
    }
    let current = origin_directory(directory, origin, false)
        .and_then(|path| private_files::read(&path.join("state.json"), LIMIT));
    let value = match current {
        Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
            private_files::read(&directory.join(format!("{origin}.json")), LIMIT)?
        }
        result => result?,
    };
    let record: Record = serde_json::from_value(value)
        .map_err(|_| Error::Invalid("invalid administration receipt"))?;
    if record.schema_version != 1
        || !digest(&record.token)
        || !digest(&record.project)
        || !digest(&record.lineage)
    {
        return Err(Error::Invalid("invalid administration receipt"));
    }
    Ok(record)
}

fn origin_directory(
    directory: &std::path::Path,
    origin: &str,
    create: bool,
) -> Result<PathBuf, Error> {
    if !digest(origin) {
        return Err(Error::Invalid("invalid administration origin"));
    }
    let origins = directory.join("origins");
    private_files::directory(&origins, create)?;
    let target = origins.join(origin);
    private_files::directory(&target, create)?;
    Ok(target)
}

fn write(directory: &std::path::Path, origin: &str, record: &Record) -> Result<(), Error> {
    let target = origin_directory(directory, origin, true)?;
    private_files::write(&target.join("state.json"), &json!(record), LIMIT)
}

fn reference(state: &State) -> Option<(&str, &str)> {
    let value = &state.operator["administration_origin"];
    if value["schema_version"] != 1 {
        return None;
    }
    Some((value["origin"].as_str()?, value["token"].as_str()?))
}

/// Exact private matching only. Unavailable evidence leaves safe copied defaults.
pub(super) fn copied(state: &State) -> Option<CopyProof> {
    let (origin, token) = reference(state)?;
    let directory = directory(false).ok()?;
    let record = read(&directory, origin).ok()?;
    if record.token != token
        || record.project != state.project_id
        || record.lineage != state.policy_lineage()
        || record.administration != administration(state)
        || record.administration["pending"] != false
        || !record.committed
    {
        return None;
    }
    if let Ok(Some(registered)) = crate::review::integrations::registration(&state.operator)
        && registered.interceptor.is_some()
    {
        return None;
    }
    Some(CopyProof {
        origin: origin.into(),
        record,
        target: Value::Null,
    })
}

impl CopyProof {
    pub(super) fn restore(self, state: &mut State) -> Self {
        state.operator["policy_lineage"] = json!(self.record.lineage);
        state
            .operator
            .as_object_mut()
            .expect("operator object")
            .remove("portable_lineage_unverified");
        state.enforcement.enabled = self.record.administration["enabled"]
            .as_bool()
            .unwrap_or(true);
        for field in ["integration", "path_access"] {
            state.operator[field] = self.record.administration[field].clone();
        }
        state
            .operator
            .as_object_mut()
            .expect("operator object")
            .remove("portable_admin_unverified");
        self
    }
    pub(super) fn bind(mut self, state: &State) -> Self {
        self.target = administration(state);
        self
    }
}

/// Called under the project writer. Private publication precedes local authority:
/// an interrupted change invalidates old copies, never revives an older token.
pub(super) fn publish(store: &Store, state: &mut State) -> Result<bool, Error> {
    let observed = incarnation::observe(store, false)?;
    if !incarnation::matches(state, &observed) {
        return Err(Error::StaleSnapshot);
    }
    let directory = directory(true)?;
    // Ordinary grant/history writes retain the existing receipt without a
    // cross-project publication lock. The project writer excludes its issuer.
    if state.administration_copy.is_none()
        && let Some((origin, token)) = reference(state)
        && origin == observed
        && let Ok(record) = read(&directory, origin)
        && record.token == token
        && record.project == state.project_id
        && record.lineage == state.policy_lineage()
        && record.administration == administration(state)
        && record.committed
    {
        return Ok(false);
    }
    let _guard = objects::Directory::enter(&directory)?;
    let value = administration(state);
    if let Some(proof) = &state.administration_copy
        && (proof.target != value || read(&directory, &proof.origin)? != proof.record)
    {
        return Err(Error::StaleSnapshot);
    }
    let current = match read(&directory, &observed) {
        Ok(record) => Some(record),
        Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error),
    };
    let previous = current.as_ref().map(|record| {
        if !record.committed
            && let Some(previous) = &record.previous
            && reference(state).is_some_and(|(_, token)| previous["token"] == token)
        {
            previous.clone()
        } else {
            json!({"token":record.token,
                "project":record.project,"lineage":record.lineage,"administration":record.administration})
        }
    });
    let record = if let Some(record) = current.filter(|record| {
        record.project == state.project_id
            && record.lineage == state.policy_lineage()
            && record.administration == value
    }) {
        record
    } else {
        let mut bytes = [0u8; 32];
        SystemRandom::new()
            .fill(&mut bytes)
            .map_err(|_| Error::Invalid("administration entropy unavailable"))?;
        let record = Record {
            schema_version: 1,
            token: crate::policy::fingerprint(&bytes),
            project: state.project_id.clone(),
            lineage: state.policy_lineage().into(),
            administration: value,
            committed: false,
            previous,
        };
        write(&directory, &observed, &record)?;
        record
    };
    state.operator["administration_origin"] =
        json!({"schema_version":1,"origin":observed,"token":record.token});
    state.administration_copy = None;
    Ok(!record.committed)
}

/// Complete only the generation present in the durable local checkpoint.
pub(super) fn finish(state: &State) -> Result<(), Error> {
    let (origin, token) = reference(state).ok_or(Error::StaleSnapshot)?;
    let directory = directory(false)?;
    let _guard = objects::Directory::enter(&directory)?;
    let mut record = read(&directory, origin)?;
    if record.token != token
        || record.project != state.project_id
        || record.lineage != state.policy_lineage()
        || record.administration != administration(state)
    {
        return Err(Error::StaleSnapshot);
    }
    if !record.committed {
        record.committed = true;
        record.previous = None;
        write(&directory, origin, &record)?;
    }
    Ok(())
}

/// Upgrade a proven local installation without permitting copy-driven seeding.
pub(super) fn seed(store: &Store, state: State) -> Result<State, Error> {
    Store::validate_checkpoint(&state)?;
    let owned = *store
        .writer_owned
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        == Some(std::thread::current().id());
    let observed = incarnation::observe(store, false)?;
    if !incarnation::matches(&state, &observed) {
        return Err(Error::StaleSnapshot);
    }
    let record = directory(false).and_then(|directory| read(&directory, &observed));
    if let Ok(record) = &record {
        let exact = reference(&state)
            .is_some_and(|(origin, token)| origin == observed && token == record.token)
            && record.project == state.project_id
            && record.lineage == state.policy_lineage()
            && record.administration == administration(&state);
        if exact && record.committed {
            return Ok(state);
        }
        let previous = reference(&state).is_some_and(|(origin, token)| {
            origin == observed
                && record.previous.as_ref().is_some_and(|previous| {
                    previous["token"] == token
                        && previous["project"] == state.project_id
                        && previous["lineage"] == state.policy_lineage()
                        && previous["administration"] == administration(&state)
                })
        });
        if record.committed || (!exact && !previous) {
            return Err(Error::Invalid(
                "administration receipt is stale; retained checkpoint unchanged",
            ));
        }
    } else if reference(&state).is_some() {
        return Err(Error::Invalid(
            "administration receipt unavailable; retained checkpoint unchanged",
        ));
    }
    if owned {
        return Err(Error::StaleSnapshot);
    }
    let lock = store.lock(&|| Ok(()))?;
    let mut current = super::upgrades::checkpoint(&store.checkpoint_bytes()?)?;
    if current.operator["administration_origin"] != state.operator["administration_origin"]
        || administration(&current) != administration(&state)
    {
        return Err(Error::StaleSnapshot);
    }
    if !incarnation::matches(&current, &observed) {
        return Err(Error::StaleSnapshot);
    }
    store.write(&mut current)?;
    lock.release()?;
    Ok(current)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (assert_fs::TempDir, crate::review::Context) {
        let root = assert_fs::TempDir::new().unwrap();
        let bundle =
            crate::configuration::Bundle::capture("schema_version: 1\npacks: []\n", &[], None)
                .unwrap();
        let context = crate::review::mcp::initialize_fixture(root.path(), &bundle);
        (root, context)
    }

    fn copy(root: &std::path::Path) -> assert_fs::TempDir {
        let target = assert_fs::TempDir::new().unwrap();
        crate::test_support::copy_fixture_directory(root, target.path());
        target
    }

    fn mode(root: &std::path::Path, enabled: bool) {
        let context = crate::review::open(root).unwrap();
        Store::open(root)
            .unwrap()
            .update_checkpoint(&context, &|| Ok(()), |state| {
                state.enforcement.enabled = enabled;
                Ok(Value::Null)
            })
            .unwrap();
    }

    #[test]
    fn administrative_origins_do_not_share_a_lifetime_entry_limit() {
        portability::consent::fixture_namespace("administrative-capacity");
        let (root, _) = fixture();
        let directory = directory(false).unwrap();
        for index in 0..4096 {
            std::fs::write(directory.join(format!("unrelated-{index}")), b"").unwrap();
        }
        let (next, _) = fixture();
        mode(next.path(), false);
        let state = Store::open(next.path()).unwrap().read_checkpoint().unwrap();
        assert!(!state.enforcement.enabled);
        let (origin, _) = reference(&state).unwrap();
        assert!(
            directory
                .join("origins")
                .join(origin)
                .join("state.json")
                .is_file()
        );
        assert!(Store::open(root.path()).unwrap().read_checkpoint().is_ok());
        for index in 0..4096 {
            std::fs::remove_file(directory.join(format!("unrelated-{index}"))).unwrap();
        }
    }

    #[test]
    fn administrative_flat_receipt_is_readable_and_updates_into_its_origin_partition() {
        portability::consent::fixture_namespace("administrative-flat-upgrade");
        let (root, _) = fixture();
        let state = Store::open(root.path()).unwrap().read_checkpoint().unwrap();
        let directory = directory(false).unwrap();
        let (origin, _) = reference(&state).unwrap();
        let path = directory.join("origins").join(origin).join("state.json");
        let old = read(&directory, origin).unwrap();
        private_files::write(
            &directory.join(format!("{origin}.json")),
            &json!(old),
            LIMIT,
        )
        .unwrap();
        std::fs::remove_file(&path).unwrap();
        assert!(read(&directory, origin).unwrap() == old);
        mode(root.path(), false);
        assert!(path.is_file());
        assert!(
            !read(&directory, origin).unwrap().administration["enabled"]
                .as_bool()
                .unwrap()
        );
    }

    #[test]
    fn administrative_copy_forks_and_source_changes_do_not_revoke_admitted_copy() {
        let (root, _) = fixture();
        mode(root.path(), false);
        let before = Store::open(root.path()).unwrap().read_checkpoint().unwrap();
        let target = copy(root.path());
        crate::review::open(target.path()).unwrap();
        let child = Store::open(target.path())
            .unwrap()
            .read_checkpoint()
            .unwrap();
        assert!(!child.enforcement.enabled);
        assert_ne!(reference(&child), reference(&before));
        mode(root.path(), true);
        assert!(
            !Store::open(target.path())
                .unwrap()
                .read_checkpoint()
                .unwrap()
                .enforcement
                .enabled
        );
        let grandchild = copy(target.path());
        crate::review::open(grandchild.path()).unwrap();
        assert!(
            !Store::open(grandchild.path())
                .unwrap()
                .read_checkpoint()
                .unwrap()
                .enforcement
                .enabled
        );
        assert!(
            Store::open(root.path())
                .unwrap()
                .read_checkpoint()
                .unwrap()
                .enforcement
                .enabled
        );
    }

    #[test]
    fn administrative_copy_cannot_replay_a_revoked_snapshot() {
        let (root, _) = fixture();
        mode(root.path(), false);
        let stale = copy(root.path());
        mode(root.path(), true);
        crate::review::open(stale.path()).unwrap();
        let loaded = Store::open(stale.path())
            .unwrap()
            .read_checkpoint()
            .unwrap();
        assert!(loaded.enforcement.enabled);
        assert_eq!(loaded.operator["portable_admin_unverified"], true);
    }

    #[test]
    fn administrative_committed_generation_refuses_native_checkpoint_rollback() {
        let (root, context) = fixture();
        mode(root.path(), false);
        let store = Store::open(root.path()).unwrap();
        let old = store.checkpoint_bytes().unwrap();
        mode(root.path(), true);
        std::fs::write(store.directory.join("state.json"), &old).unwrap();
        assert!(
            store
                .update_checkpoint(&context, &|| Ok(()), |_| Ok(Value::Null))
                .is_err()
        );
        assert!(store.read_checkpoint().is_err());
        assert_eq!(store.checkpoint_bytes().unwrap(), old);
        assert!(store.read_diagnostic_checkpoint().is_ok());
        let target = assert_fs::TempDir::new().unwrap();
        let renamed = target.path().join("renamed");
        std::fs::rename(root.path(), &renamed).unwrap();
        let moved = Store::open(&renamed).unwrap();
        assert!(moved.read_checkpoint().is_err());
        assert_eq!(moved.checkpoint_bytes().unwrap(), old);
    }

    #[test]
    fn administrative_private_publication_interruption_invalidates_old_receipt_and_recovers_locally()
     {
        let (root, _) = fixture();
        mode(root.path(), false);
        let stale = copy(root.path());
        let store = Store::open(root.path()).unwrap();
        let bytes = store.checkpoint_bytes().unwrap();
        let mut desired = store.read_checkpoint().unwrap();
        desired.enforcement.enabled = true;
        let lock = store.lock(&|| Ok(())).unwrap();
        publish(&store, &mut desired).unwrap();
        lock.release().unwrap();
        // Simulate process loss between private invalidation and checkpoint commit.
        assert_eq!(store.checkpoint_bytes().unwrap(), bytes);
        crate::review::open(stale.path()).unwrap();
        assert!(
            Store::open(stale.path())
                .unwrap()
                .read_checkpoint()
                .unwrap()
                .enforcement
                .enabled
        );
        // Recovery itself can lose the process before committing its checkpoint.
        // Keep the actual durable predecessor through both interruptions.
        let mut recovering = super::super::upgrades::checkpoint(&bytes).unwrap();
        let lock = store.lock(&|| Ok(())).unwrap();
        publish(&store, &mut recovering).unwrap();
        lock.release().unwrap();
        assert_eq!(store.checkpoint_bytes().unwrap(), bytes);
        let recovered = store.read_checkpoint().unwrap();
        assert!(!recovered.enforcement.enabled);
        assert_ne!(reference(&recovered).unwrap().1,
            serde_json::from_slice::<Value>(&bytes).unwrap()["operator"]["administration_origin"]["token"].as_str().unwrap());
    }
}
