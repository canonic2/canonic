//! Exercises domain relations, authority, scope metadata, and incomplete evidence.

use super::*;
use crate::{configuration::Candidate, policy::Decision};
use serde_json::{Value, json};

fn context() -> String {
    std::env::current_dir().unwrap().to_str().unwrap().into()
}

#[test]
fn identical_pack_artifacts_with_changed_access_paths_cannot_prove_equivalence() {
    let pack = json!({"schema_version":1,"kind":"pack","id":"acme/files","version":"0.1.0","description":"Acme file policy","license":"MIT","maintainers":["Acme"],"engines":{"shield":"*"},"capabilities":["structured-rules-v1","argument-rules-v1","location-v1","access-paths-v1","requirements-v1"],"rules":[{"id":"remove","decision":"allow","conditions":{"domain":"argv","executable":"rm","arguments":{"kind":"any"}},"requirements":{"targets":{"action":"remove","all":{"within":{"root":"write"}}}},"on_unmet":{"decision":"deny"}}]}).to_string();
    let mut config = json!({"schema_version":1,"packs":[{"id":"acme/files","version":"0.1.0","profile":"recommended","path":"pack.yml"}]});
    let before = [(
        Scope::Project,
        Candidate::from_yaml(&config.to_string())
            .unwrap()
            .capture(std::slice::from_ref(&pack), None)
            .unwrap(),
    )];
    config["access_paths"] = json!({"write":[context()]});
    let after = [(
        Scope::Project,
        Candidate::from_yaml(&config.to_string())
            .unwrap()
            .capture(&[pack], None)
            .unwrap(),
    )];
    for report in [
        compare_policy(&before, &after).unwrap(),
        result(&before, &after),
    ] {
        assert_eq!(report.relation, Relation::Unknown);
        assert!(!report.complete);
    }
}

fn snapshot(scope: Scope, rules: Value) -> Snapshot {
    let Value::Array(rows) = rules else {
        panic!("expected rows")
    };
    let document = crate::policy::rules::document(&rows).unwrap();
    let config = Candidate::from_yaml("schema_version: 1\npacks: []\nrules: rules.yml\n").unwrap();
    let snapshot = config.capture(&[], Some(&document.to_string())).unwrap();
    let inputs = [(scope, snapshot)];
    // Establish this fixture really satisfies declared-scope composition.
    super::Prepared::new(&inputs).unwrap();
    inputs.into_iter().next().unwrap().1
}

fn rule(id: &str, kind: &str, values: &[&str], decision: Decision) -> Value {
    let arguments = if kind == "any" {
        json!({"kind":kind})
    } else {
        json!({"kind":kind,"values":values})
    };
    json!({"id":id,"decision":decision,"reason":"Acme operation","conditions":{"domain":"argv","executable":"acme","arguments":arguments}})
}

fn result(before: &[(Scope, Snapshot)], after: &[(Scope, Snapshot)]) -> DomainComparison {
    compare_argv(before, after, &context(), &context()).unwrap()
}

#[test]
fn identical_selected_effect_policies_have_a_classifier_independent_identity_proof() {
    let doc = json!({"schema_version":1,"rules":[{"id":"network","decision":"deny","reason":"Acme network","conditions":{"domain":"argv","executable":"git","arguments":{"kind":"any"},"possible_effects":["network"]}}],"kind":"custom","capabilities":["argument-rules-v1","possible-effects-v1","structured-rules-v1"]});
    let candidate =
        Candidate::from_yaml("schema_version: 1\npacks: []\nrules: rules.yml\n").unwrap();
    let a = [(
        Scope::Project,
        candidate.capture(&[], Some(&doc.to_string())).unwrap(),
    )];
    let b = [(
        Scope::Project,
        candidate.capture(&[], Some(&doc.to_string())).unwrap(),
    )];
    for report in [
        compare_policy(&a, &b).unwrap(),
        compare_argv(&a, &b, &context(), &context()).unwrap(),
    ] {
        assert!(report.complete);
        let value = serde_json::to_value(report).unwrap();
        assert_eq!(value["incomplete_reason"], Value::Null);
        assert_eq!(value["relation"], "equivalent");
        assert_eq!(value["algorithm"], "selected-artifact-identity-v1");
        assert_eq!(value["evaluated_regions"], 0);
        assert_eq!(value["schema_version"], 2);
    }
    let mut different = doc;
    different["rules"][0]["decision"] = json!("ask");
    let b = [(
        Scope::Project,
        candidate
            .capture(&[], Some(&different.to_string()))
            .unwrap(),
    )];
    for report in [
        compare_policy(&a, &b).unwrap(),
        compare_argv(&a, &b, &context(), &context()).unwrap(),
    ] {
        assert!(report.complete);
        assert_eq!(report.relation, Relation::Expansive);
        assert_eq!(
            report.expansion.as_ref().unwrap().request.arguments,
            ["push", "origin", "HEAD"]
        );
        assert_eq!(
            serde_json::to_value(report).unwrap()["algorithm"],
            "argv-symbolic-possible-effects-bound-v1"
        );
    }
}

