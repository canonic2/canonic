//! Independent scenario records exercise source scope, media semantics and restoration safeguards.
use super::*;
fn capture(records: Value) -> Value {
    json!({"schemaVersion":1,"targetId":"android:Acme","appId":"com.example.acme","processGeneration":"boot:42:birth","hardware":"physical","coverage":"selected app markers and device reads","clocks":[{"id":"app","unit":"milliseconds"},{"id":"device","unit":"seconds"}],"records":records})
}
fn record(id: &str, at: f64, observation: Value) -> Value {
    json!({"id":id,"clock":"app","at":at,"scope":"app","actionId":"action1","observation":observation})
}
fn decoded(value: Value) -> Index {
    super::super::decode::decode("scenario-context-v1", &serde_json::to_vec(&value).unwrap())
        .unwrap()
}
fn q(index: &Index, operation: &str) -> Value {
    query(
        index,
        &Query {
            operation: operation.into(),
            clock_domain: None,
            action_id: None,
        },
        0,
        100,
    )
    .unwrap()
}
fn media(position: f64) -> Value {
    json!({"type":"media","sessionId":"session1","itemId":"track1","state":"playing","positionMs":position,"positionSource":"service_extrapolated","bufferState":"ready","route":"speaker reported","audioFocus":"granted"})
}
#[test]
fn diagnostic_scenario_context_lifecycle_preserves_action_and_separate_source_clocks() {
    let mut locked = record(
        "lock",
        1.0,
        json!({"type":"lifecycle","event":"device_locked","reason":"device observer"}),
    );
    locked["clock"] = json!("device");
    locked["scope"] = json!("device");
    let index = decoded(capture(json!([
        record(
            "job",
            0.0,
            json!({"type":"lifecycle","event":"job_started","resourceId":"job1","reason":"app marker"})
        ),
        locked
    ])));
    assert_eq!(q(&index, "lifecycle")["matched"], 2);
    assert!(
        query(
            &index,
            &Query {
                operation: "timeline".into(),
                clock_domain: None,
                action_id: None
            },
            0,
            10
        )
        .is_err()
    );
    let page = query(
        &index,
        &Query {
            operation: "timeline".into(),
            clock_domain: Some("app".into()),
            action_id: Some("action1".into()),
        },
        0,
        10,
    )
    .unwrap();
    assert_eq!(page["matched"], 1);
    assert_eq!(page["rows"][0]["source"], "/records/0");
    assert_eq!(index.rows[1].data["appId"], Value::Null);
}
#[test]
fn diagnostic_scenario_context_media_progress_is_not_audibility_or_process_liveness() {
    let mut sampled = media(1200.0);
    sampled["audioSample"] = json!({"rmsDbfs":-20,"peakDbfs":-10,"frames":48000,"durationMs":1000,"health":"partial","routeChanged":true});
    let index = decoded(capture(json!([
        record("first", 0.0, media(100.0)),
        record("second", 1000.0, sampled),
        record(
            "death",
            1001.0,
            json!({"type":"lifecycle","event":"process_exited","reason":"producer exit witness"})
        )
    ])));
    let result = q(&index, "media");
    assert_eq!(
        result["rows"][1]["data"]["positionObservation"]["deltaMs"],
        1100.0
    );
    assert!(
        result["rows"][1]["data"]["positionObservation"]["meaning"]
            .as_str()
            .unwrap()
            .contains("service-projected")
    );
    assert_eq!(
        result["rows"][1]["data"]["scenario"]["observation"]["audioSample"]["routeChanged"],
        true
    );
    assert!(
        result["rows"][1]["data"]["audioMeaning"]
            .as_str()
            .unwrap()
            .contains("audibility")
    );
    assert_eq!(q(&index, "lifecycle")["matched"], 1);
}
#[test]
fn diagnostic_scenario_context_media_item_and_position_source_changes_are_not_joined() {
    let mut other = media(500.0);
    other["itemId"] = json!("track2");
    let mut source = media(600.0);
    source["positionSource"] = json!("decoded");
    let index = decoded(capture(json!([
        record("a", 0.0, media(100.0)),
        record("b", 1.0, other),
        record("c", 2.0, source),
        record("d", 3.0, media(50.0))
    ])));
    assert!(index.rows[1].data.get("positionObservation").is_none());
    assert!(index.rows[2].data.get("positionObservation").is_none());
    assert!(index.rows[3].data["positionObservation"]["unavailable"].is_string());
}
#[test]
fn diagnostic_scenario_context_energy_requires_source_units_and_explicit_attribution() {
    let energy = json!({"type":"energy","kind":"energy","value":12,"unit":"millijoule","attribution":"process","attributionEvidence":"producer per-process counter","definition":"counter interval supplied by instrument"});
    let index = decoded(capture(json!([record("app", 0.0, energy.clone())])));
    assert_eq!(index.rows[0].unit.as_deref(), Some("millijoule"));
    let mut bad = energy;
    bad["attributionEvidence"] = Value::Null;
    assert!(
        super::super::decode::decode(
            "scenario-context-v1",
            &capture(json!([record("bad", 0.0, bad)]))
                .to_string()
                .into_bytes()
        )
        .is_err()
    );
    let battery = json!({"type":"energy","kind":"battery_level","value":80,"unit":"percent","attribution":"process","attributionEvidence":"battery read","definition":"device level"});
    assert!(
        super::super::decode::decode(
            "scenario-context-v1",
            capture(json!([record("bad", 0.0, battery)]))
                .to_string()
                .as_bytes()
        )
        .is_err()
    );
    let mut temperature = record(
        "temp",
        0.0,
        json!({"type":"energy","kind":"temperature","value":-2,"unit":"celsius","attribution":"device","definition":"battery sensor, not CPU"}),
    );
    temperature["scope"] = json!("device");
    let index = decoded(capture(json!([temperature])));
    assert_eq!(index.rows[0].value, Some(-2.0));
    assert!(index.rows[0].data["appId"].is_null());
}
#[test]
fn diagnostic_scenario_context_settings_preserve_external_changes_and_unknown_outcomes() {
    for (before, applied, current, phase, owner, expected) in [
        (
            json!("online"),
            json!("offline"),
            json!("offline"),
            "changed",
            "owned_scenario",
            "restore_candidate_requires_owner_authority_and_fresh_readback",
        ),
        (
            json!(1),
            json!(2),
            json!(3),
            "changed",
            "owned_scenario",
            "concurrent_change_preserved",
        ),
        (
            json!(1),
            json!(2),
            json!(2),
            "changed",
            "external",
            "external_setting_preserved",
        ),
        (
            json!(1),
            json!(2),
            json!(2),
            "outcome_unknown",
            "owned_scenario",
            "inspect_owner_outcome_before_restore",
        ),
        (
            Value::Null,
            json!(2),
            json!(2),
            "changed",
            "owned_scenario",
            "setting_readback_unavailable",
        ),
        (
            json!(1),
            json!(2),
            json!(2),
            "restored",
            "owned_scenario",
            "reported_restore_readback_mismatch",
        ),
    ] {
        assert_eq!(restore(&before, &applied, &current, phase, owner), expected);
    }
    let index = decoded(capture(json!([record(
        "setting",
        0.0,
        json!({"type":"setting","name":"connectivity","before":"online","applied":"offline","current":"offline","phase":"restore_failed","owner":"owned_scenario","receiptRef":"network-action1","restoreError":"readback unavailable"})
    )])));
    let result = q(&index, "restore");
    assert_eq!(result["matched"], 1);
    assert_eq!(
        result["rows"][0]["data"]["restoration"]["status"],
        "reported_restore_failure"
    );
    assert_eq!(result["rows"][0]["data"]["restoration"]["performed"], false);
}
#[test]
fn diagnostic_scenario_context_peripheral_markers_expose_effects_without_active_probes() {
    let index = decoded(capture(json!([record(
        "camera",
        0.0,
        json!({"type":"peripheral","kind":"camera","event":"session started","resourceId":"camera-session1","effects":["camera_active","external_device"]})
    )])));
    assert_eq!(q(&index, "peripherals")["matched"], 1);
    assert!(
        index.rows[0].data["probeMeaning"]
            .as_str()
            .unwrap()
            .contains("no active")
    );
    let mut bad = capture(json!([]));
    bad["startCamera"] = json!(true);
    assert!(
        super::super::decode::decode("scenario-context-v1", bad.to_string().as_bytes()).is_err()
    );
}
#[test]
fn diagnostic_scenario_context_invalid_clock_identity_state_and_ownership_refuse() {
    for observation in [
        json!({"type":"lifecycle","event":"made_up","reason":"none"}),
        json!({"type":"setting","name":"locale","before":"en","applied":"fr","current":"fr","phase":"changed","owner":"owned_scenario"}),
        json!({"type":"energy","kind":"power","value":1,"unit":"percent","attribution":"device","definition":"bad"}),
    ] {
        assert!(
            super::super::decode::decode(
                "scenario-context-v1",
                capture(json!([record("bad", 0.0, observation)]))
                    .to_string()
                    .as_bytes()
            )
            .is_err()
        );
    }
    let event = record(
        "a",
        1.0,
        json!({"type":"lifecycle","event":"paused","reason":"marker"}),
    );
    for value in [
        capture(json!([event.clone(), event.clone()])),
        {
            let mut v = capture(json!([event.clone()]));
            v["processGeneration"] = Value::Null;
            v
        },
        {
            let mut v = capture(json!([event]));
            v["records"][0]["clock"] = json!("missing");
            v
        },
    ] {
        assert!(
            super::super::decode::decode("scenario-context-v1", value.to_string().as_bytes())
                .is_err()
        );
    }
}
#[test]
fn diagnostic_scenario_context_pages_and_strict_surface_bound_private_results() {
    let rows:Vec<_>=(0..200).map(|i|record(&format!("setting{i}"),i as f64,json!({"type":"setting","name":"locale","before":"a".repeat(1024),"applied":"b".repeat(1024),"current":"b".repeat(1024),"phase":"changed","owner":"owned_scenario","receiptRef":"owner-action"}))).collect();
    let index = decoded(capture(json!(rows)));
    let receipt:super::super::model::Receipt=serde_json::from_value(json!({"version":1,"id":"analysis-a","collectorId":"collector-a","signature":"s","owner":"test","phase":"complete","rawSha256":"00","rawBytes":1,"format":"scenario-context-v1","capturePhase":"complete","captureReason":null,"binding":{},"metadata":{},"indexSha256":"00","reason":null})).unwrap();
    let options = |limit| {
        serde_json::from_value::<super::super::query::Query>(json!({"investigationId":"diag-a","analysisId":"analysis-a","view":"scenario","scenario":{"operation":"settings"},"limit":limit})).unwrap()
    };
    assert!(
        super::super::query::query(&receipt, &index, &options(1)).unwrap()["nextCursor"]
            .is_string()
    );
    let page = super::super::query::query(&receipt, &index, &options(100)).unwrap();
    assert!(page["nextCursor"].is_string());
    assert!(page["rows"].as_array().unwrap().len() < 100);
    assert_eq!(
        super::super::query::query(&receipt, &index, &options(200))
            .unwrap_err()
            .code,
        "invalid_arguments"
    );
    assert!(
        serde_json::from_value::<Query>(json!({"operation":"restore","execute":true})).is_err()
    );
    assert_eq!(
        crate::diagnostics::surface::definition("diagnostics_query").unwrap()["inputSchema"]["properties"]
            ["scenario"]["additionalProperties"],
        false
    );
}
#[test]
fn diagnostic_scenario_context_native_android_scopes_and_unknowns_remain_visible() {
    let mut index = decoded(capture(json!([])));
    for group in ["jobs", "battery", "audio", "font_scale"] {
        let mut row = super::super::decode::row(
            "event",
            "Android state snapshot",
            "line:1/data".into(),
            json!({"stateSource":true,"group":group,"scope":"device","appId":null,"complete":false}),
        );
        row.timestamp = Some(1.0);
        row.clock_domain = Some("device".into());
        index.rows.push(row);
    }
    assert_eq!(q(&index, "summary")["matched"], 4);
    assert_eq!(q(&index, "energy")["matched"], 1);
    assert_eq!(q(&index, "restore")["matched"], 0);
}
