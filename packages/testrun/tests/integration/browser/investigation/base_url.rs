//! Installed context baseURL resolution and attached HAR capture publication.
use super::har_replay_filter::{archive, fetch, refuses};
use super::socket_protocols::{SocketSite, relay};
use super::*;

fn context_request(base: &str) -> Value {
    json!({"operation":"context_create","options":{"viewport":{"width":800,"height":600},"deviceScaleFactor":1,"baseURL":base}})
}
async fn settled(mcp: &mut Mcp, target: &str, context: &Value, expected: u64) {
    let until = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let status = operation(
            mcp,
            target,
            json!({"operation":"har_capture_status","context":context}),
        )
        .await;
        if status["entries"] == expected && status["completedEntries"] == expected {
            return;
        }
        assert!(
            tokio::time::Instant::now() < until,
            "Attached capture did not settle: {status}"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}
fn member(path: &str, name: &str) -> Vec<u8> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(&mut zip.by_name(name).unwrap(), &mut bytes).unwrap();
    bytes
}
#[expect(
    clippy::too_many_lines,
    reason = "One owned workflow checks base validation, relative navigation, replay and capture filters, attached publication, reimport, WebSocket routing and exact server counts"
)]
async fn run(fixture: &Fixture, mcp: &mut Mcp, site: &Site, target: &str, default_page: &Value) {
    refuses(
        mcp,
        target,
        context_request("/relative"),
        "INVALID_BASE_URL",
    )
    .await;
    refuses(mcp,target,json!({"operation":"page_navigate","page":default_page,"navigation":{"kind":"goto","url":"landing?token=Acme"}}),"INVALID_NAVIGATION_URL").await;
    let prefix = format!("/har-filter/{}", uuid::Uuid::new_v4());
    let base = format!("{}{prefix}/app/", site.url);
    let started = std::time::Instant::now();
    let submitted = fixture
        .cli(&[
            "browser",
            "send",
            "--target",
            target,
            &context_request(&base).to_string(),
        ])
        .await;
    let context = completed(mcp, target, submitted).await["context"].clone();
    eprintln!("Base context fresh CLI create: {:?}", started.elapsed());
    let page = operation(
        mcp,
        target,
        json!({"operation":"page_create","context":context,"select":false}),
    )
    .await["page"]
        .clone();
    operation(mcp,target,json!({"operation":"page_navigate","page":page,"navigation":{"kind":"goto","url":"landing?token=Acme"}})).await;
    let location = operation(mcp,target,json!({"operation":"evaluate","scope":page,"isFunction":true,"expression":"()=>location.href"})).await;
    assert_eq!(
        location["value"]["value"],
        format!("{base}landing?token=Acme")
    );
    for condition in [
        json!({"kind":"url","value":"landing?token=Acme"}),
        json!({"kind":"url_match","matcher":{"kind":"glob","pattern":"landing?token=*"}}),
        json!({"kind":"url_match","matcher":{"kind":"regex","pattern":r"/APP/landing\?token=Acme$","flags":"i"}}),
    ] {
        let started = std::time::Instant::now();
        let waited = operation(
            mcp,
            target,
            json!({"operation":"page_wait","page":page,"condition":condition}),
        )
        .await;
        eprintln!("Base page URL wait: {:?}", started.elapsed());
        assert_eq!(waited["satisfied"], true, "{waited}");
    }
    refuses(mcp,target,json!({"operation":"page_wait","page":page,"condition":{"kind":"url_match","matcher":{"kind":"regex","pattern":"(?=Acme"}}}),"ROUTE_REGEX_INVALID").await;

    operation(
        mcp,
        target,
        json!({"operation":"diagnostics_start","context":context}),
    )
    .await;
    let before = operation(
        mcp,
        target,
        json!({"operation":"diagnostics_read","context":context,"query":{}}),
    )
    .await;
    let waited = format!("{base}waited");
    assert_eq!(fetch(mcp, target, &page, &waited).await, "Acme network");
    let started = std::time::Instant::now();
    let found = operation(mcp,target,json!({"operation":"diagnostics_wait","context":context,"after":before["latestSequence"],"conditions":[{"event":"response","url":"waited","status":200}]})).await;
    eprintln!("Base relative diagnostics wait: {:?}", started.elapsed());
    assert!(found.to_string().contains(&waited), "{found}");
    operation(
        mcp,
        target,
        json!({"operation":"diagnostics_stop","context":context}),
    )
    .await;
    let replayed = format!("{base}replayed?token=Acme");
    let imported = archive(
        fixture,
        mcp,
        target,
        std::slice::from_ref(&replayed),
        "Acme base archive",
    )
    .await;
    let started = std::time::Instant::now();
    operation(mcp,target,json!({"operation":"context_har_set","context":context,"archiveId":imported["archiveId"],"urlFilter":{"kind":"glob","pattern":"replayed?token=*"}})).await;
    eprintln!("Base relative replay retained MCP: {:?}", started.elapsed());
    assert_eq!(
        fetch(mcp, target, &page, &replayed).await,
        "Acme base archive"
    );
    let unmatched = format!("{base}unmatched?token=Acme");
    assert_eq!(fetch(mcp, target, &page, &unmatched).await, "Acme network");
    operation(
        mcp,
        target,
        json!({"operation":"context_har_set","context":context,"archiveId":null}),
    )
    .await;

    let captured = format!("{base}captured?token=Acme");
    let started = std::time::Instant::now();
    let capture = operation(mcp,target,json!({"operation":"har_capture_start","context":context,"content":"attach","urlFilter":{"kind":"exact","url":"captured?token=Acme"}})).await;
    eprintln!(
        "Base relative attached capture start: {:?}",
        started.elapsed()
    );
    assert_eq!(capture["content"], "attach");
    assert_eq!(fetch(mcp, target, &page, &captured).await, "Acme network");
    let excluded = format!("{base}other-capture?token=Acme");
    assert_eq!(fetch(mcp, target, &page, &excluded).await, "Acme network");
    settled(mcp, target, &context, 1).await;
    let request = json!({"operation":"har_capture_stop","context":context}).to_string();
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &request])
        .await;
    let stopped = completed(mcp, target, submitted).await;
    assert_eq!(stopped["reports"][0]["format"], "har-1.2-capture-attached");
    let path = stopped["reports"][0]["attachment"]["path"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(path.ends_with(".zip"), "{path}");
    let har: Value = serde_json::from_slice(&member(&path, "har.har")).unwrap();
    let entries = har["log"]["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 1, "{har}");
    assert_eq!(entries[0]["request"]["url"], captured);
    let file = entries[0]["response"]["content"]["_file"].as_str().unwrap();
    assert!(file.ends_with(".txt"), "{file}");
    assert!(entries[0]["response"]["content"].get("text").is_none());
    assert_eq!(member(&path, file), b"Acme network");
    let reimported = operation(mcp, target, json!({"operation":"har_register","path":path})).await;
    operation(
        mcp,
        target,
        json!({"operation":"page_har_set","page":page,"archiveId":reimported["archiveId"]}),
    )
    .await;
    assert_eq!(fetch(mcp, target, &page, &captured).await, "Acme network");
    operation(
        mcp,
        target,
        json!({"operation":"page_har_set","page":page,"archiveId":null}),
    )
    .await;
    for archive in [&imported, &reimported, &stopped] {
        operation(
            mcp,
            target,
            json!({"operation":"har_remove","archiveId":archive["archiveId"]}),
        )
        .await;
    }
    assert!(!std::path::Path::new(&path).exists());
    operation(
        mcp,
        target,
        json!({"operation":"context_close","context":context}),
    )
    .await;

    let socket = SocketSite::start().await;
    let origin = socket
        .url
        .strip_prefix("ws://")
        .unwrap()
        .split('/')
        .next()
        .unwrap()
        .to_owned();
    let socket_context = operation(mcp, target, context_request(&format!("http://{origin}/")))
        .await["context"]
        .clone();
    let socket_page = operation(
        mcp,
        target,
        json!({"operation":"page_create","context":socket_context,"select":false}),
    )
    .await["page"]
        .clone();
    let route=operation(mcp,target,json!({"operation":"web_socket_route_add_matcher","scope":socket_context,"matcher":{"kind":"exact","url":"protocol-socket?token=Acme.private"},"sdkDefaults":false})).await;
    relay(
        mcp,
        target,
        &socket_page,
        &socket_context,
        &route,
        &format!("{}?token=Acme.private", socket.url),
    )
    .await;
    operation(
        mcp,
        target,
        json!({"operation":"context_close","context":socket_context}),
    )
    .await;

    let expected = [
        format!("{base}landing?token=Acme"),
        waited,
        unmatched,
        captured,
        excluded,
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
}

#[tokio::test]
async fn installed_step_four_base_url_attached_capture_checkpoint() {
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
                &ready["browser"]["selectedPage"],
            ))
            .catch_unwind()
            .await;
            mcp.tool("disconnect", json!({"targetId":target})).await;
            eprintln!(
                "Base URL checkpoint finished {engine} {mode}: {:?}",
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
