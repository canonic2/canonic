//! Pack maintenance keeps capture and final authorization separate.
use super::*;

#[test]
fn pack_parity_bare_configure_rejects_invalid_metadata_before_asking() {
    let (transport, project) = prepared("claude", &json!({"elicitation":{"form":{}}}));
    let count = proposals(&project);
    for (index, metadata) in [
        json!({"idempotency_key":null}),
        json!({"idempotency_key":""}),
        json!({"idempotency_key":"x".repeat(129)}),
        json!({"idempotency_key":"clé"}),
        json!({"agent_reason":null}),
        json!({"agent_reason":"x".repeat(4097)}),
    ]
    .into_iter()
    .enumerate()
    {
        let mut arguments = metadata;
        arguments["reference"] = json!("acme/settings");
        let response = transport.call(json!(10 + index), "pack_configure", &arguments);
        assert_eq!(
            data(&response)["error"]["code"],
            "invalid_input",
            "{response}"
        );
        assert_eq!(proposals(&project), count);
    }
}

#[test]
fn pack_parity_supplied_values_validate_before_retaining_a_proposal() {
    for arguments in [
        json!({"reference":"fixture/acme/settings","set":{"missing":true}}),
        json!({"reference":"fixture/acme/settings","set":{"manager":"unknown"}}),
        json!({"reference":"fixture/acme/settings","variables":["manager"],"set":{"manager":"just"}}),
        json!({"packs":[{"reference":"fixture/acme/settings","set":{"manager":"just"}},{"reference":"fixture/acme/settings","set":{"enabled":true}}]}),
    ] {
        let (transport, project, _source) =
            fresh(false, VERSION, &json!({"elicitation":{"form":{}}}));
        let before = project.revision();
        let response = transport.call(json!(10), "pack_add", &arguments);
        assert!(!data(&response)["error"].is_null(), "{response}");
        assert_eq!(proposals(&project), 0);
        assert_eq!(project.revision(), before);
    }
}

#[test]
fn pack_parity_supplied_batch_values_support_local_free_text_lists_without_forms() {
    let (transport, project, source) = fresh(false, VERSION, &json!({"elicitation":{"form":{}}}));
    let mut pack: Value =
        serde_json::from_str(&std::fs::read_to_string(source.join("pack.yml")).unwrap()).unwrap();
    pack["variables"]["paths"] =
        json!({"type":"list","items":{"type":"string"},"description":"Paths","required":true});
    let mut child = pack.clone();
    child["id"] = json!("acme/child");
    publish(
        &source,
        &[
            ("pack.yml", pack.to_string()),
            ("child.yml", child.to_string()),
        ],
    );
    let before = project.revision();
    let proposal = transport.call(
        json!(10),
        "pack_add",
        &json!({"layer":"local","packs":[
            {"reference":"fixture/acme/settings","set":{"paths":["src","tests"],"manager":"just"}},
            {"reference":"fixture/acme/child","set":{"paths":["docs"],"enabled":true}}
        ]}),
    );
    assert!(proposal["result"].is_object(), "{proposal}");
    assert!(data(&proposal)["error"].is_null(), "{proposal}");
    assert_eq!(project.revision(), before);
    assert_eq!(proposals(&project), 1);
    approve(&transport, 11, &proposal);
    let config: Value = crate::policy::parse(
        &std::fs::read_to_string(
            std::path::Path::new(&project.review.project_root).join(".shield/config.local.yml"),
        )
        .unwrap(),
    )
    .unwrap();
    let rows = config["packs"].as_array().unwrap();
    assert_eq!(
        rows.iter()
            .find(|row| row["id"] == "acme/settings")
            .unwrap()["variables"]["paths"],
        json!(["src", "tests"])
    );
    assert_eq!(
        rows.iter().find(|row| row["id"] == "acme/child").unwrap()["variables"]["paths"],
        json!(["docs"])
    );
}

#[test]
fn pack_parity_update_collects_new_required_settings_from_retained_release() {
    let (transport, project, source) = fresh(false, VERSION, &json!({"elicitation":{"form":{}}}));
    let proposal = transport.call(
        json!(10),
        "pack_add",
        &json!({"reference":"fixture/acme/settings"}),
    );
    approve(&transport, 11, &proposal);
    let mut pack: Value =
        serde_json::from_str(&std::fs::read_to_string(source.join("pack.yml")).unwrap()).unwrap();
    pack["version"] = json!("1.1.0");
    pack["variables"]["target"] =
        json!({"type":"string","description":"Required target","required":true});
    publish(&source, &[("new.yml", pack.to_string())]);
    let before = project.revision();
    let count = proposals(&project);
    let form = transport.call(json!(12), "pack_update", &json!({}));
    assert_eq!(form["method"], "elicitation/create", "{form}");
    let declined = transport.request(&answer(&form, "decline", json!({})));
    assert_eq!(
        data(&declined)["error"]["code"],
        "input_declined",
        "{declined}"
    );
    assert_eq!(proposals(&project), count);
    assert_eq!(project.revision(), before);
    let form = transport.call(json!(13), "pack_update", &json!({}));
    assert_eq!(form["method"], "elicitation/create", "{form}");
    let properties = form["params"]["requestedSchema"]["properties"]
        .as_object()
        .unwrap();
    assert_eq!(properties.len(), 1);
    let key = properties.keys().next().unwrap();
    let mut values = json!({});
    values[key] = json!("src");
    std::fs::remove_file(source.join("catalog.json")).unwrap();
    std::fs::remove_file(source.join("new.yml")).unwrap();
    let proposal = transport.request(&answer(&form, "accept", values));
    assert!(data(&proposal)["error"].is_null(), "{proposal}");
    assert_eq!(project.revision(), before);
    assert_eq!(proposals(&project), count + 1);
    approve(&transport, 14, &proposal);
    let shown = transport.call(
        json!(15),
        "pack_show",
        &json!({"reference":"acme/settings"}),
    );
    assert_eq!(data(&shown)["data"]["pack"]["version"], "1.1.0", "{shown}");
}

