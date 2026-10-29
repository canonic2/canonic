//! Cross-tool effect, option and nested-execution behavior.
use super::*;
fn request(executable: &str, args: &[&str]) -> Request {
    Request {
        executable: executable.into(),
        arguments: args.iter().map(|arg| (*arg).into()).collect(),
        cwd: "/acme".into(),
        project: "/acme".into(),
    }
}
#[test]
fn git_250_builtin_families_cannot_be_replaced_by_injected_aliases() {
    // Additional builtin names from the versioned upstream inventory. The
    // classification is still possible evidence, not native authentication.
    let names = "annotate backfill bugreport check-mailmap checkout--worker cherry column count-objects credential-cache credential-cache--daemon credential-store diagnose diff-files diff-index diff-pairs diff-tree fast-export fetch-pack fmt-merge-msg for-each-repo fsck-objects fsmonitor--daemon get-tar-commit-id help hook init-db interpret-trailers mailinfo mailsplit merge-base merge-ours merge-recursive merge-recursive-ours merge-recursive-theirs merge-subtree merge-tree pack-redundant pack-refs patch-id pickaxe prune-packed range-diff receive-pack refs remote-ext remote-fd replay rerere send-pack show-branch show-index sparse-checkout stage stripspace submodule--helper unpack-file update-server-info upload-archive upload-archive--writer upload-pack var verify-commit verify-pack verify-tag version whatchanged";
    for name in names.split_whitespace() {
        let alias = format!("alias.{name}=!acme write");
        let hints = classify(&request("git", &["-c", &alias, name])).unwrap();
        assert!(
            !hints.unresolved.contains(&"alias-or-unknown-command"),
            "{name}"
        );
        assert!(hints.nested.is_empty(), "{name}");
        assert!(hints.may(Effect::CodeExecution), "{name}");
    }
    let hints = classify(&request(
        "git",
        &[
            "for-each-repo",
            "--config=acme.repos",
            "--keep-going",
            "--",
            "status",
        ],
    ))
    .unwrap();
    assert_eq!(hints.nested[0].argv, ["git", "status"]);
    assert_eq!(hints.nested[0].namespace, "configured-repositories");
    let hints = classify(&request(
        "git",
        &[
            "for-each-repo",
            "--unknown",
            "--config=acme.repos",
            "status",
        ],
    ))
    .unwrap();
    assert!(hints.nested.is_empty());
    for command in [
        "backfill",
        "fetch-pack",
        "send-pack",
        "receive-pack",
        "upload-pack",
    ] {
        assert!(
            classify(&request("git", &[command]))
                .unwrap()
                .may(Effect::Network)
        );
    }
}

#[test]
fn option_values_and_namespaces_cannot_be_reinterpreted_as_host_commands() {
    for args in [
        vec!["-C", "push", "status"],
        vec!["--work-tree=push", "status"],
        vec!["-c", "alias.x=!acme write", "status"],
    ] {
        let hints = classify(&request("git", &args)).unwrap();
        assert_eq!(hints.command.as_deref(), Some("status"));
        assert!(!hints.may(Effect::Network));
        assert!(hints.nested.is_empty());
    }
    for args in [
        vec!["-C"],
        vec!["-Cpush", "status"],
        vec!["-calias.x=!acme write", "x"],
        vec!["-C=/acme", "status"],
        vec!["--unknown", "push"],
        vec!["-zpush"],
        vec!["--bare=false", "push"],
        vec!["-pP", "push"],
    ] {
        assert!(classify(&request("git", &args)).unwrap().command.is_none());
    }
    let alias = classify(&request("git", &["-c", "alias.x=!acme write", "x"])).unwrap();
    assert_eq!(alias.nested[0].source.as_deref(), Some("acme write"));
    assert_eq!(alias.nested[0].argument_index, 0);
    let foreach = classify(&request(
        "git",
        &["submodule", "foreach", "--recursive", "acme write"],
    ))
    .unwrap();
    assert_eq!(foreach.nested[0].namespace, "submodule-worktree");
    let container = classify(&request(
        "docker",
        &[
            "--context",
            "run",
            "container",
            "exec",
            "--env",
            "ACME=value",
            "acme-container",
            "acme",
            "--privileged",
            "--mount=host",
        ],
    ))
    .unwrap();
    assert_eq!(container.command.as_deref(), Some("container exec"));
    assert_eq!(
        container.nested[0].argv,
        ["acme", "--privileged", "--mount=host"]
    );
    assert!(!container.may(Effect::HostExposure));
    assert!(!container.may(Effect::PrivilegedRuntime));
    let entrypoint = classify(&request(
        "docker",
        &[
            "run",
            "--privileged=false",
            "--entrypoint=sh",
            "acme",
            "-c",
            "acme write",
        ],
    ))
    .unwrap();
    assert!(!entrypoint.may(Effect::PrivilegedRuntime));
    assert_eq!(entrypoint.nested[0].argv, ["sh", "-c", "acme write"]);
    assert!(
        classify(&request("docker", &["run", "--unknown", "acme", "danger"]))
            .unwrap()
            .nested
            .is_empty()
    );
    let terminal = classify(&request(
        "docker",
        &["exec", "-it", "--", "acme-container", "acme", "write"],
    ))
    .unwrap();
    assert_eq!(terminal.nested[0].argv, ["acme", "write"]);
}

