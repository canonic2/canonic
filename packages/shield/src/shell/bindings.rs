//! Known environment bindings at explicit process boundaries; no ambient reads.

use super::{Dialect, Error, State, assignment, structure::Word, wrapper_offset};

#[derive(Clone, PartialEq, Eq)]
pub(super) struct Saved {
    value: Option<super::provenance::Binding>,
    exported: bool,
}

pub(super) fn retained_bytes(saved: &Saved) -> usize {
    saved
        .value
        .as_ref()
        .map_or(0, super::provenance::Binding::len)
        + 32
}

pub(super) fn unknown_builtin(name: &str) -> bool {
    matches!(
        name,
        "typeset"
            | "let"
            | "getopts"
            | "mapfile"
            | "readarray"
            | "enable"
            | "shopt"
            | "hash"
            | "bind"
            | "ulimit"
            | "umask"
            | "dirs"
            | "history"
            | "fc"
            | "logout"
            | "compgen"
            | "complete"
            | "compopt"
            | "fg"
            | "bg"
            | "disown"
            | "suspend"
    )
}

pub(super) fn dynamic_variable(name: &str) -> bool {
    matches!(
        name,
        "RANDOM"
            | "SRANDOM"
            | "SECONDS"
            | "EPOCHSECONDS"
            | "EPOCHREALTIME"
            | "LINENO"
            | "BASHPID"
            | "BASH_SUBSHELL"
            | "BASH_COMMAND"
            | "BASH_MONOSECONDS"
            | "UID"
            | "EUID"
            | "PPID"
            | "GROUPS"
            | "BASH_ALIASES"
            | "BASH_CMDS"
            | "BASH_REMATCH"
            | "BASH_ARGC"
            | "BASH_ARGV"
            | "FUNCNAME"
            | "BASH_LINENO"
            | "BASH_SOURCE"
            | "PIPESTATUS"
            | "_"
            | "SHLVL"
            | "BASH_VERSION"
            | "BASH_VERSINFO"
            | "BASHOPTS"
            | "SHELLOPTS"
    )
}

/// Whether binding `name` may change a variable that launched commands inherit.
///
/// Names the host reported as environment are inherited. Because a report may
/// be partial, any name with an uppercase letter may also be exported, and the
/// lowercase proxy names and zsh's array ties to PATH-like settings are read by
/// clients and shells as well.
pub(super) fn inherited_name(
    name: &str,
    reported: Option<&std::collections::BTreeSet<String>>,
) -> bool {
    reported.is_some_and(|names| names.contains(name))
        || name.bytes().any(|byte| byte.is_ascii_uppercase())
        || matches!(
            name,
            "http_proxy"
                | "https_proxy"
                | "all_proxy"
                | "no_proxy"
                | "ftp_proxy"
                | "path"
                | "cdpath"
                | "fpath"
                | "manpath"
                | "module_path"
        )
}

pub(super) fn loop_value(name: &str, value: Option<String>, state: &mut State) {
    if let Some(value) = value.filter(|_| !dynamic_variable(name)) {
        state.variables.insert(name.into(), value.into());
    } else {
        state.variables.remove(name);
    }
}

pub(super) fn versioned_interpreter(name: &str) -> bool {
    ["python", "pypy", "lua", "tclsh", "ruby", "perl"]
        .iter()
        .any(|prefix| {
            name.strip_prefix(prefix).is_some_and(|suffix| {
                let suffix = suffix.strip_suffix('t').unwrap_or(suffix);
                suffix.as_bytes().first().is_some_and(u8::is_ascii_digit)
                    && suffix.split('.').all(|part| {
                        !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit())
                    })
            })
        })
}

pub(super) fn printf_mutates(words: &[String]) -> bool {
    words.first().is_some_and(|name| name == "printf")
        && (words.get(1).is_some_and(|arg| arg.starts_with("-v"))
            || words[1..].iter().any(|arg| format_assignment(arg)))
}

fn format_assignment(format: &str) -> bool {
    let mut chars = format.chars();
    while let Some(ch) = chars.next() {
        if ch != '%' {
            continue;
        }
        let Some(mut conversion) = chars.next() else {
            break;
        };
        if conversion == '%' {
            continue;
        }
        while conversion.is_ascii_digit() || "#0- +'.*hlLjzt".contains(conversion) {
            let Some(next) = chars.next() else {
                return false;
            };
            conversion = next;
        }
        if conversion == 'n' {
            return true;
        }
    }
    false
}

