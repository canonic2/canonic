//! Emulator lifecycle through shipping `testrun mcp serve` against an owned fake
//! adb and emulator: explicit boot, read-only launch, responsive cancellation
//! and stops limited to the instance Testrun started.
use serde_json::{Value, json};
use std::{os::unix::fs::PermissionsExt, path::PathBuf, process::Stdio, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, ChildStdout, Command},
};
#[cfg(feature = "acceptance-android")]
mod native;

const ADB: &str = r#"#!/bin/sh
cd "$(dirname "$0")"
printf '%s\n' "$*" >> adb-calls
case "$1" in
devices)
  printf 'List of devices attached\n'
  for file in running-* external-*; do
    [ -f "$file" ] || continue
    port=${file#*-}
    printf 'emulator-%s device product:acme model:Acme_QA device:acme transport_id:%s\n' "$port" "$port"
  done ;;
-s)
  port=${2#emulator-}
  if [ -f "running-$port" ]; then name=$(cat "running-$port")
  elif [ -f "external-$port" ]; then name=$(cat "external-$port")
  else exit 1; fi
  case "$3" in
  emu)
    case "$4" in
    avd) printf '%s\nOK\n' "$name" ;;
    kill) [ -f "running-$port" ] && rm -f "running-$port"; printf 'OK: killing emulator, bye bye\nOK\n' ;;
    esac ;;
  exec-out)
    [ -f "booted-$port" ] || exit 1
    if [ "$4" = uiautomator ]; then cat tree; else cat screen.png; fi ;;
  shell)
    if [ "$4" = -T ]; then
      case "$5" in
        *io.canonic.testrun.focus.Main\ snapshot*) cat > /dev/null; cat tree; exit 0 ;;
      esac
    fi
    case "$4" in
    "'echo' 'testrun-readiness'") echo testrun-readiness ;;
    "'getprop' 'ro.build.version.release'") echo 16 ;;
    "'getprop' 'ro.build.version.sdk'") echo 36 ;;
    "'getprop' 'sys.boot_completed'") if [ -f "booted-$port" ]; then echo 1; else echo 0; fi ;;
    "'input' 'help'") printf 'Usage: input [<source>] <command> [<arg>...]\n' ;;
    esac ;;
  *) exit 2 ;;
  esac ;;
*) exit 3 ;;
esac
"#;

/// Records its arguments, refuses to start when `launch-fail` exists, and runs
/// until the console kill removes its marker. `boot-hold` delays readiness.
const EMULATOR: &str = r#"#!/bin/sh
cd "$(dirname "$0")"
printf '%s\n' "$*" >> emulator-calls
while [ $# -gt 0 ]; do
  case "$1" in -avd) avd=$2; shift ;; -port) port=$2; shift ;; esac
  shift
done
[ -f launch-fail ] && exit 1
echo $$ > "pid-$port"
trap 'rm -f "running-$port" "booted-$port"; exit 0' TERM
printf '%s' "$avd" > "running-$port"
while [ -f "running-$port" ]; do
  [ -f boot-hold ] || touch "booted-$port"
  sleep 0.05
done
rm -f "booted-$port"
"#;

struct Fixture {
    root: crate::Workspace,
    /// Control leases are host-wide, so each fixture owns distinct AVD names.
    avd: String,
    /// The MCP server, stopped when the fixture drops.
    _server: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
    seq: u64,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // A failing test must not leave a fake emulator running; the
        // workspace stops this test's owner process.
        let mut pids = Vec::new();
        if let Ok(entries) = std::fs::read_dir(self.root.path()) {
            pids.extend(entries.filter_map(Result::ok).filter_map(|entry| {
                let name = entry.file_name();
                name.to_str()?.strip_prefix("pid-")?;
                std::fs::read_to_string(entry.path())
                    .ok()?
                    .trim()
                    .parse::<i32>()
                    .ok()
            }));
        }
        for pid in pids {
            // SAFETY: each identity was recorded by or spawned for this fixture.
            unsafe {
                libc::kill(pid, libc::SIGKILL);
            }
        }
    }
}

