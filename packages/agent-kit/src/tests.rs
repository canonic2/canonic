use super::*;
use serde_json::json;
use std::fs;

#[test]
fn explicit_path_launch_policy_and_product_names_are_respected_in_both_scopes() {
    let (_temp, mut context) = fixture();
    context.product = "acme".into();
    context.launch_command = Some("acme".into());
    for scope in [Scope::Project, Scope::User] {
        let selected = Options {
            scope,
            agents: vec!["claude".into()],
            ..Default::default()
        };
        let review = prepare(&context, &selected).unwrap().review();
        assert_eq!(review["registrations"][0]["entry"]["command"], "acme");
    }
    context.product = "../outside".into();
    assert!(prepare(&context, &options(&["claude"])).is_err());
    assert!(!context.root.join(".mcp.json").exists());
}

#[cfg(unix)]
#[test]
fn discovery_defaults_follow_catalog_scope_and_explicit_intent() {
    use std::os::unix::fs::PermissionsExt;

    let (temp, context) = fixture();
    let bin = temp.path().join("cli");
    fs::create_dir(&bin).unwrap();
    for (_, names) in agents::CATALOG {
        for name in *names {
            let path = bin.join(name);
            // Valid executable metadata; discovery must never try to run it.
            fs::write(&path, "not a runnable program").unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        }
    }
    // Editor launchers and ambiguous names cannot establish agent identity.
    for name in [
        "code", "cursor", "kilo", "kiro", "windsurf", "copilot", "goose",
    ] {
        let path = bin.join(name);
        fs::write(&path, "not a runnable program").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let paths = vec![temp.path().join("missing"), bin];
    for scope in [Scope::Project, Scope::User] {
        let mut selected = Options {
            scope,
            ..Options::default()
        };
        let expected: Vec<_> = agents::CATALOG
            .iter()
            .filter(|(id, names)| {
                !names.is_empty() && agents::destination(&context, id, scope).is_ok()
            })
            .map(|(id, _)| (*id).to_owned())
            .collect();
        let detected = detection::defaults_on_path(&context, &selected, &paths);
        assert_eq!(detected, expected);
        for id in ["claude", "codex", "kimi", "qwen"] {
            assert!(detected.iter().any(|agent| agent == id));
        }
        for id in [
            "copilot-vscode",
            "cursor",
            "kilo",
            "kiro",
            "windsurf",
            "cline",
            "augment",
            "copilot-cli",
            "goose",
        ] {
            assert!(!detected.iter().any(|agent| agent == id));
        }
        selected.remove = true;
        assert!(detection::defaults_on_path(&context, &selected, &paths).is_empty());
        selected.agents = vec!["cursor".into()];
        assert_eq!(
            detection::defaults_on_path(&context, &selected, &paths),
            ["cursor"]
        );
        selected.remove = false;
        assert_eq!(
            detection::defaults_on_path(&context, &selected, &paths),
            ["cursor"]
        );
    }
    assert!(detection::defaults_on_path(&context, &Options::default(), &[]).is_empty());
}

#[test]
fn box_setup_writes_an_owned_definition_that_box_accepts() {
    let (_temp, context) = fixture();
    let context = context.with_tools(vec![
        Tool::new("observe", "Read the screen.", true),
        Tool::new("app_uninstall", "Remove an app.", false),
    ]);
    let options = options(&["box"]);
    // Box is offered first, and only where Box is configured.
    assert_eq!(agents::CATALOG.first().map(|(id, _)| *id), Some("box"));
    let error = prepare(&context, &options).err().unwrap();
    assert!(matches!(error, Error::InvalidSelection(_)), "{error}");
    assert!(error.to_string().contains("box config init"), "{error}");
    fs::create_dir_all(context.root.join(".box")).unwrap();
    fs::write(context.root.join(".box/config.yml"), "schema_version: 1\n").unwrap();
    assert_eq!(
        detection::defaults_on_path(&context, &Options::default(), &[]),
        ["box"]
    );

    let plan = prepare(&context, &options).unwrap();
    assert_eq!(
        plan.review()["tool_approvals"]["approved"],
        json!(["observe"])
    );
    plan.apply().unwrap();
    let path = context.root.join(".box/mcp/acme.yml");
    let text = fs::read_to_string(&path).unwrap();
    // The definition Box reads: the PATH command, its own arguments, the
    // approved tool allowed and the other left to ask.
    assert_eq!(
        text,
        "# Written by `acme mcp setup --agent box`; rerun it to change this file.\n\
         schema_version: 1\n\
         connection:\n  transport: stdio\n  command: \"acme\"\n  args: [\"mcp\", \"serve\"]\n\
         permissions:\n  tools:\n    \"observe\": allow\n"
    );

    // Rerunning changes nothing and keeps the earlier approvals.
    let review = prepare(&context, &options).unwrap().review();
    assert_eq!(review["registrations"][0]["change"], "unchanged");
    assert_eq!(review["tool_approvals"]["approved"], json!(["observe"]));

    // An edit is the user's; removal deletes only the owned file.
    fs::write(&path, format!("{text}# mine\n")).unwrap();
    assert!(matches!(
        prepare(&context, &options),
        Err(Error::Conflict(_))
    ));
    fs::write(&path, &text).unwrap();
    let removal = Options {
        remove: true,
        ..options.clone()
    };
    prepare(&context, &removal).unwrap().apply().unwrap();
    assert!(!path.exists());

    // An unowned file is taken over only when it is exactly what setup writes.
    fs::write(&path, "schema_version: 1\n").unwrap();
    assert!(matches!(
        prepare(&context, &options),
        Err(Error::Conflict(_))
    ));
    fs::write(&path, &text).unwrap();
    prepare(&context, &options).unwrap().apply().unwrap();
    prepare(&context, &removal).unwrap().apply().unwrap();
    assert!(!path.exists());
}

#[cfg(unix)]
#[test]
fn discovery_ignores_bad_candidates_and_follows_executable_symlinks() {
    use std::os::unix::fs::{PermissionsExt, symlink};

    let (temp, context) = fixture();
    let bin = temp.path().join("cli");
    fs::create_dir(&bin).unwrap();
    let claude = bin.join("claude");
    fs::write(&claude, "fixture").unwrap();
    fs::set_permissions(&claude, fs::Permissions::from_mode(0o644)).unwrap();
    fs::create_dir(bin.join("codex")).unwrap();
    symlink(bin.join("missing"), bin.join("kimi")).unwrap();
    let paths = vec![bin.clone()];
    let selected = Options::default();
    assert!(detection::defaults_on_path(&context, &selected, &paths).is_empty());

    fs::set_permissions(&claude, fs::Permissions::from_mode(0o755)).unwrap();
    symlink(&claude, bin.join("qwen")).unwrap();
    assert_eq!(
        detection::defaults_on_path(&context, &selected, &paths),
        ["claude", "qwen"]
    );
    // An ambiguous CLI remains unselected, even at its supported user scope.
    symlink(&claude, bin.join("goose")).unwrap();
    symlink(&claude, bin.join("copilot")).unwrap();
    assert_eq!(
        detection::defaults_on_path(&context, &selected, &paths),
        ["claude", "qwen"]
    );
    // Existing executables reached via relative PATH entries are not defaults.
    let upward: std::path::PathBuf = std::env::current_dir()
        .unwrap()
        .ancestors()
        .skip(1)
        .map(|_| "..")
        .collect();
    let relative = upward.join(bin.canonicalize().unwrap().strip_prefix("/").unwrap());
    let relative_paths = vec![std::path::PathBuf::new(), ".".into(), relative];
    let user = Options {
        scope: Scope::User,
        ..Options::default()
    };
    assert!(detection::defaults_on_path(&context, &user, &relative_paths).is_empty());
    assert_eq!(
        detection::defaults_on_path(&context, &user, &paths),
        ["claude", "qwen"]
    );
    let explicit = Options {
        agents: vec!["goose".into(), "copilot-cli".into()],
        ..user
    };
    assert_eq!(
        detection::defaults_on_path(&context, &explicit, &paths),
        ["goose", "copilot-cli"]
    );
}

fn fixture() -> (tempfile::TempDir, Context) {
    let temp = tempfile::tempdir().unwrap();
    let base = temp.path().canonicalize().unwrap();
    let context = Context {
        product: "acme".into(),
        program: base.join("Program Files/acme"),
        arguments: vec!["mcp".into(), "serve".into()],
        launch_command: None,
        check_arguments: vec!["--check".into()],
        root: base.join("project"),
        home: base.join("home"),
        state: base.join("state"),
        agent_homes: BTreeMap::default(),
        tools: Vec::new(),
    };
    fs::create_dir_all(&context.root).unwrap();
    fs::create_dir_all(&context.home).unwrap();
    (temp, context)
}
fn options(ids: &[&str]) -> Options {
    Options {
        agents: ids.iter().map(|s| (*s).into()).collect(),
        ..Options::default()
    }
}

#[test]
fn install_repeat_relocate_and_remove_preserve_other_servers_and_comments() {
    let (_temp, mut context) = fixture();
    let path = context.root.join("opencode.jsonc");
    fs::write(&path, "{\n // user comment\n \"mcp\": {\"other\": {\"url\": \"https://example.com/mcp\"},},\n \"setting\": true,\n}\n").unwrap();
    prepare(&context, &options(&["opencode"]))
        .unwrap()
        .apply()
        .unwrap();
    let installed = fs::read_to_string(&path).unwrap();
    assert!(installed.contains("// user comment"));
    assert!(installed.contains("\"other\": {\"url\": \"https://example.com/mcp\"}"));
    // A shareable project file names the command, not this user's paths.
    assert!(installed.contains("\"acme\""));
    assert!(!installed.contains("Program Files"));
    assert!(!installed.contains("cwd"));
    prepare(&context, &options(&["opencode"]))
        .unwrap()
        .apply()
        .unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), installed);
    // Relocating the install leaves project files as they are.
    context.program = context.home.join("new acme");
    prepare(&context, &options(&["opencode"]))
        .unwrap()
        .apply()
        .unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), installed);
    // User files keep the absolute install path and follow a relocation.
    let mut user = options(&["codex"]);
    user.scope = Scope::User;
    prepare(&context, &user).unwrap().apply().unwrap();
    let user_file = fs::read_to_string(context.home.join(".codex/config.toml")).unwrap();
    assert!(user_file.contains("new acme"));
    let mut remove = options(&["opencode"]);
    remove.remove = true;
    prepare(&context, &remove).unwrap().apply().unwrap();
    let removed = fs::read_to_string(&path).unwrap();
    assert!(!removed.contains("acme"));
    assert!(removed.contains("// user comment"));
    assert!(removed.contains("\"setting\": true"));
    prepare(&context, &remove).unwrap().apply().unwrap();
}

