//! Exercises strict candidate contracts and exact lock binding through the public API.

use super::*;
use serde_json::{Value, json};

fn config() -> Value {
    json!({"schema_version":1,"packs":[{"id":"acme/test","version":"0.1.0","profile":"recommended","path":"packs/test.yml"}],"rules":"rules.yml"})
}

fn pack() -> String {
    json!({"schema_version":1,"id":"acme/test","version":"0.1.0","description":"Acme","license":"UNLICENSED","maintainers":["Acme"],"engines":{"shield":env!("CARGO_PKG_VERSION")},"capabilities":["structured-rules-v1"],"rules":[],"kind":"pack"}).to_string()
}

const RULES: &str =
    "schema_version: 1\nkind: custom\ncapabilities: [structured-rules-v1]\nrules: []\n";

#[test]
fn access_paths_are_strict_bounded_and_bound_to_the_lock() {
    for paths in [
        json!(null),
        json!({"read":null}),
        json!({"write":["relative"]}),
        json!({"write":["/acme/../other"]}),
        json!({"write":["/acme","/acme"]}),
        json!({"execute":[]}),
        json!({"read":vec!["/acme";65]}),
    ] {
        let mut invalid = config();
        invalid["access_paths"] = paths;
        assert!(Candidate::from_yaml(&invalid.to_string()).is_err());
    }
    let mut configured = config();
    configured["access_paths"] = json!({"read":["/acme/reference"],"write":["/acme/scratch"]});
    let first = Candidate::from_yaml(&config().to_string())
        .unwrap()
        .capture(&[pack()], Some(RULES))
        .unwrap();
    let second = Candidate::from_yaml(&configured.to_string())
        .unwrap()
        .capture(&[pack()], Some(RULES))
        .unwrap();
    assert_ne!(
        first.lock().revision().unwrap(),
        second.lock().revision().unwrap()
    );
    assert_eq!(
        json!(second.lock())["access_paths"],
        configured["access_paths"]
    );
}

#[test]
fn strict_configuration_rejects_unsupported_fields_and_selections() {
    for (key, value) in [
        ("schema_version", json!(2)),
        ("packs", json!(null)),
        ("rules", json!(null)),
        ("approved", json!(true)),
        ("digest", json!("agent supplied")),
        ("scope", json!("organization")),
    ] {
        let mut invalid = config();
        invalid[key] = value;
        assert!(Candidate::from_yaml(&invalid.to_string()).is_err(), "{key}");
    }
    for (key, value) in [
        ("id", "git"),
        ("id", "Acme/git"),
        ("version", "^0.1.0"),
        ("profile", "automatic"),
        ("source", "registry"),
    ] {
        let mut invalid = config();
        invalid["packs"][0][key] = json!(value);
        assert!(Candidate::from_yaml(&invalid.to_string()).is_err(), "{key}");
    }
    assert!(Candidate::from_yaml("schema_version: 1\nschema_version: 1\npacks: []").is_err());
}

#[test]
fn portable_paths_reject_traversal_absolute_and_platform_ambiguity() {
    for path in [
        "",
        "/pack.yml",
        "../pack.yml",
        "a/../pack.yml",
        "./pack.yml",
        "a//b",
        "a/",
        "C:/pack.yml",
        "a\\b",
        "a\0b",
        "a\nb",
        "a/CON.yml",
        "a/LPT1",
        "a/file.",
        "a/file ",
        "a/*",
    ] {
        let mut invalid = config();
        invalid["packs"][0]["path"] = json!(path);
        assert!(
            Candidate::from_yaml(&invalid.to_string()).is_err(),
            "{path:?}"
        );
        let mut invalid = config();
        invalid["rules"] = json!(path);
        assert!(
            Candidate::from_yaml(&invalid.to_string()).is_err(),
            "rules {path:?}"
        );
    }
}

