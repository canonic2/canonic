//! Exact human trust reviews through both shipping MCP confirmation protocols.
use super::*;
use canonic_registry_kit::{
    source::Source,
    testing::{Response, Server as RegistryServer, SignedRegistry, SigningKey},
};
use std::{fs, time::Duration};

const FORMS: &str = r#"{"elicitation":{"form":{}}}"#;

fn connect(project: &Project, agent: &str) -> Transport {
    let mut context = project.review.as_ref().clone();
    context.agent = Some(agent.into());
    context.fetcher = RegistryServer::fetcher(Duration::from_secs(2));
    let transport = Transport::start(Server::for_review(context).unwrap());
    for message in initialize(&serde_json::from_str(FORMS).unwrap(), agent) {
        transport.send(&message);
    }
    assert!(transport.receive()["result"].is_object());
    transport
}

fn begin(transport: &Transport, name: &str, args: &Value, modern: bool) -> Value {
    if modern {
        transport.request(&super::skills::modern(
            10,
            "tools/call",
            json!({"name":name,"arguments":args}),
            serde_json::from_str(FORMS).unwrap(),
        ))
    } else {
        transport.call(json!(10), name, args)
    }
}

fn answer(
    transport: &Transport,
    name: &str,
    args: &Value,
    form: &Value,
    action: &str,
    modern: bool,
) -> Value {
    if modern {
        transport.request(&super::skills::modern(
            11,
            "tools/call",
            json!({"name":name,"arguments":args,"requestState":form["result"]["requestState"],
                "inputResponses":{"confirmation":{"action":action}}}),
            serde_json::from_str(FORMS).unwrap(),
        ))
    } else {
        transport.request(
            &json!({"jsonrpc":"2.0","id":form["id"],"result":{"action":action,"content":{}}}),
        )
    }
}

fn success(result: &Value) -> &Value {
    assert_eq!(result["result"]["isError"], false, "{result}");
    &result["result"]["structuredContent"]["data"]
}

fn error_code(result: &Value) -> &Value {
    &result["result"]["structuredContent"]["error"]["code"]
}

fn sources(transport: &Transport) -> Value {
    success(&transport.call(json!(12), "registry_list", &json!({})))["sources"].clone()
}

fn pack(name: &str, dependencies: &Value) -> String {
    json!({"schema_version":1,"kind":"pack","id":format!("acme/{name}"),"version":"1.0.0",
        "description":"Acme enrollment fixture","license":"UNLICENSED","maintainers":["Acme"],
        "engines":{"shield":env!("CARGO_PKG_VERSION")},"capabilities":["structured-rules-v1","dependencies-v1"],
        "dependencies":dependencies,"rules":[]}).to_string()
}

fn signed_source(registry: &SignedRegistry, identity: &str) -> Source {
    Source::signed(
        registry.server().url(),
        &registry.key().public_hex(),
        identity,
    )
    .unwrap()
}

fn reviewed(form: &Value, modern: bool) -> Value {
    let message = if modern {
        &form["result"]["inputRequests"]["confirmation"]["params"]["message"]
    } else {
        &form["params"]["message"]
    }
    .as_str()
    .unwrap();
    serde_json::from_str(message.split_once('\n').unwrap().1).unwrap()
}

fn assert_trust_form(form: &Value, source: &Source, modern: bool) {
    assert_eq!(
        if modern {
            &form["result"]["resultType"]
        } else {
            &form["method"]
        },
        if modern {
            "input_required"
        } else {
            "elicitation/create"
        },
        "{form}"
    );
    let review = reviewed(form, modern);
    let key = canonic_registry_kit::digest::decode(source.key().unwrap(), 32).unwrap();
    assert_eq!(review["trust"]["canonical_location"], source.location());
    assert_eq!(review["trust"]["source_identity"], source.identity());
    assert_eq!(
        review["trust"]["registry_identity"],
        source.registry_id().unwrap()
    );
    assert_eq!(
        review["trust"]["signing_key_fingerprint_sha256"],
        canonic_registry_kit::digest::sha256_hex(&key)
    );
    assert!(
        review["trust"]["basis"]
            .as_str()
            .unwrap()
            .contains("Trust on first use")
    );
    assert_eq!(review["scope"], "project");
}

