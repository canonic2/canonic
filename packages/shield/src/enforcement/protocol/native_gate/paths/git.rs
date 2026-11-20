//! Shared literal Git invocation parsing, named-path access, printed settings
//! and protected writes. Content reach includes revision paths, patch output
//! and searches, narrowed only by pathspecs Git is sure to use.
//!
//! Git output reaches tracked content, so review state and the cache, which
//! `.shield/.gitignore` keeps out of Git, are not part of these footprints.
//! Object IDs a command resolves itself, such as a tag naming a blob, are
//! outside this literal model.
use super::{Access, Finding, Guard, Protection, absolute, classify, resolve_existing, strictest};
mod restoration;
mod writes;
use crate::review::integrations::protected_files;
use std::cell::OnceCell;
use std::path::{Path, PathBuf};
#[cfg(test)]
use writes::policy_files;

/// Subcommands that read the working tree and change only the repository.
const RECORDS: &[&str] = &[
    "add",
    "status",
    "diff",
    "log",
    "show",
    "blame",
    "commit",
    "ls-files",
    "check-ignore",
    "grep",
    "shortlog",
];
/// Subcommands whose output can include file contents.
const CONTENT: &[&str] = &[
    "show",
    "log",
    "whatchanged",
    "reflog",
    "diff",
    "diff-index",
    "diff-files",
    "diff-tree",
    "range-diff",
    "difftool",
    "stash",
    "status",
    "grep",
    "cat-file",
    "archive",
    "format-patch",
    "fast-export",
    "ls-files",
];
/// Recognized built-ins without a modeled file-content footprint. Unknown names
/// may be aliases or external Git commands and cannot use this exemption.
const OTHER_BUILTINS: &[&str] = &[
    "branch",
    "tag",
    "remote",
    "fetch",
    "push",
    "pull",
    "checkout",
    "switch",
    "restore",
    "reset",
    "clean",
    "merge",
    "rebase",
    "cherry-pick",
    "revert",
    "worktree",
    "init",
    "clone",
    "config",
    "version",
    "help",
    "rev-parse",
    "rev-list",
    "for-each-ref",
    "ls-tree",
    "ls-remote",
    "update-index",
    "update-ref",
    "symbolic-ref",
    "read-tree",
    "write-tree",
    "hash-object",
    "repack",
    "gc",
    "maintenance",
    "fsck",
    "prune",
    "bisect",
    "submodule",
    "notes",
    "rerere",
    "rm",
    "mv",
    "apply",
    "am",
    "bundle",
    "check-attr",
    "check-ref-format",
    "credential",
    "describe",
    "count-objects",
    "verify-commit",
    "verify-tag",
    "verify-pack",
    "merge-base",
    "interpret-trailers",
];
const VALUED_GLOBALS: &[&str] = &[
    "-C",
    "-c",
    "--git-dir",
    "--work-tree",
    "--namespace",
    "--config-env",
    "--super-prefix",
];
const FLAG_GLOBALS: &[&str] = &[
    "-P",
    "-p",
    "--no-pager",
    "--paginate",
    "--bare",
    "--literal-pathspecs",
    "--glob-pathspecs",
    "--noglob-pathspecs",
    "--icase-pathspecs",
    "--no-optional-locks",
    "--no-replace-objects",
    "--no-lazy-fetch",
    "--no-advice",
];
/// Options that may take their value as the next word, which is then never a
/// pathspec. Listing a flag here only widens the checked footprint.
const VALUED: &[&str] = &[
    "-n",
    "-S",
    "-G",
    "-O",
    "-I",
    "-e",
    "-f",
    "-A",
    "-B",
    "-C",
    "-m",
    "--author",
    "--committer",
    "--grep",
    "--since",
    "--until",
    "--after",
    "--before",
    "--max-count",
    "--skip",
    "--min-parents",
    "--max-parents",
    "--output",
    "--src-prefix",
    "--dst-prefix",
    "--line-prefix",
    "--format",
    "--pretty",
    "--encoding",
    "--date",
    "--diff-filter",
    "--word-diff-regex",
    "--color-moved-ws",
    "--anchored",
    "--rotate-to",
    "--skip-to",
    "--find-object",
    "--exclude",
    "--decorate-refs",
    "--decorate-refs-exclude",
    "--max-depth",
    "--threads",
    "--remote",
    "--prefix",
    "--add-file",
    "--ignore-matching-lines",
    "--inter-hunk-context",
    "--diff-algorithm",
    "--diff-merges",
    "--unified",
    "-U",
    "-M",
    "-l",
    "-X",
    // A flag for `ls-files` and `grep`, but `format-patch` and `archive` name a file.
    "-o",
];
/// Known no-value options, including optional values accepted only with `=`.
/// Anything outside this vocabulary keeps the complete output footprint.
const FLAGS: &[&str] = &[
    "--oneline",
    "--cached",
    "--staged",
    "--no-index",
    "--exit-code",
    "--relative",
    "--no-relative",
    "--text",
    "--ignore-space-change",
    "--ignore-all-space",
    "--ignore-space-at-eol",
    "--ignore-cr-at-eol",
    "--ignore-blank-lines",
    "--minimal",
    "--patience",
    "--histogram",
    "--no-ext-diff",
    "--textconv",
    "--no-textconv",
    "--no-renames",
    "--find-copies-harder",
    "--full-index",
    "--color",
    "--no-color",
    "--abbrev",
    "--no-abbrev",
    "--no-prefix",
    "--default-prefix",
    "--short",
    "--porcelain",
    "--branch",
    "--verbose",
    "--show-stash",
    "--ahead-behind",
    "--no-ahead-behind",
    "--untracked-files",
    "--ignored",
    "--column",
    "--no-column",
    "--stage",
    "--unmerged",
    "--resolve-undo",
    "--deleted",
    "--modified",
    "--others",
    "--exclude-standard",
    "--error-unmatch",
    "--full-name",
    "--deduplicate",
    "--eol",
    "--and",
    "--or",
    "--not",
    "--all-match",
    "--heading",
    "--break",
    "--graph",
    "--decorate",
    "--no-decorate",
    "--all",
    "--branches",
    "--tags",
    "--remotes",
    "--reverse",
    "--first-parent",
    "--merges",
    "--no-merges",
    "--topo-order",
    "--date-order",
    "--author-date-order",
    "--left-right",
    "--left-only",
    "--right-only",
    "--cherry-pick",
    "--cherry-mark",
    "--cherry",
    "--boundary",
    "--parents",
    "--children",
    "--follow",
    "--source",
    "--abbrev-commit",
    "--no-abbrev-commit",
    "--relative-date",
    "--walk-reflogs",
    "--no-walk",
    "--do-walk",
    "--ancestry-path",
    "--full-history",
    "--simplify-by-decoration",
    "--simplify-merges",
    "--show-pulls",
    "--dense",
    "--sparse",
    "--exclude-first-parent-only",
    "--show-linear-break",
    "--log-size",
    "--full-diff",
    "--show-signature",
    "--no-show-signature",
    "--notes",
    "--no-notes",
    "--show-notes",
    "--use-mailmap",
    "--mailmap",
    "--no-mailmap",
    "--invert-grep",
    "--regexp-ignore-case",
    "--basic-regexp",
    "--extended-regexp",
    "--fixed-strings",
    "--perl-regexp",
    "--pickaxe-regex",
    "--pickaxe-all",
    "--count",
    "--merge-base",
    "--find-renames",
    "--find-copies",
    "--renames",
    "--submodule",
    "--ignore-submodules",
    "--check",
    "--ws-error-highlight",
    "--indent-heuristic",
    "--no-indent-heuristic",
    "--ita-invisible-in-index",
    "--no-commit-id",
    "--root",
    "--directory",
    "--no-empty-directory",
    "--recurse-submodules",
    "--no-recurse-submodules",
    "--killed",
    "--line-number",
    "--files-with-matches",
    "--files-without-match",
    "--ignore-case",
    "--word-regexp",
    "--invert-match",
    "--untracked",
    "--no-exclude-standard",
    "--only-matching",
    "--no-filename",
    "--null",
    "--show-function",
    "--recursive",
    "--no-recursive",
];
/// Options that print patches.
const PATCH: &[&str] = &[
    "-p",
    "-u",
    "-W",
    "-c",
    "-m",
    "--patch",
    "--patch-with-stat",
    "--patch-with-raw",
    "--cc",
    "--dd",
    "--remerge-diff",
    "--diff-merges",
    "--unified",
    "--word-diff",
    "--color-words",
    "--function-context",
    "--binary",
    "--ext-diff",
];
/// Options that replace a default patch with names or statistics.
const SUMMARY: &[&str] = &[
    "-s",
    "--no-patch",
    "--stat",
    "--numstat",
    "--shortstat",
    "--dirstat",
    "--dirstat-by-file",
    "--summary",
    "--name-only",
    "--name-status",
    "--raw",
    "--compact-summary",
    "--quiet",
];
/// Short options whose remaining letters are an attached value.
const ATTACHED_SHORT: &str = "nSGOIMCBlX";
const OBJECT_TYPES: &[&str] = &["blob", "tree", "commit", "tag"];

