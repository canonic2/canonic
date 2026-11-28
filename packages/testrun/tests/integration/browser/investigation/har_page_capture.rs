//! Installed page-selected capture records only the selected page's frame requests.
use super::har_replay_filter::{fetch, refuses};
use super::*;

async fn settled(mcp: &mut Mcp, target: &str, context: &Value, expected: u64) -> Value {
    let until = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let status = operation(
            mcp,
            target,
            json!({"operation":"har_capture_status","context":context}),
        )
        .await;
        if status["entries"] == expected && status["completedEntries"] == expected {
            return status;
        }
        assert!(
            tokio::time::Instant::now() < until,
            "Page capture did not settle: {status}"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}
fn archive(stopped: &Value) -> (String, Value) {
    let path = stopped["reports"][0]["attachment"]["path"]
        .as_str()
        .unwrap()
        .to_owned();
    let archive = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    (path, archive)
}
fn urls(archive: &Value) -> std::collections::BTreeSet<String> {
    archive["log"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["request"]["url"].as_str().unwrap().to_owned())
        .collect()
}
#[expect(
    clippy::too_many_lines,
    reason = "One owned workflow checks page selection, child frames, sibling exclusion, refusals, page closure and exact server counts"
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
    let sibling = operation(
        mcp,
        target,
        json!({"operation":"page_create","context":context,"select":false}),
    )
    .await["page"]
        .clone();
    operation(mcp,target,json!({"operation":"page_navigate","page":sibling,"navigation":{"kind":"goto","url":site.url}})).await;
    let foreign_context =
        operation(mcp, target, json!({"operation":"context_create"})).await["context"].clone();
    let foreign = operation(
        mcp,
        target,
        json!({"operation":"page_create","context":foreign_context,"select":false}),
    )
    .await["page"]
        .clone();
    refuses(
        mcp,
        target,
        json!({"operation":"har_capture_start","context":context,"page":foreign}),
        "FOREIGN_PAGE",
    )
    .await;
    operation(
        mcp,
        target,
        json!({"operation":"context_close","context":foreign_context}),
    )
    .await;
    refuses(
        mcp,
        target,
        json!({"operation":"har_capture_status","context":context}),
        "HAR_CAPTURE_INACTIVE",
    )
    .await;

    let prefix = format!("/har-filter/{}", uuid::Uuid::new_v4());
    let base = format!("{}{prefix}", site.url);
    let selected_url = format!("{base}/selected?token=Acme");
    let child_url = format!("{base}/child?token=Acme");
    let sibling_url = format!("{base}/sibling?token=Acme");
    let request =
        json!({"operation":"har_capture_start","context":context,"page":page,"content":"embed"})
            .to_string();
    let started = std::time::Instant::now();
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &request])
        .await;
    let capture = completed(mcp, target, submitted).await;
    eprintln!("HAR page capture fresh CLI start: {:?}", started.elapsed());
    assert_eq!(capture["page"], *page);
    assert_eq!(capture["urlFilterActive"], false);
    assert_eq!(
        fetch(mcp, target, page, &selected_url).await,
        "Acme network"
    );
    assert_eq!(
        fetch(mcp, target, &sibling, &sibling_url).await,
        "Acme network"
    );
    let child = operation(mcp,target,json!({"operation":"evaluate","scope":page,"isFunction":true,"expression":format!("async()=>{{const f=document.createElement('iframe');f.srcdoc='<p>Acme</p>';const ready=new Promise(r=>f.onload=r);document.body.append(f);await ready;try{{return await(await f.contentWindow.fetch({})).text()}}finally{{f.remove()}}}}",json!(child_url))})).await;
    assert_eq!(child["value"]["value"], "Acme network");
    let status = settled(mcp, target, context, 2).await;
    assert_eq!(status["page"], *page);
    assert_eq!(status["droppedEntries"], 0);
    let stopped = operation(
        mcp,
        target,
        json!({"operation":"har_capture_stop","context":context}),
    )
    .await;
    let (selected_path, selected) = archive(&stopped);
    assert_eq!(
        urls(&selected),
        [selected_url.clone(), child_url.clone()]
            .into_iter()
            .collect()
    );
    assert_eq!(selected["log"]["_testrunCapture"]["page"], *page);
    for entry in selected["log"]["entries"].as_array().unwrap() {
        assert_eq!(entry["response"]["content"]["text"], "QWNtZSBuZXR3b3Jr");
    }

    let closed_url = format!("{base}/closed?token=Acme");
    let started = std::time::Instant::now();
    let capture = operation(mcp,target,json!({"operation":"har_capture_start","context":context,"page":sibling,"urlFilter":{"kind":"glob","pattern":format!("**{prefix}/*")}})).await;
    eprintln!(
        "HAR page capture retained MCP start: {:?}",
        started.elapsed()
    );
    assert_eq!(capture["page"], sibling);
    assert_eq!(capture["urlFilterActive"], true);
    let main_url = format!("{base}/main?token=Acme");
    assert_eq!(fetch(mcp, target, page, &main_url).await, "Acme network");
    assert_eq!(
        fetch(mcp, target, &sibling, &closed_url).await,
        "Acme network"
    );
    settled(mcp, target, context, 1).await;
    operation(
        mcp,
        target,
        json!({"operation":"page_close","page":sibling}),
    )
    .await;
    let after_close = format!("{base}/after-close?token=Acme");
    assert_eq!(fetch(mcp, target, page, &after_close).await, "Acme network");
    assert_eq!(
        operation(
            mcp,
            target,
            json!({"operation":"har_capture_status","context":context})
        )
        .await["entries"],
        1
    );
    let request = json!({"operation":"har_capture_stop","context":context}).to_string();
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &request])
        .await;
    let stopped = completed(mcp, target, submitted).await;
    let (closed_path, closed) = archive(&stopped);
    assert_eq!(urls(&closed), [closed_url.clone()].into_iter().collect());
    refuses(
        mcp,
        target,
        json!({"operation":"har_capture_start","context":context,"page":sibling}),
        "STALE_REFERENCE",
    )
    .await;
    refuses(
        mcp,
        target,
        json!({"operation":"har_capture_status","context":context}),
        "HAR_CAPTURE_INACTIVE",
    )
    .await;

    let expected = [
        selected_url,
        child_url,
        sibling_url,
        closed_url,
        main_url,
        after_close,
    ]
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
    for (stopped, path) in [(selected, selected_path), (closed, closed_path)] {
        operation(
            mcp,
            target,
            json!({"operation":"har_remove","archiveId":stopped["log"]["_testrunCapture"]["captureId"]}),
        )
        .await;
        assert!(!std::path::Path::new(&path).exists());
    }
}

#[tokio::test]
async fn installed_step_four_har_page_capture_checkpoint() {
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
                "HAR page capture finished {engine} {mode}: {:?}",
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
