//! Owned resources and durable outcomes exercise immediate policy review.
use super::*;
use crate::review::mcp::{self, Connection};

fn fixture() -> (assert_fs::TempDir, Context, Connection) {
    let root = assert_fs::TempDir::new().unwrap();
    let bundle =
        crate::configuration::Bundle::capture("schema_version: 1\npacks: []\n", &[], None).unwrap();
    let mut context = mcp::initialize_fixture(root.path(), &bundle);
    context.surface = "mcp".into();
    context.agent = Some("codex".into());
    context.session_id = Some("acme-session".into());
    context.connection_id = Some("acme-connection".into());
    (root, context, Connection::new("acme-connection".into()))
}

#[test]
fn record_only_lifecycle_leaves_reviewed_paths_and_orphan_leases_untouched() {
    let (root, context, connection) = fixture();
    prepare(&context, &connection, "metadata-review");
    let store = Store::open(root.path()).unwrap();
    let mut state = store.read_checkpoint().unwrap();
    state.proposals[0].reviewed_paths = Some(std::collections::BTreeMap::from([(
        "rules.yml".into(),
        Some(crate::policy::fingerprint(b"different reviewed bytes")),
    )]));
    refresh_inspection(&mut state);
    assert_eq!(state.proposals[0].status, "pending");
    refresh(&mut state);
    assert_eq!(state.proposals[0].status, "stale");
    let orphan = lease_path(&context.project_root, "metadata-orphan");
    fs::write(&orphan, "preserved orphan").unwrap();
    assert!(!alive_with(&context.project_root, "metadata-orphan", false));
    assert_eq!(fs::read_to_string(&orphan).unwrap(), "preserved orphan");
    assert!(!alive(&context.project_root, "metadata-orphan"));
    assert!(!orphan.exists());
}
fn prepare(context: &Context, connection: &Connection, id: &str) -> Value {
    let result = mcp::call(
        context,
        connection,
        "policy_propose",
        &json!({"change":{"kind":"rule_add","rule":{
        "id":id,"decision":"allow","conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"exact","values":[id]}}
    }},"idempotency_key":id}),
        &|| Ok(()),
    );
    assert!(result["error"].is_null(), "{result}");
    result["data"].clone()
}
fn receipt(context: &Context, connection: &Connection, id: &Value) -> Value {
    let result = mcp::call(
        context,
        connection,
        "policy_receipt",
        &json!({"proposal_id":id}),
        &|| Ok(()),
    );
    assert!(result["error"].is_null(), "{result}");
    result["data"].clone()
}

