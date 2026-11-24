//! Pack discovery through real adapter decisions; no commands are executed.
use super::*;
use crate::review::observations::update_advisory_fixture;

fn retain(context: &review::Context, commands: &Value) {
    update_advisory_fixture(context, |_, checkpoints| {
        checkpoints.insert(
            "main".into(),
            json!({"digest":"0".repeat(64),"sequence":1,"releases":{},"commands":commands}),
        );
    });
}

#[test]
fn pack_advice_reaches_agents_without_cluttering_user_permission_prompts() {
    let (_root, context) = fixture();
    let root = Path::new(&context.project_root);
    retain(
        &context,
        &json!({"node":["canonic/node"],"npm":["canonic/node"],"npx":["canonic/node"],"acme-db":["acme/database"]}),
    );
    for agent in ["claude", "codex"] {
        for (source, reference) in [
            ("npm view express version", "main/canonic/node"),
            ("node --version", "main/canonic/node"),
            ("/opt/homebrew/bin/npx acme-cli", "main/canonic/node"),
            ("acme-db inspect", "main/acme/database"),
            (
                "git status; npm install express@5; git status",
                "main/canonic/node",
            ),
        ] {
            let value = call(root, agent, source);
            if cfg!(windows) && agent == "codex" && source != "node --version" {
                assert_unsupported_shell(&value);
                assert!(!value.to_string().contains("Briefly offer this rule pack"));
                continue;
            }
            assert_eq!(
                value["hookSpecificOutput"]["permissionDecision"],
                if agent == "claude" { "ask" } else { "deny" },
                "{value}"
            );
            if agent == "claude" {
                let text = reason(&value);
                assert!(text.contains("Allow this request?"), "{text}");
                for hidden in [
                    reference,
                    "policy_propose",
                    "policy_apply",
                    "source_entry_id",
                    "pack_add",
                ] {
                    assert!(!text.contains(hidden), "{source}: {text}");
                }
            }
            let text = advice(&value, agent);
            assert!(text.contains(reference), "{source}: {text}");
            let name = reference.rsplit('/').next().unwrap();
            assert!(
                text.contains(&format!("The `{name}` rule pack advertises this command")),
                "{text}"
            );
            assert!(text.contains("may still ask or deny"), "{text}");
            assert!(
                text.contains(&format!("Inspect pack_info for {reference}")),
                "{text}"
            );
            assert!(text.contains("retained Ask/Deny outcomes"), "{text}");
            assert!(text.contains("policy_apply"), "{text}");
            assert!(!text.contains("To allow it"), "{text}");
        }
    }
}

fn advice<'a>(value: &'a Value, agent: &str) -> &'a str {
    if agent == "claude" {
        value["hookSpecificOutput"]["additionalContext"]
            .as_str()
            .unwrap()
    } else {
        reason(value)
    }
}

#[cfg(unix)]
#[test]
fn a_temporary_executable_has_no_pack_binding_advice() {
    use std::os::unix::fs::PermissionsExt;

    let pack = json!({"schema_version":1,"kind":"pack","id":"acme/rust","version":"1.0.0",
        "description":"Acme Rust tools","license":"MIT","maintainers":["Acme"],"engines":{"shield":"*"},
        "capabilities":["structured-rules-v1","argument-rules-v1","location-v1","access-paths-v1","regex-v1","variables-v1"],
        "variables":{"cargo_executable":{"type":"string","description":"Acme cargo path","layer":"local","match":{"regex":"/[^\\r\\n]+/cargo"}}},
        "rules":[{"id":"format","conditions":{"domain":"argv","executable":{"var":"cargo_executable"},"arguments":{"kind":"exact","values":["fmt"]},"cwd":{"within":{"root":"write"}}},
            "decision":{"loose":"allow","recommended":"allow","strict":"ask"}}]}).to_string();
    let external = assert_fs::TempDir::new().unwrap();
    let cargo = external.path().join("cargo");
    fs::write(&cargo, "#!/bin/sh\nexit 0\n").unwrap();
    fs::set_permissions(&cargo, fs::Permissions::from_mode(0o755)).unwrap();
    check_installed_executable_setting(&pack, "acme/rust", "1.0.0", &cargo, false);
}

#[cfg(unix)]
#[test]
#[ignore = "requires the Rust pack artifact named by SHIELD_TEST_RUST_PACK"]
fn installed_executable_setting_covers_the_published_rust_pack() {
    let artifact = std::env::var_os("SHIELD_TEST_RUST_PACK").expect("select a Rust pack artifact");
    let pack = fs::read_to_string(artifact).unwrap();
    let document: Value = crate::policy::parse(&pack).unwrap();
    let cargo = std::env::var_os("CARGO_HOME")
        .map_or_else(
            || std::env::home_dir().unwrap().join(".cargo"),
            std::path::PathBuf::from,
        )
        .join("bin/cargo");
    check_installed_executable_setting(
        &pack,
        "canonic/rust",
        document["version"].as_str().unwrap(),
        &cargo,
        true,
    );
}

