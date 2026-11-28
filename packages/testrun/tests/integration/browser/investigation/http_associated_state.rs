//! Live associated state and recovery after its browser source has been removed.
use super::*;
const SEED: &str = include_str!("../../../fixtures/browser-associated-state-seed.js");

async fn baseline(fixture: &Fixture, engine: &str, mode: &str) -> Value {
    let output = Command::new(
        fixture
            .cache
            .join("node-24.12.0/node-v24.12.0-darwin-x64/bin/node"),
    )
    .arg(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/browser-http-associated-state-baseline.cjs"
    ))
    .arg(fixture.cache.join("playwright-core-1.64.1/package"))
    .arg(engine)
    .arg(mode)
    .arg(fixture.cache.join("browsers"))
    .current_dir(fixture.workspace.path())
    .env_clear()
    .env("PATH", "/usr/bin:/bin")
    .env("PW_LANG_NAME", "javascript")
    .output()
    .await
    .unwrap();
    for (extension, bytes) in [("json", &output.stdout), ("stderr", &output.stderr)] {
        std::fs::write(
            fixture
                .workspace
                .path()
                .join(format!("associated-state-sdk-{engine}-{mode}.{extension}")),
            bytes,
        )
        .unwrap();
    }
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}
fn expected(reference: &Value, variant: &str, origin: &str) -> Value {
    let mut state = reference["states"][variant]["state"].clone();
    for item in state["origins"].as_array_mut().unwrap() {
        assert_eq!(item["origin"], reference["origin"]);
        item["origin"] = json!(origin);
    }
    state
}
async fn source(mcp: &mut Mcp, target: &str, origin: &str, engine: &str) -> (Value, Value) {
    let context =
        operation(mcp, target, json!({"operation":"context_create"})).await["context"].clone();
    let page = operation(
        mcp,
        target,
        json!({"operation":"page_create","context":context,"select":false}),
    )
    .await["page"]
        .clone();
    operation(mcp,target,json!({"operation":"page_navigate","page":page,"navigation":{"kind":"goto","url":origin,"waitUntil":"load"}})).await;
    let seeded=operation(mcp,target,json!({"operation":"evaluate","scope":page,"expression":SEED,"isFunction":true,"argument":{"kind":"json","value":engine!="webkit"}})).await;
    assert_eq!(seeded["value"]["value"], true, "{seeded}");
    (context, page)
}
/// Pages through the context's whole public activity history.
async fn event_count(mcp: &mut Mcp, target: &str, context: &Value) -> usize {
    let mut cursor = Value::Null;
    let mut count = 0;
    loop {
        let mut request = json!({"operation":"events_read","context":context,"limit":100});
        if !cursor.is_null() {
            request["cursor"] = cursor;
        }
        let page = operation(mcp, target, request).await;
        count += page["events"].as_array().unwrap().len();
        cursor = page["nextCursor"].clone();
        if cursor.is_null() {
            return count;
        }
    }
}
async fn client(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    context: &Value,
    engine: &str,
) -> Value {
    let submitted=fixture.cli(&["browser","send","--target",target,&json!({"operation":"http_context_create","options":{"browserContext":context,"allowUnknownCookiePartitions":engine=="firefox"}}).to_string()]).await;
    completed(mcp, target, submitted).await["httpContext"].clone()
}
async fn run(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    origin: &str,
    engine: &str,
    mode: &str,
    reference: &Value,
) {
    let (context, page) = source(mcp, target, origin, engine).await;
    let began = std::time::Instant::now();
    let http = client(fixture, mcp, target, &context, engine).await;
    let fresh_ms = began.elapsed().as_secs_f64() * 1000.0;
    let retained = std::time::Instant::now();
    let blocker = http_storage_state::block_publication(fixture, &context);
    let submitted=mcp.tool("browser_send",json!({"targetId":target,"request":{"operation":"http_context_storage_state","httpContext":http}})).await;
    let failed = mcp
        .tool(
            "browser_status",
            json!({"targetId":target,"operationId":submitted["operation"]["id"],"waitMs":30000}),
        )
        .await;
    assert_eq!(failed["operation"]["outcome"], "failed", "{failed}");
    assert_eq!(failed["operation"]["code"], "EVIDENCE_IO", "{failed}");
    let partial = &failed["operation"]["result"];
    assert_eq!(partial["stateRetained"], true);
    let capture = partial["captureId"].clone();
    let pending = operation(mcp, target, json!({"operation":"http_evidence_list"})).await;
    assert_eq!(pending["pendingArtifacts"].as_array().unwrap().len(), 1);
    assert_eq!(pending["pendingArtifacts"][0]["captureId"], capture);
    operation(mcp,target,json!({"operation":"evaluate","scope":page,"expression":"()=>{localStorage.setItem('Acme-account','Acme-private-changed');document.cookie='Acme-associated=Acme-private-changed; Path=/; SameSite=Lax';return true;}"})).await;
    operation(
        mcp,
        target,
        json!({"operation":"http_context_remove","httpContext":http}),
    )
    .await;
    operation(
        mcp,
        target,
        json!({"operation":"context_close","context":context}),
    )
    .await;
    drop(blocker);
    let submitted = fixture
        .cli(&[
            "browser",
            "send",
            "--target",
            target,
            &json!({"operation":"http_evidence_finalize","captureId":capture}).to_string(),
        ])
        .await;
    let recovered = completed(mcp, target, submitted).await;
    assert_eq!(recovered["reports"][0]["browserAssociated"], true);
    assert_eq!(
        http_storage_state::state(mcp, target, &recovered).await,
        expected(reference, "defaults", origin)
    );
    let mut evidence = vec![recovered["reports"][0]["evidenceId"].clone()];

    let (context, _page) = source(mcp, target, origin, engine).await;
    let http = client(fixture, mcp, target, &context, engine).await;
    for (name, indexed_db, opfs) in [
        ("defaults", false, false),
        ("indexedDB", true, false),
        ("opfs", false, true),
        ("all", true, true),
    ] {
        if engine == "webkit" && opfs {
            assert!(
                reference["states"][name]["refusal"]["message"]
                    .as_str()
                    .unwrap()
                    .contains("OPFS")
            );
            collectors::refused(mcp,target,json!({"operation":"http_context_storage_state","httpContext":http,"options":{"indexedDB":indexed_db,"opfs":opfs}}),"OPFS_UNSUPPORTED",true).await;
            continue;
        }
        let request = json!({"operation":"http_context_storage_state","httpContext":http,"options":{"indexedDB":indexed_db,"opfs":opfs}});
        let captured = if indexed_db && opfs {
            let submitted=mcp.tool("act",json!({"targetId":target,"actions":[{"action":"http_context_storage_state","httpContext":http,"options":{"indexedDB":indexed_db,"opfs":opfs}}]})).await;
            completed(mcp, target, submitted).await["results"][0].clone()
        } else {
            operation(mcp, target, request).await
        };
        let state = http_storage_state::state(mcp, target, &captured).await;
        assert_eq!(
            state,
            expected(reference, name, origin),
            "{engine}/{mode}/{name}"
        );
        assert!(!state.to_string().contains("Acme-private-session"));
        evidence.push(captured["reports"][0]["evidenceId"].clone());
    }
    let listed = operation(mcp, target, json!({"operation":"http_evidence_list"})).await;
    assert_eq!(listed["evidence"].as_array().unwrap().len(), evidence.len());
    assert_eq!(listed["pendingArtifacts"], json!([]));
    operation(
        mcp,
        target,
        json!({"operation":"http_context_remove","httpContext":http}),
    )
    .await;
    operation(
        mcp,
        target,
        json!({"operation":"context_close","context":context}),
    )
    .await;
    for id in evidence {
        operation(
            mcp,
            target,
            json!({"operation":"http_evidence_remove","evidenceId":id}),
        )
        .await;
    }
    assert_eq!(
        operation(mcp, target, json!({"operation":"http_evidence_list"})).await["evidence"],
        json!([])
    );
    eprintln!(
        "Associated HTTP state {engine}/{mode}: freshCliCreateMs={fresh_ms}; retainedCaptureFlagsAndRecoveryMs={}",
        retained.elapsed().as_secs_f64() * 1000.0
    );
}

