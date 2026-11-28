//! Retained and journaled emulator starts, independent of the requesting future.
//! Testrun stops only the process it launched, identified by process ID and
//! start time; the AVD's console name is used only for a graceful stop.
use super::{
    Android,
    emulator::{self, AVD_PREFIX, DATA_POLICY},
};
use crate::{
    error::{Error, Result},
    lease::ControlLease,
    managed::storage,
    process,
    targets::Target,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::{sync::watch, time::Instant};

/// Even console ports adb tracks; the adb port is the next odd port.
const PORTS: std::ops::RangeInclusive<u16> = 5554..=5584;
const IDENTITY_BUDGET: Duration = Duration::from_secs(60);
const READY_BUDGET: Duration = Duration::from_secs(240);

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Record {
    target_id: String,
    avd: String,
    window: bool,
    data_policy: String,
    serial: Option<String>,
    pid: Option<u32>,
    started: Option<u64>,
    phase: String,
}
/// The console port lease, held from port selection until the instance stops,
/// so concurrent Testrun processes never launch on the same port.
type PortLease = Arc<std::sync::Mutex<Option<ControlLease>>>;
struct Start {
    ready: watch::Receiver<Option<Result<String>>>,
    cancel: watch::Sender<bool>,
    journal: PathBuf,
    serial: Option<String>,
    _lease: Arc<ControlLease>,
    _port: PortLease,
    armed: bool,
}
impl Drop for Start {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return;
        };
        self.cancel.send_replace(true);
        let ready = self.ready.clone();
        let path = self.journal.clone();
        let lease = self._lease.clone();
        let port = self._port.clone();
        runtime.spawn(async move {
            let _lease = lease;
            let _port = port;
            let _ = settled(ready).await;
            if let Err(error) = close_journal(&path).await {
                eprintln!("Emulator cleanup remains pending: {}", error.code);
            }
        });
    }
}

/// Owned emulator starts, keyed by the AVD target identity.
#[derive(Default)]
pub(crate) struct Starts {
    entries: HashMap<String, Start>,
}
pub(crate) fn journal(directory: &Path, target: &str) -> PathBuf {
    directory.join(format!(
        "emulator-start-{:x}.json",
        Sha256::digest(target.as_bytes())
    ))
}
impl Starts {
    /// Whether this registry owns the instance behind an `android:` serial target.
    pub(crate) fn owns(&self, serial_target: &str) -> bool {
        self.key_for(serial_target).is_some()
    }
    fn key_for(&self, serial_target: &str) -> Option<String> {
        self.entries
            .iter()
            .find(|(_, start)| start.serial.as_deref() == Some(serial_target))
            .map(|(key, _)| key.clone())
    }
    /// Start the AVD behind `target` and wait until it is usable. Returns the
    /// started instance's `android:emulator-PORT` target identity. The lease
    /// for the AVD identity stays with this registry until the instance stops.
    pub(crate) async fn start(
        &mut self,
        target: &Target,
        lease: ControlLease,
        directory: &Path,
        window: bool,
    ) -> Result<String> {
        let key = target.identity.target_id.clone();
        let name = key
            .strip_prefix(AVD_PREFIX)
            .filter(|name| emulator::valid_name(name))
            .ok_or_else(|| Error::invalid("Choose an explicit android:avd: target"))?
            .to_owned();
        if self.entries.contains_key(&key) {
            return Err(Error::new(
                "TARGET_BUSY",
                "Emulator startup or cleanup is already owned",
            ));
        }
        if self.entries.len() >= 16 {
            return Err(Error::new(
                "RESOURCE_LIMIT",
                "Emulator ownership capacity reached; finish pending cleanup",
            ));
        }
        let executable = emulator::executable()?;
        let path = journal(directory, &key);
        let record = Record {
            target_id: key.clone(),
            avd: name,
            window,
            data_policy: DATA_POLICY.into(),
            serial: None,
            pid: None,
            started: None,
            phase: "prepared".into(),
        };
        storage::write(&path, &record)?;
        let lease = Arc::new(lease);
        let port = PortLease::default();
        let (ready, receiver) = watch::channel(None);
        let (cancel, cancelled) = watch::channel(false);
        self.entries.insert(
            key.clone(),
            Start {
                ready: receiver.clone(),
                cancel,
                journal: path.clone(),
                serial: None,
                _lease: lease.clone(),
                _port: port.clone(),
                armed: true,
            },
        );
        let log = directory.join(format!(
            "emulator-{:x}.log",
            Sha256::digest(uuid::Uuid::new_v4().as_bytes())
        ));
        tokio::spawn(async move {
            let _lease = lease;
            let result = launch(record, &path, &executable, &log, &port, cancelled).await;
            ready.send_replace(Some(result));
        });
        let serial = settled(receiver).await?;
        if let Some(entry) = self.entries.get_mut(&key) {
            entry.serial = Some(serial.clone());
        }
        Ok(serial)
    }
    /// Stop the instance started for an AVD target, whether or not it became ready.
    pub(crate) async fn release_avd(&mut self, key: &str) -> Result<()> {
        let Some(entry) = self.entries.get(key) else {
            return Ok(());
        };
        entry.cancel.send_replace(true);
        // The durable phase decides cleanup safety even when startup failed.
        let _ = settled(entry.ready.clone()).await;
        close_journal(&entry.journal).await?;
        self.entries.get_mut(key).unwrap().armed = false;
        self.entries.remove(key);
        Ok(())
    }
    /// Stop the owned instance behind an `android:` serial target, if any.
    pub(crate) async fn release(&mut self, serial_target: &str) -> Result<()> {
        match self.key_for(serial_target) {
            Some(key) => self.release_avd(&key).await,
            None => Ok(()),
        }
    }
    /// Stop owned instances no active session uses, including unfinished starts.
    pub(crate) async fn release_unused(&mut self, active: &HashSet<String>) -> Result<()> {
        let keys: Vec<_> = self
            .entries
            .iter()
            .filter(|(_, start)| start.serial.as_ref().is_none_or(|s| !active.contains(s)))
            .map(|(key, _)| key.clone())
            .collect();
        let mut failures = Vec::new();
        for key in keys {
            if let Err(error) = self.release_avd(&key).await {
                failures.push(error);
            }
        }
        if failures.is_empty() {
            Ok(())
        } else {
            Err(Error::new(
                "CLEANUP_FAILED",
                "Owned emulator cleanup remains unfinished",
            )
            .details(json!({"failures":failures})))
        }
    }
}

