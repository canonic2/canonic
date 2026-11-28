//! Behavioral fixtures for isolation, retained outcomes, source loss and owned cleanup.
use super::*;
fn owner() -> Owners {
    Owners::new("acme-owner".into())
}
fn open(owner: &Owners, root: &Path, workspace: &str, submission: &str) -> String {
    owner.open(root,workspace,&json!({"submissionId":submission,"question":"Investigate Acme","binding":{"appId":"com.example.acme","processId":42,"processGeneration":"birth-one","runtimeId":"worker","runtimeGeneration":"one","actionIds":["existing-action"]}})).unwrap()["investigationId"].as_str().unwrap().into()
}
fn select(id: &str) -> Value {
    json!({"investigationId":id})
}
async fn import(owner: &Owners, root: &Path, id: &str, path: &Path, limits: Value) -> Value {
    let builds = builds::Owners::default();
    let args = json!({"investigationId":id,"submissionId":"copy","source":{"kind":"import","path":path,"format":"acme-bytes"},"limits":limits});
    let (_, work) = owner.start(root, "acme", &args, &builds).unwrap();
    owner
        .run(work.unwrap(), root.into(), "acme".into(), builds)
        .await;
    owner
        .inspect(root, "acme", "diagnostics_status", &select(id))
        .unwrap()
}
#[test]
fn diagnostic_submissions_and_app_generations_are_immutable_and_workspace_scoped() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("state");
    let owner = owner();
    let id = open(&owner, &root, "acme", "case");
    assert_eq!(id, open(&owner, &root, "acme", "case"));
    let conflict = owner
        .open(
            &root,
            "acme",
            &json!({"submissionId":"case","question":"Changed"}),
        )
        .unwrap_err();
    assert_eq!(conflict.code, "REQUEST_CONFLICT");
    assert!(
        owner
            .inspect(&root, "other", "diagnostics_status", &select(&id))
            .is_err()
    );
    let value = owner
        .inspect(&root, "acme", "diagnostics_status", &select(&id))
        .unwrap();
    assert_eq!(value["binding"]["actionIds"][0], "existing-action");
    assert_eq!(value["bindingProvenance"], "caller_declared");
    for binding in [
        json!({"processId":42}),
        json!({"runtimeId":"worker"}),
        json!({"processId":0,"processGeneration":"one"}),
    ] {
        assert!(
            owner
                .open(
                    &root,
                    "acme",
                    &json!({"submissionId":"invalid","question":"Acme","binding":binding})
                )
                .is_err()
        );
    }
}
#[tokio::test]
async fn diagnostic_import_is_private_retained_and_never_recollected_on_replay() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("state");
    let input = tmp.path().join("input");
    std::fs::write(&input, b"Acme evidence").unwrap();
    let owner = owner();
    let id = open(&owner, &root, "acme", "case");
    let value = import(&owner, &root, &id, &input, json!({})).await;
    let c = &value["collectors"][0];
    assert_eq!(c["phase"], "complete");
    assert_eq!(c["reason"], "source_complete");
    assert_eq!(c["bytes"], 13);
    assert!(c["sha256"].is_string());
    std::fs::remove_file(&input).unwrap();
    let args = json!({"investigationId":id,"submissionId":"copy","source":{"kind":"import","path":input,"format":"acme-bytes"},"limits":{}});
    let (_, work) = owner
        .start(&root, "acme", &args, &builds::Owners::default())
        .unwrap();
    assert!(work.is_none());
    let first = owner
        .inspect(
            &root,
            "acme",
            "diagnostics_read",
            &json!({"investigationId":id,"collectorId":c["collectorId"],"limit":5}),
        )
        .unwrap();
    let read = owner
        .inspect(
            &root,
            "acme",
            "diagnostics_read",
            &json!({"investigationId":id,"collectorId":c["collectorId"],"cursor":first["nextCursor"],"limit":8}),
        )
        .unwrap();
    use base64::Engine;
    assert_eq!(
        base64::engine::general_purpose::STANDARD
            .decode(read["dataBase64"].as_str().unwrap())
            .unwrap(),
        b"evidence"
    );
    assert!(read["nextCursor"].is_null());
    assert_eq!(read["running"], false);
    assert!(!value.to_string().contains(input.to_str().unwrap()));
}
#[tokio::test]
async fn diagnostic_byte_and_event_bounds_preserve_partial_output() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("state");
    let input = tmp.path().join("input");
    std::fs::write(&input, vec![b'x'; 40000]).unwrap();
    let owner = owner();
    for (key, limits, expected) in [
        ("bytes", json!({"maxBytes":3}), 3),
        ("events", json!({"maxEvents":1}), 16384),
    ] {
        let id = open(&owner, &root, "acme", key);
        let value = import(&owner, &root, &id, &input, limits).await;
        assert_eq!(value["collectors"][0]["phase"], "partial");
        assert_eq!(value["collectors"][0]["reason"], "capture_limit");
        assert_eq!(value["collectors"][0]["bytes"], expected);
    }
    assert_eq!(std::fs::metadata(input).unwrap().len(), 40000);
}