#[cfg(unix)]
fn check_installed_executable_setting(
    pack: &str,
    id: &str,
    version: &str,
    cargo: &Path,
    trusted: bool,
) {
    for (profile, expected) in [("recommended", trusted), ("strict", false)] {
        let root = assert_fs::TempDir::new().unwrap();
        let config = json!({"schema_version":1,"packs":[{"id":id,"version":version,"path":"rust.yml","profile":profile}]});
        let bundle = Bundle::capture(&config.to_string(), &[pack.to_owned()], None).unwrap();
        let context = review::mcp::initialize_fixture(root.path(), &bundle);
        let root = Path::new(&context.project_root);
        for agent in ["claude", "codex"] {
            let reply = call(root, agent, &format!("'{}' fmt", cargo.display()));
            let advice = advice(&reply, agent);
            assert!(advice.contains("with rule_add"), "{reply}");
            assert!(
                !advice.contains("matching installed pack setting"),
                "{reply}"
            );
        }
        call(root, "claude", &format!("'{}' publish", cargo.display()));
        let connection = review::mcp::Connection::new("acme-binding-advice".into());
        let listed = review::mcp::call(
            &context,
            &connection,
            "suggestion_list",
            &json!({"session_id":"acme-session"}),
            &|| Ok(()),
        );
        let rows = listed["data"]["requests"]
            .as_array()
            .unwrap_or_else(|| panic!("{listed}"));
        for (command, recommends_binding) in [("fmt", expected), ("publish", false)] {
            let row = rows
                .iter()
                .find(|row| row["request"]["arguments"] == json!([command]))
                .unwrap_or_else(|| panic!("{listed}"));
            assert_eq!(
                row["next_action"]["tool"] == "pack_configure",
                recommends_binding,
                "{row}"
            );
            if recommends_binding {
                let args = &row["next_action"]["arguments"];
                assert!(args["reference"].as_str().unwrap().starts_with("pack:"));
                assert_eq!(args["set"], json!({"cargo_executable":cargo}));
                assert_eq!(args["layer"], "local");
                assert_eq!(row["eligible_for_offer"], false);
                assert!(row.get("not_portable").is_none());
            }
        }
    }
}

#[test]
fn unique_pack_offers_deliver_shared_instructions_only_in_agent_context() {
    let (_root, context) = fixture();
    let root = Path::new(&context.project_root);
    retain(&context, &json!({"npm":["canonic/node"]}));
    let receipt = json!({
        "decision":"ask", "code":"approval-required",
        "history":{"request_id":"request-acme"},
        "diagnostics":{
            "message":"Acme command needs review.",
            "guidance":[{
                "source":"decisive", "message":"`npm test` matches no rule, so the user decides.",
                "request":{"executable":"npm","arguments":["test"]}
            }]
        }
    });
    let operation = json!({"kind":"shell","source":"npm test","dialect":"posix"});
    let claude = crate::agents::find("claude").unwrap();
    let instruction = claude.pack_offer_instruction().unwrap();
    let assert_offer = |text: &str| {
        assert_eq!(text.matches(instruction).count(), 1);
        assert!(text.contains("Call pack_add with arguments"), "{text}");
        assert!(!text.contains("Call policy_propose with change"), "{text}");
        assert!(text.contains("policy_apply"), "{text}");
        assert!(text.contains("Observed request request-acme"), "{text}");
        assert!(
            text.contains(&json!({"reference":"main/canonic/node"}).to_string()),
            "{text}"
        );
    };
    let reply = feedback::respond(
        root,
        claude,
        Event::Before,
        &receipt,
        &operation,
        "acme-session",
    )
    .unwrap();
    let wire = claude.reply(Event::Before, &reply);
    let output = &wire["hookSpecificOutput"];
    assert_eq!(output["hookEventName"], "PreToolUse");
    assert_eq!(output["permissionDecision"], "ask");
    assert_offer(output["additionalContext"].as_str().unwrap());
    let prompt = output["permissionDecisionReason"].as_str().unwrap();
    assert!(!prompt.contains(instruction));
    assert!(!prompt.contains("policy_propose") && !prompt.contains("policy_apply"));
    let codex = crate::agents::find("codex").unwrap();
    assert_eq!(codex.pack_offer_instruction(), Some(instruction));
    // A blocked Ask can offer a pack. An Ask allowed by fallback stays silent.
    for fallback in ["block", "allow"] {
        let mut receipt = receipt.clone();
        if fallback == "allow" {
            receipt["disposition"] = json!("allowed");
            receipt["ask_fallback"] = json!("allow");
        }
        let reply = feedback::respond(
            root,
            codex,
            Event::Before,
            &receipt,
            &operation,
            "acme-session",
        )
        .unwrap();
        let wire = codex.reply(Event::Before, &reply);
        let output = &wire["hookSpecificOutput"];
        assert_eq!(
            output["permissionDecision"],
            if fallback == "allow" {
                Value::Null
            } else {
                json!("deny")
            }
        );
        if fallback == "allow" {
            assert!(output["additionalContext"].is_null(), "{wire}");
        } else {
            assert_offer(output["additionalContext"].as_str().unwrap());
        }
        assert!(
            !output["permissionDecisionReason"]
                .as_str()
                .unwrap_or_default()
                .contains(instruction)
        );
    }
}

