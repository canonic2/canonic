//! The project's on-disk layout: which shared configuration file is the
//! policy source, the authoring bundle beside it, and the runtime state
//! directory. Discovery, loading, protection, publication and agent setup all
//! ask this module, so they agree on one selection.
//!
//! Precedence for a project is the trusted launcher's `--config` file, then
//! `<project>/.shield/config.yml`, then `<project>/.config/shield/config.yml`.
//! The two default trees are never merged. A present root file is selected
//! even when it is invalid, and an explicit file never falls back.
//!
//! The explicit file is bound once per process from its own arguments
//! ([`bind`]) and applies to the first project the process selects, which is
//! the project its command, MCP server or hook owns. Requests cannot reach it.
use std::{
    fs, io,
    path::{Component, Path, PathBuf},
    sync::OnceLock,
};

/// Shield's project directory, which holds runtime state in every layout.
pub(crate) const SHIELD: &str = ".shield";
/// Runtime state: history, pending requests, grants, proposals, project ID.
pub(crate) const STATE: &str = ".shield/state";
/// The state directory's name before it was `state`; converted on first use.
const EARLIER_STATE: &str = ".shield/review";
/// The root default configuration.
pub(crate) const ROOT_CONFIG: &str = ".shield/config.yml";
/// The grouped default configuration, used when no root file is present.
pub(crate) const GROUPED_CONFIG: &str = ".config/shield/config.yml";
/// The grouped bundle directory.
pub(crate) const GROUPED_BUNDLE: &str = ".config/shield";
/// The local files beside the selected shared configuration.
pub(crate) const LOCAL_FILES: [&str; 3] =
    ["config.local.yml", "rules.local.yml", "shield.local.lock"];

/// Where the selected shared configuration came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Source {
    /// The launcher's `--config` argument.
    Explicit,
    /// `.shield/config.yml`.
    Root,
    /// `.config/shield/config.yml`.
    Grouped,
    /// Neither default exists; new configuration is written at the root.
    Absent,
}

impl Source {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Explicit => "explicit",
            Self::Root => "root",
            Self::Grouped => "grouped",
            Self::Absent => "absent",
        }
    }
}

/// One project's selected shared configuration file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Selection {
    pub(crate) file: PathBuf,
    pub(crate) source: Source,
    /// Present default files the selection hides.
    pub(crate) shadowed: Vec<PathBuf>,
}

impl Selection {
    /// The authoring bundle: the selected file's directory, which holds its
    /// rules, locks and local files.
    pub(crate) fn bundle(&self) -> &Path {
        self.file.parent().unwrap_or(Path::new("/"))
    }
}

/// How a relative `--config` argument is resolved.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Base {
    /// Against the invoking command's working directory (operator commands).
    Invocation,
    /// Against the owning project (MCP servers and hooks, whose launch
    /// arguments agent setup writes relative to the project).
    Project,
}

struct Explicit {
    argument: PathBuf,
    cwd: PathBuf,
    base: Base,
    owner: OnceLock<PathBuf>,
}

#[cfg(not(test))]
static EXPLICIT: OnceLock<Explicit> = OnceLock::new();

#[cfg(test)]
std::thread_local! {
    /// Tests bind per thread, so a test's `--config` never applies to another
    /// test when a runner shares one process between them.
    static EXPLICIT: &'static OnceLock<Explicit> = Box::leak(Box::new(OnceLock::new()));
}

/// This process's binding.
#[cfg(not(test))]
fn explicit() -> &'static OnceLock<Explicit> {
    &EXPLICIT
}

/// This test thread's binding.
#[cfg(test)]
fn explicit() -> &'static OnceLock<Explicit> {
    EXPLICIT.with(|binding| *binding)
}

/// Bind this process's `--config` argument. Only process startup calls this,
/// with the argument the trusted launcher was started with.
pub(crate) fn bind(argument: &Path, base: Base) {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let _ = explicit().set(Explicit {
        argument: argument.to_path_buf(),
        cwd,
        base,
        owner: OnceLock::new(),
    });
}

