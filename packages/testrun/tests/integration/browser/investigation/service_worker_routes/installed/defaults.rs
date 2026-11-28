//! Context headers and credential lifetime on real owned service-worker traffic.
use super::*;
use base64::Engine;

struct DefaultsSite {
    url: String,
    wire: Arc<Mutex<Vec<Value>>>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for DefaultsSite {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl DefaultsSite {
    async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let wire = Arc::new(Mutex::new(Vec::new()));
        let records = wire.clone();
        let task = tokio::spawn(async move {
            let mut requests = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    accepted=listener.accept(),if requests.len()<32=>{
                        let (mut stream,_)=accepted.unwrap();let records=records.clone();requests.spawn(async move {
                            let mut head=vec![];let mut chunk=[0;1024];
                            while head.len()<8192&&!head.windows(4).any(|part|part==b"\r\n\r\n") {let count=stream.read(&mut chunk).await.unwrap();if count==0{return;}head.extend_from_slice(&chunk[..count]);}
                            let request=String::from_utf8(head).unwrap();let path=request.lines().next().unwrap().split_whitespace().nth(1).unwrap();
                            let headers=request.lines().skip(1).filter_map(|line|line.split_once(':')).map(|(name,value)|(name.to_ascii_lowercase(),value.trim())).collect::<std::collections::BTreeMap<_,_>>();
                            let (status,kind,extra,body)=if path=="/acme-sw.js" {(200,"application/javascript",String::new(),WORKER_SCRIPT.into())}
                            else if path.starts_with("/wire/") {
                                let wanted=if path=="/wire/auth-first"{"first"}else{"second"};
                                let expected=format!("Basic {}",base64::engine::general_purpose::STANDARD.encode(format!("Acme:{wanted}")));
                                let authorization=headers.get("authorization");let authorized=authorization.is_some_and(|value|*value==expected);
                                let row=json!({"path":path,"context":headers.get("x-acme-context"),"page":headers.get("x-acme-page"),"authorizationPresent":authorization.is_some(),"authorized":authorized});
                                records.lock().unwrap().push(row.clone());
                                if path.starts_with("/wire/auth-")&&!authorized {(401,"text/plain",format!("WWW-Authenticate: Basic realm=\"Acme {path}\"\r\n"),"Acme unauthorized".into())}
                                else {(200,"application/json",String::new(),row.to_string())}
                            }else{(200,"text/html",String::new(),"<!doctype html><title>Acme worker defaults</title>".into())};
                            let response=format!("HTTP/1.1 {status} {}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nCache-Control: no-store\r\n{extra}Connection: close\r\n\r\n{body}",if status==200{"OK"}else{"Unauthorized"},body.len());let _=stream.write_all(response.as_bytes()).await;
                        });
                    },
                    Some(result)=requests.join_next(),if !requests.is_empty()=>{result.unwrap();},
                }
            }
        });
        Self { url, wire, task }
    }
}

async fn response(mcp: &mut Mcp, target: &str, scope: &Value, path: &str) -> Value {
    let encoded=evaluate(mcp,target,scope,&format!("async()=>{{const response=await fetch({});return JSON.stringify({{status:response.status,text:await response.text()}});}}",json!(path))).await;
    serde_json::from_str(encoded.as_str().expect("Response scalar JSON")).unwrap()
}

fn same_response(actual: &Value, expected: &Value) {
    assert_eq!(
        actual["status"], expected["status"],
        "actual {actual}; expected {expected}"
    );
    if actual["status"] == 200 {
        let actual: Value = serde_json::from_str(actual["text"].as_str().unwrap()).unwrap();
        let expected: Value = serde_json::from_str(expected["text"].as_str().unwrap()).unwrap();
        assert_eq!(actual, expected);
    } else {
        assert_eq!(actual["text"], expected["text"]);
    }
}

