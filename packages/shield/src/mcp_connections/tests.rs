use super::*;
use crate::{
    configuration::{Candidate, Evaluator},
    policy::{
        Scope,
        normalized::{Context, Input},
    },
};
use serde_json::json;

fn pack() -> String {
    json!({"schema_version":1,"kind":"pack","id":"acme/cloud","version":"1.0.0","description":"Acme cloud permissions","license":"MIT","maintainers":["Acme"],"engines":{"shield":"*"},
        "capabilities":["structured-rules-v1","argument-rules-v1","mcp-integrations-v1"],
        "mcp_integrations":{"cloud":{"transport":"http","fields":{"/url":{"exact":"https://cloud.example.com/mcp"}}}},
        "rules":[{"id":"cli","decision":"allow","conditions":{"domain":"argv","executable":"acme-cloud","arguments":{"kind":"any"}}},
            {"id":"read","decision":"allow","conditions":{"domain":"mcp","server":"cloud","tool":"inspect"}},
            {"id":"destroy","decision":"deny","conditions":{"domain":"mcp","server":"cloud","tool":"delete"}}]}).to_string()
}
fn connection() -> Value {
    json!({"transport":{"type":"http","url":"https://cloud.example.com/mcp"},"agents":{"claude":"my_cloud","codex":"another_cloud"},"packs":{"acme/cloud":"cloud"}})
}
fn config(mcp: Value) -> Value {
    let mut document = json!({"schema_version":1,"packs":[{"id":"acme/cloud","version":"1.0.0","profile":"recommended","path":"pack.yml"}]});
    document["mcp"] = mcp;
    document
}
fn evaluator(document: &Value) -> Result<Evaluator, Error> {
    Evaluator::new(vec![(
        Scope::Project,
        Candidate::from_yaml(&document.to_string())?.capture(&[pack()], None)?,
    )])
}
fn decide(evaluator: &Evaluator, server: &str, tool: &str) -> Value {
    evaluator
        .evaluate_normalized(
            &Input::Mcp {
                server: server.into(),
                tool: tool.into(),
                arguments: json!({}),
                connection: None,
                original_input: json!({}),
            },
            &Context {
                cwd: "/acme".into(),
                environment: None,
                working_files: None,
            },
            "/acme",
            None,
            "2026-01-01T00:00:00Z",
            &|| Ok(()),
        )
        .unwrap()["decision"]
        .clone()
}

#[test]
fn associated_mcp_rules_bind_to_project_identity_in_source_and_restored_plans() {
    let evaluator = evaluator(&config(json!({"project-cloud":connection()}))).unwrap();
    let restored =
        Evaluator::from_authenticated_compiled(evaluator.compiled_bytes().to_vec()).unwrap();
    for image in [&evaluator, &restored] {
        assert_eq!(decide(image, "project-cloud", "inspect"), "allow");
        assert_eq!(decide(image, "project-cloud", "delete"), "deny");
        assert_eq!(decide(image, "cloud", "inspect"), "ask");
        assert_eq!(decide(image, "my_cloud", "inspect"), "ask");
    }
}

#[test]
fn mcp_connections_packs_cannot_bypass_associations_with_exact_requests() {
    let mut source: Value = policy::parse(&pack()).unwrap();
    source["rules"].as_array_mut().unwrap().push(json!({"id":"unbound-exact","decision":"allow","conditions":{"domain":"exact_request",
        "input":{"kind":"mcp","server":"project-cloud","tool":"inspect","arguments":{},"original_input":{}},
        "context":{"cwd":"/acme","environment":null},"executable":null}}));
    assert!(matches!(
        policy::Pack::from_yaml(&source.to_string()),
        Err(Error::Contract(
            "pack MCP rules require integration-scoped mcp conditions"
        ))
    ));
}

