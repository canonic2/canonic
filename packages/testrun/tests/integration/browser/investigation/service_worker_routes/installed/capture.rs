//! Public capture ownership and replay/default coexistence on native workers.
use super::*;

struct CaptureSite {
    url: String,
    wire: Arc<Mutex<Vec<Value>>>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for CaptureSite {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl CaptureSite {
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
                        let (mut stream,_)=accepted.unwrap();let wire=records.clone();requests.spawn(async move{
                            let mut head=vec![];let mut chunk=[0;1024];
                            while head.len()<8192&&!head.windows(4).any(|part|part==b"\r\n\r\n"){let count=stream.read(&mut chunk).await.unwrap();if count==0{return;}head.extend_from_slice(&chunk[..count]);}
                            let request=String::from_utf8(head).unwrap();let path=request.lines().next().unwrap().split_whitespace().nth(1).unwrap();
                            let headers=request.lines().skip(1).filter_map(|line|line.split_once(':')).map(|(name,value)|(name.to_ascii_lowercase(),value.trim())).collect::<std::collections::BTreeMap<_,_>>();
                            let (status,kind,extra,body)=if path=="/acme-sw.js"{(200,"application/javascript",String::new(),WORKER_SCRIPT.into())}
                            else if path.starts_with("/wire/"){
                                let authorization=headers.get("authorization");let authorized=authorization==Some(&"Basic QWNtZTpmaXJzdA==");
                                wire.lock().unwrap().push(json!({"path":path,"context":headers.get("x-acme-context"),"authorizationPresent":authorization.is_some(),"authorized":authorized}));
                                if path=="/wire/auth-first"&&!authorized{(401,"text/plain","WWW-Authenticate: Basic realm=\"Acme capture\"\r\n".into(),"Acme unauthorized".into())}
                                else{(200,"text/plain",String::new(),if path=="/wire/auth-first"{"Acme authenticated".into()}else{format!("Acme wire {path}")})}
                            }else{(200,"text/html",String::new(),"<!doctype html><title>Acme worker capture</title>".into())};
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

fn start(context: &Value, page: Option<&Value>, minimal: bool) -> Value {
    let mut request = json!({"operation":"har_capture_start","context":context,"content":"embed","mode":if minimal{"minimal"}else{"full"},"urlFilter":{"kind":"glob","pattern":"**/wire/*"}});
    if let Some(page) = page {
        request["page"] = page.clone();
    }
    request
}

fn entries(archive: &Value) -> Vec<Value> {
    archive["log"]["entries"].as_array().unwrap().iter().map(|entry|{
        let url=url::Url::parse(entry["request"]["url"].as_str().unwrap()).unwrap();
        let context=entry["request"]["headers"].as_array().unwrap().iter().find(|header|header["name"].as_str().unwrap().eq_ignore_ascii_case("x-acme-context")).map(|header|header["value"].clone()).unwrap_or(Value::Null);
        json!({"path":url.path(),"pageref":entry["pageref"],"workerRef":entry["_serviceWorkerRef"],"status":entry["response"]["status"],"contextHeader":context,"body":entry["response"]["content"]["text"],"encoding":entry["response"]["content"]["encoding"]})
    }).collect()
}

async fn cell(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    engine: &str,
    mode: &str,
    site: &CaptureSite,
    reference: &Value,
) {
    for (index, name) in [
        "future-full",
        "existing-minimal",
        "page-excluded",
        "coexistence",
    ]
    .into_iter()
    .enumerate()
    {
        let mut options = json!({"viewport":{"width":640,"height":480},"deviceScaleFactor":1,"extraHTTPHeaders":[{"name":"X-Acme-Context","value":"Acme capture"}]});
        if name == "coexistence" {
            options["httpCredentials"] =
                json!({"username":"Acme","password":"first","origin":site.url});
        }
        let context = deliver(
            fixture,
            mcp,
            target,
            json!({"operation":"context_create","options":options}),
            "cli",
        )
        .await["context"]
            .clone();
        if name == "future-full" && engine == "chrome" {
            deliver(fixture, mcp, target, start(&context, None, false), "cli").await;
        }
        let page = operation(
            mcp,
            target,
            json!({"operation":"page_create","context":context,"select":false}),
        )
        .await["page"]
            .clone();
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
        if name == "page-excluded" {
            deliver(
                fixture,
                mcp,
                target,
                start(&context, Some(&page), false),
                "act",
            )
            .await;
        } else {
            operation(mcp, target, json!({"operation":"page_close","page":page})).await;
            if name != "future-full" {
                deliver(
                    fixture,
                    mcp,
                    target,
                    start(&context, None, name == "existing-minimal"),
                    "mcp",
                )
                .await;
            }
        }
        let mut row = json!({"name":name});
        let mut replay = Value::Null;
        if name == "coexistence" {
            replay = har_replay_filter::archive(
                fixture,
                mcp,
                target,
                &[format!("{}/wire/replay-hit", site.url)],
                "Acme archive",
            )
            .await["archiveId"]
                .clone();
            deliver(fixture,mcp,target,json!({"operation":"context_har_set","context":context,"archiveId":replay,"notFound":"fallback","urlFilter":{"kind":"glob","pattern":"**/wire/replay*"}}),"cli").await;
            row["replayed"] =
                operation(mcp, target, fetch(&service, "/wire/replay-hit")).await["value"]["value"]
                    .clone();
            row["authenticated"] = operation(mcp, target, fetch(&service, "/wire/auth-first"))
                .await["value"]["value"]
                .clone();
            assert_eq!(row["replayed"], reference["cases"][index]["replayed"]);
            assert_eq!(
                row["authenticated"],
                reference["cases"][index]["authenticated"]
            );
        } else {
            row["fetched"] =
                operation(mcp, target, fetch(&service, "/wire/captured")).await["value"]["value"]
                    .clone();
            assert_eq!(row["fetched"], reference["cases"][index]["fetched"]);
        }
        let captured = deliver(
            fixture,
            mcp,
            target,
            json!({"operation":"har_capture_stop","context":context}),
            "cli",
        )
        .await;
        let bytes = std::fs::read(
            captured["reports"][0]["attachment"]["path"]
                .as_str()
                .unwrap(),
        )
        .unwrap();
        let archive: Value = serde_json::from_slice(&bytes).unwrap();
        let actual = entries(&archive);
        let expected = reference["cases"][index]["entries"].as_array().unwrap();
        assert_eq!(actual.len(), expected.len(), "{archive}");
        assert_eq!(
            archive["log"]["_testrunCapture"]["droppedEntries"], 0,
            "{archive}"
        );
        assert_eq!(
            archive["log"]["_testrunCapture"]["sourceGaps"], 0,
            "{archive}"
        );
        for (actual, expected) in actual.iter().zip(expected) {
            assert_eq!(actual["path"], expected["path"]);
            assert_eq!(actual["status"], expected["status"]);
            assert_eq!(actual["contextHeader"], expected["contextHeader"]);
            assert!(actual["pageref"].is_null());
            assert_eq!(
                actual["workerRef"],
                format!("worker@{}", service["id"].as_str().unwrap())
            );
            assert_eq!(
                crate::browser::investigation::service_worker_capture::body(actual),
                crate::browser::investigation::service_worker_capture::body(expected)
            );
        }
        if !replay.is_null() {
            operation(
                mcp,
                target,
                json!({"operation":"context_har_set","context":context,"archiveId":null}),
            )
            .await;
            operation(
                mcp,
                target,
                json!({"operation":"har_remove","archiveId":replay}),
            )
            .await;
        }
        operation(
            mcp,
            target,
            json!({"operation":"har_remove","archiveId":captured["archiveId"]}),
        )
        .await;
        operation(
            mcp,
            target,
            json!({"operation":"context_close","context":context}),
        )
        .await;
        std::fs::write(
            fixture.workspace.path().join(format!(
                "worker-capture-installed-{engine}-{mode}-{name}.har"
            )),
            bytes,
        )
        .unwrap();
        std::fs::write(fixture.workspace.path().join(format!("worker-capture-installed-{engine}-{mode}-{name}.json")),serde_json::to_vec_pretty(&json!({"actual":row,"entries":actual,"pinnedSdk":reference["cases"][index],"wire":site.wire.lock().unwrap().clone()})).unwrap()).unwrap();
    }
    assert_eq!(json!(site.wire.lock().unwrap().clone()), reference["wire"]);
}

#[tokio::test]
async fn installed_step_four_service_worker_capture_checkpoint() {
    let fixture = installed_fixture();
    let mut mcp = Mcp::start(&fixture).await;
    let mut failures = vec![];
    for mode in ["headless", "headed"] {
        for engine in ["chrome", "firefox", "webkit"] {
            let reference = crate::browser::investigation::service_worker_capture::reference(
                &fixture, engine, mode,
            )
            .await;
            let site = CaptureSite::start().await;
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
                        .join(format!("worker-capture-failure-{engine}-{mode}.json")),
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
                "Installed worker capture {engine}/{mode}: context identity, page exclusion, replay/default coexistence and original cleanup"
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
