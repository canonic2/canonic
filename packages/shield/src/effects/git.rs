//! Git argument/configuration recognition and conservative nested-effect evidence.
use super::{Effect, Hints, NestedExecution, arguments};

pub(super) fn classify(hints: &mut Hints) {
    let args = &hints.request.arguments;
    let globals = arguments::parse(
        args,
        &[
            "-C",
            "-c",
            "--git-dir",
            "--work-tree",
            "--namespace",
            "--config-env",
            "--exec-path",
            "--shallow-file",
            "--attr-source",
        ],
        &[
            "--no-pager",
            "--paginate",
            "-p",
            "-P",
            "--bare",
            "--no-replace-objects",
            "--no-optional-locks",
            "--literal-pathspecs",
            "--no-literal-pathspecs",
            "--glob-pathspecs",
            "--noglob-pathspecs",
            "--icase-pathspecs",
            "--no-lazy-fetch",
            "--no-advice",
        ],
        arguments::Syntax::Git,
    );
    let at = globals.command_at;
    hints.global_options = globals.options;
    if !hints.global_options.is_empty() {
        hints.effects.insert(Effect::Configuration);
    }
    if globals.query {
        hints.unresolved.push("global-print-mode");
        return;
    }
    if let Some(reason) = globals.unresolved {
        hints.unresolved.push(reason);
        return;
    }
    let Some(command) = args.get(at) else {
        hints.unresolved.push("missing-command");
        return;
    };
    let command = match command.as_str() {
        "--version" | "-v" => "version",
        "--help" | "-h" => "help",
        value => value,
    }
    .to_owned();
    hints.command = Some(command.clone());
    hints.command_path = vec![command.clone()];
    hints.effects.insert(Effect::CodeExecution); // pager, helpers, filters, hooks/config remain uninspected
    if let Some(effects) = git_effects(&command) {
        hints.effects.extend(effects);
    } else {
        hints.unresolved.push("alias-or-unknown-command");
    }
    if args[at + 1..].iter().any(|arg| {
        arg == "--output"
            || arg.starts_with("--output=")
            || arg == "--output-directory"
            || arg.starts_with("--output-directory=")
    }) {
        hints.effects.insert(Effect::WorktreeWrite);
    }
    if command == "push"
        && args[at + 1..].iter().any(|arg| {
            arg == "-f"
                || arg.starts_with("--force")
                || arg.starts_with('+')
                || arg == "--mirror"
                || arg == "--delete"
        })
    {
        hints.effects.insert(Effect::HistoryRewrite);
        hints.effects.insert(Effect::DataRemoval);
    }
    hints.unresolved.extend([
        "command-options",
        "repository-paths",
        "aliases-hooks-helpers",
    ]);
    git_nested(hints, &command, at);
}

fn git_nested(hints: &mut Hints, command: &str, at: usize) {
    git_alias(hints, command, at);
    if command == "for-each-repo" {
        git_for_each_repo(hints, at);
    }
    let args = &hints.request.arguments;
    if command == "bisect" && args.get(at + 1).is_some_and(|value| value == "run") {
        let program = args[at + 2..].to_vec();
        if program.is_empty() {
            hints.unresolved.push("missing-bisect-program");
        } else {
            hints.nested.push(NestedExecution {
                kind: "git-bisect-run",
                source: None,
                argv: program,
                argument_index: at + 2,
                namespace: "repository-worktree",
            });
        }
    }
    if command == "submodule" && args.get(at + 1).is_some_and(|value| value == "foreach") {
        let mut script_at = at + 2;
        while args
            .get(script_at)
            .is_some_and(|value| matches!(value.as_str(), "--recursive" | "--quiet"))
        {
            script_at += 1;
        }
        if args.get(script_at).is_some_and(|value| value == "--") {
            script_at += 1;
        }
        if let Some(source) = args.get(script_at).filter(|value| !value.starts_with('-')) {
            hints.nested.push(NestedExecution {
                kind: "git-submodule-foreach",
                source: Some(source.clone()),
                argv: Vec::new(),
                argument_index: script_at,
                namespace: "submodule-worktree",
            });
            if args.len() != script_at + 1 {
                hints.unresolved.push("foreach-argument-composition");
            }
        } else {
            hints.unresolved.push("foreach-options-or-source");
        }
    }
}

fn git_for_each_repo(hints: &mut Hints, at: usize) {
    let args = &hints.request.arguments;
    let options = arguments::parse(
        &args[at + 1..],
        &["--config"],
        &["--keep-going"],
        arguments::Syntax::GitCommand,
    );
    if options.unresolved.is_some() {
        hints.unresolved.push("foreach-repo-options");
        return;
    }
    if !options
        .options
        .iter()
        .any(|option| option.name == "--config")
    {
        hints.unresolved.push("missing-foreach-repo-config");
        return;
    }
    let child_at = at + 1 + options.command_at;
    if child_at == args.len() {
        hints.unresolved.push("missing-foreach-repo-command");
        return;
    }
    let mut launch = vec![hints.request.executable.clone()];
    launch.extend_from_slice(&args[..at]);
    launch.extend_from_slice(&args[child_at..]);
    hints.nested.push(NestedExecution {
        kind: "git-for-each-repo",
        source: None,
        argv: launch,
        argument_index: child_at,
        namespace: "configured-repositories",
    });
}

