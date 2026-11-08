//! Historical matching cannot be injected or edited behind an ordinary review.
use super::*;

fn modern() -> Value {
    json!({"id":"remove","decision":"deny","reason":"Prevent deletion",
        "conditions":{"domain":"argv","executable":"rm",
            "arguments":{"kind":"prefix","values":["-rf"]}}})
}

fn annotated(kind: &str) -> Value {
    let row = if kind == "historical_matcher" {
        json!({"id":"remove","scope":"project","decision":"deny","reason":"Prevent deletion",
            "matcher":{"executable":"rm","arguments":{"kind":"prefix","values":["-rf"]},
                "project":"/nonexistent"}})
    } else {
        let mut row = modern();
        row["schema_version"] = json!(1);
        row["expires_at"] = Value::Null;
        row
    };
    crate::policy::normalize_retained_custom(&json!({"schema_version":1,"rules":[row]}).to_string())
        .unwrap()["rules"][0]
        .clone()
}

fn bundle(rows: &[Value], local: bool) -> Bundle {
    let rules = crate::policy::rules::document(rows).unwrap().to_string();
    let empty = crate::policy::rules::document(&[]).unwrap().to_string();
    Bundle::capture_layers(
        "schema_version: 1\npacks: []\nrules: rules.yml\n",
        local.then_some("schema_version: 1\nrules: rules.local.yml\n"),
        &[],
        Some(if local { &empty } else { &rules }),
        local.then_some(rules.as_str()),
        &[],
    )
    .unwrap()
}

fn refused(result: &Value) {
    assert_eq!(result["error"]["code"], "invalid_input", "{result}");
    assert!(
        result["error"]["message"]
            .as_str()
            .unwrap()
            .contains("historical annotations"),
        "{result}"
    );
}

