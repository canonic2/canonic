//! Report evidence stays inspectable without source hydration or authority changes.
use super::*;
use clap::Parser as _;

#[test]
fn review_resources_recover_full_comparison_omitted_from_a_bounded_tool_summary() {
    let (root, mut context) = crate::review::tests::enrolled();
    let proposed = crate::review::tests::propose(
        &context,
        &crate::review::tests::input("acme-large-report"),
        &|| Ok(()),
    )
    .unwrap();
    let payload = "Acme comparison evidence ".repeat(50_000);
    Store::open(root.path())
        .unwrap()
        .update_checkpoint(&context, &|| Ok(()), |state| {
            state.proposals[0].comparison = json!({"evidence":payload,"complete":false});
            Ok(Value::Null)
        })
        .unwrap();
    context.surface = "mcp".into();
    context.connection_id = Some("acme-report-inspector".into());
    let connection = crate::review::mcp::Connection::new("acme-report-inspector".into());
    let shown = crate::review::mcp::call(
        &context,
        &connection,
        "policy_show",
        &json!({"proposal_id":proposed["proposal_id"]}),
        &|| Ok(()),
    );
    assert!(shown["error"].is_null(), "{shown}");
    assert!(shown["data"]["comparison"].is_null());
    assert_eq!(shown["data"]["truncated"], true);
    let uri = shown["data"]["report_resources"][0]["uri"]
        .as_str()
        .unwrap();
    let full = read(&context, uri, &|| Ok(())).unwrap();
    assert_eq!(full["data"]["evidence"], payload);
    assert_eq!(full["data"]["complete"], false);
}

#[test]
fn review_resources_redact_private_evidence_and_require_retained_identity() {
    let (root, context) = crate::review::tests::enrolled();
    let mut proposed = crate::review::tests::propose(
        &context,
        &crate::review::tests::input("acme-report"),
        &|| Ok(()),
    )
    .unwrap();
    attach(&context, "policy_propose", &mut proposed);
    let comparison_uri = proposed["report_resources"][0]["uri"].as_str().unwrap();
    let evidence_uri = proposed["report_resources"][1]["uri"].as_str().unwrap();
    let store = Store::open(root.path()).unwrap();
    store.update_checkpoint(&context, &|| Ok(()), |state| {
        let row = state.proposals.iter_mut().find(|row| row.id == proposed["proposal_id"]).unwrap();
        row.comparison = json!({"before":{"scopes":[{"lock":{"mcp":{"cloud":{"local":true,
            "transport":{"type":"stdio","args":["acme-private-profile"]}}}}}]},"coverage":{"complete":false,"unknowns":["acme-unknown"]}});
        row.reasons["token"] = json!("acme-secret-token");
        row.receipt = json!({"token":"acme-secret-receipt"});
        Ok(Value::Null)
    }).unwrap();
    let state = store.read_checkpoint().unwrap();
    let selected = &state.proposals[0];
    let source = selected.candidate["digest"].as_str().unwrap();
    std::fs::remove_file(
        root.path()
            .join(format!(".shield/review/sources/{source}.json")),
    )
    .unwrap();
    let logs = assert_fs::TempDir::new().unwrap();
    let cli = crate::cli::Cli::parse_from([
        "shield",
        "--debug-log-dir",
        logs.path().to_str().unwrap(),
        "status",
    ]);
    let log = std::sync::Arc::new(crate::diagnostics::Log::open(&cli));
    let trace = log.request("mcp", "resources/read", None, None).unwrap();
    let path = root.path().join(".shield/review/state.json");
    let before = std::fs::read(&path).unwrap();
    {
        let _scope = trace.enter();
        let comparison = read(&context, comparison_uri, &|| Ok(())).unwrap();
        assert_eq!(comparison["redacted"], true);
        assert!(!comparison.to_string().contains("acme-private-profile"));
        assert_eq!(comparison["data"]["coverage"]["complete"], false);
        assert_eq!(
            comparison["data"]["coverage"]["unknowns"][0],
            "acme-unknown"
        );
        let evidence = read(&context, evidence_uri, &|| Ok(())).unwrap();
        assert_eq!(evidence["redacted"], true);
        assert!(!evidence.to_string().contains("acme-secret"));
        assert_eq!(std::fs::read(&path).unwrap(), before);
        for counter in [
            "source.hydrated",
            "policy.preparations",
            "history.captures",
            "objects.sweeps",
        ] {
            assert_eq!(trace.recorded_count(counter), 0, "{counter}");
        }
    }
    trace.finish("success");
    assert!(matches!(
        read(&context, comparison_uri, &|| Err(
            crate::policy::Error::Interrupted
        )),
        Err(Error::Policy(crate::policy::Error::Interrupted))
    ));
    store
        .update_checkpoint(&context, &|| Ok(()), |state| {
            state.proposals.clear();
            Ok(Value::Null)
        })
        .unwrap();
    assert!(matches!(
        read(&context, comparison_uri, &|| Ok(())),
        Err(Error::Missing)
    ));
}
