use super::*;
use crate::diagnostics::analysis::model::{Clock, Metadata, PARSER, Row};

fn receipt() -> Receipt {
    serde_json::from_value(json!({"version":1,"id":format!("analysis-{}","1".repeat(32)),"collectorId":format!("collector-{}","2".repeat(32)),"signature":"signature","owner":"owner","phase":"complete","rawSha256":"a".repeat(64),"rawBytes":1,"format":"testrun-events-v1","capturePhase":"complete","binding":{"appId":"com.example.acme","processId":42,"processGeneration":"start-one"},"metadata":{},"indexSha256":"b".repeat(64)})).unwrap()
}
fn index() -> Index {
    let data = [
        json!({"appId":"com.example.acme","processGeneration":"start-one","Authorization":"Bearer private-fixture"}),
        json!({"systemContext":true,"relatedAppId":"com.example.acme","relatedProcessGeneration":"start-one","url":"wss://user:password@example.com/private-token"}),
        json!({"appId":"com.example.other","processGeneration":"start-one"}),
        json!({"appId":"com.example.acme","processGeneration":"old-process"}),
        json!({"pid":42}),
    ];
    Index {
        model_bytes: 0,
        version: 1,
        parser: PARSER.into(),
        format: "testrun-events-v1".into(),
        clocks: vec![Clock {
            id: "wss://example.com/private-clock-token".into(),
            unit: "milliseconds".into(),
            reference_domain: None,
            max_drift_ppm: None,
            anchors: vec![],
        }],
        rows: data
            .into_iter()
            .map(|data| Row {
                kind: "metric".into(),
                name: "password=private-fixture".into(),
                timestamp: Some(12.0),
                clock_domain: Some("wss://example.com/private-clock-token".into()),
                value: Some(64.0),
                unit: Some("token=private-unit-token".into()),
                definition: Some("secret definition".into()),
                correlations: vec!["private-correlation".into()],
                source: "/private-source-pointer".into(),
                data,
            })
            .collect(),
        unsupported: vec![],
    }
}
fn request(scope: Scope) -> Request {
    Request {
        investigation_id: format!("diag-{}", "3".repeat(32)),
        analysis_id: format!("analysis-{}", "1".repeat(32)),
        scope,
        kind: None,
        cursor: None,
        limit: Some(1),
        path: None,
        format: None,
        report: None,
    }
}
#[test]
fn diagnostic_projection_omits_sensitive_text_and_unknown_or_neighbor_app_generations() {
    let result = project(&receipt(), &index(), &request(Scope::App)).unwrap();
    assert_eq!(result["matched"], 2);
    assert_eq!(result["scope"]["excludedRows"], 3);
    assert!(result["nextCursor"].is_string());
    assert_eq!(result["rows"][0]["value"], 64.0);
    assert!(result["rows"][0]["unit"].is_null());
    assert_eq!(result["observationComplete"], false);
    let text = result.to_string();
    for private in [
        "private-fixture",
        "private-token",
        "private-clock-token",
        "private-unit-token",
        "secret definition",
        "private-correlation",
        "private-source-pointer",
        "com.example.other",
        "com.example.acme",
        "start-one",
    ] {
        assert!(!text.contains(private), "{private}");
    }
    let mut next = request(Scope::App);
    next.cursor = result["nextCursor"].as_str().map(str::to_owned);
    let page = project(&receipt(), &index(), &next).unwrap();
    assert_eq!(page["rows"][0]["ordinal"], 1);
    assert!(page["nextCursor"].is_null());
    assert_eq!(
        project(&receipt(), &index(), &request(Scope::All)).unwrap()["matched"],
        5
    );
}
#[test]
fn diagnostic_projection_pages_follow_cursors_and_refuse_foreign_or_malformed_ones() {
    let mut all = request(Scope::All);
    all.limit = Some(2);
    let mut ordinals = vec![];
    let mut pages = 0;
    loop {
        let page = project(&receipt(), &index(), &all).unwrap();
        pages += 1;
        ordinals.extend(
            page["rows"]
                .as_array()
                .unwrap()
                .iter()
                .map(|r| r["ordinal"].clone()),
        );
        match page["nextCursor"].as_str() {
            Some(cursor) => all.cursor = Some(cursor.into()),
            None => break,
        }
    }
    assert_eq!(ordinals, [0, 1, 2, 3, 4].map(|n| json!(n)));
    assert_eq!(pages, 3);
    // A cursor belongs to its filters and index, and limits are never clamped.
    let first = project(&receipt(), &index(), &request(Scope::All)).unwrap();
    let mut foreign = request(Scope::App);
    foreign.cursor = first["nextCursor"].as_str().map(str::to_owned);
    assert_eq!(
        project(&receipt(), &index(), &foreign).unwrap_err().code,
        "invalid_cursor"
    );
    let mut changed = receipt();
    changed.index_sha256 = Some("c".repeat(64));
    let mut stale = request(Scope::All);
    stale.cursor = first["nextCursor"].as_str().map(str::to_owned);
    assert_eq!(
        project(&changed, &index(), &stale).unwrap_err().code,
        "invalid_cursor"
    );
    stale.cursor = Some("not-a-cursor".into());
    assert_eq!(
        project(&receipt(), &index(), &stale).unwrap_err().code,
        "invalid_cursor"
    );
    for limit in [0, 101] {
        let mut q = request(Scope::All);
        q.limit = Some(limit);
        assert!(project(&receipt(), &index(), &q).is_err(), "{limit}");
    }
}
#[test]
fn diagnostic_projection_requires_explicit_broad_scope_when_app_identity_is_unknown() {
    let mut r = receipt();
    r.binding = json!({});
    r.metadata = Metadata::default();
    assert_eq!(
        project(&r, &index(), &request(Scope::App))
            .unwrap_err()
            .code,
        "DIAGNOSTIC_SCOPE_REQUIRED"
    );
    assert!(project(&r, &index(), &request(Scope::All)).is_ok());
    let mut q = request(Scope::All);
    q.limit = Some(201);
    assert!(project(&r, &index(), &q).is_err());
}
#[test]
fn diagnostic_export_is_deterministic_private_and_never_overwrites_different_content() {
    let tmp = tempfile::tempdir().unwrap();
    let runtime = tmp.path().join("state");
    std::fs::create_dir(&runtime).unwrap();
    let path = tmp.path().join("report.json");
    let report = project(&receipt(), &index(), &request(Scope::All)).unwrap();
    assert_eq!(
        export(&runtime, &path, &report).unwrap()["alreadyPresent"],
        false
    );
    assert_eq!(
        export(&runtime, &path, &report).unwrap()["alreadyPresent"],
        true
    );
    let original = std::fs::read(&path).unwrap();
    assert!(
        !String::from_utf8(original.clone())
            .unwrap()
            .contains("private-fixture")
    );
    assert_eq!(
        export(&runtime, &path, &json!({"different":true}))
            .unwrap_err()
            .code,
        "EXPORT_CONFLICT"
    );
    assert_eq!(std::fs::read(&path).unwrap(), original);
    assert_eq!(std::fs::read_dir(tmp.path()).unwrap().count(), 2);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    assert_eq!(
        export(&runtime, &runtime.join("report.json"), &report)
            .unwrap_err()
            .code,
        "EXPORT_PATH"
    );
}
#[cfg(unix)]
#[test]
fn diagnostic_export_refuses_symlinks_without_replacing_their_targets() {
    let tmp = tempfile::tempdir().unwrap();
    let runtime = tmp.path().join("state");
    std::fs::create_dir(&runtime).unwrap();
    let original = tmp.path().join("original");
    std::fs::write(&original, b"keep").unwrap();
    let link = tmp.path().join("link");
    std::os::unix::fs::symlink(&original, &link).unwrap();
    assert_eq!(
        export(&runtime, &link, &json!({})).unwrap_err().code,
        "EXPORT_CONFLICT"
    );
    assert_eq!(std::fs::read(&original).unwrap(), b"keep");
    assert!(std::fs::symlink_metadata(link).unwrap().is_symlink());
}
