//! Box MCP definition setup against real project files and state: refusal,
//! exact content, dry runs, owned removal and repair of edited definitions.
//! Box hook commands are POSIX command lines, so Windows refuses Box setup.
#![cfg(not(windows))]
use super::tests::fixture;
use super::*;
use crate::review::integrations::{fallback, status};

fn meta(context: &Context) -> Value {
    crate::review::project::metadata(&crate::review::project::load(context).unwrap())
}

fn choice(ids: &[&str]) -> IntegrationChoice {
    IntegrationChoice::Agents(IntegrationChoice::bits(ids).unwrap())
}

fn removal(ids: &[&str]) -> IntegrationChoice {
    IntegrationChoice::RemoveAgents(IntegrationChoice::bits(ids).unwrap())
}

fn box_project(root: &Path) {
    fs::create_dir_all(root.join(".box")).unwrap();
    fs::write(root.join(".box/config.yml"), "schema_version: 1\n").unwrap();
}

fn definition(root: &Path) -> Option<String> {
    fs::read_to_string(root.join(box_definition::PATH)).ok()
}

fn local(root: &Path) -> Option<String> {
    fs::read_to_string(root.join(box_definition::LOCAL_PATH)).ok()
}

/// Prepare and apply `choice` like a confirmed `shield mcp setup`.
fn setup(context: &Context, choice: IntegrationChoice) -> Result<Plan, Error> {
    let before = meta(context);
    let plan =
        prepare(Path::new(&context.project_root), &before, choice)?.expect("an integration plan");
    apply(context, &before, &plan)?;
    Ok(plan)
}

fn coverage(context: &Context) -> Value {
    status(context, &crate::review::project::load(context).unwrap())
}

#[test]
fn box_portability_copy_preserves_review_and_rebinds_only_owned_local_pin() {
    let (root, context) = fixture();
    box_project(root.path());
    let plan = setup(&context, choice(&["box"])).unwrap();
    let mut owned = plan.desired.unwrap();
    owned.binary = Path::new("/acme/previous-install/shield").to_path_buf();
    let previous =
        box_definition::render(&owned.binary, &crate::review::mcp::default_approved()).unwrap();
    fs::write(root.path().join(box_definition::PATH), &previous.shared).unwrap();
    fs::write(
        root.path().join(box_definition::LOCAL_PATH),
        &previous.local,
    )
    .unwrap();
    owned.box_definition = Some(previous.clone());
    crate::review::update_operator_fixture(&context, |operator| {
        operator["integration"] = json!(owned);
        operator["portable_registration_epoch"] = json!(context.project_id);
    })
    .unwrap();
    let unrelated =
        "{\"mcpServers\":{\"gate\":{\"type\":\"http\",\"url\":\"https://gate.example.com/mcp\"}}}";
    fs::write(root.path().join(".mcp.json"), unrelated).unwrap();
    let checkpoint = fs::read(root.path().join(".shield/review/state.json")).unwrap();
    let copied = assert_fs::TempDir::new().unwrap();
    crate::test_support::copy_fixture_directory(root.path(), copied.path());
    let child = crate::review::open(copied.path()).unwrap();
    let current = registration(&meta(&child)).unwrap().unwrap();
    assert!(current.box_definition.is_some());
    assert!(current.covers("codex"));
    assert!(current.covers("claude"));
    assert_eq!(definition(copied.path()).unwrap(), previous.shared);
    let executable = std::env::current_exe().unwrap().canonicalize().unwrap();
    assert_eq!(current.binary, executable);
    let pinned: Value = crate::policy::parse(&local(copied.path()).unwrap()).unwrap();
    assert_eq!(
        pinned["connection"]["command"],
        executable.to_str().unwrap()
    );
    assert_eq!(pinned["connection"]["args"][3], ".");
    assert_eq!(
        fs::read_to_string(copied.path().join(".mcp.json")).unwrap(),
        unrelated
    );
    assert!(!copied.path().join(".codex/config.toml").exists());
    assert!(!copied.path().join(".claude/settings.local.json").exists());
    assert_eq!(
        fs::read(root.path().join(".shield/review/state.json")).unwrap(),
        checkpoint
    );
    assert_eq!(local(root.path()).unwrap(), previous.local);

    // An edited local definition is its publisher's review boundary.
    let edited = assert_fs::TempDir::new().unwrap();
    crate::test_support::copy_fixture_directory(root.path(), edited.path());
    let text = "schema_version: 1\nconnection:\n  command: acme-user-connector\n";
    fs::write(edited.path().join(box_definition::LOCAL_PATH), text).unwrap();
    crate::review::open(edited.path()).unwrap();
    assert_eq!(local(edited.path()).unwrap(), text);
    assert_eq!(definition(edited.path()).unwrap(), previous.shared);
}

