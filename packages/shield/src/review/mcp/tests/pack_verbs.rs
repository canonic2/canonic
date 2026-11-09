//! Direct pack verbs retain CLI semantics and the common approval boundary.
use super::*;
use clap::CommandFactory;

#[test]
fn pack_updates_search_seeds_offline_cli_and_mcp_inspection_without_policy_changes() {
    let (root, context, connection) = fixture();
    let source = dependency_registry(&root, &context, &json!([]), false);
    commit_pack_change(
        &context,
        &connection,
        json!({"kind":"pack_add","reference":"acme-dependencies/acme/parent"}),
        "install",
    );
    registry_release(&source, "acme/parent", "1.1.0", None);
    let searched = invoke(
        &context,
        &connection,
        "pack_search",
        json!({"query":"acme-dependencies/"}),
    );
    assert!(searched["error"].is_null(), "{searched}");
    std::fs::remove_file(source.path().join("catalog.json")).unwrap();
    let checkpoint = root.path().join(".shield/review/state.json");
    let before = std::fs::read(&checkpoint).unwrap();
    let cli = crate::review::packs::list(&context).unwrap();
    let listed = invoke(&context, &connection, "pack_list", json!({}));
    assert_eq!(listed["data"]["pack_updates"], cli["pack_updates"]);
    assert_eq!(
        listed["data"]["pack_updates"]["availability"], "available",
        "{listed}"
    );
    assert_eq!(
        listed["data"]["pack_updates"]["items"][0]["available_version"],
        "1.1.0"
    );
    let status = invoke(&context, &connection, "status", json!({}));
    assert_eq!(status["data"]["pack_updates"], cli["pack_updates"]);
    let selected = invoke(
        &context,
        &connection,
        "pack_list",
        json!({"reference":"acme/parent"}),
    );
    assert_eq!(selected["data"]["pack_updates"], cli["pack_updates"]);
    let catalog = crate::mcp::catalog(None).unwrap();
    for (name, value) in [
        ("pack_list", &listed),
        ("status", &status),
        ("pack_list", &selected),
    ] {
        catalog
            .result_schema(name)
            .unwrap()
            .validate(&canonic_mcp_kit::ToolResult {
                meta: None,
                content: Vec::new(),
                structured_content: Some(value.clone()),
                is_error: false,
            })
            .unwrap();
    }
    assert_eq!(std::fs::read(&checkpoint).unwrap(), before);
    assert!(listed["data"]["proposal_id"].is_null());
}