#[test]
fn semantic_profile_changes_have_universal_bounds_and_real_shared_resolver_witnesses() {
    let pack = json!({"schema_version":1,"id":"acme/effects","version":"0.1.0","description":"Acme effects","license":"MIT","maintainers":["Acme"],"engines":{"shield":env!("CARGO_PKG_VERSION")},"capabilities":["argument-rules-v1","possible-effects-v1","structured-rules-v1"],"rules":[{"id":"network","reason":"Acme network","conditions":{"domain":"argv","executable":"git","arguments":{"kind":"any"},"possible_effects":["network"]},"decision":{"loose":"allow","recommended":"ask","strict":"deny"}}],"kind":"pack"});
    let capture = |profile: &str, custom: Option<Value>| {
        let config = json!({"schema_version":1,"packs":[{"id":"acme/effects","version":"0.1.0","profile":profile,"path":"pack.json"}],"rules":"rules.json"});
        let custom = custom
            .unwrap_or_else(|| json!({"schema_version":1,"rules":[],"kind":"custom","capabilities":["structured-rules-v1"]}))
            .to_string();
        [(
            Scope::Project,
            Candidate::from_yaml(&config.to_string())
                .unwrap()
                .capture(&[pack.to_string()], Some(&custom))
                .unwrap(),
        )]
    };
    let loose = capture("loose", None);
    let recommended = capture("recommended", None);
    let strict = capture("strict", None);
    for fixed in [false, true] {
        let compare = |a: &[(Scope, Snapshot)], b: &[(Scope, Snapshot)]| {
            if fixed {
                compare_argv(a, b, &context(), &context()).unwrap()
            } else {
                compare_policy(a, b).unwrap()
            }
        };
        let equivalent = compare(&loose, &recommended);
        assert!(equivalent.complete);
        assert_eq!(equivalent.relation, Relation::Equivalent);
        let restrictive = compare(&recommended, &strict);
        assert!(restrictive.complete);
        assert_eq!(restrictive.relation, Relation::Restrictive);
        let witness = restrictive.restriction.unwrap();
        assert_eq!(witness.request.arguments, ["push", "origin", "HEAD"]);
        assert_eq!(witness.before.decision, Decision::Ask);
        assert_eq!(witness.after.decision, Decision::Deny);
        assert_eq!(compare(&strict, &recommended).relation, Relation::Expansive);
    }
    let custom = json!({"schema_version":1,"rules":[{"id":"override","decision":"deny","reason":"Acme override","conditions":{"domain":"argv","executable":"git","arguments":{"kind":"any"}}}],"kind":"custom","capabilities":["argument-rules-v1","structured-rules-v1"]});
    let before = capture("recommended", Some(custom.clone()));
    let after = capture("strict", Some(custom));
    let shadowed = result(&before, &after);
    assert!(!shadowed.complete);
    assert_eq!(shadowed.relation, Relation::Unknown);
    let encoded = serde_json::to_value(shadowed).unwrap();
    assert_eq!(encoded["decision_bound"], "non-expansive");
    assert_eq!(encoded["incomplete_reason"], "strict-witness-not-found");
}

