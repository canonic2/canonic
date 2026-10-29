//! Available-pack review stays small while exact evidence remains accessible.
use super::*;

fn available(context: &Context, root: &assert_fs::TempDir) -> assert_fs::TempDir {
    let source = assert_fs::TempDir::new().unwrap();
    let directory = source.path().canonicalize().unwrap();
    let rules: Vec<_> = (0..120).map(|index| json!({
        "id":format!("inspect-{index:03}"),"reason":"Inspect Acme metadata.",
        "conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"exact","values":[format!("inspect-{index}")]}},
        "decision":{"loose":"allow","recommended":"ask","strict":"deny"}
    })).chain([json!({"id":"install","reason":"Trust Acme local dependency installation.",
        "conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"exact","values":["install"]}},
        "decision":{"loose":"allow","recommended":"allow","strict":"ask"},
        "requirements":{"cwd":{"within":{"root":"project"}}},
        "on_unmet":{"decision":"deny","message":"Use the project directory."}})]).collect();
    let pack = json!({"schema_version":1,"kind":"pack","id":"acme/runtime","version":"1.0.0",
        "description":"Acme metadata and local installs.","license":"MIT","maintainers":["Acme"],
        "engines":{"shield":env!("CARGO_PKG_VERSION")},
        "capabilities":["argument-rules-v1","location-v1","requirements-v1","structured-rules-v1","variables-v1","preconditions-v1","templates-v1"],
        "variables":{"manager":{"type":"choice","description":"Select the Acme manager.","values":["acme","other"]}},
        "preconditions":[{"id":"selected-manager","when":[{"var":"manager","equals":"acme"}],
            "conditions":{"domain":"argv","executable":{"one_of":["acme","other"]}},
            "requires_any":[{"executable":"acme"}],"on_unmet":{"decision":"deny","message":"Use {{manager}}."}}],
        "rules":rules}).to_string();
    crate::policy::Pack::from_yaml(&pack).unwrap();
    std::fs::write(directory.join("runtime.json"), &pack).unwrap();
    std::fs::write(directory.join("catalog.json"), json!({"schema_version":1,"packs":[{
        "id":"acme/runtime","version":"1.0.0","path":"runtime.json","sha256":crate::policy::fingerprint(pack.as_bytes())
    }]}).to_string()).unwrap();
    Store::open(root.path())
        .unwrap()
        .update(context, &|| Ok(()), |state| {
            state.operator["sources"]["tools"] =
                json!(canonic_registry::source::Source::local(&directory).unwrap());
            Ok(json!({}))
        })
        .unwrap();
    source
}

#[test]
fn pack_info_summarizes_profiles_and_groups_without_returning_full_rules() {
    let (root, context, connection) = fixture();
    let _source = available(&context, &root);
    let result = invoke(
        &context,
        &connection,
        "pack_info",
        json!({"reference":"tools/acme/runtime"}),
    );
    assert_eq!(result["error"], Value::Null, "{result}");
    let data = &result["data"];
    assert_eq!(data["rule_count"], 121);
    assert_eq!(data["permission_group_count"], 2);
    assert_eq!(
        data["profiles"]["recommended"]["rule_outcomes"],
        json!({"allow":1,"ask":120,"deny":0})
    );
    assert_eq!(
        data["profiles"]["strict"]["unmet_requirement_outcomes"]["deny"],
        1
    );
    assert!(data.get("rules").is_none());
    assert_eq!(data["truncated"], false);
    assert!(result.to_string().len() < 8 * 1024);
    assert_eq!(data["selected_release"]["id"], "acme/runtime");
    let examples = &data["profiles"]["recommended"]["examples"];
    assert_eq!(examples["allow"][0]["rule_id"], "install");
    assert_eq!(examples["allow"][0]["conditional"], true);
    assert_eq!(examples["allow"][0]["on_unmet_decision"], "deny");
    assert_eq!(examples["ask"].as_array().unwrap().len(), 1);
    assert_eq!(examples["ask"][0]["description"], "Inspect Acme metadata.");
    assert_eq!(
        data["profiles"]["strict"]["examples"]["deny"][0]["rule_id"],
        "inspect-000"
    );
    assert_eq!(
        data["variables"]["manager"]["values"],
        json!(["acme", "other"])
    );
    assert_eq!(data["precondition_count"], 1);
    assert_eq!(data["preconditions"][0]["on_unmet_decisions"], "deny");
    let state = Store::open(root.path()).unwrap().read().unwrap();
    assert!(state.proposals.is_empty());
    assert_eq!(state.operator["installed"], json!([]));
}

