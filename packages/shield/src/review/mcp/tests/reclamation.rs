//! Connection-level continuity after crossing the former continuation limit.
use super::*;

#[test]
fn inspection_reclamation_preserves_apply_evidence_and_reclaims_terminal_captures() {
    let (root, mut context, connection) = fixture();
    context.agent = Some("claude".into());
    let proposed = proposal(&context, &connection, "held-for-confirmation");
    assert!(proposed["error"].is_null(), "{proposed}");
    let old = proposed["data"]["after_candidate_id"].as_str().unwrap();
    let state = Store::open(root.path()).unwrap().read().unwrap();
    let churn = |start: usize| {
        let mut views = connection.views.lock().unwrap();
        for index in start..start + 300 {
            let mut meta = state.operator.clone();
            meta["inspection"] = json!(index);
            views
                .retain(&context, &state.candidates["project"], &meta)
                .unwrap();
        }
    };
    churn(0);
    let args = &proposed["data"]["apply_arguments"];
    let declined = invoke(&context, &connection, "policy_apply", args.clone());
    assert_eq!(declined["error"]["code"], "operator_review_required");
    let applied = application::apply(&context, &connection, args);
    assert_eq!(applied["data"]["status"], "applied", "{applied}");
    let committed = Store::open(root.path()).unwrap().read_checkpoint().unwrap();
    churn(300);
    assert!(matches!(
        connection.views.lock().unwrap().get(old),
        Err(Error::CandidateUnavailable)
    ));
    let shown = invoke(
        &context,
        &connection,
        "policy_show",
        json!({"proposal_id":proposed["data"]["proposal_id"]}),
    );
    assert!(shown["error"].is_null(), "{shown}");
    assert_eq!(shown["data"]["status"], "applied");
    assert_ne!(shown["data"]["after_candidate_id"], old);
    let replay = application::apply(&context, &connection, args);
    assert_eq!(
        replay["data"]["receipt"],
        committed
            .proposals
            .iter()
            .find(|row| row.id == proposed["data"]["proposal_id"])
            .unwrap()
            .receipt
    );
    assert_eq!(
        replay["data"]["receipt"]["activation_id"],
        applied["data"]["receipt"]["activation_id"]
    );
    assert_eq!(
        Store::open(root.path())
            .unwrap()
            .read_checkpoint()
            .unwrap()
            .sequence,
        committed.sequence
    );
}

#[test]
fn inspection_reclamation_long_rule_list_then_policy_proposal_on_same_connection() {
    let rules: Vec<_> = (0..310)
        .map(|id| rule(&format!("acme-{id:03}"), "allow"))
        .collect();
    let rules = crate::policy::rules::document(&rules).unwrap().to_string();
    let bundle = Bundle::capture(
        "schema_version: 1\npacks: []\nrules: rules.yml\n",
        &[],
        Some(&rules),
    )
    .unwrap();
    let (root, context, connection) = fixture_with(&bundle);
    let before = std::fs::read(root.path().join(".shield/state/state.json")).unwrap();
    let first = invoke(&context, &connection, "rule_list", json!({"limit":1}));
    assert!(first["error"].is_null(), "{first}");
    let mut cursor = first["data"]["next_cursor"].clone();
    let mut count = 1;
    while !cursor.is_null() {
        let args = json!({"limit":1,"cursor":cursor});
        let page = invoke(&context, &connection, "rule_list", args.clone());
        assert!(page["error"].is_null(), "page {count}: {page}");
        assert_eq!(page["data"]["rules"][0]["id"], format!("acme-{count:03}"));
        if count == 300 {
            let retry = invoke(&context, &connection, "rule_list", args);
            assert_eq!(retry["data"], page["data"]);
        }
        cursor = page["data"]["next_cursor"].clone();
        count += 1;
    }
    assert_eq!(count, 310);
    assert_eq!(
        std::fs::read(root.path().join(".shield/state/state.json")).unwrap(),
        before
    );
    let proposed = proposal(&context, &connection, "after-large-list");
    assert!(proposed["error"].is_null(), "{proposed}");
    let inspected = invoke(
        &context,
        &connection,
        "policy_show",
        json!({"proposal_id":proposed["data"]["proposal_id"]}),
    );
    assert_eq!(
        inspected["data"]["apply_arguments"],
        proposed["data"]["apply_arguments"]
    );
    assert!(invoke(&context, &connection, "status", json!({}))["error"].is_null());
}
