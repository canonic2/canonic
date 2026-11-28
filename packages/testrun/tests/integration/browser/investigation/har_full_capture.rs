//! Installed full-mode HAR capture: pages, cookies, query strings, versions, server address, sizes and timings.
use super::har_replay_filter::fetch;
use super::*;

async fn capture(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    context: &Value,
    page: &Value,
    mode: &str,
    url: &str,
) -> Value {
    let request = json!({"operation":"har_capture_start","context":context,"content":"embed","mode":mode,"urlFilter":{"kind":"exact","url":url}}).to_string();
    let started = std::time::Instant::now();
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &request])
        .await;
    let capture = completed(mcp, target, submitted).await;
    eprintln!(
        "HAR {mode} capture fresh CLI start: {:?}",
        started.elapsed()
    );
    assert_eq!(capture["mode"], mode);
    assert_eq!(fetch(mcp, target, page, url).await, "Acme network");
    let until = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let status = operation(
            mcp,
            target,
            json!({"operation":"har_capture_status","context":context}),
        )
        .await;
        if status["entries"] == 1 && status["completedEntries"] == 1 {
            break;
        }
        assert!(tokio::time::Instant::now() < until, "{status}");
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    let started = std::time::Instant::now();
    let stopped = operation(
        mcp,
        target,
        json!({"operation":"har_capture_stop","context":context}),
    )
    .await;
    eprintln!(
        "HAR {mode} capture retained MCP stop: {:?}",
        started.elapsed()
    );
    let path = stopped["reports"][0]["attachment"]["path"]
        .as_str()
        .unwrap();
    let archive: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    operation(
        mcp,
        target,
        json!({"operation":"har_remove","archiveId":stopped["archiveId"]}),
    )
    .await;
    assert!(!std::path::Path::new(path).exists());
    archive
}

async fn run(
    fixture: &Fixture,
    mcp: &mut Mcp,
    site: &Site,
    target: &str,
    engine: &str,
    context: &Value,
    page: &Value,
) {
    operation(mcp,target,json!({"operation":"page_navigate","page":page,"navigation":{"kind":"goto","url":site.url}})).await;
    operation(mcp,target,json!({"operation":"evaluate","scope":page,"isFunction":true,"expression":"()=>{document.cookie='acme=one; path=/';document.title='Acme full';return document.cookie}"})).await;
    let prefix = format!("/har-filter/{}", uuid::Uuid::new_v4());
    let full_url = format!("{}{prefix}/full?token=Acme&item=two", site.url);
    let archive = capture(fixture, mcp, target, context, page, "full", &full_url).await;
    let pages = archive["log"]["pages"].as_array().unwrap();
    assert_eq!(pages.len(), 1, "{archive}");
    let entry = &archive["log"]["entries"][0];
    assert_eq!(entry["pageref"], pages[0]["id"]);
    assert_eq!(pages[0]["title"], "Acme full", "{archive}");
    assert!(pages[0].get("_testrunTitleUnavailable").is_none());
    for timing in ["onContentLoad", "onLoad"] {
        assert!(
            pages[0]["pageTimings"][timing].as_f64().unwrap() > 0.0,
            "{archive}"
        );
    }
    assert_eq!(entry["request"]["httpVersion"], "HTTP/1.1");
    assert_eq!(entry["response"]["httpVersion"], "HTTP/1.1");
    assert!(
        entry["request"]["cookies"]
            .as_array()
            .unwrap()
            .contains(&json!({"name":"acme","value":"one"})),
        "{entry}"
    );
    assert_eq!(
        entry["request"]["queryString"],
        json!([{"name":"token","value":"Acme"},{"name":"item","value":"two"}])
    );
    assert_eq!(entry["request"]["bodySize"], 0);
    assert_eq!(entry["response"]["content"]["mimeType"], "text/plain");
    assert_eq!(entry["response"]["content"]["size"], 12);
    assert_eq!(entry["response"]["redirectURL"], "");
    assert_eq!(entry["serverIPAddress"], "127.0.0.1", "{entry}");
    let port = url::Url::parse(&site.url).unwrap().port().unwrap();
    assert_eq!(entry["_serverPort"], port);
    assert!(entry.get("_securityDetails").is_none());
    for field in ["dns", "connect", "ssl", "send", "wait", "receive"] {
        assert!(entry["timings"][field].is_number(), "{entry}");
    }
    assert!(entry["time"].as_f64().unwrap() >= 0.0);
    if engine == "chrome" {
        assert!(
            entry["response"]["_transferSize"].as_i64().unwrap() > 0,
            "{entry}"
        );
    } else {
        assert_eq!(entry["response"]["bodySize"], 12, "{entry}");
        assert!(
            entry["response"]["headersSize"].as_i64().unwrap() > 0,
            "{entry}"
        );
    }

    let minimal_url = format!("{}{prefix}/minimal?token=Acme", site.url);
    let archive = capture(fixture, mcp, target, context, page, "minimal", &minimal_url).await;
    let entry = &archive["log"]["entries"][0];
    assert!(archive["log"].get("pages").is_none(), "{archive}");
    for field in ["pageref", "timings", "time", "serverIPAddress"] {
        assert!(entry.get(field).is_none(), "{field}: {entry}");
    }
    assert!(entry["request"].get("queryString").is_none());
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
        [&full_url, &minimal_url]
            .into_iter()
            .map(|url| (url.strip_prefix(&site.url).unwrap().to_owned(), 1))
            .collect()
    );
}

#[tokio::test]
async fn installed_step_four_har_full_capture_checkpoint() {
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
                engine,
                &ready["browser"]["selectedContext"],
                &ready["browser"]["selectedPage"],
            ))
            .catch_unwind()
            .await;
            mcp.tool("disconnect", json!({"targetId":target})).await;
            eprintln!(
                "HAR full capture finished {engine} {mode}: {:?}",
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
