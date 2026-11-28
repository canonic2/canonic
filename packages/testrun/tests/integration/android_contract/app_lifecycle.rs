//! Managed app restart and observed launch readiness through the shipping MCP.
use super::*;

/// The first action's result, or its retained outcome after a failed act.
fn step(reply: &Value) -> Value {
    if reply["ok"] == true {
        reply["data"]["result"]["steps"][0].clone()
    } else {
        reply["details"]["result"]["lastAction"]["actionOutcomes"][0].clone()
    }
}

/// A connected fake target whose UI Automator dump shows the app's home label.
async fn connected() -> Mcp {
    let serial = format!("Acme-restart-{}", uuid::Uuid::new_v4().simple());
    let mut mcp = Mcp::start_named(&serial).await;
    std::fs::write(
        mcp._directory.path().join("ui.xml"),
        r#"<?xml version="1.0"?><hierarchy rotation="0"><node class="android.widget.FrameLayout" package="com.example.acme" bounds="[0,0][100,200]"><node class="android.widget.TextView" resource-id="com.example.acme:id/home" text="Home" package="com.example.acme" bounds="[0,0][100,40]"/></node></hierarchy>"#,
    )
    .unwrap();
    super::network_recovery::inspection_tree(&mcp, "cat ui.xml");
    let connected = mcp
        .tool("connect", json!({"targetId":format!("android:{serial}")}))
        .await;
    assert_eq!(connected["ok"], true, "{connected}");
    mcp
}

#[tokio::test]
async fn launch_environment_reaches_the_platform_adapter() {
    let mut mcp = connected().await;
    let starts = |mcp: &Mcp| {
        mcp.commands()
            .iter()
            .filter(|line| line.starts_with("'am' 'start'"))
            .count()
    };
    // `env` is an accepted launch field: an empty environment launches.
    let launched = mcp
        .tool(
            "act",
            json!({"actions":[{"action":"app_launch","appId":"com.example.acme","env":{}}],"observeAfterEach":false}),
        )
        .await;
    assert_eq!(launched["ok"], true, "{launched}");
    let before = starts(&mcp);
    // The Android adapter, not the input field list, refuses process
    // environment, and no launch is sent.
    let refused = mcp
        .tool(
            "act",
            json!({"actions":[{"action":"app_launch","appId":"com.example.acme","env":{"API_URL":"https://staging.example.com"}}],"observeAfterEach":false}),
        )
        .await;
    assert_eq!(refused["code"], "CAPABILITY_UNAVAILABLE", "{refused}");
    assert_eq!(refused["details"]["result"]["inputDispatched"], false);
    assert_eq!(starts(&mcp), before);
    let closed = mcp.tool("disconnect", json!({})).await;
    assert_eq!(closed["data"]["closed"], true, "{closed}");
}

#[tokio::test]
async fn managed_restart_reports_stages_and_observed_readiness_without_replay() {
    let mut mcp = connected().await;
    let restart = json!({"action":"app_restart","appId":"com.example.acme","readiness":{"element":{"identifier":"com.example.acme:id/home"},"timeoutMs":3000}});

    // Terminate, launch, then the focused window and the requested element.
    let ready = mcp
        .tool("act", json!({"actions":[restart],"observeAfterEach":false}))
        .await;
    assert_eq!(ready["ok"], true, "{ready}");
    let stages = &step(&ready)["data"]["result"];
    assert_eq!(stages["terminate"]["state"], "completed", "{ready}");
    assert_eq!(stages["launch"]["state"], "completed", "{ready}");
    assert_eq!(stages["readiness"]["state"], "ready", "{ready}");
    assert_eq!(stages["readiness"]["condition"], "foreground_element");
    assert_eq!(
        stages["readiness"]["observed"]["focusedPackage"],
        "com.example.acme"
    );
    assert_eq!(
        stages["readiness"]["element"]["identifier"],
        "com.example.acme:id/home"
    );
    let lifecycle = |mcp: &Mcp| -> Vec<String> {
        mcp.commands()
            .into_iter()
            .filter(|line| line.contains("'am' '") || line.contains("resolve-activity"))
            .collect()
    };
    assert_eq!(
        lifecycle(&mcp),
        [
            "'am' 'force-stop' 'com.example.acme'",
            "'cmd' 'package' 'resolve-activity' '--brief' 'com.example.acme'",
            "'am' 'start' '-W' '-n' 'com.example.acme/.MainActivity'",
        ]
    );

    // Launch completes but the app never takes focus: readiness is reported
    // separately and the step fails without another launch.
    mcp.arm("no-focus");
    std::fs::remove_file(mcp._directory.path().join("focus")).unwrap();
    let unready = mcp
        .tool(
            "act",
            json!({"actions":[{"action":"app_launch","appId":"com.example.acme","readiness":{"timeoutMs":600}}],"observeAfterEach":false}),
        )
        .await;
    assert_eq!(unready["ok"], false, "{unready}");
    let error = &step(&unready)["error"];
    assert_eq!(error["code"], "APP_NOT_READY", "{unready}");
    assert_eq!(error["details"]["stages"]["launch"]["state"], "completed");
    assert_eq!(
        error["details"]["stages"]["readiness"]["state"],
        "not_ready"
    );
    assert_eq!(
        error["details"]["stages"]["readiness"]["lastObserved"]["focusedPackage"],
        "com.example.launcher"
    );
    assert_eq!(
        lifecycle(&mcp)
            .iter()
            .filter(|line| line.starts_with("'am' 'start'"))
            .count(),
        2
    );
    std::fs::remove_file(mcp._directory.path().join("no-focus")).unwrap();

    // A failed launch keeps the completed terminate stage; neither stage reruns.
    mcp.arm("fail-launch");
    let before = lifecycle(&mcp).len();
    let partial = mcp
        .tool("act", json!({"actions":[restart],"observeAfterEach":false}))
        .await;
    assert_eq!(partial["ok"], false, "{partial}");
    let error = &step(&partial)["error"];
    let stages = &error["details"]["stages"];
    assert_eq!(stages["terminate"]["state"], "completed", "{partial}");
    assert_eq!(stages["launch"]["state"], "failed", "{partial}");
    assert_eq!(stages["readiness"]["state"], "not_started", "{partial}");
    assert_eq!(error["details"]["replayed"], false);
    assert_eq!(
        lifecycle(&mcp)[before..],
        [
            "'am' 'force-stop' 'com.example.acme'",
            "'cmd' 'package' 'resolve-activity' '--brief' 'com.example.acme'",
            "'am' 'start' '-W' '-n' 'com.example.acme/.MainActivity'",
        ]
    );
    let closed = mcp.tool("disconnect", json!({})).await;
    assert_eq!(closed["data"]["closed"], true, "{closed}");
}
