//! Targeted setup preserves unselected ownership, including unreadable destinations.
use super::*;

fn install(context: &Context, selected: &[&str]) {
    let before = meta(context);
    let plan = prepare(
        Path::new(&context.project_root),
        &before,
        IntegrationChoice::Agents(IntegrationChoice::bits(selected).unwrap()),
    )
    .unwrap()
    .unwrap();
    apply(context, &before, &plan).unwrap();
}

fn selected_files(plan: &Plan) -> Vec<&str> {
    plan.files.iter().map(|file| file.path.as_str()).collect()
}

#[cfg(not(windows))]
fn omitted_snapshot(root: &Path, context: &Context) -> Value {
    let registration = &meta(context)["integration"];
    let mut files = serde_json::Map::new();
    for path in [
        ".mcp.json",
        ".claude/settings.local.json",
        box_definition::PATH,
        box_definition::LOCAL_PATH,
    ] {
        files.insert(path.into(), json!(fs::read(root.join(path)).unwrap()));
    }
    for path in registration["interceptor"]["assets"]
        .as_object()
        .unwrap()
        .keys()
    {
        files.insert(path.clone(), json!(fs::read(root.join(path)).unwrap()));
    }
    json!({"native":registration["agents"]["claude"],
        "connection":registration["connections"]["claude"],
        "fallback":registration["ask_fallback"]["claude"],
        "box":registration["box_definition"],"legacy":registration["interceptor"],
        "files":files})
}

#[cfg(not(windows))]
#[test]
fn codex_install_and_removal_preserve_omitted_native_box_and_legacy_exactly() {
    let (root, context) = fixture();
    fs::create_dir(root.path().join(".box")).unwrap();
    fs::write(root.path().join(".box/config.yml"), "schema_version: 1\n").unwrap();
    install(&context, &["claude"]);
    let before = meta(&context);
    let plan = prepare_with(
        root.path(),
        &before,
        IntegrationChoice::agent("box"),
        false,
        None,
        false,
        false,
    )
    .unwrap()
    .unwrap();
    apply(&context, &before, &plan).unwrap();
    let mut assets = serde_json::Map::new();
    for suffix in [
        ".claude-plugin/plugin.json",
        "hooks/hooks.json",
        "hooks/register.mjs",
        "hooks/config.mjs",
        "hooks/protocol.mjs",
    ] {
        let path = format!(".claude/skills/shield/{suffix}");
        let bytes = format!("Acme retained opaque asset: {suffix}\n");
        let destination = root.path().join(&path);
        fs::create_dir_all(destination.parent().unwrap()).unwrap();
        fs::write(destination, &bytes).unwrap();
        assets.insert(path, json!(bytes));
    }
    Store::open(root.path())
        .unwrap()
        .update(&context, &|| Ok(()), |state| {
            state.operator["integration"]["interceptor"] = json!({"assets":assets});
            Ok(json!({}))
        })
        .unwrap();
    let snapshot = omitted_snapshot(root.path(), &context);
    for choice in [
        IntegrationChoice::agent("codex"),
        IntegrationChoice::RemoveAgents(IntegrationChoice::bits(&["codex"]).unwrap()),
    ] {
        let before = meta(&context);
        let plan = prepare(root.path(), &before, choice).unwrap().unwrap();
        assert_eq!(
            plan.targets.as_deref(),
            Some(["codex".to_owned()].as_slice())
        );
        assert!(
            selected_files(&plan)
                .iter()
                .all(|path| path.starts_with(".codex/"))
        );
        apply(&context, &before, &plan).unwrap();
        assert_eq!(omitted_snapshot(root.path(), &context), snapshot);
    }
}

#[test]
fn codex_only_setup_does_not_read_or_replace_malformed_shared_mcp_file() {
    let (root, context) = fixture();
    let path = root.path().join(".mcp.json");
    fs::write(&path, "{ acme malformed omitted shared MCP").unwrap();
    let before = meta(&context);
    let plan = prepare(root.path(), &before, IntegrationChoice::agent("codex"))
        .unwrap()
        .unwrap();
    assert_eq!(
        plan.targets.as_deref(),
        Some(["codex".to_owned()].as_slice())
    );
    apply(&context, &before, &plan).unwrap();
    assert_eq!(
        fs::read_to_string(path).unwrap(),
        "{ acme malformed omitted shared MCP"
    );
    assert!(root.path().join(".codex/config.toml").is_file());
}

