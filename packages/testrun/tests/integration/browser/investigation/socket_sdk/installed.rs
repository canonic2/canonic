//! Installed comparison keeps callback, frame, route and native socket owners distinct.
use super::*;
use futures_util::{SinkExt, StreamExt};
use std::sync::{Arc, Mutex};
mod close_contracts;

const FIRST: &str = "00000000-0000-0000-0000-000000000001";
const SECOND: &str = "00000000-0000-0000-0000-000000000002";
const LATER: &str = "00000000-0000-0000-0000-000000000003";
const NAMES: [&str; 6] = [
    "default",
    "replace",
    "close",
    "mock",
    "close-default",
    "server-only",
];

#[derive(Default)]
struct Wire {
    messages: Vec<Value>,
    closes: Vec<Value>,
    connected: usize,
    active: usize,
    errors: usize,
}
struct Peer {
    url: String,
    wire: Arc<Mutex<Wire>>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Peer {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Peer {
    async fn start(name: &str) -> Self {
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let url = format!("ws://{}/{name}", listener.local_addr().unwrap());
        let wire = Arc::new(Mutex::new(Wire::default()));
        let records = wire.clone();
        let task = tokio::spawn(async move {
            let mut tasks = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    accepted=listener.accept(),if tasks.len()<16=>{
                        let (stream,_)=accepted.unwrap();let wire=records.clone();
                        tasks.spawn(async move {
                            use tokio_tungstenite::tungstenite::Message;
                            let mut socket=tokio_tungstenite::accept_async(stream).await.unwrap();
                            {let mut wire=wire.lock().unwrap();wire.connected+=1;wire.active+=1;}
                            socket.send(Message::Text("Acme-open".into())).await.unwrap();
                            while let Some(message)=socket.next().await {
                                match message {
                                    Ok(Message::Text(text))=>{wire.lock().unwrap().messages.push(json!({"isBinary":false,"data":text.to_string()}));socket.send(Message::Text(text)).await.unwrap();}
                                    Ok(Message::Binary(bytes))=>{wire.lock().unwrap().messages.push(json!({"isBinary":true,"data":bytes.to_vec()}));socket.send(Message::Binary(bytes)).await.unwrap();}
                                    Ok(Message::Close(frame))=>{wire.lock().unwrap().closes.push(frame.map(|frame|json!({"code":u16::from(frame.code),"reason":frame.reason.to_string()})).unwrap_or(Value::Null));let _=socket.flush().await;break;}
                                    Ok(Message::Ping(_)|Message::Pong(_)|Message::Frame(_))=>{},
                                    Err(_)=>{wire.lock().unwrap().errors+=1;break;}
                                }
                            }
                            wire.lock().unwrap().active-=1;
                        });
                    },
                    Some(result)=tasks.join_next(),if !tasks.is_empty()=>{result.unwrap();},
                }
            }
        });
        Self { url, wire, task }
    }
    fn facts(&self) -> Value {
        let wire = self.wire.lock().unwrap();
        json!({"messages":wire.messages,"closes":wire.closes,"connected":wire.connected,"active":wire.active,"errors":wire.errors})
    }
    async fn settled(&self) {
        tokio::time::timeout(Duration::from_secs(10), async {
            while self.wire.lock().unwrap().active != 0 {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("Every selected native WebSocket must release");
        let facts = self.facts();
        assert_eq!(facts["errors"], 0, "{facts}");
    }
}
async fn deliver(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    request: Value,
    style: &str,
) -> Value {
    match style {
        "cli" => {
            let submitted = fixture
                .cli(&["browser", "send", "--target", target, &request.to_string()])
                .await;
            completed(mcp, target, submitted).await
        }
        "act" => {
            let mut action = request;
            action["action"] = action["operation"].clone();
            action.as_object_mut().unwrap().remove("operation");
            let submitted = mcp
                .tool("act", json!({"targetId":target,"actions":[action]}))
                .await;
            completed(mcp, target, submitted).await["results"][0].clone()
        }
        _ => operation(mcp, target, request).await,
    }
}
async fn evaluate(mcp: &mut Mcp, target: &str, page: &Value, expression: &str) -> Value {
    operation(
        mcp,
        target,
        json!({"operation":"evaluate","scope":page,"expression":expression,"isFunction":true}),
    )
    .await
}
async fn observe(mcp: &mut Mcp, target: &str, page: &Value) -> Value {
    let result = evaluate(
        mcp,
        target,
        page,
        "()=>JSON.stringify({ready:AcmeSdkSocket.readyState,events:AcmeSdkEvents})",
    )
    .await;
    serde_json::from_str(result["value"]["value"].as_str().unwrap()).unwrap()
}
async fn wait_events(mcp: &mut Mcp, target: &str, page: &Value, count: usize) -> Value {
    let expression =
        format!("async()=>{{await AcmeSdkWait({count});return JSON.stringify(AcmeSdkEvents);}}");
    let result = evaluate(mcp, target, page, &expression).await;
    serde_json::from_str(result["value"]["value"].as_str().unwrap()).unwrap()
}
fn field(read: &Value, name: &str) -> Value {
    socket_field(&read["arguments"][0], name)["value"].clone()
}
fn payload(read: &Value) -> Value {
    let data = socket_field(&read["arguments"][0], "data");
    json!({"data":socket_field(data,"data")["value"],"isBase64":socket_field(data,"isBase64")["value"]})
}
fn handlers(scope: &Value, route: &Value, read: &Value, value: Value) -> Value {
    json!({"operation":"web_socket_route_handlers","scope":scope,"frame":read["frame"],"routeId":route["routeId"],"socketId":field(read,"id"),"handlers":value})
}
fn close(scope: &Value, route: &Value, read: &Value, to: &str, code: u16, reason: &str) -> Value {
    json!({"operation":"web_socket_route_close","scope":scope,"frame":read["frame"],"routeId":route["routeId"],"socketId":field(read,"id"),"to":to,"options":{"code":code,"reason":reason}})
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
    assert_eq!(status["operation"]["settled"], true, "{status}");
    assert_eq!(status["operation"]["code"], code, "{status}");
}
async fn handled_message(
    mcp: &mut Mcp,
    target: &str,
    route: &Value,
    kind: &str,
    id: &str,
    events: &mut Vec<Value>,
) {
    let (callback, read) = socket_protocols::callback(mcp, target, route, kind).await;
    assert_eq!(field(&read, "listenerId"), id, "{read}");
    let from_page = kind == "onMessageFromPage";
    events.push(json!(if from_page {
        "page-message"
    } else {
        "server-message"
    }));
    socket_protocols::reply(
        mcp,
        target,
        callback,
        json!([{"type":if from_page{"sendToServer"}else{"sendToPage"},"data":payload(&read)}]),
    )
    .await;
}
async fn cell(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    context: &Value,
    http: &str,
    peers: &[Peer],
    reference: &Value,
    engine: &str,
    mode: &str,
) {
    for (index, name) in NAMES.into_iter().enumerate() {
        let peer = &peers[index];
        let page = operation(
            mcp,
            target,
            json!({"operation":"page_create","context":context,"select":false}),
        )
        .await["page"]
            .clone();
        operation(mcp,target,json!({"operation":"page_navigate","page":page,"navigation":{"kind":"goto","url":http,"waitUntil":"load"}})).await;
        let scope = if matches!(name, "replace" | "mock" | "close-default") {
            page.clone()
        } else {
            context.clone()
        };
        let request = if name == "close-default" {
            json!({"operation":"web_socket_route_add_matcher","scope":scope,"matcher":{"kind":"exact","url":peer.url},"sdkDefaults":true})
        } else {
            json!({"operation":"web_socket_route_add","scope":scope,"url":peer.url,"sdkDefaults":true})
        };
        let route = deliver(
            fixture,
            mcp,
            target,
            request,
            if name == "default" {
                "cli"
            } else if name == "close" {
                "act"
            } else {
                "mcp"
            },
        )
        .await;
        assert_eq!(route["sdkDefaults"], true, "{route}");
        let mut sibling = Value::Null;
        let mut sibling_frame = Value::Null;
        let mut other_route = Value::Null;
        let mut legacy_route = Value::Null;
        if name == "replace" {
            sibling = operation(
                mcp,
                target,
                json!({"operation":"page_create","context":context,"select":false}),
            )
            .await["page"]
                .clone();
            sibling_frame = evaluate(mcp, target, &sibling, "()=>true").await["frame"].clone();
            other_route=operation(mcp,target,json!({"operation":"web_socket_route_add","scope":scope,"url":format!("{}?Acme=other",peer.url),"sdkDefaults":true})).await;
            legacy_route=operation(mcp,target,json!({"operation":"web_socket_route_add","scope":scope,"url":format!("{}?Acme=legacy",peer.url),"sdkDefaults":false})).await;
        }
        let expression = format!(
            "()=>new Promise((resolve,reject)=>{{globalThis.AcmeSdkEvents=[];const waits=[];globalThis.AcmeSdkWait=count=>new Promise(resolve=>{{if(AcmeSdkEvents.length>=count)resolve();else waits.push({{count,resolve}});}});const append=event=>{{AcmeSdkEvents.push(event);for(let i=waits.length-1;i>=0;i--)if(AcmeSdkEvents.length>=waits[i].count){{waits[i].resolve();waits.splice(i,1);}}}};const socket=globalThis.AcmeSdkSocket=new WebSocket({});socket.binaryType='arraybuffer';socket.onmessage=event=>append(typeof event.data==='string'?{{type:'text',data:event.data}}:{{type:'binary',data:[...new Uint8Array(event.data)]}});socket.onclose=event=>append({{type:'close',code:event.code,reason:event.reason,wasClean:event.wasClean}});socket.onerror=()=>reject(Error('Acme socket error'));socket.onopen=()=>resolve(true);}})",
            json!(peer.url)
        );
        let opening=mcp.tool("browser_send",json!({"targetId":target,"request":{"operation":"evaluate","scope":page,"expression":expression,"isFunction":true}})).await;
        let (callback, created) = socket_protocols::callback(mcp, target, &route, "onCreate").await;
        assert!(
            created["arguments"][0]["entries"]
                .as_array()
                .unwrap()
                .iter()
                .all(|entry| entry["key"] != "listenerId"),
            "Factory callbacks have no message/close handler identity: {created}"
        );
        if name == "replace" {
            let mut wrong = handlers(&scope, &route, &created, json!({"pageMessage":FIRST}));
            wrong["frame"] = sibling_frame;
            refused(mcp, target, wrong, "INVALID_SCOPE").await;
            let mut wrong = handlers(&scope, &route, &created, json!({"pageMessage":FIRST}));
            wrong["scope"] = sibling.clone();
            refused(mcp, target, wrong, "UNKNOWN_WEBSOCKET_ROUTE").await;
            refused(
                mcp,
                target,
                handlers(&scope, &other_route, &created, json!({"pageMessage":FIRST})),
                "WEBSOCKET_SOCKET_EXPIRED",
            )
            .await;
            refused(
                mcp,
                target,
                handlers(
                    &scope,
                    &legacy_route,
                    &created,
                    json!({"pageMessage":FIRST}),
                ),
                "WEBSOCKET_SDK_MODE_REQUIRED",
            )
            .await;
            let first = deliver(
                fixture,
                mcp,
                target,
                handlers(
                    &scope,
                    &route,
                    &created,
                    json!({"pageMessage":FIRST,"serverMessage":FIRST}),
                ),
                "mcp",
            )
            .await;
            assert_eq!(first["configured"], true);
            let second = deliver(
                fixture,
                mcp,
                target,
                handlers(
                    &scope,
                    &route,
                    &created,
                    json!({"pageMessage":SECOND,"serverMessage":SECOND}),
                ),
                "act",
            )
            .await;
            assert_eq!(second["handlerChanges"], 2);
        }
        if name == "close" || name == "server-only" {
            deliver(
                fixture,
                mcp,
                target,
                handlers(
                    &scope,
                    &route,
                    &created,
                    if name == "close" {
                        json!({"pageClose":FIRST,"serverClose":FIRST})
                    } else {
                        json!({"serverClose":FIRST})
                    },
                ),
                "mcp",
            )
            .await;
            deliver(
                fixture,
                mcp,
                target,
                handlers(
                    &scope,
                    &route,
                    &created,
                    if name == "close" {
                        json!({"pageClose":SECOND,"serverClose":SECOND})
                    } else {
                        json!({"serverClose":SECOND})
                    },
                ),
                "cli",
            )
            .await;
        }
        socket_protocols::reply(
            mcp,
            target,
            callback,
            if name == "mock" {
                Value::Null
            } else {
                json!([{"type":"connect"}])
            },
        )
        .await;
        assert_eq!(
            completed(mcp, target, opening).await["value"]["value"],
            true
        );
        let mut events = vec![];
        if name == "replace" {
            handled_message(
                mcp,
                target,
                &route,
                "onMessageFromServer",
                SECOND,
                &mut events,
            )
            .await;
        }
        if name != "mock" {
            wait_events(mcp, target, &page, 1).await;
        }
        let mut held_server_open = false;
        let mut held_page_closing = false;
        let mut ready_after_discarded_send = Value::Null;
        if name == "server-only" {
            deliver(
                fixture,
                mcp,
                target,
                close(&scope, &route, &created, "server", 3002, "Acme server"),
                "act",
            )
            .await;
            let (callback, read) =
                socket_protocols::callback(mcp, target, &route, "onCloseServer").await;
            assert_eq!(field(&read, "listenerId"), SECOND);
            events.push(json!({"event":"server-close","code":field(&read,"code"),"reason":field(&read,"reason")}));
            socket_protocols::reply(mcp, target, callback, Value::Null).await;
            ready_after_discarded_send=evaluate(mcp,target,&page,"()=>{AcmeSdkSocket.send('Acme after server close');return AcmeSdkSocket.readyState;}").await["value"]["value"].clone();
            assert_eq!(ready_after_discarded_send, 1);
            operation(mcp,target,json!({"operation":"web_socket_route_send","scope":scope,"frame":created["frame"],"routeId":route["routeId"],"socketId":field(&created,"id"),"to":"page","message":{"data":"Acme survivor","isBase64":false}})).await;
            wait_events(mcp, target, &page, 2).await;
            deliver(
                fixture,
                mcp,
                target,
                close(&scope, &route, &created, "page", 3003, "Acme final"),
                "cli",
            )
            .await;
        } else if name == "close" {
            evaluate(
                mcp,
                target,
                &page,
                "()=>AcmeSdkSocket.close(3001,'Acme page')",
            )
            .await;
            let (callback, read) =
                socket_protocols::callback(mcp, target, &route, "onClosePage").await;
            assert_eq!(field(&read, "listenerId"), SECOND);
            events.push(json!({"event":"page-close","code":field(&read,"code"),"reason":field(&read,"reason")}));
            socket_protocols::reply(mcp, target, callback, Value::Null).await;
            held_server_open = peer.facts()["active"] == 1 && peer.facts()["closes"] == json!([]);
            assert!(held_server_open, "{}", peer.facts());
            assert_eq!(observe(mcp, target, &page).await["ready"], 2);
            let requested = deliver(
                fixture,
                mcp,
                target,
                close(&scope, &route, &created, "server", 3002, "Acme server"),
                "act",
            )
            .await;
            assert_eq!(requested["closeRequested"], true);
            assert!(requested.get("closed").is_none());
            let (callback, read) =
                socket_protocols::callback(mcp, target, &route, "onCloseServer").await;
            assert_eq!(field(&read, "listenerId"), SECOND);
            events.push(json!({"event":"server-close","code":field(&read,"code"),"reason":field(&read,"reason")}));
            socket_protocols::reply(mcp, target, callback, Value::Null).await;
            held_page_closing = observe(mcp, target, &page).await["ready"] == 2;
            assert!(held_page_closing);
            deliver(
                fixture,
                mcp,
                target,
                close(&scope, &route, &created, "page", 3003, "Acme final"),
                "cli",
            )
            .await;
            let (callback, read) =
                socket_protocols::callback(mcp, target, &route, "onClosePage").await;
            assert_eq!(field(&read, "listenerId"), SECOND);
            events.push(json!({"event":"page-close","code":field(&read,"code"),"reason":field(&read,"reason")}));
            socket_protocols::reply(mcp, target, callback, Value::Null).await;
        } else {
            if name == "replace" {
                evaluate(mcp, target, &page, "()=>AcmeSdkSocket.send('Acme-first')").await;
                let (callback, read) =
                    socket_protocols::callback(mcp, target, &route, "onMessageFromPage").await;
                assert_eq!(field(&read, "listenerId"), SECOND);
                deliver(
                    fixture,
                    mcp,
                    target,
                    handlers(&scope, &route, &created, json!({"pageMessage":LATER})),
                    "cli",
                )
                .await;
                let retained = operation(
                    mcp,
                    target,
                    json!({"operation":"binding_read","callback":callback}),
                )
                .await;
                assert_eq!(
                    field(&retained, "listenerId"),
                    SECOND,
                    "Pending delivery changed owner: {retained}"
                );
                events.push(json!("page-message"));
                socket_protocols::reply(
                    mcp,
                    target,
                    callback,
                    json!([{"type":"sendToServer","data":payload(&read)}]),
                )
                .await;
                handled_message(
                    mcp,
                    target,
                    &route,
                    "onMessageFromServer",
                    SECOND,
                    &mut events,
                )
                .await;
                wait_events(mcp, target, &page, 2).await;
                evaluate(
                    mcp,
                    target,
                    &page,
                    "()=>AcmeSdkSocket.send(new Uint8Array([0,17,255]))",
                )
                .await;
                handled_message(mcp, target, &route, "onMessageFromPage", LATER, &mut events).await;
                handled_message(
                    mcp,
                    target,
                    &route,
                    "onMessageFromServer",
                    SECOND,
                    &mut events,
                )
                .await;
                wait_events(mcp, target, &page, 3).await;
                deliver(
                    fixture,
                    mcp,
                    target,
                    handlers(
                        &scope,
                        &route,
                        &created,
                        json!({"pageMessage":null,"serverMessage":null}),
                    ),
                    "act",
                )
                .await;
                evaluate(
                    mcp,
                    target,
                    &page,
                    "()=>AcmeSdkSocket.send('Acme-default-restored')",
                )
                .await;
                wait_events(mcp, target, &page, 4).await;
            } else if name != "mock" {
                evaluate(mcp,target,&page,"()=>{AcmeSdkSocket.send('Acme-first');AcmeSdkSocket.send(new Uint8Array([0,17,255]));}").await;
                wait_events(mcp, target, &page, 3).await;
            } else {
                evaluate(
                    mcp,
                    target,
                    &page,
                    "()=>AcmeSdkSocket.send('Acme-discarded')",
                )
                .await;
            }
            if name == "close-default" {
                deliver(
                    fixture,
                    mcp,
                    target,
                    close(&scope, &route, &created, "page", 3004, "Acme route"),
                    "mcp",
                )
                .await;
            } else {
                evaluate(
                    mcp,
                    target,
                    &page,
                    "()=>AcmeSdkSocket.close(3001,'Acme page')",
                )
                .await;
            }
        }
        let page_events = wait_events(
            mcp,
            target,
            &page,
            if name == "mock" {
                1
            } else if name == "close" {
                2
            } else if name == "replace" {
                5
            } else if name == "server-only" {
                3
            } else {
                4
            },
        )
        .await;
        assert_eq!(observe(mcp, target, &page).await["ready"], 3);
        peer.settled().await;
        let wire = peer.facts();
        let row = if name == "server-only" {
            json!({"name":name,"readyAfterDiscardedSend":ready_after_discarded_send,"events":events,"page":page_events,"server":wire["messages"],"serverClosed":wire["closes"][0],"connected":wire["connected"]==1})
        } else if name == "close" {
            json!({"name":name,"heldServerOpen":held_server_open,"heldPageClosing":held_page_closing,"events":events,"page":page_events,"serverClosed":wire["closes"][0]})
        } else {
            json!({"name":name,"events":events,"page":page_events,"server":wire["messages"],"serverClosed":wire["closes"].as_array().unwrap().first().cloned().unwrap_or(Value::Null),"connected":wire["connected"]==1})
        };
        std::fs::write(
            fixture
                .workspace
                .path()
                .join(format!("socket-sdk-installed-{engine}-{mode}-{name}.json")),
            serde_json::to_vec_pretty(&json!({"row":row,"wire":wire})).unwrap(),
        )
        .unwrap();
        let expected = reference["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["name"] == name)
            .unwrap();
        if name == "replace" {
            for direction in ["page-message", "server-message"] {
                let select = |row: &Value| {
                    row["events"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .filter(|event| **event == direction)
                        .cloned()
                        .collect::<Vec<_>>()
                };
                assert_eq!(select(&row), select(expected), "{row}");
            }
            let mut actual = row.clone();
            let mut expected = expected.clone();
            actual.as_object_mut().unwrap().remove("events");
            expected.as_object_mut().unwrap().remove("events");
            assert_eq!(actual, expected, "{row}");
        } else {
            assert_eq!(&row, expected, "{row}");
        }
        refused(
            mcp,
            target,
            handlers(&scope, &route, &created, json!({"pageMessage":FIRST})),
            "WEBSOCKET_SOCKET_EXPIRED",
        )
        .await;
        refused(
            mcp,
            target,
            close(&scope, &route, &created, "page", 3001, "Acme stale"),
            "WEBSOCKET_SOCKET_EXPIRED",
        )
        .await;
        for route in [&route, &other_route, &legacy_route]
            .into_iter()
            .filter(|route| !route.is_null())
        {
            operation(mcp,target,json!({"operation":"web_socket_route_remove","scope":scope,"routeId":route["routeId"]})).await;
        }
        if !sibling.is_null() {
            operation(
                mcp,
                target,
                json!({"operation":"page_close","page":sibling}),
            )
            .await;
        }
        operation(mcp, target, json!({"operation":"page_close","page":page})).await;
    }
}

#[tokio::test]
async fn installed_step_four_socket_sdk_checkpoint() {
    let fixture = installed_fixture();
    let mut mcp = Mcp::start(&fixture).await;
    let mut failures = vec![];
    for mode in ["headless", "headed"] {
        for engine in ["chrome", "firefox", "webkit"] {
            let reference = reference(&fixture, engine, mode).await;
            let site = Site::start().await;
            let mut peers = vec![];
            for name in NAMES {
                peers.push(Peer::start(name).await);
            }
            let connected = fixture
                .cli(&["browser", "connect", "--engine", engine, "--mode", mode])
                .await;
            let target = connected["targetId"].as_str().unwrap();
            let ready = fixture.ready_target(target).await;
            let result = std::panic::AssertUnwindSafe(cell(
                &fixture,
                &mut mcp,
                target,
                &ready["browser"]["selectedContext"],
                &site.url,
                &peers,
                &reference,
                engine,
                mode,
            ))
            .catch_unwind()
            .await;
            if result.is_err() {
                let status = mcp.tool("browser_status", json!({"targetId":target})).await;
                std::fs::write(fixture.workspace.path().join(format!("socket-sdk-installed-{engine}-{mode}-failure.json")),serde_json::to_vec_pretty(&json!({"status":status,"peers":peers.iter().map(Peer::facts).collect::<Vec<_>>()})).unwrap()).unwrap();
            }
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
                failures.push(format!("{engine}/{mode}: {closed}"));
            }
            if let Err(panic) = result {
                mcp.stop().await;
                std::panic::resume_unwind(panic);
            }
            for peer in &peers {
                peer.settled().await;
            }
            eprintln!(
                "Installed SDK sockets {engine}/{mode}: six pinned workflows, exact close acknowledgement, surviving endpoint, handler snapshots and ownership refusals"
            );
        }
    }
    mcp.stop().await;
    assert!(
        failures.is_empty(),
        "Original cleanup failures: {failures:?}"
    );
}