/// Named-path access: listings and status name settings without printing them.
pub(super) fn access(arguments: &[&str]) -> Access {
    let Some(invocation) = Invocation::parse(arguments) else {
        return Access::Write;
    };
    if restoration::paths(&invocation).is_some() {
        return Access::Restore;
    }
    if invocation.execution_override
        || invocation.elsewhere
        || !RECORDS.contains(&invocation.subcommand)
        || invocation.rest.iter().any(|a| a.starts_with("--output"))
    {
        return Access::Write;
    }
    let rest = invocation.rest;
    match invocation.subcommand {
        "ls-files" if !object_ids(rest) => Access::Metadata,
        "status" if !verbose(rest) && !status_object_ids(rest) => Access::Metadata,
        "check-ignore" => Access::Metadata,
        _ => Access::Index,
    }
}

/// Short option letters before `stop`, whose remaining letters are a value.
fn short_letters(rest: &[&str], stop: char) -> String {
    options(rest)
        .filter(|option| !option.starts_with("--"))
        .flat_map(|option| option[1..].chars().take_while(move |&c| c != stop))
        .collect()
}

/// Stage, unmerged and format listings print object IDs, which name contents.
fn object_ids(rest: &[&str]) -> bool {
    options(rest).any(|option| {
        ["--stage", "--unmerged", "--resolve-undo", "--format"].contains(&option_name(option))
    }) || short_letters(rest, 'x').contains(['s', 'u'])
}

/// Verbose status prints staged and, when repeated, unstaged changes.
fn verbose(rest: &[&str]) -> bool {
    options(rest).any(|option| option == "--verbose") || short_letters(rest, 'u').contains('v')
}

fn status_object_ids(rest: &[&str]) -> bool {
    options(rest).any(|option| matches!(option, "--porcelain=v2" | "--porcelain=2"))
}

/// The strictest protection among content and writes, using one parsed invocation.
#[cfg(test)]
pub(super) fn footprint(
    arguments: &[&str],
    root: &Path,
    cwd: &Path,
    cwd_known: bool,
    guard: Guard<'_>,
) -> Finding {
    inspected(arguments, root, cwd, cwd_known, guard).finding
}

pub(super) fn inspected(
    arguments: &[&str],
    root: &Path,
    cwd: &Path,
    cwd_known: bool,
    guard: Guard<'_>,
) -> super::Inspection {
    if matches!(arguments, ["--version" | "-v" | "--help" | "-h"]) {
        return super::Inspection::plain((Protection::None, None));
    }
    let Some(invocation) = Invocation::parse(arguments) else {
        // An unmodeled global option hides where the subcommand starts.
        return super::Inspection::plain(strictest(
            [Reach::everything(root, guard), writes::unknown(arguments)].into_iter(),
        ));
    };
    let reach = invocation.reach(root, cwd, cwd_known, guard);
    let mut result = writes::inspect(&invocation, &reach);
    result.finding = strictest([content_of(&invocation, &reach), result.finding].into_iter());
    result
}

#[cfg(test)]
fn content(
    arguments: &[&str],
    root: &Path,
    cwd: &Path,
    cwd_known: bool,
    guard: Guard<'_>,
) -> Finding {
    let Some(invocation) = Invocation::parse(arguments) else {
        return Reach::everything(root, guard);
    };
    content_of(&invocation, &invocation.reach(root, cwd, cwd_known, guard))
}

