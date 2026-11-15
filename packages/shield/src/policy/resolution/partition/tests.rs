//! Checks truth-vector coverage and conservative resource/valid-request boundaries.

use super::*;
use proptest::prelude::*;
use serde_json::json;
use std::collections::BTreeSet;

fn context() -> String {
    std::env::current_dir().unwrap().to_str().unwrap().into()
}

fn matcher(executable: &str, kind: &str, values: &[String]) -> Matcher {
    let arguments = if kind == "any" {
        json!({"kind":kind})
    } else {
        json!({"kind":kind,"values":values})
    };
    serde_json::from_value(json!({"executable":executable,"arguments":arguments})).unwrap()
}

fn signature(matchers: &[&Matcher], request: &Request) -> Vec<bool> {
    matchers
        .iter()
        .map(|matcher| matcher.matches(request))
        .collect()
}

proptest! {
    #[test]
    fn all_context_partition_covers_arbitrary_context_and_argv_vectors(
        patterns in prop::collection::vec((0u8..3, 0u8..3, 0u8..3, prop::collection::vec("[ab]{0,2}", 0..3)), 0..10),
        cwd_index in 0u8..4, project_index in 0u8..4,
        arguments in prop::collection::vec("[a-zλ ]{0,8}", 0..5),
    ) {
        let paths: Vec<_> = (0..4).map(|i| format!("{}acme-{i}", absolute_root())).collect();
        let owned: Vec<Matcher> = patterns.iter().map(|(cwd, project, kind, values)| {
            let kind = match kind { 0 => "exact", 1 if !values.is_empty() => "prefix", _ => "any" };
            let mut value = serde_json::to_value(json!({"executable":"acme","arguments":if kind == "any" {
                json!({"kind":kind})
            } else {json!({"kind":kind,"values":values})}})).unwrap();
            if *cwd != 0 { value["cwd"] = json!(paths[usize::from(*cwd)]); }
            if *project != 0 { value["project"] = json!(paths[usize::from(*project)]); }
            serde_json::from_value(value).unwrap()
        }).collect();
        let matchers: Vec<_> = owned.iter().collect();
        let result = build_all_matchers(&matchers).unwrap();
        prop_assert!(result.incomplete.is_none());
        let request = Request { executable:"acme".into(), arguments,
            cwd:paths[usize::from(cwd_index)].clone(), project:paths[usize::from(project_index)].clone() };
        prop_assert!(result.requests.iter().any(|representative|
            signature(&matchers, representative) == signature(&matchers, &request)));
    }

    #[test]
    fn partition_covers_matching_vectors_for_arbitrary_literal_requests(
        patterns in prop::collection::vec(
            (0u8..3, prop::collection::vec(prop_oneof![Just(String::new()), "[ab]{1,2}", Just("λ".into()), Just("shield-unmatched-0".into())], 0..4)), 0..12),
        arguments in prop::collection::vec("[a-zλ ]{0,16}", 0..8),
        known_executable in any::<bool>(),
    ) {
        let owned: Vec<_> = patterns.iter().map(|(kind, values)| {
            let kind = match kind { 0 => "exact", 1 if !values.is_empty() => "prefix", _ => "any" };
            matcher("acme", kind, values)
        }).collect();
        let matchers: Vec<_> = owned.iter().collect();
        let context = context();
        let result = build_matchers(&matchers, &context, &context).unwrap();
        prop_assert!(result.incomplete.is_none());
        let request = Request { executable: if known_executable { "acme" } else { "other" }.into(),
            arguments, cwd: context.clone(), project: context };
        prop_assert!(result.requests.iter().any(|representative|
            signature(&matchers, representative) == signature(&matchers, &request)));
    }
}

#[test]
fn context_cartesian_product_uses_one_global_budget() {
    let owned: Vec<Matcher> = (0..60).map(|i| serde_json::from_value(json!({
        "executable":"acme", "arguments":{"kind":"exact","values":["write"]},
        "cwd":format!("{}cwd-{i}", absolute_root()), "project":format!("{}project-{i}", absolute_root())
    })).unwrap()).collect();
    let result = build_all_matchers(&owned.iter().collect::<Vec<_>>()).unwrap();
    assert_eq!(result.incomplete, Some(Incomplete::EvaluationBudget));
    assert_eq!(result.requests.len(), MAX_REGIONS);
}

