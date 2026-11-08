//! Real targeted maintenance preserves immutable authorization for other consumers.
use super::*;
use crate::review::integrations::{self, IntegrationChoice};

fn setup(context: &Context, choice: IntegrationChoice) {
    let state = store(context).unwrap().read_checkpoint().unwrap();
    let metadata = crate::review::project::metadata(&state);
    let plan = integrations::setup::prepare(Path::new(&context.project_root), &metadata, choice)
        .unwrap()
        .unwrap();
    integrations::apply(context, &metadata, &plan).unwrap();
}

fn captured(context: &Context) -> String {
    let mut receipt = json!({"code":"policy-allow","decision":"allow","disposition":"allowed"});
    handoff(context, &mut receipt, None).unwrap();
    receipt["session_context"]["session_ref"]
        .as_str()
        .unwrap()
        .into()
}

fn prepared() -> (assert_fs::TempDir, Context, String, Value, Value) {
    let (root, context, _) = fixture_for_agent("codex");
    // Replace the lightweight native matching fixture with a real direct setup
    // before capturing the authorization whose maintenance lifetime we test.
    store(&context)
        .unwrap()
        .update_checkpoint(&context, &|| Ok(()), |state| {
            state.operator["integration"] = Value::Null;
            state.session_grants = Registry::default();
            Ok(json!({}))
        })
        .unwrap();
    setup(&context, IntegrationChoice::agent("codex"));
    let reference = captured(&context);
    let proposal = proposal(&context, &reference);
    let granted = apply(&context, &proposal["apply_arguments"], true).unwrap();
    let mut alternative = scope();
    alternative[0]["conditions"]["executable"] = json!("acme-inspect");
    let pending = propose(
        &context,
        &json!({"session_ref":reference,
        "idempotency_key":"maintenance-pending","scope":alternative}),
    )
    .unwrap();
    (root, context, reference, granted, pending)
}

fn immutable(context: &Context) -> Value {
    let state = store(context).unwrap().read_checkpoint().unwrap();
    json!({"records":state.session_grants.records,"contexts":state.session_grants.contexts})
}

fn usable(context: &Context, reference: &str, granted: &Value, pending: &Value) {
    assert_eq!(captured(context), reference);
    let mut request = receipt(context, arguments("42"));
    handoff(
        context,
        &mut request,
        Some(&json!({"command":"acme merge"})),
    )
    .unwrap();
    assert_eq!(
        request["authorization"]["kind"], "session_grant",
        "{request}"
    );
    assert_eq!(
        request["authorization"]["grants"][0]["grant_id"],
        granted["grant_id"]
    );
    let shown = show(context, &json!({"proposal_id":pending["proposal_id"]})).unwrap();
    assert_eq!(shown["proposal_status"], "pending", "{shown}");
    assert!(
        confirmation(context, &pending["apply_arguments"])
            .unwrap()
            .is_some()
    );
}

fn historical_fixture(context: &Context) -> (Value, Value) {
    store(context)
        .unwrap()
        .update_checkpoint(context, &|| Ok(()), |state| {
            let registration = integrations::registration(&state.operator)?.unwrap();
            let legacy = hash(&json!(registration));
            for session in state.session_grants.contexts.values_mut() {
                session.registration.clone_from(&legacy);
            }
            for row in state.session_grants.records.values_mut() {
                row.session.registration.clone_from(&legacy);
                row.digest = row.digest();
                if let Some(grant) = &mut row.grant {
                    grant.binding = hash(&json!([
                        row.digest,
                        grant.id,
                        grant.approval,
                        grant.issued,
                        grant.expires,
                        grant.clock,
                        grant.mechanism
                    ]));
                }
            }
            Ok(json!({}))
        })
        .unwrap();
    let state = store(context).unwrap().read_checkpoint().unwrap();
    let now = crate::review::clock::now_ms().unwrap();
    let issued = state
        .session_grants
        .records
        .values()
        .find(|row| row.grant.is_some())
        .unwrap();
    let pending = state
        .session_grants
        .records
        .values()
        .find(|row| row.grant.is_none())
        .unwrap();
    (
        view::public(issued, &state, now),
        view::public(pending, &state, now),
    )
}

#[test]
fn claude_maintenance_preserves_codex_grant_candidate_and_native_reference_without_reissue() {
    for legacy in [false, true] {
        let (_root, context, reference, granted, pending) = prepared();
        let (granted, pending) = if legacy {
            historical_fixture(&context)
        } else {
            (granted, pending)
        };
        let original = immutable(&context);
        usable(&context, &reference, &granted, &pending);
        for choice in [
            IntegrationChoice::agent("claude"),
            IntegrationChoice::RemoveAgents(IntegrationChoice::bits(&["claude"]).unwrap()),
        ] {
            setup(&context, choice);
            usable(&context, &reference, &granted, &pending);
            assert_eq!(
                immutable(&context),
                original,
                "maintenance must not rewrite candidate digests or grant identity/timing/revocation"
            );
        }
    }
}