fn content_of(invocation: &Invocation<'_>, reach: &Reach<'_>) -> Finding {
    if invocation.execution_override {
        return reach.repository();
    }
    let rest = invocation.rest;
    if CONTENT.contains(&invocation.subcommand)
        && options(rest).any(|option| consumes_next(option).is_none())
    {
        return reach.repository();
    }
    // `<rev>:<path>` and `:<path>` name one blob or tree that content
    // subcommands print; cat-file checks its own modes.
    let revisions =
        if CONTENT.contains(&invocation.subcommand) && invocation.subcommand != "cat-file" {
            revision_paths(reach, rest)
        } else {
            (Protection::None, None)
        };
    // A line-range log follows content Shield does not resolve.
    if rest.iter().any(|word| word.starts_with("-L")) {
        return strictest([revisions, reach.repository()].into_iter());
    }
    let printed = match invocation.subcommand {
        "show" => {
            let revisions: Vec<&str> = before_separator(rest)
                .filter(|word| !word.starts_with('-'))
                .collect();
            // A literal object ID can name a blob, whatever the options.
            if revisions.iter().copied().any(object_id) {
                reach.repository()
            // Revision paths alone print their own blobs, not a commit patch.
            } else if !revisions.is_empty()
                && revisions.iter().all(|word| revision_path(word).is_some())
            {
                (Protection::None, None)
            } else if patch(rest) || !summary(rest) {
                reach.scoped(&pathspecs(rest, &reach.base, false), Scope::Repository)
            } else {
                (Protection::None, None)
            }
        }
        "diff" | "range-diff" | "difftool" if patch(rest) || !summary(rest) => {
            reach.scoped(&pathspecs(rest, &reach.base, false), Scope::Repository)
        }
        "log" | "whatchanged" | "reflog" | "diff-index" | "diff-files" | "diff-tree"
            if patch(rest) =>
        {
            reach.scoped(&pathspecs(rest, &reach.base, false), Scope::Repository)
        }
        "stash" => match rest.split_first() {
            // stash.showPatch can turn a plain show into a patch.
            Some((&"show", rest)) if patch(rest) || !summary(rest) => {
                reach.scoped(&pathspecs(rest, &reach.base, false), Scope::Repository)
            }
            Some((&"list", rest)) if patch(rest) => reach.repository(),
            _ => (Protection::None, None),
        },
        "status" if verbose(rest) || status_object_ids(rest) => {
            reach.scoped(&pathspecs(rest, &reach.base, false), Scope::Repository)
        }
        "ls-files" if object_ids(rest) => {
            reach.scoped(&pathspecs(rest, &reach.base, false), Scope::Folder)
        }
        "grep" => reach.scoped(&pathspecs(rest, &reach.base, true), Scope::Folder),
        "cat-file" => cat_file(reach, rest),
        "archive" | "format-patch" | "fast-export" => {
            reach.scoped(&pathspecs(rest, &reach.base, false), Scope::Repository)
        }
        name if CONTENT.contains(&name)
            || RECORDS.contains(&name)
            || OTHER_BUILTINS.contains(&name) =>
        {
            (Protection::None, None)
        }
        _ => reach.repository(),
    };
    strictest([revisions, printed].into_iter())
}

/// Content modes print each named object; type, size and existence checks do not.
fn cat_file(reach: &Reach<'_>, rest: &[&str]) -> Finding {
    let objects: Vec<&str> = rest
        .iter()
        .copied()
        .filter(|word| !word.starts_with('-'))
        .collect();
    if options(rest).any(|o| o.starts_with("--batch") && !o.starts_with("--batch-check")) {
        return reach.repository();
    }
    let content = options(rest).any(|o| ["-p", "--textconv", "--filters"].contains(&o))
        || objects
            .first()
            .is_some_and(|word| OBJECT_TYPES.contains(word));
    if !content {
        return (Protection::None, None);
    }
    // Revision paths are checked by name; any other object can be a blob.
    if objects
        .iter()
        .any(|word| !OBJECT_TYPES.contains(word) && revision_path(word).is_none())
    {
        reach.repository()
    } else {
        revision_paths(reach, rest)
    }
}

fn revision_paths(reach: &Reach<'_>, rest: &[&str]) -> Finding {
    strictest(
        before_separator(rest)
            .filter(|word| !word.starts_with('-'))
            .filter_map(|word| revision_path(word).map(|path| reach.revision_named(path, word))),
    )
}

struct Invocation<'a> {
    arguments: &'a [&'a str],
    subcommand: &'a str,
    rest: &'a [&'a str],
    directories: Vec<&'a str>,
    elsewhere: bool,
    execution_override: bool,
    /// Only directory and output globals leave fixed Git inspection equivalent.
    ordinary: bool,
}

impl<'a> Invocation<'a> {
    /// Interpret global options once, retaining directory changes in Git order.
    fn parse(arguments: &'a [&'a str]) -> Option<Self> {
        let mut directories = Vec::new();
        let mut elsewhere = false;
        let mut execution_override = false;
        let mut ordinary = true;
        let mut index = 0;
        loop {
            let word = *arguments.get(index)?;
            let name = option_name(word);
            if word == "-C" {
                directories.push(*arguments.get(index + 1)?);
                index += 2;
            } else if word == "-c" || word == "--config-env" {
                let config = arguments.get(index + 1)?;
                execution_override |= overrides_execution(config);
                elsewhere |= overrides_repository(config);
                ordinary &= word == "-c" && crate::location::git_noop_editor(config);
                index += 2;
            } else if let Some(config) = word.strip_prefix("--config-env=") {
                execution_override |= overrides_execution(config);
                elsewhere |= overrides_repository(config);
                ordinary = false;
                index += 1;
            } else if VALUED_GLOBALS.contains(&word) {
                arguments.get(index + 1)?;
                elsewhere |= ["--git-dir", "--work-tree"].contains(&word);
                ordinary = false;
                index += 2;
            } else if (name != word && VALUED_GLOBALS.contains(&name))
                || FLAG_GLOBALS.contains(&word)
                || name == "--exec-path"
            {
                elsewhere |= ["--git-dir", "--work-tree"].contains(&name);
                elsewhere |= word == "--bare";
                execution_override |= name == "--exec-path";
                ordinary &= [
                    "--no-pager",
                    "-P",
                    "-p",
                    "--paginate",
                    "--no-optional-locks",
                    "--no-advice",
                ]
                .contains(&word);
                index += 1;
            } else if word.starts_with('-') {
                return None;
            } else {
                return Some(Self {
                    arguments,
                    subcommand: word,
                    rest: &arguments[index + 1..],
                    directories,
                    elsewhere,
                    execution_override,
                    ordinary,
                });
            }
        }
    }

    fn reach<'g>(
        &self,
        root: &'g Path,
        cwd: &Path,
        cwd_known: bool,
        guard: Guard<'g>,
    ) -> Reach<'g> {
        let base = self
            .directories
            .iter()
            .filter(|path| !path.is_empty())
            .fold(cwd.to_path_buf(), |base, directory| {
                resolved(directory, &base)
            });
        Reach {
            root,
            base,
            guard,
            known: cwd_known && !self.elsewhere,
            root_query: self.ordinary,
            worktree: OnceCell::new(),
        }
    }
}

