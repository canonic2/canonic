//! Finite success/failure directory alternatives, separate from policy decisions.
use super::State;
use crate::policy::Error;
use std::path::{Component, Path, PathBuf};

const MAX_ALTERNATIVES: usize = 8;

#[cfg(all(test, unix))]
mod tests;

#[derive(Clone, PartialEq, Eq)]
pub(super) struct Directory {
    pub(super) physical: String,
    pub(super) logical: Option<String>,
    /// Only established shell bookkeeping, never ambient OLDPWD.
    pub(super) previous: Option<String>,
    pub(super) derived: bool,
}

#[derive(Clone, Default)]
pub(super) struct Set {
    pub(super) paths: Vec<Directory>,
    pub(super) unknown: bool,
}

impl Set {
    pub(super) fn join(&mut self, other: &Self) {
        self.unknown |= other.unknown;
        if self.unknown {
            self.paths.clear();
            return;
        }
        for path in &other.paths {
            let mut path = path.clone();
            let first = self
                .paths
                .iter()
                .position(|existing| existing.physical == path.physical);
            if path.logical.is_none()
                || self.paths.iter().any(|existing| {
                    existing.physical == path.physical && existing.logical.is_none()
                })
            {
                path.logical = None;
                path.previous = None;
                self.paths.retain(|existing| {
                    if existing.physical == path.physical {
                        path.derived |= existing.derived;
                        false
                    } else {
                        true
                    }
                });
                self.paths.insert(
                    first.unwrap_or(self.paths.len()).min(self.paths.len()),
                    path,
                );
            } else if let Some(existing) = self.paths.iter_mut().find(|existing| {
                existing.physical == path.physical
                    && existing.logical == path.logical
                    && existing.previous == path.previous
            }) {
                existing.derived |= path.derived;
            } else {
                self.paths.push(path);
            }
        }
        // Widen the finite context abstraction; ordinary operation/evidence
        // budgets still bound the source traversal and block on exhaustion.
        if self.paths.len() > MAX_ALTERNATIVES {
            self.paths.clear();
            self.unknown = true;
        }
    }
}

impl State {
    pub(super) fn retained_directory_bytes(&self) -> Result<usize, Error> {
        self.cwd_paths
            .iter()
            .chain(self.cwd_success.iter().flat_map(|set| &set.paths))
            .chain(self.cwd_failure.iter().flat_map(|set| &set.paths))
            .try_fold(
                3 * std::mem::size_of::<Vec<Directory>>(),
                |bytes, directory| {
                    bytes
                        .checked_add(std::mem::size_of::<Directory>())
                        .and_then(|bytes| bytes.checked_add(directory.physical.len()))
                        .and_then(|bytes| {
                            bytes.checked_add(directory.logical.as_ref().map_or(0, String::len))
                        })
                        .and_then(|bytes| {
                            bytes.checked_add(directory.previous.as_ref().map_or(0, String::len))
                        })
                        .ok_or(Error::Limit)
                },
            )
    }

    pub(super) fn directories(&self) -> Set {
        Set {
            paths: self.cwd_paths.clone(),
            unknown: self.cwd_unknown,
        }
    }

    pub(super) fn set_directories(&mut self, directories: Set) {
        self.cwd_unknown = directories.unknown;
        self.cwd_paths = directories.paths;
    }

    pub(super) fn command_outcomes(&mut self) {
        self.cwd_success = Some(self.directories());
        self.cwd_failure = Some(self.directories());
    }

    pub(super) fn include_failure(&mut self, before: &Set) {
        let mut failure = self
            .cwd_failure
            .clone()
            .unwrap_or_else(|| self.directories());
        failure.join(before);
        self.cwd_failure = Some(failure);
        let mut directories = self.directories();
        directories.join(before);
        self.set_directories(directories);
    }

    pub(super) fn outcome(&self, success: bool) -> Self {
        let mut state = self.clone();
        let directories = if success {
            &self.cwd_success
        } else {
            &self.cwd_failure
        };
        if let Some(directories) = directories {
            state.set_directories(directories.clone());
        }
        state.command_outcomes();
        state
    }

    pub(super) fn filesystem_mutation(&mut self) {
        self.filesystem_unknown = true;
        if self.cwd_paths.iter().any(|path| path.derived) {
            self.cwd_unknown = true;
            self.command_outcomes();
        }
    }
}

fn logical(path: &Path) -> Option<PathBuf> {
    let mut result = PathBuf::new();
    for part in path.components() {
        match part {
            Component::ParentDir => {
                result.pop();
            }
            Component::CurDir => {}
            _ => result.push(part.as_os_str()),
        }
    }
    result.is_absolute().then_some(result)
}

