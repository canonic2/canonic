//! Behavioral checks for the canonical project tool surface and authority separation.
mod application;
mod cross_source;
mod documentation;
mod file_editing;
mod pack_acquisition;
mod pack_identity;
mod pack_info;
mod rule_proposals;
mod suggestion_workflow;
use super::*;
use crate::{
    configuration::Bundle,
    review::{self, storage::Store},
};
use std::{collections::BTreeMap, path::Path};

fn fixture() -> (assert_fs::TempDir, Context, Connection) {
    fixture_with(&Bundle::capture(r#"{"schema_version":1,"packs":[]}"#, &[], None).unwrap())
}

#[test]
fn mcp_connections_setup_refusal_always_names_terminal_review_even_for_supported_hosts() {
    let (_root, mut context, _connection) = fixture();
    for agent in ["claude-code", "codex"] {
        context.agent = Some(agent.into());
        let response = failure(&context, &Error::McpSetupRequired);
        assert_eq!(response["error"]["code"], "mcp_setup_required");
        assert_eq!(response["error"]["operator_route"], "shield policy review");
    }
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
    review::operator::commit_proposal_fixture(
        context,
        proposed["data"]["proposal_id"].as_str().unwrap(),
    )
    .unwrap();
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
    Store::open(root.path())
        .unwrap()
        .update(&context, &|| Ok(()), |state| {
            let sources = state.operator["sources"].as_object_mut().unwrap();
            let source = sources.remove("acme-dependencies").unwrap();
            sources.insert("renamed".into(), source);
            Ok(json!({}))
        })
        .unwrap();
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
    let pending = Store::open(root.path()).unwrap().read().unwrap();
    assert_eq!(json!(pending.active), json!(before.active));
    let updated = pending
        .proposals
        .iter()
        .find(|p| p.id == proposed["data"]["proposal_id"])
        .unwrap();
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
    let stale = pack_change(
        &context,
        &connection,
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
        assert_eq!(result.0, before.candidates["project"]);
        assert_eq!(result.1, before.operator);
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
    assert_eq!(result.0, before.candidates["project"]);
    assert_eq!(result.1, before.operator);
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
    assert_eq!(json!(after.active), json!(before.active));
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
    assert_eq!(json!(after.active), json!(before.active));
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
    let pending = Store::open(root.path()).unwrap().read().unwrap();
    assert_eq!(json!(pending.active), json!(before.active));
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
        assert_eq!(json!(after.active), json!(before.active));
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
        json!(Store::open(root.path()).unwrap().read().unwrap().active),
        json!(before.active)
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
fn a_cached_diagnostic_cannot_hide_changed_active_snapshot_bytes() {
    let (root, context, connection) = fixture();
    let evaluated = invoke(&context, &connection, "policy_evaluate", request(&context));
    let id = evaluated["data"]["candidate_id"].clone();
    let path = root.path().join(".shield/review/state.json");
    let mut state: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    state["active"]["candidates"]["project"]["config"] =
        json!("schema_version: 1\npacks: []\nrules: forged.yml\n");
    std::fs::write(&path, state.to_string()).unwrap();
    assert_eq!(
        invoke(&context, &connection, "policy_evaluate", request(&context))["error"]["code"],
        "invalid_input"
    );
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
            "status",
            "doctor",
            "pack_list",
            "registry_list",
            "pack_search",
            "pack_info",
            "policy_show",
            "rule_list",
            "policy_propose",
            "suggestion_list",
            "review_defer",
            "policy_withdraw",
            "policy_apply",
            "policy_receipt",
            "request_list",
            "history_list",
            "policy_evaluate",
            "policy_compare",
            "shield_docs_list",
            "shield_docs_search",
            "shield_docs_read"
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
    for tool in tools()["tools"].as_array().unwrap() {
        assert!(tool["outputSchema"].is_object());
        assert_eq!(
            tool["annotations"]["destructiveHint"],
            ["policy_withdraw", "policy_apply"]
                .iter()
                .any(|name| tool["name"] == *name)
        );
    }
    assert_eq!(
        tools()["tools"][2]["inputSchema"]["properties"]["reference"]["type"],
        "string"
    );
}
#[test]
fn diagnostics_create_no_action_or_history_and_handles_remain_frozen_after_commit() {
    let (root, mut context, connection) = fixture();
    let initial =
        invoke(&context, &connection, "status", json!({}))["data"]["active_candidate_id"].clone();
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
        invoke(
            &context,
            &connection,
            "policy_receipt",
            json!({"proposal_id":id})
        )["error"]["code"],
        "operator_review_required"
    );
    context.surface = "cli".into();
    review::operator::commit_proposal_fixture(&context, id).unwrap();
    context.surface = "mcp".into();
    assert_eq!(
        invoke(&context, &connection, "policy_evaluate", request(&context))["data"]["decision"],
        "allow"
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
    let receipt = invoke(
        &context,
        &connection,
        "policy_receipt",
        json!({"proposal_id":id}),
    );
    assert!(
        receipt["data"]["receipt"]["activation_id"].is_string(),
        "{receipt}"
    );
    Store::open(root.path())
        .unwrap()
        .inspect_locked(&context, &|| Ok(()), |state| {
            let again = invoke(
                &context,
                &connection,
                "policy_receipt",
                json!({"proposal_id":id}),
            );
            assert_eq!(again, receipt);
            assert_eq!(
                Store::open(root.path()).unwrap().read().unwrap().sequence,
                state.sequence
            );
            Ok(())
        })
        .unwrap();
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
        json!({
            "candidate_id":candidate,"limit":1
        }),
    );
    assert!(first["error"].is_null(), "{first}");
    assert_eq!(first["data"]["rules"][0]["id"], "a");
    let cursor = first["data"]["next_cursor"].clone();
    assert!(cursor.is_string());
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
    let foreign = invoke(
        &context,
        &Connection::new("foreign".into()),
        "rule_list",
        json!({
            "candidate_id":candidate,"rule_ref":reference
        }),
    );
    assert_eq!(foreign["error"]["code"], "candidate_unavailable");
}

#[test]
fn stored_policy_receipt_does_not_prepare_unavailable_retained_source() {
    let (root, context, connection) = fixture();
    let proposed = proposal(&context, &connection, "receipt-without-source");
    let id = proposed["data"]["proposal_id"].as_str().unwrap();
    review::operator::commit_proposal_fixture(&context, id).unwrap();
    let receipt = Store::open(root.path())
        .unwrap()
        .read_checkpoint()
        .unwrap()
        .proposals
        .iter()
        .find(|row| row.id == id)
        .unwrap()
        .receipt
        .clone();
    let state_path = root.path().join(".shield/review/state.json");
    let before = std::fs::read(&state_path).unwrap();
    for entry in std::fs::read_dir(root.path().join(".shield/review/sources")).unwrap() {
        std::fs::write(entry.unwrap().path(), "invalid retained source").unwrap();
    }
    let inspected = invoke(
        &context,
        &connection,
        "policy_receipt",
        json!({"proposal_id":id}),
    );
    assert!(inspected["error"].is_null(), "{inspected}");
    assert_eq!(inspected["data"]["receipt"], receipt);
    assert_eq!(std::fs::read(state_path).unwrap(), before);
}

#[test]
fn withdrawn_proposal_receipt_is_terminal_without_an_approval_route() {
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
    let receipt = invoke(
        &context,
        &connection,
        "policy_receipt",
        json!({"proposal_id":id}),
    );
    assert_eq!(receipt["error"]["code"], "invalid_input", "{receipt}");
    assert!(
        receipt["error"]["message"]
            .as_str()
            .unwrap()
            .contains("terminal"),
        "{receipt}"
    );
    assert!(receipt["error"]["operator_route"].is_null(), "{receipt}");
}

#[test]
fn approval_required_messages_follow_the_enrolled_adapter() {
    let (_root, mut context, _connection) = fixture();
    for (agent, route, expected) in [
        (
            Some("codex"),
            "policy_apply",
            "If the approval policy is never",
        ),
        (
            Some("claude-code"),
            "policy_apply",
            "copy its apply_arguments object unchanged into policy_apply",
        ),
        (
            None,
            "shield policy review",
            "shield policy review in a terminal",
        ),
        (
            Some("acme-agent"),
            "shield policy review",
            "shield policy review in a terminal",
        ),
    ] {
        context.agent = agent.map(str::to_owned);
        let error = &failure(&context, &Error::ApprovalRequired)["error"];
        assert_eq!(error["code"], "operator_review_required");
        assert_eq!(error["operator_route"], route);
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
            Error::CursorUnavailable,
            "cursor_unavailable",
            "Restart the list without cursor",
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
        &Error::Dependency(canonic_registry::resolution::Error::Unenrolled(
            "acme-foreign".into(),
        )),
    );
    let error = &unenrolled["error"];
    assert_eq!(
        error["operator_route"], "shield registry add",
        "{unenrolled}"
    );
    let message = error["message"].as_str().unwrap();
    assert!(
        message.contains("acme-foreign") && message.contains("shield registry add"),
        "{message}"
    );
}

#[test]
fn receipts_route_pending_proposals_to_apply_and_failed_ones_to_operator_review() {
    let (root, mut context, connection) = fixture();
    context.agent = Some("claude-code".into());
    let proposed = proposal(&context, &connection, "failed-receipt");
    let id = proposed["data"]["proposal_id"].as_str().unwrap().to_owned();
    let receipt = || {
        invoke(
            &context,
            &connection,
            "policy_receipt",
            json!({"proposal_id":id}),
        )
    };
    let pending = receipt();
    assert_eq!(pending["error"]["code"], "operator_review_required");
    assert_eq!(pending["error"]["operator_route"], "policy_apply");
    Store::open(root.path())
        .unwrap()
        .update(&context, &|| Ok(()), |state| {
            let row = state.proposals.iter_mut().find(|p| p.id == id).unwrap();
            row.status = "failed".into();
            Ok(json!({}))
        })
        .unwrap();
    let failed = receipt();
    assert_eq!(failed["error"]["code"], "operator_review_required");
    assert_eq!(failed["error"]["operator_route"], "shield policy review");
    let message = failed["error"]["message"].as_str().unwrap();
    assert!(message.contains("shield policy review"), "{message}");
    assert!(!message.contains("apply_arguments"), "{message}");
}

#[test]
fn normalized_diagnostics_reject_files_as_working_directories() {
    let (root, context, connection) = fixture();
    let path = root.path().join("acme.txt");
    std::fs::write(&path, "Acme fixture").unwrap();
    let before = std::fs::read(root.path().join(".shield/review/state.json")).unwrap();
    let mut input = request(&context);
    input["context"]["cwd"] = json!(path);
    let result = invoke(&context, &connection, "policy_evaluate", input);
    assert_eq!(result["error"]["code"], "invalid_input", "{result}");
    assert_eq!(
        std::fs::read(root.path().join(".shield/review/state.json")).unwrap(),
        before
    );
}

#[test]
fn handle_capacity_refuses_a_proposal_before_it_is_persisted() {
    let (root, context, connection) = fixture();
    let state = Store::open(root.path()).unwrap().read().unwrap();
    let mut cache = connection.views.lock().unwrap();
    cache.active(&context).unwrap();
    for index in 1..256 {
        let mut metadata = state.operator.clone();
        metadata["test_identity"] = json!(index);
        cache
            .retain(&state.candidates["project"], &metadata)
            .unwrap();
    }
    drop(cache);
    assert_eq!(
        proposal(&context, &connection, "overflow")["error"]["code"],
        "overloaded"
    );
    assert!(
        Store::open(root.path())
            .unwrap()
            .read()
            .unwrap()
            .proposals
            .is_empty()
    );
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
                state.proposals.push(serde_json::from_value(copy).unwrap());
            }
            Ok(json!({}))
        })
        .unwrap();
    let refused = proposal(&context, &connection, "overflow");
    let error = &refused["error"];
    assert_eq!(error["code"], "capacity_reached", "{refused}");
    assert_eq!(error["operator_route"], "shield policy review", "{refused}");
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
        "1 pending",
        "1024 retained",
        "limit 1024",
        "cannot be cleared",
    ] {
        assert!(message.contains(expected), "{message}");
    }
    assert!(!message.contains("retry"), "{message}");
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
    for agent in ["acme", "claude-code", "codex"] {
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
        assert_eq!(error["operator_route"], "shield policy review", "{refused}");
        let message = error["message"].as_str().unwrap();
        assert!(message.contains("shield policy review"), "{message}");
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
    for id in ["a", "b", "c"] {
        assert!(proposal(&context, &connection, id)["error"].is_null());
    }
    let first = invoke(&context, &connection, "policy_show", json!({"limit":1}));
    let cursor = first["data"]["next_cursor"].clone();
    assert!(cursor.is_string());
    let id = first["data"]["proposals"][0]["proposal_id"].clone();
    invoke(
        &context,
        &connection,
        "policy_withdraw",
        json!({"proposal_id":id}),
    );
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
        "cursor_unavailable"
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
    let executable = Path::new("/bin/echo")
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
        json!({"kind":"access_path_add","access":"read","path":"/srv/acme","layer":"local"}),
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
    assert_eq!(merged["local_access_paths"]["read"], json!(["/srv/acme"]));
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
    // Every distinct asked command of the session, newest first; the agent
    // decides which deserve a rule and drafts it.
    let requests = result["data"]["requests"].as_array().unwrap();
    let commands: Vec<_> = requests.iter().map(|s| s["command"].clone()).collect();
    assert_eq!(commands, [json!("acme build"), json!("acme run dev")]);
    assert_eq!(requests[1]["times_asked"], 2);
    assert_eq!(requests[1]["pack_requests_rule"], true);
    assert_eq!(requests[0]["pack_requests_rule"], false);
    assert_eq!(requests[0]["decided_as"], "command");
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
    assert_eq!(rows[0]["command"], "npm test");
    assert_eq!(rows[0]["pack_requests_rule"], true);
    assert_eq!(rows[1]["command"], "acme-lint");
    assert_eq!(rows[1]["pack_requests_rule"], false);
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
            assert_eq!(rows[1]["command"], "acme-lint");
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
        assert_eq!(rows[0]["times_asked"], 3, "{result}");
        assert_eq!(rows[1]["times_asked"], 2, "{result}");
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
    for (row, index) in rows[..15].iter().zip((0..15).rev()) {
        assert_eq!(row["command"], format!("acme task-{index}"));
        assert_eq!(row["times_asked"], if index <= 1 { 2 } else { 1 });
    }
    assert_eq!(rows[15]["command"], "acme older-one");
    assert_eq!(rows[15]["times_asked"], 1);
    assert_eq!(rows[15]["source_entry_id"], chain["request_id"]);
    assert_eq!(rows[15]["request"]["arguments"], json!(["older-one"]));
    // Remaining rows stay available through a consistent continuation.
    assert!(rows.iter().all(|row| row["command"] != "acme older-two"));
    let next = invoke(
        &context,
        &connection,
        "suggestion_list",
        json!({"session_id":"acme-chain-limit","cursor":result["data"]["next_cursor"]}),
    );
    assert_eq!(next["data"]["requests"][0]["command"], "acme older-two");
}

#[test]
fn suggestions_list_only_the_sixteen_newest_requests() {
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
    // Newest first: the newest download, then the newest commands.
    assert_eq!(requests[0]["decided_as"], "web_fetch");
    assert_eq!(requests[1]["command"], "acme task 19");
    assert_eq!(requests[15]["command"], "acme task 5");
    // Paging does not prevent an older retained observation joining its group.
    assert_eq!(requests[0]["times_asked"], 2);
    assert!(result["data"]["next_cursor"].is_string());
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