fn refuse_manual_adoption(
    root: &Path,
    context: &Context,
    connection: &Connection,
    before: &review::model::State,
    next: &Value,
    local: bool,
) {
    let meta = review::project::metadata(before);
    let files = review::publication::projections::projections(&before.candidates["project"], &meta)
        .unwrap();
    review::publication::projections::materialize(context, &files).unwrap();
    std::fs::write(
        root.join(if local {
            ".shield/rules.local.yml"
        } else {
            ".shield/rules.yml"
        }),
        next[if local { "local_rules" } else { "rules" }]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    refused(&invoke(
        context,
        connection,
        "policy_propose",
        json!({"change":{"kind":"project_files_adopt"}}),
    ));
    let captured = review::changes::project_files::capture(context, before).unwrap();
    let error = review::publication::prepare_review(
        context,
        before,
        &captured.candidate,
        &captured.metadata,
        None,
    )
    .err()
    .unwrap();
    assert!(
        error.to_string().contains("historical annotations"),
        "{error}"
    );
    let after = BTreeMap::from([("project".into(), captured.candidate.clone())]);
    let error = review::compare_prepared(&before.candidates, &after, None)
        .err()
        .unwrap();
    assert!(
        error.to_string().contains("historical annotations"),
        "{error}"
    );
    let error =
        review::publication::commit(context, before, captured.candidate, captured.metadata, None)
            .unwrap_err();
    assert!(
        error.to_string().contains("historical annotations"),
        "{error}"
    );
    let fresh = assert_fs::TempDir::new().unwrap();
    let error = review::publication::initialize(
        fresh.path(),
        &BTreeMap::from([("project".into(), next.clone())]),
        &before.policy_revision,
        &meta,
        BTreeMap::new(),
    )
    .unwrap_err();
    assert!(
        error.to_string().contains("historical annotations"),
        "{error}"
    );
    assert!(!fresh.path().join(".shield/review/state.json").exists());
}

#[test]
fn historical_annotations_cannot_be_added_by_rule_tools_or_manual_adoption() {
    for kind in ["historical_matcher", "historical_normalized"] {
        for local in [false, true] {
            let (root, context, connection) = fixture_with(&bundle(&[modern()], local));
            let before = Store::open(root.path()).unwrap().read().unwrap();
            let forged = annotated(kind);
            assert_eq!(forged["conditions"], modern()["conditions"]);
            let next = json!(bundle(std::slice::from_ref(&forged), local));
            if kind == "historical_matcher" {
                let request = crate::policy::Request {
                    executable: "rm".into(),
                    arguments: vec!["-rf".into(), "acme".into()],
                    cwd: context.project_root.clone(),
                    project: context.project_root.clone(),
                };
                for (candidate, expected) in [
                    (
                        json!(bundle(&[modern()], local)),
                        crate::policy::Decision::Deny,
                    ),
                    (next.clone(), crate::policy::Decision::Ask),
                ] {
                    let candidate = json!(candidate);
                    let rules = crate::policy::CustomRules::from_yaml(
                        candidate[if local { "local_rules" } else { "rules" }]
                            .as_str()
                            .unwrap(),
                    )
                    .unwrap();
                    assert_eq!(
                        crate::policy::evaluate(
                            &[],
                            Some(&rules),
                            crate::policy::Profile::Recommended,
                            &request
                        )
                        .unwrap()
                        .decision,
                        expected
                    );
                }
            }
            let layer = if local { "local" } else { "project" };
            refused(&invoke(
                &context,
                &connection,
                "rule_edit",
                json!({"rule_id":"remove","rule":forged,"layer":layer}),
            ));
            let (empty_root, empty_context, empty_connection) = fixture();
            refused(&invoke(
                &empty_context,
                &empty_connection,
                "rule_add",
                json!({"rule":forged,"layer":layer}),
            ));
            assert!(
                Store::open(empty_root.path())
                    .unwrap()
                    .read()
                    .unwrap()
                    .proposals
                    .is_empty()
            );
            refuse_manual_adoption(root.path(), &context, &connection, &before, &next, local);
            let after = Store::open(root.path()).unwrap().read().unwrap();
            assert_eq!(after.policy_revision, before.policy_revision);
            assert_eq!(json!(after.active), json!(before.active));
            assert!(after.proposals.is_empty());
        }
    }
}

#[test]
fn historical_annotations_require_the_entire_same_rule_and_same_layer() {
    for kind in ["historical_matcher", "historical_normalized"] {
        for local in [false, true] {
            let original = annotated(kind);
            let (root, context, connection) =
                fixture_with(&bundle(std::slice::from_ref(&original), local));
            let mut changed = original.clone();
            if kind == "historical_matcher" {
                changed[kind]["matcher"]["project"] = json!("/another-project");
            } else {
                changed["decision"] = json!("allow");
                changed[kind]["decision"] = json!("allow");
            }
            // Both modified annotations remain internally valid policy syntax.
            bundle(std::slice::from_ref(&changed), local);
            refused(&invoke(
                &context,
                &connection,
                "rule_edit",
                json!({
                "rule_id":"remove","rule":changed,"layer":if local {"local"} else {"project"}}),
            ));
            refused(&invoke(
                &context,
                &connection,
                "rule_add",
                json!({
                "rule":original,"layer":if local {"project"} else {"local"}}),
            ));
            assert!(
                Store::open(root.path())
                    .unwrap()
                    .read()
                    .unwrap()
                    .proposals
                    .is_empty()
            );
        }
    }
}

#[test]
fn historical_annotations_preserve_unchanged_rules_and_allow_current_rewrites() {
    for kind in ["historical_matcher", "historical_normalized"] {
        for local in [false, true] {
            let original = annotated(kind);
            let (root, mut context, connection) =
                fixture_with(&bundle(std::slice::from_ref(&original), local));
            context.agent = Some("codex".into());
            let layer = if local { "local" } else { "project" };
            let added = invoke(
                &context,
                &connection,
                "rule_add",
                json!({"rule":rule("unrelated","allow"),"layer":layer}),
            );
            assert!(added["error"].is_null(), "{added}");
            let applied = invoke(
                &context,
                &connection,
                "policy_apply",
                added["data"]["apply_arguments"].clone(),
            );
            assert!(applied["error"].is_null(), "{applied}");
            let state = Store::open(root.path()).unwrap().read().unwrap();
            let source = state.candidates["project"][if local { "local_rules" } else { "rules" }]
                .as_str()
                .unwrap();
            let retained = crate::policy::normalize_retained_custom(source).unwrap();
            assert_eq!(retained["rules"][0], original);
            let edited = invoke(
                &context,
                &connection,
                "rule_edit",
                json!({"rule_id":"remove","rule":modern(),"layer":layer}),
            );
            assert!(edited["error"].is_null(), "{edited}");
            let applied = invoke(
                &context,
                &connection,
                "policy_apply",
                edited["data"]["apply_arguments"].clone(),
            );
            assert!(applied["error"].is_null(), "{applied}");
        }
    }
}
