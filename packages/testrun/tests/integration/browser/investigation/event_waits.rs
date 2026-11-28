//! Installed cursor progress, page termination and independent event alternatives.
use super::*;

async fn status(mcp: &mut Mcp, target: &str, submitted: &Value) -> Value {
    mcp.tool(
        "browser_status",
        json!({"targetId":target,"operationId":submitted["operation"]["id"],"waitMs":100}),
    )
    .await["operation"]
        .clone()
}
async fn settle(mcp: &mut Mcp, target: &str, submitted: &Value) -> Value {
    let until = std::time::Instant::now() + Duration::from_secs(15);
    loop {
        let result = status(mcp, target, submitted).await;
        if result["settled"] == true {
            return result;
        }
        assert!(std::time::Instant::now() < until, "{result}");
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}
async fn cursor(mcp: &mut Mcp, target: &str, context: &Value) -> u64 {
    operation(
        mcp,
        target,
        json!({"operation":"events_read","context":context}),
    )
    .await["latestSequence"]
        .as_u64()
        .unwrap()
}
async fn submit(
    mcp: &mut Mcp,
    target: &str,
    context: &Value,
    after: u64,
    conditions: Value,
    timeout: u64,
) -> Value {
    mcp.tool("browser_send",json!({"targetId":target,"timeoutMs":timeout,"request":{"operation":"events_wait","context":context,"after":after,"conditions":conditions}})).await
}
async fn navigate(mcp: &mut Mcp, target: &str, page: &Value, url: &str) {
    operation(mcp,target,json!({"operation":"page_navigate","page":page,"navigation":{"kind":"goto","url":url,"waitUntil":"load"}})).await;
}
#[expect(
    clippy::too_many_lines,
    reason = "One owned lifecycle correlates waiting receipts, cancellation cursors and terminal witnesses without replaying triggers"
)]
async fn exercise(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    context: &Value,
    url: &str,
) -> Value {
    let a = operation(
        mcp,
        target,
        json!({"operation":"page_create","context":context}),
    )
    .await["page"]
        .clone();
    let b = operation(
        mcp,
        target,
        json!({"operation":"page_create","context":context}),
    )
    .await["page"]
        .clone();
    let initial = operation(
        mcp,
        target,
        json!({"operation":"events_read","context":context}),
    )
    .await;
    for page in [&a, &b] {
        assert_eq!(
            initial["events"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|row| row["event"] == "page_created" && row["page"] == *page)
                .count(),
            1,
            "{initial}"
        );
    }
    let after = initial["latestSequence"].as_u64().unwrap();
    let conditions = json!([{"event":"download","page":a},{"event":"frame_navigated","page":a}]);
    let request =
        json!({"operation":"events_wait","context":context,"after":after,"conditions":conditions})
            .to_string();
    let start = std::time::Instant::now();
    let waiting = fixture
        .cli(&["browser", "send", "--target", target, &request])
        .await;
    let fresh_ms = start.elapsed().as_secs_f64() * 1000.0;
    let start = std::time::Instant::now();
    // Wait admission is observable before the independent trigger is sent.
    let until = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        let report = status(mcp, target, &waiting).await;
        assert_ne!(report["settled"], true, "{report}");
        if report["result"]["waiting"] == true {
            break;
        }
        assert!(std::time::Instant::now() < until, "{report}");
    }
    navigate(mcp, target, &b, url).await;
    let observed = cursor(mcp, target, context).await;
    assert!(observed > after);
    loop {
        let report = status(mcp, target, &waiting).await;
        assert_ne!(report["settled"], true, "{report}");
        if report["result"]["nextCursor"]
            .as_u64()
            .is_some_and(|cursor| cursor >= observed)
        {
            break;
        }
        assert!(std::time::Instant::now() < until, "{report}");
    }
    mcp.tool(
        "browser_cancel",
        json!({"targetId":target,"operationId":waiting["operation"]["id"]}),
    )
    .await;
    let cancelled = settle(mcp, target, &waiting).await;
    assert_eq!(cancelled["outcome"], "cancelled", "{cancelled}");
    assert_eq!(cancelled["code"], "CANCELLED", "{cancelled}");
    assert_eq!(cancelled["result"]["nextCursor"], observed, "{cancelled}");
    let resumed = cancelled["result"]["nextCursor"].as_u64().unwrap();
    navigate(mcp, target, &a, url).await;
    let result=operation(mcp,target,json!({"operation":"events_wait","context":context,"after":resumed,"conditions":conditions})).await;
    assert_eq!(result["conditionIndex"], 1);
    assert_eq!(result["event"]["page"], a);
    assert_eq!(result["after"], resumed);
    // Historical order wins over condition order.
    let before = cursor(mcp, target, context).await;
    navigate(mcp, target, &b, &format!("{url}/?Acme=first")).await;
    let observed = cursor(mcp, target, context).await;
    navigate(mcp, target, &a, &format!("{url}/?Acme=second")).await;
    // Shared act owns its mutation lane. Both triggers precede the batch;
    // each step must use its own explicit cursor over that retained history.
    let paired=mcp.tool("act",json!({"targetId":target,"actions":[
        {"action":"events_wait","context":context,"after":observed,"conditions":[{"event":"frame_navigated","page":a}]},
        {"action":"events_wait","context":context,"after":before,"conditions":[{"event":"frame_navigated","page":b}]}
    ]})).await;
    let paired = completed(mcp, target, paired).await;
    assert_eq!(paired["completedActions"], 2);
    assert_eq!(paired["results"][0]["event"]["page"], a);
    assert_eq!(paired["results"][1]["event"]["page"], b);
    let result=operation(mcp,target,json!({"operation":"events_wait","context":context,"after":before,"conditions":[{"event":"frame_navigated","page":a},{"event":"frame_navigated","page":b}]})).await;
    assert_eq!(result["conditionIndex"], 1);
    assert_eq!(result["event"]["page"], b);
    let before = cursor(mcp, target, context).await;
    operation(mcp, target, json!({"operation":"page_close","page":a})).await;
    let closed=operation(mcp,target,json!({"operation":"events_wait","context":context,"after":before,"conditions":[{"event":"page_closed","page":a}]})).await;
    assert_eq!(closed["event"]["page"], a);
    assert_eq!(closed["event"]["event"], "page_closed");
    let shared=mcp.tool("act",json!({"targetId":target,"actions":[{"action":"events_wait","context":context,"after":before,"conditions":[{"event":"page_closed","page":a}]}]})).await;
    let shared = completed(mcp, target, shared).await;
    assert_eq!(shared["completedActions"], 1);
    assert_eq!(shared["results"][0], closed);
    let terminal = submit(
        mcp,
        target,
        context,
        before,
        json!([{"event":"dialog","page":a}]),
        5000,
    )
    .await;
    let failed = settle(mcp, target, &terminal).await;
    assert_eq!(failed["code"], "PAGE_CLOSED", "{failed}");
    assert_eq!(failed["result"]["nextCursor"], closed["nextCursor"]);
    assert_eq!(failed["result"]["nextAction"], "events_read");
    // One terminated alternative does not cancel another page's wait.
    let alternatives = submit(
        mcp,
        target,
        context,
        before,
        json!([{"event":"dialog","page":a},{"event":"frame_navigated","page":b}]),
        10000,
    )
    .await;
    navigate(mcp, target, &b, &format!("{url}/?Acme=remaining")).await;
    let matched = settle(mcp, target, &alternatives).await;
    assert_eq!(matched["outcome"], "succeeded", "{matched}");
    assert_eq!(matched["result"]["conditionIndex"], 1);
    let before = cursor(mcp, target, context).await;
    let expiry = submit(
        mcp,
        target,
        context,
        before,
        json!([{"event":"dialog","page":b}]),
        500,
    )
    .await;
    let expired = settle(mcp, target, &expiry).await;
    assert_eq!(expired["code"], "WAIT_EXPIRED", "{expired}");
    assert_eq!(expired["result"]["nextCursor"], before);
    assert_eq!(expired["result"]["after"], before);
    assert_eq!(expired["result"]["matched"], false);
    // An expired/cancelled wait releases only itself; the other page still works.
    navigate(mcp, target, &b, &format!("{url}/?Acme=usable")).await;
    operation(mcp, target, json!({"operation":"page_close","page":b})).await;
    json!({"freshCliSubmitMs":fresh_ms,"retainedWorkflowWithInspectionMs":start.elapsed().as_secs_f64()*1000.0})
}

