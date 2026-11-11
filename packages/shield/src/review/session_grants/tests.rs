//! Session authorization through real protected storage, scopes and native matching.
use super::*;
mod maintenance;
mod reconsideration;

pub(crate) fn fixture() -> (assert_fs::TempDir, Context, String) {
    fixture_with_rules(None)
}

pub(crate) fn fixture_for_agent(agent: &str) -> (assert_fs::TempDir, Context, String) {
    let (root, mut context, reference) = fixture();
    if agent == "codex" {
        return (root, context, reference);
    }
    context.agent = Some(agent.into());
    if ["gemini", "agy"].contains(&agent) {
        store(&context)
            .unwrap()
            .update_checkpoint(&context, &|| Ok(()), |state| {
                state.operator["integration"]["agents"][agent] =
                    json!({"groups":{},"mcp":null,"settings":null});
                Ok(json!({}))
            })
            .unwrap();
    }
    context.surface = "hook".into();
    let mut receipt = json!({"code":"policy-allow","decision":"allow","disposition":"allowed"});
    handoff(&context, &mut receipt, None).unwrap();
    let reference = receipt["session_context"]["session_ref"]
        .as_str()
        .unwrap()
        .to_owned();
    context.surface = "mcp".into();
    (root, context, reference)
}

fn fixture_with_rules(rules: Option<&str>) -> (assert_fs::TempDir, Context, String) {
    let root = assert_fs::TempDir::new().unwrap();
    let bundle = crate::configuration::Bundle::capture(
        if rules.is_some() {
            "schema_version: 1\npacks: []\nrules: rules.yml\n"
        } else {
            "schema_version: 1\npacks: []\n"
        },
        &[],
        rules,
    )
    .unwrap();
    let mut context = super::super::mcp::initialize_fixture(root.path(), &bundle);
    context.agent = Some("codex".into());
    context.session_id = Some("acme-session".into());
    context.source = "host".into();
    context.surface = "hook".into();
    store(&context).unwrap().update_checkpoint(&context,&||Ok(()),|state| {
        state.operator["integration"]=json!({"binary":"/fixture/shield","binary_digest":"a".repeat(64),"version":"0.20.0","entry":{},"agents":{},"ask_fallback":{},"box_definition":{"shared":"fixture","local":"fixture"}});
        Ok(json!({}))
    }).unwrap();
    let mut receipt = json!({"code":"policy-allow","decision":"allow","disposition":"allowed"});
    handoff(
        &context,
        &mut receipt,
        Some(&json!({"command":"acme read"})),
    )
    .unwrap();
    let reference = receipt["session_context"]["session_ref"]
        .as_str()
        .unwrap()
        .to_owned();
    context.surface = "mcp".into();
    context.connection_id = Some("acme-connection".into());
    (root, context, reference)
}

