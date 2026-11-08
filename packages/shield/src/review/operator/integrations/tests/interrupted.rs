//! Pending setup reconciles only the confirmed native fragments.
use super::*;

fn interrupt(context: &Context, plan: &Plan) {
    Store::open(Path::new(&context.project_root))
        .unwrap()
        .update(context, &|| Ok(()), |state| {
            state.operator["integration_pending"] = json!(plan);
            Ok(json!({}))
        })
        .unwrap();
}

#[test]
fn interrupted_setup_preserves_unrelated_settings_before_and_after_publication() {
    for written in [false, true] {
        let (root, context) = fixture();
        let plan = prepare(
            root.path(),
            &meta(&context),
            IntegrationChoice::agent("claude"),
        )
        .unwrap()
        .unwrap();
        interrupt(&context, &plan);
        if written {
            publish(root.path(), &plan).unwrap();
            files::publish(root.path(), &plan.files).unwrap();
        }
        let mut shared = read_file(root.path()).unwrap().map_or_else(
            || json!({}),
            |source| serde_json::from_str::<Value>(&source).unwrap(),
        );
        if shared["mcpServers"].is_null() {
            shared["mcpServers"] = json!({});
        }
        shared["mcpServers"]["acme"] = json!({"command":"acme-server"});
        shared["unrelated"] = json!({"enabled":true});
        fs::write(root.path().join(".mcp.json"), shared.to_string()).unwrap();
        let settings_path = root.path().join(".claude/settings.local.json");
        fs::create_dir_all(settings_path.parent().unwrap()).unwrap();
        let mut settings = fs::read_to_string(&settings_path).ok().map_or_else(
            || json!({}),
            |source| serde_json::from_str::<Value>(&source).unwrap(),
        );
        settings["env"] = json!({"ACME_SETTING":"preserved"});
        if settings["hooks"].is_null() {
            settings["hooks"] = json!({});
        }
        if settings["hooks"]["PreToolUse"].is_null() {
            settings["hooks"]["PreToolUse"] = json!([]);
        }
        let foreign = json!({"matcher":"Read","hooks":[{"type":"command","command":"acme-hook"}]});
        settings["hooks"]["PreToolUse"]
            .as_array_mut()
            .unwrap()
            .push(foreign.clone());
        let shared_callback = json!({"type":"command","command":"acme-shared-hook"});
        if written {
            settings["hooks"]["PreToolUse"][0]["hooks"]
                .as_array_mut()
                .unwrap()
                .push(shared_callback.clone());
        }
        fs::write(&settings_path, settings.to_string()).unwrap();
        let current = meta(&context);
        let resumed = prepare(root.path(), &current, IntegrationChoice::Keep)
            .unwrap()
            .unwrap();
        apply(&context, &current, &resumed).unwrap();
        assert!(pending(&meta(&context)).unwrap().is_none());
        assert_eq!(
            config(root.path())["mcpServers"]["acme"],
            shared["mcpServers"]["acme"]
        );
        assert_eq!(config(root.path())["unrelated"], shared["unrelated"]);
        let settings: Value =
            serde_json::from_str(&fs::read_to_string(settings_path).unwrap()).unwrap();
        assert_eq!(settings["env"], json!({"ACME_SETTING":"preserved"}));
        assert!(
            settings["hooks"]["PreToolUse"]
                .as_array()
                .unwrap()
                .contains(&foreign)
        );
        if written {
            assert!(
                settings["hooks"]["PreToolUse"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|group| group["hooks"]
                        .as_array()
                        .is_some_and(|rows| rows.contains(&shared_callback)))
            );
        }
        assert_eq!(
            status(&context, &crate::review::project::load(&context).unwrap())["repair_needed"],
            false
        );
    }
}

#[test]
fn interrupted_setup_conflict_keeps_every_file_and_pending_review() {
    let (root, context) = fixture();
    let plan = prepare(
        root.path(),
        &meta(&context),
        IntegrationChoice::agent("claude"),
    )
    .unwrap()
    .unwrap();
    interrupt(&context, &plan);
    fs::write(
        root.path().join(".mcp.json"),
        r#"{"mcpServers":{"shield":{"command":"acme-custom"}}}"#,
    )
    .unwrap();
    let current = meta(&context);
    let before = read_file(root.path()).unwrap();
    assert!(prepare(root.path(), &current, IntegrationChoice::Keep).is_err());
    assert!(apply(&context, &current, &plan).is_err());
    assert_eq!(read_file(root.path()).unwrap(), before);
    assert_eq!(meta(&context), current);
    assert!(!root.path().join(".claude/settings.local.json").exists());
}

#[test]
fn interrupted_setup_json_mutation_refuses_before_preparing_recovery() {
    let (_root, context) = fixture();
    assert!(matches!(
        recover(
            &context,
            crate::review::integrations::recovery::Action::Cancel,
            true,
            false
        ),
        Err(Error::ApprovalRequired)
    ));
}
