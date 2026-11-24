use super::*;

#[test]
fn status_reporting_published_hooks_inspect_loading_without_claiming_mcp_permissions() {
    for agent in ["claude", "codex"] {
        let adapter = crate::agents::find(agent).unwrap();
        let mut diagnostics = Vec::new();
        adapter
            .diagnose_published_hooks(None, &BTreeMap::new(), None, &mut diagnostics)
            .unwrap();
        assert!(diagnostics.is_empty(), "{agent}: {diagnostics:?}");
        let (hooks, extra) = if agent == "claude" {
            (Some("{\"disableAllHooks\":true}"), None)
        } else {
            (None, Some("[features]\nhooks = false\n"))
        };
        adapter
            .diagnose_published_hooks(hooks, &BTreeMap::new(), extra, &mut diagnostics)
            .unwrap();
        assert_eq!(diagnostics.len(), 1, "{agent}: {diagnostics:?}");
        assert!(diagnostics[0].contains("disabled"));
    }
}

#[test]
fn status_reporting_keeps_activity_unchecked_until_history_is_explicitly_inspected() {
    let (root, context) = fixture();
    let report = crate::review::project::status(&context).unwrap();
    assert_eq!(report["enforcement"], "not-installed");
    install(&context);
    let mut state = crate::review::project::load(&context).unwrap();
    let report = crate::review::project::status_from(&context, &state);
    assert_eq!(report["enforcement"], "installed");
    state.settings.enforcement = crate::configuration::settings::Enforcement::Paused;
    let disabled = crate::review::project::status_from(&context, &state);
    assert_eq!(disabled["enabled"], false);
    assert_eq!(disabled["enforcement"], "disabled");
    assert_ne!(
        disabled["coverage"]["installed_integrations"]
            .as_array()
            .unwrap()
            .as_slice(),
        &[] as &[Value]
    );
    state.settings.enforcement = crate::configuration::settings::Enforcement::Enabled;
    assert_eq!(report["coverage"]["live_host_verification"], "unchecked");
    assert!(report["history"]["available"].is_null());
    let event = |surface: &str, project: &str, at: &str| {
        json!({
        "kind":"decision","recorded_at":at,"event_id":"acme-event",
        "provenance":{"surface":surface,"project_id":project,"agent":"claude","host_event_name":"PreToolUse"}})
    };
    state.events = vec![
        event("hook", &state.project_id, "2026-11-01T10:00:00Z"),
        event("hook", &state.project_id, "2026-11-02T10:00:00Z"),
        event("mcp", &state.project_id, "2026-11-03T10:00:00Z"),
        event("hook", "another-project", "2026-11-04T10:00:00Z"),
    ];
    let report = crate::review::project::status_from(&context, &state);
    assert_eq!(report["coverage"]["live_host_verification"], "unchecked");
    assert_eq!(report["coverage"]["verification"]["agents"], json!([]));
    // An explicitly supplied history view retains its existing attribution;
    // the metadata-only status report never interprets it implicitly.
    let evidence = crate::review::integrations::status(&context, &state);
    assert_eq!(evidence["live_host_verification"], "observed");
    assert_eq!(
        evidence["verification"]["agents"].as_array().unwrap().len(),
        1
    );
    assert_eq!(
        evidence["verification"]["agents"][0]["recorded_at"],
        "2026-11-02T10:00:00Z"
    );
    state.history_available = false;
    let report = crate::review::project::status_from(&context, &state);
    assert_eq!(report["coverage"]["live_host_verification"], "unchecked");
    let evidence = crate::review::integrations::status(&context, &state);
    assert_eq!(evidence["live_host_verification"], "unverified");
    // Retained activity cannot hide a currently missing configuration.
    fs::remove_file(root.path().join(".claude/settings.local.json")).unwrap();
    let report = crate::review::project::status_from(&context, &state);
    assert_eq!(report["enforcement"], "degraded");
}

#[cfg(not(windows))]
#[test]
fn status_reporting_inspects_box_hooks_and_detects_foreign_duplicates() {
    let (root, context) = fixture();
    fs::create_dir_all(root.path().join(".box")).unwrap();
    fs::write(root.path().join(".box/config.yml"), "schema_version: 1\n").unwrap();
    let before = meta(&context);
    let plan = prepare(root.path(), &before, IntegrationChoice::agent("box"))
        .unwrap()
        .unwrap();
    apply(&context, &before, &plan).unwrap();
    let report = crate::review::project::status(&context).unwrap();
    assert_eq!(report["enforcement"], "defined-for-box", "{report}");
    assert!(
        crate::review::integrations::registration(&meta(&context))
            .unwrap()
            .is_some()
    );
    let adapter = crate::agents::find("claude").unwrap();
    let groups = adapter
        .spec()
        .events
        .iter()
        .map(|(event, _)| {
            let command = crate::review::integrations::native::hook_command(
                Path::new(&context.project_root),
                "claude",
                event,
            )
            .unwrap();
            ((*event).into(), adapter.hook_group(&command))
        })
        .collect();
    let hooks = crate::agents::setup::merge(None, &BTreeMap::default(), &groups).unwrap();
    let hooks = adapter
        .merge_settings(
            Some(&hooks),
            None,
            Some(&crate::review::mcp::default_approved()),
        )
        .unwrap()
        .source;
    fs::create_dir_all(root.path().join(".claude")).unwrap();
    fs::write(root.path().join(".claude/settings.local.json"), &hooks).unwrap();
    let report = crate::review::project::status(&context).unwrap();
    assert_eq!(report["enforcement"], "installed", "{report}");
    assert_eq!(
        report["coverage"]["installed_integrations"][0]["managed_by"],
        "box"
    );
    let mut hooks: Value = serde_json::from_str(&hooks).unwrap();
    hooks["disableAllHooks"] = json!(true);
    fs::write(
        root.path().join(".claude/settings.local.json"),
        hooks.to_string(),
    )
    .unwrap();
    let report = crate::review::project::status(&context).unwrap();
    assert_eq!(report["enforcement"], "degraded", "{report}");
    hooks.as_object_mut().unwrap().remove("disableAllHooks");
    let mut duplicate = hooks["hooks"]["PreToolUse"][0].clone();
    duplicate["hooks"][0]["command"] = json!(
        "'shield' __internal native-hook --project '/srv/acme/nested' --agent claude --event PreToolUse || exit 2"
    );
    hooks["hooks"]["PreToolUse"]
        .as_array_mut()
        .unwrap()
        .push(duplicate);
    fs::write(
        root.path().join(".claude/settings.local.json"),
        hooks.to_string(),
    )
    .unwrap();
    let report = crate::review::project::status(&context).unwrap();
    assert_eq!(report["enforcement"], "degraded", "{report}");
    assert_eq!(
        report["coverage"]["installed_integrations"][0]["status"],
        "degraded"
    );
}
