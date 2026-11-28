//! Repeated acknowledged worker closure must not consume live native RPC slots.
use super::*;

async fn run(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    ready: &Value,
    mode: &str,
    site: &WorkerSite,
    expected: &Value,
    page_only: bool,
) {
    let mut rows = Vec::new();
    let preserved = if page_only {
        let context =
            operation(mcp, target, json!({"operation":"context_create"})).await["context"].clone();
        let sibling = operation(
            mcp,
            target,
            json!({"operation":"page_create","context":context,"select":false}),
        )
        .await["page"]
            .clone();
        operation(mcp,target,json!({"operation":"page_navigate","page":sibling,"navigation":{"kind":"goto","url":site.url,"waitUntil":"load"}})).await;
        assert_eq!(evaluate(mcp,target,&sibling,"async()=>{await navigator.serviceWorker.register('/acme-sw.js');await navigator.serviceWorker.ready;return true;}").await,true);
        let service = worker(mcp, target, &context, &format!("{}/acme-sw.js", site.url)).await;
        Some((context, sibling, service))
    } else {
        None
    };
    for index in 0..65 {
        let context = if let Some((context, _, _)) = &preserved {
            context.clone()
        } else {
            operation(mcp, target, json!({"operation":"context_create"})).await["context"].clone()
        };
        let page = operation(
            mcp,
            target,
            json!({"operation":"page_create","context":context,"select":false}),
        )
        .await["page"]
            .clone();
        let handler = deliver(
            fixture,
            mcp,
            target,
            add(
                if page_only { &page } else { &context },
                &format!("{}/wire/held", site.url),
            ),
            ["cli", "mcp", "act"][index % 3],
        )
        .await["handlerId"]
            .clone();
        operation(mcp,target,json!({"operation":"page_navigate","page":page,"navigation":{"kind":"goto","url":site.url,"waitUntil":"load"}})).await;
        let (pending, callback) = if page_only {
            let submitted = mcp
                .tool(
                    "browser_send",
                    json!({"targetId":target,"request":fetch(&page,"/wire/held")}),
                )
                .await;
            let callback = har_interleave::callback(mcp, target, &page, &handler).await;
            let status = mcp.detailed_browser_status(target).await;
            let route = status["browser"]["callbacks"]
                .as_array()
                .unwrap()
                .iter()
                .find(|route| route["reference"] == callback)
                .unwrap();
            assert_eq!(route["page"], page, "{route}");
            assert!(route["worker"].is_null(), "{route}");
            (submitted, callback)
        } else {
            assert_eq!(evaluate(mcp,target,&page,"async()=>{await navigator.serviceWorker.register('/acme-sw.js');await navigator.serviceWorker.ready;return true;}").await,true);
            let service = worker(mcp, target, &context, &format!("{}/acme-sw.js", site.url)).await;
            super::pending(
                mcp,
                target,
                &service,
                "/wire/held",
                &context,
                &handler,
                &service,
            )
            .await
        };
        deliver(
            fixture,
            mcp,
            target,
            if page_only {
                json!({"operation":"page_close","page":page})
            } else {
                json!({"operation":"context_close","context":context})
            },
            ["act", "cli", "mcp"][index % 3],
        )
        .await;
        let status = mcp
            .tool(
                "browser_status",
                json!({"targetId":target,"operationId":pending["operation"]["id"],"waitMs":30000}),
            )
            .await;
        assert_eq!(
            status["operation"]["settled"], true,
            "cycle {index}: {status}"
        );
        assert_eq!(
            status["operation"]["code"], "STALE_REFERENCE",
            "cycle {index}: {status}"
        );
        assert_eq!(
            status["operation"]["outcome"], "unknown",
            "cycle {index}: {status}"
        );
        assert_eq!(
            status["operation"]["result"]["callerEffectsUnknown"], true,
            "{status}"
        );
        let browser_status = mcp.detailed_browser_status(target).await;
        assert_eq!(
            browser_status["browser"]["phase"], "ready",
            "cycle {index}: {browser_status}"
        );
        assert!(
            browser_status["browser"]["callbacks"]
                .as_array()
                .unwrap()
                .iter()
                .all(|route| route["reference"] != callback)
        );
        // The SDK's successful late no-op remains an explicitly unaccepted
        // contract; native typed stale-callback protection stays in force.
        har_replay_filter::refuses(mcp,target,json!({"operation":"callback_resolve","callback":callback,"response":{"kind":"route_continue"}}),"STALE_REFERENCE").await;
        if let Some((context, sibling, service)) = &preserved {
            assert_eq!(status["operation"]["result"]["page"], page);
            assert_eq!(
                evaluate(mcp, target, sibling, "()=>42").await,
                expected["closed"][index]["sibling"]
            );
            assert_eq!(
                evaluate(mcp, target, service, "()=>42").await,
                expected["closed"][index]["worker"]
            );
            assert_eq!(
                worker(mcp, target, context, &format!("{}/acme-sw.js", site.url)).await,
                *service
            );
        }
        rows.push(json!({"index":index,"operation":status["operation"],"phase":browser_status["browser"]["phase"],"sdkPending":expected["closed"][index]["pending"]}));
        std::fs::write(
            fixture.workspace.path().join(format!(
                "{}-installed-{mode}.json",
                if page_only {
                    "page-close"
                } else {
                    "closed-call"
                }
            )),
            serde_json::to_vec_pretty(&json!({"completed":rows.len(),"cycles":rows})).unwrap(),
        )
        .unwrap();
    }
    assert_eq!(
        evaluate(mcp, target, &ready["browser"]["selectedPage"], "()=>42").await,
        expected["sibling"]
    );
    assert!(
        site.wire.lock().unwrap().is_empty(),
        "Closed held routes must never reach the origin"
    );
    if let Some((context, _, _)) = preserved {
        operation(
            mcp,
            target,
            json!({"operation":"context_close","context":context}),
        )
        .await;
    }
}

