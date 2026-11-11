//! Acceptance fixtures use historical sources and their canonical struct encodings.
//! Current capture/composition must not manufacture the historical authority.
use super::{retained, source_name};
use crate::{
    policy,
    review::{self, runtime::consumer, storage::Store},
};
use serde_json::{Value, json};
use std::{fs, path::PathBuf};

const CONFIG: &str = r#"{"schema_version":1,"packs":[{"id":"acme/legacy","registry_identity":"acme-main","version":"0.1.0","profile":"recommended","path":"legacy.yml"}],"rules":"rules.yml"}"#;

fn pack(schema: u8) -> String {
    let capabilities = match schema {
        1 => r#"["argv-v1"]"#,
        2 => r#"["argv-v1","possible-effects-v1"]"#,
        3 => r#"["argv-v1","dependencies-v1"]"#,
        _ => unreachable!(),
    };
    let dependencies = if schema == 3 {
        ",\"dependencies\":[]"
    } else {
        ""
    };
    format!(
        r#"{{"schema_version":{schema},"id":"acme/legacy","version":"0.1.0","description":"Historical fixture","license":"MIT","maintainers":["Acme"],"engine":"^0.1.0","capabilities":{capabilities},"rules":[{{"id":"inspect","matcher":{{"executable":"git","arguments":{{"kind":"exact","values":["status"]}}}},"reason":"Inspect","profiles":{{"loose":"allow","recommended":"allow","strict":"allow"}}}},{{"id":"destroy","matcher":{{"executable":"git","arguments":{{"kind":"prefix","values":["reset"]}}}},"reason":"Destroy","profiles":{{"loose":"deny","recommended":"deny","strict":"deny"}}}},{{"id":"publish","matcher":{{"executable":"git","arguments":{{"kind":"prefix","values":["push"]}}}},"reason":"Publish","profiles":{{"loose":"ask","recommended":"ask","strict":"ask"}}}}]{dependencies}}}"#
    )
}