#[test]
fn copied_policy_workflow_proposal_continuations_require_fresh_owned_review() {
    let (original, context, connection) = fixture();
    let proposed = prepare(&context, &connection, "acme-copy-change");
    let path = original.path().join(".shield/review/state.json");
    let before = fs::read(&path).unwrap();
    let copy = assert_fs::TempDir::new().unwrap();
    crate::test_support::copy_fixture_directory(original.path(), copy.path());
    let opened = crate::review::open(copy.path()).unwrap();
    let mut child = context.clone();
    child.project_id = opened.project_id;
    child.project_root = opened.project_root;
    child.connection_id = Some("copy-connection".into());
    let child_connection = Connection::new("copy-connection".into());
    let child_revision = crate::review::inspect(&child).unwrap()["policy_revision"].clone();
    // A copy is the same project; another connection still cannot continue
    // this connection's workflow.
    assert_eq!(child.project_id, context.project_id);
    let refused = mcp::call_with_confirmation(
        &child,
        &child_connection,
        "policy_apply",
        &proposed["apply_arguments"],
        &|| Ok(()),
        true,
    );
    assert!(refused["error"].is_object(), "{refused}");
    let missing = mcp::call(
        &child,
        &child_connection,
        "policy_receipt",
        &json!({"proposal_id":proposed["proposal_id"]}),
        &|| Ok(()),
    );
    // The copy carries the proposal record; another connection finds it
    // withdrawn, never applicable.
    assert_eq!(missing["data"]["status"], "withdrawn", "{missing}");
    assert_eq!(
        crate::review::inspect(&child).unwrap()["policy_revision"],
        child_revision
    );

    // The copied record keeps its idempotency key in this conversation, so a
    // fresh review uses a new key.
    let fresh = prepare(&child, &child_connection, "acme-copy-fresh");
    assert_ne!(fresh["proposal_id"], proposed["proposal_id"]);
    assert_eq!(
        crate::review::inspect(&child).unwrap()["policy_revision"],
        child_revision
    );
    let applied = mcp::call_with_confirmation(
        &child,
        &child_connection,
        "policy_apply",
        &fresh["apply_arguments"],
        &|| Ok(()),
        true,
    );
    assert!(applied["error"].is_null(), "{applied}");
    assert_eq!(
        receipt(&child, &child_connection, &fresh["proposal_id"])["status"],
        "applied"
    );
    assert_ne!(
        crate::review::inspect(&child).unwrap()["policy_revision"],
        child_revision
    );
    assert_eq!(fs::read(&path).unwrap(), before);
    let original_state = Store::open(original.path())
        .unwrap()
        .read_checkpoint()
        .unwrap();
    assert_eq!(
        original_state
            .proposals
            .iter()
            .find(|row| row.id == proposed["proposal_id"])
            .unwrap()
            .status,
        "pending"
    );
    let applied = mcp::call_with_confirmation(
        &context,
        &connection,
        "policy_apply",
        &proposed["apply_arguments"],
        &|| Ok(()),
        true,
    );
    assert!(applied["error"].is_null(), "{applied}");
    assert_eq!(
        receipt(&context, &connection, &proposed["proposal_id"])["status"],
        "applied"
    );
}

#[test]
fn prepared_changes_close_on_disconnect_without_losing_outcomes() {
    let (root, context, connection) = fixture();
    let proposed = prepare(&context, &connection, "read");
    drop(connection);
    let state = Store::open(root.path()).unwrap().read_checkpoint().unwrap();
    assert_eq!(state.proposals[0].status, "withdrawn");
    assert_eq!(
        state.proposals[0].closed_reason.as_deref(),
        Some("connection_closed")
    );
    let mut reconnected = context.clone();
    reconnected.connection_id = Some("new-connection".into());
    let connection = Connection::new("new-connection".into());
    let outcome = receipt(&reconnected, &connection, &proposed["proposal_id"]);
    assert_eq!(outcome["status"], "withdrawn");
    assert!(outcome.get("apply_arguments").is_none());
    let result = mcp::call(
        &reconnected,
        &connection,
        "policy_apply",
        &proposed["apply_arguments"],
        &|| Ok(()),
    );
    assert!(!result["error"].is_null(), "{result}");
}

#[test]
fn a_new_preparation_closes_the_previous_owned_attempt() {
    let (_root, context, connection) = fixture();
    let first = prepare(&context, &connection, "first");
    let second = prepare(&context, &connection, "second");
    let outcome = receipt(&context, &connection, &first["proposal_id"]);
    assert_eq!(outcome["status"], "withdrawn");
    assert_eq!(outcome["closed_reason"], "superseded");
    let replay = prepare(&context, &connection, "first");
    assert_eq!(replay["proposal_id"], first["proposal_id"]);
    assert!(replay.get("apply_arguments").is_none());
    let applied = mcp::call_with_confirmation(
        &context,
        &connection,
        "policy_apply",
        &second["apply_arguments"],
        &|| Ok(()),
        true,
    );
    assert!(applied["error"].is_null(), "{applied}");
}