#[test]
fn box_portability_missing_pin_retries_and_mixed_registration_can_repin_again() {
    let (root, context) = fixture();
    box_project(root.path());
    setup(&context, choice(&["codex"])).unwrap();
    let before = meta(&context);
    let plan = crate::review::integrations::setup::prepare_approving(
        root.path(),
        &before,
        choice(&["box"]),
        false,
        None,
        false,
    )
    .unwrap()
    .unwrap();
    apply(&context, &before, &plan).unwrap();
    let mut owned = registration(&meta(&context)).unwrap().unwrap();
    assert!(owned.agents.contains_key("codex"));
    owned.binary = Path::new("/acme/old-install/shield").to_path_buf();
    let definition =
        box_definition::render(&owned.binary, &crate::review::mcp::default_approved()).unwrap();
    fs::write(root.path().join(box_definition::PATH), &definition.shared).unwrap();
    fs::write(
        root.path().join(box_definition::LOCAL_PATH),
        &definition.local,
    )
    .unwrap();
    owned.box_definition = Some(definition.clone());
    crate::review::update_operator_fixture(&context, |operator| {
        operator["integration"] = json!(owned);
        operator["portable_registration_epoch"] = json!(context.project_id);
    })
    .unwrap();
    let copied = assert_fs::TempDir::new().unwrap();
    crate::test_support::copy_fixture_directory(root.path(), copied.path());
    fs::remove_file(copied.path().join(box_definition::LOCAL_PATH)).unwrap();
    let child = crate::review::open(copied.path()).unwrap();
    assert!(local(copied.path()).is_none());
    assert_ne!(
        meta(&child)["portable_registration_epoch"],
        child.project_id
    );
    fs::write(
        copied.path().join(box_definition::LOCAL_PATH),
        &definition.local,
    )
    .unwrap();
    crate::review::open(copied.path()).unwrap();
    let executable = std::env::current_exe().unwrap().canonicalize().unwrap();
    let current = registration(&meta(&child)).unwrap().unwrap();
    assert!(
        current
            .box_definition
            .as_ref()
            .unwrap()
            .pinned_to(&executable)
    );
    assert_eq!(current.binary, owned.binary);
    let state = crate::review::project::load(&child).unwrap();
    // A later same-content executable relocation must recognize the Box pin
    // independently of the historical direct-registration executable path.
    let next = crate::review::integrations::portable::prepare_box(
        copied.path(),
        &state,
        Path::new("/acme/next-install/shield"),
    )
    .unwrap()
    .unwrap();
    assert_eq!(next.files.len(), 1);
    assert_eq!(next.files[0].path, box_definition::LOCAL_PATH);
    assert!(
        next.files[0]
            .after
            .as_ref()
            .unwrap()
            .contains("/acme/next-install/shield")
    );
}

#[test]
fn box_is_listed_first_on_every_agent_page() {
    let rows = workflow::rows("Acme box", "Acme agent");
    assert_eq!(rows[0].0, "box");
    assert_eq!(rows[0].1, "Box");
    let rest: Vec<_> = rows[1..].iter().map(|row| row.0).collect();
    let adapters: Vec<_> = agents::ADAPTERS.iter().map(|a| a.spec().id).collect();
    assert_eq!(rest, adapters);
}

#[test]
fn box_setup_refuses_projects_without_box_configuration() {
    let (root, context) = fixture();
    let refused = prepare(root.path(), &meta(&context), choice(&["box"]))
        .err()
        .expect("refusal")
        .to_string();
    assert!(refused.contains("box config init"), "{refused}");
    assert!(!root.path().join(".box").exists());
    assert!(meta(&context)["integration"].is_null());
}

/// An earlier setup left the exact private pin readable by others, which Box
/// refuses. Status reports it and setup repairs the mode without rewriting it.
#[test]
fn box_setup_restricts_an_unchanged_private_pin_left_readable() {
    use std::os::unix::fs::PermissionsExt;
    let (root, context) = fixture();
    box_project(root.path());
    setup(&context, choice(&["box"])).unwrap();
    let pin = root.path().join(box_definition::LOCAL_PATH);
    let text = local(root.path()).unwrap();
    fs::set_permissions(&pin, fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(coverage(&context)["box_integration"]["status"], "changed");
    setup(&context, choice(&["box"])).unwrap();
    let mode = fs::metadata(&pin).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o600, "{mode:o}");
    assert_eq!(local(root.path()).unwrap(), text);
    assert_eq!(coverage(&context)["box_integration"]["status"], "installed");
}

