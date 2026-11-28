use super::*;
fn id(n: usize) -> String {
    format!("analysis-{n:032x}")
}
fn source(n: usize) -> Source {
    let r:Receipt=serde_json::from_value(json!({"version":1,"id":id(n),"collectorId":format!("collector-{n:032x}"),"signature":"s","owner":"o","phase":"complete","rawSha256":"a".repeat(64),"rawBytes":1,"format":"testrun-events-v1","capturePhase":"complete","binding":{"appId":"private-app","buildId":"private-build"},"metadata":{},"indexSha256":"b".repeat(64)})).unwrap();
    let i=super::super::decode::decode("testrun-events-v1",&serde_json::to_vec(&json!({"schemaVersion":1,"clocks":[{"id":"private-clock","unit":"milliseconds"}],"records":[{"kind":"failure","name":"private-name","timestamp":if n==1{100}else{1},"clockDomain":"private-clock","data":{"secret":"private-body"}},{"kind":"metric","name":"private-metric","value":4,"unit":"bytes","definition":"private-definition","data":{}}]})).unwrap()).unwrap();
    (r, Arc::new(i))
}
#[test]
fn diagnostic_reports_cross_source_pages_keep_clock_and_witness_identity() {
    let sources = vec![source(1), source(2)];
    let r = Request {
        analysis_ids: vec![id(2)],
        operation: Some("timeline".into()),
    };
    let page = query("inv", &sources, &r, 0, 1).unwrap();
    assert_eq!(page["rows"][0]["evidence"]["analysisId"], id(1));
    assert_eq!(page["rows"][0]["record"]["timestamp"], 100.0);
    assert_eq!(page["nextOffset"], 1);
    let next = query("inv", &sources, &r, 1, 1).unwrap();
    assert_eq!(next["rows"][0]["evidence"]["analysisId"], id(2));
    assert_eq!(next["rows"][0]["record"]["timestamp"], 1.0);
    assert!(next["nextOffset"].is_null());
    for op in [
        "summary",
        "failures",
        "stacks",
        "resources",
        "frames",
        "retention",
        "requests",
    ] {
        assert!(
            query(
                "inv",
                &sources,
                &Request {
                    operation: Some(op.into()),
                    ..Request::default()
                },
                0,
                20
            )
            .is_ok()
        );
    }
    assert!(ids(&id(1), &[id(1)]).is_err());
    assert!(ids(&id(1), &[id(2), id(3), id(4), id(5)]).is_err());
}
#[test]
fn diagnostic_reports_minimal_omits_private_text_and_private_notes_keep_attribution() {
    let mut sources = vec![source(1), source(2)];
    Arc::get_mut(&mut sources[0].1).unwrap().rows[0].data["buildId"] =
        json!("private-native-build");
    let mut r = Report {
        analysis_ids: vec![id(2)],
        exact_build: Some(Build {
            id: "private-build".into(),
            architecture: "private-arch".into(),
            mode: "release".into(),
        }),
        reproduction: vec!["private-reproduction".into()],
        facts: vec![Statement {
            statement: "private-fact".into(),
            witnesses: vec![Witness {
                analysis_id: id(1),
                ordinal: 0,
            }],
        }],
        hypotheses: vec![Statement {
            statement: "private-hypothesis".into(),
            witnesses: vec![],
        }],
        correction: Some("private-correction".into()),
        comparison: Some(Comparison {
            baseline_analysis_id: id(1),
            corrected_analysis_id: id(2),
        }),
        unresolved: vec!["private-unresolved".into()],
        ..Report::default()
    };
    let minimal = report(
        "inv",
        "private-question",
        &json!({"appId":"private-app"}),
        &sources,
        &r,
    )
    .unwrap();
    assert!(!minimal.to_string().contains("private-"));
    assert_eq!(minimal["facts"][0]["witnesses"][0]["ordinal"], 0);
    assert_eq!(minimal["privacy"], "minimal");
    r.privacy = Some("private".into());
    let private = report("inv", "private-question", &json!({}), &sources, &r).unwrap();
    assert_eq!(private["question"], "private-question");
    assert_eq!(
        private["sources"][0]["reportedBuilds"][0]["reported"]["buildId"],
        "private-native-build"
    );
    assert_eq!(private["facts"][0]["statement"], "private-fact");
    assert_eq!(private["hypotheses"][0]["statement"], "private-hypothesis");
    assert!(!private.to_string().contains("private-body"));
    assert!(
        !private["comparison"]["incompatible"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    r.facts[0].witnesses[0].ordinal = 100;
    assert!(report("inv", "question", &json!({}), &sources, &r).is_err());
    r.facts[0].witnesses.clear();
    assert!(report("inv", "question", &json!({}), &sources, &r).is_err());
}
#[test]
fn diagnostic_reports_bounds_and_unknown_operations_refuse_without_partial_conclusions() {
    let sources = vec![source(1)];
    assert!(
        query(
            "inv",
            &sources,
            &Request {
                operation: Some("execute".into()),
                ..Request::default()
            },
            0,
            20
        )
        .is_err()
    );
    assert!(query("inv", &sources, &Request::default(), 0, 201).is_err());
    let mut request = Report {
        reproduction: vec!["step".into(); 17],
        ..Report::default()
    };
    assert!(report("inv", "question", &json!({}), &sources, &request).is_err());
    request.reproduction.clear();
    request.privacy = Some("auto_upload".into());
    assert!(report("inv", "question", &json!({}), &sources, &request).is_err());
}
#[test]
fn diagnostic_reports_markdown_does_not_allow_fence_or_html_injection() {
    let bytes = markdown(&json!({"question":"```<script>private</script>"})).unwrap();
    let text = String::from_utf8(bytes).unwrap();
    assert!(!text.contains("<script>"));
    assert!(!text.contains("```<"));
    assert!(text.contains("\\u0060"));
}
#[test]
fn diagnostic_reports_schemas_match_public_options_and_refuse_unknown_execution() {
    let name = "diagnostics_query";
    let tool = crate::diagnostics::definition(name).unwrap();
    let args = json!({"investigationId":"diag-acme","analysisId":id(1),"view":"report","report":{"privacy":"private","facts":[{"statement":"observed","witnesses":[{"analysisId":id(1),"ordinal":0}]}]}});
    crate::schema::validate(&tool["inputSchema"], &args).unwrap();
    let mut bad = args;
    bad["report"]["execute"] = json!(true);
    assert!(crate::schema::validate(&tool["inputSchema"], &bad).is_err());
    let export = crate::diagnostics::definition("diagnostics_export").unwrap();
    crate::schema::validate(&export["inputSchema"],&json!({"investigationId":"diag-acme","analysisId":id(1),"scope":"all","format":"investigation-markdown","report":{},"path":"/tmp/report.md"})).unwrap();
}