pub(super) fn resolve(
    words: &[String],
    state: &State,
    cdpath_empty: bool,
    observer: &crate::location::directory::Directories,
    checkpoint: &dyn Fn() -> Result<(), Error>,
) -> Result<Option<Directory>, Error> {
    if state.cwd_unknown
        || state.filesystem_unknown
        || cfg!(windows)
        || words.first().is_none_or(|name| name != "cd")
    {
        return Ok(None);
    }
    let [current] = state.cwd_paths.as_slice() else {
        return Ok(None);
    };
    let mut arguments = &words[1..];
    let mut physical = false;
    let mut explicit = false;
    if let Some(mode) = arguments
        .first()
        .filter(|mode| matches!(mode.as_str(), "-L" | "-P"))
    {
        physical = mode == "-P";
        explicit = true;
        arguments = &arguments[1..];
    }
    if arguments.first().is_some_and(|argument| argument == "--") {
        arguments = &arguments[1..];
    }
    let [submitted] = arguments else {
        return Ok(None);
    };
    let target = if submitted == "-" {
        if state.environment_unknown {
            return Ok(None);
        }
        let Some(previous) = current.previous.as_ref() else {
            return Ok(None);
        };
        previous
    } else {
        submitted
    };
    if target.is_empty() || target.starts_with('-') || (!explicit && state.environment_unknown) {
        return Ok(None);
    }
    let path = Path::new(target);
    if !physical && target.starts_with("//") {
        return Ok(None);
    }
    let first = path.components().next();
    if !path.is_absolute()
        && !matches!(first, Some(Component::CurDir | Component::ParentDir))
        && (!cdpath_empty || state.environment_unknown)
    {
        return Ok(None);
    }
    let parent = path.components().any(|part| part == Component::ParentDir);
    // The host may have inherited `set -P`; reported variables do not prove
    // its shell option state. Parent traversal needs an explicit mode.
    if parent && !explicit {
        return Ok(None);
    }
    let current_logical = (!state.environment_unknown)
        .then_some(current.logical.as_ref())
        .flatten();
    let candidate = if physical {
        Path::new(&current.physical).join(path)
    } else if path.is_absolute() {
        let Some(path) = logical(path) else {
            return Ok(None);
        };
        path
    } else if let Some(base) = current_logical {
        let Some(path) = logical(&Path::new(base).join(path)) else {
            return Ok(None);
        };
        path
    } else if parent {
        return Ok(None);
    } else {
        Path::new(&current.physical).join(path)
    };
    let Some(destination) = observer.observe(&candidate, checkpoint)? else {
        return Ok(None);
    };
    let logical = if physical {
        Some(destination.clone())
    } else if (path.is_absolute() || current_logical.is_some())
        && (explicit || candidate == Path::new(&destination))
    {
        candidate.to_str().map(str::to_owned)
    } else {
        None
    };
    Ok(Some(Directory {
        physical: destination,
        logical,
        previous: current_logical.cloned(),
        derived: true,
    }))
}

pub(super) fn change(state: &mut State, target: Option<Directory>) {
    let before = state.directories();
    let success = target.map_or_else(
        || Set {
            paths: vec![],
            unknown: true,
        },
        |path| Set {
            paths: vec![path],
            unknown: false,
        },
    );
    let mut failure = before.clone();
    for directory in &mut failure.paths {
        directory.previous = None;
    }
    let mut changed_failure = success.clone();
    for directory in &mut changed_failure.paths {
        // chdir may have succeeded before updating readonly PWD/OLDPWD failed.
        directory.logical = None;
        directory.previous = None;
    }
    failure.join(&changed_failure);
    state.cwd_success = Some(success.clone());
    state.cwd_failure = Some(failure.clone());
    let mut after = before;
    after.join(&success);
    after.join(&failure);
    state.set_directories(after);
}

pub(super) fn unchanged(state: &mut State) {
    // Even a physical no-op may reset logical PWD under inherited -P. Keep
    // physical context, but do not guess which logical spelling survived.
    let mut success = state.directories();
    for directory in &mut success.paths {
        directory.previous = (!state.environment_unknown)
            .then(|| directory.logical.clone())
            .flatten();
        directory.logical = None;
    }
    let mut failure = success.clone();
    for directory in &mut failure.paths {
        directory.previous = None;
    }
    state.set_directories(failure.clone());
    state.cwd_success = Some(success);
    state.cwd_failure = Some(failure);
}
