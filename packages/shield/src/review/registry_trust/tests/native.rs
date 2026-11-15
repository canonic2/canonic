//! Final native overrides use authenticated source and retain shared trust.
use super::*;
use crate::{
    agents,
    enforcement::protocol,
    review::{hook_approval, session_grants},
};
use std::{path::Path, rc::Rc};

fn trusted() -> Rc<Fixture> {
    let fixture = Rc::new(Fixture::new());
    Prepared::new(&fixture.context, "acme", &|| Ok(()))
        .unwrap()
        .commit(&fixture.context, &|| Ok(()))
        .unwrap();
    fixture
}
fn context(fixture: &Fixture, agent: &str) -> Context {
    let mut context = fixture.context.clone();
    context.surface = "mcp".into();
    context.agent = Some(agent.into());
    context.session_id = Some("acme-session".into());
    context
}
fn hook(fixture: &Fixture) -> Value {
    hook_event(fixture, agents::Event::Before)
}
fn hook_event(fixture: &Fixture, event: agents::Event) -> Value {
    agents::translate_event(fixture.project.path(),agents::find("codex").unwrap(),
        &serde_json::to_vec(&json!({"hook_event_name":if event==agents::Event::Permission {"PermissionRequest"} else {"PreToolUse"},"tool_name":"Bash","tool_use_id":"acme-call","session_id":"acme-session","cwd":fixture.project.path(),"tool_input":{"command":"acme push"}})).unwrap(),
        event,&crate::diagnostics::Log::disabled())
}
fn raw(fixture: &Fixture) -> Value {
    serde_json::to_value(
        Store::open(fixture.project.path())
            .unwrap()
            .read_checkpoint()
            .unwrap(),
    )
    .unwrap()
}
fn approved(fixture: &Fixture) -> Value {
    // In-session approval needs Codex's recorded integration.
    if raw(fixture)["operator"]["integration"].is_null() {
        crate::review::operator::integrations::configure_fixture(
            fixture.project.path(),
            crate::review::integrations::IntegrationChoice::agent("codex"),
            crate::review::integrations::AskFallback::Block,
        );
    }
    let refusal = hook(fixture);
    assert_eq!(
        refusal["hookSpecificOutput"]["permissionDecision"], "deny",
        "{refusal}"
    );
    let row = raw(fixture)["hook_requests"][0].clone();
    hook_approval::approve(
        &context(fixture, "codex"),
        &json!({"request_id":row["request_id"]}),
        true,
    )
    .unwrap();
    let row = raw(fixture)["hook_requests"][0].clone();
    assert_eq!(row["status"], "approved");
    row
}
fn revoke(fixture: &Fixture) {
    fixture.publish(2, true);
    registries::checked_catalog(&fixture.context, &fixture.source).unwrap();
}
fn install_registration(fixture: &Fixture, fallback: &str) {
    Store::open(fixture.project.path()).unwrap().update_checkpoint(&fixture.context,&||Ok(()),|state| {
        state.operator["integration"]=json!({"binary":"/fixture/shield","binary_digest":"a".repeat(64),"version":"0.20.0","entry":{},"agents":{},"box_definition":{"shared":"fixture","local":"fixture"}});
        Ok(Value::Null)
    }).unwrap();
    crate::review::settings::fallback_fixture(
        &fixture.context,
        "codex",
        serde_json::from_value(json!(fallback)).unwrap(),
    );
}

fn read_evidence(fixture: &Fixture) -> (crate::runtime::EvaluatedRequest, String) {
    let root = Path::new(&fixture.context.project_root);
    let path = root.join("acme.txt");
    std::fs::write(&path, "Acme private text").unwrap();
    let binding =
        crate::enforcement::read_text::Binding::open(root, root, path.to_str().unwrap()).unwrap();
    let request = crate::policy::Request {
        executable: "shield:read-text".into(),
        arguments: vec![path.to_str().unwrap().into()],
        cwd: fixture.context.project_root.clone(),
        project: fixture.context.project_root.clone(),
    };
    let resource = crate::review::runtime::consumer::ResourceRequest {
        mcp: None,
        input: crate::policy::resolution::normalized::Input::File {
            action: "read".into(),
            sources: request.arguments.clone(),
            destination: None,
            recursive: false,
            original_input: json!({"file_path":path}),
        },
        context: crate::policy::resolution::normalized::Context {
            cwd: request.cwd.clone(),
            environment: None,
            working_files: None,
        },
    };
    let evidence = crate::runtime::Project::open(root)
        .unwrap()
        .snapshot()
        .unwrap()
        .evaluate_resource(request, resource)
        .unwrap();
    (evidence, binding.digest)
}

