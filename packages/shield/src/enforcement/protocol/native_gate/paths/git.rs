//! Which agent settings a literal Git invocation can print: revision paths,
//! patch output and searches, narrowed only by pathspecs Git is sure to use.
//!
//! Git output reaches tracked content, so review state and the cache, which
//! `.shield/.gitignore` keeps out of Git, are not part of these footprints.
//! Object IDs a command resolves itself, such as a tag naming a blob, are
//! outside this literal model.
use super::{Access, Finding, Guard, Protection, absolute, classify, resolve_existing, strictest};
use crate::review::integrations::protected_files;
use std::path::{Path, PathBuf};

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
    let start = if arguments.first() == Some(&"-C")
        && arguments.get(1).is_some_and(|p| Path::new(p).is_absolute())
    {
        2
    } else {
        0
    };
    let Some(&subcommand) = arguments.get(start) else {
        return Access::Write;
    };
    if !RECORDS.contains(&subcommand) || arguments.iter().any(|a| a.starts_with("--output")) {
        return Access::Write;
    }
    let rest = &arguments[start + 1..];
    match subcommand {
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

/// The strictest protection among the agent settings this invocation can print.
pub(super) fn content(
    arguments: &[&str],
    root: &Path,
    cwd: &Path,
    cwd_known: bool,
    guard: Guard<'_>,
) -> Finding {
    if matches!(arguments, ["--version" | "-v" | "--help" | "-h"]) {
        return (Protection::None, None);
    }
    let Some(invocation) = Invocation::parse(arguments, root, cwd, cwd_known, guard) else {
        // An unmodeled global option hides where the subcommand starts.
        return Reach::everything(root, guard);
    };
    let reach = &invocation.reach;
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
    subcommand: &'a str,
    rest: &'a [&'a str],
    reach: Reach<'a>,
    execution_override: bool,
}

impl<'a> Invocation<'a> {
    /// The subcommand after modeled global options, and the folder Git runs in.
    fn parse(
        arguments: &'a [&'a str],
        root: &'a Path,
        cwd: &Path,
        cwd_known: bool,
        guard: Guard<'a>,
    ) -> Option<Self> {
        let mut base = cwd.to_path_buf();
        let mut elsewhere = false;
        let mut execution_override = false;
        let mut index = 0;
        loop {
            let word = *arguments.get(index)?;
            let name = option_name(word);
            if word == "-C" {
                base = absolute(arguments.get(index + 1)?, &base);
                index += 2;
            } else if word == "-c" || word == "--config-env" {
                execution_override |= overrides_execution(arguments.get(index + 1)?);
                index += 2;
            } else if let Some(config) = word.strip_prefix("-c").filter(|_| !word.starts_with("--"))
            {
                execution_override |= overrides_execution(config);
                index += 1;
            } else if let Some(config) = word.strip_prefix("--config-env=") {
                execution_override |= overrides_execution(config);
                index += 1;
            } else if VALUED_GLOBALS.contains(&word) {
                elsewhere |= ["--git-dir", "--work-tree"].contains(&word);
                index += 2;
            } else if (name != word && VALUED_GLOBALS.contains(&name))
                || FLAG_GLOBALS.contains(&word)
                || name == "--exec-path"
            {
                elsewhere |= ["--git-dir", "--work-tree"].contains(&name);
                index += 1;
            } else if word.starts_with('-') {
                return None;
            } else {
                return Some(Self {
                    subcommand: word,
                    rest: &arguments[index + 1..],
                    execution_override,
                    reach: Reach {
                        root,
                        base,
                        guard,
                        known: cwd_known && !elsewhere,
                    },
                });
            }
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
}

impl Reach<'_> {
    fn settings(root: &Path) -> impl Iterator<Item = (&'static str, PathBuf)> + '_ {
        protected_files()
            .iter()
            .filter(|name| !name.starts_with(".shield"))
            .map(move |name| (*name, resolve_existing(&root.join(name))))
    }

    fn everything(root: &Path, guard: Guard<'_>) -> Finding {
        strictest(
            Self::settings(root)
                .map(|(name, path)| (classify(&path, guard, Access::Index), Some(name.to_owned()))),
        )
    }

    /// Settings at or below `folder`.
    fn below(&self, folder: &Path) -> Finding {
        if !self.known {
            return Self::everything(self.root, self.guard);
        }
        let folder = resolve_existing(folder);
        strictest(
            Self::settings(self.root)
                .filter(|(_, path)| path.starts_with(&folder))
                .map(|(name, path)| {
                    (
                        classify(&path, self.guard, Access::Index),
                        Some(name.to_owned()),
                    )
                }),
        )
    }

    /// The repository Git finds from `base`. It holds the project's settings
    /// unless it lies beside the project.
    fn repository(&self) -> Finding {
        let base = resolve_existing(&self.base);
        let root = resolve_existing(self.root);
        if !self.known || base.starts_with(&root) || root.starts_with(&base) {
            Self::everything(self.root, self.guard)
        } else {
            (Protection::None, None)
        }
    }

    /// A path the command names, by its own protection.
    fn named(&self, path: &str, submitted: &str) -> Finding {
        let path = absolute(path, &self.base);
        let level = classify(&path, self.guard, Access::Index).max(classify(
            &resolve_existing(&path),
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
        if !resolve_existing(&self.base).starts_with(resolve_existing(self.root)) {
            return self.repository();
        }
        Self {
            base: self.root.to_path_buf(),
            ..*self
        }
        .named(path, submitted)
    }

    fn scoped(&self, pathspecs: &[&str], scope: Scope) -> Finding {
        if pathspecs.is_empty() {
            return match scope {
                Scope::Repository => self.repository(),
                Scope::Folder => self.below(&self.base),
            };
        }
        strictest(pathspecs.iter().map(|spec| {
            // Magic and wildcard pathspecs can match anywhere in the repository.
            if spec.starts_with(':') || spec.contains(['*', '?', '[']) {
                self.repository()
            } else {
                strictest(
                    [
                        self.named(spec, spec),
                        self.below(&absolute(spec, &self.base)),
                    ]
                    .into_iter(),
                )
            }
        }))
    }
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
    for (index, letter) in option.char_indices().skip(1) {
        let end = index + letter.len_utf8();
        if VALUED
            .iter()
            .any(|value| value.len() == 2 && value.ends_with(letter))
        {
            return Some(end == option.len());
        }
        if !"puWcswbzaqvRiEFHPt".contains(letter) {
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