fn git_alias(hints: &mut Hints, command: &str, at: usize) {
    if git_effects(command).is_some() {
        return;
    }
    let option = hints.global_options.iter().rev().find(|option| {
        matches!(option.name.as_str(), "-c" | "--config-env")
            && option
                .value
                .as_deref()
                .and_then(|value| value.split_once('='))
                .and_then(|(key, _)| key.split_once('.'))
                .is_some_and(|(section, alias)| {
                    section.eq_ignore_ascii_case("alias") && alias == command
                })
    });
    let Some(option) = option else {
        return;
    };
    if option.name == "--config-env" {
        hints.unresolved.push("environment-alias-value");
        return;
    }
    let Some((_, value)) = option
        .value
        .as_deref()
        .and_then(|value| value.split_once('='))
    else {
        return;
    };
    let args = &hints.request.arguments;
    if let Some(source) = value.strip_prefix('!') {
        hints.nested.push(NestedExecution {
            kind: "git-shell-alias",
            source: Some(if args.len() > at + 1 {
                format!("{source} \"$@\"")
            } else {
                source.into()
            }),
            argv: args[at + 1..].to_vec(),
            argument_index: option.index,
            namespace: "repository-root",
        });
    } else if let Some(expanded) = arguments::alias_words(value) {
        if let Some(effects) = expanded.first().and_then(|command| git_effects(command)) {
            hints.effects.extend(effects);
        }
        let mut launch = vec![hints.request.executable.clone()];
        launch.extend_from_slice(&args[..at]);
        launch.extend(expanded);
        launch.extend_from_slice(&args[at + 1..]);
        hints.nested.push(NestedExecution {
            kind: "git-command-alias",
            source: None,
            argv: launch,
            argument_index: option.index,
            namespace: "repository-worktree",
        });
    } else {
        hints.unresolved.push("invalid-command-alias-quoting");
    }
}

fn git_effects(command: &str) -> Option<&'static [Effect]> {
    Some(match command {
        "log" | "diff" | "show" | "grep" | "blame" | "ls-files" | "ls-tree" | "rev-parse"
        | "rev-list" | "cat-file" | "show-ref" | "for-each-ref" | "check-ref-format"
        | "check-ignore" | "check-attr" | "name-rev" | "describe" | "shortlog" | "annotate"
        | "check-mailmap" | "cherry" | "column" | "count-objects" | "diff-files" | "diff-index"
        | "diff-pairs" | "diff-tree" | "fmt-merge-msg" | "get-tar-commit-id" | "help"
        | "merge-base" | "pack-redundant" | "patch-id" | "pickaxe" | "range-diff"
        | "show-branch" | "show-index" | "stripspace" | "var" | "verify-commit" | "verify-pack"
        | "verify-tag" | "version" | "whatchanged" | "hook" => &[],
        "ls-remote" | "upload-pack" | "upload-archive" | "upload-archive--writer" => {
            &[Effect::Network]
        }
        "init" | "status" | "add" | "commit" | "update-index" | "update-ref" | "hash-object"
        | "write-tree" | "commit-tree" | "mktree" | "mktag" | "pack-objects" | "index-pack"
        | "unpack-objects" | "symbolic-ref" | "replace" | "init-db" | "stage" | "pack-refs"
        | "update-server-info" | "merge-tree" | "merge-ours" | "replay" => {
            &[Effect::RepositoryWrite]
        }
        "push" | "fetch" | "clone" | "pull" | "submodule" | "svn" | "bundle" | "backfill"
        | "fetch-pack" | "send-pack" | "receive-pack" | "remote-ext" | "remote-fd"
        | "submodule--helper" => &[
            Effect::Network,
            Effect::RepositoryWrite,
            Effect::WorktreeWrite,
            Effect::HistoryRewrite,
        ],
        "reset"
        | "rebase"
        | "filter-branch"
        | "restore"
        | "checkout"
        | "switch"
        | "merge"
        | "cherry-pick"
        | "revert"
        | "apply"
        | "am"
        | "stash"
        | "worktree"
        | "rm"
        | "mv"
        | "read-tree"
        | "checkout-index"
        | "merge-index"
        | "merge-file"
        | "mergetool"
        | "checkout--worker"
        | "merge-recursive"
        | "merge-recursive-ours"
        | "merge-recursive-theirs"
        | "merge-subtree"
        | "sparse-checkout"
        | "rerere" => &[
            Effect::RepositoryWrite,
            Effect::WorktreeWrite,
            Effect::DataRemoval,
            Effect::HistoryRewrite,
        ],
        "clean" | "gc" | "prune" | "repack" | "maintenance" | "fsck" | "multi-pack-index"
        | "commit-graph" | "fsck-objects" | "prune-packed" => &[
            Effect::DataRemoval,
            Effect::RepositoryWrite,
            Effect::WorktreeWrite,
        ],
        "branch" | "tag" | "notes" | "reflog" => &[
            Effect::RepositoryWrite,
            Effect::HistoryRewrite,
            Effect::DataRemoval,
        ],
        "config"
        | "remote"
        | "credential"
        | "bisect"
        | "credential-cache"
        | "credential-cache--daemon"
        | "credential-store"
        | "fsmonitor--daemon"
        | "refs"
        | "for-each-repo" => &[
            Effect::Configuration,
            Effect::RepositoryWrite,
            Effect::WorktreeWrite,
            Effect::Network,
        ],
        "archive" | "format-patch" | "difftool" | "bugreport" | "diagnose" | "fast-export"
        | "interpret-trailers" | "mailinfo" | "mailsplit" | "unpack-file" => {
            &[Effect::WorktreeWrite]
        }
        "send-email" | "imap-send" | "daemon" => &[Effect::Network, Effect::Configuration],
        "fast-import" => &[Effect::RepositoryWrite, Effect::HistoryRewrite],
        _ => return None,
    })
}