fn overrides_execution(config: &str) -> bool {
    let key = config
        .split('=')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    key.starts_with("alias.") || ["core.pager", "diff.external"].contains(&key.as_str())
}

fn overrides_repository(config: &str) -> bool {
    let key = config
        .split('=')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    key.starts_with("include.")
        || key.starts_with("includeif.")
        || [
            "core.worktree",
            "core.bare",
            "core.repositoryformatversion",
            "extensions.worktreeconfig",
            "extensions.objectformat",
        ]
        .contains(&key.as_str())
}

/// What a footprint without pathspecs covers.
#[derive(Clone, Copy)]
enum Scope {
    /// The whole repository, as for patches.
    Repository,
    /// The folder Git runs in and below, as for `git grep`.
    Folder,
}

/// The agent settings a Git footprint can reach.
struct Reach<'a> {
    root: &'a Path,
    base: PathBuf,
    guard: Guard<'a>,
    /// Whether `base` is where Git runs and finds its repository.
    known: bool,
    /// Whether the submitted globals permit literal top-pathspec narrowing.
    root_query: bool,
    worktree: OnceCell<Option<PathBuf>>,
}

impl Reach<'_> {
    fn worktree(&self) -> Option<&Path> {
        if !self.known {
            return None;
        }
        self.worktree
            .get_or_init(|| writes::worktree_root(&self.base))
            .as_deref()
    }

    /// Literal pathspecs use Git's cwd, or the discovered worktree for top magic.
    fn path(&self, spec: &str) -> Option<PathBuf> {
        self.literal_path(spec).map(|path| resolved("", &path))
    }

    /// Keep the submitted spelling too: a settings symlink remains settings.
    fn literal_path(&self, spec: &str) -> Option<PathBuf> {
        if !self.known || spec.contains(['*', '?', '[']) {
            return None;
        }
        if let Some(short) = spec.strip_prefix(":/")
            && short.as_bytes().first().is_some_and(|byte| {
                !byte.is_ascii_alphanumeric() && ![b'.', b'_', b'-'].contains(byte)
            })
        {
            return None;
        }
        if let Some(top) = spec
            .strip_prefix(":/")
            .or_else(|| spec.strip_prefix(":(top)"))
        {
            if !self.root_query || Path::new(top).is_absolute() {
                return None;
            }
            return Some(super::submitted_path(top, self.worktree()?));
        }
        (!spec.starts_with(':')).then(|| super::submitted_path(spec, &self.base))
    }

    fn settings(root: &Path) -> impl Iterator<Item = (&'static str, PathBuf)> + '_ {
        protected_files()
            .iter()
            .filter(|name| !name.starts_with(".shield") && **name != crate::layout::GROUPED_BUNDLE)
            .flat_map(move |name| {
                let path = root.join(name);
                [(*name, path.clone()), (*name, resolve_existing(&path))]
            })
    }

    fn everything(root: &Path, guard: Guard<'_>) -> Finding {
        strictest(Self::settings(root).map(|(name, path)| {
            (
                Self::settings_access(name, &path, guard),
                Some(name.to_owned()),
            )
        }))
    }

    fn settings_access(name: &str, path: &Path, guard: Guard<'_>) -> Protection {
        classify(Path::new(name), guard, Access::Index).max(classify(path, guard, Access::Index))
    }

    /// Settings of the repository being inspected, plus enrolled paths inside it.
    fn reached_settings(&self) -> Vec<(&'static str, PathBuf)> {
        let Some(worktree) = self.worktree() else {
            return Self::settings(self.root).collect();
        };
        let mut settings: Vec<_> = Self::settings(worktree).collect();
        if self.root != worktree {
            settings
                .extend(Self::settings(self.root).filter(|(_, path)| path.starts_with(worktree)));
        }
        settings
    }

    /// Settings at or below `folder`.
    fn below(&self, folder: &Path) -> Finding {
        if !self.known {
            return Self::everything(self.root, self.guard);
        }
        let folder = resolve_existing(folder);
        let settings = self
            .reached_settings()
            .into_iter()
            .filter(|(_, path)| path.starts_with(&folder))
            .map(|(name, path)| {
                (
                    Self::settings_access(name, &path, self.guard),
                    Some(name.to_owned()),
                )
            });
        let relocated = self
            .guard
            .working_files
            .into_iter()
            .flat_map(|scope| scope.protected.iter())
            .filter(|path| path.starts_with(&folder) || resolve_existing(path).starts_with(&folder))
            .map(|path| self.named_path(path, &path.to_string_lossy()));
        strictest(settings.chain(relocated))
    }

    /// Settings in Git's actual repository, with conservative fallback on failure.
    fn repository(&self) -> Finding {
        if self.worktree().is_some() {
            return strictest(self.reached_settings().into_iter().map(|(name, path)| {
                (
                    Self::settings_access(name, &path, self.guard),
                    Some(name.to_owned()),
                )
            }));
        }
        Self::everything(self.root, self.guard)
    }

    /// A path the command names, by its own protection.
    fn named(&self, path: &str, submitted: &str) -> Finding {
        self.named_path(&super::submitted_path(path, &self.base), submitted)
    }

    fn named_path(&self, path: &Path, submitted: &str) -> Finding {
        let level = classify(path, self.guard, Access::Index).max(classify(
            &resolved("", path),
            self.guard,
            Access::Index,
        ));
        (level, Some(submitted.to_owned()))
    }

    /// Revision literals are rooted at the repository, except explicit cwd-relative paths.
    fn revision_named(&self, path: &str, submitted: &str) -> Finding {
        if !self.known {
            return Self::everything(self.root, self.guard);
        }
        if path.is_empty() {
            return self.repository();
        }
        if path.starts_with("./") || path.starts_with("../") {
            return self.named(path, submitted);
        }
        let worktree = self.worktree();
        if worktree.is_none()
            && !resolve_existing(&self.base).starts_with(resolve_existing(self.root))
        {
            return self.repository();
        }
        self.named_path(
            &super::submitted_path(path, worktree.unwrap_or(self.root)),
            submitted,
        )
    }

    fn scoped(&self, pathspecs: &[&str], scope: Scope) -> Finding {
        if pathspecs.is_empty() {
            return match scope {
                Scope::Repository => self.repository(),
                Scope::Folder => self.below(&self.base),
            };
        }
        strictest(pathspecs.iter().map(|spec| {
            match self.literal_path(spec) {
                Some(path) => strictest(
                    [
                        self.named_path(&path, spec),
                        self.below(&resolved("", &path)),
                    ]
                    .into_iter(),
                ),
                None => self.repository(),
            }
        }))
    }
}