#[test]
fn host_outcomes_close_attempts_and_never_replace_a_committed_receipt() {
    for (answer, status) in [
        ("approval_declined", "rejected"),
        ("approval_cancelled", "withdrawn"),
        ("approval_unavailable", "withdrawn"),
    ] {
        let (_root, context, connection) = fixture();
        let proposed = prepare(&context, &connection, "read");
        let result = mcp::policy_outcome(&context, &proposed["apply_arguments"], answer);
        assert_eq!(result["data"]["status"], status, "{result}");
        assert_eq!(
            receipt(&context, &connection, &proposed["proposal_id"])["status"],
            status
        );
        assert!(
            mcp::call_with_confirmation(
                &context,
                &connection,
                "policy_apply",
                &proposed["apply_arguments"],
                &|| Ok(()),
                true
            )["error"]
                .is_object()
        );
    }
    let (_root, context, connection) = fixture();
    let proposed = prepare(&context, &connection, "read");
    let applied = mcp::call_with_confirmation(
        &context,
        &connection,
        "policy_apply",
        &proposed["apply_arguments"],
        &|| Ok(()),
        true,
    );
    assert!(applied["error"].is_null(), "{applied}");
    let late = mcp::policy_outcome(&context, &proposed["apply_arguments"], "approval_cancelled");
    assert!(late["error"].is_null(), "{late}");
    for field in [
        "activation_id",
        "policy_revision",
        "candidate_digest",
        "committed",
    ] {
        assert_eq!(
            late["data"]["receipt"][field],
            applied["data"]["receipt"][field]
        );
    }
}

#[test]
fn expiry_and_legacy_migration_close_candidates_without_approval() {
    for legacy in [false, true] {
        let (root, context, connection) = fixture();
        let proposed = prepare(&context, &connection, "read");
        Store::open(root.path())
            .unwrap()
            .update_checkpoint(&context, &|| Ok(()), |state| {
                if legacy {
                    state.proposals[0].interaction = None;
                } else {
                    state.proposals[0]
                        .interaction
                        .as_mut()
                        .unwrap()
                        .expires_at_ms = 0;
                }
                Ok(Value::Null)
            })
            .unwrap();
        let outcome = receipt(&context, &connection, &proposed["proposal_id"]);
        assert_eq!(outcome["status"], "withdrawn");
        assert_eq!(
            outcome["closed_reason"],
            if legacy {
                "legacy_interaction"
            } else {
                "expired"
            }
        );
        let refused = mcp::call_with_confirmation(
            &context,
            &connection,
            "policy_apply",
            &proposed["apply_arguments"],
            &|| Ok(()),
            true,
        );
        assert!(
            refused["error"]["message"]
                .as_str()
                .unwrap()
                .contains("prepare a fresh change"),
            "{refused}"
        );
    }
}

#[test]
fn observed_task_end_closes_only_that_sessions_unapplied_change() {
    let (root, context, connection) = fixture();
    let proposed = prepare(&context, &connection, "read");
    // A mismatched authorization event cannot bypass protection or end a task.
    let mismatched = crate::agents::translate_event(
        root.path(),
        crate::agents::find("codex").unwrap(),
        &serde_json::to_vec(&json!({"hook_event_name":"PreToolUse","session_id":"acme-session"}))
            .unwrap(),
        crate::agents::Event::Ended,
        &crate::diagnostics::Log::disabled(),
    );
    assert_eq!(
        mismatched["hookSpecificOutput"]["permissionDecision"],
        "deny"
    );
    end_session(root.path(), "codex", "other-session").unwrap();
    assert_eq!(
        Store::open(root.path())
            .unwrap()
            .read_checkpoint()
            .unwrap()
            .proposals[0]
            .status,
        "pending"
    );
    let output = crate::agents::translate_event(
        root.path(),
        crate::agents::find("codex").unwrap(),
        &serde_json::to_vec(&json!({"hook_event_name":"Stop","session_id":"acme-session"}))
            .unwrap(),
        crate::agents::Event::Ended,
        &crate::diagnostics::Log::disabled(),
    );
    assert_eq!(output, json!({}));
    assert_eq!(
        receipt(&context, &connection, &proposed["proposal_id"])["closed_reason"],
        "task_ended"
    );
}

