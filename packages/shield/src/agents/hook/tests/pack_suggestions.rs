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
        assert!(text.contains("policy_propose"), "{text}");
        assert!(text.contains("policy_apply"), "{text}");
        assert!(text.contains("source_entry_id request-acme"), "{text}");
        assert!(
            text.contains(&json!({"kind":"pack_add","reference":"main/canonic/node"}).to_string()),
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
    // Both fallback paths keep the same candidate/payload and add one instruction.
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
        let text = output["additionalContext"].as_str().unwrap();
        assert_offer(text);
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
    let missing = call(root, "codex", "npm test");
    assert!(!reason(&missing).contains("pack_add"));
    assert!(!missing.to_string().contains("Briefly offer this rule pack"));
    retain(&context, &json!({"npm":["zeta/runtime","acme/node"]}));
    let value = call(root, "codex", "npm test");
    let text = reason(&value);
    assert!(text.contains("Several available packs"), "{text}");
    assert!(text.find("main/acme/node").unwrap() < text.find("main/zeta/runtime").unwrap());
    assert!(!text.contains(r#""reference":"main/acme/node""#), "{text}");
    assert!(!value.to_string().contains("Briefly offer this rule pack"));
    update_advisory_fixture(&context, |operator, _| {
        operator["installed"] = json!([{"id":"acme/node","source_identity":"main"},{"id":"zeta/runtime","source_identity":"main"}]);
    });
    let installed = call(root, "codex", "npm test");
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
fn automatic_fallback_reports_policy_ask_and_the_same_advisory_candidate() {
    let (_root, context) = fixture();
    let root = Path::new(&context.project_root);
    crate::review::operator::integrations::configure_fixture(
        root,
        crate::cli::public::IntegrationChoice::agent("codex"),
        crate::review::integrations::AskFallback::Allow,
    );
    retain(&context, &json!({"npm":["canonic/node"]}));
    for _ in 0..3 {
        let value = call(root, "codex", "npm test");
        assert_permitted(&value, "codex");
        let text = value.to_string();
        assert!(text.contains("configured Ask fallback"), "{value}");
        assert!(text.contains("main/canonic/node"), "{value}");
        assert!(!text.contains("user approved"), "{value}");
        // Suppressing repeated offers belongs to the conversation, not hook state.
        assert_eq!(text.matches("Briefly offer this rule pack").count(), 1);
    }
}

#[test]
fn proposed_pack_keeps_the_enrolled_registry_and_publisher_identity() {
    let (_root, context) = fixture();
    let root = Path::new(&context.project_root);
    let registry = assert_fs::TempDir::new().unwrap();
    let source =
        canonic_registry::source::Source::local(&registry.path().canonicalize().unwrap()).unwrap();
    let pack = json!({"schema_version":1,"kind":"pack","id":"acme/runtime","version":"1.0.0",
        "description":"Acme runtime fixture","license":"MIT","maintainers":["Acme"],
        "engines":{"shield":env!("CARGO_PKG_VERSION")},"capabilities":["argument-rules-v1","structured-rules-v1"],
        "rules":[{"id":"inspect","decision":"allow","conditions":{"domain":"argv","executable":"npm","arguments":{"kind":"exact","values":["--version"]}}}]}).to_string();
    fs::write(registry.path().join("runtime.json"), &pack).unwrap();
    let document = json!({"schema_version":2,"packs":[{"id":"acme/runtime","version":"1.0.0","path":"runtime.json","sha256":crate::policy::fingerprint(pack.as_bytes())}],"commands":{"acme/runtime":["node","npm","npx"]}}).to_string();
    fs::write(registry.path().join("catalog.json"), &document).unwrap();
    let catalog = canonic_registry::catalog::Catalog::verify(
        &source,
        document.as_bytes(),
        None,
        std::time::SystemTime::now(),
    )
    .unwrap();
    let mut checkpoints = canonic_registry::checkpoint::Checkpoints::default();
    checkpoints.admit(&catalog).unwrap();
    update_advisory_fixture(&context, |operator, retained| {
        operator["sources"]["tools"] = json!(source);
        *retained = serde_json::from_value(json!(checkpoints)).unwrap();
    });
    let value = call(root, "claude", "npm install express@5");
    let text = advice(&value, "claude");
    assert!(text.contains("tools/acme/runtime"), "{text}");
    let start = text.find("with change ").unwrap() + "with change ".len();
    let end = start + text[start..].find(" and source_entry_id").unwrap();
    let change: Value = serde_json::from_str(&text[start..end]).unwrap();
    assert_eq!(
        change,
        json!({"kind":"pack_add","reference":"tools/acme/runtime"})
    );
    let mut mcp = context.clone();
    mcp.surface = "mcp".into();
    mcp.agent = Some("claude-code".into());
    mcp.connection_id = Some("acme".into());
    let proposal = review::mcp::call(
        &mcp,
        &review::mcp::Connection::new("acme".into()),
        "policy_propose",
        &json!({"change":change,"agent_reason":"Review Node policy for this project"}),
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
        let value = call(root, agent, "git status; pnpm run acme-check -- --quiet");
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
        assert!(text.contains("eligible recurring"), "{text}");
        assert!(!text.contains("with change {"), "{text}");
        // Drafting guidance has not approved the command or activated a rule.
        assert_eq!(
            call(root, agent, "pnpm run acme-check -- --quiet")["hookSpecificOutput"]["permissionDecision"],
            if agent == "claude" { "ask" } else { "deny" }
        );
    }
}
