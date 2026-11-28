//! Real-pipe tracing admission, late setup and independently finalized archives.
use super::*;
use std::io::{Cursor, Write};

async fn acknowledge(peer: &mut Fixture, method: &str, result: Value) {
    let request = tokio::time::timeout(Duration::from_secs(2), peer.request())
        .await
        .unwrap_or_else(|_| panic!("No tracing request for {method}"));
    assert_eq!(request["method"], method, "{request}");
    peer.send(&json!({"id":request["id"],"result":result}))
        .await;
}
async fn recording(host: &Arc<Host>) {
    loop {
        let changed = host.trace.lock().unwrap().as_ref().unwrap().clone();
        let notified = changed.changed.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        if host.trace_status()["state"] == "recording" && !changed.busy.load(Ordering::SeqCst) {
            break;
        }
        tokio::time::timeout_at(deadline(), notified).await.unwrap();
    }
}
fn archive(path: &Path) {
    let mut writer = zip::ZipWriter::new_stream(Vec::new());
    writer
        .start_file("trace.trace", zip::write::SimpleFileOptions::default())
        .unwrap();
    writer
        .write_all(b"{\"type\":\"context-options\",\"browserName\":\"firefox\"}\n")
        .unwrap();
    writer
        .start_file("trace.network", zip::write::SimpleFileOptions::default())
        .unwrap();
    let bytes = writer.finish().unwrap().into_inner();
    assert!(
        zip::ZipArchive::new(Cursor::new(&bytes))
            .unwrap()
            .decompressed_size()
            .is_none()
    );
    std::fs::write(path, bytes).unwrap();
}
#[tokio::test]
async fn tracing_live_sources_and_group_locations_follow_one_private_capture_owner() {
    let (session, mut peer, root, context, _page) = harness().await;
    let host = session.0.host.clone();
    let path = root.path().join("Acme-private-source.ts");
    std::fs::write(&path, b"Acme immutable selected source").unwrap();
    let options = crate::browser::tracing::Options {
        live: true,
        sources: true,
        source_files: vec![path.to_str().unwrap().into()],
        ..Default::default()
    };
    let starting = tokio::spawn({
        let host = host.clone();
        async move { host.trace_start(context, options, deadline()).await }
    });
    let start = peer.request().await;
    assert_eq!(start["method"], "tracingStart");
    assert_eq!(start["params"]["live"], true);
    peer.send(&json!({"id":start["id"],"result":{}})).await;
    acknowledge(&mut peer, "tracingStartChunk", json!({"traceName":"Acme"})).await;
    let started = starting.await.unwrap().unwrap();
    assert_eq!(started["sourceTextsIncluded"], true);
    assert_eq!(started["options"]["sourceFileCount"], 1);
    assert!(!started.to_string().contains("Acme-private-source"));
    let location = crate::browser::automation_debugger::Location {
        file: path.to_str().unwrap().into(),
        line: Some(42),
        column: Some(3),
    };
    let grouping = tokio::spawn({
        let host = host.clone();
        let location = location.clone();
        async move {
            host.trace_command(
                Request::TraceGroupStart {
                    name: "Acme source group".into(),
                    location: Some(location),
                },
                deadline(),
            )
            .await
        }
    });
    let group = peer.request().await;
    assert_eq!(group["method"], "tracingGroup");
    assert_eq!(group["params"]["location"], json!(location));
    peer.send(&json!({"id":group["id"],"result":{}})).await;
    grouping.await.unwrap().unwrap();
    std::fs::remove_file(&path).unwrap();
    let archive_path = host.journal.directory().join("traces/Acme.zip");
    archive(&archive_path);
    peer.send(&created(
        "Acme-source-artifact",
        "Artifact",
        json!({"absolutePath":archive_path}),
    ))
    .await;
    let stopping = tokio::spawn({
        let host = host.clone();
        async move {
            host.trace_command(
                Request::TraceStop {
                    file_path: None,
                    analysis_developer: None,
                    analysis_page: None,
                },
                deadline(),
            )
            .await
        }
    });
    acknowledge(
        &mut peer,
        "tracingStopChunk",
        json!({"artifact":{"guid":"Acme-source-artifact"}}),
    )
    .await;
    acknowledge(&mut peer, "delete", json!({})).await;
    acknowledge(&mut peer, "tracingStop", json!({})).await;
    let done = stopping.await.unwrap().unwrap();
    let report = &done["reports"][0];
    assert_eq!(report["sourceTextsIncluded"], true);
    assert_eq!(report["liveRecording"], true);
    let bytes = std::fs::read(report["attachment"]["path"].as_str().unwrap()).unwrap();
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let name = zip
        .file_names()
        .map(Result::unwrap)
        .find(|name| name.starts_with("src/"))
        .unwrap()
        .to_string();
    let mut text = String::new();
    std::io::Read::read_to_string(&mut zip.by_name(&name).unwrap(), &mut text).unwrap();
    assert_eq!(text, "Acme immutable selected source");
    close(&session).await;
}
#[tokio::test]
#[expect(
    clippy::too_many_lines,
    reason = "One trace owner is followed across late setup, chunk stop, archive export and release"
)]
async fn tracing_late_start_keeps_one_owner_and_chunk_stop_preserves_a_real_archive() {
    let (session, mut peer, _root, context, _page) = harness().await;
    let host = session.0.host.clone();
    crate::artifacts::directory(&host.journal.directory().join("traces")).unwrap();
    let mut starting = tokio::spawn({
        let host = host.clone();
        async move {
            host.trace_start(
                context,
                crate::browser::tracing::Options {
                    snapshots: crate::browser::tracing::Snapshots::Selected(
                        crate::browser::tracing::SnapshotOptions {
                            dom: false,
                            aria: true,
                            screen: true,
                        },
                    ),
                    name: Some("Acme_trace".into()),
                    title: Some("Acme initial trace".into()),
                    ..Default::default()
                },
                Instant::now() + Duration::from_millis(25),
            )
            .await
        }
    });
    let start = tokio::select! {request=peer.request()=>request,result=&mut starting=>panic!("Trace admission before first command: {result:?}; status: {}",host.trace_status())};
    assert_eq!(start["method"], "tracingStart");
    assert_eq!(
        start["params"],
        json!({"name":"Acme_trace","snapshotDom":false,"snapshotAria":true,"snapshotScreen":true,"screencast":false})
    );
    let error = starting.await.unwrap().unwrap_err();
    assert_eq!(error.code, "TRACE_COMMAND_PENDING");
    assert_eq!(
        host.trace_start(context, Default::default(), deadline())
            .await
            .unwrap_err()
            .code,
        "TRACE_ALREADY_ACTIVE"
    );
    peer.send(&json!({"id":start["id"],"result":{}})).await;
    let first_chunk = peer.request().await;
    assert_eq!(first_chunk["method"], "tracingStartChunk");
    assert_eq!(first_chunk["params"]["title"], "Acme initial trace");
    peer.send(&json!({"id":first_chunk["id"],"result":{"traceName":"Acme"}}))
        .await;
    recording(&host).await;
    let grouping = tokio::spawn({
        let host = host.clone();
        async move {
            host.trace_command(
                Request::TraceGroupStart {
                    name: "Acme journey".into(),
                    location: None,
                },
                deadline(),
            )
            .await
        }
    });
    acknowledge(&mut peer, "tracingGroup", json!({})).await;
    assert_eq!(grouping.await.unwrap().unwrap()["groupDepth"], 1);
    let path = host.journal.directory().join("traces/acme.zip");
    archive(&path);
    peer.send(&created(
        "trace-artifact",
        "Artifact",
        json!({"absolutePath":path}),
    ))
    .await;
    let stopping = tokio::spawn({
        let host = host.clone();
        async move {
            host.trace_command(
                Request::TraceChunkStop {},
                Instant::now() + Duration::from_millis(25),
            )
            .await
        }
    });
    let stop = peer.request().await;
    assert_eq!(stop["method"], "tracingStopChunk");
    let pending = stopping.await.unwrap().unwrap_err();
    assert_eq!(pending.code, "TRACE_COMMAND_PENDING");
    let command_id = pending.partial.unwrap()["pendingCommand"]["id"].clone();
    assert!(command_id.is_string());
    peer.send(&json!({"id":stop["id"],"result":{"artifact":{"guid":"trace-artifact"}}}))
        .await;
    acknowledge(&mut peer, "delete", json!({})).await;
    let owner = host.trace.lock().unwrap().as_ref().unwrap().clone();
    loop {
        let changed = owner.changed.notified();
        tokio::pin!(changed);
        changed.as_mut().enable();
        if !owner.busy.load(Ordering::SeqCst) {
            break;
        }
        tokio::time::timeout_at(deadline(), changed).await.unwrap();
    }
    let reports = host.trace_status()["lastCommand"].clone();
    assert_eq!(reports["id"], command_id);
    assert_eq!(reports["outcome"], "succeeded");
    assert_eq!(reports["reports"][0]["format"], "playwright-trace-zip");
    assert_eq!(reports["reports"][0]["complete"], true);
    let bytes = std::fs::read(
        reports["reports"][0]["attachment"]["path"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    assert!(zip.by_name("trace.trace").is_ok());
    assert_eq!(host.trace_status()["groupDepth"], 0);
    let chunk = tokio::spawn({
        let host = host.clone();
        async move {
            host.trace_command(
                Request::TraceChunkStart {
                    title: "Acme second chunk".into(),
                    name: Some("Acme_second".into()),
                },
                deadline(),
            )
            .await
        }
    });
    let second_chunk = peer.request().await;
    assert_eq!(second_chunk["method"], "tracingStartChunk");
    assert_eq!(
        second_chunk["params"],
        json!({"title":"Acme second chunk","name":"Acme_second"})
    );
    peer.send(&json!({"id":second_chunk["id"],"result":{"traceName":"Acme"}}))
        .await;
    chunk.await.unwrap().unwrap();
    let other = Reference {
        id: Uuid::new_v4(),
        ..context
    };
    host.close_trace(Some(other), deadline()).await.unwrap();
    let ending = tokio::spawn({
        let host = host.clone();
        async move {
            host.trace_command(
                Request::TraceStop {
                    file_path: None,
                    analysis_developer: None,
                    analysis_page: None,
                },
                deadline(),
            )
            .await
        }
    });
    acknowledge(
        &mut peer,
        "tracingStopChunk",
        json!({"artifact":{"guid":"trace-artifact"}}),
    )
    .await;
    acknowledge(&mut peer, "delete", json!({})).await;
    acknowledge(&mut peer, "tracingStop", json!({})).await;
    let stopped = ending.await.unwrap().unwrap();
    assert_eq!(stopped["active"], false);
    assert_eq!(stopped["chunks"], 2);
    assert_eq!(stopped["reports"].as_array().unwrap().len(), 2);
    assert_eq!(
        host.trace_command(
            Request::TraceStop {
                file_path: None,
                analysis_developer: None,
                analysis_page: None
            },
            deadline()
        )
        .await
        .unwrap(),
        stopped
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.request())
            .await
            .is_err()
    );
    close(&session).await;
}

#[test]
fn sdk_trace_native_analysis_options_refuse_before_any_admission() {
    for request in [
        json!({"operation":"trace_stop","analysisPage":{"connectionId":Uuid::nil(),"id":Uuid::nil(),"generation":Uuid::nil()}}),
        json!({"operation":"trace_stop","analysisDeveloper":{"connectionId":Uuid::nil(),"id":Uuid::nil(),"generation":Uuid::nil()}}),
        json!({"operation":"trace_stop","filePath":"Acme.json"}),
    ] {
        let request: Request = serde_json::from_value(request).unwrap();
        assert_eq!(
            super::super::workflows::validate(&request)
                .unwrap_err()
                .code,
            "TRACE_NATIVE_OPTIONS_UNSUPPORTED"
        );
    }
}

#[test]
fn trace_archive_reader_refuses_foreign_files_and_oversized_source_trees() {
    let root = tempfile::tempdir().unwrap();
    let owned = root.path().join("traces");
    std::fs::create_dir(&owned).unwrap();
    let foreign = root.path().join("acme.zip");
    archive(&foreign);
    assert_eq!(
        super::super::tracing::read_archive(&owned, &foreign)
            .unwrap_err()
            .code,
        "TRACE_ARTIFACT_SCOPE"
    );
    let file = std::fs::File::create(owned.join("large.trace")).unwrap();
    file.set_len(64 * 1024 * 1024 + 1).unwrap();
    assert_eq!(
        super::super::tracing::source_size(&owned).unwrap_err().code,
        "TRACE_SOURCE_BOUNDS"
    );
}

#[tokio::test]
async fn trace_source_budget_discards_once_and_preserves_an_explicit_status_artifact() {
    let (session, mut peer, _root, context, _page) = harness().await;
    let host = session.0.host.clone();
    let starting = tokio::spawn({
        let host = host.clone();
        async move {
            host.trace_start(context, Default::default(), deadline())
                .await
        }
    });
    acknowledge(&mut peer, "tracingStart", json!({})).await;
    acknowledge(&mut peer, "tracingStartChunk", json!({"traceName":"Acme"})).await;
    starting.await.unwrap().unwrap();
    let file =
        std::fs::File::create(host.journal.directory().join("traces/large.network")).unwrap();
    file.set_len(64 * 1024 * 1024 + 1).unwrap();
    let stop = peer.request().await;
    assert_eq!(stop["method"], "tracingStopChunk");
    assert_eq!(stop["params"]["mode"], "discard");
    peer.send(&json!({"id":stop["id"],"result":{}})).await;
    acknowledge(&mut peer, "tracingStop", json!({})).await;
    let result = host
        .trace_command(
            Request::TraceStop {
                file_path: None,
                analysis_developer: None,
                analysis_page: None,
            },
            deadline(),
        )
        .await
        .unwrap_err();
    assert_eq!(result.code, "TRACE_SOURCE_BOUNDS");
    let report = host.trace_status()["reports"][0].clone();
    assert_eq!(report["format"], "playwright-trace-capture-status-json");
    assert_eq!(report["complete"], false);
    assert_eq!(report["error"], "TRACE_SOURCE_BOUNDS");
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.request())
            .await
            .is_err()
    );
    close(&session).await;
}

