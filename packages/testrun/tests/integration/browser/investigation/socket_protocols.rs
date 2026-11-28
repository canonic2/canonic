//! Subprotocol offers, native negotiation and terminal mock closure through CLI/MCP.
use super::*;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

pub(super) struct SocketSite {
    pub(super) url: String,
    connections: Arc<AtomicUsize>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for SocketSite {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl SocketSite {
    pub(super) async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let url = format!("ws://{}/protocol-socket", listener.local_addr().unwrap());
        let connections = Arc::new(AtomicUsize::new(0));
        let count = connections.clone();
        let task = tokio::spawn(async move {
            let mut requests = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    accepted=listener.accept()=>{
                        let (stream,_)=accepted.unwrap();let count=count.clone();
                        requests.spawn(async move { serve(stream,count).await; });
                    }
                    _=requests.join_next(),if !requests.is_empty()=>{}
                }
            }
        });
        Self {
            url,
            connections,
            task,
        }
    }
}
async fn serve(stream: tokio::net::TcpStream, count: Arc<AtomicUsize>) {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::{
        Message,
        handshake::server::{Request, Response},
    };
    count.fetch_add(1, Ordering::SeqCst);
    let mut socket =
        tokio_tungstenite::accept_hdr_async(stream, |request: &Request, mut response: Response| {
            let protocols = request
                .headers()
                .get("sec-websocket-protocol")
                .unwrap()
                .to_str()
                .unwrap()
                .split(',')
                .map(str::trim)
                .collect::<Vec<_>>();
            assert_eq!(protocols, vec!["Acme.v1", "Acme.v2"]);
            response
                .headers_mut()
                .insert("sec-websocket-protocol", "Acme.v2".parse().unwrap());
            Ok(response)
        })
        .await
        .unwrap();
    let message = socket.next().await.unwrap().unwrap();
    assert_eq!(message.to_text().unwrap(), "Acme hello");
    socket
        .send(Message::Text("Acme negotiated".into()))
        .await
        .unwrap();
    let message = socket.next().await.unwrap().unwrap();
    let Message::Close(Some(frame)) = message else {
        panic!("Expected owned close handshake")
    };
    assert_eq!(u16::from(frame.code), 3007);
    assert_eq!(frame.reason, "Acme protocol close");
    socket.flush().await.unwrap();
}

