//! Focus inspection failure is a known refusal, before any Android text input.
use super::*;

#[tokio::test]
async fn unicode_replacement_requires_a_unique_focused_target_and_is_never_replayed() {
    let serial = format!("Acme-replace-{}", uuid::Uuid::new_v4().simple());
    let mut mcp = Mcp::start_named(&serial).await;
    let path = mcp._directory.path().join("ui.xml");
    let tree = r#"<hierarchy scope="active_window" window-id="1"><node class="android.widget.EditText" resource-id="acme.field" text="" focused="true" enabled="true" bounds="[0,0][100,40]"/></hierarchy>"#;
    std::fs::write(&path, tree).unwrap();
    network_recovery::inspection_tree(&mcp, "cat ui.xml");
    network_recovery::rewrite_adb(
        &mcp,
        "    *acme-unhandled*)",
        r#"    *io.canonic.testrun.focus.Main\ replace*)
      count=$(printf '%s' "$4" | awk '/^dd bs=1 / {sub(/^count=/,"",$3); print $3}')
      dd bs=1 count="$count" of=/dev/null 2>/dev/null
      cat > request.json
      if [ -f stale ]; then printf '{"version":1,"code":"STALE_ELEMENT"}'
      elif [ -f lose-reply ]; then echo dispatched >> replacements; exit 1
      else echo dispatched >> replacements; printf '{"version":1,"completed":true}'; fi ;;
    *io.canonic.testrun.focus.Main\ focus*) cat > /dev/null; printf '{"version":1,"focusedEditable":true}' ;;
    *acme-unhandled*)"#,
    );
    assert_eq!(
        mcp.tool("connect", json!({"targetId":format!("android:{serial}")}))
            .await["ok"],
        true
    );
    let action = json!({"action":"type_text","text":"Acme café 日本語 😀 %s","replace":true,"element":{"query":{"identifier":"acme.field"}}});
    let typed = mcp.tool("act", json!({"actions":[action]})).await;
    assert_eq!(typed["ok"], true, "{typed}");
    assert!(!typed.to_string().contains("café"), "{typed}");
    let request: Value =
        serde_json::from_slice(&std::fs::read(mcp._directory.path().join("request.json")).unwrap())
            .unwrap();
    assert_eq!(request["text"], action["text"]);
    assert_eq!(request["index"], 1);
    assert_eq!(request["digest"].as_str().unwrap().len(), 64);
    std::fs::write(&path, tree.replace("focused=\"true\"", "focused=\"false\"")).unwrap();
    let refused = mcp.tool("act", json!({"actions":[action]})).await;
    assert!(
        refused.to_string().contains("FOCUS_NOT_PROVEN"),
        "{refused}"
    );
    assert_eq!(
        std::fs::read_to_string(mcp._directory.path().join("replacements"))
            .unwrap()
            .lines()
            .count(),
        1
    );
    std::fs::write(&path, tree).unwrap();
    mcp.arm("stale");
    let refused = mcp.tool("act", json!({"actions":[action]})).await;
    assert!(refused.to_string().contains("STALE_ELEMENT"), "{refused}");
    std::fs::remove_file(mcp._directory.path().join("stale")).unwrap();
    mcp.arm("lose-reply");
    let unknown = mcp.tool("act", json!({"actions":[action]})).await;
    assert!(unknown.to_string().contains("outcome_unknown"), "{unknown}");
    assert_eq!(
        std::fs::read_to_string(mcp._directory.path().join("replacements"))
            .unwrap()
            .lines()
            .count(),
        2
    );
    assert_eq!(mcp.tool("disconnect", json!({})).await["ok"], true);
}

#[tokio::test]
async fn stalled_focus_read_refuses_text_without_recycling_controller() {
    let serial = format!("Acme-focus-{}", uuid::Uuid::new_v4().simple());
    let mut mcp = Mcp::start_named(&serial).await;
    network_recovery::rewrite_adb(
        &mcp,
        "  shell) printf",
        "  shell) if [ \"$4\" = -T ]; then set -- \"$1\" \"$2\" \"$3\" \"$5\"; fi; printf",
    );
    network_recovery::rewrite_adb(
        &mcp,
        "    *acme-unhandled*)",
        r#"    *io.canonic.testrun.focus.Main*)
      cat > /dev/null
      if [ -f stall-focus ]; then exec sleep 30; fi
      if [ -f no-focus ]; then printf '{"version":1,"focusedEditable":false}\n'
      else printf '{"version":1,"focusedEditable":true}\n'; fi ;;
    *acme-unhandled*)"#,
    );
    let connected = mcp
        .tool("connect", json!({"targetId":format!("android:{serial}")}))
        .await;
    assert_eq!(connected["ok"], true, "{connected}");
    let before = mcp.target_status().await;
    mcp.arm("stall-focus");
    let refused = mcp
        .tool(
            "act",
            json!({"actions":[{"action":"type_text","text":"abc"}]}),
        )
        .await;
    assert_eq!(refused["code"], "BATCH_FAILED", "{refused}");
    assert!(
        refused
            .to_string()
            .contains("FOCUS_VERIFICATION_UNAVAILABLE"),
        "{refused}"
    );
    assert!(
        refused.to_string().contains("\"inputDispatched\":false"),
        "{refused}"
    );
    assert!(!mcp.commands().iter().any(|s| s.contains("'input' 'text'")));
    let after = mcp.target_status().await;
    assert!(
        before["data"]["health"]["target"]["generation"].is_string(),
        "{before}"
    );
    assert_eq!(
        before["data"]["health"]["target"]["generation"],
        after["data"]["health"]["target"]["generation"],
        "before={before} after={after}"
    );
    std::fs::remove_file(mcp._directory.path().join("stall-focus")).unwrap();
    mcp.arm("no-focus");
    let unfocused = mcp
        .tool(
            "act",
            json!({"actions":[{"action":"type_text","text":"abc"}]}),
        )
        .await;
    assert!(
        unfocused.to_string().contains("FOCUS_NOT_PROVEN"),
        "{unfocused}"
    );
    assert!(!mcp.commands().iter().any(|s| s.contains("'input' 'text'")));
    std::fs::remove_file(mcp._directory.path().join("no-focus")).unwrap();
    let typed = mcp
        .tool(
            "act",
            json!({"actions":[{"action":"type_text","text":"abc"}]}),
        )
        .await;
    assert_eq!(typed["ok"], true, "{typed}");
    assert_eq!(
        mcp.commands()
            .iter()
            .filter(|s| s.contains("'input' 'text'"))
            .count(),
        1
    );
    assert_eq!(mcp.tool("disconnect", json!({})).await["ok"], true);
}
