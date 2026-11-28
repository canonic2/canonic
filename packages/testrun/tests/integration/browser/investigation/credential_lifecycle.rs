//! Initial launch aliases and mutable SDK credentials share exact HTTP snapshots.
use super::*;
#[path = "../../../fixtures/browser_http_defaults.rs"]
mod wire;

fn credentials(origin: &str, password: &str) -> Value {
    json!({"origin":origin,"username":"Acme","password":password})
}
async fn create(mcp: &mut Mcp, target: &str, options: Value, shared: bool) -> Value {
    if shared {
        let submitted=mcp.tool("act",json!({"targetId":target,"actions":[{"action":"http_context_create","options":options}]})).await;
        completed(mcp, target, submitted).await["results"][0]["httpContext"].clone()
    } else {
        operation(
            mcp,
            target,
            json!({"operation":"http_context_create","options":options}),
        )
        .await["httpContext"]
            .clone()
    }
}
async fn configure(mcp: &mut Mcp, target: &str, context: &Value, credentials: Value, shared: bool) {
    let setting = json!({"kind":"http_credentials","credentials":credentials});
    eprintln!(
        "Credential setter {target}: shared={shared}, count={}",
        match &setting["credentials"] {
            Value::Null => 0,
            Value::Array(values) => values.len(),
            _ => 1,
        }
    );
    let result = if shared {
        let submitted=mcp.tool("act",json!({"targetId":target,"actions":[{"action":"context_configure","context":context,"setting":setting}]})).await;
        completed(mcp, target, submitted).await["results"][0].clone()
    } else {
        operation(
            mcp,
            target,
            json!({"operation":"context_configure","context":context,"setting":setting}),
        )
        .await
    };
    assert_eq!(result["scopeEnded"], false);
    assert!(result["requested"]["credentialCount"].is_u64());
    assert!(result["requested"].get("credentials").is_none());
    assert!(!result.to_string().contains("password"));
}
async fn fetch(
    mcp: &mut Mcp,
    target: &str,
    http: &Value,
    site: &wire::Site,
    path: &str,
    status: u64,
    requests: usize,
    authorization: Option<&str>,
) {
    let before = site.records().len();
    let result=operation(mcp,target,json!({"operation":"http_fetch","httpContext":http,"request":{"url":format!("{}{path}",site.url)}})).await;
    assert_eq!(result["status"], status, "{result}");
    assert_eq!(result["authenticationRetries"], requests - 1);
    assert_eq!(result["retryCount"], 0);
    assert_eq!(result["replayed"], requests > 1);
    let records = site.records();
    let selected = records[before..]
        .iter()
        .filter(|record| record["path"] == path)
        .collect::<Vec<_>>();
    assert_eq!(selected.len(), requests, "{selected:?}");
    assert_eq!(
        selected.last().unwrap()["headers"]["authorization"].as_str(),
        authorization,
        "{selected:?}"
    );
    operation(
        mcp,
        target,
        json!({"operation":"http_response_remove","requestId":result["requestId"]}),
    )
    .await;
}
async fn navigate(
    mcp: &mut Mcp,
    target: &str,
    page: &Value,
    site: &wire::Site,
    path: &str,
    authorization: &str,
) {
    let before = site.records().len();
    operation(mcp,target,json!({"operation":"page_navigate","page":page,"navigation":{"kind":"goto","url":format!("{}{path}",site.url)}})).await;
    let records = site.records();
    let document = records[before..]
        .iter()
        .filter(|record| record["path"] == path)
        .last()
        .expect("The selected navigation must reach the wire");
    assert_eq!(
        document["headers"]["authorization"], authorization,
        "{document}"
    );
}
async fn cell(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    ready: &Value,
    engine: &str,
    first: &wire::Site,
    second: &wire::Site,
) {
    let context = &ready["browser"]["selectedContext"];
    let page = &ready["browser"]["selectedPage"];
    navigate(
        mcp,
        target,
        page,
        first,
        "/auth/basic",
        "Basic QWNtZTpwYXNz",
    )
    .await;
    let inherited = create(
        mcp,
        target,
        json!({"browserContext":context,"allowUnknownCookiePartitions":engine=="firefox"}),
        false,
    )
    .await;
    let mut always = credentials(&first.url, "pass");
    always["send"] = json!("always");
    let explicit=create(mcp,target,json!({"browserContext":context,"httpCredentials":always,"allowUnknownCookiePartitions":engine=="firefox"}),true).await;
    fetch(
        mcp,
        target,
        &inherited,
        first,
        "/auth/basic",
        200,
        2,
        Some("Basic QWNtZTpwYXNz"),
    )
    .await;
    configure(mcp, target, context, credentials(&first.url, "wrong"), true).await;
    fetch(
        mcp,
        target,
        &inherited,
        first,
        "/auth/basic",
        401,
        2,
        Some("Basic QWNtZTp3cm9uZw=="),
    )
    .await;
    fetch(
        mcp,
        target,
        &explicit,
        first,
        "/auth/basic",
        200,
        1,
        Some("Basic QWNtZTpwYXNz"),
    )
    .await;
    configure(mcp, target, context, Value::Null, false).await;
    fetch(mcp, target, &inherited, first, "/auth/basic", 401, 1, None).await;
    configure(mcp, target, context, credentials(&first.url, "pass"), false).await;
    fetch(
        mcp,
        target,
        &inherited,
        first,
        "/auth/basic",
        200,
        2,
        Some("Basic QWNtZTpwYXNz"),
    )
    .await;
    configure(mcp, target, context, json!([]), true).await;
    fetch(mcp, target, &inherited, first, "/auth/basic", 401, 1, None).await;
    configure(
        mcp,
        target,
        context,
        json!({"origin":second.url,"username":"Acme2","password":"new-pass"}),
        true,
    )
    .await;
    navigate(
        mcp,
        target,
        page,
        second,
        "/auth/changed",
        "Basic QWNtZTI6bmV3LXBhc3M=",
    )
    .await;
    fetch(mcp, target, &inherited, first, "/auth/basic", 401, 1, None).await;
    fetch(
        mcp,
        target,
        &inherited,
        second,
        "/auth/changed",
        200,
        2,
        Some("Basic QWNtZTI6bmV3LXBhc3M="),
    )
    .await;
    for http in [inherited, explicit] {
        operation(
            mcp,
            target,
            json!({"operation":"http_context_remove","httpContext":http}),
        )
        .await;
    }
    if engine == "chrome" {
        native_interception(fixture, mcp, target, context, page, second).await;
    }
}