async fn cell(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    engine: &str,
    mode: &str,
    site: &DefaultsSite,
    reference: &Value,
) {
    for (index, name) in ["headers", "credentials"].into_iter().enumerate() {
        let mut options = if name == "headers" {
            json!({"extraHTTPHeaders":[{"name":"X-Acme-Context","value":"initial"}]})
        } else {
            json!({"httpCredentials":{"username":"Acme","password":"first","origin":site.url}})
        };
        options["viewport"] = json!({"width":640,"height":480});
        options["deviceScaleFactor"] = json!(1);
        let context = deliver(
            fixture,
            mcp,
            target,
            json!({"operation":"context_create","options":options}),
            "cli",
        )
        .await["context"]
            .clone();
        let page = operation(
            mcp,
            target,
            json!({"operation":"page_create","context":context,"select":false}),
        )
        .await["page"]
            .clone();
        operation(mcp,target,json!({"operation":"page_set_extra_http_headers","page":page,"headers":[{"name":"X-Acme-Page","value":"page-only"}]})).await;
        operation(mcp,target,json!({"operation":"page_navigate","page":page,"navigation":{"kind":"goto","url":site.url,"waitUntil":"load"}})).await;
        assert_eq!(evaluate(mcp,target,&page,"async()=>{await navigator.serviceWorker.register('/acme-sw.js');await navigator.serviceWorker.ready;return true;}").await,true);
        if engine != "chrome" {
            let inventory = operation(
                mcp,
                target,
                json!({"operation":"context_worker_list","context":context}),
            )
            .await;
            assert!(
                inventory["workers"].as_array().unwrap().is_empty(),
                "{inventory}"
            );
            operation(
                mcp,
                target,
                json!({"operation":"context_close","context":context}),
            )
            .await;
            continue;
        }
        let service = worker(mcp, target, &context, &format!("{}/acme-sw.js", site.url)).await;
        deliver(
            fixture,
            mcp,
            target,
            json!({"operation":"page_close","page":page}),
            "act",
        )
        .await;
        let mut row = json!({"name":name});
        let expected = &reference["cases"][index];
        row["initial"] = response(
            mcp,
            target,
            &service,
            if name == "headers" {
                "/wire/headers-initial"
            } else {
                "/wire/auth-first"
            },
        )
        .await;
        same_response(&row["initial"], &expected["initial"]);
        let setting = if name == "headers" {
            json!({"kind":"extra_http_headers","headers":[{"name":"X-Acme-Context","value":"changed"}]})
        } else {
            json!({"kind":"http_credentials","credentials":{"username":"Acme","password":"second","origin":site.url}})
        };
        let receipt = deliver(
            fixture,
            mcp,
            target,
            json!({"operation":"context_configure","context":context,"setting":setting}),
            if name == "headers" { "cli" } else { "act" },
        )
        .await;
        if name == "credentials" {
            assert_eq!(receipt["serviceWorkers"], true);
            assert_eq!(receipt["workers"], false);
        }
        row["changed"] = response(
            mcp,
            target,
            &service,
            if name == "headers" {
                "/wire/headers-changed"
            } else {
                "/wire/auth-second"
            },
        )
        .await;
        same_response(&row["changed"], &expected["changed"]);
        let setting = if name == "headers" {
            json!({"kind":"extra_http_headers","headers":[]})
        } else {
            json!({"kind":"http_credentials","credentials":null})
        };
        deliver(
            fixture,
            mcp,
            target,
            json!({"operation":"context_configure","context":context,"setting":setting}),
            if name == "headers" { "act" } else { "mcp" },
        )
        .await;
        row["cleared"] = response(
            mcp,
            target,
            &service,
            if name == "headers" {
                "/wire/headers-cleared"
            } else {
                "/wire/auth-cleared"
            },
        )
        .await;
        same_response(&row["cleared"], &expected["cleared"]);
        let status = mcp.detailed_browser_status(target).await;
        assert_eq!(
            status["browser"]["callbacks"],
            json!([]),
            "Automatic authentication must not create application callbacks: {status}"
        );
        assert_eq!(status["browser"]["phase"], "ready");
        operation(
            mcp,
            target,
            json!({"operation":"context_close","context":context}),
        )
        .await;
        std::fs::write(fixture.workspace.path().join(format!("worker-defaults-installed-{engine}-{mode}-{name}.json")),serde_json::to_vec_pretty(&json!({"actual":row,"pinnedSdk":expected,"wire":site.wire.lock().unwrap().clone()})).unwrap()).unwrap();
    }
    assert_eq!(
        json!(site.wire.lock().unwrap().clone()),
        reference["wire"],
        "Exact owned origin observations must match the pinned SDK, including cached authentication"
    );
    if engine == "chrome" {
        manual_authentication(fixture, mcp, target, mode, site, reference).await;
    }
}

