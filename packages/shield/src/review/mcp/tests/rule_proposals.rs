//! Bounded command rules and command/URL proposals preserve activation boundaries.
use super::*;

#[test]
fn development_watch_proposal_and_replay_complete_within_default_deadline() {
    // YAML exercises the installed registry format rather than the faster JSON
    // fixture path. Keep the fixture independent of a registry or user's project.
    let pack: Value = serde_json::from_str(&development_pack()).unwrap();
    let pack = serde_saphyr::to_string(&pack).unwrap();
    let config = json!({"schema_version":1,"rules":"rules.yml",
        "packs":[{"id":"acme/development","version":"1.0.0",
            "path":"development.yml","profile":"recommended"}]});
    let rules = crate::policy::rules::document(&[]).unwrap().to_string();
    let bundle = Bundle::capture(&config.to_string(), &[pack], Some(&rules)).unwrap();
    let (root, mut context, _connection) = fixture_with(&bundle);
    context.agent = Some("codex".into());
    let initial = Store::open(root.path())
        .unwrap()
        .read_checkpoint()
        .unwrap()
        .policy_revision;
    let transport = crate::mcp::tests::Transport::start(
        crate::mcp::Server::for_review(context.clone()).unwrap(),
    );
    let [initialize, ready] = crate::mcp::tests::initialize(&json!({}), "codex");
    assert!(transport.request(&initialize)["error"].is_null());
    transport.send(&ready);
    let input = json!({"change":{"kind":"rule_add","rule":{
        "id":"allow-reviewed-tests","decision":"allow","conditions":{"domain":"argv","executable":"npm",
        "arguments":{"kind":"options","prefix":["test"],"flags":[],"values":[],"operands":{"match":{"exact":"--watch"},"min":0,"max":1}},
        "cwd":{"exact":{"root":"project"}}}
    }}});
    let response = transport.call(json!(2), "policy_propose", &input);
    assert!(response["error"].is_null(), "{response}");
    let first = &response["result"]["structuredContent"];
    assert!(first["error"].is_null(), "{first}");
    let replay = transport.call(json!(3), "policy_propose", &input);
    assert!(replay["error"].is_null(), "{replay}");
    let replay = &replay["result"]["structuredContent"];
    assert!(replay["error"].is_null(), "{replay}");
    assert_eq!(first["data"]["proposal_id"], replay["data"]["proposal_id"]);
    assert_eq!(
        first["data"]["apply_arguments"],
        replay["data"]["apply_arguments"]
    );
    let recovered = transport.call(
        json!(4),
        "policy_show",
        &json!({"proposal_id": first["data"]["proposal_id"]}),
    );
    assert!(recovered["error"].is_null(), "{recovered}");
    let recovered = &recovered["result"]["structuredContent"];
    assert!(recovered["error"].is_null(), "{recovered}");
    assert_eq!(
        first["data"]["apply_arguments"],
        recovered["data"]["apply_arguments"]
    );
    let state = Store::open(root.path()).unwrap().read_checkpoint().unwrap();
    assert_eq!(state.proposals.len(), 1);
    assert_eq!(state.proposals[0].status, "pending");
    assert!(state.proposals[0].receipt.is_null());
    assert_eq!(state.policy_revision, initial);
    transport.close().1.unwrap();
}

#[test]
fn optional_script_flag_rule_covers_base_and_forwarded_flag_without_other_suffixes() {
    let (root, context, connection) = fixture();
    std::fs::create_dir(root.path().join("child")).unwrap();
    let proposed = invoke(
        &context,
        &connection,
        "policy_propose",
        json!({"change":{"kind":"rule_add","rule":{
            "id":"allow-npm-test","decision":"allow",
            "conditions":{"domain":"argv","executable":"npm","cwd":{"exact":{"root":"project"}},"arguments":{
                "kind":"options","prefix":["test"],"flags":[],"values":[],
                "operands":{"match":{"exact":"--watch"},"min":0,"max":1}
            }}
        }}}),
    );
    assert!(proposed["error"].is_null(), "{proposed}");
    for (arguments, expected) in [
        (vec!["test"], "allow"),
        (vec!["test", "--", "--watch"], "allow"),
        (vec!["test", "--"], "allow"),
        (vec!["test", "--watch"], "ask"),
        (vec!["test", "--", "--watch", "--watch"], "ask"),
        (vec!["test", "--", "--watch", "other.test.js"], "ask"),
        (vec!["test", "--", "--import=./other.js"], "ask"),
        (vec!["test", "--", "--watch=false"], "ask"),
        (vec!["test", "--", "--", "--watch"], "ask"),
        (vec!["run", "other", "--", "--watch"], "ask"),
    ] {
        let mut query = json!({"input":{"kind":"argv","executable":"npm",
            "arguments":arguments,"original_input":{}},
            "context":{"cwd":context.project_root,"environment":{}}});
        // A proposal is still inert in the active policy.
        let active = invoke(&context, &connection, "policy_evaluate", query.clone());
        assert_eq!(active["data"]["decision"], "ask", "{active}");
        query["candidate_id"] = proposed["data"]["after_candidate_id"].clone();
        let preview = invoke(&context, &connection, "policy_evaluate", query);
        assert_eq!(
            preview["data"]["decision"], expected,
            "{arguments:?}: {preview}"
        );
        let child_query = json!({"input":{"kind":"argv","executable":"npm",
            "arguments":arguments,"original_input":{}},
            "context":{"cwd":format!("{}/child", context.project_root),"environment":{}},
            "candidate_id":proposed["data"]["after_candidate_id"]});
        let child = invoke(&context, &connection, "policy_evaluate", child_query);
        assert_eq!(child["data"]["decision"], "ask", "{child}");
    }
}

