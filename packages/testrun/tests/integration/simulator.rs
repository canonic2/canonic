//! Real MCP/native lifecycle tests against an explicitly owned simulator fixture.
#[tokio::test]
async fn mcp_starts_in_any_directory_and_ignores_stale_configuration() {
    for stale_file in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        if stale_file {
            std::fs::write(
                directory.path().join("testrun.json"),
                "invalid obsolete configuration",
            )
            .unwrap();
        }
        let mut client = Client::start(directory.path()).await;
        let listed = client.request("tools/list", json!({})).await;
        let tools = listed["result"]["tools"].as_array().unwrap();
        let connect = tools.iter().find(|tool| tool["name"] == "connect").unwrap();
        assert_eq!(
            connect["inputSchema"]["anyOf"][0]["required"],
            json!(["targetId"])
        );
        assert_eq!(
            connect["inputSchema"]["anyOf"][1]["required"],
            json!(["engine"])
        );
        let status = client.call("status", json!({})).await;
        assert_eq!(status["ok"], true, "{status}");
        assert_eq!(status["data"]["state"], "not_connected", "{status}");
        assert_eq!(status["data"]["targets"], json!([]));
        let closed = client.call("disconnect", json!({})).await;
        assert_eq!(closed["ok"], true, "{closed}");
        // Close only after all responses; queue draining on EOF has its own contract.
        drop(client.input);
        assert!(
            tokio::time::timeout(Duration::from_secs(5), client.child.wait())
                .await
                .unwrap()
                .unwrap()
                .success()
        );
    }
}

#[cfg(target_os = "macos")]
mod lifecycle;
#[cfg(feature = "acceptance-ios-simulator")]
mod retained;
#[cfg(feature = "acceptance-ios-simulator")]
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::{process::Stdio, time::Duration};
#[cfg(feature = "acceptance-ios-simulator")]
use tokio::io::AsyncReadExt;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, ChildStdout, Command},
};