#[test]
fn project_scope_from_home_or_above_writes_nothing() {
    let (_temp, mut context) = fixture();
    let home = context.home.clone();
    for root in [home.clone(), home.parent().unwrap().to_path_buf()] {
        context.root = root;
        for agent in ["codex", "gemini", "cursor", "claude"] {
            assert!(prepare(&context, &options(&[agent])).is_err(), "{agent}");
        }
    }
    assert_eq!(fs::read_dir(&home).unwrap().count(), 0);
    assert!(!context.state.exists());
    // User scope from home stays available.
    let mut user = options(&["codex"]);
    user.scope = Scope::User;
    prepare(&context, &user).unwrap().apply().unwrap();
    assert!(home.join(".codex/config.toml").is_file());
}

#[test]
fn all_selected_files_are_validated_before_writes() {
    let (_temp, context) = fixture();
    fs::create_dir(context.root.join(".codex")).unwrap();
    fs::write(context.root.join(".codex/config.toml"), "not valid [toml").unwrap();
    assert!(prepare(&context, &options(&["claude", "codex"])).is_err());
    assert!(!context.root.join(".mcp.json").exists());
    assert!(!context.state.exists());
}

#[test]
fn ownership_conflicts_and_concurrent_review_changes_are_refused() {
    let (_temp, context) = fixture();
    let path = context.root.join(".mcp.json");
    fs::write(&path, "{\"mcpServers\":{\"acme\":{\"command\":\"mine\"}}}").unwrap();
    assert!(matches!(
        prepare(&context, &options(&["claude"])),
        Err(Error::Conflict(_))
    ));
    fs::write(&path, "{}").unwrap();
    let plan = prepare(&context, &options(&["claude"])).unwrap();
    fs::write(&path, "{\"concurrent\":true}").unwrap();
    assert!(matches!(plan.apply(), Err(Error::Conflict(_))));
    assert_eq!(fs::read_to_string(&path).unwrap(), "{\"concurrent\":true}");
    prepare(&context, &options(&["claude"]))
        .unwrap()
        .apply()
        .unwrap();
    let source = fs::read_to_string(&path)
        .unwrap()
        .replace("serve", "changed");
    fs::write(&path, &source).unwrap();
    assert!(matches!(
        prepare(&context, &options(&["claude"])),
        Err(Error::Conflict(_))
    ));
    let mut remove = options(&["claude"]);
    remove.remove = true;
    assert!(prepare(&context, &remove).is_err());
    assert_eq!(fs::read_to_string(&path).unwrap(), source);
}

