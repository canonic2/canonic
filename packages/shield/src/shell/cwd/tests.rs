//! Effective directory outcomes use real request-owned filesystem observations.
use crate::{
    configuration::{Candidate, Evaluator},
    policy::Scope,
    shell::{self, Dialect},
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path};

fn evaluator() -> Evaluator {
    let rules = crate::policy::rules::document(&[
        json!({"id":"read-subdirectory","decision":"allow","conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"any"}},"requirements":{"cwd":{"exact":{"root":"project","path":"sub"}}},"on_unmet":{"decision":"deny"}}),
        json!({"id":"literal-builtins","decision":"allow","conditions":{"domain":"argv","executable":{"one_of":["printf","echo","mkdir"]},"arguments":{"kind":"any"}}}),
        json!({"id":"read-files","decision":"allow","conditions":{"domain":"file","action":"read"},"requirements":{"sources":{"all":{"within":{"root":"project"}}}},"on_unmet":{"decision":"deny"}}),
        json!({"id":"write-files","decision":"allow","conditions":{"domain":"file","action":"write"},"requirements":{"destination":{"within":{"root":"project"}}},"on_unmet":{"decision":"deny"}}),
    ]).expect("valid cwd rules").to_string();
    let snapshot = Candidate::from_yaml("schema_version: 1\npacks: []\nrules: rules.yml\n")
        .expect("valid cwd configuration")
        .capture(&[], Some(&rules))
        .expect("capture cwd rules");
    Evaluator::new(vec![(Scope::Project, snapshot)]).expect("prepare cwd rules")
}

fn fixture() -> assert_fs::TempDir {
    let root = assert_fs::TempDir::new().expect("create cwd fixture");
    std::fs::create_dir(root.path().join("sub")).expect("create subdirectory");
    std::fs::write(root.path().join("sub/note.txt"), "Acme note").expect("write subdirectory note");
    std::fs::write(root.path().join("note.txt"), "Acme root note").expect("write root note");
    root
}

fn analyze(source: &str, root: &Path) -> Value {
    let root = root.canonicalize().expect("canonical cwd fixture");
    shell::analyze(
        source,
        Dialect::Bash,
        root.to_str().expect("UTF-8 cwd fixture"),
        root.to_str().expect("UTF-8 project fixture"),
        &evaluator(),
    )
    .expect("analyze cwd fixture")
}

fn commands<'a>(value: &'a Value, executable: &str) -> Vec<&'a Value> {
    value["operations"]
        .as_array()
        .expect("operation array")
        .iter()
        .filter(|operation| operation["request"]["executable"] == executable)
        .collect()
}

#[test]
fn previous_directory_requires_established_logical_bookkeeping_and_preserves_branch_pairs() {
    let root = fixture();
    let base = root.path().canonicalize().unwrap();
    std::fs::create_dir(base.join("other")).unwrap();
    let run = |source: &str, environment: Option<&BTreeMap<String, String>>| {
        shell::analyze_controlled(
            source,
            Dialect::Bash,
            base.to_str().unwrap(),
            environment,
            base.to_str().unwrap(),
            &evaluator(),
            &|| Ok(()),
        )
        .unwrap()
    };
    let exact = BTreeMap::from([("PWD".into(), base.to_str().unwrap().into())]);
    let spoofed = BTreeMap::from([
        ("PWD".into(), "/unverified/pwd".into()),
        ("OLDPWD".into(), base.join("sub").to_str().unwrap().into()),
    ]);
    for environment in [None, Some(&spoofed)] {
        let result = run("cd -P -- ./sub && cd -P -- - && acme read", environment);
        assert!(
            commands(&result, "acme")
                .iter()
                .any(|operation| operation["context"]["cwd_known"] == false),
            "{result}"
        );
    }
    let result = run("cd -P -- ./sub && cd -P -- - && acme read", Some(&exact));
    assert_eq!(commands(&result, "acme")[0]["cwd"], base.to_str().unwrap());
    assert_eq!(commands(&result, "acme")[0]["context"]["cwd_known"], true);
    let result = run(
        "cd -P -- ./sub && cd -P -- .. && cd -P -- - && acme read",
        None,
    );
    assert_eq!(
        commands(&result, "acme")[0]["cwd"],
        base.join("sub").to_str().unwrap()
    );
    assert_eq!(result["decision"], "allow");
    let result = run(
        "if echo x; then cd -P -- ./sub && cd -P -- ..; else cd -P -- ./other && cd -P -- ..; fi && cd -P -- - && acme read",
        None,
    );
    let locations: Vec<_> = commands(&result, "acme")
        .iter()
        .map(|operation| operation["cwd"].as_str().unwrap())
        .collect();
    assert!(
        locations.contains(&base.join("sub").to_str().unwrap()),
        "{result}"
    );
    assert!(
        locations.contains(&base.join("other").to_str().unwrap()),
        "{result}"
    );
    assert_eq!(result["decision"], "deny");
}

