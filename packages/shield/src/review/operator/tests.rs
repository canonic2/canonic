//! Retention and publication checks beyond ordinary live-capacity thresholds.
use crate::{
    configuration::Bundle,
    review::{self, storage::Store},
};
use serde_json::json;
use std::collections::BTreeMap;

#[test]
fn pack_rebuild_preserves_operator_access_paths() {
    let paths = json!({"read":["/acme/reference"],"write":["/acme/scratch"]});
    let candidate = json!(
        Bundle::capture(
            &json!({"schema_version":1,"packs":[],"access_paths":paths}).to_string(),
            &[],
            None
        )
        .unwrap()
    );
    let next = crate::review::packs::rebuild(&candidate, &[], &[], None).unwrap();
    let config = crate::configuration::layers::merged(&next).unwrap();
    assert_eq!(config["access_paths"], paths);
}

fn history_fixture(limit: usize) -> (assert_fs::TempDir, review::Context) {
    let root = assert_fs::TempDir::new().expect("fixture");
    let candidate =
        Bundle::capture(r#"{"schema_version":1,"packs":[]}"#, &[], None).expect("candidate");
    review::initialize(
        root.path(),
        BTreeMap::from([("project".into(), json!(candidate))]),
    )
    .expect("initialize");
    let context = review::open(root.path()).expect("context");
    Store::open(root.path())
        .expect("store")
        .update(&context, &|| Ok(()), |state| {
            state.operator = crate::review::project::metadata(state);
            state.history_limit = limit;
            Ok(json!({}))
        })
        .expect("history settings");
    (root, context)
}

#[test]
fn late_updates_do_not_evict_newer_commands_or_resurrect_pruned_entries() {
    let (root, context) = history_fixture(2);
    Store::open(root.path())
        .expect("store")
        .update(&context, &|| Ok(()), |state| {
            for (kind, id) in [
                ("decision", "a"),
                ("decision", "b"),
                ("completion", "a"),
                ("decision", "c"),
                ("completion", "a"),
            ] {
                // Completions are stored raw with the reported request.
                let (links, details) = if kind == "decision" {
                    (
                        json!({"request_id":id}),
                        json!({"operation":{"executable":id},"host_decision":"allow"}),
                    )
                } else {
                    (
                        json!({}),
                        json!({"operation":{"executable":id},"reported_request_id":id}),
                    )
                };
                let event = state.event(kind, &context, links, json!({}), details);
                state.record(event);
            }
            Ok(json!({}))
        })
        .expect("observations");
    let state = crate::review::project::load(&context).expect("state");
    let decisions: Vec<_> = state
        .events
        .iter()
        .filter(|e| e["kind"] == "decision")
        .map(|e| e["links"]["request_id"].clone())
        .collect();
    assert_eq!(decisions, [json!("b"), json!("c")]);
    // The completion that linked to "a" leaves with it; the late one stays unlinked.
    assert!(state.events.iter().all(|e| e["links"]["request_id"] != "a"));
    assert_eq!(state.events.len(), 3);
}

#[test]
fn joined_history_exposes_the_original_operation_and_preserves_ask_after_retry() {
    let (root, context) = history_fixture(5);
    let operation = json!({"kind":"argv","executable":"acme","arguments":["read"]});
    Store::open(root.path()).expect("store").update(&context, &|| Ok(()), |state| {
        for (kind, details) in [("decision", json!({"operation":operation,"request":{"cwd":"/acme"},"evaluation":{"decision":"ask"}})),
            ("run_grant_issued", json!({})), ("decision", json!({"operation":{"kind":"other"},"evaluation":{"decision":"allow"},"host_decision":"allow"})),
            ("completion", json!({"exit_code":0,"operation":{"kind":"other"},"reported_request_id":"original"}))] {
            let links = if kind == "completion" { json!({}) } else { json!({"request_id":"original"}) };
            let event = state.event(kind, &context, links, json!({}), details);
            state.record(event);
        }
        Ok(json!({}))
    }).expect("observations");
    let input = crate::review::history::Query {
        denied: false,
        approved: true,
        agent: None,
        entry: None,
    };
    let history = super::history::inspect(&context, &input).expect("history");
    let entry = &history["entries"][0];
    assert_eq!(entry["command"], operation);
    assert_eq!(entry["request"]["cwd"], "/acme");
    assert_eq!(entry["evaluation"]["decision"], "ask");
    assert_eq!(entry["approved"], true);
    assert_eq!(entry["outcome"]["exit_code"], 0);
}

#[test]
fn joined_history_retains_rewrite_disposition_and_explanation() {
    let (root, context) = history_fixture(5);
    Store::open(root.path()).unwrap().update(&context, &|| Ok(()), |state| {
        let event = state.event("decision", &context, json!({"request_id":"rewrite"}), json!({}),
            json!({"operation":{"kind":"shell","source":"acme"},
                "evaluation":{"decision":"ask","explanation":{"guidance":[{"message":"Use literal arguments."}]}},
                "disposition":"blocked","code":"request-rewrite"}));
        state.record(event);
        Ok(json!({}))
    }).unwrap();
    let history = super::history::inspect(
        &context,
        &crate::review::history::Query {
            denied: false,
            approved: false,
            agent: None,
            entry: None,
        },
    )
    .unwrap();
    let row = &history["entries"][0];
    assert_eq!(row["evaluation"]["decision"], "ask");
    assert_eq!(row["disposition"], "blocked");
    assert_eq!(row["code"], "request-rewrite");
    assert_eq!(
        row["evaluation"]["explanation"]["guidance"][0]["message"],
        "Use literal arguments."
    );
    assert_eq!(row["approved"], false);
    assert_eq!(row["denied"], false);
}
#[test]
fn repeated_publication_compacts_authority_and_history_retains_commands() {
    let root = assert_fs::TempDir::new().expect("fixture");
    let candidate =
        Bundle::capture(r#"{"schema_version":1,"packs":[]}"#, &[], None).expect("empty policy");
    let mut candidates = BTreeMap::new();
    candidates.insert("project".into(), json!(candidate));
    review::initialize(root.path(), candidates).expect("initialize fixture");
    let context = review::open(root.path()).expect("context");
    let mut state = Store::open(root.path())
        .expect("store")
        .read()
        .expect("state");
    state.operator = crate::review::project::metadata(&state);
    state.history_limit = 5;
    let compiled = review::cache::build(&state, None).expect("prepare fixture");
    for _ in 0..1300 {
        review::runtime::publish(&mut state, &context, None, compiled.clone());
        state.compact_authorization();
    }
    assert_eq!(state.authorization_events.len(), 1);
    assert_eq!(state.events.as_slice(), &[] as &[serde_json::Value]);
    state
        .active
        .as_ref()
        .expect("active")
        .validate(&state)
        .expect("current checkpoint proves authority");
    for id in 0..20 {
        for kind in ["decision", "completion"] {
            let event = state.event(
                kind,
                &context,
                json!({"request_id":format!("request-{id}")}),
                json!({}),
                json!({}),
            );
            state.record(event);
        }
    }
    assert_eq!(state.events.len(), 10);
    assert_eq!(state.events[0]["links"]["request_id"], "request-15");
    state.events.clear();
    state
        .active
        .as_ref()
        .expect("active")
        .validate(&state)
        .expect("history purge preserves authority");
}

#[test]
fn doctor_names_packs_whose_registry_is_no_longer_enrolled() {
    let (_root, context) = history_fixture(10);
    let checks = |context: &review::Context, kind: &str| {
        let report = crate::review::project::doctor(context).unwrap();
        report["checks"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|c| c["check"] == kind)
            .cloned()
            .collect::<Vec<_>>()
    };
    assert_eq!(checks(&context, "sources"), Vec::<serde_json::Value>::new());
    // This fixture was never activated, which doctor reports with its route.
    assert_eq!(checks(&context, "policy")[0]["repair"], "shield init");
    Store::open(std::path::Path::new(&context.project_root))
        .unwrap()
        .update(&context, &|| Ok(()), |state| {
            state.operator["installed"] = json!([{"id":"acme/tools","version":"1.0.0",
                "source_alias":"acme","source_identity":"local:acme-removed"}]);
            Ok(json!({}))
        })
        .unwrap();
    let found = checks(&context, "sources");
    assert!(
        found[0]["detail"]
            .as_str()
            .unwrap()
            .contains("acme/tools comes from a registry that is no longer enrolled"),
        "{found:?}"
    );
    assert_eq!(found[0]["repair"], "shield registry add");
}

#[test]
fn doctor_reports_unloadable_source_without_requiring_policy_preparation() {
    let root = assert_fs::TempDir::new().unwrap();
    let bundle = Bundle::capture(r#"{"schema_version":1,"packs":[]}"#, &[], None).unwrap();
    let context = review::mcp::initialize_fixture(root.path(), &bundle);
    Store::open(root.path())
        .unwrap()
        .update_checkpoint(&context, &|| Ok(()), |state| {
            state.operator["projection_file_hashes"] = json!({
                "config.yml":crate::policy::fingerprint(b"schema_version: 1\npacks: []\n")
            });
            Ok(json!({}))
        })
        .unwrap();
    std::fs::write(root.path().join(".shield/config.yml"), [0xff]).unwrap();
    let state_path = root.path().join(".shield/review/state.json");
    let before = std::fs::read(&state_path).unwrap();
    for source in std::fs::read_dir(root.path().join(".shield/review/sources")).unwrap() {
        std::fs::write(source.unwrap().path(), "invalid retained source").unwrap();
    }
    let report = crate::review::project::doctor(&context).unwrap();
    assert_eq!(report["missing"], json!(["complete_status"]));
    assert!(
        report["checks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["check"] == "status")
    );
    assert!(
        report["checks"].as_array().unwrap().iter().any(|row| {
            row["check"] == "policy_source" && row["repair"] == "shield policy review"
        }),
        "{report}"
    );
    assert_eq!(std::fs::read(state_path).unwrap(), before);
}