#[test]
fn absent_disabled_and_unassociated_mcp_leave_commands_working_without_mcp_allowances() {
    let mut disabled = connection();
    disabled["enabled"] = json!(false);
    let mut unassociated = connection();
    unassociated["packs"] = json!({});
    for mcp in [
        json!({}),
        json!({"project-cloud":disabled}),
        json!({"project-cloud":unassociated}),
    ] {
        let image = evaluator(&config(mcp)).unwrap();
        assert_eq!(decide(&image, "project-cloud", "inspect"), "ask");
        let request = policy::Request {
            executable: "acme-cloud".into(),
            arguments: vec!["inspect".into()],
            cwd: "/acme".into(),
            project: "/acme".into(),
        };
        assert_eq!(
            image.evaluate(&request).unwrap().decision,
            policy::Decision::Allow
        );
    }
}

#[test]
fn mcp_precondition_binding_drops_only_unavailable_server_alternatives() {
    let source = multi_integration_gate(&json!([
        {"server":"cloud","fields":{"/mode":{"text":"read"}}},
        {"server":"spare","fields":{"/mode":{"text":"backup"}}},
        {"fields":{"/mode":{"text":"universal"}}}
    ]));
    let mut spare = connection();
    spare["packs"] = json!({"acme/cloud":"spare"});
    spare["agents"] = json!({"claude":"my_spare","codex":"another_spare"});
    let mut disabled = spare.clone();
    disabled["enabled"] = json!(false);
    let mut unassociated = spare.clone();
    unassociated["packs"] = json!({});
    let mut unbound = spare.clone();
    unbound["agents"] = json!({});
    for inactive in [None, Some(disabled), Some(unassociated), Some(unbound)] {
        // The active project's ID intentionally equals the unavailable pack
        // integration name: leaving the original selector could grant access.
        let mut mcp = json!({"spare":connection()});
        if let Some(connection) = inactive {
            mcp["project-spare"] = connection;
        }
        let image = gate_image(&source, mcp);
        let restored =
            Evaluator::from_authenticated_compiled(image.compiled_bytes().to_vec()).unwrap();
        for image in [&image, &restored] {
            assert_eq!(gate_decision(image, "spare", "read"), "allow");
            assert_eq!(gate_decision(image, "spare", "universal"), "allow");
            assert_eq!(gate_decision(image, "spare", "backup"), "deny");
            assert_eq!(gate_decision(image, "project-spare", "backup"), "ask");
            assert_cli_still_allowed(image);
        }
    }
    let image = gate_image(&source, json!({"spare":connection(),"project-spare":spare}));
    assert_eq!(gate_decision(&image, "project-spare", "backup"), "allow");
    assert_eq!(gate_decision(&image, "spare", "backup"), "deny");
}

#[test]
fn mcp_precondition_binding_keeps_empty_active_gate_unmet_in_compiled_plans() {
    let mut source = multi_integration_gate(&json!([{"server":"spare"}]));
    let image = gate_image(&source, json!({"spare":connection()}));
    let restored = Evaluator::from_authenticated_compiled(image.compiled_bytes().to_vec()).unwrap();
    for image in [&image, &restored] {
        assert_eq!(gate_decision(image, "spare", "read"), "deny");
        assert_cli_still_allowed(image);
    }
    // An absent gate scope contributes nothing, even if no alternative binds.
    let image = gate_image(&source, json!({}));
    assert_eq!(gate_decision(&image, "spare", "read"), "ask");
    assert_cli_still_allowed(&image);
    // Only binding may produce this empty normalized form; source still requires
    // at least one alternative and cannot opt into an internal representation.
    source["preconditions"][0]["requires_any"] = json!([]);
    assert!(policy::Pack::from_yaml(&source.to_string()).is_err());
}

fn multi_integration_gate(alternatives: &Value) -> Value {
    let mut source: Value = policy::parse(&pack()).unwrap();
    source["capabilities"]
        .as_array_mut()
        .unwrap()
        .push(json!("preconditions-v1"));
    source["mcp_integrations"]["spare"] = source["mcp_integrations"]["cloud"].clone();
    source["rules"].as_array_mut().unwrap().push(json!({"id":"spare-read","decision":"allow","conditions":{"domain":"mcp","server":"spare","tool":"inspect"}}));
    source["preconditions"] = json!([{"id":"connector-gate","conditions":{"domain":"mcp","server":{"one_of":["cloud","spare"]},"tool":"inspect"},
        "requires_any":alternatives,"on_unmet":{"decision":"deny"}}]);
    source
}