#[test]
fn advisory_binary_version_and_digest_changes_preserve_codex_authorization() {
    for legacy in [false, true] {
        let (_root, context, reference, granted, pending) = prepared();
        let (granted, pending) = if legacy {
            historical_fixture(&context)
        } else {
            (granted, pending)
        };
        // A real unrelated maintenance transaction records compatibility for an
        // authentic old full-registration hash without rewriting its evidence.
        setup(&context, IntegrationChoice::agent("claude"));
        let original = immutable(&context);
        store(&context)
            .unwrap()
            .update_checkpoint(&context, &|| Ok(()), |state| {
                state.operator["integration"]["version"] = json!("0.0.0-advisory");
                state.operator["integration"]["binary_digest"] = json!("b".repeat(64));
                Ok(json!({}))
            })
            .unwrap();
        usable(&context, &reference, &granted, &pending);
        assert_eq!(immutable(&context), original);
    }
}

#[test]
fn selected_registration_removal_stales_grants_and_candidates_permanently() {
    for legacy in [false, true] {
        let (_root, context, _reference, granted, pending) = prepared();
        let (granted, pending) = if legacy {
            historical_fixture(&context)
        } else {
            (granted, pending)
        };
        let original =
            store(&context).unwrap().read_checkpoint().unwrap().operator["integration"].clone();
        setup(
            &context,
            IntegrationChoice::RemoveAgents(IntegrationChoice::bits(&["codex"]).unwrap()),
        );
        store(&context)
            .unwrap()
            .update_checkpoint(&context, &|| Ok(()), |state| {
                state.operator["integration"] = original.clone();
                Ok(json!({}))
            })
            .unwrap();
        let issued = show(&context, &json!({"grant_id":granted["grant_id"]})).unwrap();
        assert_eq!(issued["grant_status"], "stale", "{issued}");
        let candidate = show(&context, &json!({"proposal_id":pending["proposal_id"]})).unwrap();
        assert_eq!(candidate["proposal_status"], "stale", "{candidate}");
        assert!(matches!(
            apply(&context, &pending["apply_arguments"], true),
            Err(Error::StaleSnapshot)
        ));
        let mut request = receipt(&context, arguments("42"));
        handoff(
            &context,
            &mut request,
            Some(&json!({"command":"acme merge"})),
        )
        .unwrap();
        assert!(request["authorization"].is_null(), "{request}");
    }
}

#[test]
fn legacy_declined_offer_stays_suppressed_and_preserved_session_can_prepare_fresh_scope() {
    let (_root, context, reference, _, _) = prepared();
    let (_, pending) = historical_fixture(&context);
    decline(&context, &pending["apply_arguments"]).unwrap();
    let before = immutable(&context);
    setup(&context, IntegrationChoice::agent("claude"));
    assert_eq!(captured(&context), reference);
    let suppressed = propose(
        &context,
        &json!({"session_ref":reference,"idempotency_key":"retry-declined-after-maintenance",
            "scope":pending["scope"]}),
    )
    .unwrap();
    assert_eq!(suppressed["proposal_id"], pending["proposal_id"]);
    assert_eq!(suppressed["candidate_digest"], pending["candidate_digest"]);
    assert_eq!(suppressed["proposal_status"], "declined", "{suppressed}");
    assert_eq!(immutable(&context), before);

    let mut fresh_scope = scope();
    fresh_scope[0]["conditions"]["executable"] = json!("acme-fresh-maintenance");
    let fresh = propose(
        &context,
        &json!({"session_ref":reference,"idempotency_key":"fresh-scope-after-maintenance",
            "scope":fresh_scope}),
    )
    .unwrap();
    assert_ne!(fresh["proposal_id"], pending["proposal_id"]);
    assert_eq!(fresh["session_ref"], reference);
    assert_eq!(fresh["proposal_status"], "pending", "{fresh}");
    assert!(
        confirmation(&context, &fresh["apply_arguments"])
            .unwrap()
            .is_some()
    );
    assert!(matches!(
        apply(&context, &fresh["apply_arguments"], false),
        Err(Error::ApprovalRequired)
    ));
    let issued = apply(&context, &fresh["apply_arguments"], true).unwrap();
    assert_eq!(issued["grant_status"], "active", "{issued}");
}