pub(super) async fn callback(
    mcp: &mut Mcp,
    target: &str,
    route: &Value,
    expected: &str,
) -> (Value, Value) {
    let until = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let status = mcp.detailed_browser_status(target).await;
        let browser = &status["browser"];
        let items = browser["callbacks"]
            .as_array()
            .or_else(|| browser["resources"].as_array())
            .unwrap();
        if let Some(item) = items.iter().find(|item| item["kind"] == "binding") {
            assert!(!item.to_string().contains("Acme.v"));
            let reference = item["reference"].clone();
            let read = operation(
                mcp,
                target,
                json!({"operation":"binding_read","callback":reference}),
            )
            .await;
            assert_eq!(read["bindingId"], route["bindingId"]);
            assert_eq!(
                socket_field(&read["arguments"][0], "type")["value"],
                expected
            );
            return (reference, read);
        }
        assert!(
            tokio::time::Instant::now() < until,
            "Socket callback unavailable: {status}"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}
fn offers(read: &Value) -> Value {
    json!(
        socket_field(&read["arguments"][0], "protocols")["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item["value"].clone())
            .collect::<Vec<_>>()
    )
}
pub(super) async fn reply(mcp: &mut Mcp, target: &str, callback: Value, commands: Value) {
    operation(mcp,target,json!({"operation":"callback_resolve","callback":callback,"response":{"kind":"binding_resolve","value":commands}})).await;
}
async fn expired(mcp: &mut Mcp, target: &str, scope: &Value, route: &Value, read: &Value) {
    let submitted=mcp.tool("browser_send",json!({"targetId":target,"request":{"operation":"web_socket_route_send","scope":scope,"frame":read["frame"],"routeId":route["routeId"],"socketId":socket_field(&read["arguments"][0],"id")["value"],"to":"page","message":{"data":"Acme stale push","isBase64":false}}})).await;
    let status = mcp
        .tool(
            "browser_status",
            json!({"targetId":target,"operationId":submitted["operation"]["id"],"waitMs":30000}),
        )
        .await;
    assert_eq!(
        status["operation"]["code"], "WEBSOCKET_SOCKET_EXPIRED",
        "{status}"
    );
}

pub(super) async fn relay(
    mcp: &mut Mcp,
    target: &str,
    page: &Value,
    context: &Value,
    route: &Value,
    url: &str,
) {
    let expression = format!(
        "()=>new Promise(resolve=>{{const offers=['Acme.v1','Acme.v2'];const socket=new WebSocket({},offers);offers[0]='Acme.changed';offers.push('Acme.extra');let message;socket.onopen=()=>socket.send('Acme hello');socket.onmessage=event=>{{message=event.data;socket.close(3007,'Acme protocol close');}};socket.onclose=event=>resolve(JSON.stringify({{protocol:socket.protocol,message,code:event.code,wasClean:event.wasClean,readyState:socket.readyState}}));}})",
        json!(url)
    );
    let pending=mcp.tool("browser_send",json!({"targetId":target,"request":{"operation":"evaluate","scope":page,"expression":expression,"isFunction":true}})).await;
    let (reference, read) = callback(mcp, target, route, "onCreate").await;
    assert_eq!(offers(&read), json!(["Acme.v1", "Acme.v2"]));
    reply(mcp,target,reference,json!([
        {"type":"setForwarding","from":"page","enabled":true},{"type":"setForwarding","from":"server","enabled":true},
        {"type":"setCloseForwarding","from":"page","enabled":true},{"type":"setCloseForwarding","from":"server","enabled":true},{"type":"connect"}
    ])).await;
    let result = completed(mcp, target, pending).await;
    assert_eq!(
        serde_json::from_str::<Value>(result["value"]["value"].as_str().unwrap()).unwrap(),
        json!({"protocol":"Acme.v2","message":"Acme negotiated","code":3007,"wasClean":true,"readyState":3})
    );
    expired(mcp, target, context, route, &read).await;
}

async fn mock(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    page: &Value,
    route: &Value,
    url: &str,
) {
    let expression = format!(
        "()=>new Promise(resolve=>{{const refused=[];for(const protocols of [['Acme','Acme'],[''],['Acme bad'],['Acme,other']]){{try{{new WebSocket({},protocols);refused.push('accepted');}}catch(error){{refused.push(error.name);}}}}const socket=new WebSocket({},['Acme.mock.v1','Acme.mock.v2']);socket.onopen=()=>socket.send('Acme mock');socket.onclose=event=>resolve(JSON.stringify({{protocol:socket.protocol,refused,code:event.code,reason:event.reason,wasClean:event.wasClean,readyState:socket.readyState}}));}})",
        json!(url),
        json!(url)
    );
    let pending=mcp.tool("browser_send",json!({"targetId":target,"request":{"operation":"evaluate","scope":page,"expression":expression,"isFunction":true}})).await;
    let (reference, read) = callback(mcp, target, route, "onCreate").await;
    assert_eq!(offers(&read), json!(["Acme.mock.v1", "Acme.mock.v2"]));
    reply(mcp, target, reference, json!([{"type":"ensureOpened"}])).await;
    let (reference, _) = callback(mcp, target, route, "onMessageFromPage").await;
    let request=json!({"operation":"callback_resolve","callback":reference,"response":{"kind":"binding_resolve","value":[{"type":"closePage","code":1002,"reason":"Acme unsupported protocol"}]}}).to_string();
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &request])
        .await;
    completed(mcp, target, submitted).await;
    let result = completed(mcp, target, pending).await;
    assert_eq!(
        serde_json::from_str::<Value>(result["value"]["value"].as_str().unwrap()).unwrap(),
        json!({"protocol":"Acme.mock.v1","refused":["SyntaxError","SyntaxError","SyntaxError","SyntaxError"],"code":1002,"reason":"Acme unsupported protocol","wasClean":true,"readyState":3})
    );
    expired(mcp, target, page, route, &read).await;
}

