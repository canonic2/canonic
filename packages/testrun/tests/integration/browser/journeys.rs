//! Installed public clients share snapshots, ordered input and stale-reference fences.
use super::*;
use futures_util::FutureExt;

#[tokio::test]
async fn installed_items_two_three_cli_mcp_checkpoint() {
    let binary = std::env::var_os("TESTRUN_TEST_INSTALLED_BINARY")
        .expect("Select the installed Testrun binary");
    assert!(std::path::Path::new(&binary).is_absolute());
    let cache = std::env::var_os("TESTRUN_BROWSER_ROOT").expect("Select a private browser cache");
    let fixture = Fixture {
        workspace: crate::Workspace::new(),
        cache: cache.into(),
    };
    let mut mcp = Mcp::start(&fixture).await;
    for mode in ["headless", "headed"] {
        for engine in ["chrome", "firefox", "webkit"] {
            let connected = fixture
                .cli(&[
                    "browser",
                    "connect",
                    "--engine",
                    engine,
                    "--mode",
                    mode,
                    "--downloads",
                    "--touch",
                ])
                .await;
            let target = connected["targetId"].as_str().unwrap().to_owned();
            let ready = fixture.ready().await;
            let page = ready["browser"]["selectedPage"].clone();
            let result = std::panic::AssertUnwindSafe(exercise(&fixture, &mut mcp, &target, page))
                .catch_unwind()
                .await;
            mcp.tool("disconnect", json!({"targetId":target})).await;
            if let Err(panic) = result {
                fixture.cli(&["disconnect"]).await;
                mcp.tool("disconnect", json!({})).await;
                mcp.stop().await;
                std::panic::resume_unwind(panic);
            }
        }
    }
    fixture.cli(&["disconnect"]).await;
    mcp.tool("disconnect", json!({})).await;
    mcp.stop().await;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let retired = fixture.cli(&["manager", "stop"]).await;
        if retired["retired"] == true {
            break;
        }
        assert!(tokio::time::Instant::now() < deadline, "{retired}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}
async fn exercise(fixture: &Fixture, mcp: &mut Mcp, target: &str, page: Value) {
    let html = "<h1>Acme public journey</h1><label>Name<input id=name></label><button id=save onclick='this.textContent=\"Saved\"'>Save</button>";
    operation(mcp,target,json!({"operation":"page_navigate","page":page,"navigation":{"kind":"set_content","html":html}})).await;
    let snapshot=operation(mcp,target,json!({"operation":"page_read","page":page,"locator":{"kind":"css","selector":"#name"},"read":"snapshot"})).await;
    let locator = snapshot["result"]["value"][0]["locator"].clone();
    let batch=mcp.tool("act",json!({"targetId":target,"actions":[
        {"action":"page_action","page":page,"locator":locator,"input":{"action":"fill","value":"Acme QA"}},
        {"action":"page_action","page":page,"locator":{"kind":"css","selector":"#save"},"input":{"action":"click"},"postcondition":{"kind":"text","value":"Saved"}},
        {"action":"page_read","page":page,"locator":{"kind":"css","selector":"#name"},"read":"state"}
    ]})).await;
    let status = fixture
        .cli(&[
            "status",
            "--target",
            target,
            "--action-id",
            batch["actionId"].as_str().unwrap(),
            "--wait-ms",
            "30000",
        ])
        .await;
    assert_eq!(status["operation"]["outcome"], "succeeded", "{status}");
    assert_eq!(status["operation"]["result"]["completedActions"], 3);
    assert_eq!(
        status["operation"]["result"]["results"][2]["result"]["value"]["value"],
        "Acme QA"
    );
    let request=json!({"operation":"page_read","page":page,"locator":{"kind":"css","selector":"h1"},"read":"text"}).to_string();
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &request])
        .await;
    let outcome = mcp
        .tool(
            "browser_status",
            json!({"targetId":target,"operationId":submitted["operation"]["id"],"waitMs":30000}),
        )
        .await;
    assert_eq!(outcome["operation"]["outcome"], "succeeded", "{outcome}");
    assert_eq!(
        outcome["operation"]["result"]["result"]["value"],
        "Acme public journey"
    );
    operation(mcp,target,json!({"operation":"page_navigate","page":page,"navigation":{"kind":"set_content","html":html}})).await;
    let stale = settled(
        mcp,
        target,
        json!({"operation":"page_read","page":page,"locator":locator,"read":"state"}),
    )
    .await;
    assert_eq!(stale["code"], "STALE_REFERENCE", "{stale}");
    let captured = operation(
        mcp,
        target,
        json!({"operation":"page_screenshot","page":page}),
    )
    .await;
    assert!(std::path::Path::new(captured["attachment"]["path"].as_str().unwrap()).is_file());
    let request=json!({"operation":"capture_compare","baseline":captured["captureId"],"actual":captured["captureId"],"threshold":0,"maxChangedRatio":0}).to_string();
    let compared = fixture
        .cli(&["browser", "send", "--target", target, &request])
        .await;
    let compared = mcp
        .tool(
            "browser_status",
            json!({"targetId":target,"operationId":compared["operation"]["id"],"waitMs":30000}),
        )
        .await;
    assert_eq!(compared["operation"]["outcome"], "succeeded", "{compared}");
    assert_eq!(compared["operation"]["result"]["matches"], true);
    let semantic = operation(
        mcp,
        target,
        json!({"operation":"page_snapshot","page":page,"locator":{"kind":"css","selector":"h1"}}),
    )
    .await;
    assert!(
        semantic["value"]
            .as_str()
            .unwrap()
            .contains("Acme public journey")
    );
    operation(mcp,target,json!({"operation":"page_annotate","page":page,"markers":[{"locator":{"kind":"css","selector":"h1"},"label":"Acme capture"}]})).await;
    operation(
        mcp,
        target,
        json!({"operation":"page_annotate","page":page,"markers":[]}),
    )
    .await;
}

async fn settled(mcp: &mut Mcp, target: &str, request: Value) -> Value {
    let submitted = mcp
        .tool("browser_send", json!({"targetId":target,"request":request}))
        .await;
    let status = mcp
        .tool(
            "browser_status",
            json!({"targetId":target,"operationId":submitted["operation"]["id"],"waitMs":30000}),
        )
        .await;
    assert_eq!(status["operation"]["settled"], true, "{status}");
    status["operation"].clone()
}
async fn operation(mcp: &mut Mcp, target: &str, request: Value) -> Value {
    let operation = settled(mcp, target, request).await;
    assert_eq!(operation["outcome"], "succeeded", "{operation}");
    operation["result"].clone()
}
