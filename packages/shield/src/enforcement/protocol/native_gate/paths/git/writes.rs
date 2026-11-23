//! Git commands that write working-tree files from content other than those
//! files: checkouts and restores of paths, index checkouts, moves, removals,
//! patches and stashes. Covering a policy file in `.shield` they are
//! policy-file writes, which require a proposal or supported restoration review; covering another protected
//! file, such as an agent's hook settings, they follow the protected-file
//! access the policy files set. When the paths they write cannot be
//! determined the user decides.
//!
//! Branch switches, hard resets, merges, rebases, picks, stashes and cleans
//! move the tree to committed content, which an agent can commit itself.
//! They follow ordinary policy while a read-only Git query finds that the
//! tracked policy files stay as they are; when they would change, or the
//! query cannot tell, the user decides.
use super::{Access, Invocation, Reach, classify, resolve_existing};
#[cfg(test)]
use crate::enforcement::protocol::native_gate::paths::Guard;
use crate::enforcement::protocol::native_gate::paths::{Finding, Protection};
use crate::review::path_access::Change;
use std::path::{Path, PathBuf};
mod query;
pub(super) use query::worktree_root;
#[cfg(test)]
mod tests;
mod tree;
use tree::{Plan, Query};

/// The protection a literal Git invocation needs for what it writes into the
/// policy files.
#[cfg(test)]
pub(super) fn policy_files(
    arguments: &[&str],
    root: &Path,
    cwd: &Path,
    cwd_known: bool,
    guard: Guard<'_>,
) -> Finding {
    policy_files_with(
        arguments,
        root,
        cwd,
        cwd_known,
        guard,
        &|target, question| query::run(&target.base, root, target.worktree()?, question),
    )
}

/// Answers a [`Query`] in a folder: whether it finds a change, None when
/// unknown.
type Ask<'a> = &'a dyn Fn(&Target, &Query) -> Option<bool>;

#[cfg(test)]
fn policy_files_with(
    arguments: &[&str],
    root: &Path,
    cwd: &Path,
    cwd_known: bool,
    guard: Guard<'_>,
    ask: Ask<'_>,
) -> Finding {
    let Some(invocation) = Invocation::parse(arguments) else {
        return unknown(arguments);
    };
    inspect_with(
        &invocation,
        &invocation.reach(root, cwd, cwd_known, guard),
        ask,
    )
    .finding
}

/// An unmodeled global option can hide a writer among the submitted words.
pub(super) fn unknown(arguments: &[&str]) -> Finding {
    if arguments.iter().any(|word| WRITERS.contains(word)) {
        undetermined()
    } else {
        none()
    }
}

pub(super) fn inspect(invocation: &Invocation<'_>, reach: &Reach<'_>) -> super::super::Inspection {
    inspect_with(invocation, reach, &|target, question| {
        query::run(&target.base, reach.root, target.worktree()?, question)
    })
}

