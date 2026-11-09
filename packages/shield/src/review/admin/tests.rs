//! Administrative review freezes inputs and rejects stale or foreign commits.
use super::*;
use crate::review::{self, storage::Store};
use assert_fs::prelude::*;

fn enrolled() -> (assert_fs::TempDir, Context) {
    let root = assert_fs::TempDir::new().unwrap();
    let bundle =
        crate::configuration::Bundle::capture("schema_version: 1\npacks: []\n", &[], None).unwrap();
    let candidates = std::collections::BTreeMap::from([("project".into(), json!(bundle))]);
    let revision = crate::configuration::Evaluator::new(review::snapshots(&candidates).unwrap())
        .unwrap()
        .revision()
        .to_owned();
    Store::initialize_active(
        root.path(),
        candidates,
        &revision,
        json!({"sources":{},"default":null,"installed":[],"projection":null}),
    )
    .unwrap();
    let mut context = review::open(root.path()).unwrap();
    context.surface = "mcp".into();
    context.agent = Some("acme-agent".into());
    context.session_id = Some("acme-session".into());
    (root, context)
}

#[test]
fn parity_admin_protection_answers_remain_bound_to_the_pre_question_snapshot() {
    let (root, context) = enrolled();
    let before = project::metadata(&project::checkpoint(&context).unwrap());
    let unchanged = protection::Prepared::from_metadata(
        &context,
        &before,
        review::path_access::PathAccess::default(),
    );
    let path = root.path().join(".shield/review/state.json");
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(
        unchanged.commit(&context, &|| Ok(())).unwrap()["changed"],
        false
    );
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    prepare(
        &context,
        "protection_set",
        &json!({"agent_settings_change":"allow"}),
        &|| Ok(()),
    )
    .unwrap()
    .commit(&context, &|| Ok(()))
    .unwrap();
    let stale = protection::Prepared::from_metadata(
        &context,
        &before,
        review::path_access::PathAccess::default(),
    );
    assert!(matches!(
        stale.commit(&context, &|| Ok(())),
        Err(Error::StaleSnapshot)
    ));
}

#[test]
fn parity_admin_administration_preparation_is_inactive_and_cannot_self_confirm() {
    let (root, context) = enrolled();
    let plan = prepare(
        &context,
        "protection_set",
        &json!({"agent_settings_read":"allow"}),
        &|| Ok(()),
    )
    .unwrap();
    assert_eq!(
        review::path_access::current(root.path())
            .unwrap()
            .agent_settings_read,
        review::path_access::Read::Ask
    );
    assert!(
        prepare(
            &context,
            "protection_set",
            &json!({"agent_settings_read":"allow","confirmed":true}),
            &|| Ok(())
        )
        .is_err()
    );
    let receipt = plan.commit(&context, &|| Ok(())).unwrap();
    assert_eq!(receipt["path_access"]["agent_settings_read"], "allow");
    assert_eq!(receipt["path_access"]["agent_settings_change"], "block");
    assert_eq!(receipt["path_access"]["shield_config_change"], "block");
}

#[test]
fn parity_admin_administrative_reviews_reject_stale_and_foreign_confirmation() {
    let (root, context) = enrolled();
    let first = prepare(
        &context,
        "protection_set",
        &json!({"agent_settings_read":"allow"}),
        &|| Ok(()),
    )
    .unwrap();
    let second = prepare(
        &context,
        "protection_set",
        &json!({"shield_config_change":"allow"}),
        &|| Ok(()),
    )
    .unwrap();
    second.commit(&context, &|| Ok(())).unwrap();
    assert!(matches!(
        first.commit(&context, &|| Ok(())),
        Err(Error::StaleSnapshot)
    ));
    let plan = prepare(&context, "history_configure", &json!({"limit":10}), &|| {
        Ok(())
    })
    .unwrap();
    let mut foreign = context.clone();
    foreign.session_id = Some("another-session".into());
    assert!(matches!(
        plan.commit(&foreign, &|| Ok(())),
        Err(Error::StaleSnapshot)
    ));
    assert_ne!(project::checkpoint(&context).unwrap().history_limit, 10);
    assert!(root.path().exists());
}