fn effect_snapshot(
    tool: &str,
    kind: &str,
    values: &[&str],
    effects: &[&str],
    decision: &str,
    reason: &str,
) -> [(Scope, Snapshot); 1] {
    let arguments = if kind == "any" {
        json!({"kind":kind})
    } else {
        json!({"kind":kind,"values":values})
    };
    let doc = json!({"schema_version":1,"rules":[{"id":"operation","decision":decision,"reason":reason,"conditions":{"domain":"argv","executable":tool,"arguments":arguments,"possible_effects":effects}}],"kind":"custom","capabilities":["argument-rules-v1","possible-effects-v1","structured-rules-v1"]});
    let candidate =
        Candidate::from_yaml("schema_version: 1\npacks: []\nrules: rules.yml\n").unwrap();
    [(
        Scope::Project,
        candidate.capture(&[], Some(&doc.to_string())).unwrap(),
    )]
}

#[test]
fn changed_effect_artifacts_and_prefixes_have_symbolic_bounds_but_only_real_witnesses() {
    let before = effect_snapshot("git", "any", &[], &["network"], "deny", "Acme before");
    let equivalent = effect_snapshot(
        "git",
        "any",
        &[],
        &["network"],
        "deny",
        "Acme changed reason",
    );
    let narrower = effect_snapshot(
        "git",
        "prefix",
        &["push"],
        &["network"],
        "deny",
        "Acme narrower",
    );
    for report in [
        result(&before, &equivalent),
        compare_policy(&before, &equivalent).unwrap(),
    ] {
        assert!(report.complete);
        assert_eq!(report.relation, Relation::Equivalent);
        assert!(report.symbolic_regions.unwrap() > 0);
        assert_eq!(report.evaluated_regions, 0);
        assert!(report.expansion.is_none() && report.restriction.is_none());
    }
    for report in [
        result(&before, &narrower),
        compare_policy(&before, &narrower).unwrap(),
    ] {
        assert!(report.complete);
        assert_eq!(report.relation, Relation::Expansive);
        let witness = report.expansion.unwrap();
        assert_eq!(
            Prepared::new(&before)
                .unwrap()
                .policy(&witness.request)
                .unwrap()
                .decision,
            Decision::Deny
        );
        assert_eq!(
            Prepared::new(&narrower)
                .unwrap()
                .policy(&witness.request)
                .unwrap()
                .decision,
            Decision::Ask
        );
        assert!(
            report.symbolic_regions.unwrap() + report.evaluated_regions <= partition::MAX_REGIONS
        );
    }
    assert_eq!(result(&narrower, &before).relation, Relation::Restrictive);
    let opaque_before = effect_snapshot(
        "acme",
        "any",
        &[],
        &["network"],
        "ask",
        "Acme opaque before",
    );
    let opaque_after = effect_snapshot(
        "acme",
        "any",
        &[],
        &["network"],
        "deny",
        "Acme opaque after",
    );
    let report = result(&opaque_before, &opaque_after);
    assert!(!report.complete);
    assert_eq!(report.relation, Relation::Unknown);
    assert!(report.restriction.is_none());
    assert_eq!(
        report.incomplete_reason,
        Some(partition::Incomplete::StrictWitnessNotFound)
    );
}

#[test]
fn symbolic_effect_proof_budget_and_mixed_overapproximations_never_certify_a_relation() {
    let all = [
        "repository-write",
        "worktree-write",
        "data-removal",
        "history-rewrite",
        "network",
        "code-execution",
        "daemon-write",
        "image-build",
        "host-exposure",
        "privileged-runtime",
        "configuration",
    ];
    let before = effect_snapshot("git", "any", &[], &all, "ask", "Acme before");
    let after = effect_snapshot("git", "any", &[], &all, "deny", "Acme after");
    let report = result(&before, &after);
    assert!(!report.complete);
    assert_eq!(report.relation, Relation::Unknown);
    assert_eq!(
        report.incomplete_reason,
        Some(partition::Incomplete::EvaluationBudget)
    );
    let network = effect_snapshot("git", "any", &[], &["network"], "deny", "Acme network");
    let repository = effect_snapshot(
        "git",
        "any",
        &[],
        &["repository-write"],
        "deny",
        "Acme repository",
    );
    let report = result(&network, &repository);
    assert!(!report.complete);
    assert_eq!(report.relation, Relation::Unknown);
    assert_eq!(
        report.incomplete_reason,
        Some(partition::Incomplete::UnsupportedSemanticDomain)
    );
}

