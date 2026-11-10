//! Exercises local-layer merging, splitting and the effective selection binding.

use super::*;
use crate::configuration::{Candidate, Evaluator};
use crate::policy::{Decision, Request, Scope};

fn shared() -> Value {
    json!({"schema_version":1,"packs":[
        {"id":"acme/aws","registry_identity":"acme","version":"1.0.0","profile":"recommended","path":"cache/aws.yml","variables":{"region":"eu"},"overrides":{"list":"ask"}},
        {"id":"acme/git","registry_identity":"acme","version":"1.0.0","profile":"strict","path":"cache/git.yml"}
    ],"rules":"rules.yml","access_paths":{"read":["/srv/acme"]}})
}

fn local() -> Value {
    json!({"schema_version":1,"packs":[
        {"id":"acme/aws","variables":{"profile":"acme-read-only"},"unset":["region"],"clear_overrides":["list"]},
        {"id":"acme/git","disabled":true},
        {"id":"acme/personal","registry_identity":"acme","version":"2.0.0","profile":"loose","path":"cache/personal.yml","variables":{"editor":"vim"}}
    ],"rules":"rules.local.yml","access_paths":{"write":["/srv/scratch"]}})
}

#[test]
fn without_a_local_file_the_merged_document_is_the_shared_one() {
    let source = shared().to_string();
    assert_eq!(merge(&source, None).unwrap(), source);
    let (back, local) = split(&source, (Some(&source), None)).unwrap();
    assert_eq!(serde_json::from_str::<Value>(&back).unwrap(), shared());
    assert!(local.is_none());
}

#[test]
fn merging_and_splitting_preserve_each_layer() {
    let merged: Value =
        serde_json::from_str(&merge(&shared().to_string(), Some(&local().to_string())).unwrap())
            .unwrap();
    let packs = merged["packs"].as_array().unwrap();
    assert_eq!(packs.len(), 2);
    assert_eq!(packs[0]["local"]["variables"]["profile"], "acme-read-only");
    assert_eq!(packs[0]["local"]["unset"], json!(["region"]));
    assert_eq!(packs[1]["id"], "acme/personal");
    assert_eq!(packs[1]["layer"], "local");
    assert_eq!(merged["disabled"][0]["id"], "acme/git");
    assert_eq!(merged["local_rules"], "rules.local.yml");
    assert_eq!(
        merged["local_access_paths"]["write"],
        json!(["/srv/scratch"])
    );
    let (shared_source, local_source) = (shared().to_string(), local().to_string());
    let (back, again) = split(
        &merged.to_string(),
        (Some(&shared_source), Some(&local_source)),
    )
    .unwrap();
    assert_eq!(serde_json::from_str::<Value>(&back).unwrap(), shared());
    // The local file comes back as written: its order, and no added identity.
    assert_eq!(
        serde_json::from_str::<Value>(&again.unwrap()).unwrap(),
        local()
    );
}

#[test]
fn a_disabled_selection_keeps_its_place_in_the_shared_list() {
    let mut three = shared();
    three["packs"].as_array_mut().unwrap().push(
        json!({"id":"acme/node","registry_identity":"acme","version":"1.0.0","profile":"recommended","path":"cache/node.yml"}),
    );
    let local = json!({"schema_version":1,"packs":[{"id":"acme/git","disabled":true}]});
    let (shared_source, local_source) = (three.to_string(), local.to_string());
    let merged = merge(&shared_source, Some(&local_source)).unwrap();
    let (back, again) = split(&merged, (Some(&shared_source), Some(&local_source))).unwrap();
    assert_eq!(serde_json::from_str::<Value>(&back).unwrap(), three);
    assert_eq!(
        serde_json::from_str::<Value>(&again.unwrap()).unwrap(),
        local
    );
    // Without the previous files, the entry names its registry and the
    // selection is listed last, as a new one would be.
    let (back, again) = split(&merged, (None, None)).unwrap();
    let back: Value = serde_json::from_str(&back).unwrap();
    assert_eq!(back["packs"][2]["id"], "acme/git");
    let again: Value = serde_json::from_str(&again.unwrap()).unwrap();
    assert_eq!(again["packs"][0]["registry_identity"], "acme");
}

