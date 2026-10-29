//! Exercises closure choices, source isolation, profile propagation and refusal
//! through the shared resolver with profiles as its level.
use super::*;
use resolution::{Error, Selection, resolve};

fn id(pack: &str) -> Instance {
    Instance {
        registry: "acme-source".into(),
        pack: format!("acme/{pack}").parse().expect("pack"),
    }
}
fn root(pack: &str, requirement: &str, profile: Profile) -> Root<Profile> {
    Root {
        instance: id(pack),
        requirement: requirement.parse().expect("requirement"),
        pin: None,
        level: profile,
    }
}
fn release(pack: &str, version: &str, dependencies: Vec<Dependency<Profile>>) -> Release<Profile> {
    Release {
        instance: id(pack),
        version: version.parse().expect("version"),
        dependencies,
    }
}
fn edge(pack: &str, requirement: &str) -> Dependency<Profile> {
    Dependency {
        pack: id(pack).pack,
        registry: None,
        requirement: requirement.parse().expect("requirement"),
        levels: None,
    }
}
fn enrolled() -> BTreeSet<String> {
    BTreeSet::from(["acme-source".into()])
}
fn summary(result: &[Selection<'_, Profile>]) -> Vec<(String, String, Profile, bool)> {
    result
        .iter()
        .map(|s| {
            (
                s.release.instance.pack.to_string(),
                s.release.version.to_string(),
                s.level,
                s.direct,
            )
        })
        .collect()
}

#[test]
fn profiles_are_ordered_levels_and_declarations_map_each_parent_profile() {
    assert!(Profile::Loose < Profile::Recommended && Profile::Recommended < Profile::Strict);
    let declaration: Declaration = serde_json::from_value(serde_json::json!({
        "pack":"acme/c","requirement":"^1","registry":"foreign",
        "profiles":{"loose":"recommended","recommended":"strict","strict":"strict"}
    }))
    .expect("declaration");
    let parsed = declaration.parsed().expect("dependency");
    assert_eq!(
        parsed.levels,
        Some(vec![Profile::Recommended, Profile::Strict, Profile::Strict])
    );
    assert_eq!(parsed.registry.as_deref(), Some("foreign"));
    for (field, value) in [
        ("pack", serde_json::json!("acme")),
        ("requirement", serde_json::json!("not a range")),
        ("requirement", serde_json::json!("1".repeat(4097))),
        (
            "profiles",
            serde_json::json!({"loose":"strict","recommended":"loose","strict":"strict"}),
        ),
    ] {
        let mut altered = serde_json::to_value(&declaration).expect("encoded");
        altered[field] = value;
        let altered: Declaration = serde_json::from_value(altered).expect("declaration");
        assert_eq!(altered.parsed().expect_err(field), Error::Invalid);
    }
}

#[test]
fn highest_compatible_closure_backtracks_and_is_order_independent() {
    let roots = vec![
        root("a", "*", Profile::Recommended),
        root("b", "*", Profile::Strict),
    ];
    let mut catalog = vec![
        release("a", "2.0.0", vec![edge("c", "^2")]),
        release("a", "1.0.0", vec![edge("c", "^1")]),
        release("b", "1.0.0", vec![edge("c", "^1")]),
        release("c", "1.1.0", vec![]),
        release("c", "1.0.0", vec![]),
        release("c", "2.0.0", vec![]),
        release("unused", "9.0.0", vec![]),
    ];
    let expected = vec![
        ("acme/a".into(), "1.0.0".into(), Profile::Recommended, true),
        ("acme/b".into(), "1.0.0".into(), Profile::Strict, true),
        ("acme/c".into(), "1.1.0".into(), Profile::Strict, false),
    ];
    assert_eq!(
        summary(&resolve(&roots, &catalog, &enrolled()).expect("closure")),
        expected
    );
    catalog.reverse();
    let reversed: Vec<_> = roots.into_iter().rev().collect();
    assert_eq!(
        summary(&resolve(&reversed, &catalog, &enrolled()).expect("closure")),
        expected
    );
}

#[test]
fn pins_and_conflicting_constraints_refuse_without_partial_results() {
    let mut roots = vec![
        root("a", "*", Profile::Recommended),
        root("c", "^1", Profile::Recommended),
    ];
    roots[0].pin = Some("2.0.0".parse().expect("pin"));
    let catalog = vec![
        release("a", "2.0.0", vec![edge("c", "^2")]),
        release("a", "1.0.0", vec![edge("c", "^1")]),
        release("c", "1.0.0", vec![]),
        release("c", "2.0.0", vec![]),
    ];
    assert_eq!(
        resolve(&roots, &catalog, &enrolled()).expect_err("pin conflict"),
        Error::Conflict
    );
    roots[0].pin = None;
    assert_eq!(
        resolve(&roots, &catalog, &enrolled()).expect("unpinned")[0]
            .release
            .version
            .to_string(),
        "1.0.0"
    );
}

#[test]
fn profiles_propagate_after_all_parents_and_preserve_explicit_roots() {
    let roots = vec![
        root("a", "*", Profile::Strict),
        root("b", "*", Profile::Loose),
    ];
    let mut mapping = edge("c", "*");
    mapping.levels = Some(vec![Profile::Recommended, Profile::Strict, Profile::Strict]);
    let catalog = vec![
        release("a", "1.0.0", vec![edge("b", "*")]),
        release("b", "1.0.0", vec![mapping]),
        release("c", "1.0.0", vec![edge("d", "*")]),
        release("d", "1.0.0", vec![]),
    ];
    assert_eq!(
        resolve(&roots, &catalog, &enrolled()).expect_err("explicit weak b"),
        Error::LevelConflict(id("b"))
    );
    let result = resolve(&roots[..1], &catalog, &enrolled()).expect("inherited closure");
    assert!(result.iter().all(|s| s.level == Profile::Strict));
    let loose = vec![root("b", "*", Profile::Loose)];
    let result = resolve(&loose, &catalog, &enrolled()).expect("mapped");
    assert_eq!(result[1].level, Profile::Recommended);
    assert_eq!(result[2].level, Profile::Recommended);
}

#[test]
fn cycles_and_self_cycles_refuse_but_unused_cycles_do_not() {
    let roots = vec![root("a", "*", Profile::Recommended)];
    for edges in [vec![edge("a", "*")], vec![edge("b", "*")]] {
        let catalog = vec![
            release("a", "1.0.0", edges),
            release("b", "1.0.0", vec![edge("a", "*")]),
        ];
        assert_eq!(
            resolve(&roots, &catalog, &enrolled()).expect_err("cycle"),
            Error::Cycle
        );
    }
    let catalog = vec![
        release("a", "1.0.0", vec![]),
        release("b", "1.0.0", vec![edge("b", "*")]),
    ];
    assert_eq!(
        resolve(&roots, &catalog, &enrolled())
            .expect("unused cycle")
            .len(),
        1
    );
}

#[test]
fn dependencies_use_parent_source_and_foreign_sources_need_enrollment() {
    let roots = vec![root("a", "*", Profile::Recommended)];
    let mut foreign = release("c", "9.0.0", vec![]);
    foreign.instance.registry = "foreign".into();
    let mut catalog = vec![
        release("a", "1.0.0", vec![edge("c", "*")]),
        release("c", "1.0.0", vec![]),
        foreign,
    ];
    let result = resolve(&roots, &catalog, &enrolled()).expect("parent source");
    assert_eq!(result[1].release.version.to_string(), "1.0.0");
    catalog[0].dependencies[0].registry = Some("foreign".into());
    assert_eq!(
        resolve(&roots, &catalog, &enrolled()).expect_err("unenrolled"),
        Error::Unenrolled("foreign".into())
    );
    let mut sources = enrolled();
    sources.insert("foreign".into());
    assert!(
        resolve(&roots, &catalog, &sources)
            .expect("enrolled")
            .iter()
            .any(|s| s.release.instance.registry == "foreign")
    );
}

#[test]
fn prereleases_require_exact_requirements() {
    let catalog = vec![
        release("a", "1.0.0-beta.1", vec![]),
        release("a", "0.9.0", vec![]),
    ];
    for requirement in ["*", ">=1.0.0-beta.1", "^1.0.0-beta.1"] {
        let result = resolve(
            &[root("a", requirement, Profile::Recommended)],
            &catalog,
            &enrolled(),
        );
        if requirement == "*" {
            assert_eq!(
                result.expect("stable")[0].release.version.to_string(),
                "0.9.0"
            );
        } else {
            assert_eq!(result.expect_err("range prerelease"), Error::Conflict);
        }
    }
    assert_eq!(
        resolve(
            &[root("a", "=1.0.0-beta.1", Profile::Recommended)],
            &catalog,
            &enrolled()
        )
        .expect("exact")[0]
            .release
            .version
            .to_string(),
        "1.0.0-beta.1"
    );
}

#[test]
fn invalid_metadata_and_bounds_refuse() {
    let roots = vec![root("a", "*", Profile::Recommended)];
    let good = release("a", "1.0.0", vec![]);
    assert_eq!(
        resolve(&roots, &[good.clone(), good.clone()], &enrolled()).expect_err("duplicate"),
        Error::Invalid
    );
    let mut invalid = release("a", "1.0.0", vec![edge("b", "*")]);
    invalid.dependencies[0].levels = Some(vec![Profile::Strict, Profile::Loose, Profile::Strict]);
    assert_eq!(
        resolve(&roots, &[invalid], &enrolled()).expect_err("ordering"),
        Error::Invalid
    );
    assert_eq!(
        resolve(&vec![roots[0].clone(); 65], &[good], &enrolled()).expect_err("root bound"),
        Error::Limit
    );
    assert!(
        resolve::<Profile>(&[], &[], &enrolled())
            .expect("empty")
            .is_empty()
    );
}

#[test]
fn exact_prerelease_pins_do_not_relax_dependency_prerelease_constraints() {
    let mut pinned = root("b", "*", Profile::Recommended);
    pinned.pin = Some("1.0.0-beta.1".parse().expect("pin"));
    let mut catalog = vec![
        release("a", "1.0.0", vec![edge("b", ">=1.0.0-beta.1")]),
        release("b", "1.0.0-beta.1", vec![]),
    ];
    assert_eq!(
        resolve(&[pinned.clone()], &catalog, &enrolled()).expect("explicit pin")[0]
            .release
            .version
            .to_string(),
        "1.0.0-beta.1"
    );
    let mut incompatible = pinned.clone();
    incompatible.requirement = ">=1.0.0-beta.2".parse().expect("requirement");
    assert_eq!(
        resolve(&[incompatible], &catalog, &enrolled()).expect_err("pin outside range"),
        Error::Conflict
    );
    let roots = vec![root("a", "*", Profile::Recommended), pinned];
    assert_eq!(
        resolve(&roots, &catalog, &enrolled()).expect_err("dependency range"),
        Error::Conflict
    );
    catalog[0].dependencies[0].requirement = "=1.0.0-beta.1".parse().expect("exact");
    assert_eq!(
        resolve(&roots, &catalog, &enrolled())
            .expect("exact dependency")
            .len(),
        2
    );
}

#[test]
fn public_resolution_bounds_reachable_closure_and_backtracking_work() {
    let roots = vec![root("a00", "*", Profile::Recommended)];
    let catalog: Vec<_> = (0..65)
        .map(|i| {
            release(
                &format!("a{i:02}"),
                "1.0.0",
                if i == 64 {
                    vec![]
                } else {
                    vec![edge(&format!("a{:02}", i + 1), "*")]
                },
            )
        })
        .collect();
    assert_eq!(
        resolve(&roots, &catalog, &enrolled()).expect_err("closure bound"),
        Error::Limit
    );
    let mut roots: Vec<_> = (0..14)
        .map(|i| root(&format!("a{i:02}"), "*", Profile::Recommended))
        .collect();
    roots.push(root("z", "*", Profile::Recommended));
    let catalog: Vec<_> = (0..14)
        .flat_map(|i| {
            [
                release(&format!("a{i:02}"), "1.0.0", vec![]),
                release(&format!("a{i:02}"), "2.0.0", vec![]),
            ]
        })
        .collect();
    assert_eq!(
        resolve(&roots, &catalog, &enrolled()).expect_err("search bound"),
        Error::Limit
    );
}
