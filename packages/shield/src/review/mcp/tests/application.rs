//! The host-approved tool applies exact proposals through the shared atomic commit.
use super::*;

#[test]
fn mcp_connection_default_changes_are_detected_without_declaration_edits() {
    let config = json!({"schema_version":1,"packs":[{
        "id":"acme/cloud","version":"1.0.0","profile":"recommended","path":"pack.yml"
    }],"mcp":{"cloud":{
        "transport":{"type":"defaults","pack":"acme/cloud","integration":"cloud"},
        "agents":{"codex":"cloud"},"packs":{"acme/cloud":"cloud"}
    }}})
    .to_string();
    let candidate = |command: &str| {
        let pack = json!({"schema_version":1,"kind":"pack","id":"acme/cloud","version":"1.0.0",
            "description":"Acme cloud","license":"MIT","maintainers":["Acme"],"engines":{"shield":"*"},
            "capabilities":["structured-rules-v1","mcp-integrations-v1"],"rules":[],
            "mcp_integrations":{"cloud":{"transport":"stdio","fields":{"/command":{"exact":command}},"defaults":{"type":"stdio","command":command}}}
        }).to_string();
        json!(Bundle::capture(&config, &[pack], None).unwrap())
    };
    let before = candidate("acme-read");
    let after = candidate("acme-admin");
    assert!(!crate::review::publication::mcp_changed(&before, &before).unwrap());
    assert!(crate::review::publication::mcp_changed(&before, &after).unwrap());
}

#[test]
fn status_leaves_a_deleted_source_unchecked() {
    let (root, mut context, connection) = fixture();
    context.agent = Some("codex".into());
    let first = prepare(
        &context,
        &connection,
        &json!({"kind":"rule_add","rule":rule("initial", "allow")}),
    );
    apply(&context, &connection, &first);
    std::fs::remove_file(root.path().join(".shield/rules.yml")).unwrap();
    let status = invoke(&context, &connection, "status", json!({}));
    assert!(status["error"].is_null(), "{status}");
    assert_eq!(
        status["data"]["coverage"]["repair_needed"], false,
        "{status}"
    );
    assert_eq!(
        status["data"]["repair_needed"], false,
        "no known integration repair is required; source was not inspected"
    );
    assert_eq!(status["data"]["project_file_inspection"], "unchecked");
    assert_eq!(status["data"]["repair_assessment"]["complete"], false);
    assert_eq!(status["data"]["repair_route"], "shield policy review");
    assert!(status["data"].get("repair_arguments").is_none());
}

#[test]
fn prepared_apply_verifies_source_bytes_and_reconnect_can_prepare_its_own_plan() {
    let (root, mut context, connection) = fixture();
    context.agent = Some("codex".into());
    let args = prepare(
        &context,
        &connection,
        &json!({"kind":"rule_add","rule":rule("acme-source", "allow")}),
    );
    let store = Store::open(root.path()).unwrap();
    let state = store.read_checkpoint().unwrap();
    let original = state.proposals[0].files.clone();
    // Texts that differ from what the person reviewed are never written.
    store
        .update_checkpoint(&context, &|| Ok(()), |current| {
            let rules = current.proposals[0].files.get_mut("rules.yml").unwrap();
            rules.after = rules
                .after
                .as_ref()
                .map(|text| text.replace("acme-source", "acme-swapped"));
            Ok(Value::Null)
        })
        .unwrap();
    let refused = invoke(&context, &connection, "policy_apply", args.clone());
    assert!(!refused["error"].is_null(), "{refused}");
    assert_eq!(
        store.read_checkpoint().unwrap().policy_revision,
        state.policy_revision
    );
    assert!(
        !std::fs::read_to_string(root.path().join(".shield/rules.yml"))
            .unwrap_or_default()
            .contains("acme-swapped")
    );
    store
        .update_checkpoint(&context, &|| Ok(()), |current| {
            current.proposals[0].files = original.clone();
            Ok(Value::Null)
        })
        .unwrap();
    store
        .update_checkpoint(&context, &|| Ok(()), |current| {
            current.proposals[0].comparison["formatting"] =
                json!(["Acme review text changed after approval."]);
            Ok(Value::Null)
        })
        .unwrap();
    let refused = invoke(&context, &connection, "policy_apply", args.clone());
    assert!(
        !refused["error"].is_null(),
        "cached presentation accepted a changed review: {refused}"
    );
    store
        .update_checkpoint(&context, &|| Ok(()), |current| {
            current.proposals[0].comparison = state.proposals[0].comparison.clone();
            Ok(Value::Null)
        })
        .unwrap();
    let reconnected = Connection::new("reconnected".into());
    context.connection_id = Some("reconnected".into());
    assert!(!invoke(&context, &reconnected, "policy_apply", args)["error"].is_null());
    let args = fresh_prepare(
        &context,
        &reconnected,
        &json!({"kind":"rule_add","rule":rule("acme-source", "allow")}),
        "reconnected-plan",
    );
    apply(&context, &reconnected, &args);
    apply(&context, &reconnected, &args);
    crate::review::runtime::consumer::load(&context, &|| Ok(())).unwrap();
}

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

