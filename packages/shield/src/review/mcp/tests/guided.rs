//! Joint removals retain dependency safety and exact captured metadata offline.
use super::*;

#[test]
fn guided_removal_bounds_complete_choices_and_selected_sets_before_preparation() {
    let (root, context, connection) = fixture();
    let source = dependency_registry(&root, &context, &json!([]), false);
    for index in 0..64 {
        registry_release(&source, &format!("acme/choice-{index:02}"), "1.0.0", None);
    }
    // Keep the catalog at its real 64-entry bound, omitting its two templates.
    let path = source.path().join("catalog.json");
    let mut catalog: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    catalog["packs"]
        .as_array_mut()
        .unwrap()
        .retain(|entry| entry["id"].as_str().unwrap().starts_with("acme/choice-"));
    std::fs::write(path, catalog.to_string()).unwrap();
    for batch in 0..4 {
        let changes: Vec<_> = (batch*16..(batch+1)*16).map(|index| json!({"kind":"pack_add","reference":format!("acme-dependencies/acme/choice-{index:02}")})).collect();
        let result = invoke(
            &context,
            &connection,
            "policy_propose",
            json!({"changes":changes,"idempotency_key":format!("batch-{batch}")}),
        );
        assert!(result["error"].is_null(), "{result}");
        review::operator::commit_proposal_fixture(
            &context,
            result["data"]["proposal_id"].as_str().unwrap(),
        )
        .unwrap();
    }
    let before = Store::open(root.path()).unwrap().read_checkpoint().unwrap();
    let selectors: Vec<_> = before.operator["installed"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["selector"].clone())
        .collect();
    assert_eq!(selectors.len(), 64);
    let question = Question::prepare(&context, "pack_remove", &json!({})).unwrap();
    let invalid = question.propose(
        &context,
        &connection,
        &json!({"references":&selectors[..17]}),
        "too-many",
        &|| Ok(()),
    );
    assert_eq!(invalid["error"]["code"], "invalid_input");
    let duplicate = question.propose(
        &context,
        &connection,
        &json!({"references":[selectors[0],selectors[0]]}),
        "duplicate",
        &|| Ok(()),
    );
    assert_eq!(duplicate["error"]["code"], "invalid_input");
    assert_eq!(
        Store::open(root.path())
            .unwrap()
            .read_checkpoint()
            .unwrap()
            .proposals
            .len(),
        before.proposals.len()
    );
    let valid = question.propose(
        &context,
        &connection,
        &json!({"references":&selectors[..16]}),
        "at-limit",
        &|| Ok(()),
    );
    assert!(valid["error"].is_null(), "{valid}");
    let arguments = &valid["data"]["apply_arguments"];
    assert_eq!(arguments["changes"].as_array().unwrap().len(), 16);
    let shown = invoke(
        &context,
        &connection,
        "policy_show",
        json!({"proposal_id":valid["data"]["proposal_id"]}),
    );
    assert_eq!(shown["data"]["apply_arguments"], *arguments);
    assert_eq!(
        Store::open(root.path())
            .unwrap()
            .read_checkpoint()
            .unwrap()
            .policy_revision,
        before.policy_revision
    );
    let direct = pack_change(
        &context,
        &connection,
        json!({"kind":"pack_remove","reference":"acme/choice-63"}),
        "direct",
    );
    assert!(direct["error"].is_null(), "{direct}");
}

#[test]
fn guided_removal_checks_the_whole_selected_closure_without_order_dependence() {
    let (root, context, connection) = fixture();
    let source = dependency_registry(
        &root,
        &context,
        &json!([{"pack":"acme/child","requirement":"^1"}]),
        false,
    );
    registry_release(&source, "acme/other", "1.0.0", Some("^1"));
    for reference in ["parent", "other", "child"] {
        commit_pack_change(
            &context,
            &connection,
            json!({"kind":"pack_add","reference":format!("acme-dependencies/acme/{reference}")}),
            reference,
        );
    }
    drop(source);
    let before = Store::open(root.path()).unwrap().read().unwrap();
    let selectors: Vec<_> = before.operator["installed"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["selector"].clone())
        .collect();
    let child = before.operator["installed"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == "acme/child")
        .unwrap()["selector"]
        .clone();
    let question = Question::prepare(&context, "pack_remove", &json!({})).unwrap();
    let blocked = question.propose(
        &context,
        &connection,
        &json!({"references":[child]}),
        "acme-child",
        &|| Ok(()),
    );
    assert_eq!(blocked["error"]["code"], "dependency_required", "{blocked}");
    let mut reversed = selectors.clone();
    reversed.reverse();
    let proposed = question.propose(
        &context,
        &connection,
        &json!({"references":reversed}),
        "acme-joint",
        &|| Ok(()),
    );
    assert!(proposed["error"].is_null(), "{proposed}");
    assert!(proposed["data"]["apply_arguments"].is_object());
    let after = Store::open(root.path()).unwrap().read().unwrap();
    assert_eq!(after.policy_revision, before.policy_revision);
    assert_eq!(after.operator, before.operator);
    review::operator::commit_proposal_fixture(
        &context,
        proposed["data"]["proposal_id"].as_str().unwrap(),
    )
    .unwrap();
    assert_eq!(
        Store::open(root.path())
            .unwrap()
            .read_checkpoint()
            .unwrap()
            .operator["installed"],
        json!([])
    );
}

#[test]
fn guided_input_rejects_changed_operator_metadata_without_a_revision_change() {
    let (root, context, connection) = fixture();
    let question = Question::prepare(
        &context,
        "registry_add",
        &json!({"name":"acme","scope":"project"}),
    )
    .unwrap();
    let before = Store::open(root.path()).unwrap().read_checkpoint().unwrap();
    let _source = dependency_registry(&root, &context, &json!([]), false);
    let result = question.propose(
        &context,
        &connection,
        &json!({"location":"not-read"}),
        "acme-stale",
        &|| Ok(()),
    );
    assert_eq!(result["error"]["code"], "stale_revision", "{result}");
    assert_eq!(result["error"]["operator_route"], "registry_add");
    let after = Store::open(root.path()).unwrap().read_checkpoint().unwrap();
    assert_eq!(after.policy_revision, before.policy_revision);
    assert_eq!(after.proposals.len(), before.proposals.len());
}