fn fixture(pack_schema: u8, custom_schema: u8) -> (assert_fs::TempDir, review::Context, Value) {
    let root = assert_fs::TempDir::new().unwrap();
    // Bootstrap only the private directory and enrolled project identity.
    let bootstrap =
        crate::configuration::Bundle::capture("schema_version: 1\npacks: []\n", &[], None).unwrap();
    let context = review::mcp::initialize_fixture(root.path(), &bootstrap);
    let current = Store::open(root.path()).unwrap().read_checkpoint().unwrap();
    let mut rules = Vec::new();
    for decision in ["allow", "deny", "ask"] {
        let path = PathBuf::from(&context.project_root).join(format!("{decision}.txt"));
        fs::write(&path, format!("{decision} fixture\n")).unwrap();
        rules.push(json!({"id":format!("read-{decision}"),"scope":"project","mandatory":false,
            "decision":decision,"matcher":{"executable":"shield:read-text","arguments":{"kind":"exact","values":[path]}},"reason":"Historical SDK read"}));
    }
    let custom = json!({"schema_version":custom_schema,"rules":rules}).to_string();
    let pack = pack(pack_schema);
    let pack_digest = policy::fingerprint(pack.as_bytes());
    let rules_digest = policy::fingerprint(custom.as_bytes());
    // These strings reproduce historical typed serialization order, not Value order.
    let selection = format!(
        r#"{{"id":"acme-main/acme/legacy","version":"0.1.0","profile":"recommended","artifact":{{"path":"legacy.yml","digest":"{pack_digest}"}}}}"#
    );
    let rules_artifact = format!(r#"{{"path":"rules.yml","digest":"{rules_digest}"}}"#);
    let lock = format!(
        r#"{{"schema_version":1,"kind":"local-candidate","config_digest":"{}","packs":[{selection}],"rules":{rules_artifact}}}"#,
        policy::fingerprint(CONFIG.as_bytes())
    );
    let lock_revision = policy::fingerprint(lock.as_bytes());
    let effective = format!(
        r#"{{"schema_version":1,"kind":"effective-candidate","scopes":[{{"scope":"project","lock_revision":"{lock_revision}","lock":{lock}}}],"packs":[{{"scope":"project","selection":{selection},"shadowed":[]}}],"custom_rules":[{{"scope":"project","artifact":{rules_artifact}}}]}}"#
    );
    let revision = policy::fingerprint(effective.as_bytes());
    let bundle = json!({"schema_version":1,"kind":"candidate-bundle","trust":"unverified",
        "config":CONFIG,"packs":[pack],"rules":custom,"lock":serde_json::from_str::<Value>(&lock).unwrap()});
    let candidates = json!({"project":bundle});
    // The original candidate digest, unlike Effective/Lock, used canonical Value maps.
    let digest = policy::fingerprint(candidates.to_string().as_bytes());
    let activation = "activation-legacy-source";
    let binding = policy::fingerprint(
        json!([
            current.project_id,
            current.project_root,
            activation,
            null,
            revision,
            digest
        ])
        .to_string()
        .as_bytes(),
    );
    let installed = json!({"selector":"pack:11111111-1111-1111-1111-111111111111",
        "id":"acme/legacy","version":"0.1.0","profile":"recommended","digest":pack_digest,
        "source_identity":"acme-main","source_alias":"project-files",
        "source":{"identity":"acme-main","location":root.path().join(".shield"),"key":null},"pinned":true,"direct":true});
    let activation_event = json!({
            "schema_version":1,"event_id":"event-legacy-source","kind":"runtime_activated",
            "recorded_at":"2025-01-01T00:00:00Z","occurred_at":null,
            "provenance":{"project_id":current.project_id,"project_root":current.project_root,
                "cwd":null,"surface":"cli","agent":null,"host_version":null,"session_id":null,
                "tool_use_id":null,"connection_id":null,"host_event_name":null,
                "sources":{"project_id":"registration","project_root":"registration","surface":"registration",
                    "cwd":"unavailable","agent":"unavailable","host_version":"unavailable","session_id":"unavailable",
                    "tool_use_id":"unavailable","connection_id":"unavailable","host_event_name":"unavailable"}},
            "links":{"activation_id":activation,"request_id":null,"proposal_id":null,"approval_id":null,
                "delegation_id":null,"run_id":null,"source_event_id":null,"decision_event_id":null,"supersedes_proposal_id":null},
            "reasons":{"agent_reason":null,"policy_reason":null,"operator_reason":null},
            "operator":{"operator_id":null,"display_name":null,"mechanism":"operator-cli"},
            "details":{"binding":binding,"policy_revision":revision,"candidate_digest":digest},"redacted":false});
    let active = json!({"activation_id":activation,"previous_activation_id":null,"policy_revision":revision,
        "candidate_digest":digest,"candidates":candidates,"binding":binding});
    let operator = json!({"sources":{},"default":null,"installed":[installed],"projection":null});
    let raw = json!({"schema_version":1,"project_id":current.project_id,"project_root":current.project_root,
        "sequence":42,"policy_revision":revision,"candidates":candidates,"registry_checkpoints":{},
        "active":active,"operator":operator,
        "read_requests":[],"run_requests":[],"proposals":[],"events":[],"history_generation":null,
        "history_enabled":true,"history_limit":5000,"authorization_events":[activation_event]});
    for (name, bytes) in [
        ("config.yml", CONFIG),
        ("legacy.yml", pack.as_str()),
        ("rules.yml", custom.as_str()),
    ] {
        fs::write(root.path().join(".shield").join(name), bytes).unwrap();
    }
    (root, context, raw)
}

fn current_context(root: &std::path::Path, mut previous: review::Context) -> review::Context {
    let current = review::open(root).unwrap();
    previous.project_id = current.project_id;
    previous.project_root = current.project_root;
    previous
}

fn assert_decisions(context: &review::Context, revision: &str) {
    let snapshot = consumer::load(context, &|| Ok(())).unwrap();
    assert_eq!(snapshot.identity().policy_revision(), revision);
    for (arguments, expected) in [
        (vec!["status"], policy::Decision::Allow),
        (vec!["reset", "--hard"], policy::Decision::Deny),
        (vec!["push"], policy::Decision::Ask),
    ] {
        let evidence = snapshot
            .evaluate(policy::Request {
                executable: "git".into(),
                arguments: arguments.into_iter().map(str::to_owned).collect(),
                cwd: context.project_root.clone(),
                project: context.project_root.clone(),
            })
            .unwrap();
        assert_eq!(evidence.evaluation().decision, expected);
    }
}

fn compiled_path(state: &review::model::State) -> PathBuf {
    let active = serde_json::to_value(state.active.as_ref().unwrap()).unwrap();
    let artifact = &active["compiled"];
    PathBuf::from(artifact["directory"].as_str().unwrap())
        .join("compiled")
        .join(artifact["project_key"].as_str().unwrap())
        .join(format!("{}.bin", artifact["plan_key"].as_str().unwrap()))
}

#[test]
fn upgrade_authentic_source_reaches_mcp_confirmation_with_a_missing_cache() {
    let (root, mut context, raw) = fixture(1, 1);
    fs::write(
        root.path().join(".shield/review/state.json"),
        raw.to_string(),
    )
    .unwrap();
    context.surface = "mcp".into();
    context.agent = Some("claude".into());
    context.connection_id = Some("post-upgrade-review".into());
    let connection = review::mcp::Connection::enrolled(&context).unwrap();
    let before = Store::open(root.path()).unwrap().read_checkpoint().unwrap();
    let proposed = review::mcp::call(
        &context,
        &connection,
        "policy_propose",
        &json!({
            "idempotency_key":"post-upgrade-review",
            "change":{"kind":"rule_add","rule":{"id":"new-read","decision":"allow",
                "conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"exact","values":["read"]}}}}
        }),
        &|| Ok(()),
    );
    assert!(proposed["error"].is_null(), "{proposed}");
    assert_eq!(proposed["project_id"], before.project_id);
    context = current_context(root.path(), context);
    let args = &proposed["data"]["apply_arguments"];
    assert!(args.is_object(), "{proposed}");
    fs::remove_file(compiled_path(&before)).unwrap();
    let prompt = review::mcp::apply_confirmation(&context, &connection, args)
        .unwrap()
        .unwrap();
    assert!(!prompt.is_empty());
    assert_eq!(
        Store::open(root.path())
            .unwrap()
            .read_checkpoint()
            .unwrap()
            .policy_revision,
        before.policy_revision
    );
    let applied = review::mcp::call_with_confirmation(
        &context,
        &connection,
        "policy_apply",
        args,
        &|| Ok(()),
        true,
    );
    assert!(applied["error"].is_null(), "{applied}");
    assert_eq!(applied["data"]["status"], "applied");
    let after = Store::open(root.path()).unwrap().read_checkpoint().unwrap();
    assert_ne!(after.policy_revision, before.policy_revision);
    assert_decisions(&context, &after.policy_revision);
}

#[test]
fn upgrade_authentic_source_accepts_reviewed_changes_and_preserves_existing_decisions() {
    let (root, context, raw) = fixture(1, 1);
    fs::write(
        root.path().join(".shield/review/state.json"),
        raw.to_string(),
    )
    .unwrap();
    let context = current_context(root.path(), context);
    let before = Store::open(root.path()).unwrap().read_checkpoint().unwrap();
    let proposed = review::tests::propose(
        &context,
        &review::tests::input("post-upgrade-read"),
        &|| Ok(()),
    )
    .unwrap();
    let id = proposed["proposal_id"].as_str().unwrap();
    review::operator::commit_proposal_fixture(&context, id).unwrap();
    let after = Store::open(root.path()).unwrap().read_checkpoint().unwrap();
    assert_ne!(after.policy_revision, before.policy_revision);
    assert_ne!(
        after.active.as_ref().unwrap().activation_id(),
        before.active.as_ref().unwrap().activation_id()
    );
    assert_decisions(&context, &after.policy_revision);
    let snapshot = consumer::load(&context, &|| Ok(())).unwrap();
    let evidence = snapshot
        .evaluate(policy::Request {
            executable: "acme".into(),
            arguments: vec!["read".into()],
            cwd: context.project_root.clone(),
            project: context.project_root.clone(),
        })
        .unwrap();
    assert_eq!(evidence.evaluation().decision, policy::Decision::Allow);
    let source = Store::open(root.path())
        .unwrap()
        .read_current()
        .unwrap()
        .candidates["project"]["rules"]
        .clone();
    let document: Value = policy::parse(source.as_str().unwrap()).unwrap();
    assert!(
        document["rules"]
            .as_array()
            .unwrap()
            .iter()
            .any(|rule| rule["historical_matcher"].is_object())
    );
}

#[test]
fn upgrade_authentic_sources_preserve_original_authority_and_all_entry_points() {
    for pack_schema in [1, 2, 3] {
        for custom_schema in [1, 2] {
            let (root, context, raw) = fixture(pack_schema, custom_schema);
            let original = serde_json::to_vec(&raw).unwrap();
            fs::write(root.path().join(".shield/review/state.json"), &original).unwrap();
            let store = Store::open(root.path()).unwrap();
            let context = current_context(root.path(), context);
            let state = store.read_checkpoint().unwrap();
            // At its own root the older writer's record upgrades in place: the
            // same project, lineage and original activation evidence.
            assert_eq!(state.project_id, raw["project_id"].as_str().unwrap());
            assert_eq!(state.policy_lineage(), raw["project_id"].as_str().unwrap());
            assert_eq!(state.policy_revision, raw["policy_revision"]);
            assert!(state.operator.get("runtime_incarnation").is_none());
            let active = serde_json::to_value(state.active.as_ref().unwrap()).unwrap();
            for field in ["policy_revision", "candidate_digest", "activation_id"] {
                assert_eq!(active[field], raw["active"][field]);
            }
            super::tests::assert_converted_binding(&state, &raw);
            assert!(
                state
                    .authorization_events
                    .iter()
                    .any(|event| event["event_id"] == "event-legacy-source")
            );
            assert!(
                !state
                    .authorization_events
                    .iter()
                    .any(|event| event["operator"]["mechanism"] == "reviewed-policy-reuse")
            );
            assert_eq!(retained(&store, &source_name(&original)).unwrap(), original);
            let revision = raw["policy_revision"].as_str().unwrap();
            assert_decisions(&context, revision);
            let artifact = &active["compiled"];
            let cache = PathBuf::from(artifact["directory"].as_str().unwrap())
                .join("compiled")
                .join(artifact["project_key"].as_str().unwrap())
                .join(format!("{}.bin", artifact["plan_key"].as_str().unwrap()));
            assert!(cache.is_file());
            fs::remove_file(&cache).unwrap();
            assert_decisions(&context, revision);
            assert!(cache.is_file());
            assert_decisions(&context, revision);
            assert_eq!(
                review::project::status(&context).unwrap()["policy_revision"],
                revision
            );
            let doctor = review::project::doctor(&context).unwrap();
            assert_eq!(doctor["policy_revision"], revision);
            assert!(
                !doctor["checks"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|check| check["check"] == "policy" || check["check"] == "policy_source"),
                "{doctor}"
            );
            let mut mcp = context.clone();
            mcp.surface = "mcp".into();
            mcp.connection_id = Some("legacy-source-connection".into());
            let connection = review::mcp::Connection::enrolled(&mcp).unwrap();
            let report = review::mcp::call(&mcp, &connection, "pack_list", &json!({}), &|| Ok(()));
            assert!(report["error"].is_null(), "{report}");
            let historical_display =
                review::mcp::call(&mcp, &connection, "policy_show", &json!({}), &|| Ok(()));
            assert!(
                historical_display["error"]["message"]
                    .as_str()
                    .is_some_and(|message| message.contains("historical custom display")),
                "{historical_display}"
            );
            for (decision, expected, code) in [
                ("allow", "allow", "allowed"),
                ("deny", "deny", "policy-deny"),
                ("ask", "deny", "approval-required"),
            ] {
                let input = json!({"protocol_version":1,"request_id":format!("legacy-{decision}"),"expected_project_id":context.project_id,
                    "cwd":context.project_root,"operation":{"kind":"read-text","path":PathBuf::from(&context.project_root).join(format!("{decision}.txt"))},
                    "host":{"agent":"claude-mod","session_id":"legacy-session","tool_use_id":"legacy-tool",
                        "event":"PreToolUse","consumer_id":null}});
                let (receipt, exit) = crate::enforcement::protocol::evaluate(
                    root.path(),
                    &serde_json::to_vec(&input).unwrap(),
                );
                assert_eq!(exit, 0, "{receipt}");
                assert_eq!(receipt["decision"], expected, "{receipt}");
                assert_eq!(receipt["code"], code, "{receipt}");
                // This source fixture retains no installed interceptor; a caller
                // label alone must not select its historical receipt format.
                assert_eq!(receipt.as_object().unwrap().len(), 9, "{receipt}");
                assert_eq!(receipt["identity"]["policy_revision"], revision);
            }
        }
    }
}