#[test]
fn configuration_limits_and_duplicate_identities_are_enforced() {
    let mut invalid = config();
    let selection = invalid["packs"][0].clone();
    invalid["packs"]
        .as_array_mut()
        .unwrap()
        .push(selection.clone());
    assert!(Candidate::from_yaml(&invalid.to_string()).is_err());
    invalid["packs"] = json!(vec![selection; 65]);
    assert!(matches!(
        Candidate::from_yaml(&invalid.to_string()),
        Err(Error::Limit)
    ));
    invalid = config();
    invalid["rules"] = json!("x".repeat(4097));
    assert!(matches!(
        Candidate::from_yaml(&invalid.to_string()),
        Err(Error::Limit)
    ));
}

#[test]
fn locks_require_complete_valid_artifacts_with_exact_identity_and_version() {
    let candidate = Candidate::from_yaml(&config().to_string()).unwrap();
    assert!(candidate.capture(&[], Some(RULES)).is_err());
    assert!(candidate.capture(&[pack()], None).is_err());
    assert!(candidate.capture(&[pack()], Some("invalid: [")).is_err());
    for invalid in [
        pack().replace("acme/test", "acme/other"),
        pack().replace("0.1.0\"", "0.1.1\""),
        "invalid: [".into(),
    ] {
        assert!(candidate.capture(&[invalid], Some(RULES)).is_err());
    }
    let bundle = Bundle::capture(&config().to_string(), &[pack()], Some(RULES)).unwrap();
    let encoded = serde_json::to_string(&bundle).unwrap();
    Bundle::from_json(encoded.as_bytes()).unwrap();
    Bundle::snapshot_from_json(encoded.as_bytes()).unwrap();
    for (key, value) in [
        ("approved", json!(true)),
        ("schema_version", json!(2)),
        ("kind", json!("active")),
        ("config_digest", json!("fabricated")),
        ("packs", json!([])),
        ("rules", json!(null)),
    ] {
        let mut altered: Value = serde_json::from_str(&encoded).unwrap();
        altered["lock"][key] = value;
        assert!(
            Bundle::from_json(altered.to_string().as_bytes()).is_err(),
            "{key}"
        );
        assert!(
            Bundle::snapshot_from_json(altered.to_string().as_bytes()).is_err(),
            "{key}"
        );
    }
    for (key, value) in [
        ("id", "acme/other"),
        ("version", "0.1.1"),
        ("profile", "loose"),
    ] {
        let mut altered: Value = serde_json::from_str(&encoded).unwrap();
        altered["lock"]["packs"][0][key] = json!(value);
        assert!(
            Bundle::from_json(altered.to_string().as_bytes()).is_err(),
            "{key}"
        );
        assert!(
            Bundle::snapshot_from_json(altered.to_string().as_bytes()).is_err(),
            "{key}"
        );
    }
}

#[test]
fn fingerprints_bind_all_input_bytes_and_do_not_claim_approval() {
    let config = config().to_string();
    let candidate = Candidate::from_yaml(&config).unwrap();
    let snapshot = candidate.capture(&[pack()], Some(RULES)).unwrap();
    let revision = snapshot.lock().revision().unwrap();
    assert_eq!(
        revision,
        candidate
            .capture(&[pack()], Some(RULES))
            .unwrap()
            .lock()
            .revision()
            .unwrap()
    );
    for changed in [
        Candidate::from_yaml(&(config + "\n"))
            .unwrap()
            .capture(&[pack()], Some(RULES))
            .unwrap(),
        candidate.capture(&[pack() + "\n"], Some(RULES)).unwrap(),
        candidate
            .capture(&[pack()], Some(&(RULES.to_owned() + "\n")))
            .unwrap(),
    ] {
        assert_ne!(revision, changed.lock().revision().unwrap());
        assert_ne!(
            serde_json::to_value(changed.lock()).unwrap(),
            serde_json::to_value(snapshot.lock()).unwrap()
        );
    }
    let empty = Candidate::from_yaml("schema_version: 1\npacks: []\n").unwrap();
    assert_eq!(empty.rules_path(), None);
    assert!(empty.capture(&[], None).is_ok());
    assert!(empty.capture(&[], Some(RULES)).is_err());
}