fn canonical(root: &Path) -> PathBuf {
    root.canonicalize().unwrap_or_else(|_| root.to_path_buf())
}

fn present(path: &Path) -> bool {
    match fs::symlink_metadata(path) {
        Ok(_) => true,
        Err(error) => error.kind() != io::ErrorKind::NotFound,
    }
}

/// The explicit file bound to `root`, if this process's owning project is
/// `root`. The first project a process selects becomes its owner.
fn explicit_for(root: &Path) -> Option<PathBuf> {
    let explicit = explicit().get()?;
    let root = canonical(root);
    if *explicit.owner.get_or_init(|| root.clone()) != root {
        return None;
    }
    Some(if explicit.argument.is_absolute() {
        explicit.argument.clone()
    } else {
        match explicit.base {
            Base::Invocation => explicit.cwd.join(&explicit.argument),
            Base::Project => root.join(&explicit.argument),
        }
    })
}

/// The selected shared configuration of the project at `root`.
///
/// # Errors
///
/// An explicit file that is missing, unreadable or not a regular file refuses;
/// the defaults are never tried in its place.
pub(crate) fn select(root: &Path) -> io::Result<Selection> {
    let defaults = [root.join(ROOT_CONFIG), root.join(GROUPED_CONFIG)];
    if let Some(file) = explicit_for(root) {
        let metadata = fs::metadata(&file).map_err(|error| {
            io::Error::new(
                error.kind(),
                format!(
                    "the --config file {} cannot be read: {error}",
                    file.display()
                ),
            )
        })?;
        if !metadata.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("the --config file {} is not a regular file", file.display()),
            ));
        }
        fs::File::open(&file).map_err(|error| {
            io::Error::new(
                error.kind(),
                format!(
                    "the --config file {} cannot be read: {error}",
                    file.display()
                ),
            )
        })?;
        let file = file.canonicalize()?;
        let shadowed = defaults
            .into_iter()
            .filter(|path| present(path) && canonical(path) != file)
            .collect();
        return Ok(Selection {
            file,
            source: Source::Explicit,
            shadowed,
        });
    }
    let [root_file, grouped] = defaults;
    Ok(if present(&root_file) {
        let shadowed = present(&grouped).then_some(grouped).into_iter().collect();
        Selection {
            file: root_file,
            source: Source::Root,
            shadowed,
        }
    } else if present(&grouped) {
        Selection {
            file: grouped,
            source: Source::Grouped,
            shadowed: Vec::new(),
        }
    } else {
        Selection {
            file: root_file,
            source: Source::Absent,
            shadowed: Vec::new(),
        }
    })
}

/// The selected configuration file, or the explicit path that failed to
/// resolve so that reading it refuses instead of loading a default.
pub(crate) fn config_file(root: &Path) -> PathBuf {
    select(root).map_or_else(
        |_| explicit_for(root).unwrap_or_else(|| root.join(ROOT_CONFIG)),
        |selection| selection.file,
    )
}

/// Where `shield init` reads existing configuration or writes new: the
/// explicit file, which may not exist yet, else the selected default, else the
/// root default. Init never creates a second default bundle.
pub(crate) fn init_destination(root: &Path) -> PathBuf {
    explicit_for(root).unwrap_or_else(|| config_file(root))
}

/// The selected authoring bundle directory.
pub(crate) fn bundle(root: &Path) -> PathBuf {
    let file = config_file(root);
    file.parent()
        .map_or_else(|| root.join(SHIELD), Path::to_path_buf)
}

/// The path of a policy file named relative to the bundle. `config.yml` is the
/// selected shared configuration, which an explicit `--config` may name
/// differently; every other file sits in the bundle under its own name.
pub(crate) fn bundle_file(root: &Path, name: &str) -> PathBuf {
    if name == "config.yml" {
        config_file(root)
    } else {
        bundle(root).join(name)
    }
}

