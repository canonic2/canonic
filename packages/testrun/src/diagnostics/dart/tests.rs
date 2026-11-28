//! Owned VM Service peers, not live SDK/build acceptance.
use super::*;
use axum::{
    Router,
    extract::{
        State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    response::IntoResponse,
    routing::get,
};
use std::{
    io::Write,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};
#[derive(Clone)]
struct Peer {
    scenario: &'static str,
    calls: Arc<Mutex<Vec<Value>>>,
    clock: Arc<AtomicU64>,
    allocations: Arc<AtomicU64>,
    /// The `deep_cpu` reply's serialized functions and samples, built before
    /// any call so building them never counts against the call deadline.
    deep_cpu: Option<Arc<(String, String)>>,
}
/// Like a Debug Flutter reply: past 8 MiB, full function objects, deep stacks.
fn deep_cpu_samples() -> (String, String) {
    let functions: Vec<Value> = (0..3000).map(|i| json!({"kind":"Dart","inclusiveTicks":1,"exclusiveTicks":0,"resolvedUrl":"package:acme/main.dart","function":{"type":"@Function","id":format!("functions/{i}"),"name":format!("acme{i}"),"owner":{"type":"@Class","name":"Acme".repeat(80)},"location":{"script":{"uri":"package:acme/main.dart"},"padding":"x".repeat(1500)}}})).collect();
    let samples: Vec<Value> = (0..20000u64).map(|i| json!({"tid":1 + i % 2,"timestamp":1000 + i,"vmTag":"Dart","userTag":"Default","stack":(0..150u64).map(|f| (f + i % 50) % 3000).collect::<Vec<_>>()})).collect();
    (
        Value::Array(functions).to_string(),
        Value::Array(samples).to_string(),
    )
}
/// The prebuilt `deep_cpu` reply to a `getCpuSamples` request, spliced as text.
fn deep_cpu_reply(request: &Value, peer: &Peer) -> Option<String> {
    let (functions, samples) = &**peer
        .deep_cpu
        .as_ref()
        .filter(|_| request["method"] == "getCpuSamples")?;
    let p = &request["params"];
    Some(format!(
        r#"{{"jsonrpc":"2.0","id":{},"result":{{"type":"CpuSamples","samplePeriod":250,"sampleCount":20000,"timeOriginMicros":{},"timeExtentMicros":{},"functions":{functions},"samples":{samples}}}}}"#,
        request["id"], p["timeOriginMicros"], p["timeExtentMicros"]
    ))
}
struct Fixture {
    temp: tempfile::TempDir,
    path: PathBuf,
    peer: Peer,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}
async fn fixture(scenario: &'static str) -> Fixture {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let peer = Peer {
        scenario,
        calls: Arc::default(),
        clock: Arc::new(AtomicU64::new(1000)),
        allocations: Arc::default(),
        deep_cpu: (scenario == "deep_cpu").then(|| Arc::new(deep_cpu_samples())),
    };
    let app = Router::new()
        .route("/acme-secret=/ws", get(upgrade))
        .with_state(peer.clone());
    let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("service-uri");
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
        .open(&path)
        .unwrap()
        .write_all(format!("ws://127.0.0.1:{port}/acme-secret=/ws").as_bytes())
        .unwrap();
    Fixture {
        temp,
        path,
        peer,
        task,
    }
}
async fn upgrade(State(peer): State<Peer>, ws: WebSocketUpgrade) -> impl IntoResponse {
    ws.on_upgrade(move |socket| serve(socket, peer))
}
async fn send(socket: &mut WebSocket, value: Value) -> bool {
    socket
        .send(Message::Text(value.to_string().into()))
        .await
        .is_ok()
}
fn extensions() -> Value {
    json!([
        "ext.flutter.inspector.disposeGroup",
        "ext.flutter.inspector.getRootWidget",
        "ext.flutter.inspector.getDetailsSubtree",
        "ext.flutter.inspector.isWidgetCreationTracked",
        "ext.flutter.inspector.trackRebuildDirtyWidgets",
        "ext.flutter.inspector.trackRepaintWidgets",
        "ext.dart.io.getHttpProfile",
        "ext.dart.io.httpEnableTimelineLogging"
    ])
}
fn event(stream: &str, kind: &str, isolate: Option<&str>, extra: Value) -> Value {
    let mut e = json!({"type":"Event","kind":kind,"timestamp":1000});
    if let Some(id) = isolate {
        e["isolate"] = json!({"id":id});
    }
    for (key, value) in extra.as_object().unwrap() {
        e[key] = value.clone();
    }
    json!({"jsonrpc":"2.0","method":"streamNotify","params":{"streamId":stream,"event":e}})
}
async fn serve(mut socket: WebSocket, peer: Peer) {
    while let Some(Ok(Message::Text(message))) = socket.recv().await {
        let request: Value = serde_json::from_str(&message).unwrap();
        peer.calls.lock().unwrap().push(request.clone());
        let method = request["method"].as_str().unwrap();
        let p = &request["params"];
        if peer.scenario == "restricted" && method == "getCpuSamples" {
            send(&mut socket,json!({"jsonrpc":"2.0","id":request["id"],"error":{"code":100,"message":"private unavailable acme-secret="}})).await;
            continue;
        }
        // Independent fixtures reject global mutations and unbounded/foreign selections.
        assert!(!p.as_object().unwrap().contains_key("enabled"));
        assert!(p.get("gc").is_none() && p.get("reset").is_none());
        if let Some(id) = p.get("isolateId") {
            assert_eq!(id, "isolates/1");
        }
        if let Some(text) = deep_cpu_reply(&request, &peer) {
            if socket.send(Message::Text(text.into())).await.is_err() {
                break;
            }
            continue;
        }
        let result = match method {
            "streamListen" => {
                if p["streamId"] == "Extension" {
                    send(&mut socket,event("Extension","Extension",Some("isolates/1"),json!({"extensionKind":"Flutter.Frame","extensionData":{"build":11000,"raster":7000,"elapsed":20000,"number":1,"startTime":1000}}))).await;
                    send(&mut socket,event("Extension","Extension",Some("isolates/1"),json!({"extensionKind":"Testrun.Diagnostics","extensionData":{"schemaVersion":1,"kind":"measure","name":"platform channel","actionId":"action:channel","durationMicros":500,"metadata":{"buildId":"build-a"}}}))).await;
                    send(&mut socket,event("Logging","Logging",Some("isolates/2"),json!({"logRecord":{"message":{"valueAsString":"neighbor"},"level":1000}}))).await;
                }
                if p["streamId"] == "Logging" {
                    send(&mut socket,event("Logging","Logging",Some("isolates/1"),json!({"logRecord":{"message":{"valueAsString":"Acme exception acme-secret="},"level":1000,"stackTrace":{"valueAsString":"async stack"}}}))).await;
                }
                json!({"type":"Success"})
            }
            "getStack" => {
                assert_eq!(p["limit"], 64);
                json!({"type":"Stack","frames":[{"index":0,"function":{"name":"Acme"}}],"asyncCausalFrames":[{"function":{"name":"await Acme"}}]})
            }
            "getVMTimelineMicros" => {
                json!({"type":"Timestamp","timestamp":peer.clock.fetch_add(1000,Ordering::SeqCst)})
            }
            "getMemoryUsage" => {
                json!({"type":"MemoryUsage","heapUsage":1024,"heapCapacity":4096,"externalUsage":512})
            }
            "getAllocationProfile" => {
                if peer.scenario == "reload" {
                    send(
                        &mut socket,
                        event(
                            "Isolate",
                            "IsolateReload",
                            None,
                            json!({"isolateGroup":{"id":"isolateGroups/1"},"status":"success"}),
                        ),
                    )
                    .await;
                }
                if peer.scenario == "disconnect" {
                    break;
                }
                if peer.scenario == "hung" {
                    continue;
                }
                let n = peer.allocations.fetch_add(1, Ordering::SeqCst) + 1;
                if peer.scenario == "large" {
                    // Like a Debug Flutter isolate: every loaded class is listed, most
                    // without counters, past the shared 300,000-token parse budget.
                    let members: Vec<Value> = (0..9000u64).map(|i| {
                        let (current, accumulated) = match i { 0..900 => (i + 1, i + 1), 900..1300 => (0, i), _ => (0, 0) };
                        json!({"type":"ClassHeapStats","class":{"type":"@Class","fixedId":true,"id":format!("classes/{}", i + 1),"name":format!("Acme{i}"),"library":{"type":"@Library","id":"libraries/1","name":"acme","uri":"package:acme/main.dart"}},"bytesCurrent":current*16,"instancesCurrent":current,"accumulatedSize":accumulated*16,"instancesAccumulated":accumulated})
                    }).collect();
                    json!({"type":"AllocationProfile","members":members})
                } else {
                    json!({"type":"AllocationProfile","members":[{"class":{"id":"classes/1","name":"Retained","library":{"id":"libraries/1","uri":"package:acme/main.dart"}},"bytesCurrent":n*1024,"instancesCurrent":n,"accumulatedSize":n*1024,"instancesAccumulated":n}]})
                }
            }
            "ext.flutter.inspector.getRootWidget" => {
                if peer.scenario == "group_reload" {
                    send(
                        &mut socket,
                        event(
                            "Isolate",
                            "IsolateReload",
                            None,
                            json!({"isolateGroup":{"id":"isolateGroups/1"}}),
                        ),
                    )
                    .await;
                }
                json!({"result":{"description":"Acme","valueId":"inspector-1","children":[]}})
            }
            _ => fixture_result(method, p, &peer),
        };
        if !send(
            &mut socket,
            json!({"jsonrpc":"2.0","id":request["id"],"result":result}),
        )
        .await
        {
            break;
        }
    }
}
fn options(f: &Fixture, mode: &str) -> Options {
    Options {
        endpoint_file: f.path.clone(),
        target_id: "android:acme".into(),
        app_id: "com.example.acme".into(),
        runtime_kind: "flutter_native".into(),
        mode: mode.into(),
        isolate_id: Some("isolates/1".into()),
        vm_start_time: Some(1),
        isolate_start_time: Some(2),
        inspector: true,
        network: true,
        timeline: true,
        process_memory: true,
        include_vm_output: false,
        allow_unauthenticated: false,
        class_ids: vec!["classes/1".into()],
        object_ids: if f.peer.scenario == "auto" {
            vec![]
        } else {
            vec!["objects/1".into()]
        },
        sample_ms: 100,
        build_mode: Some("debug".into()),
        build_id: Some("build-a".into()),
        symbols_build_id: Some("build-a".into()),
    }
}
async fn collect(f: &Fixture, mode: &str, budget: Duration) -> (Vec<u8>, Value, String) {
    let mut c = Capture::new(options(f, mode)).unwrap();
    let (_sender, stopped) = watch::channel(None);
    c.control(stopped, budget);
    let mut raw = vec![];
    let mut reason = String::new();
    for _ in 0..100 {
        match c.read().await {
            Ok((packet, done, _)) => {
                raw.extend(packet);
                if done {
                    break;
                }
            }
            Err(e) => {
                reason = e.code;
                break;
            }
        }
    }
    (raw, c.settle().await, reason)
}
#[tokio::test]
async fn diagnostic_dart_owned_capture_keeps_frames_objects_samples_and_credentials_scoped() {
    let f = fixture("auto").await;
    let began = Instant::now();
    let (raw, settled, error) = collect(&f, "cpu", Duration::from_secs(10)).await;
    assert!(error.is_empty(), "{error}");
    assert_eq!(settled["cleanup"]["normalOwnedSettlement"], true);
    assert_eq!(settled["cleanup"]["groupDisposeAcknowledged"], true);
    let raw = String::from_utf8(raw).unwrap();
    assert!(!raw.contains("acme-secret="));
    assert!(!raw.contains("neighbor"));
    assert!(
        raw.contains("Flutter.Frame")
            && raw.contains("RetainingPath")
            && raw.contains("CpuSamples")
    );
    let calls = f.peer.calls.lock().unwrap();
    assert_eq!(
        calls
            .iter()
            .filter(|v| v["method"] == "ext.flutter.inspector.disposeGroup")
            .count(),
        1
    );
    // CPU samples cover the observation window only: the owned peer's clock reads are
    // 1000 (before snapshot), 2000 (window opens) and 3000 (final reads).
    let cpu = calls
        .iter()
        .find(|v| v["method"] == "getCpuSamples")
        .unwrap();
    assert_eq!(cpu["params"]["timeOriginMicros"], 2000);
    assert_eq!(cpu["params"]["timeExtentMicros"], 1000);
    eprintln!(
        "owned Dart capture {:.2}ms, {} bytes, {} RPCs",
        began.elapsed().as_secs_f64() * 1000.0,
        raw.len(),
        calls.len()
    );
}
#[tokio::test]
async fn diagnostic_dart_vm_birth_tolerates_millisecond_reads_but_fences_another_vm() {
    let f = fixture("jitter").await;
    let mut selected = options(&f, "cpu");
    selected.vm_start_time = Some(1000);
    selected.isolate_start_time = Some(2000);
    let mut c = Capture::new(selected.clone()).unwrap();
    let (_sender, stopped) = watch::channel(None);
    c.control(stopped, Duration::from_secs(8));
    let mut raw = vec![];
    loop {
        let (packet, done, _) = c.read().await.unwrap();
        raw.extend(packet);
        if done {
            break;
        }
    }
    assert_eq!(c.settle().await["generationChanged"], false);
    let raw = String::from_utf8(raw).unwrap();
    assert!(!raw.contains("DART_GENERATION_CHANGED"));
    // Every packet keeps the first observed births, so one collector stays comparable.
    let packets: Vec<Value> = raw
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert!(packets.iter().any(|p| p["data"]["position"] == "after"));
    for packet in packets.iter().filter(|p| p["kind"] != "discovery") {
        assert_eq!(packet["isolateStartTime"], 2001, "{}", packet["kind"]);
        assert_eq!(packet["vmStartTime"], 1001, "{}", packet["kind"]);
    }
    // A birth outside the bound is another VM, both at selection and before final reads.
    let f = fixture("vm_restart").await;
    let mut selected = options(&f, "cpu");
    selected.vm_start_time = Some(1000);
    let mut c = Capture::new(selected).unwrap();
    let (_sender, stopped) = watch::channel(None);
    c.control(stopped, Duration::from_secs(8));
    let mut fenced = false;
    for _ in 0..100 {
        match c.read().await {
            Ok((packet, done, _)) => {
                if String::from_utf8_lossy(&packet).contains("DART_GENERATION_CHANGED") {
                    fenced = true;
                }
                if done {
                    break;
                }
            }
            Err(e) => {
                fenced = e.code == "DART_GENERATION_CHANGED";
                break;
            }
        }
    }
    assert!(fenced);
    let f = fixture("jitter").await;
    let mut far = options(&f, "discover");
    far.vm_start_time = Some(1004);
    let mut c = Capture::new(far).unwrap();
    let (_sender, stopped) = watch::channel(None);
    c.control(stopped, Duration::from_secs(5));
    assert_eq!(c.read().await.unwrap_err().code, "DART_GENERATION_CHANGED");
}
#[tokio::test]
async fn diagnostic_dart_large_allocation_profile_keeps_counted_classes_within_packet_bounds() {
    let f = fixture("large").await;
    let (raw, settled, error) = collect(&f, "memory", Duration::from_secs(10)).await;
    assert!(error.is_empty(), "{error}");
    assert_eq!(settled["connectionLost"], false);
    assert!(
        settled["sourceLoss"]
            .to_string()
            .contains("getAllocationProfile:classes_limited_to_1024")
    );
    let text = String::from_utf8(raw.clone()).unwrap();
    let packets: Vec<Value> = text
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    for position in ["before", "after"] {
        let profile = &packets
            .iter()
            .find(|p| p["data"]["position"] == position)
            .unwrap()["data"]["allocations"];
        let members = profile["members"].as_array().unwrap();
        assert_eq!(members.len(), 1024, "{position}");
        assert_eq!(profile["membersReported"], 9000);
        assert_eq!(profile["membersOmittedWithoutCounters"], 7700);
        assert_eq!(profile["membersOmittedByLimit"], 276);
        // Every class with current instances is kept, ahead of accumulated-only ones.
        assert!(
            members[..900]
                .iter()
                .all(|m| m["instancesCurrent"].as_u64() > Some(0))
        );
        assert!(members[900..].iter().all(|m| m["instancesCurrent"] == 0));
    }
    // The reduced capture stays within the shared analysis decoder's bounds.
    let index = super::super::analysis::decode::decode("dart-diagnostics-v1", &raw).unwrap();
    assert!(index.rows.iter().filter(|r| r.kind == "allocation").count() >= 2048);
    // The analysis worker returns this index to its parent, which re-parses it within the
    // index byte bound; such an index exceeds the 300,000-token default.
    use super::super::analysis::{decode, model::MAX_INDEX};
    let output = serde_json::to_vec(&json!({ "index": index })).unwrap();
    assert!(decode::bounded_json(&output, MAX_INDEX + 1024).is_err());
    assert!(decode::bounded_json_tokens(&output, MAX_INDEX + 1024, MAX_INDEX / 4).is_ok());
}
// The peer sends on its own worker while the client receives and reduces.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn diagnostic_dart_large_cpu_reply_is_received_and_reduced_to_counted_stacks() {
    let f = fixture("deep_cpu").await;
    let (raw, settled, error) = collect(&f, "cpu", Duration::from_secs(20)).await;
    assert!(error.is_empty(), "{error}: {settled}");
    assert_eq!(settled["connectionLost"], false);
    assert!(
        settled["sourceLoss"]
            .to_string()
            .contains("getCpuSamples:stacks_or_frames_limited")
    );
    let text = String::from_utf8(raw.clone()).unwrap();
    let after: Value = text
        .lines()
        .map(|l| serde_json::from_str::<Value>(l).unwrap())
        .find(|p| p["data"]["position"] == "after")
        .unwrap();
    let cpu = after["data"]["observations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["method"] == "getCpuSamples")
        .unwrap()["result"]
        .clone();
    let stacks = cpu["samples"].as_array().unwrap();
    assert_eq!(stacks.len(), 50);
    assert_eq!(
        stacks
            .iter()
            .map(|s| s["count"].as_u64().unwrap())
            .sum::<u64>(),
        20000
    );
    // Leaf-first frames: at most 128, then within 30,000 bytes of compact functions.
    let functions = cpu["functions"].as_array().unwrap();
    for stack in stacks {
        let frames = stack["stack"].as_array().unwrap();
        let bytes: usize = frames
            .iter()
            .map(|id| {
                serde_json::to_vec(&functions[id.as_u64().unwrap() as usize])
                    .unwrap()
                    .len()
            })
            .sum();
        assert!(
            !frames.is_empty() && frames.len() < 128 && bytes <= 30_000,
            "{bytes}"
        );
    }
    assert_eq!(cpu["reduction"]["samplesWithFramesCut"], 20000);
    assert_eq!(cpu["reduction"]["stacksShortenedByBytes"], 50);
    assert!(
        cpu["functions"]
            .as_array()
            .unwrap()
            .iter()
            .all(|f| f.get("function").is_none())
    );
    // The reduced capture decodes into one counted row per distinct stack.
    let index = super::super::analysis::decode::decode("dart-diagnostics-v1", &raw).unwrap();
    let rows: Vec<_> = index
        .rows
        .iter()
        .filter(|r| r.name == "Dart sampled CPU stack")
        .collect();
    assert_eq!(rows.len(), 50);
    assert_eq!(rows.iter().map(|r| r.value.unwrap()).sum::<f64>(), 20000.0);
}
#[tokio::test]
async fn diagnostic_dart_discovery_never_subscribes_or_allocates_inspector_objects() {
    let f = fixture("normal").await;
    let (_, settled, error) = collect(&f, "discover", Duration::from_secs(5)).await;
    assert!(error.is_empty());
    assert_eq!(settled["cleanup"]["normalOwnedSettlement"], true);
    let methods: Vec<_> = f
        .peer
        .calls
        .lock()
        .unwrap()
        .iter()
        .map(|v| v["method"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(methods, vec!["getVersion", "getVM", "getIsolate"]);
}
#[tokio::test]
async fn diagnostic_dart_reload_restart_disconnect_expiry_and_restricted_extensions_are_distinct() {
    for scenario in [
        "reload",
        "group_reload",
        "restart",
        "disconnect",
        "expired",
        "restricted",
        "version",
    ] {
        let f = fixture(scenario).await;
        let (raw, settled, error) = collect(&f, "cpu", Duration::from_secs(8)).await;
        let raw = String::from_utf8(raw).unwrap();
        assert!(!raw.contains("acme-secret="), "{scenario}");
        if scenario == "restart" {
            assert!(error == "DART_GENERATION_CHANGED" || raw.contains("DART_GENERATION_CHANGED"));
            assert_eq!(settled["generationChanged"], true);
        }
        if scenario == "version" {
            assert_eq!(error, "DART_PROTOCOL_VERSION");
        }
        if scenario == "expired" {
            assert!(raw.contains("DART_OBJECT_UNAVAILABLE"));
        }
        if scenario == "reload" || scenario == "group_reload" {
            assert!(raw.contains("IsolateReload"));
            assert_eq!(settled["generationChanged"], true);
            assert!(
                !f.peer
                    .calls
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|v| v["method"] == "getCpuSamples")
            );
        }
        if scenario == "group_reload" {
            assert_eq!(settled["cleanup"]["inspectorObjectsMayBeRetained"], true);
            assert!(
                !f.peer
                    .calls
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|v| v["method"] == "ext.flutter.inspector.disposeGroup")
            );
        }
        if scenario == "restricted" {
            assert!(settled.to_string().contains("rpc_code:100"));
            assert!(
                !f.peer
                    .calls
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|v| v["method"].as_str().unwrap().starts_with("ext."))
            );
        }
    }
}
#[tokio::test]
async fn diagnostic_dart_cancel_limit_and_uncertain_rpc_settle_without_replay_or_global_reset() {
    let f = fixture("normal").await;
    let mut c = Capture::new(options(&f, "memory")).unwrap();
    let (sender, stopped) = watch::channel(None);
    c.control(stopped, Duration::from_secs(8));
    while c.stage < 4 {
        c.read().await.unwrap();
    }
    sender.send(Some("cancel".into())).unwrap();
    assert_eq!(c.read().await.unwrap_err().code, "CANCELLED");
    assert_eq!(c.settle().await["cleanup"]["normalOwnedSettlement"], true);
    c.settle().await;
    assert_eq!(
        f.peer
            .calls
            .lock()
            .unwrap()
            .iter()
            .filter(|v| v["method"] == "ext.flutter.inspector.disposeGroup")
            .count(),
        1
    );
    let f = fixture("hung").await;
    let (_, _, error) = collect(&f, "memory", Duration::from_millis(300)).await;
    assert!(matches!(error.as_str(), "DART_CONNECTION_LOST" | "TIMEOUT"));
    let calls = f.peer.calls.lock().unwrap();
    assert_eq!(
        calls
            .iter()
            .filter(|v| v["method"] == "getAllocationProfile")
            .count(),
        1
    );
    assert!(!calls.iter().any(|v| v["method"] == "getCpuSamples"));
}
#[tokio::test]
async fn diagnostic_dart_shared_owner_binding_replay_and_capture_limit_are_retained() {
    let f = fixture("normal").await;
    let root = f.temp.path().join("state");
    let owner = crate::diagnostics::Owners::new("dart-owner".into());
    let opened=owner.open(&root,"acme",&json!({"submissionId":"dart","question":"Acme retained objects","binding":{"targetId":"android:acme","appId":"com.example.acme","actionIds":["action:dart"]}})).unwrap();
    let id = opened["investigationId"].as_str().unwrap();
    let mut source = serde_json::to_value(options(&f, "memory")).unwrap();
    source["kind"] = json!("dart");
    let args = json!({"investigationId":id,"submissionId":"bounded","source":source,"limits":{"maxEvents":4}});
    let builds = crate::builds::Owners::default();
    let (_, work) = owner.start(&root, "acme", &args, &builds).unwrap();
    owner
        .run(work.unwrap(), root.clone(), "acme".into(), builds.clone())
        .await;
    let status = owner
        .inspect(
            &root,
            "acme",
            "diagnostics_status",
            &json!({"investigationId":id}),
        )
        .unwrap();
    let c = &status["collectors"][0];
    assert_eq!(c["settled"], true);
    assert_eq!(c["reason"], "capture_limit");
    assert_eq!(c["sourceReport"]["cleanup"]["normalOwnedSettlement"], true);
    assert!(!status.to_string().contains("acme-secret="));
    let calls = f.peer.calls.lock().unwrap().len();
    assert!(
        owner
            .start(&root, "acme", &args, &builds)
            .unwrap()
            .1
            .is_none()
    );
    assert_eq!(f.peer.calls.lock().unwrap().len(), calls);
    let mut mismatch = args.clone();
    mismatch["submissionId"] = json!("mismatch");
    mismatch["source"]["appId"] = json!("com.example.other");
    assert!(owner.start(&root, "acme", &mismatch, &builds).is_err());
}
#[tokio::test]
async fn diagnostic_dart_endpoint_file_and_source_schema_refuse_unsafe_or_unbounded_inputs() {
    let f = fixture("normal").await;
    let schema = &crate::diagnostics::definition("diagnostics_start").unwrap()["inputSchema"];
    let mut source = serde_json::to_value(options(&f, "cpu")).unwrap();
    source["kind"] = json!("dart");
    let mut args = json!({"investigationId":"diag-acme","submissionId":"dart","source":source});
    // Optional absent fields serialize as null internally; public callers omit them.
    args["source"]
        .as_object_mut()
        .unwrap()
        .retain(|_, v| !v.is_null());
    crate::schema::validate(schema, &args).unwrap();
    args["source"]["method"] = json!("resume");
    assert!(crate::schema::validate(schema, &args).is_err());
    let mut bad = options(&f, "cpu");
    bad.vm_start_time = None;
    assert!(Capture::new(bad).is_err());
    let mut bad = options(&f, "cpu");
    bad.runtime_kind = "flutter_web".into();
    assert!(Capture::new(bad).is_err());
    #[cfg(unix)]
    {
        use std::os::unix::fs::{PermissionsExt, symlink};
        std::fs::set_permissions(&f.path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(Capture::new(options(&f, "cpu")).is_err());
        std::fs::set_permissions(&f.path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let link = f.temp.path().join("link");
        symlink(&f.path, &link).unwrap();
        let mut bad = options(&f, "cpu");
        bad.endpoint_file = link;
        assert!(Capture::new(bad).is_err());
    }
}

fn fixture_result(method: &str, p: &Value, peer: &Peer) -> Value {
    match method {
        "getVersion" => {
            json!({"type":"Version","major":if peer.scenario=="version"{5}else{4},"minor":23})
        }
        "getVM" => fixture_vm(peer),
        "getIsolate" => fixture_isolate(peer),
        "getInstances" => {
            assert_eq!(p["objectId"], "classes/1");
            assert_eq!(p["limit"], 16);
            json!({"type":"InstanceSet","totalCount":1,"instances":[{"id":"objects/1","class":{"id":"classes/1"}}]})
        }
        "getObject" => {
            assert_eq!(p["objectId"], "objects/1");
            assert_eq!(p["count"], 32);
            if peer.scenario == "expired" {
                json!({"type":"Sentinel","kind":"Expired","valueAsString":"expired"})
            } else {
                json!({"type":"Instance","id":"objects/1","fields":[{"name":"retained","value":{"id":"objects/2"}}]})
            }
        }
        "getRetainingPath" => {
            assert_eq!(p["limit"], 32);
            json!({"type":"RetainingPath","length":2,"gcRootType":"static fields table","elements":[{"index":0,"value":{"id":"objects/1"}},{"index":1,"value":{"id":"objects/2"},"parentField":"retained"}]})
        }
        "getInboundReferences" => {
            assert_eq!(p["limit"], 32);
            json!({"type":"InboundReferences","references":[{"source":{"id":"objects/2"},"parentField":"retained"}]})
        }
        "getCpuSamples" => {
            json!({"type":"CpuSamples","samplePeriod":1000,"sampleCount":2,"timeOriginMicros":p["timeOriginMicros"],"timeExtentMicros":p["timeExtentMicros"],"functions":[{"kind":"Dart","function":{"name":"Acme"},"resolvedUrl":"package:acme/main.dart"}],"samples":[{"tid":1,"timestamp":1100,"stack":[0]},{"tid":1,"timestamp":1200,"stack":[0]}]})
        }
        "getVMTimeline" => {
            json!({"type":"Timeline","traceEvents":[{"ph":"X","name":"Layout","ts":1100,"dur":1000,"pid":42,"tid":1}]})
        }
        "getProcessMemoryUsage" => {
            json!({"type":"ProcessMemoryUsage","root":{"name":"Process","size":8192,"children":[]}})
        }
        "ext.flutter.inspector.disposeGroup" => {
            assert!(p["objectGroup"].as_str().unwrap().starts_with("testrun-"));
            json!({"result":null})
        }
        "ext.flutter.inspector.getDetailsSubtree" => {
            assert_eq!(p["subtreeDepth"], "1");
            assert_eq!(p["arg"], "inspector-1");
            json!({"result":{"description":"Acme","properties":[{"name":"size","description":"100x50"},{"name":"needsPaint","description":"false"}]}})
        }
        "ext.flutter.inspector.isWidgetCreationTracked" => json!({"result":true}),
        "ext.flutter.inspector.trackRebuildDirtyWidgets"
        | "ext.flutter.inspector.trackRepaintWidgets" => json!({"enabled":"false"}),
        "ext.dart.io.httpEnableTimelineLogging" => {
            json!({"type":"HttpTimelineLoggingState","enabled":false})
        }
        "ext.dart.io.getHttpProfile" => {
            json!({"type":"HttpProfile","timestamp":1000,"requests":[{"id":"1","uri":"https://example.com","method":"GET","startTime":100}]})
        }
        _ => panic!("Unexpected VM effect: {method}"),
    }
}

fn fixture_vm(peer: &Peer) -> Value {
    // `jitter` reports the same VM's birth 1 ms apart across reads;
    // `vm_restart` reports another VM birth after the first read.
    let reads = peer
        .calls
        .lock()
        .unwrap()
        .iter()
        .filter(|v| v["method"] == "getVM")
        .count() as u64;
    let start = match peer.scenario {
        "jitter" => 1000 + reads % 2,
        "vm_restart" if reads > 1 => 9000,
        "vm_restart" => 1000,
        _ => 1,
    };
    json!({"type":"VM","pid":42,"startTime":start,"name":"acme-secret=","version":"owned Dart fixture","isolates":[{"id":"isolates/1","name":"main"},{"id":"isolates/2","name":"worker"}]})
}

fn fixture_isolate(peer: &Peer) -> Value {
    let reads = peer
        .calls
        .lock()
        .unwrap()
        .iter()
        .filter(|v| v["method"] == "getIsolate")
        .count() as u64;
    let start = match peer.scenario {
        "restart" => 9002,
        "jitter" => 2000 + reads % 2,
        _ => 2,
    };
    json!({"type":"Isolate","id":"isolates/1","startTime":start,"isolateGroupId":"isolateGroups/1","rootLib":{"id":"libraries/1","uri":"package:acme/main.dart"},"extensionRPCs":if peer.scenario=="restricted"{json!([])}else{extensions()},"pauseEvent":{"kind":"Resume"}})
}
