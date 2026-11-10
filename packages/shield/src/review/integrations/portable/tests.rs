//! Same-binary maintenance fixtures; no host or platform acceptance inferred.
use super::*;
use crate::review::{
    Context,
    integrations::{AskFallback, IntegrationChoice, Registration},
    storage::Store,
};
use serde_json::{Value, json};

fn state(context: &Context) -> State {
    Store::inspect(Path::new(&context.project_root))
        .expect("portable fixture inspection")
        .read_diagnostic_checkpoint()
        .expect("portable fixture checkpoint")
        .0
}

#[test]
fn portable_pending_maintenance_preserves_files_when_executable_digest_changes() {
    let (root, context, _) = historical("codex");
    let before = state(&context);
    let mut plan = prepare(root.path(), &before, "codex").unwrap().unwrap();
    plan.previous.as_mut().unwrap().binary_digest = "f".repeat(64);
    plan.desired.as_mut().unwrap().binary_digest = "f".repeat(64);
    let path = root.path().join(".codex/config.toml");
    let bytes = std::fs::read(&path).unwrap();
    Store::open(root.path())
        .unwrap()
        .update_checkpoint(&context, &|| Ok(()), |state| {
            state.operator["integration"] = json!(plan.previous);
            super::super::activation::stage(&mut state.operator, &plan, None)?;
            let activation = state.active.as_ref().unwrap().activation_id().to_owned();
            let mut metadata = state.operator.clone();
            super::super::activation::bind(
                state,
                &mut metadata,
                &json!({"activation_id":activation}),
            );
            Ok(Value::Null)
        })
        .unwrap();
    let before = state(&context);
    let checkpoint = std::fs::read(root.path().join(".shield/review/state.json")).unwrap();
    super::super::rebind_launchers(&context, &before);
    assert!(matches!(
        super::super::setup::resume_confirmed(&context),
        Err(Error::Invalid(
            "automatic maintenance executable changed; review setup"
        ))
    ));
    assert_eq!(std::fs::read(path).unwrap(), bytes);
    assert_eq!(
        std::fs::read(root.path().join(".shield/review/state.json")).unwrap(),
        checkpoint
    );
    assert_eq!(state(&context).operator["integration_pending"], json!(plan));
}

fn historical(agent: &str) -> (assert_fs::TempDir, Context, Value) {
    let root = assert_fs::TempDir::new().expect("historical project directory");
    let bundle = crate::configuration::Bundle::capture("schema_version: 1\npacks: []\n", &[], None)
        .expect("historical policy");
    let context = crate::review::mcp::initialize_fixture(root.path(), &bundle);
    crate::review::operator::integrations::configure_fixture(
        root.path(),
        IntegrationChoice::agent(agent),
        AskFallback::Block,
    );
    let before = state(&context);
    let mut owned = registration(&before.operator)
        .expect("registration codec")
        .expect("reviewed registration");
    let adapter = super::super::agents::find(agent).expect("fixture adapter");
    let mut installation =
        serde_json::to_value(&owned.agents[agent]).expect("installation encoding");
    let old_root = Path::new("/acme/original-checkout");
    let events: Vec<_> = adapter.historical_hook_profiles().first().map_or_else(
        || {
            adapter
                .spec()
                .events
                .iter()
                .map(|(event, _)| *event)
                .collect()
        },
        |profile| profile.to_vec(),
    );
    let portable = installation["groups"].clone();
    let groups: serde_json::Map<_, _> = events
        .iter()
        .map(|event| {
            let command = super::super::native::posix_hook_command(
                &super::super::native::quoted(&owned.binary).expect("historical binary quoting"),
                &super::super::native::quoted(old_root).expect("historical root quoting"),
                agent,
                event,
            );
            ((*event).into(), adapter.hook_group(&command))
        })
        .collect();
    let mut source = crate::agents::setup::parse(
        super::super::files::read(root.path(), adapter.spec().hooks_path)
            .expect("installed hook source")
            .as_deref(),
    )
    .expect("installed hook JSON");
    source["hooks"] = json!({});
    for event in &events {
        source["hooks"][*event] = json!([groups[*event], portable[*event],
            {"matcher":"acme","hooks":[{"type":"command","command":"acme-audit"}]}]);
    }
    installation["groups"] = json!(groups);
    historical_mcp(
        root.path(),
        adapter,
        &mut owned,
        &mut installation,
        &mut source,
        old_root,
    );
    std::fs::write(
        root.path().join(adapter.spec().hooks_path),
        source.to_string(),
    )
    .expect("historical hooks");
    owned.agents.insert(
        agent.into(),
        serde_json::from_value(installation).expect("historical installation codec"),
    );
    crate::review::update_operator_fixture(&context, |operator| {
        operator["integration"] = json!(owned);
        operator["portable_launch_roots"] = json!([old_root]);
        operator
            .as_object_mut()
            .expect("operator metadata object")
            .remove("portable_registration_epoch");
    })
    .expect("historical registration commit");
    (root, context, portable)
}

