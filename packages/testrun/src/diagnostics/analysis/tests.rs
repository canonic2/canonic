use super::*;
use std::io::Write;

fn events() -> Value {
    json!({"schemaVersion":1,"clocks":[{"id":"app","unit":"milliseconds"}],"records":[
        {"kind":"marker","name":"begin","timestamp":100.0,"clockDomain":"app","value":null,"unit":null,"definition":null,"correlations":["action:1"],"source":"caller-pointer","data":{"buildId":"image-a","architecture":"arm64"}},
        {"kind":"metric","name":"rss","timestamp":110.0,"clockDomain":"app","value":64.0,"unit":"bytes","definition":"resident memory","correlations":["action:1"],"source":"caller-pointer","data":{}},
        {"kind":"event","name":"unmatched","timestamp":null,"clockDomain":null,"value":null,"unit":null,"definition":null,"source":"caller-pointer","data":{}}
    ]})
}
fn decode_events() -> Index {
    decode::decode("testrun-events-v1", &serde_json::to_vec(&events()).unwrap()).unwrap()
}

#[tokio::test]
async fn analysis_legacy_version_one_cost_defaults_preserve_evidence_and_replay_ids() {
    let (tmp, owner, id, cid) = fixture().await;
    let (receipt, work) = prepare(&owner, tmp.path(), &id, &cid, json!({}));
    owner.finish_analysis(work, Ok(decode_events()));
    let aid = receipt["analysisId"].as_str().unwrap();
    let dir = directory(tmp.path(), "workspace", &id).unwrap();
    let raw = read_data(&dir, &cid).unwrap();
    for path in [dir.join("session.json"), receipt_path(&dir, aid).unwrap()] {
        let mut legacy: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        if let Some(collectors) = legacy.get_mut("collectors").and_then(Value::as_array_mut) {
            for collector in collectors {
                collector.as_object_mut().unwrap().remove("cost");
            }
        }
        legacy.as_object_mut().unwrap().remove("cost");
        std::fs::write(path, serde_json::to_vec(&legacy).unwrap()).unwrap();
    }
    let offline = Owners::default();
    let status = offline
        .inspect(
            tmp.path(),
            "workspace",
            "diagnostics_status",
            &json!({"investigationId":id}),
        )
        .unwrap();
    assert_eq!(status["collectors"][0]["collectorId"], cid);
    assert!(status["collectors"][0]["cost"].is_null());
    let status = offline
        .inspect(
            tmp.path(),
            "workspace",
            "diagnostics_analysis_status",
            &json!({"investigationId":id,"analysisId":aid}),
        )
        .unwrap();
    assert_eq!(status["phase"], "complete");
    assert!(status["cost"].is_null());
    assert_eq!(read_data(&dir, &cid).unwrap(), raw);
}
async fn fixture() -> (tempfile::TempDir, Owners, String, String) {
    let tmp = tempfile::tempdir().unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(tmp.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let owner = Owners::new("one".into());
    let binding = json!({"targetId":"android:Acme","appId":"acme","buildId":"build-a","imageId":"image-a","processId":42,"processGeneration":"start-a","runtimeId":"runtime-a","runtimeGeneration":"generation-a"});
    let opened = owner
        .open(
            tmp.path(),
            "workspace",
            &json!({"submissionId":"case","question":"Acme evidence","binding":binding}),
        )
        .unwrap();
    let id = opened["investigationId"].as_str().unwrap().to_string();
    let source = tmp.path().join("input.json");
    std::fs::write(&source, serde_json::to_vec(&events()).unwrap()).unwrap();
    let (result,work)=owner.start(tmp.path(),"workspace",&json!({"investigationId":id,"submissionId":"input","source":{"kind":"import","path":source,"format":"testrun-events-v1"}}),&builds::Owners::default()).unwrap();
    let cid = result["collectors"][0]["collectorId"]
        .as_str()
        .unwrap()
        .to_string();
    owner
        .run(
            work.unwrap(),
            tmp.path().into(),
            "workspace".into(),
            builds::Owners::default(),
        )
        .await;
    (tmp, owner, id, cid)
}
fn prepare(
    owner: &Owners,
    root: &Path,
    id: &str,
    cid: &str,
    metadata: Value,
) -> (Value, AnalysisWork) {
    let (receipt,work)=owner.prepare_analysis(root,"workspace",&json!({"investigationId":id,"collectorId":cid,"submissionId":"analysis","metadata":metadata})).unwrap();
    (receipt, work.unwrap())
}
#[test]
fn analysis_decoders_preserve_real_format_units_and_source_witnesses() {
    let chrome=decode::decode("chrome-trace",br#"{"traceEvents":[{"name":"work","ph":"X","ts":100,"dur":12,"pid":1,"tid":2},{"name":"async","ph":"b","ts":103,"id":"r1"},{"name":"rss","ph":"C","ts":104,"args":{"resident":64}}]}"#).unwrap();
    assert_eq!(chrome.rows[0].value, Some(12.0));
    assert_eq!(chrome.clocks[0].unit, "microseconds");
    assert_eq!(chrome.rows[0].source, "/traceEvents/0");
    assert_eq!(chrome.rows[3].unit.as_deref(), Some("producer-unspecified"));
    assert_eq!(chrome.unsupported, vec!["b"]);
    let metadata=decode::decode("chrome-trace",br#"[{"ph":"M","name":"process_name","pid":1,"args":{"name":"Acme"}},{"ph":"M","name":"thread_name","tid":2,"args":{"name":"main"}}]"#).unwrap();
    assert_eq!(metadata.rows[0].kind, "process");
    assert_eq!(metadata.rows[1].kind, "thread");
    let har=decode::decode("har",br#"{"log":{"version":"1.2","entries":[{"startedDateTime":"2026-01-01T00:00:00Z","time":8,"request":{"method":"GET","url":"https://example.com"},"response":{"status":500},"timings":{"blocked":-1,"receive":8}}]}}"#).unwrap();
    assert_eq!(har.rows.len(), 3);
    assert_eq!(har.rows[0].data["timings"]["blocked"], -1);
    assert_eq!(
        har.rows[1].data["networkData"]["phasesMillis"]["blocked"],
        Value::Null
    );
    assert_eq!(
        har.rows[1].data["networkData"]["phasesMillis"]["receive"],
        8.0
    );
    assert_eq!(har.rows[2].kind, "failure");
    assert!(har.rows[0].timestamp.is_none());
    assert!(
        decode::decode(
            "har",
            b"\xef\xbb\xbf{\"log\":{\"version\":\"1.2\",\"entries\":[]}}"
        )
        .is_ok()
    );
    let speedscope=decode::decode("speedscope",br#"{"$schema":"https://www.speedscope.app/file-format-schema.json","shared":{"frames":[{"name":"main","file":"app.js","line":2}]},"profiles":[{"type":"sampled","name":"cpu","unit":"microseconds","startValue":0,"endValue":5,"samples":[[0],[0]],"weights":[2,3]}]}"#).unwrap();
    assert_eq!(speedscope.rows[1].timestamp, Some(2.0));
    assert_eq!(speedscope.rows[1].value, Some(3.0));
    assert_eq!(speedscope.rows[0].data["frames"][0]["name"], "main");
    assert_eq!(decode_events().rows[0].source, "/records/0");
    assert!(decode::decode("chrome-trace", br#"[{"ph":"private-fixture","name":"x"}]"#).is_err());
}

#[tokio::test]
async fn analysis_manager_drain_retains_active_ownership_across_session_cache_eviction() {
    let (tmp, owner, id, cid) = fixture().await;
    let (_, work) = prepare(&owner, tmp.path(), &id, &cid, json!({}));
    let other = owner
        .open(
            tmp.path(),
            "workspace",
            &json!({"submissionId":"other","question":"other"}),
        )
        .unwrap();
    let other_dir = directory(
        tmp.path(),
        "workspace",
        other["investigationId"].as_str().unwrap(),
    )
    .unwrap();
    let dir = directory(tmp.path(), "workspace", &id).unwrap();
    {
        let mut state = owner.state.lock().unwrap();
        let inactive = state.records[&dir].clone();
        for n in 0..256 {
            state
                .records
                .insert(tmp.path().join(format!("inactive-{n}")), inactive.clone());
        }
        state.records.remove(&other_dir);
    }
    owner
        .inspect(
            tmp.path(),
            "workspace",
            "diagnostics_status",
            &json!({"investigationId":other["investigationId"]}),
        )
        .unwrap();
    owner.stop_all(None, "manager_draining");
    assert_eq!(work.cancelled.borrow().as_deref(), Some("manager_draining"));
    owner.finish_analysis(work, Err(Error::new("ANALYSIS_CANCELLED", "Drained")));
    assert!(!owner.active());
}
#[test]
fn analysis_rejects_malformed_unsupported_deep_and_expansion_bombs() {
    for (format,bytes) in [("jfr",b"FLR\0".as_slice()),("har",b"{}".as_slice()),("chrome-trace",br#"[{"name":"x","ph":"X","ts":0,"dur":-1}]"#.as_slice()),("speedscope",br#"{"$schema":"https://www.speedscope.app/file-format-schema.json","shared":{"frames":[]},"profiles":[{"type":"sampled","name":"cpu","unit":"bytes","startValue":0,"endValue":1,"samples":[[2]],"weights":[1]}]}"#.as_slice())]{assert!(decode::decode(format,bytes).is_err());}
    let mut deep = vec![b'['; 65];
    deep.extend_from_slice(&[b']'; 65]);
    assert_eq!(
        decode::bounded_json(&deep, 1000).unwrap_err().code,
        "ANALYSIS_BOUNDS"
    );
    let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    gzip.write_all(&vec![b' '; model::MAX_RAW + 1]).unwrap();
    assert_eq!(
        decode::decode("chrome-trace", &gzip.finish().unwrap())
            .err()
            .unwrap()
            .code,
        "ANALYSIS_BOUNDS"
    );
    let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    gzip.write_all(&serde_json::to_vec(&events()).unwrap())
        .unwrap();
    assert_eq!(
        decode::decode("testrun-events-v1", &gzip.finish().unwrap())
            .unwrap()
            .rows
            .len(),
        3
    );
}

#[test]
fn analysis_repeated_frames_are_bounded_before_model_amplification() {
    let frame = json!({"name":"f","file":"x".repeat(4000)});
    let profile = |samples: Value| json!({"$schema":"https://www.speedscope.app/file-format-schema.json","shared":{"frames":[frame]},"profiles":[{"type":"sampled","name":"cpu","unit":"none","startValue":0,"endValue":1100,"weights":vec![1; samples.as_array().unwrap().len()],"samples":samples}]});
    let large_sample = profile(json!([vec![0; 9]]));
    assert_eq!(
        decode::decode("speedscope", &serde_json::to_vec(&large_sample).unwrap())
            .err()
            .unwrap()
            .code,
        "ANALYSIS_BOUNDS"
    );
    let many = profile(json!(vec![vec![0; 4]; 1100]));
    assert_eq!(
        decode::decode("speedscope", &serde_json::to_vec(&many).unwrap())
            .err()
            .unwrap()
            .code,
        "ANALYSIS_BOUNDS"
    );
}

#[tokio::test]
async fn analysis_close_keeps_investigation_busy_until_owned_parser_is_released() {
    let (tmp, owner, id, cid) = fixture().await;
    let (_, work) = prepare(&owner, tmp.path(), &id, &cid, json!({}));
    let status = owner
        .inspect(
            tmp.path(),
            "workspace",
            "diagnostics_status",
            &json!({"investigationId":id}),
        )
        .unwrap();
    assert_eq!(status["activeAnalyses"], 1);
    assert_eq!(status["settled"], false);
    owner
        .inspect(
            tmp.path(),
            "workspace",
            "diagnostics_stop",
            &json!({"investigationId":id,"collectorId":cid}),
        )
        .unwrap();
    assert!(work.cancelled.borrow().is_none());
    let closed = owner
        .inspect(
            tmp.path(),
            "workspace",
            "diagnostics_close",
            &json!({"investigationId":id}),
        )
        .unwrap();
    assert_eq!(closed["settled"], false);
    assert!(work.cancelled.borrow().is_some());
    assert_eq!(
        owner
            .inspect(
                tmp.path(),
                "workspace",
                "diagnostics_remove",
                &json!({"investigationId":id})
            )
            .unwrap_err()
            .code,
        "DIAGNOSTIC_BUSY"
    );
    owner.finish_analysis(work, Err(Error::new("ANALYSIS_CANCELLED", "Stopped")));
    assert_eq!(
        owner
            .inspect(
                tmp.path(),
                "workspace",
                "diagnostics_status",
                &json!({"investigationId":id})
            )
            .unwrap()["settled"],
        true
    );
    owner
        .inspect(
            tmp.path(),
            "workspace",
            "diagnostics_remove",
            &json!({"investigationId":id}),
        )
        .unwrap();
    assert!(tmp.path().join("input.json").exists());
}
#[tokio::test]
async fn analysis_retains_private_index_replay_and_offline_queries_without_source() {
    let (tmp, owner, id, cid) = fixture().await;
    let (receipt, work) = prepare(
        &owner,
        tmp.path(),
        &id,
        &cid,
        json!({"observationComplete":true,"sampling":"all instrumented events"}),
    );
    let aid = receipt["analysisId"].as_str().unwrap();
    owner.finish_analysis(work, Ok(decode_events()));
    std::fs::remove_file(tmp.path().join("input.json")).unwrap();
    let (replay,work)=owner.prepare_analysis(tmp.path(),"workspace",&json!({"investigationId":id,"collectorId":cid,"submissionId":"analysis","metadata":{"observationComplete":true,"sampling":"all instrumented events"}})).unwrap();
    assert!(work.is_none());
    assert_eq!(replay["phase"], "complete");
    let offline = Owners::default();
    let result = offline
        .inspect(
            tmp.path(),
            "workspace",
            "diagnostics_query",
            &json!({"investigationId":id,"analysisId":aid,"limit":1,"correlationId":"action:1"}),
        )
        .unwrap();
    assert!(result.get("matched").is_none());
    assert!(result["nextCursor"].is_string());
    let next=offline.inspect(tmp.path(),"workspace","diagnostics_query",&json!({"investigationId":id,"analysisId":aid,"limit":100,"correlationId":"action:1","cursor":result["nextCursor"]})).unwrap();
    assert_eq!(next["rows"].as_array().unwrap().len(), 1);
    assert!(next["nextCursor"].is_null());
    assert_eq!(offline.inspect(tmp.path(),"workspace","diagnostics_query",&json!({"investigationId":id,"analysisId":aid,"correlationId":"other","cursor":result["nextCursor"]})).unwrap_err().code,"invalid_cursor");
    assert_eq!(result["rows"][0]["evidence"]["source"], "/records/0");
    assert!(result["rows"][0]["alignedTime"].is_null());
    assert_eq!(result["observationComplete"], true);
    assert!(
        offline
            .inspect(
                tmp.path(),
                "foreign",
                "diagnostics_query",
                &json!({"investigationId":id,"analysisId":aid})
            )
            .is_err()
    );
    let empty = offline
        .inspect(
            tmp.path(),
            "workspace",
            "diagnostics_query",
            &json!({"investigationId":id,"analysisId":aid,"kind":"failure"}),
        )
        .unwrap();
    assert!(empty["rows"].as_array().unwrap().is_empty());
    assert!(
        empty["conclusion"]
            .as_str()
            .unwrap()
            .contains("zero matches")
    );
    assert!(
        offline
            .inspect(
                tmp.path(),
                "workspace",
                "diagnostics_query",
                &json!({"investigationId":id,"analysisId":aid,"from":0})
            )
            .is_err()
    );
    assert!(
        !serde_json::to_string(&replay)
            .unwrap()
            .contains("input.json")
    );
    owner
        .inspect(
            tmp.path(),
            "workspace",
            "diagnostics_close",
            &json!({"investigationId":id}),
        )
        .unwrap();
    owner
        .inspect(
            tmp.path(),
            "workspace",
            "diagnostics_remove",
            &json!({"investigationId":id}),
        )
        .unwrap();
    assert!(!directory(tmp.path(), "workspace", &id).unwrap().exists());
}
#[tokio::test]
async fn analysis_clocks_keep_uncertainty_and_reject_relabelled_source_units() {
    let (tmp, owner, id, cid) = fixture().await;
    let metadata = json!({"clocks":[{"id":"app","unit":"milliseconds","referenceDomain":"host","maxDriftPpm":100,"anchors":[{"source":100,"reference":1000,"uncertainty":2}]}]});
    let (receipt, work) = prepare(&owner, tmp.path(), &id, &cid, metadata);
    owner.finish_analysis(work, Ok(decode_events()));
    let result = owner
        .inspect(
            tmp.path(),
            "workspace",
            "diagnostics_query",
            &json!({"investigationId":id,"analysisId":receipt["analysisId"],"kind":"metric"}),
        )
        .unwrap();
    let time = &result["rows"][0]["alignedTime"];
    assert_eq!(time["domain"], "host");
    assert!((time["value"].as_f64().unwrap() - 1.01).abs() < 1e-9);
    assert!((time["uncertainty"].as_f64().unwrap() - 0.002001).abs() < 1e-9);
    let (receipt,work)=owner.prepare_analysis(tmp.path(),"workspace",&json!({"investigationId":id,"collectorId":cid,"submissionId":"bad-unit","metadata":{"clocks":[{"id":"app","unit":"seconds"}]}})).unwrap();
    owner.finish_analysis(work.unwrap(), Ok(decode_events()));
    let state = owner
        .inspect(
            tmp.path(),
            "workspace",
            "diagnostics_analysis_status",
            &json!({"investigationId":id,"analysisId":receipt["analysisId"]}),
        )
        .unwrap();
    assert_eq!(state["reason"], "CLOCK_MISMATCH");
}
#[tokio::test]
async fn analysis_symbol_references_require_retained_digest_build_and_architecture() {
    let (tmp, owner, id, cid) = fixture().await;
    let dir = directory(tmp.path(), "workspace", &id).unwrap();
    let hash = digest(&read_data(&dir, &cid).unwrap());
    let metadata = json!({"symbols":[{"kind":"source-map","evidenceId":cid,"sha256":hash,"buildId":"image-a","architecture":"arm64"}]});
    let (receipt, work) = prepare(&owner, tmp.path(), &id, &cid, metadata);
    owner.finish_analysis(work, Ok(decode_events()));
    let result = owner
        .inspect(
            tmp.path(),
            "workspace",
            "diagnostics_query",
            &json!({"investigationId":id,"analysisId":receipt["analysisId"]}),
        )
        .unwrap();
    assert_eq!(result["rows"][0]["symbols"]["state"], "reference_matched");
    assert_eq!(result["rows"][0]["symbols"]["resolved"], false);
    assert_eq!(result["rows"][1]["symbols"]["state"], "identity_unknown");
    assert!(owner.prepare_analysis(tmp.path(),"workspace",&json!({"investigationId":id,"collectorId":cid,"submissionId":"bad-symbol","metadata":{"symbols":[{"kind":"pdb","evidenceId":cid,"sha256":"0".repeat(64),"buildId":"image-b","architecture":"x64"}]}})).is_err());
}
#[tokio::test]
async fn analysis_owner_loss_cancellation_and_integrity_preserve_original_evidence() {
    let (tmp, owner, id, cid) = fixture().await;
    let (receipt, work) = prepare(&owner, tmp.path(), &id, &cid, json!({}));
    let aid = receipt["analysisId"].as_str().unwrap();
    let cancel = owner
        .inspect(
            tmp.path(),
            "workspace",
            "diagnostics_analysis_cancel",
            &json!({"investigationId":id,"analysisId":aid}),
        )
        .unwrap();
    assert_eq!(cancel["settled"], false);
    assert!(work.cancelled.borrow().is_some());
    owner.finish_analysis(work, Err(Error::new("ANALYSIS_CANCELLED", "Stopped")));
    assert!(!owner.active());
    let (_, work) = owner
        .prepare_analysis(
            tmp.path(),
            "workspace",
            &json!({"investigationId":id,"collectorId":cid,"submissionId":"lost"}),
        )
        .unwrap();
    let work = work.unwrap();
    let lost_id = work.receipt.id.clone();
    drop(work);
    drop(owner);
    let offline = Owners::default();
    let status = offline
        .inspect(
            tmp.path(),
            "workspace",
            "diagnostics_analysis_status",
            &json!({"investigationId":id,"analysisId":lost_id}),
        )
        .unwrap();
    assert_eq!(status["reason"], "OWNER_LOST");
    let dir = directory(tmp.path(), "workspace", &id).unwrap();
    assert_eq!(load_receipt(&dir, &lost_id).unwrap().phase, "preparing");
    assert!(!read_data(&dir, &cid).unwrap().is_empty());
    let owner = Owners::new("two".into());
    let (receipt, work) = owner
        .prepare_analysis(
            tmp.path(),
            "workspace",
            &json!({"investigationId":id,"collectorId":cid,"submissionId":"tamper"}),
        )
        .unwrap();
    owner.finish_analysis(work.unwrap(), Ok(decode_events()));
    std::fs::write(
        index_path(&dir, receipt["analysisId"].as_str().unwrap()).unwrap(),
        b"{}",
    )
    .unwrap();
    assert_eq!(
        owner
            .inspect(
                tmp.path(),
                "workspace",
                "diagnostics_query",
                &json!({"investigationId":id,"analysisId":receipt["analysisId"]})
            )
            .unwrap_err()
            .code,
        "EVIDENCE_CHANGED"
    );
}
#[tokio::test]
async fn analysis_compare_explains_process_clock_sampling_and_unit_mismatches() {
    let (tmp, owner, id, cid) = fixture().await;
    let (receipt, work) = prepare(
        &owner,
        tmp.path(),
        &id,
        &cid,
        json!({"observationComplete":true,"sampling":"all"}),
    );
    owner.finish_analysis(work, Ok(decode_events()));
    let dir = directory(tmp.path(), "workspace", &id).unwrap();
    let a = load_receipt(&dir, receipt["analysisId"].as_str().unwrap()).unwrap();
    let index = decode_events();
    assert_eq!(query::compare(&a, &index, &a, &index)["compatible"], true);
    let mut b = a.clone();
    b.binding["processGeneration"] = json!("reused-pid");
    b.metadata.sampling = Some("sampled".into());
    b.metadata.source_loss.push("dropped".into());
    let mut bi = decode_events();
    bi.clocks[0].id = "different-clock".into();
    bi.rows[1].unit = Some("KiB".into());
    let result = query::compare(&a, &index, &b, &bi);
    assert_eq!(result["compatible"], false);
    let reasons = result["incompatible"].as_array().unwrap();
    for reason in [
        "processGeneration",
        "sampling",
        "clocks",
        "measurement_definitions_or_units",
        "incomplete_observation",
    ] {
        assert!(reasons.contains(&json!(reason)));
    }
    assert!(result["observationDelta"].is_null());
}