#[test]
fn claude_setup_ignores_malformed_omitted_codex_prefix_rules() {
    let (root, context) = fixture();
    install(&context, &["claude", "codex"]);
    let path = root.path().join(".codex/rules/shield.rules");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, "prefix_rule(acme malformed owned prefix").unwrap();
    let before = meta(&context);
    let codex = before["integration"]["agents"]["codex"].clone();
    let plan = prepare(root.path(), &before, IntegrationChoice::agent("claude"))
        .unwrap()
        .unwrap();
    assert!(!selected_files(&plan).contains(&".codex/rules/shield.rules"));
    assert!(
        !plan.affects("codex"),
        "targets={:?}, reason={:?}",
        plan.targets,
        plan.scope_reason
    );
    let receipt =
        crate::review::integrations::setup::finish(&context, &before, Some(&plan), None).unwrap();
    assert!(receipt["setup_error"].is_null(), "{receipt}");
    assert_eq!(meta(&context)["integration"]["agents"]["codex"], codex);
    assert_eq!(
        fs::read_to_string(path).unwrap(),
        "prefix_rule(acme malformed owned prefix"
    );
}

#[test]
fn tracked_omitted_private_destination_does_not_block_selected_claude_setup() {
    let (root, context) = private_stdio_fixture();
    fs::create_dir(root.path().join(".codex")).unwrap();
    let path = root.path().join(".codex/config.toml");
    fs::write(&path, "# Acme tracked Codex settings\n").unwrap();
    fixture_git(root.path(), &["add", "-f", "--", ".codex/config.toml"]);
    let before = meta(&context);
    let plan = prepare(root.path(), &before, IntegrationChoice::agent("claude"))
        .unwrap()
        .unwrap();
    assert!(
        selected_files(&plan)
            .iter()
            .all(|path| !path.starts_with(".codex/"))
    );
    apply(&context, &before, &plan).unwrap();
    assert_eq!(
        fs::read_to_string(path).unwrap(),
        "# Acme tracked Codex settings\n"
    );
    assert!(meta(&context)["integration"]["connections"]["claude"].is_object());
}

#[test]
fn changed_shared_binary_expands_the_reviewed_targets() {
    let (root, context) = fixture();
    install(&context, &["claude", "codex"]);
    let mut before = meta(&context);
    before["integration"]["binary"] = json!(root.path().join("old-shield"));
    let plan = prepare(root.path(), &before, IntegrationChoice::agent("claude"))
        .unwrap()
        .unwrap();
    assert_eq!(
        plan.targets.as_deref(),
        Some(["claude".to_owned(), "codex".to_owned()].as_slice())
    );
    assert!(
        plan.scope_reason
            .as_ref()
            .is_some_and(|reason| !reason.is_empty())
    );
    assert!(selected_files(&plan).contains(&".codex/config.toml"));
}

#[test]
fn changed_shared_tool_permissions_expand_the_reviewed_targets() {
    let (root, context) = fixture();
    install(&context, &["claude", "codex"]);
    let plan = prepare_approving(
        root.path(),
        &meta(&context),
        IntegrationChoice::agent("claude"),
        false,
        Some(Vec::new()),
        false,
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        plan.targets.as_deref(),
        Some(["claude".to_owned(), "codex".to_owned()].as_slice())
    );
    assert!(
        plan.scope_reason
            .as_ref()
            .is_some_and(|reason| !reason.is_empty())
    );
    assert!(selected_files(&plan).contains(&".codex/config.toml"));
}

fn retain_older_shared_entry(root: &Path, context: &Context) -> Value {
    let current = meta(context);
    let older = json!({"type":"stdio","command":current["integration"]["binary"],
        "args":["__internal","mcp","--project",context.project_root,"--agent","claude-code"]});
    let mut shared = config(root);
    shared["mcpServers"]["shield"] = older.clone();
    fs::write(root.join(".mcp.json"), shared.to_string()).unwrap();
    Store::open(root)
        .unwrap()
        .update(context, &|| Ok(()), |state| {
            state.operator["integration"]["entry"] = older.clone();
            Ok(json!({}))
        })
        .unwrap();
    older
}

