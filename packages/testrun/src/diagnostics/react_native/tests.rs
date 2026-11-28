//! Pure route and scope refusals; socket/profiler behavior has owned peer fixtures.
use super::*;
use axum::{
    Json, Router,
    extract::{
        State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    response::IntoResponse,
    routing::get,
};
use std::sync::{Arc, Mutex};

#[derive(Clone)]
struct Peer {
    port: u16,
    scenario: &'static str,
    calls: Arc<Mutex<Vec<String>>>,
}
struct Fixture {
    base: String,
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
        port,
        scenario,
        calls: Arc::default(),
    };
    let router = Router::new()
        .route("/json/list", get(list))
        .route(
            "/json/version",
            get(|| async { Json(json!({"Browser":"owned RN fixture"})) }),
        )
        .route("/inspector/debug", get(upgrade))
        .with_state(peer.clone());
    let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    Fixture {
        base: format!("http://127.0.0.1:{port}"),
        peer,
        task,
    }
}
async fn list(State(peer): State<Peer>) -> Json<Value> {
    Json(if peer.scenario == "missing" {
        json!([])
    } else {
        json!([{"id":"device-1","appId":"com.example.acme","reactNative":{"logicalDeviceId":"device","capabilities":{"supportsMultipleDebuggers":peer.scenario!="legacy"}},"webSocketDebuggerUrl":format!("ws://127.0.0.1:{}/inspector/debug?device=device&page=1",peer.port)}])
    })
}
async fn upgrade(
    State(peer): State<Peer>,
    headers: axum::http::HeaderMap,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    if headers
        .get(axum::http::header::ORIGIN)
        .and_then(|v| v.to_str().ok())
        != Some(format!("http://127.0.0.1:{}", peer.port).as_str())
    {
        return (
            axum::http::StatusCode::FORBIDDEN,
            "Exact inspector origin required",
        )
            .into_response();
    }
    ws.on_upgrade(move |socket| serve(socket, peer))
        .into_response()
}
async fn send(socket: &mut WebSocket, value: Value) -> bool {
    socket
        .send(Message::Text(value.to_string().into()))
        .await
        .is_ok()
}
async fn serve(mut socket: WebSocket, peer: Peer) {
    while let Some(Ok(message)) = socket.recv().await {
        let Message::Text(message) = message else {
            break;
        };
        let command: Value = serde_json::from_str(&message).unwrap();
        let method = command["method"].as_str().unwrap();
        peer.calls.lock().unwrap().push(method.into());
        assert!(command.get("sessionId").is_none());
        let result = match method {
            "Runtime.enable" => {
                if peer.scenario!="background" && !send(&mut socket,json!({"method":"Runtime.executionContextCreated","params":{"context":{"id":1,"uniqueId":"fixture-generation"}}})).await {break;}
                json!({})
            }
            "Log.enable" => {
                send(&mut socket,json!({"method":"Runtime.exceptionThrown","params":{"exceptionDetails":{"text":"Acme error","stackTrace":{"callFrames":[],"parent":{"description":"async","callFrames":[]}}}}})).await;
                json!({})
            }
            "Network.enable" => {
                send(&mut socket,json!({"method":"Network.requestWillBeSent","params":{"requestId":"one","request":{"url":"https://example.com"}}})).await;
                json!({})
            }
            "Debugger.enable" => {
                send(&mut socket,json!({"method":"Debugger.scriptParsed","params":{"scriptId":"1","hash":"bundle-a","sourceMapURL":"acme.map"}})).await;
                json!({})
            }
            "Runtime.evaluate" => {
                assert_eq!(command["params"]["expression"], PROBE);
                assert_eq!(command["params"]["contextId"], 1);
                let app = if peer.scenario == "wrong_app" {
                    "com.example.other"
                } else {
                    "com.example.acme"
                };
                json!({"result":{"type":"string","value":json!({"schemaVersion":1,"hermes":true,"hermesProperties":{"fixture":true},"probe":{"schemaVersion":1,"metadata":{"appId":app,"targetId":"android:acme","runtimeGeneration":"probe-a","buildMode":"development","clientMode":"development-client","bundleHash":"bundle-a","updateId":"update-a"},"events":[{"sequence":1,"kind":"react_commit","name":"Acme","actionId":null,"timestamp":10,"fields":{"actualDuration":2}}],"dropped":0}}).to_string()}})
            }
            "Profiler.start" | "HeapProfiler.startSampling" => {
                if peer.scenario == "hung_start" {
                    continue;
                }
                if peer.scenario == "rejected" {
                    send(&mut socket,json!({"id":command["id"],"error":{"code":-32000,"message":"already active"}})).await;
                    continue;
                }
                if peer.scenario == "reload" {
                    send(
                        &mut socket,
                        json!({"method":"Runtime.executionContextsCleared","params":{}}),
                    )
                    .await;
                }
                if peer.scenario == "disconnect" {
                    break;
                }
                json!({})
            }
            "Profiler.stop" => {
                json!({"profile":{"nodes":[{"id":1,"callFrame":{"functionName":"root"},"children":[2]},{"id":2,"callFrame":{"functionName":"Acme"}}],"samples":[2,2,1],"timeDeltas":[10,10,10],"startTime":1,"endTime":31}})
            }
            "HeapProfiler.stopSampling" => {
                json!({"profile":{"head":{"id":1,"selfSize":0,"callFrame":{"functionName":"root"},"children":[{"id":2,"selfSize":32768,"callFrame":{"functionName":"Acme"},"children":[]}]}}})
            }
            _ => panic!("unregistered inspector effect: {method}"),
        };
        if !send(&mut socket, json!({"id":command["id"],"result":result})).await {
            break;
        }
    }
}
fn options(f: &Fixture, mode: &str) -> Options {
    Options {
        endpoint: f.base.clone(),
        target: "android:acme".into(),
        app: "com.example.acme".into(),
        page: Some("device-1".into()),
        logical: Some("device".into()),
        mode: mode.into(),
        network: true,
        scripts: true,
        probe: true,
        takeover: false,
        profiler: true,
        sample_ms: 100,
    }
}
async fn capture(f: &Fixture, mode: &str) -> (Vec<u8>, Value) {
    let mut capture = Capture::new(options(f, mode)).unwrap();
    let (_sender, stopped) = watch::channel(None);
    capture.control(stopped, Duration::from_secs(12));
    let mut raw = vec![];
    for _ in 0..100 {
        match capture.read().await {
            Ok((packet, complete, _)) => {
                raw.extend(packet);
                if complete {
                    break;
                }
            }
            Err(_) => break,
        }
    }
    (raw, capture.settle().await)
}

