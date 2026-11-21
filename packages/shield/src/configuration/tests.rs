//! Exercises strict candidate contracts and exact lock binding through the public API.

use super::*;
use crate::policy::Scope;
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
    configured["access_paths"] = json!({"read":[crate::test_support::absolute("/acme/reference")],"write":[crate::test_support::absolute("/acme/scratch")]});
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

fn configuration_trace() -> (assert_fs::TempDir, crate::diagnostics::trace::Request) {
    use clap::Parser as _;
    let logs = assert_fs::TempDir::new().unwrap();
    let cli = crate::cli::Cli::parse_from([
        "shield",
        "--debug-log-dir",
        logs.path().to_str().unwrap(),
        "status",
    ]);
    let log = std::sync::Arc::new(crate::diagnostics::Log::open(&cli));
    let request = log.request("command", "status", None, None).unwrap();
    (logs, request)
}

/// One process validates an exact set of sources once; any byte change, and
/// any invalid source, is validated again rather than served from memory.
#[test]
fn identical_sources_are_validated_once_and_changed_or_invalid_ones_again() {
    let (_logs, request) = configuration_trace();
    let _scope = request.enter();
    let counts = || {
        (
            request.recorded_count("policy.source_captures"),
            request.recorded_count("policy.source_capture_reuses"),
        )
    };
    let config = config().to_string();
    let first = Bundle::capture(&config, &[pack()], Some(RULES)).unwrap();
    let (validated, reused) = counts();
    let again = Bundle::capture(&config, &[pack()], Some(RULES)).unwrap();
    let shown =
        Bundle::snapshot_from_json(serde_json::to_string(&first).unwrap().as_bytes()).unwrap();
    assert_eq!(counts(), (validated, reused + 2));
    assert_eq!(json!(again), json!(first));
    assert_eq!(json!(shown.lock()), json!(first.snapshot().unwrap().lock()));
    // Changed bytes are new sources: validated, never reused.
    let edited = format!("{config}\n");
    let changed = Bundle::capture(&edited, &[pack()], Some(RULES)).unwrap();
    assert_eq!(counts().0, validated + 1);
    assert_ne!(json!(changed)["lock"], json!(first)["lock"]);
    // A failure is never remembered: the same invalid sources fail every time.
    let invalid = pack().replace("acme/test", "acme/other");
    for _ in 0..2 {
        assert!(Bundle::capture(&config, std::slice::from_ref(&invalid), Some(RULES)).is_err());
    }
    assert_eq!(counts().0, validated + 3);
}

/// Changing one of two packs parses only that pack; the unchanged one is
/// reused, and the same bytes under another binding are parsed again.
#[test]
fn only_a_changed_pack_or_binding_is_parsed_again() {
    let cache = Mutex::new(ParsedPacks::new(PARSED_PACK_BYTES));
    let (_logs, request) = configuration_trace();
    let _scope = request.enter();
    let counts = || {
        (
            request.recorded_count("pack.parse_reuses"),
            cache.lock().unwrap().entries.len(),
        )
    };
    let binding = Binding::default();
    let (first, second) = (pack(), pack().replace("acme/test", "acme/other"));
    parse_pack(&cache, &first, &binding, None).unwrap();
    parse_pack(&cache, &second, &binding, None).unwrap();
    let (reused, cached) = counts();
    // The second pack changes; the first is reused.
    parse_pack(&cache, &first, &binding, None).unwrap();
    parse_pack(&cache, &format!("{second}\n"), &binding, None).unwrap();
    assert_eq!(counts(), (reused + 1, cached + 1));
    // The same bytes under another binding are never served from the cache.
    let bound = Binding {
        variables: BTreeMap::from([("acme_value".to_owned(), json!("x"))]),
        ..Binding::default()
    };
    assert!(parse_pack(&cache, &first, &bound, None).is_err());
    assert_eq!(counts().0, reused + 1);
}

