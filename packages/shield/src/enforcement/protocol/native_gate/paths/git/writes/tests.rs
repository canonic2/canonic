use super::*;
use std::cell::RefCell;

fn guard() -> Guard<'static> {
    Guard {
        cache: None,
        allow: crate::review::path_access::PathAccess::default(),
        working_files: None,
    }
}

fn project() -> (assert_fs::TempDir, PathBuf) {
    let folder = assert_fs::TempDir::new().unwrap();
    let root = folder.path().canonicalize().unwrap();
    std::fs::create_dir_all(root.join(".shield")).unwrap();
    std::fs::write(root.join(".shield/config.yml"), "schema_version: 1\n").unwrap();
    std::fs::create_dir(root.join("src")).unwrap();
    (folder, root)
}

/// The revisions each diff query compares, as a fake Git that finds no
/// change records them; Err with the finding when the command decides
/// without unchanged answers.
fn asked(root: &Path, arguments: &[&str]) -> Result<Vec<Vec<String>>, Protection> {
    let queries = RefCell::new(Vec::new());
    let unchanged = |_: &Target, query: &Query| {
        queries.borrow_mut().push(query.clone());
        Some(false)
    };
    let finding = policy_files_with(arguments, root, root, true, guard(), &unchanged);
    if finding.0 != Protection::None {
        return Err(finding.0);
    }
    Ok(queries
        .into_inner()
        .into_iter()
        .map(|query| match query {
            Query::Diff { revisions, paths } => {
                assert!(paths.contains(&root.join(".shield")), "{arguments:?}");
                assert!(
                    paths.contains(&root.join(".config/shield")),
                    "{arguments:?}"
                );
                assert!(
                    paths.contains(&root.join(".codex/config.toml")),
                    "{arguments:?}"
                );
                revisions
            }
            Query::Untracked { .. } => vec!["untracked".to_owned()],
            Query::Stash { reference, .. } => vec!["stash".to_owned(), reference],
        })
        .collect())
}

#[test]
fn tree_changing_commands_ask_git_whether_the_policy_files_change() {
    let (_folder, root) = project();
    let strings = |words: &[&str]| words.iter().map(|word| (*word).to_owned()).collect();
    for (arguments, expected) in [
        (
            &["checkout", "acme-branch"][..],
            vec![strings(&["acme-branch"])],
        ),
        (&["checkout", "-"], vec![strings(&["@{-1}"])]),
        (
            &["checkout", "-b", "acme-feature", "HEAD~1"],
            vec![strings(&["HEAD~1"])],
        ),
        (
            &["checkout", "acme-branch", "--"],
            vec![strings(&["acme-branch"])],
        ),
        (&["checkout", "-f"], vec![strings(&["HEAD"])]),
        (&["switch", "acme-branch"], vec![strings(&["acme-branch"])]),
        (
            &["switch", "-c", "acme-feature", "origin/main"],
            vec![strings(&["origin/main"])],
        ),
        (&["switch", "--detach", "v1.0"], vec![strings(&["v1.0"])]),
        (&["reset", "--hard"], vec![strings(&["HEAD"])]),
        (&["reset", "--hard", "HEAD~1"], vec![strings(&["HEAD~1"])]),
        (
            &["reset", "--keep", "origin/main"],
            vec![strings(&["origin/main"])],
        ),
        (
            &["merge", "origin/main"],
            vec![strings(&["HEAD...origin/main"])],
        ),
        (
            &["merge", "-m", "Acme", "acme-a", "acme-b"],
            vec![strings(&["HEAD...acme-a"]), strings(&["HEAD...acme-b"])],
        ),
        (&["merge"], vec![strings(&["HEAD...@{upstream}"])]),
        (&["merge", "--abort"], vec![strings(&["HEAD"])]),
        (
            &["rebase", "origin/main"],
            vec![strings(&["HEAD...origin/main"])],
        ),
        (&["rebase"], vec![strings(&["HEAD...@{upstream}"])]),
        (
            &["cherry-pick", "1a2b3c4d"],
            vec![strings(&["1a2b3c4d^", "1a2b3c4d"])],
        ),
        (
            &["revert", "--no-edit", "HEAD"],
            vec![strings(&["HEAD^", "HEAD"])],
        ),
        (&["stash"], vec![strings(&["HEAD"])]),
        (&["stash", "push", "-m", "acme"], vec![strings(&["HEAD"])]),
        (
            &["stash", "apply"],
            vec![strings(&["stash", "refs/stash@{0}"])],
        ),
        (
            &["stash", "pop", "--index", "acme-saved"],
            vec![strings(&["stash", "acme-saved"])],
        ),
        (
            &["stash", "-ku"],
            vec![strings(&["HEAD"]), Vec::new(), strings(&["untracked"])],
        ),
        (&["clean", "-fd"], vec![strings(&["untracked"])]),
        (
            &["-C", "src", "switch", "acme-branch"],
            vec![strings(&["acme-branch"])],
        ),
    ] {
        assert_eq!(asked(&root, arguments), Ok(expected), "{arguments:?}");
    }
}