/// Resolve links before cleaning `..`, including nonexistent leaf destinations.
fn resolved(path: &str, base: &Path) -> PathBuf {
    absolute("", &resolve_existing(&super::submitted_path(path, base)))
}

/// Pathspecs Git is sure to use: every word after `--`, and earlier words that
/// name existing paths after options whose arity is known. An unknown option
/// prevents narrowing, including a `--` that it might consume as a value. A
/// `git grep` pattern comes first unless `-e` or `-f` gives it.
fn pathspecs<'a>(rest: &[&'a str], base: &Path, pattern_first: bool) -> Vec<&'a str> {
    let mut specs = Vec::new();
    let mut pattern = pattern_first
        && !options(rest).any(|o| {
            ["-e", "-f", "--regexp", "--file"].contains(&option_name(o))
                || (!o.starts_with("--") && o[1..].starts_with(['e', 'f']))
        });
    let mut words = rest.iter().copied().peekable();
    let mut value_next = false;
    while let Some(word) = words.next() {
        if std::mem::take(&mut value_next) {
            continue;
        }
        if word == "--" {
            specs.extend(words);
            break;
        }
        if word.starts_with('-') && word.len() > 1 {
            let Some(next) = consumes_next(word) else {
                return Vec::new();
            };
            value_next = next;
            continue;
        }
        if std::mem::take(&mut pattern) {
            continue;
        }
        if absolute(word, base).symlink_metadata().is_ok() {
            specs.push(word);
        }
    }
    specs
}

/// The path of `<rev>:<path>`, `:<path>` or `:<stage>:<path>`. `:/<text>`
/// names a commit by its message, and options are not revisions.
fn revision_path(word: &str) -> Option<&str> {
    if word.starts_with(":/") {
        return None;
    }
    if let Some(index) = word.strip_prefix(':') {
        return Some(
            index
                .strip_prefix(['0', '1', '2', '3'])
                .and_then(|rest| rest.strip_prefix(':'))
                .unwrap_or(index),
        );
    }
    // The first colon outside `@{...}` reflog selectors ends the revision.
    let mut depth = 0_usize;
    for (index, c) in word.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => depth = depth.saturating_sub(1),
            ':' if depth == 0 => return Some(&word[index + 1..]),
            _ => {}
        }
    }
    None
}

/// A literal abbreviated or full object ID, optionally peeled.
fn object_id(word: &str) -> bool {
    let id = word.split('^').next().unwrap_or(word);
    (4..=64).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_hexdigit())
}

fn before_separator<'a>(rest: &'a [&'a str]) -> impl Iterator<Item = &'a str> + 'a {
    rest.iter().copied().take_while(|word| *word != "--")
}

fn options<'a>(rest: &'a [&'a str]) -> impl Iterator<Item = &'a str> + 'a {
    let mut words = rest.iter().copied();
    let mut ended = false;
    std::iter::from_fn(move || {
        if ended {
            return None;
        }
        while let Some(word) = words.next() {
            if word == "--" {
                ended = true;
                return None;
            }
            if word.starts_with('-') && word.len() > 1 {
                if consumes_next(word) == Some(true) {
                    words.next();
                }
                return Some(word);
            }
        }
        None
    })
}

/// Whether this spelling consumes the next word; `None` means unknown arity.
fn consumes_next(option: &str) -> Option<bool> {
    if option.starts_with("--") {
        let name = option_name(option);
        if VALUED.contains(&name) {
            return Some(!option.contains('='));
        }
        return (FLAGS.contains(&name) || PATCH.contains(&name) || SUMMARY.contains(&name))
            .then_some(false);
    }
    // `-<n>` limits the commit count.
    if option[1..].bytes().all(|b| b.is_ascii_digit()) {
        return Some(false);
    }
    for (index, letter) in option.char_indices().skip(1) {
        let end = index + letter.len_utf8();
        if VALUED
            .iter()
            .any(|value| value.len() == 2 && value.ends_with(letter))
        {
            return Some(end == option.len());
        }
        if !"puWcswbzaqvRiEFHPtrgdDhk".contains(letter) {
            return None;
        }
    }
    Some(false)
}

fn option_name(option: &str) -> &str {
    option.split_once('=').map_or(option, |(name, _)| name)
}

fn patch(rest: &[&str]) -> bool {
    options(rest).any(|option| {
        PATCH.contains(&option_name(option))
            || option.starts_with("-U")
            || (!option.starts_with("--")
                && !option[1..].starts_with(|c| ATTACHED_SHORT.contains(c))
                && option[1..].contains(['p', 'u', 'W']))
    })
}