proptest::proptest! {
    #![proptest_config(proptest::test_runner::Config::with_cases(32))]
    #[test]
    fn symbolic_bounds_cover_real_classifier_requests_with_project_policy(
        bits in 1u8..8,
        tool in proptest::sample::select(vec!["git", "docker"]),
        prefix in proptest::sample::select(vec!["push", "status", "container", "-C"]),
        loosen in proptest::bool::ANY,
    ) {
        let effects: Vec<_> = ["network", "repository-write", "code-execution"].into_iter().enumerate()
            .filter_map(|(bit, effect)| ((bits & (1 << bit)) != 0).then_some(effect)).collect();
        let before: Vec<_> = effect_snapshot(tool, "any", &[], &effects, "deny", "Acme before").into_iter().collect();
        let after: Vec<_> = effect_snapshot(tool, "prefix", &[prefix], &effects,
            if loosen { "ask" } else { "deny" }, "Acme after").into_iter().collect();
        let report = compare_policy(&before, &after).unwrap();
        let first = Prepared::new(&before).unwrap();
        let second = Prepared::new(&after).unwrap();
        for arguments in [vec![], vec!["push"], vec!["status"], vec!["config", "--local", "acme.value", "Acme"], vec!["run", "--privileged", "acme"], vec!["container", "exec", "acme", "printf", "Acme"], vec!["--unknown", "push"], vec!["-C", "push", "status"]] {
            let request = Request { executable: tool.into(), arguments: arguments.into_iter().map(str::to_owned).collect(), cwd: context(), project: context() };
            let previous = first.policy(&request).unwrap().decision;
            let next = second.policy(&request).unwrap().decision;
            match report.decision_bound {
                Some("equivalent") => proptest::prop_assert_eq!(next, previous),
                Some("non-expansive") => proptest::prop_assert!(next >= previous),
                Some("non-restrictive") => proptest::prop_assert!(next <= previous),
                None => {},
                _ => unreachable!(),
            }
        }
        proptest::prop_assert!(report.symbolic_regions.unwrap_or_default() + report.evaluated_regions <= partition::MAX_REGIONS);
        for witness in [&report.expansion, &report.restriction].into_iter().flatten() {
            proptest::prop_assert_eq!(first.policy(&witness.request).unwrap().decision, witness.before.decision);
            proptest::prop_assert_eq!(second.policy(&witness.request).unwrap().decision, witness.after.decision);
        }
    }
}

#[test]
fn exact_effect_predicates_partition_singletons_and_keep_semantic_floor() {
    let candidate =
        Candidate::from_yaml("schema_version: 1\npacks: []\nrules: rules.yml\n").unwrap();
    let capture = |effects: &str, decision: &str| {
        let doc = json!({"schema_version":1,"rules":[{"id":"operation","decision":decision,"reason":"Acme operation","conditions":{"domain":"argv","executable":"git","arguments":{"kind":"exact","values":["push"]},"possible_effects":[effects]}}],"kind":"custom","capabilities":["argument-rules-v1","possible-effects-v1","structured-rules-v1"]});
        [(
            Scope::Project,
            candidate.capture(&[], Some(&doc.to_string())).unwrap(),
        )]
    };
    let network = capture("network", "deny");
    let absent = capture("host-exposure", "deny");
    let allow = capture("network", "allow");
    for fixed in [false, true] {
        let compare = |before: &[(Scope, Snapshot)], after: &[(Scope, Snapshot)]| {
            if fixed {
                compare_argv(before, after, &context(), &context()).unwrap()
            } else {
                compare_policy(before, after).unwrap()
            }
        };
        let report = compare(&network, &network);
        assert!(report.complete);
        assert_eq!(report.relation, Relation::Equivalent);
        let value = serde_json::to_value(report).unwrap();
        assert_eq!(value["schema_version"], 2);
        assert!(
            value["coverage"]
                .as_str()
                .unwrap()
                .contains("exact-possible-effects")
        );
        let report = compare(&network, &absent);
        assert!(report.complete);
        assert_eq!(report.relation, Relation::Expansive);
        let witness = report.expansion.unwrap();
        assert_eq!(witness.request.arguments, ["push"]);
        assert_eq!(witness.before.decision, Decision::Deny);
        assert_eq!(witness.after.decision, Decision::Ask);
        assert_eq!(compare(&absent, &network).relation, Relation::Restrictive);
        assert_eq!(compare(&absent, &allow).relation, Relation::Equivalent);
    }
}

