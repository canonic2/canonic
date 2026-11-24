//! Submitted path protection, with cwd evidence and explicit destructive operand roles.
use super::{Path, Value};
use crate::location::native_name;
use std::path::{Component, PathBuf};

use crate::review::{
    integrations::protected_files,
    path_access::{Change, PathAccess, Read},
};
mod git;

/// Non-authorizing preparation uses the same Git protected-file footprint as
/// native handoff. A clear answer is provisional; every use checks again.
pub(crate) fn git_scope_check(
    root: &Path,
    cwd: &Path,
    arguments: &[String],
    allow: PathAccess,
) -> &'static str {
    let request = serde_json::json!({"executable":"git","arguments":arguments});
    let guard = Guard {
        cache: None,
        allow,
        working_files: None,
    };
    match output_protection(&request, root, cwd, true, guard).0 {
        Protection::None => "clear_at_preparation",
        Protection::Undetermined => "one_time_approval_required",
        _ => "protected_access_refused",
    }
}

/// The compiled cache location and the operator's protected path access.
#[derive(Clone, Copy)]
pub(super) struct Guard<'a> {
    pub(super) cache: Option<&'a Path>,
    pub(super) allow: PathAccess,
    pub(super) working_files: Option<&'a crate::location::WorkingFiles>,
}

/// How far a request may touch protected state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Protection {
    None,
    /// Reads agent settings that may hold credentials; the user decides.
    Sensitive,
    /// May write Shield's policy files: from content Shield cannot inspect,
    /// such as a patch or a stash, or from committed content that differs;
    /// the user decides.
    Undetermined,
    Blocked,
    /// Git will read the stash selection from mutable input after handoff.
    UnverifiedStashPaths,
    /// Git records the entire index, whose protected entries were not clear.
    UnverifiedStashIndex,
}

/// What an operation does to the paths it names. Unmodeled commands write.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Access {
    /// Literal output arguments are data, not filesystem operands.
    Output,
    Read,
    /// Lists or records paths; may name the .shield folder itself.
    Index,
    /// Ordinary Git diff inspection may print tracked agent settings.
    Diff,
    /// Prints names and metadata only, never contents, so agent settings may
    /// be named; review state and the cache stay closed.
    Metadata,
    Write,
    /// A modeled Git restoration; policy replacement still requires human review.
    Restore,
}

/// Read-only commands whose only effect on named paths is reading them.
const READERS: &str =
    "cat head tail wc grep egrep fgrep rg stat file diff cmp jq shasum sha256sum md5";
const FIND_ACTIONS: &str = "-exec -execdir -ok -okdir -delete -fprint -fprint0 -fprintf -fls";

/// The strictest protection found, with the submitted path that caused it when
/// one path did.
pub(super) type Finding = (Protection, Option<String>);

#[derive(Default, serde::Serialize)]
pub(super) struct Restorations {
    pub(super) policy_change: bool,
    pub(super) settings_change: bool,
    pub(super) worktrees: std::collections::BTreeSet<String>,
    pub(super) calls: Vec<RestorationCall>,
}

#[derive(serde::Serialize)]
pub(super) struct RestorationCall {
    pub(super) arguments: Vec<String>,
    pub(super) cwd: String,
    pub(super) worktree: String,
}

impl Restorations {
    fn merge(&mut self, other: Self) {
        self.policy_change |= other.policy_change;
        self.settings_change |= other.settings_change;
        self.worktrees.extend(other.worktrees);
        self.calls.extend(other.calls);
    }
}

pub(super) struct Inspection {
    pub(super) finding: Finding,
    pub(super) restorations: Restorations,
}

impl Inspection {
    fn plain(finding: Finding) -> Self {
        Self {
            finding,
            restorations: Restorations::default(),
        }
    }
}

/// These names are private operator storage, including user-wide continuity
/// and registry authority. Project policy-edit permission never opens them.
fn state_component(part: &std::ffi::OsStr) -> bool {
    [
        "state",
        "review",
        "cache",
        "reviewed-policies",
        "authority",
        "runtime-incarnation",
        "registry-trust",
        "sdk-read-profiles",
        "sdk-read-contexts",
        "registries.json",
        "registries.state",
        "registries.lease",
        "registries.lock",
        "registries.new",
    ]
    .iter()
    .any(|name| native_name(part, std::ffi::OsStr::new(name)))
}

/// The index of a default authoring bundle's directory in `parts`: `.shield`,
/// or `shield` directly under `.config`.
fn bundle_index(parts: &[&std::ffi::OsStr]) -> Option<usize> {
    parts.iter().enumerate().find_map(|(index, part)| {
        if native_name(part, std::ffi::OsStr::new(".shield")) {
            return Some(index);
        }
        (index > 0
            && native_name(part, std::ffi::OsStr::new("shield"))
            && native_name(parts[index - 1], std::ffi::OsStr::new(".config")))
        .then_some(index)
    })
}

