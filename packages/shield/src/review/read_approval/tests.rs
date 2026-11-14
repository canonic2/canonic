//! Exact resources, origin, expiry, revocation and atomic consumption acceptance.
use super::*;
use crate::{enforcement::read_text::Binding, review, runtime::Project};
use assert_fs::prelude::*;

pub(crate) fn fixture() -> (assert_fs::TempDir, Context, EvaluatedRequest, String) {
    let (root, context) = review::tests::enrolled();
    root.child("acme.txt")
        .write_str("Acme private text")
        .unwrap();
    let path = Path::new(&context.project_root).join("acme.txt");
    let binding = Binding::open(
        Path::new(&context.project_root),
        Path::new(&context.project_root),
        path.to_str().unwrap(),
    )
    .unwrap();
    let request = Request {
        executable: "shield:read-text".into(),
        arguments: vec![path.to_str().unwrap().into()],
        cwd: context.project_root.clone(),
        project: context.project_root.clone(),
    };
    let resource = review::runtime::consumer::ResourceRequest {
        mcp: None,
        input: policy::normalized::Input::File {
            action: "read".into(),
            sources: request.arguments.clone(),
            destination: None,
            recursive: false,
            original_input: json!({"file_path":path}),
        },
        context: policy::normalized::Context {
            cwd: request.cwd.clone(),
            environment: None,
            working_files: None,
        },
    };
    let evidence = Project::open(Path::new(&context.project_root))
        .unwrap()
        .snapshot()
        .unwrap()
        .evaluate_resource(request, resource)
        .unwrap();
    (root, context, evidence, binding.digest)
}

fn pending(context: &Context, evidence: &EvaluatedRequest, resource: &str) -> String {
    let value = gate(context, "factory-acme", evidence, resource, None, || true).unwrap();
    assert_eq!(value["code"], "approval-required");
    value["approval"]["request_id"].as_str().unwrap().into()
}

#[test]
fn identical_runtime_reads_reuse_one_pending_request() {
    let (root, context, evidence, resource) = fixture();
    let mut state = Store::open(root.path()).unwrap().read_checkpoint().unwrap();
    let origin = Origin {
        agent: context.agent.clone(),
        session_id: context.session_id.clone(),
        consumer_id: "acme-sdk".into(),
    };
    let time = now().unwrap();
    let first = capture_context(
        &mut state,
        &context,
        &origin,
        &evidence,
        &resource,
        time,
        Some("acme-runtime"),
    )
    .unwrap();
    let second = capture_context(
        &mut state,
        &context,
        &origin,
        &evidence,
        &resource,
        time + 1,
        Some("acme-runtime"),
    )
    .unwrap();
    assert_eq!(
        first["approval"]["request_id"],
        second["approval"]["request_id"]
    );
    assert_eq!(state.read_requests.len(), 1);
    capture_context(
        &mut state,
        &context,
        &origin,
        &evidence,
        &resource,
        time + 2,
        Some("another-runtime"),
    )
    .unwrap();
    assert_eq!(state.read_requests.len(), 2);
}

#[test]
fn copied_workspace_read_grant_cannot_replay_and_original_remains_usable() {
    let (root, context, evidence, resource) = fixture();
    let id = pending(&context, &evidence, &resource);
    authorize(&context, &id, 300, None, false).unwrap();
    let original = std::fs::read(root.path().join(".shield/review/state.json")).unwrap();
    let copy = assert_fs::TempDir::new().unwrap();
    crate::test_support::copy_fixture_directory(root.path(), copy.path());
    let opened = review::open(copy.path()).unwrap();
    let mut child = context.clone();
    child.project_id = opened.project_id;
    child.project_root = opened.project_root;
    // The copy is the same project; its requests name the original's files.
    assert_eq!(child.project_id, context.project_id);
    assert!(
        gate(
            &child,
            "factory-acme",
            &evidence,
            &resource,
            Some(&id),
            || true
        )
        .is_err()
    );

    let path = Path::new(&child.project_root).join("acme.txt");
    let binding = Binding::open(
        Path::new(&child.project_root),
        Path::new(&child.project_root),
        path.to_str().unwrap(),
    )
    .unwrap();
    let request = Request {
        executable: "shield:read-text".into(),
        arguments: vec![path.to_str().unwrap().into()],
        cwd: child.project_root.clone(),
        project: child.project_root.clone(),
    };
    let claim = review::runtime::consumer::ResourceRequest {
        mcp: None,
        input: policy::normalized::Input::File {
            action: "read".into(),
            sources: request.arguments.clone(),
            destination: None,
            recursive: false,
            original_input: json!({"file_path":path}),
        },
        context: policy::normalized::Context {
            cwd: child.project_root.clone(),
            environment: None,
            working_files: None,
        },
    };
    let fresh = Project::open(copy.path())
        .unwrap()
        .snapshot()
        .unwrap()
        .evaluate_resource(request, claim)
        .unwrap();
    let refused = gate(
        &child,
        "factory-acme",
        &fresh,
        &binding.digest,
        Some(&id),
        || true,
    )
    .unwrap();
    assert_eq!(refused["code"], "approval-unavailable");
    assert_eq!(refused["allowed"], false);
    let child_id = pending(&child, &fresh, &binding.digest);
    assert_ne!(child_id, id);
    authorize(&child, &child_id, 300, None, false).unwrap();
    assert_eq!(
        consume_with_retry(&child, &fresh, &binding.digest, &child_id)["allowed"],
        true
    );
    assert_eq!(
        consume_with_retry(&child, &fresh, &binding.digest, &child_id)["allowed"],
        false
    );
    assert_eq!(
        std::fs::read(root.path().join(".shield/review/state.json")).unwrap(),
        original
    );
    assert_eq!(
        consume_with_retry(&context, &evidence, &resource, &id)["allowed"],
        true
    );
    assert_eq!(
        consume_with_retry(&context, &evidence, &resource, &id)["allowed"],
        false
    );
}