#[test]
fn context_selector_construction_is_bounded_before_any_sweep() {
    for (count, padding) in [(4096, 0), (1100, 4080)] {
        let owned: Vec<Matcher> = (0..count)
            .map(|i| {
                serde_json::from_value(json!({
                    "executable":"acme", "arguments":{"kind":"any"},
                    "cwd":format!("{}cwd-{i}{}", absolute_root(), "a".repeat(padding)),
                    "project":format!("{}project-{i}{}", absolute_root(), "a".repeat(padding))
                }))
                .unwrap()
            })
            .collect();
        let result = build_all_matchers(&owned.iter().collect::<Vec<_>>()).unwrap();
        assert_eq!(result.incomplete, Some(Incomplete::PartitionBudget));
        assert!(result.requests.is_empty());
    }
}

#[test]
fn residual_context_representatives_avoid_declared_root_and_sentinel_values() {
    let root = absolute_root();
    let sentinel = format!("{root}shield-unmatched-0");
    let owned: Vec<Matcher> = [root, sentinel.as_str()]
        .iter()
        .map(|path| {
            serde_json::from_value(json!({
                "executable":"acme","arguments":{"kind":"any"},"cwd":path,"project":path
            }))
            .unwrap()
        })
        .collect();
    let matchers: Vec<_> = owned.iter().collect();
    let result = build_all_matchers(&matchers).unwrap();
    assert!(result.incomplete.is_none());
    let residual = result.requests.last().unwrap();
    assert_ne!(residual.cwd, root);
    assert_ne!(residual.cwd, sentinel);
    assert_eq!(residual.cwd, residual.project);
    assert_eq!(signature(&matchers, residual), [false, false]);
}

#[test]
fn residual_context_byte_boundaries_never_prune_potentially_reachable_classes() {
    let root = absolute_root();
    let mut remaining = 65_536 - "acme".len() - 2 * root.len() - 30;
    let mut arguments = Vec::new();
    while remaining > 0 {
        let bytes = remaining.min(4096);
        arguments.push("a".repeat(bytes));
        remaining -= bytes;
    }
    let exact = matcher("acme", "exact", &arguments);
    let selector: Matcher = serde_json::from_value(json!({
        "executable":"context", "arguments":{"kind":"any"},"cwd":root,"project":root
    }))
    .unwrap();
    let result = build_all_matchers(&[&exact, &selector]).unwrap();
    assert_eq!(result.incomplete, Some(Incomplete::RequestLimitBoundary));
    let reachable = Request {
        executable: "acme".into(),
        arguments,
        cwd: format!("{root}x"),
        project: format!("{root}y"),
    };
    assert!(reachable.validate().is_ok());
    assert!(exact.matches(&reachable));
}

#[test]
fn termination_prefixes_empty_tokens_and_fresh_collisions_are_covered() {
    let owned = [
        matcher("acme", "exact", &[]),
        matcher("acme", "exact", &["a".into()]),
        matcher("acme", "prefix", &["a".into(), "b".into()]),
        matcher("acme", "exact", &[String::new()]),
        matcher("acme", "exact", &["shield-unmatched-0".into()]),
        matcher("shield-unmatched-0", "any", &[]),
    ];
    let matchers: Vec<_> = owned.iter().collect();
    let context = context();
    let result = build_matchers(&matchers, &context, &context).unwrap();
    assert!(result.incomplete.is_none());
    let vectors: BTreeSet<_> = result
        .requests
        .iter()
        .map(|r| signature(&matchers, r))
        .collect();
    let tokens = ["", "a", "b", "shield-unmatched-0", "λ"];
    let mut argv = vec![Vec::new()];
    for _ in 0..4 {
        let previous = argv.clone();
        argv.extend(previous.into_iter().flat_map(|values| {
            tokens.iter().map(move |token| {
                let mut next = values.clone();
                next.push((*token).into());
                next
            })
        }));
    }
    for executable in ["acme", "shield-unmatched-0", "other"] {
        for arguments in &argv {
            let request = Request {
                executable: executable.into(),
                arguments: arguments.clone(),
                cwd: context.clone(),
                project: context.clone(),
            };
            assert!(vectors.contains(&signature(&matchers, &request)));
        }
    }
}