#[test]
fn filesystem_context_comparison_remains_explicitly_incomplete() {
    let mut local = rule("local", "exact", &["write"], Decision::Allow);
    local["conditions"]["cwd"] = json!({"exact":{"root":"absolute","path":context()}});
    let before = [(
        Scope::Project,
        snapshot(
            Scope::Project,
            json!([rule("broad", "any", &[], Decision::Ask)]),
        ),
    )];
    let after = [(
        Scope::Project,
        snapshot(
            Scope::Project,
            json!([rule("broad", "any", &[], Decision::Deny), local]),
        ),
    )];
    for report in [
        compare_policy(&before, &after).unwrap(),
        result(&before, &after),
    ] {
        assert!(!report.complete);
        assert_eq!(report.relation, Relation::Unknown);
        assert_eq!(
            report.incomplete_reason,
            Some(partition::Incomplete::UnsupportedSemanticDomain)
        );
        assert!(report.expansion.is_none() && report.restriction.is_none());
    }
}

#[test]
fn all_context_equivalence_and_one_way_changes_include_unmatched_contexts() {
    let before = [(Scope::Project, snapshot(Scope::Project, json!([])))];
    let local = rule("local", "exact", &["write"], Decision::Deny);
    let after = [(Scope::Project, snapshot(Scope::Project, json!([local])))];
    assert_eq!(
        compare_policy(&before, &before).unwrap().relation,
        Relation::Equivalent
    );
    assert_eq!(
        compare_policy(&before, &after).unwrap().relation,
        Relation::Restrictive
    );
    assert_eq!(
        compare_policy(&after, &before).unwrap().relation,
        Relation::Expansive
    );
}

#[test]
fn adding_only_a_directory_option_is_never_reported_equivalent() {
    let rows = [rule("install", "exact", &["install"], Decision::Allow)];
    let captured = |options: &[Value]| {
        let document = crate::policy::rules::document_with(&rows, options).unwrap();
        let config =
            Candidate::from_yaml("schema_version: 1\npacks: []\nrules: rules.yml\n").unwrap();
        [(
            Scope::Project,
            config.capture(&[], Some(&document.to_string())).unwrap(),
        )]
    };
    let before = captured(&[]);
    let after = captured(&[
        json!({"executable":"acme","option":"--prefix","position":"anywhere","paths":"follow"}),
    ]);
    for report in [
        compare_policy(&before, &after).unwrap(),
        result(&before, &after),
    ] {
        assert!(!report.complete);
        assert_eq!(report.relation, Relation::Unknown);
        assert_eq!(
            report.incomplete_reason,
            Some(partition::Incomplete::DirectoryOptions)
        );
    }
    // The command the option rewrites is decided as enforcement decides it.
    let request = Request {
        executable: "acme".into(),
        arguments: vec!["--prefix".into(), context(), "install".into()],
        cwd: context(),
        project: context(),
    };
    let widened = compare(&before, &after, &request).unwrap();
    assert_eq!(widened.change, crate::configuration::Change::Expansion);
    assert_eq!(widened.after.decision, Decision::Allow);
    // Identical policies keep their proof.
    let same = result(&after, &after);
    assert!(same.complete);
    assert_eq!(same.relation, Relation::Equivalent);
}

#[test]
fn complete_relations_find_changes_missed_by_an_unchanged_selected_sample() {
    let before = [(
        Scope::Project,
        snapshot(
            Scope::Project,
            json!([
                rule("write", "exact", &["write"], Decision::Deny),
                rule("read", "exact", &["read"], Decision::Allow)
            ]),
        ),
    )];
    let after = [(
        Scope::Project,
        snapshot(
            Scope::Project,
            json!([
                rule("write", "exact", &["write"], Decision::Allow),
                rule("read", "exact", &["read"], Decision::Deny)
            ]),
        ),
    )];
    let mixed = result(&before, &after);
    assert!(mixed.complete);
    assert_eq!(mixed.relation, Relation::Mixed);
    assert_eq!(mixed.expansion.unwrap().request.arguments, ["write"]);
    assert_eq!(mixed.restriction.unwrap().request.arguments, ["read"]);
    let equivalent = result(&before, &before);
    assert!(equivalent.complete);
    assert_eq!(equivalent.relation, Relation::Equivalent);
    assert!(equivalent.expansion.is_none() && equivalent.restriction.is_none());
    let empty = [(Scope::Project, snapshot(Scope::Project, json!([])))];
    let deny = [(
        Scope::Project,
        snapshot(
            Scope::Project,
            json!([rule("write", "exact", &["write"], Decision::Deny)]),
        ),
    )];
    assert_eq!(result(&empty, &deny).relation, Relation::Restrictive);
    assert_eq!(result(&deny, &empty).relation, Relation::Expansive);
}

