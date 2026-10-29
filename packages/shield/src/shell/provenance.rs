//! Typed runtime origins; never manufacture literal argv from symbolic values.
use super::{State, structure::Span, words::identifier};

#[derive(Clone, PartialEq, Eq)]
pub(super) enum Binding {
    Literal(String),
    BackgroundJob(Job),
}

#[derive(Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub(super) struct Job {
    pub(super) source_unit: usize,
    pub(super) span: Span,
    pub(super) launch: usize,
}

impl Binding {
    pub(super) fn literal(&self) -> Option<&str> {
        match self {
            Self::Literal(value) => Some(value),
            Self::BackgroundJob(_) => None,
        }
    }

    pub(super) fn len(&self) -> usize {
        self.literal().map_or(64, str::len)
    }
}

impl From<String> for Binding {
    fn from(value: String) -> Self {
        Self::Literal(value)
    }
}

impl From<&str> for Binding {
    fn from(value: &str) -> Self {
        Self::Literal(value.into())
    }
}

/// Only an entire parameter reference preserves a process origin. Concatenation,
/// arithmetic, defaults, indirect expansion and command substitution do not.
pub(super) fn reference(raw: &str, state: &State) -> Option<Job> {
    let value = raw
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or(raw);
    let name = value
        .strip_prefix("${")
        .and_then(|s| s.strip_suffix('}'))
        .or_else(|| value.strip_prefix('$'))?;
    if name != "!" && !identifier(name) {
        return None;
    }
    match state.variables.get(name)? {
        Binding::BackgroundJob(job) => Some(*job),
        Binding::Literal(_) => None,
    }
}

/// Recognize only whole positive PID references, never a process group or a
/// name-based selection. Unquoted expansion must retain one numeric field.
pub(super) fn kill_targets(words: &[super::structure::Word], state: &State) -> Option<Vec<Job>> {
    let executable = words.first()?.literal.as_deref()?;
    if !matches!(executable, "kill" | "/bin/kill" | "/usr/bin/kill") {
        return None;
    }
    let mut at = 1;
    if let Some(option) = words.get(at)?.literal.as_deref() {
        if matches!(option, "-s" | "--signal") {
            let signal = words.get(at + 1)?.literal.as_deref()?;
            if !signal_name(signal) {
                return None;
            }
            at += 2;
        } else if let Some(signal) = option.strip_prefix('-').filter(|s| !s.is_empty())
            && option != "--"
        {
            if !signal_name(signal) {
                return None;
            }
            at += 1;
        }
    }
    if words.get(at).and_then(|word| word.literal.as_deref()) == Some("--") {
        at += 1;
    }
    let operands = words
        .get(at..)?
        .iter()
        .map(|word| {
            if word.literal.is_some() {
                return None;
            }
            if !word.raw.starts_with('"') {
                let ifs = state.variables.get("IFS")?.literal()?;
                if ifs.chars().any(|ch| ch.is_ascii_digit()) {
                    return None;
                }
            }
            reference(&word.raw, state)
        })
        .collect::<Option<Vec<_>>>()?;
    (!operands.is_empty()).then_some(operands)
}

fn signal_name(signal: &str) -> bool {
    matches!(
        signal,
        "TERM"
            | "SIGTERM"
            | "15"
            | "INT"
            | "SIGINT"
            | "2"
            | "KILL"
            | "SIGKILL"
            | "9"
            | "HUP"
            | "SIGHUP"
            | "1"
            | "0"
    )
}
