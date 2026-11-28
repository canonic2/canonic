//! Installed full-mode WebSocket HAR entries and caller-path archive export.
use super::har_replay_filter::refuses;
use super::socket_protocols::SocketSite;
use super::*;

fn private_directory(fixture: &Fixture) -> std::path::PathBuf {
    let directory = fixture
        .workspace
        .path()
        .join(format!("har-export-{}", uuid::Uuid::new_v4()));
    let mut builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(&directory).unwrap();
    directory
}
fn zip_har(path: &str) -> Value {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    let mut har = String::new();
    std::io::Read::read_to_string(&mut zip.by_name("har.har").unwrap(), &mut har).unwrap();
    serde_json::from_str(&har).unwrap()
}
/// An attached socket entry's messages, read from its `.jsonl` member.
fn zip_messages(path: &str, entry: &Value) -> Vec<Value> {
    assert!(entry.get("_webSocketMessages").is_none(), "{entry}");
    let name = entry["response"]["content"]["_file"].as_str().unwrap();
    assert!(name.ends_with(".jsonl"), "{name}");
    let mut zip = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    let mut lines = String::new();
    std::io::Read::read_to_string(&mut zip.by_name(name).unwrap(), &mut lines).unwrap();
    lines
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}
#[expect(
    clippy::too_many_lines,
    reason = "One owned workflow checks socket entries, export refusals, export bytes and reimport"
)]
async fn run(
    fixture: &Fixture,
    mcp: &mut Mcp,
    site: &Site,
    target: &str,
    (context, page): (&Value, &Value),
) {
    operation(mcp,target,json!({"operation":"page_navigate","page":page,"navigation":{"kind":"goto","url":site.url}})).await;
    let socket = SocketSite::start().await;
    let directory = private_directory(fixture);
    let export = directory.join("capture.zip");
    let existing = directory.join("existing.har");
    std::fs::write(&existing, b"{}").unwrap();
    refuses(
        mcp,
        target,
        json!({"operation":"har_capture_start","context":context,"path":existing}),
        "HAR_EXPORT_EXISTS",
    )
    .await;
    refuses(
        mcp,
        target,
        json!({"operation":"har_capture_start","context":context,"path":"relative.har"}),
        "HAR_EXPORT_PATH_INVALID",
    )
    .await;
    let request = json!({"operation":"har_capture_start","context":context,"content":"attach","mode":"full","path":export,"urlFilter":{"kind":"glob","pattern":format!("**/protocol-socket?token=*")}}).to_string();
    let started = std::time::Instant::now();
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &request])
        .await;
    let capture = completed(mcp, target, submitted).await;
    eprintln!("Socket capture fresh CLI start: {:?}", started.elapsed());
    assert_eq!(capture["exportConfigured"], true);
    let url = format!("{}?token=Acme", socket.url);
    let expression = format!(
        "()=>new Promise(resolve=>{{const socket=new WebSocket({},['Acme.v1','Acme.v2']);socket.onopen=()=>socket.send('Acme hello');socket.onmessage=()=>socket.close(3007,'Acme protocol close');socket.onclose=event=>resolve(event.code);}})",
        json!(url)
    );
    let closed = operation(
        mcp,
        target,
        json!({"operation":"evaluate","scope":page,"isFunction":true,"expression":expression}),
    )
    .await;
    assert_eq!(closed["value"]["value"], 3007);
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
        "Socket capture retained MCP stop with export: {:?}",
        started.elapsed()
    );
    let published = stopped["reports"][0]["attachment"]["path"]
        .as_str()
        .unwrap()
        .to_owned();
    for (label, path) in [
        ("published", published.as_str()),
        ("export", export.to_str().unwrap()),
    ] {
        let har = zip_har(path);
        let entries = har["log"]["entries"].as_array().unwrap();
        assert_eq!(entries.len(), 1, "{label}: {har}");
        let entry = &entries[0];
        assert_eq!(entry["_resourceType"], "websocket", "{label}");
        assert_eq!(entry["request"]["url"], url, "{label}");
        assert_eq!(entry["response"]["status"], 101, "{label}");
        assert!(entry["pageref"].is_string(), "{label}: {entry}");
        let messages = zip_messages(path, entry)
            .iter()
            .map(|message| {
                (
                    message["type"].as_str().unwrap().to_owned(),
                    message["data"].as_str().unwrap().to_owned(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            messages,
            [
                ("send".to_owned(), "Acme hello".to_owned()),
                ("receive".to_owned(), "Acme negotiated".to_owned())
            ],
            "{label}"
        );
    }
    assert_eq!(stopped["export"]["format"], "zip");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&export).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    let reimported = operation(
        mcp,
        target,
        json!({"operation":"har_register","path":export}),
    )
    .await;
    for archive in [&reimported, &stopped] {
        operation(
            mcp,
            target,
            json!({"operation":"har_remove","archiveId":archive["archiveId"]}),
        )
        .await;
    }
    assert!(!std::path::Path::new(&published).exists());
    assert!(export.exists(), "Caller exports belong to the caller");
}

#[tokio::test]
async fn installed_step_four_har_socket_export_checkpoint() {
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
                (
                    &ready["browser"]["selectedContext"],
                    &ready["browser"]["selectedPage"],
                ),
            ))
            .catch_unwind()
            .await;
            mcp.tool("disconnect", json!({"targetId":target})).await;
            eprintln!(
                "HAR socket export finished {engine} {mode}: {:?}",
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
