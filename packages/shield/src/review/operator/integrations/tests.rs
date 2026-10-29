//! Real file/state acceptance for owned registration and interrupted reconciliation.
use super::*;
use crate::configuration::{Bundle, Evaluator};
use crate::review::path_access::{Change, PathAccess};
use crate::review::{
    integrations::{fallback, publish, status, with_fallback},
    storage::Store,
};
use std::collections::BTreeMap;

#[test]
fn retained_setup_records_require_current_registration_and_repair_fields() {
    let (root, context) = fixture();
    let plan = prepare(root.path(), &meta(&context), IntegrationChoice::all())
        .unwrap()
        .unwrap();
    let desired = plan.desired.as_ref().unwrap();
    assert_eq!(desired.agents.len(), crate::agents::ADAPTERS.len());
    for adapter in crate::agents::ADAPTERS {
        assert!(desired.agents.contains_key(adapter.spec().id));
    }
    let mut incomplete_plan = json!(plan);
    incomplete_plan.as_object_mut().unwrap().remove("files");
    assert!(pending(&json!({"integration_pending":incomplete_plan})).is_err());

    let mut incomplete_registration = json!(plan.desired.unwrap());
    incomplete_registration
        .as_object_mut()
        .unwrap()
        .remove("ask_fallback");
    assert!(registration(&json!({"integration":incomplete_registration})).is_err());
}

#[test]
fn mcp_setup_targeted_removal_keeps_other_agents_hooks_and_mcp() {
    let (root, context) = fixture();
    let before = meta(&context);
    let initial = prepare(root.path(), &before, IntegrationChoice::all())
        .unwrap()
        .unwrap();
    apply(&context, &before, &initial).unwrap();
    let claude_path = root
        .path()
        .join(crate::agents::find("claude").unwrap().spec().hooks_path);
    let claude_before = fs::read(&claude_path).unwrap();
    let codex = crate::agents::ADAPTERS
        .iter()
        .position(|a| a.spec().id == "codex")
        .unwrap();
    let before = meta(&context);
    let plan = prepare(
        root.path(),
        &before,
        IntegrationChoice::RemoveAgents(1 << codex),
    )
    .unwrap()
    .unwrap();
    assert!(plan.desired.as_ref().unwrap().agents.contains_key("claude"));
    assert!(!plan.desired.as_ref().unwrap().agents.contains_key("codex"));
    assert!(
        !plan
            .desired
            .as_ref()
            .unwrap()
            .ask_fallback
            .contains_key("codex")
    );
    apply(&context, &before, &plan).unwrap();
    assert_eq!(fs::read(&claude_path).unwrap(), claude_before);
    assert!(config(root.path())["mcpServers"].get("shield").is_some());
    let before = meta(&context);
    let claude = crate::agents::ADAPTERS
        .iter()
        .position(|a| a.spec().id == "claude")
        .unwrap();
    let plan = prepare(
        root.path(),
        &before,
        IntegrationChoice::RemoveAgents(1 << claude),
    )
    .unwrap()
    .unwrap();
    assert!(plan.desired.is_none());
    apply(&context, &before, &plan).unwrap();
    assert!(config(root.path())["mcpServers"].get("shield").is_none());
}

#[test]
fn initial_skip_does_not_prepare_or_touch_existing_agent_configuration() {
    let root = assert_fs::TempDir::new().unwrap();
    fs::write(
        root.path().join(".mcp.json"),
        "Acme malformed existing JSON",
    )
    .unwrap();
    assert!(
        prepare_initial(root.path(), IntegrationChoice::Keep)
            .unwrap()
            .is_none()
    );
    assert_eq!(
        fs::read_to_string(root.path().join(".mcp.json")).unwrap(),
        "Acme malformed existing JSON"
    );
    assert!(!root.path().join(".shield").exists());
}

#[test]
fn codex_initial_capture_distinguishes_mcp_presence_from_invalid_configuration() {
    let root = assert_fs::TempDir::new().unwrap();
    fs::create_dir(root.path().join(".codex")).unwrap();
    let adapter = agents::find("codex").unwrap();
    let binary = Path::new("/opt/acme/shield");
    let owned = adapter.mcp_entry(binary, root.path(), &crate::review::mcp::default_approved());
    let expected = adapter.merge_mcp(None, None, Some(&owned)).unwrap();
    for (source, present) in [
        (String::new(), Some(false)),
        (
            "theme = 'acme'\n[mcp_servers.other]\ncommand = 'acme'\n".into(),
            Some(false),
        ),
        (expected, Some(true)),
        ("[mcp_servers".into(), None),
        ("mcp_servers = []\n".into(), None),
        ("[mcp_servers]\nshield = 'invalid'\n".into(), None),
    ] {
        fs::write(root.path().join(".codex/config.toml"), &source).unwrap();
        let probe = adapter.has_mcp_entry(&source);
        let captured = native::capture_matching(root.path(), "codex", binary, false);
        if let Some(present) = present {
            assert_eq!(probe.unwrap(), present);
            assert_eq!(captured.unwrap().is_some(), present);
        } else {
            assert!(probe.is_err());
            assert!(captured.is_err());
        }
        assert_eq!(
            fs::read_to_string(root.path().join(".codex/config.toml")).unwrap(),
            source
        );
    }
    // Presence alone never adopts a table that differs from the generated entry.
    let edited = "[mcp_servers.shield]\ncommand = 'acme-edited'\n";
    fs::write(root.path().join(".codex/config.toml"), edited).unwrap();
    assert!(adapter.has_mcp_entry(edited).unwrap());
    assert!(native::capture_matching(root.path(), "codex", binary, false).is_err());
    assert_eq!(
        fs::read_to_string(root.path().join(".codex/config.toml")).unwrap(),
        edited
    );
    assert!(!root.path().join(".shield").exists());
}