#[test]
fn another_connection_cannot_take_over_a_live_review() {
    let (root, context, connection) = fixture();
    let proposed = prepare(&context, &connection, "read");
    let mut foreign = context.clone();
    foreign.connection_id = Some("foreign".into());
    let foreign_connection = Connection::new("foreign".into());
    let replay = prepare(&foreign, &foreign_connection, "read");
    assert_eq!(replay["proposal_id"], proposed["proposal_id"]);
    assert!(replay.get("apply_arguments").is_none());
    let refused = mcp::call_with_confirmation(
        &foreign,
        &foreign_connection,
        "policy_apply",
        &proposed["apply_arguments"],
        &|| Ok(()),
        true,
    );
    assert!(!refused["error"].is_null(), "{refused}");
    assert_eq!(
        Store::open(root.path())
            .unwrap()
            .read_checkpoint()
            .unwrap()
            .proposals[0]
            .status,
        "pending"
    );
}

#[test]
fn reclamation_preserves_live_work_current_receipts_and_the_recovery_window() {
    let (root, context, connection) = fixture();
    let committed = prepare(&context, &connection, "committed");
    let result = mcp::call_with_confirmation(
        &context,
        &connection,
        "policy_apply",
        &committed["apply_arguments"],
        &|| Ok(()),
        true,
    );
    assert!(result["error"].is_null(), "{result}");
    let old = prepare(&context, &connection, "old");
    let recent = prepare(&context, &connection, "recent");
    let live = prepare(&context, &connection, "live");
    let store = Store::open(root.path()).unwrap();
    store
        .update_checkpoint(&context, &|| Ok(()), |state| {
            for row in &mut state.proposals {
                if row.id == committed["proposal_id"] || row.id == old["proposal_id"] {
                    row.interaction.as_mut().unwrap().expires_at_ms = 0;
                }
            }
            reclaim(state);
            Ok(Value::Null)
        })
        .unwrap();
    let state = store.read_checkpoint().unwrap();
    assert_eq!(state.proposals.len(), 3);
    assert!(
        state
            .proposals
            .iter()
            .any(|p| p.id == committed["proposal_id"] && p.status == "applied")
    );
    assert!(
        state
            .proposals
            .iter()
            .any(|p| p.id == recent["proposal_id"] && p.status == "withdrawn")
    );
    assert!(
        state
            .proposals
            .iter()
            .any(|p| p.id == live["proposal_id"] && p.status == "pending")
    );
    assert!(!state.proposals.iter().any(|p| p.id == old["proposal_id"]));
}

#[test]
fn deferred_intent_closes_its_candidate_and_resumes_only_through_fresh_preparation() {
    let (_root, context, connection) = fixture();
    let proposed = prepare(&context, &connection, "read");
    let args = json!({"proposal_id":proposed["proposal_id"],"intent":"Allow Acme inspection next week", "constraints":"Read-only, current project", "idempotency_key":"explicit-later"});
    let saved = mcp::call(&context, &connection, "policy_defer", &args, &|| Ok(()));
    assert!(saved["error"].is_null(), "{saved}");
    assert_eq!(
        receipt(&context, &connection, &proposed["proposal_id"])["closed_reason"],
        "explicitly_deferred"
    );
    let id = &saved["data"]["draft"]["id"];
    let listed = mcp::call(
        &context,
        &connection,
        "policy_draft_list",
        &json!({"draft_id":id}),
        &|| Ok(()),
    );
    let draft = &listed["data"]["draft"];
    assert_eq!(draft["intent"], args["intent"]);
    for absent in [
        "candidate",
        "apply_arguments",
        "policy_revision",
        "approved",
    ] {
        assert!(draft.get(absent).is_none(), "{draft}");
    }
    let repeated = mcp::call(&context, &connection, "policy_defer", &args, &|| Ok(()));
    assert_eq!(repeated["data"]["draft"]["id"], *id);
    let fresh = prepare(&context, &connection, "fresh-read");
    assert_ne!(fresh["proposal_id"], proposed["proposal_id"]);
    assert!(fresh["apply_arguments"].is_object());
    let removed = mcp::call(
        &context,
        &connection,
        "policy_draft_remove",
        &json!({"draft_id":id}),
        &|| Ok(()),
    );
    assert!(removed["error"].is_null(), "{removed}");
    let listed = mcp::call(
        &context,
        &connection,
        "policy_draft_list",
        &json!({}),
        &|| Ok(()),
    );
    assert_eq!(listed["data"]["drafts"], json!([]));
}