#[tokio::test]
async fn diagnostic_native_trace_retention_publishes_complete_artifact_or_size_error() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("state");
    let input = tmp.path().join("sample");
    let report = vec![b'A'; 2 * 1024 * 1024];
    std::fs::write(&input, &report).unwrap();
    let owner = owner();
    let inv = owner
        .open(
            &root,
            "acme",
            &json!({
                "submissionId":"sample","question":"Acme sample",
                "binding":{"appId":"com.example.acme","targetId":"host:macos"}
            }),
        )
        .unwrap();
    let id = inv["investigationId"].as_str().unwrap();
    let dir = directory(&root, "acme", id).unwrap();
    let builds = builds::Owners::default();
    for (submission, max_bytes) in [("default", None), ("small", Some(1024 * 1024))] {
        let mut args = json!({
            "investigationId":id,"submissionId":submission,"limits":{"maxEvents":1},
            "source":{"kind":"native_trace","backend":"stack_sample","mode":"import","targetId":"host:macos","appId":"com.example.acme","path":input}
        });
        if let Some(limit) = max_bytes {
            args["limits"]["maxBytes"] = json!(limit);
        }
        let (_, work) = owner.start(&root, "acme", &args, &builds).unwrap();
        owner
            .run(work.unwrap(), root.clone(), "acme".into(), builds.clone())
            .await;
        let status = owner
            .inspect(&root, "acme", "diagnostics_status", &select(id))
            .unwrap();
        let c = status["collectors"].as_array().unwrap().last().unwrap();
        let bytes = read_data(&dir, c["collectorId"].as_str().unwrap()).unwrap();
        if max_bytes.is_none() {
            let (raw, _) = analysis::decode::native_trace::artifact_bytes(&bytes).unwrap();
            assert_eq!(raw, report);
            assert_eq!(c["events"], 1);
            assert_eq!(c["limits"]["maxBytes"], 8 * 1024 * 1024);
        } else {
            assert!(bytes.is_empty());
            assert_eq!(c["phase"], "failed");
            assert_eq!(c["reason"], "NATIVE_TRACE_BOUNDS");
            assert_eq!(
                c["sourceReport"]["retentionError"]["details"]["maxBytes"],
                1024 * 1024
            );
        }
    }
}

