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
    calls: Arc<Mutex<Vec<String>>>,
    url: Arc<Mutex<String>>,
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
async fn fixture() -> Fixture {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let peer = Peer {
        port,
        calls: Arc::default(),
        url: Arc::new(Mutex::new("https://example.com/acme".into())),
    };
    let router = Router::new()
        .route("/json/list", get(list))
        .route("/devtools/page/acme", get(upgrade))
        .with_state(peer.clone());
    let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    Fixture {
        base: format!("http://127.0.0.1:{port}"),
        peer,
        task,
    }
}
async fn list(State(p): State<Peer>) -> Json<Value> {
    Json(
        json!([{"type":"page","id":"acme","url":*p.url.lock().unwrap(),"webSocketDebuggerUrl":format!("ws://127.0.0.1:{}/devtools/page/acme",p.port)},{"type":"worker","id":"unselected"}]),
    )
}
async fn upgrade(State(p): State<Peer>, ws: WebSocketUpgrade) -> impl IntoResponse {
    ws.on_upgrade(move |socket| serve(socket, p))
}
async fn send(socket: &mut WebSocket, v: Value) {
    socket
        .send(Message::Text(v.to_string().into()))
        .await
        .unwrap();
}
async fn serve(mut socket: WebSocket, p: Peer) {
    while let Some(Ok(Message::Text(text))) = socket.recv().await {
        let c: Value = serde_json::from_str(&text).unwrap();
        let method = c["method"].as_str().unwrap();
        p.calls.lock().unwrap().push(method.into());
        assert!(matches!(
            method,
            "Runtime.enable" | "Page.enable" | "Log.enable" | "Network.enable"
        ));
        send(&mut socket, json!({"id":c["id"],"result":{}})).await;
        if method == "Runtime.enable" {
            send(&mut socket,json!({"method":"Runtime.exceptionThrown","params":{"timestamp":1,"exceptionDetails":{"exceptionId":1,"text":"Acme bridge failed","lineNumber":1,"columnNumber":1}}})).await;
        }
        if method == "Network.enable" {
            send(&mut socket,json!({"method":"Network.requestWillBeSent","params":{"requestId":"bridge-1","loaderId":"loader-1","documentURL":"https://example.com/acme","timestamp":1,"wallTime":1,"initiator":{"type":"other"},"redirectHasExtraInfo":false,"request":{"url":"https://example.com/api","method":"GET","headers":{},"initialPriority":"High","referrerPolicy":"no-referrer"}}})).await;
            send(&mut socket,json!({"method":"Page.frameNavigated","params":{"type":"Navigation","frame":{"id":"one","loaderId":"loader-2","url":"https://example.com/next","domainAndRegistry":"example.com","securityOrigin":"https://example.com","mimeType":"text/html","secureContextType":"Secure","crossOriginIsolatedContextType":"NotIsolated","gatedAPIFeatures":[]}}})).await;
        }
    }
}
fn options(base: &str) -> Options {
    Options {
        endpoint: base.into(),
        target_id: "android:Acme".into(),
        app_id: "com.example.acme".into(),
        mode: "observe".into(),
        page_id: Some("acme".into()),
        expected_url: Some("https://example.com/acme".into()),
        allow_debugger_attachment: true,
        network: true,
        sample_ms: 100,
    }
}
#[test]
fn diagnostic_frameworks_embedded_refuses_implicit_attachment_cross_origin_and_non_cdp_pages() {
    let source:crate::diagnostics::model::Source=serde_json::from_value(json!({"kind":"embedded_cdp","endpoint":"http://127.0.0.1:9222","targetId":"android:Acme","appId":"com.example.acme"})).unwrap();
    assert_eq!(source.kind(), "embedded_cdp");
    assert_eq!(source.format(), "embedded-cdp-v1");
    let mut o = options("http://127.0.0.1:9222");
    o.allow_debugger_attachment = false;
    assert!(Capture::new(o).is_err());
    for endpoint in [
        "http://example.com:9222",
        "http://127.0.0.1",
        "http://127.0.0.1:9222/private",
        "http://user@127.0.0.1:9222",
    ] {
        assert!(Capture::new(options(endpoint)).is_err());
    }
    let base = url::Url::parse("http://127.0.0.1:9222").unwrap();
    for socket in [
        "ws://127.0.0.1:9223/devtools/page/acme",
        "ws://127.0.0.1:9222/devtools/browser/acme",
        "ws://127.0.0.1:9222/devtools/page/acme?token=secret",
    ] {
        assert!(pages(&base,&json!([{"type":"page","id":"acme","url":"https://example.com","webSocketDebuggerUrl":socket}])).is_err());
    }
}
#[tokio::test]
async fn diagnostic_frameworks_embedded_discovery_does_not_attach_and_exact_selection_is_required()
{
    let f = fixture().await;
    let mut o = options(&f.base);
    o.mode = "discover".into();
    o.allow_debugger_attachment = false;
    let mut c = Capture::new(o).unwrap();
    let (raw, done, _) = c.read().await.unwrap();
    assert!(done);
    assert!(f.peer.calls.lock().unwrap().is_empty());
    assert_eq!(
        serde_json::from_slice::<Value>(&raw).unwrap()["data"]["pages"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let mut o = options(&f.base);
    o.expected_url = Some("https://example.com/other".into());
    assert_eq!(
        Capture::new(o).unwrap().read().await.unwrap_err().code,
        "EMBEDDED_TARGET_UNAVAILABLE"
    );
    assert!(f.peer.calls.lock().unwrap().is_empty());
}
#[cfg(unix)]
#[tokio::test]
async fn diagnostic_frameworks_embedded_retains_failure_network_navigation_and_only_closes_owned_socket()
 {
    let f = fixture().await;
    let mut c = Capture::new(options(&f.base)).unwrap();
    c.actions = vec!["action-1".into()];
    let mut all = vec![];
    for _ in 0..3 {
        let (raw, _, _) = c.read().await.unwrap();
        all.extend(raw);
    }
    let i = crate::diagnostics::analysis::decode::decode("embedded-cdp-v1", &all).unwrap();
    assert!(
        i.rows
            .iter()
            .any(|r| r.kind == "failure" && r.name == "Runtime.exceptionThrown")
    );
    assert!(i.rows.iter().any(|r| r.name == "Network.requestWillBeSent"));
    assert!(
        i.rows
            .iter()
            .any(|r| r.name == "Page.frameNavigated" && r.data["generationChanged"] == true)
    );
    let cleanup = c.settle().await;
    assert_eq!(cleanup["ownedSocketClosed"], true);
    assert_eq!(cleanup["borrowedPageClosed"], false);
    assert_eq!(
        f.peer.calls.lock().unwrap().as_slice(),
        [
            "Runtime.enable",
            "Page.enable",
            "Log.enable",
            "Network.enable"
        ]
    );
    let mut again = Capture::new(options(&f.base)).unwrap();
    again.read().await.unwrap();
    again.read().await.unwrap();
    again.read().await.unwrap();
    again.settle().await;
}
#[tokio::test]
async fn diagnostic_frameworks_embedded_rechecks_page_before_attachment_and_preserves_requested_window()
 {
    let f = fixture().await;
    let mut o = options(&f.base);
    o.sample_ms = 10000;
    let mut c = Capture::new(o).unwrap();
    assert_eq!(c.window().unwrap(), Duration::from_secs(10));
    c.read().await.unwrap();
    *f.peer.url.lock().unwrap() = "https://example.com/replaced".into();
    assert_eq!(c.read().await.unwrap_err().code, "EMBEDDED_TARGET_CHANGED");
    assert!(f.peer.calls.lock().unwrap().is_empty());
    let (_stop, receive) = watch::channel(None);
    c.control(receive, Duration::from_secs(1));
    assert!(c.window().unwrap() <= Duration::from_secs(1));
    c.settle().await;
}
#[tokio::test]
async fn diagnostic_frameworks_embedded_cancellation_prevents_any_dispatch() {
    let f = fixture().await;
    let mut c = Capture::new(options(&f.base)).unwrap();
    let (stop, receive) = watch::channel(Some("cancelled".into()));
    c.control(receive, Duration::from_secs(1));
    assert_eq!(c.read().await.unwrap_err().code, "CANCELLED");
    assert!(f.peer.calls.lock().unwrap().is_empty());
    drop(stop);
    assert_eq!(c.settle().await["ownedSocketClosed"], true);
}
