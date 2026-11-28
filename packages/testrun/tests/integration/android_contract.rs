//! Published Android MCP contracts through `testrun mcp serve` and an owned fake adb.
use serde_json::{Value, json};
use std::{os::unix::fs::PermissionsExt, process::Stdio, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines},
    process::{Child, ChildStdin, ChildStdout, Command},
};

mod app_lifecycle;
mod control;
mod host_lifecycle;
mod network_recovery;
mod readiness;
mod recovery;
mod scheduling;
mod semantic;
mod target_scheduling;
mod text_focus;

const ADB: &str = r##"#!/bin/sh
set -eu
cd "$(dirname "$0")"
# Connectivity switches live in files; the readback derives Android's override
# values (wifi_on 3, bluetooth_on 2) and the default network from them.
readback() {
  ap=$(cat airplane); wifi=$(cat wifi); data=$(cat data); bt=$(cat bt)
  printf '#airplane\n'; if [ "$ap" = 1 ]; then echo enabled; else echo disabled; fi
  printf '#airplane_on\n%s\n' "$ap"
  printf '#wifi_on\n'; if [ "$ap" = 1 ] && [ "$wifi" = 1 ]; then echo 3; else echo "$wifi"; fi
  printf '#mobile_data\n%s\n' "$data"
  printf '#bluetooth_on\n'; if [ "$ap" = 1 ] && [ "$bt" = 1 ]; then echo 2; else echo "$bt"; fi
  printf '#wifi\n'; if [ "$ap" = 0 ] && [ "$wifi" = 1 ]; then printf 'Wifi is enabled\nWifi is connected to "Acme"\n'; else printf 'Wifi is disabled\nWifi is not connected\n'; fi
  printf '#bluetooth\n'; if [ "$ap" = 0 ] && [ "$bt" = 1 ]; then echo '    state: ON'; else echo '    state: OFF'; fi
  printf '#default\n'; if [ "$ap" = 0 ] && { [ "$wifi" = 1 ] || [ "$data" = 1 ]; }; then echo '  Active default network: 100'; else echo '  Active default network: none'; fi
  printf '#end\n'
}
case "$1" in
devices) printf 'List of devices attached\nAcme-contract device usb:1-2 model:Acme transport_id:9\n' ;;
-s) case "$3" in
  exec-out) cat screen.png ;;
  shell) printf '%s\n' "$4" >> input.log
    case "$4" in
    "'getprop' 'ro.build.version.sdk'") cat sdk ;;
    *"#airplane"*)
      # One-shot fault: the first readback after airplane mode is on fails.
      if [ -f fail-readback-offline ] && [ "$(cat airplane)" = 1 ]; then rm fail-readback-offline; exit 1; fi
      readback ;;
    "'cmd' 'connectivity' 'airplane-mode' 'enable'") echo 1 > airplane ;;
    "'cmd' 'connectivity' 'airplane-mode' 'disable'") echo 0 > airplane ;;
    "'svc' 'wifi' 'enable'") echo 1 > wifi ;;
    "'svc' 'wifi' 'disable'") echo 0 > wifi ;;
    "'svc' 'data' 'enable'") echo 1 > data ;;
    "'svc' 'data' 'disable'")
      # One-shot fault: adb refuses the second radio command.
      if [ -f fail-data-disable ]; then rm fail-data-disable; exit 1; fi
      echo 0 > data ;;
    # App lifecycle: launch focuses the app's window unless no-focus is armed;
    # force-stop returns focus to the launcher.
    *"'resolve-activity'"*) printf 'priority=0 preferredOrder=0\ncom.example.acme/.MainActivity\n' ;;
    "'am' 'start' '-W' '-n' "*)
      if [ -f fail-launch ]; then rm fail-launch; exit 1; fi
      [ -f no-focus ] || echo com.example.acme > focus
      printf 'Status: ok\nLaunchState: COLD\n' ;;
    "'am' 'force-stop' "*) rm -f focus ;;
    *mCurrentFocus*) if [ -f focus ]; then printf '  mCurrentFocus=Window{1a u0 %s/.MainActivity}\n' "$(cat focus)"
      else printf '  mCurrentFocus=Window{2b u0 com.example.launcher/.Home}\n'; fi ;;
    *acme-unhandled*) printf 'Starting: Intent { act=android.intent.action.VIEW }\nError: Activity not started, unable to resolve Intent\n' ;;
    *"'list' 'packages'"*) printf 'package:com.example.acme versionCode:7\npackage:android versionCode:36\n' ;;
    *"screenrecord --help"*) printf '  --time-limit TIME\n      Set the maximum recording time, in seconds.  Default / maximum is 180.\n' ;;
    *"exec screenrecord"*) echo 4242
      printf '\000\000\000\030ftypisom\000\000\002\000isomiso2' > recording.mp4
      while [ ! -f stopped ]; do sleep 0.05; done ;;
    "'kill' '-INT' '4242'") touch stopped ;;
    "'input' 'help'") printf 'Usage: input [<source>] <command> [<arg>...]\n'
      if [ -f chords ]; then printf '      keycombination [-t duration(ms)] <key code 1> <key code 2> ...\n'; fi ;;
    esac ;;
  pull) printf 'pull %s\n' "$4" >> input.log; cp recording.mp4 "$5" ;;
  *) exit 2 ;;
  esac ;;