pub(super) struct Client {
    pub(super) child: Child,
    pub(super) input: ChildStdin,
    output: BufReader<ChildStdout>,
    seq: u64,
    timeout: Duration,
    /// This client's own data root and owner process, unless it shares the
    /// acceptance data root.
    #[allow(
        dead_code,
        reason = "Held so its owner process stops with the client; acceptance suites read it"
    )]
    workspace: Option<crate::Workspace>,
    #[allow(
        dead_code,
        reason = "Acceptance suites share this data root with their CLI"
    )]
    pub(super) data_root: std::path::PathBuf,
}
impl Client {
    async fn start(directory: &std::path::Path) -> Self {
        Self::start_with_timeout(directory, Duration::from_secs(20)).await
    }
    pub(super) async fn start_with_timeout(directory: &std::path::Path, timeout: Duration) -> Self {
        Self::start_bound(directory, timeout, None).await
    }
    pub(super) async fn start_bound(
        directory: &std::path::Path,
        timeout: Duration,
        context: Option<&str>,
    ) -> Self {
        Self::start_environment(directory, timeout, context, &[]).await
    }
    /// A client of the acceptance data root that CLI commands in the same
    /// hardware suite share.
    #[cfg(feature = "acceptance-ios-device")]
    pub(super) async fn start_shared(
        directory: &std::path::Path,
        timeout: Duration,
        context: Option<&str>,
    ) -> Self {
        Self::start_in(None, directory, timeout, context, &[]).await
    }
    async fn start_environment(
        directory: &std::path::Path,
        timeout: Duration,
        context: Option<&str>,
        environment: &[(&str, &std::ffi::OsStr)],
    ) -> Self {
        Self::start_in(
            Some(crate::Workspace::new()),
            directory,
            timeout,
            context,
            environment,
        )
        .await
    }
    async fn start_in(
        workspace: Option<crate::Workspace>,
        directory: &std::path::Path,
        timeout: Duration,
        context: Option<&str>,
        environment: &[(&str, &std::ffi::OsStr)],
    ) -> Self {
        let executable = std::env::var_os("TESTRUN_TEST_BINARY")
            .unwrap_or_else(|| env!("CARGO_BIN_EXE_testrun").into());
        let data_root = workspace.as_ref().map_or_else(
            || crate::acceptance_workspace().data_root(),
            crate::Workspace::data_root,
        );
        let mut command = Command::new(executable);
        command.arg("--json");
        command
            .env("TESTRUN_DATA_ROOT", &data_root)
            .env_remove("TESTRUN_TASK_ID")
            .env_remove("CODEX_THREAD_ID")
            .current_dir(directory)
            .args(["mcp", "serve"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true);
        if let Some(context) = context {
            command
                .env("TESTRUN_TASK_ID", context)
                .env("TMPDIR", directory);
        }
        command.envs(environment.iter().copied());
        let mut child = command.spawn().unwrap();
        let input = child.stdin.take().unwrap();
        let output = BufReader::new(child.stdout.take().unwrap());
        let mut client = Self {
            child,
            input,
            output,
            seq: 0,
            timeout,
            workspace,
            data_root,
        };
        let initialized=client.request("initialize",json!({"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"testrun-acceptance","version":"1"}})).await;
        assert_eq!(
            initialized["result"]["serverInfo"]["name"],
            "canonic-testrun"
        );
        client
            .input
            .write_all(b"{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n")
            .await
            .unwrap();
        client
    }
    pub(super) async fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.begin_request(method, params).await;
        self.response(id).await
    }
    pub(super) async fn begin_request(&mut self, method: &str, params: Value) -> u64 {
        self.seq += 1;
        self.send_request(self.seq, method, params).await;
        self.seq
    }
    async fn send_request(&mut self, id: u64, method: &str, params: Value) {
        let request = json!({"jsonrpc":"2.0","id":id,"method":method,"params":params});
        let mut bytes = serde_json::to_vec(&request).unwrap();
        bytes.push(b'\n');
        self.input.write_all(&bytes).await.unwrap();
    }
    pub(super) async fn response(&mut self, id: u64) -> Value {
        let mut response = String::new();
        tokio::time::timeout(self.timeout, self.output.read_line(&mut response))
            .await
            .unwrap()
            .unwrap();
        let response: Value = serde_json::from_str(&response).unwrap();
        assert_eq!(response["id"], id);
        response
    }
    /// This client's owner process, from its record.
    #[cfg(feature = "acceptance-ios-simulator")]
    pub(super) fn owner_pid(&self) -> Option<i32> {
        self.workspace
            .as_ref()
            .map_or_else(crate::acceptance_workspace, |workspace| workspace)
            .owner_pid()
    }
    /// The manager's pool records in this client's data root.
    #[cfg(any(
        feature = "acceptance-ios-simulator",
        feature = "acceptance-ios-device"
    ))]
    pub(super) fn pool_directory(&self) -> std::path::PathBuf {
        self.workspace
            .as_ref()
            .map_or_else(crate::acceptance_workspace, |workspace| workspace)
            .pool_directory()
    }
    pub(super) async fn call(&mut self, name: &str, arguments: Value) -> Value {
        self.request("tools/call", json!({"name":name,"arguments":arguments}))
            .await["result"]["structuredContent"]
            .clone()
    }
    /// Repeats an earlier tools/call request id: the same logical request.
    #[cfg(feature = "acceptance-ios-simulator")]
    async fn call_again(&mut self, id: u64, name: &str, arguments: Value) -> Value {
        self.send_request(id, "tools/call", json!({"name":name,"arguments":arguments}))
            .await;
        self.response(id).await["result"]["structuredContent"].clone()
    }
    /// Performs `actions` on the connected device.
    #[cfg(feature = "acceptance-ios-simulator")]
    async fn act(&mut self, actions: Value) -> Value {
        self.call("act", json!({"actions":actions})).await
    }
}

