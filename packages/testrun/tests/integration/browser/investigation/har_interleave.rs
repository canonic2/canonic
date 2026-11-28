//! Installed `add_har` handlers interleaved with callback handlers in one scope.
use super::har_replay_filter::{archive, fetch, refuses};
use super::*;

/// Wait for the route callback currently held by `handler` in `scope`.
pub(super) async fn callback(mcp: &mut Mcp, target: &str, scope: &Value, handler: &Value) -> Value {
    let until = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let status = mcp.detailed_browser_status(target).await;
        let browser = &status["browser"];
        let items = browser["callbacks"]
            .as_array()
            .or_else(|| browser["resources"].as_array())
            .unwrap();
        if let Some(route) = items.iter().find(|item| {
            item["kind"] == "route"
                && item["policyScope"] == *scope
                && item["handlerId"] == *handler
        }) {
            return route["reference"].clone();
        }
        assert!(
            tokio::time::Instant::now() < until,
            "Route callback for {handler} unavailable: {status}"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}
fn pending(mcp_url: &str) -> Value {
    json!({"operation":"evaluate","isFunction":true,"expression":format!("async()=>{{try{{return await(await fetch({})).text()}}catch{{return 'Acme aborted'}}}}",json!(mcp_url))})
}
#[expect(
    clippy::too_many_lines,
    reason = "One owned workflow checks chain order, automatic archive replies, fallback across both handler kinds, archive ownership and exact server counts"
)]
/// One interleaved chain on `scope` (the page or its context), fetched from `page`.
async fn run(
    fixture: &Fixture,
    mcp: &mut Mcp,
    site: &Site,
    target: &str,
    (page, scope): (&Value, &Value),
) {
    operation(mcp,target,json!({"operation":"page_navigate","page":page,"navigation":{"kind":"goto","url":site.url}})).await;
    let prefix = format!("/har-filter/{}", uuid::Uuid::new_v4());
    let base = format!("{}{prefix}", site.url);
    let archived = format!("{base}/archived");
    let newest_url = format!("{base}/newest");
    let missing = format!("{base}/missing");
    let registered = archive(
        fixture,
        mcp,
        target,
        std::slice::from_ref(&archived),
        "Acme interleaved archive",
    )
    .await;
    let oldest = operation(mcp,target,json!({"operation":"network_route","scope":scope,"action":{"kind":"add_matcher","matcher":{"kind":"glob","pattern":format!("**{prefix}/*")}}})).await;
    let oldest = oldest["handlers"][0]["handlerId"].clone();
    let request = json!({"operation":"network_route","scope":scope,"action":{"kind":"add_har","archiveId":registered["archiveId"],"notFound":"fallback"}}).to_string();
    let started = std::time::Instant::now();
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &request])
        .await;
    let added = completed(mcp, target, submitted).await;
    eprintln!("Interleaved add_har fresh CLI: {:?}", started.elapsed());
    let har = added["handlers"][0]["handlerId"].clone();
    assert_eq!(added["handlers"][0]["kind"], "har");
    let newest = operation(mcp,target,json!({"operation":"network_route","scope":scope,"action":{"kind":"add_matcher","matcher":{"kind":"exact","url":newest_url}}})).await;
    let listed = newest["handlers"].as_array().unwrap();
    assert_eq!(
        listed
            .iter()
            .map(|handler| handler["kind"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["callback", "har", "callback"]
    );
    let newest = listed[0]["handlerId"].clone();
    assert_eq!(listed[1]["handlerId"], har);
    assert_eq!(listed[2]["handlerId"], oldest);
    refuses(
        mcp,
        target,
        json!({"operation":"har_remove","archiveId":registered["archiveId"]}),
        "HAR_ARCHIVE_IN_USE",
    )
    .await;

    assert_eq!(
        fetch(mcp, target, page, &archived).await,
        "Acme interleaved archive"
    );

    let mut evaluation = pending(&newest_url);
    evaluation["scope"] = page.clone();
    let started = std::time::Instant::now();
    let submitted = mcp
        .tool(
            "browser_send",
            json!({"targetId":target,"request":evaluation}),
        )
        .await;
    let first = callback(mcp, target, scope, &newest).await;
    let resolved = operation(mcp,target,json!({"operation":"callback_resolve","callback":first,"response":{"kind":"route_fallback"}})).await;
    assert_eq!(resolved["fallback"], true, "{resolved}");
    let last = callback(mcp, target, scope, &oldest).await;
    operation(mcp,target,json!({"operation":"callback_resolve","callback":last,"response":{"kind":"route_continue"}})).await;
    assert_eq!(
        completed(mcp, target, submitted).await["value"]["value"],
        "Acme network"
    );
    eprintln!(
        "Interleaved callback-HAR-callback chain: {:?}",
        started.elapsed()
    );

    let mut evaluation = pending(&missing);
    evaluation["scope"] = page.clone();
    let submitted = mcp
        .tool(
            "browser_send",
            json!({"targetId":target,"request":evaluation}),
        )
        .await;
    let last = callback(mcp, target, scope, &oldest).await;
    operation(
        mcp,
        target,
        json!({"operation":"callback_resolve","callback":last,"response":{"kind":"route_abort"}}),
    )
    .await;
    assert_eq!(
        completed(mcp, target, submitted).await["value"]["value"],
        "Acme aborted"
    );

    let aborting = operation(mcp,target,json!({"operation":"network_route","scope":scope,"action":{"kind":"add_har","archiveId":registered["archiveId"],"urlFilter":{"kind":"exact","url":missing},"notFound":"abort"}})).await;
    assert_eq!(aborting["handlers"][0]["notFound"], "abort");
    assert_eq!(fetch(mcp, target, page, &missing).await, "Acme aborted");

    operation(
        mcp,
        target,
        json!({"operation":"network_route","scope":scope,"action":{"kind":"clear"}}),
    )
    .await;
    operation(
        mcp,
        target,
        json!({"operation":"har_remove","archiveId":registered["archiveId"]}),
    )
    .await;
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
        [(newest_url.strip_prefix(&site.url).unwrap().to_owned(), 1)]
            .into_iter()
            .collect()
    );
}

#[tokio::test]
async fn installed_step_four_har_interleave_checkpoint() {
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
            let page = &ready["browser"]["selectedPage"];
            let context = &ready["browser"]["selectedContext"];
            let result = std::panic::AssertUnwindSafe(async {
                run(&fixture, &mut mcp, &site, target, (page, page)).await;
                run(&fixture, &mut mcp, &site, target, (page, context)).await;
            })
            .catch_unwind()
            .await;
            mcp.tool("disconnect", json!({"targetId":target})).await;
            eprintln!(
                "HAR interleave finished {engine} {mode}: {:?}",
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
