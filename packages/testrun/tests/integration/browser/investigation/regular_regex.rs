//! Exact pinned regular syntax through HTTP callbacks, HAR filters and socket DFAs.
use super::*;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

async fn reference(fixture: &Fixture) -> Value {
    let output = Command::new(
        fixture
            .cache
            .join("node-24.12.0/node-v24.12.0-darwin-x64/bin/node"),
    )
    .arg(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/browser-regular-regex-baseline.cjs"),
    )
    .arg(fixture.cache.join("playwright-core-1.64.1/package"))
    .current_dir(fixture.workspace.path())
    .env_clear()
    .env("PATH", "/usr/bin:/bin")
    .output()
    .await
    .unwrap();
    std::fs::write(
        fixture
            .workspace
            .path()
            .join("regular-regex-reference.json"),
        &output.stdout,
    )
    .unwrap();
    std::fs::write(
        fixture
            .workspace
            .path()
            .join("regular-regex-reference.stderr"),
        &output.stderr,
    )
    .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["comparisons"], 392);
    assert_eq!(report["cases"].as_array().unwrap().len(), 44);
    assert_eq!(report["unsupported"].as_array().unwrap().len(), 5);
    assert_eq!(report["invalid"].as_array().unwrap().len(), 12);
    assert_eq!(
        report["bundleSha256"],
        "829521b67ed7be90c7d6d6098ff39f2241562d47e4ad7ccab13710924a56001d"
    );
    report
}

struct Site {
    url: String,
    wire: Arc<AtomicUsize>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Site {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Site {
    async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let wire = Arc::new(AtomicUsize::new(0));
        let count = wire.clone();
        let task = tokio::spawn(async move {
            let mut requests = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    accepted=listener.accept(),if requests.len()<32=>{
                        let (mut stream,_)=accepted.unwrap();let count=count.clone();
                        requests.spawn(async move {
                            let mut head=vec![];let mut chunk=[0;1024];
                            while head.len()<8192&&!head.windows(4).any(|part|part==b"\r\n\r\n") {let n=stream.read(&mut chunk).await.unwrap();if n==0{return;}head.extend_from_slice(&chunk[..n]);}
                            let request=String::from_utf8(head).unwrap();let path=request.lines().next().unwrap().split_whitespace().nth(1).unwrap();
                            let (kind,body)=if path=="/" {("text/html","<!doctype html><link rel=icon href=\"data:,\"><title>Acme regex</title>")}else{count.fetch_add(1,Ordering::SeqCst);("text/plain","Acme network")};
                            let response=format!("HTTP/1.1 200 OK\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}",body.len());let _=stream.write_all(response.as_bytes()).await;
                        });
                    },
                    Some(result)=requests.join_next(),if !requests.is_empty()=>{result.unwrap();},
                }
            }
        });
        Self { url, wire, task }
    }
}