fn inspect_with(
    invocation: &Invocation<'_>,
    reach: &Reach<'_>,
    ask: Ask<'_>,
) -> super::super::Inspection {
    let root = reach.root;
    let guard = reach.guard;
    // Both default bundles and an explicit selection's policy files.
    let policy: Vec<PathBuf> = [crate::layout::SHIELD, crate::layout::GROUPED_BUNDLE]
        .iter()
        .map(|path| resolve_existing(&root.join(path)))
        .chain(crate::layout::explicit_policy_files())
        .collect();
    let rest = invocation.rest;
    let target = Target {
        protected: policy
            .iter()
            .cloned()
            .chain(
                crate::review::integrations::protected_files()
                    .iter()
                    .filter(|path| {
                        // Agent settings the policy files open to change are not
                        // protected from Git writes either; the bundles always are.
                        guard.allow.agent_settings_change != Change::Allow
                            && ![crate::layout::SHIELD, crate::layout::GROUPED_BUNDLE]
                                .contains(path)
                    })
                    .map(|path| resolve_existing(&root.join(path))),
            )
            .collect(),
        base: reach.base.clone(),
        known: reach.known,
        reach,
        mapped: std::cell::OnceCell::new(),
        file: crate::layout::config_file(root),
    };
    if let Some(paths) = super::restoration::paths(invocation) {
        return if let Some(worktree) = reach.worktree().and_then(Path::to_str) {
            target.restore(paths, worktree, invocation.arguments)
        } else {
            super::super::Inspection::plain(target.check(paths.iter().copied()))
        };
    }
    if invocation.subcommand == "stash"
        && let Some(finding) = stash_all(&target, invocation, ask)
    {
        return super::super::Inspection::plain(finding);
    }
    let paths = written_paths(&target, invocation.subcommand, rest);
    if paths.0 != Protection::None {
        return super::super::Inspection::plain(paths);
    }
    super::super::Inspection::plain(settle(
        tree::plan(&target, invocation.subcommand, rest),
        &target,
        invocation.ordinary,
        ask,
    ))
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
        "stash" if rest.first() == Some(&"branch") => undetermined(),
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

const STASH_VALUES: &[&str] = &["-m", "--message", "--pathspec-from-file"];

fn stash_all(target: &Target, invocation: &Invocation<'_>, ask: Ask<'_>) -> Option<Finding> {
    let options = tree::stash_options(invocation.rest)?;
    if !widens(options, &["-a", "--all"], STASH_VALUES) {
        return None;
    }
    // The hook cannot bind what Git will read from a file or stdin later.
    if invocation.rest.first() != Some(&"save")
        && widens(options, &["--pathspec-from-file"], STASH_VALUES)
    {
        return Some((Protection::UnverifiedStashPaths, None));
    }
    if invocation.ordinary
        && invocation.rest.first() == Some(&"push")
        && let Some(specs) = stash_all_paths(options)
        && let Some(worktree) = target.worktree()
        && target.base.starts_with(worktree)
    {
        let protected = specs.iter().find_map(|spec| {
            let (base, path) = spec
                .strip_prefix(":(top,literal)")
                .map_or((target.base.as_path(), *spec), |path| (worktree, path));
            target.covered_path(&base.join(path))
        });
        if protected.is_some() {
            return Some(blocked_path(protected));
        }
        // Git rolls back only the selected paths, but the stash's second
        // parent records the full index.
        let mut paths = target.tracked_paths();
        paths.extend(
            target
                .cache_paths()
                .filter(|path| path.starts_with(worktree)),
        );
        let staged = Query::Staged { paths };
        return Some(match ask(target, &staged) {
            Some(false) => none(),
            _ => (Protection::UnverifiedStashIndex, None),
        });
    }
    // With no verified selection, `--all` can remove ignored files anywhere.
    Some(target.check([":/"]))
}

/// Only these `push` options have modeled arity and preserve the exact list
/// after the separator. A message spelled `--` remains a message.
fn stash_all_paths<'a>(options: &'a [&'a str]) -> Option<&'a [&'a str]> {
    let mut words = options.iter();
    let mut all = false;
    while let Some(&word) = words.next() {
        match word {
            "-a" | "--all" => all = true,
            "-q" | "--quiet" => {}
            "-m" | "--message" => {
                words.next()?;
            }
            "--" => {
                let specs = words.as_slice();
                return (all && !specs.is_empty() && specs.iter().all(|spec| stash_literal(spec)))
                    .then_some(specs);
            }
            word if word.starts_with("--message=") => {}
            _ => return None,
        }
    }
    None
}

fn stash_literal(spec: &str) -> bool {
    let path = spec.strip_prefix(":(top,literal)").unwrap_or(spec);
    !path.contains([':', '\\', '*', '?', '['])
        && !path.chars().any(char::is_control)
        && path.split('/').all(|part| !matches!(part, "" | "." | ".."))
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
        match ask(target, query) {
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

struct Target<'a> {
    /// The project's protected files and folders: the policy bundles and,
    /// unless the policy files open them, agents' settings.
    protected: Vec<PathBuf>,
    /// Where pathspecs are resolved.
    base: PathBuf,
    /// Whether `base` is where Git runs.
    known: bool,
    reach: &'a Reach<'a>,
    mapped: std::cell::OnceCell<Vec<PathBuf>>,
    /// The file a finding names when a pathspec cannot be resolved.
    file: PathBuf,
}

impl Target<'_> {
    /// Tree operations reach relocated settings inside this worktree, while
    /// direct path checks must also retain settings outside it.
    fn tracked_paths(&self) -> Vec<PathBuf> {
        let mut paths = self.protected.clone();
        if let Some(root) = self.worktree() {
            paths.extend(self.relocated().filter_map(|path| {
                if path.starts_with(root) {
                    Some(path)
                } else if root.starts_with(&path) {
                    Some(root.to_path_buf())
                } else {
                    None
                }
            }));
        }
        paths
    }

    fn relocated(&self) -> impl Iterator<Item = PathBuf> + '_ {
        self.reach
            .guard
            .working_files
            .into_iter()
            .flat_map(|scope| scope.protected.iter())
            .filter(|_| self.reach.guard.allow.agent_settings_change != Change::Allow)
            .flat_map(|path| [path.clone(), resolve_existing(path)])
    }

    fn cache_paths(&self) -> impl Iterator<Item = PathBuf> + '_ {
        self.reach
            .guard
            .cache
            .into_iter()
            .flat_map(|path| [path.to_path_buf(), resolve_existing(path)])
    }

    fn restore(
        &self,
        specs: &[&str],
        worktree: &str,
        arguments: &[&str],
    ) -> super::super::Inspection {
        let Some(cwd) = self.base.to_str() else {
            return super::super::Inspection::plain((
                Protection::Blocked,
                Some(self.file.to_string_lossy().into_owned()),
            ));
        };
        let mut result = super::super::Inspection::plain(none());
        result.restorations.worktrees.insert(worktree.to_owned());
        result
            .restorations
            .calls
            .push(super::super::RestorationCall {
                arguments: arguments
                    .iter()
                    .map(|argument| (*argument).to_owned())
                    .collect(),
                cwd: cwd.to_owned(),
                worktree: worktree.to_owned(),
            });
        let settings: Vec<_> = Reach::settings(Path::new(worktree))
            .chain(Reach::settings(self.reach.root))
            .map(|(_, path)| path)
            .collect();
        let closed = super::super::Guard {
            allow: crate::review::path_access::PathAccess::default(),
            ..self.reach.guard
        };
        for spec in specs {
            let mut selected = none();
            let Some(literal) = self.reach.literal_path(spec) else {
                result.finding = (
                    Protection::Blocked,
                    Some(self.file.to_string_lossy().into_owned()),
                );
                return result;
            };
            let resolved = super::resolved("", &literal);
            for path in [&literal, &resolved] {
                let group =
                    super::super::group(&path.to_string_lossy(), &self.base, self.reach.guard);
                let finding = classify(path, self.reach.guard, Access::Restore);
                result.restorations.policy_change |= finding == Protection::Undetermined;
                result.restorations.settings_change |= group == "settings"
                    && classify(path, closed, Access::Restore) == Protection::Blocked;
                result.restorations.settings_change |= settings.contains(path);
                if finding > selected.0 {
                    selected = (finding, Some(path.to_string_lossy().into_owned()));
                }
            }
            // An ancestor or directory selection can also reach fixed state.
            if selected.0 < Protection::Blocked
                && (resolved.is_dir() || selected.0 == Protection::None)
                && let Some(path) = self.covered(spec)
            {
                selected = (
                    Protection::Blocked,
                    Some(path.to_string_lossy().into_owned()),
                );
            }
            result.finding = super::strictest([result.finding, selected].into_iter());
        }
        result
    }
    fn worktree(&self) -> Option<&Path> {
        self.reach.worktree()
    }

    fn paths(&self) -> &[PathBuf] {
        self.mapped.get_or_init(|| {
            let mut paths = self.protected.clone();
            paths.extend(self.relocated());
            paths.extend(self.cache_paths());
            if let Some(root) = self.worktree() {
                paths.extend(
                    crate::review::integrations::protected_files()
                        .iter()
                        .filter(|name| {
                            self.reach.guard.allow.agent_settings_change != Change::Allow
                                || [crate::layout::SHIELD, crate::layout::GROUPED_BUNDLE]
                                    .contains(name)
                        })
                        .flat_map(|name| {
                            let path = root.join(name);
                            [path.clone(), resolve_existing(&path)]
                        }),
                );
            }
            paths.sort();
            paths.dedup();
            paths
        })
    }
    /// The protected path a pathspec can select: any of them for unmodeled
    /// magic, wildcards or an unknown folder, else the one it is, lies in or holds.
    fn covered(&self, spec: &str) -> Option<PathBuf> {
        let Some(literal) = self.reach.literal_path(spec) else {
            return Some(self.file.clone());
        };
        self.covered_path(&literal)
    }

    fn covered_path(&self, literal: &Path) -> Option<PathBuf> {
        if classify(literal, self.reach.guard, Access::Write) == Protection::Blocked {
            return Some(literal.to_path_buf());
        }
        let path = super::resolved("", literal);
        if classify(&path, self.reach.guard, Access::Write) == Protection::Blocked {
            return Some(path);
        }
        self.paths()
            .iter()
            .find(|protected| protected.starts_with(&path) || path.starts_with(protected))
            .cloned()
    }

    fn path(&self, spec: &str) -> Option<PathBuf> {
        self.reach.path(spec)
    }

    /// Every protected path the pathspecs can select.
    fn reached(&self, specs: &[&str]) -> Vec<PathBuf> {
        let Some(paths) = specs
            .iter()
            .map(|spec| self.path(spec))
            .collect::<Option<Vec<_>>>()
        else {
            return self.paths().to_vec();
        };
        self.paths()
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
        blocked_path(specs.into_iter().find_map(|spec| self.covered(spec)))
    }
}

fn blocked_path(path: Option<PathBuf>) -> Finding {
    path.map_or_else(none, |path| {
        (
            Protection::Blocked,
            Some(path.to_string_lossy().into_owned()),
        )
    })
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
