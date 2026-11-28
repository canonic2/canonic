//! Owned-AVD lifecycle against the host's real emulator: cold boot with a
//! window, forced termination of an unresponsive instance and a lost owner.
//! Each case checks that the AVD's guest data images are byte-identical after.
use super::*;
use std::{collections::BTreeMap, path::Path, time::Instant};

fn avd(variable: &str) -> String {
    std::env::var(variable).unwrap_or_else(|_| panic!("Set {variable} to an owned Testrun-QA- AVD"))
}

fn avd_directory(name: &str) -> PathBuf {
    std::env::var_os("ANDROID_AVD_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var("HOME").unwrap()).join(".android/avd"))
        .join(format!("{name}.avd"))
}

/// Guest data and snapshot images; emulator bookkeeping files are excluded.
fn guest(relative: &str) -> bool {
    ["userdata-qemu.img", "encryptionkey.img", "cache.img"]
        .iter()
        .any(|prefix| relative.starts_with(prefix))
        || relative.starts_with("snapshots/") && !relative.ends_with("snapshot.pb")
}

/// Content hashes of the AVD's guest data images, by relative path.
fn guest_data(name: &str) -> BTreeMap<String, String> {
    fn walk(root: &Path, directory: &Path, hashes: &mut BTreeMap<String, String>) {
        for entry in std::fs::read_dir(directory).unwrap().filter_map(Result::ok) {
            let path = entry.path();
            if entry.file_type().unwrap().is_dir() {
                walk(root, &path, hashes);
                continue;
            }
            let relative = path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .into_owned();
            if !guest(&relative) {
                continue;
            }
            // The system tool hashes multi-gigabyte images far faster than an
            // unoptimized test build.
            let output = std::process::Command::new("shasum")
                .args(["-a", "256"])
                .arg(&path)
                .output()
                .unwrap();
            assert!(output.status.success(), "{}", path.display());
            let digest = String::from_utf8_lossy(&output.stdout)
                .split_whitespace()
                .next()
                .unwrap()
                .to_owned();
            hashes.insert(relative, digest);
        }
    }
    let root = avd_directory(name);
    let mut hashes = BTreeMap::new();
    walk(&root, &root, &mut hashes);
    assert!(
        hashes
            .keys()
            .any(|path| path.starts_with("userdata-qemu.img"))
    );
    hashes
}

/// Emulator processes launched read-only for `name`, with their command lines.
fn instances(name: &str) -> Vec<(i32, String)> {
    let output = std::process::Command::new("ps")
        .args(["-axo", "pid=,command="])
        .output()
        .unwrap();
    let selected = format!("-avd {name} ");
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| line.contains(&selected) && line.contains("-read-only"))
        .filter_map(|line| {
            let line = line.trim_start();
            let (pid, command) = line.split_once(' ')?;
            Some((pid.parse().ok()?, command.to_owned()))
        })
        .collect()
}

/// Stops a leftover instance of the owned AVD if a case fails midway.
struct Leftovers(String);
impl Drop for Leftovers {
    fn drop(&mut self) {
        for (pid, _) in instances(&self.0) {
            // SAFETY: the process is a read-only instance of this test's owned AVD.
            unsafe {
                libc::kill(pid, libc::SIGCONT);
                libc::kill(pid, libc::SIGKILL);
            }
        }
    }
}