#[test]
fn staged_replacement_refuses_destinations_edited_after_preparation() {
    let (_temp, context) = fixture();
    let path = context.root.join(".mcp.json");
    fs::write(&path, "{}").unwrap();
    let staged = storage::stage(&path, "{\"mcpServers\":{}}", false).unwrap();
    fs::write(&path, "{\"concurrent\":true}").unwrap();
    assert!(staged.replace(Some("{}")).is_err());
    assert_eq!(fs::read_to_string(&path).unwrap(), "{\"concurrent\":true}");
    let created = context.root.join(".cursor.json");
    let staged = storage::stage(&created, "{}", false).unwrap();
    fs::write(&created, "{\"concurrent\":true}").unwrap();
    assert!(staged.replace(None).is_err());
    assert_eq!(
        fs::read_to_string(&created).unwrap(),
        "{\"concurrent\":true}"
    );
    assert_eq!(fs::read_dir(&context.root).unwrap().count(), 2);
    storage::stage(&path, "{}", false)
        .unwrap()
        .replace(Some("{\"concurrent\":true}"))
        .unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), "{}");
}

#[test]
fn missing_owned_entry_is_repaired_and_shared_surfaces_are_deduplicated() {
    let (_temp, context) = fixture();
    let options = options(&["claude", "copilot-cli", "claude"]);
    let plan = prepare(&context, &options).unwrap();
    assert_eq!(plan.review()["registrations"].as_array().unwrap().len(), 1);
    plan.apply().unwrap();
    fs::write(context.root.join(".mcp.json"), "{\"user\":true}").unwrap();
    prepare(&context, &options).unwrap().apply().unwrap();
    let text = fs::read_to_string(context.root.join(".mcp.json")).unwrap();
    assert!(text.contains("\"user\":true"));
    assert!(text.contains("acme"));
}

