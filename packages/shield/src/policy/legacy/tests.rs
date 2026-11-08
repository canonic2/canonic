//! Frozen matcher documents from the historical policy engine.
use super::*;
use crate::policy::{CustomRules, Pack as PreparedPack, Profile, Request, evaluate};

fn row(id: &str, kind: &str, values: &[&str], decision: &str) -> Value {
    let arguments = if kind == "any" {
        json!({"kind":kind})
    } else {
        json!({"kind":kind,"values":values})
    };
    json!({"id":id,"matcher":{"executable":"acme","arguments":arguments},
        "reason":"Acme test policy","profiles":{
            "loose":decision,"recommended":decision,"strict":decision}})
}

fn document(rows: &[Value]) -> Value {
    json!({"schema_version":1,"id":"acme/test","version":"0.1.0",
        "description":"Acme test pack","license":"MIT","maintainers":["Acme"],
        "engine":">=0.1.0, <0.2.0","capabilities":["argv-v1"],"rules":rows})
}

fn request(arguments: &[&str]) -> Request {
    let cwd = std::env::current_dir()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    Request {
        executable: "acme".into(),
        arguments: arguments.iter().map(|s| (*s).into()).collect(),
        cwd: cwd.clone(),
        project: cwd,
    }
}

#[test]
fn retained_matcher_sources_keep_exact_digest_and_acquisition_stays_strict() {
    let source = document(&[row("read", "exact", &["read", "a b"], "allow")]).to_string();
    assert!(PreparedPack::from_yaml(&source).is_err());
    let pack = PreparedPack::from_retained_yaml(&source).unwrap();
    assert_eq!(pack.digest, super::super::fingerprint(source.as_bytes()));
    for (args, decision) in [
        (vec!["read", "a b"], Decision::Allow),
        (vec!["read", "a", "b"], Decision::Ask),
        (vec!["read", "a b", "--dangerous"], Decision::Ask),
    ] {
        assert_eq!(
            evaluate(
                std::slice::from_ref(&pack),
                None,
                Profile::Recommended,
                &request(&args)
            )
            .unwrap()
            .decision,
            decision
        );
    }
}

#[test]
fn historical_custom_context_and_specificity_survive_translation() {
    let custom = |id: &str, decision: &str| {
        let mut r = row(id, "exact", &["read"], decision);
        let object = r.as_object_mut().unwrap();
        object.remove("profiles");
        object.insert("scope".into(), json!("project"));
        object.insert("decision".into(), json!(decision));
        r
    };
    let broad = custom("unscoped", "deny");
    let mut narrow = custom("scoped", "allow");
    narrow["matcher"]["cwd"] = json!(request(&[]).cwd);
    narrow["matcher"]["project"] = json!(request(&[]).project);
    let source = json!({"schema_version":1,"rules":[broad,narrow]}).to_string();
    let rules = CustomRules::from_retained_yaml(&source).unwrap();
    assert_eq!(rules.digest, super::super::fingerprint(source.as_bytes()));
    let req = request(&["read"]);
    assert_eq!(
        evaluate(&[], Some(&rules), Profile::Recommended, &req)
            .unwrap()
            .decision,
        Decision::Allow
    );
    let other = Request {
        project: std::env::temp_dir().to_string_lossy().into_owned(),
        ..req
    };
    assert_eq!(
        evaluate(&[], Some(&rules), Profile::Recommended, &other)
            .unwrap()
            .decision,
        Decision::Deny
    );
}

#[test]
fn retained_decoder_rejects_unknown_or_modern_documents_missing_required_fields() {
    let base = document(&[row("read", "exact", &["read"], "allow")]);
    for (field, value) in [
        ("schema_version", json!(4)),
        ("capabilities", json!(["structured-rules-v1"])),
        ("engine", json!("nonsense")),
        ("unknown", json!(true)),
    ] {
        let mut changed = base.clone();
        changed[field] = value;
        assert!(PreparedPack::from_retained_yaml(&changed.to_string()).is_err());
    }
    let modern = json!({"schema_version":1,"rules":[{
        "id":"read","decision":"allow","conditions":{"domain":"argv","executable":"acme"}
    }]});
    assert!(CustomRules::from_retained_yaml(&modern.to_string()).is_err());
    for (scope, mandatory) in [("user", false), ("organization", true), ("project", true)] {
        let mut r = row("guard", "any", &[], "deny");
        r.as_object_mut().unwrap().remove("profiles");
        r["scope"] = json!(scope);
        r["mandatory"] = json!(mandatory);
        r["decision"] = json!("deny");
        assert!(
            CustomRules::from_retained_yaml(&json!({"schema_version":1,"rules":[r]}).to_string())
                .is_err()
        );
    }
}