async fn settled(mut receiver: watch::Receiver<Option<Result<String>>>) -> Result<String> {
    loop {
        if let Some(result) = receiver.borrow().clone() {
            return result;
        }
        receiver.changed().await.map_err(|_| uncertain())?;
    }
}

fn uncertain() -> Error {
    Error::new(
        "EMULATOR_START_UNCERTAIN",
        "Emulator ownership is uncertain; inspect the recorded port and log before manual recovery; start and stop are not replayed",
    )
}

fn closed(record: &mut Record, path: &Path) -> Result<()> {
    record.phase = "closed".into();
    storage::write(path, record)
}

/// Pick a console port whose serial adb does not list, whose console and adb
/// ports are free on loopback, and whose host-wide port lease is available.
fn free_port(listed: &HashSet<String>) -> Option<(u16, ControlLease)> {
    PORTS.step_by(2).find_map(|port| {
        if listed.contains(&format!("emulator-{port}"))
            || ![port, port + 1]
                .iter()
                .all(|port| std::net::TcpListener::bind(("127.0.0.1", *port)).is_ok())
        {
            return None;
        }
        let lease = ControlLease::acquire(&format!("android-emulator-port:{port}")).ok()?;
        Some((port, lease))
    })
}

async fn launch(
    mut record: Record,
    path: &Path,
    executable: &Path,
    log: &Path,
    port_lease: &PortLease,
    mut cancelled: watch::Receiver<bool>,
) -> Result<String> {
    let adb = Android::default();
    let devices = match adb.devices().await {
        Ok(devices) => devices,
        Err(error) => {
            closed(&mut record, path)?;
            return Err(error);
        }
    };
    // Never adopt or start beside an instance Testrun does not own.
    for device in devices.iter().filter(|d| d.serial.starts_with("emulator-")) {
        if adb
            .avd_name(&device.serial, Duration::from_secs(3))
            .await
            .is_ok_and(|name| name == record.avd)
        {
            closed(&mut record, path)?;
            return Err(Error::new(
                "AVD_ALREADY_RUNNING",
                "This AVD is already running outside Testrun; connect to its emulator target instead",
            )
            .details(json!({"targetId":format!("android:{}", device.serial)})));
        }
    }
    let listed = devices.iter().map(|d| d.serial.clone()).collect();
    let Some((port, lease)) = free_port(&listed) else {
        closed(&mut record, path)?;
        return Err(Error::new(
            "RESOURCE_LIMIT",
            "No free emulator console port in 5554-5584",
        ));
    };
    *port_lease.lock().unwrap_or_else(|e| e.into_inner()) = Some(lease);
    let serial = format!("emulator-{port}");
    record.serial = Some(serial.clone());
    record.phase = "dispatched".into();
    storage::write(path, &record)?;
    let mut child = match emulator::launch(executable, &record.avd, port, record.window, log) {
        Ok(child) => child,
        Err(error) => {
            // A refused spawn proves no emulator process exists.
            closed(&mut record, path)?;
            return Err(error);
        }
    };
    let identity = child
        .id()
        .and_then(|pid| process::start_time(pid).map(|started| (pid, started)));
    let Some((pid, started)) = identity else {
        let status = child.wait().await.ok().and_then(|s| s.code());
        closed(&mut record, path)?;
        return Err(
            Error::new("EMULATOR_EXITED", "Android Emulator exited at launch")
                .details(json!({"exitCode":status,"log":log})),
        );
    };
    record.pid = Some(pid);
    record.started = Some(started);
    record.phase = "owned".into();
    if let Err(error) = storage::write(path, &record) {
        // Unrecorded ownership cannot be recovered later; stop it now.
        let _ = stop(&adb, &record).await;
        return Err(uncertain().details(json!({"phase":"record_ownership","cause":error})));
    }
    // Reap the child whenever it exits so its identity check sees it gone.
    tokio::spawn(async move {
        let _ = child.wait().await;
    });
    let target = format!("android:{serial}");
    let waited = tokio::select! {
        result = wait_ready(&adb, &record, &target) => result,
        _ = cancelled.wait_for(|cancel| *cancel) => Err(Error::new("CANCELLED", "Emulator startup was cancelled")),
    };
    // Failure leaves the owned record for the registry's cleanup, which stops it.
    waited.map(|()| target).map_err(|error| {
        let details = json!({"cause":error,"log":log});
        let mut error = error;
        error.details = Some(details);
        error
    })
}

