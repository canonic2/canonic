//! Optional observations keep exact correlation and authorization independent.
use super::*;
use crate::review;
use assert_fs::prelude::*;

fn operation() -> Value {
    json!({"kind":"read-text","path":"/acme/notes.txt","offset":null,"limit":null})
}

#[test]
fn pack_candidates_preserve_current_alias_source_and_installed_identity() {
    let (_root, context) = review::tests::enrolled();
    let registry = assert_fs::TempDir::new().unwrap();
    let source =
        canonic_registry::source::Source::local(&registry.path().canonicalize().unwrap()).unwrap();
    let root = Path::new(&context.project_root);
    Store::open(root).unwrap().update(&context, &|| Ok(()), |state| {
        state.operator["sources"]["acme"] = json!(source);
        state.registry_checkpoints.insert(source.identity().into(), json!({"sequence":1,"digest":"0".repeat(64),"releases":{},"commands":{"npm":["acme/runtime"],"node":["acme/runtime"],"acme-db":["acme/database"]}}));
        // Same pack ID from another registry must not hide this candidate.
        state.operator["installed"] = json!([{"id":"acme/runtime","source_identity":"main"}]);
        Ok(Value::Null)
    }).unwrap();
    assert_eq!(
        uninstalled_packs(root, "/usr/local/bin/npm"),
        ["acme/acme/runtime"]
    );
    assert_eq!(uninstalled_packs(root, "node"), ["acme/acme/runtime"]);
    assert_eq!(uninstalled_packs(root, "acme-db"), ["acme/acme/database"]);
    assert_eq!(uninstalled_packs(root, "unrelated"), Vec::<String>::new());
    Store::open(root)
        .unwrap()
        .update(&context, &|| Ok(()), |state| {
            state.operator["installed"][0]["source_identity"] = json!(source.identity());
            Ok(Value::Null)
        })
        .unwrap();
    assert_eq!(uninstalled_packs(root, "npm"), Vec::<String>::new());
    Store::open(root)
        .unwrap()
        .update(&context, &|| Ok(()), |state| {
            state.operator["sources"]
                .as_object_mut()
                .unwrap()
                .remove("acme");
            Ok(Value::Null)
        })
        .unwrap();
    assert_eq!(uninstalled_packs(root, "acme-db"), Vec::<String>::new());
}

#[test]
fn exact_completion_is_idempotent_and_other_origins_or_windows_remain_uncorrelated() {
    let (_root, mut ctx) = review::tests::enrolled();
    ctx.surface = "hook".into();
    ctx.source = "host".into();
    ctx.tool_use_id = Some("tool-one".into());
    ctx.host_event_name = Some("PreToolUse".into());
    let receipt = decision(
        &ctx,
        json!({"operation":operation(),"host_decision":"allow","policy_revision":"historical"}),
    )
    .unwrap();
    ctx.host_event_name = Some("PostToolUse".into());
    let mut input = EventInput {
        delivery_id: "delivery-one".into(),
        request_id: Some(receipt["request_id"].as_str().unwrap().into()),
        status: "completed".into(),
    };
    let first = completion(&ctx, &input, &operation()).unwrap();
    // Receipts acknowledge raw publication only; readers derive links.
    assert_eq!(first["request_id"], Value::Null);
    let repeated = completion(&ctx, &input, &operation()).unwrap();
    assert_ne!(repeated["event_id"], first["event_id"]);
    let event = review::tests::history(&ctx, Some(first["event_id"].as_str().unwrap())).unwrap();
    assert_eq!(event["links"]["decision_event_id"], receipt["event_id"]);
    assert_eq!(event["details"]["exit_code"], Value::Null);
    assert!(review::tests::history(&ctx, Some(repeated["event_id"].as_str().unwrap())).is_err());
    input.status = "failed".into();
    let changed = completion(&ctx, &input, &operation()).unwrap();
    let changed =
        review::tests::history(&ctx, Some(changed["event_id"].as_str().unwrap())).unwrap();
    assert_eq!(changed["details"]["delivery_conflict"], true);
    assert_eq!(changed["links"]["decision_event_id"], Value::Null);
    input.delivery_id = "other-tool".into();
    ctx.tool_use_id = Some("tool-two".into());
    let result = completion(&ctx, &input, &operation()).unwrap();
    assert_eq!(result["request_id"], Value::Null);
    assert_eq!(
        review::tests::history(&ctx, Some(result["event_id"].as_str().unwrap())).unwrap()["links"]
            ["decision_event_id"],
        Value::Null
    );
    ctx.tool_use_id = Some("tool-one".into());
    input.delivery_id = "other-window".into();
    let mut changed = operation();
    changed["offset"] = json!(2);
    let result = completion(&ctx, &input, &changed).unwrap();
    assert_eq!(result["request_id"], Value::Null);
    assert_eq!(
        review::tests::history(&ctx, Some(result["event_id"].as_str().unwrap())).unwrap()["links"]
            ["decision_event_id"],
        Value::Null
    );
}

#[test]
fn optional_history_failure_opt_out_and_retention_preserve_authorization_and_missing_links() {
    let (root, ctx) = review::tests::enrolled();
    let before = std::fs::read(root.child(".shield/review/state.json").path()).unwrap();
    root.child(".shield/review/state.lock")
        .write_str("owned test barrier")
        .unwrap();
    // An optional observation does not contend with the authorization writer.
    assert_eq!(decision(&ctx, json!({})).unwrap()["status"], "recorded");
    assert_eq!(
        std::fs::read(root.child(".shield/review/state.json").path()).unwrap(),
        before
    );
    std::fs::remove_file(root.child(".shield/review/state.lock").path()).unwrap();
    review::history_settings(&ctx, None, None, true, &|| Ok(())).unwrap();
    review::history_settings(&ctx, Some(false), None, false, &|| Ok(())).unwrap();
    assert_eq!(decision(&ctx, json!({})).unwrap()["status"], "disabled");
    assert_eq!(
        review::tests::history(&ctx, None).unwrap()["events"],
        json!([])
    );
    review::history_settings(&ctx, Some(true), Some(1), false, &|| Ok(())).unwrap();
    let receipt = decision(
        &ctx,
        json!({"operation":operation(),"host_decision":"allow"}),
    )
    .unwrap();
    let latest = decision(
        &ctx,
        json!({"operation":operation(),"host_decision":"deny"}),
    )
    .unwrap();
    let input = EventInput {
        delivery_id: "after-retention".into(),
        request_id: Some(receipt["request_id"].as_str().unwrap().into()),
        status: "completed".into(),
    };
    let receipt = completion(&ctx, &input, &operation()).unwrap();
    assert_eq!(
        review::tests::history(&ctx, Some(receipt["event_id"].as_str().unwrap())).unwrap()["links"]
            ["decision_event_id"],
        Value::Null
    );
    let history = review::tests::history(&ctx, None).unwrap();
    let events = history["events"].as_array().unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0]["kind"], "decision");
    assert_eq!(events[0]["links"]["request_id"], latest["request_id"]);
    assert_eq!(events[1]["kind"], "completion");
    assert_eq!(events[1]["links"]["request_id"], Value::Null);
}