#[test]
fn mcp_denial_is_confirmed_owned_and_never_issues_a_grant() {
    let (_root, mut context, evidence, resource) = fixture();
    context.agent = Some("codex".into());
    context.session_id = Some("acme-session".into());
    let id = pending(&context, &evidence, &resource);
    context.surface = "mcp".into();
    assert!(matches!(
        review::approvals::deny(&context, &id, false),
        Err(Error::ApprovalRequired)
    ));
    let mut other = context.clone();
    other.session_id = Some("other-session".into());
    assert!(matches!(
        review::approvals::deny(&other, &id, true),
        Err(Error::Missing)
    ));
    assert!(matches!(
        authorize(&context, &id, 300, None, false),
        Err(Error::ApprovalRequired)
    ));
    assert_eq!(
        review::approvals::deny(&context, &id, true).unwrap()["status"],
        "revoked"
    );
}

#[test]
fn terminal_read_receipts_replay_after_row_compaction_without_history_or_new_grants() {
    let (_root, mut context, evidence, resource) = fixture();
    context.agent = Some("codex".into());
    review::history_settings(&context, Some(false), None, false, &|| Ok(())).unwrap();
    let id = pending(&context, &evidence, &resource);
    authorize(&context, &id, 300, None, false).unwrap();
    assert_eq!(
        gate(
            &context,
            "factory-acme",
            &evidence,
            &resource,
            Some(&id),
            || true
        )
        .unwrap()["allowed"],
        true
    );
    let state = Store::open(Path::new(&context.project_root))
        .unwrap()
        .read_checkpoint()
        .unwrap();
    assert!(state.read_requests.is_empty());
    let outcome = gate(
        &context,
        "factory-acme",
        &evidence,
        &resource,
        Some(&id),
        || true,
    )
    .unwrap();
    assert_eq!(outcome["code"], "approval-consumed");
    assert_eq!(outcome["allowed"], false);
    assert!(outcome["approval"]["approval_id"].is_null());
    context.surface = "mcp".into();
    let replay = review::approvals::approve(&context, &json!({"request_id":id}), false).unwrap();
    assert_eq!(replay["status"], "consumed");
    assert!(replay.get("approval_id").is_none());
    assert!(review::approvals::deny(&context, &id, false).is_err());
    let second = pending(&context, &evidence, &resource);
    review::approvals::deny(&context, &second, true).unwrap();
    assert_eq!(
        review::approvals::deny(&context, &second, false).unwrap()["status"],
        "revoked"
    );
    let mut foreign = context.clone();
    foreign.session_id = Some("other-session".into());
    assert!(matches!(
        review::approvals::deny(&foreign, &second, false),
        Err(Error::Missing)
    ));
}

#[test]
fn status_counts_only_unexpired_requests_awaiting_a_decision() {
    let (_root, context, evidence, resource) = fixture();
    assert_eq!(
        review::project::status(&context).unwrap()["pending_approvals"],
        0
    );
    let id = pending(&context, &evidence, &resource);
    assert_eq!(
        review::project::status(&context).unwrap()["pending_approvals"],
        1
    );
    let state = Store::open(Path::new(&context.project_root))
        .unwrap()
        .read()
        .unwrap();
    let expiry = state.read_requests[0].pending_expires_at_ms;
    assert_eq!(review::approvals::pending_count(&state, expiry - 1), 1);
    assert_eq!(review::approvals::pending_count(&state, expiry), 0);
    assert!(inspect_from(&state, Some(&id), expiry - 1).is_ok());
    assert!(matches!(
        inspect_from(&state, Some(&id), expiry),
        Err(Error::Missing)
    ));
    authorize(&context, &id, 3600, None, false).unwrap();
    assert_eq!(
        review::project::status(&context).unwrap()["pending_approvals"],
        0
    );
}