#[test]
fn parity_admin_invalid_protection_record_can_be_repaired_without_policy_admission() {
    let (root, context) = enrolled();
    review::path_access::store_fixture(root.path(), json!({"unknown":"allow"}));
    let plan = prepare(
        &context,
        "protection_set",
        &json!({"agent_settings_read":"ask"}),
        &|| Ok(()),
    )
    .unwrap();
    assert!(review::path_access::current(root.path()).is_err());
    plan.commit(&context, &|| Ok(())).unwrap();
    assert_eq!(
        review::path_access::current(root.path()).unwrap(),
        review::path_access::PathAccess::default()
    );
}

#[test]
fn parity_admin_history_purge_preserves_authority_and_invalidates_prior_review() {
    let (_root, context) = enrolled();
    let stale = prepare(&context, "history_configure", &json!({"limit":20}), &|| {
        Ok(())
    })
    .unwrap();
    let before = project::checkpoint(&context).unwrap();
    let plan = prepare(
        &context,
        "history_configure",
        &json!({"enabled":false,"limit":10,"purge":true}),
        &|| Ok(()),
    )
    .unwrap();
    plan.commit(&context, &|| Ok(())).unwrap();
    let after = project::checkpoint(&context).unwrap();
    assert!(!after.history_enabled);
    assert_eq!(after.history_limit, 10);
    assert_ne!(before.history_generation, after.history_generation);
    assert_eq!(before.policy_revision, after.policy_revision);
    assert_eq!(json!(before.active), json!(after.active));
    assert!(matches!(
        stale.commit(&context, &|| Ok(())),
        Err(Error::StaleSnapshot)
    ));
    for limit in [0, 10001] {
        assert!(
            prepare(
                &context,
                "history_configure",
                &json!({"limit":limit}),
                &|| Ok(())
            )
            .is_err()
        );
    }
}

#[test]
fn parity_admin_project_registry_add_default_remove_share_exact_trust_review() {
    let (_root, context) = enrolled();
    let source = assert_fs::TempDir::new().unwrap();
    source
        .child("catalog.json")
        .write_str(&json!({"schema_version":1,"packs":[]}).to_string())
        .unwrap();
    let plan = prepare(
        &context,
        "registry_add",
        &json!({"name":"acme","location":source.path()}),
        &|| Ok(()),
    )
    .unwrap();
    assert!(project::checkpoint(&context).unwrap().operator["sources"]["acme"].is_null());
    assert_eq!(plan.review()["scope"], "project");
    plan.commit(&context, &|| Ok(())).unwrap();
    prepare(
        &context,
        "registry_default",
        &json!({"name":"acme"}),
        &|| Ok(()),
    )
    .unwrap()
    .commit(&context, &|| Ok(()))
    .unwrap();
    assert_eq!(
        project::checkpoint(&context).unwrap().operator["default"],
        "acme"
    );
    prepare(
        &context,
        "registry_remove",
        &json!({"name":"acme"}),
        &|| Ok(()),
    )
    .unwrap()
    .commit(&context, &|| Ok(()))
    .unwrap();
    assert!(project::checkpoint(&context).unwrap().operator["sources"]["acme"].is_null());
    assert!(
        prepare(
            &context,
            "registry_remove",
            &json!({"name":"main"}),
            &|| Ok(())
        )
        .is_err()
    );
}

#[test]
fn parity_admin_proposal_rejection_is_inactive_until_exact_review_commits() {
    let (_root, context) = enrolled();
    let proposal =
        review::tests::propose(&context, &review::tests::input("acme-reject"), &|| Ok(())).unwrap();
    let id = proposal["proposal_id"].as_str().unwrap();
    let plan = prepare(
        &context,
        "policy_reject",
        &json!({"proposal_id":id,"reason":"Keep current policy"}),
        &|| Ok(()),
    )
    .unwrap();
    assert_eq!(
        project::checkpoint(&context)
            .unwrap()
            .proposals
            .iter()
            .find(|p| p.id == id)
            .unwrap()
            .status,
        "pending"
    );
    assert_eq!(
        plan.commit(&context, &|| Ok(())).unwrap()["status"],
        "rejected"
    );
    assert!(
        prepare(
            &context,
            "policy_reject",
            &json!({"proposal_id":id}),
            &|| Ok(())
        )
        .is_err()
    );
}