fn summary(rest: &[&str]) -> bool {
    options(rest).any(|option| SUMMARY.contains(&option_name(option)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn git_global_options_match_real_git_directory_syntax() {
        let root = assert_fs::TempDir::new().unwrap();
        std::fs::create_dir(root.path().join("src")).unwrap();
        for arguments in [
            ["-C", "src", "--version"].as_slice(),
            ["-c", "acme.setting=value", "--version"].as_slice(),
        ] {
            let output = crate::test_support::git()
                .current_dir(root.path())
                .args(arguments)
                .output()
                .unwrap();
            assert!(output.status.success(), "{arguments:?}: {output:?}");
        }
        for argument in ["-Csrc", "-cacme.setting=value"] {
            let output = crate::test_support::git()
                .current_dir(root.path())
                .args([argument, "--version"])
                .output()
                .unwrap();
            println!(
                "{argument}: success={} {}",
                output.status.success(),
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(
                !output.status.success(),
                "attached global unexpectedly accepted: {output:?}"
            );
            assert!(Invocation::parse(&[argument, "show", "HEAD"]).is_none());
        }
    }

    #[test]
    fn git_global_options_preserve_named_path_access() {
        for arguments in [
            &["-C", "src", "show", "HEAD", "--", ".shield/config.yml"][..],
            &["-C", "/acme", "-C", "src", "show", "HEAD"],
            &["--no-pager", "-C", "src", "-C", "..", "show", "HEAD"],
            &["-c", "color.ui=never", "show", "HEAD"],
            &["--config-env=color.ui=ACME_COLOR", "show", "HEAD"],
        ] {
            assert!(access(arguments) == Access::Index, "{arguments:?}");
        }
        for arguments in [
            &["-C", "src", "status", "--short", ".mcp.json"][..],
            &["--no-pager", "-C", "src", "ls-files", ".mcp.json"],
        ] {
            assert!(access(arguments) == Access::Metadata, "{arguments:?}");
        }
        for arguments in [
            &["-C", "src", "diff", "--output=.shield/config.yml"][..],
            &["--no-pager", "show", "--output", ".mcp.json"],
            &["-c", "alias.show=!acme", "show"],
            &["-c", "diff.external=acme", "diff"],
            &["-Csrc", "show", "HEAD"],
            &["-cdiff.external=acme", "diff"],
            &["--config-env=core.pager=ACME_PAGER", "show"],
            &["--git-dir=elsewhere", "show"],
            &["--exec-path=elsewhere", "show"],
            &["--future-global", "show"],
            &["-C"],
            &["-c"],
        ] {
            assert!(access(arguments) == Access::Write, "{arguments:?}");
        }
    }

    #[test]
    fn git_global_options_resolve_directories_without_consuming_option_values_as_paths() {
        let root = assert_fs::TempDir::new().unwrap();
        let root = root.path().canonicalize().unwrap();
        let arguments = ["-C", "src", "-C", "", "-C", "..", "-c", "-C", "show"];
        let invocation = Invocation::parse(&arguments).unwrap();
        let guard = Guard {
            cache: None,
            allow: crate::review::path_access::PathAccess::default(),
            working_files: None,
        };
        let reach = invocation.reach(&root, &root, true, guard);
        assert_eq!(reach.base, root);
        assert!(reach.known);
        assert!(!invocation.ordinary);
        let absolute = root.join("elsewhere");
        let arguments = ["-C", "src", "-C", absolute.to_str().unwrap(), "status"];
        let invocation = Invocation::parse(&arguments).unwrap();
        assert_eq!(invocation.reach(&root, &root, true, guard).base, absolute);
    }

    #[test]
    fn git_writes_of_policy_files_from_other_content_are_blocked_or_asked() {
        let root = assert_fs::TempDir::new().unwrap();
        let root = root.path().canonicalize().unwrap();
        std::fs::create_dir_all(root.join(".shield")).unwrap();
        std::fs::write(root.join(".shield/config.yml"), "schema_version: 1\n").unwrap();
        std::fs::create_dir(root.join("src")).unwrap();
        let guard = Guard {
            cache: None,
            allow: crate::review::path_access::PathAccess::default(),
            working_files: None,
        };
        let level =
            |arguments: &[&str], cwd: &Path| policy_files(arguments, &root, cwd, true, guard).0;
        for arguments in [
            &["checkout", "HEAD~3", "--", ".shield/config.yml"][..],
            &["checkout", "acme-branch", "--", "."],
            &["checkout", "HEAD~1", ".shield"],
            &["checkout", "--", ":/"],
            &["checkout", "."],
            &["checkout", "HEAD", "--", "*.yml"],
            &["restore", "--source", "HEAD~2", ".shield/rules.yml"],
            &["restore", "--source=HEAD~2", "--worktree", "--staged", "."],
            &["restore", "-s", "HEAD", "--", ".shield"],
            &["checkout-index", "-a", "-f"],
            &["checkout-index", "-f", "--", ".shield/config.yml"],
            &["-C", "src", "checkout", "HEAD~1", "--", ".."],
            // Without `--` a first word that is a path is written too.
            &["checkout", ".shield/config.yml", "src"],
            // Moves and removals put other content in place or drop it.
            &["mv", "acme.yml", ".shield/rules.yml"],
            &["mv", "-f", "acme.yml", ".shield/rules.yml"],
            &["rm", ".shield/rules.yml"],
            // Agents' protected settings files are covered too.
            &["checkout", "HEAD~5", "--", ".claude/settings.json"],
            &["restore", "--source", "HEAD~5", ".mcp.json"],
        ] {
            assert!(
                level(arguments, &root) == Protection::Blocked,
                "{arguments:?}"
            );
        }
        for arguments in [
            &["apply", "acme.patch"][..],
            &["apply", "--index", "--cached", "acme.patch"],
            &["am", "acme.mbox"],
            &["stash", "pop"],
            &["stash", "apply", "stash"],
            &["read-tree", "-u", "-m", "HEAD~1"],
            &["checkout", "HEAD", "--pathspec-from-file=paths.txt"],
            &["restore", "--pathspec-from-file", "paths.txt"],
        ] {
            assert!(
                level(arguments, &root) == Protection::Undetermined,
                "{arguments:?}"
            );
        }
        // Branch switches, resets, merges and stashes are decided by a Git
        // query; writes/tests.rs covers them.
        for arguments in [
            &["checkout", "HEAD~3", "--", "src"][..],
            &["restore", "--staged", ".shield/config.yml"],
            &["restore", "src"],
            &["apply", "--check", "acme.patch"],
            &["apply", "--cached", "acme.patch"],
            &["am", "--abort"],
            &["stash", "show", "-p"],
            &["rm", "--cached", ".shield/rules.yml"],
            &["mv", "src", "lib"],
        ] {
            assert!(level(arguments, &root) == Protection::None, "{arguments:?}");
        }
        // Agent settings the policy files open to change are ordinary files;
        // the policy files never are.
        let open = Guard {
            allow: crate::review::path_access::PathAccess {
                agent_settings_change: crate::review::path_access::Change::Allow,
                ..crate::review::path_access::PathAccess::default()
            },
            ..guard
        };
        let opened = |arguments: &[&str]| policy_files(arguments, &root, &root, true, open).0;
        assert!(opened(&["checkout", "HEAD~5", "--", ".claude/settings.json"]) == Protection::None);
        assert!(opened(&["checkout", "HEAD~5", "--", ".shield"]) == Protection::Blocked);
        // From a subfolder, `.` holds no policy file.
        assert!(level(&["checkout", "HEAD~1", "--", "."], &root.join("src")) == Protection::None);
        // An unknown folder leaves every pathspec possibly covering them.
        assert!(
            policy_files(
                &["checkout", "HEAD~1", "--", "src"],
                &root,
                &root,
                false,
                guard
            )
            .0 == Protection::Blocked
        );
    }

    #[test]
    fn git_write_options_are_read_as_git_reads_them() {
        let root = assert_fs::TempDir::new().unwrap();
        let root = root.path().canonicalize().unwrap();
        std::fs::create_dir_all(root.join(".shield")).unwrap();
        std::fs::write(root.join(".shield/config.yml"), "schema_version: 1\n").unwrap();
        let guard = Guard {
            cache: None,
            allow: crate::review::path_access::PathAccess::default(),
            working_files: None,
        };
        let level = |arguments: &[&str]| policy_files(arguments, &root, &root, true, guard).0;
        // Short options cluster, and long ones abbreviate.
        for (arguments, expected) in [
            (
                &["read-tree", "-mu", "HEAD~1"][..],
                Protection::Undetermined,
            ),
            (
                &["read-tree", "-um", "--reset", "HEAD~1"],
                Protection::Undetermined,
            ),
            (&["checkout-index", "--std"], Protection::Undetermined),
            (
                &["restore", "--pathspec-from=paths.txt"],
                Protection::Undetermined,
            ),
            (
                &["rm", "--pathspec-from-file=paths.txt"],
                Protection::Undetermined,
            ),
            (&["checkout-index", "-fa"], Protection::Blocked),
            (&["checkout-index", "--al"], Protection::Blocked),
            (
                &["restore", "-SW", ".shield/rules.yml"],
                Protection::Blocked,
            ),
            // `S` here is the source revision, not `--staged`.
            (
                &["restore", "-sS", ".shield/rules.yml"],
                Protection::Blocked,
            ),
            (
                &["restore", "--source", "-S", ".shield/rules.yml"],
                Protection::Blocked,
            ),
            // Options that make a command harmless match only exactly.
            (
                &["restore", "--stag", ".shield/rules.yml"],
                Protection::Blocked,
            ),
            (&["read-tree", "-m", "HEAD~1"], Protection::None),
            (&["restore", "-qS", ".shield/config.yml"], Protection::None),
            (&["checkout", "-qb", "acme-feature"], Protection::None),
            // `-b` takes `acme-feature`, so `HEAD~1` is the start point the
            // tree moves to, which Git cannot compare outside a repository.
            (
                &["checkout", "-fb", "acme-feature", "HEAD~1"],
                Protection::Undetermined,
            ),
        ] {
            assert!(level(arguments) == expected, "{arguments:?}");
        }
    }

    #[test]
    fn revision_paths_from_subfolders_keep_relocated_settings_protected() {
        let root = assert_fs::TempDir::new().unwrap();
        let root = root.path().canonicalize().unwrap();
        std::fs::create_dir(root.join("src")).unwrap();
        std::fs::create_dir(root.join("agent-home")).unwrap();
        let working_files = crate::location::WorkingFiles {
            protected: vec![root.join("agent-home")],
            ..Default::default()
        };
        let guard = Guard {
            cache: None,
            allow: crate::review::path_access::PathAccess::default(),
            working_files: Some(&working_files),
        };
        for arguments in [
            vec!["-C", "src", "show", "HEAD:agent-home/settings.json"],
            vec![
                "-C",
                "src",
                "cat-file",
                "-p",
                "HEAD:agent-home/settings.json",
            ],
            vec!["-C", "src", "show", ":agent-home/settings.json"],
            vec!["-C", "src", "show", "HEAD:../agent-home/settings.json"],
        ] {
            assert!(
                content(&arguments, &root, &root, true, guard).0 == Protection::Sensitive,
                "{arguments:?}"
            );
        }
        let safe = content(
            &["-C", "src", "show", "HEAD:./agent-home/settings.json"],
            &root,
            &root,
            true,
            guard,
        );
        assert!(safe.0 == Protection::None);
        let unknown = content(&["show", "HEAD:notes.txt"], &root, &root, false, guard);
        assert!(unknown.0 == Protection::Sensitive);
    }

    #[test]
    fn history_graphs_print_no_settings_unless_they_print_patches() {
        let root = assert_fs::TempDir::new().unwrap();
        let root = root.path().canonicalize().unwrap();
        std::fs::write(root.join(".mcp.json"), "{}").unwrap();
        std::fs::create_dir(root.join("src")).unwrap();
        let guard = Guard {
            cache: None,
            allow: crate::review::path_access::PathAccess::default(),
            working_files: None,
        };
        let level = |arguments: &[&str]| content(arguments, &root, &root, true, guard).0;
        for arguments in [
            &["log", "--oneline", "--graph", "-8", "HEAD", "origin/main"][..],
            &["log", "--graph", "--decorate", "--all", "-20"],
            &[
                "log",
                "--first-parent",
                "--no-merges",
                "--reverse",
                "--stat",
            ],
        ] {
            assert!(level(arguments) == Protection::None, "{arguments:?}");
        }
        for arguments in [
            &["log", "--graph", "-8", "-p"][..],
            &["log", "-8", "--graph", "--patch", "HEAD"],
            &["log", "--graph", "--future-option"],
            // `-o` names an output folder here, never a pathspec.
            &["format-patch", "-o", "src", "HEAD~1"],
            &["log", "-g", "-r", "-p"],
        ] {
            assert!(level(arguments) == Protection::Sensitive, "{arguments:?}");
        }
    }

    #[test]
    fn common_listings_and_summaries_print_no_settings() {
        let root = assert_fs::TempDir::new().unwrap();
        let root = root.path().canonicalize().unwrap();
        std::fs::write(root.join(".mcp.json"), "{}").unwrap();
        std::fs::create_dir(root.join("src")).unwrap();
        let guard = Guard {
            cache: None,
            allow: crate::review::path_access::PathAccess::default(),
            working_files: None,
        };
        let failing: Vec<&str> = COMMON_LISTINGS
            .iter()
            .copied()
            .filter(|command| {
                let arguments: Vec<&str> = command.split(' ').collect();
                content(&arguments, &root, &root, true, guard).0 != Protection::None
            })
            .collect();
        assert!(failing.is_empty(), "{failing:#?}");
    }

    /// Everyday listings and summaries, each word separated by one space.
    const COMMON_LISTINGS: &[&str] = &[
        "log --oneline -n 20",
        "log --pretty=format:%h -10",
        "log --format=%H -1 --no-color",
        "log --abbrev-commit --oneline main..HEAD",
        "log --stat -5",
        "log --name-status HEAD~3..HEAD",
        "log --author=Acme --since=2.weeks --date=short",
        "log --grep=fix -i --invert-grep --oneline",
        "log --regexp-ignore-case --extended-regexp --grep x",
        "log -g --oneline",
        "log --walk-reflogs --oneline",
        "log --oneline --no-walk --do-walk HEAD",
        "log --left-right --cherry --oneline main...HEAD",
        "log --ancestry-path --full-history --simplify-by-decoration",
        "log --show-signature --no-notes --use-mailmap -1",
        "log --oneline --follow -- src",
        "log -S needle --oneline --pickaxe-regex",
        "log --count --oneline",
        "show --stat HEAD",
        "show -s --format=%B HEAD",
        "show --abbrev-commit --no-patch --oneline HEAD",
        "show --name-only --pretty= HEAD",
        "diff --stat --merge-base main",
        "diff --name-only -w HEAD~1",
        "diff --cached --numstat --find-renames",
        "diff --dirstat=files --shortstat",
        "diff -R --stat --submodule=log --ignore-submodules",
        "diff-tree --no-commit-id --name-only -r HEAD",
        "diff-tree -r --name-status --root HEAD",
        "status -sb",
        "status --short --branch --untracked-files=all",
        "status -s -uno --ignore-submodules=dirty --renames",
        "status --porcelain=v1 -z --find-renames=50",
        "ls-files -o --exclude-standard",
        "ls-files --others --directory --no-empty-directory",
        "ls-files -m -d -c -t -z",
        "ls-files --recurse-submodules --sparse",
        "reflog -n 10 --date=iso",
        "reflog show --oneline -20",
        "stash list --oneline --date=relative",
        "-C src grep --line-number needle",
        "-C src grep -n -i -w -I needle",
        "-C src grep --count --files-with-matches needle",
        "-C src grep -l -E -F -P needle",
        "-C src grep --ignore-case --word-regexp --untracked x",
        "-C src grep -o -h -H --only-matching needle",
        "-C src grep --fixed-strings --perl-regexp --recurse-submodules x",
    ];

    #[test]
    fn revision_paths_name_blobs_but_not_message_searches() {
        for (word, path) in [
            ("HEAD:.mcp.json", Some(".mcp.json")),
            (":.mcp.json", Some(".mcp.json")),
            (":0:.mcp.json", Some(".mcp.json")),
            (
                "main@{2026-01-01 10:00:00}:.codex/hooks.json",
                Some(".codex/hooks.json"),
            ),
            ("HEAD:", Some("")),
            (":/fix settings", None),
            ("HEAD~1", None),
        ] {
            assert_eq!(revision_path(word), path, "{word}");
        }
    }

    #[test]
    fn option_values_never_narrow_a_patch_to_a_pathspec() {
        let root = assert_fs::TempDir::new().unwrap();
        std::fs::create_dir(root.path().join("src")).unwrap();
        let specs = |words: &[&'static str]| pathspecs(words, root.path(), false);
        assert_eq!(specs(&["-p", "src"]), ["src"]);
        assert_eq!(specs(&["-p", "--author", "src"]), [] as [&str; 0]);
        assert_eq!(specs(&["-p", "-n", "src"]), [] as [&str; 0]);
        assert_eq!(specs(&["--ignore-matching-lines", "src"]), [] as [&str; 0]);
        assert_eq!(specs(&["--diff-merges", "src"]), [] as [&str; 0]);
        assert_eq!(specs(&["--future-option", "src"]), [] as [&str; 0]);
        assert_eq!(specs(&["--future-option", "--", "src"]), [] as [&str; 0]);
        assert_eq!(specs(&["src", "--future-option"]), [] as [&str; 0]);
        assert_eq!(specs(&["--ignore-matching-lines=src", "src"]), ["src"]);
        assert_eq!(specs(&["--ignore-matching-lines", "--", "src"]), ["src"]);
        assert_eq!(specs(&["-wI", "src", "src"]), ["src"]);
        assert_eq!(
            specs(&["-p", "--", "missing", ".mcp.json"]),
            ["missing", ".mcp.json"]
        );
        // The first word of a grep is its pattern, even when it names a folder.
        assert_eq!(pathspecs(&["src"], root.path(), true), [] as [&str; 0]);
        assert_eq!(pathspecs(&["-e", "src", "src"], root.path(), true), ["src"]);
    }

    #[test]
    fn patch_options_include_clusters_but_not_attached_values() {
        assert!(patch(&["-p"]));
        assert!(patch(&["-wp"]));
        assert!(patch(&["-U3"]));
        assert!(patch(&["--word-diff=color"]));
        assert!(!patch(&["--stat"]));
        assert!(!patch(&["-Supdate"]));
        assert!(!patch(&["--", "-p"]));
        assert!(!patch(&["--author", "--patch"]));
        assert!(!summary(&["--ignore-matching-lines", "--stat"]));
    }
}