#[cfg(unix)]
#[test]
fn fixed_owned_git_fixture_confirms_config_arity_and_shell_alias_code() {
    // Only fixed test-owned Git/printf code is executed, never analyzer input.
    let root = assert_fs::TempDir::new().unwrap();
    let run = |args: &[&str]| {
        std::process::Command::new("/usr/bin/git")
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", root.path())
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .current_dir(root.path())
            .args(args)
            .output()
            .unwrap()
    };
    assert!(run(&["init", "--quiet"]).status.success());
    let global = run(&["-c", "acme.value=push", "config", "--get", "acme.value"]);
    assert!(global.status.success());
    assert_eq!(global.stdout, b"push\n");
    let alias = run(&["-c", "alias.acme=!printf acme-fixture", "acme"]);
    assert!(alias.status.success());
    assert_eq!(alias.stdout, b"acme-fixture");
    let parameters = run(&["-c", "alias.acme=!printf '<%s>'", "acme", "a b"]);
    assert!(parameters.status.success());
    assert_eq!(parameters.stdout, b"<a b>");
    let repeated = run(&["-c", "alias.acme=!printf '<%s>' \"$1\"", "acme", "a b"]);
    assert!(repeated.status.success());
    assert_eq!(repeated.stdout, b"<a b><a b>");
    let precedence = run(&[
        "-c",
        "alias.acme=!printf first",
        "-c",
        "alias.acme=!printf last",
        "acme",
    ]);
    assert!(precedence.status.success());
    assert_eq!(precedence.stdout, b"last");
    let builtin = run(&[
        "-c",
        "alias.status=!printf must-not-replace-builtin",
        "status",
        "--porcelain",
    ]);
    assert!(builtin.status.success());
    assert_eq!(builtin.stdout, [] as [u8; 0]);
    assert_eq!(
        classify(&request(
            "git",
            &["-c", "alias.status=!acme write", "status"]
        ))
        .unwrap()
        .nested
        .len(),
        0
    );
}

proptest::proptest! {
    #[test]
    fn arbitrary_tool_arguments_are_retained_in_possible_effect_hints(
        tool in proptest::sample::select(vec!["git", "docker", "/lookalike/git", "/lookalike/docker"]),
        args in proptest::collection::vec(".{0,48}", 0..20),
    ) {
        let input = Request { executable: tool.into(), arguments: args, cwd: "/acme".into(), project: "/acme".into() };
        let evidence = classify(&input).unwrap();
        proptest::prop_assert!(!evidence.complete);
        proptest::prop_assert_eq!(serde_json::to_value(&evidence.request).unwrap(), serde_json::to_value(&input).unwrap());
    }
}