fn enroll(transport: &Transport, alias: &str, source: &Source, modern: bool) {
    let args = json!({"name":alias,"location":source.location()});
    let form = begin(transport, "registry_add", &args, modern);
    assert_trust_form(&form, source, modern);
    assert!(sources(transport)[alias].is_null());
    let result = answer(transport, "registry_add", &args, &form, "accept", modern);
    assert_eq!(success(&result)["source"], json!(source));
    assert_eq!(sources(transport)[alias]["identity"], source.identity());
}

fn dependency_roundtrip(agent: &str, modern: bool) {
    let mut foreign = SignedRegistry::new("acme-foreign");
    let foreign_source = signed_source(&foreign, "acme-foreign");
    foreign.release("acme/read", "1.0.0", pack("read", &json!([])).as_bytes());
    foreign.publish(1, "9999-12-31T00:00:00Z");
    let mut upstream = SignedRegistry::new("acme-upstream");
    upstream.release(
        "acme/parent",
        "1.0.0",
        pack(
            "parent",
            &json!([
                {"pack":"acme/read","requirement":"^1","registry":foreign_source.identity()}
            ]),
        )
        .as_bytes(),
    );
    upstream.publish(1, "9999-12-31T00:00:00Z");
    let (_server, project) = fixture(Some(agent));
    let transport = connect(&project, agent);
    let before = project.revision();
    enroll(
        &transport,
        "upstream",
        &signed_source(&upstream, "acme-upstream"),
        modern,
    );
    let change =
        json!({"change":{"kind":"pack_add","reference":"upstream/acme/parent","profile":"strict"}});
    let blocked = transport.call(json!(20), "policy_propose", &change);
    let error = &blocked["result"]["structuredContent"]["error"];
    assert_eq!(error["operator_route"], "registry_add", "{blocked}");
    assert!(
        error["message"]
            .as_str()
            .unwrap()
            .contains(foreign_source.identity())
    );
    let args = json!({"name":"foreign","location":foreign.server().url()});
    for action in ["decline", "accept"] {
        let form = begin(&transport, "registry_add", &args, modern);
        assert_trust_form(&form, &foreign_source, modern);
        assert!(sources(&transport)["foreign"].is_null());
        assert_eq!(project.revision(), before);
        let result = answer(&transport, "registry_add", &args, &form, action, modern);
        if action == "decline" {
            assert_eq!(error_code(&result), "approval_declined", "{result}");
            assert!(sources(&transport)["foreign"].is_null());
        } else {
            assert_eq!(success(&result)["source"], json!(foreign_source));
        }
    }
    assert_eq!(project.revision(), before);
    let proposal = transport.call(json!(21), "policy_propose", &change);
    let args = success(&proposal)["apply_arguments"].clone();
    let form = begin(&transport, "policy_apply", &args, modern);
    let applied = if form["method"] == "elicitation/create"
        || form["result"]["resultType"] == "input_required"
    {
        answer(&transport, "policy_apply", &args, &form, "accept", modern)
    } else {
        form
    };
    success(&applied);
    assert_ne!(project.revision(), before);
    let installed = transport.call(json!(22), "pack_list", &json!({}));
    let installed = success(&installed).to_string();
    assert!(
        installed.contains("acme/parent") && installed.contains("acme/read"),
        "{installed}"
    );
}

#[test]
fn registry_enrollment_unblocks_dependency_installation_or_declines_in_the_same_session() {
    if !crate::test_support::isolated_home(concat!(
        module_path!(),
        "::registry_enrollment_unblocks_dependency_installation_or_declines_in_the_same_session"
    )) {
        return;
    }
    for agent in ["claude", "codex"] {
        for modern in [false, true] {
            dependency_roundtrip(agent, modern);
        }
    }
}