#[test]
fn partial_write_repair_preserves_the_reviewed_entry() {
    let (_temp, context) = fixture();
    let plan = prepare(&context, &options(&["claude"])).unwrap();
    let review = plan.review();
    let path = context.root.join(".mcp.json");
    let identity = storage::identity(&path);
    let ledger = context.state.join("acme").join(format!("{identity}.json"));
    let record = storage::Record {
        version: 1,
        path: path.clone(),
        product: "acme".into(),
        previous: None,
        desired: Some(review["registrations"][0]["entry"].clone()),
        pending: true,
    };
    storage::write(
        &ledger,
        &serde_json::to_string(&record).unwrap(),
        true,
        None,
    )
    .unwrap();
    prepare(&context, &options(&["claude"]))
        .unwrap()
        .apply()
        .unwrap();
    assert!(path.exists());
    let record: storage::Record =
        serde_json::from_str(&fs::read_to_string(ledger).unwrap()).unwrap();
    assert!(!record.pending);
    assert_eq!(record.desired, record.previous);
}

#[test]
fn publication_failure_reports_completed_and_pending_agents() {
    let (_temp, context) = fixture();
    let path = context.root.join(".qwen/settings.json");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    // Valid input fits the read budget; adding the server exceeds the write budget.
    fs::write(
        &path,
        format!(
            "{{\"padding\":\"{}\"}}",
            "x".repeat(usize::try_from(storage::LIMIT).unwrap() - 32)
        ),
    )
    .unwrap();
    let error = prepare(&context, &options(&["claude", "qwen"]))
        .unwrap()
        .apply()
        .unwrap_err();
    let Error::Partial {
        completed,
        pending,
        source,
    } = error
    else {
        panic!("Expected a partial publication failure")
    };
    assert_eq!(completed, ["claude", "copilot-cli"]);
    assert_eq!(pending, ["qwen"]);
    assert!(matches!(*source, Error::Invalid(_)));
    assert!(
        fs::read_to_string(context.root.join(".mcp.json"))
            .unwrap()
            .contains("acme")
    );
    assert!(!fs::read_to_string(path).unwrap().contains("acme"));
}

