//! Behavioral checks for the canonical project tool surface and authority separation.
mod application;
mod apply_cost;
mod cross_source;
mod documentation;
mod file_editing;
mod graphql;
mod guided;
mod historical_annotations;
mod integration_recovery;
mod pack_acquisition;
mod pack_identity;
mod pack_info;
mod pack_verbs;
mod parity;
mod reclamation;
mod registry_enrollment;
mod rule_proposals;
mod suggestion_workflow;
mod sweep_cost;
use super::*;

#[test]
fn executable_discovery_review_keeps_approval_arguments_below_host_output_limits() {
    let arguments = json!({"reference":"acme-review","change":"Configure Rust executables"});
    let mut data = json!({"apply_arguments":arguments,"proposal_id":"acme-proposal",
        "comparison":{"feedback_changes":"x".repeat(80 * 1024)},
        "rules":{"rules":"y".repeat(20 * 1024)},
        "report_resources":[{"uri":"shield-review://acme/comparison"}]});
    bound_policy_result(&mut data).unwrap();
    assert!(crate::json::fits(&data, 16 * 1024));
    assert_eq!(data["apply_arguments"], arguments);
    assert_eq!(
        data["report_resources"][0]["uri"],
        "shield-review://acme/comparison"
    );
    assert_eq!(data["missing"], json!(["rules", "comparison"]));
}

#[test]
fn an_oversized_policy_batch_leaves_the_existing_review_pending() {
    let (root, context, connection) = fixture();
    let existing = proposal(&context, &connection, "existing-review");
    let id = existing["data"]["proposal_id"].as_str().unwrap();
    let changes: Vec<_> = (0..12)
        .map(|index| {
            let mut rule = rule(&format!("acme-rule-{index}"), "allow");
            rule["reason"] = json!("Allow this Acme read for the reviewed operation. ".repeat(48));
            json!({"kind":"rule_add","rule":rule})
        })
        .collect();
    let oversized = invoke(
        &context,
        &connection,
        "policy_propose",
        json!({"changes":changes,"idempotency_key":"oversized-review"}),
    );
    assert_eq!(
        oversized["error"]["code"], "proposal_review_too_large",
        "{oversized}"
    );
    assert!(
        oversized["error"]["message"]
            .as_str()
            .unwrap()
            .contains("smaller proposals")
    );
    let state = Store::open(root.path()).unwrap().read_checkpoint().unwrap();
    assert_eq!(state.proposals.len(), 1);
    assert_eq!(state.proposals[0].id, id);
    assert_eq!(state.proposals[0].status, "pending");
    let shown = invoke(
        &context,
        &connection,
        "policy_show",
        json!({"proposal_id":id}),
    );
    assert_eq!(
        shown["data"]["apply_arguments"],
        existing["data"]["apply_arguments"]
    );
}
use crate::{
    configuration::Bundle,
    review::{self, storage::Store},
};
use std::{collections::BTreeMap, path::Path};

#[test]
fn policy_show_retains_authorization_event_pages_over_the_inline_approval_limit() {
    let (root, context, connection) = fixture();
    let result = proposal(&context, &connection, "acme-paged-review");
    let id = result["data"]["proposal_id"].as_str().unwrap();
    let expected = Store::open(root.path())
        .unwrap()
        .update_checkpoint(&context, &|| Ok(()), |state| {
            let source = state
                .authorization_events
                .iter()
                .find(|event| event["links"]["proposal_id"] == id)
                .unwrap()
                .clone();
            for _ in 0..8 {
                let mut event = source.clone();
                event["event_id"] = json!(state.id("event"));
                event["details"]["fixture"] = json!("Acme review evidence. ".repeat(200));
                state.authorization_events.push(event);
            }
            Ok(json!(
                state
                    .authorization_events
                    .iter()
                    .filter(|event| event["links"]["proposal_id"] == id)
                    .map(|event| event["event_id"].as_str().unwrap())
                    .collect::<std::collections::BTreeSet<_>>()
            ))
        })
        .unwrap();
    let mut args = json!({"proposal_id":id,"section":"authorization_events","limit":6});
    let mut seen = Vec::new();
    for page in 0..2 {
        let shown = invoke(&context, &connection, "policy_show", args.clone());
        assert!(shown["error"].is_null(), "{shown}");
        let data = &shown["data"];
        let events = data["authorization_events"].as_array().unwrap();
        if page == 0 {
            assert!(!crate::json::fits(data, 16 * 1024));
            assert_eq!(events.len(), 6);
            assert!(data["next_cursor"].is_string());
            args["cursor"] = data["next_cursor"].clone();
        } else {
            assert!(data["next_cursor"].is_null());
        }
        seen.extend(
            events
                .iter()
                .map(|event| event["event_id"].as_str().unwrap().to_owned()),
        );
    }
    seen.sort();
    assert_eq!(json!(seen), expected);
}

#[test]
fn terminal_history_handoff_omits_null_options_and_parses_with_clap() {
    use clap::Parser;
    let (_root, context, _) = fixture();
    let action = operator_action(
        &context,
        "history_configure",
        &json!({"enabled":false,"limit":null}),
    )
    .unwrap();
    let argv = action["argv"].as_array().unwrap();
    assert!(!argv.contains(&json!("--limit")));
    let invocation = std::iter::once("shield").chain(argv.iter().map(|arg| arg.as_str().unwrap()));
    crate::cli::Cli::try_parse_from(invocation).unwrap();
}

