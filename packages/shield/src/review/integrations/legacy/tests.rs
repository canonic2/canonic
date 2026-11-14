//! Retained SDK interceptor shapes and ownership paths from historical releases.
//! Fixture-owned module payloads test byte ownership, not the vendor mod loader.
use super::*;
use crate::review::{self, integrations, storage::Store};
use serde_json::json;
use std::fs;

fn fixture() -> (assert_fs::TempDir, review::Context, Value) {
    let root = assert_fs::TempDir::new().unwrap();
    let bundle =
        crate::configuration::Bundle::capture("schema_version: 1\npacks: []\n", &[], None).unwrap();
    let context = review::mcp::initialize_fixture(root.path(), &bundle);
    let binary = std::env::current_exe().unwrap().canonicalize().unwrap();
    let mut assets: BTreeMap<String, String> = ASSETS
        .into_iter()
        .map(|name| (name.into(), format!("Acme retained module: {name}\n")))
        .collect();
    assets.insert(ASSETS[0].into(), format!("{}\n", json!({"name":"shield","version":"0.1.0","description":"Shield bounded text-read protection","defaultEnabled":true})));
    assets.insert(
        ASSETS[1].into(),
        "{\"modules\":[\"./register.mjs\"]}\n".into(),
    );
    assets.insert(
        ASSETS[3].into(),
        format!(
            "export const binary = {};\nexport const project = {};\n",
            json!(binary),
            json!(context.project_root)
        ),
    );
    let record = json!({"binary":binary,"binary_digest":integrations::binary_digest(&binary).unwrap(),
        "version":"0.1.0","entry":{"type":"stdio","command":binary,"args":["__internal","mcp","--project",context.project_root,"--agent","claude-code"]},
        "interceptor":{"assets":assets}});
    (root, context, record)
}

fn metadata(context: &review::Context) -> Value {
    review::project::metadata(&review::project::checkpoint(context).unwrap())
}

fn install(context: &review::Context, record: &Value) {
    let root = Path::new(&context.project_root);
    for (name, bytes) in record["interceptor"]["assets"].as_object().unwrap() {
        let path = root.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes.as_str().unwrap()).unwrap();
    }
    fs::write(
        root.join(SETTINGS),
        json!({"theme":"acme","enabledPlugins":{KEY:true,"acme@library":true}}).to_string(),
    )
    .unwrap();
    fs::write(
        root.join(".mcp.json"),
        json!({"mcpServers":{"shield":record["entry"],"acme":{"command":"acme-server"}}})
            .to_string(),
    )
    .unwrap();
    Store::open(root)
        .unwrap()
        .update(context, &|| Ok(()), |state| {
            state.operator["integration"] = record.clone();
            Ok(json!({}))
        })
        .unwrap();
}

#[test]
fn historical_interceptor_codec_preserves_ownership_without_native_hooks() {
    let (_root, context, record) = fixture();
    install(&context, &record);
    let before =
        fs::read(Path::new(&context.project_root).join(".shield/state/state.json")).unwrap();
    let registered = integrations::registration(&metadata(&context))
        .unwrap()
        .unwrap();
    assert!(registered.agents.is_empty());
    assert!(registered.covers("claude"));
    assert!(!registered.native_covers("claude"));
    assert!(!registered.covers("codex"));
    assert!(registered.ask_fallback.is_empty());
    let input = crate::cli::ReviewContext {
        project: context.project_root.clone().into(),
        agent: Some("claude-code".into()),
        session_id: None,
        tool_use_id: None,
    };
    assert_eq!(
        review::cli::context(&input).unwrap().agent.as_deref(),
        Some("claude")
    );
    assert_eq!(
        fs::read(Path::new(&context.project_root).join(".shield/state/state.json")).unwrap(),
        before
    );
    let coverage = integrations::status(&context, &review::project::checkpoint(&context).unwrap());
    assert_eq!(
        coverage["legacy_interceptor"]["status"], "installed",
        "{coverage}"
    );
    assert_eq!(coverage["installed_integrations"], json!([]));
}

#[test]
fn historical_interceptor_folder_stays_protected_when_checkpoint_is_corrupt() {
    let (root, context, record) = fixture();
    install(&context, &record);
    let checkpoint = root.path().join(".shield/state/state.json");
    fs::write(&checkpoint, b"{corrupt").unwrap();
    assert!(integrations::protects_legacy_path(
        &root.path().join(BASE).join("register.mjs")
    ));
    assert_eq!(fs::read(checkpoint).unwrap(), b"{corrupt");
}