#[test]
fn shown_requests_carry_their_creation_time_and_policy_basis_without_operands() {
    let (_root, context, evidence, resource) = fixture();
    let id = pending(&context, &evidence, &resource);
    let (kind, shown) = review::approvals::request_view(&context, &id).unwrap();
    assert!(matches!(kind, review::approvals::RequestKind::Read));
    assert!(shown["created_at_ms"].as_u64().is_some(), "{shown}");
    let policy = &shown["policy"];
    assert_eq!(policy["decision"], "ask", "{shown}");
    assert!(
        policy["reason"].as_str().is_some_and(|r| !r.is_empty()),
        "{shown}"
    );
    let rules = policy["rules"].as_array().unwrap();
    assert!(rules.len() <= 3, "{shown}");
    assert!(!policy.to_string().contains("acme.txt"), "{policy}");
    let listed = review::approvals::inspect(&context, None).unwrap();
    assert_eq!(listed["requests"][0]["policy"], *policy, "{listed}");
    // The summary is review context: approval still binds and consumes exactly.
    authorize(&context, &id, 300, None, false).unwrap();
    let value = gate(
        &context,
        "factory-acme",
        &evidence,
        &resource,
        Some(&id),
        || true,
    )
    .unwrap();
    assert_eq!(value["code"], "allowed-once", "{value}");
}

#[test]
fn approval_replay_uses_grant_expiry_after_pending_window_closes() {
    let (_root, context, evidence, resource) = fixture();
    let id = pending(&context, &evidence, &resource);
    let issued = authorize(&context, &id, 3600, None, false).unwrap();
    let store = Store::open(Path::new(&context.project_root)).unwrap();
    let state = store.read().unwrap();
    let row = &state.read_requests[0];
    let time = row.pending_expires_at_ms;
    let grant = row.grant.as_ref().unwrap();
    let (start, end) = (grant.issued_at_ms, grant.expires_at_ms);
    let events = state.authorization_events.len();
    assert_eq!(
        authorize_with_clock(&context, &id, 3600, None, false, &|| Ok(time)).unwrap(),
        issued
    );
    for (time, ttl) in [(time, 300), (start - 1, 3600), (end, 3600)] {
        assert!(authorize_with_clock(&context, &id, ttl, None, false, &|| Ok(time)).is_err());
    }
    let mut state = store.read().unwrap();
    assert_eq!(state.authorization_events.len(), events);
    let origin = state.read_requests[0].origin.clone();
    assert_eq!(
        consume(
            &mut state, &context, &origin, &evidence, &resource, &id, time, None
        )
        .unwrap()["code"],
        "allowed-once"
    );
    assert_ne!(
        consume(
            &mut state, &context, &origin, &evidence, &resource, &id, time, None
        )
        .unwrap()["code"],
        "allowed-once"
    );
}

#[test]
fn additional_read_roots_support_exact_one_time_grants() {
    use crate::policy::normalized::{Context as PolicyContext, Input};
    use crate::review::runtime::consumer::ResourceRequest;
    let root = assert_fs::TempDir::new().unwrap();
    let external = assert_fs::TempDir::new().unwrap();
    let outside = external.path().canonicalize().unwrap();
    let path = outside.join("notes.txt");
    std::fs::write(&path, "Acme reference").unwrap();
    let bundle = crate::configuration::Bundle::capture(
        &json!({"schema_version":1,"packs":[],"access_paths":{"read":[outside]}}).to_string(),
        &[],
        None,
    )
    .unwrap();
    let context = review::mcp::initialize_fixture(root.path(), &bundle);
    let snapshot = Project::open(Path::new(&context.project_root))
        .unwrap()
        .snapshot()
        .unwrap();
    let binding = Binding::open_scoped(
        Path::new(&context.project_root),
        Path::new(&context.project_root),
        path.to_str().unwrap(),
        snapshot.access_paths(),
    )
    .unwrap();
    let request = Request {
        executable: "shield:read-text".into(),
        arguments: vec![path.to_str().unwrap().into()],
        cwd: context.project_root.clone(),
        project: context.project_root.clone(),
    };
    let resource = ResourceRequest {
        mcp: None,
        input: Input::File {
            action: "read".into(),
            sources: request.arguments.clone(),
            destination: None,
            recursive: false,
            original_input: json!({"file_path":path}),
        },
        context: PolicyContext {
            cwd: request.cwd.clone(),
            environment: None,
            working_files: None,
        },
    };
    let evidence = snapshot.evaluate_resource(request, resource).unwrap();
    let id = pending(&context, &evidence, &binding.digest);
    authorize(&context, &id, 300, None, false).unwrap();
    let allowed = gate(
        &context,
        "factory-acme",
        &evidence,
        &binding.digest,
        Some(&id),
        || binding.verify().is_ok(),
    )
    .unwrap();
    assert_eq!(allowed["code"], "allowed-once");
    let replay = gate(
        &context,
        "factory-acme",
        &evidence,
        &binding.digest,
        Some(&id),
        || binding.verify().is_ok(),
    )
    .unwrap();
    assert_ne!(replay["code"], "allowed-once");
    assert_eq!(std::fs::read_to_string(path).unwrap(), "Acme reference");
}