#[test]
fn a_complete_local_selection_replaces_a_shared_release_and_roundtrips_as_one_entry() {
    let local = json!({"schema_version":1,"packs":[
        {"id":"acme/aws","version":"2.0.0","profile":"strict","path":"cache/aws-local.yml","variables":{"region":"us"}}
    ]});
    let (shared_source, local_source) = (shared().to_string(), local.to_string());
    let merged = merge(&shared_source, Some(&local_source)).unwrap();
    let effective: Value = policy::parse(&merged).unwrap();
    assert_eq!(effective["packs"][0]["id"], "acme/git");
    assert_eq!(effective["packs"][1]["registry_identity"], "acme");
    assert_eq!(effective["packs"][1]["version"], "2.0.0");
    assert_eq!(effective["packs"][1]["layer"], "local");
    assert!(effective["packs"][1]["overrides"].is_null());
    assert_eq!(effective["disabled"][0], shared()["packs"][0]);
    Candidate::from_yaml(&merged).unwrap();
    let (back, again) = split(&merged, (Some(&shared_source), Some(&local_source))).unwrap();
    assert_eq!(policy::parse::<Value>(&back).unwrap(), shared());
    assert_eq!(policy::parse::<Value>(&again.unwrap()).unwrap(), local);
}

#[test]
fn replacement_identity_admission_still_refuses_duplicate_active_and_disabled_rows() {
    let mut effective = json!({"schema_version":1,"packs":[shared()["packs"][0].clone()],"disabled":[shared()["packs"][0].clone()]});
    assert!(Candidate::from_yaml(&effective.to_string()).is_err());
    effective["packs"][0]["layer"] = json!("local");
    Candidate::from_yaml(&effective.to_string()).unwrap();
    effective["disabled"]
        .as_array_mut()
        .unwrap()
        .push(shared()["packs"][0].clone());
    assert!(Candidate::from_yaml(&effective.to_string()).is_err());
    effective["disabled"].as_array_mut().unwrap().pop();
    let duplicate = effective["packs"][0].clone();
    effective["packs"].as_array_mut().unwrap().push(duplicate);
    assert!(Candidate::from_yaml(&effective.to_string()).is_err());
}