fn alive(record: &Record) -> bool {
    matches!((record.pid, record.started), (Some(pid), Some(started)) if process::start_time(pid) == Some(started))
}

async fn wait_ready(adb: &Android, record: &Record, target: &str) -> Result<()> {
    let serial = record.serial.as_deref().unwrap();
    let launched = Instant::now();
    let exited = || {
        Error::new(
            "EMULATOR_EXITED",
            "Android Emulator exited before it was usable",
        )
    };
    loop {
        if !alive(record) {
            return Err(exited());
        }
        match adb.avd_name(serial, Duration::from_secs(3)).await {
            Ok(name) if name == record.avd => break,
            Ok(_) => {
                return Err(Error::new(
                    "STALE_TARGET",
                    "Another emulator answered on the selected console port",
                ));
            }
            Err(_) if launched.elapsed() >= IDENTITY_BUDGET => {
                return Err(Error::new(
                    "TIMEOUT",
                    "The started emulator did not report its AVD to adb in time",
                ));
            }
            Err(_) => tokio::time::sleep(Duration::from_millis(500)).await,
        }
    }
    loop {
        if !alive(record) {
            return Err(exited());
        }
        let report = adb.doctor(target).await?;
        if report["ready"] == true {
            return Ok(());
        }
        if launched.elapsed() >= READY_BUDGET {
            return Err(Error::new(
                "TIMEOUT",
                "The started emulator did not become usable in time",
            )
            .details(json!({"readiness":report})));
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

/// Wait up to `budget` for the owned process to exit.
async fn exited(record: &Record, budget: Duration) -> bool {
    let deadline = Instant::now() + budget;
    loop {
        if !alive(record) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[cfg(unix)]
fn signal_group(record: &Record, signal: i32) {
    let Some(pid) = record.pid.and_then(|pid| i32::try_from(pid).ok()) else {
        return;
    };
    // SAFETY: getpgid only reads; the group is signalled only when the
    // identity-verified process still leads the group it was launched into.
    unsafe {
        if alive(record) && libc::getpgid(pid) == pid {
            libc::kill(-pid, signal);
        }
    }
}

/// Stop the identity-verified owned instance: console `emu kill` when the
/// console still names this AVD, then its process group. Forced termination
/// is data-safe because the instance is read-only.
async fn stop(adb: &Android, record: &Record) -> Result<()> {
    let Some(serial) = record.serial.as_deref() else {
        return Ok(());
    };
    if !alive(record) {
        return Ok(());
    }
    let console = adb
        .avd_name(serial, Duration::from_secs(3))
        .await
        .is_ok_and(|name| name == record.avd);
    if console
        && adb.emu_kill(serial).await.is_ok()
        && exited(record, Duration::from_secs(15)).await
    {
        return Ok(());
    }
    #[cfg(unix)]
    {
        signal_group(record, libc::SIGTERM);
        if exited(record, Duration::from_secs(5)).await {
            return Ok(());
        }
        signal_group(record, libc::SIGKILL);
        if exited(record, Duration::from_secs(5)).await {
            return Ok(());
        }
    }
    Err(Error::new(
        "CLEANUP_FAILED",
        "The owned emulator did not exit; retry cleanup",
    ))
}

async fn close_journal(path: &Path) -> Result<()> {
    let mut record = storage::read::<Record>(path)?.ok_or_else(uncertain)?;
    close_record(&mut record, path).await
}

async fn close_record(record: &mut Record, path: &Path) -> Result<()> {
    match record.phase.as_str() {
        "prepared" | "closed" => return Ok(()),
        "owned" => {}
        _ => {
            return Err(uncertain().details(json!({"serial":record.serial})));
        }
    }
    let adb = Android::default();
    stop(&adb, record).await?;
    // adb lists a stopped emulator briefly; wait for its serial to go. A later
    // instance on the same port is not this owner's and is left running.
    if let Some(serial) = &record.serial {
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline
            && adb
                .devices()
                .await
                .is_ok_and(|devices| devices.iter().any(|d| &d.serial == serial))
        {
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }
    closed(record, path)
}

/// Stop an instance a lost owner recorded, under fresh lease exclusion.
pub(crate) async fn recover(directory: &Path, target: &str) -> Result<()> {
    match std::fs::symlink_metadata(directory) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_) => {
            return Err(Error::new(
                "STATE_IO",
                "Cannot inspect emulator startup records",
            ));
        }
        Ok(_) => {}
    }
    let path = journal(directory, target);
    let Some(mut record) = storage::read::<Record>(&path)? else {
        return Ok(());
    };
    if record.target_id != target {
        return Err(Error::new(
            "UNAUTHORIZED",
            "Emulator start record does not belong to this owner",
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
            avd: "Acme".into(),
            window: false,
            data_policy: DATA_POLICY.into(),
            serial: Some("emulator-5554".into()),
            pid: None,
            started: None,
            phase: phase.into(),
        }
    }
    #[tokio::test]
    async fn owner_loss_before_launch_is_safe_but_dispatched_start_is_never_guessed() {
        let root = private_root();
        let target = format!("android:avd:Acme-{}", uuid::Uuid::new_v4().simple());
        let path = journal(root.path(), &target);
        recover(&root.path().join("not-allocated"), &target)
            .await
            .unwrap();
        for phase in ["prepared", "closed"] {
            storage::write(&path, &record(&target, phase)).unwrap();
            recover(root.path(), &target).await.unwrap();
        }
        storage::write(&path, &record(&target, "dispatched")).unwrap();
        assert_eq!(
            recover(root.path(), &target).await.unwrap_err().code,
            "EMULATOR_START_UNCERTAIN"
        );
        assert_eq!(
            storage::read::<Record>(&path).unwrap().unwrap().phase,
            "dispatched"
        );
        let _foreign = ControlLease::acquire(&target).unwrap();
        assert_eq!(
            recover(root.path(), &target).await.unwrap_err().code,
            "TARGET_BUSY"
        );
    }
    #[tokio::test]
    async fn foreign_records_do_not_authorize_a_stop() {
        let root = private_root();
        let target = format!("android:avd:Acme-{}", uuid::Uuid::new_v4().simple());
        storage::write(
            &journal(root.path(), &target),
            &record("android:avd:Foreign", "owned"),
        )
        .unwrap();
        assert_eq!(
            recover(root.path(), &target).await.unwrap_err().code,
            "UNAUTHORIZED"
        );
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn stop_signals_only_the_identity_verified_process() {
        use std::os::unix::process::CommandExt;
        // A replaced process ID with another start time is never signalled.
        let mut child = std::process::Command::new("/bin/sleep")
            .arg("30")
            .process_group(0)
            .spawn()
            .unwrap();
        let pid = child.id();
        let mut owned = record("android:avd:Acme", "owned");
        owned.serial = Some("Acme-no-console".into());
        owned.pid = Some(pid);
        owned.started = Some(process::start_time(pid).unwrap() + 1);
        let adb = Android {
            adb: PathBuf::from("/usr/bin/false"),
        };
        stop(&adb, &owned).await.unwrap();
        assert!(
            process::start_time(pid).is_some(),
            "foreign identity was signalled"
        );
        owned.started = process::start_time(pid);
        let stopping = tokio::spawn(async move { stop(&adb, &owned).await });
        // The verified group receives SIGTERM after the graceful wait.
        let status = tokio::task::spawn_blocking(move || child.wait().unwrap())
            .await
            .unwrap();
        assert!(!status.success());
        stopping.await.unwrap().unwrap();
    }
    #[test]
    fn port_selection_skips_listed_serials_and_bound_ports() {
        let mut listed = HashSet::new();
        let (first, first_lease) = free_port(&listed).expect("a free port in range");
        assert_eq!(first % 2, 0);
        // A port another Testrun process holds is skipped even before it binds.
        let (second, second_lease) = free_port(&listed).unwrap();
        assert_ne!(second, first);
        drop(first_lease);
        listed.insert(format!("emulator-{first}"));
        let held = std::net::TcpListener::bind(("127.0.0.1", first + 4)).unwrap();
        let (third, _) = free_port(&listed).unwrap();
        assert!(third != first && third != second && third != first + 4);
        drop((held, second_lease));
    }
}
