//! The host-approved tool applies exact proposals through the shared atomic commit.
use super::*;

/// The Codex confirmation as one text: its change line, then any details.
pub(super) fn readable(args: &Value) -> String {
    format!(
        "{}\n{}",
        args["change"].as_str().unwrap(),
        args["details"].as_str().unwrap_or_default()
    )
}

pub(super) fn prepare(context: &Context, connection: &Connection, change: &Value) -> Value {
    let result = invoke(
        context,
        connection,
        "policy_propose",
        json!({"change":change}),
    );
    assert!(result["error"].is_null(), "{change}: {result}");
    result["data"]["apply_arguments"].clone()
}

pub(super) fn apply(context: &Context, connection: &Connection, args: &Value) -> Value {
    let result =
        call_with_confirmation(context, connection, "policy_apply", args, &|| Ok(()), true);
    assert!(result["error"].is_null(), "{result}");
    assert_eq!(result["data"]["status"], "applied");
    result
}

#[test]
fn a_declared_agent_selects_its_review_adapter_only_when_its_integration_is_recorded() {
    for recorded in [false, true] {
        let (root, context, connection) = fixture();
        if recorded {
            Store::open(root.path())
                .unwrap()
                .update(&context, &|| Ok(()), |state| {
                    state.operator["integration"] = json!({"binary":"/opt/acme/shield",
                        "binary_digest":"0".repeat(64),"version":"0.0.0","entry":{},
                        "agents":{"codex":{"groups":{},"mcp":null}},"ask_fallback":{}});
                    Ok(json!({}))
                })
                .unwrap();
        }
        let mut declared = review::cli::context(&crate::cli::ReviewContext {
            project: root.path().into(),
            agent: Some("codex".into()),
            session_id: None,
            tool_use_id: None,
        })
        .unwrap();
        declared.surface = "mcp".into();
        declared.connection_id = context.connection_id.clone();
        let args = prepare(
            &declared,
            &connection,
            &json!({"kind":"rule_add","rule":rule("read", "allow")}),
        );
        let result = call_with_confirmation(
            &declared,
            &connection,
            "policy_apply",
            &args,
            &|| Ok(()),
            true,
        );
        let proposal = &Store::open(root.path()).unwrap().read().unwrap().proposals[0];
        assert_eq!(proposal.provenance["agent"], "codex");
        if recorded {
            assert!(result["error"].is_null(), "{result}");
            assert_eq!(
                result["data"]["operator"]["mechanism"],
                "host-tool-permission"
            );
        } else {
            // Without a recorded integration, --agent is attribution alone:
            // the generic adapter sends activation to the terminal.
            assert_eq!(
                result["error"]["code"], "operator_review_required",
                "{result}"
            );
            assert_eq!(result["error"]["operator_route"], "shield policy review");
            assert_eq!(proposal.status, "pending");
        }
    }
}

#[test]
fn a_connection_starting_during_a_publication_keeps_its_recorded_adapter() {
    let (root, context, _) = fixture();
    let store = Store::open(root.path()).unwrap();
    store
        .update(&context, &|| Ok(()), |state| {
            state.operator["integration"] = json!({"binary":"/opt/acme/shield",
                "binary_digest":"0".repeat(64),"version":"0.0.0","entry":{},
                "agents":{"codex":{"groups":{},"mcp":null}},"ask_fallback":{}});
            Ok(json!({}))
        })
        .unwrap();
    // The writer lock is held for the whole callback, as during a publication.
    let started = store
        .update(&context, &|| Ok(()), |_| {
            let started = review::cli::context(&crate::cli::ReviewContext {
                project: root.path().into(),
                agent: Some("codex".into()),
                session_id: None,
                tool_use_id: None,
            })?;
            Ok(json!({"agent":started.agent,"declared":started.declared_agent}))
        })
        .unwrap();
    assert_eq!(started["agent"], "codex", "{started}");
    assert!(started["declared"].is_null(), "{started}");
}