#[test]
fn final_read_authority_refuses_revoked_source_without_spending_exact_grant() {
    if !crate::test_support::isolated_home(concat!(
        module_path!(),
        "::final_read_authority_refuses_revoked_source_without_spending_exact_grant"
    )) {
        return;
    }
    let fixture = trusted();
    let (evidence, digest) = read_evidence(&fixture);
    let pending = crate::review::read_approval::gate(
        &fixture.context,
        "acme-reader",
        &evidence,
        &digest,
        None,
        || true,
    )
    .unwrap();
    let id = pending["approval"]["request_id"].as_str().unwrap();
    let mut operator = fixture.context.clone();
    operator.surface = "cli".into();
    crate::review::read_approval::authorize(&operator, id, 300, None, false).unwrap();
    let before = raw(&fixture);
    revoke(&fixture);
    assert!(matches!(
        crate::review::read_approval::gate(
            &fixture.context,
            "acme-reader",
            &evidence,
            &digest,
            Some(id),
            || panic!("revoked evidence cannot reach resource verification")
        ),
        Err(Error::RegistryRevoked { .. })
    ));
    let after = raw(&fixture);
    assert_eq!(after["read_requests"], before["read_requests"]);
    assert_eq!(after["policy_revision"], before["policy_revision"]);
}

#[test]
fn final_read_authority_refuses_busy_trust_before_creating_request() {
    if !crate::test_support::isolated_home(concat!(
        module_path!(),
        "::final_read_authority_refuses_busy_trust_before_creating_request"
    )) {
        return;
    }
    let fixture = trusted();
    let (evidence, digest) = read_evidence(&fixture);
    let before = raw(&fixture);
    let namespace = Namespace::user().unwrap();
    let _exclusive = namespace.exclusive_fixture().unwrap();
    assert!(matches!(
        crate::review::read_approval::gate(
            &fixture.context,
            "acme-reader",
            &evidence,
            &digest,
            None,
            || panic!("busy source cannot reach resource verification")
        ),
        Err(Error::Locked)
    ));
    assert_eq!(raw(&fixture)["read_requests"], before["read_requests"]);
}

#[cfg(any(target_os = "macos", target_os = "linux", windows))]
#[test]
fn final_run_authority_refuses_revoked_source_without_consumption_or_spawn() {
    if !crate::test_support::isolated_home(concat!(
        module_path!(),
        "::final_run_authority_refuses_revoked_source_without_consumption_or_spawn"
    )) {
        return;
    }
    let fixture = trusted();
    let mut context = fixture.context.clone();
    context.surface = "run".into();
    #[cfg(not(windows))]
    let (executable, arguments) = ("/bin/echo".to_owned(), vec!["Acme text".into()]);
    #[cfg(windows)]
    let (executable, arguments) = (
        std::path::PathBuf::from(winsafe::GetSystemDirectory().unwrap())
            .join("timeout.exe")
            .to_str()
            .unwrap()
            .to_owned(),
        vec!["/t".into(), "0".into(), "/nobreak".into()],
    );
    let request = crate::policy::Request {
        executable,
        arguments,
        cwd: context.project_root.clone(),
        project: context.project_root.clone(),
    };
    let resource = crate::execution::approval_fixture(&request, 5000).unwrap();
    let snapshot = crate::runtime::Project::open(fixture.project.path())
        .unwrap()
        .snapshot()
        .unwrap();
    let evidence = crate::execution::evaluate_approval(&snapshot, &request, &resource).unwrap();
    let (pending, _) = crate::review::run_approval::gate(
        &context,
        &evidence,
        &resource,
        None,
        || true,
        |_| panic!("Ask cannot spawn"),
    )
    .unwrap();
    let id = pending["approval"]["request_id"].as_str().unwrap();
    let mut operator = context.clone();
    operator.surface = "cli".into();
    crate::review::run_approval::authorize(&operator, id, 300, None, false).unwrap();
    let before = raw(&fixture);
    revoke(&fixture);
    assert!(matches!(
        crate::review::run_approval::gate(
            &context,
            &evidence,
            &resource,
            Some(id),
            || panic!("revoked evidence cannot verify"),
            |_| panic!("revoked evidence cannot spawn")
        ),
        Err(Error::RegistryRevoked { .. })
    ));
    let after = raw(&fixture);
    assert_eq!(after["run_requests"], before["run_requests"]);
    assert_eq!(after["policy_revision"], before["policy_revision"]);
}

