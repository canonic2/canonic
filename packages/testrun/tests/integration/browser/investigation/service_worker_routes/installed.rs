//! Native context/worker authority is compared with independent pinned SDK effects.
use super::*;
mod capacity;
mod capture;
mod defaults;
mod har;
use std::sync::{Arc, Mutex};
use tokio::io::AsyncWriteExt;

const WORKER_SCRIPT: &str = "self.addEventListener('install',event=>event.waitUntil(self.skipWaiting()));self.addEventListener('activate',event=>event.waitUntil(self.clients.claim()));self.addEventListener('fetch',event=>{const url=new URL(event.request.url);if(url.pathname.startsWith('/controlled/'))event.respondWith(fetch('/wire/'+url.pathname.slice('/controlled/'.length)));});";
struct WorkerSite {
    url: String,
    wire: Arc<Mutex<Vec<String>>>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for WorkerSite {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl WorkerSite {
    async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let wire = Arc::new(Mutex::new(vec![]));
        let records = wire.clone();
        let task = tokio::spawn(async move {
            let mut requests = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    accepted=listener.accept(),if requests.len()<32=>{
                        let (mut stream,_)=accepted.unwrap();let wire=records.clone();requests.spawn(async move {
                            let mut head=vec![];let mut chunk=[0;1024];
                            while head.len()<8192&&!head.windows(4).any(|part|part==b"\r\n\r\n") {let count=stream.read(&mut chunk).await.unwrap();if count==0{return;}head.extend_from_slice(&chunk[..count]);}
                            let request=String::from_utf8(head).unwrap();let path=request.lines().next().unwrap().split_whitespace().nth(1).unwrap();
                            let (kind,body)=if path=="/acme-sw.js" {("application/javascript",WORKER_SCRIPT.into())}else if path.starts_with("/wire/"){wire.lock().unwrap().push(path.into());("text/plain",format!("Acme wire {path}"))}else{("text/html","<!doctype html><title>Acme worker routes</title>".into())};
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
async fn deliver(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    mut request: Value,
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
            if request["operation"] == "network_route" {
                let mut input = request.as_object_mut().unwrap().remove("action").unwrap();
                input["action"] = input.as_object_mut().unwrap().remove("kind").unwrap();
                request["input"] = input;
            }
            request["action"] = request["operation"].clone();
            request.as_object_mut().unwrap().remove("operation");
            let submitted = mcp
                .tool("act", json!({"targetId":target,"actions":[request]}))
                .await;
            completed(mcp, target, submitted).await["results"][0].clone()
        }
        _ => operation(mcp, target, request).await,
    }
}
fn fetch(scope: &Value, path: &str) -> Value {
    json!({"operation":"evaluate","scope":scope,"expression":format!("async()=>await(await fetch({})).text()",json!(path)),"isFunction":true})
}
async fn evaluate(mcp: &mut Mcp, target: &str, scope: &Value, expression: &str) -> Value {
    operation(
        mcp,
        target,
        json!({"operation":"evaluate","scope":scope,"expression":expression,"isFunction":true}),
    )
    .await["value"]["value"]
        .clone()
}
fn add(scope: &Value, pattern: &str) -> Value {
    json!({"operation":"network_route","scope":scope,"action":{"kind":"add_matcher","matcher":{"kind":"glob","pattern":pattern}}})
}
async fn worker(mcp: &mut Mcp, target: &str, context: &Value, script: &str) -> Value {
    let until = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let inventory = operation(
            mcp,
            target,
            json!({"operation":"context_worker_list","context":context}),
        )
        .await;
        if let Some(row) = inventory["workers"].as_array().unwrap().iter().find(|row| {
            row["kind"] == "service"
                && row["executionAvailable"] == true
                && row["context"] == *context
                && row["url"] == script
        }) {
            assert_eq!(row["context"], *context);
            assert!(row["page"].is_null());
            return row["worker"].clone();
        }
        assert!(
            tokio::time::Instant::now() < until,
            "Owned service-worker world unavailable: {inventory}"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}
async fn pending(
    mcp: &mut Mcp,
    target: &str,
    scope: &Value,
    path: &str,
    context: &Value,
    handler: &Value,
    worker: &Value,
) -> (Value, Value) {
    let submitted = mcp
        .tool(
            "browser_send",
            json!({"targetId":target,"request":fetch(scope,path)}),
        )
        .await;
    let callback = har_interleave::callback(mcp, target, context, handler).await;
    let status = mcp.detailed_browser_status(target).await;
    let route = status["browser"]["callbacks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|route| route["reference"] == callback)
        .unwrap();
    assert!(
        route["page"].is_null(),
        "A worker request has no page authority: {route}"
    );
    assert_eq!(route["worker"], *worker, "{route}");
    (submitted, callback)
}
async fn fulfill(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    callback: &Value,
    body: &str,
    style: &str,
) {
    deliver(fixture,mcp,target,json!({"operation":"callback_resolve","callback":callback,"response":{"kind":"route_fulfill","status":200,"body":body,"contentType":"text/plain"}}),style).await;
}
async fn routed(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    scope: &Value,
    path: &str,
    context: &Value,
    handler: &Value,
    worker: &Value,
    body: &str,
    style: &str,
) -> Value {
    let (submitted, callback) = pending(mcp, target, scope, path, context, handler, worker).await;
    fulfill(fixture, mcp, target, &callback, body, style).await;
    completed(mcp, target, submitted).await["value"]["value"].clone()
}

async fn cell(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    engine: &str,
    mode: &str,
    site: &WorkerSite,
    baseline: &Value,
) {
    for name in ["ownership-removal", "context-close"] {
        let expected = baseline["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["name"] == name)
            .unwrap();
        let context =
            operation(mcp, target, json!({"operation":"context_create"})).await["context"].clone();
        let page = operation(
            mcp,
            target,
            json!({"operation":"page_create","context":context,"select":false}),
        )
        .await["page"]
            .clone();
        let pattern = format!("{}/wire/*", site.url);
        let first =
            deliver(fixture, mcp, target, add(&context, &pattern), "cli").await["handlerId"]
                .clone();
        assert!(first.is_string());
        operation(mcp, target, add(&page, &pattern)).await;
        deliver(
            fixture,
            mcp,
            target,
            add(&page, &format!("{}/controlled/*", site.url)),
            "act",
        )
        .await;
        operation(mcp,target,json!({"operation":"page_navigate","page":page,"navigation":{"kind":"goto","url":site.url,"waitUntil":"load"}})).await;
        assert_eq!(evaluate(mcp,target,&page,"async()=>{await navigator.serviceWorker.register('/acme-sw.js');await navigator.serviceWorker.ready;if(!navigator.serviceWorker.controller)await new Promise(resolve=>navigator.serviceWorker.addEventListener('controllerchange',resolve,{once:true}));return true;}").await,true);
        if engine != "chrome" {
            assert_eq!(
                operation(mcp, target, fetch(&page, "/controlled/first")).await["value"]["value"],
                expected["first"]
            );
            let status = mcp.detailed_browser_status(target).await;
            assert!(
                status["browser"]["resources"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|resource| resource["kind"] != "route"),
                "The SDK surface exposes no worker route callback: {status}"
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
        let first_body = routed(
            fixture,
            mcp,
            target,
            &page,
            "/controlled/first",
            &context,
            &first,
            &service,
            "Acme first",
            "act",
        )
        .await;
        assert_eq!(first_body, expected["first"]);
        operation(mcp, target, json!({"operation":"page_close","page":page})).await;
        let after_page = routed(
            fixture,
            mcp,
            target,
            &service,
            "/wire/after-page-close",
            &context,
            &first,
            &service,
            "Acme first",
            "cli",
        )
        .await;
        assert_eq!(after_page, expected["afterPageClose"]);
        let mut row = json!({"name":name,"first":first_body,"afterPageClose":after_page});
        if name == "ownership-removal" {
            let second =
                deliver(fixture, mcp, target, add(&context, &pattern), "act").await["handlerId"]
                    .clone();
            row["replaced"] = routed(
                fixture,
                mcp,
                target,
                &service,
                "/wire/replaced",
                &context,
                &second,
                &service,
                "Acme second",
                "mcp",
            )
            .await;
            assert_eq!(row["replaced"], expected["replaced"]);
            deliver(fixture,mcp,target,json!({"operation":"network_route","scope":context,"action":{"kind":"remove","handlerId":second}}),"cli").await;
            row["restored"] = routed(
                fixture,
                mcp,
                target,
                &service,
                "/wire/restored",
                &context,
                &first,
                &service,
                "Acme first",
                "act",
            )
            .await;
            assert_eq!(row["restored"], expected["restored"]);
            let (held, callback) = pending(
                mcp,
                target,
                &service,
                "/wire/held",
                &context,
                &first,
                &service,
            )
            .await;
            har_replay_filter::refuses(
                mcp,
                target,
                add(&context, &format!("{}/wire/extra", site.url)),
                "ROUTE_CALLBACKS_PENDING",
            )
            .await;
            let edit = mcp.tool("act",json!({"targetId":target,"actions":[{"action":"network_route","scope":context,"input":{"action":"add_matcher","matcher":{"kind":"glob","pattern":format!("{}/wire/extra",site.url)}}}]})).await;
            let refused = mcp
                .tool(
                    "browser_status",
                    json!({"targetId":target,"operationId":edit["operation"]["id"],"waitMs":1000}),
                )
                .await;
            assert_eq!(
                refused["operation"]["settled"], true,
                "Shared policy edits must reach their pending-callback guard: {refused}"
            );
            assert_eq!(refused["operation"]["code"], "ROUTE_CALLBACKS_PENDING");
            let clearing=mcp.tool("act",json!({"targetId":target,"actions":[{"action":"network_route","scope":context,"input":{"action":"clear","behavior":"wait"}}]})).await;
            let waiting=mcp.tool("browser_status",json!({"targetId":target,"operationId":clearing["operation"]["id"],"waitMs":50})).await;
            assert_eq!(
                waiting["operation"]["settled"], false,
                "Clear-wait must retain the original worker callback: {waiting}"
            );
            let status = mcp.detailed_browser_status(target).await;
            assert!(
                status["browser"]["callbacks"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|route| route["reference"] == callback)
            );
            fulfill(fixture, mcp, target, &callback, "Acme first", "cli").await;
            row["held"] = completed(mcp, target, held).await["value"]["value"].clone();
            assert_eq!(row["held"], expected["held"]["value"]);
            completed(mcp, target, clearing).await;
            row["afterRemoval"] = operation(mcp, target, fetch(&service, "/wire/after-removal"))
                .await["value"]["value"]
                .clone();
            assert_eq!(row["afterRemoval"], expected["afterRemoval"]);
            operation(
                mcp,
                target,
                json!({"operation":"context_close","context":context}),
            )
            .await;
        } else {
            let (held, callback) = pending(
                mcp,
                target,
                &service,
                "/wire/held",
                &context,
                &first,
                &service,
            )
            .await;
            deliver(
                fixture,
                mcp,
                target,
                json!({"operation":"context_close","context":context}),
                "act",
            )
            .await;
            let status = mcp
                .tool(
                    "browser_status",
                    json!({"targetId":target,"operationId":held["operation"]["id"],"waitMs":30000}),
                )
                .await;
            row["pending"] = status["operation"].clone();
            std::fs::write(
                fixture
                    .workspace
                    .path()
                    .join(format!("worker-route-pending-{engine}-{mode}.json")),
                serde_json::to_vec_pretty(&status).unwrap(),
            )
            .unwrap();
            assert_eq!(row["pending"]["settled"], true, "{status}");
            assert_eq!(
                row["pending"]["code"], "STALE_REFERENCE",
                "Closed worker execution must retain its typed scope failure: {status}"
            );
            assert_eq!(row["pending"]["outcome"], "unknown");
            assert_eq!(row["pending"]["result"]["callerEffectsUnknown"], true);
            assert_eq!(row["pending"]["result"]["scopeClosed"], true);
            assert_eq!(status["browser"]["phase"], "ready");
            let sibling = status["browser"]["resources"]
                .as_array()
                .unwrap()
                .iter()
                .find(|resource| resource["kind"] == "page" && resource["parent"] != context)
                .unwrap()["reference"]
                .clone();
            assert_eq!(evaluate(mcp, target, &sibling, "()=>42").await, 42);
            // Testrun's existing typed-refusal contract stays explicit. The SDK
            // route object's successful late no-op is a separate unaccepted case.
            har_replay_filter::refuses(mcp,target,json!({"operation":"callback_resolve","callback":callback,"response":{"kind":"route_continue"}}),"STALE_REFERENCE").await;
            let terminal = mcp.detailed_browser_status(target).await;
            assert!(
                terminal["browser"]["callbacks"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|route| route["reference"] != callback),
                "Context teardown must retire its worker callback: {terminal}"
            );
        }
        std::fs::write(fixture.workspace.path().join(format!("worker-route-installed-{engine}-{mode}-{name}.json")),serde_json::to_vec_pretty(&json!({"actual":row,"pinnedSdk":expected,"wire":site.wire.lock().unwrap().clone()})).unwrap()).unwrap();
    }
    assert_eq!(
        json!(site.wire.lock().unwrap().clone()),
        baseline["wire"],
        "Only unhandled worker requests may reach the owned origin"
    );
}

#[tokio::test]
async fn installed_step_four_service_worker_routes_checkpoint() {
    let fixture = installed_fixture();
    let mut mcp = Mcp::start(&fixture).await;
    let mut failures = vec![];
    for mode in ["headless", "headed"] {
        for engine in ["chrome", "firefox", "webkit"] {
            let baseline = reference(&fixture, engine, mode).await;
            let site = WorkerSite::start().await;
            let doctor = mcp.tool("browser_doctor", json!({"engine":engine})).await;
            assert_eq!(
                doctor["capabilities"]["routing"]["serviceWorkers"],
                engine == "chrome"
            );
            if engine == "chrome" {
                assert_eq!(
                    doctor["capabilities"]["routing"]["serviceWorkerScope"],
                    "context-manual-and-har-routes"
                );
                assert_eq!(doctor["capabilities"]["routing"]["serviceWorkerHar"], true);
                assert_eq!(
                    doctor["capabilities"]["routing"]["serviceWorkerAuthentication"],
                    true
                );
            }
            let connected = fixture
                .cli(&["browser", "connect", "--engine", engine, "--mode", mode])
                .await;
            let target = connected["targetId"].as_str().unwrap();
            let ready = fixture.ready_target(target).await;
            let result = std::panic::AssertUnwindSafe(cell(
                &fixture, &mut mcp, target, engine, mode, &site, &baseline,
            ))
            .catch_unwind()
            .await;
            if result.is_err() {
                let status = mcp.tool("browser_status", json!({"targetId":target})).await;
                std::fs::write(
                    fixture
                        .workspace
                        .path()
                        .join(format!("worker-route-failure-{engine}-{mode}.json")),
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
                failures.push(format!("{engine}/{mode}: {closed}"));
            }
            if let Err(panic) = result {
                mcp.stop().await;
                std::panic::resume_unwind(panic);
            }
            eprintln!(
                "Installed service-worker routing {engine}/{mode}: context ownership, policy lifetime and original cleanup"
            );
        }
    }
    mcp.stop().await;
    assert!(
        failures.is_empty(),
        "Original cleanup failures: {failures:?}"
    );
}