#[test]
fn redacted_apply_arguments_bind_and_commit_the_original_candidate() {
    let (root, mut context, connection) = fixture();
    context.agent = Some("claude-code".into());
    let outside = assert_fs::TempDir::new().unwrap();
    let path = outside.path().join("reference --token placeholder");
    std::fs::create_dir(&path).unwrap();
    let path = path.canonicalize().unwrap();
    let args = prepare(
        &context,
        &connection,
        &json!({"kind":"access_path_add","access":"read","path":path}),
    );
    assert!(args["changes"].to_string().contains("--token [REDACTED]"));
    let before = Store::open(root.path()).unwrap().read().unwrap();
    let proposal = before
        .proposals
        .iter()
        .find(|p| p.id == args["proposal_id"])
        .unwrap();
    assert_eq!(args, proposal.apply_arguments());
    assert_eq!(
        args["candidate_digest"],
        crate::policy::fingerprint(proposal.candidate.to_string().as_bytes())
    );
    let candidate = proposal.candidate.clone();
    for field in ["candidate_digest", "changes"] {
        let mut changed = args.clone();
        changed[field] = json!("altered");
        assert_eq!(
            invoke(&context, &connection, "policy_apply", changed)["error"]["code"],
            "invalid_input"
        );
    }
    apply(&context, &connection, &args);
    apply(&context, &connection, &args);
    let after = Store::open(root.path()).unwrap().read().unwrap();
    assert_eq!(after.candidates["project"], candidate);
    let config: Value = serde_saphyr::from_str(candidate["config"].as_str().unwrap()).unwrap();
    assert_eq!(config["access_paths"]["read"], json!([path]));
}

#[test]
fn inspected_apply_arguments_recover_the_reply_and_keep_approved_changes_after_apply() {
    let (_root, mut context, connection) = fixture();
    context.agent = Some("claude-code".into());
    let result = proposal(&context, &connection, "acme-inspected");
    let id = result["data"]["proposal_id"].clone();
    let reply = result["data"]["apply_arguments"].clone();
    assert_ne!(reply["changes"], json!([]));
    let inspect = || {
        let inspection = invoke(
            &context,
            &connection,
            "policy_show",
            json!({"proposal_id":id}),
        );
        assert!(inspection["error"].is_null(), "{inspection}");
        inspection["data"]["apply_arguments"].clone()
    };
    // A lost reply is recovered from inspection; the copy must apply unchanged.
    let recovered = inspect();
    assert_eq!(recovered, reply);
    apply(&context, &connection, &recovered);
    // After activation, inspection still shows the change that was approved.
    let applied = inspect();
    assert_eq!(applied["changes"], reply["changes"]);
    assert_eq!(applied["candidate_digest"], reply["candidate_digest"]);
}

#[test]
fn codex_readable_review_is_exact_retrievable_and_bound_to_the_retained_candidate() {
    let (root, mut context, connection) = fixture();
    context.agent = Some("codex".into());
    let result = proposal(&context, &connection, "acme-readable");
    let id = result["data"]["proposal_id"].clone();
    let args = &result["data"]["apply_arguments"];
    assert_eq!(args["reference"].as_str().unwrap().len(), 24);
    // Codex shows the first arguments by name: the change leads, details follow.
    let message = &readable(args);
    assert!(
        args["change"]
            .as_str()
            .unwrap()
            .contains("Allow `acme read` without asking."),
        "{message}"
    );
    // Codex's own prompt asks the question and labels Allow/Cancel.
    assert!(!message.contains("Approve this Shield policy change?"));
    assert!(!message.contains("applies this change"));
    let state = Store::open(root.path()).unwrap().read().unwrap();
    let retained = state.proposals.iter().find(|p| p.id == id).unwrap();
    let canonical = retained.apply_arguments();
    let candidate = retained.candidate.clone();
    assert!(!message.contains(canonical["candidate_digest"].as_str().unwrap()));
    assert!(!message.contains(id.as_str().unwrap()));
    for selector in [json!({"proposal_id":id}), json!({})] {
        let inspection = invoke(&context, &connection, "policy_show", selector);
        assert!(inspection["error"].is_null(), "{inspection}");
        let data = &inspection["data"];
        if data["proposals"].is_array() {
            assert_eq!(data["proposals"][0]["proposal_id"], id);
            assert!(data["proposals"][0].get("apply_arguments").is_none());
        } else {
            assert_eq!(&data["apply_arguments"], args);
        }
    }
    let initial = json!(state.active);
    for changed in [
        {
            let mut a = args.clone();
            a["change"] = json!("Allow everything");
            a
        },
        {
            let mut a = args.clone();
            a["reference"] = json!("0".repeat(24));
            a
        },
        {
            let mut a = args.clone();
            a["confirmed"] = json!(true);
            a
        },
        {
            let mut a = args.clone();
            a.as_object_mut().unwrap().remove("change");
            a
        },
        canonical,
    ] {
        let result = invoke(&context, &connection, "policy_apply", changed);
        assert!(!result["error"].is_null(), "{result}");
        assert_eq!(
            json!(Store::open(root.path()).unwrap().read().unwrap().active),
            initial
        );
    }
    for agent in [None, Some("acme-agent"), Some("claude-code")] {
        let mut other = context.clone();
        other.agent = agent.map(str::to_owned);
        let result =
            call_with_confirmation(&other, &connection, "policy_apply", args, &|| Ok(()), true);
        assert!(!result["error"].is_null(), "{result}");
    }
    apply(&context, &connection, args);
    apply(&context, &connection, args);
    assert_eq!(
        Store::open(root.path()).unwrap().read().unwrap().candidates["project"],
        candidate
    );
}

