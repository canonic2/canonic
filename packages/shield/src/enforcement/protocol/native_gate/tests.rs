//! Native coverage and protected paths use the same fresh dialect analysis.
use super::*;

#[test]
fn native_dialect_sequences_preserve_protected_access_and_opaque_refusals() {
    let root = assert_fs::TempDir::new().unwrap();
    // The current state directory and its earlier name are both protected.
    for directory in [".shield/state", ".shield/review"] {
        std::fs::create_dir_all(root.path().join(directory)).unwrap();
        std::fs::write(
            root.path().join(directory).join("state.json"),
            "Acme private state",
        )
        .unwrap();
    }
    let rules = crate::policy::rules::document(&[
        json!({"id":"native","decision":"allow","conditions":{"domain":"argv","executable":{"one_of":["git","rg","node"]},"arguments":{"kind":"any"}}}),
    ]).unwrap().to_string();
    let snapshot = crate::configuration::Candidate::from_yaml(
        "schema_version: 1\npacks: []\nrules: rules.yml\n",
    )
    .unwrap()
    .capture(&[], Some(&rules))
    .unwrap();
    let evaluator =
        crate::configuration::Evaluator::new(vec![(crate::policy::Scope::Project, snapshot)])
            .unwrap();
    let path = root.path().canonicalize().unwrap();
    for (dialect, source) in [
        (
            crate::shell::Dialect::Powershell,
            "git status; rg Acme .shield/state/state.json",
        ),
        (
            crate::shell::Dialect::Cmd,
            "git status & rg Acme .shield/state/state.json",
        ),
        (
            crate::shell::Dialect::Powershell,
            "git status; rg Acme .shield/review/state.json",
        ),
        (
            crate::shell::Dialect::Cmd,
            "git status & rg Acme .shield/review/state.json",
        ),
    ] {
        let analysis = crate::shell::analyze(
            source,
            dialect,
            path.to_str().unwrap(),
            path.to_str().unwrap(),
            &evaluator,
        )
        .unwrap();
        assert!(
            !opaque_shell(&json!({"_shell_analysis":analysis})),
            "{analysis}"
        );
        let finding = paths::protection(
            &analysis,
            &path,
            &path,
            paths::Guard {
                cache: None,
                allow: crate::review::path_access::PathAccess::default(),
                working_files: None,
            },
            paths::Access::Write,
        );
        assert!(finding.0 >= paths::Protection::SearchBlocked, "{analysis}");
    }
    let analysis = crate::shell::analyze(
        "node -e dangerous",
        crate::shell::Dialect::Cmd,
        path.to_str().unwrap(),
        path.to_str().unwrap(),
        &evaluator,
    )
    .unwrap();
    assert!(opaque_shell(&json!({"_shell_analysis":analysis})));
}
