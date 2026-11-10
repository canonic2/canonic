use super::*;
use crate::review::{self, mcp};
use clap::Parser as _;
use std::{collections::BTreeMap, fs, io::Write};

fn fixture() -> (assert_fs::TempDir, Context, Value, Value) {
    let root = assert_fs::TempDir::new().unwrap();
    let pack = |id: &str| {
        json!({"schema_version":1,"kind":"pack","id":id,"version":"0.1.0","description":"Acme inspection","license":"MIT","maintainers":["Acme"],"engines":{"shield":"*"},"capabilities":["structured-rules-v1","argument-rules-v1"],"rules":[{"id":"inspect","decision":"allow","conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"exact","values":["inspect"]}}}]}).to_string()
    };
    // Raw source order intentionally differs from the sorted canonical lock.
    let packs = [pack("acme/z-last"), pack("acme/a-first")];
    let config = json!({"schema_version":1,"rules":"rules.yml","packs":[
        {"id":"acme/z-last","version":"0.1.0","profile":"recommended","path":"last.json"},
        {"id":"acme/a-first","version":"0.1.0","profile":"recommended","path":"first.json"}]})
    .to_string();
    let custom = json!({"schema_version":1,"kind":"custom","capabilities":["structured-rules-v1","argument-rules-v1"],"rules":[{"id":"custom","decision":"allow","conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"exact","values":["custom"]}}}]}).to_string();
    let bundle = crate::configuration::Bundle::capture(&config, &packs, Some(&custom)).unwrap();
    let candidates = BTreeMap::from([("project".into(), json!(bundle))]);
    let revision = crate::configuration::Evaluator::new(review::snapshots(&candidates).unwrap())
        .unwrap()
        .revision()
        .to_owned();
    let metadata = json!({"sources":{},"default":null,"installed":packs.iter().enumerate().map(|(index,source)|json!({"selector":format!("pack-{index}"),"id":if index==0{"acme/z-last"}else{"acme/a-first"},"version":"0.1.0","profile":"recommended","digest":crate::policy::fingerprint(source.as_bytes())})).collect::<Vec<_>>(),"projection":null});
    Store::initialize_active(root.path(), candidates, &revision, metadata.clone()).unwrap();
    let context = review::open(root.path()).unwrap();
    let reference = Store::inspect(root.path())
        .unwrap()
        .read_diagnostic_checkpoint()
        .unwrap()
        .0
        .candidates["project"]
        .clone();
    assert_eq!(reference["kind"], "retained-source-v2");
    (root, context, reference, metadata)
}

#[test]
fn narrow_policy_and_rule_reads_ignore_an_unrelated_missing_blob_without_hydration() {
    let (root, context, reference, _) = fixture();
    let store = Store::inspect(root.path()).unwrap();
    let missing = sources::captured_field(&store, &reference, "packs", Some(1)).unwrap();
    fs::remove_file(root.path().join(format!(
        ".shield/review/sources/{}.blob",
        crate::policy::fingerprint(missing.as_str().unwrap().as_bytes())
    )))
    .unwrap();
    let logs = assert_fs::TempDir::new().unwrap();
    let cli = crate::cli::Cli::parse_from([
        "shield",
        "--debug-log-dir",
        logs.path().to_str().unwrap(),
        "status",
    ]);
    let log = std::sync::Arc::new(crate::diagnostics::Log::open(&cli));
    let trace = log.request("mcp", "policy_show", None, None).unwrap();
    {
        let _scope = trace.enter();
        assert!(review::project::policy(&context).is_ok());
        assert!(
            review::inspection::rules(
                &context,
                Some(&json!({"scope":"custom","rule_id":"custom"}))
            )
            .is_ok()
        );
        let selected = json!({"scope":"pack","pack_selector":"pack-0","rule_id":"inspect"});
        assert_eq!(
            review::inspection::rules(&context, Some(&selected)).unwrap()["id"],
            "inspect"
        );
        let connection = mcp::Connection::new("selected-source".into());
        let value = mcp::call(
            &context,
            &connection,
            "rule_list",
            &json!({"rule_ref":selected}),
            &|| Ok(()),
        );
        assert!(value["error"].is_null(), "{value}");
        assert_eq!(trace.recorded_count("source.hydrated"), 0);
        assert_eq!(trace.recorded_count("policy.preparations"), 0);
    }
    trace.finish("success");
    assert!(store.captured_candidate(&reference).is_err());
}

fn object(root: &Path, name: &str, bytes: &[u8]) {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    review::storage::private(&mut options);
    options
        .open(root.join(".shield/review/sources").join(name))
        .unwrap()
        .write_all(bytes)
        .unwrap();
}

#[test]
fn partial_manifest_checksums_do_not_substitute_for_canonical_artifact_and_config_identity() {
    let (root, context, reference, metadata) = fixture();
    let store = Store::inspect(root.path()).unwrap();
    let revision = store
        .read_diagnostic_checkpoint()
        .unwrap()
        .0
        .policy_revision;
    for field in ["rules", "packs", "config"] {
        let mut manifest: Value = serde_json::from_slice(
            &fs::read(root.path().join(format!(
                ".shield/review/sources/{}.manifest.json",
                reference["manifest_digest"].as_str().unwrap()
            )))
            .unwrap(),
        )
        .unwrap();
        let index = (field == "packs").then_some(0);
        let original = sources::captured_field(&store, &reference, field, index).unwrap();
        let changed = if field == "config" {
            original.as_str().unwrap().replace("recommended", "strict")
        } else {
            original.as_str().unwrap().replace("allow", "deny")
        };
        let digest = crate::policy::fingerprint(changed.as_bytes());
        object(root.path(), &format!("{digest}.blob"), changed.as_bytes());
        let blob = json!({"digest":digest,"length":changed.len()});
        if field == "packs" {
            manifest["fields"][field]["value"][0] = blob;
        } else {
            manifest["fields"][field]["value"] = blob;
        }
        let bytes = serde_json::to_vec(&manifest).unwrap();
        let digest = crate::policy::fingerprint(&bytes);
        object(root.path(), &format!("{digest}.manifest.json"), &bytes);
        let mut forged = reference.clone();
        forged["manifest_digest"] = json!(digest);
        forged["manifest_length"] = json!(bytes.len());
        assert!(
            sources::captured_field(&store, &forged, field, index).is_ok(),
            "forged field has valid local object checksums"
        );
        let selected = if field == "rules" {
            json!({"scope":"custom","rule_id":"custom"})
        } else {
            json!({"scope":"pack","pack_selector":"pack-0","rule_id":"inspect"})
        };
        assert!(
            rules(&context, &forged, &metadata, &revision, Some(&selected)).is_err(),
            "{field}: canonical lock must reject the substituted field"
        );
    }
}

#[test]
fn proposal_metadata_uses_bound_presentation_and_lazy_source_handles_without_other_pack_blobs() {
    let (root, mut context, _, _) = fixture();
    context.surface = "mcp".into();
    context.agent = Some("claude".into());
    context.connection_id = Some("proposal-source-view".into());
    let owner = mcp::Connection::new("source-view-owner".into());
    let proposed = mcp::call(
        &context,
        &owner,
        "policy_propose",
        &json!({"change":{"kind":"rule_add","rule":{"id":"new-rule","decision":"deny","conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"exact","values":["new"]}}}}}),
        &|| Ok(()),
    );
    assert!(proposed["error"].is_null(), "{proposed}");
    let id = proposed["data"]["proposal_id"].as_str().unwrap();
    let store = Store::inspect(root.path()).unwrap();
    let state = store.read_diagnostic_checkpoint().unwrap().0;
    let row = state.proposals.iter().find(|row| row.id == id).unwrap();
    assert_eq!(row.presentation["kind"], "retained-json-v2");
    let missing = sources::captured_field(&store, &row.candidate, "packs", Some(1)).unwrap();
    fs::remove_file(root.path().join(format!(
        ".shield/review/sources/{}.blob",
        crate::policy::fingerprint(missing.as_str().unwrap().as_bytes())
    )))
    .unwrap();
    let checkpoint = fs::read(root.path().join(".shield/review/state.json")).unwrap();
    let logs = assert_fs::TempDir::new().unwrap();
    let cli = crate::cli::Cli::parse_from([
        "shield",
        "--debug-log-dir",
        logs.path().to_str().unwrap(),
        "status",
    ]);
    let log = std::sync::Arc::new(crate::diagnostics::Log::open(&cli));
    let trace = log.request("mcp", "policy_show", None, None).unwrap();
    // Retain the same owned interaction with an initially empty view cache.
    let reader = mcp::Connection::new("source-view-reader".into());
    let shown;
    {
        let _scope = trace.enter();
        shown = mcp::call(
            &context,
            &reader,
            "policy_show",
            &json!({"proposal_id":id}),
            &|| Ok(()),
        );
        assert!(shown["error"].is_null(), "{shown}");
        assert_eq!(shown["data"]["presentation_availability"], "available");
        assert_eq!(
            shown["data"]["apply_arguments"],
            proposed["data"]["apply_arguments"]
        );
        assert_eq!(trace.recorded_count("source.hydrated"), 0);
        assert_eq!(trace.recorded_count("policy.preparations"), 0);
        assert_eq!(
            fs::read(root.path().join(".shield/review/state.json")).unwrap(),
            checkpoint
        );
    }
    trace.finish("success");
    let evaluated = mcp::call(
        &context,
        &reader,
        "policy_evaluate",
        &json!({"candidate_id":shown["data"]["after_candidate_id"],"input":{"kind":"argv","executable":"acme","arguments":["new"],"original_input":{}},"context":{"cwd":context.project_root,"environment":null}}),
        &|| Ok(()),
    );
    assert!(
        !evaluated["error"].is_null(),
        "explicit evaluation must validate the complete capture: {evaluated}"
    );
    let presentation_path = root.path().join(format!(
        ".shield/review/sources/{}.blob",
        row.presentation["digest"].as_str().unwrap()
    ));
    fs::write(presentation_path, b"{}").unwrap();
    let unavailable = mcp::call(
        &context,
        &reader,
        "policy_show",
        &json!({"proposal_id":id}),
        &|| Ok(()),
    );
    assert!(unavailable["error"].is_null(), "{unavailable}");
    assert_eq!(
        unavailable["data"]["presentation_availability"],
        "unavailable"
    );
    assert!(unavailable["data"].get("apply_arguments").is_none());
}

#[test]
fn proposal_presentation_requires_matching_source_metadata_and_event() {
    let (root, mut context, _, _) = fixture();
    context.surface = "mcp".into();
    context.agent = Some("claude".into());
    context.connection_id = Some("presentation-binding".into());
    let connection = mcp::Connection::new("presentation-binding".into());
    let proposed = mcp::call(
        &context,
        &connection,
        "policy_propose",
        &json!({"change":{"kind":"rule_add","rule":{"id":"bound-rule","decision":"deny","conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"exact","values":["bound"]}}}}}),
        &|| Ok(()),
    );
    assert!(proposed["error"].is_null(), "{proposed}");
    let store = Store::inspect(root.path()).unwrap();
    let original = store.read_diagnostic_checkpoint().unwrap().0;
    let namespace = Namespace::project(&store);
    let mut forged = namespace
        .read_json(&original.proposals[0].presentation)
        .unwrap();
    forged["summary"] = json!("Unbound display text");
    let (forged_reference, _pin) = namespace.retain_json(&forged).unwrap();
    for fault in ["source", "metadata", "event", "missing", "reference"] {
        let mut state = store.read_diagnostic_checkpoint().unwrap().0;
        let row = &mut state.proposals[0];
        match fault {
            "source" => row.candidate = row.base_candidate.clone(),
            "metadata" => row.candidate_metadata["extra"] = json!("unbound"),
            "event" => state
                .authorization_events
                .retain(|event| event["kind"] != "proposal_created"),
            "reference" => row.presentation = forged_reference.clone(),
            _ => row.presentation = Value::Null,
        }
        assert!(
            review::model::presentation::read(&context, &state, &state.proposals[0]).is_none(),
            "{fault}"
        );
    }
}