#[test]
fn project_default_user_scope_and_unavailable_scopes_are_separate() {
    let (_temp, context) = fixture();
    prepare(
        &context,
        &options(&[
            "qwen",
            "kimi",
            "gemini",
            "cursor",
            "kiro",
            "kilo",
            "opencode",
            "amp",
            "droid",
            "copilot-vscode",
        ]),
    )
    .unwrap()
    .apply()
    .unwrap();
    assert!(!context.home.join(".qwen/settings.json").exists());
    let mut user = options(&["qwen"]);
    user.scope = Scope::User;
    prepare(&context, &user).unwrap().apply().unwrap();
    assert!(context.home.join(".qwen/settings.json").exists());
    let text = fs::read_to_string(context.home.join(".qwen/settings.json")).unwrap();
    assert!(!text.contains("cwd"));
    for id in ["goose", "cline", "augment", "windsurf", "example-unknown"] {
        assert!(matches!(
            prepare(&context, &options(&[id])),
            Err(Error::InvalidSelection(_))
        ));
    }
}

#[test]
fn toml_entry_changes_preserve_other_settings_and_servers() {
    use agents::Format;
    let text = "# preserved\nmodel = 'example'\n[mcp_servers.other]\ncommand = 'other'\n";
    let entry = json!({"command":"/Program Files/acme", "args":["serve"], "cwd":"/project"});
    let (before, installed) =
        document::edit(text, Format::Toml, "mcp_servers", "acme", Some(&entry)).unwrap();
    assert!(before.is_none());
    assert!(installed.contains("# preserved\nmodel = 'example'"));
    assert!(installed.contains("command = 'other'"));
    let (current, repeated) = document::edit(
        &installed,
        Format::Toml,
        "mcp_servers",
        "acme",
        Some(&entry),
    )
    .unwrap();
    assert_eq!(current, Some(entry));
    assert_eq!(repeated, installed);
    let (_, removed) =
        document::edit(&installed, Format::Toml, "mcp_servers", "acme", None).unwrap();
    assert!(removed.contains("mcp_servers.other"));
    assert!(!removed.contains("Program Files"));
    let inline = "mcp_servers = { other = { command = 'other' } }\n";
    let entry = json!({"command":"/opt/acme/acme", "args":["serve"]});
    let (_, edited) =
        document::edit(inline, Format::Toml, "mcp_servers", "acme", Some(&entry)).unwrap();
    let (actual, _) = document::edit(&edited, Format::Toml, "mcp_servers", "acme", None).unwrap();
    assert_eq!(actual, Some(entry));
    assert!(edited.contains("other"));
}

#[test]
fn jsonc_editing_preserves_comments_and_rejects_ambiguous_documents() {
    use agents::Format;
    for text in [
        "{\"mcpServers\":{\"a\":1,\"owned\":2,\"b\":3}}",
        "{\"mcpServers\":{\"a\":1,\"owned\":2}}",
        "{\"mcpServers\":{\"owned\":2, /* keep */}}",
    ] {
        let (_, removed) =
            document::edit(text, Format::Jsonc, "mcpServers", "owned", None).unwrap();
        assert!(
            document::edit(&removed, Format::Jsonc, "mcpServers", "owned", None).is_ok(),
            "{removed}"
        );
        if text.contains("keep") {
            assert!(removed.contains("/* keep */"));
        }
    }
    for text in [
        "{\"x\":1,\"x\":2}",
        "{\"mcpServers\":[]}",
        "{} trailing",
        "{ /* unclosed",
        "[]",
        "{\"x\": [1 2]}",
    ] {
        assert!(
            document::edit(text, Format::Json, "mcpServers", "owned", None).is_err(),
            "{text}"
        );
    }
}

#[test]
fn dry_run_and_noninteractive_apply_never_write() {
    let (_temp, context) = fixture();
    let mut preview = options(&["claude"]);
    preview.dry_run = true;
    let value = run(&context, preview, true).unwrap();
    assert_eq!(value["scope"], "project");
    assert!(matches!(
        run(&context, options(&["claude"]), true),
        Err(Error::ConfirmationRequired(_))
    ));
    assert!(matches!(
        run(&context, Options::default(), true),
        Err(Error::ConfirmationRequired(_))
    ));
    assert!(!context.state.exists());
    assert!(!context.root.join(".mcp.json").exists());
}