#[test]
fn parity_admin_enrolled_setup_dry_run_and_removal_prepare_without_writing_agent_files() {
    let (root, context) = enrolled();
    let args = json!({"agents":["claude"],"dry_run":true});
    let plan = prepare(&context, "mcp_setup", &args, &|| Ok(())).unwrap();
    assert!(plan.dry_run());
    assert_eq!(plan.review()["dry_run"], true);
    plan.commit(&context, &|| Ok(())).unwrap();
    assert!(!root.path().join(".mcp.json").exists());
    let plan = prepare(
        &context,
        "mcp_setup",
        &json!({"agents":["claude"]}),
        &|| Ok(()),
    )
    .unwrap();
    plan.commit(&context, &|| Ok(())).unwrap();
    assert!(root.path().join(".mcp.json").exists());
    let plan = prepare(
        &context,
        "mcp_setup",
        &json!({"agents":["claude"],"remove":true}),
        &|| Ok(()),
    )
    .unwrap();
    plan.commit(&context, &|| Ok(())).unwrap();
    assert!(
        integrations::registration(&project::metadata(&project::checkpoint(&context).unwrap()))
            .unwrap()
            .is_none()
    );
    assert!(
        prepare(
            &context,
            "mcp_setup",
            &json!({"agents":["claude"],"scope":"user"}),
            &|| Ok(())
        )
        .is_err()
    );
}

#[test]
fn parity_admin_interrupted_same_project_setup_keeps_mcp_recovery() {
    let (root, context) = enrolled();
    let plan = prepare(
        &context,
        "mcp_setup",
        &json!({"agents":["claude"],"repair":true}),
        &|| Ok(()),
    )
    .unwrap();
    root.child(format!(".claude/.shield-{}.tmp", std::process::id()))
        .write_str("occupied")
        .unwrap();
    let receipt = plan.commit(&context, &|| Ok(())).unwrap();
    assert!(receipt["setup_error"].is_string());
    assert_eq!(receipt["repair_route"], "mcp_setup");
    assert_eq!(receipt["repair_arguments"], json!({"pending":"review"}));
    assert_eq!(
        receipt["pending_setup"]["cancel"],
        json!({"tool":"mcp_setup","arguments":{"pending":"cancel"}})
    );
    assert!(receipt["operator_recovery"].is_null());
}

#[test]
fn parity_admin_interrupted_cross_project_setup_hands_off_target_recovery() {
    use clap::Parser as _;
    for action in ["review", "cancel"] {
        let (source, context) = enrolled();
        let (target, target_context) = enrolled();
        let source_checkpoint = source.path().join(".shield/review/state.json");
        let before_source = std::fs::read(&source_checkpoint).unwrap();
        let before_policy = project::checkpoint(&target_context)
            .unwrap()
            .policy_revision;
        let plan = prepare(
            &context,
            "mcp_setup",
            &json!({
                "project":target.path(),"agents":["claude"],"repair":true
            }),
            &|| Ok(()),
        )
        .unwrap();
        // A real publication failure after recording the confirmed plan and
        // publishing .mcp.json: the hook writer cannot claim its temporary file.
        let occupied = target.child(format!(".claude/.shield-{}.tmp", std::process::id()));
        occupied
            .write_str("occupied by the interrupted writer")
            .unwrap();
        let receipt = plan.commit(&context, &|| Ok(())).unwrap();
        assert!(receipt["setup_error"].is_string(), "{receipt}");
        assert!(target.path().join(".mcp.json").exists());
        assert!(
            integrations::pending(&project::metadata(
                &project::checkpoint(&target_context).unwrap()
            ))
            .unwrap()
            .is_some()
        );
        assert_eq!(receipt["repair_route"], "operator_cli", "{receipt}");
        assert!(receipt["repair_arguments"].is_null());
        assert!(receipt["pending_setup"].is_null());
        assert_eq!(
            receipt["operator_recovery"]["project"],
            target_context.project_root
        );
        assert_eq!(
            receipt["operator_recovery"]["project_id"],
            target_context.project_id
        );
        let handoff = &receipt["operator_recovery"][action];
        assert_eq!(handoff["command"], "shield");
        assert_eq!(
            handoff["arguments"],
            json!([
                "--project",
                target_context.project_root,
                "mcp",
                "setup",
                "--pending",
                action
            ])
        );
        let mut argv = vec!["shield".to_owned()];
        argv.extend(
            handoff["arguments"]
                .as_array()
                .unwrap()
                .iter()
                .map(|arg| arg.as_str().unwrap().to_owned()),
        );
        let cli = crate::cli::Cli::try_parse_from(argv).unwrap();
        assert_eq!(
            cli.project.as_deref(),
            Some(Path::new(&target_context.project_root))
        );
        assert_eq!(std::fs::read(&source_checkpoint).unwrap(), before_source);

        std::fs::remove_file(occupied.path()).unwrap();
        let recovery = integrations::recovery::prepare(
            target.path(),
            &project::metadata(&project::checkpoint(&target_context).unwrap()),
            if action == "review" {
                integrations::recovery::Action::Review
            } else {
                integrations::recovery::Action::Cancel
            },
        )
        .unwrap();
        recovery.commit(&target_context, &|| Ok(())).unwrap();
        let state = project::checkpoint(&target_context).unwrap();
        assert!(
            integrations::pending(&project::metadata(&state))
                .unwrap()
                .is_none()
        );
        assert_eq!(state.policy_revision, before_policy);
        assert_eq!(std::fs::read(&source_checkpoint).unwrap(), before_source);
    }
}