/// Whether `path` is one of the explicit selection's policy files, or within
/// its bundle's pack cache.
fn explicit_policy(path: &Path) -> Option<bool> {
    crate::layout::explicit_policy_files()
        .into_iter()
        .find(|file| path == file || (file.ends_with("cache") && path.starts_with(file)))
        .map(|file| file.ends_with("cache"))
}

fn user_state_root() -> Option<&'static Path> {
    // A runtime belongs to its starting user. Resolve this fixed owner once,
    // rather than adding filesystem probes for every inspected operand.
    static ROOT: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();
    ROOT.get_or_init(|| {
        #[cfg(windows)]
        let base = std::env::var_os("LOCALAPPDATA");
        #[cfg(not(windows))]
        let base = std::env::var_os("HOME");
        base.map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .map(|path| resolve_existing(&path.join(".shield")))
    })
    .as_deref()
}

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
    if user_state_root().is_some_and(|private| path.starts_with(private)) {
        return "state";
    }
    if guard
        .working_files
        .is_some_and(|scope| scope.write.contains(&path))
    {
        return "working-root";
    }
    if guard.cache.is_some_and(|cache| path.starts_with(cache)) {
        return "state";
    }
    if let Some(cache) = explicit_policy(&path) {
        return if cache { "state" } else { "policy" };
    }
    let parts: Vec<_> = path.components().map(Component::as_os_str).collect();
    let Some(index) = bundle_index(&parts) else {
        return "settings";
    };
    match parts.get(index + 1) {
        Some(part) if state_component(part) => "state",
        None => "folder",
        Some(part) if native_name(part, std::ffi::OsStr::new(".gitignore")) => "ignore",
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

pub(super) fn inspected(
    value: &Value,
    root: &Path,
    cwd: &Path,
    guard: Guard<'_>,
    access: Access,
) -> Inspection {
    let mut restorations = Restorations::default();
    let finding = inspect_recorded(value, root, cwd, true, guard, access, &mut restorations);
    Inspection {
        finding,
        restorations,
    }
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
        "sed" | "awk" | "sort" | "uniq" | "cut" | "nl" | "comm" | "paste" | "column" | "base64"
            if crate::location::text_read_only(executable, &arguments) =>
        {
            Access::Read
        }
        _ if listed(READERS, leaf) => Access::Read,
        "ls" => Access::Metadata,
        "find" if !arguments.iter().any(|a| listed(FIND_ACTIONS, a)) => Access::Metadata,
        "git" => git::access(&arguments),
        _ => Access::Write,
    }
}

/// What a command prints beyond the paths it names: a Git invocation's
/// revision paths, patches and searches. Directory searches do not inspect
/// descendants for protected files; explicitly named auxiliary reads still do.
fn output_protection(
    request: &Value,
    root: &Path,
    cwd: &Path,
    cwd_known: bool,
    guard: Guard<'_>,
) -> Finding {
    output_recorded(
        request,
        root,
        cwd,
        cwd_known,
        guard,
        &mut Restorations::default(),
    )
}

