//! Installed HAR `notFound:"fallback"`: archive misses pass to the next handler in the chain.
use super::har_replay_filter::{archive, fetch};
use super::*;

async fn route_callback(mcp: &mut Mcp, target: &str, scope: &Value) -> Value {
    let until = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let status = mcp.detailed_browser_status(target).await;
        let browser = &status["browser"];
        let items = browser["callbacks"]
            .as_array()
            .or_else(|| browser["resources"].as_array())
            .unwrap();
        if let Some(route) = items
            .iter()
            .find(|item| item["kind"] == "route" && item["policyScope"] == *scope)
        {
            return route["reference"].clone();
        }
        assert!(
            tokio::time::Instant::now() < until,
            "Route callback unavailable: {status}"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}
#[expect(
    clippy::too_many_lines,
    reason = "One owned workflow checks automatic Page and Context fallback, an explicit handler reply, exact server counts and teardown"
)]
async fn run(
    fixture: &Fixture,
    mcp: &mut Mcp,
    site: &Site,
    target: &str,
    context: &Value,
    page: &Value,
) {
    operation(mcp,target,json!({"operation":"page_navigate","page":page,"navigation":{"kind":"goto","url":site.url}})).await;
    let prefix = format!("/har-filter/{}", uuid::Uuid::new_v4());
    let base = format!("{}{prefix}", site.url);
    let page_url = format!("{base}/page");
    let context_url = format!("{base}/context");
    let handler_url = format!("{base}/handler");
    let missing = format!("{base}/missing");
    let page_archive = archive(
        fixture,
        mcp,
        target,
        std::slice::from_ref(&page_url),
        "Acme page archive",
    )
    .await;
    let context_archive = archive(
        fixture,
        mcp,
        target,
        &[context_url.clone(), handler_url.clone()],
        "Acme context archive",
    )
    .await;
    operation(mcp,target,json!({"operation":"context_har_set","context":context,"archiveId":context_archive["archiveId"]})).await;
    let request = json!({"operation":"page_har_set","page":page,"archiveId":page_archive["archiveId"],"notFound":"fallback"}).to_string();
    let started = std::time::Instant::now();
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &request])
        .await;
    let rule = completed(mcp, target, submitted).await;
    eprintln!("HAR fallback fresh CLI Page rule: {:?}", started.elapsed());
    assert_eq!(rule["rule"]["notFound"], "fallback", "{rule}");
    assert_eq!(
        fetch(mcp, target, page, &page_url).await,
        "Acme page archive"
    );
    assert_eq!(
        fetch(mcp, target, page, &context_url).await,
        "Acme context archive"
    );
    assert_eq!(fetch(mcp, target, page, &missing).await, "Acme aborted");
    operation(
        mcp,
        target,
        json!({"operation":"context_har_set","context":context,"archiveId":null}),
    )
    .await;
    let started = std::time::Instant::now();
    operation(mcp,target,json!({"operation":"context_har_set","context":context,"archiveId":context_archive["archiveId"],"notFound":"fallback"})).await;
    eprintln!(
        "HAR fallback retained MCP Context rule: {:?}",
        started.elapsed()
    );
    assert_eq!(fetch(mcp, target, page, &missing).await, "Acme network");
    assert_eq!(
        fetch(mcp, target, page, &context_url).await,
        "Acme context archive"
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
        json!({"operation":"page_route_set","page":page,"url":handler_url}),
    )
    .await;
    let expression = format!(
        "async()=>{{try{{return await(await fetch({})).text()}}catch{{return 'Acme aborted'}}}}",
        json!(handler_url)
    );
    let pending=mcp.tool("browser_send",json!({"targetId":target,"request":{"operation":"evaluate","scope":page,"isFunction":true,"expression":expression}})).await;
    let callback = route_callback(mcp, target, page).await;
    let request=json!({"operation":"callback_resolve","callback":callback,"response":{"kind":"route_har","archiveId":page_archive["archiveId"],"notFound":"fallback"}}).to_string();
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &request])
        .await;
    let resolved = completed(mcp, target, submitted).await;
    assert_eq!(resolved["fallback"], true, "{resolved}");
    assert_eq!(resolved["har"]["matched"], false, "{resolved}");
    assert_eq!(resolved["har"]["notFound"], "fallback", "{resolved}");
    let result = completed(mcp, target, pending).await;
    assert_eq!(result["value"]["value"], "Acme context archive");

    operation(
        mcp,
        target,
        json!({"operation":"page_route_set","page":page,"url":null}),
    )
    .await;
    operation(
        mcp,
        target,
        json!({"operation":"context_har_set","context":context,"archiveId":null}),
    )
    .await;
    for archive in [page_archive, context_archive] {
        operation(
            mcp,
            target,
            json!({"operation":"har_remove","archiveId":archive["archiveId"]}),
        )
        .await;
    }
    let observed = site
        .counts
        .lock()
        .unwrap()
        .iter()
        .filter(|(url, _)| url.starts_with(&prefix))
        .map(|(url, count)| (url.clone(), *count))
        .collect::<std::collections::BTreeMap<_, _>>();
    assert_eq!(
        observed,
        [(missing.strip_prefix(&site.url).unwrap().to_owned(), 1)]
            .into_iter()
            .collect()
    );
}

#[tokio::test]
async fn installed_step_four_har_fallback_checkpoint() {
    let fixture = installed_fixture();
    let site = Site::start().await;
    let mut mcp = Mcp::start(&fixture).await;
    for mode in ["headless", "headed"] {
        for engine in ["chrome", "firefox", "webkit"] {
            let connected = fixture
                .cli(&["browser", "connect", "--engine", engine, "--mode", mode])
                .await;
            let target = connected["targetId"].as_str().unwrap();
            let ready = fixture.ready().await;
            let started = std::time::Instant::now();
            let result = std::panic::AssertUnwindSafe(run(
                &fixture,
                &mut mcp,
                &site,
                target,
                &ready["browser"]["selectedContext"],
                &ready["browser"]["selectedPage"],
            ))
            .catch_unwind()
            .await;
            mcp.tool("disconnect", json!({"targetId":target})).await;
            eprintln!(
                "HAR fallback finished {engine} {mode}: {:?}",
                started.elapsed()
            );
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
}
