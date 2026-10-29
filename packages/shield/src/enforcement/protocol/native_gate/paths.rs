//! Submitted path protection, with cwd evidence and explicit destructive operand roles.
use super::{Path, Value};
use std::path::{Component, PathBuf};

use crate::review::{
    integrations::protected_files,
    path_access::{Change, PathAccess, Read},
};

/// The compiled cache location and the operator's protected path access.
#[derive(Clone, Copy)]
pub(super) struct Guard<'a> {
    pub(super) cache: Option<&'a Path>,
    pub(super) allow: PathAccess,
    pub(super) working_files: Option<&'a crate::location::WorkingFiles>,
}

/// How far a request may touch protected state.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Protection {
    None,
    /// Reads agent settings that may hold credentials; the user decides.
    Sensitive,
    /// The modeled search's bounded metadata footprint could not be verified.
    Unverified,
    /// A content search can reach protected state; narrow its footprint.
    SearchBlocked,
    Blocked,
}

/// What an operation does to the paths it names. Unmodeled commands write.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Access {
    /// Literal output arguments are data, not filesystem operands.
    Output,
    Read,
    /// Lists or records paths; may name the .shield folder itself.
    Index,
    Write,
}

/// Read-only commands whose only effect on named paths is reading them.
const READERS: &str =
    "cat head tail wc grep egrep fgrep rg stat file diff cmp jq shasum sha256sum md5";
/// Git subcommands that read the working tree and change only the repository.
const GIT_RECORDS: &str =
    "add status diff log show blame commit ls-files check-ignore grep shortlog";
const FIND_ACTIONS: &str = "-exec -execdir -ok -okdir -delete -fprint -fprint0 -fprintf -fls";

/// The strictest protection found, with the submitted path that caused it when
/// one path did.
pub(super) type Finding = (Protection, Option<String>);

/// The first of the strictest findings, so a message names the earliest path
/// in the command that caused the block.
fn strictest(findings: impl Iterator<Item = Finding>) -> Finding {
    findings.fold((Protection::None, None), |best, finding| {
        if finding.0 > best.0 { finding } else { best }
    })
}

