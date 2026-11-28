//! Independent pinned context proxy wire facts precede adapter implementation.
use super::http_proxy::tunnel;
use super::*;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub(super) struct Origin {
    pub(super) url: String,
    pub(super) port: u16,
    records: Arc<Mutex<Vec<Value>>>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Origin {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Origin {
    pub(super) async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let address = listener.local_addr().unwrap();
        let records = Arc::new(Mutex::new(Vec::new()));
        let captured = records.clone();
        let task = tokio::spawn(async move {
            let mut tasks = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    accepted=listener.accept(),if tasks.len()<64=>{
                        let (mut stream,_)=accepted.unwrap();
                        let records=captured.clone();
                        tasks.spawn(async move {
                            let mut bytes=Vec::new();
                            let mut buffer=[0;1024];
                            while bytes.len()<=32768 && !bytes.windows(4).any(|part|part==b"\r\n\r\n") {
                                let Ok(count)=stream.read(&mut buffer).await else {return;};
                                if count==0 {return;}
                                bytes.extend_from_slice(&buffer[..count]);
                            }
                            assert!(bytes.len()<=32768 && bytes.ends_with(b"\r\n\r\n"));
                            let text=String::from_utf8(bytes).unwrap();
                            let path=text.split_whitespace().nth(1).unwrap().split('?').next().unwrap();
                            let header=|name:&str|text.lines().skip(1).filter_map(|line|line.split_once(':'))
                                .find(|(key,_)|key.eq_ignore_ascii_case(name)).map(|(_,value)|value.trim()).unwrap_or("");
                            if path!="/favicon.ico" {
                                let mut rows=records.lock().unwrap();
                                assert!(rows.len()<1024);
                                rows.push(json!({"path":path,"authorization":header("authorization"),"proxyAuthorization":header("proxy-authorization"),"custom":header("x-acme")}));
                            }
                            let challenge=path=="/auth" && header("authorization").is_empty();
                            let body="<!doctype html><title>Acme proxy</title><button>Acme</button>";
                            let status=if challenge {"401 Unauthorized\r\nWWW-Authenticate: Basic realm=\"Acme origin\""} else {"200 OK"};
                            let response=format!("HTTP/1.1 {status}\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len());
                            let _=stream.write_all(response.as_bytes()).await;
                        });
                    },
                    Some(result)=tasks.join_next(),if !tasks.is_empty()=>{result.unwrap();},
                }
            }
        });
        Self {
            url: format!("http://{address}"),
            port: address.port(),
            records,
            task,
        }
    }
    pub(super) fn records(&self) -> Vec<Value> {
        self.records.lock().unwrap().clone()
    }
}
pub(super) fn basic(username: &str, password: &str) -> String {
    use base64::Engine;
    format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode(format!("{username}:{password}"))
    )
}
fn settings(name: &str, proxy: &tunnel::Proxy) -> Value {
    let mut selected = json!({"server":proxy.url});
    if ["auth", "bypass", "username_only", "empty_password"].contains(&name) {
        selected["username"] = json!("Acme proxy");
        if name != "username_only" {
            selected["password"] = json!(if name == "empty_password" {
                ""
            } else {
                "proxy example"
            });
        }
    }
    if name == "bypass" {
        selected["bypass"] = json!(" 127.0.0.1 , localhost ");
    }
    if name == "normalized" {
        selected["server"] = json!(format!(
            "{}/ignored?Acme=1#fragment",
            proxy
                .url
                .replacen("http://", "http://AcmeIgnored:example@", 1)
        ));
    }
    json!({"viewport":{"width":640,"height":480},"deviceScaleFactor":1,"proxy":selected,
        "httpCredentials":{"username":"Acme origin","password":"origin example"},
        "extraHTTPHeaders":[{"name":"X-Acme","value":"context"}]})
}
pub(super) fn proxy_records(proxy: &tunnel::Proxy) -> Value {
    json!(
        proxy
            .requests()
            .iter()
            .filter(|row| row["path"] != "/favicon.ico")
            .map(|row| {
                let authorization = row["authorization"].as_str().unwrap_or("");
                if row["method"] == "CONNECT" {
                    json!({"kind":"connect","authorization":authorization})
                } else {
                    json!({"kind":"request","path":row["path"],"authorization":authorization})
                }
            })
            .collect::<Vec<_>>()
    )
}
async fn exercise(
    mcp: &mut Mcp,
    target: &str,
    engine: &str,
    context: &Value,
    page: &Value,
    origin: &Origin,
    proxy: &tunnel::Proxy,
    override_proxy: &tunnel::Proxy,
    expected: &Value,
) {
    operation(mcp,target,json!({"operation":"page_navigate","page":page,"navigation":{"kind":"goto","url":format!("{}/page",origin.url)}})).await;
    let mut clients = Vec::new();
    for explicit in [false, true] {
        let mut options = json!({"browserContext":context,"baseUrl":origin.url,"allowUnknownCookiePartitions":engine=="firefox"});
        if explicit {
            options["proxy"] = json!({"server":override_proxy.url,"username":"Acme override","password":"override example"});
        }
        let client = operation(
            mcp,
            target,
            json!({"operation":"http_context_create","options":options}),
        )
        .await["httpContext"]
            .clone();
        let response = operation(
            mcp,
            target,
            json!({"operation":"http_fetch","httpContext":client,"request":{"url":"/auth"}}),
        )
        .await;
        assert_eq!(response["status"], 200, "{response}");
        operation(
            mcp,
            target,
            json!({"operation":"http_response_remove","requestId":response["requestId"]}),
        )
        .await;
        clients.push(client);
    }
    operation(
        mcp,
        target,
        json!({"operation":"context_close","context":context}),
    )
    .await;
    let before = json!({"origin":origin.records(),"proxy":proxy_records(proxy),"overrideProxy":proxy_records(override_proxy)});
    for client in &clients {
        collectors::refused(
            mcp,
            target,
            json!({"operation":"http_fetch","httpContext":client,"request":{"url":"/auth"}}),
            // Pinned BrowserContext close disposes its request context first.
            "HTTP_CONTEXT_DISPOSED",
            true,
        )
        .await;
        operation(
            mcp,
            target,
            json!({"operation":"http_context_remove","httpContext":client}),
        )
        .await;
    }
    let actual = json!({"origin":origin.records(),"proxy":proxy_records(proxy),"overrideProxy":proxy_records(override_proxy)});
    assert_eq!(
        actual, before,
        "Closed source clients must not reach either endpoint"
    );
    for field in ["origin", "proxy", "overrideProxy"] {
        assert_eq!(
            actual[field], expected[field],
            "{} {engine} {field}",
            expected["name"]
        );
    }
}