*) exit 3 ;;
esac
"##;

struct Mcp {
    /// The fake device's files and this test's data root and owner process.
    _directory: std::sync::Arc<crate::Workspace>,
    adb: Option<std::path::PathBuf>,
    /// The host task identity; `None` makes the MCP connection the task.
    task: Option<String>,
    /// The owned fixture target whose detailed status is inspected explicitly.
    status_target: Option<String>,
    log: std::path::PathBuf,
    child: Child,
    input: Option<ChildStdin>,
    output: Lines<BufReader<ChildStdout>>,
    next: u64,
}

impl Mcp {
    async fn start() -> Self {
        Self::start_named("Acme-contract").await
    }
    /// Tests run concurrently and a control lease is per target, so each test
    /// that connects owns a distinct fake serial.
    async fn start_named(serial: &str) -> Self {
        let task = format!("testrun-android-contract-{}", uuid::Uuid::new_v4());
        Self::start_fake(serial, Some(task)).await
    }
    /// A server without a host label, run from its own temporary workspace.
    async fn start_unbound(serial: &str) -> Self {
        Self::start_fake(serial, None).await
    }
    async fn start_fake(serial: &str, task: Option<String>) -> Self {
        let directory = crate::Workspace::new();
        image::RgbImage::new(100, 200)
            .save(directory.path().join("screen.png"))
            .unwrap();
        let adb = directory.path().join("adb");
        std::fs::write(&adb, ADB.replace("Acme-contract", serial)).unwrap();
        std::fs::set_permissions(&adb, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::write(directory.path().join("chords"), "").unwrap();
        // Online baseline with a SIM and Bluetooth on, as the lab phone had.
        for (file, value) in [
            ("sdk", "36"),
            ("airplane", "0"),
            ("wifi", "1"),
            ("data", "1"),
            ("bt", "1"),
        ] {
            std::fs::write(directory.path().join(file), format!("{value}\n")).unwrap();
        }
        let mut mcp = Self::spawn(std::sync::Arc::new(directory), Some(adb), task).await;
        mcp.status_target = Some(format!("android:{serial}"));
        mcp
    }
    /// A server using the host's real adb, for explicitly owned emulators.
    #[cfg(feature = "acceptance-android")]
    async fn start_real() -> Self {
        Self::spawn(
            std::sync::Arc::new(crate::Workspace::new()),
            None,
            Some(format!("testrun-android-contract-{}", uuid::Uuid::new_v4())),
        )
        .await
    }
    /// A second server for the same task and fake device, as a host restart.
    async fn successor(&self) -> Self {
        let mut mcp =
            Self::spawn(self._directory.clone(), self.adb.clone(), self.task.clone()).await;
        mcp.status_target = self.status_target.clone();
        mcp
    }
    async fn spawn(
        directory: std::sync::Arc<crate::Workspace>,
        adb: Option<std::path::PathBuf>,
        task: Option<String>,
    ) -> Self {
        let mut command = Command::new(env!("CARGO_BIN_EXE_testrun"));
        command.arg("--json");
        command
            .args(["mcp", "serve"])
            .env("TESTRUN_DATA_ROOT", directory.data_root())
            .env("TMPDIR", directory.path())
            .env_remove("CODEX_THREAD_ID")
            .env_remove("TESTRUN_TASK_ID");
        if let Some(task) = &task {
            command.env("TESTRUN_TASK_ID", task);
        } else {
            command.current_dir(directory.path());
        }
        if let Some(adb) = &adb {
            command.env("TESTRUN_ADB", adb);
            command.env("ANDROID_AVD_HOME", crate::NO_AVDS);
        }
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let mut mcp = Self {
            log: directory.path().join("input.log"),
            _directory: directory,
            adb,
            task,
            status_target: None,
            input: Some(child.stdin.take().unwrap()),
            output: BufReader::new(child.stdout.take().unwrap()).lines(),
            child,
            next: 0,
        };
        mcp.request("initialize", json!({"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"Acme contract fixture","version":"1"}})).await;
        mcp.send(json!({"jsonrpc":"2.0","method":"notifications/initialized"}))
            .await;
        mcp
    }
    async fn send(&mut self, message: Value) {
        let mut bytes = serde_json::to_vec(&message).unwrap();
        bytes.push(b'\n');
        self.input
            .as_mut()
            .unwrap()
            .write_all(&bytes)
            .await
            .unwrap();
    }
    async fn request(&mut self, method: &str, params: Value) -> Value {
        self.next += 1;
        let id = self.next;
        self.send(json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))
            .await;
        loop {
            let line = tokio::time::timeout(Duration::from_secs(30), self.output.next_line())
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            let response: Value = serde_json::from_str(&line).unwrap();
            if response["id"] == id {
                return response;
            }
        }
    }
    /// The next request replies without any intervening cancelled-request reply.
    async fn cancellation_barrier(&mut self) {
        self.next += 1;
        let id = self.next;
        self.send(json!({"jsonrpc":"2.0","id":id,"method":"ping"}))
            .await;
        loop {
            let line = tokio::time::timeout(Duration::from_secs(5), self.output.next_line())
                .await
                .unwrap()
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
    /// Returns the tool's `{ok, data | code}` envelope.
    async fn tool(&mut self, name: &str, arguments: Value) -> Value {
        if name == "connect" && self.status_target.is_none() {
            self.status_target = arguments["targetId"].as_str().map(str::to_owned);
        }
        let response = self
            .request("tools/call", json!({"name":name,"arguments":arguments}))
            .await;
        serde_json::from_str(
            response["result"]["content"][0]["text"]
                .as_str()
                .unwrap_or_else(|| panic!("{name}: {response}")),
        )
        .unwrap()
    }
    async fn target_status(&mut self) -> Value {
        let target = self
            .status_target
            .clone()
            .expect("Select the fixture target");
        self.tool("status", json!({"targetId":target})).await
    }
    async fn closed_cleanup(&mut self) -> Value {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
        loop {
            let status = self.target_status().await;
            if status["code"] == "NOT_CONNECTED" || status["data"]["state"] == "closed" {
                return self._directory.closed_cleanup();
            }
            assert!(tokio::time::Instant::now() < deadline, "{status}");
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
    async fn archived_action(&mut self, history: &Value) -> Value {
        let action = self
            .tool("status", json!({"actionId":history["actionId"]}))
            .await;
        assert_eq!(action["ok"], true, "{action}");
        action["data"]["action"].clone()
    }
    fn commands(&self) -> Vec<String> {
        std::fs::read_to_string(&self.log)
            .unwrap_or_default()
            .lines()
            .filter(|line| {
                !line.contains("'list' 'packages'")
                    && *line != "'input' 'help'"
                    && *line != "'getprop' 'ro.build.version.sdk'"
                    && *line != "'dumpsys' 'input'"
                    && !line.contains("#airplane")
            })
            .map(str::to_owned)
            .collect()
    }
    /// Arm one of the fake adb's one-shot faults.
    fn arm(&self, fault: &str) {
        std::fs::write(self._directory.path().join(fault), "").unwrap();
    }
    /// The fake target's connectivity switch files.
    fn switches(&self) -> Value {
        let read = |file: &str| {
            std::fs::read_to_string(self._directory.path().join(file))
                .unwrap()
                .trim()
                .to_owned()
        };
        json!({"airplane":read("airplane"),"wifi":read("wifi"),"data":read("data"),"bt":read("bt")})
    }
}

impl Drop for Mcp {
    /// The workspace stops this test's owner process once its last fixture goes.
    fn drop(&mut self) {
        let _ = self.child.start_kill();
    }
}

#[tokio::test]
async fn android_refuses_unenforceable_options_and_runs_batches_and_urls() {
    let mut mcp = Mcp::start().await;
    let listed = mcp.tool("target_list", json!({"kind":"android"})).await;
    let target = listed["data"]["targets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|target| target["targetId"] == "android:Acme-contract")
        .cloned()
        .unwrap_or_else(|| panic!("{listed}"));
    assert_eq!(target["capabilities"]["tap"]["state"], "supported");
    assert_eq!(
        target["capabilities"]["clipboard_read"]["state"],
        "unsupported"
    );
    assert_eq!(target["platform"], "android");
    let connected = mcp
        .tool("connect", json!({"targetId":target["targetId"]}))
        .await;
    assert_eq!(connected["ok"], true, "{connected}");
    assert_eq!(
        connected["data"]["capabilities"]["key"]["modifiers"], true,
        "{connected}"
    );
    let tap = json!({"action":"tap","point":{"x":0.5,"y":0.5}});

    for actions in [
        json!([{"action":"type_text","text":"Acme","element":{"snapshotId":"missing","elementId":"missing"}}]),
        json!([tap, {"action":"key","key":"f13"}]),
        json!([tap, {"action":"type_text","text":"Acme","replace":true}]),
        json!([tap, {"action":"type_text","text":"Acmé"}]),
    ] {
        let refused = mcp.tool("act", json!({"actions":actions})).await;
        assert_eq!(
            refused["code"], "CAPABILITY_UNAVAILABLE",
            "{actions}: {refused}"
        );
        // A refused request leaves no action record.
        let status = mcp.target_status().await;
        assert!(
            status["data"]["lastAction"].is_null(),
            "{actions}: {status}"
        );
    }
    assert_eq!(mcp.commands(), Vec::<String>::new());

    let act = mcp
        .tool(
            "act",
            json!({"observeAfterEach":false,"actions":[tap,{"action":"key","key":"tab"},{"action":"key","key":"a","modifiers":["control","shift"]}]}),
        )
        .await;
    assert_eq!(act["data"]["result"]["status"], "completed", "{act}");
    assert_eq!(act["data"]["result"]["steps"][0]["tool"], "tap");
    opens_urls(&mut mcp).await;
    assert_eq!(
        mcp.commands(),
        [
            "'input' 'tap' '50' '100'",
            "'input' 'keyevent' 'KEYCODE_TAB'",
            "'input' 'keycombination' 'KEYCODE_CTRL_LEFT' 'KEYCODE_SHIFT_LEFT' 'KEYCODE_A'",
            "'am' 'start' '-W' '-a' 'android.intent.action.VIEW' '-d' 'https://example.com/acme?q=a&b=c'",
            "'am' 'start' '-W' '-a' 'android.intent.action.VIEW' '-d' 'acme-unhandled://example'",
        ]
    );
    lists_apps_with_pagination(&mut mcp).await;
    records_with_owned_cleanup(&mut mcp).await;
    let mut closed = mcp.tool("disconnect", json!({})).await;
    // On macOS the header-only fixture fails decoding, so cleanup reports the
    // failed recording once; the retried disconnect then finishes.
    if cfg!(target_os = "macos") {
        assert_eq!(closed["code"], "CLEANUP_FAILED", "{closed}");
        assert!(closed.to_string().contains("RECORDING_FAILED"), "{closed}");
        closed = mcp.tool("disconnect", json!({})).await;
    }
    assert_eq!(closed["data"]["closed"], true, "{closed}");
}

/// A handled URL completes, an unhandled one fails and an invalid one is
/// refused before dispatch.
async fn opens_urls(mcp: &mut Mcp) {
    for (url, outcome) in [
        ("https://example.com/acme?q=a&b=c", "completed"),
        ("acme-unhandled://example", "PLATFORM_FAILED"),
        ("https://example.com/a b", "INVALID_ARGUMENT"),
    ] {
        let result = mcp
            .tool("act", json!({"actions":[{"action":"open_url","url":url}]}))
            .await;
        if outcome == "completed" {
            assert_eq!(
                result["data"]["result"]["status"], outcome,
                "{url}: {result}"
            );
        } else if outcome == "INVALID_ARGUMENT" {
            assert_eq!(result["code"], outcome, "{url}: {result}");
        } else {
            // The failed step's own error names the platform failure.
            assert_eq!(result["ok"], false, "{url}: {result}");
            assert!(result.to_string().contains(outcome), "{url}: {result}");
        }
    }
}

async fn lists_apps_with_pagination(mcp: &mut Mcp) {
    let apps = mcp.tool("app_list", json!({})).await["data"].clone();
    assert_eq!(apps["total"], 2, "{apps}");
    assert!(apps["nextCursor"].is_null());
    assert!(apps.get("artifactPath").is_none());
    assert_eq!(apps["apps"][1]["appId"], "com.example.acme");
    assert_eq!(apps["apps"][1]["build"], "7");
    assert_eq!(apps["unavailableFields"], json!(["name", "version"]));
    let first = mcp.tool("app_list", json!({"limit":1})).await["data"].clone();
    let cursor = first["nextCursor"].as_str().unwrap();
    let second = mcp
        .tool("app_list", json!({"limit":1,"cursor":cursor}))
        .await["data"]
        .clone();
    assert!(second["nextCursor"].is_null());
    assert_eq!(json!([first["apps"][0], second["apps"][0]]), apps["apps"]);
}

/// Starts, observes and stops one recording, checking target limits and that
/// only the recording's own remote process and file are touched.
async fn records_with_owned_cleanup(mcp: &mut Mcp) {
    let refused = mcp
        .tool("recording_start", json!({"maxDurationMs":200_000}))
        .await;
    assert_eq!(refused["code"], "INVALID_ARGUMENT", "{refused}");
    assert_eq!(
        refused["details"]["result"]["targetMaxDurationMs"], 180_000,
        "{refused}"
    );
    let started = mcp
        .tool("recording_start", json!({"maxDurationMs":5000}))
        .await;
    assert_eq!(started["data"]["status"], "active", "{started}");
    assert_eq!(started["data"]["targetMaxDurationMs"], 180_000);
    assert_eq!(started["data"]["maxDurationMs"], 5000);
    assert_eq!(started["data"]["backend"], "adb_screenrecord");
    let recording = json!({"recordingId":started["data"]["recordingId"]});
    let status = mcp.tool("recording_status", recording.clone()).await;
    assert_eq!(status["data"]["status"], "active", "{status}");
    let stopped = mcp.tool("recording_stop", recording).await["data"].clone();
    let log = std::fs::read_to_string(&mcp.log).unwrap();
    let remote = format!(
        "/data/local/tmp/testrun-recording-{}.mp4",
        started["data"]["recordingId"].as_str().unwrap()
    );
    assert!(log.contains("--time-limit 5 "), "{log}");
    assert!(log.contains("'kill' '-INT' '4242'"), "{log}");
    assert!(log.contains(&format!("pull {remote}")), "{log}");
    assert!(log.contains(&format!("'rm' '-f' '{remote}'")), "{log}");
    // The fixture writes only an MP4 header: the macOS decoder refuses it,
    // while other hosts check the container alone.
    if cfg!(target_os = "macos") {
        assert_eq!(stopped["status"], "failed", "{stopped}");
    } else {
        assert_eq!(stopped["status"], "complete", "{stopped}");
        assert_eq!(stopped["statistics"]["reason"], "stopped", "{stopped}");
        assert!(stopped["artifact"]["path"].is_string(), "{stopped}");
    }
}

/// Offline settings persist across disconnect and owner replacement. Repeated
/// requests are no-ops, method changes are allowed and recovery is explicit.
#[tokio::test]
#[expect(
    clippy::too_many_lines,
    reason = "One connected journey checks persistence, explicit recovery and reconnect on one fake target"
)]
async fn android_network_persists_until_explicit_recovery_after_disconnect_and_reconnect() {
    // A unique serial keeps an owner left by an earlier interrupted run from
    // holding this run's lease.
    let serial = format!("Acme-network-{}", uuid::Uuid::new_v4().simple());
    let target_id = format!("android:{serial}");
    let mut mcp = Mcp::start_named(&serial).await;
    let listed = mcp.tool("target_list", json!({"kind":"android"})).await;
    let target = listed["data"]["targets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|target| target["targetId"] == target_id)
        .cloned()
        .unwrap_or_else(|| panic!("{listed}"));
    assert_eq!(target["capabilities"]["network"]["state"], "unknown");
    assert_eq!(target["capabilities"]["network"]["link"], "usb");

    let connected = mcp.tool("connect", json!({"targetId":target_id})).await;
    assert_eq!(connected["ok"], true, "{connected}");
    let network = &connected["data"]["capabilities"]["network"];
    assert_eq!(network["state"], "supported", "{connected}");
    assert_eq!(network["apiLevel"], 36);
    assert_eq!(network["methods"], json!(["airplane", "radios"]));

    let refused = mcp
        .tool("network_set", json!({"mode":"offline","force":true}))
        .await;
    assert_eq!(refused["code"], "INVALID_ARGUMENT", "{refused}");

    let offline = mcp.tool("network_set", json!({"mode":"offline"})).await;
    assert_eq!(offline["ok"], true, "{offline}");
    let offline = &offline["data"];
    assert_eq!(offline["changed"], true);
    assert_eq!(offline["agreed"], true);
    assert_eq!(offline["method"], "airplane");
    assert_eq!(offline["before"]["connectivity"], "online");
    assert_eq!(offline["after"]["connectivity"], "offline");
    assert_eq!(offline["after"]["airplane"], true);
    assert_eq!(offline["after"]["wifiSetting"], "3");
    assert_eq!(offline["after"]["bluetooth"], false);
    assert_eq!(
        offline["sideEffects"],
        json!(["bluetooth_off", "wifi_airplane_override"])
    );
    assert!(offline.get("baseline").is_none());
    assert!(offline.get("restorePending").is_none());
    assert!(offline["actionId"].as_str().unwrap().starts_with("net-"));
    assert_eq!(mcp.switches()["airplane"], "1");

    // The readback already agrees: nothing is commanded and nothing changes.
    let again = mcp.tool("network_set", json!({"mode":"offline"})).await;
    assert_eq!(again["data"]["changed"], false, "{again}");
    assert_eq!(
        mcp.commands(),
        ["'cmd' 'connectivity' 'airplane-mode' 'enable'"],
        "a repeated offline request dispatches nothing"
    );

    let status = mcp.target_status().await;
    assert!(status["data"].get("network").is_none());
    assert_eq!(
        status["data"]["lastNetworkOperation"]["state"], "completed",
        "{status}"
    );
    let by_reference = mcp
        .tool("status", json!({"actionId":offline["actionId"]}))
        .await;
    assert_eq!(
        by_reference["data"]["action"]["state"], "completed",
        "{by_reference}"
    );

    let closed = mcp.tool("disconnect", json!({})).await;
    assert_eq!(closed["data"]["closed"], true, "{closed}");
    assert!(closed["data"].get("network").is_none());
    assert_eq!(
        mcp.switches(),
        json!({"airplane":"1","wifi":"1","data":"1","bt":"1"})
    );
    assert_eq!(
        mcp.commands(),
        ["'cmd' 'connectivity' 'airplane-mode' 'enable'"]
    );

    // An explicit online request can switch methods. Owner replacement also
    // leaves the requested settings alone until the caller changes them.
    let connected = mcp.tool("connect", json!({"targetId":target_id})).await;
    assert_eq!(connected["ok"], true, "{connected}");
    assert!(
        connected["data"].get("networkRestore").is_none(),
        "{connected}"
    );
    assert_eq!(mcp.switches()["airplane"], "1");
    let mixed = mcp
        .tool("network_set", json!({"mode":"online","method":"radios"}))
        .await;
    assert_eq!(mixed["ok"], true, "{mixed}");
    let radios = mcp
        .tool("network_set", json!({"mode":"offline","method":"radios"}))
        .await;
    assert_eq!(radios["ok"], true, "{radios}");
    assert_eq!(radios["data"]["after"]["bluetooth"], true);
    assert_eq!(radios["data"]["after"]["connectivity"], "offline");
    assert_eq!(radios["data"]["sideEffects"], json!([]));
    assert_eq!(
        mcp.switches(),
        json!({"airplane":"0","wifi":"0","data":"0","bt":"1"})
    );
    mcp.kill_owner().await;
    let lost = mcp.target_status().await;
    assert_ne!(lost["data"]["state"], "ready", "{lost}");
    assert!(lost["data"].get("network").is_none());
    assert_eq!(mcp.switches()["wifi"], "0");
    let recovered = mcp.tool("connect", json!({"targetId":target_id})).await;
    assert_eq!(recovered["ok"], true, "{recovered}");
    assert!(recovered["data"].get("networkRestore").is_none());
    assert_eq!(
        mcp.switches(),
        json!({"airplane":"0","wifi":"0","data":"0","bt":"1"})
    );
    let online = mcp
        .tool("network_set", json!({"mode":"online","method":"radios"}))
        .await;
    assert_eq!(online["ok"], true, "{online}");
    let closed = mcp.tool("disconnect", json!({})).await;
    assert_eq!(closed["data"]["closed"], true, "{closed}");
    assert!(closed["data"].get("network").is_none());
    let radios: Vec<_> = mcp
        .commands()
        .into_iter()
        .filter(|c| !c.contains("airplane-mode"))
        .collect();
    assert_eq!(
        radios,
        [
            "'svc' 'wifi' 'enable'",
            "'svc' 'wifi' 'disable'",
            "'svc' 'data' 'disable'",
            "'svc' 'wifi' 'enable'",
            "'svc' 'data' 'enable'",
        ]
    );
}

/// One offline/online cycle plus persistence on disconnect against the owned
/// emulator, with the test's own adb reads confirming each state outside the
/// product. Never selects physical hardware.
#[tokio::test]
#[cfg(feature = "acceptance-android")]
async fn android_network_cycle_persists_on_the_owned_emulator() {
    let serial = std::env::var("TESTRUN_TEST_ANDROID").expect("Select an owned emulator");
    assert!(
        serial.starts_with("emulator-"),
        "Physical devices are outside this acceptance test"
    );
    let adb = |args: &[&str]| {
        let serial = serial.clone();
        let args: Vec<String> = args.iter().map(|a| (*a).to_owned()).collect();
        async move {
            let output = Command::new("adb")
                .arg("-s")
                .arg(&serial)
                .args(&args)
                .output()
                .await
                .unwrap();
            String::from_utf8_lossy(&output.stdout).trim().to_owned()
        }
    };
    assert!(
        adb(&["emu", "avd", "name"])
            .await
            .starts_with("Testrun-QA-"),
        "Only the owned Testrun-QA emulator may be exercised"
    );
    let airplane_before = adb(&["shell", "settings", "get", "global", "airplane_mode_on"]).await;
    let wifi_before = adb(&["shell", "settings", "get", "global", "wifi_on"]).await;
    let target_id = format!("android:{serial}");
    let mut mcp = Mcp::start_real().await;
    let listed = mcp
        .tool("target_list", json!({"kind":"android","limit":200}))
        .await;
    let target = listed["data"]["targets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|target| target["targetId"] == target_id)
        .cloned()
        .unwrap_or_else(|| panic!("{listed}"));
    assert_eq!(target["capabilities"]["network"]["link"], "emulator");

    let connected = mcp.tool("connect", json!({"targetId":target_id})).await;
    assert_eq!(connected["ok"], true, "{connected}");
    let network = &connected["data"]["capabilities"]["network"];
    assert_eq!(network["state"], "supported", "{connected}");
    assert!(network["apiLevel"].as_u64().unwrap() >= 30, "{connected}");

    let offline = mcp.tool("network_set", json!({"mode":"offline"})).await;
    assert_eq!(offline["ok"], true, "{offline}");
    assert_eq!(offline["data"]["agreed"], true, "{offline}");
    assert_eq!(offline["data"]["after"]["connectivity"], "offline");
    assert_eq!(
        adb(&["shell", "cmd", "connectivity", "airplane-mode"]).await,
        "enabled"
    );
    let online = mcp.tool("network_set", json!({"mode":"online"})).await;
    assert_eq!(online["ok"], true, "{online}");
    assert_eq!(online["data"]["agreed"], true, "{online}");
    assert_eq!(online["data"]["after"]["connectivity"], "online");
    assert_eq!(
        adb(&["shell", "cmd", "connectivity", "airplane-mode"]).await,
        "disabled"
    );
    // Disconnect leaves the requested setting in place. Reconnect and explicitly
    // return this test-owned emulator to its initial airplane setting before finishing.
    let offline = mcp.tool("network_set", json!({"mode":"offline"})).await;
    assert_eq!(offline["ok"], true, "{offline}");
    let closed = mcp.tool("disconnect", json!({})).await;
    assert_eq!(closed["data"]["closed"], true, "{closed}");
    assert_eq!(
        adb(&["shell", "settings", "get", "global", "airplane_mode_on"]).await,
        "1"
    );
    assert_eq!(
        mcp.tool("connect", json!({"targetId":target_id})).await["ok"],
        true
    );
    let final_mode = if airplane_before == "1" {
        "offline"
    } else {
        "online"
    };
    assert_eq!(
        mcp.tool("network_set", json!({"mode":final_mode})).await["ok"],
        true
    );
    assert_eq!(
        mcp.tool("disconnect", json!({})).await["data"]["closed"],
        true
    );
    assert_eq!(
        adb(&["shell", "settings", "get", "global", "airplane_mode_on"]).await,
        airplane_before
    );
    assert_eq!(
        adb(&["shell", "settings", "get", "global", "wifi_on"]).await,
        wifi_before
    );
}

/// Failures after the first switch command retain an unknown outcome: a readback
/// that fails once airplane mode is on, and adb refusing the second radio
/// command. Disconnect leaves the partial state alone; reconnect permits an
/// explicit recovery without replaying the uncertain request.
#[tokio::test]
async fn android_network_requires_explicit_recovery_after_failures_past_dispatch() {
    let serial = format!("Acme-failure-{}", uuid::Uuid::new_v4().simple());
    let target_id = format!("android:{serial}");
    let baseline = json!({"airplane":"0","wifi":"1","data":"1","bt":"1"});
    let mut mcp = Mcp::start_named(&serial).await;

    let connected = mcp.tool("connect", json!({"targetId":target_id})).await;
    assert_eq!(connected["ok"], true, "{connected}");
    mcp.arm("fail-readback-offline");
    let offline = mcp.tool("network_set", json!({"mode":"offline"})).await;
    assert_eq!(offline["code"], "NETWORK_OUTCOME_UNKNOWN", "{offline}");
    assert_eq!(mcp.switches()["airplane"], "1");
    let status = mcp.target_status().await;
    assert!(status["data"].get("network").is_none());
    let operation = &status["data"]["lastNetworkOperation"];
    assert_eq!(operation["state"], "outcome_unknown");
    assert_eq!(operation["error"]["dispatched"], true);
    assert_eq!(
        operation["error"]["cause"]["code"],
        "NETWORK_READBACK_FAILED"
    );
    let closed = mcp.tool("disconnect", json!({})).await;
    assert_eq!(closed["data"]["closed"], true, "{closed}");
    assert_eq!(mcp.switches()["airplane"], "1");
    assert_eq!(
        mcp.commands(),
        ["'cmd' 'connectivity' 'airplane-mode' 'enable'"]
    );

    let connected = mcp.tool("connect", json!({"targetId":target_id})).await;
    assert_eq!(connected["ok"], true, "{connected}");
    assert_eq!(mcp.switches()["airplane"], "1");
    assert_eq!(
        mcp.tool("network_set", json!({"mode":"online"})).await["ok"],
        true
    );
    assert_eq!(mcp.switches(), baseline);
    mcp.arm("fail-data-disable");
    let radios = mcp
        .tool("network_set", json!({"mode":"offline","method":"radios"}))
        .await;
    assert_eq!(radios["code"], "NETWORK_OUTCOME_UNKNOWN", "{radios}");
    assert_eq!(
        mcp.switches(),
        json!({"airplane":"0","wifi":"0","data":"1","bt":"1"}),
        "Wi-Fi went off before adb refused the second command"
    );
    let status = mcp.target_status().await;
    let operation = &status["data"]["lastNetworkOperation"];
    assert_eq!(operation["state"], "outcome_unknown");
    assert_eq!(operation["error"]["dispatched"], true);
    assert_eq!(
        operation["error"]["cause"]["code"],
        "NETWORK_COMMAND_FAILED"
    );
    let refused = mcp
        .tool("network_set", json!({"mode":"online","method":"radios"}))
        .await;
    assert_eq!(refused["code"], "SESSION_UNCERTAIN", "{refused}");
    let closed = mcp.tool("disconnect", json!({})).await;
    assert_eq!(closed["data"]["closed"], true, "{closed}");
    assert_eq!(mcp.switches()["wifi"], "0");
    assert!(!mcp.commands().contains(&"'svc' 'wifi' 'enable'".into()));
    assert_eq!(
        mcp.tool("connect", json!({"targetId":target_id})).await["ok"],
        true
    );
    // Mobile data still provides connectivity after this partial failure.
    // Online asks for connectivity, rather than restoration of each radio.
    let online = mcp
        .tool("network_set", json!({"mode":"online","method":"radios"}))
        .await;
    assert_eq!(online["ok"], true, "{online}");
    assert_eq!(online["data"]["changed"], false);
    assert_eq!(online["data"]["after"]["connectivity"], "online");
    assert_eq!(mcp.switches()["wifi"], "0");
    assert!(!mcp.commands().contains(&"'svc' 'wifi' 'enable'".into()));
    assert_eq!(
        mcp.tool("disconnect", json!({})).await["data"]["closed"],
        true
    );
}

async fn process_alive(pid: &str) -> bool {
    Command::new("kill")
        .args(["-0", pid])
        .status()
        .await
        .unwrap()
        .success()
}

impl Mcp {
    /// The PID of this test's owner process, from its own record.
    async fn owner_pid(&self) -> String {
        self._directory
            .owner_pid()
            .expect("A running owner process")
            .to_string()
    }
    /// SIGKILL the server itself, as a host crash would, and reap it.
    async fn kill_server(&mut self) {
        self.child.start_kill().unwrap();
        self.child.wait().await.unwrap();
    }
    /// SIGKILL the owner process, as a host crash would; its device actors
    /// and references go with it.
    async fn kill_owner(&mut self) {
        let pid = self.owner_pid().await;
        let killed = Command::new("kill")
            .args(["-9", &pid])
            .status()
            .await
            .unwrap();
        assert!(killed.success());
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        loop {
            let status = self.target_status().await;
            if status["data"]["state"] != "ready" {
                return;
            }
            assert!(tokio::time::Instant::now() < deadline, "{status}");
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
}

#[tokio::test]
async fn android_cli_wrappers_and_ios_only_options_report_through_exit_status() {
    let directory = crate::Workspace::new();
    let data_root = directory.data_root();
    image::RgbImage::new(100, 200)
        .save(directory.path().join("screen.png"))
        .unwrap();
    let adb = directory.path().join("adb");
    std::fs::write(&adb, ADB.replace("Acme-contract", "Acme-cli")).unwrap();
    std::fs::set_permissions(&adb, std::fs::Permissions::from_mode(0o700)).unwrap();
    let task = format!("testrun-android-cli-{}", uuid::Uuid::new_v4());
    let run = |args: &'static [&'static str]| {
        let adb = adb.clone();
        let task = task.clone();
        let data_root = data_root.clone();
        async move {
            Command::new(env!("CARGO_BIN_EXE_testrun"))
                .arg("--json")
                .args(args)
                .env("TESTRUN_DATA_ROOT", data_root)
                .env_remove("CODEX_THREAD_ID")
                .env("TESTRUN_TASK_ID", task)
                .env("TESTRUN_ADB", adb)
                .env("ANDROID_AVD_HOME", crate::NO_AVDS)
                .kill_on_drop(true)
                .output()
                .await
                .unwrap()
        }
    };
    let refused = run(&[
        "connect",
        "--target",
        "android:Acme-cli",
        "--runner-team-id",
        "ACMETEAM",
    ])
    .await;
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("INVALID_ARGUMENT"));
    let apps = run(&["app", "list", "--target", "android:Acme-cli"]).await;
    assert!(
        apps.status.success(),
        "{}",
        String::from_utf8_lossy(&apps.stderr)
    );
    let apps: Value = serde_json::from_slice(&apps.stdout).unwrap();
    assert_eq!(apps["total"], 2, "{apps}");
    let opened = run(&[
        "act",
        "--target",
        "android:Acme-cli",
        r#"{"action":"open_url","url":"https://example.com/acme"}"#,
    ])
    .await;
    assert!(
        opened.status.success(),
        "{}",
        String::from_utf8_lossy(&opened.stderr)
    );
    let unhandled = run(&[
        "act",
        "--target",
        "android:Acme-cli",
        r#"{"action":"open_url","url":"acme-unhandled://example"}"#,
    ])
    .await;
    assert!(!unhandled.status.success());
    assert!(
        String::from_utf8_lossy(&unhandled.stderr).contains("BATCH_FAILED"),
        "{}",
        String::from_utf8_lossy(&unhandled.stderr)
    );
    // app list connected the device for every client; this releases it.
    let targeted = run(&["disconnect", "--target", "android:Acme-cli"]).await;
    assert!(
        targeted.status.success(),
        "{}",
        String::from_utf8_lossy(&targeted.stderr)
    );
    let closed: serde_json::Value = serde_json::from_slice(&targeted.stdout).unwrap();
    assert_eq!(closed["closed"], true, "{closed}");
    // This target's input utility lacks keycombination, so a managed chord is
    // refused before the earlier tap is admitted.
    let connected = run(&["connect", "--target", "android:Acme-cli"]).await;
    assert!(
        connected.status.success(),
        "{}",
        String::from_utf8_lossy(&connected.stderr)
    );
    let chord = run(&[
        "act",
        r#"[{"action":"tap","point":{"x":0.5,"y":0.5}},{"action":"key","key":"a","modifiers":["control"]}]"#,
    ])
    .await;
    assert!(!chord.status.success());
    assert!(String::from_utf8_lossy(&chord.stderr).contains("CAPABILITY_UNAVAILABLE"));
    let disconnected = run(&["disconnect"]).await;
    assert!(disconnected.status.success());
    let log = std::fs::read_to_string(directory.path().join("input.log")).unwrap();
    assert!(log.contains("'-d' 'https://example.com/acme'"), "{log}");
    assert!(log.contains("'input' 'help'"), "{log}");
    assert!(!log.contains("'input' 'tap'"), "{log}");
    assert!(!log.contains("keycombination"), "{log}");
}