fn consume_with_retry(
    context: &Context,
    evidence: &EvaluatedRequest,
    resource: &str,
    id: &str,
) -> serde_json::Value {
    review::tests::retry_locked(|| {
        gate(
            context,
            "factory-acme",
            evidence,
            resource,
            Some(id),
            || true,
        )
    })
}

#[test]
fn typed_read_grants_bind_original_input_and_retain_resource_during_review() {
    use crate::policy::normalized::{Context as PolicyContext, Input};
    use crate::review::runtime::consumer::ResourceRequest;
    let (_root, context, previous, digest) = fixture();
    let project = Project::open(Path::new(&context.project_root)).unwrap();
    let untyped = project
        .snapshot()
        .unwrap()
        .evaluate(previous.request().clone())
        .unwrap();
    assert!(matches!(
        gate(&context, "factory-acme", &untyped, &digest, None, || true),
        Err(Error::Invalid(_))
    ));
    let resource = ResourceRequest {
        mcp: None,
        input: Input::File {
            action: "read".into(),
            sources: previous.request().arguments[..1].to_vec(),
            destination: None,
            recursive: false,
            original_input: json!({"file_path":previous.request().arguments[0]}),
        },
        context: PolicyContext {
            cwd: context.project_root.clone(),
            environment: None,
            working_files: None,
        },
    };
    let evidence = project
        .snapshot()
        .unwrap()
        .evaluate_resource(previous.request().clone(), resource.clone())
        .unwrap();
    let id = pending(&context, &evidence, &digest);
    authorize(&context, &id, 300, None, false).unwrap();
    let state = Store::open(Path::new(&context.project_root))
        .unwrap()
        .read()
        .unwrap();
    assert_eq!(json!(state.read_requests[0].resource), json!(resource));
    let mut incomplete = json!(state.read_requests[0]);
    incomplete.as_object_mut().unwrap().remove("resource");
    let mut incomplete_state = state;
    incomplete_state.read_requests[0] = serde_json::from_value::<ReadRequest>(incomplete).unwrap();
    assert!(validate(&incomplete_state).is_err());
    let mut changed = resource;
    if let Input::File { original_input, .. } = &mut changed.input {
        original_input["limit"] = json!(1);
    }
    let different = project
        .snapshot()
        .unwrap()
        .evaluate_resource(previous.request().clone(), changed)
        .unwrap();
    assert_eq!(
        gate(
            &context,
            "factory-acme",
            &different,
            &digest,
            Some(&id),
            || true
        )
        .unwrap()["code"],
        "approval-stale"
    );
    assert_eq!(
        consume_with_retry(&context, &evidence, &digest, &id)["allowed"],
        true
    );
}

#[test]
fn approval_is_atomic_once_and_survives_history_opt_out_and_purge() {
    let (_root, context, evidence, resource) = fixture();
    review::history_settings(&context, Some(false), None, true, &|| Ok(())).unwrap();
    let id = pending(&context, &evidence, &resource);
    let grant = authorize(&context, &id, 300, Some("Acme inspection"), false).unwrap();
    assert_eq!(authorize(&context, &id, 300, None, false).unwrap(), grant);
    assert!(authorize(&context, &id, 301, None, false).is_err());
    let store = Store::open(Path::new(&context.project_root)).unwrap();
    let mut earlier = store.read().unwrap();
    let results = std::thread::scope(|scope| {
        let handles = (0..4)
            .map(|_| scope.spawn(|| consume_with_retry(&context, &evidence, &resource, &id)))
            .collect::<Vec<_>>();
        handles
            .into_iter()
            .map(|h| h.join().unwrap())
            .collect::<Vec<_>>()
    });
    assert_eq!(
        results.iter().filter(|v| v["allowed"] == true).count(),
        1,
        "{results:?}"
    );
    // A snapshot taken before consumption still holds the approved row, as a
    // copied checkout would; the per-user ledger refuses spending it again.
    let origin = earlier.read_requests[0].origin.clone();
    let replay = consume(
        &mut earlier,
        &context,
        &origin,
        &evidence,
        &resource,
        &id,
        now().unwrap(),
        None,
    )
    .unwrap();
    assert_eq!(replay["allowed"], false);
    assert_eq!(replay["code"], "approval-consumed");
    review::history_settings(&context, None, None, true, &|| Ok(())).unwrap();
    assert_eq!(
        gate(
            &context,
            "factory-acme",
            &evidence,
            &resource,
            Some(&id),
            || true
        )
        .unwrap()["code"],
        "approval-consumed"
    );
    let receipt = authorize(&context, &id, 300, None, false).unwrap();
    assert_eq!(receipt["status"], "consumed");
    assert_eq!(receipt["allowed"], false);
    assert!(receipt.get("approval_id").is_none());
    let state = store.read().unwrap();
    assert_eq!(state.events, [] as [Value; 0]);
    assert!(state.read_requests.is_empty());
}

