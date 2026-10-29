//! Native resources and real store transitions, including failed launch handoffs.
use super::*;
use crate::{review, runtime::Project};
use assert_fs::prelude::*;

fn fixture() -> (assert_fs::TempDir, Context, EvaluatedRequest, Value) {
    let (root, mut context) = review::tests::enrolled();
    let status = review::runtime::status(&context).expect("native approval fixture");
    review::runtime::publish_fixture(
        &context,
        status["review"]["policy_revision"]
            .as_str()
            .expect("native approval fixture"),
        status["review"]["candidate_digest"]
            .as_str()
            .expect("native approval fixture"),
        None,
        &|| Ok(()),
    )
    .expect("native approval fixture");
    context.surface = "run".into();
    context.cwd = Some(context.project_root.clone());
    let request = Request {
        executable: "/bin/echo".into(),
        arguments: vec!["Acme text".into()],
        cwd: context.project_root.clone(),
        project: context.project_root.clone(),
    };
    let resource =
        crate::execution::approval_fixture(&request, 5000).expect("native approval fixture");
    let evidence = Project::open(Path::new(&context.project_root))
        .expect("native approval fixture")
        .snapshot()
        .expect("native approval fixture")
        .evaluate(request)
        .expect("native approval fixture");
    (root, context, evidence, resource)
}
fn operator(context: &Context) -> Context {
    let mut operator = context.clone();
    operator.surface = "cli".into();
    operator
}
fn pending(context: &Context, evidence: &EvaluatedRequest, resource: &Value) -> String {
    let (value, outcome) = gate(
        context,
        evidence,
        resource,
        None,
        || true,
        |_| panic!("Ask cannot reach launch callback"),
    )
    .expect("native approval fixture");
    assert!(outcome.is_none());
    assert_eq!(value["code"], "approval-required");
    value["approval"]["request_id"]
        .as_str()
        .expect("native approval fixture")
        .into()
}

#[test]
fn shown_requests_carry_their_creation_time_and_policy_basis_without_operands() {
    let (_root, context, evidence, resource) = fixture();
    let id = pending(&context, &evidence, &resource);
    let (read, shown) = review::approvals::request_view(&context, &id).unwrap();
    assert!(!read);
    assert!(shown["created_at_ms"].as_u64().is_some(), "{shown}");
    let policy = &shown["policy"];
    assert_eq!(policy["decision"], "ask", "{shown}");
    assert!(
        policy["reason"].as_str().is_some_and(|r| !r.is_empty()),
        "{shown}"
    );
    assert!(policy["rules"].as_array().unwrap().len() <= 3, "{shown}");
    assert!(!policy.to_string().contains("Acme text"), "{policy}");
    let listed = review::approvals::inspect(&context, None).unwrap();
    assert_eq!(listed["requests"][0]["policy"], *policy, "{listed}");
    // The summary is review context: the stored binding still validates.
    authorize(&operator(&context), &id, 300, None, false).unwrap();
}

#[test]
fn approval_replay_uses_grant_expiry_after_pending_window_closes() {
    let (_root, context, evidence, resource) = fixture();
    let id = pending(&context, &evidence, &resource);
    let operator = operator(&context);
    let issued = authorize(&operator, &id, 3600, None, false).unwrap();
    let store = Store::open(Path::new(&context.project_root)).unwrap();
    let state = store.read().unwrap();
    let row = &state.run_requests[0];
    let time = row.pending_expires_at_ms;
    let grant = row.grant.as_ref().unwrap();
    let (start, end) = (grant.issued_at_ms, grant.expires_at_ms);
    let events = state.authorization_events.len();
    assert_eq!(
        authorize_with_clock(&operator, &id, 3600, None, false, &|| Ok(time)).unwrap(),
        issued
    );
    for (time, ttl) in [(time, 300), (start - 1, 3600), (end, 3600)] {
        assert!(authorize_with_clock(&operator, &id, ttl, None, false, &|| Ok(time)).is_err());
    }
    let mut state = store.read().unwrap();
    assert_eq!(state.authorization_events.len(), events);
    let origin = state.run_requests[0].origin.clone();
    assert_eq!(
        consume(
            &mut state, &context, &origin, &evidence, &resource, &id, time
        )
        .unwrap()["code"],
        "allowed-once"
    );
    assert_ne!(
        consume(
            &mut state, &context, &origin, &evidence, &resource, &id, time
        )
        .unwrap()["code"],
        "allowed-once"
    );
}