#[test]
fn registry_enrollment_freezes_the_reviewed_key_without_rediscovery_on_acceptance() {
    if !crate::test_support::isolated_home(concat!(
        module_path!(),
        "::registry_enrollment_freezes_the_reviewed_key_without_rediscovery_on_acceptance"
    )) {
        return;
    }
    for modern in [false, true] {
        let mut registry = SignedRegistry::new("acme-frozen");
        let entry = registry.release("acme/read", "1.0.0", pack("read", &json!([])).as_bytes());
        registry.publish(1, "9999-12-31T00:00:00Z");
        let source = signed_source(&registry, "acme-frozen");
        let (_server, project) = fixture(Some("codex"));
        let transport = connect(&project, "codex");
        let args = json!({"name":"frozen","location":registry.server().url()});
        let form = begin(&transport, "registry_add", &args, modern);
        assert_trust_form(&form, &source, modern);
        let requests = registry.server().requests();
        assert_eq!(requests.len(), 3);
        let changed = SigningKey::generate();
        registry
            .server()
            .set("catalog-key.pub", Response::ok(changed.public_hex()));
        let bytes = json!({"schema_version":1,"registry_id":"acme-frozen","sequence":1,
            "expires_at":"9999-12-31T00:00:00Z","packs":[entry]})
        .to_string();
        registry.server().set(
            "catalog.sig",
            Response::ok(changed.sign_hex(bytes.as_bytes())),
        );
        let result = answer(&transport, "registry_add", &args, &form, "accept", modern);
        assert_eq!(success(&result)["source"], json!(source));
        assert_eq!(registry.server().requests(), requests);
        assert_eq!(sources(&transport)["frozen"]["key"], source.key().unwrap());
        let rejected = transport.call(
            json!(20),
            "pack_info",
            &json!({"reference":"frozen/acme/read"}),
        );
        assert_eq!(error_code(&rejected), "invalid_input", "{rejected}");
        assert_eq!(sources(&transport)["frozen"]["key"], source.key().unwrap());
    }
}

#[test]
fn registry_enrollment_invalid_signature_never_opens_a_trust_form() {
    if !crate::test_support::isolated_home(concat!(
        module_path!(),
        "::registry_enrollment_invalid_signature_never_opens_a_trust_form"
    )) {
        return;
    }
    let registry = SignedRegistry::new("acme-invalid");
    registry.publish(1, "9999-12-31T00:00:00Z");
    registry
        .server()
        .set("catalog.sig", Response::ok("00".repeat(64)));
    let (_server, project) = fixture(Some("claude"));
    let transport = connect(&project, "claude");
    for modern in [false, true] {
        let result = begin(
            &transport,
            "registry_add",
            &json!({"name":"invalid","location":registry.server().url()}),
            modern,
        );
        assert_eq!(error_code(&result), "invalid_input", "{result}");
        assert!(sources(&transport)["invalid"].is_null());
    }
}