fn fresh_prepare(context: &Context, connection: &Connection, change: &Value, key: &str) -> Value {
    let result = invoke(
        context,
        connection,
        "policy_propose",
        json!({"change":change,"idempotency_key":key}),
    );
    assert!(result["error"].is_null(), "{result}");
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
            // the generic adapter reports its missing in-session capability.
            assert_eq!(
                result["error"]["code"], "operator_review_required",
                "{result}"
            );
            assert!(result["error"]["operator_route"].is_null());
            assert_eq!(proposal.status, "withdrawn");
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
    context.agent = Some("claude".into());
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
    let store = Store::open(root.path()).unwrap();
    let mut before = store.read().unwrap();
    let index = before
        .proposals
        .iter()
        .position(|p| p.id == args["proposal_id"])
        .unwrap();
    store
        .hydrate_proposal(&mut before.proposals[index])
        .unwrap();
    let proposal = &before.proposals[index];
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
    let fresh = fresh_prepare(
        &context,
        &connection,
        &json!({"kind":"access_path_add","access":"read","path":path}),
        "after-invalid-arguments",
    );
    apply(&context, &connection, &fresh);
    apply(&context, &connection, &fresh);
    let after = Store::open(root.path()).unwrap().read().unwrap();
    assert_eq!(after.candidates["project"], candidate);
    let config: Value = serde_saphyr::from_str(candidate["config"].as_str().unwrap()).unwrap();
    assert_eq!(config["access_paths"]["read"], json!([path]));
}

#[test]
fn inspected_apply_arguments_recover_the_reply_and_keep_approved_changes_after_apply() {
    let (root, mut context, connection) = fixture();
    context.agent = Some("claude".into());
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
    let committed = apply(&context, &connection, &recovered);
    // Closed inspection has no reusable approval arguments; the receipt is kept.
    let applied = inspect();
    assert!(applied.is_null());
    let receipt = crate::review::proposal_outcome(root.path(), id.as_str().unwrap());
    assert_eq!(receipt["status"], "applied");
    assert_eq!(
        receipt["receipt"]["policy_revision"],
        committed["data"]["receipt"]["policy_revision"]
    );
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
    let store = Store::open(root.path()).unwrap();
    let mut state = store.read().unwrap();
    let index = state.proposals.iter().position(|p| p.id == id).unwrap();
    store.hydrate_proposal(&mut state.proposals[index]).unwrap();
    let retained = &state.proposals[index];
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
    let initial = json!(state.policy_revision);
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
            json!(
                Store::open(root.path())
                    .unwrap()
                    .read()
                    .unwrap()
                    .policy_revision
            ),
            initial
        );
    }
    for agent in [None, Some("acme-agent"), Some("claude")] {
        let mut other = context.clone();
        other.agent = agent.map(str::to_owned);
        let result =
            call_with_confirmation(&other, &connection, "policy_apply", args, &|| Ok(()), true);
        assert!(!result["error"].is_null(), "{result}");
    }
    let fresh = fresh_prepare(
        &context,
        &connection,
        &json!({"kind":"rule_add","rule":rule("acme-readable", "allow")}),
        "after-invalid-readable",
    );
    apply(&context, &connection, &fresh);
    apply(&context, &connection, &fresh);
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
    let initial = json!(
        Store::open(root.path())
            .unwrap()
            .read()
            .unwrap()
            .policy_revision
    );
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
        json!(
            Store::open(root.path())
                .unwrap()
                .read()
                .unwrap()
                .policy_revision
        ),
        initial
    );
    let stale = proposal(&context, &connection, "acme-stale");
    let mut independent = context.clone();
    independent.connection_id = Some("independent-review".into());
    let independent_connection = Connection::new("independent-review".into());
    let fresh = proposal(&independent, &independent_connection, "acme-fresh");
    apply(
        &independent,
        &independent_connection,
        &fresh["data"]["apply_arguments"],
    );
    let active = json!(
        Store::open(root.path())
            .unwrap()
            .read()
            .unwrap()
            .policy_revision
    );
    let refused = invoke(
        &context,
        &connection,
        "policy_apply",
        stale["data"]["apply_arguments"].clone(),
    );
    assert_eq!(refused["error"]["code"], "stale_revision");
    assert_eq!(
        json!(
            Store::open(root.path())
                .unwrap()
                .read()
                .unwrap()
                .policy_revision
        ),
        active
    );
}

