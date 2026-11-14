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
    let unchanged = |_: &Path, query: &Query| {
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
                assert_eq!(paths, vec![root.join(".shield")], "{arguments:?}");
                revisions
            }
            Query::Untracked { .. } => vec!["untracked".to_owned()],
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
    let changed = |_: &Path, _: &Query| Some(true);
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
    let unknown = |_: &Path, _: &Query| None;
    let finding = policy_files_with(&["reset", "--hard"], &root, &root, true, guard(), &unknown);
    assert!(finding.0 == Protection::Undetermined);
    // An unknown folder asks without running Git.
    let unexpected =
        |_: &Path, _: &Query| -> Option<bool> { panic!("no query in an unknown folder") };
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