#[tokio::test]
async fn installed_step_four_context_proxy_checkpoint() {
    let fixture = installed_fixture();
    let mut mcp = Mcp::start(&fixture).await;
    let mut failures = Vec::new();
    for mode in ["headless", "headed"] {
        for engine in ["chrome", "firefox", "webkit"] {
            let report = reference(&fixture, engine, mode).await;
            let origin = Origin::start().await;
            let proxy = tunnel::Proxy::start_forwarding(
                vec![origin.port],
                Some(basic("Acme proxy", "proxy example")),
            )
            .await;
            let override_proxy = tunnel::Proxy::start_forwarding(
                vec![origin.port],
                Some(basic("Acme override", "override example")),
            )
            .await;
            let options = settings("auth", &proxy);
            let connected = if mode == "headed" {
                fixture
                    .cli(&[
                        "connect",
                        "--engine",
                        engine,
                        "--mode",
                        mode,
                        "--context",
                        &options.to_string(),
                    ])
                    .await
            } else {
                mcp.tool(
                    "browser_connect",
                    json!({"engine":engine,"mode":mode,"context":options}),
                )
                .await
            };
            let target = connected["targetId"].as_str().unwrap();
            let ready = fixture.ready_target(target).await;
            let scenario = std::panic::AssertUnwindSafe(async {
                let expected = report
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|row| row["name"] == "auth")
                    .unwrap();
                exercise(
                    &mut mcp,
                    target,
                    engine,
                    &ready["browser"]["selectedContext"],
                    &ready["browser"]["selectedPage"],
                    &origin,
                    &proxy,
                    &override_proxy,
                    expected,
                )
                .await;
                for name in [
                    "plain",
                    "bypass",
                    "normalized",
                    "username_only",
                    "empty_password",
                ] {
                    let origin = Origin::start().await;
                    let proxy = tunnel::Proxy::start_forwarding(vec![origin.port], None).await;
                    let override_proxy = tunnel::Proxy::start_forwarding(
                        vec![origin.port],
                        Some(basic("Acme override", "override example")),
                    )
                    .await;
                    let request =
                        json!({"operation":"context_create","options":settings(name,&proxy)});
                    if engine == "webkit" && name == "username_only" {
                        collectors::refused(
                            &mut mcp,
                            target,
                            request,
                            "HTTP_PROXY_PASSWORD_REQUIRED",
                            true,
                        )
                        .await;
                        assert!(origin.records().is_empty());
                        assert!(proxy.requests().is_empty());
                        continue;
                    }
                    let context = operation(&mut mcp, target, request).await["context"].clone();
                    let page = operation(
                        &mut mcp,
                        target,
                        json!({"operation":"page_create","context":context,"select":false}),
                    )
                    .await["page"]
                        .clone();
                    let expected = report
                        .as_array()
                        .unwrap()
                        .iter()
                        .find(|row| row["name"] == name)
                        .unwrap();
                    exercise(
                        &mut mcp,
                        target,
                        engine,
                        &context,
                        &page,
                        &origin,
                        &proxy,
                        &override_proxy,
                        expected,
                    )
                    .await;
                }
            })
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
                failures.push(format!("{engine}/{mode}: {closed}"));
            }
            if let Err(panic) = scenario {
                mcp.stop().await;
                std::panic::resume_unwind(panic);
            }
            eprintln!("Context proxy {engine}/{mode}: six scenarios settled");
        }
    }
    mcp.stop().await;
    assert!(
        failures.is_empty(),
        "Original cleanup failures: {failures:?}"
    );
}