#[tokio::test]
async fn diagnostic_react_native_owned_cpu_heap_and_discovery_have_bounded_effects_and_cleanup() {
    for mode in ["cpu", "heap_sampling", "discover"] {
        let f = fixture("normal").await;
        let began = Instant::now();
        let (raw, settled) = capture(&f, mode).await;
        assert!(
            settled["cleanup"]["normalOwnedSettlement"]
                .as_bool()
                .unwrap()
        );
        let calls = f.peer.calls.lock().unwrap().clone();
        if mode == "discover" {
            assert!(calls.is_empty());
            assert_eq!(settled["cleanup"]["profilerStopAcknowledged"], false);
        } else {
            assert_eq!(settled["cleanup"]["profilerStopAcknowledged"], true);
            let stop = if mode == "cpu" {
                "Profiler.stop"
            } else {
                "HeapProfiler.stopSampling"
            };
            assert_eq!(calls.iter().filter(|m| m.as_str() == stop).count(), 1);
            let packets: Vec<Value> = raw
                .split(|b| *b == b'\n')
                .filter(|s| !s.is_empty())
                .map(|s| serde_json::from_slice(s).unwrap())
                .collect();
            assert!(
                packets
                    .iter()
                    .any(|p| p["kind"] == "profile" && !p["data"]["profile"].is_null())
            );
            assert!(
                packets
                    .iter()
                    .all(|p| p["deviceAssociationVerified"] == false)
            );
        }
        eprintln!(
            "RN owned {mode} fixture: {:.2} ms, {} retained bytes, {} commands",
            began.elapsed().as_secs_f64() * 1000.0,
            raw.len(),
            calls.len()
        );
    }
}
#[tokio::test]
async fn diagnostic_react_native_reload_disconnect_scope_mismatch_and_rejected_start_never_stop_borrowed_runtime()
 {
    for scenario in [
        "reload",
        "disconnect",
        "wrong_app",
        "rejected",
        "legacy",
        "missing",
    ] {
        let f = fixture(scenario).await;
        let (_, settled) = capture(&f, "cpu").await;
        let calls = f.peer.calls.lock().unwrap().clone();
        assert!(
            !calls.iter().any(|m| m == "Profiler.stop"),
            "{scenario}: {calls:?}"
        );
        if matches!(scenario, "wrong_app" | "legacy" | "missing") {
            assert!(!calls.iter().any(|m| m == "Profiler.start"));
        }
        if matches!(scenario, "reload" | "disconnect") {
            assert_eq!(settled["profilerMayBeActive"], true);
        }
    }
}
#[tokio::test]
async fn diagnostic_react_native_early_settlement_stops_owned_profiler_once_and_releases_lease() {
    let f = fixture("normal").await;
    let mut c = Capture::new(options(&f, "cpu")).unwrap();
    let (sender, stopped) = watch::channel(None);
    c.control(stopped, Duration::from_secs(12));
    while c.stage < 6 {
        c.read().await.unwrap();
    }
    sender.send(Some("caller stopped".into())).unwrap();
    assert_eq!(c.read().await.unwrap_err().code, "CANCELLED");
    let settled = c.settle().await;
    assert_eq!(settled["cleanup"]["normalOwnedSettlement"], true);
    assert_eq!(settled["cleanup"]["profilerStopAcknowledged"], true);
    c.settle().await;
    assert_eq!(
        f.peer
            .calls
            .lock()
            .unwrap()
            .iter()
            .filter(|m| m.as_str() == "Profiler.stop")
            .count(),
        1
    );
    let second = Capture::new(options(&f, "logs")).unwrap();
    drop(second);
}