#[test]
fn one_atomic_consumer_survives_history_purge_and_failed_launch() {
    let (_root, context, evidence, resource) = fixture();
    review::history_settings(&operator(&context), Some(false), None, true, &|| Ok(()))
        .expect("native approval fixture");
    let id = pending(&context, &evidence, &resource);
    assert_eq!(pending(&context, &evidence, &resource), id);
    assert!(authorize(&context, &id, 300, None, false).is_err());
    let issued = authorize(&operator(&context), &id, 300, Some("Acme review"), false)
        .expect("native approval fixture");
    assert_eq!(
        authorize(&operator(&context), &id, 300, None, false).expect("native approval fixture"),
        issued
    );
    assert!(authorize(&operator(&context), &id, 301, None, false).is_err());
    let outcomes = std::thread::scope(|scope| {
        let handles = (0..4)
            .map(|_| {
                scope.spawn(|| {
                    review::tests::retry_locked(|| {
                        gate(
                            &context,
                            &evidence,
                            &resource,
                            Some(&id),
                            || true,
                            |decision| {
                                if decision["allowed"] == true {
                                    let store = Store::open(Path::new(&context.project_root))
                                        .expect("native approval fixture");
                                    assert_eq!(
                                        store.read().expect("native approval fixture").run_requests
                                            [0]
                                        .status,
                                        "consumed"
                                    );
                                    // Persistence precedes host failure, cancellation or loss of response.
                                    Err("launch failed")
                                } else {
                                    Ok(())
                                }
                            },
                        )
                    })
                })
            })
            .collect::<Vec<_>>();
        handles
            .into_iter()
            .map(|h| h.join().expect("native approval fixture"))
            .collect::<Vec<_>>()
    });
    assert_eq!(
        outcomes
            .iter()
            .filter(
                |(v, result)| v["allowed"] == true && result.as_ref().is_some_and(Result::is_err)
            )
            .count(),
        1
    );
    let store = Store::open(Path::new(&context.project_root)).expect("native approval fixture");
    let mut consumed = store.read().expect("native approval fixture");
    assert_eq!(
        consumed
            .authorization_events
            .iter()
            .filter(|e| e["kind"] == "run_grant_consumed")
            .count(),
        1
    );
    consumed.run_requests[0].status = "approved".into();
    assert!(validate(&consumed).is_err());
    review::history_settings(&operator(&context), None, None, true, &|| Ok(()))
        .expect("native approval fixture");
    assert_eq!(
        gate(&context, &evidence, &resource, Some(&id), || true, |_| ())
            .expect("native approval fixture")
            .0["code"],
        "approval-unavailable"
    );
    assert!(authorize(&operator(&context), &id, 300, None, false).is_err());
    let state = store.read().expect("native approval fixture");
    assert_eq!(state.events, [] as [Value; 0]);
    assert!(state.run_requests.is_empty());
}

#[test]
fn changed_request_origin_resources_deadline_and_activation_block() {
    let (_root, context, evidence, resource) = fixture();
    let id = pending(&context, &evidence, &resource);
    authorize(&operator(&context), &id, 300, None, false).expect("native approval fixture");
    let mut changed = context.clone();
    changed.session_id = Some("other-session".into());
    assert_eq!(
        gate(&changed, &evidence, &resource, Some(&id), || true, |_| ())
            .expect("native approval fixture")
            .0["code"],
        "approval-stale"
    );
    changed = context.clone();
    changed.tool_use_id = Some("other-tool".into());
    assert_eq!(
        gate(&changed, &evidence, &resource, Some(&id), || true, |_| ())
            .expect("native approval fixture")
            .0["code"],
        "approval-stale"
    );
    changed = context.clone();
    changed.agent = Some("other-agent".into());
    assert_eq!(
        gate(&changed, &evidence, &resource, Some(&id), || true, |_| ())
            .expect("native approval fixture")
            .0["code"],
        "approval-stale"
    );
    for (key, value) in [
        ("timeout_ms", json!(5001)),
        ("resources", {
            let mut resources = resource["resources"].clone();
            resources["loader_identity"][1] = json!(42);
            resources
        }),
    ] {
        let mut altered = resource.clone();
        altered[key] = value;
        assert_eq!(
            gate(&context, &evidence, &altered, Some(&id), || true, |_| ())
                .expect("native approval fixture")
                .0["code"],
            "approval-stale"
        );
    }
    let project = Project::open(Path::new(&context.project_root)).expect("native approval fixture");
    let mut request = evidence.request().clone();
    request.arguments.push("extra".into());
    let altered = project
        .snapshot()
        .expect("native approval fixture")
        .evaluate(request)
        .expect("native approval fixture");
    assert_eq!(
        gate(&context, &altered, &resource, Some(&id), || true, |_| ())
            .expect("native approval fixture")
            .0["code"],
        "approval-stale"
    );
    assert!(
        gate(
            &context,
            &evidence,
            &resource,
            Some(&id),
            || false,
            |_| panic!("no callback before verification")
        )
        .is_err()
    );
}

