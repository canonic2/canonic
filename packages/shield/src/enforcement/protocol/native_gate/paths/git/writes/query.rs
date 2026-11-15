//! Bounded read-only Git inspection with fixed settings and no inherited Git environment.
use super::tree::Query;
use std::path::Path;
#[cfg(not(windows))]
use std::time::{Duration, Instant};

pub(super) fn worktree_root(base: &Path) -> Option<std::path::PathBuf> {
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
    let (status, bytes, total) = execute(discover, deadline)?;
    if !status.success() || total != bytes.len() as u64 {
        return None;
    }
    let text = std::str::from_utf8(&bytes).ok()?.strip_suffix('\n')?;
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
    mut command: std::process::Command,
    deadline: Instant,
) -> Option<(std::process::ExitStatus, Vec<u8>, u64)> {
    use std::{io::Read, process::Stdio};
    if Instant::now() >= deadline {
        return None;
    }
    command.stdout(Stdio::piped());
    let mut child = command.spawn().ok()?;
    crate::diagnostics::trace::count("git.inspection_processes", 1);
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
    let (bytes, total) = reader.join().ok()??;
    Some((status?, bytes, total))
}

#[cfg(not(windows))]
fn answer(base: &Path, owner: &Path, root: &Path, query: &Query) -> Option<bool> {
    let deadline = Instant::now() + Duration::from_secs(2);
    let paths = match query {
        Query::Diff { paths, .. } | Query::Untracked { paths } | Query::Stash { paths, .. } => {
            paths
        }
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
        Query::Diff { revisions, .. } => diff(base, revisions, &selected, deadline),
        Query::Untracked { .. } => {
            let mut listing = command(base);
            listing
                .args(["ls-files", "--others", "--exclude-standard", "-z", "--"])
                .args(&selected);
            let (status, _, total) = execute(listing, deadline)?;
            status.success().then_some(total > 0)
        }
        Query::Stash { reference, .. } => {
            let mut parents = command(base);
            parents.args([
                "rev-list",
                "--parents",
                "-n",
                "1",
                "--end-of-options",
                reference,
            ]);
            let (status, bytes, total) = execute(parents, deadline)?;
            if !status.success() || total != bytes.len() as u64 {
                return None;
            }
            let ids: Vec<_> = std::str::from_utf8(&bytes)
                .ok()?
                .split_whitespace()
                .collect();
            if !(ids.len() == 3 || ids.len() == 4)
                || ids.iter().any(|id| {
                    ![40, 64].contains(&id.len()) || !id.bytes().all(|b| b.is_ascii_hexdigit())
                })
            {
                return None;
            }
            // Compare both saved worktree and index with the stash's base;
            // --index cannot hide a policy edit absent from the saved worktree.
            for saved in [ids[0], ids[2]] {
                if diff(
                    base,
                    &[ids[1].to_owned(), saved.to_owned()],
                    &selected,
                    deadline,
                )? {
                    return Some(true);
                }
            }
            if let Some(untracked) = ids.get(3) {
                let mut listing = command(base);
                listing
                    .args(["ls-tree", "-r", "--name-only", "-z", untracked, "--"])
                    .args(&selected);
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
    }
}

#[cfg(not(windows))]
fn diff(
    base: &Path,
    revisions: &[String],
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
            "--end-of-options",
        ])
        .args(revisions)
        .arg("--")
        .args(paths);
    match execute(query, deadline)?.0.code() {
        Some(0) => Some(false),
        Some(1) => Some(true),
        _ => None,
    }
}
