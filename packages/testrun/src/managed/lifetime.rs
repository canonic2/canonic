//! Durable cleanup settlement for device actors: reconciling epochs whose
//! actor or manager is gone, and the cleanup inventory that status, blockers
//! and recovery report.
use super::{Caller, Connection, now, storage};
use crate::{
    error::{Error, Result},
    lease::ControlLease,
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path};

impl Caller {
    /// Settle the durable resources an actor left when it or its owner
    /// process died, then record completion. Live and already complete
    /// epochs are left unchanged.
    ///
    /// A physical device's remote media expiry is verified by the shared
    /// release wait; simulators and owned emulators are recovered from their
    /// lifecycle records; physical Android leaves no device state to settle.
    /// Device settings changed on request stay as the caller left them.
    pub(super) async fn reconcile_lost(&self, owner: &Connection) -> Result<()> {
        let path = owner.directory.join("cleanup.json");
        let mut cleanup = storage::read::<Value>(&path)?.unwrap_or(json!({}));
        if cleanup["complete"] == true || owner.held()? {
            return Ok(());
        }
        if owner.target.starts_with("device:") {
            return self.wait_release(owner).await;
        }
        if owner.target.starts_with("simulator:") {
            let Some(_authority) = Self::settling_lease(owner, &path).await? else {
                return Ok(());
            };
            let mut cleanup = storage::read::<Value>(&path)?.unwrap_or(json!({}));
            if owner.boot {
                crate::simulator::lifecycle::recover(
                    &owner.directory.join("s"),
                    &owner.target,
                    owner.simulator_device_set.as_deref(),
                )
                .await?;
            }
            cleanup["complete"] = json!(true);
            cleanup["reconciled"] =
                json!({"reason":"owner_lost","simulatorBoot":"settled","at":now()});
            return storage::write(&path, &cleanup);
        }
        if owner
            .target
            .starts_with(crate::android::emulator::AVD_PREFIX)
        {
            let Some(_authority) = Self::settling_lease(owner, &path).await? else {
                return Ok(());
            };
            let mut cleanup = storage::read::<Value>(&path)?.unwrap_or(json!({}));
            crate::android::lifecycle::recover(&owner.directory.join("s"), &owner.target).await?;
            cleanup["complete"] = json!(true);
            cleanup["reconciled"] =
                json!({"reason":"owner_lost","emulatorStart":"settled","at":now()});
            return storage::write(&path, &cleanup);
        }
        if !owner.target.starts_with("android:") {
            return Ok(());
        }
        cleanup["complete"] = json!(true);
        cleanup["reconciled"] = json!({"reason":"owner_lost","at":now()});
        storage::write(&path, &cleanup)
    }

    /// The lease that settles `owner`'s lost epoch. Another reconciliation of
    /// the same epoch, such as a manager's startup settle beside a client's
    /// disconnect, holds it briefly: wait for it, and return `None` when it
    /// completed the cleanup meanwhile.
    async fn settling_lease(owner: &Connection, cleanup: &Path) -> Result<Option<ControlLease>> {
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(60);
        loop {
            let complete = storage::read::<Value>(cleanup)?.is_some_and(|c| c["complete"] == true);
            if complete {
                return Ok(None);
            }
            match ControlLease::acquire(&owner.authority()) {
                Ok(lease) => {
                    // The other reconciliation may have finished just before.
                    let complete =
                        storage::read::<Value>(cleanup)?.is_some_and(|c| c["complete"] == true);
                    return Ok((!complete).then_some(lease));
                }
                Err(error)
                    if error.code == "TARGET_BUSY" && tokio::time::Instant::now() < deadline =>
                {
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                }
                Err(error) => return Err(error),
            }
        }
    }

