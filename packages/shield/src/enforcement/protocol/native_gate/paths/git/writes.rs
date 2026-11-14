//! Git commands that write working-tree files from content other than those
//! files: checkouts and restores of paths, index checkouts, moves, removals,
//! patches and stashes. Covering a policy file in `.shield` they are
//! policy-file writes, which agents never make; covering another protected
//! file, such as an agent's hook settings, they follow the protected-file
//! access the policy files set. When the paths they write cannot be
//! determined the user decides.
//!
//! Branch switches, hard resets, merges, rebases, picks, stashes and cleans
//! move the tree to committed content, which an agent can commit itself.
//! They follow ordinary policy while a read-only Git query finds that the
//! tracked policy files stay as they are; when they would change, or the
//! query cannot tell, the user decides.
use super::{Invocation, absolute, resolve_existing};
use crate::enforcement::protocol::native_gate::paths::{Finding, Guard, Protection};
use crate::review::path_access::Change;
use std::path::{Path, PathBuf};
mod query;
#[cfg(test)]
mod tests;
mod tree;
use tree::{Plan, Query};

/// The protection a literal Git invocation needs for what it writes into the
/// policy files.
pub(in crate::enforcement::protocol::native_gate::paths) fn policy_files(
    arguments: &[&str],
    root: &Path,
    cwd: &Path,
    cwd_known: bool,
    guard: Guard<'_>,
) -> Finding {
    policy_files_with(arguments, root, cwd, cwd_known, guard, &query::run)
}

/// Answers a [`Query`] in a folder: whether it finds a change, None when
/// unknown.
type Ask<'a> = &'a dyn Fn(&Path, &Query) -> Option<bool>;

fn policy_files_with(
    arguments: &[&str],
    root: &Path,
    cwd: &Path,
    cwd_known: bool,
    guard: Guard<'_>,
    ask: Ask<'_>,
) -> Finding {
    let Some(invocation) = Invocation::parse(arguments, root, cwd, cwd_known, guard) else {
        // An unmodeled global option hides the subcommand; a writer among the
        // words may be it.
        return if arguments.iter().any(|word| WRITERS.contains(word)) {
            undetermined()
        } else {
            none()
        };
    };
    // Both default bundles and an explicit selection's policy files.
    let policy: Vec<PathBuf> = [crate::layout::SHIELD, crate::layout::GROUPED_BUNDLE]
        .iter()
        .map(|path| resolve_existing(&root.join(path)))
        .chain(crate::layout::explicit_policy_files())
        .collect();
    let target = Target {
        protected: crate::review::integrations::protected_files()
            .iter()
            .filter(|path| {
                // Agent settings the policy files open to change are not
                // protected from Git writes either; the bundles always are.
                guard.allow.agent_settings_change != Change::Allow
                    && ![crate::layout::SHIELD, crate::layout::GROUPED_BUNDLE].contains(path)
            })
            .map(|path| resolve_existing(&root.join(path)))
            .chain(policy.iter().cloned())
            .collect(),
        base: invocation.reach.base.clone(),
        known: invocation.reach.known,
        file: crate::layout::config_file(root),
        policy,
    };
    let rest = invocation.rest;
    let paths = written_paths(&target, invocation.subcommand, rest);
    if paths.0 != Protection::None {
        return paths;
    }
    let globals = &arguments[..arguments.len() - rest.len() - 1];
    settle(
        tree::plan(&target, invocation.subcommand, rest),
        &target,
        plain(globals),
        ask,
    )
}

