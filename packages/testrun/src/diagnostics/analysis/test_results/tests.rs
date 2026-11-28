use super::*;
use serde_json::Value;
fn packet() -> Value {
    json!({"schemaVersion":1,"runId":"run-1","runner":"xctest","invocationRef":"build-check-1","control":"testrun_released","buildId":"build-a","targetId":"simulator:Acme","coverage":"complete","cases":[{"id":"checkout","attempt":1,"name":"checkout renders","outcome":"assertion_failed","message":"missing button","durationMs":12.0,"actionIds":["tap-1"],"attachments":[{"kind":"screenshot","name":"failure","reference":"capture-1"}],"capture":{"strategy":"prebuffered","beforeMs":1000,"afterMs":500,"limitBytes":1048576,"lost":false},"checks":[{"id":"button","domain":"ui","outcome":"assertion_failed","message":"not visible"}]}]})
}
#[test]
fn diagnostic_testing_normalized_runners_preserve_identity_attempts_capture_and_private_attachments()
 {
    for runner in [
        "xctest",
        "swift_testing",
        "android_instrumentation",
        "espresso",
        "ui_automator",
        "flutter",
        "dart",
        "web",
        "package_native",
        "external",
    ] {
        let mut p = packet();
        p["runner"] = json!(runner);
        let index =
            super::super::decode::decode("test-results-v1", &serde_json::to_vec(&p).unwrap())
                .unwrap();
        assert_eq!(index.rows[0].kind, "failure");
        assert_eq!(index.rows[0].data["runner"], runner);
        assert!(index.rows[0].correlations.contains(&"tap-1".into()));
        assert_eq!(index.rows[0].data["attachmentsVerified"], false);
        assert_eq!(index.rows[0].data["test"]["capture"]["beforeMs"], 1000);
    }
}
#[test]
fn diagnostic_testing_refuses_retroactive_buffer_claims_duplicates_and_false_passes() {
    let mut cases = vec![];
    let mut p = packet();
    p["cases"][0]["capture"]["strategy"] = json!("post_failure");
    cases.push(p);
    let mut p = packet();
    let duplicate = p["cases"][0].clone();
    p["cases"].as_array_mut().unwrap().push(duplicate);
    cases.push(p);
    let mut p = packet();
    p["cases"][0]["outcome"] = json!("passed");
    cases.push(p);
    let mut p = packet();
    p["command"] = json!(["must-not-run"]);
    cases.push(p);
    for p in cases {
        assert!(
            super::super::decode::decode("test-results-v1", &serde_json::to_vec(&p).unwrap())
                .is_err()
        );
    }
}

#[test]
fn diagnostic_testing_keeps_failed_prior_attempt_when_a_later_attempt_passes() {
    let mut p = packet();
    let mut corrected = p["cases"][0].clone();
    corrected["attempt"] = json!(2);
    corrected["outcome"] = json!("passed");
    corrected["checks"][0]["outcome"] = json!("passed");
    p["cases"].as_array_mut().unwrap().push(corrected);
    let index =
        super::super::decode::decode("test-results-v1", &serde_json::to_vec(&p).unwrap()).unwrap();
    assert_eq!(index.rows.len(), 2);
    assert_eq!(index.rows[0].kind, "failure");
    assert_eq!(index.rows[1].kind, "event");
    assert_eq!(index.rows[0].data["test"]["attempt"], 1);
    assert_eq!(index.rows[1].data["test"]["attempt"], 2);
}
#[test]
fn diagnostic_testing_junit_preserves_failures_errors_skips_units_and_entities() {
    let xml=br#"<testsuites><testsuite name="Acme"><testcase name="pass" time="0.02"/><testcase name="assert"><failure message="a &amp; b">line &#49;</failure></testcase><testcase name="driver"><error message="transport lost"/></testcase><testcase name="skip"><skipped/></testcase></testsuite></testsuites>"#;
    let i = super::super::decode::decode("junit-xml", xml).unwrap();
    assert_eq!(i.rows.len(), 4);
    assert_eq!(i.rows[0].data["test"]["durationMs"], 20.0);
    assert_eq!(i.rows[1].data["test"]["outcome"], "assertion_failed");
    assert!(
        i.rows[1].data["test"]["message"]
            .as_str()
            .unwrap()
            .contains("a & b")
    );
    assert_eq!(i.rows[2].data["test"]["outcome"], "unavailable");
    assert_eq!(i.rows[2].kind, "failure");
    assert_eq!(i.rows[2].data["test"]["junitErrorUnclassified"], true);
    assert_eq!(i.rows[3].data["test"]["outcome"], "skipped");
    assert!(!i.unsupported.is_empty());
}
#[test]
fn diagnostic_testing_junit_refuses_entities_unknown_results_and_malformed_structure() {
    for xml in [
        "<!DOCTYPE x [<!ENTITY x SYSTEM 'file:///private/secret'>]><testsuite/>",
        "<testsuite><testcase name='x'><rerunFailure/></testcase></testsuite>",
        "<testsuite><testcase name='x'><failure/><error/></testcase></testsuite>",
        "<testsuite><testcase name='a' name='b'/></testsuite>",
        "<testsuite><testcase name='x'></testsuite>",
    ] {
        assert!(
            super::super::decode::decode("junit-xml", xml.as_bytes()).is_err(),
            "{xml}"
        );
    }
}