fn historical_mcp(
    root: &Path,
    adapter: &dyn crate::agents::AgentAdapter,
    owned: &mut Registration,
    installation: &mut Value,
    source: &mut Value,
    old_root: &Path,
) {
    let agent = adapter.spec().id;
    let old_args = [
        "__internal",
        "mcp",
        "--project",
        old_root.to_str().expect("UTF-8 historical root"),
        "--agent",
        agent,
    ];
    if let Some(previous) = installation["mcp"].as_str() {
        let migrated = if agent == "codex" {
            let mut entry: toml_edit::DocumentMut = previous.parse().expect("installed MCP TOML");
            entry["command"] = toml_edit::value(owned.binary.to_str().expect("UTF-8 binary"));
            entry["args"] = toml_edit::value(toml_edit::Array::from_iter(old_args));
            entry.to_string()
        } else {
            let mut entry: Value = serde_json::from_str(previous).expect("installed MCP JSON");
            entry["command"] = json!(owned.binary);
            entry["args"] = json!(old_args);
            entry.to_string()
        };
        let path = adapter.spec().mcp_path.expect("adapter MCP path");
        let current = if path == adapter.spec().hooks_path {
            source.to_string()
        } else {
            super::super::files::read(root, path)
                .expect("installed MCP file")
                .expect("installed MCP source")
        };
        let edited = adapter
            .merge_mcp(Some(&current), Some(previous), Some(&migrated))
            .expect("historical MCP replacement");
        if path == adapter.spec().hooks_path {
            *source = serde_json::from_str(&edited).expect("historical MCP JSON");
        } else {
            std::fs::write(root.join(path), edited).expect("historical MCP file");
        }
        installation["mcp"] = json!(migrated);
    } else {
        owned.entry["command"] = json!(owned.binary);
        owned.entry["args"] = json!([
            "__internal",
            "mcp",
            "--project",
            old_root,
            "--agent",
            "claude-code"
        ]);
        owned.entry["env"] = json!({"ACME_UNCHANGED":"retained"});
        std::fs::write(
            root.join(".mcp.json"),
            json!({"mcpServers":{
            "shield":owned.entry,"acme":{"command":"acme-server","args":[]}}})
            .to_string(),
        )
        .expect("historical shared MCP file");
    }
}

