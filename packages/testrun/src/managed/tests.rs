//! Caller workflows against fixture device actors with private records and
//! real sockets, plus pure action and catalog validation.
use super::*;
use std::path::Path;

/// A fixture device actor: the product endpoint, records and stop protocol
/// with a scripted controller. Markers in the pool directory script it:
/// `fail-startup` (every start fails without a retry), `fail-startup-once`,
/// `hold-batch` (also in its target directory, holding
/// only that target), `hold-startup`, `transition-target`, `unhealthy` (status reports
/// uncertain input, so a reconnect replaces the actor) and, once ready,
/// `panic-actor`;
/// in its owner directory, `fail-cleanup` and `cleanup-delay-ms`. Batches are
/// logged to `dispatches.log` in both the pool and target directories.
#[cfg(unix)]
pub(super) async fn fixture_actor(owner: &Owner) {
    let connection = &owner.connection;
    let session = connection
        .directory
        .parent()
        .and_then(Path::parent)
        .unwrap()
        .to_owned();
    let directory = connection.directory.join("s");
    storage::directory(&directory).unwrap();
    let archive = crate::qa::journal::Archive::create(&directory).unwrap();
    let listener = tokio::net::UnixListener::bind(connection.socket()).unwrap();
    if fixture_startup_failed(owner, &session) {
        return;
    }
    while session.join("hold-startup").exists() {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let image = json!({"source":"fixture","display":{"displayId":"main","geometryVersion":"fixture-100x200"}});
    let reported = std::fs::read_to_string(session.join("transition-target")).ok();
    owner.state("ready", json!({"targetId":reported.as_deref().unwrap_or(&connection.target),"observation":image,"observationProfile":"fast","controlPath":"native","capabilities":{"screenshots":{"state":"supported","profiles":["fast"]}}})).unwrap();
    let panic = async {
        while !session.join("panic-actor").exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("Acme actor panic");
    };
    // Like the product controller, a held batch does not block reads.
    let run = async {
        loop {
            let (mut socket, _) = listener.accept().await.unwrap();
            let (connection, session, archive, image) = (
                connection.clone(),
                session.clone(),
                archive.clone(),
                image.clone(),
            );
            tokio::spawn(async move {
                let input = crate::interactive::local::read(&mut socket).await.unwrap();
                let data = if input["ownerEpoch"] != connection.epoch {
                    json!({"state":"refused"})
                } else {
                    fixture_reply(&connection, &session, &archive, &image, &input).await
                };
                let _ =
                    crate::interactive::local::write(&mut socket, &json!({"ok":true,"data":data}))
                        .await;
            });
        }
    };
    let lost = async {
        while connection.socket().exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    };
    tokio::select! {() = run => {}, () = panic => {}, _ = owner.stopped() => {}, () = lost => {}, _ = owner.terminated() => {}}
    drop(listener);
    finish_fixture_actor(owner).await;
}

#[cfg(unix)]
async fn fixture_reply(
    connection: &Connection,
    session: &Path,
    archive: &crate::qa::journal::Archive,
    image: &Value,
    input: &Value,
) -> Value {
    match input["kind"].as_str() {
        Some("control_status") => {
            json!({"state":"active","owner":"agent","controlVersion":1,"leaseId":"fixture-initial"})
        }
        Some("status") => {
            let uncertain = session.join("unhealthy").exists();
            json!({"state":"ready","inputHealth":{"uncertain":uncertain,"videoFresh":true}})
        }
        Some("observe") => {
            let mut observation = image.clone();
            observation["profile"] = input["profile"].as_str().unwrap_or("fast").into();
            observation
        }
        Some("inspection") => match input["tool"].as_str() {
            Some("preview_start") => {
                json!({"previewId":format!("acme-preview-{}", uuid::Uuid::new_v4().simple())})
            }
            Some("recording_start") => {
                json!({"recordingId":format!("acme-recording-{}", uuid::Uuid::new_v4().simple()),"status":"active"})
            }
            Some("recording_status") => json!({"status":"active"}),
            _ => json!({"tool":input["tool"],"fixture":true}),
        },
        Some("playback") => json!({"tool":input["tool"],"fixture":true}),
        Some("application") => json!({"actionId":input["actionId"],"fixture":true}),
        Some("finalize_recordings") => {
            storage::write(&session.join("recordings-finalized"), &true).unwrap();
            json!({"recordings":[{"recordingId":"acme-recording","result":{"status":"complete"}}]})
        }
        Some("batch") => {
            let command = input["commandId"].as_str().unwrap();
            archive
                .record(
                    command,
                    &json!({"commandId":command,"status":"outcome_unknown","steps":[]}),
                )
                .unwrap();
            let target = connection.directory.parent().unwrap();
            for log in [
                session.join("dispatches.log"),
                target.join("dispatches.log"),
            ] {
                let mut dispatched = std::fs::read_to_string(&log).unwrap_or_default();
                dispatched.push_str(command);
                dispatched.push('\n');
                std::fs::write(&log, dispatched).unwrap();
            }
            let count = storage::read::<u64>(&connection.directory.join("dispatches.json"))
                .unwrap()
                .unwrap_or(0)
                + 1;
            storage::write(&connection.directory.join("dispatches.json"), &count).unwrap();
            archive.record(&format!("{command}:0"),&json!({"commandId":format!("{command}:0"),"state":"completed","observed":false,"result":{"completion":"reports_sent","deliveryAcknowledged":false}})).unwrap();
            while session.join("hold-batch").exists() || target.join("hold-batch").exists() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            let result =
                json!({"commandId":command,"status":"completed","steps":[],"observation":image});
            archive.record(command, &result).unwrap();
            result
        }
        _ => panic!("Unexpected fixture request"),
    }
}

#[cfg(unix)]
fn fixture_startup_failed(owner: &Owner, session: &Path) -> bool {
    if session.join("fail-startup").exists() {
        owner.cleanup(true).unwrap();
        owner.state("failed",json!({"error":{"code":"DEVICE_LOCKED","message":"Fixture device is locked","retryable":false}})).unwrap();
        return true;
    }
    let transient = session.join("fail-startup-once");
    if !transient.exists() {
        return false;
    }
    std::fs::remove_file(transient).unwrap();
    owner.cleanup(true).unwrap();
    owner.state("failed",json!({"error":{"code":"CAPTURE_STALLED","message":"Fixture video startup failed","retryable":false,"details":{"nativeStatus":-42}}})).unwrap();
    true
}

/// Resumable fixture cleanup: a failed attempt keeps the device until a new
/// stop request asks again.
#[cfg(unix)]
async fn finish_fixture_actor(owner: &Owner) {
    let connection = &owner.connection;
    let mut request = owner.cleanup_request().unwrap();
    loop {
        owner.state("closing", json!({})).unwrap();
        let delay = storage::read::<u64>(&connection.directory.join("cleanup-delay-ms"))
            .unwrap()
            .unwrap_or(20);
        tokio::time::sleep(Duration::from_millis(delay)).await;
        if !connection.directory.join("fail-cleanup").exists() {
            owner
                .cleanup_progress(json!([]), None, request.as_deref())
                .unwrap();
            owner.state("closed", json!({})).unwrap();
            return;
        }
        let error = Error::new("CLEANUP_FAILED", "Acme fixture release failed");
        owner
            .cleanup_progress(json!([]), Some(&error), request.as_deref())
            .unwrap();
        owner.state("failed", json!({"error":error})).unwrap();
        tokio::select! {
            requested = owner.cleanup_requested(request.as_deref()) => requested.unwrap(),
            _ = owner.terminated() => return,
        }
        request = owner.cleanup_request().unwrap();
    }
}

/// A temporary data root short enough for fixture socket paths.
pub(super) fn root() -> tempfile::TempDir {
    let mut builder = tempfile::Builder::new();
    builder.prefix("trq-");
    #[cfg(unix)]
    return builder.tempdir_in("/tmp").unwrap();
    #[cfg(not(unix))]
    builder.tempdir().unwrap()
}

/// A client identity whose host label names the agent it stands for.
pub(super) fn identity(label: &str, root: &Path) -> identity::Identity {
    identity::Identity {
        label: Some(label.into()),
        workspace: root.to_owned(),
        device_set: None,
    }
}

/// A caller outside any admitted request, as an offline reader or a test of
/// one workflow.
fn caller(service: Option<&Arc<service::Service>>, root: &Path) -> Caller {
    Caller::new(service.cloned(), root.join("state"), Kind::Cli)
}

#[test]
fn selection_needs_only_an_explicit_target_and_retains_runner_setup_options() {
    let root = root();
    let client = caller(None, root.path());
    let directory = client.directory().unwrap();
    for target in ["device:Acme-phone", "simulator:Acme-simulator"] {
        let selected = client
            .desired(&directory, &json!({"targetId":target}))
            .unwrap();
        assert_eq!(selected.target, target);
        assert!(selected.runner_team_id.is_none());
    }
    let selected = client
        .desired(
            &directory,
            &json!({
                "targetId":"device:Acme-phone", "setup":true, "runnerTeamId":"ACMETEAM"
            }),
        )
        .unwrap();
    let record = directory.join("owner.json");
    storage::write(&record, &selected).unwrap();
    let restored: Connection = storage::read(&record).unwrap().unwrap();
    assert_eq!(restored.runner_team_id.as_deref(), Some("ACMETEAM"));
    assert!(restored.compatible(&selected));
    let another_team = client
        .desired(
            &directory,
            &json!({
                "targetId":"device:Acme-phone", "setup":true, "runnerTeamId":"OTHERTEAM"
            }),
        )
        .unwrap();
    assert!(!restored.compatible(&another_team));
}

#[test]
fn physical_iphone_connect_options_are_refused_for_other_targets() {
    let root = root();
    let client = caller(None, root.path());
    let directory = client.directory().unwrap();
    for target in ["simulator:Acme-simulator", "android:Acme"] {
        for (field, value) in [
            ("controlPath", json!("xctest")),
            ("appId", json!("com.example.acme")),
            ("runnerTeamId", json!("ACMETEAM")),
            ("setup", json!(true)),
            ("prepareSupport", json!(true)),
        ] {
            let mut args = json!({"targetId":target});
            args[field] = value;
            let error = client.desired(&directory, &args).err().unwrap();
            assert_eq!(error.code, "INVALID_ARGUMENT", "{target} {field}");
            assert!(error.message.starts_with(field), "{}", error.message);
        }
        client
            .desired(
                &directory,
                &json!({"targetId":target,"setup":false,"prepareSupport":false}),
            )
            .unwrap();
    }
}

#[test]
fn android_capture_settings_default_inherit_and_refuse_live_changes() {
    use crate::android::streaming::Mode;
    let root = root();
    let client = caller(None, root.path());
    let directory = client.directory().unwrap();
    let select = |args| client.desired(&directory, &args).unwrap();
    let default = select(json!({"targetId":"android:Acme"}));
    assert_eq!(default.android_capture, None);
    let stream = select(json!({"targetId":"android:Acme","androidCapture":"stream"}));
    let mut inherited = default.clone();
    inherited.resolve_capture(&stream, false, true).unwrap();
    assert_eq!(inherited.android_capture, Some(Mode::Stream));
    assert!(inherited.compatible(&stream));
    let mut changed = default.clone();
    assert_eq!(
        changed
            .resolve_capture(&stream, true, true)
            .unwrap_err()
            .code,
        "SESSION_SETTINGS_CONFLICT"
    );
    changed.resolve_capture(&stream, true, false).unwrap();
    let mut other = select(json!({"targetId":"device:Acme"}));
    other.resolve_capture(&stream, false, true).unwrap();
    assert_eq!(other.android_capture, None);
    assert!(
        client
            .desired(
                &directory,
                &json!({"targetId":"device:Acme","androidCapture":"stream"})
            )
            .is_err()
    );
    let mut legacy = serde_json::to_value(&default).unwrap();
    legacy.as_object_mut().unwrap().remove("androidCapture");
    let restored: Connection = serde_json::from_value(legacy).unwrap();
    assert!(restored.compatible(&default));
}

#[tokio::test]
async fn offline_status_reads_records_without_a_manager() {
    let root = root();
    let mut client = caller(None, root.path());
    let status = client.call("status", json!({})).await.unwrap();
    assert_eq!(status["state"], "not_connected", "{status}");
    assert_eq!(status["targets"], json!([]));
    assert_eq!(status["owner"], "absent");
    // Use requests need the manager.
    assert_eq!(
        client.call("observe", json!({})).await.unwrap_err().code,
        "OWNER_ABSENT"
    );
}

#[tokio::test]
async fn existing_receipts_preserve_outcomes_when_backend_or_build_admission_changes() {
    let root = root();
    let service =
        service::Service::fixture(root.path().join("state"), WIRE, Duration::from_secs(45));
    let target = format!("device:Acme-{}", uuid::Uuid::new_v4());
    let mut client = caller(Some(&service), root.path());
    let directory = client.directory().unwrap();
    let connection = client
        .desired(&directory, &json!({"targetId":target}))
        .unwrap();
    storage::write(&directory.join("selected.json"), &connection).unwrap();
    storage::directory(&connection.directory).unwrap();
    let session = connection.directory.join("s");
    storage::directory(&session).unwrap();
    let archive = crate::qa::journal::Archive::create(&session).unwrap();
    let args = json!({"actions":[{"action":"key","key":"home"}]});
    let command = "previously-admitted";
    let receipts = directory.join("receipts");
    storage::directory(&receipts).unwrap();
    storage::write(
        &receipts.join(format!("{}.json", hash(command))),
        &Receipt {
            connection: connection.clone(),
            command: command.into(),
            created: now(),
            signature: hash(json!({"target":target,"actions":args}).to_string()),
        },
    )
    .unwrap();
    for (status, incompatible) in [
        ("failed", false),
        ("outcome_unknown", false),
        ("completed", false),
        ("failed", true),
        ("outcome_unknown", true),
        ("completed", true),
    ] {
        let mut selected = connection.clone();
        if incompatible {
            selected.build_id = Some("Acme-previous-build".into());
        }
        storage::write(&directory.join("selected.json"), &selected).unwrap();
        archive
            .record(command, &json!({"status":status,"steps":[]}))
            .unwrap();
        client.request_id = Some(command.into());
        let result = client.call("act", args.clone()).await;
        if status == "completed" {
            let result = result.unwrap();
            assert_eq!(result["state"], "recorded");
            assert_eq!(result["replayed"], false);
        } else {
            let error = result.unwrap_err();
            assert_eq!(
                error.code,
                if status == "failed" {
                    "BATCH_FAILED"
                } else {
                    "BATCH_OUTCOME_UNKNOWN"
                }
            );
            let details = error.details.unwrap();
            assert_eq!(details["result"]["lastAction"]["status"], status);
            assert_ne!(details["result"]["inputDispatched"], false);
        }
    }
}

#[tokio::test]
async fn expired_media_deadline_is_refused_before_admission() {
    let root = root();
    let service =
        service::Service::fixture(root.path().join("state"), WIRE, Duration::from_secs(45));
    let mut client = caller(Some(&service), root.path());
    client.media_deadline = Some(tokio::time::Instant::now() - Duration::from_millis(1));
    let error = client
        .call("audio_probe", json!({"acceptRouteChange":true}))
        .await
        .unwrap_err();
    assert_eq!(error.code, "TIMEOUT");
    assert_eq!(error.details.unwrap()["admitted"], false);
    assert_eq!(service.supervisor.alive(), 0);
}

#[test]
fn simple_actions_refuse_authority_fields_and_diagnostics_hide_host_ownership() {
    let keys = validation::steps(
        &json!({"actions":[{"action":"key","key":"a","modifiers":["meta","shift"]}]}),
    )
    .unwrap();
    for kind in ["simulator", "device"] {
        crate::mobile::validate_backend_options(kind, Some("hid"), "key", &keys[0]["arguments"])
            .unwrap();
    }
    assert_eq!(keys[0]["arguments"]["modifiers"], json!(["meta", "shift"]));
    assert!(
        crate::mobile::validate_backend_options(
            "simulator",
            None,
            "key",
            &json!({"key":"a","modifiers":["shift","shift"]})
        )
        .is_err()
    );
    assert!(
        validation::steps(
            &json!({"actions":[{"action":"tap","point":{"x":0.5,"y":0.5},"leaseId":"foreign"}]})
        )
        .is_err()
    );
    assert_eq!(
        validation::steps(
            &json!({"actions":[{"action":"swipe","from":{"x":0.1,"y":0.2},"to":{"x":0.5,"y":0.6}}]})
        )
        .unwrap()[0]["arguments"]["durationMs"],
        150
    );
    let value = presentation::clean(
        json!({"pid":10,"socket":"private","result":{"commandId":"hidden","state":"completed","observed":false}}),
    );
    assert!(value.get("pid").is_none());
    assert!(value.get("socket").is_none());
    assert_eq!(value["result"]["observed"], false);
}

#[test]
fn delay_actions_become_bounded_delay_steps_on_every_backend() {
    let tap = json!({"action":"tap","point":{"x":0.5,"y":0.5}});
    let steps =
        validation::steps(&json!({"actions":[tap,{"action":"delay","durationMs":9500},tap]}))
            .unwrap();
    assert_eq!(
        steps[1],
        json!({"tool":"delay","arguments":{"durationMs":9500}})
    );
    for (kind, path) in [
        ("simulator", None),
        ("device", Some("hid")),
        ("device", Some("xctest")),
        ("android", None),
    ] {
        validation::backend_options(&steps, kind, path, false).unwrap();
    }
    for (actions, index) in [
        (json!([tap, {"action":"delay"}]), 1),
        (json!([{"action":"delay","durationMs":60001}]), 0),
        (
            json!([{"action":"delay","durationMs":10,"point":{"x":0.5,"y":0.5}}]),
            0,
        ),
        // A swipe keeps its own 10 s bound when the shared field allows 60 s.
        (
            json!([{"action":"swipe","from":{"x":0.1,"y":0.2},"to":{"x":0.5,"y":0.6},"durationMs":10001}]),
            0,
        ),
        // Delays and gestures share a 90 s budget; the crossing action is named.
        (
            json!([
                {"action":"long_press","point":{"x":0.5,"y":0.5},"durationMs":10000},
                {"action":"delay","durationMs":60000},
                {"action":"delay","durationMs":20001}
            ]),
            2,
        ),
    ] {
        let error = validation::steps(&json!({"actions":actions})).unwrap_err();
        assert_eq!(error.code, "INVALID_ARGUMENT", "{actions}");
        assert_eq!(error.details.unwrap()["actionIndex"], index, "{actions}");
    }
    let batch = crate::catalog::definition("batch").unwrap();
    assert!(
        batch["inputSchema"]["properties"]["steps"]["items"]["properties"]["tool"]["enum"]
            .as_array()
            .unwrap()
            .contains(&json!("delay"))
    );
}

#[test]
fn published_tools_are_the_task_level_set_with_one_vocabulary() {
    let tools = crate::catalog::tools();
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    for name in &names {
        assert!(
            name.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
            "{name}"
        );
    }
    for hidden in [
        "session_open",
        "batch",
        "tap",
        "screenshot",
        "run.begin",
        "host_info",
    ] {
        assert!(!names.contains(&hidden), "{hidden}");
    }
    let text = serde_json::to_string(&tools).unwrap();
    for old in [
        "bundleId",
        "actionRef",
        "elementRef",
        "streamId",
        "sessionRef",
        "volumeTarget",
        "observationProfile",
        "durationLimitMs",
        "readbackBudgetMs",
    ] {
        assert!(!text.contains(old), "{old}");
    }
    for tool in &tools {
        let read_only = tool["annotations"]["readOnlyHint"] == true;
        let starts = tool["name"]
            .as_str()
            .is_some_and(|name| name.ends_with("_start"));
        assert!(!(read_only && starts), "{}", tool["name"]);
    }
    // Schema unions are structural; they never introduce an action verb.
    assert!(validation::steps(&json!({"actions":[{"action":"anyOf"}]})).is_err());
    let open =
        validation::steps(&json!({"actions":[{"action":"open_url","url":"acme://home"}]})).unwrap();
    assert_eq!(open[0]["tool"], "open_url");
}

#[tokio::test]
async fn mcp_invalid_swipe_reports_pre_dispatch_rejection() {
    let mut server = crate::qa::Server::new();
    let mut protocol = crate::protocol::Protocol::default();
    protocol.request(&mut server,json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"acme-qa","version":"1"}}})).await.unwrap();
    protocol
        .request(
            &mut server,
            json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        )
        .await;
    let response = protocol.request(&mut server,json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"act","arguments":{"actions":[{"action":"swipe","from":{"x":0.145,"y":0.72},"to":{"x":0.18,"y":0.55},"durationMs":10001}]}}})).await.unwrap();
    let error = &response["result"]["structuredContent"];
    assert_eq!(error["code"], "INVALID_ARGUMENT");
    let message = error["message"].as_str().unwrap();
    assert!(message.contains("actions[0] (swipe).durationMs"));
    assert!(message.contains("must be between 0 and 10000; received 10001"));
    assert!(message.contains("correct the arguments and resubmit"));
    assert_eq!(error["details"]["nextAction"], "act");
    assert_eq!(error["details"]["result"]["actionsDispatched"], 0);
    assert_eq!(error["details"]["result"]["admitted"], false);
    assert_eq!(error["details"]["result"]["actionIndex"], 0);
    assert_eq!(error["details"]["result"]["actionType"], "swipe");
}