#[test]
fn diagnostic_native_trace_refuses_unretainable_perfetto_buffer_before_admission() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("state");
    let owner = owner();
    let opened = owner
        .open(
            &root,
            "acme",
            &json!({"submissionId":"perfetto","question":"Acme timeline",
                "binding":{"appId":"com.example.acme","targetId":"android:ACME"}}),
        )
        .unwrap();
    let id = opened["investigationId"].as_str().unwrap();
    let args = json!({"investigationId":id,"submissionId":"small","limits":{"maxBytes":1048576},
        "source":{"kind":"native_trace","mode":"capture","backend":"perfetto","targetId":"android:ACME",
            "appId":"com.example.acme","allowSystemTrace":true,"toolPath":tmp.path().join("processor")}});
    let Err(error) = owner.start(&root, "acme", &args, &builds::Owners::default()) else {
        panic!("insufficient byte budget was admitted");
    };
    assert_eq!(error.code, "NATIVE_TRACE_BOUNDS");
    let status = owner
        .inspect(&root, "acme", "diagnostics_status", &select(id))
        .unwrap();
    assert_eq!(status["collectors"], json!([]));
}
#[tokio::test]
async fn diagnostic_limit_on_final_import_chunk_is_not_false_complete() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("state");
    let input = tmp.path().join("input");
    std::fs::write(&input, b"longer").unwrap();
    let owner = owner();
    let id = open(&owner, &root, "acme", "case");
    let value = import(&owner, &root, &id, &input, json!({"maxBytes":2})).await;
    assert_eq!(value["collectors"][0]["phase"], "partial");
    assert_eq!(value["collectors"][0]["bytes"], 2);
}
#[tokio::test]
async fn diagnostic_owner_loss_retains_evidence_without_restarting_work() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("state");
    let input = tmp.path().join("input");
    std::fs::write(&input, b"evidence").unwrap();
    let first = owner();
    let id = open(&first, &root, "acme", "case");
    let args = json!({"investigationId":id,"submissionId":"copy","source":{"kind":"import","path":input,"format":"bytes"}});
    let (_, work) = first
        .start(&root, "acme", &args, &builds::Owners::default())
        .unwrap();
    drop(work);
    drop(first);
    let next = Owners::new("next-owner".into());
    let status = next
        .inspect(&root, "acme", "diagnostics_status", &select(&id))
        .unwrap();
    assert_eq!(status["collectors"][0]["phase"], "partial");
    assert_eq!(status["collectors"][0]["reason"], "owner_lost");
    let (_, work) = next
        .start(&root, "acme", &args, &builds::Owners::default())
        .unwrap();
    assert!(work.is_none());
    assert!(!next.active());
}

#[test]
fn diagnostic_native_trace_remove_recovers_interrupted_publication() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("state");
    let input = tmp.path().join("sample");
    std::fs::write(&input, b"original sample").unwrap();
    let first = owner();
    let opened = first
        .open(
            &root,
            "acme",
            &json!({
                "submissionId":"sample","question":"Acme sample",
                "binding":{"appId":"com.example.acme","targetId":"host:macos"}
            }),
        )
        .unwrap();
    let id = opened["investigationId"].as_str().unwrap();
    let (admitted, work) = first.start(&root, "acme", &json!({
        "investigationId":id,"submissionId":"sample",
        "source":{"kind":"native_trace","backend":"stack_sample","mode":"import","targetId":"host:macos","appId":"com.example.acme","path":input}
    }), &builds::Owners::default()).unwrap();
    let collector = admitted["collectors"][0]["collectorId"].as_str().unwrap();
    let dir = directory(&root, "acme", id).unwrap();
    // Model a process exit after writing staging bytes and before the atomic rename.
    let staged = dir.join(format!("{collector}.bin.partial"));
    std::fs::write(&staged, b"incomplete envelope").unwrap();
    drop(work);
    drop(first);

    let next = Owners::new("next-owner".into());
    let status = next
        .inspect(&root, "acme", "diagnostics_status", &select(id))
        .unwrap();
    assert_eq!(status["collectors"][0]["reason"], "owner_lost");
    assert_eq!(status["collectors"][0]["bytes"], 0);
    next.inspect(&root, "acme", "diagnostics_close", &select(id))
        .unwrap();
    let unrelated = dir.join("unrelated.partial");
    std::fs::write(&unrelated, b"preserve").unwrap();
    let error = next
        .inspect(&root, "acme", "diagnostics_remove", &select(id))
        .unwrap_err();
    assert_eq!(error.code, "CLEANUP_PENDING");
    assert!(staged.is_file());
    std::fs::remove_file(unrelated).unwrap();
    let removed = next
        .inspect(&root, "acme", "diagnostics_remove", &select(id))
        .unwrap();
    assert_eq!(removed["removed"], true);
    assert!(!dir.exists());
    assert_eq!(std::fs::read(input).unwrap(), b"original sample");
}

