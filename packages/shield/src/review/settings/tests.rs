use crate::review::storage::Store;
use assert_fs::prelude::*;
use serde_json::{Value, json};

fn project() -> (assert_fs::TempDir, crate::review::Context) {
    let (root, context) = crate::review::tests::enrolled();
    root.child(".shield/config.yml")
        .write_str("schema_version: 1\npacks: []\n")
        .expect("config");
    (root, context)
}

/// An enrolled folder without policy files.
fn without_files() -> (assert_fs::TempDir, crate::review::Context) {
    let root = assert_fs::TempDir::new().unwrap();
    let bundle =
        crate::configuration::Bundle::capture("schema_version: 1\npacks: []\n", &[], None).unwrap();
    let context = crate::review::mcp::initialize_fixture(root.path(), &bundle);
    std::fs::remove_file(root.path().join(".shield/config.yml")).unwrap();
    (root, context)
}

fn stored(root: &assert_fs::TempDir) -> Value {
    serde_json::from_slice(&std::fs::read(root.path().join(".shield/state/state.json")).unwrap())
        .unwrap()
}

fn local(root: &assert_fs::TempDir) -> String {
    std::fs::read_to_string(root.path().join(".shield/config.local.yml")).unwrap_or_default()
}

fn write(context: &crate::review::Context, change: impl FnOnce(&mut crate::review::model::State)) {
    Store::open(std::path::Path::new(&context.project_root))
        .unwrap()
        .update_checkpoint(context, &|| Ok(()), |state| {
            change(state);
            Ok(json!({}))
        })
        .unwrap();
}

#[test]
fn an_edit_to_a_setting_applies_to_the_next_read() {
    let (root, _) = project();
    assert!(crate::review::mode::current(root.path()).unwrap());
    root.child(".shield/config.local.yml")
        .write_str("schema_version: 1\nenforcement: paused\n")
        .unwrap();
    assert!(!crate::review::mode::current(root.path()).unwrap());
    root.child(".shield/config.yml")
        .write_str("schema_version: 1\npacks: []\nhistory: {limit: 42}\n")
        .unwrap();
    let state = Store::open(root.path()).unwrap().read_checkpoint().unwrap();
    assert_eq!(state.settings.history_limit, 42);
    assert!(!state.settings.enabled());
}

#[test]
fn repeated_settings_reads_follow_replacements_invalid_files_and_layer_removal() {
    let (root, _) = project();
    let shared = root.child(".shield/config.yml");
    let private = root.child(".shield/config.local.yml");
    shared
        .write_str("schema_version: 1\npacks: []\nhistory: {limit: 33}\n")
        .unwrap();
    for limit in [77, 88, 77] {
        private
            .write_str(&format!("schema_version: 1\nhistory: {{limit: {limit}}}\n"))
            .unwrap();
        for _ in 0..2 {
            assert_eq!(
                super::read(root.path())
                    .unwrap()
                    .unwrap()
                    .values()
                    .history_limit,
                limit
            );
        }
    }
    private
        .write_str("schema_version: 1\nenforcement: invalid\n")
        .unwrap();
    assert!(super::read(root.path()).is_err());
    std::fs::remove_file(private.path()).unwrap();
    assert_eq!(
        super::read(root.path())
            .unwrap()
            .unwrap()
            .values()
            .history_limit,
        33
    );
    shared
        .write_str("schema_version: 1\npacks: []\nhistory: {limit: 44}\n")
        .unwrap();
    assert_eq!(
        super::read(root.path())
            .unwrap()
            .unwrap()
            .values()
            .history_limit,
        44
    );
    std::fs::remove_file(shared.path()).unwrap();
    assert!(super::read(root.path()).unwrap().is_none());
}

#[test]
fn unreadable_settings_keep_enforcement_on() {
    let (root, _) = project();
    root.child(".shield/config.local.yml")
        .write_str("schema_version: 1\nenforcement: paused\n")
        .unwrap();
    assert!(!crate::review::mode::current(root.path()).unwrap());
    root.child(".shield/config.yml")
        .write_str("schema_version: 1\npacks: []\nenforcement: off\n")
        .unwrap();
    assert!(crate::review::mode::current(root.path()).unwrap());
}

#[test]
fn settings_of_an_earlier_checkpoint_move_into_the_local_file() {
    let (root, _) = project();
    let path = root.path().join(".shield/state/state.json");
    let mut raw = stored(&root);
    raw["history_enabled"] = json!(false);
    raw["history_limit"] = json!(300);
    raw["enforcement"] = json!(crate::review::mode::Mode::with(false));
    raw["operator"]["path_access"] = json!({"agent_settings_read":"allow"});
    std::fs::write(&path, raw.to_string()).unwrap();
    // Diagnostics apply them without writing anything.
    let (state, _) = Store::open(root.path())
        .unwrap()
        .read_diagnostic_checkpoint()
        .unwrap();
    assert!(!state.settings.history_enabled);
    assert_eq!(state.settings.history_limit, 300);
    assert!(!state.settings.enabled());
    assert_eq!(local(&root), "");

    // The first runtime read moves them, before any caller builds on them.
    let state = Store::open(root.path()).unwrap().read_checkpoint().unwrap();
    assert!(!state.settings.history_enabled);
    let text = local(&root);
    for written in [
        "enabled: false",
        "limit: 300",
        "enforcement: paused",
        "agent_settings_read: allow",
    ] {
        assert!(text.contains(written), "{written}: {text}");
    }
    // The checkpoint no longer holds them.
    let raw = stored(&root);
    for key in ["history_enabled", "history_limit", "enforcement"] {
        assert!(raw.get(key).is_none(), "{key}: {raw}");
    }
    assert!(raw["operator"].get("path_access").is_none(), "{raw}");
    let state = Store::open(root.path()).unwrap().read_checkpoint().unwrap();
    assert!(!state.settings.history_enabled);
    assert_eq!(state.settings.history_limit, 300);
    assert_eq!(
        crate::review::path_access::current(root.path())
            .unwrap()
            .agent_settings_read,
        crate::review::path_access::Read::Allow
    );
}

