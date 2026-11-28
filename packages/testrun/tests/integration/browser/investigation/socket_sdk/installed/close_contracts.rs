//! Discovery, direct close defaults and removal of arrival-owned close handlers.
use super::*;
use crate::browser::investigation::socket_close_contracts::reference;
const CASES: [&str; 11] = [
    "page-default",
    "page-empty",
    "page-empty-reason",
    "page-reason",
    "server-default",
    "server-empty",
    "server-empty-reason",
    "server-reason",
    "remove-page",
    "remove-server",
    "pending-server-removal",
];

fn request(scope: &Value, route: &Value, read: &Value, to: &str, options: Option<Value>) -> Value {
    let mut value = json!({"operation":"web_socket_route_close","scope":scope,"routeId":route["routeId"],"frame":read["frame"],"socketId":field(read,"id"),"to":to});
    if let Some(options) = options {
        value["options"] = options;
    }
    value
}
async fn native(mcp: &mut Mcp, target: &str, page: &Value) -> Value {
    let value = evaluate(mcp, target, page, "()=>JSON.stringify(AcmeNativeCloses)").await;
    serde_json::from_str(value["value"]["value"].as_str().unwrap()).unwrap()
}
async fn cell(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    context: &Value,
    http: &str,
    peers: &[Peer],
    baseline: &Value,
    engine: &str,
    mode: &str,
) {
    for (index, name) in CASES.into_iter().enumerate() {
        let peer = &peers[index];
        let page = operation(
            mcp,
            target,
            json!({"operation":"page_create","context":context,"select":false}),
        )
        .await["page"]
            .clone();
        operation(mcp,target,json!({"operation":"page_navigate","page":page,"navigation":{"kind":"goto","url":http,"waitUntil":"load"}})).await;
        evaluate(mcp,target,&page,"()=>{const Native=WebSocket;globalThis.AcmeNativeCloses=[];globalThis.WebSocket=class extends Native{constructor(...args){super(...args);globalThis.AcmeNativeClosed=new Promise(resolve=>this.addEventListener('close',()=>setTimeout(resolve,0),{once:true}));}close(...args){const call={arguments:args.map(value=>value===undefined?{kind:'undefined'}:{kind:typeof value,value}),error:null};AcmeNativeCloses.push(call);try{return super.close(...args);}catch(error){call.error=error.name;throw error;}}};return true;}").await;
        let scope = if index % 2 == 0 {
            context.clone()
        } else {
            page.clone()
        };
        let route=deliver(fixture,mcp,target,json!({"operation":"web_socket_route_add","scope":scope,"url":peer.url,"sdkDefaults":true}),if index%2==0{"cli"}else{"mcp"}).await;
        let expression = format!(
            "()=>new Promise((resolve,reject)=>{{globalThis.AcmeSdkEvents=[];const waits=[];globalThis.AcmeSdkWait=count=>new Promise(resolve=>{{if(AcmeSdkEvents.length>=count)resolve();else waits.push({{count,resolve}});}});const append=event=>{{AcmeSdkEvents.push(event);for(let i=waits.length-1;i>=0;i--)if(AcmeSdkEvents.length>=waits[i].count){{waits[i].resolve();waits.splice(i,1);}}}};const socket=globalThis.AcmeSdkSocket=new WebSocket({});socket.onmessage=event=>append({{type:'text',data:event.data}});socket.onclose=event=>append({{type:'close',code:event.code,reason:event.reason,wasClean:event.wasClean}});socket.onerror=()=>reject(Error('Acme socket error'));socket.onopen=()=>resolve(true);}})",
            json!(peer.url)
        );
        let opening=mcp.tool("browser_send",json!({"targetId":target,"request":{"operation":"evaluate","scope":page,"expression":expression,"isFunction":true}})).await;
        let (callback, created) = socket_protocols::callback(mcp, target, &route, "onCreate").await;
        if name.starts_with("remove-") {
            deliver(
                fixture,
                mcp,
                target,
                handlers(
                    &scope,
                    &route,
                    &created,
                    json!({"pageClose":FIRST,"serverClose":SECOND}),
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
                    if name == "remove-page" {
                        json!({"pageClose":null})
                    } else {
                        json!({"serverClose":null})
                    },
                ),
                "act",
            )
            .await;
        } else if name == "pending-server-removal" {
            deliver(
                fixture,
                mcp,
                target,
                handlers(&scope, &route, &created, json!({"serverClose":FIRST})),
                "cli",
            )
            .await;
        }
        socket_protocols::reply(mcp, target, callback, json!([{"type":"connect"}])).await;
        completed(mcp, target, opening).await;
        wait_events(mcp, target, &page, 1).await;
        let mut events = vec![];
        let mut held = Value::Null;
        let mut after = Value::Null;
        if name == "remove-page" {
            evaluate(
                mcp,
                target,
                &page,
                "()=>AcmeSdkSocket.close(3001,'Acme page')",
            )
            .await;
            let (callback, read) =
                socket_protocols::callback(mcp, target, &route, "onCloseServer").await;
            assert_eq!(field(&read, "listenerId"), SECOND);
            events.push(json!({"event":"server-close","code":field(&read,"code"),"reason":field(&read,"reason")}));
            socket_protocols::reply(mcp, target, callback, Value::Null).await;
            held = observe(mcp, target, &page).await["ready"].clone();
            assert_eq!(held, 2);
            deliver(
                fixture,
                mcp,
                target,
                request(
                    &scope,
                    &route,
                    &created,
                    "page",
                    Some(json!({"code":3002,"reason":"Acme final"})),
                ),
                "cli",
            )
            .await;
        } else if name == "remove-server" {
            deliver(
                fixture,
                mcp,
                target,
                request(
                    &scope,
                    &route,
                    &created,
                    "server",
                    Some(json!({"code":3001,"reason":"Acme server"})),
                ),
                "act",
            )
            .await;
            let (callback, read) =
                socket_protocols::callback(mcp, target, &route, "onClosePage").await;
            assert_eq!(field(&read, "listenerId"), FIRST);
            events.push(json!({"event":"page-close","code":field(&read,"code"),"reason":field(&read,"reason")}));
            socket_protocols::reply(mcp, target, callback, Value::Null).await;
        } else if name == "pending-server-removal" {
            deliver(
                fixture,
                mcp,
                target,
                request(
                    &scope,
                    &route,
                    &created,
                    "server",
                    Some(json!({"code":3001,"reason":"Acme server"})),
                ),
                "mcp",
            )
            .await;
            let (callback, read) =
                socket_protocols::callback(mcp, target, &route, "onCloseServer").await;
            assert_eq!(field(&read, "listenerId"), FIRST);
            events.push(json!({"event":"server-close","code":field(&read,"code"),"reason":field(&read,"reason")}));
            deliver(
                fixture,
                mcp,
                target,
                handlers(&scope, &route, &created, json!({"serverClose":null})),
                "act",
            )
            .await;
            let reread = operation(
                mcp,
                target,
                json!({"operation":"binding_read","callback":callback}),
            )
            .await;
            assert_eq!(
                field(&reread, "listenerId"),
                FIRST,
                "Removal cannot retarget a pending close callback"
            );
            held = observe(mcp, target, &page).await["ready"].clone();
            assert_eq!(held, 1);
            operation(mcp,target,json!({"operation":"web_socket_route_send","scope":scope,"frame":created["frame"],"routeId":route["routeId"],"socketId":field(&created,"id"),"to":"page","message":{"data":"Acme survivor"}})).await;
            wait_events(mcp, target, &page, 2).await;
            socket_protocols::reply(mcp, target, callback, Value::Null).await;
            events.push(json!({"event":"resolved"}));
            after = observe(mcp, target, &page).await["ready"].clone();
            assert_eq!(after, 1);
            deliver(
                fixture,
                mcp,
                target,
                request(
                    &scope,
                    &route,
                    &created,
                    "page",
                    Some(json!({"code":3003,"reason":"Acme final"})),
                ),
                "cli",
            )
            .await;
        } else {
            let to = if name.starts_with("page-") {
                "page"
            } else {
                "server"
            };
            let options = if name.ends_with("-default") {
                None
            } else if name.ends_with("-empty") {
                Some(json!({}))
            } else if name.ends_with("-empty-reason") {
                Some(json!({"reason":""}))
            } else {
                Some(json!({"reason":"Acme reason"}))
            };
            deliver(
                fixture,
                mcp,
                target,
                request(&scope, &route, &created, to, options),
                ["mcp", "cli", "act"][index % 3],
            )
            .await;
        }
        let page_events = wait_events(
            mcp,
            target,
            &page,
            if name == "pending-server-removal" {
                3
            } else {
                2
            },
        )
        .await;
        assert_eq!(observe(mcp, target, &page).await["ready"], 3);
        peer.settled().await;
        evaluate(mcp,target,&page,"async()=>{if(!(AcmeNativeClosed instanceof Promise))throw Error('Acme native close witness missing');await AcmeNativeClosed;return true;}").await;
        let wire = peer.facts();
        assert_eq!(wire["connected"], 1);
        assert_eq!(wire["closes"].as_array().unwrap().len(), 1);
        let closed = if wire["closes"][0].is_null() {
            json!({"code":1005,"reason":""})
        } else {
            wire["closes"][0].clone()
        };
        let calls = native(mcp, target, &page).await;
        let row = json!({"name":name,"events":events,"heldPage":held,"afterRemoval":after,"pageReady":3,"page":page_events,"nativeCalls":calls,"serverReady":3,"serverClosed":closed,"wireErrors":[]});
        let expected = baseline["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["name"] == name)
            .unwrap();
        std::fs::write(
            fixture.workspace.path().join(format!(
                "socket-close-installed-{engine}-{mode}-{name}.json"
            )),
            serde_json::to_vec_pretty(&json!({"row":row,"wire":wire,"pinnedSdk":expected}))
                .unwrap(),
        )
        .unwrap();
        if index < 8 {
            let nonempty_reason = name.ends_with("-reason") && !name.ends_with("-empty-reason");
            let expected_args = if nonempty_reason {
                json!([{"kind":"number","value":1000},{"kind":"string","value":"Acme reason"}])
            } else {
                json!([])
            };
            assert_eq!(
                calls,
                json!([{"arguments":expected_args,"error":null}]),
                "Canonical native close: {row}"
            );
            let close_code = if name.starts_with("page-") {
                0
            } else if nonempty_reason {
                1000
            } else {
                1005
            };
            let reason = if nonempty_reason { "Acme reason" } else { "" };
            assert_eq!(
                row["page"],
                json!([{"type":"text","data":"Acme-open"},{"type":"close","code":close_code,"reason":reason,"wasClean":true}]),
                "{row}"
            );
            assert_eq!(
                closed,
                json!({"code":if nonempty_reason{1000}else{1005},"reason":reason})
            );
            if !name.ends_with("-reason") {
                let mut actual = row.clone();
                let mut pinned = expected.clone();
                actual.as_object_mut().unwrap().remove("nativeCalls");
                pinned.as_object_mut().unwrap().remove("nativeCalls");
                assert_eq!(
                    actual, pinned,
                    "Empty-close effects must match the SDK; canonical native call arguments are checked separately"
                );
            }
            // Reason-only behavior is the existing corrected Testrun contract;
            // a differing full SDK case stays unaccepted pending owner decision.
        } else {
            assert_eq!(&row, expected, "{row}");
        }
        refused(
            mcp,
            target,
            request(&scope, &route, &created, "server", None),
            "WEBSOCKET_SOCKET_EXPIRED",
        )
        .await;
        let terminal = mcp.detailed_browser_status(target).await;
        assert_eq!(terminal["browser"]["phase"], "ready", "{terminal}");
        assert!(
            terminal["browser"]["callbacks"]
                .as_array()
                .or_else(|| terminal["browser"]["resources"].as_array())
                .unwrap()
                .iter()
                .all(|callback| callback["kind"] != "binding"),
            "No unexpected or unresolved callback may survive endpoint closure: {terminal}"
        );
        operation(
            mcp,
            target,
            json!({"operation":"web_socket_route_remove","scope":scope,"routeId":route["routeId"]}),
        )
        .await;
        operation(mcp, target, json!({"operation":"page_close","page":page})).await;
    }
}

#[tokio::test]
async fn installed_step_four_socket_close_contracts_checkpoint() {
    let fixture = installed_fixture();
    let mut mcp = Mcp::start(&fixture).await;
    let mut failures = vec![];
    for mode in ["headless", "headed"] {
        for engine in ["chrome", "firefox", "webkit"] {
            let baseline = reference(&fixture, engine, mode).await;
            let cli = fixture
                .cli(&["browser", "doctor", "--engine", engine])
                .await;
            let remote = mcp.tool("browser_doctor", json!({"engine":engine})).await;
            assert_eq!(
                cli["capabilities"], remote["capabilities"],
                "CLI/MCP discovery must agree"
            );
            assert_eq!(cli["capabilities"]["webSocketRouting"]["available"], true);
            for operation in ["web_socket_route_handlers", "web_socket_route_close"] {
                assert!(
                    cli["capabilities"]["webSocketRouting"]["operations"]
                        .as_array()
                        .unwrap()
                        .contains(&json!(operation))
                );
                if engine == "chrome" {
                    assert_eq!(
                        cli["capabilities"]["operations"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .filter(|value| **value == operation)
                            .count(),
                        1
                    );
                }
            }
            std::fs::write(
                fixture
                    .workspace
                    .path()
                    .join(format!("socket-close-discovery-{engine}-{mode}.json")),
                serde_json::to_vec_pretty(&cli).unwrap(),
            )
            .unwrap();
            let site = Site::start().await;
            let mut peers = vec![];
            for name in CASES {
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
                &baseline,
                engine,
                mode,
            ))
            .catch_unwind()
            .await;
            if result.is_err() {
                let status = mcp.tool("browser_status", json!({"targetId":target})).await;
                std::fs::write(fixture.workspace.path().join(format!("socket-close-installed-{engine}-{mode}-failure.json")),serde_json::to_vec_pretty(&json!({"status":status,"peers":peers.iter().map(Peer::facts).collect::<Vec<_>>()})).unwrap()).unwrap();
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
                "Installed socket close contracts {engine}/{mode}: discovery, eleven close/default/removal workflows and original cleanup"
            );
        }
    }
    mcp.stop().await;
    assert!(
        failures.is_empty(),
        "Original cleanup failures: {failures:?}"
    );
}