#[tokio::test]
async fn diagnostic_close_preserves_source_and_remove_requires_settlement() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("state");
    let input = tmp.path().join("input");
    std::fs::write(&input, b"evidence").unwrap();
    let owner = owner();
    let id = open(&owner, &root, "acme", "case");
    assert_eq!(
        owner
            .inspect(&root, "acme", "diagnostics_remove", &select(&id))
            .unwrap_err()
            .code,
        "DIAGNOSTIC_BUSY"
    );
    import(&owner, &root, &id, &input, json!({})).await;
    owner
        .inspect(&root, "acme", "diagnostics_close", &select(&id))
        .unwrap();
    let dir = directory(&root, "acme", &id).unwrap();
    std::fs::write(dir.join("unowned"), b"preserve").unwrap();
    assert_eq!(
        owner
            .inspect(&root, "acme", "diagnostics_remove", &select(&id))
            .unwrap_err()
            .code,
        "CLEANUP_PENDING"
    );
    std::fs::remove_file(dir.join("unowned")).unwrap();
    owner
        .inspect(&root, "acme", "diagnostics_remove", &select(&id))
        .unwrap();
    assert_eq!(std::fs::read(&input).unwrap(), b"evidence");
    assert!(!dir.exists());
}
#[tokio::test]
async fn diagnostic_stop_and_target_release_only_stop_owned_readers() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("state");
    let input = tmp.path().join("input");
    std::fs::write(&input, b"evidence").unwrap();
    let owner = owner();
    let id = open(&owner, &root, "acme", "case");
    let args = json!({"investigationId":id,"submissionId":"copy","source":{"kind":"import","path":input,"format":"bytes"}});
    let (_, work) = owner
        .start(&root, "acme", &args, &builds::Owners::default())
        .unwrap();
    owner
        .inspect(&root, "acme", "diagnostics_stop", &select(&id))
        .unwrap();
    owner
        .run(
            work.unwrap(),
            root.clone(),
            "acme".into(),
            builds::Owners::default(),
        )
        .await;
    let value = owner
        .inspect(&root, "acme", "diagnostics_status", &select(&id))
        .unwrap();
    assert_eq!(value["collectors"][0]["reason"], "stopped");
    assert_eq!(value["collectors"][0]["bytes"], 0);
    assert_eq!(std::fs::read(&input).unwrap(), b"evidence");
    assert!(!owner.active());
}
#[cfg(unix)]
#[test]
fn diagnostic_import_refuses_links_and_special_files_before_allocation() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("state");
    let input = tmp.path().join("input");
    std::fs::write(&input, b"x").unwrap();
    let link = tmp.path().join("link");
    std::os::unix::fs::symlink(&input, &link).unwrap();
    let owner = owner();
    let id = open(&owner, &root, "acme", "case");
    let result=owner.start(&root,"acme",&json!({"investigationId":id,"submissionId":"copy","source":{"kind":"import","path":link,"format":"bytes"}}),&builds::Owners::default());
    assert!(result.is_err());
    assert!(!owner.active());
    assert!(owner.start(&root,"acme",&json!({"investigationId":id,"submissionId":"copy","source":{"kind":"import","path":"/dev/null","format":"bytes"}}),&builds::Owners::default()).is_err());
}
#[test]
fn diagnostic_surface_accepts_typed_android_and_rejects_arbitrary_native_commands() {
    let definition = tools()
        .into_iter()
        .find(|d| d["name"] == "diagnostics_start")
        .unwrap();
    assert!(crate::schema::validate(&definition["inputSchema"], &json!({"investigationId":"diag-acme","submissionId":"x","source":{"kind":"android","targetId":"android:ACME","appId":"com.example.acme","userId":0}})).is_ok());
    assert!(crate::schema::validate(&definition["inputSchema"], &json!({"investigationId":"diag-acme","submissionId":"x","source":{"kind":"android_state","targetId":"android:ACME","appId":"com.example.acme","userId":0,"groups":["memory","process"]}})).is_ok());
    for source in [
        json!({"kind":"logcat"}),
        json!({"kind":"android_state","targetId":"android:ACME","appId":"com.example.acme","userId":0,"groups":["reset"]}),
        json!({"kind":"import","path":"/tmp/acme","format":"bytes","execute":"never"}),
    ] {
        assert!(
            crate::schema::validate(
                &definition["inputSchema"],
                &json!({"investigationId":"diag-acme","submissionId":"x","source":source})
            )
            .is_err()
        );
    }
}

