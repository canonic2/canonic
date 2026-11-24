//! Real file/state acceptance for owned registration and interrupted reconciliation.
use super::*;
use crate::configuration::{Bundle, Evaluator, settings::Values};
use crate::review::path_access::{Change, PathAccess};
use crate::review::{
    integrations::{fallback, publish, status, with_fallback},
    storage::Store,
};
use std::collections::BTreeMap;
mod agent_isolation;
mod interrupted;
mod repository_independence;
mod status_reporting;

#[test]
fn init_in_a_clone_needs_no_review_and_creates_no_activation() {
    let original = assert_fs::TempDir::new().unwrap();
    let config = "schema_version: 1\npacks: []\nrules: rules.yml\n";
    let rules = r#"{"schema_version":1,"kind":"custom","capabilities":["structured-rules-v1","argument-rules-v1"],"rules":[{"id":"inspect","decision":"allow","conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"exact","values":["inspect"]}}}]}"#;
    let bundle = Bundle::capture(config, &[], Some(rules)).unwrap();
    let before = review::mcp::initialize_fixture(original.path(), &bundle);
    let clone = assert_fs::TempDir::new().unwrap();
    fs::create_dir(clone.path().join(".shield")).unwrap();
    fs::write(clone.path().join(".shield/config.yml"), config).unwrap();
    fs::write(clone.path().join(".shield/rules.yml"), rules).unwrap();
    // JSON mode has no terminal: init succeeds because there is nothing to review.
    let report = initialize_in_cache(clone.path(), None, true, None, Baseline::Skip).unwrap();
    assert_ne!(report["project_id"], before.project_id);
    assert!(report["policy_files"]["digest"].is_string(), "{report}");
    let saved: Value =
        serde_json::from_slice(&fs::read(clone.path().join(".shield/state/state.json")).unwrap())
            .unwrap();
    assert!(saved.get("active").is_none(), "{saved}");
    // The next request is decided by the files.
    let evaluated = crate::runtime::Project::open(clone.path())
        .unwrap()
        .snapshot()
        .unwrap()
        .evaluate(crate::policy::Request {
            executable: "acme".into(),
            arguments: vec!["inspect".into()],
            cwd: clone
                .path()
                .canonicalize()
                .unwrap()
                .to_str()
                .unwrap()
                .into(),
            project: clone
                .path()
                .canonicalize()
                .unwrap()
                .to_str()
                .unwrap()
                .into(),
        })
        .unwrap();
    assert_eq!(
        evaluated.evaluation().decision,
        crate::policy::Decision::Allow
    );
    let state = Store::open(clone.path())
        .unwrap()
        .read_checkpoint()
        .unwrap();
    assert_eq!(
        fs::read_to_string(clone.path().join(".shield/config.yml")).unwrap(),
        config
    );
    assert_eq!(
        fs::read_to_string(clone.path().join(".shield/rules.yml")).unwrap(),
        rules
    );
    assert!(
        state.proposals.is_empty()
            && state.read_requests.is_empty()
            && state.run_requests.is_empty()
    );
}

#[test]
fn antigravity_doctor_reports_disabled_hooks_and_removal_deletes_empty_mcp_settings() {
    let (root, context) = fixture();
    let before = meta(&context);
    let plan = prepare(root.path(), &before, IntegrationChoice::agent("agy"))
        .unwrap()
        .unwrap();
    apply(&context, &before, &plan).unwrap();
    let hooks_path = root.path().join(".agents/hooks.json");
    let original = fs::read_to_string(&hooks_path).unwrap();
    let mut hooks: Value = serde_json::from_str(&original).unwrap();
    hooks["shield-project-enforcement"]["enabled"] = json!(false);
    fs::write(&hooks_path, hooks.to_string()).unwrap();
    let report = status(
        &context,
        &crate::review::project::checkpoint(&context).unwrap(),
    );
    assert!(
        report["diagnostics"].to_string().contains("disabled"),
        "{report}"
    );
    fs::write(&hooks_path, original).unwrap();
    let before = meta(&context);
    let plan = prepare(
        root.path(),
        &before,
        IntegrationChoice::RemoveAgents(IntegrationChoice::bits(&["agy"]).unwrap()),
    )
    .unwrap()
    .unwrap();
    apply(&context, &before, &plan).unwrap();
    assert!(!root.path().join(".agents/mcp_config.json").exists());
}