#[test]
fn different_origin_request_resource_and_activation_cannot_consume() {
    let (_root, context, evidence, resource) = fixture();
    let id = pending(&context, &evidence, &resource);
    authorize(&context, &id, 300, None, false).unwrap();
    assert_eq!(
        gate(
            &context,
            "other-factory",
            &evidence,
            &resource,
            Some(&id),
            || true
        )
        .unwrap()["code"],
        "approval-stale"
    );
    let mut other = context.clone();
    other.session_id = Some("other-session".into());
    assert_eq!(
        gate(
            &other,
            "factory-acme",
            &evidence,
            &resource,
            Some(&id),
            || true
        )
        .unwrap()["code"],
        "approval-stale"
    );
    assert_eq!(
        gate(
            &context,
            "factory-acme",
            &evidence,
            "changed",
            Some(&id),
            || true
        )
        .unwrap()["code"],
        "approval-stale"
    );
    let project = Project::open(Path::new(&context.project_root)).unwrap();
    let mut request = evidence.request().clone();
    request.arguments.extend(["--limit".into(), "1".into()]);
    let different = project
        .snapshot()
        .unwrap()
        .evaluate_resource(request, evidence.resource().unwrap().clone())
        .unwrap();
    assert_eq!(
        gate(
            &context,
            "factory-acme",
            &different,
            &resource,
            Some(&id),
            || true
        )
        .unwrap()["code"],
        "approval-stale"
    );
    assert_eq!(
        gate(
            &context,
            "factory-acme",
            &evidence,
            &resource,
            Some(&id),
            || false
        )
        .unwrap()["code"],
        "read-changed"
    );
}

#[test]
fn a_grant_binds_the_exact_request_across_a_policy_file_edit() {
    let (root, context, evidence, resource) = fixture();
    let id = pending(&context, &evidence, &resource);
    authorize(&context, &id, 300, None, false).unwrap();
    // A person edits the policy files after approving the read.
    root.child(".shield/config.yml")
        .write_str(&format!(
            "schema_version: 1\npacks: []\naccess_paths:\n  read:\n  - {}\n",
            crate::test_support::absolute("/srv/acme")
        ))
        .unwrap();
    let edited = Project::open(Path::new(&context.project_root))
        .unwrap()
        .snapshot()
        .unwrap()
        .evaluate_resource(
            evidence.request().clone(),
            evidence.resource().unwrap().clone(),
        )
        .unwrap();
    assert_ne!(
        edited.identity().policy_revision(),
        evidence.identity().policy_revision()
    );
    let allowed = gate(
        &context,
        "factory-acme",
        &edited,
        &resource,
        Some(&id),
        || true,
    )
    .unwrap();
    assert_eq!(allowed["code"], "allowed-once", "{allowed}");
}

#[test]
fn file_changes_block_operator_approval_and_non_operator_cannot_approve() {
    let (root, context, evidence, resource) = fixture();
    let id = pending(&context, &evidence, &resource);
    let mut agent = context.clone();
    agent.surface = "mcp".into();
    assert!(matches!(
        authorize(&agent, &id, 300, None, false),
        Err(Error::ApprovalRequired)
    ));
    root.child("acme.txt")
        .write_str("Changed contents")
        .unwrap();
    assert!(authorize(&context, &id, 300, None, false).is_err());
    authorize(&context, &id, 300, None, true).unwrap();
    assert_eq!(
        gate(
            &context,
            "factory-acme",
            &evidence,
            &resource,
            Some(&id),
            || true
        )
        .unwrap()["code"],
        "approval-revoked"
    );
    assert!(authorize(&context, &id, 300, None, false).is_err());
}

#[test]
fn expiry_and_clock_rollback_block_without_consumption() {
    let (_root, context, evidence, resource) = fixture();
    let id = pending(&context, &evidence, &resource);
    let store = Store::open(Path::new(&context.project_root)).unwrap();
    let mut state = store.read().unwrap();
    let origin = state.read_requests[0].origin.clone();
    let expiry = state.read_requests[0].pending_expires_at_ms;
    assert_eq!(
        consume(
            &mut state, &context, &origin, &evidence, &resource, &id, expiry, None
        )
        .unwrap()["code"],
        "approval-expired"
    );
    authorize(&context, &id, 1, None, false).unwrap();
    let mut state = store.read().unwrap();
    let grant = state.read_requests[0].grant.as_ref().unwrap();
    let (issued, expires) = (grant.issued_at_ms, grant.expires_at_ms);
    for time in [issued - 1, expires] {
        assert_eq!(
            consume(
                &mut state, &context, &origin, &evidence, &resource, &id, time, None
            )
            .unwrap()["code"],
            "approval-expired"
        );
    }
    assert_eq!(state.read_requests[0].status, "approved");
}