#[test]
fn diagnostic_surface_android_binding_is_required_before_reader_allocation() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("state");
    let owner = owner();
    for (n, binding) in [
        json!({"appId":"com.example.acme"}),
        json!({"appId":"com.example.acme","targetId":"android:OTHER"}),
        json!({"appId":"com.example.other","targetId":"android:ACME"}),
    ]
    .iter()
    .enumerate()
    {
        let opened=owner.open(&root,"acme",&json!({"submissionId":format!("case-{n}"),"question":"Inspect Android","binding":binding})).unwrap();
        let id = opened["investigationId"].as_str().unwrap();
        for kind in ["android", "android_state"] {
            let result=owner.start(&root,"acme",&json!({"investigationId":id,"submissionId":"capture","source":{"kind":kind,"targetId":"android:ACME","appId":"com.example.acme","userId":0}}),&builds::Owners::default());
            assert_eq!(result.err().unwrap().code, "DIAGNOSTIC_BINDING_REQUIRED");
        }
        let result=owner.start(&root,"acme",&json!({"investigationId":id,"submissionId":"apple","source":{"kind":"ios","targetId":"device:ACME","appId":"com.example.acme"}}),&builds::Owners::default());
        assert_eq!(result.err().unwrap().code, "DIAGNOSTIC_BINDING_REQUIRED");
        assert!(!owner.active());
        let status = owner
            .inspect(&root, "acme", "diagnostics_status", &select(id))
            .unwrap();
        assert!(status["collectors"].as_array().unwrap().is_empty());
    }
}

#[test]
fn diagnostic_browser_schema_preserves_native_requests_and_binding_before_allocation() {
    let schema = &definition("diagnostics_start").unwrap()["inputSchema"];
    let target = format!("browser:{}", uuid::Uuid::new_v4());
    let operation = uuid::Uuid::new_v4();
    let reference = json!({"connectionId":uuid::Uuid::new_v4(),"id":uuid::Uuid::new_v4(),"generation":uuid::Uuid::new_v4()});
    for request in [
        json!({"operation":"diagnostics_read","context":reference}),
        json!({"operation":"batch","requests":[{"operation":"diagnostics_read","context":reference}]}),
    ] {
        let value = json!({"investigationId":"diag-acme","submissionId":"borrow","source":{"kind":"browser_operation","targetId":target,"operationId":operation,"request":request}});
        assert!(crate::schema::validate(schema, &value).is_ok());
    }
    let mut invalid = json!({"investigationId":"diag-acme","submissionId":"borrow","source":{"kind":"browser_operation","targetId":target,"operationId":operation,"request":{"operation":"diagnostics_read","context":reference,"execute":"never"}}});
    assert!(crate::schema::validate(schema, &invalid).is_err());
    invalid["source"]["request"] = json!({"operation":"unrestricted-shell","command":"never"});
    assert!(crate::schema::validate(schema, &invalid).is_err());
    let tmp = tempfile::tempdir().unwrap();
    let owner = owner();
    let root = tmp.path().join("state");
    let id = open(&owner, &root, "acme", "browser-case");
    let value = json!({"investigationId":id,"submissionId":"borrow","source":{"kind":"browser_operation","targetId":target,"operationId":operation,"request":{"operation":"page_list"}}});
    assert_eq!(
        owner
            .start(&root, "acme", &value, &builds::Owners::default())
            .err()
            .unwrap()
            .code,
        "DIAGNOSTIC_BINDING_REQUIRED"
    );
    assert!(!owner.active());
}