#[tokio::test]
async fn installed_step_four_http_associated_storage_state_checkpoint() {
    let fixture = installed_fixture();
    let site = Site::start().await;
    let mut mcp = Mcp::start(&fixture).await;
    let mut failures = vec![];
    for engine in ["chrome", "firefox", "webkit"] {
        for mode in ["headless", "headed"] {
            let reference = baseline(&fixture, engine, mode).await;
            let connected = fixture
                .cli(&["browser", "connect", "--engine", engine, "--mode", mode])
                .await;
            let target = connected["targetId"].as_str().unwrap();
            let ready = fixture.ready_target(target).await;
            let context = &ready["browser"]["selectedContext"];
            let result = std::panic::AssertUnwindSafe(async {
                run(
                    &fixture, &mut mcp, target, &site.url, engine, mode, &reference,
                )
                .await;
            })
            .catch_unwind()
            .await;
            let closed = performance_lifecycle::disconnect_observed(
                &fixture, &mut mcp, target, engine, mode, context,
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

#[tokio::test]
async fn installed_step_four_native_http_closed_origins_checkpoint() {
    let fixture = installed_fixture();
    let site = Site::start().await;
    let child_site = Site::start().await;
    let mut mcp = Mcp::start(&fixture).await;
    let mut failures = vec![];
    for mode in ["headless", "headed"] {
        let reference = baseline(&fixture, "chrome", mode).await;
        let connected = fixture
            .cli(&["browser", "connect", "--engine", "chrome", "--mode", mode])
            .await;
        let target = connected["targetId"].as_str().unwrap();
        let ready = fixture.ready_target(target).await;
        let selected = &ready["browser"]["selectedContext"];
        let result = std::panic::AssertUnwindSafe(async {
            let (context,page) = source(&mut mcp,target,&site.url,"chrome").await;
            operation(&mut mcp,target,json!({"operation":"evaluate","scope":page,"expression":"async url=>{const frame=document.createElement('iframe');frame.id='Acme-state-child';const loaded=new Promise(resolve=>frame.onload=resolve);frame.src=url;document.body.append(frame);await loaded;return true;}","isFunction":true,"argument":{"kind":"json","value":child_site.url}})).await;
            let child = operation(&mut mcp,target,json!({"operation":"frame_enter","page":page,"locator":{"kind":"css","selector":"#Acme-state-child"}})).await["frame"].clone();
            let seeded = operation(&mut mcp,target,json!({"operation":"evaluate","scope":child,"expression":SEED,"isFunction":true,"argument":{"kind":"json","value":true}})).await;
            assert_eq!(seeded["value"]["value"],true,"{seeded}");
            let http = client(&fixture,&mut mcp,target,&context,"chrome").await;
            operation(&mut mcp,target,json!({"operation":"page_close","page":page})).await;
            operation(&mut mcp,target,json!({"operation":"context_init_script_add","context":context,"source":"localStorage.setItem('Acme-private-collector-leak','Acme-unexpected-init');"})).await;
            operation(&mut mcp,target,json!({"operation":"binding_add","scope":context,"name":"acmeStorageBridge"})).await;
            operation(&mut mcp,target,json!({"operation":"network_route","scope":context,"action":{"kind":"add_matcher","matcher":{"kind":"glob","pattern":"**/*"}}})).await;
            let before = event_count(&mut mcp,target,&context).await;
            let mut evidence = vec![];
            for (name,indexed_db,opfs) in [("defaults",false,false),("indexedDB",true,false),("opfs",false,true),("all",true,true)] {
                let captured = operation(&mut mcp,target,json!({"operation":"http_context_storage_state","httpContext":http,"options":{"indexedDB":indexed_db,"opfs":opfs}})).await;
                let state = http_storage_state::state(&mut mcp,target,&captured).await;
                let mut expected = reference["closedStates"][name]["state"].clone();
                for item in expected["origins"].as_array_mut().unwrap() {
                    item["origin"] = if item["origin"] == reference["origin"] { json!(site.url) }
                        else { assert_eq!(item["origin"],reference["childOrigin"]); json!(child_site.url) };
                }
                assert_eq!(expected["origins"].as_array().unwrap().len(),2);
                assert_eq!(state,expected,"chrome/{mode}/closed/{name}");
                assert!(!state.to_string().contains("Acme-private-collector-leak"));
                evidence.push(captured["reports"][0]["evidenceId"].clone());
                assert_eq!(event_count(&mut mcp,target,&context).await,before,"Private pages produced public activity");
                let status = mcp.detailed_browser_status(target).await;
                assert!(status["browser"]["callbacks"].as_array().unwrap().is_empty(),"{status}");
                assert!(status["browser"]["resources"].as_array().unwrap().iter().all(|resource|resource["parent"]!=context || resource["kind"]!="page"),"{status}");
            }
            operation(&mut mcp,target,json!({"operation":"http_context_remove","httpContext":http})).await;
            operation(&mut mcp,target,json!({"operation":"context_close","context":context})).await;
            for id in evidence { operation(&mut mcp,target,json!({"operation":"http_evidence_remove","evidenceId":id})).await; }
        }).catch_unwind().await;
        let closed = performance_lifecycle::disconnect_observed(
            &fixture, &mut mcp, target, "chrome", mode, selected,
        )
        .await;
        if closed["result"]["structuredContent"]["ok"] != true {
            failures.push(format!("chrome/{mode}: {closed}"));
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
