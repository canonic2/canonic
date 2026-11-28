use super::*;
fn fixture() -> (Receipt, Index) {
    let r=serde_json::from_value(json!({"version":1,"id":"analysis-a","collectorId":"collector-a","signature":"s","owner":"fixture","phase":"complete","rawSha256":"00","rawBytes":1,"format":"testrun-events-v1","capturePhase":"complete","captureReason":null,"binding":{},"metadata":{"observationComplete":true},"indexSha256":"00","reason":null})).unwrap();
    let raw = json!({"schemaVersion":1,"clocks":[],"records":[{"kind":"metric","name":"checkout","value":40,"unit":"milliseconds","definition":"request to rendered result","data":{"visible":true,"nullable":null}},{"kind":"metric","name":"checkout","value":60,"unit":"milliseconds","definition":"request to rendered result","data":{"visible":true,"nullable":null}}]});
    let i = super::super::decode::decode("testrun-events-v1", &serde_json::to_vec(&raw).unwrap())
        .unwrap();
    (r, i)
}
fn check(predicate: Value) -> Request {
    serde_json::from_value(json!({"checks":[{"id":"c1","domain":"performance","name":"checkout","predicate":predicate}]})).unwrap()
}
#[test]
fn diagnostic_testing_assertions_use_exact_metric_definitions_and_digest_witnesses() {
    let (r, i) = fixture();
    let q = check(
        json!({"type":"metric","unit":"milliseconds","definition":"request to rendered result","max":55,"aggregation":"mean"}),
    );
    let result = evaluate(&r, &i, &q).unwrap();
    assert_eq!(result["outcome"], "passed");
    assert_eq!(result["checks"][0]["witnessOrdinals"], json!([0, 1]));
    assert_eq!(result["rawSha256"], "00");
    let q = check(
        json!({"type":"metric","unit":"milliseconds","definition":"request to rendered result","max":55,"aggregation":"max"}),
    );
    assert_eq!(evaluate(&r, &i, &q).unwrap()["outcome"], "assertion_failed");
    let q = check(
        json!({"type":"metric","unit":"bytes","definition":"request to rendered result","max":55,"aggregation":"mean"}),
    );
    assert_eq!(evaluate(&r, &i, &q).unwrap()["outcome"], "unavailable");
}
#[test]
fn diagnostic_testing_assertions_distinguish_missing_null_and_incomplete_absence() {
    let (mut r, i) = fixture();
    for (pointer, expected) in [("/nullable", "passed"), ("/missing", "unavailable")] {
        assert_eq!(
            evaluate(
                &r,
                &i,
                &check(json!({"type":"equals","pointer":pointer,"expected":null}))
            )
            .unwrap()["outcome"],
            expected
        );
    }
    r.metadata.observation_complete = false;
    assert_eq!(
        evaluate(&r, &i, &check(json!({"type":"count","min":0,"max":2}))).unwrap()["outcome"],
        "unavailable"
    );
    assert_eq!(
        evaluate(&r, &i, &check(json!({"type":"count","min":1}))).unwrap()["outcome"],
        "passed"
    );
}
#[test]
fn diagnostic_testing_assertions_keep_automation_fixture_and_unavailable_outcomes_distinct() {
    let (r, mut i) = fixture();
    for outcome in ["automation_failed", "fixture_failed", "unavailable"] {
        i.rows[0].data["test"] = json!({"outcome":outcome});
        assert_eq!(
            evaluate(
                &r,
                &i,
                &check(json!({"type":"equals","pointer":"/visible","expected":true}))
            )
            .unwrap()["outcome"],
            outcome
        );
    }
}
#[test]
fn diagnostic_testing_assertions_bound_selections_and_refuse_execution_payloads() {
    let (r, mut i) = fixture();
    let row = i.rows[0].clone();
    i.rows = vec![row; 201];
    let result = evaluate(&r, &i, &check(json!({"type":"count","min":1}))).unwrap();
    assert_eq!(result["outcome"], "unavailable");
    assert_eq!(result["checks"][0]["selectionTruncated"], true);
    assert!(serde_json::from_value::<Request>(json!({"checks":[{"id":"c","domain":"ui","predicate":{"type":"evaluate","expression":"never()"}}]})).is_err());
    let q=serde_json::from_value(json!({"checks":[{"id":"c","domain":"ui","kind":"falure","predicate":{"type":"count","min":0,"max":0}}]})).unwrap();
    assert!(evaluate(&r, &i, &q).is_err());
}
