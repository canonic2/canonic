//! Installed CLI/MCP connection acceptance across the three private engines.

use super::*;
use futures_util::FutureExt;

async fn ready(fixture: &Fixture, target: &str) -> Value {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(600);
    loop {
        let status = fixture
            .cli(&["browser", "status", "--target", target])
            .await;
        assert!(status["failure"].is_null(), "{status}");
        match status["browser"]["phase"].as_str() {
            Some("ready") => return status,
            None | Some("starting") => {}
            _ => panic!("Browser did not become ready: {status}"),
        }
        assert!(tokio::time::Instant::now() < deadline, "{status}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn operation(mcp: &mut Mcp, target: &str, request: Value) -> Value {
    let submitted = mcp
        .tool("browser_send", json!({"targetId":target,"request":request}))
        .await;
    let status = mcp
        .tool(
            "browser_status",
            json!({"targetId":target,"operationId":submitted["operation"]["id"],"waitMs":30000}),
        )
        .await;
    assert!(status["operation"]["settled"] == true, "{status}");
    status["operation"].clone()
}

async fn exercise(fixture: &Fixture, mcp: &mut Mcp, targets: &[String]) {
    let mut pages = Vec::new();
    for target in targets {
        let status = ready(fixture, target).await;
        assert_eq!(status["targetId"], *target);
        pages.push(status["browser"]["selectedPage"].clone());
        let context = operation(mcp, target, json!({"operation":"context_create"})).await;
        assert_eq!(context["outcome"], "succeeded", "{context}");
        let context = context["result"]["context"].clone();
        let batch = mcp.tool("act", json!({"targetId":target,"actions":[{"action":"page_create","context":context,"select":true},{"action":"page_list"}]})).await;
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
        assert_eq!(status["operation"]["result"]["completedActions"], 2);
        // Cancelling an already settled operation must preserve its result.
        let cancelled = mcp
            .tool(
                "cancel",
                json!({"targetId":target,"actionId":batch["actionId"]}),
            )
            .await;
        assert_eq!(cancelled["operation"]["outcome"], "succeeded");
        assert_eq!(
            operation(
                mcp,
                target,
                json!({"operation":"context_close","context":context})
            )
            .await["outcome"],
            "succeeded"
        );
        assert!(
            mcp.tool("browser_status", json!({"targetId":target})).await["browser"]["selectedPage"]
                .is_null()
        );
    }
    for (index, target) in targets.iter().enumerate() {
        let foreign = &pages[(index + 1) % pages.len()];
        let refused = operation(
            mcp,
            target,
            json!({"operation":"page_close","page":foreign}),
        )
        .await;
        assert_eq!(refused["code"], "FOREIGN_REFERENCE", "{refused}");
        assert_eq!(
            mcp.tool("browser_status", json!({"targetId":target})).await["browser"]["phase"],
            "ready"
        );
    }
    let ambiguous = mcp
        .request(
            "tools/call",
            json!({"name":"browser_send","arguments":{"request":{"operation":"context_list"}}}),
        )
        .await;
    assert_eq!(
        ambiguous["result"]["structuredContent"]["code"], "TARGET_REQUIRED",
        "{ambiguous}"
    );
    // Closing one engine must leave its neighbours usable.
    let closed = fixture.cli(&["disconnect", "--target", &targets[0]]).await;
    assert_eq!(closed["closed"], true, "{closed}");
    for target in &targets[1..] {
        assert_eq!(
            operation(mcp, target, json!({"operation":"context_list"})).await["outcome"],
            "succeeded"
        );
    }
}

#[tokio::test]
async fn installed_connections_prepare_isolate_cancel_and_release_all_engines() {
    let workspace = crate::Workspace::new();
    let cache = std::env::var_os("TESTRUN_BROWSER_ROOT")
        .map(std::path::PathBuf::from)
        .expect("Select an explicit private TESTRUN_BROWSER_ROOT");
    assert!(cache.is_absolute());
    let fixture = Fixture { workspace, cache };
    let listed = fixture.cli(&["browser", "list"]).await;
    assert_eq!(listed["browsers"].as_array().unwrap().len(), 3);
    let mut mcp = Mcp::start(&fixture).await;
    for mode in ["headless", "headed"] {
        let mut targets = Vec::new();
        let result = std::panic::AssertUnwindSafe(async {
            for engine in ["chrome", "firefox", "webkit"] {
                let connected = mcp
                    .tool("connect", json!({"engine":engine,"mode":mode}))
                    .await;
                targets.push(connected["targetId"].as_str().unwrap().to_owned());
                let preparation = mcp
                    .tool(
                        "browser_status",
                        json!({"operationId":connected["preparationOperationId"],"waitMs":30000}),
                    )
                    .await;
                assert_eq!(
                    preparation["operation"]["outcome"], "succeeded",
                    "{preparation}"
                );
                let retained = mcp
                    .tool(
                        "cancel",
                        json!({"targetId":connected["targetId"],"actionId":connected["preparationOperationId"]}),
                    )
                    .await;
                assert_eq!(retained["operation"]["outcome"], "succeeded", "{retained}");
            }
            exercise(&fixture, &mut mcp, &targets).await;
        })
        .catch_unwind()
        .await;
        // Retain the same owner until cleanup completes even after an assertion.
        for target in &targets {
            let closed = mcp.tool("disconnect", json!({"targetId":target})).await;
            assert_eq!(closed["closed"], true, "{closed}");
        }
        if let Err(panic) = result {
            mcp.tool("disconnect", json!({})).await;
            mcp.stop().await;
            std::panic::resume_unwind(panic);
        }
    }
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