#[test]
fn lock_order_is_canonical_and_per_pack_profiles_are_bound() {
    let mut input = config();
    input.as_object_mut().unwrap().remove("rules");
    let mut second = input["packs"][0].clone();
    second["id"] = json!("acme/another");
    second["profile"] = json!("strict");
    second["path"] = json!("packs/another.yml");
    input["packs"].as_array_mut().unwrap().push(second);
    let snapshot = Candidate::from_yaml(&input.to_string())
        .unwrap()
        .capture(&[pack(), pack().replace("acme/test", "acme/another")], None)
        .unwrap();
    let bundle = Bundle::capture(
        &input.to_string(),
        &[pack(), pack().replace("acme/test", "acme/another")],
        None,
    )
    .unwrap();
    let mut retained = serde_json::to_value(&bundle).unwrap();
    let value = &mut retained["lock"];
    assert_eq!(value["packs"][0]["id"], "acme/another");
    assert_eq!(value["packs"][0]["profile"], "strict");
    assert_eq!(value["packs"][1]["profile"], "recommended");
    value["packs"].as_array_mut().unwrap().reverse();
    assert!(Bundle::from_json(retained.to_string().as_bytes()).is_err());
    input["packs"][0]["profile"] = json!("loose");
    let changed = Candidate::from_yaml(&input.to_string())
        .unwrap()
        .capture(&[pack(), pack().replace("acme/test", "acme/another")], None)
        .unwrap();
    assert_ne!(
        snapshot.lock().revision().unwrap(),
        changed.lock().revision().unwrap()
    );
    let empty = Bundle::capture("schema_version: 1\npacks: []\n", &[], None).unwrap();
    let mut invalid = serde_json::to_value(&empty).unwrap();
    invalid["lock"]["packs"] = json!(null);
    assert!(Bundle::from_json(invalid.to_string().as_bytes()).is_err());
}

#[test]
fn dependency_candidates_require_complete_versions_profiles_and_source_identity() {
    let mut parent: Value = serde_json::from_str(&pack()).unwrap();
    parent["schema_version"] = json!(1);
    parent["id"] = json!("acme/parent");
    parent["capabilities"] = json!(["structured-rules-v1", "dependencies-v1"]);
    parent["dependencies"] = json!([{"pack":"acme/test","requirement":"^0.1","profiles":{"loose":"recommended","recommended":"strict","strict":"strict"}}]);
    let mut config = json!({"schema_version":1,"packs":[{"id":"acme/parent","version":"0.1.0","registry_identity":"acme-source","profile":"recommended","path":"parent.yml"}]});
    assert!(
        Candidate::from_yaml(&config.to_string())
            .unwrap()
            .capture(&[parent.to_string()], None)
            .is_err()
    );
    config["packs"].as_array_mut().unwrap().push(json!({"id":"acme/test","version":"0.1.0","registry_identity":"acme-source","profile":"strict","path":"child.yml"}));
    let bytes = [parent.to_string(), pack()];
    assert!(
        Candidate::from_yaml(&config.to_string())
            .unwrap()
            .capture(&bytes, None)
            .is_ok()
    );
    config["packs"][1]["profile"] = json!("recommended");
    assert!(
        Candidate::from_yaml(&config.to_string())
            .unwrap()
            .capture(&bytes, None)
            .is_err()
    );
    config["packs"][1]["profile"] = json!("strict");
    config["packs"][1]["registry_identity"] = json!("foreign-source");
    assert!(
        Candidate::from_yaml(&config.to_string())
            .unwrap()
            .capture(&bytes, None)
            .is_err()
    );
    for changed in [
        json!(null),
        json!([{"pack":"acme/test","requirement":"*","registry":null}]),
        json!([{"pack":"acme/test","requirement":"*","profiles":{"loose":"strict","recommended":"loose","strict":"strict"}}]),
        json!([{"pack":"acme/test","requirement":"*","install":"run"}]),
    ] {
        parent["dependencies"] = changed;
        assert!(Pack::from_yaml(&parent.to_string()).is_err());
    }
    parent["dependencies"] = json!([]);
    parent["schema_version"] = json!(1);
    parent["capabilities"] = json!(["structured-rules-v1"]);
    assert!(Pack::from_yaml(&parent.to_string()).is_err());
}

