//! Reviewed form metadata needs exact human proof and follows moves and copies.
use super::*;
use crate::{
    configuration,
    review::{self, Context},
};
use serde_json::{Value, json};
use std::fs;

const CONFIG: &str = "schema_version: 1\npacks: []\nrules: rules.yml\n";
const RULES: &str = r#"{"schema_version":1,"kind":"custom","capabilities":["structured-rules-v1","argument-rules-v1"],"rules":[{"id":"inspect","decision":"allow","conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"exact","values":["inspect"]}}}]}"#;

fn fixture() -> (assert_fs::TempDir, Context) {
    let root = assert_fs::TempDir::new().expect("fixture root");
    let bundle = configuration::Bundle::capture(CONFIG, &[], Some(RULES)).expect("fixture policy");
    let context = review::mcp::initialize_fixture(root.path(), &bundle);
    fs::write(root.path().join(".shield/config.yml"), CONFIG).expect("fixture config");
    fs::write(root.path().join(".shield/rules.yml"), RULES).expect("fixture rules");
    (root, context)
}

fn copy(from: &Path, to: &Path) {
    for entry in fs::read_dir(from).expect("fixture directory") {
        let entry = entry.expect("fixture entry");
        let target = to.join(entry.file_name());
        if entry.file_type().expect("fixture file kind").is_dir() {
            fs::create_dir(&target).expect("fixture child directory");
            copy(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).expect("copy fixture file");
        }
    }
}

fn add_approval(root: &Path, context: &Context) {
    let path = Path::new(&context.project_root).join("acme.txt");
    fs::write(&path, "Acme text").expect("fixture text");
    let input = json!({"protocol_version":1,"request_id":"acme-read","expected_project_id":context.project_id,
        "cwd":context.project_root,"operation":{"kind":"read-text","path":path,"offset":null,"limit":null},
        "host":{"agent":"claude-mod","session_id":"acme-session","tool_use_id":"acme-tool","event":"PreToolUse","consumer_id":"acme-session"}});
    let (receipt, code) = crate::enforcement::protocol::evaluate(
        root,
        &serde_json::to_vec(&input).expect("read input"),
    );
    assert_eq!(code, 0, "{receipt}");
    assert_eq!(receipt["code"], "approval-required", "{receipt}");
    let id = receipt["approval"]["request_id"]
        .as_str()
        .expect("pending read request");
    review::read_approval::authorize(context, id, 300, None, false).expect("fixture grant");
}

fn record_integration_review(
    state: &mut State,
    context: &Context,
    review_id: &str,
    mechanism: &str,
) -> Value {
    let marker = json!({"project_id":state.project_id,"review_id":review_id});
    state.operator["integration_review"] = marker.clone();
    let mut event = state.event(
        "integration_review_confirmed",
        context,
        json!({}),
        json!({}),
        json!({"review_channel":marker}),
    );
    event["operator"] = json!({"operator_id":null,"display_name":null,"mechanism":mechanism});
    state.record(event);
    marker
}

#[test]
fn integration_review_forms_require_exact_human_proof_and_current_project() {
    for mechanism in ["host-elicitation", "operator-cli"] {
        let (root, context) = fixture();
        let store = Store::open(root.path()).unwrap();
        assert!(!forms_reviewed_at(root.path()));
        store
            .update(&context, &|| Ok(()), |state| {
                record_integration_review(state, &context, "review-confirmed", mechanism);
                Ok(json!({}))
            })
            .unwrap();
        assert!(forms_reviewed_at(root.path()));
        let path = root.path().join(".shield/state/state.json");
        let baseline = fs::read(&path).unwrap();
        for fault in [
            "mechanism",
            "review-id",
            "empty-id",
            "missing-event",
            "project-id",
            "event-project",
        ] {
            let mut raw: Value = serde_json::from_slice(&baseline).unwrap();
            let marker = &mut raw["operator"]["integration_review"];
            match fault {
                "review-id" => marker["review_id"] = json!("another-review"),
                "empty-id" => marker["review_id"] = json!(""),
                "project-id" => marker["project_id"] = json!("f".repeat(64)),
                _ => {}
            }
            let events = raw["authorization_events"].as_array_mut().unwrap();
            if fault == "missing-event" {
                events.retain(|event| event["kind"] != "integration_review_confirmed");
            } else {
                let event = events
                    .iter_mut()
                    .find(|event| event["kind"] == "integration_review_confirmed")
                    .unwrap();
                match fault {
                    "mechanism" => event["operator"]["mechanism"] = json!("host-tool-permission"),
                    "empty-id" => event["details"]["review_channel"]["review_id"] = json!(""),
                    "event-project" => event["provenance"]["project_id"] = json!("f".repeat(64)),
                    _ => {}
                }
            }
            fs::write(&path, raw.to_string()).unwrap();
            assert!(!forms_reviewed_at(root.path()), "{mechanism}: {fault}");
        }
        fs::write(&path, baseline).unwrap();
        assert!(forms_reviewed_at(root.path()));
        let copied = assert_fs::TempDir::new().unwrap();
        copy(root.path(), copied.path());
        assert!(
            forms_reviewed_at(copied.path()),
            "reviewed form capability metadata is portable"
        );
    }
}

#[test]
fn integration_review_compaction_and_history_purge_preserve_only_current_proof() {
    let (root, context) = fixture();
    add_approval(root.path(), &context);
    let store = Store::open(root.path()).unwrap();
    let before = store.read_checkpoint().unwrap();
    let authority = json!({"policy":before.policy_revision,"read":before.read_requests,"run":before.run_requests,
        "hooks":before.hook_requests,"sessions":before.session_grants});
    store
        .update(&context, &|| Ok(()), |state| {
            state.settings.history_enabled = false;
            record_integration_review(state, &context, "older-review", "host-elicitation");
            let current =
                record_integration_review(state, &context, "current-review", "host-elicitation");
            let mut foreign = state
                .authorization_events
                .iter()
                .find(|event| {
                    event["kind"] == "integration_review_confirmed"
                        && event["details"]["review_channel"] == current
                })
                .unwrap()
                .clone();
            foreign["provenance"]["project_id"] = json!("f".repeat(64));
            state.record(foreign);
            state.compact_authorization();
            let retained: Vec<_> = state
                .authorization_events
                .iter()
                .filter(|event| event["kind"] == "integration_review_confirmed")
                .collect();
            assert_eq!(retained.len(), 1);
            assert_eq!(retained[0]["details"]["review_channel"], current);
            Ok(json!({}))
        })
        .unwrap();
    assert!(forms_reviewed_at(root.path()));
    review::admin::prepare(
        &context,
        "history_configure",
        &json!({"enabled":false,"purge":true}),
        &|| Ok(()),
    )
    .unwrap()
    .commit(&context, &|| Ok(()))
    .unwrap();
    let after = store.read_checkpoint().unwrap();
    assert!(!after.settings.history_enabled);
    assert!(store.read_history().unwrap().events.is_empty());
    assert!(forms_reviewed_at(root.path()));
    assert_eq!(
        json!({"policy":after.policy_revision,"read":after.read_requests,"run":after.run_requests,
        "hooks":after.hook_requests,"sessions":after.session_grants}),
        authority
    );
    assert_eq!(
        after
            .authorization_events
            .iter()
            .filter(|event| event["kind"] == "integration_review_confirmed")
            .count(),
        1
    );
}