#[test]
fn operation_families_retain_positive_mutation_evidence_and_exact_inputs() {
    for (tool, commands, effect) in [
        (
            "git",
            "push fetch clone pull submodule ls-remote",
            Effect::Network,
        ),
        (
            "git",
            "add commit update-index update-ref hash-object write-tree commit-tree mktree mktag pack-objects index-pack unpack-objects",
            Effect::RepositoryWrite,
        ),
        (
            "git",
            "reset rebase restore checkout switch merge cherry-pick revert apply am stash worktree rm mv",
            Effect::WorktreeWrite,
        ),
        (
            "git",
            "clean gc prune repack maintenance fsck multi-pack-index commit-graph",
            Effect::DataRemoval,
        ),
        ("git", "branch tag notes reflog", Effect::HistoryRewrite),
        (
            "git",
            "config remote credential bisect",
            Effect::Configuration,
        ),
        (
            "docker",
            "run create exec start restart attach",
            Effect::CodeExecution,
        ),
        ("docker", "build buildx", Effect::ImageBuild),
        ("docker", "pull push login logout", Effect::Network),
        ("docker", "rm rmi prune", Effect::DataRemoval),
        (
            "docker",
            "stop kill pause unpause rename update commit tag load import",
            Effect::DaemonWrite,
        ),
        ("docker", "cp save export", Effect::WorktreeWrite),
    ] {
        for command in commands.split_whitespace() {
            let input = request(tool, &[command, "'literal'", "--unknown"]);
            let hints = classify(&input).unwrap();
            assert!(hints.may(effect), "{tool} {command}");
            assert_eq!(
                serde_json::to_value(&hints.request).unwrap(),
                serde_json::to_value(&input).unwrap()
            );
            assert!(!hints.complete);
        }
    }
    let mounted = classify(&request(
        "docker",
        &[
            "container",
            "run",
            "-v/acme:/data",
            "--cap-add=SYS_ADMIN",
            "image",
            "acme",
        ],
    ))
    .unwrap();
    assert!(mounted.may(Effect::HostExposure));
    assert!(mounted.may(Effect::PrivilegedRuntime));
    let output = classify(&request("git", &["diff", "--output=/acme/result"])).unwrap();
    assert!(output.may(Effect::WorktreeWrite));
    assert!(!output.may(Effect::Network));
}
#[test]
fn builder_bootstrap_flags_preserve_boolean_values_and_interspersed_operands() {
    for args in [
        vec!["buildx", "inspect", "--bootstrap"],
        vec!["buildx", "inspect", "acme-builder", "--bootstrap=true"],
    ] {
        let hints = classify(&request("docker", &args)).unwrap();
        assert!(hints.may(Effect::DaemonWrite));
        assert!(hints.may(Effect::Network));
    }
    for args in [
        vec!["buildx", "inspect", "--bootstrap=false"],
        vec!["buildx", "inspect", "--bootstrap", "--bootstrap=false"],
        vec!["buildx", "inspect", "--format", "--bootstrap"],
        vec!["buildx", "inspect", "--", "--bootstrap"],
    ] {
        let hints = classify(&request("docker", &args)).unwrap();
        assert!(!hints.may(Effect::DaemonWrite));
        assert!(!hints.may(Effect::Network));
        assert!(!hints.complete);
    }
    let hints = classify(&request(
        "docker",
        &["buildx", "inspect", "--unknown", "--bootstrap"],
    ))
    .unwrap();
    assert!(hints.unresolved.contains(&"builder-inspection-options"));
}

#[test]
fn git_global_query_modes_stop_at_their_own_option_boundary() {
    for query in [
        "--exec-path",
        "--html-path",
        "--man-path",
        "--info-path",
        "--list-cmds=builtins",
    ] {
        let hints = classify(&request("git", &["-c", "alias.x=!acme write", query, "x"])).unwrap();
        assert!(hints.command.is_none(), "{query}");
        assert!(hints.nested.is_empty(), "{query}");
        assert!(hints.unresolved.contains(&"global-print-mode"));
        assert_eq!(hints.global_options.len(), 2);
    }
    for args in [
        vec!["--exec-path=/acme/bin", "push"],
        vec!["--shallow-file", "--exec-path", "push"],
        vec!["--attr-source=HEAD", "--no-literal-pathspecs", "push"],
    ] {
        let hints = classify(&request("git", &args)).unwrap();
        assert_eq!(hints.command.as_deref(), Some("push"));
        assert!(hints.may(Effect::Network));
    }
    for (flag, command) in [
        ("--version", "version"),
        ("-v", "version"),
        ("--help", "help"),
        ("-h", "help"),
    ] {
        let hints = classify(&request("git", &["--no-pager", flag])).unwrap();
        assert_eq!(hints.command.as_deref(), Some(command));
        assert!(hints.nested.is_empty());
    }
    let hints = classify(&request("git", &["--shallow-file=/acme/data", "push"])).unwrap();
    assert!(hints.command.is_none());
}