#[tokio::test]
async fn trace_lost_stop_retains_cleanup_and_status_until_owned_connection_release() {
    let (session, mut peer, _root, context, _page) = harness().await;
    let host = session.0.host.clone();
    let starting = tokio::spawn({
        let host = host.clone();
        async move {
            host.trace_start(context, Default::default(), deadline())
                .await
        }
    });
    acknowledge(&mut peer, "tracingStart", json!({})).await;
    acknowledge(&mut peer, "tracingStartChunk", json!({"traceName":"Acme"})).await;
    starting.await.unwrap().unwrap();
    let stopping = tokio::spawn({
        let host = host.clone();
        async move {
            host.trace_command(
                Request::TraceChunkStop {},
                Instant::now() + Duration::from_millis(25),
            )
            .await
        }
    });
    let stop = peer.request().await;
    assert_eq!(stop["method"], "tracingStopChunk");
    assert_eq!(
        stopping.await.unwrap().unwrap_err().code,
        "TRACE_COMMAND_PENDING"
    );
    drop(peer);
    let owner = host.trace.lock().unwrap().as_ref().unwrap().clone();
    loop {
        let changed = owner.changed.notified();
        tokio::pin!(changed);
        changed.as_mut().enable();
        if host.trace_status()["active"] == false {
            break;
        }
        tokio::time::timeout_at(deadline(), changed).await.unwrap();
    }
    let status = host.trace_status();
    assert_eq!(status["cleanupPending"], true);
    assert_eq!(
        status["reports"][0]["format"],
        "playwright-trace-capture-status-json"
    );
    assert_eq!(
        host.trace_start(context, Default::default(), deadline())
            .await
            .unwrap_err()
            .code,
        "TRACE_ALREADY_ACTIVE"
    );
    close(&session).await;
    assert_eq!(host.trace_status()["cleanupPending"], false);
    let evidence = host
        .trace_evidence_operation(Request::DeveloperEvidenceList {})
        .unwrap();
    assert_eq!(evidence["capacity"]["reservedItems"], 0);
}