#[test]
fn full_read_requests_refuse_with_pending_and_retained_counts() {
    let (_root, context, evidence, resource) = fixture();
    let id = pending(&context, &evidence, &resource);
    authorize(&context, &id, 3600, None, false).unwrap();
    let mut state = Store::open(Path::new(&context.project_root))
        .unwrap()
        .read()
        .unwrap();
    let origin = state.read_requests[0].origin.clone();
    let time = state.read_requests[0].grant.as_ref().unwrap().issued_at_ms;
    while state.read_requests.len() < 128 {
        let other = format!("acme-resource-{}", state.read_requests.len());
        capture(&mut state, &context, &origin, &evidence, &other, time).unwrap();
    }
    let refused = capture(
        &mut state,
        &context,
        &origin,
        &evidence,
        "acme-overflow",
        time,
    );
    assert!(
        matches!(
            refused,
            Err(Error::Capacity {
                kind: review::CapacityKind::ReadRequests,
                pending: 127,
                retained: 128,
                limit: 128,
            })
        ),
        "{refused:?}"
    );
    assert_eq!(state.read_requests.len(), 128);
}

/// Encode the historical pre-resource issuer's exact binding, independently of
/// `ReadRequest::binding`. The intermediate codec also accepted explicit null.
fn historical_read_state(mut state: Value, explicit_null: bool) -> Value {
    // The earlier format always recorded its absolute root.
    state["project_root"] = state["read_requests"][0]["request"]["project"].clone();
    let row = &mut state["read_requests"][0];
    row.as_object_mut().unwrap().remove("policy");
    if explicit_null {
        row["resource"] = Value::Null;
    } else {
        row.as_object_mut().unwrap().remove("resource");
    }
    let binding = policy::fingerprint(
        json!([
            row["request_id"],
            row["created_at_ms"],
            row["pending_expires_at_ms"],
            row["project_id"],
            row["activation_id"],
            row["policy_revision"],
            row["candidate_digest"],
            row["request"],
            row["resource_digest"],
            row["origin"],
            row["provenance"]
        ])
        .to_string()
        .as_bytes(),
    );
    row["binding"] = json!(binding);
    let grant_binding = if row["grant"].is_object() {
        let grant = &row["grant"];
        Some(policy::fingerprint(
            json!([
                binding,
                grant["approval_id"],
                grant["issued_at_ms"],
                grant["expires_at_ms"]
            ])
            .to_string()
            .as_bytes(),
        ))
    } else {
        None
    };
    if let Some(grant_binding) = &grant_binding {
        row["grant"]["binding"] = json!(grant_binding);
    }
    let request_id = row["request_id"].clone();
    for event in state["authorization_events"].as_array_mut().unwrap() {
        if event["links"]["request_id"] != request_id {
            continue;
        }
        event["details"]["request_binding"] = json!(binding);
        if event["kind"] == "read_grant_issued" {
            event["details"]["binding"] = json!(grant_binding.as_ref().unwrap());
        } else if event["kind"] == "read_grant_consumed" {
            event["details"]["grant_binding"] = json!(grant_binding.as_ref().unwrap());
        }
    }
    state
}

#[test]
fn legacy_read_pending_and_approved_continue_without_resigning() {
    for explicit_null in [false, true] {
        for already_approved in [false, true] {
            let (_root, context, evidence, digest) = fixture();
            let id = pending(&context, &evidence, &digest);
            if already_approved {
                authorize(&context, &id, 300, None, false).unwrap();
            }
            let store = Store::open(Path::new(&context.project_root)).unwrap();
            let wire =
                historical_read_state(json!(store.read_checkpoint().unwrap()), explicit_null);
            let original_binding = wire["read_requests"][0]["binding"].clone();
            let original_grant = wire["read_requests"][0]["grant"].clone();
            let original_events = wire["authorization_events"].clone();
            std::fs::write(
                Path::new(&context.project_root).join(".shield/review/state.json"),
                wire.to_string(),
            )
            .unwrap();
            let loaded = store.read_checkpoint().unwrap();
            assert_eq!(json!(loaded.read_requests[0].binding), original_binding);
            assert_eq!(json!(loaded.read_requests[0].grant), original_grant);
            assert_eq!(json!(loaded.authorization_events), original_events);
            validate(&loaded).unwrap();
            assert_eq!(
                inspect_from(&loaded, Some(&id), now().unwrap()).unwrap()["status"],
                if already_approved {
                    "approved"
                } else {
                    "pending"
                }
            );
            authorize(&context, &id, 300, None, false).unwrap();
            let loaded = store.read_checkpoint().unwrap();
            assert_eq!(json!(loaded.read_requests[0].binding), original_binding);
            if already_approved {
                assert_eq!(json!(loaded.read_requests[0].grant), original_grant);
                assert_eq!(json!(loaded.authorization_events), original_events);
            }
            assert_eq!(
                consume_with_retry(&context, &evidence, &digest, &id)["code"],
                "allowed-once"
            );
            assert_ne!(
                consume_with_retry(&context, &evidence, &digest, &id)["allowed"],
                true
            );
        }
    }
}

