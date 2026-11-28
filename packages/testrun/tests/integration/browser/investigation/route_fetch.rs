//! Installed `route_fetch`: a paused route request fetched through a browser-associated HTTP client and fulfilled from its evidence.
use super::har_interleave::callback;
use super::*;

/// Fetch one paused page request through `httpContext`, then fulfill the page from that evidence.
async fn run(
    fixture: &Fixture,
    mcp: &mut Mcp,
    site: &Site,
    target: &str,
    (page, browser_context, engine): (&Value, &Value, &str),
) {
    operation(mcp,target,json!({"operation":"page_navigate","page":page,"navigation":{"kind":"goto","url":site.url}})).await;
    let prefix = format!("/har-filter/{}", uuid::Uuid::new_v4());
    let url = format!("{}{prefix}/fetched", site.url);
    let created = operation(mcp,target,json!({"operation":"http_context_create","options":{"browserContext":browser_context,"allowUnknownCookiePartitions":engine == "firefox","timeoutMs":0}})).await;
    let http = created["httpContext"].clone();
    let added = operation(mcp,target,json!({"operation":"network_route","scope":page,"action":{"kind":"add_matcher","matcher":{"kind":"exact","url":url}}})).await;
    let handler = added["handlers"][0]["handlerId"].clone();
    // Page-lane requests wait behind the paused evaluation, so count copies first.
    let before =
        operation(mcp, target, json!({"operation":"network_evidence_list"})).await["evidence"]
            .as_array()
            .unwrap()
            .len();
    let evaluation = json!({"operation":"evaluate","scope":page,"isFunction":true,"expression":format!("async()=>{{const response=await fetch({});return response.status+' '+response.headers.get('content-type')+' '+await response.text()}}",json!(url))});
    let started = std::time::Instant::now();
    let submitted = mcp
        .tool(
            "browser_send",
            json!({"targetId":target,"request":evaluation}),
        )
        .await;
    let paused = callback(mcp, target, page, &handler).await;
    let request =
        json!({"operation":"route_fetch","callback":paused,"httpContext":http,"timeoutMs":0})
            .to_string();
    let fetched = fixture
        .cli(&["browser", "send", "--target", target, &request])
        .await;
    let fetched = completed(mcp, target, fetched).await;
    assert_eq!(fetched["status"], 200, "{fetched}");
    let evidence = fetched["body"]["evidenceId"].clone();
    assert!(evidence.is_string(), "{fetched}");
    let count = |site: &Site| {
        site.counts
            .lock()
            .unwrap()
            .iter()
            .filter(|(path, _)| path.starts_with(&prefix))
            .map(|(_, count)| *count)
            .sum::<usize>()
    };
    assert_eq!(count(site), 1);
    operation(mcp,target,json!({"operation":"callback_resolve","callback":paused,"response":{"kind":"route_fulfill_http_evidence","httpEvidenceId":evidence,"headers":[{"name":"content-type","value":"text/plain"}]}})).await;
    assert_eq!(
        completed(mcp, target, submitted).await["value"]["value"],
        "200 text/plain Acme network"
    );
    eprintln!("Route fetch round trip: {:?}", started.elapsed());
    assert_eq!(count(site), 1, "The page request reached the server");

    let listed = operation(mcp, target, json!({"operation":"network_evidence_list"})).await;
    let copies = listed["evidence"].as_array().unwrap();
    assert_eq!(copies.len(), before + 1, "{listed}");
    for copy in copies {
        operation(
            mcp,
            target,
            json!({"operation":"network_evidence_remove","evidenceId":copy["evidenceId"]}),
        )
        .await;
    }

    // Overrides fetch another URL and method; the paused request continues unchanged.
    let other = format!("{}{prefix}/other", site.url);
    let evaluation = json!({"operation":"evaluate","scope":page,"isFunction":true,"expression":format!("async()=>await (await fetch({})).text()",json!(url))});
    let submitted = mcp
        .tool(
            "browser_send",
            json!({"targetId":target,"request":evaluation}),
        )
        .await;
    let paused = callback(mcp, target, page, &handler).await;
    let overridden = operation(mcp,target,json!({"operation":"route_fetch","callback":paused,"httpContext":http,"timeoutMs":10000,"maxRetries":1,"overrides":{"url":other,"method":"POST","bodyBase64":"QWNtZQ=="}})).await;
    assert_eq!(overridden["status"], 200, "{overridden}");
    operation(mcp,target,json!({"operation":"callback_resolve","callback":paused,"response":{"kind":"route_continue"}})).await;
    assert_eq!(
        completed(mcp, target, submitted).await["value"]["value"],
        "Acme network"
    );
    let observed = site
        .counts
        .lock()
        .unwrap()
        .iter()
        .filter(|(path, _)| path.contains(&prefix))
        .map(|(path, count)| (path.clone(), *count))
        .collect::<std::collections::BTreeMap<_, _>>();
    assert_eq!(
        observed,
        [
            (format!("{prefix}/fetched"), 2),
            (format!("POST {prefix}/other Acme"), 1)
        ]
        .into_iter()
        .collect()
    );
    operation(
        mcp,
        target,
        json!({"operation":"http_evidence_remove","evidenceId":overridden["body"]["evidenceId"]}),
    )
    .await;
    operation(
        mcp,
        target,
        json!({"operation":"network_route","scope":page,"action":{"kind":"clear"}}),
    )
    .await;
    operation(
        mcp,
        target,
        json!({"operation":"http_evidence_remove","evidenceId":evidence}),
    )
    .await;
    operation(
        mcp,
        target,
        json!({"operation":"http_context_remove","httpContext":http}),
    )
    .await;
}

#[tokio::test]
async fn installed_step_four_route_fetch_checkpoint() {
    let fixture = installed_fixture();
    let site = Site::start().await;
    let mut mcp = Mcp::start(&fixture).await;
    let mut failures = vec![];
    for mode in ["headless", "headed"] {
        for engine in ["chrome", "firefox", "webkit"] {
            let connected = fixture
                .cli(&["browser", "connect", "--engine", engine, "--mode", mode])
                .await;
            let target = connected["targetId"].as_str().unwrap();
            let ready = fixture.ready_target(target).await;
            let started = std::time::Instant::now();
            let page = &ready["browser"]["selectedPage"];
            let context = &ready["browser"]["selectedContext"];
            let result = std::panic::AssertUnwindSafe(run(
                &fixture,
                &mut mcp,
                &site,
                target,
                (page, context, engine),
            ))
            .catch_unwind()
            .await;
            let closed = performance_lifecycle::disconnect_observed(
                &fixture, &mut mcp, target, engine, mode, context,
            )
            .await;
            if closed["result"]["structuredContent"]["ok"] != true {
                failures.push(format!("{engine}/{mode}: {closed}"));
            }
            eprintln!(
                "Route fetch finished {engine} {mode}: {:?}",
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
    assert!(
        failures.is_empty(),
        "Original cleanup failures: {failures:?}"
    );
}
