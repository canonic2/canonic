//! Target selection and validation of saved device actor identity. The
//! pool's records directory holds one directory per target any client
//! connected, with that target's selection, actions and device actor records.
use super::{Caller, Connection, WIRE, hash, identity::POOL, now, storage};
use crate::error::{Error, Result};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// The private records directory of the pool named by `key` under `root`.
pub(crate) fn pool_directory(root: &Path, key: &str) -> Result<PathBuf> {
    storage::directory(root)?;
    // Protocol generations own separate records; target leases still exclude
    // controllers from every version, including ones finishing shutdown.
    let directory = root.join(format!("v{WIRE}-{}", &hash(key)[..16]));
    storage::directory(&directory)?;
    Ok(directory)
}

/// The records directory of `target` inside pool directory `pool`.
pub(crate) fn target_directory(pool: &Path, target: &str) -> Result<PathBuf> {
    let directory = pool.join(format!("{TARGET_PREFIX}{}", &hash(target)[..16]));
    storage::directory(&directory)?;
    Ok(directory)
}

pub(crate) const TARGET_PREFIX: &str = "t-";

/// Target record directories inside pool directory `pool`.
pub(crate) fn target_directories(pool: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(pool) else {
        return Vec::new();
    };
    let mut directories: Vec<PathBuf> = entries
        .take(4096)
        .flatten()
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(TARGET_PREFIX)
                && entry.file_type().is_ok_and(|kind| kind.is_dir())
        })
        .map(|entry| entry.path())
        .collect();
    directories.sort();
    directories
}

/// Targets with a recorded connection that is still held or not yet closed,
/// read from durable records while no manager describes them.
pub(crate) fn recorded_targets(pool: &Path) -> Vec<String> {
    target_directories(pool)
        .into_iter()
        .filter_map(|directory| {
            let connection = storage::read::<Connection>(&directory.join("selected.json"))
                .ok()
                .flatten()?;
            let closed = storage::read::<Value>(&connection.directory.join("state.json"))
                .ok()
                .flatten()
                .is_some_and(|state| state["state"] == "closed");
            (connection.held().unwrap_or(false) || !closed).then_some(connection.target)
        })
        .collect()
}

/// The connection pool directory `pool` recorded most recently, open or closed.
fn last_connection(pool: &Path) -> Option<Connection> {
    target_directories(pool)
        .into_iter()
        .filter_map(|directory| {
            storage::read::<Connection>(&directory.join("selected.json"))
                .ok()
                .flatten()
        })
        .max_by_key(|connection| connection.created)
}

/// The target of the most recent connection when it was interrupted, as by
/// a manager loss, rather than released: with nothing connected an omitted
/// target resolves to it, so its durable action records still answer. A
/// release marks the connection disconnected, and then nothing is resolved.
pub(crate) fn interrupted_target(pool: &Path) -> Option<String> {
    let connection = last_connection(pool)?;
    let disconnected = storage::read::<Value>(&connection.directory.join("disconnected.json"))
        .ok()
        .flatten()
        .is_some_and(|marker| marker["epoch"] == connection.epoch);
    (!disconnected).then_some(connection.target)
}

