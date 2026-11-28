//! Installed-path correctness checks for retained input, exact compact frames and reboot fencing.
use super::*;
use base64::Engine;
use std::path::{Path, PathBuf};

struct Fixture {
    client: Client,
    uuid: String,
    data: PathBuf,
    _directory: tempfile::TempDir,
}
fn sim(args: &[&str]) -> Vec<u8> {
    let output = std::process::Command::new("/usr/bin/xcrun")
        .arg("simctl")
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "simctl setup/probe failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}
async fn state(path: &Path, predicate: impl Fn(&Value) -> bool) -> Value {
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            if let Ok(bytes) = std::fs::read(path.join("Documents/state.json"))
                && let Ok(value) = serde_json::from_slice::<Value>(&bytes)
                && predicate(&value)
            {
                return value;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("Independent fixture state did not become ready")
}
fn marker(bytes: &[u8], state: &Value) -> Option<u64> {
    let image = image::load_from_memory(bytes).ok()?.to_rgb8();
    let frame = &state["marker"];
    let mut bits = [0_u64; 8];
    for (i, bit) in bits.iter_mut().enumerate() {
        let x = (frame["x"].as_f64()? + frame["width"].as_f64()? * (i as f64 + 0.5) / 8.0)
            / state["width"].as_f64()?;
        let y =
            (frame["y"].as_f64()? + frame["height"].as_f64()? / 2.0) / state["height"].as_f64()?;
        if !(0.0..1.0).contains(&x) || !(0.0..1.0).contains(&y) {
            return None;
        }
        let pixel = image.get_pixel(
            (x * f64::from(image.width())) as u32,
            (y * f64::from(image.height())) as u32,
        );
        *bit = if pixel.0.iter().all(|c| *c < 70) {
            0
        } else if pixel.0.iter().all(|c| *c > 185) {
            1
        } else {
            return None;
        };
    }
    if bits[..7].iter().sum::<u64>() % 2 != bits[7] {
        return None;
    }
    Some(bits[..7].iter().enumerate().map(|(i, b)| b << i).sum())
}
impl Fixture {
    async fn start() -> Self {
        let uuid = std::env::var("TESTRUN_TEST_SIMULATOR").unwrap();
        let target_id = format!("simulator:{uuid}");
        let directory = tempfile::tempdir().unwrap();
        let mut client =
            Client::start_with_timeout(directory.path(), Duration::from_secs(45)).await;
        let targets = client
            .call("target_list", json!({"kind":"simulator","limit":200}))
            .await;
        let target = targets["data"]["targets"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["targetId"] == target_id)
            .unwrap();
        assert!(target["name"].as_str().unwrap().starts_with("Testrun-QA-"));
        let opened = client.call("connect", json!({"targetId":target_id})).await;
        assert_eq!(opened["ok"], true, "{opened}");
        let data = PathBuf::from(
            String::from_utf8(sim(&[
                "get_app_container",
                &uuid,
                "com.canonic.testrun.fixture",
                "data",
            ]))
            .unwrap()
            .trim(),
        );
        let mut fixture = Self {
            client,
            uuid,
            data,
            _directory: directory,
        };
        fixture.reset().await;
        fixture
    }
    async fn reset(&mut self) {
        let _ = std::process::Command::new("/usr/bin/xcrun")
            .args([
                "simctl",
                "terminate",
                &self.uuid,
                "com.canonic.testrun.fixture",
            ])
            .output();
        for name in ["state.json", "command.json"] {
            let _ = std::fs::remove_file(self.data.join("Documents").join(name));
        }
        sim(&["launch", &self.uuid, "com.canonic.testrun.fixture"]);
        state(&self.data, |v| {
            v["count"] == 0 && v["marker"]["width"].as_f64().is_some_and(|w| w > 0.0)
        })
        .await;
        self.rendered(0).await;
    }
    async fn rendered(&self, expected: u64) {
        let png = self._directory.path().join("independent.png");
        let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
        loop {
            sim(&[
                "io",
                &self.uuid,
                "screenshot",
                "--type=png",
                png.to_str().unwrap(),
            ]);
            let current = state(&self.data, |_| true).await;
            if marker(&std::fs::read(&png).unwrap(), &current) == Some(expected) {
                return;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "Independent rendered-state barrier failed"
            );
        }
    }
    async fn image(&mut self, expected: u64) {
        let reply = self
            .client
            .request(
                "tools/call",
                json!({"name":"observe","arguments":{"profile":"fast"}}),
            )
            .await;
        let result = &reply["result"]["structuredContent"];
        assert_eq!(result["ok"], true, "{result}");
        let observation = &result["data"]["observation"];
        let image = reply["result"]["content"]
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["type"] == "image")
            .unwrap();
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(image["data"].as_str().unwrap())
            .unwrap();
        let decoded = image::load_from_memory(&bytes).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (441, 960));
        assert_eq!(observation["sourceWidth"], 1206);
        assert_eq!(observation["sourceHeight"], 2622);
        assert_eq!(observation["privacy"], "local");
        assert_eq!(image["mimeType"], "image/jpeg");
        let current = state(&self.data, |v| v["count"] == expected).await;
        assert_eq!(marker(&bytes, &current), Some(expected));
    }
    async fn tap(&mut self, point: &Value) -> Value {
        self.client
            .call("act", json!({"actions":[{"action":"tap","point":point}]}))
            .await
    }
    /// The owner process serving this fixture's connection, from its record.
    fn owner(&self) -> u32 {
        let pid = self.client.owner_pid().expect("A running owner process");
        u32::try_from(pid).unwrap()
    }
    async fn point(&mut self, identifier: &str) -> Value {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(8);
        loop {
            let query=self.client.call("ui_find",json!({"scope":{"kind":"display"},"allowPartial":true,"query":{"identifier":identifier}})).await;
            assert_eq!(query["ok"], true, "{query}");
            let matches = &query["data"]["data"]["matches"];
            if matches.as_array().is_some_and(|v| v.len() == 1)
                && matches[0]["frame"]["x"].is_number()
            {
                let frame = &matches[0]["frame"];
                return json!({"x":(frame["x"].as_f64().unwrap()+frame["width"].as_f64().unwrap()/2.0)/402.0,"y":(frame["y"].as_f64().unwrap()+frame["height"].as_f64().unwrap()/2.0)/874.0});
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "Fixture accessibility target did not become ready: {query}"
            );
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
    async fn finish(mut self) {
        let closed = self.client.call("disconnect", json!({})).await;
        assert_eq!(closed["ok"], true, "{closed}");
        drop(self.client.input);
        tokio::time::timeout(Duration::from_secs(15), self.client.child.wait())
            .await
            .unwrap()
            .unwrap();
    }
}
fn owned_workers(parent: u32, kind: &str, uuid: &str) -> Vec<u32> {
    let output = std::process::Command::new("ps")
        .args(["-axo", "pid,ppid"])
        .output()
        .unwrap();
    let mut matches = Vec::new();
    for line in String::from_utf8(output.stdout).unwrap().lines().skip(1) {
        let fields: Vec<_> = line.split_whitespace().collect();
        if fields.len() != 2 || fields[1].parse::<u32>().ok() != Some(parent) {
            continue;
        }
        let pid = fields[0].parse::<u32>().unwrap();
        // Inspect only verified direct children; never retain or print process arguments.
        let output = std::process::Command::new("ps")
            .args(["-p", &pid.to_string(), "-o", "command="])
            .output()
            .unwrap();
        let command = String::from_utf8(output.stdout).unwrap();
        if command.contains(&format!(" {kind} ")) && command.contains(uuid) {
            matches.push(pid);
        }
    }
    matches
}
fn owned_worker(parent: u32, kind: &str, uuid: &str) -> u32 {
    let workers = owned_workers(parent, kind, uuid);
    assert_eq!(workers.len(), 1, "Expected exactly one owned {kind} worker");
    workers[0]
}

#[tokio::test]
async fn retained_reuse_freshness_worker_interruption_and_clean_disconnect() {
    let mut fixture = Fixture::start().await;
    fixture.image(0).await;
    let point = fixture.point("qa.increment").await;
    let mut worker = None;
    for i in 1..=3 {
        let result = fixture.tap(&point).await;
        assert_eq!(result["ok"], true, "{result}");
        fixture.rendered(i).await;
        fixture.image(i).await;
        let pid = owned_worker(fixture.owner(), "native-input", &fixture.uuid);
        if let Some(previous) = worker {
            assert_eq!(pid, previous);
        }
        worker = Some(pid);
    }
    for (seq, count) in [(1, 21), (2, 42)] {
        std::fs::write(
            fixture.data.join("Documents/command.json"),
            json!({"seq":seq,"count":count}).to_string(),
        )
        .unwrap();
        state(&fixture.data, |v| v["command"] == seq).await;
        fixture.rendered(count).await;
        fixture.image(count).await;
    }
    interrupt(fixture, worker.unwrap()).await;
}

async fn interrupt(mut fixture: Fixture, pid: u32) {
    let point = fixture.point("qa.touch-surface").await;
    let hold = json!({"actions":[{"action":"long_press","point":point,"durationMs":3000}]});
    let parent = fixture.owner();
    let data = fixture.data.clone();
    let uuid = fixture.uuid.clone();
    let result = {
        let action = async { fixture.client.call("act", hold).await };
        tokio::pin!(action);
        tokio::select! {result=&mut action=>panic!("Gesture returned before fault: {result}"),_=state(&data,|v|v["down"].as_u64().unwrap_or(0)>v["up"].as_u64().unwrap_or(0)+v["cancelled"].as_u64().unwrap_or(0))=>{}}
        assert_eq!(owned_worker(parent, "native-input", &uuid), pid);
        assert!(
            std::process::Command::new("/bin/kill")
                .args(["-KILL", &pid.to_string()])
                .status()
                .unwrap()
                .success()
        );
        action.await
    };
    assert_eq!(result["code"], "BATCH_OUTCOME_UNKNOWN", "{result}");
    let released = state(&data, |v| {
        v["down"] == v["up"].as_u64().unwrap_or(0) + v["cancelled"].as_u64().unwrap_or(0)
    })
    .await;
    assert_eq!(released["down"], 1, "the interrupted gesture was replayed");
    fixture.image(42).await;
    fixture.finish().await;
}

#[tokio::test]
async fn reboot_refuses_old_session_before_dispatch_and_allows_fresh_acquisition() {
    let mut fixture = Fixture::start().await;
    fixture.image(0).await;
    assert_eq!(
        fixture
            .client
            .call("preview_start", json!({"format":"jpeg"}))
            .await["ok"],
        true
    );
    let point = json!({"x":0.5,"y":0.9});
    assert_eq!(fixture.tap(&point).await["ok"], true);
    sim(&["shutdown", &fixture.uuid]);
    sim(&["boot", &fixture.uuid]);
    sim(&["bootstatus", &fixture.uuid, "-b"]);
    fixture.reset().await;
    let refused = fixture.tap(&point).await;
    assert_eq!(refused["ok"], false, "{refused}");
    assert!(refused.to_string().contains("STALE_TARGET"), "{refused}");
    assert_eq!(state(&fixture.data, |_| true).await["count"], 0);
    let opened = fixture
        .client
        .call(
            "connect",
            json!({"targetId":format!("simulator:{}", fixture.uuid)}),
        )
        .await;
    assert_eq!(opened["ok"], true, "{opened}");
    fixture.image(0).await;
    fixture.finish().await;
}

fn primary_image(reply: &Value, current: &Value, expected: u64) {
    let result = &reply["result"]["structuredContent"];
    assert_eq!(result["ok"], true, "{result}");
    let image = reply["result"]["content"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["type"] == "image")
        .unwrap();
    assert_eq!(image["mimeType"], "image/jpeg");
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(image["data"].as_str().unwrap())
        .unwrap();
    let decoded = image::load_from_memory(&bytes).unwrap();
    assert_eq!((decoded.width(), decoded.height()), (441, 960));
    assert_eq!(marker(&bytes, current), Some(expected));
}
/// A CLI command in `caller`'s data root, so it shares that owner process.
fn cli(fixture: &Fixture, caller: &Client, context: &str, args: &[&str]) -> Value {
    let binary = std::env::var_os("TESTRUN_TEST_BINARY")
        .unwrap_or_else(|| env!("CARGO_BIN_EXE_testrun").into());
    let output = std::process::Command::new(binary)
        .arg("--json")
        .env("TESTRUN_DATA_ROOT", &caller.data_root)
        .env_remove("CODEX_THREAD_ID")
        .env("TESTRUN_TASK_ID", context)
        .env("TMPDIR", fixture._directory.path())
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "Installed CLI failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

struct ManagedCleanup {
    binary: std::ffi::OsString,
    directory: PathBuf,
    data_root: PathBuf,
    context: String,
    armed: bool,
}
impl Drop for ManagedCleanup {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let result = std::process::Command::new(&self.binary)
            .arg("--json")
            .env("TESTRUN_DATA_ROOT", &self.data_root)
            .env_remove("CODEX_THREAD_ID")
            .env("TESTRUN_TASK_ID", &self.context)
            .env("TMPDIR", &self.directory)
            .arg("disconnect")
            .output();
        match result {
            Ok(output) if output.status.success() => {}
            Ok(output) => eprintln!(
                "Owned acceptance cleanup failed; records retained: {}",
                String::from_utf8_lossy(&output.stderr)
            ),
            Err(error) => {
                eprintln!("Owned acceptance cleanup could not run; records retained: {error}")
            }
        }
    }
}

#[tokio::test]
async fn managed_cli_and_mcp_share_retained_control_and_exact_observations() {
    let mut fixture = Fixture::start().await;
    let point = fixture.point("qa.increment").await;
    assert_eq!(
        fixture.client.call("disconnect", json!({})).await["ok"],
        true
    );
    let context = format!("retained-acceptance-{}", uuid::Uuid::new_v4());
    let mut caller = Client::start_bound(
        fixture._directory.path(),
        Duration::from_secs(45),
        Some(&context),
    )
    .await;
    let mut cleanup = ManagedCleanup {
        binary: std::env::var_os("TESTRUN_TEST_BINARY")
            .unwrap_or_else(|| env!("CARGO_BIN_EXE_testrun").into()),
        directory: fixture._directory.path().into(),
        data_root: caller.data_root.clone(),
        context: context.clone(),
        armed: true,
    };
    let connected = caller
        .call(
            "connect",
            json!({"targetId":format!("simulator:{}",fixture.uuid)}),
        )
        .await;
    assert_eq!(connected["ok"], true, "{connected}");
    assert_eq!(connected["data"]["profile"], "fast");
    assert_eq!(connected["data"]["observation"]["profile"], "fast");
    verify_managed_inspection(&mut caller).await;
    let target = format!("simulator:{}", fixture.uuid);
    assert_eq!(
        cli(
            &fixture,
            &caller,
            &context,
            &["connect", "--target", &target]
        )["reused"],
        true
    );
    let action = caller
        .request(
            "tools/call",
            json!({"name":"act","arguments":{"actions":[{"action":"tap","point":point}]}}),
        )
        .await;
    assert_eq!(
        action["result"]["structuredContent"]["ok"], true,
        "{action}"
    );
    // A fresh post-action image can precede rendering. Verify the named effect
    // independently and observe it without sending input again.
    fixture.rendered(1).await;
    let observed = caller
        .request(
            "tools/call",
            json!({"name":"observe","arguments":{"profile":"fast"}}),
        )
        .await;
    primary_image(
        &observed,
        &state(&fixture.data, |v| v["count"] == 1).await,
        1,
    );
    let action = json!({"action":"tap","point":point}).to_string();
    cli(&fixture, &caller, &context, &["act", &action]);
    fixture.rendered(2).await;
    let observed = cli(
        &fixture,
        &caller,
        &context,
        &["observe", "--profile", "fast"],
    );
    let observation = &observed["observation"];
    let bytes = std::fs::read(observation["path"].as_str().unwrap()).unwrap();
    assert_eq!(
        marker(&bytes, &state(&fixture.data, |v| v["count"] == 2).await),
        Some(2)
    );
    assert_eq!(observation["width"], 441);
    assert_eq!(observation["height"], 960);
    verify_eight_action_results(&mut fixture, &mut caller, &point).await;
    assert!(!cli(&fixture, &caller, &context, &["status"])["lastAction"].is_null());
    assert_eq!(caller.call("status", json!({})).await["ok"], true);
    assert_eq!(caller.call("disconnect", json!({})).await["ok"], true);
    cli(&fixture, &caller, &context, &["disconnect"]);
    let archived = cli(&fixture, &caller, &context, &["status"]);
    assert_eq!(archived["lastAction"]["status"], "completed");
    assert_eq!(archived["lastAction"]["steps"].as_array().unwrap().len(), 8);
    assert_eq!(
        archived["lastAction"]["actionOutcomes"]
            .as_array()
            .unwrap()
            .len(),
        8
    );
    cleanup.armed = false;
    let record = caller.pool_directory();
    assert!(
        record.is_dir(),
        "One versioned record for the manager's pool"
    );
    drop(caller.input);
    tokio::time::timeout(Duration::from_secs(15), caller.child.wait())
        .await
        .unwrap()
        .unwrap();
    fixture.finish().await;
}

async fn verify_managed_inspection(caller: &mut Client) {
    let inventory = caller.call("app_list", json!({})).await;
    assert_eq!(inventory["ok"], true, "{inventory}");
    let full_inventory: Value = serde_json::from_slice(
        &std::fs::read(inventory["data"]["artifactPath"].as_str().unwrap()).unwrap(),
    )
    .unwrap();
    assert!(
        full_inventory["apps"]
            .as_array()
            .unwrap()
            .iter()
            .any(|app| app["appId"] == "com.canonic.testrun.fixture")
    );
    let found = caller
        .call(
            "ui_find",
            json!({"scope":{"kind":"display"},"allowPartial":true,"query":{"role":"button"}}),
        )
        .await;
    assert_eq!(found["ok"], true, "{found}");
    assert!(
        !found["data"]["data"]["matches"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let status = caller.call("status", json!({})).await;
    assert_eq!(
        status["data"]["capabilities"]["accessibility"]["state"],
        "supported"
    );
    verify_managed_media(caller).await;
}

async fn verify_managed_media(caller: &mut Client) {
    let preview = caller.call("preview_start", json!({"format":"jpeg"})).await;
    assert_eq!(preview["ok"], true, "{preview}");
    let args = json!({"previewId":preview["data"]["previewId"]});
    let status = caller.call("preview_status", args.clone()).await;
    assert_eq!(status["ok"], true, "{status}");
    let denied = caller
        .call("preview_stop", json!({"previewId":"foreign"}))
        .await;
    assert_eq!(denied["code"], "NOT_OWNED", "{denied}");
    let recording = caller
        .call("recording_start", json!({"maxDurationMs":1000}))
        .await;
    assert_eq!(recording["ok"], true, "{recording}");
    let recording_args = json!({"recordingId":recording["data"]["recordingId"]});
    assert_eq!(
        caller
            .call("recording_status", recording_args.clone())
            .await["ok"],
        true
    );
    tokio::time::sleep(Duration::from_millis(1100)).await;
    let completed = caller.call("recording_stop", recording_args).await;
    assert_eq!(completed["data"]["status"], "complete", "{completed}");
    assert_eq!(
        caller.call("preview_status", args.clone()).await["ok"],
        true
    );
    assert_eq!(caller.call("preview_stop", args).await["ok"], true);
}

#[tokio::test]
async fn connect_reports_input_capabilities_without_input_and_disconnect_releases_ownership() {
    let mut fixture = Fixture::start().await;
    let target = json!({"targetId":format!("simulator:{}", fixture.uuid)});
    assert_eq!(
        fixture.client.call("disconnect", json!({})).await["ok"],
        true
    );
    let before = state(&fixture.data, |v| v["count"].is_number()).await;
    let opened = fixture.client.call("connect", target.clone()).await;
    assert_eq!(opened["ok"], true, "{opened}");
    for capability in ["input", "long_press", "type_text"] {
        assert_eq!(
            opened["data"]["capabilities"][capability]["state"], "supported",
            "{opened}"
        );
    }
    assert_eq!(
        state(&fixture.data, |v| v["count"].is_number()).await["count"],
        before["count"]
    );
    let status = fixture.client.call("status", json!({})).await;
    assert_eq!(
        status["data"]["capabilities"]["input"]["state"],
        "supported"
    );
    assert_eq!(
        fixture.client.call("disconnect", json!({})).await["ok"],
        true
    );
    let reopened = fixture.client.call("connect", target).await;
    assert_eq!(reopened["ok"], true, "{reopened}");
    assert_eq!(reopened["data"]["reused"], false, "{reopened}");
    fixture.finish().await;
}

async fn verify_eight_action_results(fixture: &mut Fixture, caller: &mut Client, point: &Value) {
    for each in [true, false] {
        let actions = vec![json!({"action":"tap","point":point}); 8];
        let reply = caller
            .request(
                "tools/call",
                json!({"name":"act","arguments":{"actions":actions,"observeAfterEach":each}}),
            )
            .await;
        let data = &reply["result"]["structuredContent"]["data"];
        assert_eq!(reply["result"]["structuredContent"]["ok"], true, "{reply}");
        let result = &data["result"];
        assert_eq!(result["status"], "completed");
        assert_eq!(result["steps"].as_array().unwrap().len(), 8);
        assert_eq!(
            reply["result"]["content"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|item| item["type"] == "image")
                .count(),
            if each { 8 } else { 1 }
        );
        if each {
            assert_eq!(result["observation"], result["steps"][7]["observation"]);
        }
        let status = caller.call("status", json!({})).await;
        let retained = &status["data"]["lastAction"];
        assert_eq!(retained["status"], "completed");
        assert_eq!(retained["steps"], result["steps"]);
        assert_eq!(retained["observation"], result["observation"]);
        assert_eq!(retained["actionOutcomes"].as_array().unwrap().len(), 8);
        assert!(
            retained["actionOutcomes"]
                .as_array()
                .unwrap()
                .iter()
                .all(|outcome| outcome["state"] == "completed")
        );
        let count = if each { 10 } else { 18 };
        fixture.rendered(count).await;
        let observed = caller
            .request(
                "tools/call",
                json!({"name":"observe","arguments":{"profile":"fast"}}),
            )
            .await;
        primary_image(
            &observed,
            &state(&fixture.data, |v| v["count"] == count).await,
            count,
        );
    }
}