#[test]
fn commands_that_keep_the_tree_ask_nothing_and_unpredictable_ones_ask() {
    let (_folder, root) = project();
    // Writes that leave the working tree, or write only the paths the path
    // checks cover, ask nothing.
    for arguments in [
        &["checkout", "-b", "acme-feature"][..],
        &["checkout", "src"],
        &["checkout", "HEAD~3", "--", "src"],
        &["checkout", "HEAD~3", "src"],
        &["switch", "-c", "acme-feature"],
        &["reset", "HEAD~1"],
        &["reset", "--soft", "HEAD~1"],
        &["merge", "--continue"],
        &["rebase", "--quit"],
        &["cherry-pick", "--quit"],
        &["stash", "list"],
        &["stash", "show", "-p"],
        &["clean", "-n"],
        &["clean", "-fdn"],
        &["-C", "src", "clean", "-fd"],
        &["bisect", "log"],
        &["sparse-checkout", "list"],
        &["status"],
    ] {
        assert_eq!(asked(&root, arguments), Ok(Vec::new()), "{arguments:?}");
    }
    // Results no query predicts ask.
    for arguments in [
        &["pull"][..],
        &["stash", "apply", "0"],
        &["stash", "apply", "0000"],
        &["stash", "pop", "--index", "1234"],
        &["pull", "--rebase", "origin", "main"],
        &["switch", "--orphan", "acme-empty"],
        &["rebase", "-i", "HEAD~3"],
        &["rebase", "--onto", "main", "acme-a"],
        &["rebase", "--continue"],
        &["rebase", "--abort"],
        &["rebase", "main", "acme-topic"],
        &["cherry-pick", "main~3..main"],
        &["cherry-pick", "--continue"],
        &["cherry-pick", "-m", "1", "1a2b3c4d"],
        &["revert", "--abort"],
        &["bisect", "start"],
        &["sparse-checkout", "set", "src"],
        &["filter-branch", "--tree-filter", "true"],
        &["update-index", "--index-info"],
        // Settings and other repositories change what Git reads.
        &["-c", "core.useReplaceRefs=false", "reset", "--hard"],
        &["--no-replace-objects", "switch", "acme-branch"],
        &["--git-dir", "elsewhere.git", "switch", "acme-branch"],
    ] {
        assert_eq!(
            asked(&root, arguments),
            Err(Protection::Undetermined),
            "{arguments:?}"
        );
    }
}