#[test]
fn parity_admin_explicit_project_repair_is_bound_to_caller_and_target() {
    let (source, context) = enrolled();
    let (target, target_context) = enrolled();
    let args = json!({"project":target.path(),"agents":["claude"],"repair":true});
    let preview = prepare(
        &context,
        "mcp_setup",
        &json!({"dry_run":true, "project":target.path(),"agents":["claude"],"repair":true}),
        &|| Ok(()),
    )
    .unwrap();
    assert_eq!(
        preview.public_review()["project"],
        target_context.project_root
    );
    assert!(!target.path().join(".mcp.json").exists());
    let plan = prepare(&context, "mcp_setup", &args, &|| Ok(())).unwrap();
    assert!(matches!(
        plan.commit(&target_context, &|| Ok(())),
        Err(Error::StaleSnapshot)
    ));
    let plan = prepare(&context, "mcp_setup", &args, &|| Ok(())).unwrap();
    plan.commit(&context, &|| Ok(())).unwrap();
    assert!(target.path().join(".mcp.json").exists());
    assert!(!source.path().join(".mcp.json").exists());
    assert_eq!(
        context.project_root,
        source.path().canonicalize().unwrap().to_str().unwrap()
    );
    for invalid in [
        json!({"project":target.path(),"agents":["claude"]}),
        json!({"project":target.path(),"agents":["claude"],"repair":true,"remove":true}),
        json!({"project":"relative","agents":["claude"],"repair":true}),
        json!({"project":target.path(),"repair":true}),
    ] {
        assert!(prepare(&context, "mcp_setup", &invalid, &|| Ok(())).is_err());
    }
    let plan = prepare(&context, "mcp_setup", &args, &|| Ok(())).unwrap();
    target
        .child(".mcp.json")
        .write_str("{\"changed\":true}\n")
        .unwrap();
    assert!(plan.commit(&context, &|| Ok(())).is_err());
    assert_eq!(
        std::fs::read_to_string(target.path().join(".mcp.json")).unwrap(),
        "{\"changed\":true}\n"
    );
    let absent = assert_fs::TempDir::new().unwrap();
    assert!(
        prepare(
            &context,
            "mcp_setup",
            &json!({"project":absent.path(),"agents":["claude"],"repair":true}),
            &|| Ok(())
        )
        .is_err()
    );
    assert!(
        prepare(
            &context,
            "protection_set",
            &json!({"project":target.path(),"agent_settings_change":"allow"}),
            &|| Ok(())
        )
        .is_err()
    );
}