async fn reject_mock(mcp: &mut Mcp, target: &str, page: &Value, route: &Value, url: &str) {
    let expression = format!(
        "()=>new Promise(resolve=>{{let opened=false;const socket=new WebSocket({},['Acme.unsupported']);socket.onopen=()=>opened=true;socket.onclose=event=>resolve(JSON.stringify({{opened,protocol:socket.protocol,code:event.code,readyState:socket.readyState}}));}})",
        json!(url)
    );
    let pending=mcp.tool("browser_send",json!({"targetId":target,"request":{"operation":"evaluate","scope":page,"expression":expression,"isFunction":true}})).await;
    let (reference, read) = callback(mcp, target, route, "onCreate").await;
    assert_eq!(offers(&read), json!(["Acme.unsupported"]));
    reply(
        mcp,
        target,
        reference,
        json!([{"type":"closePage","code":1002},{"type":"connect"}]),
    )
    .await;
    let result = completed(mcp, target, pending).await;
    assert_eq!(
        serde_json::from_str::<Value>(result["value"]["value"].as_str().unwrap()).unwrap(),
        json!({"opened":false,"protocol":"","code":1002,"readyState":3})
    );
    expired(mcp, target, page, route, &read).await;
}

async fn cell(fixture: &Fixture, mcp: &mut Mcp, target: &str, context: &Value, site: &SocketSite) {
    let context_route = operation(
        mcp,
        target,
        json!({"operation":"web_socket_route_add","scope":context,"url":site.url,"sdkDefaults":false}),
    )
    .await;
    let page = operation(
        mcp,
        target,
        json!({"operation":"page_create","context":context,"select":false}),
    )
    .await["page"]
        .clone();
    relay(mcp, target, &page, context, &context_route, &site.url).await;
    let page_route = operation(
        mcp,
        target,
        json!({"operation":"web_socket_route_add","scope":page,"url":site.url,"sdkDefaults":false}),
    )
    .await;
    mock(fixture, mcp, target, &page, &page_route, &site.url).await;
    reject_mock(mcp, target, &page, &page_route, &site.url).await;
    for (scope, route) in [(&page, &page_route), (context, &context_route)] {
        operation(
            mcp,
            target,
            json!({"operation":"web_socket_route_remove","scope":scope,"routeId":route["routeId"]}),
        )
        .await;
    }
    operation(mcp, target, json!({"operation":"page_close","page":page})).await;
}