#[test]
fn box_definition_renders_the_hooks_and_server_shield_installs_directly() {
    let (root, context) = fixture();
    box_project(root.path());
    let plan = setup(&context, choice(&["box"])).unwrap();
    let registered = plan.desired.as_ref().unwrap();
    assert!(registered.agents.is_empty());
    let text = definition(root.path()).unwrap();
    let pinned = local(root.path()).unwrap();
    let owned = registered.box_definition.as_ref().unwrap();
    assert_eq!((&owned.shared, &owned.local), (&text, &pinned));
    let written: Value = crate::policy::parse(&text).unwrap();
    let binary = registered.binary.to_str().unwrap();
    let args = json!(["mcp", "serve", "--project", ".", "--agent", "{agent}"]);
    // The shared file is portable: the bare command and no absolute path.
    assert!(!text.contains(binary) && !text.contains(root.path().to_str().unwrap()));
    assert_eq!(written["schema_version"], 1);
    assert_eq!(
        written["connection"],
        json!({"transport":"stdio","command":"shield","args":args})
    );
    // Box's private layer pins this binary with a complete connection only.
    assert_eq!(
        crate::policy::parse::<Value>(&pinned).unwrap(),
        json!({"schema_version":1,"connection":{"transport":"stdio","command":binary,"args":args}})
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for path in [".box/local/mcp", box_definition::LOCAL_PATH] {
            let mode = fs::metadata(root.path().join(path))
                .unwrap()
                .permissions()
                .mode();
            // Box refuses its private layer unless it is owner-only.
            assert_eq!(mode & 0o077, 0, "{path}: {mode:o}");
        }
    }
    let allowed: Vec<_> = crate::review::mcp::default_approved();
    // Every tool has a decision; unapproved tools and policy_apply ask.
    let decisions = written["permissions"]["tools"].as_object().unwrap();
    assert_eq!(written["permissions"]["agents"], json!(["claude", "codex"]));
    assert_eq!(decisions.len(), crate::review::mcp::tool_summaries().len());
    for (tool, _) in crate::review::mcp::tool_summaries() {
        let allow = tool != "policy_apply" && allowed.contains(&tool);
        assert_eq!(
            decisions[&tool],
            if allow { "allow" } else { "ask" },
            "{tool}"
        );
    }
    assert_eq!(decisions["policy_apply"], "ask");
    assert!(decisions.values().any(|decision| decision == "ask"));
    matches_direct_installation(root.path(), &written, &allowed);
    let report = coverage(&context);
    assert_eq!(report["box_integration"]["status"], "installed");
    assert_eq!(report["repair_needed"], false, "{report}");
}

/// The shared definition's expansions reproduce direct installation. The
/// private connection's executable pin is checked separately above.
fn matches_direct_installation(root: &Path, written: &Value, allowed: &[String]) {
    let canonical_root = root.canonicalize().unwrap();
    let root = canonical_root.as_path();
    let binary = Path::new("shield");
    let pinned_word = native::quoted(binary).unwrap();
    let direct = prepare(root, &json!({}), IntegrationChoice::all())
        .unwrap()
        .unwrap();
    let direct = direct.desired.unwrap();
    let hooks = written["hooks"].as_array().unwrap();
    let supported: Vec<_> = agents::ADAPTERS
        .iter()
        .filter(|adapter| adapter.supports_box_hooks())
        .collect();
    assert_eq!(hooks.len(), supported.len());
    for (hook, adapter) in hooks.iter().zip(supported) {
        let spec = adapter.spec();
        assert_eq!(hook["agents"], json!([spec.id]));
        let events = hook["events"].as_array().unwrap();
        assert_eq!(events.len(), spec.events.len());
        for ((event, _), portable) in spec.events.iter().zip(events) {
            assert_eq!(
                portable.as_str().unwrap().replace('_', ""),
                match *event {
                    "BeforeTool" => "pretooluse".into(),
                    "AfterTool" => "posttooluse".into(),
                    _ => event.to_ascii_lowercase(),
                }
            );
            let template = hook["command"].as_str().unwrap();
            assert!(template.starts_with("{command} "), "{template}");
            assert!(!template.contains("{project}"), "{template}");
            // Quoting the literal dot does not change the shell argument;
            // direct hooks quote it while the Box template leaves it bare.
            let command = template
                .replace("{command}", &pinned_word)
                .replace("--project . ", "--project '.' ")
                .replace("{agent}", spec.id)
                .replace("{event}", event);
            assert_eq!(
                command,
                native::hook_command(binary, root, spec.id, event).unwrap()
            );
            let mut handler = json!({"type":"command","command":command,"timeout":hook["timeout"]});
            if spec.id == "gemini" {
                handler["timeout"] = json!(hook["timeout"].as_u64().unwrap() * 1000);
                handler["name"] = hook["name"].clone();
                handler["description"] = hook["description"].clone();
            } else if !hook["status_message"].is_null() {
                handler["statusMessage"] = hook["status_message"].clone();
            }
            let published = json!({"matcher":hook["matcher"],"hooks":[handler]});
            assert_eq!(published, direct.agents[spec.id].groups[*event]);
        }
        let expanded: Vec<String> = written["connection"]["args"]
            .as_array()
            .unwrap()
            .iter()
            .map(|arg| match arg.as_str().unwrap() {
                "{agent}" => spec.id.to_owned(),
                other => other.to_owned(),
            })
            .collect();
        let entry = adapter.mcp_entry(binary, root, allowed);
        let direct_entry = serde_json::from_str::<Value>(&entry).unwrap_or_else(|_| {
            toml_json(entry.parse::<toml_edit::DocumentMut>().unwrap().as_item())
        });
        // The shared executable and direct arguments match, with Codex's
        // prompt default and every tool's approval in the server entry.
        let mut projected = json!({"command":"shield","args":expanded});
        match spec.id {
            "claude" => projected["type"] = json!("stdio"),
            "codex" => {
                projected["default_tools_approval_mode"] = json!("prompt");
                projected["tools"] = written["permissions"]["tools"]
                    .as_object()
                    .unwrap()
                    .iter()
                    .map(|(tool, decision)| {
                        let mode = if decision == "allow" {
                            "approve"
                        } else {
                            "prompt"
                        };
                        (tool.clone(), json!({"approval_mode":mode}))
                    })
                    .collect();
            }
            "gemini" | "agy" => {}
            other => panic!("no Box projection modelled for {other}"),
        }
        assert_eq!(direct_entry, projected, "{}", spec.id);
    }
}