fn output_recorded(
    request: &Value,
    root: &Path,
    cwd: &Path,
    cwd_known: bool,
    guard: Guard<'_>,
    restorations: &mut Restorations,
) -> Finding {
    let Some(executable) = request["executable"].as_str() else {
        return (Protection::None, None);
    };
    let arguments: Vec<&str> = request["arguments"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    let leaf = Path::new(executable)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(executable);
    match leaf {
        "git" => {
            let result = git::inspected(&arguments, root, cwd, cwd_known, guard);
            restorations.merge(result.restorations);
            result.finding
        }
        "grep" | "egrep" | "fgrep" => grep_protection(
            &arguments
                .iter()
                .map(|s| (*s).to_owned())
                .collect::<Vec<_>>(),
            cwd,
            guard,
        ),
        _ => (Protection::None, None),
    }
}

/// Protect explicit auxiliary reads, including attached pattern-file values.
fn grep_protection(arguments: &[String], cwd: &Path, guard: Guard<'_>) -> Finding {
    let parsed = crate::location::grep_parse(arguments, cwd);
    strictest(
        parsed
            .reads
            .iter()
            .map(|path| inspect_path(path, cwd, guard, Access::Read)),
    )
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

pub(super) fn outside(
    operation: &Value,
    root: &Path,
    cwd: &Path,
    unapproved: &dyn Fn(&Path, &str) -> bool,
) -> Option<Outside> {
    let request = &operation["request"];
    let access = if matches!(access(request), Access::Write | Access::Restore) {
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
        .find(|path| path.is_absolute() && unapproved(path, access))
        .map(|path| {
            let folder = if path.is_dir() {
                path.clone()
            } else {
                path.parent().map_or(path.clone(), Path::to_path_buf)
            };
            let home = std::env::home_dir().map(|home| resolve_existing(&home));
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
    operator_route(value, cwd).is_some()
}

/// A recovery category derived from inspected argv, never an allowance.
pub(super) fn operator_route(value: &Value, cwd: &Path) -> Option<&'static str> {
    match value {
        Value::Object(fields) => {
            let cwd = fields
                .get("cwd")
                .and_then(Value::as_str)
                .map_or(cwd, Path::new);
            value["executable"]
                .as_str()
                .and_then(|executable| {
                    let shield = Path::new(executable).file_name().is_some_and(operator_name)
                        || submitted_path(executable, cwd)
                            .canonicalize()
                            .is_ok_and(|p| p.file_name().is_some_and(operator_name));
                    let arguments = &value["arguments"];
                    if shield && (version_only(arguments) || help_only(arguments)) {
                        return None;
                    }
                    let server = arguments.as_array().is_some_and(|arguments| {
                        arguments
                            .windows(2)
                            .any(|pair| pair[0] == "mcp" && pair[1] == "serve")
                    });
                    (server || shield).then(|| recovery_category(arguments))
                })
                .or_else(|| {
                    fields
                        .iter()
                        .filter(|(key, _)| !["evaluation", "source_units"].contains(&key.as_str()))
                        .find_map(|(_, v)| operator_route(v, cwd))
                })
        }
        Value::Array(values) => values.iter().find_map(|v| operator_route(v, cwd)),
        _ => None,
    }
}

fn recovery_category(arguments: &Value) -> &'static str {
    let Some(arguments) = arguments.as_array() else {
        return "administration";
    };
    let mut words = arguments.iter().filter_map(Value::as_str);
    let mut command = words.next();
    while let Some(word) = command {
        if matches!(word, "--project" | "--debug-log-dir" | "--cache-dir") {
            words.next();
        } else if !matches!(word, "--json" | "--no-debug-log")
            && !["--project=", "--debug-log-dir=", "--cache-dir="]
                .iter()
                .any(|prefix| word.starts_with(prefix))
        {
            break;
        }
        command = words.next();
    }
    match (command, words.next()) {
        (Some("doctor" | "status"), _) => "diagnostics",
        (Some("setup"), _) | (Some("mcp"), Some("setup" | "remove")) => "registration",
        (Some("rule" | "pack" | "policy"), _) => "policy",
        _ => "administration",
    }
}

/// Clap prints the top-level version and exits before running Shield.
fn version_only(arguments: &Value) -> bool {
    matches!(arguments.as_array().map(Vec::as_slice), Some([flag]) if flag == "--version" || flag == "-V")
}

/// A trailing help flag after plain words cannot select an operator action.
/// Only project selection may precede the command words; ordinary policy and path guards
/// still decide the command and any redirections or neighboring operations.
fn help_only(arguments: &Value) -> bool {
    let Some(arguments) = arguments.as_array() else {
        return false;
    };
    let Some((last, commands)) = arguments.split_last() else {
        return false;
    };
    if last != "--help" && last != "-h" {
        return false;
    }
    let mut commands = commands.iter();
    while let Some(word) = commands.next() {
        if word == "--project" {
            if !commands.next().is_some_and(|path| {
                path.as_str()
                    .is_some_and(|s| !s.is_empty() && !s.starts_with('-'))
            }) {
                return false;
            }
        } else if !word.as_str().is_some_and(|word| {
            (word.starts_with("--project=") && word.len() > 10)
                || (word.starts_with(|c: char| c.is_ascii_lowercase())
                    && word.bytes().all(|c| c.is_ascii_lowercase() || c == b'-'))
        }) {
            return false;
        }
    }
    true
}

fn operator_name(name: &std::ffi::OsStr) -> bool {
    #[cfg(windows)]
    {
        [
            "shield",
            "shield.exe",
            "shield.com",
            "shield.cmd",
            "shield.bat",
        ]
        .iter()
        .any(|expected| native_name(name, std::ffi::OsStr::new(expected)))
    }
    #[cfg(not(windows))]
    {
        name == "shield"
    }
}

/// Operations carry their request; parsed structures carry literal words and
/// redirection roles. Preserve each command's access when inspecting compounds.
fn inspected_access(value: &Value, fallback: Access) -> Access {
    let input = |operator: &Value| {
        if operator == "<" || operator == "<<" || operator == "<<-" || operator == "<<<" {
            Access::Read
        } else {
            Access::Write
        }
    };
    if value["request"].is_object() {
        access(&value["request"])
    } else if value["coverage"] == "redirection" {
        input(&value["redirection"]["operator"])
    } else if let Some(words) = value["words"].as_array() {
        let literals: Option<Vec<&str>> =
            words.iter().map(|word| word["literal"].as_str()).collect();
        literals
            .and_then(|words| {
                let (executable, arguments) = words.split_first()?;
                Some(access(
                    &serde_json::json!({"executable":executable,"arguments":arguments}),
                ))
            })
            .unwrap_or(Access::Write)
    } else if value["operator"].is_string() && value.get("target").is_some() {
        input(&value["operator"])
    } else {
        fallback
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
    inspect_recorded(
        value,
        root,
        cwd,
        cwd_known,
        guard,
        access,
        &mut Restorations::default(),
    )
}

fn inspect_recorded(
    value: &Value,
    root: &Path,
    cwd: &Path,
    cwd_known: bool,
    guard: Guard<'_>,
    access: Access,
    restorations: &mut Restorations,
) -> Finding {
    match value {
        Value::Object(fields) => {
            let cwd = fields
                .get("cwd")
                .and_then(Value::as_str)
                .map_or(cwd, Path::new);
            let cwd_known = value["context"]["cwd_known"].as_bool().unwrap_or(cwd_known);
            if fields
                .keys()
                .all(|key| ["executable", "arguments", "cwd", "project"].contains(&key.as_str()))
                && let Some(finding) = touch_request(value, cwd, cwd_known, guard)
            {
                return finding;
            }
            if unresolved_redirect(value) || destructive(value, root, cwd, cwd_known, guard) {
                return (Protection::Blocked, None);
            }
            let access = inspected_access(value, access);
            let named = named_files(&value["request"], cwd, guard);
            let output =
                output_recorded(&value["request"], root, cwd, cwd_known, guard, restorations);
            strictest(
                [named, output].into_iter().chain(
                    fields
                        .iter()
                        .filter(|(key, _)| {
                            ![
                                "evaluation",
                                "source",
                                "source_units",
                                "project",
                                "cwd",
                                "cwd_success",
                                "cwd_failure",
                                "logical_cwd_success",
                                "context",
                                "stdin",
                            ]
                            .contains(&key.as_str())
                        })
                        .map(|(key, v)| {
                            if key == "words"
                                && let Some(words) = v.as_array()
                            {
                                let literals: Option<Vec<_>> =
                                    words.iter().map(|word| word["literal"].as_str()).collect();
                                if let Some(words) = literals
                                    && let Some((executable, arguments)) = words.split_first()
                                    && let Some(finding) =
                                        touch_paths(executable, arguments, cwd, cwd_known, guard)
                                {
                                    return finding;
                                }
                            }
                            if matches!(access, Access::Output | Access::Restore)
                                && key == "words"
                                && let Some(words) = v.as_array()
                            {
                                return strictest(words.iter().enumerate().map(|(index, word)| {
                                    inspect_recorded(
                                        word,
                                        root,
                                        cwd,
                                        cwd_known,
                                        guard,
                                        if index == 0 { Access::Write } else { access },
                                        restorations,
                                    )
                                }));
                            }
                            let access = if matches!(access, Access::Output | Access::Restore)
                                && key == "executable"
                            {
                                Access::Write
                            } else {
                                access
                            };
                            inspect_recorded(v, root, cwd, cwd_known, guard, access, restorations)
                        }),
                ),
            )
        }
        Value::Array(values) => strictest(
            values
                .iter()
                .map(|v| inspect_recorded(v, root, cwd, cwd_known, guard, access, restorations)),
        ),
        Value::String(s) => inspect_path(s, cwd, guard, access),
        _ => (Protection::None, None),
    }
}

fn touch_request(
    request: &Value,
    cwd: &Path,
    cwd_known: bool,
    guard: Guard<'_>,
) -> Option<Finding> {
    let executable = request["executable"].as_str()?;
    let arguments: Option<Vec<_>> = request["arguments"]
        .as_array()?
        .iter()
        .map(Value::as_str)
        .collect();
    touch_paths(executable, &arguments?, cwd, cwd_known, guard)
}

fn touch_paths(
    executable: &str,
    arguments: &[&str],
    cwd: &Path,
    cwd_known: bool,
    guard: Guard<'_>,
) -> Option<Finding> {
    let footprint = crate::location::touch::parse(executable, arguments)?;
    if !footprint.complete {
        return None;
    }
    if !cwd_known
        && footprint
            .reference
            .into_iter()
            .chain(footprint.destinations.iter().copied())
            .any(|path| !Path::new(path).is_absolute())
    {
        return Some((Protection::Blocked, None));
    }
    Some(strictest(
        std::iter::once(inspect_path(executable, cwd, guard, Access::Write))
            .chain(
                footprint
                    .reference
                    .into_iter()
                    .map(|path| inspect_path(path, cwd, guard, Access::Metadata)),
            )
            .chain(
                footprint
                    .destinations
                    .into_iter()
                    .map(|path| inspect_path(path, cwd, guard, Access::Write)),
            ),
    ))
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
    if ["rg", "/bin/rg", "/usr/bin/rg"].contains(&executable) {
        files.extend(
            crate::location::rg_auxiliary_reads(
                &arguments
                    .iter()
                    .map(|arg| (*arg).to_owned())
                    .collect::<Vec<_>>(),
            )
            .into_iter()
            .map(|path| (false, path)),
        );
    }
    if let Some(path) = crate::http_client::fetches(executable, &arguments)
        .and_then(|fetches| fetches.into_iter().find_map(|fetch| fetch.output))
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

/// Search selectors must remain within the explicitly decided root. Searches
/// do not inspect descendants for protected state or impose metadata walk limits.
pub(super) fn recursive_read(search: &Value) -> Finding {
    let unbounded = search["globs"].as_array().is_some_and(|globs| {
        globs.iter().filter_map(Value::as_str).any(|glob| {
            glob.split(['{', ','])
                .any(|part| part.trim_start_matches('!').starts_with(['/', '~']))
                || glob.split(['/', '{', '}', ',']).any(|part| part == "..")
        })
    });
    if unbounded {
        (Protection::Blocked, None)
    } else {
        (Protection::None, None)
    }
}

// Policy files may be read and recorded; review state and the cache never.
// Agent settings may carry credentials, so reading them is the user's call.
// The operator's path access can open agent settings and policy files, never
// review state, the cache, the .shield folder itself or its ignore file.
fn classify(path: &Path, guard: Guard<'_>, access: Access) -> Protection {
    if user_state_root().is_some_and(|private| path.starts_with(private)) {
        return Protection::Blocked;
    }
    if matches!(access, Access::Write | Access::Restore)
        && guard.working_files.is_some_and(|scope| {
            scope
                .write
                .iter()
                .chain(&scope.skills)
                .any(|root| root == path)
        })
    {
        return Protection::Blocked;
    }
    // Explicit bundles keep the same direct-write boundary and reviewed Git
    // restoration route as defaults; their caches never open.
    if let Some(cache) = explicit_policy(path) {
        return if cache || access == Access::Write {
            Protection::Blocked
        } else if access == Access::Restore {
            Protection::Undetermined
        } else {
            Protection::None
        };
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
    let Some(index) = bundle_index(&parts) else {
        if configuration
            && guard.working_files.is_some_and(|scope| {
                scope
                    .skills
                    .iter()
                    .any(|root| resolve_existing(root) == *root && path.starts_with(root))
            })
        {
            return Protection::None;
        }
        if configuration
            && !matches!(access, Access::Write | Access::Restore)
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
            Access::Write | Access::Restore if allowed(guard.allow.agent_settings_change) => {
                Protection::None
            }
            Access::Write | Access::Restore => Protection::Blocked,
            Access::Metadata | Access::Diff => Protection::None,
            _ if guard.allow.agent_settings_read == Read::Allow => Protection::None,
            _ => Protection::Sensitive,
        };
    };
    match parts.get(index + 1) {
        Some(part) if state_component(part) => Protection::Blocked,
        // Naming the folder reaches its state unless Git and listings skip it.
        None if matches!(access, Access::Index | Access::Metadata | Access::Diff)
            && path.join(".gitignore").is_file() =>
        {
            Protection::None
        }
        None => Protection::Blocked,
        // The ignore file keeps review state and the cache out of Git.
        Some(part)
            if matches!(access, Access::Write | Access::Restore)
                && native_name(part, std::ffi::OsStr::new(".gitignore")) =>
        {
            Protection::Blocked
        }
        // Arbitrary edits require a proposal; modeled restorations ask once.
        Some(_) if access == Access::Write => Protection::Blocked,
        Some(_) if access == Access::Restore => Protection::Undetermined,
        Some(_) => Protection::None,
    }
}

/// A bounded SDK read cannot use policy to open private operator storage or
/// credential-bearing host configuration. The operand is already canonical.
pub(crate) fn read_allowed(path: &Path, cache: Option<&Path>) -> bool {
    classify(
        path,
        Guard {
            cache,
            allow: PathAccess::default(),
            working_files: None,
        },
        Access::Read,
    ) == Protection::None
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
            let historical = *owned == crate::review::integrations::LEGACY_INTERCEPTOR_PATH;
            let owned: Vec<_> = Path::new(owned)
                .components()
                .map(Component::as_os_str)
                .collect();
            parts.windows(owned.len()).any(|window| {
                window
                    .iter()
                    .zip(&owned)
                    .all(|(left, right)| native_name(left, right))
                    && (!historical || crate::review::integrations::protects_legacy_path(path))
            })
        })
}

#[cfg(test)]
mod portable_policy_guards {
    use super::*;

    /// Grouped and explicit bundles hold policy files like `.shield/`: readable,
    /// never writable even when agent settings are opened, caches closed.
    #[test]
    fn grouped_and_explicit_bundles_are_policy_files_whatever_the_settings_choice() {
        let root = assert_fs::TempDir::new().unwrap();
        let cwd = root.path().canonicalize().unwrap();
        std::fs::create_dir_all(cwd.join("policy")).unwrap();
        std::fs::write(cwd.join("policy/acme.yml"), "schema_version: 1\n").unwrap();
        crate::layout::bind(
            &cwd.join("policy/acme.yml"),
            crate::layout::Base::Invocation,
        );
        crate::layout::select(&cwd).unwrap();
        let guard = Guard {
            cache: None,
            allow: PathAccess {
                agent_settings_read: Read::Allow,
                agent_settings_change: Change::Allow,
                ..PathAccess::default()
            },
            working_files: None,
        };
        for policy in [
            ".config/shield/config.yml",
            ".config/shield/rules.yml",
            ".config/shield/config.local.yml",
            "policy/acme.yml",
            "policy/shield.lock",
            "policy/config.local.yml",
        ] {
            let path = cwd.join(policy);
            assert_eq!(
                classify(&path, guard, Access::Write),
                Protection::Blocked,
                "{policy}"
            );
            assert_eq!(
                classify(&path, guard, Access::Read),
                Protection::None,
                "{policy}"
            );
        }
        for closed in [
            ".shield/state/state.json",
            ".shield/review/state.json",
            "policy/cache/acme.yml",
        ] {
            assert_eq!(
                classify(&cwd.join(closed), guard, Access::Read),
                Protection::Blocked,
                "{closed}"
            );
        }
        // Other files beside an explicit selection stay ordinary project files.
        assert_eq!(
            classify(&cwd.join("policy/notes.md"), guard, Access::Write),
            Protection::None
        );
        assert_eq!(group(".config/shield/rules.yml", &cwd, guard), "policy");
        assert_eq!(group("policy/acme.yml", &cwd, guard), "policy");
    }

    /// An explicit configuration's rules, local rules and pack artifacts are
    /// policy files wherever it names them, not only at the usual names.
    #[test]
    fn files_an_explicit_configuration_names_are_policy_files() {
        let root = assert_fs::TempDir::new().unwrap();
        let cwd = root.path().canonicalize().unwrap();
        std::fs::create_dir_all(cwd.join("policy")).unwrap();
        std::fs::write(
            cwd.join("policy/acme.yml"),
            "schema_version: 1\nrules: custom.yml\npacks:\n- id: acme/read\n  registry_identity: main\n  version: 1.0.0\n  profile: recommended\n  path: vendor/acme-read.yml\n",
        )
        .unwrap();
        std::fs::write(
            cwd.join("policy/config.local.yml"),
            "schema_version: 1\nrules: personal.yml\n",
        )
        .unwrap();
        crate::layout::bind(
            &cwd.join("policy/acme.yml"),
            crate::layout::Base::Invocation,
        );
        crate::layout::select(&cwd).unwrap();
        let guard = Guard {
            cache: None,
            allow: PathAccess {
                agent_settings_read: Read::Allow,
                agent_settings_change: Change::Allow,
                ..PathAccess::default()
            },
            working_files: None,
        };
        for policy in [
            "policy/custom.yml",
            "policy/personal.yml",
            "policy/vendor/acme-read.yml",
        ] {
            let path = cwd.join(policy);
            assert_eq!(
                classify(&path, guard, Access::Write),
                Protection::Blocked,
                "{policy}"
            );
            assert!(crate::layout::in_shield_files(&cwd, &path), "{policy}");
            assert_eq!(group(policy, &cwd, guard), "policy");
        }
        assert_eq!(
            classify(&cwd.join("policy/vendor/notes.md"), guard, Access::Write),
            Protection::None
        );
        // A changed reference moves the protection with it.
        std::fs::write(
            cwd.join("policy/acme.yml"),
            "schema_version: 1\nrules: renamed.yml\npacks: []\n",
        )
        .unwrap();
        assert_eq!(
            classify(&cwd.join("policy/renamed.yml"), guard, Access::Write),
            Protection::Blocked
        );
    }

    #[test]
    #[cfg(unix)]
    fn touch_unknown_cwd_blocks_relative_references_and_destinations_in_request_and_ast() {
        let root = assert_fs::TempDir::new().unwrap();
        let cwd = root.path().canonicalize().unwrap();
        let reference = cwd.join("reference").to_str().unwrap().to_owned();
        let destination = cwd.join("destination").to_str().unwrap().to_owned();
        let guard = Guard {
            cache: None,
            allow: PathAccess::default(),
            working_files: None,
        };
        for (reference, destination, blocked) in [
            ("relative-reference", destination.as_str(), true),
            (reference.as_str(), "relative-destination", true),
            (reference.as_str(), destination.as_str(), false),
        ] {
            let request = serde_json::json!({"executable":"touch","arguments":["-r",reference,destination],"cwd":cwd,"project":cwd});
            let structure = serde_json::json!({"words":[{"literal":"touch"},{"literal":"-r"},{"literal":reference},{"literal":destination}]});
            for value in [request, structure] {
                assert_eq!(
                    inspect(&value, &cwd, &cwd, false, guard, Access::Write).0
                        == Protection::Blocked,
                    blocked,
                    "{value}"
                );
            }
        }
    }

    #[test]
    #[cfg(unix)]
    fn touch_reference_is_metadata_and_private_references_or_any_private_write_are_blocked() {
        let root = assert_fs::TempDir::new().unwrap();
        let cwd = root.path().canonicalize().unwrap();
        std::fs::create_dir(cwd.join(".shield")).unwrap();
        std::fs::write(cwd.join(".shield/config.yml"), "Acme policy").unwrap();
        std::fs::create_dir(cwd.join(".codex")).unwrap();
        std::fs::write(
            cwd.join(".codex/config.toml"),
            "Acme sensitive configuration",
        )
        .unwrap();
        for directory in [".shield/state", ".shield/review"] {
            std::fs::create_dir_all(cwd.join(directory)).unwrap();
            std::fs::write(cwd.join(directory).join("state.json"), "Acme private").unwrap();
        }
        std::os::unix::fs::symlink(cwd.join(".shield/state/state.json"), cwd.join("alias"))
            .unwrap();
        let guard = Guard {
            cache: None,
            allow: PathAccess::default(),
            working_files: None,
        };
        for (reference, targets, blocked) in [
            (".shield/config.yml", vec!["one", "two"], false),
            (".codex/config.toml", vec!["one"], false),
            (".shield/state/state.json", vec!["one"], true),
            (".shield/review/state.json", vec!["one"], true),
            ("alias", vec!["one"], true),
            ("ordinary", vec!["one", ".shield/state/state.json"], true),
            ("ordinary", vec!["one", ".shield/review/state.json"], true),
        ] {
            let mut arguments = vec!["-r", reference];
            arguments.extend(targets);
            let request = serde_json::json!({"executable":"touch","arguments":arguments,"cwd":cwd,"project":cwd});
            let found = inspect(&request, &cwd, &cwd, true, guard, Access::Write);
            assert_eq!(found.0 == Protection::Blocked, blocked, "{arguments:?}");
        }
    }

    #[test]
    fn reviewed_policy_receipts_remain_private_with_all_edit_permissions_enabled() {
        let root = assert_fs::TempDir::new().unwrap();
        let guard = Guard {
            cache: None,
            allow: PathAccess {
                agent_settings_read: Read::Allow,
                agent_settings_change: Change::Allow,
                ..PathAccess::default()
            },
            working_files: None,
        };
        let path = root
            .path()
            .join("developer/.shield/reviewed-policies/receipt.json");
        assert_eq!(classify(&path, guard, Access::Read), Protection::Blocked);
        assert_eq!(classify(&path, guard, Access::Write), Protection::Blocked);
        assert_eq!(group(path.to_str().unwrap(), root.path(), guard), "state");
        assert!(removal(path.parent().unwrap(), root.path(), guard));
    }

    #[test]
    fn user_continuity_and_registry_authority_stay_private_under_policy_edit_permission() {
        let root = assert_fs::TempDir::new().unwrap();
        let guard = Guard {
            cache: None,
            working_files: None,
            allow: PathAccess {
                agent_settings_read: Read::Allow,
                agent_settings_change: Change::Allow,
                ..PathAccess::default()
            },
        };
        for name in [
            "authority/registration/acme.json",
            "authority/consumed/acme.json",
            "runtime-incarnation/host.json",
            "registry-trust/active.json",
            "sdk-read-profiles/index.json",
            "sdk-read-contexts/index.json",
            "registries.json",
            "registries.state/active.json",
            "registries.lease",
            "registries.lock",
            "registries.new",
        ] {
            let path = root.path().join("developer/.shield").join(name);
            for access in [
                Access::Read,
                Access::Write,
                Access::Restore,
                Access::Metadata,
                Access::Index,
            ] {
                assert!(
                    classify(&path, guard, access) == Protection::Blocked,
                    "{name}"
                );
            }
            assert_eq!(group(path.to_str().unwrap(), root.path(), guard), "state");
            assert!(removal(&path, root.path(), guard));
        }
        // User-wide storage is protected independently of this project's
        // location or cache override, including removal through an ancestor.
        if let Some(private) = user_state_root() {
            assert_eq!(
                classify(
                    &private.join("registry-trust/active.json"),
                    guard,
                    Access::Write
                ),
                Protection::Blocked
            );
            assert!(removal(private.parent().unwrap(), root.path(), guard));
        }
    }
}

pub(super) fn removal(path: &Path, root: &Path, guard: Guard<'_>) -> bool {
    let path = resolve_existing(path);
    let locked = |path: &Path| classify(path, guard, Access::Write) == Protection::Blocked;
    locked(&path)
        || user_state_root().is_some_and(|private| private.starts_with(&path))
        || guard.working_files.is_some_and(|scope| {
            scope
                .write
                .iter()
                .chain(&scope.skills)
                .any(|root| root.starts_with(&path))
        })
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

#[cfg(test)]
mod historical_skills {
    use super::*;
    use serde_json::json;

    #[test]
    fn operator_recovery_category_skips_all_global_flags() {
        for arguments in [
            json!(["--no-debug-log", "doctor"]),
            json!(["--debug-log-dir", "/tmp/acme", "status"]),
            json!(["--debug-log-dir=/tmp/acme", "doctor"]),
            json!(["--cache-dir", "/tmp/acme", "status"]),
            json!(["--cache-dir=/tmp/acme", "doctor"]),
            json!([
                "--project=/projects/acme",
                "--json",
                "--no-debug-log",
                "--debug-log-dir",
                "mcp",
                "status"
            ]),
        ] {
            assert_eq!(recovery_category(&arguments), "diagnostics", "{arguments}");
        }
        assert_eq!(
            recovery_category(&json!(["--debug-log-dir", "/tmp/acme", "mcp", "setup"])),
            "registration"
        );
        assert_eq!(
            recovery_category(&json!(["--cache-dir=/tmp/acme", "pack", "update"])),
            "policy"
        );
        assert_eq!(
            recovery_category(&json!(["--unknown", "status"])),
            "administration"
        );
    }

    #[test]
    fn unowned_shield_skill_folders_follow_ordinary_path_policy() {
        let root = assert_fs::TempDir::new().unwrap();
        let bundle =
            crate::configuration::Bundle::capture("schema_version: 1\npacks: []\n", &[], None)
                .unwrap();
        crate::review::mcp::initialize_fixture(root.path(), &bundle);
        let guard = Guard {
            cache: None,
            allow: PathAccess::default(),
            working_files: None,
        };
        for relative in [
            ".claude/skills/shield",
            ".claude/skills/shield/SKILL.md",
            "demo/.claude/skills/shield/SKILL.md",
        ] {
            assert!(
                classify(&root.path().join(relative), guard, Access::Write) == Protection::None,
                "{relative}"
            );
        }
        assert!(!removal(
            &root.path().join(".claude/skills"),
            root.path(),
            guard
        ));
        assert_eq!(
            classify(
                &root.path().join(".codex/config.toml"),
                guard,
                Access::Write
            ),
            Protection::Blocked
        );
    }
}

#[cfg(all(test, unix))]
mod editable_skills {
    use super::*;
    #[test]
    fn skill_write_scope_preserves_configuration_and_state_guards() {
        let root = assert_fs::TempDir::new().unwrap();
        let config = root.path().canonicalize().unwrap().join(".codex");
        let skills = config.join("skills");
        std::fs::create_dir_all(&skills).unwrap();
        let scope = crate::location::WorkingFiles {
            skills: vec![skills.clone()],
            protected: vec![config.clone()],
            ..Default::default()
        };
        let guard = Guard {
            cache: None,
            allow: PathAccess::default(),
            working_files: Some(&scope),
        };
        assert_eq!(
            classify(&skills.join("new/SKILL.md"), guard, Access::Write),
            Protection::None
        );
        assert_eq!(classify(&skills, guard, Access::Write), Protection::Blocked);
        assert!(removal(&skills, root.path(), guard));
        assert!(removal(&config, root.path(), guard));
        assert!(!removal(&skills.join("new/SKILL.md"), root.path(), guard));
        assert_eq!(
            classify(&config.join("config.toml"), guard, Access::Write),
            Protection::Blocked
        );
        for state in [".shield/state/state.json", ".shield/review/state.json"] {
            assert_eq!(
                classify(&skills.join(state), guard, Access::Write),
                Protection::Blocked
            );
        }
        std::fs::remove_dir(&skills).unwrap();
        std::os::unix::fs::symlink(&config, &skills).unwrap();
        assert_eq!(
            classify(&config.join("credentials.json"), guard, Access::Write),
            Protection::Blocked
        );
    }
}

#[cfg(all(test, windows))]
mod windows_tests {
    use super::*;

    #[test]
    fn protected_state_and_settings_include_native_case_and_stream_aliases() {
        let guard = Guard {
            cache: None,
            allow: PathAccess::default(),
            working_files: None,
        };
        for path in [
            r"C:\Acme\.SHIELD\ReVieW\state.json",
            r"C:\Acme\.shield.\review\state.json:stream",
            r"C:\Acme\.shield\CACHE\plan.bin",
            r"C:\Acme\.CoDeX\CONFIG.TOML:stream",
            r"C:\Acme\.CLAUDE\settings.json::$DATA",
        ] {
            assert!(
                matches!(
                    classify(Path::new(path), guard, Access::Write),
                    Protection::Blocked
                ),
                "{path}"
            );
        }
        assert!(matches!(
            classify(Path::new(r"C:\Acme\notes.txt"), guard, Access::Write),
            Protection::None
        ));
        assert_eq!(
            group(
                r"C:\Acme\.SHIELD\ReVieW\state.json",
                Path::new(r"C:\Acme"),
                guard
            ),
            "state"
        );
    }

    #[test]
    fn windows_operator_cli_includes_native_executables_and_shims() {
        for executable in [
            "shield",
            "shield.exe",
            r"C:\Tools\SHIELD.EXE",
            r"C:\Tools\shield.cmd",
        ] {
            assert!(
                operator_cli(
                    &serde_json::json!({"request":{"executable":executable,"arguments":["policy","review"]}}),
                    Path::new(r"C:\Acme")
                ),
                "{executable}"
            );
        }
        assert!(!operator_cli(
            &serde_json::json!({"request":{"executable":"shield-notes.exe","arguments":[]}}),
            Path::new(r"C:\Acme")
        ));
    }
}
