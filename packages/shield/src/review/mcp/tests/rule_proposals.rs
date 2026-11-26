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
            "context":{"cwd":Path::new(&context.project_root).join("child"),"environment":{}},
            "candidate_id":proposed["data"]["after_candidate_id"]});
        let child = invoke(&context, &connection, "policy_evaluate", child_query);
        assert_eq!(child["data"]["decision"], "ask", "{child}");
    }
}

#[test]
fn codex_prefix_proposal_publishes_exact_rule_after_host_approval() {
    use crate::review::{integrations, project};

    let rule = json!({
        "id":"smoke-prefix",
        "decision":"allow",
        "reason":"Run the Acme smoke test with its local browser runtime.",
        "conditions":{"domain":"argv","executable":"node",
            "arguments":{"kind":"exact","values":["scripts/smoke.cjs"]},
            "cwd":{"exact":{"root":"project"}}},
        "agents":{"codex":{"prefixes":[["node","scripts/smoke.cjs"]]}}
    });
    for (tool, arguments) in [
        (
            "policy_propose",
            json!({"change":{"kind":"rule_add","layer":"project","rule":rule}}),
        ),
        ("rule_add", json!({"layer":"project","rule":rule})),
    ] {
        let (root, mut context, _connection) = fixture();
        context.agent = Some("codex".into());
        let before = project::checkpoint(&context).unwrap();
        let metadata = project::metadata(&before);
        let plan = integrations::setup::prepare(
            root.path(),
            &metadata,
            integrations::IntegrationChoice::agent("codex"),
        )
        .unwrap()
        .unwrap();
        integrations::apply(&context, &metadata, &plan).unwrap();

        let transport = crate::mcp::tests::Transport::start(
            crate::mcp::Server::for_review(context.clone()).unwrap(),
        );
        let [initialize, ready] = crate::mcp::tests::initialize(&json!({}), "codex");
        assert!(transport.request(&initialize)["error"].is_null());
        transport.send(&ready);
        let reply = transport.call(json!(2), tool, &arguments);
        let proposal = &reply["result"]["structuredContent"]["data"];
        assert_eq!(proposal["status"], "pending", "{tool}: {reply}");
        let apply = &proposal["apply_arguments"];
        assert_eq!(
            apply["change"],
            "Grant Codex host access outside its sandbox."
        );
        assert_eq!(
            apply["details"].as_str().unwrap().lines().next(),
            Some(r#"["node", "scripts/smoke.cjs"]"#),
        );
        let prefix_file = root.path().join(".codex/rules/shield.rules");
        assert!(!prefix_file.exists());
        assert_eq!(
            project::checkpoint(&context).unwrap().policy_revision,
            before.policy_revision
        );

        let reply = transport.call(json!(3), "policy_apply", apply);
        let applied = &reply["result"]["structuredContent"]["data"];
        assert_eq!(applied["status"], "applied", "{tool}: {reply}");
        assert_eq!(applied["operator"]["mechanism"], "host-tool-permission");
        let published = std::fs::read_to_string(prefix_file).unwrap();
        assert!(
            published.contains(r#"pattern = ["node", "scripts/smoke.cjs"]"#),
            "{published}"
        );
        transport.close().1.unwrap();
    }
}

#[test]
fn codex_prefix_proposal_preserves_argument_path_checks_and_review() {
    let (root, mut context, connection) = fixture();
    context.agent = Some("codex".into());
    std::fs::write(root.path().join("Cargo.toml"), "[workspace]\n").unwrap();
    let before = Store::open(root.path()).unwrap().read_checkpoint().unwrap();
    let grammar = json!({"kind":"options","prefix":["nextest","run"],
        "flags":["--locked"],"values":["--manifest-path","-E"],"operands":false});
    let rule = json!({
        "id":"nextest-prefix",
        "decision":"allow",
        "conditions":{"domain":"argv","executable":"cargo","arguments":grammar,
            "cwd":{"within":{"root":"project"}}},
        "requirements":{"argument_paths":{"grammar":grammar,"options":{
            "--manifest-path":{"path":{"within":{"root":"project"}}}
        }}},
        "on_unmet":{"decision":"deny"},
        "agents":{"codex":{"prefixes":[["cargo","nextest","run"]]}}
    });
    let change = json!({"change":{"kind":"rule_add","layer":"local","rule":rule}});
    let schemas = tools();
    for (name, args) in [
        ("policy_propose", change.clone()),
        ("rule_add", json!({"layer":"local","rule":rule})),
        (
            "rule_edit",
            json!({"layer":"local","rule_id":"nextest-prefix","rule":rule}),
        ),
    ] {
        let schema = &schemas["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|tool| tool["name"] == name)
            .unwrap()["inputSchema"];
        canonic_mcp_kit::schema::validate(schema, &args).unwrap();
    }
    let mut wider = change.clone();
    wider["change"]["rule"]["agents"]["codex"]["prefixes"] = json!([["cargo", "nextest"]]);
    let refused = invoke(&context, &connection, "policy_propose", wider);
    assert!(!refused["error"].is_null(), "{refused}");

    let proposed = invoke(&context, &connection, "policy_propose", change);
    assert!(proposed["error"].is_null(), "{proposed}");
    assert_eq!(proposed["data"]["status"], "pending");
    let apply = &proposed["data"]["apply_arguments"];
    assert!(
        apply["change"]
            .as_str()
            .unwrap()
            .contains("outside its sandbox"),
        "{apply}"
    );
    let outside = root.path().join("../outside/Cargo.toml");
    for (manifest, expected) in [("Cargo.toml", "allow"), (outside.to_str().unwrap(), "deny")] {
        let query = json!({
            "candidate_id":proposed["data"]["after_candidate_id"],
            "input":{"kind":"argv","executable":"cargo",
                "arguments":["nextest","run","--locked","--manifest-path",manifest,"-E","test(smoke)"],
                "original_input":{}},
            "context":{"cwd":context.project_root}
        });
        let preview = invoke(&context, &connection, "policy_evaluate", query);
        assert_eq!(preview["data"]["decision"], expected, "{preview}");
    }
    let pending = Store::open(root.path()).unwrap().read_checkpoint().unwrap();
    assert_eq!(pending.policy_revision, before.policy_revision);
    assert_eq!(pending.proposals.len(), 1);
    let revision = application::apply(&context, &connection, apply);
    assert_ne!(revision["data"]["policy_revision"], before.policy_revision);
    let snapshot = crate::runtime::Project::open(root.path())
        .unwrap()
        .snapshot()
        .unwrap();
    assert!(snapshot.codex_prefixes()[0].covers("cargo", &["nextest", "run", "-E", "test(smoke)"]));
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
        // Neither creation nor inspection can write either rule.
        let shown = transport.call(
            json!(index + 4),
            "policy_show",
            &json!({"proposal_id":proposed["data"]["proposal_id"]}),
        );
        assert_eq!(
            shown["result"]["structuredContent"]["data"]["status"],
            "pending"
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
    use clap::Parser as _;
    let (root, mut context, connection) = fixture();
    context.agent = Some("codex".into());
    let source = dependency_registry(&root, &context, &json!([]), false);
    // Optional immutable public artifact for the reported pack-add workload;
    // no project identity, credentials or settings enter this fixture.
    let development: Value = match std::env::var_os("SHIELD_PACK_PROFILE_ARTIFACT") {
        Some(path) => crate::policy::parse(&std::fs::read_to_string(path).unwrap()).unwrap(),
        None => serde_json::from_str(&development_pack()).unwrap(),
    };
    let catalog_path = source.path().join("catalog.json");
    let mut catalog: Value =
        serde_json::from_slice(&std::fs::read(&catalog_path).unwrap()).unwrap();
    for (index, name) in ["parent", "child"].into_iter().enumerate() {
        let mut pack = development.clone();
        pack["id"] = json!(format!("acme/{name}"));
        pack["version"] = json!("1.0.0");
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
    // Keep payload-free stage timings for deadline failures, whose transport
    // panic otherwise reports only Timeout. Logs are test-owned, printed only
    // on failure, and the original panic is resumed without changing deadlines.
    let logs = assert_fs::TempDir::new().unwrap();
    let cli = crate::cli::Cli::parse_from([
        "shield",
        "--debug-log-dir",
        logs.path().to_str().unwrap(),
        "status",
    ]);
    let log = std::sync::Arc::new(crate::diagnostics::Log::open(&cli));
    let path = log.path().unwrap().to_owned();
    let transport = crate::mcp::tests::Transport::start(
        crate::mcp::Server::for_review(context.clone())
            .unwrap()
            .with_log(log),
    );
    let [initialize, ready] = crate::mcp::tests::initialize(&json!({}), "codex");
    assert!(transport.request(&initialize)["error"].is_null());
    transport.send(&ready);
    let response = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        transport.call(
            json!(10),
            "pack_add",
            &json!({"reference":"acme-dependencies/acme/parent"}),
        )
    }))
    .unwrap_or_else(|panic| {
        eprintln!("{}", std::fs::read_to_string(&path).unwrap());
        std::panic::resume_unwind(panic)
    });
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