async fn wait(what: &str, budget: Duration, mut condition: impl FnMut() -> bool) {
    tokio::time::timeout(budget, async {
        while !condition() {
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("timed out waiting for {what}"));
}

/// Connect with boot, repeating after a deadline as an agent would.
async fn connect(fixture: &mut Fixture, window: bool) -> Value {
    for _ in 0..4 {
        let connected = fixture
            .call(
                "connect",
                json!({"targetId":fixture.target(),"boot":true,"window":window}),
            )
            .await;
        if connected["code"] != "TIMEOUT" {
            return connected;
        }
    }
    panic!("connect did not finish after repeated deadlines");
}

fn single_instance(name: &str) -> (i32, String) {
    let instances = instances(name);
    assert_eq!(instances.len(), 1, "{instances:?}");
    instances.into_iter().next().unwrap()
}

#[tokio::test]
async fn cold_boot_with_a_window_reaches_readiness_and_keeps_guest_data() {
    let name = avd("TESTRUN_TEST_COLD_AVD");
    assert!(instances(&name).is_empty(), "the AVD is already running");
    let before = guest_data(&name);
    assert!(
        !before.keys().any(|path| path.starts_with("snapshots/")),
        "the cold-boot AVD has a saved snapshot"
    );
    let _leftovers = Leftovers(name.clone());
    let mut fixture = Fixture::start_host(&name).await;
    let started = Instant::now();
    let connected = connect(&mut fixture, true).await;
    assert_eq!(connected["ok"], true, "{connected}");
    eprintln!("cold boot ready after {:?}", started.elapsed());
    let (pid, command) = single_instance(&name);
    assert!(!command.contains("-no-window"), "{command}");
    assert!(command.contains("-no-snapshot-save"), "{command}");
    let observed = fixture.call("observe", json!({"profile":"evidence"})).await;
    assert_eq!(observed["ok"], true, "{observed}");
    let closed = fixture.call("disconnect", json!({})).await;
    assert_eq!(closed["ok"], true, "{closed}");
    wait("cold-booted emulator exit", Duration::from_secs(30), || {
        !alive(pid)
    })
    .await;
    assert!(instances(&name).is_empty());
    assert_eq!(guest_data(&name), before);
}

#[tokio::test]
async fn an_unresponsive_instance_is_terminated_and_keeps_guest_data() {
    let name = avd("TESTRUN_TEST_AVD");
    assert!(instances(&name).is_empty(), "the AVD is already running");
    let before = guest_data(&name);
    let _leftovers = Leftovers(name.clone());
    let mut fixture = Fixture::start_host(&name).await;
    let connected = connect(&mut fixture, false).await;
    assert_eq!(connected["ok"], true, "{connected}");
    let (pid, command) = single_instance(&name);
    assert!(command.contains("-no-window"), "{command}");
    // A stopped process cannot answer its console, so cleanup must fall back
    // to signalling the verified process group.
    // SAFETY: the process is this case's own read-only emulator instance.
    unsafe {
        libc::kill(pid, libc::SIGSTOP);
    }
    let started = Instant::now();
    let closed = fixture.call("disconnect", json!({})).await;
    assert_eq!(closed["ok"], true, "{closed}");
    eprintln!("forced stop finished after {:?}", started.elapsed());
    wait("terminated emulator exit", Duration::from_secs(30), || {
        !alive(pid)
    })
    .await;
    assert!(instances(&name).is_empty());
    assert_eq!(guest_data(&name), before);
}

#[tokio::test]
async fn a_lost_owner_instance_is_stopped_on_disconnect() {
    let name = avd("TESTRUN_TEST_AVD");
    assert!(instances(&name).is_empty(), "the AVD is already running");
    let before = guest_data(&name);
    let _leftovers = Leftovers(name.clone());
    let mut fixture = Fixture::start_host(&name).await;
    let connected = connect(&mut fixture, false).await;
    assert_eq!(connected["ok"], true, "{connected}");
    let (pid, _) = single_instance(&name);
    let owner = fixture.root.owner_pid().expect("A running owner process");
    // SAFETY: the process is this test's own owner process, from its record.
    unsafe {
        libc::kill(owner, libc::SIGKILL);
    }
    wait("owner exit", Duration::from_secs(10), || !alive(owner)).await;
    assert!(alive(pid), "the emulator should outlive its owner");
    let closed = fixture.call("disconnect", json!({})).await;
    assert_eq!(closed["ok"], true, "{closed}");
    wait("recorded emulator exit", Duration::from_secs(30), || {
        !alive(pid)
    })
    .await;
    assert!(instances(&name).is_empty());
    assert_eq!(guest_data(&name), before);
}
