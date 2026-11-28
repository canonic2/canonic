//! Installed filtered replay selects scopes before applying archive-miss policy.
use super::*;

pub(super) async fn refuses(mcp: &mut Mcp, target: &str, request: Value, expected: &str) {
    let response = mcp
        .request(
            "tools/call",
            json!({"name":"browser_send","arguments":{"targetId":target,"request":request}}),
        )
        .await;
    let result = &response["result"]["structuredContent"];
    let code = if result["ok"] == true {
        let status=mcp.tool("browser_status",json!({"targetId":target,"operationId":result["data"]["operation"]["id"],"waitMs":30000})).await;
        assert_eq!(status["operation"]["outcome"], "failed", "{status}");
        status["operation"]["code"].clone()
    } else {
        result["code"].clone()
    };
    assert_eq!(code, expected, "{response}");
}
pub(super) async fn fetch(mcp: &mut Mcp, target: &str, page: &Value, url: &str) -> Value {
    operation(mcp,target,json!({"operation":"evaluate","scope":page,"isFunction":true,"expression":format!("async()=>{{try{{return await(await fetch({})).text()}}catch{{return 'Acme aborted'}}}}",json!(url))})).await["value"]["value"].clone()
}
pub(super) async fn archive(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    urls: &[String],
    body: &str,
) -> Value {
    let directory = fixture
        .workspace
        .path()
        .join(format!("replay-filter-{}", uuid::Uuid::new_v4()));
    let mut builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(&directory).unwrap();
    let path = directory.join("source.har");
    let entries = urls.iter().map(|url|json!({"request":{"url":url,"method":"GET","headers":[]},"response":{"status":200,"headers":[{"name":"Content-Type","value":"text/plain"}],"content":{"text":body}}})).collect::<Vec<_>>();
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    serde_json::to_writer(
        options.open(&path).unwrap(),
        &json!({"log":{"version":"1.2","entries":entries}}),
    )
    .unwrap();
    operation(mcp, target, json!({"operation":"har_register","path":path})).await
}
#[expect(
    clippy::too_many_lines,
    reason = "One owned workflow checks scope selection, archive-miss policy, future pages, exact server counts and independent teardown"
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
    let page_url = format!("{base}/page?token=Acme");
    let context_url = format!("{base}/context?token=Acme");
    let continue_url = format!("{base}/page?token=ContextOnly");
    let context_archive = archive(
        fixture,
        mcp,
        target,
        &[page_url.clone(), context_url.clone(), continue_url.clone()],
        "Acme context archive",
    )
    .await;
    let page_archive = archive(
        fixture,
        mcp,
        target,
        &[page_url.clone(), context_url.clone()],
        "Acme page archive",
    )
    .await;
    let request=json!({"operation":"context_har_set","context":context,"archiveId":context_archive["archiveId"],"urlFilter":{"kind":"regex","pattern":format!(r"{}/(?:PAGE|CONTEXT|MISSING)\?token=(?:Acme|ContextOnly)$",regex::escape(&prefix)),"flags":"i"}}).to_string();
    refuses(mcp,target,json!({"operation":"context_har_set","context":context,"archiveId":context_archive["archiveId"],"urlFilter":{"kind":"regex","pattern":"(?=Acme"}}),"ROUTE_REGEX_INVALID").await;
    assert!(
        operation(mcp, target, json!({"operation":"har_list"})).await["routes"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let started = std::time::Instant::now();
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &request])
        .await;
    let registered = completed(mcp, target, submitted).await;
    eprintln!(
        "HAR replay fresh CLI Context regex: {:?}",
        started.elapsed()
    );
    assert_eq!(registered["rule"]["urlFilterActive"], true);
    assert!(!registered.to_string().contains("ContextOnly"));
    let started = std::time::Instant::now();
    operation(mcp,target,json!({"operation":"page_har_set","page":page,"archiveId":page_archive["archiveId"],"notFound":"continue","urlFilter":{"kind":"glob","pattern":format!("**{prefix}/page?token=*")}})).await;
    eprintln!("HAR replay retained MCP Page glob: {:?}", started.elapsed());
    assert_eq!(
        fetch(mcp, target, page, &page_url).await,
        "Acme page archive"
    );
    assert_eq!(
        fetch(mcp, target, page, &context_url).await,
        "Acme context archive"
    );
    let other = format!("{base}/context?token=Other");
    let unmatched = format!("{base}/unmatched?token=Acme");
    let missing = format!("{base}/missing?token=Acme");
    for url in [&other, &unmatched, &continue_url] {
        assert_eq!(fetch(mcp, target, page, url).await, "Acme network");
    }
    assert_eq!(fetch(mcp, target, page, &missing).await, "Acme aborted");
    let child = operation(mcp,target,json!({"operation":"evaluate","scope":page,"isFunction":true,"expression":format!("async()=>{{const f=document.createElement('iframe');f.srcdoc='<p>Acme</p>';const ready=new Promise(r=>f.onload=r);document.body.append(f);await ready;try{{return await(await f.contentWindow.fetch({})).text()}}finally{{f.remove()}}}}",json!(context_url))})).await;
    assert_eq!(child["value"]["value"], "Acme context archive");
    let future = operation(
        mcp,
        target,
        json!({"operation":"page_create","context":context,"select":false}),
    )
    .await["page"]
        .clone();
    operation(mcp,target,json!({"operation":"page_navigate","page":future,"navigation":{"kind":"goto","url":site.url}})).await;
    assert_eq!(
        fetch(mcp, target, &future, &page_url).await,
        "Acme context archive"
    );
    operation(mcp, target, json!({"operation":"page_close","page":future})).await;
    operation(
        mcp,
        target,
        json!({"operation":"page_har_set","page":page,"archiveId":null}),
    )
    .await;
    let started = std::time::Instant::now();
    operation(mcp,target,json!({"operation":"page_har_set","page":page,"archiveId":page_archive["archiveId"],"urlFilter":{"kind":"exact","url":context_url}})).await;
    eprintln!(
        "HAR replay retained MCP Page exact: {:?}",
        started.elapsed()
    );
    refuses(mcp,target,json!({"operation":"page_har_set","page":page,"archiveId":null,"urlFilter":{"kind":"exact","url":context_url}}),"HAR_FILTER_REQUIRES_ARCHIVE").await;
    assert_eq!(
        fetch(mcp, target, page, &context_url).await,
        "Acme page archive"
    );
    operation(
        mcp,
        target,
        json!({"operation":"context_har_set","context":context,"archiveId":null}),
    )
    .await;
    assert_eq!(
        fetch(mcp, target, page, &context_url).await,
        "Acme page archive"
    );
    assert_eq!(fetch(mcp, target, page, &page_url).await, "Acme network");
    let expected = [other, unmatched, continue_url, page_url]
        .into_iter()
        .map(|url| (url.strip_prefix(&site.url).unwrap().to_owned(), 1))
        .collect::<std::collections::BTreeMap<_, _>>();
    let observed = site
        .counts
        .lock()
        .unwrap()
        .iter()
        .filter(|(url, _)| url.starts_with(&prefix))
        .map(|(url, count)| (url.clone(), *count))
        .collect::<std::collections::BTreeMap<_, _>>();
    assert_eq!(observed, expected);
    let started = std::time::Instant::now();
    operation(
        mcp,
        target,
        json!({"operation":"page_har_set","page":page,"archiveId":null}),
    )
    .await;
    for archive in [context_archive, page_archive] {
        let path = archive["reports"][0]["attachment"]["path"]
            .as_str()
            .unwrap();
        operation(
            mcp,
            target,
            json!({"operation":"har_remove","archiveId":archive["archiveId"]}),
        )
        .await;
        assert!(!std::path::Path::new(path).exists());
    }
    assert!(
        operation(mcp, target, json!({"operation":"har_list"})).await["routes"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    eprintln!("HAR replay policy/archive cleanup: {:?}", started.elapsed());
}

#[tokio::test]
async fn installed_step_four_har_replay_filter_checkpoint() {
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
            eprintln!("HAR replay finished {engine} {mode}");
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