/// Whether `directory` holds a Shield project: either default configuration
/// file or runtime state. Unreadable entries count, so they are not skipped.
pub(crate) fn is_project(directory: &Path) -> bool {
    [ROOT_CONFIG, GROUPED_CONFIG, STATE, EARLIER_STATE]
        .iter()
        .any(|name| present(&directory.join(name)))
}

/// Whether discovery, walking up from its start folder, stops at `directory`:
/// it holds a project marker ([`is_project`]), or it holds this process's
/// explicit `--config` file. An operator's argument (resolved against its cwd)
/// claims the nearest folder containing the file, and a launcher's
/// project-relative argument claims the folder where it names an existing
/// file. Once the explicit file is bound to a project, only that folder
/// claims it.
pub(crate) fn discovery_root(directory: &Path) -> bool {
    is_project(directory) || holds_explicit(directory)
}

fn holds_explicit(directory: &Path) -> bool {
    let Some(explicit) = explicit().get() else {
        return false;
    };
    let directory = canonical(directory);
    if let Some(owner) = explicit.owner.get() {
        return *owner == directory;
    }
    if explicit.base == Base::Project && !explicit.argument.is_absolute() {
        return directory.join(&explicit.argument).is_file();
    }
    let file = if explicit.argument.is_absolute() {
        explicit.argument.clone()
    } else {
        explicit.cwd.join(&explicit.argument)
    };
    prospective(&file).is_ok_and(|file| file.starts_with(&directory))
}

/// Whether `root` has a present shared configuration file in its selection.
pub(crate) fn configured(root: &Path) -> bool {
    select(root)
        .is_ok_and(|selection| selection.source != Source::Absent && selection.file.is_file())
}

/// The runtime state directory as it is: `.shield/state`, or the earlier
/// `.shield/review` while only that exists. Locating never renames; the state
/// store converts the earlier name under its writer lock when it opens the
/// folder (`review::storage`).
pub(crate) fn state(root: &Path) -> PathBuf {
    let current = root.join(STATE);
    let earlier = root.join(EARLIER_STATE);
    if !present(&current) && present(&earlier) {
        earlier
    } else {
        current
    }
}

/// Whether the project at `root` has runtime state, under either name.
pub(crate) fn enrolled(root: &Path) -> bool {
    state(root).join("state.json").exists()
}

/// Whether both state names hold state: an earlier Shield still running
/// against this folder recreated `.shield/review` after it was converted.
pub(crate) fn split_state(root: &Path) -> bool {
    root.join(STATE).join("state.json").exists()
        && root.join(EARLIER_STATE).join("state.json").exists()
}

/// Both state directory names, for path guards that must protect either.
pub(crate) fn state_directories(root: &Path) -> [PathBuf; 2] {
    [root.join(STATE), root.join(EARLIER_STATE)]
}

/// The policy files of this process's explicit selection, canonical, for path
/// guards: the selected file and its fixed siblings in the bundle (rules,
/// locks, local files, ignore file and pack cache). Empty without an explicit
/// file. Defaults are protected by their directory names instead; an explicit
/// bundle may be any folder, even the project root, so only these names are.
pub(crate) fn explicit_policy_files() -> Vec<PathBuf> {
    let Some(explicit) = explicit().get() else {
        return Vec::new();
    };
    let Some(owner) = explicit.owner.get() else {
        return Vec::new();
    };
    let Ok(selection) = select(owner) else {
        return Vec::new();
    };
    if selection.source != Source::Explicit {
        return Vec::new();
    }
    let bundle = selection.bundle().to_path_buf();
    let referenced = referenced_files(&selection.file, &bundle);
    std::iter::once(selection.file.clone())
        .chain(
            ["rules.yml", "shield.lock", ".gitignore", "cache"]
                .into_iter()
                .map(str::to_owned)
                .chain(LOCAL_FILES.map(str::to_owned))
                .chain(referenced)
                .map(|name| bundle.join(name)),
        )
        .collect()
}