#[test]
fn historical_interceptor_folder_protection_requires_ownership_at_its_own_root() {
    let (root, context, record) = fixture();
    let path = root.path().join(BASE).join("SKILL.md");
    assert!(!integrations::protects_legacy_path(&path));
    install(&context, &record);
    assert!(integrations::protects_legacy_path(&path));
    assert!(integrations::protects_legacy_path(&root.path().join(BASE)));
    assert!(!integrations::protects_legacy_path(
        &root.path().join("demo").join(BASE).join("SKILL.md")
    ));
    assert!(!integrations::protects_legacy_path(
        &root.path().join(".claude/skills/shield-example/SKILL.md")
    ));
    Store::open(root.path())
        .unwrap()
        .update(&context, &|| Ok(()), |state| {
            state
                .operator
                .as_object_mut()
                .unwrap()
                .remove("integration");
            Ok(json!({}))
        })
        .unwrap();
    // Files still matching a registration this user reviewed could register
    // this folder again; removing the owned files ends the protection.
    fs::remove_dir_all(root.path().join(BASE)).unwrap();
    assert!(!integrations::protects_legacy_path(&path));
}

#[test]
fn historical_interceptor_codex_setup_after_binary_move_preserves_claude() {
    let (_root, context, mut record) = fixture();
    record["binary"] = json!(Path::new(&context.project_root).join("previous-shield"));
    record["entry"]["command"] = record["binary"].clone();
    record["interceptor"]["assets"][ASSETS[3]] = json!(format!(
        "export const binary = {};\nexport const project = {};\n",
        record["binary"],
        json!(context.project_root)
    ));
    install(&context, &record);
    let root = Path::new(&context.project_root);
    let before = metadata(&context);
    let plan = integrations::setup::prepare(
        root,
        &before,
        integrations::IntegrationChoice::agent("codex"),
    )
    .unwrap()
    .unwrap();
    let desired = plan.desired.as_ref().unwrap();
    assert!(desired.interceptor.is_some());
    assert!(!desired.native_covers("claude"));
    assert!(plan.scope_reason.is_some());
    assert!(
        plan.files
            .iter()
            .all(|file| !asset_path(&file.path) && file.path != SETTINGS)
    );
    integrations::apply(&context, &before, &plan).unwrap();
    for (name, bytes) in record["interceptor"]["assets"].as_object().unwrap() {
        assert_eq!(
            fs::read_to_string(root.join(name)).unwrap(),
            bytes.as_str().unwrap()
        );
    }
}

#[test]
fn historical_interceptor_setup_preserves_unselected_assets_and_removes_only_owned_files() {
    let (_root, context, record) = fixture();
    install(&context, &record);
    let root = Path::new(&context.project_root);
    let before = metadata(&context);
    let plan = integrations::setup::prepare(
        root,
        &before,
        integrations::IntegrationChoice::agent("codex"),
    )
    .unwrap()
    .unwrap();
    assert!(plan.desired.as_ref().unwrap().interceptor.is_some());
    assert!(!plan.desired.as_ref().unwrap().native_covers("claude"));
    assert!(plan.files.iter().all(|file| !asset_path(&file.path)));
    integrations::apply(&context, &before, &plan).unwrap();
    fs::write(root.join(BASE).join("notes.txt"), "Acme user file").unwrap();
    let before = metadata(&context);
    let remove = integrations::IntegrationChoice::RemoveAgents(
        integrations::IntegrationChoice::bits(&["claude"]).unwrap(),
    );
    let plan = integrations::setup::prepare(root, &before, remove)
        .unwrap()
        .unwrap();
    assert!(plan.desired.as_ref().unwrap().interceptor.is_none());
    integrations::apply(&context, &before, &plan).unwrap();
    for asset in ASSETS {
        assert!(!root.join(asset).exists());
    }
    assert_eq!(
        fs::read_to_string(root.join(BASE).join("notes.txt")).unwrap(),
        "Acme user file"
    );
    let settings: Value = serde_json::from_slice(&fs::read(root.join(SETTINGS)).unwrap()).unwrap();
    assert_eq!(settings["enabledPlugins"]["acme@library"], true);
    assert_eq!(settings["theme"], "acme");
    assert!(settings["enabledPlugins"][KEY].is_null());
    assert!(
        integrations::registration(&metadata(&context))
            .unwrap()
            .unwrap()
            .agents
            .contains_key("codex")
    );
}