#[test]
fn drafts_scope_removal_and_retries_to_the_creating_session_and_reclaim_expiry() {
    let (root, context, _connection) = fixture();
    let args = json!({"intent":"Inspect Acme later","idempotency_key":"later"});
    let saved = drafts::save(&context, &args).unwrap();
    let id = saved["draft"]["id"].as_str().unwrap();
    let mut foreign = context.clone();
    foreign.session_id = Some("other-session".into());
    assert!(drafts::remove(&foreign, id).is_err());
    let other = drafts::save(&foreign, &args).unwrap();
    assert_ne!(other["draft"]["id"], saved["draft"]["id"]);
    let mut reconnected = context.clone();
    reconnected.connection_id = Some("reconnected".into());
    assert_eq!(
        drafts::save(&reconnected, &args).unwrap()["draft"]["id"],
        saved["draft"]["id"]
    );
    drafts::remove(&reconnected, id).unwrap();
    let store = Store::open(root.path()).unwrap();
    store
        .update_checkpoint(&context, &|| Ok(()), |state| {
            state.policy_drafts = (0..128)
                .map(|index| {
                    let mut draft = other["draft"].clone();
                    draft["id"] = json!(format!("old-{index}"));
                    draft["created_at"] = json!("2000-01-01T00:00:00Z");
                    serde_json::from_value(draft).unwrap()
                })
                .collect();
            Ok(Value::Null)
        })
        .unwrap();
    assert_eq!(drafts::list(&context, None).unwrap()["drafts"], json!([]));
    assert!(drafts::save(&context, &args).is_ok());
    assert_eq!(store.read_checkpoint().unwrap().policy_drafts.len(), 1);
}

#[test]
fn lease_startup_reclaims_unlocked_orphans_and_preserves_live_connections() {
    let (root, context, _connection) = fixture();
    let live = Lease::new(&context).unwrap();
    let orphan = lease_path(&context.project_root, "orphan");
    fs::write(&orphan, b"").unwrap();
    let directory = orphan.parent().unwrap();
    let unrelated = directory.join("notes.lock");
    fs::write(&unrelated, b"keep").unwrap();
    let mut another = context.clone();
    another.connection_id = Some("another".into());
    let second = Lease::new(&another).unwrap();
    assert!(!orphan.exists());
    assert!(unrelated.exists());
    assert!(alive(&context.project_root, "acme-connection"));
    assert!(alive(&context.project_root, "another"));
    drop((live, second));
    assert!(!lease_path(root.path().to_str().unwrap(), "another").exists());
}

#[test]
fn transient_apply_failures_keep_the_review_live_and_retryable() {
    let (root, context, connection) = fixture();
    let proposed = prepare(&context, &connection, "retryable");
    for error in [
        Error::Locked,
        Error::Io(std::io::Error::other("fixture disk error")),
    ] {
        assert!(
            mcp::settle_apply_failure(&context, &proposed["apply_arguments"], &error).is_none()
        );
        assert_eq!(
            Store::open(root.path())
                .unwrap()
                .read_checkpoint()
                .unwrap()
                .proposals[0]
                .status,
            "pending"
        );
    }
    let result = mcp::call_with_confirmation(
        &context,
        &connection,
        "policy_apply",
        &proposed["apply_arguments"],
        &|| Ok(()),
        true,
    );
    assert!(result["error"].is_null(), "{result}");
}

