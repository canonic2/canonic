//! Bounded read-only Git inspection with fixed settings and no inherited Git environment.
use super::tree::Query;
#[cfg(not(windows))]
use super::tree::RebaseStep;
use std::path::Path;
#[cfg(not(windows))]
use std::time::{Duration, Instant};

pub(in crate::enforcement::protocol::native_gate::paths::git) fn worktree_root(
    base: &Path,
) -> Option<std::path::PathBuf> {
    #[cfg(not(windows))]
    return discover_root(base, Instant::now() + Duration::from_secs(2));
    #[cfg(windows)]
    {
        let _ = base;
        None
    }
}

#[cfg(not(windows))]
fn discover_root(base: &Path, deadline: Instant) -> Option<std::path::PathBuf> {
    crate::diagnostics::trace::count("git.worktree_root_queries", 1);
    let mut discover = command(base);
    discover.args(["rev-parse", "--path-format=absolute", "--show-toplevel"]);
    let text = stdout(discover, deadline)?;
    let text = text.strip_suffix('\n')?;
    if text.contains(['\n', '\r']) {
        return None;
    }
    let root = Path::new(text).canonicalize().ok()?;
    root.is_absolute().then_some(root)
}

pub(super) fn run(base: &Path, owner: &Path, root: &Path, query: &Query) -> Option<bool> {
    #[cfg(not(windows))]
    return answer(base, owner, root, query);
    #[cfg(windows)]
    {
        let _ = (base, owner, root, query);
        None
    }
}

#[cfg(not(windows))]
fn command(base: &Path) -> std::process::Command {
    use std::process::{Command, Stdio};
    let mut command = Command::new(crate::execution::GIT_EXECUTABLE);
    command
        .current_dir(base)
        .env_clear()
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("LC_ALL", "C")
        .args(crate::execution::GIT_SETTINGS)
        .stdin(Stdio::null())
        .stderr(Stdio::null());
    command
}

/// Drain output even after the capture bound, without retaining it. Every child
/// is reaped at the same deadline; no inherited hooks, filters or transports run.
#[cfg(not(windows))]
fn execute(
    command: std::process::Command,
    deadline: Instant,
) -> Option<(std::process::ExitStatus, Vec<u8>, u64)> {
    execute_with(command, None, deadline)
}

/// The complete text output of a command that succeeds.
#[cfg(not(windows))]
fn stdout(command: std::process::Command, deadline: Instant) -> Option<String> {
    let (status, bytes, total) = execute(command, deadline)?;
    if !status.success() || total != bytes.len() as u64 {
        return None;
    }
    String::from_utf8(bytes).ok()
}

/// Like [`execute`], writing `input` to the child's standard input.
#[cfg(not(windows))]
fn execute_with(
    mut command: std::process::Command,
    input: Option<Vec<u8>>,
    deadline: Instant,
) -> Option<(std::process::ExitStatus, Vec<u8>, u64)> {
    use std::{
        io::{Read, Write},
        process::Stdio,
    };
    if Instant::now() >= deadline {
        return None;
    }
    command.stdout(Stdio::piped());
    if input.is_some() {
        command.stdin(Stdio::piped());
    }
    let mut child = command.spawn().ok()?;
    crate::diagnostics::trace::count("git.inspection_processes", 1);
    // A child that exits early closes its input; the writer then fails and ends.
    let writer = input.zip(child.stdin.take()).map(|(bytes, mut stdin)| {
        std::thread::spawn(move || {
            let _ = stdin.write_all(&bytes);
        })
    });
    let mut output = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        output.by_ref().take(65_536).read_to_end(&mut bytes).ok()?;
        let discarded = std::io::copy(&mut output, &mut std::io::sink()).ok()?;
        let total = bytes.len() as u64 + discarded;
        Some((bytes, total))
    });
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(2));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    if let Some(writer) = writer {
        writer.join().ok()?;
    }
    let (bytes, total) = reader.join().ok()??;
    Some((status?, bytes, total))
}

