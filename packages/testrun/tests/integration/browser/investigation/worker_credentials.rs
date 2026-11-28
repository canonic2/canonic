//! Installed dedicated-worker credentials and context headers; shared workers
//! keep the pinned Chromium boundary.
use super::*;
#[path = "../../../fixtures/browser_http_defaults.rs"]
mod wire;

/// One long-lived blob worker per page answers each posted URL with its status.
async fn dedicated_fetch(mcp: &mut Mcp, target: &str, page: &Value, url: &str) -> i64 {
    let expression = "async url => {const source='onmessage=async e=>{try{const r=await fetch(e.data);postMessage(r.status)}catch(x){postMessage(-1)}}';globalThis.AcmeWorker??=new Worker(URL.createObjectURL(new Blob([source],{type:'text/javascript'})));return await new Promise(resolve=>{globalThis.AcmeWorker.onmessage=e=>resolve(e.data);globalThis.AcmeWorker.postMessage(url);});}";
    let result = operation(mcp,target,json!({"operation":"evaluate","scope":page,"isFunction":true,"expression":expression,"argument":{"kind":"json","value":url}})).await;
    result["value"]["value"]
        .as_i64()
        .unwrap_or_else(|| panic!("{result}"))
}
async fn shared_fetch(mcp: &mut Mcp, target: &str, page: &Value, url: &str) -> i64 {
    let expression = "async url => {const source='onconnect=e=>{const p=e.ports[0];p.onmessage=async m=>{try{const r=await fetch(m.data);p.postMessage(r.status)}catch(x){p.postMessage(-1)}}}';const worker=new SharedWorker(URL.createObjectURL(new Blob([source],{type:'text/javascript'})));return await new Promise(resolve=>{worker.port.onmessage=e=>resolve(e.data);worker.port.postMessage(url);});}";
    let result = operation(mcp,target,json!({"operation":"evaluate","scope":page,"isFunction":true,"expression":expression,"argument":{"kind":"json","value":url}})).await;
    result["value"]["value"]
        .as_i64()
        .unwrap_or_else(|| panic!("{result}"))
}
fn last(site: &wire::Site, path: &str) -> Value {
    site.records()
        .into_iter()
        .filter(|record| record["path"] == path)
        .last()
        .unwrap_or_else(|| panic!("Missing worker request {path}"))
}
async fn cell(mcp: &mut Mcp, target: &str, ready: &Value, site: &wire::Site, engine: &str) {
    let context = &ready["browser"]["selectedContext"];
    let page = &ready["browser"]["selectedPage"];
    operation(mcp,target,json!({"operation":"page_navigate","page":page,"navigation":{"kind":"goto","url":format!("{}/header-page?workers",site.url)}})).await;
    let path = "/auth/basic?worker=dedicated";
    assert_eq!(
        dedicated_fetch(mcp, target, page, &format!("{}{path}", site.url)).await,
        200,
        "{engine}: dedicated workers answer challenges with context credentials"
    );
    let record = last(site, path);
    assert_eq!(record["headers"]["authorization"], "Basic QWNtZTpwYXNz");
    assert_eq!(record["headers"]["x-acme"], "worker", "{record}");
    operation(mcp,target,json!({"operation":"context_configure","context":context,"setting":{"kind":"extra_http_headers","headers":[{"name":"X-Acme","value":"updated"}]}})).await;
    let updated = "/echo?worker=dedicated-updated";
    assert_eq!(
        dedicated_fetch(mcp, target, page, &format!("{}{updated}", site.url)).await,
        200
    );
    assert_eq!(
        last(site, updated)["headers"]["x-acme"],
        "updated",
        "{engine}: a running dedicated worker follows context header updates"
    );
    if engine == "chrome" {
        // Pinned Chromium detaches shared workers: no context headers.
        let shared = "/echo?worker=shared";
        assert_eq!(
            shared_fetch(mcp, target, page, &format!("{}{shared}", site.url)).await,
            200
        );
        assert!(
            last(site, shared)["headers"]["x-acme"].is_null(),
            "Shared workers must not receive context headers"
        );
    }
}

#[tokio::test]
async fn installed_step_four_worker_credentials_checkpoint() {
    let fixture = installed_fixture();
    let mut mcp = Mcp::start(&fixture).await;
    let site = wire::Site::start(None).await;
    let mut failures = vec![];
    for mode in ["headless", "headed"] {
        for engine in ["chrome", "firefox", "webkit"] {
            let connected = mcp
                .tool(
                    "browser_connect",
                    json!({"engine":engine,"mode":mode,"context":{"viewport":{"width":768,"height":512},"deviceScaleFactor":1,"httpCredentials":{"origin":site.url,"username":"Acme","password":"pass"},"extraHTTPHeaders":[{"name":"X-Acme","value":"worker"}]}}),
                )
                .await;
            let target = connected["targetId"].as_str().unwrap();
            let ready = fixture.ready_target(target).await;
            let result =
                std::panic::AssertUnwindSafe(cell(&mut mcp, target, &ready, &site, engine))
                    .catch_unwind()
                    .await;
            std::fs::write(
                fixture
                    .workspace
                    .path()
                    .join(format!("worker-credentials-wire-{engine}-{mode}.json")),
                serde_json::to_vec_pretty(&site.records()).unwrap(),
            )
            .unwrap();
            if result.is_err() {
                let status = mcp.tool("browser_status", json!({"targetId":target})).await;
                std::fs::write(
                    fixture
                        .workspace
                        .path()
                        .join(format!("worker-credentials-failure-{engine}-{mode}.json")),
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
                failures.push(format!("{engine}/{mode}: {closed}"));
            }
            if let Err(panic) = result {
                mcp.stop().await;
                std::panic::resume_unwind(panic);
            }
        }
    }
    mcp.stop().await;
    assert!(
        failures.is_empty(),
        "Original cleanup failures: {failures:?}"
    );
}
