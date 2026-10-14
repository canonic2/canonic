//! Exercises closure choices, source isolation, level propagation and refusal.
use super::*;
use proptest::prelude::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Profile {
    Loose,
    Recommended,
    Strict,
}

impl Level for Profile {
    fn all() -> &'static [Self] {
        &[Self::Loose, Self::Recommended, Self::Strict]
    }
}

fn id(pack: &str) -> Instance {
    Instance {
        registry: "acme-source".into(),
        pack: format!("acme/{pack}").parse().expect("pack"),
    }
}
fn root(pack: &str, requirement: &str, level: Profile) -> Root<Profile> {
    Root {
        instance: id(pack),
        requirement: requirement.parse().expect("requirement"),
        pin: None,
        level,
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
    Dependency::parse(&format!("acme/{pack}"), requirement, None, None).expect("edge")
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
fn unsafe_candidate_closures_fall_back_with_diagnostics_but_pins_refuse() {
    let foreign = Dependency::parse("acme/c", "*", Some("foreign"), None).unwrap();
    for (dependency, reason) in [
        (foreign, Error::Unenrolled("foreign".into())),
        (edge("b", "*"), Error::LevelConflict(id("b"))),
        (edge("a", "*"), Error::Cycle),
    ] {
        let mut roots = vec![
            root("a", "*", Profile::Strict),
            root("b", "*", Profile::Loose),
        ];
        let mut catalog = vec![
            release("a", "2.0.0", vec![dependency]),
            release("a", "1.0.0", vec![]),
            release("b", "1.0.0", vec![]),
        ];
        let report = resolve_with_diagnostics(&roots, &catalog, &enrolled()).unwrap();
        assert_eq!(
            summary(&report.selections),
            summary(&resolve(&roots, &catalog, &enrolled()).unwrap())
        );
        assert_eq!(report.selections[0].release.version.to_string(), "1.0.0");
        assert!(report.rejected.iter().any(|attempt| {
            attempt.release.instance == id("a")
                && attempt.release.version.to_string() == "2.0.0"
                && attempt.reason == reason
        }));
        let attempts = |report: &ResolutionReport<'_, Profile>| {
            report
                .rejected
                .iter()
                .map(|attempt| {
                    (
                        attempt.release.instance.clone(),
                        attempt.release.version.clone(),
                        attempt.reason.clone(),
                    )
                })
                .collect::<Vec<_>>()
        };
        let expected_attempts = attempts(&report);
        catalog.reverse();
        roots.reverse();
        let reversed = resolve_with_diagnostics(&roots, &catalog, &enrolled()).unwrap();
        assert_eq!(attempts(&reversed), expected_attempts);

        let pinned = roots
            .iter_mut()
            .find(|root| root.instance == id("a"))
            .unwrap();
        pinned.pin = Some("2.0.0".parse().unwrap());
        assert_eq!(resolve(&roots, &catalog, &enrolled()).unwrap_err(), reason);
        assert_eq!(
            resolve_with_diagnostics(&roots, &catalog, &enrolled()).unwrap_err(),
            reason
        );
    }
}

#[test]
fn diagnostics_describe_branches_rather_than_globally_rejected_releases() {
    let roots = vec![
        root("a", "*", Profile::Recommended),
        root("b", "*", Profile::Recommended),
    ];
    let catalog = vec![
        release("a", "2.0.0", vec![edge("c", "^2")]),
        release("a", "1.0.0", vec![edge("c", "^1")]),
        release("b", "1.0.0", vec![edge("c", "^1")]),
        release("c", "1.0.0", vec![]),
        release("c", "2.0.0", vec![]),
    ];
    let report = resolve_with_diagnostics(&roots, &catalog, &enrolled()).unwrap();
    assert!(report.rejected.iter().any(|attempt|
        attempt.release.instance == id("b") && attempt.reason == Error::Conflict
    ));
    assert!(
        report
            .selections
            .iter()
            .any(|selected| selected.release.instance == id("b"))
    );

    let report = resolve_with_diagnostics(&roots[..1], &catalog, &enrolled()).unwrap();
    assert!(report.rejected.is_empty());
    assert_eq!(report.selections[0].release.version.to_string(), "2.0.0");
}

#[test]
fn levels_propagate_after_all_parents_and_preserve_explicit_roots() {
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

/// A product level whose `all` omits a value and is out of order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Broken {
    Low,
    Middle,
    High,
}

impl Level for Broken {
    fn all() -> &'static [Self] {
        &[Self::High, Self::Low]
    }
}