#[tokio::test]
async fn mcp_refused_later_typing_identifies_the_action() {
    let mut server = crate::qa::Server::new();
    let mut protocol = crate::protocol::Protocol::default();
    protocol.request(&mut server,json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"acme-qa","version":"1"}}})).await.unwrap();
    protocol
        .request(
            &mut server,
            json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        )
        .await;
    for (id, typing) in [
        json!({"action":"type_text"}),
        json!({"action":"type_text","text":"Acme","composition":{"text":"é","commit":true}}),
    ]
    .into_iter()
    .enumerate()
    {
        let response = protocol.request(&mut server,json!({"jsonrpc":"2.0","id":id+2,"method":"tools/call","params":{"name":"act","arguments":{"actions":[{"action":"tap","point":{"x":0.5,"y":0.5}},typing]}}})).await.unwrap();
        let error = &response["result"]["structuredContent"];
        assert_eq!(error["code"], "INVALID_ARGUMENT");
        assert!(
            error["message"]
                .as_str()
                .unwrap()
                .contains("actions[1] (type_text)")
        );
        assert_eq!(error["details"]["nextAction"], "act");
        assert_eq!(error["details"]["result"]["actionIndex"], 1);
        assert_eq!(error["details"]["result"]["actionType"], "type_text");
        assert_eq!(error["details"]["result"]["actionsDispatched"], 0);
    }
}

