//! A declined offer stays terminal while an explicit fresh review preserves its exact scope.
use super::*;

fn retry(previous: &Value, key: &str) -> Value {
    json!({"proposal_id":previous["proposal_id"],"candidate_digest":previous["candidate_digest"],"idempotency_key":key})
}

#[test]
fn session_reconsideration_keeps_a_maximum_size_scope_reviewable() {
    let (_root, context, reference) = fixture();
    let mut exact = scope();
    exact[0]["conditions"]["arguments"] = json!({"kind":"exact","values":["x".repeat(4000)]});
    let mut second = exact[0].clone();
    second["conditions"]["arguments"] =
        json!({"kind":"exact","values":["y".repeat(4000),"z".repeat(1000)]});
    exact.as_array_mut().unwrap().push(second);
    let measured = propose(
        &context,
        &json!({"session_ref":reference,"scope":exact,"idempotency_key":"measure"}),
    )
    .unwrap();
    let padding = MAX_FORM - measured["review"].as_str().unwrap().len();
    assert!(1000 + padding <= 4096);
    exact[1]["conditions"]["arguments"]["values"][1] = json!("z".repeat(1000 + padding));
    let original = propose(
        &context,
        &json!({"session_ref":reference,"scope":exact,"idempotency_key":"maximum"}),
    )
    .unwrap();
    assert_eq!(original["review"].as_str().unwrap().len(), MAX_FORM);
    decline(&context, &original["apply_arguments"]).unwrap();
    let fresh = reconsider(&context, &retry(&original, "explicit")).unwrap();
    assert_eq!(
        fresh["reconsiders_candidate_digest"],
        original["candidate_digest"]
    );
    assert_eq!(fresh["scope"], original["scope"]);
    let form = confirmation(&context, &fresh["apply_arguments"])
        .unwrap()
        .unwrap();
    assert!(form.len() <= MAX_FORM);
    assert!(form.contains("Fresh review of declined proposal"));
}