#[test]
fn index_entries_ignored_files_and_merged_files_of_policy_files_are_blocked() {
    let (_folder, root) = project();
    for arguments in [
        &[
            "update-index",
            "--cacheinfo",
            "100644,1a2b3c4d,.shield/config.yml",
        ][..],
        &[
            "update-index",
            "--cacheinfo",
            "100644",
            "1a2b3c4d",
            ".shield/config.yml",
        ],
        &["update-index", "--assume-unchanged", ".shield/config.yml"],
        &["update-index", "--skip-worktree", ".shield/rules.yml"],
        &["clean", "-fdx"],
        &["clean", "-f", "-X", ".shield"],
        &["stash", "--all"],
        &["stash", "push", "-a"],
        &["merge-file", ".shield/config.yml", "base.yml", "other.yml"],
    ] {
        assert_eq!(
            asked(&root, arguments),
            Err(Protection::Blocked),
            "{arguments:?}"
        );
    }
    for arguments in [
        &["update-index", "--refresh"][..],
        &["update-index", "--add", "src/app.txt"],
        &["-C", "src", "clean", "-fdx"],
        &[
            "merge-file",
            "-p",
            ".shield/config.yml",
            "base.yml",
            "other.yml",
        ],
    ] {
        assert_eq!(asked(&root, arguments), Ok(Vec::new()), "{arguments:?}");
    }
}

#[test]
fn a_changed_or_unknown_answer_asks_and_names_the_policy_folder() {
    let (_folder, root) = project();
    let changed = |_: &Target, _: &Query| Some(true);
    let finding = policy_files_with(
        &["switch", "acme-branch"],
        &root,
        &root,
        true,
        guard(),
        &changed,
    );
    assert!(finding.0 == Protection::Undetermined);
    assert_eq!(
        finding.1.as_deref(),
        Some(root.join(".shield").to_str().unwrap())
    );
    let unknown = |_: &Target, _: &Query| None;
    let finding = policy_files_with(&["reset", "--hard"], &root, &root, true, guard(), &unknown);
    assert!(finding.0 == Protection::Undetermined);
    // An unknown folder asks without running Git.
    let unexpected =
        |_: &Target, _: &Query| -> Option<bool> { panic!("no query in an unknown folder") };
    let finding = policy_files_with(
        &["switch", "acme-branch"],
        &root,
        &root,
        false,
        guard(),
        &unexpected,
    );
    assert!(finding.0 == Protection::Undetermined);
}