async fn reference(fixture: &Fixture, engine: &str, mode: &str) -> Value {
    let result = Command::new(
        fixture
            .cache
            .join("node-24.12.0/node-v24.12.0-darwin-x64/bin/node"),
    )
    .arg(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/browser-context-proxy-baseline.cjs"),
    )
    .arg(fixture.cache.join("playwright-core-1.64.1/package"))
    .arg(engine)
    .arg(mode)
    .arg(fixture.cache.join("browsers"))
    .current_dir(fixture.workspace.path())
    .env_clear()
    .env("PATH", "/usr/bin:/bin")
    .env("PW_LANG_NAME", "javascript")
    .env("PLAYWRIGHT_BROWSERS_PATH", fixture.cache.join("browsers"))
    .output()
    .await
    .unwrap();
    std::fs::write(
        fixture
            .workspace
            .path()
            .join(format!("context-proxy-sdk-{engine}-{mode}.json")),
        &result.stdout,
    )
    .unwrap();
    std::fs::write(
        fixture
            .workspace
            .path()
            .join(format!("context-proxy-sdk-{engine}-{mode}.stderr")),
        &result.stderr,
    )
    .unwrap();
    let report: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(report.as_array().unwrap().len(), 6);
    assert_eq!(report[5]["name"], "empty_password");
    assert_eq!(
        report[5]["pageStatus"], 200,
        "{engine}/{mode}: {}",
        report[5]
    );
    assert_eq!(
        report[5]["fetchStatus"], 200,
        "{engine}/{mode}: {}",
        report[5]
    );
    let username_only = &report[4];
    assert_eq!(username_only["name"], "username_only");
    if engine == "webkit" {
        assert!(
            !result.status.success(),
            "Pinned WebKit's rejected credential initializer must retain its disposal failure"
        );
        assert_eq!(username_only["error"]["phase"], "context_create");
        assert_eq!(
            username_only["error"]["message"],
            "browser.newContext: options.httpCredentials[0].password: expected string, got undefined"
        );
        assert!(username_only["origin"].as_array().unwrap().is_empty());
        assert!(username_only["proxy"].as_array().unwrap().is_empty());
        let stderr = String::from_utf8_lossy(&result.stderr);
        assert!(
            stderr.contains("_BrowserContextDispatcher._onDispose"),
            "{stderr}"
        );
        assert!(
            stderr.contains("Acme SDK fatal cleanup {\"ownedProcesses\":0}"),
            "{stderr}"
        );
    } else {
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert_eq!(username_only["pageStatus"], 200, "{username_only}");
        assert_eq!(username_only["fetchStatus"], 200, "{username_only}");
    }
    for row in report.as_array().unwrap().iter().take(4) {
        assert_eq!(row["pageStatus"], 200, "{engine}/{mode}: {row}");
        assert_eq!(row["fetchStatus"], 200, "{engine}/{mode}: {row}");
        assert!(row["error"].is_null(), "{engine}/{mode}: {row}");
    }
    for row in report
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["error"].is_null())
    {
        assert_eq!(row["overrideStatus"], 200, "{engine}/{mode}: {row}");
        assert_eq!(row["closedRejected"], true, "{engine}/{mode}: {row}");
        assert_eq!(row["overrideProxy"].as_array().unwrap().len(), 2, "{row}");
    }
    report
}

#[tokio::test]
async fn step_four_context_proxy_reference_diagnostic() {
    let fixture = installed_fixture();
    for mode in ["headless", "headed"] {
        for engine in ["chrome", "firefox", "webkit"] {
            let report = reference(&fixture, engine, mode).await;
            eprintln!(
                "Pinned context proxy {engine}/{mode}: {} scenarios recorded",
                report.as_array().unwrap().len()
            );
        }
    }
}
