use super::*;

fn layer(yaml: &str) -> Layer {
    crate::policy::parse(yaml).expect("settings layer")
}

#[test]
fn the_local_layer_wins_leaf_by_leaf() {
    let shared = layer(
        "schema_version: 1\npacks: []\nenforcement: paused\nprotection: {agent_settings_read: allow}\nhistory: {enabled: false, limit: 200}\nask_fallback: {codex: allow, opencode: allow}\n",
    );
    let local = layer(
        "schema_version: 1\nenforcement: enabled\nprotection: {agent_settings_change: allow}\nhistory: {limit: 300}\nask_fallback: {codex: block}\n",
    );
    let values = shared.under(&local).values();
    assert_eq!(values.enforcement, Enforcement::Enabled);
    assert_eq!(values.agent_settings_read, Read::Allow);
    assert_eq!(values.agent_settings_change, Change::Allow);
    assert!(!values.history_enabled);
    assert_eq!(values.history_limit, 300);
    assert_eq!(values.ask_fallback["codex"], Fallback::Block);
    assert_eq!(values.ask_fallback["opencode"], Fallback::Allow);
}

#[test]
fn absent_keys_take_their_defaults() {
    let values = layer("schema_version: 1\npacks: []\n").values();
    assert_eq!(values.enforcement, Enforcement::Enabled);
    assert_eq!(values.agent_settings_read, Read::Ask);
    assert_eq!(values.agent_settings_change, Change::Block);
    assert!(values.history_enabled);
    assert_eq!(values.history_limit, DEFAULT_HISTORY_LIMIT);
    assert!(values.registries.sources.is_empty());
}

#[test]
fn local_registries_replace_the_shared_ones_whole() {
    let source = |name: &str| {
        canonic_registry_kit::source::Source::local(std::path::Path::new(
            crate::test_support::absolute(&format!("/srv/{name}")).as_ref(),
        ))
        .expect("source")
    };
    let registries = |names: &[&str], default: Option<&str>| Layer {
        registries: Some(Registrations {
            sources: names.iter().map(|n| ((*n).to_owned(), source(n))).collect(),
            default: default.map(str::to_owned),
        }),
        ..Layer::default()
    };
    let shared = registries(&["acme", "beta"], Some("acme"));
    let local = registries(&["acme"], None);
    let registries = shared.under(&local).values().registries;
    assert_eq!(registries.sources.keys().collect::<Vec<_>>(), ["acme"]);
    assert_eq!(registries.default, None);
}

#[test]
fn changes_name_only_the_settings_that_differ() {
    let files = layer("enforcement: paused\nhistory: {limit: 200}\n");
    let mut target = files.values();
    assert!(files.changes_to(&target).is_empty());
    target.enforcement = Enforcement::Enabled;
    target.agent_settings_read = Read::Allow;
    target.ask_fallback.insert("codex".into(), Fallback::Block);
    let changes = files.changes_to(&target);
    assert_eq!(changes.enforcement, Some(Enforcement::Enabled));
    assert_eq!(
        changes.protection,
        Some(Protection {
            agent_settings_read: Some(Read::Allow),
            agent_settings_change: None,
        })
    );
    // A fallback the target names is written even when it is the default,
    // so a confirmed Block stays confirmed.
    assert_eq!(changes.ask_fallback.get("codex"), Some(&Fallback::Block));
    assert_eq!(changes.history, None);
    assert_eq!(files.under(&changes).values(), target);
    assert!(files.under(&changes).changes_to(&target).is_empty());
}

#[test]
fn invalid_values_are_refused() {
    assert!(layer("history: {limit: 0}\n").validate().is_err());
    assert!(layer("history: {limit: 10001}\n").validate().is_err());
    assert!(layer("ask_fallback: {Codex: allow}\n").validate().is_err());
    assert!(crate::policy::parse::<Layer>("enforcement: off\n").is_err());
    assert!(crate::policy::parse::<Layer>("protection: {agent_settings_read: maybe}\n").is_err());
    assert!(crate::policy::parse::<Layer>("history: {keep: 1}\n").is_err());
}

#[test]
fn configuration_files_accept_settings_in_either_layer() {
    let shared = "schema_version: 1\npacks: []\nenforcement: paused\nhistory: {limit: 200}\n";
    let local = "schema_version: 1\nprotection: {agent_settings_read: allow}\nask_fallback: {codex: allow}\n";
    let merged = crate::configuration::layers::merge(shared, Some(local)).expect("merge");
    crate::configuration::Candidate::from_yaml(&merged).expect("valid configuration");
    let (shared_after, local_after) =
        crate::configuration::layers::split(&merged, (Some(shared), Some(local))).expect("split");
    assert_eq!(layer(&shared_after), layer(shared));
    assert_eq!(layer(&local_after.expect("local layer")), layer(local));
    let invalid = "schema_version: 1\npacks: []\nhistory: {limit: 0}\n";
    assert!(crate::configuration::Candidate::from_yaml(invalid).is_err());
}

#[test]
fn settings_are_not_part_of_the_policy_revision() {
    let revision = |shared: &str, local: Option<&str>| {
        let merged = crate::configuration::layers::merge(shared, local).unwrap();
        let candidate = crate::configuration::Candidate::from_yaml(&merged).unwrap();
        candidate.digest
    };
    let plain = "schema_version: 1\npacks: []\n";
    // Files without settings keep the digest of their bytes.
    assert_eq!(
        revision(plain, None),
        crate::policy::fingerprint(plain.as_bytes())
    );
    let paused = revision(plain, Some("schema_version: 1\nenforcement: paused\n"));
    let enabled = revision(plain, Some("schema_version: 1\nenforcement: enabled\n"));
    let history = revision(
        plain,
        Some("schema_version: 1\nhistory: {limit: 9}\nprotection: {agent_settings_read: allow}\n"),
    );
    assert_eq!(paused, enabled);
    assert_eq!(paused, history);
    // A local file of settings alone is no policy change at all.
    assert_eq!(paused, revision(plain, None));
    let shared = revision("schema_version: 1\npacks: []\nenforcement: paused\n", None);
    let shared_enabled = revision("schema_version: 1\npacks: []\nenforcement: enabled\n", None);
    assert_eq!(shared, shared_enabled);
}

#[test]
fn a_settings_only_local_file_survives_a_shared_change() {
    let shared = "schema_version: 1\npacks: []\n";
    let local = "schema_version: 1\nenforcement: paused\nask_fallback: {codex: allow}\n";
    let merged = crate::configuration::layers::merge(shared, Some(local)).expect("merge");
    // A change to the shared document, as a pack or rule edit makes.
    let mut edited: Value = crate::policy::parse(&merged).expect("document");
    edited["rules"] = json!("rules.yml");
    let (_, local_after) =
        crate::configuration::layers::split(&edited.to_string(), (Some(shared), Some(local)))
            .expect("split");
    assert_eq!(layer(&local_after.expect("local layer")), layer(local));
}

#[test]
fn set_into_keeps_other_keys_and_leaves() {
    let mut document = serde_json::json!({"schema_version":1,"packs":[],"protection":{"agent_settings_change":"allow"}});
    let fields = document.as_object_mut().expect("mapping");
    set_into(
        fields,
        &Layer {
            protection: Some(Protection {
                agent_settings_read: Some(Read::Allow),
                agent_settings_change: None,
            }),
            enforcement: Some(Enforcement::Paused),
            ..Layer::default()
        },
    );
    assert_eq!(
        document,
        serde_json::json!({"schema_version":1,"packs":[],"enforcement":"paused",
            "protection":{"agent_settings_change":"allow","agent_settings_read":"allow"}})
    );
}