#[test]
fn codex_readable_reviews_cannot_apply_withdrawn_or_stale_proposals() {
    let (root, mut context, connection) = fixture();
    context.agent = Some("codex".into());
    let withdrawn = proposal(&context, &connection, "acme-withdrawn");
    let initial = json!(Store::open(root.path()).unwrap().read().unwrap().active);
    let result = invoke(
        &context,
        &connection,
        "policy_withdraw",
        json!({"proposal_id":withdrawn["data"]["proposal_id"]}),
    );
    assert!(result["error"].is_null(), "{result}");
    let refused = invoke(
        &context,
        &connection,
        "policy_apply",
        withdrawn["data"]["apply_arguments"].clone(),
    );
    assert!(!refused["error"].is_null());
    assert_eq!(
        json!(Store::open(root.path()).unwrap().read().unwrap().active),
        initial
    );
    let stale = proposal(&context, &connection, "acme-stale");
    let fresh = proposal(&context, &connection, "acme-fresh");
    apply(&context, &connection, &fresh["data"]["apply_arguments"]);
    let active = json!(Store::open(root.path()).unwrap().read().unwrap().active);
    let refused = invoke(
        &context,
        &connection,
        "policy_apply",
        stale["data"]["apply_arguments"].clone(),
    );
    assert_eq!(refused["error"]["code"], "stale_revision");
    assert_eq!(
        json!(Store::open(root.path()).unwrap().read().unwrap().active),
        active
    );
}

#[test]
fn withdrawing_a_proposal_after_its_base_changed_reports_the_stale_revision() {
    let (root, mut context, connection) = fixture();
    context.agent = Some("claude-code".into());
    let first = proposal(&context, &connection, "acme-first");
    let second = proposal(&context, &connection, "acme-second");
    apply(&context, &connection, &first["data"]["apply_arguments"]);
    let id = second["data"]["proposal_id"].clone();
    for _ in 0..2 {
        let result = invoke(
            &context,
            &connection,
            "policy_withdraw",
            json!({"proposal_id":id}),
        );
        assert_eq!(result["error"]["code"], "stale_revision", "{result}");
        assert_eq!(result["error"]["operator_route"], "policy_propose");
        assert!(result["data"].is_null(), "{result}");
    }
    let state = Store::open(root.path()).unwrap().read().unwrap();
    let row = state.proposals.iter().find(|p| p.id == id).unwrap();
    assert_eq!(row.status, "stale");
    let receipt = invoke(
        &context,
        &connection,
        "policy_receipt",
        json!({"proposal_id":id}),
    );
    assert_eq!(receipt["error"]["code"], "stale_revision", "{receipt}");
    assert_eq!(receipt["error"]["operator_route"], "policy_propose");
}

#[test]
fn a_pending_proposal_whose_base_changed_has_a_stale_receipt() {
    let (root, mut context, connection) = fixture();
    context.agent = Some("claude-code".into());
    let first = proposal(&context, &connection, "acme-first");
    let second = proposal(&context, &connection, "acme-second");
    let id = second["data"]["proposal_id"].clone();
    // Before the base changes, the receipt routes to apply.
    let pending = invoke(
        &context,
        &connection,
        "policy_receipt",
        json!({"proposal_id":id}),
    );
    assert_eq!(
        pending["error"]["code"], "operator_review_required",
        "{pending}"
    );
    apply(&context, &connection, &first["data"]["apply_arguments"]);
    let receipt = invoke(
        &context,
        &connection,
        "policy_receipt",
        json!({"proposal_id":id}),
    );
    assert_eq!(receipt["error"]["code"], "stale_revision", "{receipt}");
    assert_eq!(receipt["error"]["operator_route"], "policy_propose");
    // The receipt only inspects; the proposal stays pending until a change.
    let state = Store::open(root.path()).unwrap().read().unwrap();
    let row = state.proposals.iter().find(|p| p.id == id).unwrap();
    assert_eq!(row.status, "pending");
}

