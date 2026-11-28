//! Shipping managed semantic reads, wait evidence and pre-dispatch refusal.
use super::*;

#[tokio::test]
async fn managed_semantic_reads_waits_and_stale_target_refusal() {
    let serial = format!("Acme-semantic-{}", uuid::Uuid::new_v4().simple());
    let mut mcp = Mcp::start_named(&serial).await;
    let tree = r#"<hierarchy scope="active_window"><node class="android.widget.FrameLayout" bounds="[0,0][100,200]"><node class="android.widget.Button" resource-id="acme.home" text="Home" enabled="true" bounds="[0,0][100,40]"/><node class="android.widget.EditText" resource-id="acme.secret" password="true" text="private-value" bounds="[0,40][100,80]"/></node></hierarchy>"#;
    let path = mcp._directory.path().join("ui.xml");
    std::fs::write(&path, tree).unwrap();
    network_recovery::inspection_tree(&mcp, "cat ui.xml");
    let connected = mcp
        .tool("connect", json!({"targetId":format!("android:{serial}")}))
        .await;
    assert_eq!(connected["ok"], true, "{connected}");
    let read = mcp
        .tool(
            "ui_read",
            json!({"element":{"query":{"identifier":"acme.home"}}}),
        )
        .await;
    assert_eq!(read["ok"], true, "{read}");
    assert_eq!(read["data"]["quality"], "complete");
    assert_eq!(read["data"]["element"]["label"], "Home");
    let reference = json!({"snapshotId":read["data"]["snapshotId"],"elementId":read["data"]["element"]["elementId"]});
    let fresh = mcp.tool("ui_read", json!({"element":reference})).await;
    assert_eq!(fresh["ok"], true, "{fresh}");
    let ambiguous = mcp
        .tool(
            "ui_read",
            json!({"element":{"query":{"label":{"text":"","mode":"substring"}}}}),
        )
        .await;
    assert_eq!(ambiguous["code"], "AMBIGUOUS_ELEMENT", "{ambiguous}");
    assert!(!ambiguous.to_string().contains("private-value"));
    check_waits(&mut mcp).await;
    check_input(&mut mcp, reference, &path, tree).await;
    check_partial(&mut mcp, &path, tree).await;
    let closed = mcp.tool("disconnect", json!({})).await;
    assert_eq!(closed["data"]["closed"], true, "{closed}");
}

async fn check_partial(mcp: &mut Mcp, path: &std::path::Path, tree: &str) {
    std::fs::write(
        path,
        tree.replace(
            "scope=\"active_window\"",
            "scope=\"active_window\" truncated=\"true\"",
        ),
    )
    .unwrap();
    let single = mcp
        .tool(
            "ui_read",
            json!({"element":{"query":{"identifier":"acme.home"}},"allowPartial":true}),
        )
        .await;
    assert_eq!(single["code"], "UNIQUENESS_UNPROVEN", "{single}");
    let gone = mcp.tool("wait", json!({"conditions":[{"kind":"gone","query":{"identifier":"acme.missing"}}],"allowPartial":true,"timeoutMs":0})).await;
    assert_eq!(gone["data"]["state"], "unsatisfied", "{gone}");
    assert!(
        gone.to_string().contains("\"absenceConclusive\":false"),
        "{gone}"
    );
}

async fn check_waits(mcp: &mut Mcp) {
    let ready = mcp.tool("wait",json!({"conditions":[{"kind":"state","query":{"identifier":"acme.home"},"state":"enabled","equals":true}],"timeoutMs":1000})).await;
    assert_eq!(ready["ok"], true, "{ready}");
    let value = mcp.tool("wait",json!({"conditions":[{"kind":"value","query":{"identifier":"acme.secret"},"value":{"text":"","mode":"substring"}}],"timeoutMs":0})).await;
    assert_eq!(value["data"]["state"], "unsatisfied", "{value}");
    assert!(!value.to_string().contains("private-value"));
    let gone = mcp
        .tool(
            "wait",
            json!({"conditions":[{"kind":"gone","query":{"identifier":"acme.missing"}}],"timeoutMs":0}),
        )
        .await;
    assert_eq!(gone["data"]["state"], "satisfied", "{gone}");
    let timeout = mcp.tool("wait",json!({"conditions":[{"kind":"exists","query":{"identifier":"acme.missing"}}],"timeoutMs":600})).await;
    assert_eq!(timeout["code"], "TIMEOUT", "{timeout}");
    assert_eq!(
        timeout["message"],
        "The requested condition was not established before its deadline"
    );
    assert!(timeout["details"].to_string().contains("lastObserved"));
}

async fn check_input(mcp: &mut Mcp, reference: Value, path: &std::path::Path, tree: &str) {
    let tapped = mcp
        .tool(
            "act",
            json!({"actions":[{"action":"tap","element":{"query":{"identifier":"acme.home"}}}]}),
        )
        .await;
    assert_eq!(tapped["ok"], true, "{tapped}");
    std::fs::write(path, tree.replace("Home", "Changed")).unwrap();
    let before = mcp
        .commands()
        .iter()
        .filter(|s| s.contains("'input' 'tap'"))
        .count();
    let stale = mcp
        .tool(
            "act",
            json!({"actions":[{"action":"tap","element":reference}]}),
        )
        .await;
    assert_eq!(stale["ok"], false, "{stale}");
    assert!(stale.to_string().contains("STALE_ELEMENT"), "{stale}");
    assert_eq!(
        mcp.commands()
            .iter()
            .filter(|s| s.contains("'input' 'tap'"))
            .count(),
        before
    );
}