#[tokio::test]
async fn diagnostic_browser_borrow_retains_results_replays_after_owner_loss_and_preserves_native_files()
 {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("state");
    let artifacts = tmp.path().join("browser");
    crate::artifacts::directory(&artifacts).unwrap();
    let file = artifacts.join("private-trace.json");
    std::fs::write(&file, b"private-owned-trace").unwrap();
    let (state, operation) = borrowed_browser_state(&file);
    let browsers = crate::browser::runtime::Owners::default();
    let connection = browsers.fixture_diagnostic("pool", state, &artifacts);
    let target = crate::browser::runtime::target(connection);
    let direct_began = Instant::now();
    browsers
        .diagnostic_operation(
            "pool",
            &target,
            &operation.to_string(),
            &json!({"operation":"page_list"}),
        )
        .unwrap();
    let direct_ms = direct_began.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(
        browsers
            .diagnostic_operation(
                "other-pool",
                &target,
                &operation.to_string(),
                &json!({"operation":"page_list"})
            )
            .unwrap_err()
            .code,
        "NOT_CONNECTED"
    );
    let owner = owner();
    let builds = builds::Owners::default();
    let opened=owner.open(&root,"acme",&json!({"submissionId":"browser","question":"Inspect browser","binding":{"targetId":target,"actionIds":["action:browser"]}})).unwrap();
    let id = opened["investigationId"].as_str().unwrap();
    let args = json!({"investigationId":id,"submissionId":"borrow","source":{"kind":"browser_operation","targetId":target,"operationId":operation,"request":{"operation":"page_list"}}});
    let capture_began = Instant::now();
    let (_, work) = owner
        .start_with_browser(&root, "acme", &args, &builds, Some((&browsers, "pool")))
        .unwrap();
    drop(browsers);
    owner
        .run(work.unwrap(), root.clone(), "acme".into(), builds.clone())
        .await;
    let capture_ms = capture_began.elapsed().as_secs_f64() * 1000.0;
    let status = owner
        .inspect(&root, "acme", "diagnostics_status", &select(id))
        .unwrap();
    assert_eq!(
        status["collectors"][0]["reason"],
        "browser_operation_snapshot"
    );
    assert!(!status.to_string().contains("private-native-result"));
    assert_eq!(status["collectors"][0]["cost"]["sourceReadCalls"], 1);
    let collector = status["collectors"][0]["collectorId"].as_str().unwrap();
    let raw = read_data(&directory(&root, "acme", id).unwrap(), collector).unwrap();
    assert!(String::from_utf8_lossy(&raw).contains("private-native-result"));
    assert!(!String::from_utf8_lossy(&raw).contains("private-owned-trace"));
    let packet: Value = serde_json::from_slice(&raw).unwrap();
    assert_eq!(packet["actionIds"], json!(["action:browser"]));
    let replay_began = Instant::now();
    assert!(
        owner
            .start_with_browser(&root, "acme", &args, &builds, None)
            .unwrap()
            .1
            .is_none()
    );
    eprintln!(
        "browser borrow fixture (debug, warm in-process, one observation): direct_snapshot_ms={direct_ms:.3} admission_through_finalization_ms={capture_ms:.3} retained_replay_ms={:.3} retained_bytes={} source_read_calls={}",
        replay_began.elapsed().as_secs_f64() * 1000.0,
        status["collectors"][0]["bytes"],
        status["collectors"][0]["cost"]["sourceReadCalls"]
    );
    owner
        .inspect(&root, "acme", "diagnostics_close", &select(id))
        .unwrap();
    owner
        .inspect(&root, "acme", "diagnostics_remove", &select(id))
        .unwrap();
    assert_eq!(std::fs::read(file).unwrap(), b"private-owned-trace");
}

fn borrowed_browser_state(file: &Path) -> (crate::browser::lifecycle::State, uuid::Uuid) {
    use crate::browser::{
        contract::{ContextOptions, Engine, LaunchOptions, Mode, Request},
        lifecycle::{Outcome, State},
    };
    let request: Request = serde_json::from_value(json!({"operation":"page_list"})).unwrap();
    let mut state = State::new(
        uuid::Uuid::new_v4(),
        uuid::Uuid::new_v4(),
        &LaunchOptions {
            media_fixtures: Default::default(),
            engine: Engine::Chrome,
            mode: Mode::Headless,
            proxy: None,
            context: ContextOptions::default(),
        },
    );
    let operation = uuid::Uuid::new_v4();
    state
        .begin(operation, digest(&serde_json::to_vec(&request).unwrap()))
        .unwrap();
    let op = state.operations.get_mut(&operation).unwrap();
    op.settled = true;
    op.outcome = Outcome::Succeeded;
    op.result = Some(
        json!({"evidence":{"path":file,"sha256":"native-private-digest"},"events":[{"kind":"page_error","data":{"text":"private-native-result"}}]}),
    );
    (state, operation)
}

