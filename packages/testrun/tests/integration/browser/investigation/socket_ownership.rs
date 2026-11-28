//! Close-code fidelity and independent WebSocket endpoint ownership on the installed binary.
use super::*;
use futures_util::{SinkExt, StreamExt};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tokio::sync::{mpsc, oneshot};

struct Peer {
    url: String,
    connections: Arc<AtomicUsize>,
    closes: mpsc::Receiver<(Value, oneshot::Sender<()>)>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Peer {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Peer {
    async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let url = format!("ws://{}/Acme-close", listener.local_addr().unwrap());
        let connections = Arc::new(AtomicUsize::new(0));
        let count = connections.clone();
        let (sender, closes) = mpsc::channel(16);
        let task = tokio::spawn(async move {
            let mut tasks = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    accepted=listener.accept()=>{
                        let (stream,_) = accepted.unwrap(); let sender=sender.clone(); let count=count.clone();
                        tasks.spawn(async move {
                            let mut socket=tokio_tungstenite::accept_async(stream).await.unwrap();
                            count.fetch_add(1,Ordering::SeqCst);
                            let message=socket.next().await.unwrap().unwrap();
                            let tokio_tungstenite::tungstenite::Message::Close(frame)=message else { panic!("Unexpected message before owned close: {message}") };
                            let frame=frame.map(|frame|json!({"code":u16::from(frame.code),"reason":frame.reason.to_string()})).unwrap_or(Value::Null);
                            let (ack,release)=oneshot::channel();
                            sender.send((frame,ack)).await.unwrap();
                            // Tungstenite queues its reply while reading the frame. Hold
                            // flush so the test can observe CLOSING before acknowledgement.
                            let _=release.await;
                            let _=socket.flush().await;
                        });
                    }
                    completed=tasks.join_next(),if !tasks.is_empty()=>{ completed.unwrap().unwrap(); }
                }
            }
        });
        Self {
            url,
            connections,
            closes,
            task,
        }
    }
    async fn closing(&mut self) -> (Value, oneshot::Sender<()>) {
        tokio::time::timeout(Duration::from_secs(10), self.closes.recv())
            .await
            .unwrap()
            .unwrap()
    }
}
async fn evaluate(mcp: &mut Mcp, target: &str, page: &Value, source: &str) -> Value {
    operation(
        mcp,
        target,
        json!({"operation":"evaluate","scope":page,"expression":source,"isFunction":true}),
    )
    .await["value"]["value"]
        .clone()
}
async fn observe(mcp: &mut Mcp, target: &str, page: &Value) -> Value {
    let value=evaluate(mcp,target,page,"()=>JSON.stringify({ready:AcmeOwnedSocket.readyState,opened:AcmeOwnedOpened,messages:AcmeOwnedMessages,closed:AcmeOwnedClose,nativeCloses:globalThis.AcmeOwnedNativeCloses??[]})").await;
    serde_json::from_str(value.as_str().unwrap()).unwrap()
}
fn send(scope: &Value, route: &Value, read: &Value, to: &str, data: &str) -> Value {
    json!({"operation":"web_socket_route_send","scope":scope,"routeId":route["routeId"],"frame":read["frame"],"socketId":socket_field(&read["arguments"][0],"id")["value"],"to":to,"message":{"data":data,"isBase64":false}})
}
async fn refused(mcp: &mut Mcp, target: &str, request: Value, code: &str) {
    let submitted = mcp
        .tool("browser_send", json!({"targetId":target,"request":request}))
        .await;
    let status = mcp
        .tool(
            "browser_status",
            json!({"targetId":target,"operationId":submitted["operation"]["id"],"waitMs":30000}),
        )
        .await;
    assert_eq!(status["operation"]["code"], code, "{status}");
    assert!(!status.to_string().contains("Acme forbidden"));
}
async fn commands(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    callback: Value,
    commands: Value,
    cli: bool,
) -> f64 {
    let start = std::time::Instant::now();
    if cli {
        let request=json!({"operation":"callback_resolve","callback":callback,"response":{"kind":"binding_resolve","value":commands}}).to_string();
        let submitted = fixture
            .cli(&["browser", "send", "--target", target, &request])
            .await;
        completed(mcp, target, submitted).await;
    } else {
        socket_protocols::reply(mcp, target, callback, commands).await;
    }
    start.elapsed().as_secs_f64() * 1000.0
}
#[expect(
    clippy::too_many_lines,
    reason = "One endpoint workflow correlates private callback ownership with held peer close acknowledgements before cleanup"
)]
async fn cell(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    context: &Value,
    page: &Value,
    peer: &mut Peer,
) -> Value {
    let route = operation(
        mcp,
        target,
        json!({"operation":"web_socket_route_add","scope":context,"url":peer.url,"sdkDefaults":false}),
    )
    .await;
    let initial_connections = peer.connections.load(Ordering::SeqCst);
    let mut fresh_ms = 0.0;
    let mut retained_start = None;
    for mode in [
        "mock-omitted",
        "server-omitted",
        "server-explicit",
        "server-reason",
        "duplicate-connect",
    ] {
        let source = format!(
            "()=>{{globalThis.AcmeOwnedOpened=0;globalThis.AcmeOwnedMessages=[];globalThis.AcmeOwnedClose=null;const socket=globalThis.AcmeOwnedSocket=new WebSocket({});globalThis.AcmeOwnedOpenPromise=new Promise(resolve=>socket.onopen=()=>{{AcmeOwnedOpened++;resolve(true);}});globalThis.AcmeOwnedClosePromise=new Promise(resolve=>socket.onclose=event=>{{AcmeOwnedClose={{code:event.code,reason:event.reason,wasClean:event.wasClean}};resolve(true);}});socket.onmessage=event=>AcmeOwnedMessages.push(event.data);return socket.readyState;}}",
            json!(peer.url)
        );
        assert_eq!(evaluate(mcp, target, page, &source).await, 0);
        let (callback, read) = socket_protocols::callback(mcp, target, &route, "onCreate").await;
        let duplicate = mode == "duplicate-connect";
        let reply = if duplicate {
            json!([{"type":"sendToPage","data":{"data":"Acme leaked","isBase64":false}},{"type":"connect"},{"type":"connect"}])
        } else {
            json!([{"type":if mode=="mock-omitted"{"ensureOpened"}else{"connect"}}])
        };
        let elapsed = commands(
            fixture,
            mcp,
            target,
            callback,
            reply,
            mode == "mock-omitted",
        )
        .await;
        if mode == "mock-omitted" {
            fresh_ms = elapsed;
            retained_start = Some(std::time::Instant::now());
        }
        if duplicate {
            assert_eq!(
                evaluate(mcp, target, page, "async()=>await AcmeOwnedClosePromise").await,
                true
            );
            let state = observe(mcp, target, page).await;
            assert_eq!(state["opened"], 0, "{state}");
            assert_eq!(state["closed"]["code"], 3001, "{state}");
            assert_eq!(state["messages"], json!([]));
            refused(
                mcp,
                target,
                send(context, &route, &read, "page", "Acme forbidden"),
                "WEBSOCKET_SOCKET_EXPIRED",
            )
            .await;
            continue;
        }
        assert_eq!(
            evaluate(mcp, target, page, "async()=>await AcmeOwnedOpenPromise").await,
            true
        );
        if mode == "mock-omitted" {
            refused(
                mcp,
                target,
                send(context, &route, &read, "server", "Acme forbidden"),
                "WEBSOCKET_SERVER_NOT_CONNECTED",
            )
            .await;
        }
        evaluate(
            mcp,
            target,
            page,
            "()=>{AcmeOwnedSocket.send('Acme close');return true;}",
        )
        .await;
        let (callback, _) =
            socket_protocols::callback(mcp, target, &route, "onMessageFromPage").await;
        if mode == "mock-omitted" {
            commands(
                fixture,
                mcp,
                target,
                callback,
                json!([{"type":"closePage"}]),
                false,
            )
            .await;
            assert_eq!(
                evaluate(mcp, target, page, "async()=>await AcmeOwnedClosePromise").await,
                true
            );
            let state = observe(mcp, target, page).await;
            assert_eq!(state["closed"]["code"], 0, "{state}");
            assert_eq!(state["ready"], 3);
        } else {
            evaluate(mcp,target,page,"()=>{globalThis.AcmeOwnedNativeCloses=[];const socket=AcmeOwnedSocket._ws,close=socket.close;socket.close=function(...args){const entry={argumentCount:args.length,code:args[0]===undefined?'undefined':args[0],reason:args[1]===undefined?'undefined':args[1]};AcmeOwnedNativeCloses.push(entry);try{return Reflect.apply(close,this,args);}catch(error){entry.error={name:error.name,message:error.message};throw error;}};return true;}").await;
            let close = if mode == "server-explicit" {
                json!({"type":"closeServer","code":1000})
            } else if mode == "server-reason" {
                json!({"type":"closeServer","reason":"Acme reason only"})
            } else {
                json!({"type":"closeServer"})
            };
            commands(
                fixture,
                mcp,
                target,
                callback,
                json!([close.clone(), close]),
                false,
            )
            .await;
            let (frame, ack) = peer.closing().await;
            let state = observe(mcp, target, page).await;
            std::fs::write(
                fixture
                    .workspace
                    .path()
                    .join(format!("socket-close-{target}-{mode}.json")),
                serde_json::to_vec_pretty(&json!({"frame":frame,"state":state})).unwrap(),
            )
            .unwrap();
            if mode == "server-omitted" {
                assert!(frame.is_null(), "{frame}");
                assert_eq!(state["nativeCloses"][0]["argumentCount"], 0, "{state}");
            } else if mode == "server-reason" {
                assert_eq!(frame, json!({"code":1000,"reason":"Acme reason only"}));
            } else {
                assert_eq!(frame, json!({"code":1000,"reason":""}));
            }
            assert_eq!(
                state["nativeCloses"].as_array().unwrap().len(),
                1,
                "{state}"
            );
            assert!(state["nativeCloses"][0].get("error").is_none(), "{state}");
            refused(
                mcp,
                target,
                send(context, &route, &read, "server", "Acme forbidden"),
                "WEBSOCKET_SOCKET_CLOSED",
            )
            .await;
            assert_eq!(observe(mcp, target, page).await["ready"], 1);
            operation(
                mcp,
                target,
                send(context, &route, &read, "page", "Acme survivor"),
            )
            .await;
            assert_eq!(
                observe(mcp, target, page).await["messages"],
                json!(["Acme survivor"])
            );
            ack.send(()).unwrap();
            let (callback, event) =
                socket_protocols::callback(mcp, target, &route, "onCloseServer").await;
            assert_eq!(
                socket_field(&event["arguments"][0], "code")["value"],
                if mode == "server-omitted" { 1005 } else { 1000 }
            );
            refused(
                mcp,
                target,
                send(context, &route, &read, "server", "Acme forbidden"),
                "WEBSOCKET_SOCKET_CLOSED",
            )
            .await;
            assert_eq!(observe(mcp, target, page).await["ready"], 1);
            let reply = if mode == "server-omitted" {
                json!([{"type":"closePage"}])
            } else if mode == "server-reason" {
                json!([{"type":"closePage","reason":"Acme reason only"}])
            } else {
                json!([{"type":"sendToPage","data":{"data":"Acme leaked","isBase64":false}},{"type":"connect"}])
            };
            commands(fixture, mcp, target, callback, reply, false).await;
            assert_eq!(
                evaluate(mcp, target, page, "async()=>await AcmeOwnedClosePromise").await,
                true
            );
            let state = observe(mcp, target, page).await;
            assert_eq!(
                state["closed"]["code"],
                if mode == "server-explicit" { 3001 } else { 0 },
                "{state}"
            );
            assert_eq!(state["messages"], json!(["Acme survivor"]));
            if mode == "server-reason" {
                assert_eq!(state["closed"]["reason"], "Acme reason only");
            }
        }
        refused(
            mcp,
            target,
            send(context, &route, &read, "page", "Acme forbidden"),
            "WEBSOCKET_SOCKET_EXPIRED",
        )
        .await;
    }
    assert_eq!(
        peer.connections.load(Ordering::SeqCst),
        initial_connections + 3
    );
    operation(
        mcp,
        target,
        json!({"operation":"web_socket_route_remove","scope":context,"routeId":route["routeId"]}),
    )
    .await;
    let listed = operation(
        mcp,
        target,
        json!({"operation":"web_socket_route_list","scope":context}),
    )
    .await;
    assert_eq!(listed["routes"], json!([]));
    json!({"freshCliCreationReplyMs":fresh_ms,"retainedWorkflowWithWireInspectionMs":retained_start.expect("Fresh CLI creation reply observed").elapsed().as_secs_f64()*1000.0,"nativeConnections":3})
}
#[tokio::test]
async fn installed_step_four_socket_ownership_checkpoint() {
    let fixture = installed_fixture();
    let mut peer = Peer::start().await;
    let mut mcp = Mcp::start(&fixture).await;
    let mut cleanup_failures = vec![];
    for mode in ["headless", "headed"] {
        for engine in ["chrome", "firefox", "webkit"] {
            let started = std::time::Instant::now();
            let connected = fixture
                .cli(&["browser", "connect", "--engine", engine, "--mode", mode])
                .await;
            let target = connected["targetId"].as_str().unwrap();
            let ready = fixture.ready_target(target).await;
            let connect_ms = started.elapsed().as_secs_f64() * 1000.0;
            let result = std::panic::AssertUnwindSafe(cell(
                &fixture,
                &mut mcp,
                target,
                &ready["browser"]["selectedContext"],
                &ready["browser"]["selectedPage"],
                &mut peer,
            ))
            .catch_unwind()
            .await;
            let started = std::time::Instant::now();
            let closed = performance_lifecycle::disconnect_observed(
                &fixture,
                &mut mcp,
                target,
                engine,
                mode,
                &ready["browser"]["selectedContext"],
            )
            .await;
            if closed["result"]["structuredContent"]["ok"] != true {
                cleanup_failures.push(format!("{engine}/{mode}: {closed}"));
            }
            let cleanup_ms = started.elapsed().as_secs_f64() * 1000.0;
            match result {
                Ok(metrics) => eprintln!(
                    "Socket ownership {engine} {mode}: {metrics}; connectMs={connect_ms}; cleanupMs={cleanup_ms}"
                ),
                Err(panic) => {
                    fixture.cli(&["disconnect"]).await;
                    mcp.tool("disconnect", json!({})).await;
                    mcp.stop().await;
                    std::panic::resume_unwind(panic);
                }
            }
        }
    }
    fixture.cli(&["disconnect"]).await;
    mcp.tool("disconnect", json!({})).await;
    mcp.stop().await;
    assert!(
        cleanup_failures.is_empty(),
        "Original cleanup failures: {cleanup_failures:?}"
    );
}