fn scope() -> Value {
    json!([{"conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"options","prefix":["merge"],"flags":["--squash"],"values":[{"name":"--repo","match":{"exact":"github.example.com/acme/app"},"repeat":"forbid"},{"name":"--head","match":{"regex":"[a-f0-9]{40}"},"repeat":"forbid"}],"require_all":["--squash","--repo","--head"],"operands":{"min":1,"max":1,"match":{"regex":"[1-9][0-9]*"}}},"cwd":{"within":{"root":"project"}}}}])
}

fn proposal(context: &Context, reference: &str) -> Value {
    propose(
        context,
        &json!({"session_ref":reference,"idempotency_key":"batch-1","scope":scope()}),
    )
    .unwrap()
}

fn receipt(context: &Context, arguments: Vec<String>) -> Value {
    let arguments = Value::Array(arguments.into_iter().map(Value::String).collect());
    let state = store(context).unwrap().read_checkpoint().unwrap();
    json!({"identity":{"activation_id":state.active.as_ref().unwrap().activation_id(),"policy_revision":state.policy_revision},
        "code":"approval-required","decision":"ask","disposition":"approval_required",
        "_observation":{"evaluation":{"evaluation":{"matches":[]}}},
        "_shell_analysis":{"coverage":"single-literal-invocation","operations":[{"source_unit":0,"span":{"start":0,"end":10},"coverage":"literal-argv","decision":"ask","request":{"executable":"acme","arguments":arguments,"cwd":context.project_root,"project":context.project_root},"evaluation":{"decision":"ask","evaluation":{"matches":[]}}}]}})
}
fn arguments(pr: &str) -> Vec<String> {
    [
        "merge",
        pr,
        "--repo",
        "github.example.com/acme/app",
        "--squash",
        "--head",
        &"a".repeat(40),
    ]
    .map(str::to_owned)
    .to_vec()
}

#[test]
fn a_copied_session_grant_serves_the_same_conversation_and_each_folder_revokes_its_own() {
    let (original, context, reference) = fixture();
    let proposed = proposal(&context, &reference);
    let granted = apply(&context, &proposed["apply_arguments"], true).unwrap();
    let before = std::fs::read(original.path().join(".shield/review/state.json")).unwrap();
    let copy = assert_fs::TempDir::new().unwrap();
    crate::test_support::copy_fixture_directory(original.path(), copy.path());
    let opened = super::super::open(copy.path()).unwrap();
    let mut child = context.clone();
    child.project_id = opened.project_id;
    child.project_root = opened.project_root;
    assert_eq!(child.project_id, context.project_id);
    // Session grants are keyed by the host conversation, which a copy shares.
    let mut request = receipt(&child, arguments("42"));
    handoff(&child, &mut request, Some(&json!({"command":"acme merge"}))).unwrap();
    assert_eq!(
        request["authorization"]["grants"][0]["grant_id"],
        granted["grant_id"]
    );
    revoke(&child, &json!({"grant_id":granted["grant_id"]})).unwrap();
    let mut revoked = receipt(&child, arguments("42"));
    handoff(&child, &mut revoked, Some(&json!({"command":"acme merge"}))).unwrap();
    assert!(revoked["authorization"].is_null());
    // Revoking in the copy changes only the copy's state.
    assert_eq!(
        std::fs::read(original.path().join(".shield/review/state.json")).unwrap(),
        before
    );
    let mut request = receipt(&context, arguments("42"));
    handoff(
        &context,
        &mut request,
        Some(&json!({"command":"acme merge"})),
    )
    .unwrap();
    assert_eq!(
        request["authorization"]["grants"][0]["grant_id"],
        granted["grant_id"]
    );
    assert_eq!(request["disposition"], "allowed");
}

#[test]
fn session_grants_runtime_controls_do_not_disable_matching() {
    let (_root, context, reference) = fixture();
    let proposed = proposal(&context, &reference);
    apply(&context, &proposed["apply_arguments"], true).unwrap();
    for field in [
        "timeout",
        "timeout_ms",
        "yield_time_ms",
        "max_output_tokens",
    ] {
        let mut input = json!({"command":"acme merge"});
        input[field] = json!(1000);
        let mut request = receipt(&context, arguments("42"));
        handoff(&context, &mut request, Some(&input)).unwrap();
        assert_eq!(
            request["authorization"]["kind"], "session_grant",
            "{field}: {request}"
        );
    }
    for (field, value) in [
        ("env", json!({"ACME":"other"})),
        ("shell", json!("other-shell")),
        ("run_in_background", json!(true)),
        ("login", json!(true)),
        ("tty", json!(true)),
        ("sandbox_permissions", json!("require_escalated")),
    ] {
        let mut input = json!({"command":"acme merge"});
        input[field] = value;
        let mut request = receipt(&context, arguments("42"));
        handoff(&context, &mut request, Some(&input)).unwrap();
        assert!(request["authorization"].is_null());
    }
}

#[test]
fn session_grants_reject_open_operand_grammars() {
    let (_root, context, reference) = fixture();
    for arguments in [
        json!({"kind":"options","prefix":["push"],"operands":true}),
        json!({"kind":"options","prefix":["push"],"parse":"stop_at_operand","operands":{"match":{"regex":".*"},"min":0,"max":3}}),
    ] {
        assert!(
            propose(
                &context,
                &json!({"session_ref":reference,"idempotency_key":"open",
            "scope":[{"conditions":{"domain":"argv","executable":"git","arguments":arguments,
                "cwd":{"within":{"root":"project"}}}}]})
            )
            .is_err()
        );
    }
}

#[test]
fn session_capture_is_best_effort_and_existing_context_needs_no_lock() {
    let (root, mut context, reference) = fixture();
    context.surface = "hook".into();
    let mut blocked = receipt(&context, arguments("42"));
    let lock = root.path().join(".shield/review/state.lock");
    std::fs::write(&lock, "fixture writer").unwrap();
    let mut allowed = json!({"code":"policy-allow","decision":"allow","disposition":"allowed"});
    handoff(&context, &mut allowed, None).unwrap();
    assert_eq!(allowed["session_context"]["session_ref"], reference);
    let mut fresh = context.clone();
    fresh.session_id = Some("new-session".into());
    let mut uncaptured = json!({"code":"policy-allow","decision":"allow","disposition":"allowed"});
    handoff(&fresh, &mut uncaptured, None).unwrap();
    assert!(uncaptured["session_context"].is_null());
    assert_eq!(uncaptured["disposition"], "allowed");
    assert!(
        handoff(
            &context,
            &mut blocked,
            Some(&json!({"command":"acme merge"}))
        )
        .is_err()
    );
    assert!(blocked["authorization"].is_null());
    assert_eq!(blocked["decision"], "ask");
    std::fs::remove_file(lock).unwrap();
}

#[test]
fn operator_can_prepare_a_recorded_session_without_impersonating_the_agent() {
    let (_root, context, reference) = fixture();
    let mut operator = context.clone();
    operator.surface = "cli".into();
    operator.agent = None;
    operator.session_id = None;
    let prepared = proposal(&operator, &reference);
    assert_eq!(prepared["target"]["agent"], "codex");
    assert!(prepared["grant_id"].is_null());
    assert!(matches!(
        apply(&operator, &prepared["apply_arguments"], false),
        Err(Error::ApprovalRequired)
    ));
    let mut foreign = context.clone();
    foreign.agent = Some("claude-code".into());
    assert!(matches!(
        propose(
            &foreign,
            &json!({"session_ref":reference,"idempotency_key":"foreign","scope":scope()})
        ),
        Err(Error::Invalid(_))
    ));
}

#[test]
fn reusable_grant_requires_confirmation_and_covers_only_closed_family() {
    let (_root, context, reference) = fixture();
    let proposed = proposal(&context, &reference);
    let args = &proposed["apply_arguments"];
    assert!(matches!(
        apply(&context, args, false),
        Err(Error::ApprovalRequired)
    ));
    let question = confirmation(&context, args).unwrap().unwrap();
    assert!(
        question.contains("Unlimited matching uses")
            && question.contains("github.example.com/acme/app")
    );
    let granted = apply(&context, args, true).unwrap();
    for pr in ["42", "43", "42"] {
        let mut request = receipt(&context, arguments(pr));
        handoff(
            &context,
            &mut request,
            Some(&json!({"command":"acme merge"})),
        )
        .unwrap();
        assert_eq!(request["disposition"], "allowed", "{request}");
        assert_eq!(request["decision"], "ask");
        assert_eq!(request["authorization"]["kind"], "session_grant");
    }
    for arguments in [
        arguments("x"),
        vec!["merge".into()],
        {
            let mut args = arguments("42");
            args.push("--admin".into());
            args
        },
        {
            let mut args = arguments("42");
            args[3] = "github.example.com/acme/other".into();
            args
        },
    ] {
        let mut request = receipt(&context, arguments);
        handoff(
            &context,
            &mut request,
            Some(&json!({"command":"acme merge"})),
        )
        .unwrap();
        assert_eq!(request["disposition"], "approval_required", "{request}");
    }
    assert_eq!(
        apply(&context, args, true).unwrap()["expires_at_ms"],
        granted["expires_at_ms"]
    );
    assert!(confirmation(&context, args).unwrap().is_none());
}

#[test]
fn revocation_replay_and_decline_never_restore_authority() {
    let (_root, context, reference) = fixture();
    let proposed = proposal(&context, &reference);
    let args = &proposed["apply_arguments"];
    let granted = apply(&context, args, true).unwrap();
    revoke(&context, &json!({"grant_id":granted["grant_id"]})).unwrap();
    assert_eq!(
        apply(&context, args, true).unwrap()["grant_status"],
        "revoked"
    );
    let mut request = receipt(&context, arguments("42"));
    handoff(
        &context,
        &mut request,
        Some(&json!({"command":"acme merge"})),
    )
    .unwrap();
    assert_eq!(request["disposition"], "approval_required");
    let proposed = propose(
        &context,
        &json!({"session_ref":reference,"idempotency_key":"batch-2","scope":scope()}),
    )
    .unwrap();
    decline(&context, &proposed["apply_arguments"]).unwrap();
    assert!(matches!(
        apply(&context, &proposed["apply_arguments"], true),
        Err(Error::StaleSnapshot)
    ));
}

#[test]
fn session_isolation_execution_options_and_compound_requests_keep_boundaries() {
    let (_root, context, reference) = fixture();
    let proposed = proposal(&context, &reference);
    apply(&context, &proposed["apply_arguments"], true).unwrap();
    let mut foreign = context.clone();
    foreign.session_id = Some("other-session".into());
    let mut request = receipt(&context, arguments("42"));
    handoff(
        &foreign,
        &mut request,
        Some(&json!({"command":"acme merge"})),
    )
    .unwrap();
    assert_eq!(request["disposition"], "approval_required");
    let mut request = receipt(&context, arguments("42"));
    handoff(
        &context,
        &mut request,
        Some(&json!({"command":"acme merge","run_in_background":true})),
    )
    .unwrap();
    assert_eq!(request["disposition"], "approval_required");
    let mut request = receipt(&context, arguments("42"));
    request["_shell_analysis"]["coverage"] = json!("structured-literal-operations");
    let extra = json!({"coverage":"literal-argv","decision":"ask","request":{"executable":"acme","arguments":["delete"],"cwd":context.project_root,"project":context.project_root},"evaluation":{"decision":"ask","evaluation":{"matches":[]}}});
    request["_shell_analysis"]["operations"]
        .as_array_mut()
        .unwrap()
        .push(extra);
    handoff(
        &context,
        &mut request,
        Some(&json!({"command":"acme merge && acme delete"})),
    )
    .unwrap();
    assert_eq!(request["disposition"], "approval_required");
}

#[test]
fn idempotency_scope_validation_and_foreign_revocation_are_enforced() {
    let (_root, context, reference) = fixture();
    let first = proposal(&context, &reference);
    assert_eq!(
        proposal(&context, &reference)["proposal_id"],
        first["proposal_id"]
    );
    assert!(propose(&context,&json!({"session_ref":reference,"idempotency_key":"batch-1","duration_seconds":60,"scope":scope()})).is_err());
    let mut bad = scope();
    bad[0]["conditions"]["arguments"] = json!({"kind":"any"});
    assert!(scope::prepare(&bad).is_err());
    let mut bad = scope();
    bad[0]["conditions"]["cwd"] = json!({"within":{"root":"absolute","path":"/"}});
    assert!(scope::prepare(&bad).is_err());
    let mut other = context.clone();
    other.connection_id = Some("other-connection".into());
    other.session_id = None;
    assert!(revoke(&other, &json!({"proposal_id":first["proposal_id"]})).is_err());
    other.session_id = Some("foreign-session".into());
    assert!(
        propose(
            &other,
            &json!({"session_ref":reference,"idempotency_key":"foreign","scope":scope()})
        )
        .is_err()
    );
    assert!(propose(&context,&json!({"session_ref":reference,"session_id":"invented","idempotency_key":"raw","scope":scope()})).is_err());
}

#[test]
fn scope_alternatives_are_independent_and_long_option_abbreviations_are_refused() {
    let (_root, context, _reference) = fixture();
    let mut alternatives = scope();
    alternatives[0]["requirements"] = json!({"cwd":{"exact":{"root":"project","path":"other"}}});
    alternatives
        .as_array_mut()
        .unwrap()
        .push(scope()[0].clone());
    let request = crate::policy::Request {
        executable: "acme".into(),
        arguments: arguments("42"),
        cwd: context.project_root.clone(),
        project: context.project_root.clone(),
    };
    assert!(scope::matches(&scope::prepare(&alternatives).unwrap(), &request).unwrap());
    let mut invalid = scope();
    invalid[0]["conditions"]["arguments"]["long_abbreviations"] = json!(true);
    assert!(scope::prepare(&invalid).is_err());
    let mut explicit = scope();
    explicit[0]["conditions"]["arguments"]["unknown_options"] = json!("refuse");
    assert!(scope::prepare(&explicit).is_ok());
}

#[test]
fn session_grants_bounded_history_keeps_grant_links_and_marks_missing_details() {
    let authorization = json!({"kind":"session_grant","grants":[{"grant_id":"session-grant-acme-1","approval_id":"approval-acme-2","operation_refs":(0..256).map(|i|format!("shell:0:{i}-{}",i+1)).collect::<Vec<_>>(),"override_denies":[]}]});
    let event = json!({"kind":"decision","details":{"authorization":authorization,"evaluation":{"decision":"ask","reason":"x".repeat(12_000)},"disposition":"allowed"}});
    let bounded = super::super::storage::history::bound_event(event);
    assert!(crate::json::fits(&bounded, 8192));
    assert_eq!(bounded["details"]["authorization"]["kind"], "session_grant");
    assert_eq!(
        bounded["details"]["authorization"]["grants"][0]["grant_id"],
        "session-grant-acme-1"
    );
    assert_eq!(bounded["truncated"], true);
}

#[test]
fn published_mcp_catalog_admits_the_session_workflow() {
    let (_root, context, reference) = fixture();
    let catalog = crate::mcp::catalog(Some("codex")).unwrap();
    assert!(catalog.get("session_grant_apply").is_none());
    for name in [
        "session_grant_show",
        "session_grant_propose",
        "session_grant_reconsider",
        "session_grant_approve",
        "session_grant_revoke",
    ] {
        assert!(catalog.get(name).is_some(), "{name}");
    }
    let connection = super::super::mcp::Connection::enrolled(&context).unwrap();
    let proposed = super::super::mcp::call_with_confirmation(
        &context,
        &connection,
        "session_grant_propose",
        &json!({"session_ref":reference,"idempotency_key":"wire-1","scope":scope()}),
        &|| Ok(()),
        false,
    );
    assert!(proposed["error"].is_null(), "{proposed}");
    let denied = super::super::mcp::call_with_confirmation(
        &context,
        &connection,
        "session_grant_approve",
        &proposed["data"]["apply_arguments"],
        &|| Ok(()),
        false,
    );
    assert_eq!(denied["error"]["code"], "operator_review_required");
}

#[test]
fn session_grants_do_not_cross_a_nested_review_enrollment_without_config() {
    let (_root, context, reference) = fixture();
    let proposed = proposal(&context, &reference);
    apply(&context, &proposed["apply_arguments"], true).unwrap();
    let child = Path::new(&context.project_root).join("child");
    std::fs::create_dir_all(child.join(".shield/review")).unwrap();
    let mut request = receipt(&context, arguments("42"));
    request["_shell_analysis"]["operations"][0]["request"]["cwd"] = json!(child);
    handoff(
        &context,
        &mut request,
        Some(&json!({"command":"acme merge"})),
    )
    .unwrap();
    assert!(request["authorization"].is_null());
}

#[test]
fn native_hooks_reuse_session_grants_for_both_hosts_and_permission_request() {
    for agent in ["codex", "claude"] {
        let (_root, mut context, reference) = fixture();
        let reference = if agent == "claude" {
            context.agent = Some(agent.into());
            let mut value = json!({"code":"policy-allow"});
            handoff(&context, &mut value, None).unwrap();
            value["session_context"]["session_ref"]
                .as_str()
                .unwrap()
                .to_owned()
        } else {
            reference
        };
        let proposed = proposal(&context, &reference);
        apply(&context, &proposed["apply_arguments"], true).unwrap();
        for (event, name) in [
            (crate::agents::Event::Before, "PreToolUse"),
            (crate::agents::Event::Permission, "PermissionRequest"),
        ] {
            if agent == "claude" && event == crate::agents::Event::Permission {
                continue;
            }
            let command = format!(
                "acme merge 42 --repo github.example.com/acme/app --squash --head {}",
                "a".repeat(40)
            );
            // Exercise vendor argument shapes through normalization and policy,
            // rather than feeding an already-normalized handoff receipt.
            let input = if agent == "claude" {
                json!({"command":command,"description":"Merge the reviewed Acme change","timeout":120_000})
            } else {
                json!({"command":command,"workdir":context.project_root,"description":"Merge the reviewed Acme change",
                    "timeout_ms":120_000,"yield_time_ms":1000,"max_output_tokens":2000})
            };
            let host = json!({"hook_event_name":name,"tool_name":"Bash","tool_use_id":"acme-call","session_id":"acme-session","cwd":context.project_root,
                "tool_input":input});
            let reply = crate::agents::translate_event(
                Path::new(&context.project_root),
                crate::agents::find(agent).unwrap(),
                &serde_json::to_vec(&host).unwrap(),
                event,
                &crate::diagnostics::Log::disabled(),
            );
            if agent == "codex" && event == crate::agents::Event::Permission {
                assert_eq!(
                    reply["hookSpecificOutput"]["decision"]["behavior"], "allow",
                    "{reply}"
                );
            } else if agent == "codex" {
                assert!(reply.to_string().contains("session grant"), "{reply}");
                assert!(
                    reply["hookSpecificOutput"]["permissionDecision"].is_null(),
                    "{reply}"
                );
            } else {
                assert_eq!(
                    reply["hookSpecificOutput"]["permissionDecision"], "allow",
                    "{reply}"
                );
            }
        }
        let history = super::super::history::inspect_filtered(
            &context,
            &super::super::history::Query::default(),
            Some("session_grant"),
            None,
        )
        .unwrap();
        let entries = history["entries"].as_array().unwrap();
        assert!(
            !entries.is_empty(),
            "{history}; unfiltered: {}",
            super::super::history::inspect(&context, &super::super::history::Query::default())
                .unwrap()
        );
        for entry in entries {
            assert_eq!(entry["approved"], false);
            assert_eq!(entry["evaluation"]["decision"], "ask");
            assert_eq!(
                entry["authorization"]["grants"][0]["grant_id"],
                apply(&context, &proposed["apply_arguments"], true).unwrap()["grant_id"]
            );
        }
    }
}

#[test]
fn policy_changes_and_registration_changes_permanently_invalidate_grants() {
    for change in ["policy", "registration", "protection"] {
        let (_root, context, reference) = fixture();
        let proposed = proposal(&context, &reference);
        apply(&context, &proposed["apply_arguments"], true).unwrap();
        let original = store(&context).unwrap().read_checkpoint().unwrap();
        store(&context)
            .unwrap()
            .update_checkpoint(&context, &|| Ok(()), |state| {
                if change == "policy" {
                    state.policy_revision = "changed".into();
                } else if change == "registration" {
                    state.operator["integration"]["binary"] = json!("/fixture/replaced-shield");
                } else {
                    state.operator["path_access"] = json!({"agent_settings_read":"allow"});
                }
                Ok(json!({}))
            })
            .unwrap();
        store(&context)
            .unwrap()
            .update_checkpoint(&context, &|| Ok(()), |state| {
                state.policy_revision = original.policy_revision.clone();
                state.operator = original.operator.clone();
                Ok(json!({}))
            })
            .unwrap();
        let value = show(&context, &json!({"proposal_id":proposed["proposal_id"]})).unwrap();
        assert_eq!(value["grant_status"], "stale", "{change}: {value}");
    }
}

#[test]
fn pack_deny_references_bind_registry_identity_rather_than_display_alias() {
    let reference = json!({"rule_ref":{"scope":"pack","pack_selector":"acme-selector","rule_id":"merge"},"branch":"met","rule_digest":"a".repeat(64)});
    let rules = json!({"rules":[{"rule_ref":reference["rule_ref"],"source":{"id":"acme/git","source_identity":"registry-identity","source_alias":"friendly"},"decision":"deny","rule_digest":reference["rule_digest"],"reason":"Review merges."}]});
    let evidence = scope::overrides(&json!([reference]), &rules).unwrap();
    let decision = |source| json!({"matches":[{"source":source,"rule_id":"merge","decision":"deny","branch":"met","applied":true}]});
    assert!(scope::covered_denies(
        &evidence,
        &decision("registry-identity/acme/git")
    ));
    assert!(!scope::covered_denies(
        &evidence,
        &decision("friendly/acme/git")
    ));
}

#[test]
fn equivalent_offers_do_not_duplicate_forms_or_reopen_declines() {
    let (_root, context, reference) = fixture();
    let first = proposal(&context, &reference);
    let args = json!({"session_ref":reference,"idempotency_key":"another-key","scope":scope()});
    assert_eq!(
        propose(&context, &args).unwrap()["proposal_id"],
        first["proposal_id"]
    );
    decline(&context, &first["apply_arguments"]).unwrap();
    assert_eq!(
        propose(&context, &args).unwrap()["proposal_status"],
        "declined"
    );
}

#[test]
fn grants_do_not_depend_on_optional_history() {
    let (_root, context, reference) = fixture();
    let proposed = proposal(&context, &reference);
    apply(&context, &proposed["apply_arguments"], true).unwrap();
    store(&context)
        .unwrap()
        .update_checkpoint(&context, &|| Ok(()), |state| {
            state.history_enabled = false;
            state.events.clear();
            Ok(json!({}))
        })
        .unwrap();
    let mut value = receipt(&context, arguments("42"));
    handoff(&context, &mut value, Some(&json!({"command":"acme merge"}))).unwrap();
    assert_eq!(value["authorization"]["kind"], "session_grant");
}

#[test]
fn explicit_deny_override_covers_only_the_named_current_rule() {
    let rules=json!({"schema_version":1,"kind":"custom","capabilities":["structured-rules-v1","argument-rules-v1"],"rules":[
        {"id":"acme-merge","decision":"deny","reason":"Merges require review.","conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"any"}}}
    ]}).to_string();
    let (_root, context, reference) = fixture_with_rules(Some(&rules));
    let ordinary = proposal(&context, &reference);
    apply(&context, &ordinary["apply_arguments"], true).unwrap();
    let listed = super::super::inspection::rules(&context, None).unwrap();
    let rule = &listed["rules"][0];
    let proposed=propose(&context,&json!({"session_ref":reference,"idempotency_key":"override","scope":scope(),"override_denies":[{"rule_ref":rule["rule_ref"],"branch":"met","rule_digest":rule["rule_digest"]}]})).unwrap();
    assert!(
        confirmation(&context, &proposed["apply_arguments"])
            .unwrap()
            .unwrap()
            .contains("Merges require review.")
    );
    let denied = |source: &str, id: &str, branch: &str| {
        let mut value = receipt(&context, arguments("42"));
        value["decision"] = json!("deny");
        value["code"] = json!("policy-deny");
        value["_shell_analysis"]["operations"][0]["decision"] = json!("deny");
        value["_shell_analysis"]["operations"][0]["evaluation"]["evaluation"]["matches"] = json!([{"source":source,"rule_id":id,"decision":"deny","applied":true,"branch":branch}]);
        value
    };
    let mut value = denied("custom", "acme-merge", "met");
    handoff(&context, &mut value, Some(&json!({"command":"acme merge"}))).unwrap();
    assert_ne!(value["disposition"], "allowed");
    apply(&context, &proposed["apply_arguments"], true).unwrap();
    let mut value = denied("custom", "acme-merge", "met");
    handoff(&context, &mut value, Some(&json!({"command":"acme merge"}))).unwrap();
    assert_eq!(value["disposition"], "allowed", "{value}");
    assert_eq!(value["decision"], "deny");
    for (source, id, branch) in [
        ("custom", "other-rule", "met"),
        ("other", "acme-merge", "met"),
        ("custom", "acme-merge", "unmet"),
    ] {
        let mut value = denied(source, id, branch);
        handoff(&context, &mut value, Some(&json!({"command":"acme merge"}))).unwrap();
        assert_ne!(value["disposition"], "allowed");
    }
}

#[test]
fn failed_requirements_and_protected_state_cannot_use_session_grants() {
    let (_root, context, reference) = fixture();
    let proposed = proposal(&context, &reference);
    apply(&context, &proposed["apply_arguments"], true).unwrap();
    for field in ["fixed_protection", "requirements", "partial"] {
        let mut value = receipt(&context, arguments("42"));
        match field {
            "fixed_protection" => value["fixed_protection"] = json!(true),
            "requirements" => {
                value["_shell_analysis"]["operations"][0]["evaluation"]["evaluation"]["requirement_checks"] =
                    json!([{"status":"unmet"}]);
            }
            _ => value["_shell_analysis"]["coverage"] = json!("partial"),
        }
        handoff(&context, &mut value, Some(&json!({"command":"acme merge"}))).unwrap();
        assert_ne!(value["disposition"], "allowed", "{field}");
    }
}

#[test]
fn expiry_is_terminal_and_retained_receipts_are_reclaimed() {
    let (_root, context, reference) = fixture();
    let proposed = proposal(&context, &reference);
    let granted = apply(&context, &proposed["apply_arguments"], true).unwrap();
    let expires = granted["expires_at_ms"].as_u64().unwrap();
    store(&context)
        .unwrap()
        .update_checkpoint(&context, &|| Ok(()), |state| {
            compact(state, expires);
            Ok(json!({}))
        })
        .unwrap();
    assert_eq!(
        apply(&context, &proposed["apply_arguments"], true).unwrap()["grant_status"],
        "expired"
    );
    let mut value = receipt(&context, arguments("42"));
    handoff(&context, &mut value, Some(&json!({"command":"acme merge"}))).unwrap();
    assert_ne!(value["disposition"], "allowed");
    store(&context)
        .unwrap()
        .update_checkpoint(&context, &|| Ok(()), |state| {
            compact(state, expires + RETAIN);
            Ok(json!({}))
        })
        .unwrap();
    assert!(show(&context, &json!({"grant_id":granted["grant_id"]})).is_err());
}