async fn native_interception(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    context: &Value,
    page: &Value,
    site: &wire::Site,
) {
    configure(mcp, target, context, credentials(&site.url, "pass"), true).await;
    operation(mcp,target,json!({"operation":"page_navigate","page":page,"navigation":{"kind":"goto","url":format!("{}/header-page",site.url)}})).await;
    let url = format!("{}/auth/basic", site.url);
    let added=operation(mcp,target,json!({"operation":"network_route","scope":page,"action":{"kind":"add_matcher","matcher":{"kind":"exact","url":url}}})).await;
    let handler = added["handlers"][0]["handlerId"].clone();
    let before = site.records().len();
    operation(mcp,target,json!({"operation":"evaluate","scope":page,"isFunction":true,"expression":format!("()=>{{globalThis.AcmeCredentialReply=fetch({}).then(r=>r.status,()=>-1);return true}}",json!(url))})).await;
    let callback = har_interleave::callback(mcp, target, page, &handler).await;
    collectors::refused(mcp,target,json!({"operation":"context_configure","context":context,"setting":{"kind":"http_credentials","credentials":null}}),"ROUTE_CALLBACKS_PENDING",true).await;
    operation(mcp,target,json!({"operation":"callback_resolve","callback":callback,"response":{"kind":"route_continue"}})).await;
    let received=operation(mcp,target,json!({"operation":"evaluate","scope":page,"isFunction":true,"expression":"async()=>await globalThis.AcmeCredentialReply"})).await;
    assert_eq!(received["value"]["value"], 200, "{received}");
    let records = site.records();
    let own = records[before..]
        .iter()
        .filter(|record| record["path"] == "/auth/basic")
        .last()
        .unwrap();
    assert_eq!(own["headers"]["authorization"], "Basic QWNtZTpwYXNz");
    operation(mcp,target,json!({"operation":"network_route","scope":page,"action":{"kind":"clear","behavior":"wait"}})).await;
    let archived = format!("{}/credential-archive", site.url);
    let registered = har_replay_filter::archive(
        fixture,
        mcp,
        target,
        std::slice::from_ref(&archived),
        "Acme credential archive",
    )
    .await;
    operation(
        mcp,
        target,
        json!({"operation":"page_har_set","page":page,"archiveId":registered["archiveId"]}),
    )
    .await;
    let before = site.records().len();
    assert_eq!(
        har_replay_filter::fetch(mcp, target, page, &archived).await,
        "Acme credential archive"
    );
    assert_eq!(
        site.records().len(),
        before,
        "The HAR reply must not reach the wire"
    );
    operation(
        mcp,
        target,
        json!({"operation":"page_har_set","page":page,"archiveId":null}),
    )
    .await;
    operation(
        mcp,
        target,
        json!({"operation":"har_remove","archiveId":registered["archiveId"]}),
    )
    .await;
    let response = har_replay_filter::fetch(mcp, target, page, &url).await;
    let response: Value = serde_json::from_str(response.as_str().unwrap()).unwrap();
    assert_eq!(
        response["headers"]["authorization"], "Basic QWNtZTpwYXNz",
        "Clearing manual/HAR interception must preserve configured authentication"
    );
}
#[tokio::test]
async fn installed_step_four_native_credential_transition_diagnostic() {
    let fixture = installed_fixture();
    let mut mcp = Mcp::start(&fixture).await;
    let first = wire::Site::start(None).await;
    let second = wire::Site::start(None).await;
    let mut failures = Vec::new();
    for mode in ["headless", "headed"] {
        let connected = mcp
            .tool("browser_connect", json!({"engine":"chrome","mode":mode}))
            .await;
        let target = connected["targetId"].as_str().unwrap();
        let ready = fixture.ready_target(target).await;
        let result=std::panic::AssertUnwindSafe(async {
            for iteration in 0..8 {
                eprintln!("Native credential transition chrome/{mode}: source {iteration}");
                let context=operation(&mut mcp,target,json!({"operation":"context_create","options":{"viewport":{"width":768,"height":512},"deviceScaleFactor":1,"baseURL":first.url,"httpCredentials":credentials(&first.url,"pass")}})).await["context"].clone();
                let page=operation(&mut mcp,target,json!({"operation":"page_create","context":context,"select":false})).await["page"].clone();
                let selected=json!({"browser":{"selectedContext":context,"selectedPage":page}});
                cell(&fixture,&mut mcp,target,&selected,"chrome",&first,&second).await;
                operation(&mut mcp,target,json!({"operation":"context_close","context":context})).await;
            }
        }).catch_unwind().await;
        if result.is_err() {
            let status = mcp.tool("browser_status", json!({"targetId":target})).await;
            std::fs::write(
                fixture
                    .workspace
                    .path()
                    .join(format!("native-credential-transition-failure-{mode}.json")),
                serde_json::to_vec_pretty(&status).unwrap(),
            )
            .unwrap();
        }
        let closed = performance_lifecycle::disconnect_observed(
            &fixture,
            &mut mcp,
            target,
            "chrome",
            mode,
            &ready["browser"]["selectedContext"],
        )
        .await;
        if closed["result"]["structuredContent"]["ok"] != true {
            failures.push(closed);
        }
        if let Err(panic) = result {
            mcp.stop().await;
            std::panic::resume_unwind(panic);
        }
    }
    mcp.stop().await;
    assert!(
        failures.is_empty(),
        "Original cleanup failures: {failures:?}"
    );
}