#[test]
fn codex_readable_folder_confirmation_redacts_without_changing_the_installed_path() {
    let (root, mut context, connection) = fixture();
    context.agent = Some("codex".into());
    let outside = assert_fs::TempDir::new().unwrap();
    let path = outside.path().join("reference --token placeholder");
    std::fs::create_dir(&path).unwrap();
    let path = path.canonicalize().unwrap();
    let args = prepare(
        &context,
        &connection,
        &json!({"kind":"access_path_add","access":"read","path":path}),
    );
    let message = readable(&args);
    assert!(message.contains("--token [REDACTED]"), "{message}");
    assert!(!message.contains("placeholder"));
    apply(&context, &connection, &args);
    let state = Store::open(root.path()).unwrap().read().unwrap();
    let config: Value =
        serde_saphyr::from_str(state.candidates["project"]["config"].as_str().unwrap()).unwrap();
    assert_eq!(config["access_paths"]["read"], json!([path]));
}

#[test]
fn host_approved_rules_domains_and_folder_changes_activate_and_retries_are_idempotent() {
    let (root, mut context, connection) = fixture();
    context.agent = Some("codex".into());
    let outside = assert_fs::TempDir::new().unwrap();
    for change in [
        json!({"kind":"rule_add","rule":rule("read", "allow")}),
        json!({"kind":"rule_edit","rule_id":"read","rule":rule("read", "ask")}),
        json!({"kind":"rule_remove","rule_id":"read"}),
        json!({"kind":"rule_add","rule":{"id":"docs-domain","decision":"allow","conditions":{"domain":"web","action":"fetch"},"requirements":{"url":{"schemes":["https"],"hosts":["docs.example.com"]}},"on_unmet":{"decision":"ask","message":"Ask about other domains."}}}),
        json!({"kind":"access_path_add","access":"read","path":outside.path()}),
        json!({"kind":"access_path_remove","access":"read","path":outside.path()}),
    ] {
        let args = prepare(&context, &connection, &change);
        if change["rule"]["conditions"]["domain"] == "web" {
            assert!(readable(&args).contains("could not compare this change"));
        }
        let result = apply(&context, &connection, &args);
        assert_eq!(
            result["data"]["operator"]["mechanism"],
            "host-tool-permission"
        );
        let state = Store::open(root.path()).unwrap().read().unwrap();
        state.active.as_ref().unwrap().validate(&state).unwrap();
        let active = json!(state.active);
        apply(&context, &connection, &args);
        assert_eq!(
            json!(Store::open(root.path()).unwrap().read().unwrap().active),
            active
        );
    }
}

#[test]
fn host_application_refuses_changed_stale_withdrawn_or_unregistered_requests() {
    let (root, mut context, connection) = fixture();
    context.agent = Some("claude-code".into());
    let args = prepare(
        &context,
        &connection,
        &json!({"kind":"rule_add","rule":rule("read", "allow")}),
    );
    let initial = json!(Store::open(root.path()).unwrap().read().unwrap().active);
    for field in ["candidate_digest", "changes", "review", "confirmed"] {
        let mut changed = args.clone();
        changed[field] = json!(true);
        assert!(!invoke(&context, &connection, "policy_apply", changed)["error"].is_null());
    }
    let mut missing_review = args.clone();
    missing_review.as_object_mut().unwrap().remove("review");
    assert!(!invoke(&context, &connection, "policy_apply", missing_review)["error"].is_null());
    let mut other = context.clone();
    other.agent = None;
    assert!(!invoke(&other, &connection, "policy_apply", args.clone())["error"].is_null());
    let cancelled = call(&context, &connection, "policy_apply", &args, &|| {
        Err(crate::policy::Error::Interrupted)
    });
    assert!(!cancelled["error"].is_null());
    assert_eq!(
        json!(Store::open(root.path()).unwrap().read().unwrap().active),
        initial
    );
    invoke(
        &context,
        &connection,
        "policy_withdraw",
        json!({"proposal_id":args["proposal_id"]}),
    );
    assert!(!invoke(&context, &connection, "policy_apply", args)["error"].is_null());
    let stale = prepare(
        &context,
        &connection,
        &json!({"kind":"rule_add","rule":rule("old", "allow")}),
    );
    let fresh = prepare(
        &context,
        &connection,
        &json!({"kind":"rule_add","rule":rule("new", "deny")}),
    );
    apply(&context, &connection, &fresh);
    assert_eq!(
        invoke(&context, &connection, "policy_apply", stale)["error"]["code"],
        "stale_revision"
    );
}

