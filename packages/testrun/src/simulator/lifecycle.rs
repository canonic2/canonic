//! Retained and journaled simulator boots, independent of the requesting future.
use super::device_set::DeviceSet;
use crate::{
    error::{Error, Result},
    lease::ControlLease,
    managed::storage,
    targets::{Discovery, Target},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::sync::watch;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Record {
    target_id: String,
    uuid: String,
    device_set: Option<PathBuf>,
    device_set_identity: String,
    phase: String,
    epoch: String,
    /// When `simctl boot` was sent, in Unix milliseconds: a boot whose outcome
    /// was uncertain is ours when the simulator's boot time follows it.
    #[serde(default)]
    dispatched_at_ms: Option<u64>,
}
/// How long after dispatch a boot can still appear, and how long a Shutdown
/// observation must follow dispatch before it proves no boot happened.
const BOOT_WINDOW_MS: u64 = 120_000;
/// Clock difference tolerated between Testrun and CoreSimulator boot times.
const BOOT_SKEW_MS: u64 = 5_000;

/// What cleanup does with a boot whose command was sent but whose outcome
/// was never recorded.
#[derive(Debug, PartialEq, Eq)]
enum Dispatched {
    /// The simulator booted within the window after dispatch: shut it down.
    Ours,
    /// It is shut down, or another boot outside the window runs it: leave it.
    NotOurs,
    /// The boot may still be settling: retry later.
    Settling,
    /// The record has no dispatch time: ownership cannot be decided.
    Unknown,
}
fn dispatched(at: Option<u64>, state: &str, epoch: &str, now: u64) -> Dispatched {
    let Some(at) = at else {
        return Dispatched::Unknown;
    };
    match state {
        "Shutdown" if now.saturating_sub(at) >= BOOT_WINDOW_MS => Dispatched::NotOurs,
        "Booted" => match boot_time_ms(epoch) {
            Some(booted) if booted + BOOT_SKEW_MS >= at && booted <= at + BOOT_WINDOW_MS => {
                Dispatched::Ours
            }
            Some(_) => Dispatched::NotOurs,
            None => Dispatched::Unknown,
        },
        _ => Dispatched::Settling,
    }
}
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| {
            u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
        })
}
struct Boot {
    ready: watch::Receiver<Option<Result<Target>>>,
    journal: PathBuf,
    _lease: Arc<ControlLease>,
    armed: bool,
}
impl Drop for Boot {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let ready = self.ready.clone();
        let path = self.journal.clone();
        let lease = self._lease.clone();
        runtime.spawn(async move {
            let _lease = lease;
            let _ = settled(ready).await;
            let result = async {
                let mut record = storage::read::<Record>(&path)?.ok_or_else(uncertain)?;
                close_record(&mut record, &path).await
            }
            .await;
            if let Err(error) = result {
                eprintln!("Simulator boot cleanup remains pending: {}", error.code);
            }
        });
    }
}
#[derive(Default)]
pub(crate) struct Boots {
    entries: HashMap<String, Boot>,
}
pub(crate) fn journal(directory: &Path, target: &str) -> PathBuf {
    directory.join(format!(
        "simulator-boot-{:x}.json",
        Sha256::digest(target.as_bytes())
    ))
}
impl Boots {
    pub(crate) fn contains(&self, target: &str) -> bool {
        self.entries.contains_key(target)
    }
    pub(crate) async fn start(
        &mut self,
        target: &Target,
        lease: ControlLease,
        directory: &Path,
    ) -> Result<Target> {
        let key = target.identity.target_id.clone();
        if self.entries.contains_key(&key) {
            return Err(Error::new(
                "TARGET_BUSY",
                "Simulator startup or cleanup is already owned",
            ));
        }
        if self.entries.len() >= 64 {
            return Err(Error::new(
                "RESOURCE_LIMIT",
                "Simulator boot ownership capacity reached; finish pending cleanup",
            ));
        }
        let set = DeviceSet::current()?;
        let path = journal(directory, &key);
        let record = Record {
            target_id: key.clone(),
            uuid: target.platform_id.clone(),
            device_set: set.path.clone(),
            device_set_identity: set.identity.clone(),
            phase: "prepared".into(),
            epoch: target.epoch.clone(),
            dispatched_at_ms: None,
        };
        storage::write(&path, &record)?;
        let lease = Arc::new(lease);
        let (ready, receiver) = watch::channel(None);
        self.entries.insert(
            key,
            Boot {
                ready: receiver.clone(),
                journal: path.clone(),
                _lease: lease.clone(),
                armed: true,
            },
        );
        let target = target.clone();
        tokio::spawn(async move {
            let _lease = lease;
            ready.send_replace(Some(boot(&target, &set, record, &path).await));
        });
        settled(receiver).await
    }
    pub(crate) async fn release(&mut self, target: &str) -> Result<()> {
        let Some(entry) = self.entries.get(target) else {
            return Ok(());
        };
        // The durable phase decides cleanup safety even when startup failed.
        let _ = settled(entry.ready.clone()).await;
        let mut record: Record = storage::read(&entry.journal)?.ok_or_else(uncertain)?;
        close_record(&mut record, &entry.journal).await?;
        self.entries.get_mut(target).unwrap().armed = false;
        self.entries.remove(target);
        Ok(())
    }
    pub(crate) async fn release_unused(&mut self, active: &HashSet<String>) -> Result<()> {
        let keys: Vec<_> = self
            .entries
            .keys()
            .filter(|key| !active.contains(*key))
            .cloned()
            .collect();
        let mut failures = Vec::new();
        for key in keys {
            if let Err(error) = self.release(&key).await {
                failures.push(error);
            }
        }
        if failures.is_empty() {
            Ok(())
        } else {
            Err(Error::new(
                "CLEANUP_FAILED",
                "Owned simulator cleanup remains unfinished",
            )
            .details(serde_json::json!({"failures":failures})))
        }
    }
}
async fn settled(mut receiver: watch::Receiver<Option<Result<Target>>>) -> Result<Target> {
    loop {
        if let Some(result) = receiver.borrow().clone() {
            return result;
        }
        receiver.changed().await.map_err(|_| uncertain())?;
    }
}
async fn discover(set: &DeviceSet, id: &str) -> Result<Target> {
    Discovery::default()
        .refresh_set(set)
        .await?
        .into_iter()
        .find(|target| target.identity.target_id == id)
        .ok_or_else(|| {
            Error::new(
                "STALE_TARGET",
                "Owned simulator identity is no longer available",
            )
        })
}
async fn boot(target: &Target, set: &DeviceSet, mut record: Record, path: &Path) -> Result<Target> {
    let before = discover(set, &target.identity.target_id).await?;
    if before.epoch != target.epoch || before.state != "Shutdown" {
        return Err(Error::new(
            "STALE_TARGET",
            "Simulator changed before boot dispatch",
        ));
    }
    record.phase = "dispatched".into();
    record.dispatched_at_ms = Some(now_ms());
    storage::write(path, &record)?;
    let result = set
        .run(
            &["boot", &target.platform_id],
            Duration::from_secs(30),
            65536,
        )
        .await;
    if let Err(error) = result {
        // Only spawn refusal proves no command reached CoreSimulator. A failed
        // or timed-out dispatched command may still allocate after an inventory
        // observes Shutdown, so that observation cannot clear its uncertainty.
        if error.code == "PREREQUISITE_MISSING" {
            record.phase = "closed".into();
            storage::write(path, &record)?;
            return Err(error);
        }
        return Err(uncertain().details(serde_json::json!({"phase":"boot_dispatch","cause":error})));
    }
    // The finite readiness waiter belongs to the boot owner, so caller
    // cancellation cannot discard a resource halfway through allocation.
    let readiness = set
        .run(
            &["bootstatus", &target.platform_id, "-b"],
            Duration::from_secs(90),
            65536,
        )
        .await;
    let observed = discover(set, &target.identity.target_id).await.map_err(|error|
        uncertain().details(serde_json::json!({"phase":"post_boot_inventory","cause":error,"readinessError":readiness.as_ref().err()})))?;
    if observed.state != "Booted" || boot_identity(&observed.epoch).is_none() {
        return Err(uncertain().details(serde_json::json!({"phase":"boot_readiness","state":observed.state,"readinessError":readiness.err()})));
    }
    record.epoch.clone_from(&observed.epoch);
    record.phase = "owned".into();
    storage::write(path, &record).map_err(|error| {
        uncertain().details(serde_json::json!({"phase":"record_boot_ownership","cause":error}))
    })?;
    readiness?;
    Ok(observed)
}
fn uncertain() -> Error {
    Error::new(
        "SIMULATOR_BOOT_UNCERTAIN",
        "Simulator boot ownership is uncertain; inspect the recorded target before manual recovery; boot and shutdown are not replayed",
    )
}
fn boot_identity(epoch: &str) -> Option<(String, String)> {
    let mut fields = epoch.splitn(3, ':');
    let runtime = fields.next()?;
    fields.next()?;
    let boot = plist::Date::from_xml_format(fields.next()?)
        .ok()?
        .to_xml_format();
    Some((runtime.into(), boot))
}
/// The simulator's boot time from its epoch, in Unix milliseconds.
fn boot_time_ms(epoch: &str) -> Option<u64> {
    let date = plist::Date::from_xml_format(epoch.splitn(3, ':').nth(2)?).ok()?;
    let elapsed = std::time::SystemTime::from(date)
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?;
    u64::try_from(elapsed.as_millis()).ok()
}
async fn close_record(record: &mut Record, path: &Path) -> Result<()> {
    if matches!(record.phase.as_str(), "prepared" | "closed") {
        return Ok(());
    }
    if record.phase != "owned"
        && (record.phase != "dispatched" || record.dispatched_at_ms.is_none())
    {
        return Err(uncertain());
    }
    let set = match &record.device_set {
        Some(path) => DeviceSet::at(path.clone())?,
        None => DeviceSet {
            path: None,
            identity: "default".into(),
        },
    };
    if set.identity != record.device_set_identity || set.uuid(&record.target_id)? != record.uuid {
        return Err(Error::new(
            "STALE_TARGET",
            "Recorded simulator device set was replaced",
        ));
    }
    let current = discover(&set, &record.target_id).await?;
    if record.phase == "dispatched" {
        // A boot sent without a recorded outcome: the boot time decides.
        match dispatched(
            record.dispatched_at_ms,
            &current.state,
            &current.epoch,
            now_ms(),
        ) {
            Dispatched::Ours => {
                record.epoch.clone_from(&current.epoch);
                record.phase = "owned".into();
                storage::write(path, record)?;
            }
            Dispatched::NotOurs => {
                record.phase = "closed".into();
                return storage::write(path, record);
            }
            Dispatched::Settling => {
                return Err(Error::new(
                    "SIMULATOR_BOOT_UNCERTAIN",
                    "The simulator boot Testrun sent may still be settling; cleanup is retried and shuts the simulator down only if its boot time shows Testrun booted it",
                )
                .details(serde_json::json!({"state":current.state,"dispatchedAtMs":record.dispatched_at_ms})));
            }
            Dispatched::Unknown => return Err(uncertain()),
        }
    }
    if current.state != "Shutdown" {
        let current_boot = boot_identity(&current.epoch).ok_or_else(uncertain)?;
        let owned_boot = boot_identity(&record.epoch).ok_or_else(uncertain)?;
        if current_boot == owned_boot {
            if current.state != "Booted" {
                return Err(Error::new(
                    "CLEANUP_FAILED",
                    "Owned simulator is still transitioning; retry cleanup",
                ));
            }
            set.run(&["shutdown", &record.uuid], Duration::from_secs(30), 65536)
                .await?;
            if discover(&set, &record.target_id).await?.state != "Shutdown" {
                return Err(Error::new(
                    "CLEANUP_FAILED",
                    "Simulator shutdown was not confirmed",
                ));
            }
        }
        // A new boot stamp belongs to a replacement and is preserved.
    }
    record.phase = "closed".into();
    storage::write(path, record)
}
pub(crate) async fn recover(
    directory: &Path,
    target: &str,
    expected_set: Option<&Path>,
) -> Result<()> {
    match std::fs::symlink_metadata(directory) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_) => {
            return Err(Error::new(
                "STATE_IO",
                "Cannot inspect simulator startup records",
            ));
        }
        Ok(_) => {}
    }
    let path = journal(directory, target);
    let Some(mut record) = storage::read::<Record>(&path)? else {
        return Ok(());
    };
    if record.target_id != target || record.device_set.as_deref() != expected_set {
        return Err(Error::new(
            "UNAUTHORIZED",
            "Simulator boot record does not belong to this owner",
        ));
    }
    let _lease = ControlLease::acquire(target)?;
    close_record(&mut record, &path).await
}