#[test]
fn session_reconsideration_preserves_exact_deny_overrides_and_history_independence() {
    let rules = json!({"schema_version":1,"kind":"custom","capabilities":["structured-rules-v1","argument-rules-v1"],"rules":[
        {"id":"acme-merge","decision":"deny","reason":"Merges require review.","conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"any"}}}
    ]}).to_string();
    let (_root, context, reference) = fixture_with_rules(Some(&rules));
    let listed = super::super::super::inspection::rules(&context, None).unwrap();
    let rule = &listed["rules"][0];
    let original = propose(&context, &json!({"session_ref":reference,"idempotency_key":"override","scope":scope(),
        "duration_seconds":600,"override_denies":[{"rule_ref":rule["rule_ref"],"branch":"met","rule_digest":rule["rule_digest"]}]})).unwrap();
    decline(&context, &original["apply_arguments"]).unwrap();
    store(&context)
        .unwrap()
        .update_checkpoint(&context, &|| Ok(()), |state| {
            state.history_enabled = false;
            state.events.clear();
            Ok(json!({}))
        })
        .unwrap();
    let fresh = reconsider(&context, &retry(&original, "explicit-override")).unwrap();
    assert_eq!(fresh["override_denies"], original["override_denies"]);
    let form = confirmation(&context, &fresh["apply_arguments"])
        .unwrap()
        .unwrap();
    assert!(form.contains("OVERRIDES DENY") && form.contains("Merges require review."));
    apply(&context, &fresh["apply_arguments"], true).unwrap();
    for id in ["acme-merge", "another-rule"] {
        let mut request = receipt(&context, arguments("42"));
        request["decision"] = json!("deny");
        request["code"] = json!("policy-deny");
        request["_shell_analysis"]["operations"][0]["decision"] = json!("deny");
        request["_shell_analysis"]["operations"][0]["evaluation"]["evaluation"]["matches"] = json!([{"source":"custom","rule_id":id,"decision":"deny","applied":true,"branch":"met"}]);
        handoff(
            &context,
            &mut request,
            Some(&json!({"command":"acme merge"})),
        )
        .unwrap();
        assert_eq!(
            request["disposition"] == "allowed",
            id == "acme-merge",
            "{request}"
        );
    }
    assert_eq!(
        show(&context, &json!({"proposal_id":original["proposal_id"]})).unwrap()["proposal_status"],
        "declined"
    );
}

#[test]
fn session_reconsideration_metadata_work_stays_independent_of_policy_and_history() {
    use clap::Parser as _;
    let (_root, context, reference) = fixture();
    let original = proposal(&context, &reference);
    decline(&context, &original["apply_arguments"]).unwrap();
    let logs = assert_fs::TempDir::new().unwrap();
    let cli = crate::cli::Cli::parse_from([
        "shield",
        "--debug-log-dir",
        logs.path().to_str().unwrap(),
        "status",
    ]);
    let log = std::sync::Arc::new(crate::diagnostics::Log::open(&cli));
    for (age, retained) in [("fresh", 0), ("aged", 32)] {
        for index in 0..retained {
            let mut other = scope();
            other[0]["conditions"]["executable"] = json!(format!("acme-{index}"));
            let pending = propose(&context, &json!({"session_ref":reference,"scope":other,"idempotency_key":format!("unrelated-{index}")})).unwrap();
            decline(&context, &pending["apply_arguments"]).unwrap();
        }
        let trace = log
            .request("command", "session_grant_reconsider", None, None)
            .unwrap();
        let scope = trace.enter();
        let started = std::time::Instant::now();
        let fresh = reconsider(&context, &retry(&original, age)).unwrap();
        eprintln!(
            "session_reconsideration {age} elapsed_us={}",
            started.elapsed().as_micros()
        );
        for counter in ["policy.preparations", "source.hydrated", "history.captures"] {
            assert_eq!(trace.recorded_count(counter), 0, "{age} {counter}");
        }
        drop(scope);
        trace.finish("success");
        decline(&context, &fresh["apply_arguments"]).unwrap();
    }
}

#[test]
fn session_reconsideration_preserves_decline_and_requires_fresh_confirmation() {
    let (_root, context, reference) = fixture();
    let original = proposal(&context, &reference);
    decline(&context, &original["apply_arguments"]).unwrap();
    let before = store(&context).unwrap().read_checkpoint().unwrap();
    let old = serde_json::to_value(
        &before.session_grants.records[original["proposal_id"].as_str().unwrap()],
    )
    .unwrap();
    assert!(old.get("reconsiders").is_none());
    let args = retry(&original, "person-asked-again");
    let fresh = reconsider(&context, &args).unwrap();
    assert_ne!(fresh["proposal_id"], original["proposal_id"]);
    assert_ne!(fresh["candidate_digest"], original["candidate_digest"]);
    assert_eq!(fresh["reconsiders_proposal_id"], original["proposal_id"]);
    for field in ["scope", "duration_seconds", "override_denies", "target"] {
        assert_eq!(fresh[field], original[field], "{field}");
    }
    assert_eq!(fresh["proposal_status"], "pending");
    assert!(fresh["grant_id"].is_null());
    assert!(matches!(
        apply(&context, &fresh["apply_arguments"], false),
        Err(Error::ApprovalRequired)
    ));
    let mut request = receipt(&context, arguments("42"));
    handoff(
        &context,
        &mut request,
        Some(&json!({"command":"acme merge"})),
    )
    .unwrap();
    assert_eq!(request["disposition"], "approval_required");
    let form = confirmation(&context, &fresh["apply_arguments"])
        .unwrap()
        .unwrap();
    assert!(
        form.contains("Fresh review of declined proposal")
            && form.contains("github.example.com/acme/app")
    );
    let issued = apply(&context, &fresh["apply_arguments"], true).unwrap();
    for pr in ["42", "43"] {
        let mut request = receipt(&context, arguments(pr));
        handoff(
            &context,
            &mut request,
            Some(&json!({"command":"acme merge"})),
        )
        .unwrap();
        assert_eq!(request["disposition"], "allowed");
    }
    let mut other = context.clone();
    other.session_id = Some("another-session".into());
    let mut request = receipt(&other, arguments("42"));
    handoff(&other, &mut request, Some(&json!({"command":"acme merge"}))).unwrap();
    assert_eq!(request["disposition"], "approval_required");
    assert_eq!(
        reconsider(&context, &args).unwrap()["grant_id"],
        issued["grant_id"]
    );
    let new_key = reconsider(&context, &retry(&original, "another-explicit-retry")).unwrap();
    assert_eq!(new_key["grant_id"], issued["grant_id"]);
    assert_eq!(new_key["expires_at_ms"], issued["expires_at_ms"]);
    let automatic = propose(
        &context,
        &json!({"session_ref":reference,"scope":scope(),"idempotency_key":"automatic-new-key"}),
    )
    .unwrap();
    assert_eq!(automatic["proposal_id"], fresh["proposal_id"]);
    let after = store(&context).unwrap().read_checkpoint().unwrap();
    assert_eq!(
        serde_json::to_value(
            &after.session_grants.records[original["proposal_id"].as_str().unwrap()]
        )
        .unwrap(),
        old
    );
    assert_eq!(before.policy_revision, after.policy_revision);
    assert_eq!(
        before.active.as_ref().unwrap().activation_id(),
        after.active.as_ref().unwrap().activation_id()
    );
}

#[test]
fn session_reconsideration_decline_suppresses_ordinary_and_same_key_retries() {
    let (_root, context, reference) = fixture();
    let original = proposal(&context, &reference);
    decline(&context, &original["apply_arguments"]).unwrap();
    let args = retry(&original, "explicit-1");
    let fresh = reconsider(&context, &args).unwrap();
    assert_eq!(
        reconsider(&context, &retry(&original, "duplicate-key")).unwrap()["proposal_id"],
        fresh["proposal_id"]
    );
    decline(&context, &fresh["apply_arguments"]).unwrap();
    let again = reconsider(&context, &args).unwrap();
    assert_eq!(again["proposal_id"], fresh["proposal_id"]);
    assert_eq!(again["proposal_status"], "declined");
    let automatic = propose(
        &context,
        &json!({"session_ref":reference,"scope":scope(),"idempotency_key":"new-automatic-key"}),
    )
    .unwrap();
    assert_eq!(automatic["proposal_status"], "declined");
    for row in [&original, &fresh] {
        assert!(confirmation(&context, &row["apply_arguments"]).is_err());
        assert!(apply(&context, &row["apply_arguments"], true).is_err());
    }
    let explicit = reconsider(&context, &retry(&fresh, "explicit-2")).unwrap();
    assert_eq!(explicit["reconsiders_proposal_id"], fresh["proposal_id"]);
    assert_eq!(explicit["proposal_status"], "pending");
    assert!(explicit["grant_id"].is_null());
}

#[test]
fn session_reconsideration_rejects_foreign_changed_and_unavailable_sources() {
    for mutation in [
        "pending",
        "agent",
        "session",
        "digest",
        "scope",
        "duration",
        "policy",
        "protection",
        "registration",
        "context",
    ] {
        let (_root, mut context, reference) = fixture();
        let original = proposal(&context, &reference);
        let mut args = retry(&original, "explicit");
        if mutation != "pending" {
            decline(&context, &original["apply_arguments"]).unwrap();
        }
        match mutation {
            "agent" => context.agent = Some("claude".into()),
            "session" => context.session_id = Some("other-session".into()),
            "digest" => args["candidate_digest"] = json!("wrong"),
            "scope" => args["scope"] = scope(),
            "duration" => args["duration_seconds"] = json!(60),
            "policy" | "protection" | "registration" | "context" => {
                store(&context)
                    .unwrap()
                    .update_checkpoint(&context, &|| Ok(()), |state| {
                        match mutation {
                            "policy" => state.policy_revision = "changed".into(),
                            "protection" => {
                                state.operator["path_access"] =
                                    json!({"agent_settings_read":"allow"});
                            }
                            "registration" => {
                                state.operator["integration"]["binary"] =
                                    json!("/fixture/replaced-shield");
                            }
                            "context" => state.session_grants.contexts.clear(),
                            _ => unreachable!(),
                        }
                        Ok(json!({}))
                    })
                    .unwrap();
            }
            _ => {}
        }
        assert!(reconsider(&context, &args).is_err(), "{mutation}");
        assert_eq!(
            store(&context)
                .unwrap()
                .read_checkpoint()
                .unwrap()
                .session_grants
                .records
                .len(),
            1
        );
    }
}