#[test]
fn legacy_read_terminal_receipts_remain_terminal_and_keep_original_proof() {
    for terminal in ["consumed", "revoked"] {
        let (_root, context, evidence, digest) = fixture();
        let id = pending(&context, &evidence, &digest);
        authorize(&context, &id, 300, None, false).unwrap();
        let mut state = Store::open(Path::new(&context.project_root))
            .unwrap()
            .read_checkpoint()
            .unwrap();
        if terminal == "consumed" {
            let origin = state.read_requests[0].origin.clone();
            assert_eq!(
                consume(
                    &mut state,
                    &context,
                    &origin,
                    &evidence,
                    &digest,
                    &id,
                    now().unwrap(),
                    None
                )
                .unwrap()["allowed"],
                true
            );
        } else {
            revoke_request(&mut state, &context, 0, None, now().unwrap()).unwrap();
        }
        let wire = historical_read_state(json!(state), false);
        let binding = wire["read_requests"][0]["binding"].clone();
        let grant = wire["read_requests"][0]["grant"].clone();
        let events = wire["authorization_events"].clone();
        let mut loaded: State = serde_json::from_value(wire).unwrap();
        loaded.project_root.clone_from(&context.project_root);
        validate(&loaded).unwrap();
        assert_eq!(json!(loaded.read_requests[0].binding), binding);
        assert_eq!(json!(loaded.read_requests[0].grant), grant);
        let original = json!(loaded.read_requests[0]);
        let origin = loaded.read_requests[0].origin.clone();
        let outcome = consume(
            &mut loaded,
            &context,
            &origin,
            &evidence,
            &digest,
            &id,
            now().unwrap(),
            None,
        )
        .unwrap();
        assert_ne!(outcome["allowed"], true);
        assert_eq!(outcome["approval"]["status"], terminal);
        assert_eq!(json!(loaded.read_requests[0]), original);
        assert_eq!(json!(loaded.authorization_events), events);
    }
}

#[test]
fn legacy_read_checkpoint_upgrade_preserves_policy_and_closes_unbound_operation_authority() {
    for status in ["pending", "approved", "consumed", "revoked"] {
        let (_root, context, evidence, digest) = fixture();
        let id = pending(&context, &evidence, &digest);
        if status != "pending" {
            authorize(&context, &id, 300, None, false).unwrap();
        }
        let store = Store::open(Path::new(&context.project_root)).unwrap();
        let mut state = store.read_checkpoint().unwrap();
        if status == "consumed" {
            let origin = state.read_requests[0].origin.clone();
            assert_eq!(
                consume(
                    &mut state,
                    &context,
                    &origin,
                    &evidence,
                    &digest,
                    &id,
                    now().unwrap(),
                    None
                )
                .unwrap()["allowed"],
                true
            );
        } else if status == "revoked" {
            revoke_request(&mut state, &context, 0, None, now().unwrap()).unwrap();
        }
        let mut wire = historical_read_state(json!(state), status == "approved");
        for name in [
            "hook_requests",
            "session_grants",
            "policy_drafts",
            "dismissed_reviews",
            "enforcement",
            "configuration_changes",
            "terminal_receipts",
            "followed_files",
        ] {
            wire.as_object_mut().unwrap().remove(name);
        }
        for field in ["runtime_incarnation", "policy_lineage", "portable_policy"] {
            wire["operator"].as_object_mut().unwrap().remove(field);
        }
        // The original format carried a root-bound activation, which the
        // reader accepts and ignores: the policy files are the policy.
        let activation_binding = crate::policy::fingerprint(
            json!([
                wire["project_id"],
                wire["project_root"],
                "acme-original-activation",
                null,
                wire["policy_revision"],
                "acme-candidate-digest"
            ])
            .to_string()
            .as_bytes(),
        );
        wire["active"] = json!({"activation_id":"acme-original-activation",
            "previous_activation_id":null,"policy_revision":wire["policy_revision"],
            "candidate_digest":"acme-candidate-digest","binding":activation_binding});
        wire["history_generation"] = Value::Null;
        let state_path = Path::new(&context.project_root).join(".shield/review/state.json");
        std::fs::write(&state_path, wire.to_string()).unwrap();
        let loaded = store.read_checkpoint().unwrap();
        validate(&loaded).unwrap();
        assert_eq!(loaded.project_id, context.project_id, "{status}");
        assert_eq!(loaded.policy_lineage(), context.project_id, "{status}");
        assert!(loaded.read_requests.is_empty(), "{status}");
        assert!(loaded.terminal_receipts.is_empty(), "{status}");
        loaded.check_operations().unwrap();
        let next = review::open(Path::new(&context.project_root)).unwrap();
        assert!(authorize(&next, &id, 300, None, false).is_err(), "{status}");
        review::runtime::consumer::load(&next, &|| Ok(())).unwrap();
        assert_eq!(
            store.read_checkpoint().unwrap().project_id,
            loaded.project_id,
            "ordinary reopen is stable"
        );
    }
}