impl Caller {
    /// The records directory this request works in: its target's, or the
    /// pool's when it addresses no target.
    pub(super) fn directory(&self) -> Result<PathBuf> {
        let pool = self.pool_records()?;
        match &self.target {
            Some(target) => target_directory(&pool, target),
            None => Ok(pool),
        }
    }
    /// The pool's own records directory.
    pub(super) fn pool_records(&self) -> Result<PathBuf> {
        pool_directory(&self.root, POOL)
    }
    /// Whether the pool's records show `target` still open.
    pub(super) fn recorded(&self, target: &str) -> bool {
        self.pool_records()
            .is_ok_and(|pool| recorded_targets(&pool).iter().any(|open| open == target))
    }
    /// The connected device targets: in the pool, or recorded as still open
    /// when no manager describes them.
    pub(super) fn pool_targets(&self) -> Result<Vec<String>> {
        if let (Some(service), Some(_)) = (&self.service, &self.pool) {
            return Ok(service.devices());
        }
        Ok(recorded_targets(&self.pool_records()?))
    }
    pub(super) fn selected(&self, directory: &Path) -> Result<Connection> {
        let connection: Connection = match storage::read(&directory.join("selected.json"))? {
            Some(connection) => connection,
            None => {
                let mut owners = Vec::new();
                for entry in std::fs::read_dir(directory)
                    .map_err(|_| Error::new("STATE_IO", "Cannot resolve this target's connection"))?
                    .take(4096)
                {
                    let entry = entry.map_err(|_| {
                        Error::new("STATE_IO", "Cannot resolve this target's connection")
                    })?;
                    if entry.file_name().to_string_lossy().starts_with("o-")
                        && entry.file_type().is_ok_and(|kind| kind.is_dir())
                        && let Some(owner) =
                            storage::read::<Connection>(&entry.path().join("owner.json"))?
                        && owner.task == hash(POOL)
                        && owner.held()?
                    {
                        self.validate_saved(directory, &owner)?;
                        if owner.directory != entry.path() {
                            return Err(Error::new(
                                "STATE_IO",
                                "Device actor location does not match its record",
                            ));
                        }
                        owners.push(owner);
                    }
                }
                if owners.len() != 1 {
                    return Err(Error::new(
                        "NOT_CONNECTED",
                        "Select a device with connect first",
                    ));
                }
                let owner = owners.remove(0);
                storage::write(&directory.join("selected.json"), &owner)?;
                owner
            }
        };
        self.validate_saved(directory, &connection)?;
        Ok(connection)
    }
    pub(super) fn validate_saved(&self, directory: &Path, connection: &Connection) -> Result<()> {
        if connection.task != hash(POOL) {
            return Err(Error::new(
                "STATE_IO",
                "The saved connection does not belong to this manager",
            ));
        }
        if connection.directory.parent() != Some(directory) {
            return Err(Error::new(
                "STATE_IO",
                "Saved connection location is invalid",
            ));
        }
        if connection.version != WIRE {
            return Err(Error::new(
                "OWNER_PROTOCOL_UNSUPPORTED",
                "Finish cleanup with the owning Testrun build before using this protocol",
            ));
        }
        Ok(())
    }
    pub(super) fn desired(&self, directory: &Path, args: &Value) -> Result<Connection> {
        let target = args["targetId"]
            .as_str()
            .filter(|s| {
                s.starts_with("device:") || s.starts_with("simulator:") || s.starts_with("android:")
            })
            .ok_or_else(|| Error::invalid("Choose an explicit discovered targetId"))?;
        let physical = target.starts_with("device:");
        let avd = target.starts_with(crate::android::emulator::AVD_PREFIX);
        if args["boot"] == true && !target.starts_with("simulator:") && !avd {
            return Err(Error::invalid(
                "boot applies to iOS simulators and android:avd: targets only",
            ));
        }
        if avd && args["boot"] != true {
            return Err(Error::new(
                "BOOT_REQUIRED",
                "An AVD target has no running instance; connect with boot to start one, or select its android:emulator- target",
            ));
        }
        if args["window"] == true && !avd {
            return Err(Error::invalid(
                "window applies to android:avd: targets only",
            ));
        }
        let simulator_device_set = if target.starts_with("simulator:") {
            // The set travels with the client's request, not the manager.
            let set = match self.device_set.clone() {
                Some(path) => crate::simulator::device_set::DeviceSet::at(path)?,
                None => crate::simulator::device_set::DeviceSet {
                    path: None,
                    identity: "default".into(),
                },
            };
            set.uuid(target)?;
            set.path
        } else {
            None
        };
        let control_path = if physical {
            Some(args["controlPath"].as_str().unwrap_or("hid").to_owned())
        } else {
            None
        };
        if let Some(field) = [
            "controlPath",
            "appId",
            "runnerTeamId",
            "setup",
            "prepareSupport",
        ]
        .into_iter()
        .find(|field| !physical && !matches!(args[*field], Value::Null | Value::Bool(false)))
        {
            return Err(Error::invalid(format!(
                "{field} applies to physical iPhones only"
            )));
        }
        if physical && (args["appId"].is_string() != (control_path.as_deref() == Some("xctest"))) {
            return Err(Error::invalid(
                "XCTest inspection requires appId; HID uses full-display scope without appId",
            ));
        }
        let desired = Connection {
            version: WIRE,
            build_id: Some(super::BUILD_ID.into()),
            task: hash(POOL),
            target: target.into(),
            runner_team_id: args["runnerTeamId"].as_str().map(String::from),
            control_path,
            android_capture: crate::android::streaming::Mode::parse(
                args.get("androidCapture"),
                target.starts_with("android:"),
            )?,
            bundle_id: args["appId"].as_str().map(String::from),
            setup: args["setup"] == true,
            boot: args["boot"] == true,
            window: args["window"] == true,
            simulator_device_set,
            epoch: uuid::Uuid::new_v4().to_string(),
            directory: directory.join(format!(
                "o-{}",
                &uuid::Uuid::new_v4().simple().to_string()[..16]
            )),
            created: now(),
            host_label: self.host.clone(),
        };
        Ok(desired)
    }
}