#[test]
fn historical_interceptor_replacement_is_reviewed_and_keeps_unrelated_settings() {
    let (_root, context, record) = fixture();
    install(&context, &record);
    let root = Path::new(&context.project_root);
    let before = metadata(&context);
    let plan = integrations::setup::prepare(
        root,
        &before,
        integrations::IntegrationChoice::agent("claude"),
    )
    .unwrap()
    .unwrap();
    assert!(plan.desired.as_ref().unwrap().interceptor.is_none());
    assert!(plan.desired.as_ref().unwrap().agents.contains_key("claude"));
    assert!(ASSETS.iter().all(|asset| root.join(asset).exists()));
    integrations::apply(&context, &before, &plan).unwrap();
    assert!(ASSETS.iter().all(|asset| !root.join(asset).exists()));
    let settings: Value = serde_json::from_slice(&fs::read(root.join(SETTINGS)).unwrap()).unwrap();
    assert_eq!(settings["theme"], "acme");
    assert_eq!(settings["enabledPlugins"]["acme@library"], true);
    assert!(settings["hooks"]["PreToolUse"].is_array());
}

#[test]
fn interrupted_setup_edited_legacy_asset_refuses_impossible_review_but_keeps_cancel_reachable() {
    let (root, context, record) = fixture();
    install(&context, &record);
    let before = metadata(&context);
    let plan = integrations::setup::prepare(
        root.path(),
        &before,
        integrations::IntegrationChoice::agent("claude"),
    )
    .unwrap()
    .unwrap();
    Store::open(root.path())
        .unwrap()
        .update(&context, &|| Ok(()), |state| {
            state.operator["integration_pending"] = json!(plan);
            Ok(json!({}))
        })
        .unwrap();
    let edited = root.path().join(ASSETS[2]);
    fs::write(&edited, "Acme manually edited interceptor\n").unwrap();
    let checkpoint = root.path().join(".shield/state/state.json");
    let bytes = fs::read(&checkpoint).unwrap();
    let Err(error) = integrations::recovery::prepare(
        root.path(),
        &metadata(&context),
        integrations::recovery::Action::Review,
    ) else {
        panic!("an edited opaque asset cannot satisfy retained legacy ownership");
    };
    assert!(error.to_string().contains("pending cancel"), "{error}");
    assert_eq!(fs::read(&checkpoint).unwrap(), bytes);
    let cancel = integrations::recovery::prepare(
        root.path(),
        &metadata(&context),
        integrations::recovery::Action::Cancel,
    )
    .unwrap();
    assert!(
        cancel.review(root.path())["effect"]
            .as_str()
            .unwrap()
            .contains("without editing any host file")
    );
    cancel.commit(&context, &|| Ok(())).unwrap();
    assert!(
        integrations::pending(&metadata(&context))
            .unwrap()
            .is_none()
    );
    assert_eq!(
        fs::read_to_string(edited).unwrap(),
        "Acme manually edited interceptor\n"
    );
}

#[test]
fn historical_interceptor_disabled_plugin_can_be_removed_or_replaced() {
    for enabled in [true, false] {
        for replace in [true, false] {
            let (root, context, record) = fixture();
            install(&context, &record);
            let settings =
                json!({"theme":"acme","enabledPlugins":{KEY:enabled,"acme@library":true}})
                    .to_string();
            fs::write(root.path().join(SETTINGS), &settings).unwrap();
            let choice = if replace {
                integrations::IntegrationChoice::agent("claude")
            } else {
                integrations::IntegrationChoice::RemoveAgents(
                    integrations::IntegrationChoice::bits(&["claude"]).unwrap(),
                )
            };
            let before = metadata(&context);
            let plan = integrations::setup::prepare(root.path(), &before, choice)
                .unwrap()
                .unwrap();
            assert_eq!(
                fs::read_to_string(root.path().join(SETTINGS)).unwrap(),
                settings
            );
            assert!(ASSETS.iter().all(|asset| root.path().join(asset).exists()));
            integrations::apply(&context, &before, &plan).unwrap();
            let after: Value =
                serde_json::from_slice(&fs::read(root.path().join(SETTINGS)).unwrap()).unwrap();
            assert!(after["enabledPlugins"].get(KEY).is_none());
            assert_eq!(after["enabledPlugins"]["acme@library"], true);
            assert_eq!(after["theme"], "acme");
            assert!(ASSETS.iter().all(|asset| !root.path().join(asset).exists()));
        }
    }
}

