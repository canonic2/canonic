//! Owned WebSocket mocks and real-server relay through private binding replies.
use super::investigation::Site;
use super::*;
use crate::browser::{
    contract::CallbackResponse,
    evaluation::{self, EvaluationValue},
    socket_routes::{Destination, Message},
    values,
};
use serde_json::json;
macro_rules! cell {
    ($name:ident,$engine:ident,$mode:ident) => {
        #[tokio::test]
        async fn $name() {
            run(Engine::$engine, Mode::$mode).await;
        }
    };
}
cell!(item_four_socket_routes_chrome_headless, Chrome, Headless);
cell!(item_four_socket_routes_chrome_headed, Chrome, Headed);
cell!(item_four_socket_routes_firefox_headless, Firefox, Headless);
cell!(item_four_socket_routes_firefox_headed, Firefox, Headed);
cell!(item_four_socket_routes_webkit_headless, Webkit, Headless);
cell!(item_four_socket_routes_webkit_headed, Webkit, Headed);
fn reference(value: &Value, key: &str) -> Reference {
    serde_json::from_value(value[key].clone()).unwrap()
}
fn plain(value: &Value) -> Value {
    let value: EvaluationValue = serde_json::from_value(value.clone()).unwrap();
    values::result(&evaluation::argument(&value, |_| Err("FOREIGN_HANDLE")).unwrap()["value"])
        .unwrap()
}
fn evaluate(page: Reference, expression: String) -> Request {
    Request::Evaluate {
        scope: page,
        expression,
        is_function: Some(true),
        argument: Default::default(),
        return_handle: false,
    }
}
async fn push_ready(session: &Session, binding: Uuid) {
    let until = Instant::now() + Duration::from_secs(10);
    loop {
        let status = session.status();
        let items = status["callbacks"]
            .as_array()
            .or_else(|| status["resources"].as_array())
            .unwrap();
        let callbacks = items
            .iter()
            .filter(|item| item["kind"] == "binding")
            .map(|item| reference(item, "reference"))
            .collect::<Vec<_>>();
        for callback in callbacks {
            let read = successful(session, Request::BindingRead { callback }).await;
            assert_eq!(read["bindingId"], json!(binding));
            let event = plain(&read["arguments"][0]);
            successful(
                session,
                Request::CallbackResolve {
                    callback,
                    response: CallbackResponse::BindingResolve { value: json!([]) },
                },
            )
            .await;
            if event["type"] == "onMessageFromPage" && event["data"]["data"] == "Acme push ready" {
                return;
            }
        }
        assert!(
            Instant::now() < until,
            "Push listener was not armed: {status}"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}
async fn callbacks(
    session: &Session,
    route: &Value,
    scope: Reference,
    frame: Reference,
    relay: bool,
    pending: Uuid,
) -> Value {
    let binding: Uuid = serde_json::from_value(route["bindingId"].clone()).unwrap();
    let route_id: Uuid = serde_json::from_value(route["routeId"].clone()).unwrap();
    let mut socket_id = Value::Null;
    let until = Instant::now() + Duration::from_secs(20);
    let mut creation = false;
    let mut page_message = false;
    let mut server_message = false;
    loop {
        let status = session.status();
        let items = status["callbacks"]
            .as_array()
            .or_else(|| status["resources"].as_array())
            .unwrap();
        let current = items
            .iter()
            .filter(|r| r["kind"] == "binding")
            .map(|r| reference(r, "reference"))
            .collect::<Vec<_>>();
        for callback in current {
            let read = successful(session, Request::BindingRead { callback }).await;
            assert_eq!(read["bindingId"], json!(binding), "{read}");
            assert_eq!(read["frame"], json!(frame), "{read}");
            let event = plain(&read["arguments"][0]);
            assert!(
                !serde_json::to_string(items)
                    .unwrap()
                    .contains("Acme client secret")
            );
            let commands = match event["type"].as_str().unwrap() {
                "onCreate" => {
                    socket_id = event["id"].clone();
                    assert!(!creation);
                    creation = true;
                    json!([{"type":if relay{"connect"}else{"ensureOpened"}}])
                }
                "onMessageFromPage" => {
                    page_message = true;
                    if relay {
                        assert_eq!(event["data"]["data"], "Acme client secret");
                        json!([])
                    } else {
                        assert_eq!(event["data"]["data"], "AP+A");
                        json!([])
                    }
                }
                "onMessageFromServer" if event["data"]["isBase64"] == true => {
                    server_message = true;
                    assert_eq!(event["data"]["data"], "AAEC/w==");
                    json!([{"type":"sendToPage","data":event["data"]}])
                }
                _ => json!([]),
            };
            successful(
                session,
                Request::CallbackResolve {
                    callback,
                    response: CallbackResponse::BindingResolve { value: commands },
                },
            )
            .await;
            if event["type"] == "onMessageFromPage" {
                successful(
                    session,
                    Request::WebSocketRouteSend {
                        scope,
                        frame,
                        route_id,
                        socket_id: event["id"].as_str().unwrap().into(),
                        to: if relay {
                            Destination::Server
                        } else {
                            Destination::Page
                        },
                        message: serde_json::from_value(event["data"].clone()).unwrap(),
                    },
                )
                .await;
            }
        }
        if creation && page_message && (!relay || server_message) {
            break;
        }
        assert!(
            Instant::now() < until,
            "Socket callbacks unavailable: {status}"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    let operation = settled(session, pending).await;
    assert_eq!(operation.outcome, Outcome::Succeeded, "{operation:?}");
    let mut result = operation.result.unwrap();
    result["socketId"] = socket_id;
    result
}
async fn automatic_forwarding(session: &Session, route: &Value, page: Reference, url: &str) {
    let expression = format!(
        "()=>new Promise(resolve=>{{const socket=globalThis.AcmeAutomaticSocket=new WebSocket({});let bytes;socket.binaryType='arraybuffer';socket.onopen=()=>socket.send('Acme automatic close');socket.onmessage=event=>{{if(event.data instanceof ArrayBuffer)bytes=Array.from(new Uint8Array(event.data));}};socket.onclose=event=>resolve({{bytes,code:event.code,reason:event.reason,wasClean:event.wasClean,readyState:socket.readyState}});}})",
        json!(url)
    );
    let pending = session
        .submit(Uuid::new_v4(), evaluate(page, expression), 30000)
        .unwrap();
    let until = Instant::now() + Duration::from_secs(10);
    let callback = loop {
        let status = session.status();
        let items = status["callbacks"]
            .as_array()
            .or_else(|| status["resources"].as_array())
            .unwrap();
        if let Some(item) = items.iter().find(|item| item["kind"] == "binding") {
            break reference(item, "reference");
        }
        assert!(
            Instant::now() < until,
            "Automatic socket callback unavailable: {status}"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    };
    let read = successful(session, Request::BindingRead { callback }).await;
    assert_eq!(read["bindingId"], route["bindingId"]);
    assert_eq!(plain(&read["arguments"][0])["type"], "onCreate");
    successful(
        session,
        Request::CallbackResolve {
            callback,
            response: CallbackResponse::BindingResolve {
                value: json!([
                    {"type":"setForwarding","from":"page","enabled":true},
                    {"type":"setForwarding","from":"server","enabled":true},
                    {"type":"setCloseForwarding","from":"page","enabled":true},
                    {"type":"setCloseForwarding","from":"server","enabled":true},
                    {"type":"connect"}
                ]),
            },
        },
    )
    .await;
    // Message and close events reach both sides without binding replies.
    let result = settled(session, pending).await;
    assert_eq!(result.outcome, Outcome::Succeeded, "{result:?}");
    assert_eq!(
        plain(&result.result.unwrap()["value"]),
        json!({"bytes":[0,1,2,255],"code":1001,"reason":"Acme relay closed","wasClean":true,"readyState":3})
    );
}
#[expect(
    clippy::too_many_lines,
    reason = "One owned engine cell verifies route precedence, mock bytes, future documents and real relay with cleanup"
)]
async fn run(engine: Engine, mode: Mode) {
    std::env::var_os("TESTRUN_BROWSER_ROOT").expect("Select a private browser cache");
    let site = Site::start().await;
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("state");
    crate::artifacts::directory(&directory).unwrap();
    let bundle = if engine == Engine::Chrome {
        Bundle::Chrome(
            super::super::preparation::prepare_chrome(|_| {})
                .await
                .unwrap(),
        )
    } else {
        Bundle::Playwright(
            super::super::preparation::prepare(engine, |_| {})
                .await
                .unwrap(),
        )
    };
    let session = Session::launch_connection(
        bundle,
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        LaunchOptions {
            media_fixtures: Default::default(),
            proxy: None,
            engine,
            mode,
            context: Default::default(),
        },
        &directory,
    )
    .unwrap();
    let result=std::panic::AssertUnwindSafe(async{
        let ready=settled(&session,session.startup()).await;assert_eq!(ready.outcome,Outcome::Succeeded,"{ready:?}");let ready=ready.result.unwrap();let page=reference(&ready,"page");let context=reference(&ready,"context");
        successful(&session,Request::PageNavigate{page,navigation:Navigation::Goto{url:format!("{}/audit",site.url),wait_until:Load::Load}}).await;
        let url=format!("{}/socket",site.url.replacen("http:","ws:",1));
        let context_route=successful(&session,Request::WebSocketRouteAdd{scope:context,url:url.clone(),sdk_defaults:false}).await;
        let page_route=successful(&session,Request::WebSocketRouteAdd{scope:page,url:url.clone(),sdk_defaults:false}).await;
        let expression=format!("()=>new Promise(resolve=>{{const socket=globalThis.AcmeSocket=new WebSocket({});socket.binaryType='arraybuffer';socket.onopen=()=>socket.send(new Uint8Array([9,0,255,128,9]).subarray(1,4));socket.onmessage=event=>resolve(Array.from(new Uint8Array(event.data)));}})",json!(url));
        let frame_value=successful(&session,evaluate(page,"()=>true".into())).await;
        let frame=reference(&frame_value,"frame");
        let pending=session.submit(Uuid::new_v4(),evaluate(page,expression),30000).unwrap();
        let value=callbacks(&session,&page_route,page,frame,false,pending).await;
        assert_eq!(plain(&value["value"]),json!([0,255,128]));
        let socket=value["socketId"].as_str().unwrap().to_owned();
        let sibling=reference(&successful(&session,Request::PageCreate{context,select:false}).await,"page");
        let sibling_frame=reference(&successful(&session,evaluate(sibling,"()=>true".into())).await,"frame");
        let foreign=settled(&session,session.submit(Uuid::new_v4(),Request::WebSocketRouteSend{scope:page,frame:sibling_frame,route_id:serde_json::from_value(page_route["routeId"].clone()).unwrap(),socket_id:socket.clone(),to:Destination::Page,message:Message{data:"Acme refused".into(),is_base64:false}},30000).unwrap()).await;
        assert_eq!(foreign.code.as_deref(),Some("INVALID_SCOPE"),"{foreign:?}");
        successful(&session,Request::PageClose{page:sibling}).await;
        let waiting=session.submit(Uuid::new_v4(),evaluate(page,"()=>new Promise(resolve=>{AcmeSocket.onmessage=event=>resolve(Array.from(new Uint8Array(event.data)));AcmeSocket.send('Acme push ready')})".into()),30000).unwrap();
        push_ready(&session,serde_json::from_value(page_route["bindingId"].clone()).unwrap()).await;
        successful(&session,Request::WebSocketRouteSend{scope:page,frame,route_id:serde_json::from_value(page_route["routeId"].clone()).unwrap(),socket_id:socket.clone(),to:Destination::Page,message:Message{data:"AQI=".into(),is_base64:true}}).await;
        let pushed=settled(&session,waiting).await;assert_eq!(pushed.outcome,Outcome::Succeeded,"{pushed:?}");assert_eq!(plain(&pushed.result.unwrap()["value"]),json!([1,2]));
        let wrong_owner=settled(&session,session.submit(Uuid::new_v4(),Request::WebSocketRouteSend{scope:context,frame,route_id:serde_json::from_value(context_route["routeId"].clone()).unwrap(),socket_id:socket.clone(),to:Destination::Page,message:Message{data:"Acme rejected".into(),is_base64:false}},30000).unwrap()).await;
        assert_eq!(wrong_owner.code.as_deref(),Some("WEBSOCKET_SOCKET_EXPIRED"),"{wrong_owner:?}");
        successful(&session,Request::WebSocketRouteRemove{scope:page,route_id:serde_json::from_value(page_route["routeId"].clone()).unwrap()}).await;
        assert_eq!(successful(&session,evaluate(page,"()=>AcmeSocket.readyState".into())).await["value"]["value"],3);
        let future=successful(&session,Request::PageCreate{context,select:false}).await;let future=reference(&future,"page");
        successful(&session,Request::PageNavigate{page:future,navigation:Navigation::Goto{url:format!("{}/audit",site.url),wait_until:Load::Load}}).await;
        let expression=format!("()=>new Promise(resolve=>{{const socket=globalThis.AcmeSocket=new WebSocket({});socket.binaryType='arraybuffer';socket.onopen=()=>socket.send('Acme client secret');socket.onmessage=event=>resolve(Array.from(new Uint8Array(event.data)));}})",json!(url));
        let future_frame=reference(&successful(&session,evaluate(future,"()=>true".into())).await,"frame");
        let pending=session.submit(Uuid::new_v4(),evaluate(future,expression),30000).unwrap();
        let value=callbacks(&session,&context_route,context,future_frame,true,pending).await;
        assert_eq!(plain(&value["value"]),json!([0,1,2,255]));
        // Closing the future page witnesses retirement of any queued server-close callback.
        successful(&session,Request::PageClose{page:future}).await;
        let automatic=reference(&successful(&session,Request::PageCreate{context,select:false}).await,"page");
        successful(&session,Request::PageNavigate{page:automatic,navigation:Navigation::Goto{url:format!("{}/audit",site.url),wait_until:Load::Load}}).await;
        automatic_forwarding(&session,&context_route,automatic,&url).await;
        successful(&session,Request::PageClose{page:automatic}).await;
        successful(&session,Request::WebSocketRouteRemove{scope:context,route_id:serde_json::from_value(context_route["routeId"].clone()).unwrap()}).await;
    }).catch_unwind().await;
    if result.is_err() {
        eprintln!("Socket fixture state: {}", session.status());
    }
    let closed = settled(&session, session.disconnect(30000).unwrap()).await;
    assert_eq!(closed.outcome, Outcome::Succeeded, "{closed:?}");
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}