#[cfg(test)]
mod tests {
    use super::*;
    fn private_root() -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        root
    }
    fn record(target: &str, phase: &str) -> Record {
        Record {
            target_id: target.into(),
            uuid: "Acme".into(),
            device_set: None,
            device_set_identity: "default".into(),
            phase: phase.into(),
            epoch: "iOS:Booted:2026-10-19T10:00:00Z".into(),
            dispatched_at_ms: None,
        }
    }
    #[test]
    fn a_dispatched_boot_is_ours_only_when_the_simulator_booted_just_after_it() {
        let dispatched_at = boot_time_ms("iOS:Booted:2026-10-19T10:00:00Z").unwrap();
        let at = |offset: i64| {
            let ms = dispatched_at.saturating_add_signed(offset * 1000);
            let date: plist::Date = (std::time::UNIX_EPOCH + Duration::from_millis(ms)).into();
            format!("iOS:Booted:{}", date.to_xml_format())
        };
        let now = dispatched_at + 10_000;
        // Booted within the window after dispatch, allowing small clock skew.
        for offset in [-4, 0, 30, 119] {
            assert_eq!(
                dispatched(Some(dispatched_at), "Booted", &at(offset), now),
                Dispatched::Ours,
                "{offset}"
            );
        }
        // Booted well before dispatch, or long after it: another boot.
        for offset in [-60, 600] {
            assert_eq!(
                dispatched(Some(dispatched_at), "Booted", &at(offset), now),
                Dispatched::NotOurs,
                "{offset}"
            );
        }
        // Shut down: settling inside the window, never booted after it.
        assert_eq!(
            dispatched(Some(dispatched_at), "Shutdown", &at(0), now),
            Dispatched::Settling
        );
        assert_eq!(
            dispatched(
                Some(dispatched_at),
                "Shutdown",
                &at(0),
                dispatched_at + BOOT_WINDOW_MS
            ),
            Dispatched::NotOurs
        );
        assert_eq!(
            dispatched(Some(dispatched_at), "Booting", &at(0), now),
            Dispatched::Settling
        );
        // Without a dispatch time, or an unreadable boot time, nothing is guessed.
        assert_eq!(dispatched(None, "Booted", &at(0), now), Dispatched::Unknown);
        assert_eq!(
            dispatched(Some(dispatched_at), "Booted", "iOS:Booted:unknown", now),
            Dispatched::Unknown
        );
    }
    #[tokio::test]
    async fn owner_loss_before_allocation_is_safe_but_dispatched_boot_is_never_guessed() {
        let root = private_root();
        let target = format!("simulator:Acme-{}", uuid::Uuid::new_v4());
        let path = journal(root.path(), &target);
        recover(&root.path().join("not-allocated"), &target, None)
            .await
            .unwrap();
        for phase in ["prepared", "closed"] {
            storage::write(&path, &record(&target, phase)).unwrap();
            recover(root.path(), &target, None).await.unwrap();
        }
        storage::write(&path, &record(&target, "dispatched")).unwrap();
        assert_eq!(
            recover(root.path(), &target, None).await.unwrap_err().code,
            "SIMULATOR_BOOT_UNCERTAIN"
        );
        assert_eq!(
            storage::read::<Record>(&path).unwrap().unwrap().phase,
            "dispatched"
        );
        let _foreign = ControlLease::acquire(&target).unwrap();
        assert_eq!(
            recover(root.path(), &target, None).await.unwrap_err().code,
            "TARGET_BUSY"
        );
    }
    #[tokio::test]
    async fn foreign_records_and_replaced_sets_do_not_authorize_shutdown() {
        let root = private_root();
        let target = format!("simulator:Acme-{}", uuid::Uuid::new_v4());
        let path = journal(root.path(), &target);
        storage::write(&path, &record("simulator:foreign", "owned")).unwrap();
        assert_eq!(
            recover(root.path(), &target, None).await.unwrap_err().code,
            "UNAUTHORIZED"
        );
        let set_path = root.path().join("Acme-set");
        std::fs::create_dir(&set_path).unwrap();
        let set = DeviceSet::at(set_path.clone()).unwrap();
        let mut owned = record(&set.target_id("Acme"), "owned");
        owned.device_set = Some(set.path.unwrap());
        owned.device_set_identity = set.identity;
        let target = owned.target_id.clone();
        let path = journal(root.path(), &target);
        storage::write(&path, &owned).unwrap();
        std::fs::rename(&set_path, root.path().join("retired-set")).unwrap();
        std::fs::create_dir(&set_path).unwrap();
        assert_eq!(
            recover(root.path(), &target, owned.device_set.as_deref())
                .await
                .unwrap_err()
                .code,
            "STALE_TARGET"
        );
    }
}
