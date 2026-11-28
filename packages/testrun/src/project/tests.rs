//! Project workflow behavior through shared config, HTTP and retained operations.
use super::*;
#[cfg(unix)]
use crate::builds::Owners;
use crate::builds::Request;
fn profile() -> Value {
    json!({"builder":"custom","platform":"ios","target_kind":"device","custom":{"command":["make","app"]},"artifact":{"path":"dist/Acme.app"},"development":{"app_id":"com.example.acme.staging","expo":{"url":"http://127.0.0.1:8081/","slug":"acme"},"prepare":{"command":["make","prepare"]}}})
}
fn manifest() -> Value {
    json!({"extra":{"expoClient":{"slug":"acme","hostUri":"192.0.2.2:8081","ios":{"bundleIdentifier":"com.example.acme.staging"}}},"launchAsset":{"url":"http://192.0.2.2:8081/index.bundle"}})
}
fn expo() -> Expo {
    serde_json::from_value(profile()["development"]["expo"].clone()).unwrap()
}

#[test]
fn manifest_identity_and_device_reachability_remain_distinct() {
    let mut m = manifest();
    let r = expo::summarize(&expo(), "ios", true, &m).unwrap();
    assert_eq!(r["connectionState"], "candidate");
    assert_eq!(r["deviceLoaded"], "unknown");
    assert_eq!(r["connectionUrl"], "exp://192.0.2.2:8081");
    m["extra"]["expoClient"]["hostUri"] = json!("127.0.0.1:8081");
    assert_eq!(
        expo::summarize(&expo(), "ios", true, &m).unwrap()["state"],
        "blocked"
    );
    assert_eq!(
        expo::summarize(&expo(), "ios", false, &m).unwrap()["state"],
        "ready"
    );
    let mut forwarded = expo();
    forwarded.connection_mode = Some("forwarded".into());
    forwarded.connection_url = Some("exp://127.0.0.1:8081".into());
    assert_eq!(
        expo::summarize(&forwarded, "ios", true, &m).unwrap()["connectionState"],
        "candidate"
    );
    m["extra"]["expoClient"]["slug"] = json!("another-project");
    assert_eq!(
        expo::summarize(&expo(), "ios", true, &m).unwrap_err().code,
        "EXPO_PROJECT_MISMATCH"
    );
}
#[test]
fn manifest_cannot_leak_credentials_or_turn_inspection_into_a_proxy() {
    for value in [
        "file:///etc/hosts",
        "http://user:secret@localhost/",
        "http://localhost/?token=private",
    ] {
        assert!(expo::endpoint(value).is_err());
    }
    let mut m = manifest();
    m["launchAsset"]["url"] = json!("http://user:secret@localhost/");
    m["extra"]["expoClient"]["hostUri"] = json!("user:secret@localhost:8081");
    let result = expo::summarize(&expo(), "ios", true, &m).unwrap();
    assert!(result["bundleUrl"].is_null());
    assert!(result["connectionUrl"].is_null());
    assert!(!result.to_string().contains("secret"));
    assert!(
        expo::connection("acme://expo-development-client/?url=http%3A%2F%2F192.0.2.2%3A8081")
            .is_ok()
    );
}
#[test]
fn inventory_absence_is_distinct_from_unavailable_inventory() {
    assert_eq!(
        installed(&json!({"apps":[]}), "com.example.acme")["state"],
        "blocked"
    );
    assert_eq!(
        installed(&json!({}), "com.example.acme")["state"],
        "unknown"
    );
    assert_eq!(
        installed(
            &json!({"apps":[{"appId":"com.example.acme","build":"42"}]}),
            "com.example.acme"
        )["app"]["build"],
        "42"
    );
}