#[test]
fn claude_mcp_presence_validates_the_shared_json_shape() {
    let adapter = agents::find("claude").unwrap();
    assert!(!adapter.has_mcp_entry("{}").unwrap());
    assert!(
        !adapter
            .has_mcp_entry(r#"{"mcpServers":{"other":{}}}"#)
            .unwrap()
    );
    assert!(
        adapter
            .has_mcp_entry(r#"{"mcpServers":{"shield":{}}}"#)
            .unwrap()
    );
    for source in ["not JSON", "[]", r#"{"mcpServers":[]}"#] {
        assert!(adapter.has_mcp_entry(source).is_err());
    }
}

#[test]
fn initial_review_reclaims_only_exact_generated_entries_without_writes() {
    let root = assert_fs::TempDir::new().unwrap();
    let original = prepare(root.path(), &json!({}), IntegrationChoice::all())
        .unwrap()
        .unwrap();
    for change in &original.files {
        let path = root.path().join(&change.path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, change.after.as_ref().unwrap()).unwrap();
    }
    fs::write(root.path().join(".mcp.json"), &original.after).unwrap();
    assert!(prepare(root.path(), &json!({}), IntegrationChoice::all()).is_err());
    let captured = prepare_initial(root.path(), IntegrationChoice::all())
        .unwrap()
        .unwrap();
    assert_eq!(captured.previous.as_ref().unwrap().agents.len(), 2);
    assert_eq!(
        fs::read_to_string(root.path().join(".mcp.json")).unwrap(),
        original.after
    );
    assert!(!root.path().join(".shield").exists());
    let path = root.path().join(".claude/settings.local.json");
    let mut changed: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    changed["hooks"]["PreToolUse"][0]["hooks"][0]["command"] = json!("acme edited command");
    let bytes = changed.to_string();
    fs::write(&path, &bytes).unwrap();
    assert!(prepare_initial(root.path(), IntegrationChoice::all()).is_err());
    assert_eq!(fs::read_to_string(path).unwrap(), bytes);
    assert!(!root.path().join(".shield").exists());
}

pub(super) fn fixture() -> (assert_fs::TempDir, Context) {
    let root = assert_fs::TempDir::new().unwrap();
    let candidates = BTreeMap::from([(
        "project".into(),
        json!(Bundle::capture(r#"{"schema_version":1,"packs":[]}"#, &[], None).unwrap()),
    )]);
    let evaluator = Evaluator::new(review::snapshots(&candidates).unwrap()).unwrap();
    Store::initialize_active(
        root.path(),
        candidates,
        evaluator.revision(),
        json!({"sources":{},"default":null,"installed":[],"projection":null}),
    )
    .unwrap();
    let context = review::open(root.path()).unwrap();
    (root, context)
}
fn meta(context: &Context) -> Value {
    crate::review::project::metadata(&crate::review::project::load(context).unwrap())
}

fn cloud_fixture(local: bool) -> (assert_fs::TempDir, Context) {
    let root = assert_fs::TempDir::new().unwrap();
    let connection = json!({"transport":{"type":"http","url":"https://cloud.example.com/mcp"},"agents":{"claude":"acme","codex":"acme"}});
    let config=json!({"schema_version":1,"packs":[],"mcp":if local {json!({})} else {json!({"cloud":connection})}}).to_string();
    let local_config = local
        .then(|| json!({"schema_version":1,"packs":[],"mcp":{"cloud":connection}}).to_string());
    let bundle =
        Bundle::capture_layers(&config, local_config.as_deref(), &[], None, None, &[]).unwrap();
    let context = review::mcp::initialize_fixture(root.path(), &bundle);
    (root, context)
}

#[test]
fn mcp_connections_setup_repair_and_removal_preserve_manual_servers() {
    let (root, context) = cloud_fixture(false);
    fs::write(
        root.path().join(".mcp.json"),
        json!({"mcpServers":{"manual":{"type":"http","url":"https://manual.example.com/mcp"}}})
            .to_string(),
    )
    .unwrap();
    let before = meta(&context);
    let plan = prepare(root.path(), &before, IntegrationChoice::all())
        .unwrap()
        .unwrap();
    assert!(plan.after.contains("cloud.example.com"));
    apply(&context, &before, &plan).unwrap();
    let original = fs::read_to_string(root.path().join(".codex/config.toml")).unwrap();
    assert!(original.contains("[mcp_servers.acme]"));
    for agent in ["claude", "codex"] {
        crate::review::integrations::verify_mcp_connection(
            root.path(),
            agent,
            "acme",
            &crate::mcp_connections::Transport::Http {
                url: "https://cloud.example.com/mcp".into(),
            },
        )
        .unwrap();
    }
    let before = meta(&context);
    let repair = prepare(root.path(), &before, IntegrationChoice::all())
        .unwrap()
        .unwrap();
    apply(&context, &before, &repair).unwrap();
    assert_eq!(
        fs::read_to_string(root.path().join(".codex/config.toml")).unwrap(),
        original
    );
    let before = meta(&context);
    let removal = prepare(root.path(), &before, IntegrationChoice::remove_all())
        .unwrap()
        .unwrap();
    apply(&context, &before, &removal).unwrap();
    assert!(config(root.path())["mcpServers"].get("acme").is_none());
    assert_eq!(
        config(root.path())["mcpServers"]["manual"]["url"],
        "https://manual.example.com/mcp"
    );
    assert!(
        !fs::read_to_string(root.path().join(".codex/config.toml"))
            .unwrap()
            .contains("mcp_servers.acme")
    );
}

#[test]
fn mcp_connections_local_settings_refuse_before_any_host_write() {
    let (root, context) = cloud_fixture(true);
    assert!(prepare(root.path(), &meta(&context), IntegrationChoice::all()).is_err());
    assert!(!root.path().join(".mcp.json").exists());
    assert!(!root.path().join(".codex/config.toml").exists());
    assert!(!root.path().join(".claude/settings.local.json").exists());
}

fn private_stdio_fixture() -> (assert_fs::TempDir, Context) {
    let root = assert_fs::TempDir::new().unwrap();
    fixture_git(root.path(), &["init", "-q"]);
    let connection = json!({"transport":{"type":"stdio","command":"/acme/private/bin/connector","args":["--profile","acme-personal"],"env":{"ACME_REGION":"acme-private-region"}},"agents":{"claude":"acme","codex":"acme"}});
    let local = json!({"schema_version":1,"packs":[],"mcp":{"cloud":connection}}).to_string();
    let bundle = Bundle::capture_layers(
        "schema_version: 1\npacks: []\n",
        Some(&local),
        &[],
        None,
        None,
        &[],
    )
    .unwrap();
    let context = review::mcp::initialize_fixture(root.path(), &bundle);
    (root, context)
}

fn fixture_git(root: &Path, args: &[&str]) {
    let output = std::process::Command::new("/usr/bin/git")
        .current_dir(root)
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success(), "{:?}", output.stderr);
}

#[test]
fn mcp_connections_private_stdio_uses_random_indirection_without_local_values() {
    let (root, context) = private_stdio_fixture();
    let before = meta(&context);
    let plan = prepare(root.path(), &before, IntegrationChoice::all())
        .unwrap()
        .unwrap();
    let output = json!({"shared":plan.after,"files":plan.files}).to_string();
    for private in [
        "/acme/private/bin/connector",
        "acme-personal",
        "acme-private-region",
    ] {
        assert!(!output.contains(private), "{private}");
    }
    let token = plan.desired.as_ref().unwrap().connections["claude"]["launches"]["cloud"]["token"]
        .as_str()
        .unwrap()
        .to_owned();
    let digest =
        plan.desired.as_ref().unwrap().connections["claude"]["launches"]["cloud"]["digest"]
            .as_str()
            .unwrap();
    assert!(!output.contains(digest));
    assert!(!output.contains("cloud"));
    assert!(plan.after.contains(&token));
    assert!(!plan.after.contains("--digest"));
    assert!(plan.after.contains("mcp-connect"));
    apply(&context, &before, &plan).unwrap();
    let snapshot = crate::runtime::Project::open(root.path())
        .unwrap()
        .snapshot()
        .unwrap();
    let connection = &snapshot.mcp_connections()["cloud"];
    for agent in ["claude", "codex"] {
        let expected =
            crate::review::integrations::mcp_projection(root.path(), agent, "cloud", connection)
                .unwrap();
        crate::review::integrations::verify_mcp_connection(root.path(), agent, "acme", &expected)
            .unwrap();
    }
    let (id, binding) =
        crate::review::integrations::resolve_mcp_token(root.path(), &token).unwrap();
    assert_eq!(id, "cloud");
    assert!(binding.matches(connection));
    assert!(crate::review::integrations::resolve_mcp_token(root.path(), &"0".repeat(64)).is_err());
    let repeated = prepare(root.path(), &meta(&context), IntegrationChoice::all())
        .unwrap()
        .unwrap();
    assert_eq!(repeated.after, plan.after);
    let codex_token =
        plan.desired.as_ref().unwrap().connections["codex"]["launches"]["cloud"]["token"]
            .as_str()
            .unwrap();
    let claude = agents::ADAPTERS
        .iter()
        .position(|a| a.spec().id == "claude")
        .unwrap();
    let before = meta(&context);
    let removal = prepare(
        root.path(),
        &before,
        IntegrationChoice::RemoveAgents(1 << claude),
    )
    .unwrap()
    .unwrap();
    apply(&context, &before, &removal).unwrap();
    assert!(crate::review::integrations::resolve_mcp_token(root.path(), &token).is_err());
    assert!(crate::review::integrations::resolve_mcp_token(root.path(), codex_token).is_ok());
    let before = meta(&context);
    let plan = prepare(root.path(), &before, IntegrationChoice::remove_all())
        .unwrap()
        .unwrap();
    apply(&context, &before, &plan).unwrap();
    assert!(config(root.path())["mcpServers"].get("acme").is_none());
    assert!(crate::review::integrations::resolve_mcp_token(root.path(), &token).is_err());
}

#[test]
fn mcp_connections_private_projection_refuses_tracked_destinations_before_writes() {
    for path in [
        ".mcp.json",
        ".codex/config.toml",
        ".claude/settings.local.json",
    ] {
        let (root, context) = private_stdio_fixture();
        fs::create_dir_all(root.path().join(".codex")).unwrap();
        fs::create_dir_all(root.path().join(".claude")).unwrap();
        fs::write(
            root.path().join(path),
            if path.ends_with("json") {
                "{}\n"
            } else {
                "# manual\n"
            },
        )
        .unwrap();
        fixture_git(root.path(), &["add", "-f", "--", path]);
        // An ignore pattern does not make an already tracked file private.
        fs::write(
            root.path().join(".gitignore"),
            ".mcp.json\n.codex/config.toml\n.claude/settings.local.json\n",
        )
        .unwrap();
        let original = fs::read(root.path().join(path)).unwrap();
        let before = meta(&context);
        let error = prepare(root.path(), &before, IntegrationChoice::all())
            .err()
            .unwrap();
        assert!(error.to_string().contains("destination is tracked"));
        assert_eq!(fs::read(root.path().join(path)).unwrap(), original);
        if path != ".claude/settings.local.json" {
            assert!(!root.path().join(".claude/settings.local.json").exists());
        }
        assert_eq!(meta(&context), before);
    }
}

#[test]
fn mcp_connections_private_projection_rechecks_tracking_at_apply() {
    for path in [
        ".mcp.json",
        ".codex/config.toml",
        ".claude/settings.local.json",
    ] {
        let (root, context) = private_stdio_fixture();
        fs::create_dir_all(root.path().join(".codex")).unwrap();
        fs::create_dir_all(root.path().join(".claude")).unwrap();
        fs::write(
            root.path().join(path),
            if path.ends_with("json") {
                "{}\n"
            } else {
                "# manual\n"
            },
        )
        .unwrap();
        let before = meta(&context);
        let plan = prepare(root.path(), &before, IntegrationChoice::all())
            .unwrap()
            .unwrap();
        fixture_git(root.path(), &["add", "-f", "--", path]);
        let original = fs::read(root.path().join(path)).unwrap();
        assert!(apply(&context, &before, &plan).is_err());
        assert_eq!(fs::read(root.path().join(path)).unwrap(), original);
        if path != ".claude/settings.local.json" {
            assert!(!root.path().join(".claude/settings.local.json").exists());
        }
        assert_eq!(meta(&context), before);
    }
}

#[test]
fn mcp_connections_private_projection_refuses_unverifiable_or_legacy_plans() {
    let (root, context) = private_stdio_fixture();
    let before = meta(&context);
    let mut plan = prepare(root.path(), &before, IntegrationChoice::all())
        .unwrap()
        .unwrap();
    let owned = plan
        .desired
        .as_mut()
        .unwrap()
        .connections
        .get_mut("claude")
        .unwrap();
    owned.as_object_mut().unwrap().remove("launches");
    assert!(apply(&context, &before, &plan).is_err());
    assert!(!root.path().join(".mcp.json").exists());
    fs::rename(root.path().join(".git"), root.path().join("saved-git")).unwrap();
    assert!(prepare(root.path(), &before, IntegrationChoice::all()).is_err());
    assert!(!root.path().join(".mcp.json").exists());
    assert_eq!(meta(&context), before);
}

#[test]
fn mcp_connections_owned_drift_conflicts_without_overwriting() {
    let (root, context) = cloud_fixture(false);
    let before = meta(&context);
    let plan = prepare(root.path(), &before, IntegrationChoice::all())
        .unwrap()
        .unwrap();
    apply(&context, &before, &plan).unwrap();
    let path = root.path().join(".mcp.json");
    let changed = fs::read_to_string(&path)
        .unwrap()
        .replace("cloud.example.com", "other.example.com");
    fs::write(&path, &changed).unwrap();
    assert!(prepare(root.path(), &meta(&context), IntegrationChoice::all()).is_err());
    assert_eq!(fs::read_to_string(&path).unwrap(), changed);
}
fn config(root: &Path) -> Value {
    serde_json::from_str(&read_file(root).unwrap().unwrap()).unwrap()
}
fn install(context: &Context) -> Plan {
    let before = meta(context);
    let plan = prepare(
        Path::new(&context.project_root),
        &before,
        IntegrationChoice::agent("claude"),
    )
    .unwrap()
    .unwrap();
    apply(context, &before, &plan).unwrap();
    plan
}

#[test]
fn codex_repairs_missing_permission_registration_and_removes_only_owned_groups() {
    let (root, context) = fixture();
    let initial = prepare(
        root.path(),
        &meta(&context),
        IntegrationChoice::agent("codex"),
    )
    .unwrap()
    .unwrap();
    apply(&context, &meta(&context), &initial).unwrap();
    // Repair a saved registration missing one event, preserving user hooks.
    let mut saved = registration(&meta(&context)).unwrap().unwrap();
    let installed = saved.agents.get_mut("codex").unwrap();
    let permission = installed.groups.remove("PermissionRequest").unwrap();
    let user = json!({"matcher":"Bash","hooks":[{"type":"command","command":"echo Acme permission hook"}]});
    let config = json!({"hooks":{"PreToolUse":[installed.groups["PreToolUse"]],"PostToolUse":[installed.groups["PostToolUse"]],"PermissionRequest":[user]}});
    fs::write(root.path().join(".codex/hooks.json"), config.to_string()).unwrap();
    let old = json!({"integration":saved,"integration_pending":null});
    let upgraded = prepare(root.path(), &old, IntegrationChoice::agent("codex"))
        .unwrap()
        .unwrap();
    let change = upgraded
        .files
        .iter()
        .find(|c| c.path == ".codex/hooks.json")
        .unwrap();
    let after: Value = serde_json::from_str(change.after.as_ref().unwrap()).unwrap();
    assert_eq!(
        after["hooks"]["PermissionRequest"],
        json!([user, permission])
    );
    let installation = upgraded
        .desired
        .as_ref()
        .unwrap()
        .agents
        .get("codex")
        .unwrap();
    let codec = crate::agents::find("codex").unwrap();
    let removed = codec
        .merge_hooks(
            Some(&after.to_string()),
            &installation.groups,
            &crate::agents::setup::HookGroups::new(),
        )
        .unwrap();
    let removed: Value = serde_json::from_str(&removed).unwrap();
    assert_eq!(removed, json!({"hooks":{"PermissionRequest":[user]}}));
    let mut edited = after;
    edited["hooks"]["PermissionRequest"][1]["hooks"][0]["command"] = json!("echo changed");
    assert!(
        codec
            .merge_hooks(
                Some(&edited.to_string()),
                &installation.groups,
                &installation.groups
            )
            .is_err()
    );
}

#[test]
fn registration_is_repeatable_and_removal_preserves_unrelated_content_and_policy() {
    let (root, context) = fixture();
    fs::write(
        root.path().join(".mcp.json"),
        r#"{"mcpServers":{"acme":{"command":"acme"}},"custom":{"keep":true}}"#,
    )
    .unwrap();
    let before = crate::review::project::load(&context).unwrap();
    install(&context);
    let bytes = read_file(root.path()).unwrap();
    install(&context);
    assert_eq!(read_file(root.path()).unwrap(), bytes);
    assert_eq!(
        config(root.path())["mcpServers"].as_object().unwrap().len(),
        2
    );
    let mut changed = config(root.path());
    changed["custom"]["later"] = json!(true);
    fs::write(root.path().join(".mcp.json"), changed.to_string()).unwrap();
    let current = meta(&context);
    let removed = prepare(root.path(), &current, IntegrationChoice::remove_all())
        .unwrap()
        .unwrap();
    apply(&context, &current, &removed).unwrap();
    assert_eq!(
        config(root.path())["mcpServers"],
        json!({"acme":{"command":"acme"}})
    );
    assert_eq!(
        config(root.path())["custom"],
        json!({"keep":true,"later":true})
    );
    let after = crate::review::project::load(&context).unwrap();
    assert_eq!(before.candidates, after.candidates);
    assert_eq!(json!(before.active), json!(after.active));
    assert!(meta(&context)["integration"].is_null());
}

#[cfg(unix)]
#[test]
fn setup_keeps_existing_file_permissions_and_creates_ordinary_files() {
    use std::os::unix::fs::PermissionsExt;
    let (root, context) = fixture();
    let mode = |name: &str| {
        fs::metadata(root.path().join(name))
            .unwrap()
            .permissions()
            .mode()
            & 0o777
    };
    fs::write(root.path().join(".mcp.json"), "{}").unwrap();
    fs::set_permissions(
        root.path().join(".mcp.json"),
        fs::Permissions::from_mode(0o644),
    )
    .unwrap();
    fs::create_dir_all(root.path().join(".codex")).unwrap();
    fs::write(root.path().join(".codex/config.toml"), "model = 'acme'\n").unwrap();
    fs::set_permissions(
        root.path().join(".codex/config.toml"),
        fs::Permissions::from_mode(0o600),
    )
    .unwrap();
    for choice in [
        IntegrationChoice::agent("claude"),
        IntegrationChoice::agent("codex"),
    ] {
        let before = meta(&context);
        let plan = prepare(root.path(), &before, choice).unwrap().unwrap();
        apply(&context, &before, &plan).unwrap();
    }
    assert_eq!(mode(".mcp.json"), 0o644);
    assert_eq!(mode(".codex/config.toml"), 0o600);
    // New files get the same permissions as any file the user creates.
    fs::write(root.path().join("acme-ordinary.txt"), "").unwrap();
    for created in [".claude/settings.local.json", ".codex/hooks.json"] {
        assert_eq!(mode(created), mode("acme-ordinary.txt"), "{created}");
    }
}

#[test]
fn native_agents_coexist_preserve_user_configuration_and_remove_only_owned_entries() {
    let (root, context) = fixture();
    fs::create_dir_all(root.path().join(".codex/rules")).unwrap();
    fs::create_dir_all(root.path().join(".claude")).unwrap();
    fs::write(
        root.path().join(".codex/rules/default.rules"),
        "Acme user rules\n",
    )
    .unwrap();
    let codex = "# Acme configuration\nmodel = 'acme'\n[mcp_servers.acme]\ncommand = 'acme'\n";
    fs::write(root.path().join(".codex/config.toml"), codex).unwrap();
    let user = json!({"hooks":{"PreToolUse":[{"matcher":"Acme","hooks":[{"type":"command","command":"acme"}]}]},"enabledPlugins":{"acme":true},"custom":42,"permissions":{"allow":["Read"],"deny":["Read(.env)"]},"enabledMcpjsonServers":["other"]});
    fs::write(
        root.path().join(".claude/settings.local.json"),
        user.to_string(),
    )
    .unwrap();
    for choice in [
        IntegrationChoice::agent("codex"),
        IntegrationChoice::agent("claude"),
        IntegrationChoice::agent("codex"),
        IntegrationChoice::agent("claude"),
    ] {
        let before = meta(&context);
        let plan = prepare(root.path(), &before, choice).unwrap().unwrap();
        apply(&context, &before, &plan).unwrap();
    }
    let report = status(&context, &crate::review::project::load(&context).unwrap());
    assert_eq!(report["repair_needed"], false, "{report}");
    assert_eq!(
        report["installed_integrations"].as_array().unwrap().len(),
        2
    );
    assert_eq!(report["live_host_verification"], "unverified");
    let settings: Value = serde_json::from_str(
        &fs::read_to_string(root.path().join(".claude/settings.local.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(settings["hooks"]["PreToolUse"].as_array().unwrap().len(), 2);
    assert_eq!(settings["enabledPlugins"], user["enabledPlugins"]);
    let mut allow = vec!["Read".to_owned()];
    allow.extend(
        crate::review::mcp::default_approved()
            .iter()
            .map(|tool| format!("mcp__shield__{tool}")),
    );
    assert_eq!(settings["permissions"]["allow"], json!(allow));
    assert_eq!(
        settings["enabledMcpjsonServers"],
        json!(["other", "shield"])
    );
    let before = meta(&context);
    assert!(
        prepare(root.path(), &before, IntegrationChoice::Keep)
            .unwrap()
            .is_none()
    );
    let plan = prepare(root.path(), &before, IntegrationChoice::remove_all())
        .unwrap()
        .unwrap();
    apply(&context, &before, &plan).unwrap();
    let remaining: Value = serde_json::from_str(
        &fs::read_to_string(root.path().join(".claude/settings.local.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(remaining, user);
    let remaining = fs::read_to_string(root.path().join(".codex/config.toml")).unwrap();
    assert!(remaining.contains("# Acme configuration"));
    assert!(remaining.contains("command = 'acme'"));
    assert!(!remaining.contains("mcp_servers.shield"));
    assert_eq!(
        fs::read_to_string(root.path().join(".codex/rules/default.rules")).unwrap(),
        "Acme user rules\n"
    );
}

#[test]
fn native_conflicts_and_missing_groups_require_reviewed_repair() {
    let (root, context) = fixture();
    install(&context);
    let settings = root.path().join(".claude/settings.local.json");
    let mut config: Value = serde_json::from_str(&fs::read_to_string(&settings).unwrap()).unwrap();
    let original = config.clone();
    config["hooks"]["PreToolUse"][0]["hooks"][0]["timeout"] = json!(30);
    fs::write(&settings, config.to_string()).unwrap();
    assert!(
        prepare(
            root.path(),
            &meta(&context),
            IntegrationChoice::agent("claude")
        )
        .is_err()
    );
    assert!(
        prepare(
            root.path(),
            &meta(&context),
            IntegrationChoice::remove_all()
        )
        .is_err()
    );
    assert_eq!(
        serde_json::from_str::<Value>(&fs::read_to_string(&settings).unwrap()).unwrap(),
        config
    );
    fs::write(&settings, original.to_string()).unwrap();
    fs::remove_file(&settings).unwrap();
    assert_eq!(
        status(&context, &crate::review::project::load(&context).unwrap())["repair_needed"],
        true
    );
    let before = meta(&context);
    let plan = prepare(root.path(), &before, IntegrationChoice::agent("claude"))
        .unwrap()
        .unwrap();
    apply(&context, &before, &plan).unwrap();
    assert_eq!(
        status(&context, &crate::review::project::load(&context).unwrap())["repair_needed"],
        false
    );
}

#[test]
fn native_project_registration_is_isolated_and_symlink_parents_refuse() {
    let (first, a) = fixture();
    let (second, b) = fixture();
    install(&a);
    install(&b);
    let first_bytes = fs::read(first.path().join(".claude/settings.local.json")).unwrap();
    let second_bytes = fs::read(second.path().join(".claude/settings.local.json")).unwrap();
    assert_ne!(first_bytes, second_bytes);
    let before = meta(&a);
    apply(
        &a,
        &before,
        &prepare(first.path(), &before, IntegrationChoice::remove_all())
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        fs::read(second.path().join(".claude/settings.local.json")).unwrap(),
        second_bytes
    );
    #[cfg(unix)]
    {
        fs::create_dir(first.path().join("acme")).unwrap();
        std::os::unix::fs::symlink(first.path().join("acme"), first.path().join(".codex")).unwrap();
        assert!(prepare(first.path(), &meta(&a), IntegrationChoice::agent("codex")).is_err());
    }
}

#[test]
fn native_setup_preserves_unowned_plugin_files_and_settings() {
    let (root, context) = fixture();
    let path = ".claude/skills/shield/hooks/register.mjs";
    fs::create_dir_all(root.path().join(".claude/skills/shield/hooks")).unwrap();
    fs::write(root.path().join(path), "Acme plugin asset").unwrap();
    fs::write(
        root.path().join(".claude/settings.local.json"),
        r#"{"enabledPlugins":{"shield@skills-dir":true,"acme":true}}"#,
    )
    .unwrap();
    install(&context);
    let before = meta(&context);
    let plan = prepare(root.path(), &before, IntegrationChoice::agent("codex"))
        .unwrap()
        .unwrap();
    apply(&context, &before, &plan).unwrap();
    assert_eq!(
        fs::read_to_string(root.path().join(path)).unwrap(),
        "Acme plugin asset"
    );
    let config: Value = serde_json::from_str(
        &fs::read_to_string(root.path().join(".claude/settings.local.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        config["enabledPlugins"],
        json!({"shield@skills-dir":true,"acme":true})
    );
    assert_eq!(config["hooks"]["PreToolUse"].as_array().unwrap().len(), 1);
    assert_eq!(
        status(&context, &crate::review::project::load(&context).unwrap())["live_host_verification"],
        "unverified"
    );
    assert!(meta(&context)["integration_verification"].is_null());
    assert_eq!(
        meta(&context)["integration"]["agents"]
            .as_object()
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn unsupported_ownership_and_repair_paths_refuse_without_writing() {
    let (root, context) = fixture();
    let plan = install(&context);
    let before = meta(&context);
    let settings = fs::read(root.path().join(".claude/settings.local.json")).unwrap();
    let mut unsupported = before.clone();
    unsupported["integration"]["interceptor"] = json!({"assets":{}});
    assert!(prepare(root.path(), &unsupported, IntegrationChoice::agent("codex")).is_err());

    let mut pending = plan;
    pending.files = vec![files::Change {
        path: ".claude/skills/shield/hooks/register.mjs".into(),
        before: None,
        after: Some("unowned file".into()),
    }];
    unsupported = before.clone();
    unsupported["integration_pending"] = json!(pending);
    assert!(prepare(root.path(), &unsupported, IntegrationChoice::Keep).is_err());
    assert_eq!(meta(&context), before);
    assert_eq!(
        fs::read(root.path().join(".claude/settings.local.json")).unwrap(),
        settings
    );
    assert!(!root.path().join(".claude/skills/shield").exists());
}

#[test]
fn invalid_or_unowned_settings_refuse_before_writing() {
    let (root, context) = fixture();
    for bytes in [
        "broken",
        "[]",
        r#"{"mcpServers":[]}"#,
        r#"{"mcpServers":{},"mcpServers":{}}"#,
        r#"{"mcpServers":{"shield":{"command":"acme"}}}"#,
    ] {
        fs::write(root.path().join(".mcp.json"), bytes).unwrap();
        let before = meta(&context);
        assert!(prepare(root.path(), &before, IntegrationChoice::agent("claude")).is_err());
        assert_eq!(read_file(root.path()).unwrap().as_deref(), Some(bytes));
        assert_eq!(meta(&context), before);
    }
}

#[test]
fn keep_and_removal_without_ownership_create_no_host_configuration() {
    let (root, context) = fixture();
    let before = meta(&context);
    for choice in [IntegrationChoice::Keep, IntegrationChoice::remove_all()] {
        assert!(prepare(root.path(), &before, choice).unwrap().is_none());
        assert!(!root.path().join(".mcp.json").exists());
    }
    assert_eq!(meta(&context), before);
}

#[test]
fn concurrent_file_and_ownership_changes_require_fresh_confirmation() {
    let (root, context) = fixture();
    let before = meta(&context);
    let plan = prepare(root.path(), &before, IntegrationChoice::agent("claude"))
        .unwrap()
        .unwrap();
    fs::write(root.path().join(".mcp.json"), r#"{"custom":true}"#).unwrap();
    assert!(apply(&context, &before, &plan).is_err());
    assert_eq!(config(root.path()), json!({"custom":true}));
    assert!(pending(&meta(&context)).unwrap().is_none());
    let plan = prepare(root.path(), &before, IntegrationChoice::agent("claude"))
        .unwrap()
        .unwrap();
    Store::open(root.path())
        .unwrap()
        .update(&context, &|| Ok(()), |state| {
            state.operator["integration_pending"] = json!(plan);
            Ok(json!({}))
        })
        .unwrap();
    assert!(apply(&context, &before, &plan).is_err());
}

#[test]
fn confirmed_plans_resume_before_and_after_file_publication() {
    for already_written in [false, true] {
        let (root, context) = fixture();
        let before = meta(&context);
        let plan = prepare(root.path(), &before, IntegrationChoice::agent("claude"))
            .unwrap()
            .unwrap();
        Store::open(root.path())
            .unwrap()
            .update(&context, &|| Ok(()), |state| {
                state.operator["integration_pending"] = json!(plan);
                Ok(json!({}))
            })
            .unwrap();
        if already_written {
            publish(root.path(), &plan).unwrap();
        }
        let current = meta(&context);
        assert_eq!(
            status(&context, &crate::review::project::load(&context).unwrap())["repair_needed"],
            true
        );
        let resumed = prepare(root.path(), &current, IntegrationChoice::Keep)
            .unwrap()
            .unwrap();
        apply(&context, &current, &resumed).unwrap();
        assert_eq!(
            status(&context, &crate::review::project::load(&context).unwrap())["repair_needed"],
            false
        );
        assert!(pending(&meta(&context)).unwrap().is_none());
        assert_eq!(
            config(root.path())["mcpServers"]["shield"],
            plan.desired.unwrap().entry
        );
    }
}

#[test]
fn failed_publication_retains_confirmed_plan_for_reviewed_retry() {
    let (root, context) = fixture();
    let before = meta(&context);
    let plan = prepare(root.path(), &before, IntegrationChoice::agent("claude"))
        .unwrap()
        .unwrap();
    let collision = root
        .path()
        .join(format!(".shield-mcp-{}.tmp", std::process::id()));
    fs::write(&collision, "Acme owned collision").unwrap();
    assert!(apply(&context, &before, &plan).is_err());
    assert!(!root.path().join(".mcp.json").exists());
    assert_eq!(
        fs::read_to_string(&collision).unwrap(),
        "Acme owned collision"
    );
    let current = meta(&context);
    assert!(pending(&current).unwrap().is_some());
    fs::remove_file(&collision).unwrap();
    let resumed = prepare(root.path(), &current, IntegrationChoice::Keep)
        .unwrap()
        .unwrap();
    apply(&context, &current, &resumed).unwrap();
    assert!(pending(&meta(&context)).unwrap().is_none());
    assert!(root.path().join(".mcp.json").is_file());
}

#[test]
fn manual_edits_during_recovery_and_owned_entry_changes_are_preserved() {
    let (root, context) = fixture();
    let installed = install(&context);
    let mut changed = config(root.path());
    changed["mcpServers"]["shield"]["command"] = json!("acme-custom");
    fs::write(root.path().join(".mcp.json"), changed.to_string()).unwrap();
    assert!(
        prepare(
            root.path(),
            &meta(&context),
            IntegrationChoice::remove_all()
        )
        .is_err()
    );
    assert_eq!(
        status(&context, &crate::review::project::load(&context).unwrap())["repair_needed"],
        true
    );
    Store::open(root.path())
        .unwrap()
        .update(&context, &|| Ok(()), |state| {
            state.operator["integration_pending"] = json!(installed);
            Ok(json!({}))
        })
        .unwrap();
    assert!(prepare(root.path(), &meta(&context), IntegrationChoice::Keep).is_err());
    assert_eq!(config(root.path()), changed);
}

#[test]
fn diagnostic_reads_detect_missing_binary_and_do_not_mutate_state_or_files() {
    let (root, context) = fixture();
    install(&context);
    Store::open(root.path())
        .unwrap()
        .update(&context, &|| Ok(()), |state| {
            state.operator["integration"]["binary"] = json!(root.path().join("missing-shield"));
            Ok(json!({}))
        })
        .unwrap();
    let before = fs::read(root.path().join(".shield/review/state.json")).unwrap();
    let file = read_file(root.path()).unwrap();
    let report = status(&context, &crate::review::project::load(&context).unwrap());
    assert_eq!(report["registered_integrations"][0]["status"], "degraded");
    assert_eq!(
        report["tool_interception"],
        "native hooks installed; host loading is unverified"
    );
    assert_eq!(report["installed_integrations"][0]["status"], "degraded");
    assert_eq!(
        before,
        fs::read(root.path().join(".shield/review/state.json")).unwrap()
    );
    assert_eq!(file, read_file(root.path()).unwrap());
}

#[test]
#[cfg(unix)]
fn symlinked_and_oversized_configuration_is_never_replaced() {
    let (root, context) = fixture();
    let other = assert_fs::TempDir::new().unwrap();
    let target = other.path().join("config.json");
    fs::write(&target, "{}").unwrap();
    std::os::unix::fs::symlink(&target, root.path().join(".mcp.json")).unwrap();
    assert!(
        prepare(
            root.path(),
            &meta(&context),
            IntegrationChoice::agent("claude")
        )
        .is_err()
    );
    assert_eq!(fs::read_to_string(&target).unwrap(), "{}");
    fs::remove_file(root.path().join(".mcp.json")).unwrap();
    fs::write(
        root.path().join(".mcp.json"),
        " ".repeat(usize::try_from(MAX_CONFIG).unwrap() + 1),
    )
    .unwrap();
    assert!(
        prepare(
            root.path(),
            &meta(&context),
            IntegrationChoice::agent("claude")
        )
        .is_err()
    );
}

#[test]
fn fallback_is_preserved_by_agent_repair_and_validated_under_current_activation() {
    let (root, context) = fixture();
    configure_fixture(
        root.path(),
        IntegrationChoice::agent("codex"),
        AskFallback::Allow,
    );
    let before = meta(&context);
    let plan = prepare(root.path(), &before, IntegrationChoice::agent("claude"))
        .unwrap()
        .unwrap();
    apply(&context, &before, &plan).unwrap();
    assert!(fallback(&meta(&context), "codex").unwrap() == AskFallback::Allow);
    let active = crate::review::project::load(&context)
        .unwrap()
        .active
        .unwrap();
    let checked = PathAccess::default();
    assert!(
        with_fallback(root.path(), active.activation_id(), "codex", checked, |s| s
            == AskFallback::Allow)
        .unwrap()
    );
    assert!(
        with_fallback(root.path(), "stale", "codex", checked, |_| panic!(
            "stale handoff"
        ))
        .is_err()
    );
    // A request checked against access the operator has since changed is stale.
    let relaxed = PathAccess {
        agent_settings_change: Change::Allow,
        ..checked
    };
    crate::review::path_access::configure_fixture(root.path(), relaxed);
    assert!(
        with_fallback(
            root.path(),
            active.activation_id(),
            "codex",
            checked,
            |_| panic!("handoff checked against replaced access")
        )
        .is_err()
    );
    assert!(
        with_fallback(
            root.path(),
            active.activation_id(),
            "codex",
            relaxed,
            |_| ()
        )
        .is_ok()
    );
    let report = status(&context, &crate::review::project::load(&context).unwrap());
    assert!(
        report["installed_integrations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["agent"] == "codex"
                && r["ask_fallback"] == "allow"
                && r["native_approval"] == false)
    );
}

#[test]
fn retained_policy_proposal_cannot_restore_a_superseded_ask_fallback() {
    let (root, context) = fixture();
    configure_fixture(
        root.path(),
        IntegrationChoice::agent("codex"),
        AskFallback::Allow,
    );
    let mut mcp_context = context.clone();
    mcp_context.surface = "mcp".into();
    mcp_context.agent = Some("codex".into());
    mcp_context.connection_id = Some("fallback-review".into());
    let connection = review::mcp::Connection::new("fallback-review".into());
    let propose = |id: &str| {
        review::mcp::call(
            &mcp_context,
            &connection,
            "policy_propose",
            &json!({
                "change":{"kind":"rule_add","rule":{
                    "id":id,"decision":"allow","conditions":{
                        "domain":"argv","executable":"acme","arguments":{"kind":"any"}
                    }
                }},"idempotency_key":id
            }),
            &|| Ok(()),
        )
    };
    let proposed = propose("before-fallback-change");
    assert!(proposed["error"].is_null(), "{proposed}");
    let original = crate::review::project::load(&context).unwrap();
    configure_fixture(
        root.path(),
        IntegrationChoice::agent("codex"),
        AskFallback::Block,
    );
    let current = crate::review::project::load(&context).unwrap();
    assert_eq!(current.policy_revision, original.policy_revision);
    let state_path = root.path().join(".shield/review/state.json");
    let before = fs::read(&state_path).unwrap();
    let applied = review::mcp::call(
        &mcp_context,
        &connection,
        "policy_apply",
        &proposed["data"]["apply_arguments"],
        &|| Ok(()),
    );
    assert_eq!(applied["error"]["code"], "stale_revision", "{applied}");
    assert!(matches!(
        super::super::commit_proposal_fixture(
            &context,
            proposed["data"]["proposal_id"].as_str().unwrap()
        ),
        Err(Error::StaleSnapshot)
    ));
    assert_eq!(fs::read(&state_path).unwrap(), before);
    assert!(fallback(&meta(&context), "codex").unwrap() == AskFallback::Block);

    // A fresh proposal may apply while independently confirmed path access stays current.
    let fresh = propose("after-fallback-change");
    assert!(fresh["error"].is_null(), "{fresh}");
    let access = PathAccess {
        agent_settings_change: Change::Allow,
        ..PathAccess::default()
    };
    crate::review::path_access::configure_fixture(root.path(), access);
    super::super::commit_proposal_fixture(&context, fresh["data"]["proposal_id"].as_str().unwrap())
        .unwrap();
    assert_eq!(
        crate::review::path_access::current(root.path()).unwrap(),
        access
    );
    assert!(fallback(&meta(&context), "codex").unwrap() == AskFallback::Block);
}

#[test]
fn interrupted_fallback_revocation_blocks_until_confirmed_repair_finishes() {
    let (root, context) = fixture();
    configure_fixture(
        root.path(),
        IntegrationChoice::agent("codex"),
        AskFallback::Allow,
    );
    let before = meta(&context);
    let active = crate::review::project::load(&context)
        .unwrap()
        .active
        .unwrap();
    let mut plan = prepare(root.path(), &before, IntegrationChoice::agent("codex"))
        .unwrap()
        .unwrap();
    plan.desired
        .as_mut()
        .unwrap()
        .ask_fallback
        .insert("codex".into(), AskFallback::Block);
    Store::open(root.path())
        .unwrap()
        .update(&context, &|| Ok(()), |state| {
            state.operator["integration_pending"] = json!(plan);
            Ok(json!({}))
        })
        .unwrap();
    assert!(
        with_fallback(
            root.path(),
            active.activation_id(),
            "codex",
            PathAccess::default(),
            |s| s == AskFallback::Block
        )
        .unwrap()
    );
    let report = status(&context, &crate::review::project::load(&context).unwrap());
    assert_eq!(report["repair_needed"], true);
    assert_eq!(report["installed_integrations"][0]["ask_fallback"], "block");
    let current = meta(&context);
    let resumed = prepare(root.path(), &current, IntegrationChoice::Keep)
        .unwrap()
        .unwrap();
    apply(&context, &current, &resumed).unwrap();
    assert!(fallback(&meta(&context), "codex").unwrap() == AskFallback::Block);
    assert_eq!(
        crate::review::project::load(&context)
            .unwrap()
            .active
            .unwrap()
            .activation_id(),
        active.activation_id()
    );
}

#[test]
fn setup_review_summarizes_agents_and_never_repeats_file_contents() {
    let root = assert_fs::TempDir::new().unwrap();
    fs::create_dir_all(root.path().join(".codex")).unwrap();
    fs::write(
        root.path().join(".codex/config.toml"),
        "model = \"acme-model\"\n",
    )
    .unwrap();
    let plan = prepare(root.path(), &json!({}), IntegrationChoice::all())
        .unwrap()
        .unwrap();
    let text = super::super::presentation::review(&workflow::preview(root.path(), Some(&plan)));
    assert!(text.contains("Claude Code added · hooks"), "{text}");
    assert!(text.contains("Codex       added · hooks"), "{text}");
    assert!(
        text.contains(".claude/settings.local.json  created"),
        "{text}"
    );
    assert!(
        text.contains(".codex/config.toml           updated"),
        "{text}"
    );
    // Neither user settings, Shield's entries nor raw JSON fields are repeated.
    for raw in [
        "model = ",
        "mcp_servers",
        "\\n",
        "groups",
        "before:",
        "after:",
    ] {
        assert!(!text.contains(raw), "{raw}: {text}");
    }
}

#[test]
fn codex_prefix_rules_follow_the_active_policy_and_the_agent() {
    let (root, context) = fixture();
    let before = meta(&context);
    let plan = prepare(root.path(), &before, IntegrationChoice::agent("codex"))
        .unwrap()
        .unwrap();
    apply(&context, &before, &plan).unwrap();
    let path = root.path().join(".codex/rules/shield.rules");
    let activate = |decision: &str| {
        let state = crate::review::project::load(&context).unwrap();
        let rules = json!({"schema_version":1,"kind":"custom",
            "capabilities":["agent-outputs-v1","argument-rules-v1","structured-rules-v1"],
            "rules":[{"id":"logs","decision":decision,"agents":{"codex":{"prefixes":[["acme","logs"]]}},
                "conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"prefix","values":["logs"]}}}]})
        .to_string();
        let candidate =
            crate::review::packs::rebuild(&state.candidates["project"], &[], &[], Some(&rules))
                .unwrap();
        crate::review::publication::commit(
            &context,
            &state,
            candidate,
            crate::review::project::metadata(&state),
            None,
        )
        .unwrap()
    };
    let repair =
        || crate::review::project::status(&context).unwrap()["coverage"]["repair_needed"].clone();
    let resync = || {
        prefix_rules::sync(
            root.path(),
            &meta(&context),
            &prefix_rules::active(root.path()).unwrap(),
        )
    };

    let receipt = activate("allow");
    assert_eq!(receipt["repair_needed"], false, "{receipt}");
    let text = fs::read_to_string(&path).unwrap();
    assert!(text.starts_with("# Generated by Shield"), "{text}");
    assert!(text.contains(r#"pattern = ["acme", "logs"]"#), "{text}");
    assert!(text.contains("custom logs"), "{text}");
    assert_eq!(repair(), false);

    // A hand edit is drift that setup repairs.
    fs::write(&path, format!("{text}prefix_rule(pattern = [\"rm\"])\n")).unwrap();
    assert_eq!(repair(), true);
    resync().unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), text);

    // A rule that no longer allows leaves no prefix.
    activate("ask");
    assert!(!path.exists());

    // A file Shield did not write is never replaced.
    fs::write(&path, "Acme user rules\n").unwrap();
    let receipt = activate("allow");
    assert_eq!(receipt["repair_needed"], true);
    // Both the receipt and status say to move the file, not to repair it.
    assert!(
        receipt["setup_error"]
            .as_str()
            .is_some_and(|e| e.contains("not Shield's; move it aside")),
        "{receipt}"
    );
    let status = crate::review::project::status(&context).unwrap();
    assert!(
        status["coverage"]["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d.as_str().is_some_and(|d| d.contains("move it aside"))),
        "{status}"
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), "Acme user rules\n");
    assert!(resync().is_err());
    fs::remove_file(&path).unwrap();
    resync().unwrap();
    assert!(path.exists());

    // Removing the agent removes its prefix rules.
    let before = meta(&context);
    let plan = prepare(root.path(), &before, IntegrationChoice::remove_all())
        .unwrap()
        .unwrap();
    apply(&context, &before, &plan).unwrap();
    resync().unwrap();
    assert!(!path.exists());
}

#[test]
fn reviews_list_the_codex_prefixes_a_change_adds_or_removes() {
    let (root, context) = fixture();
    let candidate = |state: &crate::review::model::State, decision: &str| {
        let rules = json!({"schema_version":1,"kind":"custom",
            "capabilities":["agent-outputs-v1","argument-rules-v1","structured-rules-v1"],
            "rules":[{"id":"logs","decision":decision,"agents":{"codex":{"prefixes":[["acme","logs"]]}},
                "conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"prefix","values":["logs"]}}}]})
        .to_string();
        crate::review::packs::rebuild(&state.candidates["project"], &[], &[], Some(&rules)).unwrap()
    };
    let review = |state: &crate::review::model::State, candidate: &Value| {
        super::super::presentation::review(
            &crate::review::publication::change_review(
                &context,
                state,
                candidate,
                &crate::review::project::metadata(state),
            )
            .unwrap(),
        )
    };

    // A policy review names each prefix a change adds, with its rule.
    let current = crate::review::project::load(&context).unwrap();
    let allow = candidate(&current, "allow");
    let text = review(&current, &allow);
    assert!(
        text.contains("Codex runs these outside its sandbox without asking (.codex/rules/shield.rules):\n    acme logs (custom logs)"),
        "{text}"
    );
    crate::review::publication::commit(
        &context,
        &current,
        allow,
        crate::review::project::metadata(&current),
        None,
    )
    .unwrap();

    // Setup shows the prefix rules file it writes for the active policy.
    let before = meta(&context);
    let plan = prepare(root.path(), &before, IntegrationChoice::agent("codex"))
        .unwrap()
        .unwrap();
    let mut preview = workflow::preview(root.path(), Some(&plan));
    prefix_rules::preview(
        root.path(),
        &mut preview,
        &prefix_rules::active(root.path()).unwrap(),
    );
    let text = super::super::presentation::review(&preview);
    assert!(text.contains("acme logs (custom logs)"), "{text}");
    assert!(
        text.contains(".codex/rules/shield.rules  created    Codex prefix rules"),
        "{text}"
    );

    // A change that stops allowing removes the prefix, and says so.
    let current = crate::review::project::load(&context).unwrap();
    let text = review(&current, &candidate(&current, "ask"));
    assert!(
        text.contains("Codex stops running these outside its sandbox (.codex/rules/shield.rules):\n    acme logs (custom logs)"),
        "{text}"
    );
    // Changes that leave prefixes alone add nothing to the review.
    let text = review(&current, &current.candidates["project"]);
    assert!(!text.contains("Codex"), "{text}");
}