#[test]
fn previous_directory_failure_scope_and_mutation_do_not_reuse_unproved_bookkeeping() {
    let root = fixture();
    let base = root.path().canonicalize().unwrap();
    let environment = BTreeMap::from([("PWD".into(), base.to_str().unwrap().into())]);
    let run = |source: &str| {
        shell::analyze_controlled(
            source,
            Dialect::Bash,
            base.to_str().unwrap(),
            Some(&environment),
            base.to_str().unwrap(),
            &evaluator(),
            &|| Ok(()),
        )
        .unwrap()
    };
    for source in [
        "cd -P -- ./sub; cd -P -- - && acme read",
        "cd -P -- ./sub && cd -P -- - > ./missing/out || cd -P -- - && acme read",
        "cd -P -- ./sub && mkdir new && cd -P -- - && acme read",
        "cd -P -- ./sub && OLDPWD=/unverified; cd -P -- - && acme read",
    ] {
        let result = run(source);
        assert!(
            commands(&result, "acme")
                .iter()
                .any(|operation| operation["context"]["cwd_known"] == false),
            "{source}: {result}"
        );
    }
    let result = run("cd -P -- ./sub && (cd -P -- ..) && cd -P -- - && acme read");
    assert_eq!(commands(&result, "acme")[0]["cwd"], base.to_str().unwrap());
}

#[test]
fn previous_logical_directory_is_resolved_fresh_when_its_symlink_changes() {
    let root = fixture();
    let base = root.path().canonicalize().unwrap();
    std::fs::create_dir(base.join("other")).unwrap();
    let alias = base.join("alias");
    let evaluator = evaluator();
    for target in [base.join("sub"), base.join("other")] {
        if std::fs::symlink_metadata(&alias).is_ok() {
            std::fs::remove_file(&alias).unwrap();
        }
        std::os::unix::fs::symlink(&target, &alias).unwrap();
        let environment = BTreeMap::from([("PWD".into(), alias.to_str().unwrap().into())]);
        let result = shell::analyze_controlled(
            "cd -P -- .. && cd -P -- - && acme read",
            Dialect::Bash,
            alias.to_str().unwrap(),
            Some(&environment),
            base.to_str().unwrap(),
            &evaluator,
            &|| Ok(()),
        )
        .unwrap();
        assert_eq!(
            commands(&result, "acme")[0]["cwd"],
            target.to_str().unwrap(),
            "{result}"
        );
        assert_eq!(commands(&result, "acme")[0]["context"]["cwd_known"], true);
    }
}

#[test]
fn sequential_cd_keeps_success_and_failure_directories_separate() {
    let root = fixture();
    let base = root.path().canonicalize().unwrap();
    let sub = base.join("sub");
    for (source, expected, decision) in [
        ("cd ./sub && acme read", vec![sub.clone()], "allow"),
        (
            "cd ./sub || acme read",
            vec![base.clone(), sub.clone()],
            "deny",
        ),
        ("! cd -P -- ./sub || acme read", vec![sub.clone()], "allow"),
        (
            "! cd -P -- ./sub && acme read",
            vec![base.clone(), sub.clone()],
            "deny",
        ),
        (
            "cd ./sub; acme read",
            vec![base.clone(), sub.clone()],
            "deny",
        ),
        (
            "if cd ./sub; then acme read; else acme read; fi",
            vec![sub.clone(), base.clone(), sub.clone()],
            "deny",
        ),
    ] {
        let value = analyze(source, root.path());
        assert_eq!(value["decision"], decision, "{source}: {value}");
        let actual: Vec<_> = commands(&value, "acme")
            .iter()
            .map(|operation| Path::new(operation["cwd"].as_str().unwrap()).to_path_buf())
            .collect();
        assert_eq!(actual, expected, "{source}: {value}");
        assert!(
            commands(&value, "acme")
                .iter()
                .all(|operation| operation["context"]["cwd_known"] == true)
        );
    }
    let failed = analyze("cd ./missing || acme read", root.path());
    assert_eq!(commands(&failed, "acme")[0]["cwd"], base.to_str().unwrap());
    assert_eq!(commands(&failed, "acme")[0]["context"]["cwd_known"], false);
}