#[test]
fn historical_interceptor_removal_still_refuses_nonboolean_plugin_settings() {
    let (root, context, record) = fixture();
    install(&context, &record);
    let before = metadata(&context);
    for invalid in [json!("false"), Value::Null, json!({})] {
        let settings = json!({"enabledPlugins":{KEY:invalid}}).to_string();
        fs::write(root.path().join(SETTINGS), &settings).unwrap();
        let choice = integrations::IntegrationChoice::RemoveAgents(
            integrations::IntegrationChoice::bits(&["claude"]).unwrap(),
        );
        assert!(integrations::setup::prepare(root.path(), &before, choice).is_err());
        assert_eq!(
            fs::read_to_string(root.path().join(SETTINGS)).unwrap(),
            settings
        );
        assert!(ASSETS.iter().all(|asset| root.path().join(asset).exists()));
    }
}

#[test]
fn historical_interceptor_pending_plan_resumes_exact_owned_assets() {
    let (_root, context, record) = fixture();
    let root = Path::new(&context.project_root);
    let mut changes: Vec<Value> = record["interceptor"]["assets"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(name, bytes)| json!({"path":name,"before":null,"after":bytes}))
        .collect();
    changes.push(json!({"path":SETTINGS,"before":null,"after":json!({"enabledPlugins":{KEY:true}}).to_string()}));
    let pending = json!({"before":null,"after":json!({"mcpServers":{"shield":record["entry"]}}).to_string(),
        "previous":null,"desired":record,"files":changes});
    Store::open(root)
        .unwrap()
        .update(&context, &|| Ok(()), |state| {
            state.operator["integration_pending"] = pending.clone();
            Ok(json!({}))
        })
        .unwrap();
    let before = metadata(&context);
    let plan = integrations::setup::prepare(root, &before, integrations::IntegrationChoice::Keep)
        .unwrap()
        .unwrap();
    integrations::apply(&context, &before, &plan).unwrap();
    for (name, bytes) in record["interceptor"]["assets"].as_object().unwrap() {
        assert_eq!(
            fs::read_to_string(root.join(name)).unwrap(),
            bytes.as_str().unwrap()
        );
    }
    assert!(metadata(&context)["integration_pending"].is_null());
    let owned = integrations::registration(&metadata(&context))
        .unwrap()
        .unwrap();
    assert!(owned.interceptor.is_some());
    assert!(owned.agents.is_empty());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(root.join(ASSETS[0]))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
}

#[test]
fn historical_interceptor_omitted_fields_do_not_relax_current_plan_ownership() {
    let (_root, _context, record) = fixture();
    let source = json!({"before":null,"after":"{}","previous":null,"desired":record});
    let old = integrations::pending(&json!({"integration_pending":source}))
        .unwrap()
        .unwrap();
    assert!(old.files.is_empty());
    assert!(old.desired.as_ref().unwrap().interceptor.is_some());
    let mut current = json!(old);
    current["desired"]["interceptor"] = Value::Null;
    current["desired"]["version"] = json!(env!("CARGO_PKG_VERSION"));
    current.as_object_mut().unwrap().remove("files");
    assert!(integrations::pending(&json!({"integration_pending":current})).is_err());
    for malformed in [json!([]), json!("invalid"), json!(true)] {
        assert!(integrations::pending(&json!({"integration_pending":malformed})).is_err());
    }
}

#[test]
fn historical_interceptor_rejects_unowned_paths_changes_and_edited_assets() {
    let (_root, context, record) = fixture();
    install(&context, &record);
    let root = Path::new(&context.project_root);
    let mut invalid = record.clone();
    invalid["interceptor"]["assets"]["../acme.txt"] = json!("Acme");
    assert!(integrations::registration(&json!({"integration":invalid})).is_err());
    let mut plan: Plan = serde_json::from_value(
        json!({"before":null,"after":"{}","previous":null,"desired":null,
        "files":[{"path":ASSETS[0],"before":null,"after":"unowned"}]}),
    )
    .unwrap();
    assert!(files::publish_plan(root, &plan).is_err());
    let mut desired = record.clone();
    normalize_registration(&mut desired);
    plan.desired = Some(serde_json::from_value(desired).unwrap());
    assert!(files::publish_plan(root, &plan).is_err());
    assert!(files::read(root, ASSETS[0]).is_err());
    let before = metadata(&context);
    let edited = "Acme edited retained asset";
    fs::write(root.join(ASSETS[2]), edited).unwrap();
    assert!(
        integrations::setup::prepare(
            root,
            &before,
            integrations::IntegrationChoice::agent("claude")
        )
        .is_err()
    );
    assert_eq!(metadata(&context), before);
    assert_eq!(fs::read_to_string(root.join(ASSETS[2])).unwrap(), edited);
}