#[test]
fn same_policy_reactivation_invalidates_grants_and_owned_evidence() {
    let (_root, context, evidence, resource) = fixture();
    let id = pending(&context, &evidence, &resource);
    authorize(&operator(&context), &id, 300, None, false).expect("native approval fixture");
    let project = Project::open(Path::new(&context.project_root)).expect("native approval fixture");
    let status = review::runtime::status(&operator(&context)).expect("native approval fixture");
    review::runtime::publish_fixture(
        &operator(&context),
        status["review"]["policy_revision"]
            .as_str()
            .expect("native approval fixture"),
        status["review"]["candidate_digest"]
            .as_str()
            .expect("native approval fixture"),
        status["active"]["activation_id"].as_str(),
        &|| Ok(()),
    )
    .expect("native approval fixture");
    assert!(
        gate(
            &context,
            &evidence,
            &resource,
            Some(&id),
            || true,
            |_| panic!("stale evidence")
        )
        .is_err()
    );
    let refreshed = project
        .snapshot()
        .expect("native approval fixture")
        .evaluate(evidence.request().clone())
        .expect("native approval fixture");
    assert_eq!(
        gate(&context, &refreshed, &resource, Some(&id), || true, |_| ())
            .expect("native approval fixture")
            .0["code"],
        "approval-stale"
    );
    assert!(authorize(&operator(&context), &id, 300, None, false).is_err());
}

#[test]
fn revocation_expiry_missing_and_pending_limits_fail_closed() {
    let (_root, context, evidence, resource) = fixture();
    let id = pending(&context, &evidence, &resource);
    authorize(&operator(&context), &id, 300, None, true).expect("native approval fixture");
    assert_eq!(
        gate(&context, &evidence, &resource, Some(&id), || true, |_| ())
            .expect("native approval fixture")
            .0["code"],
        "approval-unavailable"
    );
    assert!(
        gate(
            &context,
            &evidence,
            &resource,
            Some("read-request-foreign"),
            || true,
            |_| panic!("foreign grant")
        )
        .is_err()
    );
    let id = pending(&context, &evidence, &resource);
    authorize(&operator(&context), &id, 1, None, false).expect("native approval fixture");
    std::thread::sleep(std::time::Duration::from_millis(1100));
    assert_eq!(
        gate(&context, &evidence, &resource, Some(&id), || true, |_| ())
            .expect("native approval fixture")
            .0["code"],
        "approval-expired"
    );
    assert!(authorize(&operator(&context), &id, 1, None, false).is_err());
    pending(&context, &evidence, &resource);
    let mut state = Store::open(Path::new(&context.project_root))
        .expect("native approval fixture")
        .read()
        .expect("native approval fixture");
    let origin = state.run_requests[0].origin.clone();
    let time = now().expect("native approval fixture");
    while state.run_requests.len() < 128 {
        let mut altered = resource.clone();
        altered["timeout_ms"] = json!(state.run_requests.len() + 1);
        capture(&mut state, &context, &origin, &evidence, &altered, time)
            .expect("native approval fixture");
    }
    let mut altered = resource.clone();
    altered["timeout_ms"] = json!(1000);
    let refused = capture(&mut state, &context, &origin, &evidence, &altered, time);
    let pending = state
        .run_requests
        .iter()
        .filter(|r| r.status == "pending")
        .count();
    assert!(
        matches!(
            refused,
            Err(Error::Capacity {
                kind: review::CapacityKind::RunRequests,
                pending: counted,
                retained: 128,
                limit: 128,
            }) if counted == pending
        ),
        "{refused:?}"
    );
    let row = &state.run_requests[2];
    let expired = row.pending_expires_at_ms;
    let reference = row.request_id.clone();
    let resource = row.resource.clone();
    assert_eq!(
        consume(
            &mut state, &context, &origin, &evidence, &resource, &reference, expired
        )
        .expect("native approval fixture")["code"],
        "approval-expired"
    );
}

