//! Installed `recordHar` context option: capture from creation, publication at `context_close`.
use super::har_replay_filter::{fetch, refuses};
use super::*;

async fn run(fixture: &Fixture, mcp: &mut Mcp, site: &Site, target: &str) {
    let options = |record: Value, base: &str| json!({"operation":"context_create","options":{"viewport":{"width":800,"height":600},"deviceScaleFactor":1,"baseURL":base,"recordHar":record}});
    refuses(
        mcp,
        target,
        options(
            json!({"urlFilter":{"kind":"regex","pattern":"(?=Acme"}}),
            &site.url,
        ),
        "ROUTE_REGEX_INVALID",
    )
    .await;
    let prefix = format!("/har-filter/{}", uuid::Uuid::new_v4());
    let base = format!("{}{prefix}/", site.url);
    let request = options(
        json!({"content":"embed","urlFilter":{"kind":"glob","pattern":"recorded/*"}}),
        &base,
    );
    let started = std::time::Instant::now();
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &request.to_string()])
        .await;
    let context = completed(mcp, target, submitted).await["context"].clone();
    eprintln!(
        "recordHar fresh CLI context create: {:?}",
        started.elapsed()
    );
    let status = operation(
        mcp,
        target,
        json!({"operation":"har_capture_status","context":context}),
    )
    .await;
    assert_eq!(status["mode"], "full");
    assert_eq!(status["content"], "embed");
    assert_eq!(status["urlFilterActive"], true);
    let page = operation(
        mcp,
        target,
        json!({"operation":"page_create","context":context,"select":false}),
    )
    .await["page"]
        .clone();
    operation(mcp,target,json!({"operation":"page_navigate","page":page,"navigation":{"kind":"goto","url":"recorded/page"}})).await;
    let one = format!("{base}recorded/one");
    let other = format!("{base}other");
    assert_eq!(fetch(mcp, target, &page, &one).await, "Acme network");
    assert_eq!(fetch(mcp, target, &page, &other).await, "Acme network");
    let until = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let status = operation(
            mcp,
            target,
            json!({"operation":"har_capture_status","context":context}),
        )
        .await;
        if status["entries"] == 2 && status["completedEntries"] == 2 {
            break;
        }
        assert!(tokio::time::Instant::now() < until, "{status}");
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    let request = json!({"operation":"context_close","context":context}).to_string();
    let started = std::time::Instant::now();
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &request])
        .await;
    let closed = completed(mcp, target, submitted).await;
    eprintln!("recordHar fresh CLI context close: {:?}", started.elapsed());
    let path = closed["har"]["reports"][0]["attachment"]["path"]
        .as_str()
        .unwrap_or_else(|| panic!("{closed}"))
        .to_owned();
    let archive: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    let urls = archive["log"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["request"]["url"].as_str().unwrap().to_owned())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        urls,
        [format!("{base}recorded/page"), one.clone()]
            .into_iter()
            .collect()
    );
    assert_eq!(archive["log"]["pages"].as_array().unwrap().len(), 1);
    assert!(
        archive["log"]["entries"]
            .as_array()
            .unwrap()
            .iter()
            .all(|entry| entry["response"]["content"]["text"] == "QWNtZSBuZXR3b3Jr"),
        "{archive}"
    );
    refuses(
        mcp,
        target,
        json!({"operation":"har_capture_status","context":context}),
        "HAR_CAPTURE_INACTIVE",
    )
    .await;
    operation(
        mcp,
        target,
        json!({"operation":"har_remove","archiveId":closed["har"]["archiveId"]}),
    )
    .await;
    assert!(!std::path::Path::new(&path).exists());
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
        [format!("{base}recorded/page"), one, other]
            .into_iter()
            .map(|url| (url.strip_prefix(&site.url).unwrap().to_owned(), 1))
            .collect()
    );
}

#[tokio::test]
async fn installed_step_four_har_record_checkpoint() {
    let fixture = installed_fixture();
    let site = Site::start().await;
    let mut mcp = Mcp::start(&fixture).await;
    for mode in ["headless", "headed"] {
        for engine in ["chrome", "firefox", "webkit"] {
            let connected = fixture
                .cli(&["browser", "connect", "--engine", engine, "--mode", mode])
                .await;
            let target = connected["targetId"].as_str().unwrap();
            fixture.ready().await;
            let started = std::time::Instant::now();
            let result = std::panic::AssertUnwindSafe(run(&fixture, &mut mcp, &site, target))
                .catch_unwind()
                .await;
            mcp.tool("disconnect", json!({"targetId":target})).await;
            eprintln!(
                "recordHar finished {engine} {mode}: {:?}",
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