fn borrowed_build(root: &Path, builds: &builds::Owners) -> (String, builds::Ticket) {
    let request:builds::Request=serde_json::from_value(json!({"project":root,"inline":{"builder":"custom","platform":"android","target_kind":"device","custom":{"command":["never-executed"]},"artifact":{"path":"Acme.apk"}}})).unwrap();
    let (record, ticket) = builds
        .begin(root, "acme", "existing-project-server", &request)
        .unwrap();
    let ticket = ticket.unwrap();
    std::fs::write(ticket.directory.join("build.log"), b"Acme server ready\n").unwrap();
    (record["operationId"].as_str().unwrap().into(), ticket)
}
async fn progress(owner: &Owners, root: &Path, id: &str) -> Value {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let status = owner
                .inspect(root, "acme", "diagnostics_status", &select(id))
                .unwrap();
            if status["collectors"][0]["events"].as_u64().unwrap() > 0 {
                return status;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap()
}
#[tokio::test(flavor = "multi_thread")]
async fn diagnostic_concurrent_readers_stop_independently_without_cancelling_source() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("state");
    let builds = builds::Owners::default();
    let (source, ticket) = borrowed_build(&root, &builds);
    let owner = owner();
    let one = owner
        .open(
            &root,
            "acme",
            &json!({"submissionId":"one","question":"Acme","binding":{"targetId":"android:Acme"}}),
        )
        .unwrap()["investigationId"]
        .as_str()
        .unwrap()
        .to_owned();
    let two = open(&owner, &root, "acme", "two");
    let mut tasks = Vec::new();
    for id in [&one, &two] {
        let (_,work)=owner.start(&root,"acme",&json!({"investigationId":id,"submissionId":"observe","source":{"kind":"build_output","operationId":source},"limits":{"durationMs":10000}}),&builds).unwrap();
        let task_owner = owner.clone();
        let task_root = root.clone();
        let task_builds = builds.clone();
        tasks.push(tokio::spawn(async move {
            task_owner
                .run(work.unwrap(), task_root, "acme".into(), task_builds)
                .await;
        }));
    }
    progress(&owner, &root, &one).await;
    progress(&owner, &root, &two).await;
    owner.stop_all(Some("android:Acme"), "target_released");
    let first = tasks.remove(0);
    tokio::time::timeout(Duration::from_secs(5), first)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        owner
            .inspect(&root, "acme", "diagnostics_status", &select(&one))
            .unwrap()["collectors"][0]["reason"],
        "target_released"
    );
    assert!(owner.active());
    assert!(!*ticket.cancelled.borrow());
    assert!(builds.active());
    owner
        .inspect(&root, "acme", "diagnostics_close", &select(&two))
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), tasks.remove(0))
        .await
        .unwrap()
        .unwrap();
    assert!(!owner.active());
    assert!(!*ticket.cancelled.borrow());
}
#[tokio::test]
async fn diagnostic_duration_bound_and_source_loss_keep_partial_bytes() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("state");
    let builds = builds::Owners::default();
    let (source, ticket) = borrowed_build(&root, &builds);
    let owner = owner();
    let id = open(&owner, &root, "acme", "case");
    let (_,work)=owner.start(&root,"acme",&json!({"investigationId":id,"submissionId":"observe","source":{"kind":"build_output","operationId":source},"limits":{"durationMs":100}}),&builds).unwrap();
    owner
        .run(work.unwrap(), root.clone(), "acme".into(), builds.clone())
        .await;
    let status = owner
        .inspect(&root, "acme", "diagnostics_status", &select(&id))
        .unwrap();
    assert_eq!(status["collectors"][0]["reason"], "duration_limit");
    assert_eq!(status["collectors"][0]["phase"], "partial");
    assert!(!*ticket.cancelled.borrow());
}
#[test]
fn diagnostic_offline_inspection_does_not_rewrite_a_live_managers_record() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("state");
    let input = tmp.path().join("input");
    std::fs::write(&input, b"evidence").unwrap();
    let first = owner();
    let id = open(&first, &root, "acme", "case");
    let (_,work)=first.start(&root,"acme",&json!({"investigationId":id,"submissionId":"copy","source":{"kind":"import","path":input,"format":"bytes"}}),&builds::Owners::default()).unwrap();
    let path = directory(&root, "acme", &id).unwrap().join("session.json");
    let before = std::fs::read(&path).unwrap();
    let offline = Owners::default();
    let value = offline
        .inspect(&root, "acme", "diagnostics_status", &select(&id))
        .unwrap();
    assert_eq!(value["collectors"][0]["reason"], "owner_lost");
    assert_eq!(std::fs::read(path).unwrap(), before);
    drop(work);
}
#[tokio::test]
async fn diagnostic_recovery_retries_finalization_without_reading_the_source() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("state");
    let input = tmp.path().join("input");
    std::fs::write(&input, b"evidence").unwrap();
    let owner = owner();
    let id = open(&owner, &root, "acme", "case");
    let result = import(&owner, &root, &id, &input, json!({})).await;
    let collector = result["collectors"][0]["collectorId"]
        .as_str()
        .unwrap()
        .to_owned();
    let dir = directory(&root, "acme", &id).unwrap();
    {
        let mut state = owner.state.lock().unwrap();
        let record = state.records.get_mut(&dir).unwrap();
        record.collectors[0].phase(Phase::CleanupPending);
        persist(&dir, record).unwrap();
    }
    std::fs::remove_file(input).unwrap();
    let status = owner
        .inspect(&root, "acme", "diagnostics_status", &select(&id))
        .unwrap();
    assert_eq!(status["collectors"][0]["phase"], "complete");
    assert_eq!(status["collectors"][0]["collectorId"], collector);
    assert!(!owner.active());
}

