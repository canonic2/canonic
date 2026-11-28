//! Real Rust Android operations against an explicitly selected, owned emulator.
use serde_json::{Value, json};
use std::time::Duration;
use testrun::qa::Server;

#[tokio::test]
async fn android_capture_queries_input_and_cleanup() {
    let serial = std::env::var("TESTRUN_TEST_ANDROID").expect("Select an owned emulator");
    assert!(
        serial.starts_with("emulator-"),
        "Physical devices are outside this acceptance test"
    );
    let adapter = testrun::android::Android::default();
    let name = testrun::process::run(
        &adapter.adb,
        &["-s", &serial, "emu", "avd", "name"],
        Duration::from_secs(5),
        4096,
    )
    .await
    .unwrap();
    assert!(
        std::str::from_utf8(&name.bytes)
            .unwrap()
            .starts_with("Testrun-QA-")
    );
    let target_id = format!("android:{serial}");
    let mut server = Server::new();
    let readiness = server
        .call("doctor", json!({"targetId":target_id}))
        .await
        .unwrap();
    assert_eq!(readiness["ready"], true, "{readiness}");
    assert_eq!(readiness["inputReadiness"], "unknown");
    assert!(readiness["checks"]["os"]["apiLevel"].as_u64().unwrap() > 0);
    let targets = server
        .call("target_list", json!({"kind":"android","limit":200}))
        .await
        .unwrap();
    let target = targets["targets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["targetId"] == target_id)
        .unwrap();
    let run = server
        .call("run.begin", json!({"goalIds":["android"]}))
        .await
        .unwrap();
    let session = server.call("session_open", json!({"runId":run["runId"],"goalId":"android","target":{"targetId":target["targetId"],"generation":target["generation"]},"mode":"control"})).await.unwrap();
    let reference =
        json!({"runId":run["runId"],"goalId":"android","sessionId":session["sessionId"]});
    let mut command = reference.clone();
    command["leaseId"] = session["leaseId"].clone();
    command["commandId"] = json!("settings-launch");
    command["appId"] = json!("com.android.settings");
    let launched = server.call("app_launch", command.clone()).await.unwrap();
    assert_eq!(launched["state"], "completed");
    assert_eq!(server.call("app_launch", command).await.unwrap(), launched);
    let screenshot = server.call("screenshot", reference.clone()).await.unwrap();
    assert_eq!(screenshot["source"], "adb_screencap");
    assert!(screenshot["width"].as_u64().unwrap() > 0);
    let image = std::fs::read(screenshot["path"].as_str().unwrap()).unwrap();
    testrun::ui::png_dimensions(&image).unwrap();
    let mut observation = reference.clone();
    observation["scope"] = json!({"kind":"display"});
    observation["allowPartial"] = json!(true);
    let tree = server.call("ui_snapshot", observation).await.unwrap();
    assert!(tree["data"]["nodeCount"].as_u64().unwrap() > 0);
    let mut authority = reference.clone();
    authority["leaseId"] = session["leaseId"].clone();
    sends_extended_keys_and_chords(&mut server, &session, &authority, &screenshot).await;
    lists_installed_apps(&mut server, &reference).await;
    #[cfg(not(target_os = "macos"))]
    records_a_clip(&mut server, &reference).await;
    opens_urls(&mut server, &authority).await;
    let mut key = reference.clone();
    key["leaseId"] = session["leaseId"].clone();
    key["commandId"] = json!("home");
    key["display"] = screenshot["display"].clone();
    key["key"] = json!("home_screen");
    assert_eq!(server.call("key", key).await.unwrap()["state"], "completed");
    server.shutdown().await.unwrap();
    assert!(
        std::path::Path::new(run["evidenceDir"].as_str().unwrap())
            .join("summary.json")
            .is_file()
    );
    std::fs::remove_dir_all(run["evidenceDir"].as_str().unwrap()).unwrap();
    #[cfg(target_os = "macos")]
    records_a_decodable_clip_through_mcp(&target_id).await;
}

fn with(base: &Value, fields: Value) -> Value {
    let mut arguments = base.clone();
    for (key, value) in fields.as_object().unwrap() {
        arguments[key] = value.clone();
    }
    arguments
}

/// Tab is an extended key; a chord runs only where the session probe found
/// `keycombination`, and is refused before dispatch elsewhere.
async fn sends_extended_keys_and_chords(
    server: &mut Server,
    session: &Value,
    authority: &Value,
    screenshot: &Value,
) {
    let display = &screenshot["display"];
    let tab = server
        .call(
            "key",
            with(
                authority,
                json!({"commandId":"tab","display":display,"key":"tab"}),
            ),
        )
        .await
        .unwrap();
    assert_eq!(tab["state"], "completed", "{tab}");
    let chord = server
        .call(
            "key",
            with(
                authority,
                json!({"commandId":"select-all","display":display,"key":"a","modifiers":["control"]}),
            ),
        )
        .await;
    if session["target"]["capabilities"]["key"]["modifiers"] == true {
        assert_eq!(chord.unwrap()["state"], "completed");
    } else {
        assert_eq!(chord.unwrap_err().code, "CAPABILITY_UNAVAILABLE");
    }
}

async fn lists_installed_apps(server: &mut Server, reference: &Value) {
    let apps = server.call("app_list", reference.clone()).await.unwrap();
    assert!(apps["count"].as_u64().unwrap() > 0, "{apps}");
    // The reply may be one page; the private evidence holds the whole inventory.
    let evidence: Value =
        serde_json::from_slice(&std::fs::read(apps["artifactPath"].as_str().unwrap()).unwrap())
            .unwrap();
    let all = evidence["apps"].as_array().unwrap();
    assert_eq!(all.len() as u64, apps["count"].as_u64().unwrap());
    assert!(all.iter().any(|app| app["appId"] == "com.android.settings"));
}

#[cfg(not(target_os = "macos"))]
async fn records_a_clip(server: &mut Server, reference: &Value) {
    let started = server
        .call(
            "recording_start",
            with(reference, json!({"maxDurationMs":10_000})),
        )
        .await
        .unwrap();
    assert_eq!(started["status"], "active", "{started}");
    tokio::time::sleep(Duration::from_secs(3)).await;
    let stopped = server
        .call(
            "recording_stop",
            with(reference, json!({"recordingId":started["recordingId"]})),
        )
        .await
        .unwrap();
    assert_eq!(stopped["status"], "complete", "{stopped}");
}

/// macOS decodes the finished MP4 in a child `testrun` process, so the
/// recording runs through the shipping binary rather than this test process.
#[cfg(target_os = "macos")]
async fn records_a_decodable_clip_through_mcp(target_id: &str) {
    let directory = tempfile::tempdir().unwrap();
    let mut mcp =
        crate::simulator::Client::start_with_timeout(directory.path(), Duration::from_secs(60))
            .await;
    let connected = mcp.call("connect", json!({"targetId":target_id})).await;
    assert_eq!(connected["ok"], true, "{connected}");
    let started = mcp
        .call("recording_start", json!({"maxDurationMs":10_000}))
        .await;
    assert_eq!(started["data"]["status"], "active", "{started}");
    tokio::time::sleep(Duration::from_secs(3)).await;
    let stopped = mcp
        .call(
            "recording_stop",
            json!({"recordingId":started["data"]["recordingId"]}),
        )
        .await;
    assert_eq!(stopped["data"]["status"], "complete", "{stopped}");
    let closed = mcp.call("disconnect", json!({})).await;
    assert_eq!(closed["ok"], true, "{closed}");
}

/// A web URL opens, an unhandled scheme fails and an invalid URL is refused.
async fn opens_urls(server: &mut Server, authority: &Value) {
    let opened = server
        .call(
            "open_url",
            with(
                authority,
                json!({"commandId":"open-url","url":"https://example.com/acme"}),
            ),
        )
        .await
        .unwrap();
    assert_eq!(opened["state"], "completed", "{opened}");
    for (command, url, code) in [
        (
            "open-unhandled",
            "acme-unhandled://example",
            "PLATFORM_FAILED",
        ),
        (
            "open-invalid",
            "https://example.com/a b",
            "INVALID_ARGUMENT",
        ),
    ] {
        let refused = server
            .call(
                "open_url",
                with(authority, json!({"commandId":command,"url":url})),
            )
            .await
            .unwrap_err();
        assert_eq!(refused.code, code, "{url}");
    }
}