impl Fixture {
    /// `task` binds a managed host task; without it the connection is the task.
    async fn start(task: bool) -> Self {
        let root = crate::Workspace::new();
        let path = root.path();
        for (name, script) in [("adb", ADB), ("emulator", EMULATOR)] {
            std::fs::write(path.join(name), script).unwrap();
            std::fs::set_permissions(path.join(name), std::fs::Permissions::from_mode(0o700))
                .unwrap();
        }
        image::RgbImage::new(100, 200)
            .save(path.join("screen.png"))
            .unwrap();
        std::fs::write(path.join("tree"), r#"<hierarchy><node text="Acme" class="android.widget.TextView" bounds="[0,0][100,200]"/></hierarchy>"#).unwrap();
        let home = path.join("avd");
        let avd = format!(
            "Acme-QA-{}",
            &uuid::Uuid::new_v4().simple().to_string()[..12]
        );
        for name in [avd.clone(), avd.replace("QA", "User")] {
            let directory = home.join(format!("{name}.avd"));
            std::fs::create_dir_all(&directory).unwrap();
            std::fs::write(
                home.join(format!("{name}.ini")),
                format!("path={}\ntarget=android-36\n", directory.display()),
            )
            .unwrap();
            std::fs::write(
                directory.join("config.ini"),
                "abi.type = x86_64\ntag.id = google_apis\n",
            )
            .unwrap();
        }
        let mut command = Command::new(env!("CARGO_BIN_EXE_testrun"));
        command.arg("--json");
        command
            .env("TESTRUN_ADB", path.join("adb"))
            .env("TESTRUN_EMULATOR", path.join("emulator"))
            .env("ANDROID_AVD_HOME", &home);
        Self::serve(root, avd, command, task).await
    }
    /// The host's adb, emulator and AVDs, for an explicitly owned AVD.
    #[cfg(feature = "acceptance-android")]
    async fn start_host(avd: &str) -> Self {
        assert!(
            avd.starts_with("Testrun-QA-"),
            "Only owned Testrun-QA- AVDs may be started by this test"
        );
        let binary = std::env::var_os("TESTRUN_TEST_BINARY")
            .unwrap_or_else(|| env!("CARGO_BIN_EXE_testrun").into());
        let command = Command::new(binary);
        Self::serve(crate::Workspace::new(), avd.into(), command, true).await
    }
    async fn serve(root: crate::Workspace, avd: String, mut command: Command, task: bool) -> Self {
        command
            .args(["mcp", "serve"])
            .env("TESTRUN_DATA_ROOT", root.data_root())
            .current_dir(root.path())
            .env("TMPDIR", root.path())
            .env_remove("CODEX_THREAD_ID")
            .env_remove("TESTRUN_TASK_ID")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        if task {
            command.env(
                "TESTRUN_TASK_ID",
                format!("testrun-emulator-{}", uuid::Uuid::new_v4()),
            );
        }
        let mut child = command.spawn().unwrap();
        let mut fixture = Self {
            input: child.stdin.take().unwrap(),
            output: BufReader::new(child.stdout.take().unwrap()),
            _server: child,
            root,
            avd,
            seq: 0,
        };
        fixture.request("initialize", json!({"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"Acme emulator fixture","version":"1"}})).await;
        fixture
            .send(json!({"jsonrpc":"2.0","method":"notifications/initialized"}))
            .await;
        fixture
    }
    fn target(&self) -> String {
        format!("android:avd:{}", self.avd)
    }
    /// A second AVD that stands in for a user's own and must never start.
    fn user_avd(&self) -> String {
        self.avd.replace("QA", "User")
    }
    fn path(&self, name: &str) -> PathBuf {
        self.root.path().join(name)
    }
    async fn send(&mut self, message: Value) {
        let mut bytes = serde_json::to_vec(&message).unwrap();
        bytes.push(b'\n');
        self.input.write_all(&bytes).await.unwrap();
    }
    async fn begin(&mut self, method: &str, params: Value) -> u64 {
        self.seq += 1;
        let id = self.seq;
        self.send(json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))
            .await;
        id
    }
    async fn response(&mut self, id: u64) -> Value {
        loop {
            let mut line = String::new();
            // Longer than managed connect's own 90-second deadline.
            tokio::time::timeout(Duration::from_secs(150), self.output.read_line(&mut line))
                .await
                .expect("response deadline")
                .unwrap();
            let response: Value = serde_json::from_str(&line).unwrap();
            if response["id"] == id {
                return response;
            }
        }
    }
    async fn cancellation_barrier(&mut self) {
        let id = self.begin("ping", json!({})).await;
        loop {
            let mut line = String::new();
            tokio::time::timeout(Duration::from_secs(5), self.output.read_line(&mut line))
                .await
                .unwrap()
                .unwrap();
            let response: Value = serde_json::from_str(&line).unwrap();
            if response.get("id").is_none() {
                continue;
            }
            assert_eq!(
                response["id"], id,
                "Cancelled requests must emit no reply: {response}"
            );
            assert_eq!(response["result"], json!({}));
            return;
        }
    }
    async fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.begin(method, params).await;
        self.response(id).await
    }
    async fn call(&mut self, name: &str, arguments: Value) -> Value {
        self.request("tools/call", json!({"name":name,"arguments":arguments}))
            .await["result"]["structuredContent"]
            .clone()
    }
    fn lines(&self, name: &str) -> Vec<String> {
        std::fs::read_to_string(self.path(name))
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect()
    }
    /// Console ports of fake emulator processes still running.
    fn running(&self) -> Vec<String> {
        let mut ports: Vec<_> = std::fs::read_dir(self.root.path())
            .unwrap()
            .filter_map(Result::ok)
            .filter_map(|entry| {
                entry
                    .file_name()
                    .to_str()
                    .and_then(|name| name.strip_prefix("running-"))
                    .map(str::to_owned)
            })
            .collect();
        ports.sort();
        ports
    }
    fn pid(&self, port: &str) -> i32 {
        std::fs::read_to_string(self.path(&format!("pid-{port}")))
            .unwrap()
            .trim()
            .parse()
            .unwrap()
    }
}

fn alive(pid: i32) -> bool {
    // SAFETY: signal zero only observes this fixture's recorded process.
    unsafe { libc::kill(pid, 0) == 0 }
}

async fn until(what: &str, mut condition: impl FnMut() -> bool) {
    tokio::time::timeout(Duration::from_secs(30), async {
        while !condition() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("timed out waiting for {what}"));
}

impl Fixture {
    /// Device actors the manager still runs, from its own report.
    async fn actors(&self) -> u64 {
        let output = Command::new(env!("CARGO_BIN_EXE_testrun"))
            .arg("--json")
            .args(["manager", "status"])
            .env("TESTRUN_DATA_ROOT", self.root.data_root())
            .output()
            .await
            .unwrap();
        let status: Value = serde_json::from_slice(&output.stdout).unwrap();
        status["owner"]["actors"].as_u64().unwrap_or(0)
    }
}

#[tokio::test]
async fn managed_boot_starts_read_only_and_disconnect_stops_only_the_owned_instance() {
    let mut fixture = Fixture::start(true).await;
    std::fs::write(fixture.path("external-5554"), "Acme-Other").unwrap();
    let listed = fixture.call("target_list", json!({"kind":"android"})).await;
    assert_eq!(listed["ok"], true, "{listed}");
    let targets = listed["data"]["targets"].as_array().unwrap();
    let avd = targets
        .iter()
        .find(|t| t["targetId"] == fixture.target())
        .unwrap_or_else(|| panic!("{listed}"));
    assert_eq!(avd["state"], "Available");
    assert_eq!(avd["runtime"], "android-36");
    assert_eq!(avd["avd"]["dataPolicy"], "read-only");
    assert_eq!(avd["avd"]["emulator"]["state"], "ready");
    assert_eq!(avd["capabilities"]["tap"]["state"], "unsupported");
    assert!(
        targets
            .iter()
            .any(|t| t["targetId"] == "android:emulator-5554")
    );
    assert!(
        fixture.lines("emulator-calls").is_empty(),
        "discovery started a process"
    );

    let refused = fixture
        .call("connect", json!({"targetId":fixture.target()}))
        .await;
    assert_eq!(refused["code"], "BOOT_REQUIRED", "{refused}");
    let refused = fixture
        .call(
            "connect",
            json!({"targetId":"android:emulator-5554","window":true}),
        )
        .await;
    assert_eq!(refused["code"], "INVALID_ARGUMENT", "{refused}");
    let doctor = fixture
        .call("doctor", json!({"targetId":fixture.target()}))
        .await;
    assert_eq!(doctor["code"], "INVALID_ARGUMENT", "{doctor}");

    let connected = fixture
        .call("connect", json!({"targetId":fixture.target(),"boot":true}))
        .await;
    assert_eq!(connected["ok"], true, "{connected}");
    assert_eq!(connected["data"]["state"], "ready", "{connected}");
    let ports = fixture.running();
    assert_eq!(ports.len(), 1, "{ports:?}");
    assert_ne!(ports[0], "5554", "an external console port was reused");
    let launches = fixture.lines("emulator-calls");
    assert_eq!(launches.len(), 1, "{launches:?}");
    let selected = format!("-avd {}", fixture.avd);
    for flag in [&selected, "-read-only", "-no-snapshot-save", "-no-window"] {
        assert!(launches[0].contains(flag), "{flag}: {launches:?}");
    }
    assert!(!launches[0].contains(&fixture.user_avd()), "{launches:?}");
    let pid = fixture.pid(&ports[0]);
    let status = fixture.call("status", json!({})).await;
    assert_eq!(status["ok"], true, "{status}");

    let closed = fixture.call("disconnect", json!({})).await;
    assert_eq!(closed["ok"], true, "{closed}");
    assert_eq!(closed["data"]["closed"], true, "{closed}");
    assert!(fixture.running().is_empty());
    until("owned emulator exit", || !alive(pid)).await;
    assert!(
        fixture.path("external-5554").exists(),
        "external instance was stopped"
    );
    assert!(
        fixture
            .lines("adb-calls")
            .iter()
            .all(|call| !call.starts_with("-s emulator-5554 emu kill")),
        "external console received a stop"
    );
    assert_eq!(fixture.lines("emulator-calls").len(), 1);
}

#[tokio::test]
async fn an_avd_running_elsewhere_is_refused_and_a_window_is_explicit() {
    let mut fixture = Fixture::start(true).await;
    std::fs::write(fixture.path("external-5554"), &fixture.avd).unwrap();
    let refused = fixture
        .call("connect", json!({"targetId":fixture.target(),"boot":true}))
        .await;
    assert_eq!(refused["code"], "AVD_ALREADY_RUNNING", "{refused}");
    assert!(
        refused.to_string().contains("android:emulator-5554"),
        "{refused}"
    );
    assert!(fixture.lines("emulator-calls").is_empty());
    assert!(fixture.path("external-5554").exists());

    std::fs::remove_file(fixture.path("external-5554")).unwrap();
    let connected = fixture
        .call(
            "connect",
            json!({"targetId":fixture.target(),"boot":true,"window":true}),
        )
        .await;
    assert_eq!(connected["ok"], true, "{connected}");
    let launches = fixture.lines("emulator-calls");
    assert!(!launches[0].contains("-no-window"), "{launches:?}");
    assert!(launches[0].contains("-read-only"), "{launches:?}");
    let closed = fixture.call("disconnect", json!({})).await;
    assert_eq!(closed["ok"], true, "{closed}");
    assert!(fixture.running().is_empty());
}

#[tokio::test]
async fn cancelled_start_keeps_requests_responsive_and_stops_the_started_instance() {
    let mut fixture = Fixture::start(false).await;
    std::fs::write(fixture.path("boot-hold"), "held").unwrap();
    let id = fixture
        .begin(
            "tools/call",
            json!({"name":"connect","arguments":{"targetId":fixture.target(),"boot":true}}),
        )
        .await;
    until("emulator launch", || !fixture.running().is_empty()).await;
    let port = fixture.running()[0].clone();
    let pid = fixture.pid(&port);
    assert!(fixture.request("ping", json!({})).await["result"].is_object());
    fixture
        .send(json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":id}}))
        .await;
    fixture.cancellation_barrier().await;
    assert!(fixture.request("ping", json!({})).await["result"].is_object());
    // The cancelled request does not abandon the started instance: disconnect
    // stops it through its console.
    std::fs::remove_file(fixture.path("boot-hold")).unwrap();
    let closed = fixture.call("disconnect", json!({})).await;
    assert_eq!(closed["ok"], true, "{closed}");
    until("cancelled emulator exit", || !alive(pid)).await;
    assert!(fixture.running().is_empty());
    assert!(
        fixture
            .lines("adb-calls")
            .iter()
            .any(|call| call == &format!("-s emulator-{port} emu kill")),
        "the owned instance was not stopped through its console"
    );
    assert_eq!(
        fixture.lines("emulator-calls").len(),
        1,
        "start was replayed"
    );
}

#[tokio::test]
async fn a_repeated_connect_awaits_a_start_that_outlasted_the_first_request() {
    let mut fixture = Fixture::start(true).await;
    std::fs::write(fixture.path("boot-hold"), "held").unwrap();
    let args = json!({"name":"connect","arguments":{"targetId":fixture.target(),"boot":true}});
    let first = fixture.begin("tools/call", args.clone()).await;
    until("emulator launch", || !fixture.running().is_empty()).await;
    // Cancelling stands in for the caller's deadline; the owner keeps starting.
    fixture
        .send(json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":first}}))
        .await;
    fixture.cancellation_barrier().await;
    let second = fixture.begin("tools/call", args).await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    std::fs::remove_file(fixture.path("boot-hold")).unwrap();
    let connected = fixture.response(second).await["result"]["structuredContent"].clone();
    assert_eq!(connected["ok"], true, "{connected}");
    assert_eq!(connected["data"]["state"], "ready", "{connected}");
    assert_eq!(
        fixture.lines("emulator-calls").len(),
        1,
        "start was replaced"
    );
    assert_eq!(fixture.running().len(), 1);
    let closed = fixture.call("disconnect", json!({})).await;
    assert_eq!(closed["ok"], true, "{closed}");
    assert!(fixture.running().is_empty());
}

#[tokio::test]
async fn launch_failure_is_reported_without_a_retained_owner() {
    let mut fixture = Fixture::start(false).await;
    std::fs::write(fixture.path("launch-fail"), "fault").unwrap();
    let opened = fixture
        .call("connect", json!({"targetId":fixture.target(),"boot":true}))
        .await;
    assert_eq!(opened["ok"], false, "{opened}");
    assert_eq!(opened["code"], "EMULATOR_EXITED", "{opened}");
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    while fixture.actors().await != 0 {
        assert!(
            tokio::time::Instant::now() < deadline,
            "a device actor was retained"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let status = fixture.call("status", json!({})).await;
    assert_ne!(status["data"]["state"], "ready", "{status}");
    assert!(fixture.running().is_empty());
}

#[tokio::test]
async fn disconnect_after_owner_death_stops_the_recorded_instance() {
    let mut fixture = Fixture::start(true).await;
    let connected = fixture
        .call("connect", json!({"targetId":fixture.target(),"boot":true}))
        .await;
    assert_eq!(connected["ok"], true, "{connected}");
    let port = fixture.running()[0].clone();
    let pid = fixture.pid(&port);
    let owner = fixture.root.owner_pid().expect("A running owner process");
    // SAFETY: the process is this test's own owner process, from its record.
    unsafe {
        libc::kill(owner, libc::SIGKILL);
    }
    until("owner exit", || !alive(owner)).await;
    assert!(alive(pid), "the emulator should outlive its owner");
    let closed = fixture.call("disconnect", json!({})).await;
    assert_eq!(closed["ok"], true, "{closed}");
    until("recorded emulator exit", || !alive(pid)).await;
    assert!(fixture.running().is_empty());
}

#[tokio::test]
async fn the_next_manager_stops_an_emulator_a_killed_manager_left_running() {
    let mut fixture = Fixture::start(true).await;
    let connected = fixture
        .call("connect", json!({"targetId":fixture.target(),"boot":true}))
        .await;
    assert_eq!(connected["ok"], true, "{connected}");
    let port = fixture.running()[0].clone();
    let pid = fixture.pid(&port);
    let owner = fixture.root.owner_pid().expect("A running manager");
    // SAFETY: the process is this test's own manager, from its record.
    unsafe {
        libc::kill(owner, libc::SIGKILL);
    }
    until("manager exit", || !alive(owner)).await;
    assert!(alive(pid), "the emulator should outlive its manager");
    // Any request that starts a new manager settles what the lost one left,
    // without anyone disconnecting or reconnecting the device.
    let observed = fixture
        .call("observe", json!({"targetId":"android:Acme-absent"}))
        .await;
    assert_eq!(observed["code"], "NOT_CONNECTED", "{observed}");
    until("recorded emulator exit", || !alive(pid)).await;
    assert!(fixture.running().is_empty());
}