#[test]
fn retained_setup_records_preserve_legacy_block_fallback_and_require_file_ownership() {
    let (root, context) = fixture();
    let plan = prepare(root.path(), &meta(&context), IntegrationChoice::all())
        .unwrap()
        .unwrap();
    let desired = plan.desired.as_ref().unwrap();
    let choice = IntegrationChoice::all();
    assert_eq!(
        desired.agents.len(),
        crate::agents::ADAPTERS
            .iter()
            .filter(|a| choice.contains(a.spec().id))
            .count()
    );
    for adapter in crate::agents::ADAPTERS {
        assert_eq!(
            desired.agents.contains_key(adapter.spec().id),
            choice.contains(adapter.spec().id)
        );
    }
    let mut incomplete_plan = json!(plan);
    incomplete_plan.as_object_mut().unwrap().remove("files");
    assert!(pending(&json!({"integration_pending":incomplete_plan})).is_err());

    let mut incomplete_registration = json!(plan.desired.as_ref().unwrap());
    incomplete_registration
        .as_object_mut()
        .unwrap()
        .remove("ask_fallback");
    let legacy_meta = json!({"integration":incomplete_registration});
    assert!(
        registration(&legacy_meta)
            .unwrap()
            .unwrap()
            .ask_fallback
            .is_empty()
    );
    assert!(matches!(
        fallback(&legacy_meta, &Values::default(), "codex").unwrap(),
        AskFallback::Block
    ));
    // The same retained codec is used inside interrupted setup plans.
    let mut legacy_plan = json!(plan);
    legacy_plan["desired"] = legacy_meta["integration"].clone();
    assert!(
        pending(&json!({"integration_pending":legacy_plan}))
            .unwrap()
            .unwrap()
            .desired
            .unwrap()
            .ask_fallback
            .is_empty()
    );
}