#[test]
fn native_revocation_between_core_and_override_never_consumes_approved_grant() {
    if !crate::test_support::isolated_home(concat!(
        module_path!(),
        "::native_revocation_between_core_and_override_never_consumes_approved_grant"
    )) {
        return;
    }
    let fixture = trusted();
    let row = approved(&fixture);
    let before = raw(&fixture);
    let changed = fixture.clone();
    let _boundary = protocol::before_native_authority(move || revoke(&changed));
    let denied = hook(&fixture);
    assert_eq!(denied["hookSpecificOutput"]["permissionDecision"], "deny");
    assert!(
        denied["hookSpecificOutput"]["additionalContext"]
            .as_str()
            .unwrap()
            .contains("unavailable"),
        "{denied}"
    );
    let after = raw(&fixture);
    assert_eq!(after["project_id"], before["project_id"]);
    assert_eq!(after["policy_revision"], before["policy_revision"]);
    assert_eq!(
        after["hook_requests"][0], row,
        "known revocation cannot consume or rewrite the exact grant"
    );
}

#[test]
fn native_busy_final_guard_blocks_allowing_fallback_without_consuming_grant() {
    if !crate::test_support::isolated_home(concat!(
        module_path!(),
        "::native_busy_final_guard_blocks_allowing_fallback_without_consuming_grant"
    )) {
        return;
    }
    let fixture = trusted();
    install_registration(&fixture, "allow");
    let baseline = hook(&fixture);
    assert_ne!(
        baseline["hookSpecificOutput"]["permissionDecision"], "deny",
        "standing fallback is genuinely allowing: {baseline}"
    );
    assert!(
        baseline["hookSpecificOutput"]["additionalContext"]
            .as_str()
            .unwrap()
            .contains("allow")
    );
    install_registration(&fixture, "block");
    let row = approved(&fixture);
    install_registration(&fixture, "allow");
    let held = Rc::new(std::cell::RefCell::new(None));
    let target = held.clone();
    let _boundary = protocol::before_native_authority(move || {
        let namespace = Namespace::user().unwrap();
        *target.borrow_mut() = Some(namespace.exclusive_fixture().unwrap());
    });
    let denied = hook(&fixture);
    assert_eq!(denied["hookSpecificOutput"]["permissionDecision"], "deny");
    assert_eq!(raw(&fixture)["hook_requests"][0], row);
    drop(held.borrow_mut().take());
}

#[test]
fn native_revocation_does_not_spend_the_consumed_grants_one_use_followup() {
    if !crate::test_support::isolated_home(concat!(
        module_path!(),
        "::native_revocation_does_not_spend_the_consumed_grants_one_use_followup"
    )) {
        return;
    }
    let fixture = trusted();
    approved(&fixture);
    let allowed = hook(&fixture);
    assert_ne!(allowed["hookSpecificOutput"]["permissionDecision"], "deny");
    let before = raw(&fixture)["hook_requests"][0].clone();
    assert_eq!(before["status"], "consumed");
    assert_eq!(before["grant"]["followup_used"], false);
    let changed = fixture.clone();
    let _boundary = protocol::before_native_authority(move || revoke(&changed));
    let denied = hook_event(&fixture, agents::Event::Permission);
    assert_eq!(
        denied["hookSpecificOutput"]["decision"]["behavior"], "deny",
        "{denied}"
    );
    assert_eq!(raw(&fixture)["hook_requests"][0], before);
}