#[cfg(not(windows))]
fn answer(base: &Path, owner: &Path, root: &Path, query: &Query) -> Option<bool> {
    let deadline = Instant::now() + Duration::from_secs(2);
    let paths = match query {
        Query::Diff { paths, .. }
        | Query::Staged { paths }
        | Query::Untracked { paths }
        | Query::Stash { paths, .. }
        | Query::Rebase { paths, .. } => paths,
    };
    // Protected configuration belongs to the worktree Git actually writes. Keep the
    // enrolled project's paths too when that repository contains them. An
    // explicit external policy selection stays unknown, never silently omitted.
    let mut selected = Vec::new();
    for path in paths {
        let bundle = crate::review::integrations::protected_files()
            .iter()
            .copied()
            .chain([crate::layout::SHIELD, crate::layout::GROUPED_BUNDLE])
            .find(|name| super::resolve_existing(&owner.join(name)) == *path);
        if path.starts_with(root) {
            selected.push(path.clone());
        }
        if let Some(bundle) = bundle {
            selected.push(root.join(bundle));
        } else if !path.starts_with(root) {
            return None;
        }
    }
    selected.sort();
    selected.dedup();
    if selected.is_empty() {
        return None;
    }
    match query {
        Query::Diff { revisions, .. } => diff(base, &[], revisions, &selected, deadline),
        Query::Staged { .. } => diff(base, &["--cached"], &["HEAD"], &selected, deadline),
        Query::Untracked { .. } => {
            let mut listing = command(base);
            listing
                .args(["ls-files", "--others", "--exclude-standard", "-z", "--"])
                .args(&selected);
            let (status, _, total) = execute(listing, deadline)?;
            status.success().then_some(total > 0)
        }
        Query::Stash { reference, .. } => stash(base, reference, &selected, deadline),
        Query::Rebase { step, .. } => rebase(base, *step, &selected, deadline),
    }
}

/// Whether applying a stash changes `paths`: its saved worktree and index
/// against its base, the working tree it merges into against that base (where
/// a rename can carry a stashed edit), and any untracked-file parent.
#[cfg(not(windows))]
fn stash(
    base: &Path,
    reference: &str,
    paths: &[std::path::PathBuf],
    deadline: Instant,
) -> Option<bool> {
    let mut parents = command(base);
    parents.args([
        "rev-list",
        "--parents",
        "-n",
        "1",
        "--end-of-options",
        reference,
    ]);
    let listed = stdout(parents, deadline)?;
    let ids: Vec<_> = listed.split_whitespace().collect();
    if !(ids.len() == 3 || ids.len() == 4) || !ids.iter().all(|id| object_id(id)) {
        return None;
    }
    // Compare both saved worktree and index with the stash's base;
    // --index cannot hide a policy edit absent from the saved worktree.
    // Applying merges the base, the working tree and the stash, so the
    // working tree must also match the base on the protected paths.
    let comparisons: [&[&str]; 3] = [&[ids[1], ids[0]], &[ids[1], ids[2]], &[ids[1]]];
    for revisions in comparisons {
        if diff(base, &[], revisions, paths, deadline)? {
            return Some(true);
        }
    }
    if let Some(untracked) = ids.get(3) {
        let mut listing = command(base);
        listing
            .args(["ls-tree", "-r", "--name-only", "-z", untracked, "--"])
            .args(paths);
        let (status, _, total) = execute(listing, deadline)?;
        if !status.success() {
            return None;
        }
        if total > 0 {
            return Some(true);
        }
    }
    Some(false)
}

/// Largest rebase state file read, and most todo commits checked: `HEAD` and
/// each commit and its parent, one 65-byte SHA-256 line each, stay within the
/// output bound.
#[cfg(not(windows))]
const REBASE_STATE_BYTES: u64 = 262_144;
#[cfg(not(windows))]
const REBASE_COMMITS: usize = 500;

/// Whether the next step of the rebase in progress changes the policy files.
/// Continuing applies each remaining todo commit as a merge of its parent,
/// `HEAD` and itself; aborting returns the working tree to the original tip.
/// Finishing or aborting also applies the autostash. A todo step other than
/// applying, dropping or stopping (exec, merge, label, reset, update-ref), the
/// apply backend's patch queue, or unreadable state is unknown.
#[cfg(not(windows))]
fn rebase(
    base: &Path,
    step: RebaseStep,
    paths: &[std::path::PathBuf],
    deadline: Instant,
) -> Option<bool> {
    let mut locate = command(base);
    locate.args([
        "rev-parse",
        "--path-format=absolute",
        "--git-path",
        "rebase-merge",
        "--git-path",
        "rebase-apply",
    ]);
    let text = stdout(locate, deadline)?;
    let [merge, apply] = text.lines().map(Path::new).collect::<Vec<_>>()[..] else {
        return None;
    };
    let state = match step {
        RebaseStep::Abort => [merge, apply].into_iter().find(|state| state.is_dir())?,
        RebaseStep::Remaining => merge,
    };
    let changed = match step {
        RebaseStep::Abort => {
            let tip = read_state(&state.join("orig-head"))?;
            let tip = tip.trim();
            if !object_id(tip) {
                return None;
            }
            diff(base, &[], &[tip], paths, deadline)?
        }
        RebaseStep::Remaining => {
            let todo = read_state(&state.join("git-rebase-todo"))?;
            let ids = todo_commits(&todo)?;
            !ids.is_empty() && touched(base, &ids, paths, deadline)?
        }
    };
    if changed {
        return Some(true);
    }
    // A missing autostash is the common case; any other state is unknown.
    let autostash = state.join("autostash");
    if std::fs::symlink_metadata(&autostash)
        .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound)
    {
        return Some(false);
    }
    let reference = read_state(&autostash)?;
    let reference = reference.trim();
    if !object_id(reference) {
        return None;
    }
    stash(base, reference, paths, deadline)
}