/// Asserts that `stage` refused `selection` for its unset required `tool` variable.
fn assert_unset_tool<T>(result: Result<T, Error>, selection: &str, stage: &str) {
    match result {
        Err(Error::Binding {
            selection: named,
            variable,
            reason,
        }) => {
            assert_eq!((named.as_str(), variable.as_str()), (selection, "tool"));
            assert_eq!(reason, "a required pack variable is unset");
        }
        _ => panic!("{stage} accepted an unset required variable"),
    }
}

#[test]
fn selection_bindings_resolve_the_captured_pack_and_change_its_revision() {
    let source = json!({"schema_version":1,"id":"acme/test","version":"0.1.0","description":"Acme","license":"UNLICENSED","maintainers":["Acme"],"engines":{"shield":env!("CARGO_PKG_VERSION")},"capabilities":["argument-rules-v1","structured-rules-v1","variables-v1"],"kind":"pack",
        "variables":{"tool":{"type":"choice","description":"Build tool","values":["make","just"],"required":true}},
        "rules":[{"id":"build","decision":"allow","conditions":{"domain":"argv","executable":{"var":"tool"},"arguments":{"kind":"any"}}}]}).to_string();
    let request = |executable: &str| crate::policy::Request {
        executable: executable.into(),
        arguments: vec![],
        cwd: "/acme".into(),
        project: "/acme".into(),
    };
    let mut config = config();
    config.as_object_mut().unwrap().remove("rules");
    // Capture keeps an unset required variable for setup to fill; activation refuses it.
    let unset = Candidate::from_yaml(&config.to_string())
        .unwrap()
        .capture(std::slice::from_ref(&source), None)
        .unwrap();
    assert_eq!(
        unset.packs[0].unset_required().collect::<Vec<_>>(),
        ["tool"]
    );
    let selection = unset.packs[0].id().to_owned();
    // One-shot evaluation, which also serves domain comparison, refuses alike.
    let inputs = vec![(policy::Scope::Project, unset)];
    assert_unset_tool(
        evaluation::evaluate(&inputs, &request("make")),
        &selection,
        "evaluation",
    );
    assert_unset_tool(Evaluator::new(inputs), &selection, "activation");
    let mut wrong = config.clone();
    wrong["packs"][0]["variables"] = json!({"tool":"cargo"});
    match Candidate::from_yaml(&wrong.to_string())
        .unwrap()
        .capture(std::slice::from_ref(&source), None)
    {
        Err(Error::Binding {
            selection: named,
            variable,
            ..
        }) => assert_eq!(
            (named.as_str(), variable.as_str()),
            (selection.as_str(), "tool")
        ),
        _ => panic!("capture accepted an invalid binding"),
    }
    let mut revisions = Vec::new();
    for tool in ["make", "just"] {
        config["packs"][0]["variables"] = json!({"tool":tool});
        let snapshot = Candidate::from_yaml(&config.to_string())
            .unwrap()
            .capture(std::slice::from_ref(&source), None)
            .unwrap();
        revisions.push(snapshot.lock().revision().unwrap());
        let inputs = [(policy::Scope::Project, snapshot)];
        assert_eq!(
            evaluate(&inputs, &request(tool)).unwrap().decision,
            policy::Decision::Allow
        );
        let other = if tool == "make" { "just" } else { "make" };
        assert_eq!(
            evaluate(&inputs, &request(other)).unwrap().decision,
            policy::Decision::Ask
        );
    }
    assert_ne!(revisions[0], revisions[1]);
    for invalid in [
        json!({"variables":{"tool":"cargo"}}),
        json!({"variables":{"other":"make"}}),
        json!({"variables":{"tool":"make"},"overrides":{"missing":"off"}}),
        json!({"variables":null}),
    ] {
        let mut selection = config.clone();
        for (key, value) in invalid.as_object().unwrap() {
            selection["packs"][0][key] = value.clone();
        }
        assert!(
            Candidate::from_yaml(&selection.to_string())
                .and_then(|c| c.capture(std::slice::from_ref(&source), None))
                .is_err(),
            "{invalid}"
        );
    }
}

