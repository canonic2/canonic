//! Independent producer fixtures exercise semantics, refusal and bounded pages.
use super::*;
fn capture(records: Value) -> Value {
    json!({"schemaVersion":1,"runtime":"flutter","buildMode":"profile","instrumentation":"Acme explicit app probe","clocks":[{"id":"app","unit":"microseconds"},{"id":"display","unit":"milliseconds"}],"records":records})
}
fn decoded(value: Value) -> Index {
    super::super::decode::decode("ui-performance-v1", &serde_json::to_vec(&value).unwrap()).unwrap()
}
fn point(name: &str, at: f64) -> Value {
    json!({"name":name,"clock":"app","at":at,"uncertainty":2,"definition":"producer observed milestone"})
}
fn query_for(index: &Index, operation: &str) -> Value {
    query(
        index,
        &Query {
            operation: operation.into(),
            min_target_size: None,
        },
        0,
        100,
    )
    .unwrap()
}
#[test]
fn diagnostic_performance_startup_keeps_stage_state_and_uncertainty_without_cross_clock_guessing() {
    let mut content = point("content_ready", 1.0);
    content["clock"] = json!("display");
    let index = decoded(capture(
        json!([{"type":"startup","runId":"launch1","state":"warm","stateEvidence":"process exists; Activity recreated","milestones":[point("launch_requested",100.0),point("first_display",450.0),point("interactive",600.0),content]}]),
    ));
    let result = query_for(&index, "startup");
    assert_eq!(result["matched"], 7);
    assert_eq!(result["rows"][2]["value"], 350.0);
    assert_eq!(
        result["rows"][2]["data"]["performance"]["intervalRangeMicros"],
        json!({"lower":346.0,"upper":354.0})
    );
    assert_eq!(result["rows"][2]["source"], "/records/0/milestones/1");
    assert_eq!(result["rows"][6]["value"], Value::Null);
    assert!(
        result["rows"][6]["data"]["performance"]["unavailable"]
            .as_str()
            .unwrap()
            .contains("different source clocks")
    );
    assert_eq!(result["measurementGroups"][0]["context"]["state"], "warm");
}
#[test]
fn diagnostic_performance_frame_deadlines_change_and_do_not_invent_display_or_dropped_frames() {
    let frame = |id: &str, end: u64, deadline: u64| json!({"type":"frame","id":id,"clock":"app","start":100,"end":end,"deadline":deadline,"thread":"raster","definition":"reported engine completion interval"});
    let index = decoded(capture(json!([
        frame("60hz", 16100, 16767),
        frame("120hz", 9100, 8433)
    ])));
    let result = query_for(&index, "frames");
    assert_eq!(
        result["rows"][0]["data"]["performance"]["missedDeadline"],
        false
    );
    assert_eq!(
        result["rows"][1]["data"]["performance"]["missedDeadline"],
        true
    );
    assert_eq!(
        result["rows"][1]["data"]["performance"]["dropped"],
        Value::Null
    );
    assert_eq!(result["measurementGroups"][0]["p95"], 16000.0);
    assert_eq!(result["measurementGroups"][0]["sum"], Value::Null);
}
#[test]
fn diagnostic_performance_interaction_preserves_unknown_delivery_and_unverified_media_references() {
    let index = decoded(capture(
        json!([{"type":"interaction","actionId":"tap1","points":[point("input_dispatched",100.0),point("frame_presented",800.0)],"evidenceRefs":["video:observed-frame-12"]}]),
    ));
    let rows = query_for(&index, "interaction");
    assert_eq!(rows["rows"][2]["value"], 700.0);
    assert_eq!(
        rows["rows"][2]["data"]["performance"]["from"],
        "input_dispatched"
    );
    assert_eq!(
        rows["rows"][2]["data"]["performance"]["context"]["evidenceRefs"][0],
        "video:observed-frame-12"
    );
    assert!(!rows.to_string().contains("app_received"));
}
fn node(id: &str, parent: Option<&str>) -> Value {
    json!({"id":id,"parent":parent,"role":"button","name":"","interactive":true,"hidden":false,"bounds":[0,0,30,20]})
}
#[test]
fn diagnostic_performance_tree_checks_do_not_substitute_component_or_view_nodes_for_accessibility()
{
    let tree = |id: &str, kind: &str| json!({"type":"tree","id":id,"treeKind":kind,"framework":"UIKit","nodes":[node("button",None)]});
    let index = decoded(capture(json!([
        tree("view", "view"),
        tree("component", "component"),
        tree("ax", "accessibility")
    ])));
    let q = Query {
        operation: "accessibility".into(),
        min_target_size: Some(44.0),
    };
    let result = query(&index, &q, 0, 1).unwrap();
    assert_eq!(result["matched"], 1);
    assert_eq!(result["rows"][0]["treeId"], "ax");
    assert_eq!(result["rows"][0]["missingAccessibleName"], true);
    assert_eq!(result["rows"][0]["belowDeclaredTargetSize"], true);
    assert_eq!(query_for(&index, "trees")["matched"], 3);
}
#[test]
fn diagnostic_performance_malformed_and_ambiguous_sources_fail_closed() {
    let records = [
        json!({"type":"startup","runId":"x","state":"cold","stateEvidence":"declared","milestones":[point("first_display",200.0),point("launch_requested",100.0)]}),
        json!({"type":"startup","runId":"x","state":"cold","stateEvidence":"declared","milestones":[point("first_display",200.0),point("first_display",300.0)]}),
        json!({"type":"stage","name":"layout","clock":"missing","start":1,"end":2,"thread":"UI","definition":"work"}),
        json!({"type":"tree","id":"cyclic","treeKind":"view","framework":"Compose","nodes":[node("a",Some("b")),node("b",Some("a"))]}),
        json!({"type":"tree","id":"missing-parent","treeKind":"view","framework":"SwiftUI","nodes":[node("a",Some("absent"))]}),
        json!({"type":"frame","id":"x","clock":"app","start":4,"end":3,"thread":"UI","definition":"work"}),
        json!({"type":"gpu","name":"busy","value":101,"unit":"percent","definition":"reported","tool":"Metal","device":"Acme","captureRestrictions":"unknown"}),
    ];
    for record in records {
        assert!(
            super::super::decode::decode(
                "ui-performance-v1",
                &serde_json::to_vec(&capture(json!([record]))).unwrap()
            )
            .is_err()
        );
    }
    let mut extra = capture(json!([]));
    extra["evaluate"] = json!("must-not-run");
    assert!(
        super::super::decode::decode("ui-performance-v1", &serde_json::to_vec(&extra).unwrap())
            .is_err()
    );
    let mut aligned = capture(json!([]));
    aligned["clocks"][0]["referenceDomain"] = json!("host");
    assert!(
        super::super::decode::decode("ui-performance-v1", &serde_json::to_vec(&aligned).unwrap())
            .is_err()
    );
}
#[test]
fn diagnostic_performance_existing_android_flutter_and_browser_spans_keep_definitions_and_pages() {
    let mut index = decoded(capture(json!([])));
    for (kind, name, data, n) in [
        (
            "metric",
            "frame_duration",
            json!({"stateSource":true,"processGeneration":"birth1"}),
            22.0,
        ),
        (
            "frame",
            "flutter_build_duration",
            json!({"isolateId":"one"}),
            9000.0,
        ),
        (
            "frame",
            "flutter_raster_duration",
            json!({"isolateId":"one"}),
            12000.0,
        ),
        ("span", "Layout", json!({"pid":42}), 1.0),
    ] {
        emit(
            &mut index,
            kind,
            name,
            "/fixture".into(),
            data,
            None,
            Some((n, "producer-unit", "distinct producer duration")),
        )
        .unwrap();
    }
    assert_eq!(query_for(&index, "frames")["measurementGroupCount"], 3);
    let page = query(
        &index,
        &Query {
            operation: "summary".into(),
            min_target_size: None,
        },
        1,
        1,
    )
    .unwrap();
    assert_eq!(page["rows"].as_array().unwrap().len(), 1);
    assert_eq!(page["nextOffset"], 2);
    assert_eq!(page["measurementGroups"].as_array().unwrap().len(), 1);
    assert_eq!(query_for(&index, "stages")["matched"], 1);
}
fn package(bytes: u64) -> Value {
    json!({"schemaVersion":1,"platform":"android","architecture":"arm64","buildMode":"release","buildId":"build1","basis":"archive_compressed","source":"Acme package exporter","entries":[{"path":"assets/hero.webp","bytes":bytes,"category":"image"}]})
}
fn receipt() -> super::super::model::Receipt {
    serde_json::from_value(json!({"version":1,"id":"analysis-a","collectorId":"collector-a","signature":"s","owner":"test","phase":"complete","rawSha256":"00","rawBytes":1,"format":"package-size-v1","capturePhase":"complete","captureReason":null,"binding":{"targetId":"android:Acme","appId":"com.example.acme"},"metadata":{},"indexSha256":"00","reason":null})).unwrap()
}
#[test]
fn diagnostic_performance_package_delta_requires_same_basis_and_never_predicts_startup() {
    let decode = |v: Value| {
        super::super::decode::decode("package-size-v1", &serde_json::to_vec(&v).unwrap()).unwrap()
    };
    let a = decode(package(100));
    let b = decode(package(60));
    let result = compare(&receipt(), &a, &receipt(), &b);
    assert_eq!(result["rows"][0]["deltaBytes"], -40);
    assert_eq!(result["startupImpactEstablished"], false);
    let mut installed = package(60);
    installed["basis"] = json!("installed");
    assert_eq!(
        compare(&receipt(), &a, &receipt(), &decode(installed))["compatible"],
        false
    );
    let mut other = receipt();
    other.binding["appId"] = Value::Null;
    assert_eq!(compare(&other, &a, &receipt(), &b)["compatible"], false);
    let mut duplicate = package(10);
    let entry = duplicate["entries"][0].clone();
    duplicate["entries"].as_array_mut().unwrap().push(entry);
    assert!(
        super::super::decode::decode("package-size-v1", &serde_json::to_vec(&duplicate).unwrap())
            .is_err()
    );
    let mut traversal = package(10);
    traversal["entries"][0]["path"] = json!("../escape");
    assert!(
        super::super::decode::decode("package-size-v1", &serde_json::to_vec(&traversal).unwrap())
            .is_err()
    );
}
#[test]
fn diagnostic_performance_perfetto_actual_and_expected_keep_layer_process_and_jank_fields() {
    use base64::Engine;
    let envelope = json!({"schemaVersion":1,"backend":"perfetto","targetId":"android:Acme","declaredAppId":"com.example.acme","actionIds":[],"report":{"identity":{},"gaps":[]},"artifact":{"encoding":"identity","sha256":crate::diagnostics::digest(b"trace"),"base64":base64::engine::general_purpose::STANDARD.encode(b"trace")},"sections":[
        {"name":"actual_frames","format":"csv","text":"id,ts,dur,name,upid,layer_name,present_type,jank_type\n1,100,20000,88,5,Acme,Late,App Deadline Missed\n2,200,12000,88,6,Neighbor,On-time,None\n3,300,-1,89,5,Acme,Unknown,Unknown\n"},
        {"name":"expected_frames","format":"csv","text":"id,ts,dur,name,upid,layer_name\n4,100,16667,88,5,Acme\n"}
    ]});
    let index =
        super::super::decode::decode("native-trace-v1", &serde_json::to_vec(&envelope).unwrap())
            .unwrap();
    let result = query_for(&index, "frames");
    assert_eq!(result["matched"], 3);
    assert_eq!(result["measurementGroupCount"], 3);
    assert_eq!(
        result["rows"][0]["data"]["observation"]["fields"]["jank_type"],
        "App Deadline Missed"
    );
    assert_eq!(result["rows"][2]["name"], "expected_frames");
    assert!(
        index
            .unsupported
            .iter()
            .any(|s| s.contains("unfinished actual_frames"))
    );
}
#[test]
fn diagnostic_performance_large_tree_pages_keep_source_and_enforce_public_byte_budget() {
    let nodes: Vec<_> = (0..220)
        .map(|i| {
            let mut n = node(&format!("node{i}"), None);
            n["name"] = json!("x".repeat(1024));
            n
        })
        .collect();
    let index = decoded(capture(
        json!([{"type":"tree","id":"ax","treeKind":"accessibility","framework":"Acme","nodes":nodes}]),
    ));
    let options = |limit| {
        serde_json::from_value::<super::super::query::Query>(json!({"investigationId":"diag-a","analysisId":"analysis-a","view":"performance","performance":{"operation":"trees"},"limit":limit})).unwrap()
    };
    let page = super::super::query::query(&receipt(), &index, &options(1)).unwrap();
    assert!(page["nextCursor"].is_string());
    assert_eq!(page["rows"][0]["source"], "/records/0/nodes/0");
    assert_eq!(
        super::super::query::query(&receipt(), &index, &options(200))
            .unwrap_err()
            .code,
        "invalid_arguments"
    );
}
#[test]
fn diagnostic_performance_surface_exposes_strict_options_and_static_routes_without_capture() {
    let caps = crate::diagnostics::access::capabilities();
    assert_eq!(caps["performance"]["registration"], "implemented");
    let schema =
        &crate::diagnostics::surface::definition("diagnostics_query").unwrap()["inputSchema"];
    assert_eq!(
        schema["properties"]["performance"]["additionalProperties"],
        false
    );
    assert!(serde_json::from_value::<Query>(json!({"operation":"frames","forceGc":true})).is_err());
    assert!(
        query(
            &decoded(capture(json!([]))),
            &Query {
                operation: "frames".into(),
                min_target_size: Some(44.0)
            },
            0,
            10
        )
        .is_err()
    );
}