#[tokio::test]
async fn installed_step_four_event_waits_checkpoint() {
    let site = Site::start().await;
    let fixture = installed_fixture();
    let mut mcp = Mcp::start(&fixture).await;
    for mode in ["headless", "headed"] {
        for engine in ["chrome", "firefox", "webkit"] {
            let start = std::time::Instant::now();
            let connected = fixture
                .cli(&["browser", "connect", "--engine", engine, "--mode", mode])
                .await;
            let target = connected["targetId"].as_str().unwrap();
            let ready = fixture.ready().await;
            let connect_ms = start.elapsed().as_secs_f64() * 1000.0;
            let result = std::panic::AssertUnwindSafe(exercise(
                &fixture,
                &mut mcp,
                target,
                &ready["browser"]["selectedContext"],
                &site.url,
            ))
            .catch_unwind()
            .await;
            let start = std::time::Instant::now();
            mcp.tool("disconnect", json!({"targetId":target})).await;
            let cleanup_ms = start.elapsed().as_secs_f64() * 1000.0;
            match result {
                Ok(metrics) => eprintln!(
                    "Event waits {engine} {mode}: {metrics}; connectMs={connect_ms}; cleanupMs={cleanup_ms}"
                ),
                Err(panic) => {
                    fixture.cli(&["disconnect"]).await;
                    mcp.tool("disconnect", json!({})).await;
                    mcp.stop().await;
                    std::panic::resume_unwind(panic);
                }
            }
        }
    }
    fixture.cli(&["disconnect"]).await;
    mcp.tool("disconnect", json!({})).await;
    mcp.stop().await;
}