/// Runs the test Git with a neutral identity in `root`.
fn git(root: &Path, arguments: &[&str]) -> String {
    let output = crate::test_support::git()
        .args([
            "-c",
            "user.name=Acme",
            "-c",
            "user.email=acme@example.com",
            "-c",
            "init.defaultBranch=main",
            "-c",
            "commit.gpgsign=false",
        ])
        .current_dir(root)
        .args(arguments)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{arguments:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

#[test]
#[cfg(not(windows))]
fn root_pathspecs_resolve_from_the_git_worktree_including_gate_entries() {
    let (_folder, root) = project();
    let entry = root.join(".gate/repos/public");
    std::fs::create_dir_all(&entry).unwrap();
    git(&entry, &["init", "-q"]);
    git(&entry, &["config", "core.worktree", root.to_str().unwrap()]);
    std::fs::write(root.join("src/app.txt"), "Acme source\n").unwrap();
    git(&entry, &["add", "--", ":/.shield", ":/src"]);
    git(&entry, &["commit", "-q", "-m", "Acme pathspec fixture"]);
    for spec in [":/!src", ":/^src", ":/:.shield"] {
        // Exclude-only pathspecs use the cwd's default selection; explicitly
        // select the root when demonstrating exclusion magic from an entry.
        let mut arguments = vec!["ls-files", "--"];
        if spec != ":/:.shield" {
            arguments.push(":/");
        }
        arguments.push(spec);
        assert!(
            git(&entry, &arguments).contains(".shield/config.yml"),
            "{spec}"
        );
    }
    let level = |arguments: &[&str]| policy_files(arguments, &root, &root, true, guard()).0;
    for spec in [":/src/app.txt", ":(top)src/app.txt"] {
        for subcommand in ["checkout", "restore"] {
            let arguments = ["-C", ".gate/repos/public", subcommand, "--ours", "--", spec];
            assert!(level(&arguments) == Protection::None, "{arguments:?}");
        }
    }
    // Whole-tree, policy and agent-settings selections still cannot bypass protection.
    for spec in [
        ":/",
        ":/!src",
        ":/^src",
        ":/:.shield",
        ":(top)",
        ":/.shield/config.yml",
        ":(top).shield",
        ":/.claude/settings.json",
        ":/src/../.shield/config.yml",
        ":/src/*.txt",
        ":(icase,top)src/app.txt",
    ] {
        assert!(
            level(&["-C", ".gate/repos/public", "checkout", "--ours", "--", spec])
                == Protection::Blocked,
            "{spec}"
        );
    }
    // A different repository's root must not be confused with the Shield project.
    git(&root.join("src"), &["init", "-q"]);
    assert!(level(&["-C", "src", "checkout", "--", ":/.shield/config.yml"]) == Protection::None);
    // Overrides and unavailable roots retain conservative handling.
    for arguments in [
        &[
            "-C",
            ".gate/repos/public",
            "--icase-pathspecs",
            "checkout",
            "--",
            ":/src",
        ][..],
        &[
            "-C",
            ".gate/repos/public",
            "-c",
            "core.worktree=src",
            "checkout",
            "--",
            ":/src",
        ],
        &["-C", ".gate/repos/missing", "checkout", "--", ":/src"],
    ] {
        assert!(level(arguments) == Protection::Blocked, "{arguments:?}");
    }
}

#[test]
#[cfg(not(windows))]
fn worktree_discovery_is_shared_by_all_queries_in_an_invocation() {
    use clap::Parser as _;
    let (_folder, root) = project();
    git(&root, &["init", "-q"]);
    std::fs::write(root.join("src/app.txt"), "Acme base\n").unwrap();
    git(&root, &["add", "--", ".shield", "src"]);
    git(&root, &["commit", "-q", "-m", "Acme base"]);
    std::fs::write(root.join("src/app.txt"), "Acme edit\n").unwrap();
    git(&root, &["commit", "-q", "-am", "Acme source"]);
    let logs = assert_fs::TempDir::new().unwrap();
    let cli = crate::cli::Cli::parse_from([
        "shield",
        "--debug-log-dir",
        logs.path().to_str().unwrap(),
        "status",
    ]);
    let log = std::sync::Arc::new(crate::diagnostics::Log::open(&cli));
    for (arguments, processes) in [
        (&["cherry-pick", "HEAD", "HEAD"][..], 3),
        (&["checkout", "--", ":/src/a", ":/src/b"][..], 1),
    ] {
        let trace = log.request("command", "git-footprint", None, None).unwrap();
        let scope = trace.enter();
        assert_eq!(
            policy_files(arguments, &root, &root, true, guard()).0,
            Protection::None
        );
        assert_eq!(trace.recorded_count("git.worktree_root_queries"), 1);
        assert_eq!(trace.recorded_count("git.inspection_processes"), processes);
        drop(scope);
        trace.finish("success");
    }
}

#[test]
fn real_git_answers_whether_committed_content_changes_the_policy_files() {
    let (_folder, root) = project();
    std::fs::write(root.join("src/app.txt"), "acme\n").unwrap();
    git(&root, &["init", "-q"]);
    git(&root, &["add", "--", ".shield", "src"]);
    git(&root, &["commit", "-q", "-m", "Acme base"]);
    git(&root, &["branch", "acme-same"]);
    git(&root, &["switch", "-q", "-c", "acme-src"]);
    std::fs::write(root.join("src/app.txt"), "acme src\n").unwrap();
    git(&root, &["commit", "-q", "-am", "Acme src"]);
    git(&root, &["switch", "-q", "main"]);
    git(&root, &["switch", "-q", "-c", "acme-open"]);
    std::fs::write(
        root.join(".shield/config.yml"),
        "schema_version: 1\n# acme open\n",
    )
    .unwrap();
    git(&root, &["commit", "-q", "-am", "Acme open"]);
    git(&root, &["switch", "-q", "main"]);
    let level =
        |arguments: &[&str], cwd: &Path| policy_files(arguments, &root, cwd, true, guard()).0;
    for arguments in [
        &["switch", "acme-same"][..],
        &["checkout", "acme-src"],
        &["merge", "acme-src"],
        &["rebase", "acme-src"],
        &["cherry-pick", "acme-src"],
        &["reset", "--hard"],
        &["reset", "--hard", "acme-src"],
        &["stash", "-u"],
        &["clean", "-f"],
    ] {
        assert!(level(arguments, &root) == Protection::None, "{arguments:?}");
    }
    assert!(level(&["-C", "src", "switch", "acme-same"], &root) == Protection::None);
    for arguments in [
        &["switch", "acme-open"][..],
        &["checkout", "acme-open"],
        &["merge", "acme-open"],
        &["cherry-pick", "acme-open"],
        &["reset", "--hard", "acme-open"],
        &["switch", "acme-missing"],
        &["pull"],
    ] {
        assert!(
            level(arguments, &root) == Protection::Undetermined,
            "{arguments:?}"
        );
    }
    // An untracked file in .shield is one a clean or untracked stash removes.
    std::fs::write(root.join(".shield/acme.yml"), "acme\n").unwrap();
    assert!(level(&["clean", "-f"], &root) == Protection::Undetermined);
    assert!(level(&["stash", "-u"], &root) == Protection::Undetermined);
    assert!(level(&["stash"], &root) == Protection::None);
    std::fs::remove_file(root.join(".shield/acme.yml")).unwrap();
    // A permissive configuration staged without touching the file: keeping
    // the index rewrites the file, a plain stash or reset does not.
    let open = git(&root, &["rev-parse", "acme-open:.shield/config.yml"]);
    let entry = format!("100644,{open},.shield/config.yml");
    assert!(level(&["update-index", "--cacheinfo", &entry], &root) == Protection::Blocked);
    git(&root, &["update-index", "--cacheinfo", &entry]);
    assert!(level(&["stash", "--keep-index"], &root) == Protection::Undetermined);
    assert!(level(&["reset", "--hard"], &root) == Protection::None);
    // Committed, the reset to that commit rewrites it: the user decides.
    git(&root, &["commit", "-q", "-m", "Acme staged"]);
    assert_eq!(
        std::fs::read_to_string(root.join(".shield/config.yml")).unwrap(),
        "schema_version: 1\n"
    );
    for arguments in [
        &["reset", "--hard"][..],
        &["checkout", "-f"],
        &["stash"],
        &["switch", "acme-open"],
    ] {
        assert!(
            level(arguments, &root) == Protection::Undetermined,
            "{arguments:?}"
        );
    }
    // Switching to a branch whose policy files match the working tree is
    // ordinary.
    assert!(level(&["switch", "acme-same"], &root) == Protection::None);
}

#[test]
#[cfg(not(windows))]
fn real_git_worktree_entry_queries_its_own_policy_and_stashes() {
    let (_folder, root) = project();
    let entry = root.join(".repos/private");
    std::fs::create_dir_all(&entry).unwrap();
    git(&entry, &["init", "-q"]);
    git(&entry, &["config", "core.worktree", root.to_str().unwrap()]);
    git(&entry, &["config", "extensions.worktreeConfig", "true"]);
    std::fs::write(root.join("src/app.txt"), "base\n").unwrap();
    git(&entry, &["add", "--", ":/.shield", ":/src"]);
    git(&entry, &["commit", "-q", "-m", "Acme base"]);
    let base = git(&entry, &["rev-parse", "HEAD"]);
    std::fs::write(root.join("src/app.txt"), "source change\n").unwrap();
    git(&entry, &["commit", "-q", "-am", "Acme source"]);
    let source = git(&entry, &["rev-parse", "HEAD"]);
    std::fs::write(root.join(".shield/config.yml"), "policy change\n").unwrap();
    git(&entry, &["commit", "-q", "-am", "Acme policy"]);
    let changed = git(&entry, &["rev-parse", "HEAD"]);
    let child = linked_worktree(&root, &entry, &base);
    let child_entry = child.join(".repos/private");
    let prefix = child_entry.strip_prefix(&root).unwrap().to_str().unwrap();
    let level = |rest: &[&str]| {
        let mut arguments = vec!["-C", prefix];
        arguments.extend(rest);
        policy_files(&arguments, &root, &root, true, guard()).0
    };
    assert_eq!(level(&["merge", "--ff-only", &source]), Protection::None);
    assert_eq!(
        level(&["merge", "--ff-only", &changed]),
        Protection::Undetermined
    );
    assert_eq!(level(&["stash", "push"]), Protection::None);
    std::fs::write(child.join("src/app.txt"), "saved source\n").unwrap();
    git(&child_entry, &["stash", "push", "-m", "Acme source"]);
    let saved = git(&child_entry, &["rev-parse", "stash@{0}"]);
    assert_eq!(level(&["stash", "apply", &saved]), Protection::None);
    std::fs::write(child.join("src/new.txt"), "Acme untracked source\n").unwrap();
    git(
        &child_entry,
        &["stash", "push", "-u", "-m", "Acme untracked source"],
    );
    assert_eq!(level(&["stash", "apply"]), Protection::None);
    assert_eq!(
        level(&["stash", "pop", "--index", &saved]),
        Protection::None
    );
    std::fs::write(child.join(".shield/config.yml"), "saved policy\n").unwrap();
    git(&child_entry, &["stash", "push", "-m", "Acme policy"]);
    assert_eq!(level(&["stash", "apply"]), Protection::Undetermined);
    // An index-only policy change must still refuse --index restoration.
    let policy_object = git(
        &entry,
        &["rev-parse", &format!("{changed}:.shield/config.yml")],
    );
    git(
        &child_entry,
        &[
            "update-index",
            "--cacheinfo",
            &format!("100644,{policy_object},.shield/config.yml"),
        ],
    );
    git(&child_entry, &["stash", "push", "-m", "Acme index"]);
    assert_eq!(
        level(&["stash", "apply", "--index"]),
        Protection::Undetermined
    );
    // Protected agent settings cannot be restored unseen either.
    std::fs::create_dir(child.join(".codex")).unwrap();
    std::fs::write(child.join(".codex/config.toml"), "# saved settings\n").unwrap();
    git(
        &child_entry,
        &["stash", "push", "-u", "-m", "Acme settings"],
    );
    assert_eq!(level(&["stash", "apply"]), Protection::Undetermined);
    std::fs::write(child.join(".shield/untracked.yml"), "untracked policy\n").unwrap();
    git(
        &child_entry,
        &["stash", "push", "-u", "-m", "Acme untracked"],
    );
    assert_eq!(level(&["stash", "apply"]), Protection::Undetermined);
    assert_eq!(
        level(&["stash", "apply", "acme-missing"]),
        Protection::Undetermined
    );
    assert_eq!(
        level(&["stash", "apply", "--unmodeled", &saved]),
        Protection::Undetermined
    );
}

#[cfg(not(windows))]
fn linked_worktree(root: &Path, entry: &Path, revision: &str) -> PathBuf {
    let child = root.join(".worktrees/acme");
    let child_entry = child.join(".repos/private");
    git(
        entry,
        &[
            "worktree",
            "add",
            "--detach",
            child_entry.to_str().unwrap(),
            revision,
        ],
    );
    git(
        &child_entry,
        &[
            "config",
            "--worktree",
            "core.worktree",
            child.to_str().unwrap(),
        ],
    );
    for directory in [".shield", "src"] {
        std::fs::rename(child_entry.join(directory), child.join(directory)).unwrap();
    }
    child
}
