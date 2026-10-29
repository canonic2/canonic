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
fn local_entries_that_cannot_merge_refuse() {
    let mut twin = shared();
    twin["packs"][1]["id"] = json!("acme/aws");
    twin["packs"][1]["registry_identity"] = json!("other");
    for (base, entry) in [
        (shared(), json!({"id":"acme/aws","version":"2.0.0"})),
        (shared(), json!({"id":"acme/aws","profile":"loose"})),
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
                cwd: "/acme".into(),
                project: "/acme".into(),
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