    /// Recovery's settle: only epochs whose actor stopped are reconciled. A
    /// running actor closes only through its target's release.
    pub(crate) async fn settle_stopped(&self, directory: &Path) -> Result<()> {
        let (owners, mut failures) = self.owners(directory)?;
        for owner in owners {
            let complete = storage::read::<Value>(&owner.directory.join("cleanup.json"))?
                .is_some_and(|cleanup| cleanup["complete"] == true);
            if complete {
                continue;
            }
            let supervised = self
                .service
                .as_ref()
                .is_some_and(|service| service.supervisor.running(&owner.epoch));
            if supervised {
                continue;
            }
            let result = if owner.held()? {
                Err(Error::new(
                    "DEVICE_IN_USE",
                    "Another manager still holds this epoch's device lease",
                ))
            } else {
                self.reconcile_lost(&owner).await
            };
            if let Err(error) = result {
                failures.push(json!({"targetId":owner.target,"epoch":owner.epoch,"error":error}));
            }
        }
        if failures.is_empty() {
            Ok(())
        } else {
            Err(Error::new("CLEANUP_FAILED", "Unfinished cleanup remains")
                .details(json!({"failures":failures})))
        }
    }

    /// Every device actor record under `directory` with its cleanup state.
    pub(super) fn cleanup_inventory(&self, directory: &Path) -> Result<Value> {
        let (owners, failures) = self.owners(directory)?;
        let mut pending = false;
        let mut failed = !failures.is_empty();
        let mut cleanup = Vec::new();
        for owner in owners {
            let held = owner.held()?;
            let state = storage::read::<Value>(&owner.directory.join("state.json"))?;
            let progress = storage::read::<Value>(&owner.directory.join("cleanup.json"))?;
            let complete = progress.as_ref().is_some_and(|p| p["complete"] == true);
            pending |= held || !complete;
            failed |=
                !complete && (!held || state.as_ref().is_some_and(|s| s["state"] == "failed"));
            cleanup.push(json!({"targetId":owner.target,"epoch":owner.epoch,"held":held,"closed":!held && complete,"state":state.as_ref().map(|s| &s["state"]),"error":state.as_ref().map(|s| &s["error"]),"cleanup":progress}));
        }
        Ok(
            json!({"state":if failed {"failed"}else if pending {"closing"}else {"closed"},"closed":!pending && !failed,"owners":cleanup,"failures":failures}),
        )
    }

    /// The device actor records under the pool's or a target's directory,
    /// including every target directory of the pool.
    pub(super) fn owners(&self, directory: &Path) -> Result<(Vec<Connection>, Vec<Value>)> {
        let mut owners = BTreeMap::new();
        let mut failures = Vec::new();
        for parent in std::iter::once(directory.to_path_buf())
            .chain(super::selection::target_directories(directory))
        {
            self.owners_in(&parent, &mut owners, &mut failures)?;
        }
        Ok((owners.into_values().collect(), failures))
    }

    fn owners_in(
        &self,
        directory: &Path,
        owners: &mut BTreeMap<std::path::PathBuf, Connection>,
        failures: &mut Vec<Value>,
    ) -> Result<()> {
        for (index, entry) in std::fs::read_dir(directory)
            .map_err(|_| Error::new("STATE_IO", "Cannot enumerate device actor records"))?
            .enumerate()
        {
            if index >= 4096 {
                failures.push(json!({"error":Error::new("RESOURCE_LIMIT", "Device actor history exceeds the cleanup scan limit")}));
                break;
            }
            let owner = (|| {
                let entry = entry
                    .map_err(|_| Error::new("STATE_IO", "Cannot inspect a device actor record"))?;
                if !entry.file_name().to_string_lossy().starts_with("o-")
                    || !entry.file_type().is_ok_and(|kind| kind.is_dir())
                {
                    return Ok(None);
                }
                let owner = storage::read::<Connection>(&entry.path().join("owner.json"))?;
                if let Some(owner) = &owner {
                    self.validate_saved(directory, owner)?;
                    if owner.directory != entry.path() {
                        return Err(Error::new(
                            "STATE_IO",
                            "Device actor location does not match its record",
                        ));
                    }
                }
                Ok(owner)
            })();
            match owner {
                Ok(Some(owner)) => {
                    owners.insert(owner.directory.clone(), owner);
                }
                Ok(None) => {}
                Err(error) => failures.push(json!({"error":error})),
            }
        }
        Ok(())
    }
}