async fn cell(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    context: &Value,
    report: &Value,
) -> Value {
    let site = Site::start().await;
    let sockets = socket_protocols::SocketSite::start().await;
    let socket_origin = url::Url::parse(&sockets.url)
        .unwrap()
        .origin()
        .ascii_serialization();
    let page = operation(
        mcp,
        target,
        json!({"operation":"page_create","context":context,"select":false}),
    )
    .await["page"]
        .clone();
    operation(mcp,target,json!({"operation":"page_navigate","page":page,"navigation":{"kind":"goto","url":site.url,"waitUntil":"load"}})).await;
    let mut rows = vec![];
    let mut expected_wire = 0;
    for (field, code) in [
        ("invalid", "ROUTE_REGEX_INVALID"),
        ("unsupported", "ROUTE_REGEX_UNSUPPORTED"),
    ] {
        for case in report[field].as_array().unwrap() {
            let matcher = json!({"kind":"regex","pattern":case["pattern"],"flags":case["flags"]});
            for (request, queued) in [
                (
                    json!({"operation":"network_route","scope":page,"action":{"kind":"add_matcher","matcher":matcher}}),
                    false,
                ),
                (
                    json!({"operation":"har_capture_start","context":context,"content":"embed","mode":"minimal","urlFilter":matcher}),
                    true,
                ),
                (
                    json!({"operation":"web_socket_route_add_matcher","scope":page,"matcher":matcher}),
                    false,
                ),
            ] {
                let socket = request["operation"] == "web_socket_route_add_matcher";
                if field == "unsupported" {
                    // HTTP routing and HAR capture run these on the pinned RegExp
                    // engine and WebSocket routes on the page's native RegExp.
                    if request["operation"] == "network_route" {
                        operation(mcp, target, request).await;
                        operation(mcp,target,json!({"operation":"network_route","scope":page,"action":{"kind":"remove_matcher","matcher":matcher}})).await;
                    } else if socket {
                        let route = operation(mcp, target, request).await;
                        operation(mcp,target,json!({"operation":"web_socket_route_remove","scope":page,"routeId":route["routeId"]})).await;
                    }
                    continue;
                }
                let transport_code = if socket {
                    "WEBSOCKET_REGEX_INVALID"
                } else {
                    code
                };
                collectors::refused(mcp, target, request, transport_code, queued).await;
            }
        }
    }
    for (index, case) in report["cases"].as_array().unwrap().iter().enumerate() {
        let expected = case["expected"].as_bool().unwrap();
        let matcher = json!({"kind":"regex","pattern":case["pattern"],"flags":case["flags"]});
        let url = format!("{}{}", site.url, case["path"].as_str().unwrap());
        let request = json!({"operation":"network_route","scope":page,"action":{"kind":"add_matcher","matcher":matcher}});
        let submitted = fixture
            .cli(&["browser", "send", "--target", target, &request.to_string()])
            .await;
        let handler = completed(mcp, target, submitted).await["handlerId"].clone();
        let pending=mcp.tool("browser_send",json!({"targetId":target,"request":{"operation":"evaluate","scope":page,"isFunction":true,"expression":format!("async()=>await(await fetch({})).text()",json!(url))}})).await;
        if expected {
            let callback = har_interleave::callback(mcp, target, &page, &handler).await;
            operation(mcp,target,json!({"operation":"callback_resolve","callback":callback,"response":{"kind":"route_fulfill","status":200,"body":"Acme matched","contentType":"text/plain"}})).await;
        } else {
            expected_wire += 1;
        }
        let fetched = completed(mcp, target, pending).await;
        assert_eq!(
            fetched["value"]["value"],
            if expected {
                "Acme matched"
            } else {
                "Acme network"
            },
            "{case}"
        );
        operation(mcp,target,json!({"operation":"network_route","scope":page,"action":{"kind":"remove","handlerId":handler}})).await;
        let capture = mcp
            .tool(
                "act",
                json!({"targetId":target,"actions":[{"action":"har_capture_start","context":context,"content":"embed","mode":"minimal","urlFilter":matcher}]}),
            )
            .await;
        let started = completed(mcp, target, capture).await;
        assert_eq!(started["results"].as_array().unwrap().len(), 1);
        assert_eq!(
            har_replay_filter::fetch(mcp, target, &page, &url).await,
            "Acme network"
        );
        expected_wire += 1;
        let until = tokio::time::Instant::now() + Duration::from_secs(10);
        loop {
            let status = operation(
                mcp,
                target,
                json!({"operation":"har_capture_status","context":context}),
            )
            .await;
            if status["completedEntries"] == u64::from(expected) {
                assert_eq!(status["entries"], u64::from(expected));
                break;
            }
            assert!(tokio::time::Instant::now() < until, "{case}: {status}");
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        let stopped = operation(
            mcp,
            target,
            json!({"operation":"har_capture_stop","context":context}),
        )
        .await;
        let bytes = std::fs::read(
            stopped["reports"][0]["attachment"]["path"]
                .as_str()
                .unwrap(),
        )
        .unwrap();
        let archive: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            archive["log"]["entries"].as_array().unwrap().len(),
            usize::from(expected),
            "{case}: {archive}"
        );
        assert_eq!(archive["log"]["_testrunCapture"]["sourceGaps"], 0);
        assert_eq!(archive["log"]["_testrunCapture"]["droppedEntries"], 0);
        operation(
            mcp,
            target,
            json!({"operation":"har_remove","archiveId":stopped["archiveId"]}),
        )
        .await;
        let route = operation(
            mcp,
            target,
            json!({"operation":"web_socket_route_add_matcher","scope":page,"matcher":matcher,"sdkDefaults":false}),
        )
        .await;
        let socket_url = format!("{}{}", socket_origin, case["path"].as_str().unwrap());
        let expression = format!(
            "()=>new Promise(resolve=>{{let opened=false;const socket=new WebSocket({},['Acme.v1','Acme.v2']);socket.onopen=()=>{{opened=true;socket.send('Acme hello');}};socket.onmessage=()=>socket.close(3007,'Acme protocol close');socket.onclose=event=>resolve(JSON.stringify({{opened,code:event.code}}));}})",
            json!(socket_url)
        );
        let pending=mcp.tool("browser_send",json!({"targetId":target,"request":{"operation":"evaluate","scope":page,"isFunction":true,"expression":expression}})).await;
        if expected {
            let (callback, _read) =
                socket_protocols::callback(mcp, target, &route, "onCreate").await;
            socket_protocols::reply(
                mcp,
                target,
                callback,
                json!([{"type":"closePage","code":1002}]),
            )
            .await;
        }
        let result = completed(mcp, target, pending).await;
        let socket: Value =
            serde_json::from_str(result["value"]["value"].as_str().unwrap()).unwrap();
        assert_eq!(
            socket,
            json!({"opened":!expected,"code":if expected {1002}else{3007}}),
            "{case}"
        );
        operation(
            mcp,
            target,
            json!({"operation":"web_socket_route_remove","scope":page,"routeId":route["routeId"]}),
        )
        .await;
        assert_eq!(site.wire.load(Ordering::SeqCst), expected_wire, "{case}");
        rows.push(json!({"index":index,"reference":case,"http":fetched["value"],"captureEntries":archive["log"]["entries"].as_array().unwrap().len(),"socket":socket,"wire":expected_wire}));
        std::fs::write(
            fixture
                .workspace
                .path()
                .join(format!("regular-regex-installed-{target}.json")),
            serde_json::to_vec_pretty(&rows).unwrap(),
        )
        .unwrap();
    }
    operation(mcp, target, json!({"operation":"page_close","page":page})).await;
    json!({"cases":rows.len(),"httpWire":expected_wire})
}

#[tokio::test]
async fn pinned_regular_regex_reference_checkpoint() {
    let fixture = installed_fixture();
    reference(&fixture).await;
}

#[tokio::test]
async fn installed_step_four_regular_regex_checkpoint() {
    let fixture = installed_fixture();
    let report = reference(&fixture).await;
    let mut mcp = Mcp::start(&fixture).await;
    let mut failures = vec![];
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
                &report,
            ))
            .catch_unwind()
            .await;
            if result.is_err() {
                let status = mcp.tool("browser_status", json!({"targetId":target})).await;
                std::fs::write(
                    fixture
                        .workspace
                        .path()
                        .join(format!("regular-regex-failure-{engine}-{mode}.json")),
                    serde_json::to_vec_pretty(&status).unwrap(),
                )
                .unwrap();
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
                failures.push(closed);
            }
            match result {
                Ok(metrics) => eprintln!("Regular regex {engine}/{mode}: {metrics}"),
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
        failures.is_empty(),
        "Original cleanup failures: {failures:?}"
    );
}