#[test]
fn failed_inspection_does_not_claim_an_existing_endpoint_is_unused() {
    let refused = ureq::Error::Io(std::io::Error::from(std::io::ErrorKind::ConnectionRefused));
    assert_eq!(expo::transport_error(refused).code, "EXPO_UNREACHABLE");
    assert_eq!(
        expo::transport_error(ureq::Error::HostNotFound).code,
        "EXPO_INSPECTION_FAILED"
    );
    let reset = ureq::Error::Io(std::io::Error::from(std::io::ErrorKind::ConnectionReset));
    assert_eq!(expo::transport_error(reset).code, "EXPO_INSPECTION_FAILED");
}
#[test]
fn strict_configuration_layers_development_without_executing_it() {
    let root = tempfile::tempdir().unwrap();
    let mut value = profile();
    value["development"]["prepare"]["command"] =
        json!(["/usr/bin/touch", root.path().join("must-not-exist")]);
    std::fs::write(
        root.path().join("testrun.yml"),
        json!({"version":1,"builds":{"ios-dev":value}}).to_string(),
    )
    .unwrap();
    std::fs::write(root.path().join("testrun.local.yml"),"version: 1\nbuilds:\n  ios-dev:\n    development:\n      expo:\n        url: http://127.0.0.1:9090/\n").unwrap();
    let (loaded, selected_profile) = selected(root.path(), None, "ios-dev").unwrap();
    assert_eq!(
        selected_profile.development.unwrap().expo.unwrap().url,
        "http://127.0.0.1:9090/"
    );
    assert_eq!(
        loaded.provenance["/builds/ios-dev/development/expo/url"],
        "testrun.local.yml"
    );
    assert!(!root.path().join("must-not-exist").exists());
    let result = configuration::inspect(root.path(), None).unwrap();
    assert_eq!(
        result["configuration"]["builds"]["ios-dev"]["development"]["prepare"]["command"],
        "<redacted>"
    );
    let mut bad = profile();
    bad["development"]["unknown"] = json!(true);
    assert!(configuration::decode(json!({"version":1,"builds":{"ios-dev":bad}})).is_err());
}

#[tokio::test]
async fn expo_wire_uses_platform_headers_and_never_follows_redirects() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    for status in [200, 302] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut bytes = Vec::new();
            loop {
                let b = socket.read_u8().await.unwrap();
                bytes.push(b);
                if bytes.ends_with(b"\r\n\r\n") {
                    break;
                }
            }
            let request = String::from_utf8(bytes).unwrap().to_lowercase();
            assert!(request.contains("expo-platform: ios"));
            assert!(request.contains("application/expo+json"));
            let body = manifest().to_string();
            socket.write_all(format!("HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nLocation: http://127.0.0.1:1/private\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
        });
        let mut settings = expo();
        settings.url = format!("http://{address}/");
        let result = expo::inspect(settings, "ios".into(), true).await;
        server.await.unwrap();
        assert_eq!(
            result["state"],
            if status == 200 { "ready" } else { "blocked" }
        );
        if status == 302 {
            assert_eq!(result["error"]["code"], "EXPO_HTTP");
        }
    }
}

#[cfg(unix)]
#[tokio::test]
async fn preparation_is_retained_replayable_and_does_not_require_an_artifact() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let mut p = profile();
    p["development"]["prepare"]["command"] = json!([
        "/bin/sh",
        "-c",
        "printf prepared; printf done > prepared.txt"
    ]);
    let request: Request = serde_json::from_value(
        json!({"project":root.path(),"inline":p,"task":"prepare","submissionId":"acme-prepare"}),
    )
    .unwrap();
    let prepared = crate::builds::prepare(&request).unwrap();
    let owners = Owners::default();
    let (record, ticket) = owners
        .begin(state.path(), "workspace", "transport", &request)
        .unwrap();
    let ticket = ticket.unwrap();
    let result = owners.build(&ticket, &prepared).await;
    assert!(result.is_ok());
    owners.finish(&ticket, result);
    assert_eq!(
        std::fs::read_to_string(root.path().join("prepared.txt")).unwrap(),
        "done"
    );
    let first = owners
        .log(
            state.path(),
            "workspace",
            &json!({"operationId":record["operationId"],"limit":2}),
        )
        .unwrap();
    assert_eq!(first["text"], "pr");
    assert!(!first["nextCursor"].is_null());
    assert_eq!(
        owners
            .log(
                state.path(),
                "workspace",
                &json!({"operationId":record["operationId"],"cursor":first["nextCursor"],"limit":3})
            )
            .unwrap()["text"],
        "epa"
    );
    let replay = owners
        .replay(state.path(), "workspace", "another-transport", &request)
        .unwrap()
        .unwrap();
    assert_eq!(replay["state"], "completed");
    assert!(
        owners
            .log(
                state.path(),
                "other-workspace",
                &json!({"operationId":record["operationId"]})
            )
            .is_err()
    );
    assert!(replay["stages"]["artifact"]["artifact"].is_null());
}
#[test]
fn project_tasks_reject_device_mutations_and_missing_check_names() {
    let root = tempfile::tempdir().unwrap();
    for extra in [
        json!({"task":"server","install":true,"targetId":"device:ACME"}),
        json!({"task":"check"}),
        json!({"task":"prepare","prepare":true}),
    ] {
        let mut v = json!({"project":root.path(),"inline":profile()});
        v.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        let request: Request = serde_json::from_value(v).unwrap();
        assert!(crate::builds::prepare(&request).is_err());
    }
}

#[test]
fn flat_tap_coordinates_return_the_valid_shape_without_dispatch() {
    let error =
        crate::managed::action_steps(&json!({"actions":[{"action":"tap","x":0.3,"y":0.4}]}))
            .unwrap_err();
    assert_eq!(
        error.details.unwrap()["example"]["point"],
        json!({"x":0.5,"y":0.5})
    );
}