#[test]
fn host_application_refuses_after_a_hand_edit_and_keeps_the_edit() {
    let (root, mut context, connection) = fixture();
    context.agent = Some("claude-code".into());
    // An applied rule writes the project rules file the hand edit then changes.
    let first = prepare(
        &context,
        &connection,
        &json!({"kind":"rule_add","rule":rule("read", "allow")}),
    );
    apply(&context, &connection, &first);
    let args = prepare(
        &context,
        &connection,
        &json!({"kind":"rule_add","rule":rule("write", "deny")}),
    );
    let revision = Store::open(root.path())
        .unwrap()
        .read()
        .unwrap()
        .policy_revision;
    let rules = root.path().join(".shield/rules.yml");
    let edited = format!(
        "{}# Acme hand edit\n",
        std::fs::read_to_string(&rules).unwrap()
    );
    std::fs::write(&rules, &edited).unwrap();
    let result = call_with_confirmation(
        &context,
        &connection,
        "policy_apply",
        &args,
        &|| Ok(()),
        true,
    );
    assert!(
        result["error"]["message"]
            .as_str()
            .is_some_and(|m| m.contains("project files changed during confirmation; review again")),
        "{result}"
    );
    assert_eq!(std::fs::read_to_string(&rules).unwrap(), edited);
    assert_eq!(
        Store::open(root.path())
            .unwrap()
            .read()
            .unwrap()
            .policy_revision,
        revision
    );
}

#[test]
fn host_approved_pack_add_configuration_update_and_removal_share_the_operator_pipeline() {
    let (root, mut context, connection) = fixture();
    context.agent = Some("claude-code".into());
    let source = assert_fs::TempDir::new().unwrap();
    let directory = source.path().canonicalize().unwrap();
    let rules = json!([{"id":"build","decision":"allow","conditions":{"domain":"argv","executable":{"var":"tool"},"arguments":{"kind":"any"}}}]);
    tools_release(&directory, "1.0.0", &rules);
    Store::open(root.path()).unwrap().update(&context, &|| Ok(()), |state| {
        state.operator["sources"]["acme-tools"] = json!({"identity":crate::policy::fingerprint(directory.to_str().unwrap().as_bytes()),"location":directory,"key":null});
        Ok(json!({}))
    }).unwrap();
    for change in [
        json!({"kind":"pack_add","reference":"acme-tools/acme/tools"}),
        json!({"kind":"pack_configure","reference":"acme-tools/acme/tools","set":{"tool":"make"}}),
        json!({"kind":"pack_update","reference":"acme-tools/acme/tools"}),
        json!({"kind":"pack_remove","reference":"acme-tools/acme/tools"}),
    ] {
        if change["kind"] == "pack_update" {
            tools_release(&directory, "1.1.0", &rules);
        }
        let args = prepare(&context, &connection, &change);
        let comparison = &args["review"]["comparison"];
        assert!(comparison["complete"].is_boolean(), "{args}");
        assert!(comparison["relation"].is_string(), "{args}");
        let packs = &args["review"]["packs"];
        if change["kind"] == "pack_add" {
            assert_eq!(packs["before"], json!([]));
            assert_eq!(packs["after"][0]["rule_count"], 1);
            // The variable-dependent rule is inactive until configured.
            assert_eq!(comparison["relation"], "equivalent");
            assert!(comparison["expansion"].is_null());
        } else if change["kind"] == "pack_configure" {
            assert_eq!(comparison["relation"], "expansive");
            assert_eq!(comparison["expansion"]["request"]["executable"], "make");
            assert_eq!(comparison["expansion"]["before"], "ask");
            assert_eq!(comparison["expansion"]["after"], "allow");
        } else if change["kind"] == "pack_remove" {
            assert_eq!(packs["before"][0]["rule_count"], 1);
            assert_eq!(packs["after"], json!([]));
            assert_eq!(comparison["restriction"]["before"], "allow");
            assert_eq!(comparison["restriction"]["after"], "ask");
        }
        apply(&context, &connection, &args);
    }
    assert_eq!(
        Store::open(root.path()).unwrap().read().unwrap().operator["installed"],
        json!([])
    );
}