/// A Codex TOML entry as the JSON value Box compares.
fn toml_json(item: &toml_edit::Item) -> Value {
    fn scalar(toml: &toml_edit::Value) -> Value {
        match toml {
            toml_edit::Value::String(text) => json!(text.value()),
            toml_edit::Value::Array(items) => items.iter().map(scalar).collect(),
            toml_edit::Value::InlineTable(table) => table
                .iter()
                .map(|(key, item)| (key.to_owned(), scalar(item)))
                .collect(),
            other => panic!("unexpected TOML value {other}"),
        }
    }
    match item {
        toml_edit::Item::Value(item) => scalar(item),
        toml_edit::Item::Table(table) => table
            .iter()
            .map(|(key, item)| (key.to_owned(), toml_json(item)))
            .collect(),
        other => panic!("unexpected TOML item {other:?}"),
    }
}

#[test]
fn reset_tool_permissions_previews_current_defaults_and_preserves_saved_choices() {
    for saved in [vec![], vec!["status".to_owned()]] {
        let (root, context) = fixture();
        box_project(root.path());
        let before = meta(&context);
        let plan = prepare_approving(
            root.path(),
            &before,
            choice(&["box"]),
            false,
            Some(saved.clone()),
            true,
        )
        .unwrap()
        .unwrap();
        apply(&context, &before, &plan).unwrap();
        let saved_meta = meta(&context);
        let saved_definition = definition(root.path()).unwrap();

        let kept = workflow::review(&context, Some(choice(&["box"])), true, false).unwrap();
        assert_eq!(kept["tool_approvals"]["approved"], json!(saved));
        let reset = workflow::review(&context, Some(choice(&["box"])), true, true).unwrap();
        assert_eq!(reset["tool_approvals"]["ask"], json!(["policy_apply"]));
        let file = reset["files"]
            .as_array()
            .unwrap()
            .iter()
            .find(|file| file["path"] == box_definition::PATH)
            .unwrap();
        let projected: Value = crate::policy::parse(file["after"].as_str().unwrap()).unwrap();
        for (tool, _) in crate::review::mcp::tool_summaries() {
            assert_eq!(
                projected["permissions"]["tools"][&tool],
                if tool == "policy_apply" {
                    "ask"
                } else {
                    "allow"
                },
                "{tool}"
            );
        }
        assert_eq!(meta(&context), saved_meta);
        assert_eq!(definition(root.path()).unwrap(), saved_definition);
    }
}

