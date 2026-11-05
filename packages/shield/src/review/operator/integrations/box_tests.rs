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
    let args = json!([
        "mcp",
        "serve",
        "--project",
        "{project}",
        "--agent",
        "{agent}"
    ]);
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
        let mode = fs::metadata(root.path().join(".box/local/mcp"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o077, 0, "{mode:o}");
    }
    let allowed: Vec<_> = crate::review::mcp::default_approved();
    // Every tool has a decision; unapproved tools and policy_apply ask.
    let decisions = written["permissions"]["tools"].as_object().unwrap();
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
    matches_direct_installation(root.path(), registered, &written, &allowed);
    let report = coverage(&context);
    assert_eq!(report["box_integration"]["status"], "installed");
    assert_eq!(report["repair_needed"], false, "{report}");
}

/// Box's expansions must reproduce the groups and MCP arguments a direct
/// installation writes, so Box can adopt existing entries.
fn matches_direct_installation(
    root: &Path,
    registered: &crate::review::integrations::Registration,
    written: &Value,
    allowed: &[String],
) {
    let pinned_word = native::quoted(&registered.binary).unwrap();
    let direct = prepare(root, &json!({}), IntegrationChoice::all())
        .unwrap()
        .unwrap();
    let direct = direct.desired.unwrap();
    let project = native::quoted(root).unwrap();
    let hooks = written["hooks"].as_array().unwrap();
    assert_eq!(hooks.len(), agents::ADAPTERS.len());
    for (hook, adapter) in hooks.iter().zip(agents::ADAPTERS) {
        let spec = adapter.spec();
        assert_eq!(hook["agents"], json!([spec.id]));
        let events = hook["events"].as_array().unwrap();
        assert_eq!(events.len(), spec.events.len());
        for ((event, _), portable) in spec.events.iter().zip(events) {
            assert_eq!(
                portable.as_str().unwrap().replace('_', ""),
                event.to_ascii_lowercase()
            );
            let template = hook["command"].as_str().unwrap();
            assert!(template.starts_with("{command} "), "{template}");
            let command = template
                .replace("{command}", &pinned_word)
                .replace("{project}", &project)
                .replace("{agent}", spec.id)
                .replace("{event}", event);
            assert_eq!(
                command,
                native::hook_command(&registered.binary, root, spec.id, event).unwrap()
            );
            let published = json!({"matcher":hook["matcher"],"hooks":[{"type":"command",
                "command":command,"timeout":hook["timeout"],"statusMessage":hook["status_message"]}]});
            assert_eq!(published, direct.agents[spec.id].groups[*event]);
        }
        let expanded: Vec<String> = written["connection"]["args"]
            .as_array()
            .unwrap()
            .iter()
            .map(|arg| match arg.as_str().unwrap() {
                "{project}" => root.to_str().unwrap().to_owned(),
                "{agent}" => spec.id.to_owned(),
                other => other.to_owned(),
            })
            .collect();
        let entry = adapter.mcp_entry(&registered.binary, root, allowed);
        let direct_entry = serde_json::from_str::<Value>(&entry).unwrap_or_else(|_| {
            toml_json(entry.parse::<toml_edit::DocumentMut>().unwrap().as_item())
        });
        // Box adopts an unowned entry only when its whole projection is equal:
        // the pinned binary with the direct arguments and, for Codex, a prompt
        // default with every tool's approval in the server entry.
        let mut projected = json!({"command":registered.binary.to_str().unwrap(),"args":expanded});
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
fn box_dry_run_reviews_the_definition_without_writing() {
    let (root, context) = fixture();
    box_project(root.path());
    let review = workflow::review(&context, Some(choice(&["box", "claude"])), true).unwrap();
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
    let review = workflow::review(&context, Some(choice(&["box"])), true).unwrap();
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
    workflow::with_fallbacks(&before, choice(&["box"]), Some(&mut plan)).unwrap();
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
    setup(&context, IntegrationChoice::all()).unwrap();
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
    )
    .unwrap()
    .unwrap();
    assert_eq!(kept.handed_over, Vec::<String>::new());
    assert_eq!(kept.desired.as_ref().unwrap().agents.len(), 2);
    // The default hand-over is part of the reviewed plan.
    let review = workflow::review(&context, Some(choice(&["box"])), true).unwrap();
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
    assert_eq!(report["installed_integrations"], json!([]));
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
    let review = workflow::review(&context, Some(choice(&["box"])), true).unwrap();
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
    let review = workflow::review(&context, Some(choice(&["box"])), true).unwrap();
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
    prefix_rules::sync(root, &meta(context), &prefix_rules::active(root).unwrap()).unwrap();
}
