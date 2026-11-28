//! Real custom-set boot/capture/cleanup using only a simulator created by this test.
use super::*;
use sha2::{Digest, Sha256};
use std::{path::Path, process::Command as SyncCommand};

struct Owned {
    root: tempfile::TempDir,
    /// This test's data root and owner process.
    workspace: crate::Workspace,
    set: PathBuf,
    uuid: String,
    task: String,
    cleaned: bool,
}
fn command(set: Option<&Path>, args: &[&str]) -> Vec<u8> {
    let mut command = SyncCommand::new("/usr/bin/xcrun");
    command.arg("simctl");
    if let Some(set) = set {
        command.arg("--set").arg(set);
    }
    let output = command.args(args).output().unwrap();
    assert!(
        output.status.success(),
        "simctl {}: {}",
        args[0],
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}
impl Owned {
    fn kill_owner(&self, target: &str) {
        // Use only this fixture's durable identities, never process-name scans.
        // SAFETY: read-only query of this test's effective user identity.
        let uid = unsafe { libc::geteuid() };
        let directory = self.workspace.target_records();
        let selected: Value =
            serde_json::from_slice(&std::fs::read(directory.join("selected.json")).unwrap())
                .unwrap();
        assert_eq!(selected["target"], target);
        assert_eq!(selected["session"], self.task);
        let authority = format!("managed:{target}");
        let anchor = format!("{:x}", Sha256::digest(authority.as_bytes()));
        let lease: Value = serde_json::from_slice(
            &std::fs::read(format!("/tmp/testrun-control-{uid}/{anchor}.lock")).unwrap(),
        )
        .unwrap();
        assert_eq!(lease["targetId"], target);
        assert_eq!(lease["task"], selected["task"]);
        assert_eq!(lease["epoch"], selected["epoch"]);
        use std::os::{fd::AsRawFd, unix::fs::OpenOptionsExt};
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(format!("/tmp/testrun-control-{uid}/{anchor}.lock"))
            .unwrap();
        // SAFETY: probe only the fixture's opened anchor; do not signal a stale PID.
        let locked = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        if locked == 0 {
            // SAFETY: release only this test's unexpectedly acquired probe lock.
            unsafe {
                libc::flock(file.as_raw_fd(), libc::LOCK_UN);
            }
            panic!("Fixture owner released its authority before the loss check");
        }
        assert_eq!(
            std::io::Error::last_os_error().kind(),
            std::io::ErrorKind::WouldBlock
        );
        // The device actor's lease belongs to this test's owner process.
        let pid = lease["pid"].as_u64().unwrap();
        assert_eq!(
            Some(pid),
            self.workspace
                .owner_pid()
                .and_then(|owner| u64::try_from(owner).ok())
        );
        assert!(
            SyncCommand::new("kill")
                .args(["-KILL", &pid.to_string()])
                .status()
                .unwrap()
                .success()
        );
    }
    fn create() -> Self {
        let root = tempfile::Builder::new()
            .prefix("testrun-lifecycle-")
            .tempdir_in("/private/tmp")
            .unwrap();
        let set = root.path().join("Acme device set");
        std::fs::create_dir(&set).unwrap();
        let inventory: Value =
            serde_json::from_slice(&command(None, &["list", "runtimes", "--json"])).unwrap();
        let runtime = inventory["runtimes"]
            .as_array()
            .unwrap()
            .iter()
            .rev()
            .find(|runtime| {
                runtime["isAvailable"] == true
                    && runtime["identifier"]
                        .as_str()
                        .is_some_and(|id| id.contains(".iOS-"))
            })
            .expect("Install an iOS simulator runtime before lifecycle acceptance")["identifier"]
            .as_str()
            .unwrap();
        let name = format!("Testrun-QA-Lifecycle-{}", uuid::Uuid::new_v4().simple());
        // An explicit device type selects another family, such as an iPad.
        let device_type = std::env::var("TESTRUN_TEST_DEVICE_TYPE")
            .unwrap_or_else(|_| "com.apple.CoreSimulator.SimDeviceType.iPhone-16".into());
        eprintln!("Lifecycle fixture: {runtime}, {device_type}, isolated custom device set");
        let uuid = String::from_utf8(command(
            Some(&set),
            &["create", &name, &device_type, runtime],
        ))
        .unwrap()
        .trim()
        .to_owned();
        Self {
            root,
            workspace: crate::Workspace::new(),
            set,
            uuid,
            task: format!("Acme-lifecycle-{}", uuid::Uuid::new_v4()),
            cleaned: false,
        }
    }
    async fn cli(&self, args: &[&str]) -> Value {
        let executable = std::env::var_os("TESTRUN_TEST_BINARY")
            .unwrap_or_else(|| env!("CARGO_BIN_EXE_testrun").into());
        let output = tokio::time::timeout(
            Duration::from_secs(180),
            Command::new(executable)
                .arg("--json")
                .args(args)
                .env("TESTRUN_DATA_ROOT", self.workspace.data_root())
                .env("TESTRUN_SIMULATOR_DEVICE_SET", &self.set)
                .env("TESTRUN_TASK_ID", &self.task)
                .env_remove("CODEX_THREAD_ID")
                .env("TMPDIR", self.root.path())
                .kill_on_drop(true)
                .output(),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(
            output.status.success(),
            "CLI {}: {}",
            args[0],
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }
    async fn finish(&mut self) {
        assert_eq!(self.cli(&["disconnect"]).await["closed"], true);
        command(Some(&self.set), &["shutdown", &self.uuid]);
        command(Some(&self.set), &["delete", &self.uuid]);
        self.cleaned = true;
    }
}
impl Drop for Owned {
    fn drop(&mut self) {
        if self.cleaned {
            return;
        }
        let executable = std::env::var_os("TESTRUN_TEST_BINARY")
            .unwrap_or_else(|| env!("CARGO_BIN_EXE_testrun").into());
        let _ = SyncCommand::new(executable)
            .arg("--json")
            .arg("disconnect")
            .env("TESTRUN_DATA_ROOT", self.workspace.data_root())
            .env("TESTRUN_TASK_ID", &self.task)
            .env("TESTRUN_SIMULATOR_DEVICE_SET", &self.set)
            .env("TMPDIR", self.root.path())
            .output();
        let _ = SyncCommand::new("/usr/bin/xcrun")
            .arg("simctl")
            .arg("--set")
            .arg(&self.set)
            .args(["shutdown", &self.uuid])
            .output();
        let result = SyncCommand::new("/usr/bin/xcrun")
            .arg("simctl")
            .arg("--set")
            .arg(&self.set)
            .args(["delete", &self.uuid])
            .output();
        if !result.is_ok_and(|result| result.status.success()) {
            eprintln!("Owned lifecycle simulator deletion failed: {}", self.uuid);
        }
    }
}

#[tokio::test]
async fn owned_custom_set_native_boot_capture_and_cli_disconnect() {
    let mut fixture = Owned::create();
    let mut client = Client::start_environment(
        fixture.root.path(),
        Duration::from_secs(180),
        None,
        &[("TESTRUN_SIMULATOR_DEVICE_SET", fixture.set.as_os_str())],
    )
    .await;
    let listed = client
        .call("target_list", json!({"kind":"simulator"}))
        .await;
    assert_eq!(listed["ok"], true, "{listed}");
    let target = listed["data"]["targets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|target| {
            target["targetId"]
                .as_str()
                .is_some_and(|id| id.ends_with(&format!(":{}", fixture.uuid)))
        })
        .expect("Newly created owned simulator was not discovered");
    assert_eq!(target["deviceSetPath"], fixture.set.to_str().unwrap());
    let target_id = target["targetId"].as_str().unwrap().to_owned();
    // A first boot can outlast one connect deadline; connecting again awaits it.
    let mut opened = Value::Null;
    for _ in 0..3 {
        opened = client
            .call("connect", json!({"targetId":target_id,"boot":true}))
            .await;
        if !matches!(opened["code"].as_str(), Some("TIMEOUT" | "RECOVERING")) {
            break;
        }
    }
    assert_eq!(opened["ok"], true, "{opened}");
    assert_eq!(
        opened["data"]["capabilities"]["input"]["state"], "supported",
        "{opened}"
    );
    let image = client.call("observe", json!({"profile":"fast"})).await;
    assert_eq!(image["ok"], true, "{image}");
    assert!(image["data"]["observation"]["width"].as_u64().unwrap() > 0);
    assert_eq!(image["data"]["observation"]["profile"], "fast");
    assert_eq!(client.call("disconnect", json!({})).await["ok"], true);
    let listed = client
        .call("target_list", json!({"kind":"simulator"}))
        .await;
    assert_eq!(
        listed["data"]["targets"][0]["state"], "Shutdown",
        "the owned boot is shut down: {listed}"
    );
    let connected = fixture
        .cli(&["connect", "--target", &target_id, "--boot"])
        .await;
    assert_eq!(connected["state"], "ready", "{connected}");
    let evidence = fixture.cli(&["observe", "--profile", "evidence"]).await;
    assert_eq!(evidence["observation"]["profile"], "evidence", "{evidence}");
    fixture.kill_owner(&target_id);
    assert_eq!(fixture.cli(&["disconnect"]).await["closed"], true);
    let listed = client
        .call("target_list", json!({"kind":"simulator"}))
        .await;
    assert_eq!(
        listed["data"]["targets"][0]["state"], "Shutdown",
        "{listed}"
    );
    // Externally booted resources remain running after the same connect intent.
    command(Some(&fixture.set), &["boot", &fixture.uuid]);
    command(Some(&fixture.set), &["bootstatus", &fixture.uuid, "-b"]);
    assert_eq!(
        fixture
            .cli(&["connect", "--target", &target_id, "--boot"])
            .await["state"],
        "ready"
    );
    assert_eq!(fixture.cli(&["disconnect"]).await["closed"], true);
    let listed = client
        .call("target_list", json!({"kind":"simulator"}))
        .await;
    assert_eq!(listed["data"]["targets"][0]["state"], "Booted", "{listed}");
    drop(client.input);
    assert!(client.child.wait().await.unwrap().success());
    fixture.finish().await;
}