#[test]
fn retained_pack_header_accepts_exact_historical_metadata_limits() {
    let mut source = document(&[]);
    source["maintainers"] = json!(["M".repeat(4096), "M".repeat(4096)]);
    source["version"] = json!(format!("0.1.0+{}", "a".repeat(200)));
    let pack = PreparedPack::from_retained_yaml(&source.to_string()).unwrap();
    assert_eq!(pack.version(), source["version"].as_str().unwrap());
    let mut descriptor = pack.document.descriptor(|_| true);
    descriptor.validate().unwrap();
    descriptor.retained_legacy = false;
    assert!(descriptor.validate().is_err());
}

#[test]
fn historical_dependency_schema_and_effect_declarations_remain_exact() {
    let mut source = document(&[row("read", "exact", &["read"], "allow")]);
    source["schema_version"] = json!(3);
    source["capabilities"] = json!(["argv-v1", "dependencies-v1"]);
    source["dependencies"] = json!([{"pack":"acme/base","requirement":"^0.1"}]);
    assert_eq!(
        PreparedPack::from_retained_yaml(&source.to_string())
            .unwrap()
            .dependencies()
            .len(),
        1
    );
    source["dependencies"] = Value::Null;
    assert!(PreparedPack::from_retained_yaml(&source.to_string()).is_err());
    source = document(&[row("network", "any", &[], "deny")]);
    source["rules"][0]["matcher"]["possible_effects"] = json!(["network"]);
    assert!(PreparedPack::from_retained_yaml(&source.to_string()).is_err());
    source["schema_version"] = json!(2);
    source["capabilities"] = json!(["argv-v1", "possible-effects-v1"]);
    assert!(PreparedPack::from_retained_yaml(&source.to_string()).is_ok());
}

#[test]
fn reviewed_normalization_preserves_context_and_requires_explicit_capability() {
    let source = json!({"schema_version":1,"rules":[{
        "id":"read","scope":"project","decision":"allow","reason":"Acme test policy",
        "matcher":{"executable":"acme","arguments":{"kind":"exact","values":["read"]},
            "project":request(&[]).project}
    }]})
    .to_string();
    let normalized = super::super::normalize_retained_custom(&source).unwrap();
    assert!(
        normalized["capabilities"]
            .as_array()
            .unwrap()
            .contains(&json!("historical-matchers-v1"))
    );
    let parsed = CustomRules::from_yaml(&normalized.to_string()).unwrap();
    assert_eq!(
        evaluate(
            &[],
            Some(&parsed),
            Profile::Recommended,
            &request(&["read"])
        )
        .unwrap()
        .decision,
        Decision::Allow
    );
    let mut other = request(&["read"]);
    other.project = std::env::temp_dir().to_string_lossy().into_owned();
    assert_eq!(
        evaluate(&[], Some(&parsed), Profile::Recommended, &other)
            .unwrap()
            .decision,
        Decision::Ask
    );
    let mut undeclared = normalized.clone();
    undeclared["capabilities"] = json!(["structured-rules-v1"]);
    assert!(CustomRules::from_yaml(&undeclared.to_string()).is_err());
    let mut smuggled = normalized.clone();
    smuggled["rules"][0]["conditions"]["arguments"] = json!({"kind":"any"});
    assert!(CustomRules::from_yaml(&smuggled.to_string()).is_err());
    let mut mandatory = normalized;
    mandatory["rules"][0]["historical_matcher"]["mandatory"] = json!(true);
    assert!(CustomRules::from_yaml(&mandatory.to_string()).is_err());
}

#[test]
fn original_normalized_project_rules_keep_nullable_fields_and_source_identity() {
    // Shape and selectors from the historical normalized policy schema.
    let source = json!({"schema_version":1,"rules":[{
        "schema_version":1,"id":"Acme read rule","decision":"allow",
        "conditions":{"domain":"argv","executable":"acme",
            "arguments":{"kind":"prefix","values":["read"]}},
        "reason":null,"expires_at":null
    }]})
    .to_string();
    let rules = CustomRules::from_retained_yaml(&source).unwrap();
    assert_eq!(rules.digest, super::super::fingerprint(source.as_bytes()));
    assert_eq!(
        evaluate(
            &[],
            Some(&rules),
            Profile::Recommended,
            &request(&["read", "a b"])
        )
        .unwrap()
        .decision,
        Decision::Allow
    );
    assert_eq!(
        evaluate(
            &[],
            Some(&rules),
            Profile::Recommended,
            &request(&["write"])
        )
        .unwrap()
        .decision,
        Decision::Ask
    );
    let normalized = super::super::normalize_retained_custom(&source).unwrap();
    assert!(
        normalized["capabilities"]
            .as_array()
            .unwrap()
            .contains(&json!("historical-normalized-v1"))
    );
    assert!(CustomRules::from_yaml(&normalized.to_string()).is_ok());
    let original: Value = serde_json::from_str(&source).unwrap();
    for field in ["schema_version", "reason", "expires_at"] {
        let mut missing = original.clone();
        missing["rules"][0].as_object_mut().unwrap().remove(field);
        assert!(
            CustomRules::from_retained_yaml(&missing.to_string()).is_err(),
            "{field}"
        );
    }
    let mut changed = normalized;
    changed["rules"][0]["conditions"]["arguments"] = json!({"kind":"any"});
    assert!(CustomRules::from_yaml(&changed.to_string()).is_err());
}

