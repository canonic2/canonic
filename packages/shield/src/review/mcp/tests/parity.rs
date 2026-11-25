//! Canonical direct verbs share the existing proposal and inspection owners.
use super::*;
use clap::CommandFactory;

#[test]
fn parity_catalog_covers_shared_cli_verbs_with_explicit_lifecycle_exceptions() {
    let cli = crate::cli::Cli::command();
    let catalog = crate::mcp::catalog(None).unwrap();
    for family in [
        "pack",
        "registry",
        "rule",
        "request",
        "history",
        "session-grant",
        "protection",
        "docs",
        "policy",
        "mcp",
    ] {
        for verb in cli.find_subcommand(family).unwrap().get_subcommands() {
            // Serve owns transport lifetime.
            if verb.get_name() == "help" || (family == "mcp" && verb.get_name() == "serve") {
                continue;
            }
            let tool = format!("{}_{}", family.replace('-', "_"), verb.get_name());
            assert!(catalog.get(&tool).is_some(), "missing {tool}");
        }
    }
}

#[test]
fn parity_local_pack_removal_preserves_the_project_selection_and_lock() {
    let (root, mut context, connection) = fixture();
    context.agent = Some("claude".into());
    let _registry = dependency_registry(
        &root,
        &context,
        &json!([{"pack":"acme/child","requirement":"^1"}]),
        false,
    );
    commit_pack_change(
        &context,
        &connection,
        json!({"kind":"pack_add","reference":"acme-dependencies/acme/parent"}),
        "install",
    );
    let shared = root.path().join(".shield/config.yml");
    let lock = root.path().join(".shield/shield.lock");
    let local = root.path().join(".shield/config.local.yml");
    let before_local = std::fs::read(&local).unwrap();
    let before = [
        std::fs::read(&shared).unwrap(),
        std::fs::read(&lock).unwrap(),
    ];
    let result = invoke(
        &context,
        &connection,
        "pack_remove",
        json!({"reference":"acme/parent","layer":"local"}),
    );
    assert!(result["error"].is_null(), "{result}");
    assert_eq!(std::fs::read(&local).unwrap(), before_local);
    let applied = call_with_confirmation(
        &context,
        &connection,
        "policy_apply",
        &result["data"]["apply_arguments"],
        &|| Ok(()),
        true,
    );
    assert!(applied["error"].is_null(), "{applied}");
    assert_eq!(
        [std::fs::read(shared).unwrap(), std::fs::read(lock).unwrap()],
        before
    );
    assert_ne!(std::fs::read(local).unwrap(), before_local);
}

#[test]
fn parity_direct_local_rule_verbs_remain_inactive_until_confirmed_apply() {
    for agent in ["claude", "codex"] {
        let (root, mut context, connection) = fixture();
        context.agent = Some(agent.into());
        let revision = Store::open(root.path())
            .unwrap()
            .read()
            .unwrap()
            .policy_revision;
        let added = invoke(
            &context,
            &connection,
            "rule_add",
            json!({"rule":rule("direct-local", "allow"),"layer":"local"}),
        );
        assert!(added["error"].is_null(), "{added}");
        assert_eq!(
            Store::open(root.path())
                .unwrap()
                .read()
                .unwrap()
                .policy_revision,
            revision
        );
        assert!(!root.path().join(".shield/rules.local.yml").exists());
        let applied = call_with_confirmation(
            &context,
            &connection,
            "policy_apply",
            &added["data"]["apply_arguments"],
            &|| Ok(()),
            true,
        );
        assert!(applied["error"].is_null(), "{applied}");
        assert!(root.path().join(".shield/rules.local.yml").exists());
        for (verb, args) in [
            (
                "rule_edit",
                json!({"rule_id":"direct-local","rule":rule("direct-local", "deny"),"layer":"local"}),
            ),
            (
                "rule_remove",
                json!({"rule_id":"direct-local","layer":"local"}),
            ),
        ] {
            let proposed = invoke(&context, &connection, verb, args);
            assert!(proposed["error"].is_null(), "{proposed}");
            let applied = call_with_confirmation(
                &context,
                &connection,
                "policy_apply",
                &proposed["data"]["apply_arguments"],
                &|| Ok(()),
                true,
            );
            assert!(applied["error"].is_null(), "{applied}");
        }
    }
}