#[test]
fn exact_native_migration_deduplicates_known_callbacks_and_preserves_profile_and_neighbors() {
    for agent in ["claude", "codex", "gemini", "agy"] {
        let (root, context, _) = historical(agent);
        let before = state(&context);
        let previous = registration(&before.operator).unwrap().unwrap();
        let plan = prepare(root.path(), &before, agent).unwrap().unwrap();
        let desired = plan.desired.as_ref().unwrap();
        assert_eq!(desired.approved_tools, previous.approved_tools);
        assert!(desired.ask_fallback == previous.ask_fallback);
        assert_eq!(desired.connections, previous.connections);
        assert_eq!(
            desired.agents[agent].groups.keys().collect::<Vec<_>>(),
            previous.agents[agent].groups.keys().collect::<Vec<_>>()
        );
        let adapter = super::super::agents::find(agent).unwrap();
        let hooks = plan
            .files
            .iter()
            .find(|change| change.path == adapter.spec().hooks_path)
            .unwrap();
        let after = crate::agents::setup::parse(hooks.after.as_deref()).unwrap();
        for event in desired.agents[agent].groups.keys() {
            let callbacks: Vec<_> = after["hooks"][event]
                .as_array()
                .unwrap()
                .iter()
                .flat_map(|group| group["hooks"].as_array().unwrap())
                .collect();
            assert_eq!(
                callbacks
                    .iter()
                    .filter(|callback| crate::agents::setup::shield_callback(callback))
                    .count(),
                1
            );
            assert!(
                callbacks
                    .iter()
                    .any(|callback| callback["command"] == "acme-audit")
            );
        }
        if agent == "claude" {
            let after: Value = serde_json::from_str(&plan.after).unwrap();
            assert_eq!(after["mcpServers"]["acme"]["command"], "acme-server");
            assert_eq!(after["mcpServers"]["shield"]["env"], previous.entry["env"]);
            assert_eq!(after["mcpServers"]["shield"]["args"][3], ".");
        }
    }
}

#[test]
fn unknown_callbacks_and_box_ownership_preserve_files_without_adoption() {
    let (root, context, _) = historical("claude");
    let path = root.path().join(".claude/settings.local.json");
    let mut settings: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    settings["hooks"]["PreToolUse"].as_array_mut().unwrap().push(json!({"matcher":"*","hooks":[{
        "type":"command","command":"unreviewed-shield __internal native-hook --project /foreign --agent claude"}]}));
    let bytes = settings.to_string();
    std::fs::write(&path, &bytes).unwrap();
    assert!(prepare(root.path(), &state(&context), "claude").is_err());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), bytes);
    let mut before = state(&context);
    before.operator["integration"]["box_definition"] =
        json!({"shared":"owned publisher definition","local":"owned private definition"});
    assert!(!needed(&before));
    assert!(prepare(root.path(), &before, "claude").unwrap().is_none());
    assert_eq!(std::fs::read_to_string(path).unwrap(), bytes);
}

#[test]
fn runtime_native_migration_retries_missing_files_without_review_or_policy_change() {
    let (root, context, _) = historical("claude");
    let before = state(&context);
    let path = root.path().join(".claude/settings.local.json");
    let saved = path.with_extension("saved");
    std::fs::rename(&path, &saved).unwrap();
    crate::review::open(root.path()).unwrap();
    let unavailable = state(&context);
    assert_ne!(
        unavailable.operator["portable_registration_epoch"],
        unavailable.project_id
    );
    assert_eq!(unavailable.policy_revision, before.policy_revision);
    assert_eq!(
        unavailable.active.as_ref().unwrap().activation_id(),
        before.active.as_ref().unwrap().activation_id()
    );
    std::fs::rename(&saved, &path).unwrap();
    crate::review::open(root.path()).unwrap();
    let after = state(&context);
    assert_eq!(
        after.operator["portable_registration_epoch"],
        after.project_id
    );
    assert_eq!(after.project_id, before.project_id);
    assert_eq!(after.policy_revision, before.policy_revision);
    assert_eq!(
        after.active.as_ref().unwrap().activation_id(),
        before.active.as_ref().unwrap().activation_id()
    );
    let settings =
        crate::agents::setup::parse(Some(&std::fs::read_to_string(&path).unwrap())).unwrap();
    let callbacks: Vec<_> = settings["hooks"]["PreToolUse"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|group| group["hooks"].as_array().unwrap())
        .collect();
    assert_eq!(
        callbacks
            .iter()
            .filter(|entry| crate::agents::setup::shield_callback(entry))
            .count(),
        1
    );
    let file = std::fs::read(&path).unwrap();
    let checkpoint = std::fs::read(root.path().join(".shield/review/state.json")).unwrap();
    crate::review::open(root.path()).unwrap();
    assert_eq!(std::fs::read(path).unwrap(), file);
    assert_eq!(
        std::fs::read(root.path().join(".shield/review/state.json")).unwrap(),
        checkpoint
    );
}