#[test]
fn installed_missing_and_ambiguous_metadata_do_not_produce_a_unique_install_offer() {
    let (_root, context) = fixture();
    let root = Path::new(&context.project_root);
    // No name-based guess when the catalog has no command associations.
    let missing = call(root, "codex", "node --version");
    assert!(!reason(&missing).contains("pack_add"));
    assert!(!missing.to_string().contains("Briefly offer this rule pack"));
    retain(&context, &json!({"node":["zeta/runtime","acme/node"]}));
    let value = call(root, "codex", "node --version");
    let text = reason(&value);
    assert!(text.contains("Several available packs"), "{text}");
    assert!(text.contains("Call pack_add with arguments"), "{text}");
    assert!(!text.contains("Call policy_propose with change"), "{text}");
    assert!(text.find("main/acme/node").unwrap() < text.find("main/zeta/runtime").unwrap());
    assert!(!text.contains(r#""reference":"main/acme/node""#), "{text}");
    assert!(!value.to_string().contains("Briefly offer this rule pack"));
    update_advisory_fixture(&context, |operator, _| {
        operator["installed"] = json!([{"id":"acme/node","source_identity":"main"},{"id":"zeta/runtime","source_identity":"main"}]);
    });
    let installed = call(root, "codex", "node --version");
    assert!(!reason(&installed).contains("pack_add"));
    assert!(
        !installed
            .to_string()
            .contains("Briefly offer this rule pack")
    );
}

#[test]
fn refusals_and_unrelated_commands_receive_no_pack_installation_advice() {
    let (_root, context) = fixture();
    let root = Path::new(&context.project_root);
    retain(
        &context,
        &json!({"git":["canonic/git"],"cat":["acme/readers"],"npm":["canonic/node"]}),
    );
    for agent in ["claude", "codex"] {
        for source in [
            "git reset --hard",
            "cat .shield/state/state.json",
            "cat .shield/review/state.json",
            "npm test $(acme-path)",
            "npm test; git reset --hard",
            "git status",
            "acme-other inspect",
        ] {
            let value = call(root, agent, source);
            assert!(!reason(&value).contains("pack_add"), "{source}: {value}");
            assert!(
                !value.to_string().contains("Briefly offer this rule pack"),
                "{source}: {value}"
            );
        }
    }
}

#[test]
fn automatic_fallback_does_not_offer_an_available_pack_inline() {
    let (_root, context) = fixture();
    let root = Path::new(&context.project_root);
    crate::review::operator::integrations::configure_fixture(
        root,
        crate::cli::public::IntegrationChoice::agent("codex"),
        crate::review::integrations::AskFallback::Allow,
    );
    retain(&context, &json!({"node":["canonic/node"]}));
    for _ in 0..3 {
        let value = call(root, "codex", "node --version");
        assert_eq!(
            value,
            json!({"hookSpecificOutput":{"hookEventName":"PreToolUse"}})
        );
    }
}

#[test]
fn proposed_pack_keeps_the_enrolled_registry_and_publisher_identity() {
    let (_root, context) = fixture();
    let root = Path::new(&context.project_root);
    let registry = assert_fs::TempDir::new().unwrap();
    let source =
        canonic_registry_kit::source::Source::local(&registry.path().canonicalize().unwrap())
            .unwrap();
    let pack = json!({"schema_version":1,"kind":"pack","id":"acme/runtime","version":"1.0.0",
        "description":"Acme runtime fixture","license":"MIT","maintainers":["Acme"],
        "engines":{"shield":env!("CARGO_PKG_VERSION")},"capabilities":["argument-rules-v1","structured-rules-v1"],
        "rules":[{"id":"inspect","decision":"allow","conditions":{"domain":"argv","executable":"npm","arguments":{"kind":"exact","values":["--version"]}}}]}).to_string();
    fs::write(registry.path().join("runtime.json"), &pack).unwrap();
    let document = json!({"schema_version":2,"packs":[{"id":"acme/runtime","version":"1.0.0","path":"runtime.json","sha256":crate::policy::fingerprint(pack.as_bytes())}],"commands":{"acme/runtime":["node","npm","npx"]}}).to_string();
    fs::write(registry.path().join("catalog.json"), &document).unwrap();
    let catalog = canonic_registry_kit::catalog::Catalog::verify(
        &source,
        document.as_bytes(),
        None,
        std::time::SystemTime::now(),
    )
    .unwrap();
    let mut checkpoints = canonic_registry_kit::checkpoint::Checkpoints::default();
    checkpoints.admit(&catalog).unwrap();
    update_advisory_fixture(&context, |operator, retained| {
        operator["sources"]["tools"] = json!(source);
        *retained = serde_json::from_value(json!(checkpoints)).unwrap();
    });
    let value = call(root, "claude", "npm install express@5");
    let text = advice(&value, "claude");
    assert!(text.contains("tools/acme/runtime"), "{text}");
    let start = text.find("with arguments ").unwrap() + "with arguments ".len();
    let end = start + text[start..].find(" to collect").unwrap();
    let arguments: Value = serde_json::from_str(&text[start..end]).unwrap();
    assert_eq!(arguments, json!({"reference":"tools/acme/runtime"}));
    let mut mcp = context.clone();
    mcp.surface = "mcp".into();
    mcp.agent = Some("claude".into());
    mcp.connection_id = Some("acme".into());
    let question = review::mcp::PackQuestion::prepare(&mcp, "pack_add", &arguments).unwrap();
    assert!(!question.needs_input);
    let proposal = question.propose(
        &mcp,
        &review::mcp::Connection::new("acme".into()),
        &json!({}),
        "acme-pack-offer",
        &|| Ok(()),
    );
    assert!(proposal["data"]["proposal_id"].is_string(), "{proposal}");
    assert!(
        proposal.to_string().contains(source.identity()),
        "{proposal}"
    );
    assert!(proposal.to_string().contains("acme/runtime"), "{proposal}");
    // A draft may acquire a candidate for review; it activates no policy.
    assert_eq!(
        update_advisory_fixture(&context, |_, _| {})["installed"],
        json!([])
    );
    assert_eq!(
        call(root, "claude", "npm install express@5")["hookSpecificOutput"]["permissionDecision"],
        "ask"
    );
}

#[test]
fn initial_script_asks_keep_proposal_instructions_in_agent_feedback() {
    let (_root, context) = fixture();
    let root = Path::new(&context.project_root);
    for agent in ["claude", "codex"] {
        let native = cfg!(windows) && agent == "codex";
        let script = if native {
            "cargo run --bin acme-check -- --quiet"
        } else {
            "pnpm run acme-check -- --quiet"
        };
        if native {
            assert_unsupported_shell(&call(
                root,
                agent,
                "git status; pnpm run acme-check -- --quiet",
            ));
        }
        let value = call(
            root,
            agent,
            if native {
                script
            } else {
                "git status; pnpm run acme-check -- --quiet"
            },
        );
        assert_eq!(
            value["hookSpecificOutput"]["permissionDecision"],
            if agent == "claude" { "ask" } else { "deny" },
            "{value}"
        );
        if agent == "claude" {
            let text = reason(&value);
            assert!(text.contains("matches no rule"), "{text}");
            assert!(text.contains("Allow this request?"), "{text}");
            assert!(
                !text.contains("policy_propose") && !text.contains("policy_apply"),
                "{text}"
            );
        }
        let text = advice(&value, agent);
        let instruction = crate::agents::find(agent)
            .unwrap()
            .pack_offer_instruction()
            .unwrap();
        assert!(!text.contains(instruction));
        assert!(text.contains("suggestion_list"), "{text}");
        assert!(
            text.contains("propose a narrow custom rule now")
                && text.contains("skill://shield/custom-rules/SKILL.md"),
            "{text}"
        );
        assert!(!text.contains("with change {"), "{text}");
        // Drafting guidance has not approved the command or activated a rule.
        assert_eq!(
            call(root, agent, script)["hookSpecificOutput"]["permissionDecision"],
            if agent == "claude" { "ask" } else { "deny" }
        );
    }
}