#[test]
fn cd_failure_includes_a_destination_where_unknown_bookkeeping_can_fail() {
    let root = fixture();
    let base = root.path().canonicalize().unwrap();
    let rules = crate::policy::rules::document(&[
        json!({"id":"root-only","decision":"allow","conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"any"}},"requirements":{"cwd":{"exact":{"root":"project"}}},"on_unmet":{"decision":"deny"}}),
    ]).unwrap().to_string();
    let snapshot = Candidate::from_yaml("schema_version: 1\npacks: []\nrules: rules.yml\n")
        .unwrap()
        .capture(&[], Some(&rules))
        .unwrap();
    let evaluator = Evaluator::new(vec![(Scope::Project, snapshot)]).unwrap();
    let value = shell::analyze(
        "cd -P -- ./sub || acme read",
        Dialect::Bash,
        base.to_str().unwrap(),
        base.to_str().unwrap(),
        &evaluator,
    )
    .unwrap();
    // A pre-chdir failure permits the fallback in the root; a post-chdir
    // bookkeeping failure reaches the forbidden destination. Check both.
    assert_eq!(value["decision"], "deny", "{value}");
    let outcomes: Vec<_> = commands(&value, "acme")
        .iter()
        .map(|operation| operation["decision"].clone())
        .collect();
    assert_eq!(outcomes, [json!("allow"), json!("deny")]);
}

#[test]
fn independent_status_nodes_and_redirection_failures_do_not_reuse_cd_success() {
    let root = fixture();
    let base = root.path().canonicalize().unwrap();
    for source in [
        "cd ./sub; [[ x = x ]] && acme read",
        "cd ./sub; f() { :; } && acme read",
        "cd ./sub; for item in; do :; done && acme read",
        "cd ./sub; case x in esac && acme read",
        "{ ! cd -P -- ./sub; } < note.txt || acme read",
    ] {
        let value = analyze(source, root.path());
        assert_eq!(value["decision"], "deny", "{source}: {value}");
        assert!(
            commands(&value, "acme")
                .iter()
                .any(|operation| operation["cwd"] == base.to_str().unwrap()),
            "{source}: {value}"
        );
    }
}

#[test]
fn subshells_background_functions_and_lastpipe_preserve_directory_scope() {
    let root = fixture();
    let base = root.path().canonicalize().unwrap();
    for source in [
        "(cd -P -- ./sub); acme read",
        "cd -P -- ./sub & acme read",
        "coproc cd -P -- ./sub && acme read",
    ] {
        let value = analyze(source, root.path());
        assert_eq!(
            commands(&value, "acme")[0]["cwd"],
            base.to_str().unwrap(),
            "{value}"
        );
    }
    for source in [
        "f() { cd -P -- ./sub; }; f && acme read",
        "command cd -P -- ./sub && acme read",
    ] {
        let value = analyze(source, root.path());
        assert_eq!(
            commands(&value, "acme")[0]["cwd"],
            base.join("sub").to_str().unwrap(),
            "{value}"
        );
        assert_eq!(commands(&value, "acme")[0]["context"]["cwd_known"], true);
    }
    let pipeline = analyze("echo x | cd -P -- ./sub; acme read", root.path());
    assert_eq!(
        commands(&pipeline, "acme")[0]["context"]["cwd_known"],
        false
    );
}

#[test]
fn logical_and_physical_cd_preserve_symlink_parent_semantics() {
    let root = fixture();
    let base = root.path().canonicalize().unwrap();
    std::fs::create_dir_all(base.join("physical/deep")).unwrap();
    std::os::unix::fs::symlink(base.join("physical/deep"), base.join("alias")).unwrap();
    let alias = format!(
        "'{}'",
        base.join("alias").to_str().unwrap().replace('\'', "'\\''")
    );
    for (mode, expected) in [("-L", base.clone()), ("-P", base.join("physical"))] {
        let value = analyze(
            &format!("cd -L -- {alias} && cd {mode} -- .. && acme read"),
            root.path(),
        );
        assert_eq!(
            commands(&value, "acme")[0]["cwd"],
            expected.to_str().unwrap(),
            "{value}"
        );
        assert_eq!(commands(&value, "acme")[0]["context"]["cwd_known"], true);
    }
    let sub = base.join("sub");
    let environment = BTreeMap::from([("PWD".into(), sub.to_str().unwrap().into())]);
    let observed = shell::analyze_controlled(
        "cd -L -- .. && acme read",
        Dialect::Bash,
        sub.to_str().unwrap(),
        Some(&environment),
        base.to_str().unwrap(),
        &evaluator(),
        &|| Ok(()),
    )
    .unwrap();
    assert_eq!(
        commands(&observed, "acme")[0]["cwd"],
        base.to_str().unwrap()
    );
    let unknown = shell::analyze(
        "cd -L -- .. && acme read",
        Dialect::Bash,
        sub.to_str().unwrap(),
        base.to_str().unwrap(),
        &evaluator(),
    )
    .unwrap();
    assert_eq!(commands(&unknown, "acme")[0]["context"]["cwd_known"], false);
}

#[test]
fn implicit_cd_does_not_infer_the_hosts_logical_shell_mode() {
    let root = fixture();
    let base = root.path().canonicalize().unwrap();
    std::fs::create_dir_all(base.join("physical/deep")).unwrap();
    std::os::unix::fs::symlink(base.join("physical/deep"), base.join("alias")).unwrap();
    let alias = format!(
        "'{}'",
        base.join("alias").to_str().unwrap().replace('\'', "'\\''")
    );
    for source in [
        format!("cd {alias}/.. && acme read"),
        format!("cd {alias} && cd -L -- .. && acme read"),
        format!("cd -L -- {alias} && cd . && cd -L -- .. && acme read"),
    ] {
        let value = analyze(&source, root.path());
        assert_eq!(
            commands(&value, "acme")[0]["context"]["cwd_known"],
            false,
            "{source}: {value}"
        );
        assert_ne!(value["decision"], "allow");
    }
    let value = analyze(
        &format!("cd -L -- {alias} && cd -L -- . && cd -L -- .. && acme read"),
        root.path(),
    );
    assert_eq!(commands(&value, "acme")[0]["cwd"], base.to_str().unwrap());
    assert_eq!(commands(&value, "acme")[0]["context"]["cwd_known"], true);
}

#[test]
fn derived_directories_apply_to_typed_reads_redirections_and_mutation_invalidation() {
    let root = fixture();
    let base = root.path().canonicalize().unwrap();
    let value = analyze(
        "cd ./sub && printf text > note.txt && sed -n 1p note.txt",
        root.path(),
    );
    assert_eq!(value["decision"], "allow", "{value}");
    for operation in value["operations"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|operation| {
            operation["coverage"] == "file-redirection"
                || operation["request"]["executable"] == "sed"
        })
    {
        assert_eq!(operation["cwd"], base.join("sub").to_str().unwrap());
    }
    assert_eq!(commands(&value, "sed")[0]["policy_request"]["kind"], "file");
    for source in [
        "mkdir existing; cd -P -- ./sub && acme read",
        "mkdir existing & cd -P -- ./sub && acme read",
        "mkdir existing | echo x; cd -P -- ./sub && acme read",
        "mkdir existing | { cd -P -- ./sub && acme read; }",
        "{ cd -P -- ./sub && acme read; } | mkdir existing",
        "{ cd -P -- ./sub && acme read; } & mkdir existing",
        "coproc { cd -P -- ./sub && acme read; }; mkdir existing",
        "cd ./sub && mkdir existing && acme read",
        "printf '%s' \"$(mkdir existing)\"; cd -P -- ./sub && acme read",
        "command --unknown mkdir existing; cd -P -- ./sub && acme read",
        "builtin --unknown eval 'mkdir existing'; cd -P -- ./sub && acme read",
    ] {
        let value = analyze(source, root.path());
        assert_ne!(value["decision"], "allow", "{source}: {value}");
        assert_eq!(
            commands(&value, "acme")[0]["context"]["cwd_known"],
            false,
            "{source}: {value}"
        );
    }
}

#[test]
fn redirected_groups_resolve_every_possible_directory() {
    let root = fixture();
    let base = root.path().canonicalize().unwrap();
    for source in [
        "cd -P -- ..; { printf text; } > out",
        "cd -P -- ..; if printf condition; then printf yes; else printf no; fi > out",
        "cd -P -- ./sub; { printf text; } > ../out",
    ] {
        let value = analyze(source, root.path());
        let redirects = value["operations"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|operation| operation["coverage"] == "file-redirection")
            .collect::<Vec<_>>();
        assert_eq!(redirects.len(), 2, "{source}: {value}");
        assert!(
            redirects
                .iter()
                .any(|operation| operation["cwd"] == base.to_str().unwrap())
        );
        assert_eq!(value["decision"], "deny", "{source}: {value}");
    }
}

#[test]
fn directory_alternative_widening_and_observation_limits_are_bounded() {
    let root = fixture();
    let base = root.path().canonicalize().unwrap();
    let mut sources = Vec::new();
    for number in 0..9 {
        let target = base.join(format!("dir-{number}"));
        std::fs::create_dir(&target).unwrap();
        sources.push(format!(
            "cd -P -- '{}'",
            target.to_str().unwrap().replace('\'', "'\\''")
        ));
    }
    sources.push("acme read".into());
    let value = analyze(&sources.join("; "), root.path());
    assert_eq!(commands(&value, "acme")[0]["context"]["cwd_known"], false);
    let observer = crate::location::directory::Directories::new();
    for _ in 0..128 {
        assert!(observer.observe(&base, &|| Ok(())).unwrap().is_some());
    }
    assert!(matches!(
        observer.observe(&base, &|| Ok(())),
        Err(crate::policy::Error::Limit)
    ));
    assert!(matches!(
        crate::location::directory::Directories::new()
            .observe(&base, &|| Err(crate::policy::Error::Interrupted)),
        Err(crate::policy::Error::Interrupted)
    ));
}