fn gate_image(source: &Value, connections: Value) -> Evaluator {
    let snapshot = Candidate::from_yaml(&config(connections).to_string())
        .unwrap()
        .capture(&[source.to_string()], None)
        .unwrap();
    Evaluator::new(vec![(Scope::Project, snapshot)]).unwrap()
}

fn gate_decision(image: &Evaluator, server: &str, mode: &str) -> Value {
    image
        .evaluate_normalized(
            &Input::Mcp {
                server: server.into(),
                tool: "inspect".into(),
                arguments: json!({"mode":mode}),
                connection: None,
                original_input: json!({}),
            },
            &Context {
                cwd: "/acme".into(),
                environment: None,
                working_files: None,
            },
            "/acme",
            None,
            "2026-01-01T00:00:00Z",
            &|| Ok(()),
        )
        .unwrap()["decision"]
        .clone()
}

fn assert_cli_still_allowed(image: &Evaluator) {
    let request = policy::Request {
        executable: "acme-cloud".into(),
        arguments: vec!["inspect".into()],
        cwd: "/acme".into(),
        project: "/acme".into(),
    };
    assert_eq!(
        image.evaluate(&request).unwrap().decision,
        policy::Decision::Allow
    );
}

#[test]
fn local_connection_replacements_round_trip_and_refuse_wrong_associations() {
    let shared = config(json!({"project-cloud":connection()})).to_string();
    let mut local_connection = connection();
    local_connection["enabled"] = json!(false);
    let local = json!({"schema_version":1,"mcp":{"project-cloud":local_connection}}).to_string();
    let merged = crate::configuration::layers::merge(&shared, Some(&local)).unwrap();
    let candidate = Candidate::from_yaml(&merged).unwrap();
    let snapshot = candidate.capture(&[pack()], None).unwrap();
    let image = Evaluator::new(vec![(Scope::Project, snapshot)]).unwrap();
    assert_eq!(decide(&image, "project-cloud", "inspect"), "ask");
    let (shared_again, local_again) =
        crate::configuration::layers::split(&merged, (Some(&shared), Some(&local))).unwrap();
    let restored =
        crate::configuration::layers::merge(&shared_again, local_again.as_deref()).unwrap();
    assert_eq!(
        policy::parse::<Value>(&merged).unwrap(),
        policy::parse::<Value>(&restored).unwrap()
    );
    let mut wrong = connection();
    wrong["transport"]["url"] = json!("https://other.example.com/mcp");
    assert!(evaluator(&config(json!({"project-cloud":wrong}))).is_err());
    let mut missing = connection();
    missing["packs"] = json!({"acme/absent":"cloud"});
    assert!(evaluator(&config(json!({"project-cloud":missing}))).is_err());
}

#[test]
fn project_only_mcp_rules_need_no_pack_and_alias_collisions_refuse() {
    let rules=json!({"schema_version":1,"kind":"custom","capabilities":["structured-rules-v1"],"rules":[{"id":"inspect","decision":"allow","conditions":{"domain":"mcp","server":"project-cloud","tool":"inspect"}}]}).to_string();
    let candidate=Candidate::from_yaml(&json!({"schema_version":1,"packs":[],"rules":"rules.yml","mcp":{"project-cloud":{"transport":{"type":"http","url":"https://cloud.example.com/mcp"},"agents":{"codex":"cloud"}}}}).to_string()).unwrap();
    let image = Evaluator::new(vec![(
        Scope::Project,
        candidate.capture(&[], Some(&rules)).unwrap(),
    )])
    .unwrap();
    assert_eq!(decide(&image, "project-cloud", "inspect"), "allow");
    assert!(
        Candidate::from_yaml(
            &config(json!({"first":connection(),"second":connection()})).to_string()
        )
        .is_err()
    );
    for alias in ["acme_", "acme__shadow", "Acme", "acme-cloud"] {
        let mut ambiguous = connection();
        ambiguous["agents"] = json!({"codex":alias});
        assert!(Candidate::from_yaml(&config(json!({"cloud":ambiguous})).to_string()).is_err());
    }
    let mut spoofed = connection();
    spoofed["local"] = json!(true);
    let image = evaluator(&config(json!({"cloud":spoofed}))).unwrap();
    assert!(!image.effective().mcp_connections()["cloud"].local);
}

