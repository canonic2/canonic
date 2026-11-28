//! Testrun's use of the shared documentation bundle: exact pages, error
//! mapping and cancellation of the blocking query.
use super::*;
use serde_json::json;
use std::time::Duration;

async fn query(name: &str, arguments: Value) -> Result<Value> {
    call(
        name,
        &arguments,
        Some(Instant::now() + Duration::from_secs(30)),
    )
    .await
}

#[tokio::test]
async fn paginated_reads_reproduce_every_bundled_page_byte_for_byte() {
    let mut paths = Vec::new();
    let mut cursor = None;
    loop {
        let list = query(
            "docs_list",
            canonic_mcp_kit::page::arguments(cursor.as_deref(), Some(2)),
        )
        .await
        .unwrap();
        assert_eq!(list["version"], env!("CARGO_PKG_VERSION"));
        for row in list["documents"].as_array().unwrap() {
            let path = row["path"].as_str().unwrap();
            let source = std::fs::read_to_string(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("docs")
                    .join(path),
            )
            .unwrap();
            let mut actual = String::new();
            let mut read_cursor = None;
            loop {
                let read = query("docs_read", {
                    let mut args =
                        canonic_mcp_kit::page::arguments(read_cursor.as_deref(), Some(7));
                    args["path"] = json!(path);
                    args
                })
                .await
                .unwrap();
                assert_eq!(read["total_lines"], row["total_lines"]);
                actual.push_str(read["content"].as_str().unwrap());
                let Some(next) = read["nextCursor"].as_str() else {
                    break;
                };
                read_cursor = Some(next.to_owned());
            }
            assert_eq!(actual, source, "{path}");
            paths.push(path.to_owned());
        }
        let Some(next) = list["nextCursor"].as_str() else {
            break;
        };
        cursor = Some(next.to_owned());
    }
    assert!(paths.contains(&"mcp.md".into()));
}

#[tokio::test]
async fn shared_failures_keep_testrun_codes_and_recovery_hints() {
    for (name, arguments) in [
        ("docs_list", json!({"limit":101})),
        ("docs_list", json!({"targetId":"android:Acme"})),
        ("docs_search", json!({"query":" \t"})),
        ("docs_read", json!({})),
    ] {
        let error = query(name, arguments.clone()).await.unwrap_err();
        assert_eq!(error.code, "INVALID_ARGUMENT", "{name} {arguments}");
    }
    for (name, arguments) in [
        ("docs_read", json!({"path":"../README.md"})),
        ("docs_search", json!({"path":"MCP.md","query":"testrun"})),
    ] {
        let error = query(name, arguments).await.unwrap_err();
        assert_eq!(error.code, "NOT_FOUND");
        assert!(error.message.contains("docs_list"), "{}", error.message);
    }
    let unavailable = failure(
        "docs_read",
        &DocsError::Unavailable,
        &Control::new(Duration::from_secs(30)),
    );
    assert_eq!(unavailable.code, "INTERNAL");
}

#[tokio::test]
async fn cancellation_and_deadlines_reach_the_blocking_query() {
    let search = json!({"query":"testrun"});
    let cancelled = Arc::new(Control::new(Duration::from_secs(30)));
    cancelled.cancel();
    let error = run("docs_search", &search, cancelled).await.unwrap_err();
    assert_eq!(error.code, "CANCELLED");
    let expired = call("docs_search", &search, Some(Instant::now()))
        .await
        .unwrap_err();
    assert_eq!(expired.code, "TIMEOUT");

    // Dropping the request's future, as cancellation and the transport's
    // timeout do, signals the query already running on its blocking thread.
    let control = Arc::new(Control::new(Duration::from_secs(30)));
    {
        let pending = run("docs_search", &search, control.clone());
        tokio::pin!(pending);
        assert!(futures_util::poll!(&mut pending).is_pending());
        assert!(!control.cancelled());
    }
    assert!(control.cancelled());
    assert!(query("docs_search", search).await.is_ok());
}