pub(super) fn local_declaration(words: &[String], state: &mut State) -> bool {
    if words.first().is_none_or(|word| word != "local")
        || !words[1..].iter().all(|word| assignment(word))
    {
        return false;
    }
    let Some(frame) = state.locals.last_mut() else {
        return false;
    };
    for word in &words[1..] {
        if let Some((name, value)) = word.split_once('=') {
            frame.entry(name.into()).or_insert_with(|| Saved {
                value: state.variables.get(name).cloned(),
                exported: state.exported.contains(name),
            });
            if dynamic_variable(name) {
                state.variables.remove(name);
            } else {
                state.variables.insert(name.into(), value.into());
            }
        }
    }
    state.environment_unknown = true;
    true
}

pub(super) fn restore_locals(state: &mut State) {
    for (name, saved) in state.locals.pop().unwrap_or_default() {
        if let Some(value) = saved.value {
            state.variables.insert(name.clone(), value);
        } else {
            state.variables.remove(&name);
        }
        if saved.exported {
            state.exported.insert(name);
        } else {
            state.exported.remove(&name);
        }
    }
}

pub(super) fn restore_prefix(words: &[Word], original: &State, launch: &mut State) {
    for word in words {
        if let Some((name, _)) = word.raw.split_once('=') {
            if let Some(value) = original.variables.get(name) {
                launch.variables.insert(name.into(), value.clone());
            } else {
                launch.variables.remove(name);
            }
            if original.exported.contains(name) {
                launch.exported.insert(name.into());
            } else {
                launch.exported.remove(name);
            }
        }
    }
}

pub(super) fn assignments(
    words: &[Word],
    state: &mut State,
    dialect: Dialect,
    export: bool,
) -> Result<usize, Error> {
    let mut count = 0;
    for word in words.iter().take_while(|word| assignment(&word.raw)) {
        count += 1;
        if let Some((name, raw)) = word.raw.split_once('=')
            && !dynamic_variable(name)
            && let Some(job) = super::provenance::reference(raw, state)
        {
            state
                .variables
                .insert(name.into(), super::provenance::Binding::BackgroundJob(job));
            if export {
                state.exported.insert(name.into());
            }
            continue;
        }
        if let Some((name, value)) = super::expansion::assignment_word(word, state, dialect)?
            .as_deref()
            .and_then(|value| value.split_once('='))
        {
            if dynamic_variable(name) {
                state.variables.remove(name);
            } else {
                state.variables.insert(name.into(), value.into());
            }
            if export {
                state.exported.insert(name.into());
            }
        } else if let Some((name, _)) = word.raw.split_once('=') {
            state.variables.remove(name);
        }
    }
    Ok(count)
}

pub(super) fn stdin_environment(
    mut words: &[Word],
    state: &mut State,
    dialect: Dialect,
) -> Result<(), Error> {
    for _ in 0..32 {
        let prefix = assignments(words, state, dialect, true)?;
        words = &words[prefix..];
        let Some(wrapper) = wrapper_offset(words) else {
            break;
        };
        wrapper_environment(&words[..wrapper.offset], state);
        words = &words[wrapper.offset..];
    }
    state
        .variables
        .retain(|name, _| state.exported.contains(name) && !dynamic_variable(name));
    state.brace_expansion = None;
    Ok(())
}

pub(super) fn wrapper_environment(words: &[Word], state: &mut State) {
    let Some(name) = words.first().and_then(|word| word.literal.as_deref()) else {
        return;
    };
    let name = match name {
        "/usr/bin/env" => "env",
        "/usr/bin/sudo" => "sudo",
        value => value,
    };
    if name == "sudo" {
        // Sudo's policy determines which environment values are retained with -E.
        state.variables.clear();
        state.exported.clear();
        return;
    }
    let mut at = 1;
    while at < words.len() {
        let Some(value) = words[at].literal.as_deref() else {
            state.variables.clear();
            state.exported.clear();
            return;
        };
        if (name == "env" && matches!(value, "-i" | "--ignore-environment"))
            || (name == "exec" && value == "-c")
        {
            state.variables.clear();
            state.exported.clear();
        } else if name == "env" && matches!(value, "-u" | "--unset") {
            at += 1;
            if let Some(name) = words.get(at).and_then(|word| word.literal.as_deref()) {
                state.variables.remove(name);
                state.exported.remove(name);
            }
        } else if (name == "env" && matches!(value, "-C" | "--chdir"))
            || (name == "exec" && value == "-a")
        {
            at += 1;
        } else if name == "env" {
            if let Some(name) = value.strip_prefix("--unset=") {
                state.variables.remove(name);
                state.exported.remove(name);
            } else if assignment(value)
                && let Some((name, value)) = value.split_once('=')
            {
                state.variables.insert(name.into(), value.into());
                state.exported.insert(name.into());
            }
        }
        at += 1;
    }
}
