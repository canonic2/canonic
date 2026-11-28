//! Native pending and uncertain creations share the retained-reference bound.
use super::*;

async fn capacity(mcp: &mut Mcp, target: &str) -> Value {
    mcp.detailed_browser_status(target).await["browser"]["handleCapacity"].clone()
}
async fn exercise(fixture: &Fixture, mcp: &mut Mcp, target: &str, context: &Value, mode: &str) {
    let page = operation(
        mcp,
        target,
        json!({"operation":"page_create","context":context,"select":true}),
    )
    .await["page"]
        .clone();
    let mut handles = Vec::new();
    while handles.len() < 255 {
        let count = (255 - handles.len()).min(8);
        let actions=(0..count).map(|_| json!({"action":"evaluate","scope":page,"expression":"()=>({Acme:true})","isFunction":true,"returnHandle":true})).collect::<Vec<_>>();
        let submitted = mcp
            .tool("act", json!({"targetId":target,"actions":actions}))
            .await;
        let result = completed(mcp, target, submitted).await;
        handles.extend(
            result["results"]
                .as_array()
                .unwrap()
                .iter()
                .map(|result| result["handle"].clone()),
        );
    }
    assert_eq!(
        capacity(mcp, target).await,
        json!({"limit":256,"retained":255,"pending":0,"uncertain":0})
    );
    operation(mcp,target,json!({"operation":"evaluate","scope":page,"expression":"()=>{globalThis.acmeRejectedAllocations=0;globalThis.acmeAllocationReady=false}","isFunction":true})).await;
    let held=mcp.tool("browser_send",json!({"targetId":target,"request":{"operation":"evaluate","scope":page,"expression":"()=>new Promise(resolve=>{acmeAllocationReady=true;globalThis.acmeResolveAllocation=()=>resolve({Acme:true})})","isFunction":true,"returnHandle":true}})).await;
    let until = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let ready=operation(mcp,target,json!({"operation":"evaluate","scope":page,"expression":"()=>acmeAllocationReady===true","isFunction":true})).await;
        if ready["value"]["value"] == true {
            break;
        }
        assert!(tokio::time::Instant::now() < until, "{ready}");
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert_eq!(
        capacity(mcp, target).await,
        json!({"limit":256,"retained":255,"pending":1,"uncertain":0})
    );
    let refused = json!({"operation":"evaluate","scope":page,"expression":"()=>{acmeRejectedAllocations++;return {Acme:true}}","isFunction":true,"returnHandle":true});
    collectors::refused(mcp, target, refused.clone(), "HANDLE_CAPACITY", true).await;
    assert_eq!(operation(mcp,target,json!({"operation":"evaluate","scope":page,"expression":"()=>acmeRejectedAllocations","isFunction":true})).await["value"]["value"],0);
    operation(mcp,target,json!({"operation":"evaluate","scope":page,"expression":"()=>{acmeResolveAllocation();return true}","isFunction":true})).await;
    let first = completed(mcp, target, held).await["handle"].clone();
    assert_eq!(
        capacity(mcp, target).await,
        json!({"limit":256,"retained":256,"pending":0,"uncertain":0})
    );
    collectors::refused(mcp, target, refused.clone(), "HANDLE_CAPACITY", true).await;
    operation(
        mcp,
        target,
        json!({"operation":"handle_dispose","handle":first}),
    )
    .await;
    operation(mcp,target,json!({"operation":"evaluate","scope":page,"expression":"()=>{globalThis.acmeUnknownAllocationStarted=false}","isFunction":true})).await;
    let pending=mcp.tool("browser_send",json!({"targetId":target,"timeoutMs":250,"request":{"operation":"evaluate","scope":page,"expression":"()=>new Promise(resolve=>{acmeUnknownAllocationStarted=true;globalThis.acmeResolveUnknownAllocation=()=>resolve({AcmeLate:true})})","isFunction":true,"returnHandle":true}})).await;
    let unknown = mcp
        .tool(
            "browser_status",
            json!({"targetId":target,"operationId":pending["operation"]["id"],"waitMs":30000}),
        )
        .await;
    std::fs::write(
        fixture
            .workspace
            .path()
            .join(format!("retained-allocation-unknown-{mode}.json")),
        serde_json::to_vec_pretty(&unknown).unwrap(),
    )
    .unwrap();
    assert_eq!(unknown["operation"]["outcome"], "unknown", "{unknown}");
    assert_eq!(
        unknown["operation"]["result"]["handleAllocationPending"],
        true
    );
    assert_eq!(operation(mcp,target,json!({"operation":"evaluate","scope":page,"expression":"()=>acmeUnknownAllocationStarted===true","isFunction":true})).await["value"]["value"],true);
    assert_eq!(
        capacity(mcp, target).await,
        json!({"limit":256,"retained":255,"pending":1,"uncertain":1})
    );
    collectors::refused(mcp, target, refused, "HANDLE_CAPACITY", true).await;
    operation(mcp,target,json!({"operation":"page_navigate","page":page,"navigation":{"kind":"set_content","html":"<!doctype html><body>Acme replacement</body>","waitUntil":"load"}})).await;
    let rotated = capacity(mcp, target).await;
    assert_eq!(
        rotated,
        json!({"limit":256,"retained":0,"pending":1,"uncertain":1})
    );
    operation(mcp,target,json!({"operation":"evaluate","scope":page,"expression":"()=>{acmeResolveUnknownAllocation();return true}","isFunction":true})).await;
    let repeated = mcp
        .tool(
            "browser_status",
            json!({"targetId":target,"operationId":pending["operation"]["id"],"waitMs":1000}),
        )
        .await;
    assert_eq!(
        repeated["operation"], unknown["operation"],
        "A late native completion cannot invent a delivered handle receipt"
    );
    assert_eq!(capacity(mcp, target).await, rotated);
    let fresh=operation(mcp,target,json!({"operation":"evaluate","scope":page,"expression":"()=>document.body","isFunction":true,"returnHandle":true})).await["handle"].clone();
    operation(
        mcp,
        target,
        json!({"operation":"handle_dispose","handle":fresh}),
    )
    .await;
    operation(mcp, target, json!({"operation":"page_close","page":page})).await;
    let until = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let status = capacity(mcp, target).await;
        if status["pending"] == 0 {
            assert_eq!(
                status,
                json!({"limit":256,"retained":0,"pending":0,"uncertain":0})
            );
            break;
        }
        assert!(
            tokio::time::Instant::now() < until,
            "Native page detachment has not ended uncertain allocation ownership: {status}"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    std::fs::write(fixture.workspace.path().join(format!("retained-allocation-{mode}.json")),serde_json::to_vec_pretty(&json!({"unknown":unknown["operation"],"afterReferenceRotation":rotated,"final":capacity(mcp,target).await})).unwrap()).unwrap();
}

#[tokio::test]
async fn installed_step_four_retained_allocation_checkpoint() {
    let fixture = installed_fixture();
    let mut mcp = Mcp::start(&fixture).await;
    let mut failures = Vec::new();
    for mode in ["headless", "headed"] {
        let connected = fixture
            .cli(&["browser", "connect", "--engine", "chrome", "--mode", mode])
            .await;
        let target = connected["targetId"].as_str().unwrap();
        let ready = fixture.ready_target(target).await;
        let context = &ready["browser"]["selectedContext"];
        let result =
            std::panic::AssertUnwindSafe(exercise(&fixture, &mut mcp, target, context, mode))
                .catch_unwind()
                .await;
        let before_disconnect = mcp.detailed_browser_status(target).await;
        std::fs::write(
            fixture
                .workspace
                .path()
                .join(format!("retained-allocation-before-disconnect-{mode}.json")),
            serde_json::to_vec_pretty(&before_disconnect).unwrap(),
        )
        .unwrap();
        let closed = performance_lifecycle::disconnect_observed(
            &fixture, &mut mcp, target, "chrome", mode, context,
        )
        .await;
        if closed["result"]["structuredContent"]["ok"] != true {
            failures.push(format!("chrome/{mode}: {closed}"));
        }
        if let Err(panic) = result {
            mcp.stop().await;
            std::panic::resume_unwind(panic);
        }
        eprintln!(
            "Retained allocations chrome/{mode} passed; workspace {}",
            fixture.workspace.path().display()
        );
    }
    mcp.stop().await;
    assert!(
        failures.is_empty(),
        "Original cleanup failures: {failures:?}"
    );
}