#[test]
fn diagnostic_react_native_context_and_fast_refresh_identity_changes_are_fenced() {
    let opts = Options {
        endpoint: "http://127.0.0.1:18081".into(),
        target: "android:acme".into(),
        app: "com.example.acme".into(),
        page: Some("device-1".into()),
        logical: Some("device".into()),
        mode: "cpu".into(),
        network: false,
        scripts: false,
        probe: false,
        takeover: false,
        profiler: false,
        sample_ms: 100,
    };
    assert!(Capture::new(opts).is_err());
    for events in [
        vec![
            (
                "Runtime.executionContextCreated",
                json!({"context":{"id":1,"uniqueId":"a"}}),
            ),
            (
                "Runtime.executionContextCreated",
                json!({"context":{"id":1,"uniqueId":"b"}}),
            ),
        ],
        vec![
            ("Debugger.scriptParsed", json!({"scriptId":"1","hash":"a"})),
            ("Debugger.scriptParsed", json!({"scriptId":"1","hash":"b"})),
        ],
    ] {
        let mut c = Capture::new(Options {
            endpoint: "http://127.0.0.1:18081".into(),
            target: "android:acme".into(),
            app: "com.example.acme".into(),
            page: None,
            logical: None,
            mode: "discover".into(),
            network: false,
            scripts: false,
            probe: false,
            takeover: false,
            profiler: false,
            sample_ms: 100,
        })
        .unwrap();
        for (method, params) in events {
            c.event(EventItem::Event(crate::browser::cdp::Event {
                method: method.into(),
                session_id: None,
                params,
            }));
        }
        assert!(c.changed);
    }
}
#[test]
fn diagnostic_react_native_routes_refuse_implicit_remote_credentialed_or_legacy_sockets() {
    let schema = &crate::diagnostics::definition("diagnostics_start").unwrap()["inputSchema"];
    let mut request = json!({"investigationId":"diag-acme","submissionId":"rn","source":{"kind":"react_native","endpoint":"http://127.0.0.1:18081","targetId":"android:acme","appId":"com.example.acme"}});
    crate::schema::validate(schema, &request).unwrap();
    request["source"]["expression"] = json!("arbitrary()");
    assert!(crate::schema::validate(schema, &request).is_err());
    for endpoint in [
        "http://localhost:8081",
        "http://192.0.2.1:8081",
        "https://127.0.0.1:8081",
        "http://name:secret@127.0.0.1:8081",
        "http://127.0.0.1:8081/private?token=secret",
    ] {
        assert!(discovery::endpoint(endpoint).is_err());
    }
    let base = discovery::endpoint("http://127.0.0.1:18081").unwrap();
    let page = json!({"id":"device-1","appId":"com.example.acme","reactNative":{"logicalDeviceId":"device","capabilities":{"supportsMultipleDebuggers":true}},"webSocketDebuggerUrl":"ws://127.0.0.1:18081/inspector/debug?device=device&page=1"});
    assert_eq!(
        discovery::pages(&base, &json!([page.clone()]), "com.example.acme")
            .unwrap()
            .len(),
        1
    );
    let mut other = page.clone();
    other["webSocketDebuggerUrl"] =
        json!("ws://127.0.0.1:18082/inspector/debug?device=device&page=1");
    assert!(discovery::pages(&base, &json!([other]), "com.example.acme").is_err());
    let mut neighbor = page.clone();
    neighbor["appId"] = json!("com.example.other");
    neighbor["webSocketDebuggerUrl"] = json!("ws://192.0.2.1:8081/foreign");
    assert_eq!(
        discovery::pages(&base, &json!([neighbor, page.clone()]), "com.example.acme")
            .unwrap()
            .len(),
        1
    );
    let mut wrong = page.clone();
    wrong["reactNative"]["logicalDeviceId"] = json!("other-device");
    assert!(discovery::pages(&base, &json!([wrong]), "com.example.acme").is_err());
    let mut legacy = page;
    legacy["webSocketDebuggerUrl"] =
        json!("ws://127.0.0.1:18081/inspector/debug?device=device&page=-1");
    assert!(discovery::pages(&base, &json!([legacy]), "com.example.acme").is_err());
}