/// What the paths a command names, or all of them, let it write.
fn written_paths(target: &Target, subcommand: &str, rest: &[&str]) -> Finding {
    match subcommand {
        "checkout" => checkout(target, rest),
        "restore" => restore(target, rest),
        "checkout-index" => checkout_index(target, rest),
        "mv" => target.check(operands(rest, &[]).0),
        "rm" => rm(target, rest),
        "read-tree" if widens(rest, &["-u"], &[]) => undetermined(),
        "apply" => apply(rest),
        "am" if !has(rest, &["--abort", "--quit", "--show-current-patch"], &[]) => undetermined(),
        "stash" => match rest.first().copied() {
            Some("pop" | "apply" | "branch") => undetermined(),
            // `--all` removes ignored files anywhere in the repository.
            _ if tree::stash_options(rest)
                .is_some_and(|options| widens(options, &["-a", "--all"], &["-m", "--message"])) =>
            {
                target.check([":/"])
            }
            _ => none(),
        },
        "clean"
            if !has(rest, &["-n", "--dry-run"], &["-e", "--exclude"])
                && widens(rest, &["-x", "-X"], &["-e", "--exclude"]) =>
        {
            target.check(tree::clean_specs(rest))
        }
        "update-index" => update_index(target, rest),
        "merge-file" if !has(rest, &["-p", "--stdout", "--object-id"], &["-L"]) => {
            target.check(operands(rest, &["-L"]).0.into_iter().take(1))
        }
        _ => none(),
    }
}

/// The plan's answer: ordinary policy when every query finds the policy
/// files unchanged, else the user decides. Queries run only where Git runs
/// in a known folder with no settings that could change what it reads.
fn settle(plan: Plan, target: &Target, ordinary: bool, ask: Ask<'_>) -> Finding {
    let queries = match plan {
        Plan::Leaves => return none(),
        Plan::Asks => return undetermined(),
        Plan::Queries(queries) => queries,
    };
    if !target.known || !ordinary {
        return undetermined();
    }
    for query in &queries {
        match ask(&target.base, query) {
            Some(false) => {}
            Some(true) => {
                return (
                    Protection::Undetermined,
                    query.path().map(|path| path.to_string_lossy().into_owned()),
                );
            }
            None => return undetermined(),
        }
    }
    none()
}

/// Whether the global options leave what Git reads as the query reads it:
/// only `-C` and output settings. A `-c` setting or a replace-object switch
/// could show the command other content.
fn plain(globals: &[&str]) -> bool {
    let mut words = globals.iter();
    while let Some(word) = words.next() {
        if *word == "-C" {
            words.next();
        } else if ![
            "--no-pager",
            "-P",
            "-p",
            "--paginate",
            "--no-optional-locks",
            "--no-advice",
        ]
        .contains(word)
        {
            return false;
        }
    }
    true
}

/// `git update-index` changes index entries of the paths it names, from which
/// a commit and a reset or switch rewrite the files; paths read from input
/// are unknown.
fn update_index(target: &Target, rest: &[&str]) -> Finding {
    const VALUED: &[&str] = &["--index-version"];
    if widens(rest, &["--index-info", "--stdin"], VALUED) {
        return undetermined();
    }
    // `--cacheinfo <mode>,<object>,<path>` names its path last.
    target.check(
        operands(rest, VALUED)
            .0
            .into_iter()
            .map(|word| word.rsplit(',').next().unwrap_or(word)),
    )
}

/// Subcommands this module models.
const WRITERS: &[&str] = &[
    "checkout",
    "restore",
    "checkout-index",
    "read-tree",
    "mv",
    "rm",
    "apply",
    "am",
    "stash",
    "switch",
    "reset",
    "merge",
    "rebase",
    "cherry-pick",
    "revert",
    "pull",
    "bisect",
    "clean",
    "update-index",
    "merge-file",
    "sparse-checkout",
    "filter-branch",
];

struct Target {
    /// The project's protected files and folders: the policy bundles and,
    /// unless the policy files open them, agents' settings.
    protected: Vec<PathBuf>,
    /// Where pathspecs are resolved.
    base: PathBuf,
    /// Whether `base` is where Git runs.
    known: bool,
    /// The file a finding names when a pathspec cannot be resolved.
    file: PathBuf,
    /// The policy bundles and explicit policy files, which tree queries compare.
    policy: Vec<PathBuf>,
}