#[test]
fn native_session_override_cannot_use_revoked_source_after_initial_core_evaluation() {
    if !crate::test_support::isolated_home(concat!(
        module_path!(),
        "::native_session_override_cannot_use_revoked_source_after_initial_core_evaluation"
    )) {
        return;
    }
    let fixture = trusted();
    install_registration(&fixture, "block");
    hook(&fixture);
    let state = raw(&fixture);
    let reference = state["session_grants"]["contexts"]
        .as_object()
        .unwrap()
        .values()
        .next()
        .unwrap()["reference"]
        .clone();
    let context = context(&fixture, "codex");
    let proposed=session_grants::propose(&context,&json!({"session_ref":reference,"idempotency_key":"trusted-native-session","scope":[{"conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"exact","values":["push"]},"cwd":{"within":{"root":"project"}}}}]})).unwrap();
    session_grants::apply(&context, &proposed["apply_arguments"], true).unwrap();
    let allowed = hook(&fixture);
    assert_ne!(
        allowed["hookSpecificOutput"]["permissionDecision"], "deny",
        "session grant really covers the operation: {allowed}"
    );
    assert!(
        allowed["hookSpecificOutput"]["additionalContext"]
            .as_str()
            .unwrap()
            .contains("allowed by the active session grant")
    );
    let before = raw(&fixture);
    let changed = fixture.clone();
    let _boundary = protocol::before_native_authority(move || revoke(&changed));
    let denied = hook(&fixture);
    assert_eq!(denied["hookSpecificOutput"]["permissionDecision"], "deny");
    let after = raw(&fixture);
    assert_eq!(after["session_grants"], before["session_grants"]);
    assert_eq!(after["hook_requests"], before["hook_requests"]);
    assert_eq!(after["policy_revision"], before["policy_revision"]);
}

#[test]
fn native_final_callback_retains_source_guard_and_omitted_installed_rows_do_not_hide_revocation() {
    use clap::Parser as _;
    if !crate::test_support::isolated_home(concat!(
        module_path!(),
        "::native_final_callback_retains_source_guard_and_omitted_installed_rows_do_not_hide_revocation"
    )) {
        return;
    }
    let fixture = trusted();
    Store::open(fixture.project.path())
        .unwrap()
        .update_checkpoint(&fixture.context, &|| Ok(()), |state| {
            state.operator["installed"] = json!([]);
            Ok(Value::Null)
        })
        .unwrap();
    let project = crate::runtime::Project::open(fixture.project.path()).unwrap();
    let snapshot = project.snapshot().unwrap();
    let source = snapshot.native_source();
    let namespace = Namespace::user().unwrap();
    let previous = namespace.read(fixture.source.identity()).unwrap().unwrap();
    fixture.publish(2, true);
    let update = fetch(
        &fixture.context.fetcher,
        &fixture.source,
        Some(&previous),
        &json!({}),
        &|| Ok(()),
    )
    .unwrap();
    let logs = assert_fs::TempDir::new().unwrap();
    let cli = crate::cli::Cli::parse_from([
        "shield",
        "--debug-log-dir",
        logs.path().to_str().unwrap(),
        "status",
    ]);
    let log = std::sync::Arc::new(crate::diagnostics::Log::open(&cli));
    let trace = log
        .request("hook", "native_final_authority", None, None)
        .unwrap();
    {
        let _scope = trace.enter();
        let authority = source
            .acquire(Path::new(&fixture.context.project_root))
            .unwrap();
        agents::finalize_native(vec![authority], || {
            assert!(
                matches!(
                    namespace.publish(&update, Some(&previous)),
                    Err(Error::Locked)
                ),
                "exclusive revocation publication must wait until vendor decision finalization"
            );
            "Acme vendor disposition"
        });
        assert_eq!(trace.recorded_count("policy.preparations"), 0);
        assert_eq!(trace.recorded_count("policy.captures"), 0);
    }
    trace.finish("success");
    namespace.publish(&update, Some(&previous)).unwrap();
    assert!(
        matches!(
            source.acquire(Path::new(&fixture.context.project_root)),
            Err(Error::RegistryRevoked { .. })
        ),
        "actual qualified compiled artifact lock still guards source after optional installed metadata disappears"
    );
}