#[test]
fn yes_applies_without_a_terminal_after_the_catalog_check() {
    let (_temp, context) = fixture();
    let mut install = options(&["claude"]);
    install.yes = true;
    // The fixture's program does not exist, so the catalog check refuses
    // before any write; --yes skips only the confirmation.
    assert!(matches!(
        run(&context, install, true),
        Err(Error::CatalogCheck(_))
    ));
    assert!(!context.root.join(".mcp.json").exists());
    prepare(&context, &options(&["claude"]))
        .unwrap()
        .apply()
        .unwrap();
    assert!(
        fs::read_to_string(context.root.join(".mcp.json"))
            .unwrap()
            .contains("acme")
    );
    let mut remove = options(&["claude"]);
    remove.remove = true;
    remove.yes = true;
    run(&context, remove, true).unwrap();
    assert!(
        !fs::read_to_string(context.root.join(".mcp.json"))
            .unwrap()
            .contains("acme")
    );
}

#[cfg(unix)]
#[test]
fn linked_destinations_refuse_and_existing_file_modes_survive() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let (_temp, context) = fixture();
    symlink(&context.home, context.root.join(".qwen")).unwrap();
    assert!(prepare(&context, &options(&["qwen"])).is_err());
    let path = context.root.join(".mcp.json");
    fs::write(&path, "{}").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
    prepare(&context, &options(&["claude"]))
        .unwrap()
        .apply()
        .unwrap();
    assert_eq!(
        fs::metadata(path).unwrap().permissions().mode() & 0o777,
        0o640
    );
}

#[cfg(unix)]
#[test]
fn special_settings_files_are_refused_without_blocking() {
    let (_temp, context) = fixture();
    let path = context.root.join(".qwen/settings.json");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    assert!(
        std::process::Command::new("mkfifo")
            .arg(&path)
            .status()
            .unwrap()
            .success()
    );
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut preview = options(&["qwen"]);
        preview.dry_run = true;
        let _ = sender.send(run(&context, preview, true).is_err());
    });
    // A regression blocks in open; the deadline only bounds that hang.
    assert_eq!(
        receiver.recv_timeout(std::time::Duration::from_secs(10)),
        Ok(true)
    );
}

#[test]
fn nested_project_resolution_stops_at_the_closest_checkout() {
    let (_temp, context) = fixture();
    fs::write(context.root.join("gate.yml"), "").unwrap();
    let nested = context.root.join("nested/project/deep");
    fs::create_dir_all(&nested).unwrap();
    assert_eq!(project_root(&nested).unwrap(), context.root);
    fs::write(
        context.root.join("nested/project/.git"),
        "gitdir: elsewhere",
    )
    .unwrap();
    assert_eq!(
        project_root(&nested).unwrap(),
        context.root.join("nested/project")
    );
}

#[test]
fn gate_repository_metadata_resolves_to_its_workspace_but_tasks_stay_boundaries() {
    let (_temp, context) = fixture();
    fs::write(context.root.join("gate.yml"), "").unwrap();
    let metadata = context.root.join(".gate/repos/a/.git/hooks");
    fs::create_dir_all(&metadata).unwrap();
    assert_eq!(project_root(&metadata).unwrap(), context.root);
    assert_eq!(
        project_root(&context.root.join(".gate/repos/a")).unwrap(),
        context.root
    );
    let task = context.root.join(".gate/tasks/acme");
    fs::create_dir_all(task.join("src")).unwrap();
    fs::write(task.join(".git"), "gitdir: elsewhere").unwrap();
    assert_eq!(project_root(&task.join("src")).unwrap(), task);
}

#[test]
fn user_only_surfaces_and_yaml_preserve_other_values() {
    let (_temp, context) = fixture();
    let mut options = options(&["goose", "augment-cli", "cline-cli", "kilo"]);
    options.scope = Scope::User;
    let path = context.home.join(".config/goose/config.yaml");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(
        &path,
        "active_provider: example\nextensions:\n  other:\n    type: builtin\n    name: other\n",
    )
    .unwrap();
    let plan = prepare(&context, &options).unwrap();
    assert!(human(&plan.review()).contains("comments and formatting"));
    plan.apply().unwrap();
    let result: serde_json::Value =
        serde_saphyr::from_str(&fs::read_to_string(path).unwrap()).unwrap();
    assert_eq!(result["active_provider"], "example");
    assert_eq!(result["extensions"]["other"]["name"], "other");
    assert_eq!(result["extensions"]["acme"]["type"], "stdio");
}

#[test]
fn other_scope_entries_are_reported_and_preserved() {
    let (_temp, context) = fixture();
    let path = context.home.join(".qwen/settings.json");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let existing = "{\"mcpServers\":{\"acme\":{\"command\":\"other\"}}}";
    fs::write(&path, existing).unwrap();
    let plan = prepare(&context, &options(&["qwen"])).unwrap();
    assert!(
        plan.review()["other_scopes"][0]["status"]
            .as_str()
            .unwrap()
            .contains("exists")
    );
    plan.apply().unwrap();
    assert_eq!(fs::read_to_string(path).unwrap(), existing);
}

