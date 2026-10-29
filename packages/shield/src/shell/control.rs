//! Scoped abrupt exits preserve successor state without executing shell code.

use super::{Error, State};

pub(super) fn special_builtin(name: &str) -> bool {
    matches!(
        name,
        ":" | "."
            | "break"
            | "continue"
            | "eval"
            | "exec"
            | "exit"
            | "export"
            | "readonly"
            | "return"
            | "set"
            | "shift"
            | "trap"
            | "unset"
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    Function,
    Loop,
    Process,
}

pub(super) struct Frame {
    kind: Kind,
    exits: Vec<(bool, State)>,
}

impl Frame {
    pub(super) fn new(kind: Kind) -> Self {
        Self {
            kind,
            exits: Vec::new(),
        }
    }

    pub(super) fn merge(self, state: &mut State) {
        for (_, mut exit) in self.exits {
            exit.stopped = false;
            state.join(&exit);
        }
    }

    /// Break endpoints bypass later iterations; continue endpoints feed them.
    pub(super) fn iteration(self, state: &mut State) -> Vec<State> {
        let mut breaks = Vec::new();
        for (is_break, mut exit) in self.exits {
            exit.stopped = false;
            if is_break {
                breaks.push(exit);
            } else {
                state.join(&exit);
            }
        }
        breaks
    }

    pub(super) fn function(self, state: &mut State) {
        super::bindings::restore_locals(state);
        for (_, mut exit) in self.exits {
            super::bindings::restore_locals(&mut exit);
            exit.stopped = false;
            state.join(&exit);
        }
    }
}

/// Literal loop transfers are structural when their target stays in this scope.
pub(super) fn resolved_loop_transfer(words: &[String], frames: &[Frame]) -> bool {
    if !matches!(words[0].as_str(), "break" | "continue") {
        return false;
    }
    let level = match &words[1..] {
        [] => 1,
        [value] => match value.parse::<usize>() {
            Ok(level) if level > 0 => level,
            _ => return false,
        },
        _ => return false,
    };
    let targets: Vec<_> = frames
        .iter()
        .enumerate()
        .rev()
        .take_while(|(_, frame)| frame.kind != Kind::Process)
        .filter(|(_, frame)| frame.kind == Kind::Loop)
        .map(|(index, _)| index)
        .collect();
    targets
        .get(level.min(targets.len()).saturating_sub(1))
        .is_some_and(|target| {
            frames[target + 1..]
                .iter()
                .all(|frame| frame.kind != Kind::Function)
        })
}

pub(super) fn transfer(
    words: &[String],
    frames: &mut [Frame],
    state: &mut State,
    suppressed: bool,
    bytes: &mut usize,
) -> Result<bool, Error> {
    let kind = match words[0].as_str() {
        "return" => Kind::Function,
        "break" | "continue" => Kind::Loop,
        "exit" => Kind::Process,
        _ => return Ok(false),
    };
    let level = match &words[1..] {
        [] => Some(1),
        [value] if kind != Kind::Loop && value.parse::<i64>().is_ok() => Some(1),
        [value] if kind == Kind::Loop => value.parse::<usize>().ok().filter(|n| *n > 0),
        _ => None,
    };
    // Bash bounds an oversized break/continue count by the enclosing loops.
    let targets: Vec<_> = frames
        .iter()
        .enumerate()
        .rev()
        .take_while(|(_, frame)| kind == Kind::Process || frame.kind != Kind::Process)
        .filter(|(_, frame)| frame.kind == kind)
        .map(|(index, _)| index)
        .collect();
    let Some(level) = level else {
        // Retain both invalid/continuing and possible exit alternatives.
        state.variables.clear();
        state.positionals = None;
        state.environment_unknown = true;
        if !suppressed {
            for target in targets {
                capture(&mut frames[target], state, bytes, words[0] == "break")?;
                if kind != Kind::Loop {
                    break;
                }
            }
        }
        return Ok(true);
    };
    let Some(target) = targets
        .get(level.min(targets.len()).saturating_sub(1))
        .copied()
    else {
        if kind == Kind::Process {
            state.stopped = true;
        }
        return Ok(true);
    };
    if kind == Kind::Loop
        && frames[target + 1..]
            .iter()
            .any(|frame| frame.kind == Kind::Function)
    {
        // Cross-function loop transfers differ by shell/version and Bash compatibility mode.
        // Keep both the continuing call and a possible caller-loop exit.
        state.variables.clear();
        state.positionals = None;
        state.environment_unknown = true;
        if !suppressed {
            capture(&mut frames[target], state, bytes, words[0] == "break")?;
        }
        return Ok(true);
    }
    if !suppressed {
        capture(&mut frames[target], state, bytes, words[0] == "break")?;
    }
    state.stopped = true;
    Ok(true)
}

fn capture(
    frame: &mut Frame,
    state: &State,
    bytes: &mut usize,
    is_break: bool,
) -> Result<(), Error> {
    let size = state
        .variables
        .iter()
        .map(|(k, v)| k.len() + v.len())
        .sum::<usize>()
        + state
            .positionals
            .iter()
            .flatten()
            .flatten()
            .map(String::len)
            .sum::<usize>()
        + state
            .functions
            .iter()
            .map(|(name, bindings)| name.len() + bindings.len() * 64)
            .sum::<usize>()
        + state
            .locals
            .iter()
            .flat_map(|frame| frame.iter())
            .map(|(name, saved)| name.len() + super::bindings::retained_bytes(saved))
            .sum::<usize>()
        + state.exported.iter().map(String::len).sum::<usize>()
        + state.arg0.as_ref().map_or(0, String::len)
        + 128;
    *bytes = bytes.checked_add(size).ok_or(Error::Limit)?;
    if *bytes > super::budget::MAX_EVIDENCE_BYTES {
        return Err(Error::Limit);
    }
    frame.exits.push((is_break, state.clone()));
    Ok(())
}