#[test]
fn pack_parity_current_update_does_not_create_an_approval() {
    let (transport, project, source) = fresh(false, VERSION, &json!({"elicitation":{"form":{}}}));
    let proposal = transport.call(
        json!(10),
        "pack_add",
        &json!({"reference":"fixture/acme/settings"}),
    );
    approve(&transport, 11, &proposal);
    let mut pack: Value =
        serde_json::from_str(&std::fs::read_to_string(source.join("pack.yml")).unwrap()).unwrap();
    pack["version"] = json!("1.1.0");
    publish(&source, &[("new.yml", pack.to_string())]);
    let proposal = transport.call(json!(12), "pack_update", &json!({}));
    assert!(data(&proposal)["error"].is_null(), "{proposal}");
    let retry = transport.call(json!(13), "pack_update", &json!({}));
    assert_eq!(
        data(&retry)["data"]["proposal_id"],
        data(&proposal)["data"]["proposal_id"],
        "{retry}"
    );
    approve(&transport, 14, &proposal);
    let before = project.revision();
    let count = proposals(&project);
    let result = transport.call(json!(15), "pack_update", &json!({}));
    assert!(data(&result)["error"].is_null(), "{result}");
    assert_eq!(data(&result)["data"]["unchanged"], true, "{result}");
    assert!(data(&result)["data"].get("apply_arguments").is_none());
    assert_eq!(proposals(&project), count);
    assert_eq!(project.revision(), before);
}

#[test]
fn pack_parity_explicit_update_retry_returns_applied_receipt_without_acquisition() {
    let (transport, project, source) = fresh(false, VERSION, &json!({"elicitation":{"form":{}}}));
    let proposal = transport.call(
        json!(10),
        "pack_add",
        &json!({"reference":"fixture/acme/settings"}),
    );
    approve(&transport, 11, &proposal);
    let mut pack: Value =
        serde_json::from_str(&std::fs::read_to_string(source.join("pack.yml")).unwrap()).unwrap();
    pack["version"] = json!("1.1.0");
    publish(&source, &[("new.yml", pack.to_string())]);
    let args = json!({"idempotency_key":"update-once"});
    let proposal = transport.call(json!(12), "pack_update", &args);
    approve(&transport, 13, &proposal);
    let revision = project.revision();
    let count = proposals(&project);
    std::fs::remove_file(source.join("catalog.json")).unwrap();
    std::fs::remove_file(source.join("new.yml")).unwrap();
    let retry = transport.call(json!(14), "pack_update", &args);
    assert!(data(&retry)["error"].is_null(), "{retry}");
    assert_eq!(
        data(&retry)["data"]["proposal_id"],
        data(&proposal)["data"]["proposal_id"],
        "{retry}"
    );
    assert_eq!(data(&retry)["data"]["status"], "applied", "{retry}");
    assert_eq!(project.revision(), revision);
    assert_eq!(proposals(&project), count);
}

#[test]
fn pack_parity_bare_configuration_includes_rule_overrides() {
    let (transport, project) = prepared("claude", &json!({"elicitation":{"form":{}}}));
    let before = project.revision();
    let form = transport.call(
        json!(10),
        "pack_configure",
        &json!({"reference":"acme/settings"}),
    );
    assert_eq!(form["method"], "elicitation/create", "{form}");
    let mut values = values();
    let mut overridden = false;
    for (key, field) in form["params"]["requestedSchema"]["properties"]
        .as_object()
        .unwrap()
    {
        if field["title"]
            .as_str()
            .unwrap()
            .starts_with("Rule override:")
        {
            values[key] = json!(if overridden { "keep" } else { "off" });
            overridden = true;
        }
    }
    assert!(overridden);
    let proposal = transport.request(&answer(&form, "accept", values));
    assert!(data(&proposal)["error"].is_null(), "{proposal}");
    assert_eq!(project.revision(), before);
    approve(&transport, 11, &proposal);
    let config: Value = crate::policy::parse(
        &std::fs::read_to_string(
            std::path::Path::new(&project.review.project_root).join(".shield/config.yml"),
        )
        .unwrap(),
    )
    .unwrap();
    assert!(
        !config["packs"][0]["overrides"]
            .as_object()
            .unwrap()
            .is_empty()
    );
}