#[test]
fn registry_enrollment_cancel_disconnect_and_missing_forms_never_grant_trust() {
    if !crate::test_support::isolated_home(concat!(
        module_path!(),
        "::registry_enrollment_cancel_disconnect_and_missing_forms_never_grant_trust"
    )) {
        return;
    }
    for action in ["cancel", "disconnect", "unavailable"] {
        let registry = SignedRegistry::new("acme-cancel");
        registry.publish(1, "9999-12-31T00:00:00Z");
        let (_server, project) = fixture(Some("codex"));
        let mut context = project.review.as_ref().clone();
        context.fetcher = RegistryServer::fetcher(Duration::from_secs(2));
        let transport = Transport::start(Server::for_review(context).unwrap());
        let capabilities = if action == "unavailable" {
            json!({})
        } else {
            serde_json::from_str(FORMS).unwrap()
        };
        for message in initialize(&capabilities, "codex") {
            transport.send(&message);
        }
        assert!(transport.receive()["result"].is_object());
        let before = project.revision();
        let args = json!({"name":"cancelled","location":registry.server().url()});
        let form = begin(&transport, "registry_add", &args, false);
        if action == "unavailable" {
            assert_eq!(error_code(&form), "approval_unavailable", "{form}");
            assert!(sources(&transport)["cancelled"].is_null());
        } else if action == "cancel" {
            let result = answer(&transport, "registry_add", &args, &form, "cancel", false);
            assert_eq!(error_code(&result), "approval_declined", "{result}");
            assert!(sources(&transport)["cancelled"].is_null());
        } else {
            assert_eq!(form["method"], "elicitation/create", "{form}");
            let (_, result) = transport.close();
            assert!(result.is_ok());
        }
        let state: Value = serde_json::from_slice(
            &fs::read(project.root.path().join(".shield/state/state.json")).unwrap(),
        )
        .unwrap();
        assert!(state["operator"]["sources"]["cancelled"].is_null());
        assert_eq!(project.revision(), before);
    }
}

#[test]
fn registry_enrollment_unknown_hosts_and_invalid_aliases_do_not_discover_sources() {
    if !crate::test_support::isolated_home(concat!(
        module_path!(),
        "::registry_enrollment_unknown_hosts_and_invalid_aliases_do_not_discover_sources"
    )) {
        return;
    }
    let registry = SignedRegistry::new("acme-untrusted");
    registry.publish(1, "9999-12-31T00:00:00Z");
    for agent in ["unknown-agent", "codex"] {
        let (_server, project) = fixture(Some(agent));
        let transport = connect(&project, agent);
        if agent == "unknown-agent" {
            let result = begin(
                &transport,
                "registry_add",
                &json!({"name":"acme","location":registry.server().url()}),
                false,
            );
            assert_eq!(error_code(&result), "operator_review_required", "{result}");
        }
        for name in ["main", "invalid alias"] {
            let result = begin(
                &transport,
                "registry_add",
                &json!({"name":name,"location":registry.server().url()}),
                false,
            );
            assert!(result["result"]["isError"].as_bool().unwrap(), "{result}");
        }
    }
    assert_eq!(registry.server().requests(), [] as [std::string::String; 0]);
}

