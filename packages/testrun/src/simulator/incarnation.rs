//! Simulator incarnation evidence from the native-selected device record, never a TTL cache.
use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Incarnation {
    path: PathBuf,
    uuid: String,
    runtime: String,
    boot: String,
    directories: [(u64, u64); 2],
    #[serde(default)]
    peer: Option<Peer>,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Peer {
    pub pid: i32,
    pub seconds: u64,
    pub micros: u64,
}
impl Incarnation {
    #[cfg(target_os = "macos")]
    pub(crate) fn with_peer(mut self, peer: Peer) -> Self {
        self.peer = Some(peer);
        self
    }
    pub(crate) fn read(path: &Path, uuid: &str) -> Result<Self> {
        let (identity, state) = Self::record(path, uuid)?;
        if state != 3 {
            return Err(stale());
        }
        Ok(identity)
    }
    fn record(path: &Path, uuid: &str) -> Result<(Self, u64)> {
        let directories = [directory(path)?, directory(&path.join("data"))?];
        let file = fs::File::open(path.join("device.plist")).map_err(|_| stale())?;
        if file.metadata().map_err(|_| stale())?.len() > 65536 {
            return Err(stale());
        }
        let mut bytes = Vec::new();
        file.take(65537)
            .read_to_end(&mut bytes)
            .map_err(|_| stale())?;
        let value = plist::Value::from_reader(std::io::Cursor::new(bytes)).map_err(|_| stale())?;
        let record = value.as_dictionary().ok_or_else(stale)?;
        let string = |key: &str| {
            record
                .get(key)
                .and_then(plist::Value::as_string)
                .ok_or_else(stale)
        };
        let state = record
            .get("state")
            .and_then(plist::Value::as_unsigned_integer)
            .ok_or_else(stale)?;
        if !string("UDID")?.eq_ignore_ascii_case(uuid)
            || record.get("isDeleted").and_then(plist::Value::as_boolean) == Some(true)
        {
            return Err(stale());
        }
        let result = Self {
            path: path.to_owned(),
            uuid: uuid.to_ascii_uppercase(),
            runtime: string("runtime")?.into(),
            boot: record
                .get("lastBootedAt")
                .and_then(plist::Value::as_date)
                .ok_or_else(stale)?
                .to_xml_format(),
            directories,
            peer: None,
        };
        if [directory(path)?, directory(&path.join("data"))?] != directories {
            return Err(stale());
        }
        Ok((result, state))
    }
    pub(crate) fn validate(&self) -> Result<()> {
        #[cfg(target_os = "macos")]
        if self
            .peer
            .is_some_and(|peer| !crate::native::peer_alive(peer))
        {
            return Err(stale());
        }
        let mut current = Self::read(&self.path, &self.uuid)?;
        current.peer = self.peer;
        if current != *self {
            return Err(stale());
        }
        Ok(())
    }
    pub(crate) fn retired(&self) -> bool {
        Self::record(&self.path, &self.uuid).is_ok_and(|(current, state)| {
            current.directories == self.directories
                && current.runtime == self.runtime
                && (state == 1 && current.boot == self.boot
                    || state == 3 && current.boot != self.boot)
        })
    }
    pub(crate) fn matches_discovery(&self, epoch: &str) -> Result<()> {
        let mut fields = epoch.splitn(3, ':');
        let runtime = fields.next();
        let state = fields.next();
        let boot = fields.next();
        let date = boot
            .and_then(|s| plist::Date::from_xml_format(s).ok())
            .map(|d| d.to_xml_format());
        if runtime != Some(self.runtime.as_str())
            || state != Some("Booted")
            || date.as_deref() != Some(self.boot.as_str())
        {
            return Err(stale());
        }
        self.validate()
    }
}
fn directory(path: &Path) -> Result<(u64, u64)> {
    let metadata = fs::symlink_metadata(path).map_err(|_| stale())?;
    if !metadata.is_dir() || path.canonicalize().map_err(|_| stale())? != path {
        return Err(stale());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Ok((metadata.dev(), metadata.ino()))
    }
    #[cfg(not(unix))]
    {
        Err(stale())
    }
}
fn stale() -> Error {
    Error::new(
        "STALE_TARGET",
        "Simulator incarnation is unavailable or changed; discover and acquire a new session",
    )
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    pub(crate) fn record(path: &Path, boot: &str, state: u64) {
        let mut record = plist::Dictionary::new();
        record.insert("UDID".into(), "Acme".into());
        record.insert("runtime".into(), "iOS-Acme".into());
        record.insert("state".into(), state.into());
        record.insert(
            "lastBootedAt".into(),
            plist::Value::Date(plist::Date::from_xml_format(boot).unwrap()),
        );
        plist::Value::Dictionary(record)
            .to_file_xml(path.join("device.plist"))
            .unwrap();
    }
    pub(crate) fn fixture() -> (tempfile::TempDir, Incarnation) {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().canonicalize().unwrap();
        fs::create_dir(path.join("data")).unwrap();
        record(&path, "2026-10-15T12:00:00Z", 3);
        let identity = Incarnation::read(&path, "Acme").unwrap();
        (temp, identity)
    }
    #[test]
    fn boot_shutdown_missing_evidence_and_discovery_mismatch_refuse_old_identity() {
        let (_temp, identity) = fixture();
        identity
            .matches_discovery("iOS-Acme:Booted:2026-10-15T12:00:00Z")
            .unwrap();
        assert!(
            identity
                .matches_discovery("iOS-Acme:Booted:2026-10-15T11:00:00Z")
                .is_err()
        );
        record(&identity.path, "2026-10-15T12:00:00Z", 1);
        assert!(identity.validate().is_err());
        assert!(identity.retired());
        record(&identity.path, "2026-10-15T12:01:00Z", 3);
        assert!(identity.validate().is_err());
        assert!(identity.retired());
        fs::remove_file(identity.path.join("device.plist")).unwrap();
        assert!(identity.validate().is_err());
        assert!(!identity.retired());
    }
    #[test]
    fn replacing_device_data_or_symlinking_a_record_path_cannot_retain_authority() {
        let (_temp, identity) = fixture();
        fs::rename(
            identity.path.join("data"),
            identity.path.join("previous-data"),
        )
        .unwrap();
        fs::create_dir(identity.path.join("data")).unwrap();
        assert!(identity.validate().is_err());
        assert!(!identity.retired());
        #[cfg(unix)]
        {
            fs::remove_dir(identity.path.join("data")).unwrap();
            std::os::unix::fs::symlink(
                identity.path.join("previous-data"),
                identity.path.join("data"),
            )
            .unwrap();
            assert!(identity.validate().is_err());
        }
    }
    #[cfg(target_os = "macos")]
    #[test]
    fn service_process_death_and_pid_birth_mismatch_invalidate_an_unchanged_boot_record() {
        let (_temp, identity) = fixture();
        let mut child = std::process::Command::new("python3")
            .args(["-c", "import time; time.sleep(60)"])
            .spawn()
            .unwrap();
        let peer = crate::native::process_peer(child.id() as i32);
        let bound = identity.clone().with_peer(peer);
        let alive = bound.validate();
        let mut reused = peer;
        reused.micros += 1;
        let mismatched = identity.with_peer(reused).validate();
        child.kill().unwrap();
        child.wait().unwrap();
        alive.unwrap();
        assert!(mismatched.is_err());
        assert!(bound.validate().is_err());
    }
}