#[test]
fn historical_interceptor_sdk_protocol_keeps_exact_read_approval_and_consumption() {
    retained_sdk_protocol_cases();
}

fn retained_sdk_protocol_cases() -> (assert_fs::TempDir, Vec<Value>) {
    let (fixture_root, context, record) = fixture();
    install(&context, &record);
    let root = Path::new(&context.project_root);
    let path = root.join("acme.txt");
    fs::write(&path, "Acme private text").unwrap();
    let mut cases = Vec::new();
    let mut call = |id: &str, operation: Value, project: Value, approval: Value| {
        let host = if operation["kind"] == "register" {
            Value::Null
        } else {
            json!({"agent":"claude-mod","session_id":"acme-session","tool_use_id":id,"consumer_id":"acme-session",
                "event":if operation["kind"] == "completion" {"PostToolUse"} else {"PreToolUse"}})
        };
        let (receipt, code) = crate::enforcement::protocol::evaluate(
            root,
            &serde_json::to_vec(&json!({"protocol_version":1,
            "request_id":id,"expected_project_id":project,"cwd":root,"operation":operation,
            "host":host,"approval_request_id":approval}))
            .unwrap(),
        );
        assert_eq!(receipt.as_object().unwrap().len(), 8, "{receipt}");
        cases.push(json!({"receipt":receipt,"exit":code,"expected":{"id":id,"project":root,
            "projectId":project,"reference":approval,"phase":operation["kind"],
            "operation":if operation["kind"] == "read-text" {operation.clone()} else {Value::Null}}}));
        (receipt, code)
    };
    let (registered, code) = call(
        "register-1",
        json!({"kind":"register"}),
        Value::Null,
        Value::Null,
    );
    assert_eq!(code, 0);
    assert_eq!(registered["code"], "registration");
    let identity = registered["identity"]["project_id"].clone();
    let operation = json!({"kind":"read-text","path":path,"offset":null,"limit":null});
    let (pending, _) = call("read-1", operation.clone(), identity.clone(), Value::Null);
    assert_eq!(pending["code"], "approval-required", "{pending}");
    let id = pending["approval"]["request_id"].as_str().unwrap();
    review::read_approval::authorize(&context, id, 300, None, false).unwrap();
    let (allowed, code) = call("read-2", operation.clone(), identity.clone(), json!(id));
    assert_eq!(code, 0);
    assert_eq!(allowed["decision"], "allow", "{allowed}");
    let (completed, code) = call(
        "read-2",
        json!({"kind":"completion","delivery_id":"read-2-1",
        "request_id":allowed["history"]["request_id"],"status":"completed","operation":operation}),
        identity.clone(),
        Value::Null,
    );
    assert_eq!(code, 0);
    assert_eq!(completed["code"], "completion-observed", "{completed}");
    let (replayed, _) = call("read-3", operation, identity.clone(), json!(id));
    assert_ne!(replayed["decision"], "allow");
    let (unsupported, _) = call(
        "shell-1",
        json!({"kind":"unsupported","name":"Bash"}),
        identity,
        Value::Null,
    );
    assert_eq!(unsupported["decision"], "deny");
    let (modern, _) = crate::enforcement::protocol::evaluate(root, &serde_json::to_vec(&json!({"protocol_version":1,
        "request_id":"registration","expected_project_id":null,"cwd":root,"operation":{"kind":"register"},"host":null,"approval_request_id":null})).unwrap());
    assert_eq!(modern.as_object().unwrap().len(), 9, "{modern}");
    (fixture_root, cases)
}

#[test]
#[ignore = "requires Node; opt-in interoperability with the retained installed validator"]
fn historical_interceptor_receipts_pass_legacy_js_validator() {
    let (fixture_root, cases) = retained_sdk_protocol_cases();
    let root = fixture_root.path();
    let input = root.join("legacy-receipts.json");
    fs::write(&input, serde_json::to_vec(&cases).unwrap()).unwrap();
    let output = std::process::Command::new("node")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/review/integrations/legacy/receipt-legacy.mjs"
        ))
        .arg(&input)
        .output()
        .expect("Node validates the retained installed mod receipts");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