#[test]
fn parity_admin_box_managed_hook_repair_preserves_publication_ownership() {
    let (root, context) = enrolled();
    root.child(".box/config.yml")
        .write_str("schema_version: 1\n")
        .unwrap();
    prepare(
        &context,
        "mcp_setup",
        &json!({"agents":["box"]}),
        &|| Ok(()),
    )
    .unwrap()
    .commit(&context, &|| Ok(()))
    .unwrap();
    let before = project::metadata(&project::checkpoint(&context).unwrap());
    let error = prepare(
        &context,
        "mcp_setup",
        &json!({"agents":["claude","codex"],"repair":true}),
        &|| Ok(()),
    )
    .err()
    .unwrap();
    assert!(error.to_string().contains("config_sync"), "{error}");
    assert!(error.to_string().contains("repair_hooks"), "{error}");
    assert_eq!(
        project::metadata(&project::checkpoint(&context).unwrap()),
        before
    );
    assert!(!root.path().join(".codex/hooks.json").exists());
    assert!(!root.path().join(".claude/settings.local.json").exists());
}

#[test]
fn parity_admin_repair_does_not_apply_a_review_after_operator_state_changes() {
    let (_root, context) = enrolled();
    assert!(
        prepare(
            &context,
            "policy_repair",
            &json!({"confirmed":true}),
            &|| Ok(())
        )
        .is_err()
    );
    let plan = prepare(&context, "policy_repair", &json!({}), &|| Ok(())).unwrap();
    Store::open(Path::new(&context.project_root))
        .unwrap()
        .update_checkpoint(&context, &|| Ok(()), |state| {
            state.operator["path_access"] = json!({});
            Ok(json!({}))
        })
        .unwrap();
    assert!(plan.commit(&context, &|| Ok(())).is_err());
}

#[test]
fn parity_admin_cache_preparation_uses_committed_policy_without_new_authority() {
    let (root, context) = enrolled();
    let before = project::checkpoint(&context).unwrap();
    root.child(".shield/config.yml")
        .write_str("unreviewed: invalid policy")
        .unwrap();
    let receipt = prepare(&context, "cache_prepare", &json!({}), &|| Ok(()))
        .unwrap()
        .commit(&context, &|| Ok(()))
        .unwrap();
    let after = project::checkpoint(&context).unwrap();
    assert_eq!(receipt["prepared"], true);
    assert_eq!(after.policy_revision, before.policy_revision);
    assert_eq!(
        after.active.as_ref().unwrap().activation_id(),
        before.active.as_ref().unwrap().activation_id()
    );
    assert_eq!(
        after.active.as_ref().unwrap().revision(),
        before.active.as_ref().unwrap().revision()
    );
    assert!(
        prepare(
            &context,
            "cache_prepare",
            &json!({"cache_dir":"relative-cache"}),
            &|| Ok(())
        )
        .is_err()
    );
}

#[test]
fn parity_admin_mcp_setup_dry_run_does_not_disclose_protected_credentials() {
    let (root, context) = enrolled();
    let sentinel = "acme-protected-credential-sentinel";
    let shared =
        json!({"mcpServers":{"acme":{"command":"acme","env":{"ACME_TOKEN":sentinel}}}}).to_string();
    let settings = json!({"env":{"ACME_TOKEN":sentinel}}).to_string();
    root.child(".mcp.json").write_str(&shared).unwrap();
    root.child(".claude/settings.json")
        .write_str(&settings)
        .unwrap();
    let args = json!({"agents":["claude"],"dry_run":true});
    let plan = prepare(&context, "mcp_setup", &args, &|| Ok(())).unwrap();
    assert!(plan.review().to_string().contains(sentinel));
    assert!(!plan.public_review().to_string().contains(sentinel));
    let connection = review::mcp::Connection::new("acme-preview".into());
    let response = review::mcp::call(&context, &connection, "mcp_setup", &args, &|| Ok(()));
    assert!(response["error"].is_null(), "{response}");
    assert!(!response.to_string().contains(sentinel));
    assert_eq!(
        std::fs::read_to_string(root.path().join(".mcp.json")).unwrap(),
        shared
    );
    assert_eq!(
        std::fs::read_to_string(root.path().join(".claude/settings.json")).unwrap(),
        settings
    );
    assert!(
        response["data"]["files"]
            .as_array()
            .unwrap()
            .iter()
            .all(|file| file.get("before").is_none() && file.get("after").is_none())
    );
    assert!(response["data"].get("connections").is_none());
}
