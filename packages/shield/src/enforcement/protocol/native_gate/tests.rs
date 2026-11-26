//! Native coverage and protected paths use the same fresh dialect analysis.
use super::*;

#[test]
fn unavailable_path_evidence_does_not_offer_another_folder_grant() {
    let dir = assert_fs::TempDir::new().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let project = root.join("project");
    let outside_file = root.join("registry/schema.mjs");
    std::fs::create_dir(&project).unwrap();
    std::fs::create_dir(outside_file.parent().unwrap()).unwrap();
    std::fs::write(&outside_file, "Acme").unwrap();
    let mut value = json!({
        "_observation":{"evaluation":{"explanation":{
            "guidance":[{"operation_ref":"shell:0:0-9"}],
            "requirement_checks":[]
        }}},
        "_shell_analysis":{"operations":[{
            "source_unit":0,"span":{"start":0,"end":9},
            "request":{"executable":"cat","arguments":[outside_file]}
        }]}
    });
    // Establish that the same modeled outside read normally offers access.
    let grants = crate::configuration::AccessPaths::default();
    let outside = |value: &Value| unapproved_outside(value, &project, &project, None, &grants);
    assert!(outside(&value).is_some());
    value["_observation"]["evaluation"]["explanation"]["requirement_checks"] = json!([
        {"code":"evidence_unavailable","operation_ref":"shell:0:0-9","status":"unknown"},
        {"code":"location_outside","operation_ref":"shell:0:0-9","status":"failed"}
    ]);
    assert!(outside(&value).is_none());
    value["_observation"]["evaluation"]["explanation"]["requirement_checks"] =
        json!([{"code":"location_outside","operation_ref":"shell:0:0-9","status":"failed"}]);
    assert!(outside(&value).is_some());
}

#[test]
#[cfg(unix)]
fn base64_option_paths_preserve_native_protection() {
    let dir = assert_fs::TempDir::new().unwrap();
    let root = dir.path().canonicalize().unwrap();
    std::fs::create_dir_all(root.join(".shield/state")).unwrap();
    std::fs::write(root.join(".shield/state/state.json"), "private").unwrap();
    let snapshot = crate::configuration::Candidate::from_yaml("schema_version: 1\npacks: []\n")
        .unwrap()
        .capture(&[], None)
        .unwrap();
    let evaluator =
        crate::configuration::Evaluator::new(vec![(crate::policy::Scope::Project, snapshot)])
            .unwrap();
    for (source, blocked) in [
        ("base64 -d", false),
        ("base64 --input=ordinary --output=result", false),
        ("base64 --input=.shield/state/state.json", true),
        ("base64 -o.shield/state/state.json", true),
        ("base64 --output=.shield/state/state.json", true),
    ] {
        let analysis = crate::shell::analyze(
            source,
            crate::shell::Dialect::Bash,
            root.to_str().unwrap(),
            root.to_str().unwrap(),
            &evaluator,
        )
        .unwrap();
        let finding = paths::protection(
            &analysis,
            &root,
            &root,
            paths::Guard {
                cache: None,
                allow: crate::review::path_access::PathAccess::default(),
                working_files: None,
            },
            paths::Access::Write,
        );
        assert_eq!(
            finding.0 != paths::Protection::None,
            blocked,
            "{source}: {finding:?}"
        );
    }
}

#[test]
fn grep_searches_and_auxiliary_reads_preserve_native_protection() {
    let dir = assert_fs::TempDir::new().unwrap();
    let root = dir.path().canonicalize().unwrap();
    for directory in [".shield/state", ".shield/cache", "src"] {
        std::fs::create_dir_all(root.join(directory)).unwrap();
    }
    for file in [
        ".shield/state/state.json",
        ".shield/cache/plan",
        "src/main.rs",
        "patterns",
    ] {
        std::fs::write(root.join(file), "Acme").unwrap();
    }
    let rules = crate::policy::rules::document(&[
        json!({"id":"grep","decision":"allow","conditions":{"domain":"argv","executable":"grep","arguments":{"kind":"any"}},"requirements":{"targets":{"action":"read","all":{"within":{"root":"project"}}}},"on_unmet":{"decision":"deny"}}),
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
    for (source, blocked) in [
        ("grep --rec Acme", false),
        ("grep --recur Acme", false),
        ("grep --dir=recurse Acme", false),
        ("grep -d rec Acme", false),
        ("grep --directories=recu Acme", false),
        ("grep -d RECURSE Acme", false),
        ("grep -f.shield/state/state.json src/main.rs", true),
        ("grep -f.shield/cache/plan", true),
        (
            "grep --exclude-f=.shield/state/state.json Acme src/main.rs",
            true,
        ),
        ("grep --rec Acme src", false),
        ("grep -fpatterns src/main.rs", false),
        ("grep Acme", false),
    ] {
        let analysis = crate::shell::analyze(
            source,
            crate::shell::Dialect::Bash,
            root.to_str().unwrap(),
            root.to_str().unwrap(),
            &evaluator,
        )
        .unwrap();
        let finding = paths::protection(
            &analysis,
            &root,
            &root,
            paths::Guard {
                cache: None,
                allow: crate::review::path_access::PathAccess::default(),
                working_files: None,
            },
            paths::Access::Write,
        );
        assert_eq!(
            finding.0 != paths::Protection::None,
            blocked,
            "{source}: {finding:?} {analysis}"
        );
    }
}

#[test]
fn a_busy_protection_check_reports_contention_instead_of_invalid_authorization() {
    let root = assert_fs::TempDir::new().unwrap();
    let bundle =
        crate::configuration::Bundle::capture("schema_version: 1\npacks: []\n", &[], None).unwrap();
    let context = crate::review::mcp::initialize_fixture(root.path(), &bundle);
    let root = Path::new(&context.project_root);
    let marker = root.join(".shield/state/state.lock");
    std::fs::write(&marker, "fixture writer").unwrap();
    let mut receipt =
        json!({"identity":{"project_root":root},"_observation":{"request":{"cwd":root}}});
    assert_eq!(
        resolve(
            root,
            NativeContext {
                integration: "codex",
                event: crate::agents::Event::Before,
                approval: NativeApproval::Fallback,
                context_complete: true,
                working_files: None,
            },
            None,
            &mut receipt
        ),
        Err("runtime-busy")
    );
    std::fs::remove_file(marker).unwrap();
}

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
        assert_eq!(finding.0, paths::Protection::Blocked, "{analysis}");
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