#[test]
fn typed_stale_failures_persist_and_a_concurrent_commit_wins_reconciliation() {
    let (root, context, connection) = fixture();
    let proposed = prepare(&context, &connection, "stale-race");
    assert!(
        mcp::settle_apply_failure(
            &context,
            &proposed["apply_arguments"],
            &Error::StaleSnapshot
        )
        .is_none()
    );
    let raw: Value =
        serde_json::from_slice(&fs::read(root.path().join(".shield/review/state.json")).unwrap())
            .unwrap();
    assert_eq!(raw["proposals"][0]["status"], "stale");
    let proposed = prepare(&context, &connection, "commit-race");
    let applied = mcp::call_with_confirmation(
        &context,
        &connection,
        "policy_apply",
        &proposed["apply_arguments"],
        &|| Ok(()),
        true,
    );
    assert!(applied["error"].is_null(), "{applied}");
    let reconciled = mcp::settle_apply_failure(
        &context,
        &proposed["apply_arguments"],
        &Error::StaleSnapshot,
    )
    .unwrap();
    assert_eq!(
        reconciled["data"]["receipt"]["activation_id"],
        applied["data"]["receipt"]["activation_id"]
    );
    let mut changed = proposed["apply_arguments"].clone();
    changed["unexpected"] = json!(true);
    let refused = mcp::call_with_confirmation(
        &context,
        &connection,
        "policy_apply",
        &changed,
        &|| Ok(()),
        true,
    );
    assert!(refused["error"].is_object(), "{refused}");
    assert_eq!(
        receipt(&context, &connection, &proposed["proposal_id"])["status"],
        "applied"
    );
}

#[test]
fn ordinary_authority_callbacks_do_not_read_proposal_payloads_or_unlink_orphans() {
    use clap::Parser as _;
    let (root, context, connection) = fixture();
    let proposed = prepare(&context, &connection, "callback-cost");
    let store = Store::open(root.path()).unwrap();
    store
        .update_checkpoint(&context, &|| Ok(()), |state| {
            state.proposals[0].reviewed_paths = Some(std::collections::BTreeMap::from([(
                "rules.yml".into(),
                Some(crate::policy::fingerprint(b"reviewed")),
            )]));
            Ok(Value::Null)
        })
        .unwrap();
    fs::write(
        root.path().join(".shield/rules.yml"),
        "Acme changed payload".repeat(32_768),
    )
    .unwrap();
    let orphan = lease_path(&context.project_root, "unrelated-orphan");
    fs::write(&orphan, "preserved").unwrap();
    let logs = assert_fs::TempDir::new().unwrap();
    let cli = crate::cli::Cli::parse_from([
        "shield",
        "--debug-log-dir",
        logs.path().to_str().unwrap(),
        "status",
    ]);
    let log = std::sync::Arc::new(crate::diagnostics::Log::open(&cli));
    let trace = log
        .request("command", "callback-fixture", None, None)
        .unwrap();
    {
        let _scope = trace.enter();
        store
            .update_checkpoint(&context, &|| Ok(()), |_| Ok(Value::Null))
            .unwrap();
        assert_eq!(trace.recorded_count("proposal.capture_file_reads"), 0);
        assert_eq!(trace.recorded_count("source.hydrated"), 0);
        assert_eq!(trace.recorded_count("policy.preparations"), 0);
        assert!(orphan.exists());
        let state = store.read_checkpoint().unwrap();
        let row = state
            .proposals
            .iter()
            .find(|row| row.id == proposed["proposal_id"])
            .unwrap();
        assert_eq!(row.status, "pending");
        assert!(matches!(
            validate_capture(&context.project_root, row),
            Err(Error::StaleSnapshot)
        ));
        assert_eq!(trace.recorded_count("proposal.capture_file_reads"), 1);
    }
    trace.finish("success");
}