#[test]
fn malformed_optional_terminal_outcomes_do_not_block_active_policy_admission() {
    let (_root, context, _, _) = fixture();
    let store = Store::open(Path::new(&context.project_root)).unwrap();
    let mut wire = json!(store.read_checkpoint().unwrap());
    let opaque = json!({"records":[{"unsupported":"terminal receipt"}]});
    wire["terminal_receipts"] = opaque.clone();
    let path = Path::new(&context.project_root).join(".shield/review/state.json");
    std::fs::write(&path, wire.to_string()).unwrap();
    let loaded = store.read_checkpoint().unwrap();
    assert_eq!(json!(loaded)["terminal_receipts"]["unavailable"], opaque);
    review::runtime::consumer::load(&context, &|| Ok(())).unwrap();
    store
        .update_checkpoint(&context, &|| Ok(()), |_| Ok(Value::Null))
        .unwrap();
    let saved: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(saved["terminal_receipts"]["unavailable"], opaque);
}

#[test]
fn legacy_read_rejects_corrupt_bindings_and_widened_resources() {
    let (_root, context, evidence, digest) = fixture();
    let id = pending(&context, &evidence, &digest);
    authorize(&context, &id, 300, None, false).unwrap();
    let state = Store::open(Path::new(&context.project_root))
        .unwrap()
        .read_checkpoint()
        .unwrap();
    let wire = historical_read_state(json!(state), false);
    for field in ["binding", "resource_digest"] {
        let mut corrupt = wire.clone();
        corrupt["read_requests"][0][field] = json!("0".repeat(64));
        assert!(validate(&serde_json::from_value::<State>(corrupt).unwrap()).is_err());
    }
    let mut corrupt = wire.clone();
    corrupt["read_requests"][0]["grant"]["binding"] = json!("0".repeat(64));
    assert!(validate(&serde_json::from_value::<State>(corrupt).unwrap()).is_err());
    let loaded: State = serde_json::from_value(wire).unwrap();
    let row = &loaded.read_requests[0];
    assert!(row.matches(&evidence, &digest, &row.origin));
    assert!(!row.matches(&evidence, "different-file-digest", &row.origin));
    let mut foreign = row.origin.clone();
    foreign.consumer_id = "different-factory".into();
    assert!(!row.matches(&evidence, &digest, &foreign));
    let snapshot = Project::open(Path::new(&context.project_root))
        .unwrap()
        .snapshot()
        .unwrap();
    let mut window = evidence.request().clone();
    window.arguments.extend(["--offset".into(), "1".into()]);
    let changed = snapshot
        .evaluate_resource(window, evidence.resource().unwrap().clone())
        .unwrap();
    assert!(!row.matches(&changed, &digest, &row.origin));
    for variant in ["window", "path", "environment", "effect"] {
        let mut resource = evidence.resource().unwrap().clone();
        match variant {
            "environment" => resource.context.environment = Some(std::collections::BTreeMap::new()),
            _ => {
                if let policy::normalized::Input::File {
                    action,
                    sources,
                    original_input,
                    ..
                } = &mut resource.input
                {
                    match variant {
                        "window" => original_input["limit"] = json!(1),
                        "path" => sources.push(
                            Path::new(&context.project_root)
                                .join("other.txt")
                                .to_str()
                                .unwrap()
                                .into(),
                        ),
                        "effect" => *action = "remove".into(),
                        _ => unreachable!(),
                    }
                }
            }
        }
        let changed = snapshot
            .evaluate_resource(evidence.request().clone(), resource)
            .unwrap();
        assert!(!row.matches(&changed, &digest, &row.origin), "{variant}");
    }
    for path in [
        "/acme/outside.txt".to_owned(),
        format!("{}/.shield/private.txt", context.project_root),
    ] {
        let mut request = evidence.request().clone();
        request.arguments = vec![path];
        assert!(legacy::resource(&request).is_err());
    }
}