/// Which protected group a submitted path belongs to, for the agent's message:
/// review state or a cache, the `.shield` folder or its ignore file, a Shield
/// policy file, or agent settings.
pub(super) fn group(submitted: &str, cwd: &Path, guard: Guard<'_>) -> &'static str {
    let path = resolve_existing(&submitted_path(submitted, cwd));
    if guard
        .working_files
        .is_some_and(|scope| scope.write.contains(&path))
    {
        return "working-root";
    }
    if guard.cache.is_some_and(|cache| path.starts_with(cache)) {
        return "state";
    }
    let parts: Vec<_> = path.components().map(Component::as_os_str).collect();
    let Some(index) = parts.iter().position(|part| *part == ".shield") else {
        return "settings";
    };
    match parts.get(index + 1) {
        Some(part) if *part == "review" || *part == "cache" => "state",
        None => "folder",
        Some(part) if *part == ".gitignore" => "ignore",
        Some(_) => "policy",
    }
}

// This classifies inspected literals; it does not attest unobserved executable effects.
pub(super) fn protection(
    value: &Value,
    root: &Path,
    cwd: &Path,
    guard: Guard<'_>,
    access: Access,
) -> Finding {
    inspect(value, root, cwd, true, guard, access)
}

fn access(request: &Value) -> Access {
    let Some(executable) = request["executable"].as_str() else {
        return Access::Write;
    };
    let leaf = Path::new(executable)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(executable);
    let arguments: Vec<&str> = request["arguments"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    let listed = |list: &str, word: &str| list.split_whitespace().any(|item| item == word);
    match leaf {
        "node" if crate::location::node_check_file(executable, &arguments).is_some() => {
            Access::Read
        }
        "echo" | "printf" => Access::Output,
        "rg" if arguments.contains(&"--files") => Access::Index,
        "sed" | "awk" | "sort" | "uniq" | "cut" | "nl" | "comm" | "paste" | "column"
            if crate::location::text_read_only(executable, &arguments) =>
        {
            Access::Read
        }
        _ if listed(READERS, leaf) => Access::Read,
        "ls" => Access::Index,
        "find" if !arguments.iter().any(|a| listed(FIND_ACTIONS, a)) => Access::Index,
        "git"
            if arguments
                .get(
                    if arguments.first() == Some(&"-C")
                        && arguments.get(1).is_some_and(|p| Path::new(p).is_absolute())
                    {
                        2
                    } else {
                        0
                    },
                )
                .is_some_and(|sub| listed(GIT_RECORDS, sub))
                && !arguments.iter().any(|a| a.starts_with("--output")) =>
        {
            Access::Index
        }
        _ => Access::Write,
    }
}

/// An outside-location refusal: the first argument of the refused operation
/// that resolves outside the project, whether it is read or written, and the
/// folder to offer, a directory as itself and a file as its folder.
pub(super) struct Outside {
    pub(super) path: PathBuf,
    pub(super) access: &'static str,
    /// None for `/`, the home folder or a folder holding the project: granting
    /// one of those would open far more than the request needs.
    pub(super) folder: Option<PathBuf>,
}

pub(super) fn outside(operation: &Value, root: &Path, cwd: &Path) -> Option<Outside> {
    let request = &operation["request"];
    let access = if access(request) == Access::Write {
        "write"
    } else {
        "read"
    };
    let root = resolve_existing(root);
    let cwd = operation["cwd"].as_str().map_or(cwd, Path::new);
    request["arguments"]
        .as_array()?
        .iter()
        .filter_map(Value::as_str)
        .filter_map(|s| {
            let value = s.strip_prefix('-').map_or(Some(s), |option| {
                option.split_once('=').map(|(_, value)| value)
            })?;
            let path = resolve_existing(&absolute(value, cwd));
            (!value.is_empty() && !path.starts_with(&root)).then_some(path)
        })
        .find(|path| path.is_absolute())
        .map(|path| {
            let folder = if path.is_dir() {
                path.clone()
            } else {
                path.parent().map_or(path.clone(), Path::to_path_buf)
            };
            let home = std::env::var_os("HOME").map(|home| resolve_existing(Path::new(&home)));
            let broad = folder.parent().is_none()
                || root.starts_with(&folder)
                || home.is_some_and(|home| home == folder);
            Outside {
                folder: (!broad).then_some(folder),
                path,
                access,
            }
        })
}

/// Whether an inspected operation runs the operator's `shield` CLI, by name or
/// through a path that resolves to it, or launches an `mcp serve` argument
/// sequence under another executable name, which may be a copy of Shield.
pub(super) fn operator_cli(value: &Value, cwd: &Path) -> bool {
    match value {
        Value::Object(fields) => {
            let cwd = fields
                .get("cwd")
                .and_then(Value::as_str)
                .map_or(cwd, Path::new);
            value["executable"].as_str().is_some_and(|executable| {
                value["arguments"].as_array().is_some_and(|arguments| {
                    arguments
                        .windows(2)
                        .any(|pair| pair[0] == "mcp" && pair[1] == "serve")
                }) || Path::new(executable)
                    .file_name()
                    .is_some_and(|n| n == "shield")
                    || submitted_path(executable, cwd)
                        .canonicalize()
                        .is_ok_and(|p| p.file_name().is_some_and(|n| n == "shield"))
            }) || fields
                .iter()
                .filter(|(key, _)| !["evaluation", "source_units"].contains(&key.as_str()))
                .any(|(_, v)| operator_cli(v, cwd))
        }
        Value::Array(values) => values.iter().any(|v| operator_cli(v, cwd)),
        _ => false,
    }
}

fn inspect(
    value: &Value,
    root: &Path,
    cwd: &Path,
    cwd_known: bool,
    guard: Guard<'_>,
    access: Access,
) -> Finding {
    match value {
        Value::Object(fields) => {
            let cwd = fields
                .get("cwd")
                .and_then(Value::as_str)
                .map_or(cwd, Path::new);
            let cwd_known = value["context"]["cwd_known"].as_bool().unwrap_or(cwd_known);
            if unresolved_redirect(value) || destructive(value, root, cwd, cwd_known, guard) {
                return (Protection::Blocked, None);
            }
            // Operations carry their request; the parsed structure carries the
            // same command's literal words, so compound commands keep per-command
            // access instead of defaulting to a write.
            let input = |operator: &Value| {
                if operator == "<" || operator == "<<" || operator == "<<-" || operator == "<<<" {
                    Access::Read
                } else {
                    Access::Write
                }
            };
            let access = if value["request"].is_object() {
                self::access(&value["request"])
            } else if value["coverage"] == "redirection" {
                input(&value["redirection"]["operator"])
            } else if let Some(words) = value["words"].as_array() {
                let literals: Option<Vec<&str>> =
                    words.iter().map(|word| word["literal"].as_str()).collect();
                literals
                    .and_then(|words| {
                        let (executable, arguments) = words.split_first()?;
                        Some(self::access(
                            &serde_json::json!({"executable":executable,"arguments":arguments}),
                        ))
                    })
                    .unwrap_or(Access::Write)
            } else if value["operator"].is_string() && value.get("target").is_some() {
                input(&value["operator"])
            } else {
                access
            };
            let piped = value["stdin"] == "pipe";
            let mut search =
                search_protection(&value["request"], root, cwd, cwd_known, guard, piped);
            if search.0 == Protection::Blocked {
                search.0 = Protection::SearchBlocked;
            }
            let named = named_files(&value["request"], cwd, guard);
            strictest(
                [named, search].into_iter().chain(
                    fields
                        .iter()
                        .filter(|(key, _)| {
                            ![
                                "evaluation",
                                "source",
                                "source_units",
                                "project",
                                "cwd",
                                "context",
                                "stdin",
                            ]
                            .contains(&key.as_str())
                        })
                        .map(|(key, v)| {
                            if access == Access::Output
                                && key == "words"
                                && let Some(words) = v.as_array()
                            {
                                return strictest(words.iter().enumerate().map(|(index, word)| {
                                    inspect(
                                        word,
                                        root,
                                        cwd,
                                        cwd_known,
                                        guard,
                                        if index == 0 { Access::Write } else { access },
                                    )
                                }));
                            }
                            let access = if access == Access::Output && key == "executable" {
                                Access::Write
                            } else {
                                access
                            };
                            inspect(v, root, cwd, cwd_known, guard, access)
                        }),
                ),
            )
        }
        Value::Array(values) => strictest(
            values
                .iter()
                .map(|v| inspect(v, root, cwd, cwd_known, guard, access)),
        ),
        Value::String(s) => inspect_path(s, cwd, guard, access),
        _ => (Protection::None, None),
    }
}

/// Files a modeled filter or download names inside a program or option,
/// with their own access; plain arguments are inspected separately.
fn named_files(request: &Value, cwd: &Path, guard: Guard<'_>) -> Finding {
    let Some(executable) = request["executable"].as_str() else {
        return (Protection::None, None);
    };
    let arguments: Vec<&str> = request["arguments"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    let mut files = crate::location::text_files(executable, &arguments).unwrap_or_default();
    if let Some(path) =
        crate::http_client::fetch(executable, &arguments).and_then(|fetch| fetch.output)
    {
        files.push((true, path.to_owned()));
    }
    strictest(files.iter().map(|(write, path)| {
        inspect_path(
            path,
            cwd,
            guard,
            if *write { Access::Write } else { Access::Read },
        )
    }))
}

fn inspect_path(s: &str, cwd: &Path, guard: Guard<'_>, access: Access) -> Finding {
    if access == Access::Output {
        return (Protection::None, None);
    }
    // `--option=path` names a path too.
    let value = s
        .strip_prefix('-')
        .and_then(|option| option.split_once('='))
        .map(|(_, value)| value);
    strictest(
        [Some(s), value]
            .into_iter()
            .flatten()
            .filter(|s| !s.is_empty())
            .map(|s| {
                let level = classify(&absolute(s, cwd), guard, access).max(classify(
                    &resolve_existing(&submitted_path(s, cwd)),
                    guard,
                    access,
                ));
                (level, Some(s.to_owned()))
            }),
    )
}

/// A host tool's recursive read, checked as the ripgrep walk it performs: a
/// content search, or a names-only listing, of `path` with its globs and types.
/// A glob that is absolute or has a `..` segment is not bounded by `path`.
pub(super) fn recursive_read(
    search: &Value,
    path: &str,
    root: &Path,
    cwd: &Path,
    guard: Guard<'_>,
) -> Finding {
    let strings = |key: &str| -> Vec<String> {
        search[key]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect()
    };
    let globs = strings("globs");
    let unbounded = |glob: &String| {
        glob.split(['{', ','])
            .any(|part| part.trim_start_matches('!').starts_with(['/', '~']))
            || glob.split(['/', '{', '}', ',']).any(|part| part == "..")
    };
    if globs.iter().any(unbounded) {
        return (Protection::Unverified, None);
    }
    // Nothing exists below a missing path; the host tool reports the error.
    if Path::new(path).try_exists().is_ok_and(|exists| !exists) {
        return (Protection::None, None);
    }
    let names_only = search["names_only"] == true;
    let mut arguments: Vec<String> = Vec::new();
    if names_only {
        arguments.push("--files".into());
    }
    for glob in globs {
        arguments.extend(["-g".into(), glob]);
    }
    for kind in strings("types") {
        arguments.extend(["-t".into(), kind]);
    }
    arguments.push("--".into());
    if !names_only {
        // The pattern does not change which files the search reads.
        arguments.push("shield-pattern".into());
    }
    arguments.push(path.into());
    let request = serde_json::json!({"executable":"rg","arguments":arguments});
    let mut finding = search_protection(&request, root, cwd, true, guard, false);
    if finding.0 == Protection::Blocked {
        finding.0 = Protection::SearchBlocked;
    }
    finding
}

fn search_protection(
    request: &Value,
    root: &Path,
    cwd: &Path,
    cwd_known: bool,
    guard: Guard<'_>,
    piped: bool,
) -> Finding {
    let Some(executable) = request["executable"].as_str() else {
        return (Protection::None, None);
    };
    if !["rg", "/bin/rg", "/usr/bin/rg"].contains(&executable) {
        return (Protection::None, None);
    }
    if !cwd_known {
        return (Protection::Unverified, None);
    }
    let Some(arguments) = request["arguments"].as_array().and_then(|a| {
        a.iter()
            .map(|v| v.as_str().map(str::to_owned))
            .collect::<Option<Vec<_>>>()
    }) else {
        return (Protection::Blocked, None);
    };
    let input = crate::policy::normalized::Input::Argv {
        executable: executable.into(),
        arguments,
        original_input: Value::Null,
    };
    let observer = crate::location::Observer::new(
        &input,
        &root.to_string_lossy(),
        &cwd.to_string_lossy(),
        None,
    );
    // Searching only a pattern after a pipe reads the pipe, not files.
    if piped && observer.search_reads_input() {
        return (Protection::None, None);
    }
    let mut finding = strictest(
        [root.join(".shield/review"), root.join(".shield/cache")]
            .into_iter()
            .chain(
                protected_files()
                    .iter()
                    .filter(|p| **p != ".shield")
                    .map(|p| root.join(p)),
            )
            .chain(guard.cache.map(Path::to_path_buf))
            .map(|path| match observer.search_reaches(&path) {
                Some(true) => (
                    classify(&path, guard, Access::Read),
                    Some(path.to_string_lossy().into_owned()),
                ),
                Some(false) => (Protection::None, None),
                None => (Protection::Unverified, None),
            }),
    );
    if finding.0 == Protection::Blocked {
        return finding;
    }
    if observer
        .search_entries(|path| {
            let level = classify(path, guard, Access::Read);
            if level > finding.0 {
                finding = (level, Some(path.to_string_lossy().into_owned()));
            }
            finding.0 != Protection::Blocked
        })
        .is_none()
    {
        return (Protection::Unverified, None);
    }
    finding
}

// Policy files may be read and recorded; review state and the cache never.
// Agent settings may carry credentials, so reading them is the user's call.
// The operator's path access can open agent settings and policy files, never
// review state, the cache, the .shield folder itself or its ignore file.
fn classify(path: &Path, guard: Guard<'_>, access: Access) -> Protection {
    if access == Access::Write
        && guard
            .working_files
            .is_some_and(|scope| scope.write.iter().any(|root| root == path))
    {
        return Protection::Blocked;
    }
    let configuration = configuration_path(path, guard);
    if !configuration && !protected_path(path, guard.cache) {
        return Protection::None;
    }
    if guard.cache.is_some_and(|cache| path.starts_with(cache)) {
        return Protection::Blocked;
    }
    let allowed = |change: Change| change == Change::Allow;
    let parts: Vec<_> = path.components().map(Component::as_os_str).collect();
    let Some(index) = parts.iter().position(|p| *p == ".shield") else {
        if configuration
            && access != Access::Write
            && guard.working_files.is_some_and(|scope| {
                scope
                    .read
                    .iter()
                    .any(|root| path.starts_with(resolve_existing(root)))
            })
        {
            return Protection::None;
        }
        return match access {
            Access::Write if allowed(guard.allow.agent_settings_change) => Protection::None,
            Access::Write => Protection::Blocked,
            _ if guard.allow.agent_settings_read == Read::Allow => Protection::None,
            _ => Protection::Sensitive,
        };
    };
    match parts.get(index + 1) {
        Some(part) if *part == "review" || *part == "cache" => Protection::Blocked,
        // Naming the folder reaches its state unless Git and listings skip it.
        None if access == Access::Index && path.join(".gitignore").is_file() => Protection::None,
        None => Protection::Blocked,
        // The ignore file keeps review state and the cache out of Git.
        Some(part) if access == Access::Write && *part == ".gitignore" => Protection::Blocked,
        Some(_) if access == Access::Write && !allowed(guard.allow.shield_config_change) => {
            Protection::Blocked
        }
        Some(_) => Protection::None,
    }
}

fn configuration_path(path: &Path, guard: Guard<'_>) -> bool {
    guard.working_files.is_some_and(|scope| {
        scope
            .protected
            .iter()
            .any(|root| path.starts_with(resolve_existing(root)))
    })
}

fn unresolved_redirect(value: &Value) -> bool {
    let redirect = &value["redirection"];
    let operator = redirect["operator"].as_str().unwrap_or("");
    let target = redirect["target"]["literal"].as_str();
    let descriptor = [">&", "<&"].contains(&operator)
        && target.is_some_and(|s| s == "-" || s.trim_end_matches('-').parse::<u32>().is_ok());
    value["coverage"] == "redirection"
        && value["context"]["cwd_known"] == false
        && !["<<", "<<-", "<<<"].contains(&operator)
        && !descriptor
        && target.is_none_or(|s| !Path::new(s).is_absolute())
}

fn submitted_path(s: &str, cwd: &Path) -> PathBuf {
    if Path::new(s).is_absolute() {
        PathBuf::from(s)
    } else {
        cwd.join(s)
    }
}

// Resolve existing ancestors too: a new file below a symlink still targets the
// symlink's directory. Resolve before lexical cleanup to preserve link/.. semantics.
fn resolve_existing(path: &Path) -> PathBuf {
    let mut ancestor = path;
    let mut suffix = Vec::new();
    loop {
        if let Ok(mut resolved) = ancestor.canonicalize() {
            for part in suffix.iter().rev() {
                resolved.push(part);
            }
            return resolved;
        }
        let Some(name) = ancestor.file_name() else {
            return path.to_path_buf();
        };
        suffix.push(name);
        let Some(parent) = ancestor.parent() else {
            return path.to_path_buf();
        };
        ancestor = parent;
    }
}

fn absolute(s: &str, cwd: &Path) -> PathBuf {
    let path = submitted_path(s, cwd);
    let mut normalized = PathBuf::new();
    for part in path.components() {
        match part {
            Component::ParentDir => {
                normalized.pop();
            }
            Component::CurDir => {}
            _ => normalized.push(part.as_os_str()),
        }
    }
    normalized
}

fn protected_path(path: &Path, cache: Option<&Path>) -> bool {
    let parts: Vec<_> = path.components().map(Component::as_os_str).collect();
    cache.is_some_and(|cache| path.starts_with(cache))
        || protected_files().iter().any(|owned| {
            let owned: Vec<_> = Path::new(owned)
                .components()
                .map(Component::as_os_str)
                .collect();
            parts.windows(owned.len()).any(|window| window == owned)
        })
}

pub(super) fn removal(path: &Path, root: &Path, guard: Guard<'_>) -> bool {
    let path = resolve_existing(path);
    let locked = |path: &Path| classify(path, guard, Access::Write) == Protection::Blocked;
    locked(&path)
        || guard
            .working_files
            .is_some_and(|scope| scope.write.iter().any(|root| root.starts_with(&path)))
        || guard.working_files.is_some_and(|scope| {
            scope.protected.iter().any(|root| {
                let root = resolve_existing(root);
                root.starts_with(&path) && locked(&root)
            })
        })
        || guard.cache.is_some_and(|cache| cache.starts_with(&path))
        || root.starts_with(&path)
        || protected_files().iter().any(|owned| {
            let owned = root.join(owned);
            owned.starts_with(&path) && locked(&owned)
        })
}

// Only these fixed path-changing forms are inspected here. Other executable
// effects remain the native submitted-command policy boundary.
fn destructive(value: &Value, root: &Path, cwd: &Path, cwd_known: bool, guard: Guard<'_>) -> bool {
    let Some(executable) = value["executable"].as_str() else {
        return false;
    };
    let leaf = Path::new(executable)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(executable);
    if !["rm", "rmdir", "mv", "cp"].contains(&leaf) {
        return false;
    }
    let Some(arguments) = value["arguments"].as_array() else {
        return false;
    };
    let mut args = arguments.iter().filter_map(Value::as_str);
    let mut operands = Vec::new();
    let mut target = None;
    let mut options = true;
    let mut directory_target = true;
    while let Some(arg) = args.next() {
        if options && arg == "--" {
            options = false;
        } else if options && ["-t", "--target-directory"].contains(&arg) {
            target = args.next();
        } else if options && arg.starts_with("--target-directory=") {
            target = arg.strip_prefix("--target-directory=");
        } else if options && arg.starts_with("-t") && arg.len() > 2 {
            target = arg.strip_prefix("-t");
        } else if options && ["-T", "--no-target-directory"].contains(&arg) {
            directory_target = false;
        } else if !options || !arg.starts_with('-') || arg == "-" {
            operands.push(arg);
        }
    }
    if !cwd_known
        && operands
            .iter()
            .copied()
            .chain(target)
            .any(|s| !Path::new(s).is_absolute())
    {
        return true;
    }
    if ["rm", "rmdir"].contains(&leaf) {
        return operands
            .iter()
            .any(|s| removal(&absolute(s, cwd), root, guard));
    }
    let target = target.or_else(|| operands.pop());
    let Some(target) = target else {
        return false;
    };
    let target = absolute(target, cwd);
    operands.iter().any(|source| {
        let source = absolute(source, cwd);
        let destination = if directory_target && target.is_dir() {
            source
                .file_name()
                .map_or_else(|| target.clone(), |name| target.join(name))
        } else {
            target.clone()
        };
        removal(&source, root, guard) || removal(&destination, root, guard)
    })
}