#[test]
fn publication_failure_withholds_callback_and_release_failure_keeps_consumption() {
    let (_root, context, evidence, resource) = fixture();
    let id = pending(&context, &evidence, &resource);
    authorize(&operator(&context), &id, 300, None, false).expect("native approval fixture");
    let store = Store::open(Path::new(&context.project_root)).expect("native approval fixture");
    store
        .update(&context, &|| Ok(()), |state| {
            let activation = state
                .active
                .as_ref()
                .expect("active fixture")
                .activation_id();
            let filler =
                json!({"kind":"acme-capacity-fixture","links":{"activation_id":activation}});
            state.authorization_events.resize(1024, filler);
            Ok(json!({}))
        })
        .expect("fill authority journal");
    assert!(
        gate(
            &context,
            &evidence,
            &resource,
            Some(&id),
            || true,
            |_| panic!("publication must precede launch")
        )
        .is_err()
    );
    assert_eq!(
        store.read().expect("native approval fixture").run_requests[0].status,
        "approved"
    );
    store
        .update(&context, &|| Ok(()), |state| {
            state
                .authorization_events
                .retain(|e| e["kind"] != "acme-capacity-fixture");
            Ok(json!({}))
        })
        .expect("restore owned fixture capacity");
    let effect = Path::new(&context.project_root).join("handoff-effect");
    assert!(
        gate(
            &context,
            &evidence,
            &resource,
            Some(&id),
            || true,
            |decision| {
                assert_eq!(decision["allowed"], true);
                std::fs::write(&effect, b"observed effect").expect("owned effect");
                std::fs::remove_file(
                    Path::new(&context.project_root).join(".shield/review/state.lock"),
                )
                .expect("simulate release failure");
            }
        )
        .is_err()
    );
    assert!(effect.exists());
    assert_eq!(
        store.read().expect("native approval fixture").run_requests[0].status,
        "consumed"
    );
    assert_eq!(
        gate(&context, &evidence, &resource, Some(&id), || true, |_| ())
            .expect("native approval fixture")
            .0["code"],
        "approval-consumed"
    );
}

#[test]
fn cwd_replacement_and_unverified_environment_refuse_issuance_and_consumption() {
    let (root, mut context, evidence, _) = fixture();
    root.child("nested").create_dir_all().expect("owned cwd");
    let mut request = evidence.request().clone();
    request.cwd = Path::new(&context.project_root)
        .join("nested")
        .to_str()
        .expect("cwd")
        .into();
    context.cwd = Some(request.cwd.clone());
    let evidence = Project::open(root.path())
        .expect("native project")
        .snapshot()
        .expect("snapshot")
        .evaluate(request)
        .expect("evidence");
    let resource =
        crate::execution::approval_fixture(evidence.request(), 5000).expect("native binding");
    let id = pending(&context, &evidence, &resource);
    authorize(&operator(&context), &id, 300, None, false).expect("approve owned cwd");
    std::fs::rename(root.child("nested").path(), root.child("old-nested").path())
        .expect("replace owned cwd");
    root.child("nested")
        .create_dir_all()
        .expect("replacement cwd");
    assert!(authorize(&operator(&context), &id, 300, None, false).is_err());
    assert!(
        gate(
            &context,
            &evidence,
            &resource,
            Some(&id),
            || crate::execution::verify_approval_binding(evidence.request(), &resource).is_ok(),
            |_| panic!("changed cwd")
        )
        .is_err()
    );
    assert_eq!(
        inspect(&operator(&context), Some(&id)).expect("inspect")["status"],
        "approved"
    );
    let mut altered = resource.clone();
    altered["resources"]["environment"] = json!("inherit");
    assert!(
        gate(
            &context,
            &evidence,
            &altered,
            None,
            || true,
            |_| panic!("unknown environment")
        )
        .is_err()
    );
    let mut shell = evidence.request().clone();
    shell.executable = "/bin/sh".into();
    let shell = Project::open(root.path())
        .expect("project")
        .snapshot()
        .expect("snapshot")
        .evaluate(shell)
        .expect("shell preview");
    assert!(
        gate(
            &context,
            &shell,
            &resource,
            None,
            || true,
            |_| panic!("shell binding unavailable")
        )
        .is_err()
    );
}

#[test]
fn original_operator_context_is_bound_and_cannot_be_substituted_on_retry() {
    let (_root, context, evidence, resource) = fixture();
    let id = pending(&context, &evidence, &resource);
    let mut issuer = operator(&context);
    issuer.agent = Some("acme-reviewer".into());
    let grant = authorize(&issuer, &id, 300, None, false).expect("issue grant");
    let store = Store::open(Path::new(&context.project_root)).expect("native approval fixture");
    let original = store.read().expect("native approval fixture").run_requests[0]
        .grant
        .as_ref()
        .expect("grant")
        .operator_provenance
        .clone();
    assert_eq!(original, issuer.provenance());
    issuer.agent = Some("other-reviewer".into());
    assert_eq!(
        authorize(&issuer, &id, 300, None, false).expect("idempotent grant"),
        grant
    );
    let mut state = store.read().expect("native approval fixture");
    let row = &mut state.run_requests[0];
    row.grant.as_mut().expect("grant").operator_provenance = issuer.provenance();
    let changed = row.grant_binding(row.grant.as_ref().expect("grant"));
    row.grant.as_mut().expect("grant").binding = changed;
    assert!(validate(&state).is_err());
}