async fn manual_authentication(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    mode: &str,
    site: &DefaultsSite,
    reference: &Value,
) {
    let context=operation(mcp,target,json!({"operation":"context_create","options":{"viewport":{"width":640,"height":480},"deviceScaleFactor":1,"httpCredentials":{"username":"Acme","password":"first","origin":site.url}}})).await["context"].clone();
    let page = operation(
        mcp,
        target,
        json!({"operation":"page_create","context":context,"select":false}),
    )
    .await["page"]
        .clone();
    deliver(fixture,mcp,target,json!({"operation":"context_route_set","context":context,"url":format!("{}/wire/auth-manual",site.url),"authenticationChallenges":true}),"cli").await;
    operation(mcp,target,json!({"operation":"page_navigate","page":page,"navigation":{"kind":"goto","url":site.url,"waitUntil":"load"}})).await;
    assert_eq!(evaluate(mcp,target,&page,"async()=>{await navigator.serviceWorker.register('/acme-sw.js');await navigator.serviceWorker.ready;return true;}").await,true);
    let service = worker(mcp, target, &context, &format!("{}/acme-sw.js", site.url)).await;
    operation(mcp, target, json!({"operation":"page_close","page":page})).await;
    let (held, first) = pending(
        mcp,
        target,
        &service,
        "/wire/auth-manual",
        &context,
        &Value::Null,
        &service,
    )
    .await;
    deliver(fixture,mcp,target,json!({"operation":"callback_resolve","callback":first,"response":{"kind":"route_continue"}}),"cli").await;
    let callback = har_interleave::callback(mcp, target, &context, &Value::Null).await;
    let status = mcp.detailed_browser_status(target).await;
    let route = status["browser"]["callbacks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|route| route["reference"] == callback)
        .unwrap();
    assert_eq!(route["details"]["phase"], "authentication");
    assert_eq!(route["worker"], service);
    assert!(route["page"].is_null());
    assert!(!route.to_string().contains("password"));
    deliver(fixture,mcp,target,json!({"operation":"callback_resolve","callback":callback,"response":{"kind":"auth_credentials","username":"Acme","password":"second"}}),"act").await;
    let result = completed(mcp, target, held).await["value"]["value"].clone();
    let body: Value = serde_json::from_str(result.as_str().unwrap()).unwrap();
    assert_eq!(
        body["authorized"], true,
        "The caller's credentials must win over inherited defaults"
    );
    assert!(body["page"].is_null());
    let mut expected = reference["wire"].as_array().unwrap().clone();
    expected.extend([
        json!({"path":"/wire/auth-manual","context":null,"page":null,"authorizationPresent":false,"authorized":false}),
        json!({"path":"/wire/auth-manual","context":null,"page":null,"authorizationPresent":true,"authorized":true}),
    ]);
    assert_eq!(site.wire.lock().unwrap().clone(), expected);
    operation(
        mcp,
        target,
        json!({"operation":"context_close","context":context}),
    )
    .await;
    std::fs::write(fixture.workspace.path().join(format!("worker-defaults-manual-extension-{mode}.json")),serde_json::to_vec_pretty(&json!({"extension":"manual-authentication-precedence","pinnedSdkUnavailable":true,"body":body,"wire":site.wire.lock().unwrap().clone()})).unwrap()).unwrap();
}

#[tokio::test]
async fn installed_step_four_service_worker_defaults_checkpoint() {
    let fixture = installed_fixture();
    let mut mcp = Mcp::start(&fixture).await;
    let mut failures = vec![];
    for mode in ["headless", "headed"] {
        for engine in ["chrome", "firefox", "webkit"] {
            let reference = crate::browser::investigation::service_worker_defaults::reference(
                &fixture, engine, mode,
            )
            .await;
            let site = DefaultsSite::start().await;
            let doctor = mcp.tool("browser_doctor", json!({"engine":engine})).await;
            assert_eq!(
                doctor["capabilities"]["routing"]["serviceWorkers"],
                engine == "chrome"
            );
            if engine == "chrome" {
                assert_eq!(
                    doctor["capabilities"]["routing"]["serviceWorkerAuthentication"],
                    true
                );
                assert_eq!(
                    doctor["capabilities"]["settings"]["authentication"]["serviceWorkers"],
                    true
                );
                assert_eq!(
                    doctor["capabilities"]["settings"]["authentication"]["workers"],
                    true
                );
                assert_eq!(
                    doctor["capabilities"]["settings"]["authentication"]["sharedWorkers"],
                    false
                );
            }
            let connected = fixture
                .cli(&["browser", "connect", "--engine", engine, "--mode", mode])
                .await;
            let target = connected["targetId"].as_str().unwrap();
            let ready = fixture.ready_target(target).await;
            let result = std::panic::AssertUnwindSafe(cell(
                &fixture, &mut mcp, target, engine, mode, &site, &reference,
            ))
            .catch_unwind()
            .await;
            if result.is_err() {
                let status = mcp.tool("browser_status", json!({"targetId":target})).await;
                std::fs::write(
                    fixture
                        .workspace
                        .path()
                        .join(format!("worker-defaults-failure-{engine}-{mode}.json")),
                    serde_json::to_vec_pretty(
                        &json!({"status":status,"wire":site.wire.lock().unwrap().clone()}),
                    )
                    .unwrap(),
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
            if let Err(panic) = result {
                fixture.cli(&["disconnect"]).await;
                mcp.tool("disconnect", json!({})).await;
                mcp.stop().await;
                std::panic::resume_unwind(panic);
            }
            eprintln!(
                "Installed worker defaults {engine}/{mode}: headers, credentials, cache observations and original cleanup"
            );
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