#[test]
fn mcp_defaults_validation_rejects_bad_shapes_before_any_project_binding() {
    for (kind, invalid) in [
        ("string", json!(42)),
        ("string", json!(["acme-private-value"])),
        ("list", json!({"var":"setting"})),
        ("boolean", json!({"var":"setting"})),
    ] {
        let mut source: Value = policy::parse(&pack()).unwrap();
        source["capabilities"]
            .as_array_mut()
            .unwrap()
            .push(json!("variables-v1"));
        let mut declaration = json!({"type":kind,"description":"Connector setting"});
        if kind == "list" {
            declaration["items"] = json!({"type":"string"});
        }
        source["variables"] = json!({"setting":declaration});
        source["mcp_integrations"]["cloud"] = json!({"transport":"stdio","fields":{"/command":{"exact":"acme-connector"}},
            "defaults":{"type":"stdio","command":"acme-connector","env":{"OPTIONAL":{"var":"setting"},"X":invalid}}});
        // OPTIONAL is unset: ordinary resolution would discard these defaults.
        let error = policy::Pack::from_yaml(&source.to_string())
            .err()
            .expect("invalid defaults must fail pack admission");
        assert!(
            matches!(&error, Error::McpDefaults { integration, variables, .. }
            if integration == "cloud" && variables == &["setting"]),
            "{error}"
        );
        assert!(!error.to_string().contains("acme-private-value"));
    }
    // Static defaults use the same named diagnostic, without inventing a variable.
    let mut source: Value = policy::parse(&pack()).unwrap();
    source["mcp_integrations"]["cloud"] = json!({"transport":"stdio","fields":{"/command":{"exact":"acme-connector"}},
        "defaults":{"type":"stdio","command":"acme-connector","env":{"X":42}}});
    let error = policy::Pack::from_yaml(&source.to_string()).err().unwrap();
    assert!(
        matches!(&error, Error::McpDefaults { integration, variables, .. }
        if integration == "cloud" && variables.is_empty()),
        "{error}"
    );
}

#[test]
fn mcp_defaults_validation_accepts_typed_lists_and_unbound_endpoint_strings() {
    let mut source: Value = policy::parse(&pack()).unwrap();
    source["capabilities"]
        .as_array_mut()
        .unwrap()
        .push(json!("variables-v1"));
    source["variables"] = json!({"args":{"type":"list","description":"Connector arguments","items":{"type":"string"}}});
    source["mcp_integrations"]["cloud"] = json!({"transport":"stdio","fields":{"/command":{"exact":"acme-connector"}},
        "defaults":{"type":"stdio","command":"acme-connector","args":{"var":"args"}}});
    assert!(policy::Pack::from_yaml(&source.to_string()).is_ok());
    let binding = policy::Binding {
        variables: std::collections::BTreeMap::from([(
            "args".into(),
            json!(["--profile", "acme-ro"]),
        )]),
        ..Default::default()
    };
    let bound = policy::Pack::from_yaml_bound(&source.to_string(), &binding, false).unwrap();
    assert_eq!(
        bound.mcp_default("cloud").unwrap(),
        Transport::Stdio {
            command: "acme-connector".into(),
            args: vec!["--profile".into(), "acme-ro".into()],
            env: BTreeMap::new()
        }
    );
    source["variables"] = json!({"endpoint":{"type":"string","description":"Cloud endpoint"}});
    source["mcp_integrations"]["cloud"] = json!({"transport":"http","fields":{"/url":{"exact":"https://cloud.example.com/mcp"}},
        "defaults":{"type":"http","url":{"var":"endpoint"}}});
    assert!(policy::Pack::from_yaml(&source.to_string()).is_ok());
    let binding = policy::Binding {
        variables: std::collections::BTreeMap::from([(
            "endpoint".into(),
            json!("https://cloud.example.com/mcp"),
        )]),
        ..Default::default()
    };
    assert!(policy::Pack::from_yaml_bound(&source.to_string(), &binding, false).is_ok());
}