fn with_tools(context: Context) -> Context {
    context.with_tools(vec![
        Tool::new("observe", "Read the screen.", true),
        Tool::new("act", "Send input.", true),
        Tool::new("app_uninstall", "Remove an app.", false),
    ])
}

#[test]
fn documentation_defaults_reach_hosts_and_saved_approvals_keep_new_tools_unmarked() {
    for existing in [false, true] {
        let (_temp, context) = fixture();
        let mut context = with_tools(context);
        if existing {
            prepare(&context, &options(&["claude", "codex"]))
                .unwrap()
                .apply()
                .unwrap();
        }
        let docs = [json!({"name":"docs_read","description":"Read an embedded guide."})];
        context.tools.extend(docs.iter().map(|tool| {
            Tool::new(
                tool["name"].as_str().unwrap(),
                tool["description"].as_str().unwrap(),
                true,
            )
        }));
        let plan = prepare(&context, &options(&["claude", "codex"])).unwrap();
        let review = plan.review();
        for tool in &docs {
            let field = if existing { "ask" } else { "approved" };
            assert!(
                review["tool_approvals"][field]
                    .as_array()
                    .unwrap()
                    .contains(&tool["name"])
            );
        }
        plan.apply().unwrap();
        let codex: toml_edit::DocumentMut =
            fs::read_to_string(context.root.join(".codex/config.toml"))
                .unwrap()
                .parse()
                .unwrap();
        let rules = claude_rules(&context);
        for tool in &docs {
            let name = tool["name"].as_str().unwrap();
            assert_eq!(
                codex["mcp_servers"]["acme"]["tools"][name]["approval_mode"].as_str(),
                Some(if existing { "prompt" } else { "approve" })
            );
            assert_eq!(rules.contains(&format!("mcp__acme__{name}")), !existing);
        }
    }
}

