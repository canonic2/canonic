//! Git commands that move the working tree to other committed content:
//! branch switches, hard resets, merges, rebases, cherry-picks, reverts,
//! stashes and cleans. Their writes follow ordinary policy unless they change
//! the policy files; each plan names the read-only Git queries that tell, and
//! commands whose result no query can predict ask.
use super::{Target, has, operands, widens};
use std::path::PathBuf;

/// What a command does to the policy files in the working tree.
pub(super) enum Plan {
    /// Leaves them as they are.
    Leaves,
    /// May change them in a way no query predicts; the user decides.
    Asks,
    /// Changes them when any query finds a difference.
    Queries(Vec<Query>),
}

/// A read-only Git question about protected paths.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Query {
    /// `git diff --quiet` over `paths`: between two revisions, between a
    /// revision and the working tree, or with none between the index and the
    /// working tree. `A...B` compares `B` with the merge base.
    Diff {
        revisions: Vec<String>,
        paths: Vec<PathBuf>,
    },
    /// Untracked files under `paths` that no ignore rule keeps.
    Untracked { paths: Vec<PathBuf> },
}

impl Query {
    /// The protected path a change is reported under.
    pub(super) fn path(&self) -> Option<&PathBuf> {
        match self {
            Self::Diff { paths, .. } | Self::Untracked { paths } => paths.first(),
        }
    }
}

pub(super) fn plan(target: &Target, subcommand: &str, rest: &[&str]) -> Plan {
    match subcommand {
        "checkout" => checkout(target, rest),
        "switch" => switch(target, rest),
        "reset" => reset(target, rest),
        "merge" => merge(target, rest),
        "rebase" => rebase(target, rest),
        "cherry-pick" | "revert" => pick(target, rest),
        "stash" => stash_options(rest).map_or(Plan::Leaves, |options| stash(target, options)),
        "clean" => clean(target, rest),
        // Bisection and sparse patterns check out content no query names.
        "bisect" => match rest.first().copied() {
            None | Some("log" | "view" | "visualize" | "visualise" | "terms" | "help") => {
                Plan::Leaves
            }
            Some(_) => Plan::Asks,
        },
        "sparse-checkout" => match rest.first().copied() {
            None | Some("list" | "check-rules") => Plan::Leaves,
            Some(_) => Plan::Asks,
        },
        // A pull merges what it fetches, which is unknown until it runs.
        "pull" | "filter-branch" => Plan::Asks,
        _ => Plan::Leaves,
    }
}

