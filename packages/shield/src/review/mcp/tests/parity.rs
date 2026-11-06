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
        "cache",
        "docs",
        "policy",
        "mcp",
    ] {
        for verb in cli.find_subcommand(family).unwrap().get_subcommands() {
            // A terminal menu is a presentation, and serve owns transport lifetime.
            if verb.get_name() == "help"
                || (family == "policy" && verb.get_name() == "review")
                || (family == "mcp" && verb.get_name() == "serve")
            {
                continue;
            }
            let tool = format!("{}_{}", family.replace('-', "_"), verb.get_name());
            assert!(catalog.get(&tool).is_some(), "missing {tool}");
        }
    }
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