impl Target {
    /// The protected path a pathspec can select: any of them for magic and
    /// wildcard pathspecs or an unknown folder, else the one it is, lies in
    /// or holds.
    fn covered(&self, spec: &str) -> Option<&Path> {
        if Self::anything(self.known, spec) {
            return Some(&self.file);
        }
        let path = resolve_existing(&absolute(spec, &self.base));
        self.protected
            .iter()
            .find(|protected| protected.starts_with(&path) || path.starts_with(protected))
            .map(PathBuf::as_path)
    }

    fn anything(known: bool, spec: &str) -> bool {
        !known || spec.starts_with(':') || spec.contains(['*', '?', '['])
    }

    /// Every protected path the pathspecs can select.
    fn reached(&self, specs: &[&str]) -> Vec<PathBuf> {
        if specs.iter().any(|spec| Self::anything(self.known, spec)) {
            return self.protected.clone();
        }
        let paths: Vec<PathBuf> = specs
            .iter()
            .map(|spec| resolve_existing(&absolute(spec, &self.base)))
            .collect();
        self.protected
            .iter()
            .filter(|protected| {
                paths
                    .iter()
                    .any(|path| protected.starts_with(path) || path.starts_with(protected))
            })
            .cloned()
            .collect()
    }

    fn check<'a>(&self, specs: impl IntoIterator<Item = &'a str>) -> Finding {
        specs
            .into_iter()
            .find_map(|spec| self.covered(spec))
            .map_or_else(none, |path| {
                (
                    Protection::Blocked,
                    Some(path.to_string_lossy().into_owned()),
                )
            })
    }
}

fn none() -> Finding {
    (Protection::None, None)
}

fn undetermined() -> Finding {
    (Protection::Undetermined, None)
}

/// The options before `--`, by name, as Git reads them: `--name` without its
/// `=value`, and each letter of a short cluster such as `-mu` as `-m` and
/// `-u`, up to a letter that takes a value, which is the rest of the word or
/// the next one. Options in `valued` take values.
fn options(rest: &[&str], valued: &[&str]) -> Vec<String> {
    let mut names = Vec::new();
    let mut words = rest.iter().take_while(|word| **word != "--");
    while let Some(word) = words.next() {
        if let Some(long) = word.strip_prefix("--") {
            let (name, value) = long
                .split_once('=')
                .map_or((long, None), |(n, v)| (n, Some(v)));
            let name = format!("--{name}");
            if value.is_none() && valued.contains(&name.as_str()) {
                words.next();
            }
            names.push(name);
        } else if let Some(cluster) = word.strip_prefix('-') {
            for (at, letter) in cluster.char_indices() {
                let name = format!("-{letter}");
                let takes = valued.contains(&name.as_str());
                names.push(name);
                if takes {
                    if at + letter.len_utf8() == cluster.len() {
                        words.next();
                    }
                    break;
                }
            }
        }
    }
    names
}

/// Whether an option that makes the command harmless is given. Long names
/// match exactly: a looser match would turn a write into a harmless command.
fn has(rest: &[&str], names: &[&str], valued: &[&str]) -> bool {
    options(rest, valued)
        .iter()
        .any(|option| names.contains(&option.as_str()))
}

/// Whether an option that widens what the command writes may be given. Git
/// accepts unique abbreviations of long names, so any prefix matches: a
/// looser match only asks where Git would refuse the word.
fn widens(rest: &[&str], names: &[&str], valued: &[&str]) -> bool {
    options(rest, valued).iter().any(|option| {
        names.iter().any(|name| {
            *name == option.as_str()
                || (option.len() > 2
                    && option.starts_with("--")
                    && name.starts_with(option.as_str()))
        })
    })
}