#[test]
fn settings_of_an_earlier_checkpoint_stay_in_effect_without_policy_files() {
    let (root, context) = without_files();
    let path = root.path().join(".shield/state/state.json");
    let mut raw = stored(&root);
    raw["history_limit"] = json!(300);
    std::fs::write(&path, raw.to_string()).unwrap();
    // A write keeps them where they were until the folder has policy files.
    write(&context, |_| {});
    assert_eq!(stored(&root)["history_limit"], 300);
    assert!(!root.path().join(".shield/config.local.yml").exists());
    let state = Store::open(root.path()).unwrap().read_checkpoint().unwrap();
    assert_eq!(state.settings.history_limit, 300);
}

#[test]
fn a_settings_command_without_policy_files_creates_nothing() {
    let (root, context) = without_files();
    let refused = super::write(&context, &super::history(true, 250)).unwrap_err();
    assert!(refused.to_string().contains("no policy files"), "{refused}");
    assert!(!root.path().join(".shield/config.local.yml").exists());
    assert!(!root.path().join(".shield/config.yml").exists());
}

#[test]
fn a_changed_setting_is_written_into_the_local_file_keeping_its_text() {
    let (root, context) = project();
    root.child(".shield/config.local.yml")
        .write_str("# Acme local settings\nschema_version: 1\nhistory:\n  enabled: true # keep\n")
        .unwrap();
    super::write(&context, &super::history(true, 250)).unwrap();
    let text = local(&root);
    assert!(
        text.starts_with("# Acme local settings\n") && text.contains("enabled: true # keep"),
        "{text}"
    );
    assert!(text.contains("limit: 250"), "{text}");
    assert!(stored(&root).get("history_limit").is_none());
    let state = Store::open(root.path()).unwrap().read_checkpoint().unwrap();
    assert_eq!(state.settings.history_limit, 250);
}

#[test]
fn a_writer_keeps_an_edit_it_did_not_make() {
    let (root, context) = project();
    write(&context, |_| {
        // A person edits the file while another writer holds the state.
        std::fs::write(
            root.path().join(".shield/config.local.yml"),
            "schema_version: 1\nhistory: {limit: 77}\n",
        )
        .unwrap();
    });
    assert_eq!(local(&root), "schema_version: 1\nhistory: {limit: 77}\n");
    let state = Store::open(root.path()).unwrap().read_checkpoint().unwrap();
    assert_eq!(state.settings.history_limit, 77);
}

#[test]
fn removing_a_key_returns_the_setting_to_its_default() {
    let (root, context) = project();
    root.child(".shield/config.local.yml")
        .write_str("schema_version: 1\nhistory: {limit: 77}\n")
        .unwrap();
    write(&context, |_| {});
    root.child(".shield/config.local.yml")
        .write_str("schema_version: 1\n")
        .unwrap();
    let state = Store::open(root.path()).unwrap().read_checkpoint().unwrap();
    assert_eq!(
        state.settings.history_limit,
        crate::configuration::settings::DEFAULT_HISTORY_LIMIT
    );
}

#[test]
fn registries_of_an_earlier_checkpoint_move_into_the_local_file() {
    let (root, _) = project();
    let registry = assert_fs::TempDir::new().unwrap();
    let location = registry.path().canonicalize().unwrap();
    let path = root.path().join(".shield/state/state.json");
    let mut raw = stored(&root);
    raw["operator"]["sources"] = json!({"acme":{
        "identity":crate::policy::fingerprint(location.to_str().unwrap().as_bytes()),
        "location":location,"key":null}});
    raw["operator"]["default"] = json!("acme");
    std::fs::write(&path, raw.to_string()).unwrap();
    let state = Store::open(root.path()).unwrap().read_checkpoint().unwrap();
    assert!(
        state.settings.registries.sources.contains_key("acme"),
        "{}",
        local(&root)
    );
    assert_eq!(state.settings.registries.default.as_deref(), Some("acme"));
    let text = local(&root);
    assert!(
        text.contains("registries:") && text.contains("acme"),
        "{text}"
    );
    let raw = stored(&root);
    assert!(raw["operator"].get("sources").is_none(), "{raw}");
    let state = Store::open(root.path()).unwrap().read_checkpoint().unwrap();
    assert!(state.settings.registries.sources.contains_key("acme"));
}