#[test]
fn a_level_list_that_is_unordered_or_incomplete_refuses() {
    let instance = Instance {
        registry: "acme-source".into(),
        pack: "acme/a".parse().expect("pack"),
    };
    let releases = [Release {
        instance: instance.clone(),
        version: "1.0.0".parse().expect("version"),
        dependencies: vec![],
    }];
    for level in [Broken::Low, Broken::Middle] {
        let roots = [Root {
            instance: instance.clone(),
            requirement: VersionReq::STAR,
            pin: None,
            level,
        }];
        assert_eq!(
            resolve(&roots, &releases, &enrolled()).expect_err("broken level list"),
            Error::Invalid
        );
    }
}

#[test]
fn products_without_levels_resolve_with_unit() {
    let instance = |pack: &str| Instance {
        registry: "acme-source".into(),
        pack: pack.parse().expect("pack"),
    };
    let roots = [Root {
        instance: instance("acme/skills"),
        requirement: VersionReq::STAR,
        pin: None,
        level: (),
    }];
    let releases = [
        Release {
            instance: instance("acme/skills"),
            version: "1.0.0".parse().expect("version"),
            dependencies: vec![Dependency::parse("acme/rules", "^1", None, None).expect("edge")],
        },
        Release {
            instance: instance("acme/rules"),
            version: "1.4.0".parse().expect("version"),
            dependencies: vec![],
        },
    ];
    let result = resolve(&roots, &releases, &enrolled()).expect("closure");
    assert_eq!(result.len(), 2);
    assert!(result.iter().any(|s| !s.direct));
    assert_eq!(
        Dependency::<()>::parse("acme/rules", "^1", None, Some(vec![(), ()]))
            .expect_err("mapping length"),
        Error::Invalid
    );
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
    assert_eq!(
        resolve(
            &[Root {
                instance: Instance {
                    registry: "unknown".into(),
                    pack: id("a").pack
                },
                ..root("a", "*", Profile::Loose)
            }],
            &catalog,
            &sources
        )
        .expect_err("unenrolled root"),
        Error::Unenrolled("unknown".into())
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
    let mut budget = 0;
    assert_eq!(
        search(
            &roots,
            &[],
            &enrolled(),
            &BTreeMap::new(),
            &mut budget,
            &mut None
        )
        .expect_err("work bound"),
        Error::Limit
    );
    assert!(
        resolve::<Profile>(&[], &[], &enrolled())
            .expect("empty")
            .is_empty()
    );
    for (pack, requirement, registry) in [
        ("acme", "*", None),
        ("acme/a", "not a range", None),
        ("acme/a", "*", Some(" ")),
    ] {
        assert_eq!(
            Dependency::<Profile>::parse(pack, requirement, registry, None).expect_err(pack),
            Error::Invalid
        );
    }
    assert_eq!(
        Dependency::<Profile>::parse("acme/a", &"1".repeat(4097), None, None)
            .expect_err("oversized"),
        Error::Invalid
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
        resolve(std::slice::from_ref(&pinned), &catalog, &enrolled()).expect("explicit pin")[0]
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
    assert_eq!(
        resolve_with_diagnostics(&roots, &catalog, &enrolled()).expect_err("closure bound"),
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
    assert_eq!(
        resolve_with_diagnostics(&roots, &catalog, &enrolled()).expect_err("search bound"),
        Error::Limit
    );
}

proptest! {
    #[test]
    fn selected_closures_satisfy_every_reachable_requirement(
        versions in proptest::collection::vec((0u64..3, 0u64..3, proptest::option::of(0u64..3)), 1..6)
    ) {
        // Each release of `a` depends on a `b` major chosen by the strategy.
        let mut catalog = vec![release("b", "0.0.0", vec![]), release("b", "1.0.0", vec![]), release("b", "2.0.0", vec![])];
        for (major, minor, dependency) in &versions {
            let edges = dependency.map(|m| vec![edge("b", &format!("^{m}"))]).unwrap_or_default();
            catalog.push(release("a", &format!("{major}.{minor}.0"), edges));
        }
        let mut seen = BTreeSet::new();
        catalog.retain(|r| seen.insert((r.instance.clone(), r.version.clone())));
        let roots = [root("a", "*", Profile::Recommended)];
        let result = resolve(&roots, &catalog, &enrolled()).expect("some release of a always resolves");
        let report = resolve_with_diagnostics(&roots, &catalog, &enrolled()).expect("same closure with reporting");
        prop_assert_eq!(summary(&report.selections), summary(&result));
        let a = result.iter().find(|s| s.release.instance == id("a")).expect("root selected");
        let highest = catalog.iter().filter(|r| r.instance == id("a")).map(|r| &r.version).max().expect("a");
        prop_assert_eq!(&a.release.version, highest);
        for edge in &a.release.dependencies {
            let b = result.iter().find(|s| s.release.instance.pack == edge.pack).expect("dependency selected");
            prop_assert!(edge.requirement.matches(&b.release.version));
        }
    }
}