/// Option words, and the words they consume, removed; `--` ends options.
/// Returns the remaining words and whether `--` was given.
fn operands<'a>(rest: &[&'a str], valued: &[&str]) -> (Vec<&'a str>, Option<usize>) {
    let mut words = Vec::new();
    let mut separator = None;
    let mut iter = rest.iter().copied();
    while let Some(word) = iter.next() {
        if word == "--" {
            separator = Some(words.len());
            words.extend(iter.by_ref());
            break;
        }
        if word.starts_with('-') && word.len() > 1 {
            // A valued option takes the next word unless its value is in this
            // one: after `=`, or after its letter in a short cluster.
            let takes = if word.starts_with("--") {
                valued.contains(&word)
            } else {
                word[1..]
                    .char_indices()
                    .find(|(_, letter)| valued.contains(&format!("-{letter}").as_str()))
                    .is_some_and(|(at, letter)| at + letter.len_utf8() == word.len() - 1)
            };
            if takes {
                iter.next();
            }
            continue;
        }
        words.push(word);
    }
    (words, separator)
}

/// `git checkout [<tree-ish>] [--] <pathspec>...` writes the named paths
/// from a revision or the index; a lone revision switches branches.
fn checkout(target: &Target, rest: &[&str]) -> Finding {
    const VALUED: &[&str] = &["-b", "-B", "--orphan", "--conflict", "--pathspec-from-file"];
    if widens(rest, &["--pathspec-from-file"], VALUED) {
        return undetermined();
    }
    if has(rest, &["-b", "-B", "--orphan"], VALUED) {
        return none();
    }
    let (words, separator) = operands(rest, VALUED);
    // Without `--` the first word is a revision only when Git resolves it as
    // one; a path there is written too. Checking it as a path only ever
    // refuses a revision named like a protected path.
    let specs: &[&str] = match separator {
        Some(index) => &words[index..],
        None => &words,
    };
    target.check(specs.iter().copied())
}

/// `git restore` writes the working tree unless only `--staged` is asked.
fn restore(target: &Target, rest: &[&str]) -> Finding {
    const VALUED: &[&str] = &["-s", "--source", "--conflict", "--pathspec-from-file"];
    if widens(rest, &["--pathspec-from-file"], VALUED) {
        return undetermined();
    }
    if has(rest, &["--staged", "-S"], VALUED) && !widens(rest, &["--worktree", "-W"], VALUED) {
        return none();
    }
    let (words, _) = operands(rest, VALUED);
    target.check(words)
}

/// `git checkout-index` writes index entries: all of them with `--all`.
fn checkout_index(target: &Target, rest: &[&str]) -> Finding {
    const VALUED: &[&str] = &["--prefix", "--stage"];
    if widens(rest, &["--stdin"], VALUED) {
        return undetermined();
    }
    if widens(rest, &["-a", "--all"], VALUED) {
        return target.check(["."]);
    }
    let (words, _) = operands(rest, VALUED);
    target.check(words)
}

/// `git rm` drops the working-tree files it names unless `--cached` keeps
/// them.
fn rm(target: &Target, rest: &[&str]) -> Finding {
    const VALUED: &[&str] = &["--pathspec-from-file"];
    if widens(rest, &["--pathspec-from-file"], VALUED) {
        return undetermined();
    }
    if has(rest, &["--cached"], VALUED) {
        return none();
    }
    target.check(operands(rest, VALUED).0)
}

/// `git apply` writes the files its patches name unless it only reports or
/// changes the index; the patches are not read here.
fn apply(rest: &[&str]) -> Finding {
    const VALUED: &[&str] = &[
        "-p",
        "-C",
        "--directory",
        "--exclude",
        "--include",
        "--whitespace",
        "--build-fake-ancestor",
    ];
    let reports = has(
        rest,
        &["--check", "--stat", "--numstat", "--summary"],
        VALUED,
    );
    if (reports && !widens(rest, &["--apply"], VALUED))
        || (has(rest, &["--cached"], VALUED) && !widens(rest, &["--index"], VALUED))
    {
        return none();
    }
    undetermined()
}