#[test]
fn historical_native_registration_retains_exact_ownership_without_new_permissions() {
    // Native registration shape and launch bytes from 27343ffa (0.1.0),
    // before saved fallback/tool selections and per-event hook arguments.
    let group = json!({"matcher":".*","hooks":[{"type":"command",
        "command":"'/usr/local/bin/shield' __internal native-hook --project '/srv/acme' --agent claude || exit 2",
        "timeout":10,"statusMessage":"Shield project enforcement"}]});
    let legacy = json!({"binary":"/usr/local/bin/shield","binary_digest":"a".repeat(64),"version":"0.1.0",
        "entry":{"type":"stdio","command":"/usr/local/bin/shield",
            "args":["__internal","mcp","--project","/srv/acme","--agent","claude-code"]},
        "agents":{"claude":{"groups":{"PreToolUse":group,"PostToolUse":group,"PostToolUseFailure":group},"mcp":null}}});
    let meta = json!({"integration":legacy});
    let bytes = meta.to_string();
    let decoded = registration(&meta).unwrap().unwrap();
    assert_eq!(json!(decoded.agents), legacy["agents"]);
    assert_eq!(decoded.entry, legacy["entry"]);
    assert!(decoded.approved_tools.is_none());
    assert!(decoded.connections.is_empty());
    assert!(matches!(
        fallback(&meta, &Values::default(), "codex").unwrap(),
        AskFallback::Block
    ));
    assert_eq!(meta.to_string(), bytes);
    let mut explicit = legacy.clone();
    explicit["agents"]["codex"] = explicit["agents"]["claude"].clone();
    explicit["ask_fallback"] = json!({"codex":"allow"});
    // A record's fallback still loads; the policy files decide, and the
    // record is never written with it again.
    let explicit = json!({"integration":explicit});
    let mut allow = Values::default();
    allow
        .ask_fallback
        .insert("codex".into(), AskFallback::Allow);
    assert!(matches!(
        fallback(&explicit, &allow, "codex").unwrap(),
        AskFallback::Allow
    ));
    assert!(
        json!(registration(&explicit).unwrap())
            .get("ask_fallback")
            .is_none()
    );
    // Legacy interceptor ownership is a different protocol, not native hooks.
    let mut interceptor = legacy;
    interceptor["interceptor"] = json!({"assets":{}});
    assert!(registration(&json!({"integration":interceptor})).is_err());
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
    assert_eq!(
        plan.desired
            .as_ref()
            .unwrap()
            .agents
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["agy", "gemini", "opencode"]
    );
    apply(&context, &before, &plan).unwrap();
    assert!(config(root.path())["mcpServers"].get("shield").is_none());
    let before = meta(&context);
    let plan = prepare(root.path(), &before, IntegrationChoice::remove_all())
        .unwrap()
        .unwrap();
    assert!(plan.desired.is_none());
    apply(&context, &before, &plan).unwrap();
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
    assert_eq!(
        captured.previous.as_ref().unwrap().agents.len(),
        crate::agents::ADAPTERS
            .iter()
            .filter(|adapter| IntegrationChoice::all().contains(adapter.spec().id))
            .count()
    );
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
    // Setup writes Ask fallbacks into the policy files.
    fs::create_dir(root.path().join(".shield")).unwrap();
    fs::write(
        root.path().join(".shield/config.yml"),
        "schema_version: 1\npacks: []\n",
    )
    .unwrap();
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

/// The Ask fallback in effect for `agent` in the project's current state.
fn fallback_in(context: &Context, agent: &str) -> Result<AskFallback, Error> {
    let state = crate::review::project::load(context).unwrap();
    fallback(
        &crate::review::project::metadata(&state),
        &state.settings,
        agent,
    )
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
fn hand_edited_mcp_connections_are_reported_and_mcp_setup_writes_them() {
    let (root, context) = cloud_fixture(false);
    let before = meta(&context);
    let plan = prepare(root.path(), &before, IntegrationChoice::all())
        .unwrap()
        .unwrap();
    apply(&context, &before, &plan).unwrap();
    let diagnostics = |context: &Context| {
        status(
            context,
            &crate::review::project::checkpoint(context).unwrap(),
        )["diagnostics"]
            .to_string()
    };
    let stale = "project MCP binding needs reviewed setup";
    assert!(!diagnostics(&context).contains(stale));
    // A person edits the connection in the policy files.
    let config = root.path().join(".shield/config.yml");
    let text = fs::read_to_string(&config).unwrap();
    assert!(text.contains("cloud.example.com"));
    fs::write(
        &config,
        text.replace("cloud.example.com", "next.example.com"),
    )
    .unwrap();
    // Status never compiles the edited files, so it cannot compare them yet.
    let report = status(
        &context,
        &crate::review::project::checkpoint(&context).unwrap(),
    );
    assert!(
        report.to_string().contains(r#""availability":"unchecked""#),
        "{report}"
    );
    assert!(!diagnostics(&context).contains(stale));
    // Once a request compiles their plan, status reports the stale binding.
    crate::review::files::load(root.path()).unwrap();
    assert!(diagnostics(&context).contains(stale));
    let before = meta(&context);
    let repair = prepare(root.path(), &before, IntegrationChoice::all())
        .unwrap()
        .unwrap();
    apply(&context, &before, &repair).unwrap();
    for agent in ["claude", "codex"] {
        crate::review::integrations::verify_mcp_connection(
            root.path(),
            agent,
            "acme",
            &crate::mcp_connections::Transport::Http {
                url: "https://next.example.com/mcp".into(),
            },
        )
        .unwrap();
    }
    assert!(!diagnostics(&context).contains(stale));
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

#[cfg(not(windows))]
#[test]
fn mcp_connections_box_setup_repairs_bindings_and_retains_handover_ownership() {
    for handover in [false, true] {
        let (root, context) = cloud_fixture(false);
        let state = crate::review::project::load(&context).unwrap();
        let policy_revision = state.policy_revision.clone();
        let report = crate::review::integrations::status(&context, &state);
        assert_eq!(
            report["diagnostics"],
            json!(["project MCP binding needs reviewed setup"])
        );
        assert_eq!(report["mcp_connections"].as_array().unwrap().len(), 2);
        fs::create_dir_all(root.path().join(".box")).unwrap();
        fs::write(root.path().join(".box/config.yml"), "schema_version: 1\n").unwrap();
        if handover {
            let before = meta(&context);
            let plan = prepare(root.path(), &before, IntegrationChoice::all())
                .unwrap()
                .unwrap();
            apply(&context, &before, &plan).unwrap();
        }
        let choice = IntegrationChoice::Agents(IntegrationChoice::bits(&["box"]).unwrap());
        for repair in [false, true] {
            if repair {
                fs::write(root.path().join(".mcp.json"), "{\"mcpServers\":{}}").unwrap();
                let doctor = crate::review::project::doctor(&context).unwrap();
                assert_eq!(doctor["repair_needed"], true, "{doctor}");
                assert_eq!(
                    doctor["repair_route"],
                    if handover {
                        "shield mcp setup"
                    } else {
                        "shield mcp setup --agent box"
                    }
                );
                let preview = workflow::review(&context, Some(choice), true, false).unwrap();
                assert!(
                    preview["files"].as_array().unwrap().iter().any(|file| {
                        file["path"] == ".mcp.json" && file["before"] != file["after"]
                    }),
                    "{preview}"
                );
            }
            let before = meta(&context);
            let plan = prepare(root.path(), &before, choice)
                .unwrap_or_else(|error| panic!("handover={handover} repair={repair}: {error}"))
                .unwrap();
            apply(&context, &before, &plan).unwrap();
            let state = crate::review::project::load(&context).unwrap();
            let report = crate::review::integrations::status(&context, &state);
            assert_eq!(report["repair_needed"], false, "{report}");
            let doctor = crate::review::project::doctor(&context).unwrap();
            assert_eq!(doctor["repair_needed"], false, "{doctor}");
            assert_eq!(doctor["checks"], json!([]), "{doctor}");
            assert_eq!(
                crate::review::project::load(&context)
                    .unwrap()
                    .policy_revision,
                policy_revision
            );
            for agent in ["claude", "codex"] {
                assert!(!meta(&context)["integration"]["connections"][agent]["entries"].is_null());
            }
        }
        let before = meta(&context);
        let plan = prepare(
            root.path(),
            &before,
            IntegrationChoice::RemoveAgents(IntegrationChoice::bits(&["box"]).unwrap()),
        )
        .unwrap()
        .unwrap();
        apply(&context, &before, &plan).unwrap();
        assert!(config(root.path())["mcpServers"].get("acme").is_none());
    }
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
    let output = crate::test_support::git()
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
fn mcp_connections_private_projection_supports_tracked_destinations_without_private_values() {
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
        // Neither tracking nor ignore patterns determine launcher privacy.
        fs::write(
            root.path().join(".gitignore"),
            ".mcp.json\n.codex/config.toml\n.claude/settings.local.json\n",
        )
        .unwrap();
        let index = fs::read(root.path().join(".git/index")).unwrap();
        let before = meta(&context);
        let plan = prepare(root.path(), &before, IntegrationChoice::all())
            .unwrap()
            .unwrap();
        let shown = crate::review::integrations::setup::preview(root.path(), Some(&plan));
        assert!(
            shown["host_path_warning"]
                .as_str()
                .unwrap()
                .contains("machine-specific")
        );
        apply(&context, &before, &plan).unwrap();
        assert_private_host_output(root.path());
        assert_eq!(fs::read(root.path().join(".git/index")).unwrap(), index);
    }
}

#[test]
fn mcp_connections_private_projection_allows_tracking_changes_during_confirmation() {
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
        let index = fs::read(root.path().join(".git/index")).unwrap();
        apply(&context, &before, &plan).unwrap();
        assert_private_host_output(root.path());
        assert_eq!(fs::read(root.path().join(".git/index")).unwrap(), index);
    }
}

#[test]
fn mcp_connections_private_projection_refuses_missing_bindings_before_writes() {
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
    assert_eq!(meta(&context), before);
}

fn assert_private_host_output(root: &Path) {
    for path in [
        ".mcp.json",
        ".codex/config.toml",
        ".claude/settings.local.json",
    ] {
        let output = fs::read_to_string(root.join(path)).unwrap();
        for private in [
            "/acme/private/bin/connector",
            "acme-personal",
            "acme-private-region",
        ] {
            assert!(!output.contains(private), "private value in {path}");
        }
    }
}

#[test]
fn mcp_connections_private_projection_needs_no_repository_or_repository_metadata() {
    for invalid_metadata in [false, true] {
        let (root, context) = private_stdio_fixture();
        fs::remove_dir_all(root.path().join(".git")).unwrap();
        if invalid_metadata {
            fs::write(root.path().join(".git"), "gitdir: /acme/missing-metadata\n").unwrap();
        }
        let before = meta(&context);
        let plan = prepare(root.path(), &before, IntegrationChoice::all())
            .unwrap()
            .unwrap();
        apply(&context, &before, &plan).unwrap();
        assert_private_host_output(root.path());
        let repair = prepare(root.path(), &meta(&context), IntegrationChoice::all())
            .unwrap()
            .unwrap();
        apply(&context, &meta(&context), &repair).unwrap();
        assert_eq!(repair.after, plan.after);
        let snapshot = crate::runtime::Project::open(root.path())
            .unwrap()
            .snapshot()
            .unwrap();
        for agent in ["claude", "codex"] {
            let expected = crate::review::integrations::mcp_projection(
                root.path(),
                agent,
                "cloud",
                &snapshot.mcp_connections()["cloud"],
            )
            .unwrap();
            crate::review::integrations::verify_mcp_connection(
                root.path(),
                agent,
                "acme",
                &expected,
            )
            .unwrap();
        }
    }
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
    assert_eq!(before.policy_revision, after.policy_revision);
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
    assert_eq!(
        first_bytes, second_bytes,
        "owned launchers are portable across project roots"
    );
    assert_ne!(
        a.project_id, b.project_id,
        "runtime enrollment identity remains isolated"
    );
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
fn installed_binary_upgrade_preserves_integration_without_rewriting_project_state() {
    let (root, context) = fixture();
    install(&context);
    Store::open(root.path())
        .unwrap()
        .update(&context, &|| Ok(()), |state| {
            state.operator["integration"]["version"] = json!("0.0.0");
            state.operator["integration"]["binary_digest"] = json!("0".repeat(64));
            Ok(json!({}))
        })
        .unwrap();
    let before = fs::read(root.path().join(".shield/state/state.json")).unwrap();
    let files = read_file(root.path()).unwrap();
    let doctor = crate::review::project::doctor(&context).unwrap();
    assert_eq!(doctor["repair_needed"], false, "{doctor}");
    assert_eq!(doctor["checks"], json!([]), "{doctor}");
    assert_eq!(
        before,
        fs::read(root.path().join(".shield/state/state.json")).unwrap()
    );
    assert_eq!(files, read_file(root.path()).unwrap());
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
    let before = fs::read(root.path().join(".shield/state/state.json")).unwrap();
    let file = read_file(root.path()).unwrap();
    let report = status(&context, &crate::review::project::load(&context).unwrap());
    assert_eq!(report["registered_integrations"][0]["status"], "degraded");
    assert_eq!(
        crate::review::integrations::enforcement(&report),
        "degraded"
    );
    assert_eq!(report["installed_integrations"][0]["status"], "degraded");
    assert_eq!(
        before,
        fs::read(root.path().join(".shield/state/state.json")).unwrap()
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
    configure_fixture(
        root.path(),
        IntegrationChoice::agent("claude"),
        AskFallback::Block,
    );
    assert_eq!(fallback_in(&context, "codex").unwrap(), AskFallback::Allow);
    let checked = PathAccess::default();
    assert!(with_fallback(root.path(), "codex", checked, |s| s == AskFallback::Allow).unwrap());
    // A request checked against access the operator has since changed is stale.
    let relaxed = PathAccess {
        agent_settings_change: Change::Allow,
        ..checked
    };
    crate::review::path_access::configure_fixture(root.path(), relaxed);
    assert!(
        with_fallback(root.path(), "codex", checked, |_| panic!(
            "handoff checked against replaced access"
        ))
        .is_err()
    );
    assert!(with_fallback(root.path(), "codex", relaxed, |_| ()).is_ok());
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
    let state_path = root.path().join(".shield/state/state.json");
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
    // The refusal only closes the proposal as stale; settings and policy stay.
    let before: Value = serde_json::from_slice(&before).unwrap();
    let after: Value = serde_json::from_slice(&fs::read(&state_path).unwrap()).unwrap();
    assert_eq!(after["operator"], before["operator"]);
    assert_eq!(after["policy_revision"], before["policy_revision"]);
    let row = after["proposals"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["proposal_id"] == proposed["data"]["proposal_id"])
        .unwrap();
    assert_eq!(row["status"], "stale");
    assert_eq!(fallback_in(&context, "codex").unwrap(), AskFallback::Block);

    // A protection change edits the policy files, so a proposal prepared
    // before it is refused and the confirmed access stays current.
    let fresh = propose("after-fallback-change");
    assert!(fresh["error"].is_null(), "{fresh}");
    let access = PathAccess {
        agent_settings_change: Change::Allow,
        ..PathAccess::default()
    };
    crate::review::path_access::configure_fixture(root.path(), access);
    assert!(matches!(
        super::super::commit_proposal_fixture(
            &context,
            fresh["data"]["proposal_id"].as_str().unwrap()
        ),
        Err(Error::StaleSnapshot)
    ));
    assert_eq!(
        crate::review::path_access::current(root.path()).unwrap(),
        access
    );
    assert_eq!(fallback_in(&context, "codex").unwrap(), AskFallback::Block);
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
    let revision = crate::review::project::load(&context)
        .unwrap()
        .policy_revision;
    let mut plan = prepare(root.path(), &before, IntegrationChoice::agent("codex"))
        .unwrap()
        .unwrap();
    plan.desired
        .as_mut()
        .unwrap()
        .ask_fallback
        .insert("codex".into(), AskFallback::Block);
    // As an interrupted setup leaves it: the plan pending and its
    // revocation already in the policy files.
    Store::open(root.path())
        .unwrap()
        .update(&context, &|| Ok(()), |state| {
            state.operator["integration_pending"] = json!(plan);
            crate::review::settings::write(
                &context,
                &crate::review::settings::fallbacks(BTreeMap::from([(
                    "codex".into(),
                    AskFallback::Block,
                )])),
            )?;
            Ok(json!({}))
        })
        .unwrap();
    assert!(
        with_fallback(root.path(), "codex", PathAccess::default(), |s| s
            == AskFallback::Block)
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
    assert_eq!(fallback_in(&context, "codex").unwrap(), AskFallback::Block);
    assert_eq!(
        crate::review::project::load(&context)
            .unwrap()
            .policy_revision,
        revision
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
            crate::review::packs::rebuild(&state.candidates["project"], &mut [], &[], Some(&rules))
                .unwrap();
        crate::review::files::write::commit_fixture(
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
            crate::review::mode::current(root.path()).unwrap(),
        )
    };

    let receipt = activate("allow");
    assert_eq!(receipt["repair_needed"], false, "{receipt}");
    let text = fs::read_to_string(&path).unwrap();
    assert!(text.starts_with("# Generated by Shield"), "{text}");
    assert!(text.contains(r#"pattern = ["acme", "logs"]"#), "{text}");
    assert!(text.contains("custom logs"), "{text}");
    assert_eq!(repair(), false);

    // Pausing clears derived host permissions while retaining registration.
    // Setup and policy publication must not restore them until resumed.
    let toggle = |enabled| {
        crate::review::mode::Prepared::new(&context, enabled)
            .unwrap()
            .commit(&context, &|| Ok(()))
            .unwrap()
    };
    assert_eq!(toggle(false)["enabled"], false);
    assert!(!path.exists());
    resync().unwrap();
    assert!(!path.exists());
    assert_eq!(activate("allow")["repair_needed"], false);
    assert!(!path.exists());
    assert_eq!(repair(), false);
    assert_eq!(toggle(true)["enabled"], true);
    assert_eq!(fs::read_to_string(&path).unwrap(), text);

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
        crate::review::packs::rebuild(&state.candidates["project"], &mut [], &[], Some(&rules))
            .unwrap()
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
    crate::review::files::write::commit_fixture(
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
        plan.desired.as_ref(),
        &prefix_rules::active(root.path()).unwrap(),
        crate::review::mode::current(root.path()).unwrap(),
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
#[test]
fn project_setup_fresh_mutation_requires_an_operator_terminal() {
    let root = assert_fs::TempDir::new().unwrap();
    let result = super::project_setup(root.path(), &["claude".into()], None, true, None);
    assert!(matches!(
        result,
        Err(Error::Invalid(
            "JSON is read-only; run this command interactively"
        ))
    ));
    assert!(!root.path().join(".shield").exists());
}