fn development_pack() -> String {
    // Option grammars exercise the retained pack parsing used by development
    // packs without copying registry artifacts or relying on a local registry.
    let flags: Vec<_> = (0..128)
        .map(|index| format!("--acme-development-option-{index}"))
        .collect();
    let rows: Vec<_> = (0..128)
        .map(|index| {
            json!({"id":format!("inspect-{index}"),"decision":"ask",
                "conditions":{"domain":"argv","executable":format!("acme-{index}"),
                    "arguments":{"kind":"options","prefix":["inspect"],
                        "flags":flags,"values":[],"operands":false}}})
        })
        .collect();
    json!({"schema_version":1,"kind":"pack","id":"acme/development",
        "version":"1.0.0","description":"Acme development fixture","license":"MIT",
        "maintainers":["Acme"],"engines":{"shield":"*"},
        "capabilities":["argument-rules-v1","structured-rules-v1"],"rules":rows})
    .to_string()
}

#[test]
fn development_rule_proposals_complete_without_activating_policy() {
    let pack = development_pack();
    let config = json!({"schema_version":1,"rules":"rules.yml",
        "packs":[{"id":"acme/development","version":"1.0.0",
            "path":"development.yml","profile":"recommended"}]});
    let rules = crate::policy::rules::document(&[]).unwrap().to_string();
    let bundle = Bundle::capture(&config.to_string(), &[pack], Some(&rules)).unwrap();
    let (root, mut context, _connection) = fixture_with(&bundle);
    context.agent = Some("codex".into());
    // Exercise the same five-second stdio deadline as the installed server.
    let transport = crate::mcp::tests::Transport::start(
        crate::mcp::Server::for_review(context.clone()).unwrap(),
    );
    let [initialize, ready] = crate::mcp::tests::initialize(&json!({}), "codex");
    assert!(transport.request(&initialize)["error"].is_null());
    transport.send(&ready);
    let initial = Store::open(root.path())
        .unwrap()
        .read()
        .unwrap()
        .policy_revision;
    for (index, rule) in [
        json!({"id":"allow-dev","decision":"allow","conditions":{"domain":"argv",
            "executable":"npm","arguments":{"kind":"exact","values":["run","dev"]}}}),
        json!({"id":"allow-localhost","decision":"allow","conditions":{"domain":"web",
            "action":"fetch","url":{"schemes":["http","https"],
                "hosts":["localhost","127.0.0.1","[::1]"]}}}),
    ]
    .into_iter()
    .enumerate()
    {
        let response = transport.call(
            json!(index + 2),
            "policy_propose",
            &json!({"change":{"kind":"rule_add","rule":rule}}),
        );
        assert!(response["error"].is_null(), "{response}");
        let proposed = &response["result"]["structuredContent"];
        assert!(proposed["error"].is_null(), "{proposed}");
        assert_eq!(proposed["data"]["status"], "pending");
        assert!(proposed["data"]["apply_arguments"]["reference"].is_string());
        // Neither creation nor receipt inspection can activate either rule.
        let receipt = transport.call(
            json!(index + 4),
            "policy_receipt",
            &json!({"proposal_id":proposed["data"]["proposal_id"]}),
        );
        assert_eq!(
            receipt["result"]["structuredContent"]["error"]["code"],
            "operator_review_required"
        );
        assert_eq!(
            Store::open(root.path())
                .unwrap()
                .read()
                .unwrap()
                .policy_revision,
            initial
        );
    }
    transport.close().1.unwrap();
}

#[test]
fn development_pack_proposals_complete_without_activating_policy() {
    let (root, mut context, connection) = fixture();
    context.agent = Some("codex".into());
    let source = dependency_registry(&root, &context, &json!([]), false);
    let development: Value = serde_json::from_str(&development_pack()).unwrap();
    let catalog_path = source.path().join("catalog.json");
    let mut catalog: Value =
        serde_json::from_slice(&std::fs::read(&catalog_path).unwrap()).unwrap();
    for (index, name) in ["parent", "child"].into_iter().enumerate() {
        let mut pack = development.clone();
        pack["id"] = json!(format!("acme/{name}"));
        let bytes = pack.to_string();
        std::fs::write(source.path().join(format!("{name}.yml")), &bytes).unwrap();
        catalog["packs"][index]["sha256"] = json!(crate::policy::fingerprint(bytes.as_bytes()));
    }
    std::fs::write(catalog_path, catalog.to_string()).unwrap();
    // Seed a genuine installation so candidate bytes and installed metadata
    // agree, then add another large artifact through the five-second transport.
    commit_pack_change(
        &context,
        &connection,
        json!({"kind":"pack_add", "reference":"acme-dependencies/acme/child"}),
        "seed",
    );
    let initial = Store::open(root.path())
        .unwrap()
        .read()
        .unwrap()
        .policy_revision;
    let transport = crate::mcp::tests::Transport::start(
        crate::mcp::Server::for_review(context.clone()).unwrap(),
    );
    let [initialize, ready] = crate::mcp::tests::initialize(&json!({}), "codex");
    assert!(transport.request(&initialize)["error"].is_null());
    transport.send(&ready);
    let response = transport.call(
        json!(10),
        "policy_propose",
        &json!({
        "change":{"kind":"pack_add","reference":"acme-dependencies/acme/parent"}}),
    );
    assert!(response["error"].is_null(), "{response}");
    let proposed = &response["result"]["structuredContent"];
    assert!(proposed["error"].is_null(), "{proposed}");
    assert_eq!(proposed["data"]["status"], "pending");
    assert!(proposed["data"]["apply_arguments"]["reference"].is_string());
    assert_eq!(
        Store::open(root.path())
            .unwrap()
            .read()
            .unwrap()
            .policy_revision,
        initial
    );
    transport.close().1.unwrap();
}