#[test]
fn local_entries_that_cannot_merge_refuse() {
    let mut twin = shared();
    twin["packs"][1]["id"] = json!("acme/aws");
    twin["packs"][1]["registry_identity"] = json!("other");
    for (base, entry) in [
        (shared(), json!({"id":"acme/aws","version":"2.0.0"})),
        (shared(), json!({"id":"acme/aws","profile":"loose"})),
        (
            shared(),
            json!({"id":"acme/aws","version":"2.0.0","profile":"strict","path":"cache/a.yml","disabled":true}),
        ),
        (shared(), json!({"id":"acme/missing","variables":{"a":"b"}})),
        (
            shared(),
            json!({"id":"acme/new","version":"1.0.0","profile":"loose","path":"cache/n.yml","disabled":true}),
        ),
        (
            shared(),
            json!({"id":"acme/new","version":"1.0.0","profile":"loose","path":"cache/n.yml","unset":["a"]}),
        ),
        (shared(), json!({"id":"acme/aws","unknown":true})),
        (twin, json!({"id":"acme/aws","variables":{"a":"b"}})),
    ] {
        let document = json!({"schema_version":1,"packs":[entry]});
        assert!(
            merge(&base.to_string(), Some(&document.to_string())).is_err(),
            "{document}"
        );
    }
    assert!(merge(&shared().to_string(), Some(r#"{"schema_version":2}"#)).is_err());
}

#[test]
fn several_local_entries_for_one_selection_refuse() {
    let personal = json!({"id":"acme/personal","version":"2.0.0","profile":"loose","path":"cache/personal.yml"});
    for entries in [
        // Two overlays of one shared selection would replace each other.
        json!([{"id":"acme/aws","variables":{"a":"b"}},{"id":"acme/aws","overrides":{"list":"deny"}}]),
        json!([{"id":"acme/aws","variables":{"a":"b"}},{"id":"acme/aws","registry_identity":"acme","unset":["region"]}]),
        // A disabled selection named again.
        json!([{"id":"acme/git","disabled":true},{"id":"acme/git","variables":{"a":"b"}}]),
        json!([{"id":"acme/git","disabled":true},{"id":"acme/git","disabled":true}]),
        // A local-only selection named again, as an overlay or a second selection.
        json!([personal.clone(),{"id":"acme/personal","variables":{"a":"b"}}]),
        json!([personal.clone(), personal.clone()]),
    ] {
        let document = json!({"schema_version":1,"packs":entries});
        let error = merge(&shared().to_string(), Some(&document.to_string())).unwrap_err();
        assert!(
            matches!(error, Error::Contract(reason) if reason.contains("several local entries")),
            "{document}"
        );
    }
}

#[test]
fn overlays_apply_and_recover_the_difference_between_layers() {
    let shared: policy::Binding = serde_json::from_value(
        json!({"variables":{"region":"eu","tool":"make"},"overrides":{"list":"ask","lint":"off"}}),
    )
    .unwrap();
    let overlay: Overlay = serde_json::from_value(
        json!({"variables":{"tool":"just","profile":"ro"},"unset":["region"],"overrides":{"lint":"deny"},"clear_overrides":["list"]}),
    )
    .unwrap();
    let mut effective = shared.clone();
    overlay.apply(&mut effective);
    assert_eq!(
        json!(effective),
        json!({"variables":{"tool":"just","profile":"ro"},"overrides":{"lint":"deny"}})
    );
    assert_eq!(Overlay::between(&shared, &effective), overlay);
    assert!(Overlay::between(&shared, &shared).is_empty());
}

#[test]
fn local_values_bind_the_effective_selection_and_change_the_revision() {
    let pack = json!({"schema_version":1,"kind":"pack","id":"acme/tools","version":"1.0.0","description":"Acme","license":"MIT","maintainers":["Acme"],"engines":{"shield":env!("CARGO_PKG_VERSION")},"capabilities":["argument-rules-v1","structured-rules-v1","variables-v1"],
        "variables":{"tool":{"type":"choice","description":"Tool","values":["make","just"]}},
        "rules":[{"id":"build","decision":"allow","conditions":{"domain":"argv","executable":{"var":"tool"},"arguments":{"kind":"any"}}}]}).to_string();
    let shared = json!({"schema_version":1,"packs":[{"id":"acme/tools","version":"1.0.0","profile":"recommended","path":"cache/tools.yml","variables":{"tool":"make"}}]}).to_string();
    let decide = |merged: &str, executable: &str| {
        let snapshot = Candidate::from_yaml(merged)
            .unwrap()
            .capture(std::slice::from_ref(&pack), None)
            .unwrap();
        let revision = snapshot.lock().revision().unwrap();
        let decision = Evaluator::new(vec![(Scope::Project, snapshot)])
            .unwrap()
            .evaluate(&Request {
                executable: executable.into(),
                arguments: vec![],
                cwd: crate::test_support::absolute("/acme").into_owned(),
                project: crate::test_support::absolute("/acme").into_owned(),
            })
            .unwrap()
            .decision;
        (decision, revision)
    };
    let (make, before) = decide(&shared, "make");
    assert_eq!(make, Decision::Allow);
    let local =
        json!({"schema_version":1,"packs":[{"id":"acme/tools","variables":{"tool":"just"}}]})
            .to_string();
    let merged = merge(&shared, Some(&local)).unwrap();
    let (just, after) = decide(&merged, "just");
    assert_eq!(just, Decision::Allow);
    assert_eq!(decide(&merged, "make").0, Decision::Ask);
    assert_ne!(before, after);
}

#[test]
fn override_decision_provenance_survives_preparation_and_compiled_reload() {
    let pack = json!({"schema_version":1,"kind":"pack","id":"acme/tools","version":"1.0.0","description":"Acme provenance fixture","license":"MIT","maintainers":["Acme"],"engines":{"shield":env!("CARGO_PKG_VERSION")},"capabilities":["structured-rules-v1","argument-rules-v1"],
        "rules":[{"id":"status","decision":{"loose":"allow","recommended":"ask","strict":"deny"},"conditions":{"domain":"argv","executable":"git","arguments":{"kind":"any"}}}]}).to_string();
    let request = Request {
        executable: "git".into(),
        arguments: vec!["status".into()],
        cwd: crate::test_support::absolute("/acme").into_owned(),
        project: crate::test_support::absolute("/acme").into_owned(),
    };
    for (project, local, profile, source, authored, before, final_decision, layer) in [
        (
            json!({}),
            None,
            "recommended",
            "pack",
            "ask",
            "ask",
            "ask",
            Value::Null,
        ),
        (
            json!({"status":"deny"}),
            None,
            "recommended",
            "project",
            "ask",
            "ask",
            "deny",
            json!("project"),
        ),
        (
            json!({"status":"deny"}),
            Some(json!({"id":"acme/tools","overrides":{"status":"allow"}})),
            "recommended",
            "local",
            "ask",
            "deny",
            "allow",
            json!("local"),
        ),
        (
            json!({"status":"deny"}),
            Some(json!({"id":"acme/tools","clear_overrides":["status"]})),
            "recommended",
            "pack",
            "ask",
            "ask",
            "ask",
            Value::Null,
        ),
        (
            json!({"status":"deny"}),
            Some(
                json!({"id":"acme/tools","version":"1.0.0","profile":"strict","path":"local.yml","overrides":{"status":"allow"}}),
            ),
            "strict",
            "local",
            "deny",
            "deny",
            "allow",
            json!("local"),
        ),
    ] {
        let shared = json!({"schema_version":1,"packs":[{"id":"acme/tools","version":"1.0.0","profile":"recommended","path":"tools.yml","overrides":project}]}).to_string();
        let local = local.map(|entry| json!({"schema_version":1,"packs":[entry]}).to_string());
        let merged = merge(&shared, local.as_deref()).unwrap();
        let disabled = if policy::parse::<Value>(&merged).unwrap()["disabled"].is_array() {
            vec![pack.clone()]
        } else {
            vec![]
        };
        let snapshot = Candidate::from_yaml(&merged)
            .unwrap()
            .capture_layers(std::slice::from_ref(&pack), None, None, &disabled)
            .unwrap();
        let inputs = vec![(Scope::Project, snapshot)];
        let direct = crate::configuration::evaluate(&inputs, &request).unwrap();
        let evaluator = Evaluator::new(inputs).unwrap();
        let reloaded =
            Evaluator::from_authenticated_compiled(evaluator.compiled_bytes().to_vec()).unwrap();
        let direct = serde_json::to_value(direct).unwrap();
        for evaluation in [
            evaluator.evaluate(&request).unwrap(),
            reloaded.evaluate(&request).unwrap(),
        ] {
            let evaluation = serde_json::to_value(evaluation).unwrap();
            assert_eq!(
                evaluation["evaluation"]["matches"],
                direct["evaluation"]["matches"]
            );
        }
        let evidence = &direct["evaluation"]["matches"][0]["decision_provenance"];
        assert_eq!(evidence["profile"], profile);
        assert_eq!(evidence["source"], source);
        assert_eq!(evidence["default_decision"], authored);
        assert_eq!(evidence["before_override"], before);
        assert_eq!(evidence["final_decision"], final_decision);
        assert_eq!(evidence["override_layer"], layer);
    }
}

#[test]
fn a_local_override_reactivating_a_shared_off_entry_names_its_absent_previous_decision() {
    let pack = json!({"schema_version":1,"kind":"pack","id":"acme/tools","version":"1.0.0","description":"Acme provenance fixture","license":"MIT","maintainers":["Acme"],"engines":{"shield":env!("CARGO_PKG_VERSION")},"capabilities":["structured-rules-v1","argument-rules-v1"],"rules":[{"id":"status","decision":"ask","conditions":{"domain":"argv","executable":"git","arguments":{"kind":"any"}}}]}).to_string();
    let shared = json!({"schema_version":1,"packs":[{"id":"acme/tools","version":"1.0.0","profile":"recommended","path":"tools.yml","overrides":{"status":"off"}}]}).to_string();
    let local =
        json!({"schema_version":1,"packs":[{"id":"acme/tools","overrides":{"status":"allow"}}]})
            .to_string();
    let snapshot = Candidate::from_yaml(&merge(&shared, Some(&local)).unwrap())
        .unwrap()
        .capture(&[pack], None)
        .unwrap();
    let evaluator = Evaluator::new(vec![(Scope::Project, snapshot)]).unwrap();
    let request = Request {
        executable: "git".into(),
        arguments: vec!["status".into()],
        cwd: crate::test_support::absolute("/acme").into_owned(),
        project: crate::test_support::absolute("/acme").into_owned(),
    };
    let evaluation = serde_json::to_value(evaluator.evaluate(&request).unwrap()).unwrap();
    assert_eq!(
        evaluation["evaluation"]["matches"][0]["decision_provenance"],
        json!({"profile":"recommended","source":"local","default_decision":"ask","before_override":null,"final_decision":"allow","override_layer":"local"})
    );
}