#[test]
fn defaults_use_typed_pack_values_and_local_sources_remain_local() {
    let mut source: Value = policy::parse(&pack()).unwrap();
    source["capabilities"]
        .as_array_mut()
        .unwrap()
        .push(json!("variables-v1"));
    source["variables"] = json!({"endpoint":{"type":"string","description":"Cloud endpoint","default":"https://cloud.example.com/mcp"}});
    source["mcp_integrations"]["cloud"]["defaults"] =
        json!({"type":"http","url":{"var":"endpoint"}});
    let mut document = config(json!({"project-cloud":connection()}));
    document["mcp"]["project-cloud"]["transport"] =
        json!({"type":"defaults","pack":"acme/cloud","integration":"cloud"});
    let build = |doc: &Value| {
        Evaluator::new(vec![(
            Scope::Project,
            Candidate::from_yaml(&doc.to_string())
                .unwrap()
                .capture(&[source.to_string()], None)
                .unwrap(),
        )])
        .unwrap()
    };
    let image = build(&document);
    assert_eq!(
        image.effective().mcp_connections()["project-cloud"].transport,
        Transport::Http {
            url: "https://cloud.example.com/mcp".into()
        }
    );
    assert!(!image.effective().mcp_connections()["project-cloud"].local);
    document["packs"][0]["local"] =
        json!({"variables":{"endpoint":"https://cloud.example.com/mcp"}});
    assert!(build(&document).effective().mcp_connections()["project-cloud"].local);
    source["variables"]["endpoint"]
        .as_object_mut()
        .unwrap()
        .remove("default");
    // Unset optional defaults cannot prevent command-only use of a pack.
    assert!(
        Candidate::from_yaml(&config(json!({})).to_string())
            .unwrap()
            .capture(&[source.to_string()], None)
            .is_ok()
    );
    assert!(Candidate::from_yaml(&config(json!({"project-cloud":{"transport":{"type":"defaults","pack":"acme/cloud","integration":"cloud"},"packs":{"acme/cloud":"cloud"}}})).to_string()).unwrap().capture(&[source.to_string()],None).is_err());
}

fn custom_image(connection: Value, rules: Value, capabilities: Value) -> Evaluator {
    let mut source = json!({"schema_version":1,"kind":"custom"});
    source["capabilities"] = capabilities;
    source["rules"] = rules;
    let source = source.to_string();
    let mut config = json!({"schema_version":1,"packs":[],"rules":"rules.yml","mcp":{}});
    config["mcp"]["cloud"] = connection;
    let config = config.to_string();
    Evaluator::new(vec![(
        Scope::Project,
        Candidate::from_yaml(&config)
            .unwrap()
            .capture(&[], Some(&source))
            .unwrap(),
    )])
    .unwrap()
}