fn fixture() -> (assert_fs::TempDir, Context, Connection) {
    fixture_with(&Bundle::capture(r#"{"schema_version":1,"packs":[]}"#, &[], None).unwrap())
}

#[test]
fn explicit_history_project_is_read_only_and_does_not_rebind_the_connection() {
    let (_source, source, connection) = fixture();
    let target_root = Path::new(&source.project_root).join("worktree");
    std::fs::create_dir(&target_root).unwrap();
    let mut target = initialize_fixture(
        &target_root,
        &Bundle::capture(r#"{"schema_version":1,"packs":[]}"#, &[], None).unwrap(),
    );
    target.agent = Some("codex".into());
    Store::open(&target_root).unwrap().update(&target, &||Ok(()), |state| {
        for id in ["target-one", "target-two"] {
            for _ in 0..20 {
                let event=state.event("decision",&target,json!({"request_id":id}),json!({}),json!({"operation":{"kind":"argv","executable":"acme","arguments":[]},"evaluation":{"decision":"deny"}}));
                state.record(event);
            }
        }
        Ok(Value::Null)
    }).unwrap();
    let args = json!({"project":target.project_root,"denied":true,"agent":"codex","limit":1});
    let result = invoke(&source, &connection, "history_list", args.clone());
    assert!(result["error"].is_null(), "{result}");
    assert_eq!(result["project_id"], target.project_id);
    assert_eq!(result["data"]["entries"].as_array().unwrap().len(), 1);
    let route = &result["data"]["entries"][0]["observations_list"];
    assert_eq!(route["project"], target.project_root);
    let mut show = route.clone();
    show.as_object_mut().unwrap().remove("tool");
    let shown = invoke(&source, &connection, "history_show", show);
    assert!(shown["error"].is_null(), "{shown}");
    assert_eq!(shown["project_id"], target.project_id);
    let mut wrong_cursor = args.clone();
    wrong_cursor["cursor"] = result["data"]["next_cursor"].clone();
    wrong_cursor["project"] = json!(source.project_root);
    assert_eq!(
        invoke(&source, &connection, "history_list", wrong_cursor)["error"]["code"],
        "invalid_cursor"
    );
    let normal = invoke(&source, &connection, "history_list", json!({}));
    assert_eq!(normal["project_id"], source.project_id);
    assert_eq!(
        normal["data"]["entries"].as_array().unwrap().as_slice(),
        [] as [serde_json::Value; 0]
    );
    let unauthorized = invoke(
        &source,
        &connection,
        "disable",
        json!({"project":target.project_root}),
    );
    assert!(unauthorized["error"].is_object());
    let nested = target_root.join("nested");
    std::fs::create_dir(&nested).unwrap();
    assert!(
        invoke(
            &source,
            &connection,
            "history_list",
            json!({"project":nested})
        )["error"]
            .is_object()
    );
    let absent = assert_fs::TempDir::new().unwrap();
    assert!(
        invoke(
            &source,
            &connection,
            "history_list",
            json!({"project":absent.path()})
        )["error"]
            .is_object()
    );
    assert!(!absent.path().join(".shield").exists());
    let (_outside, outside, _) = fixture();
    assert!(
        invoke(
            &source,
            &connection,
            "history_list",
            json!({"project":outside.project_root})
        )["error"]
            .is_object()
    );
    assert!(
        invoke(
            &source,
            &connection,
            "history_list",
            json!({"project":"relative"})
        )["error"]
            .is_object()
    );
}

#[test]
fn history_previews_name_a_complete_stateless_observation_route() {
    let (root, context, connection) = fixture();
    Store::open(root.path()).unwrap().update(&context, &||Ok(()), |state| {
        for _ in 0..20 {
            let event=state.event("decision",&context,json!({"request_id":"acme-history"}),json!({}),json!({"operation":{"kind":"argv","executable":"acme","arguments":[]},"evaluation":{"decision":"ask"}}));
            state.record(event);
        }
        Ok(Value::Null)
    }).unwrap();
    let listed = invoke(&context, &connection, "history_list", json!({"limit":1}));
    assert!(listed["error"].is_null(), "{listed}");
    let entry = &listed["data"]["entries"][0];
    assert_eq!(entry["observations_total"], 20);
    assert!(entry["observations"].as_array().unwrap().len() <= 16);
    assert_eq!(entry["observations_list"]["section"], "observations");
    let connection = Connection::new("reconnected-history".into());
    let mut args = json!({"entry_id":"acme-history","section":"observations","limit":3});
    let mut seen = std::collections::BTreeSet::new();
    loop {
        let page = invoke(&context, &connection, "history_show", args.clone());
        assert!(page["error"].is_null(), "{page}");
        for event in page["data"]["observations"].as_array().unwrap() {
            assert!(seen.insert(event["event_id"].as_str().unwrap().to_owned()));
        }
        if page["data"]["next_cursor"].is_null() {
            break;
        }
        args["cursor"] = page["data"]["next_cursor"].clone();
    }
    assert_eq!(seen.len(), 20);
    args["cursor"] = json!("malformed");
    assert_eq!(
        invoke(&context, &connection, "history_show", args)["error"]["code"],
        "invalid_cursor"
    );
}

#[test]
fn first_runtime_use_keeps_diagnostic_handles() {
    let (_root, context, connection) = fixture();
    let status = call(&context, &connection, "status", &json!({}), &|| Ok(()));
    let handle = status["data"]["policy_candidate_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let proposed = call(
        &context,
        &connection,
        "policy_propose",
        &json!({"change":{"kind":"rule_add","rule":rule("after-diagnostics","allow")}}),
        &|| Ok(()),
    );
    assert!(proposed["error"].is_null(), "{proposed}");
    assert!(connection.views.lock().unwrap().get(&handle).is_ok());
}

#[test]
fn contextual_resolution_refusals_preserve_safe_mcp_invalid_input_messages() {
    let names = std::collections::BTreeSet::from(["profile".into()]);
    let error = crate::policy::Error::Contract("template names an undeclared variable")
        .in_resolution("inspect", "guidance.message", &names)
        .in_selection("pack:acme-selection");
    let (code, message) = error_fields(&error.into());
    assert_eq!(code, "invalid_input");
    for identifier in [
        "pack:acme-selection",
        "inspect",
        "guidance.message",
        "profile",
    ] {
        assert!(message.contains(identifier), "{message}");
    }
    assert!(!message.contains("retained state is invalid"));
}

#[test]
fn shield_mode_mcp_requires_the_exact_human_confirmation_even_while_disabled() {
    for agent in ["claude", "codex"] {
        let (root, mut context, connection) = fixture();
        context.agent = Some(agent.into());
        for (name, enabled) in [("disable", false), ("enable", true)] {
            let before = crate::review::mode::current(root.path()).unwrap();
            let unconfirmed = call(&context, &connection, name, &json!({}), &|| Ok(()));
            assert!(unconfirmed["error"].is_object(), "{unconfirmed}");
            let fabricated =
                call_with_confirmation(&context, &connection, name, &json!({}), &|| Ok(()), true);
            assert!(fabricated["error"].is_object(), "{fabricated}");
            assert!(
                admin_confirmation(
                    &context,
                    &connection,
                    name,
                    &json!({"confirmed":true}),
                    &|| Ok(())
                )
                .is_err()
            );
            assert_eq!(crate::review::mode::current(root.path()).unwrap(), before);
            let (_, declined) =
                admin_confirmation(&context, &connection, name, &json!({}), &|| Ok(()))
                    .unwrap()
                    .unwrap();
            discard_admin(&connection, declined);
            let (_, binding) =
                admin_confirmation(&context, &connection, name, &json!({}), &|| Ok(()))
                    .unwrap()
                    .unwrap();
            let foreign = Connection::new("acme-foreign".into());
            let rejected = call_with_bound_confirmation(
                &context,
                &foreign,
                name,
                &json!({}),
                &|| Ok(()),
                true,
                Some(binding),
            );
            assert!(rejected["error"].is_object());
            assert_eq!(crate::review::mode::current(root.path()).unwrap(), before);
            let result = call_with_bound_confirmation(
                &context,
                &connection,
                name,
                &json!({}),
                &|| Ok(()),
                true,
                Some(binding),
            );
            assert!(result["error"].is_null(), "{agent}: {result}");
            assert_eq!(result["data"]["enabled"], enabled);
            assert_eq!(crate::review::mode::current(root.path()).unwrap(), enabled);
            let replay = call_with_bound_confirmation(
                &context,
                &connection,
                name,
                &json!({}),
                &|| Ok(()),
                true,
                Some(binding),
            );
            assert!(replay["error"].is_object());
        }
    }
}

#[test]
fn candidate_artifact_failure_does_not_blame_tool_arguments() {
    let (_root, context, _connection) = fixture();
    let response = failure(&context, &crate::policy::Error::ArtifactMismatch.into());
    assert_eq!(response["error"]["code"], "candidate_error");
    let message = response["error"]["message"].as_str().unwrap();
    assert!(message.contains("The policy files were not changed"));
    assert!(message.contains("Continue without this proposal"));
    assert!(!message.contains("Invalid input"));
    assert!(response["error"]["operator_route"].is_null());
}

/// An enrolled project whose active policy is `candidate`.
fn fixture_with(candidate: &Bundle) -> (assert_fs::TempDir, Context, Connection) {
    let root = assert_fs::TempDir::new().unwrap();
    let candidates = BTreeMap::from([("project".into(), json!(candidate))]);
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
    context.agent = Some("acme".into());
    context.connection_id = Some("one".into());
    (root, context, Connection::new("one".into()))
}
fn invoke(context: &Context, connection: &Connection, name: &str, args: Value) -> Value {
    let result = call(context, connection, name, &args, &|| Ok(()));
    drop(args);
    result
}

#[test]
fn opencode_mcp_setup_preview_accepts_a_matching_external_entry_without_writes() {
    let (root, mut context, connection) = fixture();
    context.agent = Some("opencode".into());
    let binary = std::env::current_exe().unwrap().canonicalize().unwrap();
    let source = json!({"mcp":{"shield":{"type":"local","command":[binary,"mcp","serve","--project",".","--agent","opencode"],"enabled":true},"acme":{"type":"remote","url":"https://example.com/mcp"}}}).to_string();
    std::fs::write(root.path().join("opencode.json"), &source).unwrap();
    // The MCP preview alias maps to this canonical read-only administrative call.
    let result = invoke(
        &context,
        &connection,
        "mcp_setup",
        json!({"agents":["opencode"],"dry_run":true}),
    );
    assert!(result["error"].is_null(), "{result}");
    assert!(
        result["data"].to_string().contains("mcp_ownership"),
        "{result}"
    );
    assert!(result["data"].to_string().contains("external"), "{result}");
    assert_eq!(
        std::fs::read_to_string(root.path().join("opencode.json")).unwrap(),
        source
    );
    assert!(!root.path().join(".opencode/plugins/shield.js").exists());
}
fn rule(id: &str, decision: &str) -> Value {
    json!({"id":id,"decision":decision,"conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"exact","values":["read"]}},"reason":"Acme review"})
}
fn request(context: &Context) -> Value {
    json!({"input":{"kind":"argv","executable":"acme","arguments":["read"],"original_input":{"command":"acme read"}},"context":{"cwd":context.project_root,"environment":{}}})
}
fn proposal(context: &Context, connection: &Connection, id: &str) -> Value {
    invoke(
        context,
        connection,
        "policy_propose",
        json!({"change":{"kind":"rule_add","rule":rule(id,"allow")},"idempotency_key":id}),
    )
}

#[test]
fn record_only_proposal_inspection_exposes_lazy_handles_with_exact_arguments() {
    use clap::Parser as _;
    let (root, context, connection) = fixture();
    let proposed = proposal(&context, &connection, "lazy-inspection");
    assert!(proposed["error"].is_null(), "{proposed}");
    *connection.views.lock().unwrap() = views::Views::new("acme-fresh-inspection".into());
    let checkpoint = root.path().join(".shield/state/state.json");
    let before = std::fs::read(&checkpoint).unwrap();
    let logs = assert_fs::TempDir::new().unwrap();
    let cli = crate::cli::Cli::parse_from([
        "shield",
        "--debug-log-dir",
        logs.path().to_str().unwrap(),
        "status",
    ]);
    let log = std::sync::Arc::new(crate::diagnostics::Log::open(&cli));
    let trace = log.request("command", "policy_show", None, None).unwrap();
    let inspected;
    {
        let _scope = trace.enter();
        inspected = invoke(
            &context,
            &connection,
            "policy_show",
            json!({"proposal_id":proposed["data"]["proposal_id"]}),
        );
        assert!(inspected["error"].is_null(), "{inspected}");
        assert_eq!(trace.recorded_count("policy.preparations"), 0);
        assert_eq!(trace.recorded_count("history.captures"), 0);
    }
    trace.finish("success");
    assert_eq!(
        inspected["data"]["apply_arguments"],
        proposed["data"]["apply_arguments"]
    );
    assert_eq!(std::fs::read(&checkpoint).unwrap(), before);
    let mut input = request(&context);
    input["candidate_id"] = inspected["data"]["after_candidate_id"].clone();
    let evaluated = invoke(&context, &connection, "policy_evaluate", input);
    assert_eq!(evaluated["data"]["decision"], "allow", "{evaluated}");
}

fn dependency_registry(
    root: &assert_fs::TempDir,
    context: &Context,
    dependencies: &Value,
    corrupt: bool,
) -> assert_fs::TempDir {
    let source = assert_fs::TempDir::new().unwrap();
    let directory = source.path().canonicalize().unwrap();
    let parent = json!({"schema_version":1,"id":"acme/parent","version":"1.0.0","description":"Acme dependency fixture","license":"UNLICENSED","maintainers":["Acme"],"engines":{"shield":env!("CARGO_PKG_VERSION")},"capabilities":["dependencies-v1","structured-rules-v1"],"dependencies":dependencies,"rules":[],"kind":"pack"}).to_string();
    let child = json!({"schema_version":1,"id":"acme/child","version":"1.0.0","description":"Acme child","license":"UNLICENSED","maintainers":["Acme"],"engines":{"shield":env!("CARGO_PKG_VERSION")},"capabilities":["argument-rules-v1","structured-rules-v1"],"rules":[{"id":"read","reason":"Acme profile fixture","conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"exact","values":["read"]}},"decision":{"loose":"allow","recommended":"ask","strict":"deny"}}],"kind":"pack"}).to_string();
    std::fs::write(directory.join("parent.yml"), &parent).unwrap();
    std::fs::write(directory.join("child.yml"), &child).unwrap();
    let catalog = json!({"schema_version":1,"packs":[{"id":"acme/parent","version":"1.0.0","path":"parent.yml","sha256":crate::policy::fingerprint(parent.as_bytes())},{"id":"acme/child","version":"1.0.0","path":"child.yml","sha256":if corrupt { "0".repeat(64) } else { crate::policy::fingerprint(child.as_bytes()) }}]});
    std::fs::write(directory.join("catalog.json"), catalog.to_string()).unwrap();
    Store::open(root.path()).unwrap().update(context, &|| Ok(()), |state| {
        state.operator["sources"]["acme-dependencies"] = json!({"identity":crate::policy::fingerprint(directory.to_str().unwrap().as_bytes()),"location":directory,"key":null});
        Ok(json!({}))
    }).unwrap();
    source
}

fn dependency_proposal(context: &Context, connection: &Connection, key: &str) -> Value {
    invoke(
        context,
        connection,
        "policy_propose",
        json!({"change":{"kind":"pack_add","reference":"acme-dependencies/acme/parent"},"idempotency_key":key}),
    )
}

fn registry_release(
    source: &assert_fs::TempDir,
    id: &str,
    version: &str,
    requirement: Option<&str>,
) {
    let template = if requirement.is_some() {
        "parent.yml"
    } else {
        "child.yml"
    };
    let mut pack: Value =
        serde_json::from_slice(&std::fs::read(source.path().join(template)).unwrap()).unwrap();
    pack["id"] = json!(id);
    pack["version"] = json!(version);
    if let Some(requirement) = requirement {
        pack["dependencies"][0]["requirement"] = json!(requirement);
    }
    let path = format!("{}-{version}.yml", id.replace('/', "-"));
    let bytes = pack.to_string();
    std::fs::write(source.path().join(&path), &bytes).unwrap();
    let catalog_path = source.path().join("catalog.json");
    let mut catalog: Value =
        serde_json::from_slice(&std::fs::read(&catalog_path).unwrap()).unwrap();
    catalog["packs"].as_array_mut().unwrap().push(json!({"id":id,"version":version,"path":path,"sha256":crate::policy::fingerprint(bytes.as_bytes())}));
    std::fs::write(catalog_path, catalog.to_string()).unwrap();
}

fn pack_change(context: &Context, connection: &Connection, change: Value, key: &str) -> Value {
    let result = invoke(
        context,
        connection,
        "policy_propose",
        json!({"change":change,"idempotency_key":key}),
    );
    drop(change);
    result
}

fn commit_pack_change(context: &Context, connection: &Connection, change: Value, key: &str) {
    let proposed = pack_change(context, connection, change, key);
    assert_eq!(proposed["error"], Value::Null, "{proposed}");
    if proposed["data"]["unchanged"] == true {
        assert_eq!(proposed["data"]["committed"], false, "{proposed}");
        assert!(proposed["data"].get("proposal_id").is_none(), "{proposed}");
        return;
    }
    review::operator::commit_proposal_fixture(
        context,
        proposed["data"]["proposal_id"].as_str().unwrap(),
    )
    .unwrap();
}

#[test]
fn a_pack_proposal_applies_from_the_per_user_store_after_the_folder_cache_is_gone() {
    let (root, context, connection) = fixture();
    let _source = dependency_registry(&root, &context, &json!([]), false);
    let proposed = pack_change(
        &context,
        &connection,
        json!({"kind":"pack_add","reference":"acme-dependencies/acme/child"}),
        "install-from-store",
    );
    assert_eq!(proposed["error"], Value::Null, "{proposed}");
    // The proposal holds file texts; its pack artifact lives in the per-user
    // store, so a folder without its ignored cache still applies it.
    let _ = std::fs::remove_dir_all(root.path().join(".shield/cache"));
    review::operator::commit_proposal_fixture(
        &context,
        proposed["data"]["proposal_id"].as_str().unwrap(),
    )
    .unwrap();
    let state = review::storage::Store::open(root.path())
        .unwrap()
        .read_checkpoint()
        .unwrap();
    assert_eq!(state.operator["installed"][0]["id"], "acme/child");
    assert!(crate::review::files::load(root.path()).is_ok());
}

#[test]
fn status_fetches_a_locked_pack_this_machine_lacks() {
    let (root, context, connection) = fixture();
    let _source = dependency_registry(&root, &context, &json!([]), false);
    commit_pack_change(
        &context,
        &connection,
        json!({"kind":"pack_add","reference":"acme-dependencies/acme/child"}),
        "install",
    );
    // A fresh worktree has the files but not the ignored artifact cache, and
    // this machine has never stored the release.
    let cache = root.path().join(".shield/cache");
    let artifacts: Vec<_> = std::fs::read_dir(&cache)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    std::fs::remove_dir_all(&cache).unwrap();
    let store = super::super::files::artifact_store().unwrap();
    for name in &artifacts {
        let _ = std::fs::remove_file(store.join(name));
    }
    let status = review::project::status(&context).unwrap();
    assert_eq!(
        status["policy_files"]["fetched"],
        json!(["acme/child@1.0.0"]),
        "{status}"
    );
    assert!(status["policy_files"]["digest"].is_string(), "{status}");
    for name in &artifacts {
        assert!(store.join(name).exists(), "{name:?}");
    }
}

/// A project with `acme/child` installed whose artifact this machine has lost.
fn lost_child(
    root: &assert_fs::TempDir,
    context: &Context,
    connection: &Connection,
) -> std::path::PathBuf {
    commit_pack_change(
        context,
        connection,
        json!({"kind":"pack_add","reference":"acme-dependencies/acme/child"}),
        "install",
    );
    let cache = root.path().join(".shield/cache");
    let store = super::super::files::artifact_store().unwrap();
    for entry in std::fs::read_dir(&cache).unwrap() {
        let _ = std::fs::remove_file(store.join(entry.unwrap().file_name()));
    }
    std::fs::remove_dir_all(&cache).unwrap();
    store
}

#[test]
fn one_unfetchable_release_does_not_stop_the_others() {
    let (root, context, connection) = fixture();
    let _source = dependency_registry(&root, &context, &json!([]), false);
    lost_child(&root, &context, &connection);
    let lock = root.path().join(".shield/shield.lock");
    let mut locked: Value = serde_json::from_str(&std::fs::read_to_string(&lock).unwrap()).unwrap();
    locked["packs"].as_array_mut().unwrap().insert(
        0,
        json!({"id":"acme/ghost","version":"1.0.0","digest":"a".repeat(64),"source_identity":"acme-unconfigured"}),
    );
    std::fs::write(&lock, locked.to_string()).unwrap();
    let status = review::project::status(&context).unwrap();
    assert_eq!(
        status["policy_files"]["fetched"],
        json!(["acme/child@1.0.0"]),
        "{status}"
    );
    let failures = &status["policy_files"]["fetch_failures"];
    assert_eq!(failures[0]["release"], "acme/ghost@1.0.0", "{status}");
    assert!(
        failures[0]["error"]
            .as_str()
            .unwrap()
            .contains("not configured"),
        "{status}"
    );
}

#[test]
fn a_fetched_release_whose_bytes_differ_from_the_lock_is_refused() {
    let (root, context, connection) = fixture();
    let source = dependency_registry(&root, &context, &json!([]), false);
    let store = lost_child(&root, &context, &connection);
    // The registry now serves other bytes for the same release.
    let path = source.path().join("child.yml");
    let mut child: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    child["description"] = json!("Acme substituted");
    let bytes = child.to_string();
    std::fs::write(&path, &bytes).unwrap();
    let catalog_path = source.path().join("catalog.json");
    let mut catalog: Value =
        serde_json::from_slice(&std::fs::read(&catalog_path).unwrap()).unwrap();
    catalog["packs"][1]["sha256"] = json!(crate::policy::fingerprint(bytes.as_bytes()));
    std::fs::write(&catalog_path, catalog.to_string()).unwrap();
    let status = review::project::status(&context).unwrap();
    let failures = &status["policy_files"]["fetch_failures"];
    assert_eq!(failures[0]["release"], "acme/child@1.0.0", "{status}");
    assert!(
        failures[0]["error"]
            .as_str()
            .unwrap()
            .contains("differ from the lock"),
        "{status}"
    );
    assert!(
        !store
            .join(format!(
                "{}.yml",
                crate::policy::fingerprint(bytes.as_bytes())
            ))
            .exists()
    );
}

#[test]
fn cli_and_mcp_follow_files_whose_packs_come_from_the_store() {
    let (root, context, connection) = fixture();
    let _source = dependency_registry(&root, &context, &json!([]), false);
    commit_pack_change(
        &context,
        &connection,
        json!({"kind":"pack_add","reference":"acme-dependencies/acme/child"}),
        "install",
    );
    // A fresh worktree after a pull: the ignored artifact cache is absent and
    // the per-user store has the release. CLI and MCP follow the files hooks
    // enforce instead of keeping an older copy.
    std::fs::remove_dir_all(root.path().join(".shield/cache")).unwrap();
    let state = review::project::load(&context).unwrap();
    assert_eq!(
        state.followed_files.as_deref(),
        Some(super::super::files::digest(root.path()).unwrap().as_str())
    );
}

#[test]
fn dependency_removal_prunes_transitive_orphans_but_retains_explicit_selections() {
    let (root, context, connection) = fixture();
    let source = dependency_registry(
        &root,
        &context,
        &json!([{"pack":"acme/child","requirement":"^1"}]),
        false,
    );
    registry_release(&source, "acme/bridge", "1.0.0", Some("^1"));
    let parent_path = source.path().join("parent.yml");
    let mut parent: Value = serde_json::from_slice(&std::fs::read(&parent_path).unwrap()).unwrap();
    parent["dependencies"][0]["pack"] = json!("acme/bridge");
    let bytes = parent.to_string();
    std::fs::write(parent_path, &bytes).unwrap();
    let catalog_path = source.path().join("catalog.json");
    let mut catalog: Value =
        serde_json::from_slice(&std::fs::read(&catalog_path).unwrap()).unwrap();
    catalog["packs"][0]["sha256"] = json!(crate::policy::fingerprint(bytes.as_bytes()));
    std::fs::write(catalog_path, catalog.to_string()).unwrap();
    commit_pack_change(
        &context,
        &connection,
        json!({"kind":"pack_add","reference":"acme-dependencies/acme/parent"}),
        "install",
    );
    commit_pack_change(
        &context,
        &connection,
        json!({"kind":"pack_add","reference":"acme-dependencies/acme/child"}),
        "direct-child",
    );
    drop(source);
    commit_pack_change(
        &context,
        &connection,
        json!({"kind":"pack_remove","reference":"acme/parent"}),
        "prune",
    );
    let active = Store::open(root.path()).unwrap().read().unwrap();
    let rows = active.operator["installed"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["id"], "acme/child");
    assert_eq!(rows[0]["direct"], true);
    assert_eq!(rows[0]["incoming"], json!([]));
}

#[test]
fn dependency_updates_capture_atomic_closures_preserve_rules_and_reject_stale_review() {
    let (root, context, connection) = fixture();
    let source = dependency_registry(
        &root,
        &context,
        &json!([{"pack":"acme/child","requirement":"^1"}]),
        false,
    );
    let rules = proposal(&context, &connection, "custom");
    review::operator::commit_proposal_fixture(
        &context,
        rules["data"]["proposal_id"].as_str().unwrap(),
    )
    .unwrap();
    commit_pack_change(
        &context,
        &connection,
        json!({"kind":"pack_add","reference":"acme-dependencies/acme/parent"}),
        "install",
    );
    review::settings::registries_fixture(&context, |registries| {
        let source = registries.sources.remove("acme-dependencies").unwrap();
        registries.sources.insert("renamed".into(), source);
    });
    let before = Store::open(root.path()).unwrap().read().unwrap();
    registry_release(&source, "acme/parent", "2.0.0", Some("^2"));
    registry_release(&source, "acme/child", "2.0.0", None);
    let proposed = pack_change(
        &context,
        &connection,
        json!({"kind":"pack_update"}),
        "update",
    );
    assert_eq!(proposed["error"], Value::Null, "{proposed}");
    let store = Store::open(root.path()).unwrap();
    let mut pending = store.read().unwrap();
    assert_eq!(pending.policy_revision, before.policy_revision);
    let index = pending
        .proposals
        .iter()
        .position(|p| p.id == proposed["data"]["proposal_id"])
        .unwrap();
    store
        .hydrate_proposal(&mut pending.proposals[index])
        .unwrap();
    let updated = &pending.proposals[index];
    assert_eq!(
        updated.candidate["rules"],
        before.candidates["project"]["rules"]
    );
    for row in updated.candidate_metadata["installed"].as_array().unwrap() {
        assert_eq!(row["version"], "2.0.0");
        let old = before.operator["installed"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["id"] == row["id"])
            .unwrap();
        assert_eq!(row["selector"], old["selector"]);
        assert_eq!(row["profile"], old["profile"]);
    }
    // A second proposal is stale once the complete update is committed.
    let mut independent = context.clone();
    independent.connection_id = Some("independent-review".into());
    let independent_connection = Connection::new("independent-review".into());
    let stale = pack_change(
        &independent,
        &independent_connection,
        json!({"kind":"pack_remove","reference":"acme/parent"}),
        "stale-remove",
    );
    assert_eq!(stale["error"], Value::Null);
    drop(source);
    review::operator::commit_proposal_fixture(&context, &updated.id).unwrap();
    assert!(
        review::operator::commit_proposal_fixture(
            &context,
            stale["data"]["proposal_id"].as_str().unwrap()
        )
        .is_err()
    );
    let active = Store::open(root.path()).unwrap().read().unwrap();
    assert!(
        active.operator["installed"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["version"] == "2.0.0")
    );
    assert_eq!(
        active.candidates["project"]["rules"],
        before.candidates["project"]["rules"]
    );
}

#[test]
fn dependency_updates_prune_dependencies_no_longer_declared_by_selected_releases() {
    let (root, context, connection) = fixture();
    let source = dependency_registry(
        &root,
        &context,
        &json!([{"pack":"acme/child","requirement":"^1"}]),
        false,
    );
    commit_pack_change(
        &context,
        &connection,
        json!({"kind":"pack_add","reference":"acme-dependencies/acme/parent"}),
        "install",
    );
    registry_release(&source, "acme/parent", "2.0.0", None);
    commit_pack_change(
        &context,
        &connection,
        json!({"kind":"pack_update"}),
        "prune-on-update",
    );
    let active = Store::open(root.path()).unwrap().read().unwrap();
    let rows = active.operator["installed"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["id"], "acme/parent");
    assert_eq!(rows[0]["version"], "2.0.0");
    assert_eq!(rows[0]["direct"], true);
}

#[test]
fn unqualified_pack_update_keeps_its_nondefault_registry() {
    let (root, context, connection) = fixture();
    let source = dependency_registry(&root, &context, &json!([]), false);
    assert_ne!(
        review::registries::effective(Some(&context))
            .unwrap()
            .default_alias(),
        "acme-dependencies"
    );
    commit_pack_change(
        &context,
        &connection,
        json!({"kind":"pack_add","reference":"acme-dependencies/acme/child"}),
        "add-nondefault-child",
    );
    let store = Store::open(root.path()).unwrap();
    let before = store.read().unwrap().operator["installed"][0].clone();
    registry_release(&source, "acme/child", "1.1.0", None);
    commit_pack_change(
        &context,
        &connection,
        json!({"kind":"pack_update","reference":"child"}),
        "update-unique-child",
    );
    let after = store.read().unwrap().operator["installed"][0].clone();
    assert_eq!(after["version"], "1.1.0");
    assert_eq!(after["selector"], before["selector"]);
    assert_eq!(after["source_identity"], before["source_identity"]);
}

#[test]
fn dependency_updates_honor_pinned_parents_shared_constraints_and_explicit_dependency_updates() {
    let (root, context, connection) = fixture();
    let source = dependency_registry(
        &root,
        &context,
        &json!([{"pack":"acme/child","requirement":"^1"}]),
        false,
    );
    commit_pack_change(
        &context,
        &connection,
        json!({"kind":"pack_add","reference":"acme-dependencies/acme/parent@1.0.0"}),
        "pinned",
    );
    registry_release(&source, "acme/child", "1.1.0", None);
    registry_release(&source, "acme/child", "2.0.0", None);
    let before = Store::open(root.path()).unwrap().read().unwrap();
    for target in [None, Some("acme/parent")] {
        let result = crate::review::packs::prepare_change(
            &context,
            &before,
            &review::changes::Change::PackUpdate {
                reference: target.map(str::to_owned),
            },
        )
        .unwrap();
        assert_ne!(result.0, before.candidates["project"]);
        let rows = result.1["installed"].as_array().unwrap();
        assert_eq!(
            rows.iter().find(|r| r["id"] == "acme/parent").unwrap()["pinned"],
            true
        );
        assert_eq!(
            rows.iter().find(|r| r["id"] == "acme/child").unwrap()["version"],
            "1.1.0"
        );
    }
    commit_pack_change(
        &context,
        &connection,
        json!({"kind":"pack_update","reference":"acme/child"}),
        "child-update",
    );
    let active = Store::open(root.path()).unwrap().read().unwrap();
    assert_eq!(active.operator["installed"][0]["version"], "1.1.0");
    assert_eq!(active.operator["installed"][1]["version"], "1.0.0");
    assert_eq!(active.operator["installed"][1]["pinned"], true);
    registry_release(&source, "acme/tracking", "1.0.0", Some(">=1, <3"));
    commit_pack_change(
        &context,
        &connection,
        json!({"kind":"pack_add","reference":"acme-dependencies/acme/tracking"}),
        "tracking",
    );
    registry_release(&source, "acme/child", "1.2.0", None);
    commit_pack_change(
        &context,
        &connection,
        json!({"kind":"pack_update"}),
        "shared-update",
    );
    let active = Store::open(root.path()).unwrap().read().unwrap();
    let child = active.operator["installed"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == "acme/child")
        .unwrap();
    assert_eq!(
        child["version"], "1.2.0",
        "pinned parent's ^1 excludes 2.0.0"
    );
    assert_eq!(child["incoming"].as_array().unwrap().len(), 2);
    commit_pack_change(
        &context,
        &connection,
        json!({"kind":"pack_add","reference":"acme-dependencies/acme/child@1.2.0"}),
        "pin-child",
    );
    registry_release(&source, "acme/child", "1.3.0", None);
    let before = Store::open(root.path()).unwrap().read().unwrap();
    let result = crate::review::packs::prepare_change(
        &context,
        &before,
        &review::changes::Change::PackUpdate { reference: None },
    )
    .unwrap();
    assert_ne!(result.0, before.candidates["project"]);
    let child = result.1["installed"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == "acme/child")
        .unwrap();
    assert_eq!(child["version"], "1.3.0");
    assert_eq!(child["pinned"], true);
}

#[test]
fn dependency_removal_preserves_shared_and_direct_packs_and_reports_required_identities_offline() {
    let (root, context, connection) = fixture();
    let source = dependency_registry(
        &root,
        &context,
        &json!([{"pack":"acme/child","requirement":"^1"}]),
        false,
    );
    registry_release(&source, "acme/other", "1.0.0", Some("^1"));
    for (reference, key) in [("parent", "parent"), ("other", "other")] {
        commit_pack_change(
            &context,
            &connection,
            json!({"kind":"pack_add","reference":format!("acme-dependencies/acme/{reference}")}),
            key,
        );
    }
    drop(source);
    let before = Store::open(root.path()).unwrap().read().unwrap();
    let blocked = pack_change(
        &context,
        &connection,
        json!({"kind":"pack_remove","reference":"acme/child"}),
        "required",
    );
    assert_eq!(blocked["error"]["code"], "dependency_required");
    let message = blocked["error"]["message"].as_str().unwrap();
    assert!(message.contains("acme/parent") && message.contains("acme/other"));
    let after = Store::open(root.path()).unwrap().read().unwrap();
    assert_eq!(after.operator, before.operator);
    assert_eq!(after.policy_revision, before.policy_revision);
    assert_eq!(after.proposals.len(), before.proposals.len());
    commit_pack_change(
        &context,
        &connection,
        json!({"kind":"pack_remove","reference":"acme/parent"}),
        "remove-parent",
    );
    let remaining = Store::open(root.path()).unwrap().read().unwrap();
    assert_eq!(remaining.operator["installed"].as_array().unwrap().len(), 2);
    commit_pack_change(
        &context,
        &connection,
        json!({"kind":"pack_remove","reference":"acme/other"}),
        "remove-other",
    );
    assert_eq!(
        Store::open(root.path()).unwrap().read().unwrap().operator["installed"],
        json!([])
    );
}

#[test]
fn dependency_update_verification_failures_leave_active_policy_and_proposals_unchanged() {
    let (root, context, connection) = fixture();
    let source = dependency_registry(
        &root,
        &context,
        &json!([{"pack":"acme/child","requirement":"^1"}]),
        false,
    );
    commit_pack_change(
        &context,
        &connection,
        json!({"kind":"pack_add","reference":"acme-dependencies/acme/parent"}),
        "install",
    );
    registry_release(&source, "acme/child", "1.1.0", None);
    std::fs::write(source.path().join("acme-child-1.1.0.yml"), "corrupt").unwrap();
    let before = Store::open(root.path()).unwrap().read().unwrap();
    let result = pack_change(
        &context,
        &connection,
        json!({"kind":"pack_update"}),
        "bad-update",
    );
    assert_ne!(result["error"], Value::Null);
    let after = Store::open(root.path()).unwrap().read().unwrap();
    assert_eq!(after.policy_revision, before.policy_revision);
    assert_eq!(after.operator, before.operator);
    assert_eq!(after.proposals.len(), before.proposals.len());
}

#[test]
fn dependency_proposals_capture_complete_verified_closures_and_activate_once() {
    let (root, context, connection) = fixture();
    let source = dependency_registry(
        &root,
        &context,
        &json!([{"pack":"acme/child","requirement":"^1","profiles":{"loose":"recommended","recommended":"strict","strict":"strict"}}]),
        false,
    );
    let before = Store::open(root.path()).unwrap().read().unwrap();
    let proposed = dependency_proposal(&context, &connection, "closure");
    assert_eq!(proposed["error"], Value::Null, "{proposed}");
    let store = Store::open(root.path()).unwrap();
    let mut pending = store.read().unwrap();
    assert_eq!(pending.policy_revision, before.policy_revision);
    store.hydrate_proposal(&mut pending.proposals[0]).unwrap();
    let row = &pending.proposals[0];
    let installed = row.candidate_metadata["installed"].as_array().unwrap();
    assert_eq!(installed.len(), 2);
    let child = installed.iter().find(|p| p["id"] == "acme/child").unwrap();
    assert_eq!(child["profile"], "strict");
    assert_eq!(child["direct"], false);
    assert_eq!(child["incoming"][0]["requirement"], "^1");
    assert_eq!(row.candidate["packs"].as_array().unwrap().len(), 2);
    assert_eq!(
        dependency_proposal(&context, &connection, "closure")["data"]["proposal_id"],
        proposed["data"]["proposal_id"]
    );
    // Confirmation uses captured exact bytes even when the source is unavailable.
    drop(source);
    review::operator::commit_proposal_fixture(&context, row.id.as_str()).unwrap();
    assert!(review::operator::commit_proposal_fixture(&context, row.id.as_str()).is_err());
    let result = invoke(&context, &connection, "policy_evaluate", request(&context));
    assert_eq!(result["data"]["decision"], "deny", "{result}");
    let locked: Value =
        serde_json::from_slice(&std::fs::read(root.path().join(".shield/shield.lock")).unwrap())
            .unwrap();
    // The lock records each installed row without a local source location.
    let portable: Vec<_> = installed
        .iter()
        .map(review::publication::projections::portable)
        .collect();
    assert_eq!(locked["packs"], json!(portable));
    assert!(!locked.to_string().contains(&*root.path().to_string_lossy()));
}

#[test]
fn invalid_dependency_acquisition_and_cycles_preserve_active_policy_without_proposals() {
    for (dependencies, corrupt) in [
        (json!([{"pack":"acme/child","requirement":"^1"}]), true),
        (json!([{"pack":"acme/parent","requirement":"^1"}]), false),
        (json!([{"pack":"acme/missing","requirement":"^1"}]), false),
        (json!([{"pack":"acme/child","requirement":"^2"}]), false),
        (
            json!([{"pack":"acme/child","requirement":"^1","registry":"foreign"}]),
            false,
        ),
    ] {
        let (root, context, connection) = fixture();
        let _source = dependency_registry(&root, &context, &dependencies, corrupt);
        let before = Store::open(root.path()).unwrap().read().unwrap();
        let result = dependency_proposal(&context, &connection, "invalid");
        assert_ne!(result["error"], Value::Null, "{result}");
        let after = Store::open(root.path()).unwrap().read().unwrap();
        assert_eq!(after.policy_revision, before.policy_revision);
        assert_eq!(after.operator, before.operator);
        assert!(after.proposals.is_empty());
    }
}

#[test]
fn stale_dependency_proposals_cannot_publish_after_another_policy_change() {
    let (root, context, connection) = fixture();
    let _source = dependency_registry(
        &root,
        &context,
        &json!([{"pack":"acme/child","requirement":"^1"}]),
        false,
    );
    let first = dependency_proposal(&context, &connection, "closure");
    let other = proposal(&context, &connection, "other");
    review::operator::commit_proposal_fixture(
        &context,
        other["data"]["proposal_id"].as_str().unwrap(),
    )
    .unwrap();
    let before = Store::open(root.path()).unwrap().read().unwrap();
    assert!(
        review::operator::commit_proposal_fixture(
            &context,
            first["data"]["proposal_id"].as_str().unwrap()
        )
        .is_err()
    );
    assert_eq!(
        Store::open(root.path())
            .unwrap()
            .read()
            .unwrap()
            .policy_revision,
        before.policy_revision
    );
}

#[test]
fn sample_comparison_reports_only_a_witness_and_never_a_whole_policy_proof() {
    let (_root, context, connection) = fixture();
    let proposal = proposal(&context, &connection, "sample");
    let data = &proposal["data"];
    let mut args = request(&context);
    args["domain"] = json!("sample");
    args["before_candidate_id"] = data["before_candidate_id"].clone();
    args["after_candidate_id"] = data["after_candidate_id"].clone();
    let result = invoke(&context, &connection, "policy_compare", args);
    assert_eq!(result["error"], Value::Null, "{result}");
    assert_eq!(result["data"]["relation"], "unknown");
    assert_eq!(result["data"]["complete"], false);
    assert_eq!(result["data"]["witnesses"][0]["relation"], "expansive");
    assert_eq!(result["data"]["witnesses"][0]["before"]["decision"], "ask");
    assert_eq!(result["data"]["witnesses"][0]["after"]["decision"], "allow");
}

#[test]
fn effect_predicates_report_incomplete_coverage_and_cannot_establish_an_allowance() {
    let root = assert_fs::TempDir::new().unwrap();
    let mut effect_rule = rule("network", "allow");
    effect_rule["conditions"]["executable"] = json!("git");
    effect_rule["conditions"]["arguments"] = json!({"kind":"any"});
    effect_rule["conditions"]["possible_effects"] = json!(["network"]);
    let source = crate::policy::rules::document(&[effect_rule])
        .unwrap()
        .to_string();
    let bundle = Bundle::capture(
        "schema_version: 1\npacks: []\nrules: rules.yml\n",
        &[],
        Some(&source),
    )
    .unwrap();
    let context = initialize_fixture(root.path(), &bundle);
    let connection = Connection::new("effects".into());
    let mut args = request(&context);
    args["input"]["executable"] = json!("git");
    args["input"]["arguments"] = json!(["push"]);
    let result = invoke(&context, &connection, "policy_evaluate", args);
    assert_eq!(result["error"], Value::Null, "{result}");
    assert_eq!(result["data"]["decision"], "ask");
    assert_eq!(result["data"]["analysis_coverage"]["complete"], false);
}

#[test]
fn a_cached_diagnostic_cannot_hide_changed_policy_files() {
    let (root, context, connection) = fixture();
    let evaluated = invoke(&context, &connection, "policy_evaluate", request(&context));
    let id = evaluated["data"]["candidate_id"].clone();
    std::fs::write(
        root.path().join(".shield/config.yml"),
        "schema_version: 1\npacks: []\nrules: missing.yml\n",
    )
    .unwrap();
    let current = invoke(&context, &connection, "policy_evaluate", request(&context));
    assert!(!current["error"].is_null(), "{current}");
    let mut retained = request(&context);
    retained["candidate_id"] = id;
    assert_eq!(
        invoke(&context, &connection, "policy_evaluate", retained)["error"],
        Value::Null
    );
}

#[test]
fn unsupported_shell_dialects_are_valid_incomplete_diagnostics() {
    let (_root, context, connection) = fixture();
    for dialect in ["powershell", "cmd"] {
        let mut args = request(&context);
        args["input"] = json!({"kind":"shell","source":"acme read","dialect":dialect,"dialect_version":null,"original_input":{"command":"acme read"}});
        let result = invoke(&context, &connection, "policy_evaluate", args);
        assert_eq!(result["error"], Value::Null, "{result}");
        assert_eq!(result["data"]["decision"], "ask");
        assert_eq!(result["data"]["analysis_coverage"]["complete"], false);
        assert_eq!(
            result["data"]["analysis_coverage"]["unknown"],
            "shell dialect analysis is unsupported"
        );
    }
}

#[test]
fn oversized_policy_evidence_keeps_identity_and_marks_missing_details() {
    let mut data = json!({"proposal_id":"acme-proposal","status":"pending","before_candidate_id":"before","after_candidate_id":"after","rules":{"original_input":"x".repeat(1024*1024)},"comparison":{"complete":false,"relation":"unknown"}});
    bound_policy_result(&mut data).unwrap();
    assert!(data.to_string().len() <= 1024 * 1024);
    assert_eq!(data["proposal_id"], "acme-proposal");
    assert_eq!(data["after_candidate_id"], "after");
    assert_eq!(data["comparison"]["relation"], "unknown");
    assert_eq!(data["missing"], json!(["rules"]));
    assert_eq!(data["truncated"], true);
}

#[test]
fn admin_reviews_use_tool_routes_and_prepare_outside_the_connection_lock() {
    let (_root, mut context, connection) = fixture();
    context.agent = Some("claude".into());
    let checkpoint = || {
        assert!(
            connection.admin.try_lock().is_ok(),
            "preparation held the connection lock"
        );
        Ok(())
    };
    let (_, binding) = admin_confirmation(
        &context,
        &connection,
        "history_configure",
        &json!({"enabled":false}),
        &checkpoint,
    )
    .unwrap()
    .unwrap();
    discard_admin(&connection, binding);
    assert!(
        admin_confirmation(
            &context,
            &connection,
            "history_configure",
            &json!({"enabled":false}),
            &|| Err(crate::policy::Error::Interrupted)
        )
        .is_err()
    );
    for tool in [
        "protection_set",
        "registry_add",
        "mcp_setup",
        "history_configure",
    ] {
        let required = admin_error(&context, tool, &Error::ApprovalRequired);
        assert!(required["error"]["operator_route"].is_null());
        assert!(
            !required["error"]["message"]
                .as_str()
                .unwrap()
                .contains("policy review")
        );
        let stale = admin_error(&context, tool, &Error::StaleSnapshot);
        assert_eq!(stale["error"]["operator_route"], tool);
        assert!(
            stale["error"]["message"]
                .as_str()
                .unwrap()
                .contains(&format!("Call {tool} again"))
        );
    }
    for location in ["https://127.0.0.1:1/registry", "/nonexistent/acme-registry"] {
        let args = json!({"name":"acme","location":location});
        // A bare tool call or fabricated approval cannot perform discovery or
        // enrollment without a retained human-review binding.
        let result = call(&context, &connection, "registry_add", &args, &checkpoint);
        assert!(result["error"]["operator_route"].is_null());
        assert_eq!(result["error"]["code"], "operator_review_required");
    }
}

#[test]
fn inventory_has_only_canonical_tools_and_no_operator_or_completion_authority() {
    let (_root, context, connection) = fixture();
    let names: Vec<_> = tools()["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(
        names,
        vec![
            "setup",
            "enable",
            "disable",
            "protection_set",
            "registry_add",
            "registry_trust",
            "registry_remove",
            "registry_default",
            "history_configure",
            "policy_reject",
            "mcp_setup",
            "status",
            "doctor",
            "pack_list",
            "pack_show",
            "registry_list",
            "pack_search",
            "pack_info",
            "pack_add",
            "pack_configure",
            "pack_update",
            "pack_remove",
            "policy_show",
            "rule_list",
            "policy_propose",
            "policy_defer",
            "policy_draft_list",
            "policy_draft_remove",
            "suggestion_list",
            "review_defer",
            "policy_withdraw",
            "policy_apply",
            "request_list",
            "request_approve",
            "request_show",
            "request_deny",
            "history_show",
            "session_grant_list",
            "session_grant_show",
            "session_grant_propose",
            "session_grant_reconsider",
            "session_grant_approve",
            "rule_add",
            "rule_edit",
            "rule_remove",
            "session_grant_revoke",
            "history_list",
            "policy_evaluate",
            "policy_compare",
            "mcp_setup_preview",
            "docs_list",
            "docs_search",
            "docs_read"
        ]
    );
    for name in [
        "shield_propose_rule",
        "shield_review_argv",
        "shield_apply_proposal",
        "shield_record_completion",
        "shield_approve",
        "shield_runtime_activate",
    ] {
        assert_eq!(
            invoke(&context, &connection, name, json!({}))["error"]["code"],
            "invalid_input"
        );
    }
}

#[test]
fn inventory_annotations_and_pack_selection_schema_are_canonical() {
    for tool in tools()["tools"].as_array().unwrap() {
        assert!(tool["outputSchema"].is_object());
        assert_eq!(
            tool["annotations"]["destructiveHint"],
            [
                "disable",
                "policy_withdraw",
                "policy_apply",
                "protection_set",
                "registry_remove",
                "history_configure",
                "policy_reject",
                "mcp_setup"
            ]
            .iter()
            .any(|name| tool["name"] == *name)
        );
    }
    let listing = tools();
    let pack_list = listing["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == "pack_list")
        .unwrap();
    assert_eq!(
        pack_list["inputSchema"]["properties"]["reference"]["type"],
        "string"
    );
}
#[test]
fn diagnostics_create_no_action_or_history_and_handles_remain_frozen_after_commit() {
    let (root, mut context, connection) = fixture();
    let initial =
        invoke(&context, &connection, "status", json!({}))["data"]["policy_candidate_id"].clone();
    assert!(initial.is_string());
    let before = Store::open(root.path()).unwrap().read().unwrap().sequence;
    assert_eq!(
        invoke(&context, &connection, "policy_evaluate", request(&context))["data"]["decision"],
        "ask"
    );
    let state = Store::open(root.path()).unwrap().read().unwrap();
    assert_eq!(state.sequence, before);
    assert!(state.read_requests.is_empty());
    assert!(state.run_requests.is_empty());
    assert_eq!(state.events.as_slice(), &[] as &[Value]);
    let proposed = proposal(&context, &connection, "allow-read");
    assert!(proposed["error"].is_null(), "{proposed}");
    let id = proposed["data"]["proposal_id"].as_str().unwrap();
    let mut preview = request(&context);
    preview["candidate_id"] = proposed["data"]["after_candidate_id"].clone();
    assert_eq!(
        invoke(&context, &connection, "policy_evaluate", preview)["data"]["decision"],
        "allow"
    );
    assert_eq!(
        review::proposal_outcome(root.path(), id)["status"],
        "pending"
    );
    context.surface = "cli".into();
    review::operator::commit_proposal_fixture(&context, id).unwrap();
    context.surface = "mcp".into();
    let active = invoke(&context, &connection, "policy_evaluate", request(&context));
    assert_eq!(active["data"]["decision"], "allow");
    assert!(
        active["data"]["next_action"]
            .as_str()
            .unwrap()
            .contains("check shell analysis")
    );
    let mut old = request(&context);
    old["candidate_id"] = initial;
    assert_eq!(
        invoke(&context, &connection, "policy_evaluate", old)["data"]["decision"],
        "ask"
    );
    let retry = proposal(&context, &connection, "allow-read");
    assert_eq!(
        retry["data"]["proposal_id"],
        proposed["data"]["proposal_id"]
    );
    assert_eq!(
        retry["data"]["after_candidate_id"],
        proposed["data"]["after_candidate_id"]
    );
    let receipt = review::proposal_outcome(root.path(), id);
    assert!(
        receipt["receipt"]["policy_revision"].is_string(),
        "{receipt}"
    );
    // Reading the outcome again changes nothing.
    let sequence = Store::open(root.path()).unwrap().read().unwrap().sequence;
    assert_eq!(review::proposal_outcome(root.path(), id), receipt);
    assert_eq!(
        Store::open(root.path()).unwrap().read().unwrap().sequence,
        sequence
    );
}

#[test]
fn rule_lookup_uses_retained_snapshots_for_exact_and_paged_inspection() {
    let (_root, context, connection) = fixture();
    let proposed = invoke(
        &context,
        &connection,
        "policy_propose",
        json!({
            "changes":[
                {"kind":"rule_add","rule":rule("a", "allow")},
                {"kind":"rule_add","rule":rule("b", "allow")}
            ],
            "idempotency_key":"captured-rules"
        }),
    );
    assert!(proposed["error"].is_null(), "{proposed}");
    let candidate = proposed["data"]["after_candidate_id"].clone();
    let reference = json!({"scope":"custom","rule_id":"a"});
    let missing = invoke(
        &context,
        &connection,
        "rule_list",
        json!({
            "candidate_id":proposed["data"]["before_candidate_id"],"rule_ref":reference
        }),
    );
    assert_eq!(missing["error"]["code"], "not_found");
    let first = invoke(
        &context,
        &connection,
        "rule_list",
        json!({"candidate_id":candidate,"limit":1}),
    );
    assert!(first["error"].is_null(), "{first}");
    assert_eq!(first["data"]["rules"][0]["id"], "a");
    let cursor = first["data"]["next_cursor"].clone();
    assert!(cursor.is_string());
    let reconnected = Connection::new("different-capture-owner".into());
    let stale = invoke(
        &context,
        &reconnected,
        "rule_list",
        json!({"candidate_id":candidate,"cursor":cursor,"limit":1}),
    );
    assert_eq!(stale["error"]["code"], "invalid_cursor", "{stale}");
    review::operator::commit_proposal_fixture(
        &context,
        proposed["data"]["proposal_id"].as_str().unwrap(),
    )
    .unwrap();
    commit_pack_change(
        &context,
        &connection,
        json!({
            "kind":"rule_edit","rule_id":"a","rule":rule("a", "deny")
        }),
        "changed-rule",
    );
    let captured = invoke(
        &context,
        &connection,
        "rule_list",
        json!({
            "candidate_id":candidate,"rule_ref":reference
        }),
    );
    assert!(captured["error"].is_null(), "{captured}");
    assert_eq!(captured["data"]["candidate_id"], candidate);
    assert_eq!(captured["data"]["decision"], "allow");
    assert_eq!(
        invoke(
            &context,
            &connection,
            "rule_list",
            json!({"rule_ref":reference})
        )["data"]["decision"],
        "deny"
    );
    let next = invoke(
        &context,
        &connection,
        "rule_list",
        json!({
            "candidate_id":candidate,"limit":1,"cursor":cursor
        }),
    );
    assert!(next["error"].is_null(), "{next}");
    assert_eq!(next["data"]["rules"][0]["id"], "b");
    assert_eq!(next["data"]["candidate_id"], candidate);
    assert!(next["data"]["next_cursor"].is_null());
    let mut foreign_context = context.clone();
    foreign_context.connection_id = Some("foreign".into());
    let foreign = invoke(
        &foreign_context,
        &Connection::new("foreign".into()),
        "rule_list",
        json!({
            "candidate_id":candidate,"rule_ref":reference
        }),
    );
    assert_eq!(foreign["error"]["code"], "candidate_unavailable");
}

#[test]
fn a_withdrawn_proposal_is_terminal_without_an_approval_route() {
    let (_root, context, connection) = fixture();
    let proposed = proposal(&context, &connection, "withdrawn-receipt");
    let id = proposed["data"]["proposal_id"].clone();
    let withdrawn = invoke(
        &context,
        &connection,
        "policy_withdraw",
        json!({"proposal_id":id}),
    );
    assert!(withdrawn["error"].is_null(), "{withdrawn}");
    let shown = invoke(
        &context,
        &connection,
        "policy_show",
        json!({"proposal_id":id}),
    );
    assert!(shown["error"].is_null(), "{shown}");
    assert_eq!(shown["data"]["status"], "withdrawn");
    assert!(shown["data"].get("apply_arguments").is_none());
}

#[test]
fn approval_required_messages_follow_the_enrolled_adapter() {
    let (_root, mut context, _connection) = fixture();
    for (agent, route, expected) in [
        (
            Some("codex"),
            Some("policy_apply"),
            "Closed attempts require fresh preparation",
        ),
        (
            Some("claude"),
            Some("policy_apply"),
            "copy its apply_arguments object unchanged into policy_apply",
        ),
        (None, None, "In-session approval is unavailable"),
        (
            Some("acme-agent"),
            None,
            "In-session approval is unavailable",
        ),
    ] {
        context.agent = agent.map(str::to_owned);
        let error = &failure(&context, &Error::ApprovalRequired)["error"];
        assert_eq!(error["code"], "operator_review_required");
        assert_eq!(error["operator_route"].as_str(), route);
        let message = error["message"].as_str().unwrap();
        assert!(message.contains(expected), "{message}");
    }
}

#[test]
fn structured_error_messages_name_their_recovery() {
    let (_root, context, _connection) = fixture();
    for (error, code, recovery) in [
        (
            Error::CandidateUnavailable,
            "candidate_unavailable",
            "status",
        ),
        (
            Error::InvalidCursor,
            "invalid_cursor",
            "Restart the list without a cursor",
        ),
        (Error::Missing, "not_found", "List current records"),
        (
            Error::Overloaded,
            "overloaded",
            "Wait for the current operation",
        ),
        (Error::StaleSnapshot, "stale_revision", "re-propose"),
        (Error::Cancelled, "cancelled", "Inspect current state"),
        (
            crate::policy::Error::Limit.into(),
            "budget_exceeded",
            "Narrow the query",
        ),
        (
            Error::Io(std::io::Error::other("acme")),
            "io_failure",
            "doctor",
        ),
        (
            Error::RequiredPack {
                pack: "acme-registry/acme/git".into(),
                dependents: vec!["acme-registry/acme/workflow".into()],
            },
            "dependency_required",
            "first remove the packs that depend on it",
        ),
    ] {
        let value = failure(&context, &error);
        assert_eq!(value["error"]["code"], code);
        let message = value["error"]["message"].as_str().unwrap();
        assert!(message.contains(recovery), "{code}: {message}");
    }
    let required = failure(
        &context,
        &Error::RequiredPack {
            pack: "acme-registry/acme/git".into(),
            dependents: vec!["acme-registry/acme/workflow".into()],
        },
    );
    let error = &required["error"];
    assert_eq!(error["pack"], "acme-registry/acme/git");
    assert_eq!(error["dependents"], json!(["acme-registry/acme/workflow"]));
    assert!(
        error["message"]
            .as_str()
            .unwrap()
            .contains("required by acme-registry/acme/workflow")
    );
}

#[test]
fn many_dependents_are_bounded_in_the_message_and_data() {
    let (_root, context, _connection) = fixture();
    let dependents: Vec<String> = (0..2000)
        .map(|index| format!("acme-registry/acme/workflow-{index}"))
        .collect();
    let value = failure(
        &context,
        &Error::RequiredPack {
            pack: "acme-registry/acme/git".into(),
            dependents,
        },
    );
    let error = &value["error"];
    assert_eq!(error["code"], "dependency_required");
    assert_eq!(
        error["dependents"].as_array().unwrap().len(),
        crate::review::MAX_ALTERNATIVES
    );
    assert_eq!(error["dependents_total"], 2000);
    let message = error["message"].as_str().unwrap();
    assert!(message.len() <= 4096, "{message}");
    assert!(
        message.contains("acme-registry/acme/workflow-0"),
        "{message}"
    );
    assert!(
        !message.contains("acme-registry/acme/workflow-8,"),
        "{message}"
    );
    assert!(message.contains("and 1992 more"), "{message}");
}

#[test]
fn release_and_registry_refusals_name_the_user_step() {
    let (_root, context, _connection) = fixture();
    let unsupported = failure(
        &context,
        &Error::UnsupportedReleases("acme-registry/acme/git".into()),
    );
    let error = &unsupported["error"];
    assert_eq!(error["code"], "unsupported_version", "{unsupported}");
    assert!(error["operator_route"].is_null());
    let message = error["message"].as_str().unwrap();
    assert!(
        message.contains("acme-registry/acme/git") && message.contains("update Shield"),
        "{message}"
    );
    let unenrolled = failure(
        &context,
        &Error::Dependency(canonic_registry_kit::resolution::Error::Unenrolled(
            "acme-foreign".into(),
        )),
    );
    let error = &unenrolled["error"];
    assert_eq!(error["operator_route"], "registry_add", "{unenrolled}");
    let message = error["message"].as_str().unwrap();
    assert!(
        message.contains("acme-foreign") && message.contains("registry_add"),
        "{message}"
    );
}

#[test]
fn shown_live_proposals_carry_apply_arguments_and_closed_failures_do_not() {
    let (root, mut context, connection) = fixture();
    context.agent = Some("claude".into());
    let proposed = proposal(&context, &connection, "failed-receipt");
    let id = proposed["data"]["proposal_id"].as_str().unwrap().to_owned();
    let receipt = || {
        invoke(
            &context,
            &connection,
            "policy_show",
            json!({"proposal_id":id}),
        )
    };
    let pending = receipt();
    assert!(pending["error"].is_null(), "{pending}");
    assert_eq!(pending["data"]["status"], "pending");
    assert!(pending["data"]["apply_arguments"].is_object(), "{pending}");
    Store::open(root.path())
        .unwrap()
        .update(&context, &|| Ok(()), |state| {
            let row = state.proposals.iter_mut().find(|p| p.id == id).unwrap();
            row.status = "failed".into();
            Ok(json!({}))
        })
        .unwrap();
    let failed = receipt();
    assert!(failed["error"].is_null(), "{failed}");
    assert_eq!(failed["data"]["status"], "failed");
    assert!(failed["data"].get("apply_arguments").is_none());
}

#[test]
fn normalized_diagnostics_reject_files_as_working_directories() {
    let (root, context, connection) = fixture();
    let path = root.path().join("acme.txt");
    std::fs::write(&path, "Acme fixture").unwrap();
    let before = std::fs::read(root.path().join(".shield/state/state.json")).unwrap();
    let mut input = request(&context);
    input["context"]["cwd"] = json!(path);
    let result = invoke(&context, &connection, "policy_evaluate", input);
    assert_eq!(result["error"]["code"], "invalid_input", "{result}");
    assert_eq!(
        std::fs::read(root.path().join(".shield/state/state.json")).unwrap(),
        before
    );
}

#[test]
fn inspection_reclamation_keeps_live_review_handles_and_allows_more_proposals() {
    let (root, context, connection) = fixture();
    let proposed = proposal(&context, &connection, "retained-review");
    assert!(proposed["error"].is_null(), "{proposed}");
    let pinned = proposed["data"]["after_candidate_id"].as_str().unwrap();
    let state = Store::open(root.path()).unwrap().read().unwrap();
    let mut cache = connection.views.lock().unwrap();
    let mut first = None;
    for index in 1..600 {
        let mut metadata = state.operator.clone();
        metadata["test_identity"] = json!(index);
        let id = cache
            .retain(&context, &state.candidates["project"], &metadata)
            .unwrap();
        first.get_or_insert(id);
    }
    assert!(cache.get(pinned).is_ok());
    assert!(matches!(
        cache.get(first.as_deref().unwrap()),
        Err(Error::CandidateUnavailable)
    ));
    drop(cache);
    let replay = proposal(&context, &connection, "retained-review");
    assert_eq!(replay["data"]["after_candidate_id"], pinned);
    assert_eq!(
        replay["data"]["apply_arguments"],
        proposed["data"]["apply_arguments"]
    );
    let mut preview = request(&context);
    preview["candidate_id"] = json!(pinned);
    assert_eq!(
        invoke(&context, &connection, "policy_evaluate", preview)["data"]["decision"],
        "allow"
    );
    let next = proposal(&context, &connection, "after-reclamation");
    assert!(next["error"].is_null(), "{next}");
    assert!(invoke(&context, &connection, "status", json!({}))["error"].is_null());
}

#[test]
fn full_pending_proposals_refuse_with_counts_and_a_review_route() {
    let (root, context, connection) = fixture();
    assert!(proposal(&context, &connection, "seed")["error"].is_null());
    Store::open(root.path())
        .unwrap()
        .update(&context, &|| Ok(()), |state| {
            let seed = serde_json::to_value(&state.proposals[0]).unwrap();
            for index in 1..128 {
                let mut copy = seed.clone();
                copy["proposal_id"] = json!(format!("acme-pending-{index}"));
                copy["retry_key"] = json!(format!("acme-{index}"));
                copy["retry_owner"] = json!("another-live-owner");
                state.proposals.push(serde_json::from_value(copy).unwrap());
            }
            state.proposals[0].retry_owner = "another-live-owner".into();
            Ok(json!({}))
        })
        .unwrap();
    let refused = proposal(&context, &connection, "overflow");
    let error = &refused["error"];
    assert_eq!(error["code"], "capacity_reached", "{refused}");
    assert!(error["operator_route"].is_null(), "{refused}");
    let message = error["message"].as_str().unwrap();
    assert!(message.contains("128 pending"), "{message}");
    assert!(message.contains("limit 128"), "{message}");
    assert_eq!(
        Store::open(root.path())
            .unwrap()
            .read()
            .unwrap()
            .proposals
            .len(),
        128
    );
}

#[test]
fn full_retained_proposals_refuse_without_a_review_and_retry_route() {
    let (root, context, connection) = fixture();
    assert!(proposal(&context, &connection, "seed")["error"].is_null());
    Store::open(root.path())
        .unwrap()
        .update(&context, &|| Ok(()), |state| {
            let seed = serde_json::to_value(&state.proposals[0]).unwrap();
            for index in 1..1024 {
                let mut copy = seed.clone();
                copy["proposal_id"] = json!(format!("acme-withdrawn-{index}"));
                copy["retry_key"] = json!(format!("acme-{index}"));
                copy["status"] = json!("withdrawn");
                state.proposals.push(serde_json::from_value(copy).unwrap());
            }
            Ok(json!({}))
        })
        .unwrap();
    let refused = proposal(&context, &connection, "overflow");
    let error = &refused["error"];
    assert_eq!(error["code"], "capacity_reached", "{refused}");
    assert!(error["operator_route"].is_null(), "{refused}");
    let message = error["message"].as_str().unwrap();
    for expected in [
        "0 pending",
        "1024 retained",
        "limit 1024",
        "24-hour recovery window",
    ] {
        assert!(message.contains(expected), "{message}");
    }
    assert!(!message.contains("shield policy review"), "{message}");
    assert_eq!(
        Store::open(root.path())
            .unwrap()
            .read()
            .unwrap()
            .proposals
            .len(),
        1024
    );
}

#[test]
fn invalid_change_keys_and_mismatched_edits_do_not_mutate_or_rebase() {
    let (root, context, connection) = fixture();
    for args in [
        json!({"change":{"kind":"rule_remove","rule_id":"missing"},"idempotency_key":"λ"}),
        json!({"change":{"kind":"rule_add","rule":rule("one","allow")},"idempotency_key":"one","source_entry_id":"x".repeat(129)}),
        json!({"change":{"kind":"rule_edit","rule_id":"one","rule":rule("two","allow")},"idempotency_key":"edit"}),
        json!({"change":{"kind":"rule_add","rule":rule("one","allow"),"registry":"injected"},"idempotency_key":"one"}),
        json!({"change":{"kind":"pack_add","reference":"acme/test","profile":null},"idempotency_key":"pack"}),
        json!({"change":{"kind":"rule_add","rule":rule("one","allow")},"idempotency_key":"one","agent_reason":null}),
    ] {
        assert!(invoke(&context, &connection, "policy_propose", args)["error"].is_object());
    }
    assert!(
        Store::open(root.path())
            .unwrap()
            .read()
            .unwrap()
            .proposals
            .is_empty()
    );
    let original = proposal(&context, &connection, "one");
    let different = invoke(
        &context,
        &connection,
        "policy_propose",
        json!({"change":{"kind":"rule_add","rule":rule("one","deny")},"idempotency_key":"one"}),
    );
    assert_eq!(different["error"]["code"], "invalid_input");
    assert_eq!(
        proposal(&context, &connection, "one")["data"],
        original["data"]
    );
}
#[test]
fn withdrawal_checks_registered_origin_and_reconnect_requires_new_handles() {
    let (_root, context, connection) = fixture();
    let proposed = proposal(&context, &connection, "withdraw");
    let id = proposed["data"]["proposal_id"].clone();
    let mut other = context.clone();
    other.connection_id = Some("two".into());
    let second = Connection::new("two".into());
    for agent in ["acme", "claude", "codex"] {
        other.agent = Some(agent.into());
        let refused = invoke(
            &other,
            &second,
            "policy_withdraw",
            json!({"proposal_id":id}),
        );
        let error = &refused["error"];
        assert_eq!(error["code"], "operator_review_required", "{refused}");
        // A foreign withdrawal is never routed to apply.
        assert_eq!(error["operator_route"], "policy_show", "{refused}");
        let message = error["message"].as_str().unwrap();
        assert!(message.contains("interaction"), "{message}");
        assert!(!message.contains("policy_apply"), "{message}");
    }
    other.agent = context.agent.clone();
    let mut foreign = request(&other);
    foreign["candidate_id"] = proposed["data"]["after_candidate_id"].clone();
    assert_eq!(
        invoke(&other, &second, "policy_evaluate", foreign)["error"]["code"],
        "candidate_unavailable"
    );
    let inspected = invoke(&other, &second, "policy_show", json!({"proposal_id":id}));
    assert!(inspected["data"]["after_candidate_id"].is_string());
    assert_ne!(
        inspected["data"]["after_candidate_id"],
        proposed["data"]["after_candidate_id"]
    );
    for _ in 0..2 {
        assert_eq!(
            invoke(
                &context,
                &connection,
                "policy_withdraw",
                json!({"proposal_id":id})
            )["data"]["status"],
            "withdrawn"
        );
    }
}
#[test]
fn cursors_keep_a_consistent_query_and_refuse_changed_filters_or_selectors() {
    let (_root, context, connection) = fixture();
    let mut owners = Vec::new();
    for id in ["a", "b", "c"] {
        let mut owner = context.clone();
        owner.connection_id = Some(format!("page-{id}"));
        let owner_connection = Connection::new(format!("page-{id}"));
        let proposed = proposal(&owner, &owner_connection, id);
        assert!(proposed["error"].is_null(), "{proposed}");
        owners.push((
            proposed["data"]["proposal_id"].clone(),
            owner,
            owner_connection,
        ));
    }
    let first = invoke(&context, &connection, "policy_show", json!({"limit":1}));
    let cursor = first["data"]["next_cursor"].clone();
    assert!(cursor.is_string());
    let id = first["data"]["proposals"][0]["proposal_id"].clone();
    let (_, owner, owner_connection) = owners
        .iter()
        .find(|(proposal, _, _)| proposal == &id)
        .unwrap();
    let withdrawn = invoke(
        owner,
        owner_connection,
        "policy_withdraw",
        json!({"proposal_id":id}),
    );
    assert!(withdrawn["error"].is_null(), "{withdrawn}");
    let next = invoke(
        &context,
        &connection,
        "policy_show",
        json!({"cursor":cursor,"limit":1}),
    );
    assert!(next["error"].is_null(), "{next}");
    assert_eq!(next["data"]["proposals"].as_array().unwrap().len(), 1);
    assert_eq!(
        invoke(
            &context,
            &connection,
            "policy_show",
            json!({"cursor":cursor,"limit":1,"status":"withdrawn"})
        )["error"]["code"],
        "invalid_cursor"
    );
    assert_eq!(
        invoke(
            &context,
            &connection,
            "rule_list",
            json!({"rule_ref":{"scope":"custom","rule_id":"a"},"limit":1})
        )["error"]["code"],
        "invalid_input"
    );
}
#[test]
fn exact_conditions_match_vendor_fields_environment_and_observed_executable() {
    let (_root, context, connection) = fixture();
    let executable = std::env::current_exe()
        .unwrap()
        .canonicalize()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let input = json!({"kind":"argv","executable":executable,"arguments":["hello"],"original_input":{"command":"echo hello","vendor_flag":false}});
    let ctx = json!({"cwd":context.project_root,"environment":{"PATH":"/bin"}});
    let mut exact = rule("exact", "allow");
    exact["conditions"] =
        json!({"domain":"exact_request","input":input,"context":ctx,"executable":executable});
    let proposed = invoke(
        &context,
        &connection,
        "policy_propose",
        json!({"change":{"kind":"rule_add","rule":exact},"idempotency_key":"exact"}),
    );
    assert!(proposed["error"].is_null(), "{proposed}");
    let mut args =
        json!({"input":input,"context":ctx,"candidate_id":proposed["data"]["after_candidate_id"]});
    assert_eq!(
        invoke(&context, &connection, "policy_evaluate", args.clone())["data"]["decision"],
        "allow"
    );
    args["input"]["original_input"]["vendor_flag"] = json!(true);
    assert_eq!(
        invoke(&context, &connection, "policy_evaluate", args.clone())["data"]["decision"],
        "ask"
    );
    args["input"]["original_input"]["vendor_flag"] = json!(false);
    args["context"]["environment"] = Value::Null;
    assert_eq!(
        invoke(&context, &connection, "policy_evaluate", args)["data"]["decision"],
        "ask"
    );
}

#[test]
fn proposals_without_a_key_dedupe_identical_retries_and_keep_changes_apart() {
    let (_root, context, connection) = fixture();
    let submit = |id: &str| {
        invoke(
            &context,
            &connection,
            "policy_propose",
            json!({"change":{"kind":"rule_add","rule":rule(id,"allow")},"source_entry_id":"request-acme"}),
        )["data"]["proposal_id"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    let first = submit("acme-read");
    assert_eq!(submit("acme-read"), first);
    assert_ne!(submit("acme-read-narrow"), first);
}

fn tools_release(source: &Path, version: &str, rules: &Value) {
    let pack = json!({"schema_version":1,"kind":"pack","id":"acme/tools","version":version,"description":"Acme tools","license":"UNLICENSED","maintainers":["Acme"],"engines":{"shield":env!("CARGO_PKG_VERSION")},"capabilities":["argument-rules-v1","structured-rules-v1","variables-v1"],
        "variables":{"tool":{"type":"choice","description":"Build tool agents use","values":["make","just"]}},"rules":rules}).to_string();
    let path = format!("tools-{version}.yml");
    std::fs::write(source.join(&path), &pack).unwrap();
    let catalog_path = source.join("catalog.json");
    let mut catalog: Value = std::fs::read(&catalog_path).map_or_else(
        |_| json!({"schema_version":1,"packs":[]}),
        |bytes| serde_json::from_slice(&bytes).unwrap(),
    );
    catalog["packs"].as_array_mut().unwrap().push(json!({"id":"acme/tools","version":version,"path":path,"sha256":crate::policy::fingerprint(pack.as_bytes())}));
    std::fs::write(catalog_path, catalog.to_string()).unwrap();
}

/// Enroll a tools registry holding release 1.0.0 with `rules`, then install it.
fn install_tools(
    root: &assert_fs::TempDir,
    context: &Context,
    connection: &Connection,
    rules: &Value,
) -> assert_fs::TempDir {
    let source = assert_fs::TempDir::new().unwrap();
    let directory = source.path().canonicalize().unwrap();
    tools_release(&directory, "1.0.0", rules);
    Store::open(root.path()).unwrap().update(context, &|| Ok(()), |state| {
        state.operator["sources"]["acme-tools"] = json!({"identity":crate::policy::fingerprint(directory.to_str().unwrap().as_bytes()),"location":directory,"key":null});
        Ok(json!({}))
    }).unwrap();
    commit_pack_change(
        context,
        connection,
        json!({"kind":"pack_add","reference":"acme-tools/acme/tools"}),
        "install",
    );
    source
}

fn decide(root: &Path, executable: &str) -> crate::policy::Decision {
    let active = Store::open(root).unwrap().read().unwrap();
    crate::configuration::Evaluator::new(review::snapshots(&active.candidates).unwrap())
        .unwrap()
        .evaluate(&crate::policy::Request {
            executable: executable.into(),
            arguments: vec![],
            cwd: root.to_str().unwrap().into(),
            project: root.to_str().unwrap().into(),
        })
        .unwrap()
        .decision
}

#[test]
fn pack_configure_binds_values_and_overrides_that_survive_updates_until_stale() {
    let (root, context, connection) = fixture();
    let source = assert_fs::TempDir::new().unwrap();
    let directory = source.path().canonicalize().unwrap();
    let build = json!({"id":"build","decision":"allow","conditions":{"domain":"argv","executable":{"var":"tool"},"arguments":{"kind":"any"}}});
    let lint = json!({"id":"lint","decision":"deny","conditions":{"domain":"argv","executable":"lint","arguments":{"kind":"any"}}});
    tools_release(&directory, "1.0.0", &json!([build, lint]));
    Store::open(root.path()).unwrap().update(&context, &|| Ok(()), |state| {
        state.operator["sources"]["acme-tools"] = json!({"identity":crate::policy::fingerprint(directory.to_str().unwrap().as_bytes()),"location":directory,"key":null});
        Ok(json!({}))
    }).unwrap();
    commit_pack_change(
        &context,
        &connection,
        json!({"kind":"pack_add","reference":"acme-tools/acme/tools"}),
        "install",
    );
    assert_eq!(decide(root.path(), "make"), crate::policy::Decision::Ask);
    assert_eq!(decide(root.path(), "lint"), crate::policy::Decision::Deny);
    for (change, key) in [
        (
            json!({"kind":"pack_configure","reference":"acme/tools"}),
            "empty",
        ),
        (
            json!({"kind":"pack_configure","reference":"acme/tools","set":{"tool":"cargo"}}),
            "invalid",
        ),
        (
            json!({"kind":"pack_configure","reference":"acme/tools","set":{"other":"make"}}),
            "unknown",
        ),
        (
            json!({"kind":"pack_configure","reference":"acme/tools","override":{"missing":"off"}}),
            "stale",
        ),
    ] {
        assert_ne!(
            pack_change(&context, &connection, change, key)["error"],
            Value::Null,
            "{key}"
        );
    }
    let proposed = pack_change(
        &context,
        &connection,
        json!({"kind":"pack_configure","reference":"acme/tools","set":{"tool":"make"},"override":{"lint":"ask"}}),
        "configure",
    );
    assert_eq!(proposed["data"]["comparison"]["relation"], "expansive");
    let summary = &proposed["data"]["comparison"]["override_permission_review"];
    assert_override_expansion(summary);
    let retained = Store::open(root.path()).unwrap().read_checkpoint().unwrap();
    let proposal = retained
        .proposals
        .iter()
        .find(|row| Some(row.id.as_str()) == proposed["data"]["proposal_id"].as_str())
        .unwrap();
    assert_eq!(&proposal.comparison["override_permission_review"], summary);
    review::operator::commit_proposal_fixture(
        &context,
        proposed["data"]["proposal_id"].as_str().unwrap(),
    )
    .unwrap();
    assert_eq!(decide(root.path(), "make"), crate::policy::Decision::Allow);
    assert_eq!(decide(root.path(), "just"), crate::policy::Decision::Ask);
    assert_eq!(decide(root.path(), "lint"), crate::policy::Decision::Ask);
    let rules = invoke(&context, &connection, "rule_list", json!({}));
    let build_row = rules["data"]["rules"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == "build")
        .unwrap()
        .clone();
    assert_eq!(
        build_row["resolution"]["variables"]["tool"], "make",
        "{build_row}"
    );
    assert_eq!(
        build_row["resolution"]["resolved"]["conditions"]["executable"],
        "make"
    );
    tools_release(&directory, "1.1.0", &json!([build]));
    commit_pack_change(
        &context,
        &connection,
        json!({"kind":"pack_update","reference":"acme/tools"}),
        "update",
    );
    let active = Store::open(root.path()).unwrap().read().unwrap();
    let config: Value =
        serde_saphyr::from_str(active.candidates["project"]["config"].as_str().unwrap()).unwrap();
    assert_eq!(config["packs"][0]["version"], "1.1.0");
    assert_eq!(config["packs"][0]["variables"], json!({"tool":"make"}));
    assert!(config["packs"][0].get("overrides").is_none(), "{config}");
    assert!(active.operator["installed"][0].get("variables").is_none());
    assert_eq!(decide(root.path(), "make"), crate::policy::Decision::Allow);
}

fn assert_override_expansion(summary: &Value) {
    assert_eq!(summary["scope"], "proposal");
    assert_eq!(summary["complete"], true);
    assert_eq!(summary["relation"], "expansive");
    assert_eq!(summary["widens"], true);
    assert_eq!(summary["expansion_witness"], "/comparison/expansion");
}

#[test]
fn pack_configure_refusal_names_the_variable_without_its_value() {
    let (root, context, connection) = fixture();
    let build = json!({"id":"build","decision":"allow","conditions":{"domain":"argv","executable":{"var":"tool"},"arguments":{"kind":"any"}}});
    let _source = install_tools(&root, &context, &connection, &json!([build]));
    let refused = pack_change(
        &context,
        &connection,
        json!({"kind":"pack_configure","reference":"acme/tools","set":{"tool":"acme-secret-tool"}}),
        "invalid",
    );
    assert_eq!(refused["error"]["code"], "invalid_input", "{refused}");
    let message = refused["error"]["message"].as_str().unwrap();
    assert!(message.contains("variable tool"), "{message}");
    assert!(!message.contains("acme-secret-tool"), "{message}");
}

#[test]
fn local_layer_changes_bind_personal_values_rules_roots_and_packs() {
    let (root, context, connection) = fixture();
    let build = json!({"id":"build","decision":"allow","conditions":{"domain":"argv","executable":{"var":"tool"},"arguments":{"kind":"any"}}});
    let _source = install_tools(&root, &context, &connection, &json!([build]));
    commit_pack_change(
        &context,
        &connection,
        json!({"kind":"pack_configure","reference":"acme/tools","set":{"tool":"make"}}),
        "shared",
    );
    commit_pack_change(
        &context,
        &connection,
        json!({"kind":"pack_configure","reference":"acme/tools","set":{"tool":"just"},"layer":"local"}),
        "personal",
    );
    assert_eq!(decide(root.path(), "just"), crate::policy::Decision::Allow);
    assert_eq!(decide(root.path(), "make"), crate::policy::Decision::Ask);
    let active = Store::open(root.path()).unwrap().read().unwrap();
    let merged = crate::configuration::layers::merged(&active.candidates["project"]).unwrap();
    assert_eq!(merged["packs"][0]["variables"], json!({"tool":"make"}));
    assert_eq!(
        merged["packs"][0]["local"]["variables"],
        json!({"tool":"just"})
    );
    let rules = invoke(&context, &connection, "rule_list", json!({}));
    let row = rules["data"]["rules"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == "build")
        .unwrap()
        .clone();
    assert_eq!(row["resolution"]["layers"]["tool"], "local", "{row}");
    let mut local_rule = rule("personal", "deny");
    local_rule["conditions"]["executable"] = json!("rm");
    commit_pack_change(
        &context,
        &connection,
        json!({"kind":"rule_add","rule":local_rule,"layer":"local"}),
        "local-rule",
    );
    commit_pack_change(
        &context,
        &connection,
        json!({"kind":"access_path_add","access":"read","path":crate::test_support::absolute("/srv/acme"),"layer":"local"}),
        "local-root",
    );
    let active = Store::open(root.path()).unwrap().read().unwrap();
    let candidate = &active.candidates["project"];
    assert!(candidate["rules"].is_null());
    assert!(
        candidate["local_rules"]
            .as_str()
            .unwrap()
            .contains("personal")
    );
    let inspected = invoke(
        &context,
        &connection,
        "rule_list",
        json!({
            "rule_ref":{"scope":"custom","rule_id":"personal"}
        }),
    );
    assert!(inspected["error"].is_null(), "{inspected}");
    assert_eq!(inspected["data"]["decision"], "deny");
    let merged = crate::configuration::layers::merged(candidate).unwrap();
    assert_eq!(
        merged["local_access_paths"]["read"],
        json!([crate::test_support::absolute("/srv/acme")])
    );
    assert!(
        merged
            .get("access_paths")
            .is_none_or(|p| p["read"].as_array().is_none_or(Vec::is_empty))
    );
    // Each layer retains its own file's source.
    let shared = candidate["config"].as_str().unwrap();
    assert!(
        !shared.contains("just") && !shared.contains("/srv/acme"),
        "{shared}"
    );
    let local = candidate["local_config"].as_str().unwrap();
    assert!(
        local.contains("just") && local.contains("rules.local.yml"),
        "{local}"
    );
    // Unchanged personal settings survive an unrelated shared change.
    commit_pack_change(
        &context,
        &connection,
        json!({"kind":"pack_configure","reference":"acme/tools","override":{"build":"ask"}}),
        "shared-override",
    );
    let active = Store::open(root.path()).unwrap().read().unwrap();
    let merged = crate::configuration::layers::merged(&active.candidates["project"]).unwrap();
    assert_eq!(
        merged["packs"][0]["local"]["variables"],
        json!({"tool":"just"})
    );
    assert_eq!(decide(root.path(), "just"), crate::policy::Decision::Ask);
}

#[test]
fn suggestions_list_a_sessions_asked_commands_for_one_batch_proposal() {
    let (_root, context, connection) = fixture();
    let mut hook = context.clone();
    hook.surface = "hook".into();
    hook.source = "host".into();
    hook.session_id = Some("acme-session".into());
    hook.host_event_name = Some("PreToolUse".into());
    let asked = |context: &Context, arguments: &[&str], invited: bool| {
        let scope = if invited { "pack" } else { "custom" };
        let guidance = json!([
            {"source":"decisive","message":"Acme asks.","request":{"executable":"acme","arguments":arguments}},
            {"rule_ref":{"scope":scope,"pack_selector":"acme/tools","rule_id":"scripts"},
                "alternatives":["Propose a narrow project rule with policy_propose."]}
        ]);
        review::observations::decision(
            context,
            json!({"decision":"ask","operation":{"kind":"argv","executable":"acme","arguments":arguments},
                "evaluation":{"explanation":{"guidance":guidance}}}),
        )
        .unwrap();
    };
    asked(&hook, &["run", "dev"], true);
    asked(&hook, &["run", "dev"], true);
    asked(&hook, &["build"], false);
    let mut other = hook.clone();
    other.session_id = Some("acme-other".into());
    asked(&other, &["run", "lint"], true);
    let result = invoke(
        &context,
        &connection,
        "suggestion_list",
        json!({"session_id":"acme-session"}),
    );
    assert!(result["error"].is_null(), "{result}");
    // Every distinct asked command remains available in stable review-ID order.
    let requests = result["data"]["requests"].as_array().unwrap();
    let mut commands: Vec<_> = requests
        .iter()
        .map(|s| s["command"].as_str().unwrap())
        .collect();
    commands.sort_unstable();
    assert_eq!(commands, ["acme build", "acme run dev"]);
    let run = requests
        .iter()
        .find(|row| row["command"] == "acme run dev")
        .unwrap();
    let build = requests
        .iter()
        .find(|row| row["command"] == "acme build")
        .unwrap();
    assert_eq!(run["times_asked"], 2);
    assert_eq!(run["pack_requests_rule"], true);
    assert_eq!(build["pack_requests_rule"], false);
    assert_eq!(build["decided_as"], "command");
    for row in requests {
        assert!(row.get("exact_change").is_none(), "{row}");
        assert!(row.get("suggested_change").is_none(), "{row}");
        assert_eq!(row["request"]["executable"], "acme");
    }
    // The agent authors rules from the evidence instead of receiving policy
    // drafts for each observed argv. Batches still use the ordinary workflow.
    let changes = vec![
        json!({"kind":"rule_add","rule":{"id":"acme-build","decision":"allow",
            "conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"options","prefix":["build"],"flags":["--watch"],"values":[],"operands":false}}}}),
        json!({"kind":"rule_add","rule":{"id":"acme-dev","decision":"allow",
            "conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"options","prefix":["run","dev"],"flags":["--watch"],"values":[],"operands":false}}}}),
    ];
    let both = json!({"change":changes[0],"changes":changes});
    assert_eq!(
        invoke(&context, &connection, "policy_propose", both)["error"]["code"],
        "invalid_input"
    );
    let proposal = invoke(
        &context,
        &connection,
        "policy_propose",
        json!({"changes":changes}),
    );
    assert!(proposal["error"].is_null(), "{proposal}");
    let data = &proposal["data"];
    assert_eq!(
        data["apply_arguments"]["changes"].as_array().unwrap().len(),
        2
    );
    assert_eq!(
        data["change_summary"],
        "2 changes: allow limited `acme` forms; allow limited `acme` forms."
    );
}

#[test]
fn confirmation_npm_proposal_summary_preserves_closed_project_scope() {
    let (_root, context, connection) = fixture();
    let result = invoke(
        &context,
        &connection,
        "policy_propose",
        json!({"changes":[{"kind":"rule_add","rule":{
            "id":"project-scripts","decision":"allow","conditions":{
                "domain":"argv","executable":"npm","cwd":{"exact":{"root":"project"}},
                "arguments":{"kind":"options","prefix":["run",{"one_of":["dev","start","test","lint","format"]}],
                    "flags":[],"values":[],"operands":false}
            }
        }},{"kind":"rule_add","rule":{
            "id":"project-test","decision":"allow","conditions":{
                "domain":"argv","executable":"npm","cwd":{"exact":{"root":"project"}},
                "arguments":{"kind":"options","prefix":["test"],"flags":[],"values":[],"operands":false}
            }
        }}]}),
    );
    assert!(result["error"].is_null(), "{result}");
    let summary = result["data"]["change_summary"].as_str().unwrap();
    for expected in [
        "`npm run (dev|start|test|lint|format)`",
        "`npm test`",
        "no extra arguments",
        "working directory: exactly project directory",
        "future script edits and lifecycle hooks inherit this permission",
    ] {
        assert!(summary.contains(expected), "{summary}");
    }
    assert_eq!(result["data"]["status"], "pending");
}

#[test]
fn suggestions_group_portable_web_rules_across_download_variants() {
    let (_root, context, connection) = fixture();
    let mut fetches = context.clone();
    fetches.surface = "hook".into();
    fetches.source = "host".into();
    fetches.host_event_name = Some("PreToolUse".into());
    fetches.session_id = Some("acme-fetches".into());
    // Read-only downloads share one web fetch rule across flags and ports.
    let output_path = std::env::temp_dir().join("acme-catalog.json");
    let output = output_path.to_str().unwrap();
    for arguments in [
        vec!["-fsS", "http://localhost:3000/health"],
        vec!["--silent", "--include", "http://127.0.0.1:8080/status"],
        vec![
            "--fail",
            "--silent",
            "--show-error",
            "--output",
            output,
            "http://localhost:3000/catalog.json",
        ],
    ] {
        let guidance = json!([{"source":"decisive","message":"curl asks.","request":{"executable":"curl","arguments":arguments}}]);
        review::observations::decision(
            &fetches,
            json!({"decision":"ask","operation":{"kind":"argv","executable":"curl","arguments":arguments},
                "evaluation":{"explanation":{"guidance":guidance}}}),
        )
        .unwrap();
    }
    let result = invoke(
        &context,
        &connection,
        "suggestion_list",
        json!({"session_id":"acme-fetches"}),
    );
    let downloads = result["data"]["requests"].as_array().unwrap();
    assert_eq!(downloads.len(), 1, "{result}");
    assert_eq!(downloads[0]["decided_as"], "web_fetch");
    assert_eq!(downloads[0]["times_asked"], 3);
    // The newest request creates the row before older downloads merge into it.
    // Its local output path does not make the suggested web rule non-portable.
    assert_eq!(downloads[0]["request"]["arguments"][4], output);
    assert!(downloads[0].get("not_portable").is_none(), "{result}");
    assert_eq!(
        downloads[0]["suggested_change"]["rule"]["id"],
        "allow-fetch-localhost"
    );
}

#[test]
fn suggestions_scope_pack_invitations_to_each_command_in_a_chain() {
    let (_root, context, connection) = fixture();
    let mut hook = context.clone();
    hook.surface = "hook".into();
    hook.source = "host".into();
    hook.session_id = Some("acme-chain".into());
    hook.host_event_name = Some("PreToolUse".into());
    let invitation = json!({"operation_ref":"shell:0:0-8",
        "rule_ref":{"scope":"pack","pack_selector":"acme/scripts","rule_id":"scripts"},
        "alternatives":["Propose a project rule with policy_propose."]});
    let mut unscoped = invitation.clone();
    unscoped.as_object_mut().unwrap().remove("operation_ref");
    review::observations::decision(
        &hook,
        json!({"decision":"ask",
            "operation":{"kind":"shell","source":"npm test && acme-lint"},
            "evaluation":{"explanation":{"guidance":[
                {"source":"decisive","operation_ref":"shell:0:0-8","message":"Review project scripts.",
                    "request":{"executable":"npm","arguments":["test"]}},
                {"source":"decisive","operation_ref":"shell:0:12-21","message":"Acme lint asks.",
                    "request":{"executable":"acme-lint","arguments":[]}},
                invitation,unscoped]}}}),
    )
    .unwrap();
    let result = invoke(
        &context,
        &connection,
        "suggestion_list",
        json!({"session_id":"acme-chain"}),
    );
    let rows = result["data"]["requests"].as_array().unwrap();
    assert_eq!(rows.len(), 2, "{result}");
    assert_eq!(
        rows.iter()
            .find(|row| row["command"] == "npm test")
            .unwrap()["pack_requests_rule"],
        true
    );
    assert_eq!(
        rows.iter()
            .find(|row| row["command"] == "acme-lint")
            .unwrap()["pack_requests_rule"],
        false
    );
}

#[test]
fn suggestions_count_each_change_once_per_request() {
    let (_root, context, connection) = fixture();
    let mut hook = context.clone();
    hook.surface = "hook".into();
    hook.source = "host".into();
    hook.host_event_name = Some("PreToolUse".into());
    for (session, executable, first, second) in [
        ("acme-format", "acme-format", vec![], vec![]),
        (
            "acme-fetch",
            "curl",
            vec!["-fsS", "http://localhost:3000/health"],
            vec!["--silent", "http://127.0.0.1:8080/status"],
        ),
    ] {
        hook.session_id = Some(session.into());
        let guidance = |executable: &str, arguments: &[&str]| {
            json!({"source":"decisive","message":"Acme asks.",
                "request":{"executable":executable,"arguments":arguments}})
        };
        let source = format!(
            "{executable} {} && acme-lint && {executable} {}",
            first.join(" "),
            second.join(" "),
        );
        let chain = json!({"decision":"ask","operation":{"kind":"shell","source":source},
            "evaluation":{"explanation":{"guidance":[
                guidance(executable,&first),guidance("acme-lint",&[]),guidance(executable,&second)]}}});
        for count in 1..=2 {
            review::observations::decision(&hook, chain.clone()).unwrap();
            let result = invoke(
                &context,
                &connection,
                "suggestion_list",
                json!({"session_id":session}),
            );
            let rows = result["data"]["requests"].as_array().unwrap();
            assert_eq!(rows.len(), 2, "{result}");
            assert_eq!(rows[0]["times_asked"], count, "{result}");
            assert!(rows.iter().any(|row| row["command"] == "acme-lint"));
            assert_eq!(rows[1]["times_asked"], count, "{result}");
        }
        // A separate operation proposing the same change contributes once more.
        review::observations::decision(
            &hook,
            json!({"decision":"ask",
                "operation":{"kind":"argv","executable":executable,"arguments":first},
                "evaluation":{"explanation":{"guidance":[guidance(executable,&first)]}}}),
        )
        .unwrap();
        let result = invoke(
            &context,
            &connection,
            "suggestion_list",
            json!({"session_id":session}),
        );
        let rows = result["data"]["requests"].as_array().unwrap();
        assert_eq!(rows.len(), 2, "{result}");
        assert_eq!(
            rows.iter()
                .find(|row| row["command"] != "acme-lint")
                .unwrap()["times_asked"],
            3,
            "{result}"
        );
        assert_eq!(
            rows.iter()
                .find(|row| row["command"] == "acme-lint")
                .unwrap()["times_asked"],
            2,
            "{result}"
        );
    }
}

#[test]
fn suggestions_page_a_chain_without_losing_later_matches() {
    let (_root, context, connection) = fixture();
    let mut hook = context.clone();
    hook.surface = "hook".into();
    hook.source = "host".into();
    hook.session_id = Some("acme-chain-limit".into());
    hook.host_event_name = Some("PreToolUse".into());
    let guidance = |argument: &str| {
        json!({"source":"decisive","message":"Acme asks.",
            "request":{"executable":"acme","arguments":[argument]}})
    };
    let arguments = ["older-one", "task-0", "older-two", "task-1"];
    let source = arguments
        .map(|argument| format!("acme {argument}"))
        .join(" && ");
    let chain = review::observations::decision(
        &hook,
        json!({"decision":"ask","operation":{"kind":"shell","source":source},
            "evaluation":{"explanation":{"guidance":arguments.map(guidance)}}}),
    )
    .unwrap();
    // Fifteen newer requests leave room for only the chain's first new rule.
    for index in 0..15 {
        let argument = format!("task-{index}");
        review::observations::decision(
            &hook,
            json!({"decision":"ask",
                "operation":{"kind":"argv","executable":"acme","arguments":[argument]},
                "evaluation":{"explanation":{"guidance":[guidance(&argument)]}}}),
        )
        .unwrap();
    }
    let result = invoke(
        &context,
        &connection,
        "suggestion_list",
        json!({"session_id":"acme-chain-limit"}),
    );
    let rows = result["data"]["requests"].as_array().unwrap();
    assert_eq!(rows.len(), 16, "{result}");
    let mut all = rows.clone();
    let mut cursor = result["data"]["next_cursor"].clone();
    while cursor.is_string() {
        let page = invoke(
            &context,
            &connection,
            "suggestion_list",
            json!({"session_id":"acme-chain-limit","cursor":cursor}),
        );
        assert!(page["error"].is_null(), "{page}");
        all.extend(page["data"]["requests"].as_array().unwrap().iter().cloned());
        cursor = page["data"]["next_cursor"].clone();
    }
    assert_eq!(all.len(), 17);
    let ids: std::collections::BTreeSet<_> = all
        .iter()
        .map(|row| row["review_id"].as_str().unwrap())
        .collect();
    assert_eq!(ids.len(), 17);
    for index in 0..15 {
        let row = all
            .iter()
            .find(|row| row["command"] == format!("acme task-{index}"))
            .unwrap();
        assert_eq!(row["times_asked"], if index <= 1 { 2 } else { 1 });
    }
    for command in ["older-one", "older-two"] {
        let row = all
            .iter()
            .find(|row| row["command"] == format!("acme {command}"))
            .unwrap();
        assert_eq!(row["times_asked"], 1);
        assert_eq!(row["source_entry_id"], chain["request_id"]);
        assert_eq!(row["request"]["arguments"], json!([command]));
    }
}

#[test]
fn suggestions_default_to_sixteen_and_page_every_stable_group() {
    let (_root, context, connection) = fixture();
    let mut hook = context.clone();
    hook.surface = "hook".into();
    hook.source = "host".into();
    hook.session_id = Some("acme-session".into());
    hook.host_event_name = Some("PreToolUse".into());
    let asked = |executable: &str, arguments: &[&str]| {
        let guidance = json!([{"source":"decisive","message":"Acme asks.","request":{"executable":executable,"arguments":arguments}}]);
        review::observations::decision(
            &hook,
            json!({"decision":"ask","operation":{"kind":"argv","executable":executable,"arguments":arguments},
                "evaluation":{"explanation":{"guidance":guidance}}}),
        )
        .unwrap();
    };
    // The oldest is a download; then twenty distinct commands; the newest
    // downloads from the same site again.
    asked("curl", &["-fsS", "http://localhost:3000/health"]);
    for index in 0..20 {
        asked("acme", &["task", &index.to_string()]);
    }
    asked("curl", &["--silent", "http://localhost:8080/status"]);
    let result = invoke(
        &context,
        &connection,
        "suggestion_list",
        json!({"session_id":"acme-session"}),
    );
    let requests = result["data"]["requests"].as_array().unwrap();
    assert_eq!(requests.len(), 16, "{result}");
    assert!(result["data"]["next_cursor"].is_string());
    let mut all = requests.clone();
    let mut cursor = result["data"]["next_cursor"].clone();
    while cursor.is_string() {
        let page = invoke(
            &context,
            &connection,
            "suggestion_list",
            json!({"session_id":"acme-session","cursor":cursor}),
        );
        assert!(page["error"].is_null(), "{page}");
        all.extend(page["data"]["requests"].as_array().unwrap().iter().cloned());
        cursor = page["data"]["next_cursor"].clone();
    }
    assert_eq!(all.len(), 21);
    let ids: std::collections::BTreeSet<_> = all
        .iter()
        .map(|row| row["review_id"].as_str().unwrap())
        .collect();
    assert_eq!(ids.len(), 21);
    for index in 0..20 {
        assert!(
            all.iter()
                .any(|row| row["command"] == format!("acme task {index}"))
        );
    }
    assert_eq!(
        all.iter()
            .find(|row| row["decided_as"] == "web_fetch")
            .unwrap()["times_asked"],
        2
    );
}

#[test]
fn rules_naming_the_checkout_are_refused_with_the_portable_form() {
    let (_root, context, connection) = fixture();
    let mut hook = context.clone();
    hook.surface = "hook".into();
    hook.source = "host".into();
    hook.session_id = Some("acme-session".into());
    hook.host_event_name = Some("PreToolUse".into());
    let arguments = ["--prefix", context.project_root.as_str(), "start"];
    let guidance = json!([{"source":"decisive","message":"npm asks.","request":{"executable":"npm","arguments":arguments}}]);
    review::observations::decision(
        &hook,
        json!({"decision":"ask","operation":{"kind":"argv","executable":"npm","arguments":arguments},
            "evaluation":{"explanation":{"guidance":guidance}}}),
    )
    .unwrap();
    let result = invoke(
        &context,
        &connection,
        "suggestion_list",
        json!({"session_id":"acme-session"}),
    );
    // The suggestion warns before the agent spends a proposal on it.
    let suggestion = &result["data"]["requests"][0];
    let warning = suggestion["not_portable"].as_str().unwrap();
    assert!(warning.starts_with("The request names `"), "{warning}");
    assert!(warning.contains(&context.project_root), "{warning}");
    assert!(warning.contains("project folder"), "{warning}");
    assert!(warning.contains("cwd within {root: project}"), "{warning}");
    assert!(!warning.contains("allow-npm-"), "{warning}");
    let refused = invoke(
        &context,
        &connection,
        "policy_propose",
        json!({"change":{"kind":"rule_add","rule":{"id":"machine-specific","decision":"allow",
            "conditions":{"domain":"argv","executable":suggestion["request"]["executable"],
                "arguments":{"kind":"exact","values":suggestion["request"]["arguments"]}}}}}),
    );
    assert_eq!(refused["error"]["code"], "invalid_input", "{refused}");
    let message = refused["error"]["message"].as_str().unwrap();
    assert!(message.contains("Rule machine-specific names"), "{message}");
    assert!(message.contains(&context.project_root), "{message}");
    assert!(message.contains("cwd within {root: project}"), "{message}");
    let portable = invoke(
        &context,
        &connection,
        "policy_propose",
        json!({"change":{"kind":"rule_add","rule":{"id":"npm-start","decision":"allow",
            "conditions":{"domain":"argv","executable":"npm","arguments":{"kind":"exact","values":["start"]},
                "cwd":{"within":{"root":"project"}}}}}}),
    );
    assert!(portable["error"].is_null(), "{portable}");
}