/// The options of a `git stash` that saves changes, which resets the working
/// tree; None for the other stash commands.
pub(super) fn stash_options<'a>(rest: &'a [&'a str]) -> Option<&'a [&'a str]> {
    match rest.first().copied() {
        None => Some(rest),
        Some("push" | "save") => Some(&rest[1..]),
        Some(word) if word.starts_with('-') => Some(rest),
        Some(_) => None,
    }
}

/// A revision as Git reads it from a command line; None for a word Git would
/// read as an option.
fn revision(word: &str) -> Option<String> {
    match word {
        "-" => Some("@{-1}".to_owned()),
        _ if word.is_empty() || word.starts_with('-') => None,
        _ => Some(word.to_owned()),
    }
}

/// The policy files differ between the working tree and `word`.
fn against(target: &Target, word: &str) -> Plan {
    revision(word).map_or(Plan::Asks, |revision| {
        Plan::Queries(vec![target.diff(vec![revision])])
    })
}

/// Each revision's side changes the policy files since its merge base with
/// `HEAD`.
fn incoming(target: &Target, words: &[&str]) -> Plan {
    words
        .iter()
        .map(|word| revision(word).map(|revision| target.diff(vec![format!("HEAD...{revision}")])))
        .collect::<Option<Vec<_>>>()
        .map_or(Plan::Asks, Plan::Queries)
}

impl Target {
    fn diff(&self, revisions: Vec<String>) -> Query {
        Query::Diff {
            revisions,
            paths: vec![self.shield.clone()],
        }
    }
}

/// `git checkout <branch>` and its forms that create a branch move the tree
/// to a revision; with paths it writes only them, which the path checks
/// cover.
fn checkout(target: &Target, rest: &[&str]) -> Plan {
    const VALUED: &[&str] = &["-b", "-B", "--orphan", "--conflict", "--pathspec-from-file"];
    if has(rest, &["-p", "--patch"], VALUED) || widens(rest, &["--pathspec-from-file"], VALUED) {
        return Plan::Leaves;
    }
    let creates = has(rest, &["-b", "-B", "--orphan"], VALUED);
    let (words, separator) = operands(rest, VALUED);
    let (revisions, paths) = words.split_at(separator.unwrap_or(words.len()));
    if !paths.is_empty() {
        return Plan::Leaves;
    }
    match revisions {
        [] if widens(rest, &["-f", "--force"], VALUED) => against(target, "HEAD"),
        [] => Plan::Leaves,
        // Git reads a lone existing path as a path, and refuses a word that
        // is both.
        [word] if separator.is_none() && !creates && target.base.join(word).exists() => {
            Plan::Leaves
        }
        [revision] => against(target, revision),
        // Without `--`, a revision followed by paths.
        _ if separator.is_none() => Plan::Leaves,
        _ => Plan::Asks,
    }
}

/// `git switch <branch>`, or the start point of a branch it creates.
fn switch(target: &Target, rest: &[&str]) -> Plan {
    const VALUED: &[&str] = &[
        "-c",
        "-C",
        "--create",
        "--force-create",
        "--orphan",
        "--conflict",
    ];
    // An orphan branch starts with no tracked files.
    if widens(rest, &["--orphan"], VALUED) {
        return Plan::Asks;
    }
    match operands(rest, VALUED).0.as_slice() {
        [] if widens(rest, &["-f", "--force", "--discard-changes"], VALUED) => {
            against(target, "HEAD")
        }
        [] => Plan::Leaves,
        [revision] => against(target, revision),
        _ => Plan::Asks,
    }
}

/// `git reset --hard`, `--merge` and `--keep` move the tree to a revision;
/// other modes change only the index and `HEAD`.
fn reset(target: &Target, rest: &[&str]) -> Plan {
    const VALUED: &[&str] = &["--pathspec-from-file"];
    if !widens(rest, &["--hard", "--merge", "--keep"], VALUED) {
        return Plan::Leaves;
    }
    let (words, separator) = operands(rest, VALUED);
    match words.split_at(separator.unwrap_or(words.len())) {
        ([], []) => against(target, "HEAD"),
        ([revision], []) => against(target, revision),
        _ => Plan::Asks,
    }
}

/// `git merge` brings in what each revision changed since the merge base;
/// `--abort` returns the tree to `HEAD`.
fn merge(target: &Target, rest: &[&str]) -> Plan {
    const VALUED: &[&str] = &[
        "-m",
        "-F",
        "-s",
        "-X",
        "--message",
        "--file",
        "--strategy",
        "--strategy-option",
        "--into-name",
    ];
    if has(rest, &["--abort"], VALUED) {
        return against(target, "HEAD");
    }
    if has(rest, &["--quit", "--continue"], VALUED) {
        return Plan::Leaves;
    }
    let words = operands(rest, VALUED).0;
    if words.is_empty() {
        incoming(target, &["@{upstream}"])
    } else {
        incoming(target, &words)
    }
}

/// `git rebase [<upstream>]` replays local commits on what the upstream
/// changed since the merge base. Interactive, scripted and `--onto` rebases,
/// and continuing or aborting one, produce trees no query names.
fn rebase(target: &Target, rest: &[&str]) -> Plan {
    const VALUED: &[&str] = &[
        "-s",
        "-X",
        "-x",
        "--strategy",
        "--strategy-option",
        "--exec",
        "--onto",
    ];
    if has(
        rest,
        &["--quit", "--edit-todo", "--show-current-patch"],
        VALUED,
    ) {
        return Plan::Leaves;
    }
    if widens(
        rest,
        &[
            "--abort",
            "--continue",
            "--skip",
            "-i",
            "--interactive",
            "-x",
            "--exec",
            "--onto",
            "--root",
        ],
        VALUED,
    ) {
        return Plan::Asks;
    }
    match operands(rest, VALUED).0.as_slice() {
        [] => incoming(target, &["@{upstream}"]),
        [upstream] => incoming(target, &[upstream]),
        _ => Plan::Asks,
    }
}

/// `git cherry-pick` and `git revert` apply what each named commit changed.
fn pick(target: &Target, rest: &[&str]) -> Plan {
    const VALUED: &[&str] = &["-m", "-X", "--mainline", "--strategy", "--strategy-option"];
    if has(rest, &["--quit"], VALUED) {
        return Plan::Leaves;
    }
    if widens(
        rest,
        &[
            "--abort",
            "--continue",
            "--skip",
            "-m",
            "--mainline",
            "--stdin",
        ],
        VALUED,
    ) {
        return Plan::Asks;
    }
    let words = operands(rest, VALUED).0;
    if words.is_empty() {
        return Plan::Asks;
    }
    words
        .iter()
        .map(|word| {
            if word.contains("..") || word.starts_with('^') {
                return None;
            }
            revision(word).map(|revision| target.diff(vec![format!("{revision}^"), revision]))
        })
        .collect::<Option<Vec<_>>>()
        .map_or(Plan::Asks, Plan::Queries)
}

/// A stash resets the working tree to `HEAD`, to the index with
/// `--keep-index`, and removes untracked files with `--include-untracked`.
/// `--all` also removes ignored files, which the path checks cover.
fn stash(target: &Target, options: &[&str]) -> Plan {
    const VALUED: &[&str] = &["-m", "--message", "--pathspec-from-file"];
    let mut queries = vec![target.diff(vec!["HEAD".to_owned()])];
    if widens(options, &["-k", "--keep-index"], VALUED) {
        queries.push(target.diff(Vec::new()));
    }
    if widens(options, &["-u", "--include-untracked"], VALUED) {
        queries.push(Query::Untracked {
            paths: vec![target.shield.clone()],
        });
    }
    Plan::Queries(queries)
}

/// `git clean` removes untracked files below its pathspecs or the folder it
/// runs in; with `-x` or `-X` ignored files too, which the path checks cover.
fn clean(target: &Target, rest: &[&str]) -> Plan {
    const VALUED: &[&str] = &["-e", "--exclude"];
    if has(rest, &["-n", "--dry-run"], VALUED) {
        return Plan::Leaves;
    }
    let reached = target.reached(&clean_specs(rest));
    if reached.is_empty() {
        Plan::Leaves
    } else {
        Plan::Queries(vec![Query::Untracked { paths: reached }])
    }
}

/// The pathspecs `git clean` removes below.
pub(super) fn clean_specs<'a>(rest: &[&'a str]) -> Vec<&'a str> {
    let words = operands(rest, &["-e", "--exclude"]).0;
    if words.is_empty() { vec!["."] } else { words }
}