#[test]
fn prefix_exceptions_use_the_shared_resolver() {
    let before = [(
        Scope::Project,
        snapshot(
            Scope::Project,
            json!([
                rule("broad", "any", &[], Decision::Deny),
                rule("prefix", "prefix", &["write"], Decision::Ask)
            ]),
        ),
    )];
    let exact = rule("exact", "exact", &["write", "local"], Decision::Allow);
    let after = [(
        Scope::Project,
        snapshot(
            Scope::Project,
            json!([
                rule("broad", "any", &[], Decision::Deny),
                rule("prefix", "prefix", &["write"], Decision::Ask),
                exact
            ]),
        ),
    )];
    let alternate = format!("{}/other", context());
    let unrestricted = &after[..1];
    let changed = result(&before, unrestricted);
    assert_eq!(changed.relation, Relation::Expansive);
    let witness = changed.expansion.unwrap();
    assert_eq!(witness.request.arguments, ["write", "local"]);
    assert_eq!(witness.before.decision, Decision::Ask);
    assert_eq!(witness.after.decision, Decision::Allow);
    assert_eq!(
        compare_argv(&before, unrestricted, &alternate, &context())
            .unwrap()
            .relation,
        Relation::Expansive
    );
}

#[test]
fn repeated_calls_are_deterministic_and_every_context_is_bound() {
    let inputs = vec![(
        Scope::Project,
        snapshot(
            Scope::Project,
            json!([rule("operation", "any", &[], Decision::Allow)]),
        ),
    )];
    let original = result(&inputs, &inputs);
    let repeated = result(&inputs, &inputs);
    assert_eq!(
        serde_json::to_value(&original).unwrap(),
        serde_json::to_value(&repeated).unwrap()
    );
    let changed = compare_argv(
        &inputs,
        &inputs,
        &format!("{}/other", context()),
        &context(),
    )
    .unwrap();
    assert_ne!(original.revision, changed.revision);
    assert_eq!(original.relation, changed.relation);
    assert!(compare_argv(&inputs, &inputs, "relative", &context()).is_err());
    assert!(compare_argv(&[], &inputs, &context(), &context()).is_err());
    let encoded = serde_json::to_value(&original).unwrap();
    assert_eq!(encoded["trust"], "unverified");
    assert_eq!(encoded["coverage"], "argv-v1-fixed-context");
    let mut canonical = original;
    canonical.revision.clear();
    assert_eq!(
        encoded["revision"],
        policy::fingerprint(&serde_json::to_vec(&canonical).unwrap())
    );
}

#[test]
fn bounded_partial_comparison_retains_witnesses_and_never_claims_equivalence() {
    let rules = json!(
        (0..1024)
            .map(|i| rule(
                &format!("rule-{i}"),
                "exact",
                &[&format!("operation-{i}")],
                Decision::Deny
            ))
            .collect::<Vec<_>>()
    );
    let before = [(Scope::Project, snapshot(Scope::Project, rules.clone()))];
    let mut loosened = rules;
    loosened[0]["decision"] = json!("allow");
    let after = [(Scope::Project, snapshot(Scope::Project, loosened))];
    let partial = result(&before, &after);
    assert!(!partial.complete);
    assert_eq!(partial.relation, Relation::Unknown);
    assert!(partial.expansion.is_some());
    assert!(partial.restriction.is_none());
    assert_eq!(
        partial.incomplete_reason,
        Some(partition::Incomplete::EvaluationBudget)
    );
    let identical = result(&before, &before);
    assert!(!identical.complete);
    assert_eq!(identical.relation, Relation::Unknown);
    let all = compare_policy(&before, &after).unwrap();
    assert!(!all.complete);
    assert_eq!(all.relation, Relation::Unknown);
    assert!(all.expansion.is_some());
}