#[test]
fn gesture_duration_bounds_match_cli_mcp_and_hid() {
    let definitions = crate::catalog::tools();
    let schema = &definitions.iter().find(|t| t["name"] == "act").unwrap()["inputSchema"];
    for operation in ["swipe", "long_press"] {
        let mut action = if operation == "swipe" {
            json!({"action":operation,"from":{"x":0,"y":0},"to":{"x":1,"y":1}})
        } else {
            json!({"action":operation,"point":{"x":0.5,"y":0.5}})
        };
        for duration in [
            json!(0),
            json!(1),
            json!(45),
            json!(10000),
            json!(-1),
            json!(10001),
            json!(0.5),
        ] {
            action["durationMs"] = duration.clone();
            let arguments = json!({"actions":[action]});
            let valid = duration.as_u64().is_some_and(|v| v <= 10000);
            // The shared field also carries delays; gestures keep their own
            // 10 s bound, which action validation enforces before dispatch.
            assert_eq!(
                crate::schema::validate(schema, &arguments).is_ok(),
                duration.as_u64().is_some()
            );
            let steps = validation::steps(&arguments);
            assert_eq!(steps.is_ok(), valid);
            #[cfg(target_os = "macos")]
            if let Ok(steps) = steps {
                let mut input = steps[0]["arguments"].clone();
                input["scope"] = json!({"kind":"display"});
                assert_eq!(
                    crate::hid::points(operation, &input).unwrap().2,
                    duration.as_u64().unwrap()
                );
            }
        }
    }
}