#[test]
fn file_effects_report_changes_without_reverting_existing_work() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("tsconfig.json"), "existing local work").unwrap();
    let effects = vec!["tsconfig.json".into(), "ios".into()];
    let before = files::capture(root.path(), &effects);
    std::fs::create_dir(root.path().join("ios")).unwrap();
    std::fs::write(root.path().join("ios/project.txt"), "generated").unwrap();
    let delta = files::changes(&before, &files::capture(root.path(), &effects));
    assert_eq!(delta["complete"], true);
    assert_eq!(delta["restored"], false);
    assert!(
        delta["paths"]
            .as_array()
            .unwrap()
            .contains(&json!("ios/project.txt"))
    );
    assert_eq!(
        std::fs::read_to_string(root.path().join("tsconfig.json")).unwrap(),
        "existing local work"
    );
    for path in ["../outside", "/outside", "."] {
        assert!(files::relative(path).is_err());
    }
}

#[cfg(unix)]
#[tokio::test]
async fn live_task_logs_and_cancellation_share_retained_ownership() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let mut p = profile();
    p["development"]["server"] =
        json!({"command":["/bin/sh","-c","printf ready; exec /bin/sleep 30"],"timeout_ms":60000});
    p["development"].as_object_mut().unwrap().remove("expo");
    let request: Request =
        serde_json::from_value(json!({"project":root.path(),"inline":p,"task":"server"})).unwrap();
    let prepared = crate::builds::prepare(&request).unwrap();
    let owners = Owners::default();
    let (record, ticket) = owners
        .begin(state.path(), "workspace", "server", &request)
        .unwrap();
    let id = record["operationId"].as_str().unwrap().to_owned();
    let worker = owners.clone();
    let task = tokio::spawn(async move {
        let ticket = ticket.unwrap();
        let result = worker.build(&ticket, &prepared).await;
        assert_eq!(result.as_ref().unwrap_err().code, "CANCELLED");
        worker.finish(&ticket, result);
    });
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let log = owners
                .log(state.path(), "workspace", &json!({"operationId":id}))
                .unwrap();
            if log["text"] == "ready" {
                assert_eq!(log["settled"], false);
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let (_, duplicate) = owners
        .begin(state.path(), "workspace", "another-server", &request)
        .unwrap();
    let duplicate = duplicate.unwrap();
    let refused = owners
        .build(&duplicate, &crate::builds::prepare(&request).unwrap())
        .await
        .unwrap_err();
    assert_eq!(refused.code, "BUILD_BUSY");
    assert_eq!(refused.details.as_ref().unwrap()["operationId"], id);
    owners.finish(&duplicate, Err(refused));
    let running = owners.status(state.path(), "workspace", &id, true).unwrap();
    assert_eq!(running["process"]["reaped"], false);
    tokio::time::timeout(std::time::Duration::from_secs(5), task)
        .await
        .unwrap()
        .unwrap();
    let stopped = owners
        .status(state.path(), "workspace", &id, false)
        .unwrap();
    assert_eq!(stopped["state"], "cancelled");
    assert_eq!(stopped["process"]["reaped"], true);
    assert_eq!(stopped["stages"]["artifact"]["state"], "not_requested");
    assert_eq!(
        owners.active_project("workspace", root.path().to_str().unwrap()),
        json!([])
    );
}

#[cfg(unix)]
#[tokio::test]
async fn expected_version_failure_and_failed_preparation_keep_build_unstarted() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let mut p = profile();
    p["development"]["prepare"] =
        json!({"command":["/usr/bin/printf","12.14.0"],"expected_output":"11.27.0"});
    let request: Request =
        serde_json::from_value(json!({"project":root.path(),"inline":p,"prepare":true})).unwrap();
    let prepared = crate::builds::prepare(&request).unwrap();
    let owners = Owners::default();
    let (record, ticket) = owners
        .begin(state.path(), "workspace", "prepare-before-build", &request)
        .unwrap();
    let ticket = ticket.unwrap();
    let result = owners.build(&ticket, &prepared).await;
    assert_eq!(result.as_ref().unwrap_err().code, "PROJECT_CHECK_MISMATCH");
    owners.finish(&ticket, result);
    let record = owners
        .status(
            state.path(),
            "workspace",
            record["operationId"].as_str().unwrap(),
            false,
        )
        .unwrap();
    assert_eq!(record["stages"]["build"]["state"], "not_started");
    assert_eq!(record["stages"]["prepare"]["state"], "failed");
    assert_eq!(
        std::fs::read_to_string(ticket.directory.join("build.log")).unwrap(),
        "12.14.0"
    );
}