/// Revising one pack many times keeps the parsed-pack cache within its count
/// and byte budget, while an identical pack is still reused.
#[test]
fn parsed_pack_cache_stays_bounded_across_revisions() {
    let cache = Mutex::new(ParsedPacks::new(PARSED_PACK_BYTES));
    let (_logs, request) = configuration_trace();
    let _scope = request.enter();
    let binding = Binding::default();
    for revision in 0..(2 * PARSED_PACKS_KEPT) {
        let bytes = format!("{}{}", pack(), "\n".repeat(revision));
        parse_pack(&cache, &bytes, &binding, None).unwrap();
        let memo = cache.lock().unwrap();
        assert!(memo.entries.len() <= PARSED_PACKS_KEPT, "{revision}");
        assert!(memo.bytes <= PARSED_PACK_BYTES, "{revision}");
        assert_eq!(
            memo.bytes,
            memo.entries
                .iter()
                .map(|(_, _, size, _)| size)
                .sum::<usize>()
        );
    }
    let newest = format!("{}{}", pack(), "\n".repeat(2 * PARSED_PACKS_KEPT - 1));
    let before = request.recorded_count("pack.parse_reuses");
    parse_pack(&cache, &newest, &binding, None).unwrap();
    assert_eq!(request.recorded_count("pack.parse_reuses"), before + 1);
}