/// A bounded regular state file. Opening never blocks on a named pipe or
/// follows a final link, and the opened descriptor must be a regular file.
#[cfg(not(windows))]
fn read_state(path: &Path) -> Option<String> {
    use std::{io::Read, os::unix::fs::OpenOptionsExt};
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(nix::fcntl::OFlag::O_NOFOLLOW.bits() | nix::fcntl::OFlag::O_NONBLOCK.bits())
        .open(path)
        .ok()?;
    if !file.metadata().ok()?.is_file() {
        return None;
    }
    let mut text = String::new();
    file.take(REBASE_STATE_BYTES + 1)
        .read_to_string(&mut text)
        .ok()?;
    (text.len() as u64 <= REBASE_STATE_BYTES).then_some(text)
}

/// The commits a rebase todo list still applies; None for a step that does
/// something else or a line this reader does not know.
#[cfg(not(windows))]
fn todo_commits(todo: &str) -> Option<Vec<&str>> {
    let mut ids = Vec::new();
    for line in todo.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut words = line.split_whitespace();
        match words.next()? {
            "pick" | "p" | "reword" | "r" | "edit" | "e" | "squash" | "s" => {
                ids.push(words.next()?);
            }
            "fixup" | "f" => {
                let word = words.next()?;
                ids.push(if matches!(word, "-C" | "-c") {
                    words.next()?
                } else {
                    word
                });
            }
            "drop" | "d" | "break" | "b" | "noop" => {}
            _ => return None,
        }
    }
    let known = ids.len() <= REBASE_COMMITS
        && ids
            .iter()
            .all(|id| (4..=64).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_hexdigit()));
    known.then_some(ids)
}

#[cfg(not(windows))]
fn object_id(word: &str) -> bool {
    [40, 64].contains(&word.len()) && word.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Whether replaying any commit can change `paths`. A pick merges the commit's
/// parent, `HEAD` and the commit, so it can write a protected path the commit
/// changes, or one that differs between its parent and `HEAD`, where a rename
/// or directory rename can carry the commit's edits. One resolution of `HEAD`,
/// each commit and its first parent (a root commit is unknown), then one
/// `git diff-tree --stdin` over every commit and parent-to-`HEAD` pair.
#[cfg(not(windows))]
fn touched(
    base: &Path,
    ids: &[&str],
    paths: &[std::path::PathBuf],
    deadline: Instant,
) -> Option<bool> {
    let mut resolve = command(base);
    resolve.args(["rev-parse", "HEAD^{commit}"]).args(
        ids.iter()
            .flat_map(|id| [format!("{id}^{{commit}}"), format!("{id}^")]),
    );
    let resolved = stdout(resolve, deadline)?;
    let full: Vec<_> = resolved.lines().collect();
    if full.len() != 1 + 2 * ids.len() || !full.iter().all(|id| object_id(id)) {
        return None;
    }
    let head = full[0];
    let mut input = String::new();
    for pair in full[1..].chunks(2) {
        for part in [pair[0], "\n", pair[1], " ", head, "\n"] {
            input.push_str(part);
        }
    }
    let mut changes = command(base);
    changes
        .args([
            "diff-tree",
            "--stdin",
            "-r",
            "-m",
            "--no-commit-id",
            "--name-only",
            "--no-ext-diff",
            "--no-textconv",
            "--ignore-submodules=all",
            "--",
        ])
        .args(paths);
    let (status, _, total) = execute_with(changes, Some(input.into_bytes()), deadline)?;
    status.success().then_some(total > 0)
}

#[cfg(not(windows))]
fn diff(
    base: &Path,
    options: &[&str],
    revisions: &[impl AsRef<std::ffi::OsStr>],
    paths: &[std::path::PathBuf],
    deadline: Instant,
) -> Option<bool> {
    let mut query = command(base);
    query
        .args([
            "diff",
            "--quiet",
            "--no-ext-diff",
            "--no-textconv",
            "--ignore-submodules=all",
        ])
        .args(options)
        .arg("--end-of-options")
        .args(revisions)
        .arg("--")
        .args(paths);
    match execute(query, deadline)?.0.code() {
        Some(0) => Some(false),
        Some(1) => Some(true),
        _ => None,
    }
}