#[tokio::test]
async fn installed_step_four_closed_call_capacity_checkpoint() {
    checkpoint(false).await;
}

#[tokio::test]
async fn installed_step_four_page_close_capacity_checkpoint() {
    checkpoint(true).await;
}

async fn checkpoint(page_only: bool) {
    let fixture = installed_fixture();
    let mut mcp = Mcp::start(&fixture).await;
    let mut failures = Vec::new();
    for mode in ["headless", "headed"] {
        let expected = if page_only {
            crate::browser::investigation::closed_call_capacity::page_reference(&fixture, mode)
                .await
        } else {
            crate::browser::investigation::closed_call_capacity::reference(&fixture, mode).await
        };
        let site = WorkerSite::start().await;
        let connected = fixture
            .cli(&["browser", "connect", "--engine", "chrome", "--mode", mode])
            .await;
        let target = connected["targetId"].as_str().unwrap();
        let ready = fixture.ready_target(target).await;
        let result = std::panic::AssertUnwindSafe(run(
            &fixture, &mut mcp, target, &ready, mode, &site, &expected, page_only,
        ))
        .catch_unwind()
        .await;
        if result.is_err() {
            let status = mcp.tool("browser_status", json!({"targetId":target})).await;
            std::fs::write(
                fixture.workspace.path().join(format!(
                    "{}-failure-{mode}.json",
                    if page_only {
                        "page-close"
                    } else {
                        "closed-call"
                    }
                )),
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
            "chrome",
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
            "Closed native caller capacity chrome/{mode}: 65 owned {} closures and sibling execution",
            if page_only { "page" } else { "context" }
        );
    }
    fixture.cli(&["disconnect"]).await;
    mcp.tool("disconnect", json!({})).await;
    mcp.stop().await;
    assert!(
        failures.is_empty(),
        "Original cleanup failures: {failures:?}"
    );
}