#[test]
fn pack_info_filters_pages_retains_evidence_and_selects_exact_rules() {
    let (root, context, connection) = fixture();
    let source = available(&context, &root);
    let args = json!({"reference":"tools/acme/runtime","view":"rules","query":"inspect"});
    let first = invoke(&context, &connection, "pack_info", args.clone());
    assert_eq!(first["error"], Value::Null, "{first}");
    assert_eq!(first["data"]["rules"].as_array().unwrap().len(), 10);
    let exact = invoke(
        &context,
        &connection,
        "pack_info",
        json!({"reference":"tools/acme/runtime","rule_id":"install"}),
    );
    assert_eq!(
        exact["data"]["rule"]["conditions"]["arguments"]["values"],
        json!(["install"])
    );
    assert_eq!(exact["data"]["rule"]["effective_on_unmet_decision"], "deny");
    let guard = invoke(
        &context,
        &connection,
        "pack_info",
        json!({"reference":"tools/acme/runtime","rule_id":"selected-manager"}),
    );
    assert_eq!(
        guard["data"]["rule"]["requires_any"],
        json!([{"executable":"acme"}])
    );
    let settings = invoke(
        &context,
        &connection,
        "pack_info",
        json!({"reference":"tools/acme/runtime","view":"settings"}),
    );
    assert_eq!(
        settings["data"]["preconditions"][0]["requires_any"],
        json!([{"executable":"acme"}])
    );
    assert_eq!(settings["data"]["variables"]["manager"]["type"], "choice");
    let missing = invoke(
        &context,
        &connection,
        "pack_info",
        json!({"reference":"tools/acme/runtime","rule_id":"absent"}),
    );
    assert_eq!(missing["error"]["code"], "not_found");
    // Cursor continuation reads the acquired evidence, without another registry fetch.
    std::fs::remove_file(source.path().join("runtime.json")).unwrap();
    let mut continuation = args;
    continuation["cursor"] = first["data"]["next_cursor"].clone();
    let next = invoke(&context, &connection, "pack_info", continuation.clone());
    assert_eq!(next["error"], Value::Null, "{next}");
    assert_eq!(next["data"]["rules"][0]["id"], "inspect-010");
    continuation["query"] = json!("install");
    assert_eq!(
        invoke(&context, &connection, "pack_info", continuation)["error"]["code"],
        "cursor_unavailable"
    );
}

#[test]
fn large_pack_info_marks_omissions_and_preserves_complete_outcome_counts() {
    let (root, context, connection) = fixture();
    let source = available(&context, &root);
    let path = source.path().join("runtime.json");
    let mut document: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    for index in 0..60 {
        let mut row = rule(&format!("category-{index}"), "ask");
        row["reason"] = json!(format!(
            "Acme permission class {index}: {}",
            "x".repeat(300)
        ));
        document["rules"].as_array_mut().unwrap().push(row);
    }
    let bytes = document.to_string();
    std::fs::write(&path, &bytes).unwrap();
    let catalog_path = source.path().join("catalog.json");
    let mut catalog: Value =
        serde_json::from_slice(&std::fs::read(&catalog_path).unwrap()).unwrap();
    catalog["packs"][0]["sha256"] = json!(crate::policy::fingerprint(bytes.as_bytes()));
    std::fs::write(catalog_path, catalog.to_string()).unwrap();
    let result = invoke(
        &context,
        &connection,
        "pack_info",
        json!({"reference":"tools/acme/runtime"}),
    );
    assert_eq!(result["error"], Value::Null, "{result}");
    assert!(result["data"].to_string().len() <= 16 * 1024);
    assert_eq!(result["truncated"], true);
    assert!(
        result["missing"]
            .as_array()
            .unwrap()
            .contains(&json!("permission_groups"))
    );
    assert_eq!(result["data"]["rule_count"], 181);
    assert_eq!(result["data"]["permission_group_count"], 62);
    assert_eq!(
        result["data"]["profiles"]["recommended"]["rule_outcomes"]["ask"],
        180
    );
}

#[test]
fn pack_info_rejects_mixed_summary_and_rule_arguments() {
    let (_root, context, connection) = fixture();
    for fields in [
        json!({"view":"raw"}),
        json!({"limit":2}),
        json!({"query":"install"}),
        json!({"rule_id":"install","view":"rules"}),
        json!({"rule_id":"install","query":"install"}),
    ] {
        let mut args = fields;
        args["reference"] = json!("tools/acme/runtime");
        assert_eq!(
            invoke(&context, &connection, "pack_info", args)["error"]["code"],
            "invalid_input"
        );
    }
}