#[test]
fn codex_only_setup_preserves_an_older_shared_entry_without_expanding_scope() {
    let (root, context) = fixture();
    install(&context, &["claude", "codex"]);
    let older = retain_older_shared_entry(root.path(), &context);
    let bytes = fs::read(root.path().join(".mcp.json")).unwrap();
    let before = meta(&context);
    let plan = prepare(root.path(), &before, IntegrationChoice::agent("codex"))
        .unwrap()
        .unwrap();
    assert_eq!(
        plan.targets.as_deref(),
        Some(["codex".to_owned()].as_slice())
    );
    assert!(plan.scope_reason.is_none());
    assert_eq!(plan.desired.as_ref().unwrap().entry, older);
    apply(&context, &before, &plan).unwrap();
    assert_eq!(fs::read(root.path().join(".mcp.json")).unwrap(), bytes);
    assert_eq!(meta(&context)["integration"]["entry"], older);
}

#[test]
fn claude_refreshes_older_shared_entry_without_reading_malformed_omitted_codex() {
    let (root, context) = fixture();
    install(&context, &["claude", "codex"]);
    let older = retain_older_shared_entry(root.path(), &context);
    let path = root.path().join(".codex/config.toml");
    fs::write(&path, "[acme malformed omitted configuration").unwrap();
    let before = meta(&context);
    let omitted = json!({"native":before["integration"]["agents"]["codex"],
        "connection":before["integration"]["connections"]["codex"]});
    let plan = prepare(root.path(), &before, IntegrationChoice::agent("claude"))
        .unwrap()
        .unwrap();
    assert_eq!(
        plan.targets.as_deref(),
        Some(["claude".to_owned()].as_slice())
    );
    assert!(plan.scope_reason.is_none());
    assert_ne!(plan.desired.as_ref().unwrap().entry, older);
    assert!(!selected_files(&plan).contains(&".codex/config.toml"));
    apply(&context, &before, &plan).unwrap();
    let after = meta(&context);
    assert_eq!(
        json!({"native":after["integration"]["agents"]["codex"],
        "connection":after["integration"]["connections"]["codex"]}),
        omitted
    );
    assert_ne!(config(root.path())["mcpServers"]["shield"], older);
    assert_eq!(
        fs::read_to_string(path).unwrap(),
        "[acme malformed omitted configuration"
    );
}

#[test]
fn claude_setup_preserves_orphan_codex_fallback_until_codex_is_removed() {
    let (root, context) = fixture();
    install(&context, &["claude"]);
    crate::review::settings::write(
        &context,
        &crate::review::settings::fallbacks(std::collections::BTreeMap::from([(
            "codex".into(),
            AskFallback::Allow,
        )])),
    )
    .unwrap();
    let named = |context: &Context| {
        crate::review::project::load(context)
            .unwrap()
            .settings
            .ask_fallback
            .get("codex")
            .copied()
    };
    let before = meta(&context);
    assert!(before["integration"]["agents"]["codex"].is_null());
    let plan = prepare(root.path(), &before, IntegrationChoice::agent("claude"))
        .unwrap()
        .unwrap();
    assert!(!plan.affects("codex"));
    let receipt =
        crate::review::integrations::setup::finish(&context, &before, Some(&plan), None).unwrap();
    assert!(receipt["setup_error"].is_null(), "{receipt}");
    assert_eq!(named(&context), Some(AskFallback::Allow));
    assert!(!root.path().join(".codex/config.toml").exists());

    let before = meta(&context);
    let removal = IntegrationChoice::RemoveAgents(IntegrationChoice::bits(&["codex"]).unwrap());
    let plan = prepare(root.path(), &before, removal).unwrap().unwrap();
    apply(&context, &before, &plan).unwrap();
    // Removing the agent revokes its fallback in the policy files.
    assert_eq!(named(&context), Some(AskFallback::Block));
}
