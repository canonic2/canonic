//! Installed pinned addScriptTag/addStyleTag through CLI and MCP.
use super::*;

async fn value(mcp: &mut Mcp, target: &str, page: &Value, expression: &str) -> Value {
    operation(
        mcp,
        target,
        json!({"operation":"evaluate","scope":page,"expression":expression,"isFunction":true}),
    )
    .await["value"]["value"]
        .clone()
}
/// A CSP-blocked tag settles as a failure, as the pinned SDK rejects, rather
/// than returning the element or waiting for a load that never comes.
async fn blocked(mcp: &mut Mcp, target: &str, request: Value) {
    let submitted = mcp
        .tool("browser_send", json!({"targetId":target,"request":request}))
        .await;
    let status = mcp
        .tool(
            "browser_status",
            json!({"targetId":target,"operationId":submitted["operation"]["id"],"waitMs":30000}),
        )
        .await;
    let operation = &status["operation"];
    assert_eq!(operation["settled"], true, "{status}");
    assert_eq!(operation["outcome"], "failed", "{status}");
    // Native Chrome returns the page's rejection text; the Playwright driver
    // keeps method error messages private.
    if status["browser"]["backend"] == "playwright" {
        assert_eq!(operation["code"], "BROWSER_METHOD_FAILED", "{status}");
    } else {
        assert!(
            operation.to_string().contains("Content-Security-Policy"),
            "{status}"
        );
    }
}
/// Writes `body` to a caller-relative file and adds it as a tag through the CLI.
async fn cli_file(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    page: &Value,
    operation: &str,
    name: &str,
    body: &str,
) {
    std::fs::write(fixture.workspace.path().join(name), body).unwrap();
    let request = json!({"operation":operation,"scope":page,"path":name}).to_string();
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &request])
        .await;
    completed(mcp, target, submitted).await;
}
async fn cell(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    ready: &Value,
    engine: &str,
    mode: &str,
) {
    let page = &ready["browser"]["selectedPage"];
    operation(mcp,target,json!({"operation":"page_navigate","page":page,"navigation":{"kind":"set_content","html":"<!doctype html><title>Acme tags</title><p id=acme>Acme</p>"}})).await;
    // Inline content over MCP returns the element handle.
    let script = operation(mcp,target,json!({"operation":"page_add_script_tag","scope":page,"content":"window.acmeInline = 'inline';"})).await;
    assert!(script["handle"].is_object(), "{script}");
    let tag = operation(mcp,target,json!({"operation":"evaluate","scope":script["handle"],"expression":"element=>element.tagName","isFunction":true})).await;
    assert_eq!(tag["value"]["value"], "SCRIPT", "{tag}");
    assert_eq!(
        value(mcp, target, page, "()=>window.acmeInline").await,
        "inline"
    );
    // A caller-relative file through the CLI, labeled like the pinned client.
    let name = format!("acme-tag-{engine}-{mode}.js");
    cli_file(
        fixture,
        mcp,
        target,
        page,
        "page_add_script_tag",
        &name,
        "window.acmeFile = 'file';",
    )
    .await;
    assert_eq!(
        value(mcp, target, page, "()=>window.acmeFile").await,
        "file"
    );
    // A URL source waits for its load.
    operation(mcp,target,json!({"operation":"page_add_script_tag","scope":page,"url":"data:text/javascript,window.acmeUrl='url'"})).await;
    assert_eq!(value(mcp, target, page, "()=>window.acmeUrl").await, "url");
    let style = operation(mcp,target,json!({"operation":"page_add_style_tag","scope":page,"content":"#acme{color:rgb(1, 2, 3)}"})).await;
    assert!(style["handle"].is_object(), "{style}");
    assert_eq!(
        value(
            mcp,
            target,
            page,
            "()=>getComputedStyle(document.querySelector('#acme')).color"
        )
        .await,
        "rgb(1, 2, 3)"
    );
    let css = format!("acme-tag-{engine}-{mode}.css");
    cli_file(
        fixture,
        mcp,
        target,
        page,
        "page_add_style_tag",
        &css,
        "#acme{background-color:rgb(4, 5, 6)}",
    )
    .await;
    assert_eq!(
        value(
            mcp,
            target,
            page,
            "()=>getComputedStyle(document.querySelector('#acme')).backgroundColor"
        )
        .await,
        "rgb(4, 5, 6)"
    );
    let refused = mcp
        .request("tools/call", json!({"name":"browser_send","arguments":{"targetId":target,"request":{"operation":"page_add_style_tag","scope":page}}}))
        .await;
    assert_eq!(
        refused["result"]["structuredContent"]["code"], "INVALID_ARGUMENT",
        "{refused}"
    );
    // An enforced page policy blocks inline content; each tag rejects.
    operation(mcp,target,json!({"operation":"page_navigate","page":page,"navigation":{"kind":"set_content","html":"<!doctype html><meta http-equiv=Content-Security-Policy content=\"script-src 'none'; style-src 'none'\"><title>Acme CSP</title><p id=acme>Acme</p>"}})).await;
    blocked(mcp,target,json!({"operation":"page_add_script_tag","scope":page,"content":"window.acmeBlocked = 'ran';"})).await;
    assert!(
        value(mcp, target, page, "()=>window.acmeBlocked===undefined").await == true,
        "a blocked inline script must not run"
    );
    blocked(mcp,target,json!({"operation":"page_add_style_tag","scope":page,"content":"#acme{color:rgb(7, 8, 9)}"})).await;
}

#[tokio::test]
async fn installed_step_four_page_tags_checkpoint() {
    let fixture = installed_fixture();
    let mut mcp = Mcp::start(&fixture).await;
    let mut failures = vec![];
    for mode in ["headless", "headed"] {
        for engine in ["chrome", "firefox", "webkit"] {
            let connected = mcp
                .tool("browser_connect", json!({"engine":engine,"mode":mode}))
                .await;
            let target = connected["targetId"].as_str().unwrap();
            let ready = fixture.ready_target(target).await;
            let result = std::panic::AssertUnwindSafe(cell(
                &fixture, &mut mcp, target, &ready, engine, mode,
            ))
            .catch_unwind()
            .await;
            let closed = performance_lifecycle::disconnect_observed(
                &fixture,
                &mut mcp,
                target,
                engine,
                mode,
                &ready["browser"]["selectedContext"],
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