pub(super) async fn matched_urls(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    context: &Value,
    kind: &str,
) -> Value {
    let site = SocketSite::start().await;
    let url = format!("{}?token=Acme.private", site.url);
    let path = url::Url::parse(&site.url).unwrap().path().to_owned();
    let regex_matchers = kind != "glob";
    let started = std::time::Instant::now();
    let context_matcher = match kind {
        "regex" => {
            json!({"kind":"regex","pattern":format!(r"^{}\?TOKEN=Acme[.]private$",regex::escape(&site.url.to_uppercase())),"flags":"i"})
        }
        "url_pattern" => {
            json!({"kind":"url_pattern","protocol":"WS","pathname":path.to_uppercase(),"search":"TOKEN=Acme.private"})
        }
        _ => json!({"kind":"glob","pattern":format!("{}?token=*",site.url)}),
    };
    let request=json!({"operation":"web_socket_route_add_matcher","scope":context,"matcher":context_matcher,"sdkDefaults":false}).to_string();
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &request])
        .await;
    let context_route = completed(mcp, target, submitted).await;
    let fresh_ms = started.elapsed().as_secs_f64() * 1000.0;
    let page = operation(
        mcp,
        target,
        json!({"operation":"page_create","context":context,"select":false}),
    )
    .await["page"]
        .clone();
    relay(mcp, target, &page, context, &context_route, &url).await;
    let started = std::time::Instant::now();
    let page_route=operation(mcp,target,json!({"operation":"web_socket_route_add_matcher","scope":page,"matcher":{"kind":"exact","url":url},"sdkDefaults":false})).await;
    let retained_ms = started.elapsed().as_secs_f64() * 1000.0;
    mock(fixture, mcp, target, &page, &page_route, &url).await;
    operation(
        mcp,
        target,
        json!({"operation":"web_socket_route_remove","scope":page,"routeId":page_route["routeId"]}),
    )
    .await;
    let page_matcher = match kind {
        "regex" => {
            json!({"kind":"regex","pattern":r"\bprotocol-socket\?token=Acme(?:[.]private|_[0-9]+)$"})
        }
        "url_pattern" => {
            json!({"kind":"url_pattern","pathname":path,"search":r"token=Acme(\.private|_[0-9]+)"})
        }
        _ => json!({"kind":"glob","pattern":format!("{}?token=Acme.*",site.url)}),
    };
    let started = std::time::Instant::now();
    let page_route = operation(
        mcp,
        target,
        json!({"operation":"web_socket_route_add_matcher","scope":page,"matcher":page_matcher,"sdkDefaults":false}),
    )
    .await;
    let matcher_ms = started.elapsed().as_secs_f64() * 1000.0;
    reject_mock(mcp, target, &page, &page_route, &url).await;
    if kind == "regex" {
        // The Page's case-sensitive regex must lose to the Context's /i regex.
        relay(
            mcp,
            target,
            &page,
            context,
            &context_route,
            &format!("{}?token=acme.PRIVATE", site.url),
        )
        .await;
    }
    let expression = format!(
        "()=>new Promise(resolve=>{{const socket=new WebSocket({},['Acme.v1','Acme.v2']);socket.onopen=()=>socket.send('Acme hello');socket.onmessage=()=>socket.close(3007,'Acme protocol close');socket.onclose=event=>resolve(JSON.stringify({{protocol:socket.protocol,code:event.code,wasClean:event.wasClean}}));}})",
        json!(if regex_matchers {
            format!("{}?token=Acme.privateExtra", site.url)
        } else {
            format!("{}?other=Acme.private", site.url)
        })
    );
    let result = operation(
        mcp,
        target,
        json!({"operation":"evaluate","scope":page,"expression":expression,"isFunction":true}),
    )
    .await;
    assert_eq!(
        serde_json::from_str::<Value>(result["value"]["value"].as_str().unwrap()).unwrap(),
        json!({"protocol":"Acme.v2","code":3007,"wasClean":true})
    );
    let started = std::time::Instant::now();
    for (scope, route) in [(&page, &page_route), (context, &context_route)] {
        operation(
            mcp,
            target,
            json!({"operation":"web_socket_route_remove","scope":scope,"routeId":route["routeId"]}),
        )
        .await;
    }
    operation(mcp, target, json!({"operation":"page_close","page":page})).await;
    assert_eq!(
        site.connections.load(Ordering::SeqCst),
        if kind == "regex" { 3 } else { 2 }
    );
    json!({"freshCliRegistrationMs":fresh_ms,"retainedMcpRegistrationMs":retained_ms,"retainedPageMatcherRegistrationMs":matcher_ms,"scopeRemovalAndPageCloseMs":started.elapsed().as_secs_f64()*1000.0})
}

#[tokio::test]
async fn installed_step_four_socket_protocol_checkpoint() {
    let fixture = installed_fixture();
    let site = SocketSite::start().await;
    let mut mcp = Mcp::start(&fixture).await;
    let mut cleanup_failures = vec![];
    let mut expected = 0;
    for mode in ["headless", "headed"] {
        for engine in ["chrome", "firefox", "webkit"] {
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
                &site,
            ))
            .catch_unwind()
            .await;
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
            if let Err(panic) = result {
                fixture.cli(&["disconnect"]).await;
                mcp.tool("disconnect", json!({})).await;
                mcp.stop().await;
                std::panic::resume_unwind(panic);
            }
            expected += 1;
            assert_eq!(site.connections.load(Ordering::SeqCst), expected);
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
