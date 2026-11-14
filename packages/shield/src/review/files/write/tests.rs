use super::*;
use crate::review::{changes, project, storage::Store};
use assert_fs::prelude::*;

fn rule_in(id: &str, layer: Option<changes::Layer>) -> changes::Change {
    changes::Change::RuleAdd {
        rule: serde_json::from_value(json!({"id":id,"decision":"allow","reason":"Acme read",
            "conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"exact","values":["read"]}}}))
        .unwrap(),
        layer,
    }
}

fn rule(id: &str) -> changes::Change {
    rule_in(id, None)
}

/// An edit prepared against the files as they are, with its base.
fn prepare(context: &Context, change: &changes::Change) -> (State, Value, Value, Base) {
    let (before, _) = project::load_prepared(context).unwrap();
    let (candidate, meta, _) = changes::custom_rule(context, &before, change, None).unwrap();
    let base = base(context, &before, &candidate, &meta).unwrap();
    (before, candidate, meta, base)
}

fn edit(before: &State, candidate: Value, meta: Value, base: Base) -> Edit<'_> {
    Edit {
        before,
        candidate,
        meta,
        base,
        evaluator: None,
        reviewed: None,
    }
}

#[test]
fn a_written_edit_is_the_policy_of_the_next_request() {
    let (root, context) = crate::review::tests::enrolled();
    let (before, candidate, meta, base) = prepare(&context, &rule("acme-read"));
    let receipt = write(
        &context,
        edit(&before, candidate, meta, base),
        None,
        "operator-cli",
    )
    .unwrap();
    assert_eq!(receipt["committed"], true);
    let rules = std::fs::read_to_string(root.path().join(".shield/rules.yml")).unwrap();
    assert!(rules.contains("acme-read"), "{rules}");
    // Requests decide by the files; review state follows the same files.
    let policy = super::super::load(root.path()).unwrap();
    assert_eq!(receipt["policy_revision"], policy.evaluator.revision());
    let store = Store::open(root.path()).unwrap();
    // The checkpoint holds no copy of the policy; a reader follows the files.
    assert!(store.read_checkpoint().unwrap().policy_revision.is_empty());
    let state = store.read_following_files().unwrap();
    assert_eq!(state.policy_revision, policy.evaluator.revision());
    assert_eq!(
        state.followed_files,
        Some(super::super::digest(root.path()).unwrap())
    );
}

#[test]
fn an_edit_to_the_files_during_review_is_refused_and_kept() {
    let (root, context) = crate::review::tests::enrolled();
    let (before, candidate, meta, base) = prepare(&context, &rule("acme-read"));
    // A person edits the configuration while the review is open.
    let config = root.child(".shield/config.yml");
    let edited = format!(
        "# Acme edit\n{}",
        std::fs::read_to_string(config.path()).unwrap()
    );
    config.write_str(&edited).unwrap();
    let result = write(
        &context,
        edit(&before, candidate, meta, base),
        None,
        "operator-cli",
    );
    assert!(matches!(result, Err(Error::StaleSnapshot)), "{result:?}");
    assert_eq!(std::fs::read_to_string(config.path()).unwrap(), edited);
}

#[test]
fn a_local_rule_edit_reaches_the_local_files() {
    let (root, context) = crate::review::tests::enrolled();
    let local_rule = rule_in("acme-local", Some(changes::Layer::Local));
    let (before, candidate, meta, base) = prepare(&context, &local_rule);
    write(
        &context,
        edit(&before, candidate, meta, base),
        None,
        "operator-cli",
    )
    .unwrap();
    let remove = changes::Change::RuleRemove {
        rule_id: "acme-local".into(),
        layer: Some(changes::Layer::Local),
    };
    let (before, candidate, meta, base) = prepare(&context, &remove);
    write(
        &context,
        edit(&before, candidate, meta, base),
        None,
        "operator-cli",
    )
    .unwrap();
    let policy = super::super::load(root.path()).unwrap();
    let state = Store::open(root.path())
        .unwrap()
        .read_following_files()
        .unwrap();
    assert_eq!(state.policy_revision, policy.evaluator.revision());
    let local =
        std::fs::read_to_string(root.path().join(".shield/rules.local.yml")).unwrap_or_default();
    assert!(!local.contains("acme-local"), "{local}");
}

#[test]
fn a_hand_edit_that_keeps_the_policy_survives_the_next_write() {
    let (root, context) = crate::review::tests::enrolled();
    let config = root.child(".shield/config.yml");
    let commented = format!(
        "# Acme team policy\n{}",
        std::fs::read_to_string(config.path()).unwrap()
    );
    config.write_str(&commented).unwrap();
    let (before, candidate, meta, base) = prepare(&context, &rule("acme-read"));
    write(
        &context,
        edit(&before, candidate, meta, base),
        None,
        "operator-cli",
    )
    .unwrap();
    let after = std::fs::read_to_string(config.path()).unwrap();
    assert!(after.starts_with("# Acme team policy\n"), "{after}");
}

#[test]
fn local_settings_survive_edits_before_and_after_them() {
    let (root, context) = crate::review::tests::enrolled();
    // A shared edit while the local file does not exist yet.
    let (before, candidate, meta, base) = prepare(&context, &rule("acme-read"));
    write(
        &context,
        edit(&before, candidate, meta, base),
        None,
        "operator-cli",
    )
    .unwrap();
    let local = root.child(".shield/config.local.yml");
    local
        .write_str("schema_version: 1\nenforcement: paused\n")
        .unwrap();
    // A shared edit, then a local one, both keep the local settings.
    let (before, candidate, meta, base) = prepare(&context, &rule("acme-write"));
    write(
        &context,
        edit(&before, candidate, meta, base),
        None,
        "operator-cli",
    )
    .unwrap();
    let (before, candidate, meta, base) = prepare(
        &context,
        &rule_in("acme-local", Some(changes::Layer::Local)),
    );
    write(
        &context,
        edit(&before, candidate, meta, base),
        None,
        "operator-cli",
    )
    .unwrap();
    let text = std::fs::read_to_string(local.path()).unwrap();
    assert!(text.contains("enforcement: paused"), "{text}");
    assert!(!crate::review::mode::current(root.path()).unwrap());
}

fn proposed(
    context: &Context,
    key: &str,
) -> (String, BTreeMap<String, crate::review::model::Texts>) {
    let proposal =
        crate::review::tests::propose(context, &crate::review::tests::input(key), &|| Ok(()))
            .unwrap();
    let id = proposal["proposal_id"].as_str().unwrap().to_owned();
    let state = Store::open(Path::new(&context.project_root))
        .unwrap()
        .read_checkpoint()
        .unwrap();
    let row = state.proposals.iter().find(|row| row.id == id).unwrap();
    (id, row.files.clone())
}

#[test]
fn applying_a_proposal_writes_exactly_the_reviewed_texts() {
    let (root, context) = crate::review::tests::enrolled();
    let (id, files) = proposed(&context, "acme-read");
    // The proposal stores texts, not a retained copy of the policy.
    let raw: Value = serde_json::from_slice(
        &std::fs::read(root.path().join(".shield/state/state.json")).unwrap(),
    )
    .unwrap();
    let stored = raw["proposals"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["proposal_id"] == id.as_str())
        .unwrap();
    assert!(stored.get("candidate").is_none() && stored.get("base_candidate").is_none());
    assert!(files.values().any(|texts| texts.before != texts.after));
    crate::review::operator::commit_proposal_fixture(&context, &id).unwrap();
    for (name, texts) in &files {
        let written = std::fs::read_to_string(root.path().join(".shield").join(name)).ok();
        assert_eq!(written, texts.after, "{name}");
    }
}

#[test]
fn a_file_changed_after_review_refuses_the_proposal() {
    let (root, context) = crate::review::tests::enrolled();
    let (id, _) = proposed(&context, "acme-read");
    let config = root.child(".shield/config.yml");
    let edited = format!(
        "# Acme edit\n{}",
        std::fs::read_to_string(config.path()).unwrap()
    );
    config.write_str(&edited).unwrap();
    assert!(crate::review::operator::commit_proposal_fixture(&context, &id).is_err());
    assert_eq!(std::fs::read_to_string(config.path()).unwrap(), edited);
}

#[test]
fn a_proposal_an_earlier_release_stored_closes_as_stale() {
    let (root, context) = crate::review::tests::enrolled();
    let (id, _) = proposed(&context, "acme-read");
    let path = root.path().join(".shield/state/state.json");
    let mut raw: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    for row in raw["proposals"].as_array_mut().unwrap() {
        let fields = row.as_object_mut().unwrap();
        fields.remove("files");
        fields.insert(
            "candidate".into(),
            json!({"kind":"retained-source-v2","digest":"0".repeat(64)}),
        );
        fields.insert(
            "base_candidate".into(),
            json!({"kind":"retained-source-v2","digest":"1".repeat(64)}),
        );
    }
    std::fs::write(&path, raw.to_string()).unwrap();
    let state = Store::open(root.path()).unwrap().read_checkpoint().unwrap();
    let row = state.proposals.iter().find(|row| row.id == id).unwrap();
    assert_eq!(row.status, "stale");
    assert!(crate::review::operator::commit_proposal_fixture(&context, &id).is_err());
}

#[test]
fn proposal_texts_refuse_a_pack_neither_they_nor_the_store_hold() {
    let (root, _context) = crate::review::tests::enrolled();
    let shield = root.path().join(".shield");
    let texts = BTreeMap::from([(
        "config.yml".to_owned(),
        crate::review::model::Texts {
            before: Some("schema_version: 1\npacks: []\n".into()),
            after: Some("schema_version: 1\npacks: []\n".into()),
        },
    )]);
    assert!(bundle(root.path(), &texts, true).is_ok());
    // The per-user store case is covered by the MCP pack proposal test.
    let missing = format!("cache/{}.yml", "2".repeat(64));
    assert!(
        crate::review::integrations::project_sources::from_texts(&shield, &|name| {
            (name == "config.yml").then(|| {
                format!(
                    "schema_version: 1\npacks:\n- id: acme/missing\n  version: \"1.0.0\"\n  profile: strict\n  path: {missing}\n"
                )
            })
        })
        .is_err()
    );
}

#[test]
fn a_write_ends_the_requests_asked_of_the_earlier_policy() {
    let (root, mut context) = crate::review::tests::enrolled();
    context.session_id = Some("acme-session".into());
    Store::open(root.path())
        .unwrap()
        .update(&context, &|| Ok(()), |state| {
            let mut event = state.event(
                "decision",
                &context,
                json!({}),
                json!({}),
                json!({"decision":"ask","operation":{"argv":["acme","read"]}}),
            );
            event["provenance"]["host_event_name"] = json!("PreToolUse");
            event["provenance"]["session_id"] = json!("acme-session");
            state.record(event);
            Ok(json!({}))
        })
        .unwrap();
    let asked = crate::review::observations::asked_requests(root.path(), None, 16);
    assert_eq!(asked.requests.len(), 1);
    let (before, candidate, meta, base) = prepare(&context, &rule("acme-read"));
    write(
        &context,
        edit(&before, candidate, meta, base),
        None,
        "operator-cli",
    )
    .unwrap();
    let asked = crate::review::observations::asked_requests(root.path(), None, 16);
    assert!(asked.requests.is_empty());
}

#[test]
fn files_an_edit_no_longer_produces_are_removed() {
    let (root, context) = crate::review::tests::enrolled();
    let local = root.child(".shield/config.local.yml");
    local.write_str("schema_version: 1\n").unwrap();
    let base = Base::from([(
        "config.local.yml".to_owned(),
        Some(crate::policy::fingerprint(b"schema_version: 1\n")),
    )]);
    publish_files(&context, &BTreeMap::new(), &base).unwrap();
    // A stale local layer would otherwise keep applying.
    assert!(!local.path().exists());
}