#[tokio::test]
async fn diagnostic_react_native_shared_owner_retains_action_binding_and_settles_before_terminal_status()
 {
    let f = fixture("normal").await;
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("state");
    let owner = crate::diagnostics::Owners::new("acme-owner".into());
    let opened=owner.open(&root,"acme",&json!({"submissionId":"rn","question":"Acme runtime","binding":{"targetId":"android:acme","appId":"com.example.acme","actionIds":["action:acme"]}})).unwrap();
    let id = opened["investigationId"].as_str().unwrap();
    let source = json!({"kind":"react_native","endpoint":f.base,"targetId":"android:acme","appId":"com.example.acme","pageId":"device-1","logicalDeviceId":"device","mode":"cpu","probe":true,"allowProfilerControl":true,"sampleMs":100});
    let args = json!({"investigationId":id,"submissionId":"runtime","source":source});
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
    let collector = &status["collectors"][0];
    assert_eq!(collector["settled"], true);
    assert_eq!(
        collector["sourceReport"]["cleanup"]["normalOwnedSettlement"],
        true
    );
    assert_eq!(collector["sourceReport"]["profilerMayBeActive"], false);
    assert!(!status.to_string().contains(&f.base));
    let read = owner
        .inspect(
            &root,
            "acme",
            "diagnostics_read",
            &json!({"investigationId":id,"collectorId":collector["collectorId"],"limit":16384}),
        )
        .unwrap();
    use base64::Engine;
    let raw = base64::engine::general_purpose::STANDARD
        .decode(read["dataBase64"].as_str().unwrap())
        .unwrap();
    let raw = String::from_utf8(raw).unwrap();
    assert!(raw.contains("action:acme"));
    assert!(
        owner
            .start(&root, "acme", &args, &builds)
            .unwrap()
            .1
            .is_none()
    );
    assert_eq!(
        f.peer
            .calls
            .lock()
            .unwrap()
            .iter()
            .filter(|m| m.as_str() == "Profiler.start")
            .count(),
        1
    );
    let mut mismatch = args.clone();
    mismatch["submissionId"] = json!("wrong-device");
    mismatch["source"]["targetId"] = json!("android:other");
    assert!(owner.start(&root, "acme", &mismatch, &builds).is_err());
    let mut limited = args.clone();
    limited["submissionId"] = json!("limited");
    limited["limits"] = json!({"maxEvents":6});
    let (_, work) = owner.start(&root, "acme", &limited, &builds).unwrap();
    owner
        .run(work.unwrap(), root.clone(), "acme".into(), builds)
        .await;
    let status = owner
        .inspect(
            &root,
            "acme",
            "diagnostics_status",
            &json!({"investigationId":id}),
        )
        .unwrap();
    let limited = status["collectors"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["reason"] == "capture_limit")
        .unwrap();
    assert_eq!(limited["phase"], "partial");
    assert_eq!(
        limited["sourceReport"]["cleanup"]["normalOwnedSettlement"],
        true
    );
    assert_eq!(
        f.peer
            .calls
            .lock()
            .unwrap()
            .iter()
            .filter(|m| m.as_str() == "Profiler.stop")
            .count(),
        2
    );
}

#[tokio::test]
async fn diagnostic_react_native_background_and_uncertain_start_have_bounded_distinct_settlement() {
    for scenario in ["background", "hung_start"] {
        let f = fixture(scenario).await;
        let mut c = Capture::new(options(&f, "cpu")).unwrap();
        let (_sender, stopped) = watch::channel(None);
        c.control(stopped, Duration::from_millis(300));
        let began = Instant::now();
        let mut error = String::new();
        for _ in 0..20 {
            match c.read().await {
                Ok((_, true, _)) => break,
                Ok(_) => {}
                Err(e) => {
                    error = e.code;
                    break;
                }
            }
        }
        let settled = c.settle().await;
        assert!(began.elapsed() < Duration::from_secs(5));
        let calls = f.peer.calls.lock().unwrap().clone();
        if scenario == "background" {
            assert_eq!(error, "RN_CONTEXT_UNAVAILABLE");
            assert!(
                !calls.iter().any(|m| m == "Runtime.evaluate"
                    || m == "Profiler.start"
                    || m == "Profiler.stop")
            );
        } else {
            assert_eq!(error, "TIMEOUT");
            assert_eq!(
                calls
                    .iter()
                    .filter(|m| m.as_str() == "Profiler.start")
                    .count(),
                1
            );
            assert_eq!(
                calls
                    .iter()
                    .filter(|m| m.as_str() == "Profiler.stop")
                    .count(),
                1
            );
            assert_eq!(settled["cleanup"]["normalOwnedSettlement"], true);
        }
    }
}
