//! Guided installation retains artifacts and user input across separate approval.
use super::*;

fn fresh(
    required: bool,
    version: &str,
    capabilities: &Value,
) -> (Transport, Project, std::path::PathBuf) {
    let (server, project) = fixture(Some("claude"));
    let source = std::path::Path::new(&project.review.project_root).join("registry");
    std::fs::create_dir(&source).unwrap();
    let source = source.canonicalize().unwrap();
    let mut pack: Value =
        serde_json::from_str(include_str!("../../../../fixtures/elicitation/pack.yml")).unwrap();
    if required {
        for declaration in pack["variables"].as_object_mut().unwrap().values_mut() {
            declaration["required"] = json!(true);
            declaration.as_object_mut().unwrap().remove("default");
        }
    }
    let bytes = pack.to_string();
    std::fs::write(source.join("pack.yml"), &bytes).unwrap();
    std::fs::write(source.join("catalog.json"), json!({"schema_version":1,"packs":[{"id":"acme/settings","version":"1.0.0","path":"pack.yml","sha256":crate::policy::fingerprint(bytes.as_bytes())}]}).to_string()).unwrap();
    crate::review::mcp::register_fixture(&project.review, &source);
    let transport = Transport::start(server);
    let mut messages = initialize(capabilities, "claude-code");
    messages[0]["params"]["protocolVersion"] = json!(version);
    for message in messages {
        transport.send(&message);
    }
    assert!(transport.receive()["result"].is_object());
    (transport, project, source)
}

fn form_values(form: &Value) -> Value {
    let mut result = json!({});
    for (key, field) in form["params"]["requestedSchema"]["properties"]
        .as_object()
        .unwrap()
    {
        let name = field["title"].as_str().unwrap().split(": ").last().unwrap();
        result[key] = values()[name].clone();
    }
    result
}

fn publish(source: &std::path::Path, documents: &[(&str, String)]) {
    let mut entries = Vec::new();
    for (path, bytes) in documents {
        let document: Value = serde_json::from_str(bytes).unwrap();
        std::fs::write(source.join(path), bytes).unwrap();
        entries.push(json!({"id":document["id"],"version":document["version"],"path":path,"sha256":crate::policy::fingerprint(bytes.as_bytes())}));
    }
    std::fs::write(
        source.join("catalog.json"),
        json!({"schema_version":1,"packs":entries}).to_string(),
    )
    .unwrap();
}

fn approve(transport: &Transport, id: i64, proposal: &Value) {
    assert!(data(proposal)["error"].is_null(), "{proposal}");
    let approval = transport.call(
        json!(id),
        "policy_apply",
        &data(proposal)["data"]["apply_arguments"],
    );
    assert_eq!(approval["method"], "elicitation/create", "{approval}");
    let applied = transport.request(&answer(&approval, "accept", json!({})));
    assert!(data(&applied)["error"].is_null(), "{applied}");
}