/// The bundle-relative files the selected configuration names, with its local
/// layer applied and on its own: rules, local rules, and selected and disabled
/// pack artifacts. Parsed once per version of the two files' bytes, since path
/// guards ask for every operand they classify.
fn referenced_files(file: &Path, bundle: &Path) -> Vec<String> {
    type Last = Option<((String, Option<String>), Vec<String>)>;
    static LAST: std::sync::Mutex<Last> = std::sync::Mutex::new(None);
    let Ok(shared) = fs::read_to_string(file) else {
        return Vec::new();
    };
    let local = fs::read_to_string(bundle.join("config.local.yml")).ok();
    let key = (shared, local);
    let mut last = LAST
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some((known, paths)) = last.as_ref()
        && *known == key
    {
        return paths.clone();
    }
    // The shared layer alone still names its files when the local layer is
    // invalid; the merged result adds what the local layer selects.
    let merged = crate::configuration::layers::merge(&key.0, key.1.as_deref()).ok();
    let mut paths: Vec<String> = [Some(key.0.clone()), merged]
        .into_iter()
        .flatten()
        .filter_map(|text| crate::configuration::Candidate::from_yaml(&text).ok())
        .flat_map(|candidate| {
            candidate
                .pack_paths()
                .chain(candidate.disabled_paths())
                .chain(candidate.rules_path())
                .chain(candidate.local_rules_path())
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .collect();
    paths.sort();
    paths.dedup();
    *last = Some((key, paths.clone()));
    paths
}

/// Whether `path`, inside the project at `root`, lies in Shield's own files:
/// `.shield/` (state and the root bundle), the grouped bundle, or the explicit
/// selection's policy files. Plain reads and checked runs refuse these.
pub(crate) fn in_shield_files(root: &Path, path: &Path) -> bool {
    use std::ffi::OsStr;
    let named = |part: Option<Component<'_>>, name: &str| {
        part.is_some_and(|part| crate::location::native_name(part.as_os_str(), OsStr::new(name)))
    };
    if let Ok(relative) = path.strip_prefix(root) {
        let mut parts = relative.components();
        let first = parts.next();
        if named(first, SHIELD) || (named(first, ".config") && named(parts.next(), "shield")) {
            return true;
        }
    }
    explicit_policy_files()
        .iter()
        .any(|file| path == file || path.starts_with(file))
}

/// The launch arguments agent setup writes so the project's hooks and MCP
/// server select the same configuration: `--config` relative to the project
/// for an explicit file inside it, none for a default selection.
///
/// # Errors
///
/// An explicit file outside the project cannot be named portably.
pub(crate) fn launch_args(root: &Path) -> io::Result<Vec<String>> {
    // The bound file, which `shield init` may not have created yet. Commands
    // that read policy refuse a missing file through [`select`]; naming it in
    // launch arguments does not need it to exist.
    let Some(file) = explicit_for(root) else {
        return Ok(Vec::new());
    };
    let file = prospective(&file)?;
    let root = canonical(root);
    let relative = file.strip_prefix(&root).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "agent setup can record only a --config file inside the project",
        )
    })?;
    let text = relative
        .components()
        .map(|part| match part {
            Component::Normal(name) => name.to_str(),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "--config path is not UTF-8"))?
        .join("/");
    Ok(vec!["--config".into(), text])
}

/// `path` resolved through its deepest existing ancestor, with the missing
/// remainder appended as plain names, so a file init has yet to create has
/// the same canonical form it will have once written.
///
/// # Errors
///
/// A missing remainder with `.` or `..` components cannot be resolved without
/// guessing, so it refuses.
fn prospective(path: &Path) -> io::Result<PathBuf> {
    let mut existing = path;
    let mut missing = Vec::new();
    while !present(existing) {
        let (Some(parent), Some(name)) = (existing.parent(), existing.file_name()) else {
            break;
        };
        missing.push(name);
        existing = parent;
    }
    if !missing.is_empty()
        && path
            .components()
            .rev()
            .take(missing.len())
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "a --config file that does not exist yet must be named without . or ..",
        ));
    }
    let mut resolved = existing.canonicalize()?;
    resolved.extend(missing.iter().rev());
    Ok(resolved)
}

#[cfg(test)]
mod tests;