#[test]
fn pack_verbs_cover_every_cli_operation_and_classify_proposals() {
    let cli = crate::cli::Cli::command();
    let packs = cli.find_subcommand("pack").unwrap();
    let tools = tools();
    for command in packs
        .get_subcommands()
        .filter(|command| command.get_name() != "help")
    {
        let name = format!("pack_{}", command.get_name());
        assert!(
            tools["tools"]
                .as_array()
                .unwrap()
                .iter()
                .any(|tool| tool["name"] == name),
            "{name}"
        );
    }
    for name in ["pack_update", "pack_remove", "pack_configure"] {
        let tool = tools["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|tool| tool["name"] == name)
            .unwrap();
        assert_eq!(tool["annotations"]["readOnlyHint"], false);
        assert_eq!(
            tool["annotations"]["destructiveHint"], false,
            "proposals do not activate changes"
        );
    }
}

#[test]
fn pack_verbs_update_one_or_all_and_remove_only_after_approval() {
    for agent in ["claude", "codex"] {
        for arguments in [json!({"reference":"acme/parent"}), json!({})] {
            let (root, mut context, connection) = fixture();
            context.agent = Some(agent.into());
            let source = dependency_registry(
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
            let shown = invoke(
                &context,
                &connection,
                "pack_show",
                json!({"reference":"acme/child"}),
            );
            assert!(shown["error"].is_null(), "{shown}");
            assert_eq!(
                shown["data"],
                invoke(
                    &context,
                    &connection,
                    "pack_list",
                    json!({"reference":"acme/child"})
                )["data"]
            );
            registry_release(&source, "acme/child", "1.1.0", None);
            let before = Store::open(root.path())
                .unwrap()
                .read()
                .unwrap()
                .policy_revision;
            let updated = invoke(&context, &connection, "pack_update", arguments.clone());
            assert!(updated["error"].is_null(), "{updated}");
            assert_eq!(
                Store::open(root.path())
                    .unwrap()
                    .read()
                    .unwrap()
                    .policy_revision,
                before
            );
            assert_eq!(
                invoke(&context, &connection, "pack_update", arguments)["data"]["proposal_id"],
                updated["data"]["proposal_id"]
            );
            application::apply(&context, &connection, &updated["data"]["apply_arguments"]);
            assert_eq!(
                invoke(
                    &context,
                    &connection,
                    "pack_show",
                    json!({"reference":"acme/child"})
                )["data"]["pack"]["version"],
                "1.1.0"
            );
            let required = invoke(
                &context,
                &connection,
                "pack_remove",
                json!({"reference":"acme/child"}),
            );
            assert_eq!(required["error"]["code"], "dependency_required");
            let before = Store::open(root.path())
                .unwrap()
                .read()
                .unwrap()
                .policy_revision;
            let removed = invoke(
                &context,
                &connection,
                "pack_remove",
                json!({"reference":"acme/parent"}),
            );
            assert!(removed["error"].is_null(), "{removed}");
            assert_eq!(
                Store::open(root.path())
                    .unwrap()
                    .read()
                    .unwrap()
                    .policy_revision,
                before
            );
            application::apply(&context, &connection, &removed["data"]["apply_arguments"]);
            assert_eq!(
                Store::open(root.path()).unwrap().read().unwrap().operator["installed"],
                json!([])
            );
        }
    }
}

#[test]
fn pack_verbs_configure_supports_cli_values_and_rule_overrides_without_a_form() {
    let (root, mut context, connection) = fixture();
    context.agent = Some("claude".into());
    let _source = install_tools(
        &root,
        &context,
        &connection,
        &json!([
            {"id":"build","decision":"allow","conditions":{"domain":"argv","executable":{"var":"tool"},"arguments":{"kind":"any"}}}
        ]),
    );
    for (index, arguments) in [
        json!({"reference":"acme/tools","set":{"tool":"make"},"override":{"build":"ask"}}),
        json!({"reference":"acme/tools","unset":["tool"],"clear_override":["build"]}),
    ]
    .into_iter()
    .enumerate()
    {
        let before = Store::open(root.path())
            .unwrap()
            .read()
            .unwrap()
            .policy_revision;
        let result = invoke(&context, &connection, "pack_configure", arguments);
        assert!(result["error"].is_null(), "{result}");
        assert_eq!(
            Store::open(root.path())
                .unwrap()
                .read()
                .unwrap()
                .policy_revision,
            before
        );
        application::apply(&context, &connection, &result["data"]["apply_arguments"]);
        let active = Store::open(root.path()).unwrap().read().unwrap();
        let config: Value =
            serde_saphyr::from_str(active.candidates["project"]["config"].as_str().unwrap())
                .unwrap();
        if index == 0 {
            assert_eq!(config["packs"][0]["variables"], json!({"tool":"make"}));
            assert!(config["packs"][0]["overrides"].get("build").is_some());
        } else {
            assert!(config["packs"][0].get("variables").is_none());
            assert!(config["packs"][0].get("overrides").is_none());
        }
    }
    for (name, arguments) in [
        ("pack_remove", json!({})),
        ("pack_show", json!({})),
        ("pack_update", json!({"reference":null})),
        ("pack_update", json!({"confirm":true})),
        ("pack_configure", json!({"reference":"acme/tools"})),
        (
            "pack_configure",
            json!({"reference":"acme/tools","variables":["tool"],"set":{"tool":"make"}}),
        ),
    ] {
        assert_eq!(
            invoke(&context, &connection, name, arguments)["error"]["code"],
            "invalid_input"
        );
    }
}