#[test]
fn registry_enrollment_user_scope_uses_an_isolated_operator_home() {
    let home = assert_fs::TempDir::new().unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "mcp::tests::registry_enrollment::registry_enrollment_user_scope_child",
            "--ignored",
            "--nocapture",
        ])
        .env("HOME", home.path())
        .env("USERPROFILE", home.path())
        .env("SHIELD_REGISTRY_ENROLLMENT_HOME", home.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
#[ignore = "owned subprocess with an isolated operator home"]
fn registry_enrollment_user_scope_child() {
    let home =
        std::env::var_os("SHIELD_REGISTRY_ENROLLMENT_HOME").expect("isolated harness required");
    assert_eq!(
        std::env::home_dir().unwrap(),
        std::path::PathBuf::from(&home)
    );
    for modern in [false, true] {
        let (_server, project) = fixture(Some("codex"));
        let root = project.root.path().canonicalize().unwrap();
        let directory = root.join("registry");
        fs::create_dir(&directory).unwrap();
        fs::write(
            directory.join("catalog.json"),
            "{\"schema_version\":1,\"packs\":[]}",
        )
        .unwrap();
        let transport = connect(&project, "codex");
        let args = json!({"name":"local","location":"registry","scope":"user"});
        let before = project.revision();
        for action in ["decline", "accept"] {
            let form = begin(&transport, "registry_add", &args, modern);
            let review = reviewed(&form, modern);
            let text = review.to_string();
            assert!(
                text.contains("other projects") && text.contains("unsigned catalog"),
                "{form}"
            );
            assert_eq!(review["trust"]["canonical_location"], json!(directory));
            assert!(sources(&transport)["local"].is_null());
            let result = answer(&transport, "registry_add", &args, &form, action, modern);
            if action == "accept" {
                assert_eq!(success(&result)["scope"], "user");
                assert_eq!(sources(&transport)["local"]["location"], json!(directory));
                let repeated = begin(&transport, "registry_add", &args, modern);
                assert_eq!(success(&repeated)["replayed"], true, "{repeated}");
                assert_eq!(
                    success(&repeated)["global_generation"],
                    success(&result)["global_generation"]
                );
            } else {
                assert_eq!(error_code(&result), "approval_declined", "{result}");
            }
        }
        let state: Value =
            serde_json::from_slice(&fs::read(root.join(".shield/state/state.json")).unwrap())
                .unwrap();
        assert!(state["operator"]["sources"]["local"].is_null());
        assert_eq!(project.revision(), before);
        let (_server, other) = fixture(Some("codex"));
        let peer = connect(&other, "codex");
        assert_eq!(sources(&peer)["local"]["location"], json!(directory));
        let remove = json!({"name":"local","scope":"user"});
        let form = begin(&transport, "registry_remove", &remove, modern);
        let removed = answer(
            &transport,
            "registry_remove",
            &remove,
            &form,
            "accept",
            modern,
        );
        success(&removed);
        let repeated = begin(&transport, "registry_remove", &remove, modern);
        assert_eq!(success(&repeated)["replayed"], true, "{repeated}");
        assert_eq!(
            success(&repeated)["global_generation"],
            success(&removed)["global_generation"]
        );
        global_signing_key_replacement(&transport, modern);
    }
}

fn global_signing_key_replacement(transport: &Transport, modern: bool) {
    let registry = SignedRegistry::new("acme-global-replay");
    registry.publish(1, "9999-12-31T00:00:00Z");
    let args = json!({"name":"signed","location":registry.server().url(),"scope":"user"});
    let form = begin(transport, "registry_add", &args, modern);
    let first = answer(transport, "registry_add", &args, &form, "accept", modern);
    let initial = success(&first).clone();
    let repeated = begin(transport, "registry_add", &args, modern);
    assert_eq!(success(&repeated)["replayed"], true, "{repeated}");
    assert_eq!(
        success(&repeated)["global_generation"],
        initial["global_generation"]
    );

    let changed = SigningKey::generate();
    let bytes = json!({"schema_version":1,"registry_id":"acme-global-replay","sequence":2,
        "expires_at":"9999-12-31T00:00:00Z","packs":[]})
    .to_string();
    registry
        .server()
        .set("catalog-key.pub", Response::ok(changed.public_hex()));
    registry
        .server()
        .set("catalog.json", Response::ok(bytes.clone()));
    registry.server().set(
        "catalog.sig",
        Response::ok(changed.sign_hex(bytes.as_bytes())),
    );
    for action in ["decline", "accept"] {
        let form = begin(transport, "registry_add", &args, modern);
        let review = reviewed(&form, modern);
        assert_eq!(review["change"], "replace");
        assert_eq!(review["previous_source"]["key"], initial["source"]["key"]);
        assert_eq!(review["source"]["key"], changed.public_hex());
        assert_ne!(review["source"]["identity"], initial["source"]["identity"]);
        assert_eq!(
            sources(transport)["signed"]["key"],
            initial["source"]["key"]
        );
        let result = answer(transport, "registry_add", &args, &form, action, modern);
        if action == "accept" {
            assert_ne!(
                success(&result)["global_generation"],
                initial["global_generation"]
            );
            assert_eq!(sources(transport)["signed"]["key"], changed.public_hex());
        } else {
            assert_eq!(error_code(&result), "approval_declined");
            assert_eq!(
                sources(transport)["signed"]["key"],
                initial["source"]["key"]
            );
        }
    }
    let remove = json!({"name":"signed","scope":"user"});
    let form = begin(transport, "registry_remove", &remove, modern);
    success(&answer(
        transport,
        "registry_remove",
        &remove,
        &form,
        "accept",
        modern,
    ));
}