#[test]
fn box_dry_run_reviews_the_definition_without_writing() {
    let (root, context) = fixture();
    box_project(root.path());
    let review = workflow::review(&context, Some(choice(&["box", "claude"])), true, false).unwrap();
    assert_eq!(review["dry_run"], true);
    assert_eq!(review["agents"][0]["agent"], "box");
    assert_eq!(review["agents"][0]["change"], "added");
    let file = review["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["path"] == box_definition::PATH)
        .unwrap();
    assert!(file["before"].is_null() && file["after"].is_string());
    assert!(
        review["files"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["path"] == box_definition::LOCAL_PATH)
    );
    let text = super::super::presentation::review(&review);
    assert!(text.contains("box config sync"), "{text}");
    assert!(text.contains("box config sync --adopt"), "{text}");
    assert!(text.contains("Box MCP definition"), "{text}");
    assert!(text.contains("Box local Shield path"), "{text}");
    assert!(definition(root.path()).is_none());
    assert!(!root.path().join(".box/mcp").exists());
    assert!(!root.path().join(".box/local").exists());
    assert!(!root.path().join(".claude").exists());
    assert!(meta(&context)["integration"].is_null());
}

#[test]
fn box_removal_deletes_only_the_definition_shield_wrote() {
    let (root, context) = fixture();
    box_project(root.path());
    for folder in [".box/mcp", ".box/local/mcp"] {
        fs::create_dir_all(root.path().join(folder)).unwrap();
        fs::write(
            root.path().join(folder).join("acme.yml"),
            "schema_version: 1\n",
        )
        .unwrap();
    }
    setup(&context, choice(&["box", "claude"])).unwrap();
    let claude = root.path().join(".claude/settings.local.json");
    let hooks = fs::read(&claude).unwrap();
    setup(&context, removal(&["box"])).unwrap();
    assert!(definition(root.path()).is_none() && local(root.path()).is_none());
    assert_eq!(fs::read(&claude).unwrap(), hooks);
    assert!(root.path().join(".box/config.yml").is_file());
    assert!(root.path().join(".box/mcp/acme.yml").is_file());
    assert!(root.path().join(".box/local/mcp/acme.yml").is_file());
    let registered = registration(&meta(&context)).unwrap().unwrap();
    assert!(registered.box_definition.is_none());
    assert!(registered.agents.contains_key("claude"));

    // An edited file of the pair is never deleted, nor is its partner.
    // Selecting Claude again keeps its direct installation.
    setup(&context, choice(&["box", "claude"])).unwrap();
    let edited = "schema_version: 1\n# Acme edit\n";
    fs::write(root.path().join(box_definition::LOCAL_PATH), edited).unwrap();
    let shared = definition(root.path()).unwrap();
    let refused = setup(&context, removal(&["box"]))
        .err()
        .expect("refusal")
        .to_string();
    assert!(refused.contains("edited"), "{refused}");
    assert_eq!(local(root.path()).as_deref(), Some(edited));
    assert_eq!(definition(root.path()), Some(shared));
    // Once the operator deletes the edited file, removal deletes only the
    // unchanged one.
    fs::remove_file(root.path().join(box_definition::LOCAL_PATH)).unwrap();
    let forgotten = setup(&context, removal(&["box"])).unwrap();
    assert_eq!(
        forgotten
            .files
            .iter()
            .map(|f| f.path.as_str())
            .collect::<Vec<_>>(),
        [box_definition::PATH]
    );
    assert!(definition(root.path()).is_none());
    let registered = registration(&meta(&context)).unwrap().unwrap();
    assert!(registered.box_definition.is_none());
    setup(&context, IntegrationChoice::remove_all()).unwrap();
    assert!(meta(&context)["integration"].is_null());
}

#[test]
fn edited_or_deleted_definitions_are_reported_and_repaired() {
    let (root, context) = fixture();
    box_project(root.path());
    setup(&context, choice(&["box"])).unwrap();
    let owned = (
        definition(root.path()).unwrap(),
        local(root.path()).unwrap(),
    );
    let recorded = || {
        registration(&meta(&context))
            .unwrap()
            .unwrap()
            .box_definition
            .unwrap()
    };
    let pair = recorded();
    assert_eq!((pair.shared, pair.local), owned.clone());
    for (path, damage) in [
        (
            box_definition::PATH,
            Some("schema_version: 1\n# Acme edit\n"),
        ),
        (box_definition::PATH, None),
        (
            box_definition::LOCAL_PATH,
            Some("schema_version: 1\n# Acme edit\n"),
        ),
        (box_definition::LOCAL_PATH, None),
    ] {
        let path = root.path().join(path);
        match damage {
            Some(text) => fs::write(&path, text).unwrap(),
            None => fs::remove_file(&path).unwrap(),
        }
        let report = coverage(&context);
        let expected = if damage.is_some() {
            "changed"
        } else {
            "missing"
        };
        assert_eq!(report["box_integration"]["status"], expected);
        assert_eq!(report["repair_needed"], true);
        let route = crate::review::project::status(&context).unwrap()["repair_route"].clone();
        assert_eq!(route, "shield mcp setup --agent box");
        // Setting up another agent keeps the damaged definition as it is.
        let other = prepare(root.path(), &meta(&context), choice(&["claude"]))
            .unwrap()
            .unwrap();
        assert!(other.files.iter().all(|f| !f.path.starts_with(".box/")));
        assert!(other.desired.unwrap().box_definition == Some(recorded()));
        setup(&context, choice(&["box"])).unwrap();
        assert_eq!(
            (
                definition(root.path()).unwrap(),
                local(root.path()).unwrap()
            ),
            owned
        );
        assert_eq!(coverage(&context)["box_integration"]["status"], "installed");
    }
}

#[test]
fn box_setup_never_replaces_a_definition_shield_did_not_write() {
    let (root, context) = fixture();
    box_project(root.path());
    fs::create_dir_all(root.path().join(".box/mcp")).unwrap();
    let foreign = "schema_version: 1\n# Acme server\n";
    fs::write(root.path().join(box_definition::PATH), foreign).unwrap();
    assert!(prepare(root.path(), &meta(&context), choice(&["box"])).is_err());
    assert_eq!(definition(root.path()).as_deref(), Some(foreign));
    // A foreign private pin refuses too, and identical files are adopted.
    fs::remove_file(root.path().join(box_definition::PATH)).unwrap();
    fs::create_dir_all(root.path().join(".box/local/mcp")).unwrap();
    fs::write(root.path().join(box_definition::LOCAL_PATH), foreign).unwrap();
    assert!(prepare(root.path(), &meta(&context), choice(&["box"])).is_err());
    assert_eq!(local(root.path()).as_deref(), Some(foreign));
    let expected = prepare(root.path(), &json!({}), choice(&["claude"]))
        .unwrap()
        .unwrap()
        .desired
        .unwrap()
        .binary;
    let rendered =
        box_definition::render(&expected, &crate::review::mcp::default_approved()).unwrap();
    fs::write(root.path().join(box_definition::PATH), &rendered.shared).unwrap();
    fs::write(
        root.path().join(box_definition::LOCAL_PATH),
        &rendered.local,
    )
    .unwrap();
    let adopted = setup(&context, choice(&["box"])).unwrap();
    assert!(adopted.files.iter().all(|f| f.before == f.after));
}

#[test]
fn box_definition_files_are_protected_integration_files() {
    for path in [box_definition::PATH, box_definition::LOCAL_PATH] {
        assert!(crate::review::integrations::protected_files().contains(&path));
    }
}

#[test]
fn box_carries_the_codex_ask_fallback_used_at_hook_time() {
    let (root, context) = fixture();
    box_project(root.path());
    // Without a confirmed choice, a Box-only Codex blocks Ask requests.
    setup(&context, choice(&["box"])).unwrap();
    assert!(matches!(
        fallback(&meta(&context), "codex").unwrap(),
        AskFallback::Block
    ));
    // Setup offers Box the same Allow default it offers a direct Codex.
    let review = workflow::review(&context, Some(choice(&["box"])), true, false).unwrap();
    assert_eq!(review["ask_fallbacks"]["codex"]["after"], "allow");
    let text = super::super::presentation::review(&review);
    assert!(
        text.contains("(Codex · Ask fallback allows requests)"),
        "{text}"
    );
    let before = meta(&context);
    let mut plan = prepare(root.path(), &before, choice(&["box"]))
        .unwrap()
        .unwrap();
    crate::review::integrations::setup::with_fallbacks(&before, choice(&["box"]), Some(&mut plan))
        .unwrap();
    apply(&context, &before, &plan).unwrap();
    let registered = registration(&meta(&context)).unwrap().unwrap();
    assert!(registered.agents.is_empty());
    assert!(matches!(
        fallback(&meta(&context), "codex").unwrap(),
        AskFallback::Allow
    ));
    assert_eq!(
        coverage(&context)["box_integration"]["ask_fallback"]["codex"],
        "allow"
    );
    // Fallbacks remain limited to agents that need one.
    assert!(!registered.ask_fallback.contains_key("claude"));
    // Removing Box drops the fallback it carried.
    setup(&context, removal(&["box"])).unwrap();
    assert!(matches!(
        fallback(&meta(&context), "codex").unwrap(),
        AskFallback::Block
    ));
}

#[test]
fn box_install_hands_direct_installations_over_without_touching_their_files() {
    let (root, context) = fixture();
    box_project(root.path());
    setup(&context, choice(&["claude", "codex"])).unwrap();
    let native = [
        ".mcp.json",
        ".claude/settings.local.json",
        ".codex/hooks.json",
        ".codex/config.toml",
    ];
    let snapshot = || {
        native
            .iter()
            .map(|path| fs::read(root.path().join(path)).unwrap())
            .collect::<Vec<_>>()
    };
    let before = snapshot();
    // Declining the hand-over keeps Shield managing the direct installations.
    let kept = prepare_with(
        root.path(),
        &meta(&context),
        choice(&["box"]),
        false,
        None,
        false,
        false,
    )
    .unwrap()
    .unwrap();
    assert_eq!(kept.handed_over, Vec::<String>::new());
    assert_eq!(kept.desired.as_ref().unwrap().agents.len(), 2);
    // The default hand-over is part of the reviewed plan.
    let review = workflow::review(&context, Some(choice(&["box"])), true, false).unwrap();
    assert_eq!(review["box"]["handed_over"], json!(["claude", "codex"]));
    assert_eq!(review["agents"][1]["change"], "handed over");
    let text = super::super::presentation::review(&review);
    assert!(text.contains("box config sync --adopt"), "{text}");
    let plan = setup(&context, choice(&["box"])).unwrap();
    assert_eq!(plan.handed_over, ["claude", "codex"]);
    assert!(
        plan.files
            .iter()
            .all(|f| [box_definition::PATH, box_definition::LOCAL_PATH].contains(&f.path.as_str()))
    );
    assert_eq!(snapshot(), before);
    let registered = registration(&meta(&context)).unwrap().unwrap();
    assert!(registered.agents.is_empty());
    assert!(registered.box_definition.is_some());
    let report = coverage(&context);
    assert_eq!(
        report["installed_integrations"],
        json!([
            {"agent":"claude","surface":"native-hooks","managed_by":"box","status":"installed","ask_fallback":null},
            {"agent":"codex","surface":"native-hooks","managed_by":"box","status":"installed","ask_fallback":null}
        ])
    );
    assert_eq!(report["repair_needed"], false, "{report}");
    // Plain removal of a direct installation still deletes its entries.
    let (root, context) = fixture();
    setup(&context, choice(&["claude"])).unwrap();
    setup(&context, removal(&["claude"])).unwrap();
    let settings =
        fs::read_to_string(root.path().join(".claude/settings.local.json")).unwrap_or_default();
    assert!(!settings.contains("native-hook"), "{settings}");
}

#[test]
fn google_adapters_hand_over_to_box_with_native_hooks() {
    let (root, context) = fixture();
    box_project(root.path());
    let direct = setup(&context, IntegrationChoice::all())
        .unwrap()
        .desired
        .unwrap();
    let retained: Vec<_> = direct
        .agents
        .keys()
        .filter(|id| !agents::find(id).is_some_and(agents::AgentAdapter::supports_box_hooks))
        .map(String::as_str)
        .collect();
    let before = fs::read(root.path().join(".gemini/settings.json")).unwrap();
    let plan = setup(&context, choice(&["box"])).unwrap();
    assert_eq!(plan.handed_over, ["agy", "claude", "codex", "gemini"]);
    let registered = registration(&meta(&context)).unwrap().unwrap();
    assert_eq!(
        registered
            .agents
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        retained
    );
    assert_eq!(
        fs::read(root.path().join(".gemini/settings.json")).unwrap(),
        before
    );
    let config: Value = crate::policy::parse(&definition(root.path()).unwrap()).unwrap();
    assert_eq!(config["hooks"].as_array().unwrap().len(), 4);
    let (root, context) = fixture();
    box_project(root.path());
    setup(&context, choice(&["box"])).unwrap();
    let registered = registration(&meta(&context)).unwrap().unwrap();
    assert!(registered.covers("gemini"));
    assert!(registered.covers("agy"));
    assert!(registered.covers("claude"));
}

#[test]
fn guided_setup_preselects_only_box_in_a_box_project() {
    let (root, context) = fixture();
    assert!(
        !workflow::preselected(root.path(), &meta(&context))
            .unwrap()
            .contains(&"box".to_owned())
    );
    box_project(root.path());
    assert_eq!(
        workflow::preselected(root.path(), &meta(&context)).unwrap(),
        ["box"]
    );
    // Recorded integrations still take precedence.
    setup(&context, choice(&["claude"])).unwrap();
    assert_eq!(
        workflow::preselected(root.path(), &meta(&context)).unwrap(),
        ["claude"]
    );
}

#[test]
fn guided_setup_rows_report_registration_and_box_availability() {
    let (root, context) = fixture();
    let choices = workflow::setup_choices(root.path(), &meta(&context), false).unwrap();
    assert!(
        choices[0]
            .disabled
            .as_deref()
            .unwrap()
            .contains("Box project")
    );
    assert!(
        choices
            .iter()
            .find(|row| row.id == "claude")
            .unwrap()
            .detail
            .starts_with("not registered")
    );
    // Removal stays available even after the Box marker is gone.
    assert!(
        workflow::setup_choices(root.path(), &meta(&context), true).unwrap()[0]
            .disabled
            .is_none()
    );
    box_project(root.path());
    setup(&context, choice(&["claude"])).unwrap();
    let choices = workflow::setup_choices(root.path(), &meta(&context), false).unwrap();
    assert!(choices[0].disabled.is_none());
    assert!(
        choices
            .iter()
            .find(|row| row.id == "claude")
            .unwrap()
            .detail
            .starts_with("configured; ownership checked on review")
    );
}

#[test]
fn codex_covered_through_box_keeps_its_prefix_rules() {
    let (root, context) = fixture();
    box_project(root.path());
    setup(&context, choice(&["box"])).unwrap();
    // Box has no form for Codex's prefix rules, so Shield writes them itself
    // whenever the active policy has some.
    allow_acme_logs(&context);
    let text = fs::read_to_string(root.path().join(PREFIX_RULES)).unwrap();
    assert!(text.contains("acme"), "{text}");
}

const PREFIX_RULES: &str = ".codex/rules/shield.rules";

/// Activate a policy giving Codex the `acme logs` prefix.
fn allow_acme_logs(context: &Context) {
    let state = crate::review::project::load(context).unwrap();
    let rules = json!({"schema_version":1,"kind":"custom",
        "capabilities":["agent-outputs-v1","argument-rules-v1","structured-rules-v1"],
        "rules":[{"id":"logs","decision":"allow","agents":{"codex":{"prefixes":[["acme","logs"]]}},
            "conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"prefix","values":["logs"]}}}]})
    .to_string();
    let candidate =
        crate::review::packs::rebuild(&state.candidates["project"], &mut [], &[], Some(&rules))
            .unwrap();
    crate::review::publication::commit(
        context,
        &state,
        candidate,
        crate::review::project::metadata(&state),
        None,
    )
    .unwrap();
}

/// The prefix rules file's (before, after) in a setup review.
fn previewed_prefix_rules(review: &Value) -> (Value, Value) {
    let file = review["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["path"] == PREFIX_RULES)
        .unwrap_or_else(|| panic!("{review}"));
    (file["before"].clone(), file["after"].clone())
}

#[test]
fn setup_previews_codex_prefix_rules_box_writes_or_keeps() {
    // A first Box setup previews the prefix rules file it then creates.
    let (root, context) = fixture();
    box_project(root.path());
    allow_acme_logs(&context);
    assert!(!root.path().join(PREFIX_RULES).exists());
    let review = workflow::review(&context, Some(choice(&["box"])), true, false).unwrap();
    let (before, after) = previewed_prefix_rules(&review);
    assert!(before.is_null());
    assert!(after.as_str().unwrap().contains("acme"), "{after}");
    assert_eq!(review["codex_prefixes"].as_array().map(Vec::len), Some(1));
    setup_and_publish_prefix_rules(&context);
    assert_eq!(
        fs::read_to_string(root.path().join(PREFIX_RULES)).unwrap(),
        after.as_str().unwrap()
    );

    // Handing a direct Codex installation over to Box keeps the file, and
    // the preview says so instead of reporting its deletion.
    let (root, context) = fixture();
    box_project(root.path());
    setup(&context, choice(&["codex"])).unwrap();
    allow_acme_logs(&context);
    let existing = fs::read_to_string(root.path().join(PREFIX_RULES)).unwrap();
    let review = workflow::review(&context, Some(choice(&["box"])), true, false).unwrap();
    assert_eq!(review["box"]["handed_over"], json!(["codex"]));
    assert_eq!(
        previewed_prefix_rules(&review),
        (json!(existing), json!(existing))
    );
    setup_and_publish_prefix_rules(&context);
    assert_eq!(
        fs::read_to_string(root.path().join(PREFIX_RULES)).unwrap(),
        existing
    );
}

/// Set Box up, then bring prefix rules in line as a confirmed setup does.
fn setup_and_publish_prefix_rules(context: &Context) {
    setup(context, choice(&["box"])).unwrap();
    let root = Path::new(&context.project_root);
    prefix_rules::sync(
        root,
        &meta(context),
        &prefix_rules::active(root).unwrap(),
        crate::review::mode::current(root).unwrap(),
    )
    .unwrap();
}