#[cfg(feature = "acceptance-ios-simulator")]
fn center(found: &Value) -> Value {
    let frame = &found["data"]["data"]["matches"][0]["frame"];
    json!({"x":(frame["x"].as_f64().unwrap()+frame["width"].as_f64().unwrap()/2.0)/402.0,"y":(frame["y"].as_f64().unwrap()+frame["height"].as_f64().unwrap()/2.0)/874.0})
}

#[tokio::test]
#[cfg(feature = "acceptance-ios-simulator")]
#[expect(
    clippy::too_many_lines,
    reason = "One owned UIKit connection verifies touch, typing, privacy and cancellation outcomes together"
)]
async fn native_touch_phases_secure_fields_typing_and_cancel() {
    let uuid = std::env::var("TESTRUN_TEST_SIMULATOR").unwrap();
    let target_id = format!("simulator:{uuid}");
    let fixture = tempfile::tempdir().unwrap();
    let mut client = Client::start(fixture.path()).await;
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
    let app = "com.canonic.testrun.fixture";
    let _ = client
        .act(json!([{"action":"app_terminate","appId":app}]))
        .await;
    let launched = client
        .act(json!([{"action":"app_launch","appId":app}]))
        .await;
    assert_eq!(
        launched["data"]["result"]["status"], "completed",
        "{launched}"
    );
    let observation = json!({"scope":{"kind":"display"},"allowPartial":true});
    let mut query = observation.clone();
    query["query"] = json!({"identifier":"qa.password"});
    let deadline = tokio::time::Instant::now() + Duration::from_secs(8);
    let secure = loop {
        let observed = client.call("ui_find", query.clone()).await;
        if observed["data"]["data"]["matches"]
            .as_array()
            .is_some_and(|m| !m.is_empty())
        {
            break observed;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "Fixture did not become observable: {observed}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    };
    let password = &secure["data"]["data"]["matches"][0];
    assert_eq!(password["secure"], true);
    assert!(password.get("value").is_none());
    let bytes = std::fs::read(secure["data"]["artifactPath"].as_str().unwrap()).unwrap();
    assert!(
        !String::from_utf8(bytes)
            .unwrap()
            .contains("fixture-secret-example")
    );
    query["query"] = json!({"identifier":"qa.touch-surface"});
    let point = center(&client.call("ui_find", query.clone()).await);
    let tapped = client.act(json!([{"action":"tap","point":point}])).await;
    assert_eq!(tapped["data"]["result"]["status"], "completed", "{tapped}");
    query["query"] = json!({"identifier":"qa.touch-summary"});
    let summary = client.call("ui_find", query.clone()).await;
    assert!(
        summary["data"]["data"]["matches"][0]["label"]
            .as_str()
            .unwrap()
            .contains("down=1 moved=0 up=1")
    );
    let batch = json!({"actions":[{"action":"tap","point":point},{"action":"tap","point":point}],"observeAfterEach":true});
    let response = client
        .request("tools/call", json!({"name":"act","arguments":batch}))
        .await;
    let request_id = client.seq;
    let batched = &response["result"]["structuredContent"];
    assert_eq!(batched["ok"], true, "{batched}");
    let result = &batched["data"]["result"];
    assert_eq!(result["steps"].as_array().unwrap().len(), 2);
    assert_eq!(result["steps"][0]["observation"]["profile"], "fast");
    assert_ne!(
        result["steps"][0]["observation"]["artifactId"],
        result["steps"][1]["observation"]["artifactId"]
    );
    assert_eq!(result["observation"], result["steps"][1]["observation"]);
    assert!(
        response["result"]["content"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|c| c["type"] == "image")
            .count()
            == 2
    );
    // The same request id is the same logical request: it is not replayed.
    let repeated = client.call_again(request_id, "act", batch).await;
    assert_eq!(repeated["data"]["replayed"], false, "{repeated}");
    let mut summary_query = observation.clone();
    summary_query["query"] = json!({"identifier":"qa.touch-summary"});
    let summary = client.call("ui_find", summary_query).await;
    assert!(
        summary["data"]["data"]["matches"][0]["label"]
            .as_str()
            .unwrap()
            .contains("down=3 moved=0 up=3")
    );
    query["query"] = json!({"identifier":"qa.contact"});
    let contact = center(&client.call("ui_find", query.clone()).await);
    let focused = client.act(json!([{"action":"tap","point":contact}])).await;
    assert_eq!(
        focused["data"]["result"]["status"], "completed",
        "{focused}"
    );
    let typed = client
        .act(json!([{"action":"type_text","text":"Acme42!","replace":true}]))
        .await;
    assert_eq!(typed["data"]["result"]["status"], "completed", "{typed}");
    assert!(!typed.to_string().contains("Acme42!"));
    let contact = client.call("ui_find", query.clone()).await;
    assert_eq!(contact["data"]["data"]["matches"][0]["value"], "Acme42!");
    client.act(json!([{"action":"key","key":"escape"}])).await;
    let hold = json!({"actions":[{"action":"long_press","point":point,"durationMs":10000}]});
    let request_id = client
        .begin_request("tools/call", json!({"name":"act","arguments":hold}))
        .await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    let ping = tokio::time::timeout(Duration::from_secs(2), client.request("ping", json!({})))
        .await
        .unwrap();
    assert_eq!(ping["result"], json!({}));
    let cancellation = json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":request_id}});
    let mut bytes = serde_json::to_vec(&cancellation).unwrap();
    bytes.push(b'\n');
    client.input.write_all(&bytes).await.unwrap();
    let response = tokio::time::timeout(Duration::from_secs(8), client.request("ping", json!({})))
        .await
        .unwrap();
    assert_eq!(response["result"], json!({}));
    let status = client.call("status", json!({})).await;
    assert_ne!(
        status["data"]["lastAction"]["status"], "completed",
        "{status}"
    );
    let blocked = client
        .act(json!([{"action":"app_launch","appId":app}]))
        .await;
    assert_eq!(blocked["ok"], false, "{blocked}");
    assert_eq!(client.call("disconnect", json!({})).await["ok"], true);
    drop(client.input);
    client.child.wait().await.unwrap();
}

#[tokio::test]
#[cfg(feature = "acceptance-ios-simulator")]
#[expect(
    clippy::too_many_lines,
    reason = "One MCP connection verifies capture, preview authentication, MP4 finalization and cleanup together"
)]
async fn native_capture_accessibility_media_and_cleanup() {
    let uuid = std::env::var("TESTRUN_TEST_SIMULATOR").expect("Select an owned simulator fixture");
    let target_id = format!("simulator:{uuid}");
    let fixture = tempfile::tempdir().unwrap();
    let mut client = Client::start(fixture.path()).await;
    let discovery = client
        .call("target_list", json!({"kind":"simulator","limit":200}))
        .await;
    assert_eq!(discovery["ok"], true);
    let target = discovery["data"]["targets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["targetId"] == target_id)
        .unwrap();
    assert!(
        target["name"].as_str().unwrap().starts_with("Testrun-QA-"),
        "Only owned Testrun QA fixtures may be used by this test"
    );
    assert_eq!(target["platform"], "ios");
    let opened = client.call("connect", json!({"targetId":target_id})).await;
    assert_eq!(opened["ok"], true, "{opened}");
    let launch = json!({"actions":[{"action":"app_launch","appId":"com.apple.Preferences"}]});
    let first = client
        .request("tools/call", json!({"name":"act","arguments":launch}))
        .await["result"]["structuredContent"]
        .clone();
    let launch_id = client.seq;
    assert_eq!(first["data"]["result"]["status"], "completed", "{first}");
    let duplicate = client.call_again(launch_id, "act", launch).await;
    assert_eq!(duplicate["data"]["replayed"], false, "{duplicate}");
    let conflicting = client
        .call_again(
            launch_id,
            "act",
            json!({"actions":[{"action":"app_launch","appId":"com.apple.mobilesafari"}]}),
        )
        .await;
    assert_eq!(conflicting["code"], "COMMAND_ID_CONFLICT", "{conflicting}");
    let screenshot = client.call("observe", json!({"profile":"evidence"})).await;
    assert_eq!(screenshot["ok"], true, "{screenshot}");
    let screenshot = &screenshot["data"]["observation"];
    let png = std::fs::read(screenshot["path"].as_str().unwrap()).unwrap();
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    assert_eq!(screenshot["source"], "native_iosurface");
    let snapshot = json!({"scope":{"kind":"display"}});
    let mut readiness = snapshot.clone();
    readiness["allowPartial"] = json!(true);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(8);
    loop {
        let observed = client.call("ui_snapshot", readiness.clone()).await;
        if observed["ok"] == true {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "Launched application did not expose its UI: {observed}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(
        client.call("ui_snapshot", snapshot.clone()).await["code"],
        "PARTIAL_UI_TREE"
    );
    let tree = client.call("ui_snapshot", readiness).await;
    assert_eq!(tree["ok"], true, "{tree}");
    assert!(tree["data"]["data"]["nodeCount"].as_u64().unwrap() > 0);
    let codec = std::env::var("TESTRUN_TEST_CODEC").unwrap_or_else(|_| "jpeg".into());
    let preview = client.call("preview_start", json!({"format":codec})).await;
    assert_eq!(preview["ok"], true, "{preview}");
    let broker = preview["data"]["grantBrokerPath"].as_str().unwrap();
    let mut grants = tokio::net::UnixStream::connect(broker).await.unwrap();
    // The grant broker's wire names the preview by its media stream id.
    let grant_request = json!({"streamId":preview["data"]["previewId"]})
        .to_string()
        .into_bytes();
    grants
        .write_all(&(grant_request.len() as u32).to_be_bytes())
        .await
        .unwrap();
    grants.write_all(&grant_request).await.unwrap();
    let mut header = [0; 4];
    grants.read_exact(&mut header).await.unwrap();
    let mut response = vec![0; u32::from_be_bytes(header) as usize];
    grants.read_exact(&mut response).await.unwrap();
    let grant: Value = serde_json::from_slice(&response).unwrap();
    assert!(
        !preview
            .to_string()
            .contains(grant["token"].as_str().unwrap())
    );
    let url = preview["data"]["mediaUrl"]
        .as_str()
        .unwrap()
        .replacen("http:", "ws:", 1);
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    let mut origin = url.clone().into_client_request().unwrap();
    origin
        .headers_mut()
        .insert("Origin", "https://example.com".parse().unwrap());
    assert!(tokio_tungstenite::connect_async(origin).await.is_err());
    let (mut ws, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
    ws.send(tokio_tungstenite::tungstenite::Message::Text(
        json!({"token":grant["token"]}).to_string().into(),
    ))
    .await
    .unwrap();
    let ready = tokio::time::timeout(Duration::from_secs(3), ws.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(ready.to_text().unwrap()).unwrap()["event"],
        "ready"
    );
    let (mut reused, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
    reused
        .send(tokio_tungstenite::tungstenite::Message::Text(
            json!({"token":grant["token"]}).to_string().into(),
        ))
        .await
        .unwrap();
    let rejected = tokio::time::timeout(Duration::from_secs(3), reused.next())
        .await
        .unwrap();
    assert!(!matches!(
        rejected,
        Some(Ok(tokio_tungstenite::tungstenite::Message::Text(_)))
    ));
    let frame = tokio::time::timeout(Duration::from_secs(3), ws.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap()
        .into_data();
    let wire_header = frame[..24].try_into().unwrap();
    let (meta, payload) = testrun::media::lengths(&wire_header).unwrap();
    let metadata: Value = serde_json::from_slice(&frame[24..24 + meta]).unwrap();
    let source_epoch = metadata["epoch"].clone();
    assert_eq!(screenshot["captureTimingsMs"]["sourceEpoch"], source_epoch);
    assert_eq!(frame.len(), 24 + meta + payload);
    if codec == "jpeg" {
        assert_eq!(&frame[24 + meta..24 + meta + 2], b"\xff\xd8");
    } else {
        assert_eq!(frame[6], 2, "H.264 starts with decoder configuration");
        assert_eq!(&frame[24 + meta..24 + meta + 4], &[0, 0, 0, 1]);
        let keyframe = tokio::time::timeout(Duration::from_secs(3), ws.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap()
            .into_data();
        assert_eq!(keyframe[6], 1);
        assert_eq!(keyframe[7] & 1, 1, "A new viewer starts at a keyframe");
    }
    ws.close(None).await.unwrap();
    let recording = client
        .call("recording_start", json!({"maxDurationMs":1000}))
        .await;
    assert_eq!(recording["ok"], true, "{recording}");
    assert_eq!(recording["data"]["maxDurationMs"], 1000, "{recording}");
    tokio::time::sleep(Duration::from_millis(300)).await;
    let completed = client
        .call(
            "recording_stop",
            json!({"recordingId":recording["data"]["recordingId"]}),
        )
        .await;
    assert_eq!(completed["data"]["status"], "complete", "{completed}");
    assert_eq!(completed["data"]["statistics"]["sourceEpoch"], source_epoch);
    let neighbor = client
        .call(
            "preview_start",
            json!({"format":if codec == "jpeg" { "h264" } else { "jpeg" }}),
        )
        .await;
    assert_eq!(neighbor["ok"], true, "{neighbor}");
    let fresh = client.call("observe", json!({"profile":"evidence"})).await;
    assert_eq!(
        fresh["data"]["observation"]["captureTimingsMs"]["sourceEpoch"],
        source_epoch
    );
    let second_recording = client
        .call("recording_start", json!({"maxDurationMs":400}))
        .await;
    assert_eq!(second_recording["ok"], true, "{second_recording}");
    tokio::time::sleep(Duration::from_millis(200)).await;
    let second_result = client
        .call(
            "recording_stop",
            json!({"recordingId":second_recording["data"]["recordingId"]}),
        )
        .await;
    assert_eq!(
        second_result["data"]["status"], "complete",
        "{second_result}"
    );
    assert_eq!(
        second_result["data"]["statistics"]["sourceEpoch"],
        source_epoch
    );
    let video = std::fs::read(completed["data"]["artifact"]["path"].as_str().unwrap()).unwrap();
    assert!(video.windows(4).any(|bytes| bytes == b"moov"));
    assert_eq!(&video[4..8], b"ftyp");
    assert_eq!(
        client
            .call(
                "preview_stop",
                json!({"previewId":preview["data"]["previewId"]})
            )
            .await["data"]["stopped"],
        true
    );
    assert_eq!(
        client
            .call(
                "preview_status",
                json!({"previewId":neighbor["data"]["previewId"]})
            )
            .await["data"]["metadata"]["epoch"],
        source_epoch
    );
    assert_eq!(
        client.call("disconnect", json!({})).await["data"]["closed"],
        true
    );
    assert_eq!(
        client.call("observe", json!({})).await["code"],
        "NOT_CONNECTED"
    );
    drop(client.input);
    tokio::time::timeout(Duration::from_secs(5), client.child.wait())
        .await
        .unwrap()
        .unwrap();
    assert!(!std::path::Path::new(broker).exists());
}
