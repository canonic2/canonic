//! Parent-context close and disconnect cancel exact HTTP work without losing cleanup owners.
use super::installed::{submit, terminal};
use super::*;
fn sealed(fixture: &Fixture, context: &Value, operation: &Value) -> Value {
    let connection = context["connectionId"].as_str().unwrap();
    for namespace in std::fs::read_dir(fixture.workspace.data_root())
        .unwrap()
        .flatten()
    {
        let directory = namespace
            .path()
            .join("browser")
            .join(format!("browser-{connection}"));
        let Ok(entries) = std::fs::read_dir(directory) else {
            continue;
        };
        for entry in entries.take(4096).flatten() {
            let path = entry.path();
            if path.extension().is_none_or(|extension| extension != "json") {
                continue;
            }
            let bytes = std::fs::read(path).unwrap();
            let Ok(record) = serde_json::from_slice::<Value>(&bytes) else {
                continue;
            };
            if record["operationId"] == operation["operation"]["id"] && record["settled"] == true {
                return record;
            }
        }
    }
    panic!("Original disconnect returned before its exact HTTP caller was durably sealed");
}
async fn finalize_unknown(mcp: &mut Mcp, target: &str, result: &Value) {
    let id = &result["result"]["requestId"];
    assert!(id.as_str().is_some(), "{result}");
    let until = std::time::Instant::now() + Duration::from_secs(2);
    loop {
        let submitted=mcp.tool("browser_send",json!({"targetId":target,"request":{"operation":"http_response_finalize","requestId":id}})).await;
        let value = terminal(mcp, target, &submitted).await;
        if value["code"] == "HTTP_RESPONSE_PENDING" {
            assert!(std::time::Instant::now() < until);
            tokio::task::yield_now().await;
            continue;
        }
        assert_eq!(value["code"], "HTTP_REQUEST_OUTCOME_UNKNOWN", "{value}");
        break;
    }
}
async fn parent_close(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    engine: &str,
    site: &mut wire::Site,
) {
    let parent =
        operation(mcp, target, json!({"operation":"context_create"})).await["context"].clone();
    let client=operation(mcp,target,json!({"operation":"http_context_create","options":{"browserContext":parent,"allowUnknownCookiePartitions":engine=="firefox"}})).await["httpContext"].clone();
    let pending=submit(fixture,mcp,target,json!({"operation":"http_fetch","httpContext":client,"request":{"url":format!("{}/hold-parent",site.url),"readBody":true}}),0).await;
    let held = site.held(1).await;
    operation(
        mcp,
        target,
        json!({"operation":"context_close","context":parent}),
    )
    .await;
    assert_eq!(site.closed(1).await, ["/hold-parent"]);
    let result = terminal(mcp, target, &pending).await;
    assert_eq!(result["code"], "HTTP_CONTEXT_DISPOSED", "{result}");
    finalize_unknown(mcp, target, &result).await;
    let metadata = operation(
        mcp,
        target,
        json!({"operation":"http_context_read","httpContext":client}),
    )
    .await;
    assert_eq!(metadata["disposed"], true);
    operation(
        mcp,
        target,
        json!({"operation":"http_context_remove","httpContext":client}),
    )
    .await;
    drop(held);
}
async fn held_disconnect(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    parent: &Value,
    engine: &str,
    site: &mut wire::Site,
) -> (
    Value,
    Value,
    std::collections::BTreeMap<String, tokio::sync::oneshot::Sender<()>>,
) {
    let isolated =
        operation(mcp, target, json!({"operation":"http_context_create"})).await["httpContext"]
            .clone();
    let associated=operation(mcp,target,json!({"operation":"http_context_create","options":{"browserContext":parent,"allowUnknownCookiePartitions":engine=="firefox"}})).await["httpContext"].clone();
    let a=submit(fixture,mcp,target,json!({"operation":"http_fetch","httpContext":isolated,"request":{"url":format!("{}/hold-isolated",site.url),"readBody":true}}),1).await;
    let b=submit(fixture,mcp,target,json!({"operation":"http_fetch","httpContext":associated,"request":{"url":format!("{}/hold-associated",site.url),"readBody":true}}),0).await;
    let held = site.held(2).await;
    (a, b, held)
}
#[tokio::test]
async fn installed_step_four_http_disposal_closure_checkpoint() {
    let fixture = installed_fixture();
    let mut mcp = Mcp::start(&fixture).await;
    let mut failures = vec![];
    for engine in ["chrome", "firefox", "webkit"] {
        for mode in ["headless", "headed"] {
            let reference = reference(&fixture, engine, mode).await;
            assert_eq!(
                reference["parentClosure"],
                json!({"requestFailed":true,"socketClosed":true})
            );
            let mut site = wire::Site::start().await;
            let connected = mcp
                .tool("browser_connect", json!({"engine":engine,"mode":mode}))
                .await;
            let target = connected["targetId"].as_str().unwrap();
            let ready = fixture.ready_target(target).await;
            let browser = &ready["browser"]["selectedContext"];
            let result = std::panic::AssertUnwindSafe(async {
                parent_close(&fixture, &mut mcp, target, engine, &mut site).await;
                held_disconnect(&fixture, &mut mcp, target, browser, engine, &mut site).await
            })
            .catch_unwind()
            .await;
            let cleanup = std::time::Instant::now();
            let closed = performance_lifecycle::disconnect_observed(
                &fixture, &mut mcp, target, engine, mode, browser,
            )
            .await;
            if closed["result"]["structuredContent"]["ok"] != true {
                failures.push(closed.clone());
            }
            if let Err(panic) = result {
                fixture.cli(&["disconnect"]).await;
                mcp.tool("disconnect", json!({})).await;
                mcp.stop().await;
                std::panic::resume_unwind(panic);
            }
            let (a, b, held) = result.unwrap();
            assert_eq!(site.closed(2).await, ["/hold-associated", "/hold-isolated"]);
            for submitted in [&a, &b] {
                let terminal = sealed(&fixture, browser, submitted);
                assert_eq!(
                    terminal["code"],
                    if engine == "chrome" {
                        "CANCELLED"
                    } else {
                        "HTTP_CONTEXT_DISPOSED"
                    },
                    "{terminal}"
                );
            }
            assert_eq!(site.count.load(std::sync::atomic::Ordering::SeqCst), 3);
            std::fs::write(fixture.workspace.path().join(format!("http-disposal-closure-{engine}-{mode}.json")),serde_json::to_vec_pretty(&json!({"reference":reference,"closed":closed,"cleanupMs":cleanup.elapsed().as_secs_f64()*1000.})).unwrap()).unwrap();
            drop(held);
            eprintln!(
                "HTTP disposal closure {engine}/{mode}: parent close, isolated/associated disconnect and original cleanup"
            );
        }
    }
    fixture.cli(&["disconnect"]).await;
    mcp.tool("disconnect", json!({})).await;
    mcp.stop().await;
    assert!(
        failures.is_empty(),
        "Original disconnect failures: {failures:?}"
    );
}