#[test]
fn historical_normalized_expiry_uses_original_calendar_validation() {
    for (expiry, accepted) in [
        ("2024-02-29T12:00:00Z", true),
        ("2025-02-29T12:00:00Z", false),
        ("2025-04-31T12:00:00Z", false),
    ] {
        let source = json!({"schema_version":1,"rules":[{
            "schema_version":1,"id":"read","decision":"ask","reason":null,
            "conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"any"}},
            "expires_at":expiry
        }]})
        .to_string();
        assert_eq!(
            CustomRules::from_retained_yaml(&source).is_ok(),
            accepted,
            "{expiry}"
        );
    }
}

#[test]
fn schema_three_dependencies_keep_the_exact_original_nested_fields() {
    let mut source = document(&[]);
    source["schema_version"] = json!(3);
    source["capabilities"] = json!(["argv-v1", "dependencies-v1"]);
    let dependency = json!({"pack":"acme/base","requirement":"^0.1","registry":"acme-source",
        "profiles":{"loose":"loose","recommended":"recommended","strict":"strict"}});
    source["dependencies"] = json!([dependency.clone()]);
    let retained = PreparedPack::from_retained_yaml(&source.to_string()).unwrap();
    let declaration = &retained.dependencies()[0];
    assert_eq!(declaration.registry.as_deref(), Some("acme-source"));
    assert_eq!(
        declaration.profiles.as_ref().unwrap().strict,
        Profile::Strict
    );
    for (field, value) in [
        ("unknown", json!(true)),
        ("registry", Value::Null),
        ("profiles", Value::Null),
        (
            "profiles",
            json!({"loose":"loose","recommended":"recommended","strict":"strict","future":"allow"}),
        ),
    ] {
        source["dependencies"] = json!([dependency.clone()]);
        source["dependencies"][0][field] = value;
        assert!(
            PreparedPack::from_retained_yaml(&source.to_string()).is_err(),
            "{field}"
        );
    }
}

#[test]
fn historical_source_size_limit_is_the_original_one_mib() {
    let source = document(&[]).to_string();
    let exact = format!("{}{}", source, " ".repeat(1_048_576 - source.len()));
    assert!(PreparedPack::from_retained_yaml(&exact).is_ok());
    let oversized = format!("{exact} ");
    assert!(matches!(
        PreparedPack::from_retained_yaml(&oversized),
        Err(Error::Limit)
    ));
}

#[test]
fn retained_large_modern_sources_keep_the_current_budget_and_field_order() {
    let padding = " ".repeat(1_048_576);
    for source in [
        format!(
            "{{\"rules\":[],{padding}\"schema_version\":1,\"kind\":\"custom\",\"capabilities\":[\"structured-rules-v1\"]}}"
        ),
        format!(
            "rules: []\n{padding}\nschema_version: 1\nkind: custom\ncapabilities: [structured-rules-v1]\n"
        ),
    ] {
        assert!(CustomRules::from_retained_yaml(&source).is_ok());
    }
    let oversized_historical = format!("{{\"schema_version\":1,\"rules\":[]}}{padding}");
    assert!(matches!(
        CustomRules::from_retained_yaml(&oversized_historical),
        Err(Error::Limit)
    ));
}

#[test]
fn retained_yaml_decoding_preserves_typed_plain_scalar_text() {
    let modern = "schema_version: 1\nkind: custom\ncapabilities: [structured-rules-v1, argument-rules-v1]\nrules:\n- id: inspect\n  decision: allow\n  reason: true\n  conditions:\n    domain: argv\n    executable: acme\n    arguments: {kind: any}\n";
    let current = CustomRules::from_yaml(modern).unwrap();
    for source in [
        modern,
        "schema_version: 1\nrules:\n- id: inspect\n  scope: project\n  decision: allow\n  reason: true\n  matcher:\n    executable: acme\n    arguments: {kind: any}\n",
        "schema_version: 1\nrules:\n- schema_version: 1\n  id: inspect\n  decision: allow\n  reason: true\n  expires_at: null\n  conditions:\n    domain: argv\n    executable: acme\n    arguments: {kind: any}\n",
    ] {
        let retained = CustomRules::from_retained_yaml(source).unwrap();
        for rules in [&current, &retained] {
            let result = evaluate(&[], Some(rules), Profile::Recommended, &request(&[])).unwrap();
            assert_eq!(result.decision, Decision::Allow);
            assert_eq!(result.matches[0].reason, "true");
        }
    }
}