#[test]
fn parsed_pack_cache_evicts_revisions_by_expanded_data_size() {
    let mut source: Value = serde_json::from_str(&pack()).unwrap();
    source["capabilities"] = json!([
        "structured-rules-v1",
        "argument-rules-v1",
        "variables-v1",
        "preconditions-v1"
    ]);
    source["variables"] = json!({
        "commands":{"type":"list","description":"Commands","items":{"type":"string","max_length":64},"max_items":24}
    });
    let preconditions: Vec<_> = (0..8)
        .map(|i| {
            json!({"id":format!("gate-{i}"),
                "conditions":{"domain":"argv","executable":{"var":"commands"},"arguments":{"kind":"any"}},
                "requires_any":[{"arguments":{"kind":"exact","values":["status"]}}],
                "on_unmet":{"decision":"deny"}})
        })
        .collect();
    source["preconditions"] = json!(preconditions);
    let source = source.to_string();
    let bound = |count| Binding {
        variables: BTreeMap::from([(
            "commands".to_owned(),
            json!(
                (0..count)
                    .map(|i| format!("acme-{}-{i}", "command".repeat(6)))
                    .collect::<Vec<_>>()
            ),
        )]),
        ..Binding::default()
    };
    let small = bound(1);
    let expanded = bound(24);
    let charge = |binding| {
        Pack::from_retained_yaml_reusing(&source, binding, false, None)
            .unwrap()
            .cache_bytes(binding, source.len(), PARSED_PACK_BYTES)
            .unwrap()
    };
    let size = charge(&expanded);
    assert!(size > charge(&small) + crate::json::encoded_len(&expanded).unwrap());
    assert!(size > 2 * source.len());
    let cache = Mutex::new(ParsedPacks::new(size + 1));
    let revised = format!("{source}\n");
    for bytes in [&source, &revised] {
        parse_pack(&cache, bytes, &expanded, None).unwrap();
        let memo = cache.lock().unwrap();
        assert_eq!(memo.entries.len(), 1);
        assert!(
            memo.get(&policy::fingerprint(bytes.as_bytes()), &expanded)
                .is_some()
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
        cwd: crate::test_support::absolute("/acme").into_owned(),
        project: crate::test_support::absolute("/acme").into_owned(),
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

fn local_source() -> String {
    LOCAL.replace(
        "/acme/reference",
        &crate::test_support::absolute("/acme/reference"),
    )
}

#[test]
fn modern_custom_rule_merge_preserves_the_previous_raw_rule_digest() {
    let cwd = std::env::current_dir()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let request = policy::Request {
        executable: "acme".into(),
        arguments: vec!["read".into()],
        cwd: cwd.clone(),
        project: cwd,
    };
    for reason in [None, Some("Acme rule")] {
        let mut shared_rule = json!({"id":"shared","decision":"deny",
            "conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"any"}}});
        if let Some(reason) = reason {
            shared_rule["reason"] = json!(reason);
        }
        let local_rule = json!({"id":"local","decision":"allow",
            "conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"exact","values":["read"]}},
            "guidance":{"message":"Acme guidance"}});
        let shared = policy::rules::document(&[shared_rule.clone()])
            .unwrap()
            .to_string();
        let local = policy::rules::document(std::slice::from_ref(&local_rule))
            .unwrap()
            .to_string();
        for shared_source in [None, Some(shared.as_str())] {
            let actual = super::custom_rules(shared_source, Some(&local))
                .unwrap()
                .unwrap();
            // Before retained-source normalization, the merge fed the original
            // rule Values directly to the same canonical document builder.
            let mut rows = if shared_source.is_some() {
                vec![shared_rule.clone()]
            } else {
                vec![]
            };
            rows.push(local_rule.clone());
            let expected = policy::CustomRules::from_yaml(
                &policy::rules::document(&rows).unwrap().to_string(),
            )
            .unwrap();
            let actual = policy::evaluate_selected(&[], &[&actual], &request).unwrap();
            let expected = policy::evaluate_selected(&[], &[&expected], &request).unwrap();
            assert_eq!(actual.revision, expected.revision);
            assert_eq!(actual.decision, expected.decision);
        }
    }
}

#[test]
fn custom_rule_merge_validates_patterns_even_when_local_rules_replace_them() {
    let shared = json!({"schema_version":1,"kind":"custom",
        "capabilities":["structured-rules-v1","argument-rules-v1","regex-v1"],"rules":[{
        "id":"inspect","decision":"allow",
            "conditions":{"domain":"argv","executable":{"regex":"["},"arguments":{"kind":"any"}}
    }]})
    .to_string();
    let local = json!({"schema_version":1,"kind":"custom",
        "capabilities":["structured-rules-v1","argument-rules-v1"],"rules":[{
        "id":"inspect","decision":"allow",
            "conditions":{"domain":"argv","executable":"git","arguments":{"kind":"any"}}
    }]})
    .to_string();
    assert!(super::custom_rules(Some(&shared), Some(&local)).is_err());
    assert!(super::custom_rules(Some(&local), Some(&shared)).is_err());
    assert!(super::custom_rules(Some(&local), Some(&local)).is_ok());
}

fn layered() -> Value {
    json!(
        Bundle::capture_layers(
            SHARED,
            Some(&local_source()),
            &[pack()],
            Some(RULES),
            None,
            &[]
        )
        .unwrap()
    )
}

#[test]
fn bundles_retain_each_layer_and_bind_both_to_the_lock() {
    let bundle = layered();
    assert_eq!(bundle["config"], SHARED);
    assert_eq!(bundle["local_config"], local_source());
    let merged = layers::merged(&bundle).unwrap();
    assert_eq!(
        merged["local_access_paths"]["read"],
        json!([crate::test_support::absolute("/acme/reference")])
    );
    assert_eq!(merged["packs"][0]["id"], "acme/test");
    Bundle::from_json(bundle.to_string().as_bytes()).unwrap();
    let mut altered = bundle.clone();
    altered["local_config"] = json!(local_source().replace("/acme/reference", "/acme/other"));
    assert!(Bundle::from_json(altered.to_string().as_bytes()).is_err());
    let mut dropped = bundle;
    dropped.as_object_mut().unwrap().remove("local_config");
    assert!(Bundle::from_json(dropped.to_string().as_bytes()).is_err());
}

#[test]
fn empty_or_null_settings_capture_a_lock_that_verifies() {
    for settings in [
        "ask_fallback: {}\n",
        "ask_fallback: null\n",
        "history: {}\n",
        "history: null\n",
        "protection: {}\n",
        "registries: {}\n",
        "enforcement: null\n",
    ] {
        let config = format!("{SHARED}{settings}");
        // A document the reader refuses is refused alike everywhere.
        let Ok(candidate) = Candidate::from_yaml(&config) else {
            continue;
        };
        assert_eq!(
            candidate.digest,
            settings::config_digest(&config).unwrap(),
            "{settings}"
        );
        let bundle =
            Bundle::capture_layers(&config, None, &[pack()], Some(RULES), None, &[]).unwrap();
        Bundle::from_json(json!(bundle).to_string().as_bytes())
            .unwrap_or_else(|error| panic!("{settings}: {error:?}"));
    }
}

#[test]
fn revising_a_bundle_edits_only_the_layer_that_changed() {
    let previous = layered();
    let mut merged = layers::merged(&previous).unwrap();
    merged["local_access_paths"]["read"]
        .as_array_mut()
        .unwrap()
        .push(json!(crate::test_support::absolute("/acme/mine")));
    let (revised, captured) =
        Bundle::revise_prepared(&previous, &merged, &[pack()], Some(RULES), None, &[], None)
            .unwrap();
    let prepared = Evaluator::new(vec![(Scope::Project, captured)]).unwrap();
    let decoded = Evaluator::new(vec![(Scope::Project, revised.snapshot().unwrap())]).unwrap();
    assert_eq!(prepared.revision(), decoded.revision());
    assert_eq!(prepared.compiled_bytes(), decoded.compiled_bytes());
    let revised = json!(revised);
    assert_eq!(revised["config"], SHARED);
    assert_eq!(revised["rules"], RULES);
    assert_eq!(
        revised["local_config"],
        format!(
            "{}  - {}\n",
            local_source(),
            crate::test_support::absolute("/acme/mine")
        )
    );
    assert_eq!(layers::merged(&revised).unwrap(), merged);

    let mut merged = layers::merged(&previous).unwrap();
    merged["packs"][0]["profile"] = json!("strict");
    let revised =
        json!(Bundle::revise(&previous, &merged, &[pack()], Some(RULES), None, &[]).unwrap());
    assert_eq!(
        revised["config"],
        SHARED.replace("profile: recommended", "profile: strict")
    );
    assert_eq!(revised["local_config"], local_source());
    assert_ne!(revised["lock"], previous["lock"]);
}

#[test]
fn revising_writes_new_layers_and_rules_as_yaml() {
    let previous = json!(Bundle::capture(SHARED, &[pack()], Some(RULES)).unwrap());
    let mut merged = layers::merged(&previous).unwrap();
    merged["local_access_paths"] = json!({"read":[crate::test_support::absolute("/acme/mine")]});
    let rules = json!({"schema_version":1,"kind":"custom","capabilities":["argument-rules-v1","structured-rules-v1"],"rules":[{"id":"acme-read","decision":"allow","reason":"Acme read","conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"exact","values":["read"]}}}]}).to_string();
    let revised =
        json!(Bundle::revise(&previous, &merged, &[pack()], Some(&rules), None, &[]).unwrap());
    assert_eq!(revised["config"], SHARED);
    let local = revised["local_config"].as_str().unwrap();
    assert!(local.starts_with("schema_version: 1\n"), "{local}");
    assert!(
        local.contains(&format!(
            "- {}\n",
            crate::test_support::absolute("/acme/mine")
        )),
        "{local}"
    );
    let source = revised["rules"].as_str().unwrap();
    assert!(
        source.starts_with("schema_version: 1\nkind: custom\ncapabilities:\n- argument-rules-v1\n- structured-rules-v1\nrules:\n- id: acme-read\n"),
        "{source}"
    );
}