#[test]
fn withdrawing_a_proposal_after_its_base_changed_reports_the_stale_revision() {
    let (root, mut context, connection) = fixture();
    context.agent = Some("claude".into());
    let mut other = context.clone();
    other.connection_id = Some("independent-review".into());
    let other_connection = Connection::new("independent-review".into());
    let first = proposal(&other, &other_connection, "acme-first");
    let second = proposal(&context, &connection, "acme-second");
    apply(&other, &other_connection, &first["data"]["apply_arguments"]);
    let id = second["data"]["proposal_id"].clone();
    for _ in 0..2 {
        let result = invoke(
            &context,
            &connection,
            "policy_withdraw",
            json!({"proposal_id":id}),
        );
        assert!(result["error"].is_null(), "{result}");
        assert_eq!(result["data"]["status"], "stale");
    }
    let state = Store::open(root.path()).unwrap().read().unwrap();
    let row = state.proposals.iter().find(|p| p.id == id).unwrap();
    assert_eq!(row.status, "stale");
}

#[test]
fn a_cache_location_change_during_review_survives_applying_the_proposal() {
    let (root, mut context, connection) = fixture();
    context.agent = Some("claude".into());
    let pending = proposal(&context, &connection, "acme-before-relocation");
    let relocated = assert_fs::TempDir::new().unwrap();
    let cache = relocated.path().join("cache");
    crate::review::files::relocate_cache(&context, &cache).unwrap();
    let location =
        Store::open(root.path()).unwrap().read().unwrap().operator["compiled_cache_root"].clone();
    assert!(location.is_string());
    // A proposal writes policy files; the cache location stays the current one.
    apply(&context, &connection, &pending["data"]["apply_arguments"]);
    let state = Store::open(root.path()).unwrap().read().unwrap();
    assert_eq!(state.operator["compiled_cache_root"], location);
}

#[test]
fn a_pending_proposal_whose_base_changed_becomes_stale() {
    let (root, mut context, connection) = fixture();
    context.agent = Some("claude".into());
    let mut other = context.clone();
    other.connection_id = Some("independent-review".into());
    let other_connection = Connection::new("independent-review".into());
    let first = proposal(&other, &other_connection, "acme-first");
    let second = proposal(&context, &connection, "acme-second");
    let id = second["data"]["proposal_id"].clone();
    // Before the base changes, the proposal can still be applied.
    let pending = invoke(
        &context,
        &connection,
        "policy_show",
        json!({"proposal_id":id}),
    );
    assert_eq!(pending["data"]["status"], "pending", "{pending}");
    assert!(pending["data"]["apply_arguments"].is_object(), "{pending}");
    apply(&other, &other_connection, &first["data"]["apply_arguments"]);
    let shown = invoke(
        &context,
        &connection,
        "policy_show",
        json!({"proposal_id":id}),
    );
    assert!(shown["error"].is_null(), "{shown}");
    assert_eq!(shown["data"]["status"], "stale");
    // Closure follows the changed base, without an attempted application.
    let state = Store::open(root.path()).unwrap().read().unwrap();
    let row = state.proposals.iter().find(|p| p.id == id).unwrap();
    assert_eq!(row.status, "stale");
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
        crate::review::runtime::consumer::load(&context, &|| Ok(())).unwrap();
        let active = json!(state.policy_revision);
        apply(&context, &connection, &args);
        assert_eq!(
            json!(
                Store::open(root.path())
                    .unwrap()
                    .read()
                    .unwrap()
                    .policy_revision
            ),
            active
        );
    }
}

#[test]
fn host_application_refuses_changed_stale_withdrawn_or_unregistered_requests() {
    let (root, mut context, connection) = fixture();
    context.agent = Some("claude".into());
    let args = prepare(
        &context,
        &connection,
        &json!({"kind":"rule_add","rule":rule("read", "allow")}),
    );
    let initial = json!(
        Store::open(root.path())
            .unwrap()
            .read()
            .unwrap()
            .policy_revision
    );
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
        json!(
            Store::open(root.path())
                .unwrap()
                .read()
                .unwrap()
                .policy_revision
        ),
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
    let mut independent = context.clone();
    independent.connection_id = Some("independent-review".into());
    let independent_connection = Connection::new("independent-review".into());
    let fresh = prepare(
        &independent,
        &independent_connection,
        &json!({"kind":"rule_add","rule":rule("new", "deny")}),
    );
    apply(&independent, &independent_connection, &fresh);
    assert_eq!(
        invoke(&context, &connection, "policy_apply", stale)["error"]["code"],
        "stale_revision"
    );
}

#[test]
fn host_application_refuses_after_a_hand_edit_and_keeps_the_edit() {
    let (root, mut context, connection) = fixture();
    context.agent = Some("claude".into());
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
    // The review no longer matches the files: prepare it again and ask again.
    assert_eq!(result["error"]["code"], "stale_revision", "{result}");
    assert_eq!(std::fs::read_to_string(&rules).unwrap(), edited);
}

#[test]
fn host_approved_pack_add_configuration_update_and_removal_share_the_operator_pipeline() {
    let (root, mut context, connection) = fixture();
    context.agent = Some("claude".into());
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
