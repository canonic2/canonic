//! Owned project fixtures exercise persistence, authority transitions and correlation.
use super::*;
use assert_fs::prelude::*;

pub(super) fn retry_locked<T>(mut operation: impl FnMut() -> Result<T, Error>) -> T {
    let started = std::time::Instant::now();
    loop {
        match operation() {
            Err(Error::Locked) if started.elapsed() < std::time::Duration::from_secs(5) => {
                // Only acquisition contention is retryable; other failures may follow effects.
                std::thread::sleep(std::time::Duration::from_millis(25));
            }
            result => return result.expect("concurrent review operation"),
        }
    }
}

pub(super) fn enrolled() -> (assert_fs::TempDir, Context) {
    let root = assert_fs::TempDir::new().unwrap();
    let bundle =
        configuration::Bundle::capture("schema_version: 1\npacks: []\n", &[], None).unwrap();
    initialize(
        root.path(),
        BTreeMap::from([("project".into(), json!(bundle))]),
    )
    .unwrap();
    let mut context = open(root.path()).unwrap();
    context.agent = Some("acme-agent".into());
    context.session_id = Some("acme-session".into());
    (root, context)
}
pub(super) fn input(key: &str) -> changes::Input {
    serde_json::from_value(json!({"change":{"kind":"rule_add","rule":{"id":key,"decision":"allow","reason":"Acme read","conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"exact","values":["read"]}}}},"agent_reason":"Read Acme data","idempotency_key":key})).unwrap()
}
pub(super) fn propose(
    context: &Context,
    input: &changes::Input,
    checkpoint: Checkpoint<'_>,
) -> Result<Value, Error> {
    changes::propose(
        context,
        input,
        checkpoint,
        &mut |_, _, _, _, _| Ok(()),
        &|row| row.public(false),
    )
}
pub(super) fn receipt(context: &Context, id: &str) -> Value {
    mcp::call(
        context,
        &mcp::Connection::new("receipt-test".into()),
        "policy_receipt",
        &json!({"proposal_id":id}),
        &|| Ok(()),
    )
}
pub(super) fn evaluate(
    context: &Context,
    request: &policy::Request,
    reason: Option<&str>,
    checkpoint: Checkpoint<'_>,
) -> Result<Value, Error> {
    checkpoint()?;
    request.validate()?;
    let state = Store::open(Path::new(&context.project_root))?.read()?;
    state.check(context)?;
    let evaluation = configuration::Evaluator::new(snapshots(&state.candidates)?)?
        .evaluate_controlled(request, checkpoint)?;
    let mut origin = context.clone();
    origin.cwd = Some(request.cwd.clone());
    let observed = observations::decision(
        &origin,
        json!({"request":request,"policy_revision":state.policy_revision,"decision":evaluation.decision,"evaluation":evaluation,"agent_reason":reason,"execution":"unobserved"}),
    )?;
    Ok(
        json!({"request_id":observed["request_id"],"decision_event_id":observed["event_id"],"evaluation":evaluation}),
    )
}
pub(super) fn history(context: &Context, id: Option<&str>) -> Result<Value, Error> {
    let state = Store::open(Path::new(&context.project_root))?.read()?;
    state.check(context)?;
    if let Some(id) = id {
        return state
            .events
            .iter()
            .find(|e| e["event_id"] == id)
            .cloned()
            .ok_or(Error::Missing);
    }
    Ok(
        json!({"schema_version":1,"project_id":state.project_id,"enabled":state.history_enabled,"limit":state.history_limit,"events":state.events}),
    )
}

#[test]
fn atomic_proposal_commit_and_receipts_preserve_origin_when_history_is_purged() {
    let (root, context) = enrolled();
    let proposal = propose(&context, &input("one"), &|| Ok(())).unwrap();
    let id = proposal["proposal_id"].as_str().unwrap();
    assert_eq!(proposal["provenance"]["session_id"], "acme-session");
    assert_eq!(
        propose(&context, &input("one"), &|| Ok(())).unwrap()["proposal_id"],
        id
    );
    assert!(receipt(&context, id)["error"].is_object());
    let mut mcp = context.clone();
    mcp.surface = "mcp".into();
    assert!(matches!(
        transition(&mcp, id, "approve", None, &|| Ok(())),
        Err(Error::Invalid("invalid proposal transition"))
    ));
    operator::commit_proposal_fixture(&context, id).unwrap();
    history_settings(&context, Some(false), None, true, &|| Ok(())).unwrap();
    let applied = receipt(&mcp, id)["data"].clone();
    assert_eq!(applied["status"], "applied");
    assert_eq!(applied["provenance"], proposal["provenance"]);
    assert_eq!(receipt(&mcp, id)["data"].clone(), applied);
    let state = Store::open(root.path()).unwrap().read().unwrap();
    assert_eq!(state.authorization_events.len(), 1);
    assert_eq!(state.events.len(), 0);
    let request = policy::Request {
        executable: "acme".into(),
        arguments: vec!["read".into()],
        cwd: context.project_root.clone(),
        project: context.project_root.clone(),
    };
    assert_eq!(
        evaluate(&context, &request, None, &|| Ok(())).unwrap()["evaluation"]["decision"],
        "allow"
    );
}

#[test]
fn a_prepared_proposal_refuses_a_concurrently_activated_base() {
    let (root, context) = enrolled();
    let other = propose(&context, &input("other"), &|| Ok(())).unwrap();
    let result = changes::propose(
        &context,
        &input("prepared"),
        &|| Ok(()),
        &mut |_, _, _, _, _| {
            operator::commit_proposal_fixture(&context, other["proposal_id"].as_str().unwrap())?;
            Ok(())
        },
        &|row| row.public(false),
    );
    assert!(matches!(result, Err(Error::StaleSnapshot)));
    let state = Store::open(root.path()).unwrap().read_checkpoint().unwrap();
    assert_eq!(state.proposals.len(), 1);
    assert_eq!(state.proposals[0].status, "applied");
}

#[test]
fn stale_rejected_changed_retries_and_cross_project_context_do_not_apply() {
    let (_root, context) = enrolled();
    let first = propose(&context, &input("one"), &|| Ok(())).unwrap();
    let second = propose(&context, &input("two"), &|| Ok(())).unwrap();
    let mut changed = input("one");
    changed.agent_reason = Some("different".into());
    assert!(propose(&context, &changed, &|| Ok(())).is_err());
    let id = first["proposal_id"].as_str().unwrap();
    operator::commit_proposal_fixture(&context, id).unwrap();
    assert_eq!(
        transition(
            &context,
            second["proposal_id"].as_str().unwrap(),
            "withdraw",
            None,
            &|| Ok(())
        )
        .unwrap()["error"]["code"],
        "stale-proposal"
    );
    let mut other = context.clone();
    other.project_id = "a".repeat(64);
    assert!(history(&other, None).is_err());
    let third = propose(&context, &input("three"), &|| Ok(())).unwrap();
    let id = third["proposal_id"].as_str().unwrap();
    transition(&context, id, "reject", None, &|| Ok(())).unwrap();
    assert!(receipt(&context, id)["error"].is_object());
}

#[test]
fn cancelled_writes_corruption_locks_and_snapshot_retention_are_visible() {
    let (root, context) = enrolled();
    assert!(
        propose(&context, &input("cancelled"), &|| Err(
            policy::Error::Interrupted
        ))
        .is_err()
    );
    assert_eq!(
        proposals(&context, None, false).unwrap()["proposals"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
    root.child(".shield/review/state.lock")
        .write_str("owned")
        .unwrap();
    assert!(matches!(
        propose(&context, &input("locked"), &|| Ok(())),
        Err(Error::Locked)
    ));
    std::fs::remove_file(root.path().join(".shield/review/state.lock")).unwrap();
    history_settings(&context, None, Some(1), false, &|| Ok(())).unwrap();
    propose(&context, &input("one"), &|| Ok(())).unwrap();
    propose(&context, &input("two"), &|| Ok(())).unwrap();
    assert_eq!(
        history(&context, None).unwrap()["events"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
    assert_eq!(
        proposals(&context, None, false).unwrap()["proposals"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    root.child(".shield/review/state.json")
        .write_str("broken")
        .unwrap();
    assert!(open(root.path()).is_err());
    assert_eq!(
        std::fs::read_to_string(root.path().join(".shield/review/state.json")).unwrap(),
        "broken"
    );
}

#[test]
fn concurrent_writers_keep_distinct_proposals() {
    let (_root, context) = enrolled();
    let handles: Vec<_> = (0..4)
        .map(|index| {
            let context = context.clone();
            std::thread::spawn(move || {
                retry_locked(|| propose(&context, &input(&format!("writer-{index}")), &|| Ok(())))
            })
        })
        .collect();
    let ids: std::collections::BTreeSet<_> = handles
        .into_iter()
        .map(|h| {
            h.join().unwrap()["proposal_id"]
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect();
    assert_eq!(ids.len(), 4);
    assert_eq!(
        proposals(&context, None, false).unwrap()["proposals"]
            .as_array()
            .unwrap()
            .len(),
        4
    );
}

#[test]
fn accepting_an_older_proposal_keeps_the_current_protected_file_access() {
    use crate::review::path_access::{self, Change, PathAccess};
    let (root, context) = enrolled();
    let rule = propose(&context, &input("one"), &|| Ok(())).unwrap();
    let relaxed = PathAccess {
        shield_config_change: Change::Allow,
        ..PathAccess::default()
    };
    path_access::configure_fixture(root.path(), relaxed);
    operator::commit_proposal_fixture(&context, rule["proposal_id"].as_str().unwrap()).unwrap();
    let state = Store::open(root.path()).unwrap().read().unwrap();
    assert_eq!(path_access::load(&state.operator).unwrap(), relaxed);
    // A later tightening also survives a proposal captured while access was open.
    let rule = propose(&context, &input("two"), &|| Ok(())).unwrap();
    path_access::configure_fixture(root.path(), PathAccess::default());
    operator::commit_proposal_fixture(&context, rule["proposal_id"].as_str().unwrap()).unwrap();
    let state = Store::open(root.path()).unwrap().read().unwrap();
    assert_eq!(
        path_access::load(&state.operator).unwrap(),
        PathAccess::default()
    );
}

#[test]
fn agents_propose_extra_folders_that_apply_only_after_operator_commit() {
    let (root, context) = enrolled();
    let rule = propose(&context, &input("one"), &|| Ok(())).unwrap();
    operator::commit_proposal_fixture(&context, rule["proposal_id"].as_str().unwrap()).unwrap();
    let scratch = assert_fs::TempDir::new().unwrap();
    let folder = scratch.path().to_str().unwrap().to_owned();
    let change = |kind: &str, key: &str| -> changes::Input {
        serde_json::from_value(
            json!({"change":{"kind":kind,"access":"write","path":folder},"idempotency_key":key}),
        )
        .unwrap()
    };
    let active = || {
        let state = Store::open(root.path()).unwrap().read().unwrap();
        let config: Value =
            serde_saphyr::from_str(state.candidates["project"]["config"].as_str().unwrap())
                .unwrap();
        (
            config["access_paths"]["write"].clone(),
            state.candidates["project"]["rules"].clone(),
        )
    };
    let (_, rules) = active();
    let added = propose(&context, &change("access_path_add", "add"), &|| Ok(())).unwrap();
    assert_eq!(added["operation"], "access_path_add");
    assert!(active().0.is_null(), "a proposal alone changes nothing");
    operator::commit_proposal_fixture(&context, added["proposal_id"].as_str().unwrap()).unwrap();
    assert_eq!(active(), (json!([folder]), rules.clone()));
    assert!(propose(&context, &change("access_path_add", "again"), &|| Ok(())).is_err());
    let relative: changes::Input = serde_json::from_value(
        json!({"change":{"kind":"access_path_add","access":"read","path":"acme"},"idempotency_key":"relative"}),
    )
    .unwrap();
    assert!(propose(&context, &relative, &|| Ok(())).is_err());
    let removed = propose(
        &context,
        &change("access_path_remove", "remove"),
        &|| Ok(()),
    )
    .unwrap();
    operator::commit_proposal_fixture(&context, removed["proposal_id"].as_str().unwrap()).unwrap();
    assert_eq!(active(), (json!([]), rules));
}

#[test]
fn formatting_changes_name_only_files_that_lose_formatting() {
    let config = "schema_version: 1\npacks: []\nrules: rules.yml\n";
    let rules = "# Acme rules.\nschema_version: 1\nkind: custom\ncapabilities: [argument-rules-v1, structured-rules-v1]\nrules:\n- id: read\n  decision: allow\n  reason: Acme read\n  conditions:\n    domain: argv\n    executable: acme\n    arguments:\n      kind: exact\n      values:\n      - read # only reads\n- id: list\n  decision: ask\n  reason: Acme list\n  conditions:\n    domain: argv\n    executable: acme-list\n    arguments:\n      kind: any\n";
    let before = json!({"config":config,"rules":rules});
    let edited = |change: &dyn Fn(&mut Value)| {
        let mut target: Value = policy::parse(rules).unwrap();
        change(&mut target);
        json!({"config":config,"rules":crate::policy_files::write(Some(rules), &target).unwrap().text})
    };
    let kept = edited(&|target| target["rules"][0]["decision"] = json!("ask"));
    assert_eq!(formatting_changes(&before, &kept), None);
    let dropped =
        edited(&|target| target["rules"][0]["conditions"]["arguments"]["values"] = json!(["list"]));
    assert_eq!(
        formatting_changes(&before, &dropped),
        Some(json!([
            "rules.yml: 1 comment in the changed entries will not be kept."
        ]))
    );
    let reordered = edited(&|target| target["rules"].as_array_mut().unwrap().reverse());
    assert_eq!(
        formatting_changes(&before, &reordered),
        Some(json!([
            "rules.yml will be rewritten in Shield's layout; its 1 comment will not be kept."
        ]))
    );
    // Bytes Shield did not write, such as adopted manual edits, are not named.
    let manual = json!({"config":config,"rules":"schema_version: 1\nkind: custom\ncapabilities: [structured-rules-v1]\nrules: []\n"});
    assert_eq!(formatting_changes(&before, &manual), None);
    // A JSON source was already projected in Shield's layout.
    let json =
        json!({"config":config,"rules":json!(policy::parse::<Value>(rules).unwrap()).to_string()});
    assert_eq!(formatting_changes(&json, &dropped), None);
}