#[tokio::test]
async fn diagnostic_damaged_record_cannot_remove_files_outside_its_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("state");
    let input = tmp.path().join("input");
    std::fs::write(&input, b"preserve").unwrap();
    let owner = owner();
    let id = open(&owner, &root, "acme", "case");
    import(&owner, &root, &id, &input, json!({})).await;
    owner
        .inspect(&root, "acme", "diagnostics_close", &select(&id))
        .unwrap();
    let dir = directory(&root, "acme", &id).unwrap();
    let mut record: Investigation = storage::read(&dir.join("session.json")).unwrap().unwrap();
    record.collectors[0].id = "../../input".into();
    persist(&dir, &record).unwrap();
    let next = Owners::new("next".into());
    assert_eq!(
        next.inspect(&root, "acme", "diagnostics_remove", &select(&id))
            .unwrap_err()
            .code,
        "STATE_IO"
    );
    assert_eq!(std::fs::read(input).unwrap(), b"preserve");
}

#[tokio::test]
async fn diagnostic_real_publication_failure_keeps_cleanup_retry_and_does_not_recollect() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("state");
    let input = tmp.path().join("input");
    std::fs::write(&input, b"preserve").unwrap();
    let owner = owner();
    let id = open(&owner, &root, "acme", "case");
    let (_,work)=owner.start(&root,"acme",&json!({"investigationId":id,"submissionId":"copy","source":{"kind":"import","path":input,"format":"bytes"}}),&builds::Owners::default()).unwrap();
    let dir = directory(&root, "acme", &id).unwrap();
    let path = dir.join("session.json");
    let backup = dir.join("saved.json");
    std::fs::rename(&path, &backup).unwrap();
    std::fs::create_dir(&path).unwrap();
    owner
        .run(
            work.unwrap(),
            root.clone(),
            "acme".into(),
            builds::Owners::default(),
        )
        .await;
    assert!(!owner.active());
    assert_eq!(
        owner.state.lock().unwrap().records[&dir].collectors[0].phase,
        Phase::CleanupPending
    );
    std::fs::remove_file(input).unwrap();
    std::fs::remove_dir(&path).unwrap();
    std::fs::rename(backup, path).unwrap();
    let status = owner
        .inspect(&root, "acme", "diagnostics_status", &select(&id))
        .unwrap();
    assert_eq!(status["collectors"][0]["phase"], "failed");
    assert_eq!(status["collectors"][0]["reason"], "STATE_IO");
    assert_eq!(status["collectors"][0]["bytes"], 0);
}