#[test]
fn pack_installation_batch_collects_optional_and_required_settings_and_activates_once() {
    let (transport, project, source) = fresh(false, VERSION, &json!({"elicitation":{"form":{}}}));
    let mut root: Value =
        serde_json::from_str(&std::fs::read_to_string(source.join("pack.yml")).unwrap()).unwrap();
    root["capabilities"]
        .as_array_mut()
        .unwrap()
        .push(json!("dependencies-v1"));
    root["dependencies"] = json!([{"pack":"acme/child","requirement":"^1"}]);
    let mut child = root.clone();
    child["id"] = json!("acme/child");
    child["dependencies"] = json!([]);
    child["variables"] =
        json!({"label":{"type":"string","description":"Required label","required":true}});
    publish(
        &source,
        &[
            ("pack.yml", root.to_string()),
            ("child.yml", child.to_string()),
        ],
    );
    let before = project.revision();
    let form = transport.call(json!(10), "pack_add", &json!({
        "packs":[{"reference":"fixture/acme/settings","variables":["manager"]},{"reference":"fixture/acme/child","profile":"strict","variables":["label"]}],
        "layer":"local"
    }));
    assert_eq!(form["method"], "elicitation/create", "{form}");
    assert_eq!(
        form["params"]["requestedSchema"]["properties"]
            .as_object()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(proposals(&project), 0);
    assert_eq!(project.revision(), before);
    // Neither the answer nor final activation may reacquire registry artifacts.
    std::fs::remove_file(source.join("catalog.json")).unwrap();
    std::fs::remove_file(source.join("pack.yml")).unwrap();
    std::fs::remove_file(source.join("child.yml")).unwrap();
    let proposal = transport.request(&answer(&form, "accept", form_values(&form)));
    assert!(data(&proposal)["error"].is_null(), "{proposal}");
    assert_eq!(proposals(&project), 1);
    assert_eq!(project.revision(), before);
    approve(&transport, 11, &proposal);
    assert_ne!(project.revision(), before);
    let folder = std::path::Path::new(&project.review.project_root).join(".shield");
    let local: Value =
        crate::policy::parse(&std::fs::read_to_string(folder.join("config.local.yml")).unwrap())
            .unwrap();
    let rows = local["packs"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    let root = rows
        .iter()
        .find(|row| row["id"] == "acme/settings")
        .unwrap();
    assert_eq!(root["variables"], json!({"manager":"just"}));
    let child = rows.iter().find(|row| row["id"] == "acme/child").unwrap();
    assert_eq!(child["profile"], "strict");
    assert_eq!(child["variables"], json!({"label":"Acme"}));
}

#[test]
fn pack_installation_batch_reduces_three_installations_to_one_publication() {
    for batch in [false, true] {
        let (transport, project, source) =
            fresh(false, VERSION, &json!({"elicitation":{"form":{}}}));
        let root: Value =
            serde_json::from_str(&std::fs::read_to_string(source.join("pack.yml")).unwrap())
                .unwrap();
        let documents: Vec<_> = ["first", "second", "third"]
            .into_iter()
            .map(|name| {
                let mut pack = root.clone();
                pack["id"] = json!(format!("acme/{name}"));
                (format!("{name}.yml"), pack.to_string())
            })
            .collect();
        publish(
            &source,
            &documents
                .iter()
                .map(|(path, bytes)| (path.as_str(), bytes.clone()))
                .collect::<Vec<_>>(),
        );
        let selections: Vec<_> = ["first", "second", "third"]
            .into_iter()
            .map(|name| json!({"reference":format!("fixture/acme/{name}"),"variables":["manager"]}))
            .collect();
        let calls = if batch {
            vec![json!({"packs":selections})]
        } else {
            selections
        };
        let mut elapsed = std::time::Duration::ZERO;
        for (index, arguments) in calls.iter().enumerate() {
            let started = std::time::Instant::now();
            let form = transport.call(json!(10 + index * 2), "pack_add", arguments);
            assert_eq!(form["method"], "elicitation/create", "{form}");
            let proposal = transport.request(&answer(&form, "accept", form_values(&form)));
            approve(
                &transport,
                i64::try_from(11 + index * 2).unwrap(),
                &proposal,
            );
            elapsed += started.elapsed();
        }
        assert_eq!(proposals(&project), if batch { 1 } else { 3 });
        let config: Value = crate::policy::parse(
            &std::fs::read_to_string(
                std::path::Path::new(&project.review.project_root).join(".shield/config.yml"),
            )
            .unwrap(),
        )
        .unwrap();
        let rows = config["packs"].as_array().unwrap();
        assert_eq!(rows.len(), 3);
        assert!(
            rows.iter()
                .all(|row| row["variables"] == json!({"manager":"just"}))
        );
        assert!(transport.close().1.is_ok());
        eprintln!(
            "pack installation retained MCP batch={batch} complete_installation_us={} settings_forms={} final_approvals={}",
            elapsed.as_micros(),
            calls.len(),
            calls.len()
        );
    }
}

#[test]
fn pack_installation_explicit_optional_setting_overrides_default_only_after_approval() {
    let (transport, project, _) = fresh(false, VERSION, &json!({"elicitation":{"form":{}}}));
    let before = project.revision();
    let form = transport.call(
        json!(10),
        "pack_add",
        &json!({"reference":"fixture/acme/settings","variables":["manager"]}),
    );
    assert_eq!(form["method"], "elicitation/create", "{form}");
    assert_eq!(
        form["params"]["requestedSchema"]["properties"]
            .as_object()
            .unwrap()
            .len(),
        1
    );
    let proposal = transport.request(&answer(&form, "accept", form_values(&form)));
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
    assert_eq!(config["packs"][0]["variables"], json!({"manager":"just"}));
}

#[test]
fn pack_installation_optional_repository_setting_prepares_scope_before_final_approval() {
    let (transport, project, source) = fresh(false, VERSION, &json!({"elicitation":{"form":{}}}));
    let mut pack: Value =
        serde_json::from_str(&std::fs::read_to_string(source.join("pack.yml")).unwrap()).unwrap();
    pack["variables"] = json!({"repository":{"type":"string","description":"Optional exact owner/repository; omit for no restriction","match":{"regex":"[a-z]+/[a-z]+"}}});
    pack["capabilities"]
        .as_array_mut()
        .unwrap()
        .push(json!("preconditions-v1"));
    pack["capabilities"]
        .as_array_mut()
        .unwrap()
        .push(json!("regex-v1"));
    pack["rules"] = json!([{"id":"inspect","conditions":{"domain":"argv","executable":"gh","arguments":{"kind":"any"}},"decision":"allow"}]);
    pack["preconditions"] = json!([{"id":"repository","conditions":{"domain":"argv","executable":"gh"},
        "requires_any":[{"arguments":{"kind":"exact","values":["repo","view","-R",{"var":"repository"}]}}],"on_unmet":{"decision":"deny"}}]);
    publish(&source, &[("pack.yml", pack.to_string())]);
    let before = project.revision();
    let unscoped = transport.call(
        json!(10),
        "pack_add",
        &json!({"reference":"fixture/acme/settings"}),
    );
    assert!(data(&unscoped)["error"].is_null(), "{unscoped}");
    let form = transport.call(
        json!(11),
        "pack_add",
        &json!({"reference":"fixture/acme/settings","variables":["repository"]}),
    );
    assert_eq!(form["method"], "elicitation/create", "{form}");
    let key = form["params"]["requestedSchema"]["properties"]
        .as_object()
        .unwrap()
        .keys()
        .next()
        .unwrap();
    let mut values = json!({});
    values[key] = json!("acme/web");
    let scoped = transport.request(&answer(&form, "accept", values));
    assert!(data(&scoped)["error"].is_null(), "{scoped}");
    assert_eq!(project.revision(), before);
    for (id, proposal, repository, expected) in [
        (12, &unscoped, "outside/web", "allow"),
        (13, &scoped, "acme/web", "allow"),
        (14, &scoped, "outside/web", "deny"),
    ] {
        let result = transport.call(json!(id), "policy_evaluate", &json!({
            "candidate_id":data(proposal)["data"]["after_candidate_id"],
            "input":{"kind":"argv","executable":"gh","arguments":["repo","view","-R",repository],"original_input":{}},
            "context":{"cwd":project.review.project_root}
        }));
        assert_eq!(data(&result)["data"]["decision"], expected, "{result}");
    }
    assert_eq!(project.revision(), before);
    approve(&transport, 15, &scoped);
    assert_ne!(project.revision(), before);
}

#[test]
fn pack_installation_batch_refuses_invalid_selections_without_partial_proposals() {
    for arguments in [
        json!({"packs":[]}),
        json!({"packs":vec![json!({"reference":"fixture/acme/settings"});17]}),
        json!({"packs":[{"reference":"fixture/acme/settings"}],"reference":"fixture/acme/settings"}),
        json!({"packs":[{"reference":"fixture/acme/settings"}],"profile":"strict"}),
        json!({"packs":[{"reference":"fixture/acme/settings"},{"reference":"fixture/acme/settings"}]}),
        json!({"packs":[{"reference":"fixture/acme/settings"},{"reference":"fixture/acme/missing"}]}),
        json!({"packs":[{"reference":"fixture/acme/settings","variables":["missing"]}]}),
        json!({"reference":"fixture/acme/settings","variables":["manager","manager"]}),
    ] {
        let (transport, project, _) = fresh(false, VERSION, &json!({"elicitation":{"form":{}}}));
        let before = project.revision();
        let result = transport.call(json!(10), "pack_add", &arguments);
        assert_eq!(result["id"], 10, "{arguments}: {result}");
        assert!(
            result.get("error").is_some() || !data(&result)["error"].is_null(),
            "{result}"
        );
        assert_eq!(proposals(&project), 0);
        assert_eq!(project.revision(), before);
    }
}

#[test]
fn pack_installation_batch_defaults_need_no_form_and_commit_one_generation() {
    let (transport, project, source) = fresh(false, VERSION, &json!({"elicitation":{"form":{}}}));
    let root = std::fs::read_to_string(source.join("pack.yml")).unwrap();
    let mut other: Value = serde_json::from_str(&root).unwrap();
    other["id"] = json!("acme/other");
    publish(
        &source,
        &[("pack.yml", root), ("other.yml", other.to_string())],
    );
    let before = project.revision();
    let result = transport.call(
        json!(10),
        "pack_add",
        &json!({"packs":[
            {"reference":"fixture/acme/settings"},{"reference":"fixture/acme/other"}
        ]}),
    );
    assert!(data(&result)["error"].is_null(), "{result}");
    assert_eq!(proposals(&project), 1);
    assert_eq!(project.revision(), before);
    approve(&transport, 11, &result);
    let applied = project.revision();
    assert_ne!(applied, before);
    // Reapplying returns the same receipt without another activation.
    let repeated = transport.call(
        json!(12),
        "policy_apply",
        &data(&result)["data"]["apply_arguments"],
    );
    assert!(data(&repeated)["error"].is_null(), "{repeated}");
    assert_eq!(project.revision(), applied);
    let listed = transport.call(json!(13), "pack_list", &json!({}));
    assert_eq!(data(&listed)["data"]["packs"].as_array().unwrap().len(), 2);
}

#[test]
fn pack_installation_batch_decline_cancel_and_stale_input_leave_no_pack_proposal() {
    for outcome in ["decline", "cancel", "stale"] {
        let (transport, project, source) =
            fresh(false, VERSION, &json!({"elicitation":{"form":{}}}));
        let root = std::fs::read_to_string(source.join("pack.yml")).unwrap();
        let mut other: Value = serde_json::from_str(&root).unwrap();
        other["id"] = json!("acme/other");
        publish(
            &source,
            &[("pack.yml", root), ("other.yml", other.to_string())],
        );
        let form = transport.call(
            json!(10),
            "pack_add",
            &json!({"packs":[
                {"reference":"fixture/acme/settings","variables":["manager"]},
                {"reference":"fixture/acme/other","variables":["manager"]}
            ]}),
        );
        assert_eq!(form["method"], "elicitation/create", "{form}");
        assert_eq!(
            form["params"]["requestedSchema"]["properties"]
                .as_object()
                .unwrap()
                .len(),
            2
        );
        if outcome == "stale" {
            let other = transport.call(json!(11), "policy_propose", &json!({"change":{"kind":"rule_add","rule":{"id":"other","decision":"ask","conditions":{"domain":"argv","executable":"other","arguments":{"kind":"any"}}}}}));
            // A separate operator/connection changes policy while this form owns
            // the connection's pending input slot.
            let applied = crate::review::mcp::call_with_confirmation(
                &project.review,
                &crate::review::mcp::Connection::new("other".into()),
                "policy_apply",
                &data(&other)["data"]["apply_arguments"],
                &|| Ok(()),
                true,
            );
            assert!(applied["error"].is_null(), "{applied}");
        }
        let before = project.revision();
        let count = proposals(&project);
        let (action, values, code) = match outcome {
            "stale" => ("accept", form_values(&form), "stale_revision"),
            "decline" => ("decline", json!({}), "input_declined"),
            _ => ("cancel", json!({}), "input_cancelled"),
        };
        let result = transport.request(&answer(&form, action, values));
        assert_eq!(data(&result)["error"]["code"], code, "{result}");
        assert_eq!(project.revision(), before);
        assert_eq!(proposals(&project), count);
        let listed = transport.call(json!(13), "pack_list", &json!({}));
        assert!(
            data(&listed)["data"]["packs"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }
}

#[test]
fn pack_installation_batch_bounds_requested_settings_across_all_selections() {
    for fields in [8, 9] {
        let (transport, project, source) =
            fresh(false, VERSION, &json!({"elicitation":{"form":{}}}));
        let mut root: Value =
            serde_json::from_str(&std::fs::read_to_string(source.join("pack.yml")).unwrap())
                .unwrap();
        let names: Vec<_> = (0..fields).map(|i| format!("flag_{i}")).collect();
        root["variables"] = json!(
            names
                .iter()
                .map(|name| (
                    name,
                    json!({"type":"boolean","description":"Optional flag","default":false})
                ))
                .collect::<std::collections::BTreeMap<_, _>>()
        );
        let mut other = root.clone();
        other["id"] = json!("acme/other");
        publish(
            &source,
            &[
                ("pack.yml", root.to_string()),
                ("other.yml", other.to_string()),
            ],
        );
        let result = transport.call(
            json!(10),
            "pack_add",
            &json!({"packs":[
                {"reference":"fixture/acme/settings","variables":names},
                {"reference":"fixture/acme/other","variables":names}
            ]}),
        );
        if fields == 8 {
            assert_eq!(result["method"], "elicitation/create", "{result}");
            assert_eq!(
                result["params"]["requestedSchema"]["properties"]
                    .as_object()
                    .unwrap()
                    .len(),
                16
            );
            let declined = transport.request(&answer(&result, "decline", json!({})));
            assert_eq!(data(&declined)["error"]["code"], "input_declined");
        } else {
            assert_eq!(data(&result)["error"]["code"], "invalid_input", "{result}");
        }
        assert_eq!(proposals(&project), 0);
    }
}

#[test]
fn pack_installation_allows_sixteen_missing_settings_but_refuses_seventeen_without_proposal() {
    for count in [16, 17] {
        let (transport, project, source) =
            fresh(true, VERSION, &json!({"elicitation":{"form":{}}}));
        let mut pack: Value =
            serde_json::from_str(&std::fs::read_to_string(source.join("pack.yml")).unwrap())
                .unwrap();
        pack["variables"] = json!(
            (0..count)
                .map(|i| (
                    format!("value_{i}"),
                    json!({"type":"boolean","description":"Required flag","required":true})
                ))
                .collect::<std::collections::BTreeMap<_, _>>()
        );
        publish(&source, &[("pack.yml", pack.to_string())]);
        let before = project.revision();
        let proposals_before = proposals(&project);
        let result = transport.call(
            json!(10),
            "pack_add",
            &json!({"reference":"fixture/acme/settings"}),
        );
        if count == 16 {
            assert_eq!(result["method"], "elicitation/create", "{result}");
            assert_eq!(
                result["params"]["requestedSchema"]["properties"]
                    .as_object()
                    .unwrap()
                    .len(),
                count
            );
            let declined = transport.request(&answer(&result, "decline", json!({})));
            assert_eq!(data(&declined)["error"]["code"], "input_declined");
        } else {
            assert_eq!(result["id"], 10, "{result}");
            assert_eq!(data(&result)["error"]["code"], "invalid_input", "{result}");
            assert!(
                data(&result)["error"]["message"]
                    .as_str()
                    .unwrap()
                    .contains("more than 16 settings")
            );
        }
        assert_eq!(project.revision(), before);
        assert_eq!(proposals(&project), proposals_before);
    }
}

#[test]
fn pack_installation_refuses_more_than_four_mib_of_retained_source_without_proposal() {
    let (transport, project, source) = fresh(true, VERSION, &json!({"elicitation":{"form":{}}}));
    let mut bytes = std::fs::read_to_string(source.join("pack.yml")).unwrap();
    bytes.push_str(&" ".repeat(4 * 1024 * 1024));
    publish(&source, &[("pack.yml", bytes)]);
    let before = project.revision();
    let proposals_before = proposals(&project);
    let result = transport.call(
        json!(10),
        "pack_add",
        &json!({"reference":"fixture/acme/settings"}),
    );
    assert_eq!(result["id"], 10, "{result}");
    assert_eq!(data(&result)["error"]["code"], "invalid_input", "{result}");
    assert!(
        data(&result)["error"]["message"]
            .as_str()
            .unwrap()
            .contains("at most 4 MiB of source")
    );
    assert_eq!(project.revision(), before);
    assert_eq!(proposals(&project), proposals_before);
}

#[test]
fn pack_installation_local_batch_refuses_profiles_for_preexisting_packs() {
    let (transport, project, source) = fresh(false, VERSION, &json!({"elicitation":{"form":{}}}));
    let root: Value =
        serde_json::from_str(&std::fs::read_to_string(source.join("pack.yml")).unwrap()).unwrap();
    let mut child = root.clone();
    child["id"] = json!("acme/child");
    publish(
        &source,
        &[
            ("pack.yml", root.to_string()),
            ("child.yml", child.to_string()),
        ],
    );
    let proposal = transport.call(
        json!(10),
        "policy_propose",
        &json!({
            "change":{"kind":"pack_add","reference":"fixture/acme/settings"}
        }),
    );
    approve(&transport, 11, &proposal);
    let folder = std::path::Path::new(&project.review.project_root).join(".shield");
    let shared_before = std::fs::read(folder.join("config.yml")).unwrap();
    let revision = project.revision();
    let count = proposals(&project);
    for profile in ["recommended", "strict"] {
        for existing_first in [true, false] {
            let existing = json!({"reference":"fixture/acme/settings","profile":profile});
            let new = json!({"reference":"fixture/acme/child"});
            let packs = if existing_first {
                json!([existing, new])
            } else {
                json!([new, existing])
            };
            let refused = transport.call(
                json!(12),
                "pack_add",
                &json!({"packs":packs,"layer":"local"}),
            );
            assert_eq!(
                data(&refused)["error"]["code"],
                "invalid_input",
                "{refused}"
            );
            assert!(
                data(&refused)["error"]["message"]
                    .as_str()
                    .unwrap()
                    .contains("cannot change its profile"),
                "{refused}"
            );
            assert_eq!(project.revision(), revision);
            assert_eq!(proposals(&project), count);
            assert_eq!(
                std::fs::read(folder.join("config.yml")).unwrap(),
                shared_before
            );
        }
    }
    // Without a profile, an existing selection can still receive personal values.
    let form = transport.call(
        json!(13),
        "pack_add",
        &json!({
            "packs":[{"reference":"fixture/acme/settings","variables":["manager"]},
                {"reference":"fixture/acme/child"}],"layer":"local"
        }),
    );
    assert_eq!(form["method"], "elicitation/create", "{form}");
    let proposal = transport.request(&answer(&form, "accept", form_values(&form)));
    approve(&transport, 14, &proposal);
    assert_eq!(
        std::fs::read(folder.join("config.yml")).unwrap(),
        shared_before
    );
    let local: Value =
        crate::policy::parse(&std::fs::read_to_string(folder.join("config.local.yml")).unwrap())
            .unwrap();
    let overlay = local["packs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == "acme/settings")
        .unwrap();
    assert_eq!(overlay["variables"], json!({"manager":"just"}));
    assert!(overlay.get("profile").is_none());
}

#[test]
fn pack_installation_local_settings_for_an_existing_shared_dependency_use_its_overlay() {
    let (transport, project, source) = fresh(true, VERSION, &json!({"elicitation":{"form":{}}}));
    let mut root: Value =
        serde_json::from_str(&std::fs::read_to_string(source.join("pack.yml")).unwrap()).unwrap();
    root["capabilities"]
        .as_array_mut()
        .unwrap()
        .push(json!("dependencies-v1"));
    root["dependencies"] = json!([{"pack":"acme/child","requirement":"^1.1"}]);
    let mut shared = root.clone();
    shared["id"] = json!("acme/shared");
    shared["variables"] = json!({});
    shared["capabilities"] = json!([
        "structured-rules-v1",
        "argument-rules-v1",
        "dependencies-v1"
    ]);
    shared["dependencies"] = json!([{"pack":"acme/child","requirement":"^1"}]);
    let mut child = root.clone();
    child["id"] = json!("acme/child");
    child["dependencies"] = json!([]);
    child["variables"] =
        json!({"enabled":{"type":"boolean","description":"Enable checks","default":false}});
    publish(
        &source,
        &[
            ("shared.yml", shared.to_string()),
            ("child.yml", child.to_string()),
        ],
    );
    let proposal = transport.call(
        json!(10),
        "policy_propose",
        &json!({"change":{"kind":"pack_add","reference":"fixture/acme/shared"}}),
    );
    approve(&transport, 11, &proposal);
    let proposal = transport.call(json!(12), "policy_propose", &json!({"change":{"kind":"pack_configure","reference":"acme/child","set":{"enabled":true}}}));
    approve(&transport, 13, &proposal);
    // The new local root needs a newer transitive release. Its additional required
    // setting belongs to the existing shared selection's local overlay.
    let old_child = child.to_string();
    child["version"] = json!("1.1.0");
    child["variables"]["manager"] = json!({"type":"choice","description":"Build manager","values":["make","just"],"required":true});
    publish(
        &source,
        &[
            ("pack.yml", root.to_string()),
            ("shared.yml", shared.to_string()),
            ("child.yml", old_child),
            ("child-new.yml", child.to_string()),
        ],
    );
    let before = project.revision();
    let form = transport.call(
        json!(14),
        "pack_add",
        &json!({"reference":"fixture/acme/settings","layer":"local"}),
    );
    assert_eq!(form["method"], "elicitation/create", "{form}");
    assert_eq!(
        form["params"]["requestedSchema"]["properties"]
            .as_object()
            .unwrap()
            .len(),
        5
    );
    let proposal = transport.request(&answer(&form, "accept", form_values(&form)));
    assert!(data(&proposal)["error"].is_null(), "{proposal}");
    assert_eq!(project.revision(), before);
    approve(&transport, 15, &proposal);
    let folder = std::path::Path::new(&project.review.project_root).join(".shield");
    let shared: Value =
        crate::policy::parse(&std::fs::read_to_string(folder.join("config.yml")).unwrap()).unwrap();
    let shared_rows = shared["packs"].as_array().unwrap();
    assert!(!shared_rows.iter().any(|row| row["id"] == "acme/settings"));
    let child = shared_rows
        .iter()
        .find(|row| row["id"] == "acme/child")
        .unwrap();
    assert_eq!(child["version"], "1.1.0");
    assert_eq!(child["variables"], json!({"enabled":true}));
    let local: Value =
        crate::policy::parse(&std::fs::read_to_string(folder.join("config.local.yml")).unwrap())
            .unwrap();
    let local_rows = local["packs"].as_array().unwrap();
    let overlay = local_rows
        .iter()
        .find(|row| row["id"] == "acme/child")
        .unwrap();
    assert_eq!(overlay["variables"], json!({"manager":"just"}));
    for field in ["version", "profile", "path"] {
        assert!(overlay.get(field).is_none(), "{overlay}");
    }
    assert_eq!(
        local_rows
            .iter()
            .find(|row| row["id"] == "acme/settings")
            .unwrap()["variables"],
        values()
    );
}

#[test]
fn pack_installation_retains_verified_artifacts_and_requires_separate_approval() {
    let (transport, project, source) = fresh(true, VERSION, &json!({"elicitation":{"form":{}}}));
    let before = project.revision();
    let count = proposals(&project);
    let form = transport.call(
        json!(10),
        "pack_add",
        &json!({"reference":"fixture/acme/settings","profile":"strict","layer":"local"}),
    );
    assert_eq!(form["method"], "elicitation/create", "{form}");
    assert_eq!(
        form["params"]["requestedSchema"]["properties"]
            .as_object()
            .unwrap()
            .len(),
        4
    );
    assert_eq!(proposals(&project), count);
    // Acquisition must not happen again after the user answers.
    std::fs::remove_file(source.join("pack.yml")).unwrap();
    std::fs::remove_file(source.join("catalog.json")).unwrap();
    let proposal = transport.request(&answer(&form, "accept", form_values(&form)));
    assert!(data(&proposal)["error"].is_null(), "{proposal}");
    assert_eq!(project.revision(), before);
    assert_eq!(proposals(&project), count + 1);
    let approval = transport.call(
        json!(11),
        "policy_apply",
        &data(&proposal)["data"]["apply_arguments"],
    );
    assert_eq!(approval["method"], "elicitation/create", "{approval}");
    assert_eq!(project.revision(), before);
    let applied = transport.request(&answer(&approval, "accept", json!({})));
    assert!(data(&applied)["error"].is_null(), "{applied}");
    let local = std::fs::read_to_string(
        std::path::Path::new(&project.review.project_root).join(".shield/config.local.yml"),
    )
    .unwrap();
    assert!(local.contains("Acme") && local.contains("just"));
    let config: Value = crate::policy::parse(&local).unwrap();
    assert_eq!(config["packs"][0]["profile"], "strict");
}

#[test]
fn pack_installation_decline_dismiss_invalid_missing_support_and_legacy_refuse_without_proposal() {
    for (action, expected) in [
        ("decline", "input_declined"),
        ("cancel", "input_cancelled"),
        ("accept", "invalid_input"),
    ] {
        let (transport, project, _) = fresh(true, VERSION, &json!({"elicitation":{"form":{}}}));
        let before = project.revision();
        let count = proposals(&project);
        let form = transport.call(
            json!(10),
            "pack_add",
            &json!({"reference":"fixture/acme/settings"}),
        );
        let result = transport.request(&answer(&form, action, json!({})));
        assert_eq!(data(&result)["error"]["code"], expected, "{result}");
        assert_eq!(project.revision(), before);
        assert_eq!(proposals(&project), count);
    }
    for (version, capabilities) in [
        (VERSION, json!({})),
        ("2025-06-18", json!({"elicitation":{}})),
    ] {
        let (transport, project, _) = fresh(true, version, &capabilities);
        let count = proposals(&project);
        let result = transport.call(
            json!(10),
            "pack_add",
            &json!({"reference":"fixture/acme/settings"}),
        );
        assert_eq!(
            data(&result)["error"]["code"],
            "input_unavailable",
            "{result}"
        );
        assert_eq!(proposals(&project), count);
    }
}

#[test]
fn pack_installation_with_defaults_prepares_without_form_and_rejects_private_candidate_arguments() {
    let (transport, project, _) = fresh(false, VERSION, &json!({}));
    let before = project.revision();
    let result = transport.call(
        json!(10),
        "pack_add",
        &json!({"reference":"fixture/acme/settings"}),
    );
    assert!(data(&result)["error"].is_null(), "{result}");
    assert_eq!(project.revision(), before);
    let count = proposals(&project);
    let forged = transport.call(json!(11), "policy_propose", &json!({"change":{"kind":"pack_add","reference":"fixture/acme/settings"},"prepared":{"candidate":{},"metadata":{},"binding":"forged"}}));
    assert!(!data(&forged)["error"].is_null(), "{forged}");
    assert_eq!(proposals(&project), count);
}

#[test]
fn pack_installation_stale_policy_routes_to_fresh_user_input() {
    let (transport, project, _) = fresh(true, VERSION, &json!({"elicitation":{"form":{}}}));
    let form = transport.call(
        json!(10),
        "pack_add",
        &json!({"reference":"fixture/acme/settings"}),
    );
    let proposal = transport.call(json!(11), "policy_propose", &json!({"change":{"kind":"rule_add","rule":{"id":"other","decision":"ask","conditions":{"domain":"argv","executable":"other","arguments":{"kind":"any"}}}}}));
    let approved = crate::review::mcp::call_with_confirmation(
        &project.review,
        &crate::review::mcp::Connection::new("other".into()),
        "policy_apply",
        &data(&proposal)["data"]["apply_arguments"],
        &|| Ok(()),
        true,
    );
    assert!(approved["error"].is_null());
    let count = proposals(&project);
    let result = transport.request(&answer(&form, "accept", form_values(&form)));
    assert_eq!(data(&result)["error"]["code"], "stale_revision", "{result}");
    assert_eq!(data(&result)["error"]["operator_route"], "pack_add");
    assert_eq!(proposals(&project), count);
}

#[test]
fn pack_installation_rejects_registry_context_drift_without_a_policy_revision_change() {
    let (transport, project, source) = fresh(true, VERSION, &json!({"elicitation":{"form":{}}}));
    let form = transport.call(
        json!(10),
        "pack_add",
        &json!({"reference":"fixture/acme/settings"}),
    );
    assert_eq!(form["method"], "elicitation/create", "{form}");
    let before = project.revision();
    let replacement = source.with_file_name("other-registry");
    std::fs::create_dir(&replacement).unwrap();
    crate::review::mcp::register_fixture(&project.review, &replacement);
    assert_eq!(project.revision(), before);
    let result = transport.request(&answer(&form, "accept", form_values(&form)));
    assert_eq!(data(&result)["error"]["code"], "stale_revision", "{result}");
    assert_eq!(data(&result)["error"]["operator_route"], "pack_add");
    assert_eq!(proposals(&project), 0);
}

#[test]
fn pack_installation_modern_partial_replay_and_cross_tool_binding() {
    let (transport, project, _) = fresh(true, VERSION, &json!({}));
    let arguments = json!({"reference":"fixture/acme/settings"});
    let call = |id, state: Option<Value>, responses: Option<Value>| {
        let mut request = modern(id, arguments.clone(), state, responses);
        request["params"]["name"] = json!("pack_add");
        request
    };
    let first = transport.request(&call(10, None, None));
    assert_eq!(first["result"]["resultType"], "input_required", "{first}");
    let state = first["result"]["requestState"].clone();
    let partial = transport.request(&call(11, Some(state.clone()), Some(json!({}))));
    assert_eq!(partial["result"]["resultType"], "input_required");
    let form = json!({"params":{"requestedSchema":first["result"]["inputRequests"]["settings"]["params"]["requestedSchema"]}});
    let answers = json!({"settings":{"action":"accept","content":form_values(&form)}});
    let accepted = transport.request(&call(12, Some(state.clone()), Some(answers.clone())));
    assert_eq!(accepted["result"]["resultType"], "complete", "{accepted}");
    assert_eq!(proposals(&project), 1);
    let replay = transport.request(&call(13, Some(state), Some(answers)));
    assert_eq!(replay["error"]["code"], -32602);
    let first = transport.request(&call(14, None, None));
    let mut wrong = call(15, Some(first["result"]["requestState"].clone()), None);
    wrong["params"]["name"] = json!("pack_configure");
    wrong["params"]["arguments"] = args();
    assert_eq!(transport.request(&wrong)["error"]["code"], -32602);
}

#[test]
fn pack_installation_collects_dependency_settings_without_name_collisions() {
    let (transport, project, source) = fresh(true, VERSION, &json!({"elicitation":{"form":{}}}));
    let mut parent: Value =
        serde_json::from_str(&std::fs::read_to_string(source.join("pack.yml")).unwrap()).unwrap();
    parent["capabilities"]
        .as_array_mut()
        .unwrap()
        .push(json!("dependencies-v1"));
    parent["dependencies"] = json!([{"pack":"acme/child","requirement":"^1"}]);
    let mut child = parent.clone();
    child["id"] = json!("acme/child");
    child["dependencies"] = json!([]);
    child["variables"] = json!({"manager":{"type":"choice","description":"Child manager","values":["make","just"],"required":true}});
    let parent = parent.to_string();
    let child = child.to_string();
    std::fs::write(source.join("pack.yml"), &parent).unwrap();
    std::fs::write(source.join("child.yml"), &child).unwrap();
    std::fs::write(source.join("catalog.json"), json!({"schema_version":1,"packs":[
        {"id":"acme/settings","version":"1.0.0","path":"pack.yml","sha256":crate::policy::fingerprint(parent.as_bytes())},
        {"id":"acme/child","version":"1.0.0","path":"child.yml","sha256":crate::policy::fingerprint(child.as_bytes())}
    ]}).to_string()).unwrap();
    let form = transport.call(
        json!(10),
        "pack_add",
        &json!({"reference":"fixture/acme/settings"}),
    );
    assert_eq!(form["method"], "elicitation/create", "{form}");
    assert_eq!(
        form["params"]["requestedSchema"]["properties"]
            .as_object()
            .unwrap()
            .len(),
        5
    );
    let proposal = transport.request(&answer(&form, "accept", form_values(&form)));
    assert!(data(&proposal)["error"].is_null(), "{proposal}");
    let approval = transport.call(
        json!(11),
        "policy_apply",
        &data(&proposal)["data"]["apply_arguments"],
    );
    let applied = transport.request(&answer(&approval, "accept", json!({})));
    assert!(data(&applied)["error"].is_null(), "{applied}");
    let config = std::fs::read_to_string(
        std::path::Path::new(&project.review.project_root).join(".shield/config.yml"),
    )
    .unwrap();
    let config: Value = crate::policy::parse(&config).unwrap();
    let packs = config["packs"].as_array().unwrap();
    assert_eq!(packs.len(), 2);
    for pack in packs {
        assert_eq!(pack["variables"]["manager"], "just");
    }
}

#[test]
fn pack_installation_revalidates_values_and_rejects_unsupported_settings() {
    let (transport, project, source) = fresh(true, VERSION, &json!({"elicitation":{"form":{}}}));
    let form = transport.call(
        json!(10),
        "pack_add",
        &json!({"reference":"fixture/acme/settings"}),
    );
    let mut values = form_values(&form);
    for (key, field) in form["params"]["requestedSchema"]["properties"]
        .as_object()
        .unwrap()
    {
        if field["title"].as_str().unwrap().ends_with(": label") {
            values[key] = json!("x".repeat(21));
        }
    }
    let rejected = transport.request(&answer(&form, "accept", values));
    assert!(!data(&rejected)["error"].is_null(), "{rejected}");
    assert_eq!(proposals(&project), 0);
    let mut pack: Value =
        serde_json::from_str(&std::fs::read_to_string(source.join("pack.yml")).unwrap()).unwrap();
    pack["variables"]["regions"]["items"] = json!({"type":"string"});
    let bytes = pack.to_string();
    std::fs::write(source.join("pack.yml"), &bytes).unwrap();
    std::fs::write(source.join("catalog.json"), json!({"schema_version":1,"packs":[{"id":"acme/settings","version":"1.0.0","path":"pack.yml","sha256":crate::policy::fingerprint(bytes.as_bytes())}]}).to_string()).unwrap();
    let rejected = transport.call(
        json!(11),
        "pack_add",
        &json!({"reference":"fixture/acme/settings"}),
    );
    assert!(!data(&rejected)["error"].is_null(), "{rejected}");
    assert_eq!(proposals(&project), 0);
}