#[tokio::test]
async fn installed_step_four_credential_lifecycle_checkpoint() {
    let fixture = installed_fixture();
    let mut mcp = Mcp::start(&fixture).await;
    let first = wire::Site::start(None).await;
    let second = wire::Site::start(None).await;
    let mut failures = vec![];
    for mode in ["headless", "headed"] {
        for engine in ["chrome", "firefox", "webkit"] {
            let mut arguments = json!({"engine":engine,"mode":mode});
            if engine != "chrome" {
                let credential = credentials(&first.url, "pass");
                arguments["context"] = json!({"viewport":{"width":768,"height":512},"deviceScaleFactor":1,"baseURL":first.url,"userAgent":"Acme","httpCredentials":if mode=="headless"{credential}else{json!([credentials("http://example.invalid","wrong"),credential])}});
            } else {
                arguments["context"] = json!({"viewport":{"width":768,"height":512},"deviceScaleFactor":1,"baseURL":first.url,"httpCredentials":if mode=="headless"{credentials(&first.url,"pass")}else{json!([credentials("http://example.invalid","wrong"),credentials(&first.url,"pass")])}});
            }
            let connected = mcp
                .tool(
                    if mode == "headless" {
                        "browser_connect"
                    } else {
                        "connect"
                    },
                    arguments,
                )
                .await;
            let target = connected["targetId"].as_str().unwrap();
            let ready = fixture.ready_target(target).await;
            let result = std::panic::AssertUnwindSafe(cell(
                &fixture, &mut mcp, target, &ready, engine, &first, &second,
            ))
            .catch_unwind()
            .await;
            if result.is_err() {
                let status = mcp.tool("browser_status", json!({"targetId":target})).await;
                std::fs::write(
                    fixture
                        .workspace
                        .path()
                        .join(format!("credential-lifecycle-failure-{engine}-{mode}.json")),
                    serde_json::to_vec_pretty(&status).unwrap(),
                )
                .unwrap();
            }
            std::fs::write(
                fixture
                    .workspace
                    .path()
                    .join(format!("credential-lifecycle-wire-{engine}-{mode}.json")),
                serde_json::to_vec_pretty(
                    &json!({"first":first.records(),"second":second.records()}),
                )
                .unwrap(),
            )
            .unwrap();
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