#[test]
fn management_suboperations_keep_inspection_operands_separate_from_mutation() {
    for family in [
        "volume", "network", "system", "builder", "swarm", "service", "stack", "secret", "config",
        "context", "buildx", "compose", "plugin",
    ] {
        let hints = classify(&request(
            "docker",
            &[family, "inspect", "rm", "--mount=operand"],
        ))
        .unwrap();
        assert!(!hints.may(Effect::DataRemoval), "{family}");
        assert!(!hints.may(Effect::HostExposure), "{family}");
        assert!(hints.nested.is_empty());
        assert_eq!(hints.command_path, [family, "inspect"]);
        assert!(!hints.complete);
    }
    for family in [
        "volume", "network", "builder", "service", "secret", "config",
    ] {
        assert!(
            classify(&request("docker", &[family, "rm", "acme"]))
                .unwrap()
                .may(Effect::DataRemoval)
        );
        assert!(
            classify(&request("docker", &[family, "create", "acme"]))
                .unwrap()
                .may(Effect::DaemonWrite)
        );
    }
    assert!(
        classify(&request("docker", &["buildx", "bake", "acme"]))
            .unwrap()
            .may(Effect::ImageBuild)
    );
    assert!(
        classify(&request("docker", &["plugin", "install", "acme"]))
            .unwrap()
            .may(Effect::PrivilegedRuntime)
    );
    assert!(
        classify(&request("docker", &["network", "create", "acme"]))
            .unwrap()
            .nested
            .is_empty()
    );
}

#[test]
fn global_context_and_indirect_execution_stay_unresolved() {
    let hints = classify(&request(
        "/fake/git",
        &["-C", "other", "-c", "alias.s=!danger", "s"],
    ))
    .unwrap();
    assert!(hints.may(Effect::Configuration));
    assert!(hints.may(Effect::CodeExecution));
    assert!(!hints.complete);
    assert_eq!(hints.command.as_deref(), Some("s"));
    let hints = classify(&request("git", &["--unknown", "status"])).unwrap();
    assert!(hints.command.is_none());
}
#[test]
fn mutations_and_runtime_exposure_are_possible_effects_not_grants() {
    let hints = classify(&request("git", &["push", "origin", "+HEAD:main"])).unwrap();
    assert!(hints.may(Effect::HistoryRewrite));
    assert!(hints.may(Effect::Network));
    let hints = classify(&request(
        "docker",
        &[
            "run",
            "--mount=type=bind,src=/acme,dst=/data",
            "--privileged",
            "acme",
            "sh",
            "-c",
            "danger",
        ],
    ))
    .unwrap();
    assert!(hints.may(Effect::CodeExecution));
    assert!(hints.may(Effect::HostExposure));
    assert!(hints.may(Effect::PrivilegedRuntime));
    assert!(!hints.complete);
    assert!(classify(&request("acme", &["read"])).is_none());
}
#[test]
fn explicit_git_alias_precedence_and_argv_data_do_not_become_shell_code() {
    let hints = classify(&request(
        "git",
        &[
            "-c",
            "alias.x=!acme write",
            "-c",
            "alias.x=push 'origin name'",
            "x",
            "HEAD",
        ],
    ))
    .unwrap();
    assert_eq!(hints.nested.len(), 1);
    assert_eq!(hints.nested[0].kind, "git-command-alias");
    assert_eq!(&hints.nested[0].argv[5..], ["push", "origin name", "HEAD"]);
    assert!(hints.may(Effect::Network));
    let hints = classify(&request(
        "git",
        &["-c", "ALIAS.x=!printf '<%s>'", "x", "a b"],
    ))
    .unwrap();
    assert_eq!(
        hints.nested[0].source.as_deref(),
        Some("printf '<%s>' \"$@\"")
    );
    assert_eq!(hints.nested[0].argv, ["a b"]);
    assert!(
        classify(&request("git", &["--exec-path", "status"]))
            .unwrap()
            .command
            .is_none()
    );
    let hints = classify(&request(
        "git",
        &[
            "-c",
            "alias.x=status",
            "--config-env=alias.x=ACME_ALIAS",
            "x",
        ],
    ))
    .unwrap();
    assert!(hints.nested.is_empty());
    let hints = classify(&request(
        "git",
        &["-c", "alias.x=log '$(acme write)' ';'", "x"],
    ))
    .unwrap();
    assert_eq!(&hints.nested[0].argv[3..], ["log", "$(acme write)", ";"]);
}