#[test]
fn inactive_context_rules_do_not_create_boundaries_but_invalid_context_is_rejected() {
    let rule: Matcher = serde_json::from_value(json!({
        "executable":"acme", "arguments":{"kind":"exact","values":["write"]},
        "cwd":format!("{}/other", context())
    }))
    .unwrap();
    let context = context();
    let result = build_matchers(&[&rule], &context, &context).unwrap();
    assert!(result.incomplete.is_none());
    assert_eq!(result.requests.len(), 1);
    assert!(build_matchers(&[], "relative", &context).is_err());
}

#[test]
fn node_byte_and_evaluation_budgets_are_explicit_incomplete_results() {
    let context = context();
    let owned: Vec<_> = (0..40)
        .map(|index| matcher(&format!("acme-{index}"), "exact", &vec!["a".into(); 256]))
        .collect();
    let result = build_matchers(&owned.iter().collect::<Vec<_>>(), &context, &context).unwrap();
    assert_eq!(result.incomplete, Some(Incomplete::PartitionBudget));
    assert!(result.requests.is_empty());
    let large: Vec<_> = (0..10)
        .map(|index| {
            matcher(
                &format!("acme-{index}"),
                "exact",
                &vec!["a".repeat(4096); 256],
            )
        })
        .collect();
    assert_eq!(
        build_matchers(&large.iter().collect::<Vec<_>>(), &context, &context)
            .unwrap()
            .incomplete,
        Some(Incomplete::PartitionBudget)
    );
    let many: Vec<_> = (0..1500)
        .map(|index| matcher("acme", "exact", &[format!("operation-{index}")]))
        .collect();
    let result = build_matchers(&many.iter().collect::<Vec<_>>(), &context, &context).unwrap();
    assert_eq!(result.incomplete, Some(Incomplete::EvaluationBudget));
    assert_eq!(result.requests.len(), MAX_RULE_CHECKS / many.len());
    let deep: Vec<_> = (0..9)
        .map(|index| matcher(&format!("acme-{index}"), "prefix", &vec!["a".into(); 256]))
        .collect();
    let result = build_matchers(&deep.iter().collect::<Vec<_>>(), &context, &context).unwrap();
    assert_eq!(result.incomplete, Some(Incomplete::EvaluationBudget));
    assert_eq!(result.requests.len(), MAX_REGIONS);
    let heavy: Vec<_> = (0..9)
        .map(|index| {
            matcher(
                &format!("acme-{index}"),
                "prefix",
                &vec!["a".repeat(4096); 15],
            )
        })
        .collect();
    let result = build_matchers(&heavy.iter().collect::<Vec<_>>(), &context, &context).unwrap();
    assert_eq!(result.incomplete, Some(Incomplete::PartitionBudget));
    assert!(!result.requests.is_empty());
}

#[test]
fn request_byte_boundaries_cannot_turn_an_invalid_representative_into_a_proof() {
    let context = context();
    let mut remaining = 65_535 - "acme".len() - 2 * context.len();
    let mut arguments = Vec::new();
    while remaining > 0 {
        let bytes = remaining.min(4096);
        arguments.push("a".repeat(bytes));
        remaining -= bytes;
    }
    let prefix = matcher("acme", "prefix", &arguments);
    arguments.push(String::new());
    let exact = matcher("acme", "exact", &arguments);
    let result = build_matchers(&[&prefix, &exact], &context, &context).unwrap();
    assert_eq!(result.incomplete, Some(Incomplete::RequestLimitBoundary));
    let mut reachable = result.requests.last().unwrap().clone();
    reachable.arguments.push("x".into());
    assert!(reachable.validate().is_ok());
}

#[test]
fn impossible_overlong_subtrees_are_pruned_and_maximum_argument_termination_is_valid() {
    let context = context();
    let impossible = matcher("acme", "exact", &vec!["a".repeat(4096); 17]);
    let result = build_matchers(&[&impossible], &context, &context).unwrap();
    assert!(result.incomplete.is_none());
    assert!(
        result
            .requests
            .iter()
            .all(|request| !impossible.matches(request))
    );
    let boundary = matcher("acme", "prefix", &vec![String::new(); 256]);
    let result = build_matchers(&[&boundary], &context, &context).unwrap();
    assert!(result.incomplete.is_none());
    assert!(
        result
            .requests
            .iter()
            .any(|request| request.arguments.len() == 256 && boundary.matches(request))
    );
    assert!(
        result
            .requests
            .iter()
            .all(|request| request.arguments.len() <= 256)
    );
}