const SHARED: &str = "\
# Acme team policy.
schema_version: 1
packs:
- id: acme/test # shared toolchain
  version: 0.1.0
  profile: recommended
  path: packs/test.yml
rules: rules.yml
";

const LOCAL: &str = "\
# Personal settings.
schema_version: 1
packs: []
access_paths:
  read:
  - /acme/reference
";

fn layered() -> Value {
    json!(Bundle::capture_layers(SHARED, Some(LOCAL), &[pack()], Some(RULES), None, &[]).unwrap())
}

#[test]
fn bundles_retain_each_layer_and_bind_both_to_the_lock() {
    let bundle = layered();
    assert_eq!(bundle["config"], SHARED);
    assert_eq!(bundle["local_config"], LOCAL);
    let merged = layers::merged(&bundle).unwrap();
    assert_eq!(
        merged["local_access_paths"]["read"],
        json!(["/acme/reference"])
    );
    assert_eq!(merged["packs"][0]["id"], "acme/test");
    Bundle::from_json(bundle.to_string().as_bytes()).unwrap();
    let mut altered = bundle.clone();
    altered["local_config"] = json!(LOCAL.replace("/acme/reference", "/acme/other"));
    assert!(Bundle::from_json(altered.to_string().as_bytes()).is_err());
    let mut dropped = bundle;
    dropped.as_object_mut().unwrap().remove("local_config");
    assert!(Bundle::from_json(dropped.to_string().as_bytes()).is_err());
}

#[test]
fn revising_a_bundle_edits_only_the_layer_that_changed() {
    let previous = layered();
    let mut merged = layers::merged(&previous).unwrap();
    merged["local_access_paths"]["read"]
        .as_array_mut()
        .unwrap()
        .push(json!("/acme/mine"));
    let revised =
        json!(Bundle::revise(&previous, &merged, &[pack()], Some(RULES), None, &[]).unwrap());
    assert_eq!(revised["config"], SHARED);
    assert_eq!(revised["rules"], RULES);
    assert_eq!(revised["local_config"], format!("{LOCAL}  - /acme/mine\n"));
    assert_eq!(layers::merged(&revised).unwrap(), merged);

    let mut merged = layers::merged(&previous).unwrap();
    merged["packs"][0]["profile"] = json!("strict");
    let revised =
        json!(Bundle::revise(&previous, &merged, &[pack()], Some(RULES), None, &[]).unwrap());
    assert_eq!(
        revised["config"],
        SHARED.replace("profile: recommended", "profile: strict")
    );
    assert_eq!(revised["local_config"], LOCAL);
    assert_ne!(revised["lock"], previous["lock"]);
}

#[test]
fn revising_writes_new_layers_and_rules_as_yaml() {
    let previous = json!(Bundle::capture(SHARED, &[pack()], Some(RULES)).unwrap());
    let mut merged = layers::merged(&previous).unwrap();
    merged["local_access_paths"] = json!({"read":["/acme/mine"]});
    let rules = json!({"schema_version":1,"kind":"custom","capabilities":["argument-rules-v1","structured-rules-v1"],"rules":[{"id":"acme-read","decision":"allow","reason":"Acme read","conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"exact","values":["read"]}}}]}).to_string();
    let revised =
        json!(Bundle::revise(&previous, &merged, &[pack()], Some(&rules), None, &[]).unwrap());
    assert_eq!(revised["config"], SHARED);
    let local = revised["local_config"].as_str().unwrap();
    assert!(local.starts_with("schema_version: 1\n"), "{local}");
    assert!(local.contains("- /acme/mine\n"), "{local}");
    let source = revised["rules"].as_str().unwrap();
    assert!(
        source.starts_with("schema_version: 1\nkind: custom\ncapabilities:\n- argument-rules-v1\n- structured-rules-v1\nrules:\n- id: acme-read\n"),
        "{source}"
    );
}
