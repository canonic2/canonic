//! Publication binding, recovery and projection behavior.
use super::projections::materialize;
use super::*;
use std::fs;

fn fixture() -> (assert_fs::TempDir, Context) {
    let root = assert_fs::TempDir::new().expect("fixture");
    let candidate = json!(
        configuration::Bundle::capture(r#"{"schema_version":1,"packs":[]}"#, &[], None)
            .expect("candidate")
    );
    let meta = json!({"sources":{},"default":null,"installed":[],"projection":null});
    let files = projections(&candidate, &meta).expect("projections");
    let mut meta = meta;
    meta["projection"] = json!(digest(&files));
    let mut candidates = BTreeMap::new();
    candidates.insert("project".into(), candidate);
    let evaluator =
        configuration::Evaluator::new(review::snapshots(&candidates).expect("snapshots"))
            .expect("evaluator");
    Store::initialize_active(root.path(), candidates, evaluator.revision(), meta)
        .expect("initialize");
    let context = review::open(root.path()).expect("context");
    materialize(&context, &files).expect("materialize");
    (root, context)
}

fn changed(before: &State) -> Value {
    crate::review::packs::rebuild(&before.candidates["project"], &mut [], &[], Some(r#"{"schema_version":1,"rules":[{"id":"read","decision":"deny","reason":"Acme fixture","conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"any"}}}],"kind":"custom","capabilities":["argument-rules-v1","structured-rules-v1"]}"#)).expect("changed candidate")
}

#[test]
fn operator_review_retains_its_plan_but_still_refuses_changed_files() {
    use clap::Parser as _;
    use std::sync::Arc;
    for drift in [false, true] {
        let (root, context) = fixture();
        let logs = assert_fs::TempDir::new().unwrap();
        let cli = crate::cli::Cli::parse_from([
            "shield",
            "--debug-log-dir",
            logs.path().to_str().unwrap(),
            "status",
        ]);
        let log = Arc::new(crate::diagnostics::Log::open(&cli));
        let path = log.path().unwrap().to_owned();
        let request = log
            .request("cli_owner", "review_commit", None, None)
            .unwrap();
        let scope = request.enter();
        let (before, base) = crate::review::project::load_prepared(&context).unwrap();
        let candidate = changed(&before);
        let (_, plan) = prepare_review(
            &context,
            &before,
            &candidate,
            &metadata(&before),
            Some(&base),
        )
        .unwrap();
        if drift {
            fs::write(
                root.path().join(".shield/rules.yml"),
                "# Acme operator edit\n",
            )
            .unwrap();
        }
        let result = commit_prepared(
            &context,
            &before,
            candidate,
            metadata(&before),
            None,
            None,
            &plan,
        );
        drop(scope);
        request.finish(if result.is_ok() { "success" } else { "refused" });
        if drift {
            assert!(result.is_err());
            assert_eq!(
                fs::read_to_string(root.path().join(".shield/rules.yml")).unwrap(),
                "# Acme operator edit\n"
            );
            assert_eq!(
                Store::open(root.path())
                    .unwrap()
                    .read_checkpoint()
                    .unwrap()
                    .policy_revision,
                before.policy_revision
            );
        } else {
            assert_eq!(result.unwrap()["policy_revision"], plan.revision());
            let state = Store::open(root.path()).unwrap().read().unwrap();
            state.active.as_ref().unwrap().validate(&state).unwrap();
        }
        drop(request);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
        let records: Vec<Value> = loop {
            let text = fs::read_to_string(&path).unwrap();
            let records: Vec<Value> = text
                .split_inclusive('\n')
                .filter(|line| line.ends_with('\n'))
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();
            if records.iter().any(|r| r["event"] == "request_end") {
                break records;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        };
        drop(log);
        let end = records
            .iter()
            .find(|r| r["event"] == "request_end")
            .unwrap();
        // Inspecting the committed state above is verification, outside the
        // measured review/commit work in the assertion below.
        assert_eq!(end["fields"]["counts"]["policy.preparations"], 1, "{end}");
    }
}

#[test]
fn unchanged_projection_bytes_cannot_skip_path_validation() {
    let (root, context) = fixture();
    fs::write(root.path().join("escape.yml"), "unchanged").unwrap();
    let files = BTreeMap::from([("../escape.yml".into(), "unchanged".into())]);
    assert!(repair_projections(&context, &files, &json!({})).is_err());
}

#[cfg(unix)]
#[test]
fn unchanged_projection_bytes_cannot_skip_symlinked_parent_checks() {
    let (root, context) = fixture();
    let outside = assert_fs::TempDir::new().unwrap();
    fs::write(outside.path().join("pack.yml"), "unchanged").unwrap();
    std::os::unix::fs::symlink(outside.path(), root.path().join(".shield/cache")).unwrap();
    let files = BTreeMap::from([("cache/pack.yml".into(), "unchanged".into())]);
    assert!(repair_projections(&context, &files, &json!({})).is_err());
    assert_eq!(
        fs::read_to_string(outside.path().join("pack.yml")).unwrap(),
        "unchanged"
    );
}

#[test]
fn local_mcp_values_stay_out_of_proposal_views_receipts_and_activation_history() {
    let (root, context) = fixture();
    let before = load(&context).unwrap();
    let local=json!({"schema_version":1,"mcp":{"cloud":{"transport":{"type":"stdio","command":"/acme/private/connector","args":["--profile","acme-personal-profile"],"env":{"ACME_REGION":"acme-private-region"}},"agents":{"claude":"personal_cloud"}}}}).to_string();
    let candidate = json!(
        configuration::Bundle::capture_layers(
            "schema_version: 1\npacks: []\n",
            Some(&local),
            &[],
            None,
            None,
            &[]
        )
        .unwrap()
    );
    let proposal: crate::review::model::Proposal=serde_json::from_value(json!({
        "proposal_id":"proposal-acme","created_at":"2026-01-01T00:00:00Z","status":"pending","provenance":{},"links":{},"reasons":{},
        "policy_revision":before.policy_revision,"target_scope":"project","operation":"fixture","rule_id":"",
        "candidate":candidate,"base_candidate":before.candidates["project"],"base_metadata":metadata(&before),"candidate_metadata":metadata(&before),
        "receipt":null,"comparison":{"relation":"unknown","complete":false},"operator":null,"retry_key":"","retry_owner":"","signature":""
    })).unwrap();
    let public = proposal.public(false);
    let arguments = proposal.apply_arguments();
    let receipt = commit(&context, &before, candidate, metadata(&before), None).unwrap();
    let history = review::history_events(root.path()).unwrap();
    for record in [&public, &arguments, &receipt, &history] {
        for private in [
            "/acme/private/connector",
            "acme-personal-profile",
            "acme-private-region",
            "personal_cloud",
        ] {
            assert!(!record.to_string().contains(private), "{record}");
        }
    }
    let expected = json!([{"kind":"mcp","id":"cloud","layer":"local","change":"changed"}]);
    assert_eq!(public["apply_arguments"]["changes"], expected);
    assert_eq!(arguments["changes"], expected);
    assert_eq!(receipt["changes"], expected);
}

#[test]
fn mcp_connections_host_confirmation_cannot_activate_connection_changes() {
    let (_root, context) = fixture();
    let before = load(&context).unwrap();
    let candidate=json!(configuration::Bundle::capture(&json!({"schema_version":1,"packs":[],"mcp":{"cloud":{
        "transport":{"type":"http","url":"https://cloud.example.com/mcp"},"agents":{"codex":"cloud"}}}}).to_string(),&[],None).unwrap());
    let error = commit_confirmed(
        &context,
        &before,
        candidate,
        metadata(&before),
        None,
        None,
        "host-tool-permission",
        None,
    )
    .unwrap_err();
    assert!(error.to_string().contains("terminal"));
    assert_eq!(
        load(&context).unwrap().policy_revision,
        before.policy_revision
    );
}

#[test]
fn mcp_connections_project_only_survive_permission_rebuild_without_null_associations() {
    let config=json!({"schema_version":1,"packs":[],"mcp":{"cloud":{"transport":{"type":"http","url":"https://cloud.example.com/mcp"},"agents":{"codex":"cloud"}}}}).to_string();
    let candidate = json!(configuration::Bundle::capture(&config, &[], None).unwrap());
    let rebuilt = review::packs::rebuild(&candidate, &mut [], &[], None).unwrap();
    assert_eq!(
        configuration::layers::merged(&rebuilt).unwrap()["mcp"],
        configuration::layers::merged(&candidate).unwrap()["mcp"]
    );
}

#[test]
fn mcp_connections_variable_driven_settings_need_terminal_review_and_rebuild_keeps_connections() {
    let root = assert_fs::TempDir::new().unwrap();
    let pack=json!({"schema_version":1,"kind":"pack","id":"acme/cloud","version":"1.0.0","description":"Acme cloud","license":"MIT","maintainers":["Acme"],"engines":{"shield":"*"},
        "capabilities":["structured-rules-v1","variables-v1","mcp-integrations-v1"],"variables":{"endpoint":{"type":"string","description":"Endpoint","default":"https://cloud.example.com/mcp"}},
        "mcp_integrations":{"cloud":{"transport":"http","fields":{"/type":{"exact":"http"}},"defaults":{"type":"http","url":{"var":"endpoint"}}}},"rules":[]}).to_string();
    let config = |url: &str| {
        json!({"schema_version":1,"packs":[{"id":"acme/cloud","registry_identity":"acme-registry","version":"1.0.0","profile":"recommended","path":"pack.yml","variables":{"endpoint":url}}],
        "mcp":{"cloud":{"transport":{"type":"defaults","pack":"acme-registry/acme/cloud","integration":"cloud"},"agents":{"codex":"cloud"},"packs":{"acme-registry/acme/cloud":"cloud"}}}}).to_string()
    };
    let bundle = configuration::Bundle::capture(
        &config("https://cloud.example.com/mcp"),
        std::slice::from_ref(&pack),
        None,
    )
    .unwrap();
    let context = review::mcp::initialize_fixture(root.path(), &bundle);
    let before = load(&context).unwrap();
    let candidate = json!(
        configuration::Bundle::capture(
            &config("https://other.example.com/mcp"),
            std::slice::from_ref(&pack),
            None
        )
        .unwrap()
    );
    let error = commit_confirmed(
        &context,
        &before,
        candidate,
        metadata(&before),
        None,
        None,
        "host-tool-permission",
        None,
    )
    .unwrap_err();
    assert!(error.to_string().contains("defaults changed"), "{error}");
    let mut rows = vec![
        json!({"id":"acme/cloud","source_identity":"acme-registry","version":"1.0.0","profile":"recommended","digest":policy::fingerprint(pack.as_bytes())}),
    ];
    let rebuilt = review::packs::rebuild(
        &before.candidates["project"],
        &mut rows,
        std::slice::from_ref(&pack),
        None,
    )
    .unwrap();
    assert_eq!(
        configuration::layers::merged(&rebuilt).unwrap()["mcp"],
        configuration::layers::merged(&before.candidates["project"]).unwrap()["mcp"]
    );
    let removed =
        review::packs::rebuild(&before.candidates["project"], &mut [], &[], None).unwrap();
    let image = configuration::Evaluator::new(
        review::snapshots(&BTreeMap::from([("project".into(), removed)])).unwrap(),
    )
    .unwrap();
    let connection = &image.effective().mcp_connections()["cloud"];
    assert!(connection.enabled);
    assert!(connection.packs.is_empty());
    assert_eq!(
        connection.transport,
        crate::mcp_connections::Transport::Http {
            url: "https://cloud.example.com/mcp".into()
        }
    );
}

#[cfg(unix)]
#[test]
fn failed_initialization_reports_its_cause_instead_of_missing_state() {
    use std::os::unix::fs::PermissionsExt;
    let root = assert_fs::TempDir::new().expect("fixture");
    let project = root.path().join("acme");
    let cache = root.path().join("shared-cache");
    fs::create_dir(&project).expect("project");
    fs::create_dir(&cache).expect("cache");
    fs::set_permissions(&cache, fs::Permissions::from_mode(0o755)).expect("permissions");
    let candidate = json!(
        configuration::Bundle::capture(r#"{"schema_version":1,"packs":[]}"#, &[], None)
            .expect("candidate")
    );
    let candidates = BTreeMap::from([("project".to_owned(), candidate)]);
    let evaluator =
        configuration::Evaluator::new(review::snapshots(&candidates).expect("snapshots"))
            .expect("evaluator");
    let meta = json!({"sources":{},"default":null,"installed":[],"projection":null,
        "compiled_cache_root":cache});
    let result = initialize(
        &project,
        &candidates,
        evaluator.revision(),
        &meta,
        BTreeMap::new(),
    );
    assert!(matches!(result, Err(Error::CacheUnavailable)), "{result:?}");
    assert!(!project.join(".shield/review/state.json").exists());
}

#[test]
fn proposal_bytes_changed_after_review_cannot_be_committed() {
    let (root, context) = fixture();
    let proposal = review::tests::propose(&context, &review::tests::input("reviewed"), &|| Ok(()))
        .expect("proposal");
    let id = proposal["proposal_id"].as_str().expect("id");
    let before = load(&context).expect("reviewed state");
    let candidate = before
        .proposals
        .iter()
        .find(|p| p.id == id)
        .expect("proposal")
        .candidate
        .clone();
    let candidate = Store::open(root.path())
        .unwrap()
        .captured_candidate(&candidate)
        .unwrap();
    Store::open(root.path())
        .expect("store")
        .update(&context, &|| Ok(()), |current| {
            current
                .proposals
                .iter_mut()
                .find(|p| p.id == id)
                .expect("proposal")
                .candidate = changed(&before);
            Ok(json!({}))
        })
        .expect("concurrent edit");
    assert!(matches!(
        commit(&context, &before, candidate, metadata(&before), Some(id)),
        Err(Error::Invalid("proposal changed or is stale"))
    ));
    let after = load(&context).expect("state");
    assert_eq!(json!(after.active), json!(before.active));
    assert_eq!(after.proposals[0].status, "pending");
}

#[test]
fn identical_candidate_lock_failure_is_not_a_committed_receipt() {
    let (root, context) = fixture();
    let before = load(&context).expect("state");
    fs::write(root.path().join(".shield/review/state.lock"), "held").expect("lock");
    assert!(matches!(
        commit(
            &context,
            &before,
            before.candidates["project"].clone(),
            metadata(&before),
            None
        ),
        Err(Error::Locked)
    ));
    assert_eq!(
        json!(load(&context).expect("state").active),
        json!(before.active)
    );
}

#[test]
fn failed_projection_writes_remove_their_temporary() {
    let (root, context) = fixture();
    let shield = root.path().join(".shield");
    fs::remove_file(shield.join("rules.yml")).expect("remove rules");
    fs::create_dir(shield.join("rules.yml")).expect("blocking directory");
    fs::write(shield.join("rules.yml/acme"), "kept").expect("blocking entry");
    let files = BTreeMap::from([("rules.yml".to_owned(), "rules: []\n".to_owned())]);
    assert!(materialize(&context, &files).is_err());
    let temporary = shield.join(format!("rules.shield-{}.tmp", std::process::id()));
    assert!(!temporary.exists());
}

#[cfg(any(unix, windows))]
#[test]
fn projection_writes_remove_only_exited_writers_temporaries() {
    let (root, context) = fixture();
    let shield = root.path().join(".shield");
    let dead = review::storage::exited_pid();
    let unverified = i32::from(!cfg!(windows));
    let exited = shield.join(format!("rules.shield-{dead}.tmp"));
    let kept = [
        shield.join(format!("rules.shield-{unverified}.tmp")),
        shield.join(format!("rules.{dead}.tmp")),
        shield.join(format!("rules.acme-{dead}.tmp")),
    ];
    for path in kept.iter().chain([&exited]) {
        fs::write(path, "partial").expect("leftover");
    }
    let files = BTreeMap::from([("rules.yml".to_owned(), "rules: []\n".to_owned())]);
    materialize(&context, &files).expect("materialize");
    assert!(!exited.exists());
    for path in &kept {
        assert!(path.exists(), "{}", path.display());
    }
}

#[test]
fn repair_recovers_partial_files_but_preserves_manual_edits_and_refuses_stale_review() {
    let (root, context) = fixture();
    let before = load(&context).expect("state");
    let collision = root
        .path()
        .join(format!(".shield/rules.shield-{}.tmp", std::process::id()));
    fs::write(&collision, "owned collision").expect("collision");
    let receipt =
        commit(&context, &before, changed(&before), metadata(&before), None).expect("committed");
    assert_eq!(receipt["committed"], true);
    assert_eq!(receipt["repair_needed"], true);
    assert!(
        projection_changed(&context, &load(&context).expect("state")).expect("projection status")
    );
    fs::remove_file(collision).expect("remove collision");
    assert!(
        reconcile(&context, &before).is_err(),
        "stale review must refuse"
    );
    let current = load(&context).expect("state");
    assert_eq!(
        reconcile(&context, &current).expect("repair")["repair_needed"],
        false
    );
    let manual = root.path().join(".shield/rules.yml");
    fs::write(&manual, "manual edit").expect("manual edit");
    assert!(reconcile(&context, &current).is_err());
    assert_eq!(
        fs::read_to_string(manual).expect("preserved"),
        "manual edit"
    );
}

#[test]
fn projections_split_the_local_layer_into_its_own_files() {
    let pack = |id: &str| {
        json!({"schema_version":1,"kind":"pack","id":id,"version":"1.0.0","description":"Acme","license":"MIT","maintainers":["Acme"],"engines":{"shield":env!("CARGO_PKG_VERSION")},"capabilities":["structured-rules-v1"],"rules":[]}).to_string()
    };
    let rules = |id: &str| {
        json!({"schema_version":1,"kind":"custom","capabilities":["structured-rules-v1","argument-rules-v1"],"rules":[{"id":id,"decision":"deny","conditions":{"domain":"argv","executable":"rm","arguments":{"kind":"any"}}}]}).to_string()
    };
    let shared = json!({"schema_version":1,"packs":[
            {"id":"acme/shared","version":"1.0.0","profile":"recommended","path":"cache/shared.yml"},
            {"id":"acme/off","version":"1.0.0","profile":"recommended","path":"cache/off.yml"}
        ],"rules":"rules.yml"}).to_string();
    let local = json!({"schema_version":1,"packs":[
            {"id":"acme/off","disabled":true},
            {"id":"acme/mine","version":"1.0.0","profile":"loose","path":"cache/mine.yml"}
        ],"rules":"rules.local.yml"})
    .to_string();
    let candidate = json!(
        configuration::Bundle::capture_layers(
            &shared,
            Some(&local),
            &[pack("acme/shared"), pack("acme/mine")],
            Some(&rules("team")),
            Some(&rules("mine")),
            &[pack("acme/off")],
        )
        .expect("capture")
    );
    let meta = json!({"installed":[{"id":"acme/shared"},{"id":"acme/mine","layer":"local"}]});
    let files = projections(&candidate, &meta).expect("projections");
    let config: Value = policy::parse(&files["config.yml"]).expect("config");
    let ids: Vec<_> = config["packs"]
        .as_array()
        .expect("packs")
        .iter()
        .map(|p| p["id"].clone())
        .collect();
    assert_eq!(ids, [json!("acme/shared"), json!("acme/off")]);
    assert!(files["config.local.yml"].contains("acme/mine"));
    assert!(files["rules.local.yml"].contains("mine"));
    assert!(
        files["shield.lock"].contains("acme/shared") && !files["shield.lock"].contains("acme/mine")
    );
    assert!(files["shield.local.lock"].contains("acme/mine"));
    assert!(files.contains_key("cache/off.yml") && files.contains_key("cache/mine.yml"));
    let alone = json!({"schema_version":1,"packs":[{"id":"acme/shared","version":"1.0.0","profile":"recommended","path":"cache/shared.yml"}],"rules":"rules.yml"}).to_string();
    let plain = json!(
        configuration::Bundle::capture(&alone, &[pack("acme/shared")], Some(&rules("team")))
            .expect("plain")
    );
    let files =
        projections(&plain, &json!({"installed":[{"id":"acme/shared"}]})).expect("projections");
    assert!(!files.contains_key("config.local.yml") && !files.contains_key("shield.local.lock"));
}

#[test]
fn projections_write_yaml_and_keep_yaml_sources() {
    let rules = json!({"schema_version":1,"kind":"custom","capabilities":["structured-rules-v1","argument-rules-v1"],"rules":[{"id":"acme-yes","decision":"allow","reason":"Acme: health check","conditions":{"domain":"argv","executable":"curl","arguments":{"kind":"exact","values":["--max-time","2","yes","http://localhost:3000/health"]}}}]});
    let config = r#"{"schema_version":1,"packs":[],"rules":"rules.yml"}"#;
    let candidate = json!(
        configuration::Bundle::capture(config, &[], Some(&rules.to_string())).expect("candidate")
    );
    let files = projections(&candidate, &json!({"installed":[]})).expect("projections");
    for name in ["config.yml", "rules.yml"] {
        assert!(
            !files[name].trim_start().starts_with('{'),
            "{name}: {}",
            files[name]
        );
    }
    assert!(files["config.yml"].starts_with("schema_version: 1\n"));
    assert!(files["rules.yml"].starts_with("schema_version: 1\nkind: custom\n"));
    assert!(files["rules.yml"].contains("- id: acme-yes\n"));
    // Strings YAML could read as other types survive the strict reader.
    assert_eq!(
        policy::parse::<Value>(&files["rules.yml"]).expect("rules"),
        rules
    );
    policy::CustomRules::from_yaml(&files["rules.yml"]).expect("valid rules");

    let manual = "# Acme team rules\nschema_version: 1\nkind: custom\ncapabilities: [structured-rules-v1]\nrules: []\n";
    let candidate =
        json!(configuration::Bundle::capture(config, &[], Some(manual)).expect("manual candidate"));
    let files = projections(&candidate, &json!({"installed":[]})).expect("projections");
    assert_eq!(files["rules.yml"], manual);
}