#[test]
fn command_mapping_keeps_outer_and_inner_restrictions_and_unknown_remote_paths() {
    let image = custom_image(
        json!({"transport":{"type":"http","url":"https://cloud.example.com/mcp"},"agents":{"codex":"acme"},
        "command_fields":{"call":{"executable":"acme","pointer":"/command","encoding":"posix-literal-v1"}}}),
        json!([
            {"id":"outer","decision":"allow","conditions":{"domain":"mcp","server":"cloud","tool":"call"}},
            {"id":"region","decision":"deny","conditions":{"domain":"mcp","server":"cloud","tool":"call","fields":{"/region":{"exact":"blocked"}}}},
            {"id":"read","decision":"allow","conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"exact","values":["inspect","a b"]}}},
            {"id":"delete","decision":"deny","conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"prefix","values":["delete"]}}},
            {"id":"exact-local","decision":"allow","conditions":{"domain":"exact_request","input":{"kind":"argv","executable":"acme","arguments":["exact"],"original_input":{}},"context":{"cwd":"/acme","environment":null},"executable":null}},
            {"id":"folder","decision":"allow","conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"prefix","values":["local"]},"cwd":{"exact":{"root":"project"}}}}
        ]),
        json!(["structured-rules-v1", "argument-rules-v1", "location-v1"]),
    );
    for (command, region, expected) in [
        ("acme inspect 'a b'", "allowed", "allow"),
        ("acme inspect 'a b'", "blocked", "deny"),
        ("acme delete all", "allowed", "deny"),
        ("other inspect 'a b'", "allowed", "ask"),
        ("acme inspect $(whoami)", "allowed", "ask"),
        ("acme inspect a; acme delete all", "allowed", "ask"),
        ("acme local report", "allowed", "ask"),
        ("acme exact", "allowed", "ask"),
    ] {
        let result = image
            .evaluate_normalized(
                &Input::Mcp {
                    server: "cloud".into(),
                    tool: "call".into(),
                    arguments: json!({"command":command,"region":region}),
                    connection: None,
                    original_input: json!({}),
                },
                &Context {
                    cwd: "/acme".into(),
                    environment: None,
                    working_files: None,
                },
                "/acme",
                None,
                "2026-01-01T00:00:00Z",
                &|| Ok(()),
            )
            .unwrap();
        assert_eq!(result["decision"], expected, "{command}: {result}");
    }
    let mapping = CommandField {
        executable: "acme".into(),
        pointer: "/command".into(),
        encoding: "argv-v1".into(),
    };
    assert_eq!(
        mapping
            .arguments(&json!({"command":["acme","inspect","a b"]}))
            .unwrap(),
        ["inspect", "a b"]
    );
    assert!(
        mapping
            .arguments(&json!({"command":["acme",null]}))
            .is_err()
    );
}

#[test]
fn mandatory_fields_fail_closed_with_safe_field_evidence() {
    let mut source: Value = policy::parse(&pack()).unwrap();
    source["capabilities"]
        .as_array_mut()
        .unwrap()
        .push(json!("preconditions-v1"));
    source["preconditions"] = json!([{"id":"required-region","conditions":{"domain":"mcp","server":"cloud","tool":"inspect"},
        "requires_any":[{"fields":{"/region":{"text":"acme-region"}}}],"on_unmet":{"decision":"deny"}}]);
    let document = config(json!({"project-cloud":connection()}));
    let image = Evaluator::new(vec![(
        Scope::Project,
        Candidate::from_yaml(&document.to_string())
            .unwrap()
            .capture(&[source.to_string()], None)
            .unwrap(),
    )])
    .unwrap();
    for (arguments, expected) in [
        (json!({}), "deny"),
        (json!({"region":null}), "deny"),
        (json!({"region":"private-value"}), "deny"),
        (json!({"region":"acme-region"}), "allow"),
    ] {
        let result = image
            .evaluate_normalized(
                &Input::Mcp {
                    server: "project-cloud".into(),
                    tool: "inspect".into(),
                    arguments,
                    connection: None,
                    original_input: json!({}),
                },
                &Context {
                    cwd: "/acme".into(),
                    environment: None,
                    working_files: None,
                },
                "/acme",
                None,
                "2026-01-01T00:00:00Z",
                &|| Ok(()),
            )
            .unwrap();
        assert_eq!(result["decision"], expected, "{result}");
        if expected == "deny" {
            let evidence = &result["explanation"]["requirement_checks"][0]["evidence"];
            assert_eq!(evidence["fields"][0]["pointer"], "/region", "{result}");
            assert_eq!(evidence["fields"][0]["source"], "request");
            assert!(!evidence.to_string().contains("private-value"));
        }
    }
}

#[test]
fn mcp_field_evidence_omits_expected_values_from_sensitive_effective_sources() {
    let expected = "tok-acme-sensitive-fixture";
    let image = custom_image(
        json!({"transport":{"type":"stdio","command":"acme-connector","env":{"ACME_PROFILE":"acme-ro"}},"agents":{"codex":"acme"},
            "effective_fields":{"profile":{"argument":"/credentials/token","default":"/env/ACME_PROFILE"}}}),
        json!([{"id":"field","decision":"allow","conditions":{"domain":"mcp","server":"cloud","tool":"inspect"},
            "requirements":{"connection":{"/effective/profile":{"text":expected}}},"on_unmet":{"decision":"deny"}}]),
        json!([
            "structured-rules-v1",
            "requirements-v1",
            "mcp-connection-facts-v1"
        ]),
    );
    for (arguments, sensitive) in [
        (json!({"credentials":{"token":"different"}}), true),
        (json!({}), false),
    ] {
        let result = image
            .evaluate_normalized(
                &Input::Mcp {
                    server: "cloud".into(),
                    tool: "inspect".into(),
                    arguments,
                    connection: None,
                    original_input: json!({}),
                },
                &Context {
                    cwd: "/acme".into(),
                    environment: None,
                    working_files: None,
                },
                "/acme",
                None,
                "2026-01-01T00:00:00Z",
                &|| Ok(()),
            )
            .unwrap();
        assert_eq!(result["decision"], "deny");
        let field = &result["explanation"]["requirement_checks"][0]["evidence"]["fields"][0];
        assert_eq!(field["pointer"], "/effective/profile");
        assert_eq!(field.get("expected").is_none(), sensitive, "{result}");
        if sensitive {
            assert_eq!(field["evidence_pointer"], "/credentials/token");
            assert!(!result.to_string().contains(expected), "{result}");
            assert!(
                !crate::agent_message::detail(&result["explanation"])
                    .unwrap()
                    .contains(expected)
            );
        }
    }
}

#[test]
fn model_supplied_connection_facts_are_rejected() {
    assert!(serde_json::from_value::<Input>(json!({"kind":"mcp","server":"cloud","tool":"inspect","arguments":{},"connection":{"transport":{"env":{"AWS_PROFILE":"acme-ro"}}},"original_input":{}})).is_err());
}

#[test]
fn mcp_connections_exact_request_binds_submitted_input_without_internal_facts() {
    let image = custom_image(
        json!({"transport":{"type":"http","url":"https://cloud.example.com/mcp"},"agents":{"codex":"acme"}}),
        json!([{"id":"exact","decision":"allow","conditions":{"domain":"exact_request",
            "input":{"kind":"mcp","server":"cloud","tool":"inspect","arguments":{},"original_input":{}},
            "context":{"cwd":"/acme","environment":null},"executable":null}}]),
        json!(["structured-rules-v1"]),
    );
    assert_eq!(decide(&image, "cloud", "inspect"), "allow");
    assert_eq!(decide(&image, "cloud", "delete"), "ask");
}

#[test]
fn mcp_connections_effective_fields_distinguish_known_defaults_and_explicit_invalid_values() {
    let image = custom_image(
        json!({"transport":{"type":"stdio","command":"acme-connector","env":{"ACME_PROFILE":"acme-ro"}},"agents":{"codex":"acme"},
        "effective_fields":{"profile":{"argument":"/profile","default":"/env/ACME_PROFILE"}}}),
        json!([{ "id":"read","decision":"allow","conditions":{"domain":"mcp","server":"cloud","tool":"inspect"},
            "requirements":{"connection":{"/effective/profile":{"text":"acme-ro"}}},"on_unmet":{"decision":"deny"}}]),
        json!([
            "structured-rules-v1",
            "requirements-v1",
            "mcp-connection-facts-v1"
        ]),
    );
    for (arguments, expected, source) in [
        (json!({}), "allow", "reviewed-connection"),
        (json!({"profile":"acme-ro"}), "allow", "request"),
        (json!({"profile":"acme-admin"}), "deny", "request"),
        (json!({"profile":null}), "deny", "request"),
    ] {
        let result = image
            .evaluate_normalized(
                &Input::Mcp {
                    server: "cloud".into(),
                    tool: "inspect".into(),
                    arguments,
                    connection: None,
                    original_input: json!({}),
                },
                &Context {
                    cwd: "/acme".into(),
                    environment: None,
                    working_files: None,
                },
                "/acme",
                None,
                "2026-01-01T00:00:00Z",
                &|| Ok(()),
            )
            .unwrap();
        assert_eq!(result["decision"], expected, "{result}");
        assert_eq!(
            result["normalized_request"]["connection"]["evidence_sources"]["profile"]["source"],
            source
        );
    }
}