fn claude_rules(context: &Context) -> Vec<String> {
    let text = fs::read_to_string(context.root.join(".claude/settings.local.json")).unwrap();
    serde_json::from_str::<serde_json::Value>(&text).unwrap()["permissions"]["allow"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn default_approvals_reach_claude_rules_and_codex_tool_settings() {
    let (_temp, context) = fixture();
    let context = with_tools(context);
    let settings = context.root.join(".claude/settings.local.json");
    fs::create_dir_all(settings.parent().unwrap()).unwrap();
    fs::write(
        &settings,
        "{\"permissions\":{\"allow\":[\"Bash(ls)\",\"mcp__acme__act\"]},\"model\":\"acme\"}",
    )
    .unwrap();
    let plan = prepare(&context, &options(&["claude", "codex"])).unwrap();
    let review = plan.review();
    assert_eq!(
        review["tool_approvals"]["approved"],
        json!(["observe", "act"])
    );
    assert_eq!(review["tool_approvals"]["ask"], json!(["app_uninstall"]));
    assert!(human(&review).contains("Ask each time: app_uninstall"));
    plan.apply().unwrap();
    assert_eq!(
        claude_rules(&context),
        ["Bash(ls)", "mcp__acme__act", "mcp__acme__observe"]
    );
    assert!(
        fs::read_to_string(&settings)
            .unwrap()
            .contains("\"model\":\"acme\"")
    );
    let codex = fs::read_to_string(context.root.join(".codex/config.toml")).unwrap();
    let parsed: toml_edit::DocumentMut = codex.parse().unwrap();
    let tools = &parsed["mcp_servers"]["acme"]["tools"];
    assert_eq!(tools["observe"]["approval_mode"].as_str(), Some("approve"));
    assert_eq!(
        tools["app_uninstall"]["approval_mode"].as_str(),
        Some("prompt")
    );
    assert_eq!(
        parsed["mcp_servers"]["acme"]["default_tools_approval_mode"].as_str(),
        Some("prompt")
    );
    // A repeated setup keeps the configured approvals and changes nothing.
    let repeat = prepare(&context, &options(&["claude", "codex"])).unwrap();
    assert!(
        repeat.review()["registrations"]
            .as_array()
            .unwrap()
            .iter()
            .all(|row| row["change"] == "unchanged")
    );
    repeat.apply().unwrap();
    assert_eq!(
        fs::read_to_string(context.root.join(".codex/config.toml")).unwrap(),
        codex
    );
}

#[test]
fn changed_and_removed_approvals_keep_rules_the_user_wrote() {
    let (_temp, context) = fixture();
    let context = with_tools(context);
    let settings = context.root.join(".claude/settings.local.json");
    fs::create_dir_all(settings.parent().unwrap()).unwrap();
    fs::write(
        &settings,
        "{\"permissions\":{\"allow\":[\"mcp__acme__act\"]}}",
    )
    .unwrap();
    prepare(&context, &options(&["claude"]))
        .unwrap()
        .apply()
        .unwrap();
    let mut narrowed = options(&["claude"]);
    narrowed.approved = Some(vec!["act".into(), "app_uninstall".into()]);
    prepare(&context, &narrowed).unwrap().apply().unwrap();
    assert_eq!(
        claude_rules(&context),
        ["mcp__acme__act", "mcp__acme__app_uninstall"]
    );
    // Without a new selection, setup keeps what is configured.
    let review = prepare(&context, &options(&["claude"])).unwrap().review();
    assert_eq!(
        review["tool_approvals"]["approved"],
        json!(["act", "app_uninstall"])
    );
    let mut remove = options(&["claude"]);
    remove.remove = true;
    prepare(&context, &remove).unwrap().apply().unwrap();
    assert_eq!(claude_rules(&context), ["mcp__acme__act"]);
    let ledger = context
        .state
        .join("acme")
        .join(format!("{}.json", storage::identity(&settings)));
    assert!(!ledger.exists());
}

#[test]
fn review_reports_tools_still_approved_by_rules_setup_did_not_write() {
    let (_temp, context) = fixture();
    let context = with_tools(context);
    let settings = context.root.join(".claude/settings.local.json");
    fs::create_dir_all(settings.parent().unwrap()).unwrap();
    fs::write(
        &settings,
        "{\"permissions\":{\"allow\":[\"mcp__acme__act\"]}}",
    )
    .unwrap();
    let mut narrowed = options(&["claude"]);
    narrowed.approved = Some(vec!["observe".into()]);
    let review = prepare(&context, &narrowed).unwrap().review();
    let approvals = &review["tool_approvals"];
    assert_eq!(approvals["ask"], json!(["app_uninstall"]), "{approvals}");
    assert_eq!(approvals["approved_by_other_rules"], json!(["act"]));
    assert_eq!(approvals["other_rules"], json!(["mcp__acme__act"]));
    assert!(human(&review).contains("rules setup did not write"));
    // A server-wide rule approves every tool, whatever is marked.
    fs::write(&settings, "{\"permissions\":{\"allow\":[\"mcp__acme\"]}}").unwrap();
    let review = prepare(&context, &narrowed).unwrap().review();
    assert_eq!(review["tool_approvals"]["ask"], json!([]));
    assert_eq!(
        review["tool_approvals"]["approved_by_other_rules"],
        json!(["act", "app_uninstall"])
    );
    // Rules setup wrote itself are not reported as someone else's.
    fs::write(&settings, "{}").unwrap();
    prepare(&context, &options(&["claude"]))
        .unwrap()
        .apply()
        .unwrap();
    let review = prepare(&context, &narrowed).unwrap().review();
    assert_eq!(
        review["tool_approvals"]["approved_by_other_rules"],
        json!([])
    );
}

#[test]
fn products_without_tools_write_no_permission_settings() {
    let (_temp, context) = fixture();
    let plan = prepare(&context, &options(&["claude", "codex"])).unwrap();
    assert!(plan.review()["tool_approvals"].is_null());
    plan.apply().unwrap();
    assert!(!context.root.join(".claude").exists());
    let codex = fs::read_to_string(context.root.join(".codex/config.toml")).unwrap();
    assert!(!codex.contains("approval"));
}

#[test]
fn removal_of_an_already_missing_file_clears_ownership_without_recreating_it() {
    let (_temp, context) = fixture();
    let selected = options(&["claude"]);
    prepare(&context, &selected).unwrap().apply().unwrap();
    let path = context.root.join(".mcp.json");
    let ledger = context
        .state
        .join("acme")
        .join(format!("{}.json", storage::identity(&path)));
    assert!(ledger.exists());
    fs::remove_file(&path).unwrap();
    let mut remove = selected;
    remove.remove = true;
    prepare(&context, &remove).unwrap().apply().unwrap();
    assert!(!path.exists());
    assert!(!ledger.exists());
}
