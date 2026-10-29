//! Explicit wrapper option boundaries; unknown arity never implies a child command.

use super::{Dialect, Wrapper, assignment, structure};

/// Locate a command string without mistaking option values for source.
/// Only options whose parsing effects are represented here are accepted.
pub(super) fn shell_command(words: &[String]) -> Option<(usize, Dialect)> {
    let bash = matches!(words.first()?.as_str(), "bash" | "/bin/bash");
    let zsh = matches!(words[0].as_str(), "zsh" | "/bin/zsh");
    if !bash && !zsh && !matches!(words[0].as_str(), "sh" | "/bin/sh" | "dash" | "/bin/dash") {
        return None;
    }
    if zsh {
        return zsh_command(words);
    }
    let mut posix = !bash;
    let mut at = 1;
    // Bash long invocation options must precede short options.
    if bash {
        while let Some(option) = words.get(at) {
            match option.as_str() {
                "--norc" | "--noprofile" | "--login" => at += 1,
                "--posix" => {
                    posix = true;
                    at += 1;
                }
                "--rcfile" | "--init-file" => {
                    words.get(at + 1)?;
                    at += 2;
                }
                _ => break,
            }
        }
    }
    let mut command = false;
    while let Some(option) = words.get(at) {
        if option == "--" {
            at += 1;
            break;
        }
        if !option.starts_with(['-', '+']) || option == "-" || option == "+" {
            break;
        }
        if matches!(option.as_str(), "-o" | "+o") {
            match words.get(at + 1)?.as_str() {
                "errexit" | "nounset" | "xtrace" | "verbose" | "noclobber" => {}
                "pipefail" if bash => {}
                "posix" if bash => posix = option == "-o",
                _ => return None,
            }
            at += 2;
            continue;
        }
        for flag in option[1..].chars() {
            match flag {
                'c' if option.starts_with('-') => command = true,
                'e' | 'u' | 'v' | 'x' | 'C' => {}
                'l' if bash && option.starts_with('-') => {}
                _ => return None,
            }
        }
        at += 1;
    }
    (command && words.get(at).is_some())
        .then_some((at, if posix { Dialect::Posix } else { Dialect::Bash }))
}

/// zsh has no parser of its own here. A `-c` script is read as POSIX only
/// when it avoids the syntax whose zsh meaning differs; any other script
/// stays unread. Only short options that do not change parsing are accepted.
fn zsh_command(words: &[String]) -> Option<(usize, Dialect)> {
    let mut at = 1;
    let mut command = false;
    while let Some(option) = words.get(at) {
        if option == "--" {
            at += 1;
            break;
        }
        if !option.starts_with('-') || option == "-" {
            break;
        }
        for flag in option[1..].chars() {
            match flag {
                'c' => command = true,
                'e' | 'u' | 'v' | 'x' | 'l' | 'f' => {}
                _ => return None,
            }
        }
        at += 1;
    }
    let source = words.get(at)?;
    (command && posix_compatible_zsh(source)).then_some((at, Dialect::Posix))
}

/// Excludes parameter, command, arithmetic and history expansion, globbing
/// and glob qualifiers, brace and tilde expansion, `=command` expansion and
/// zsh-only redirections and pipes, whether quoted or not.
fn posix_compatible_zsh(source: &str) -> bool {
    let bytes = source.as_bytes();
    let special = |byte: u8| b"$`*?[]{}()~^#!%".contains(&byte);
    let word_start = |at: usize| at == 0 || b" \t\n;|&<>".contains(&bytes[at - 1]);
    !bytes.iter().enumerate().any(|(at, &byte)| {
        special(byte)
            || (byte == b'=' && word_start(at))
            || (b"<>".contains(&byte)
                && bytes.get(at + 1) == Some(&b'&')
                && !bytes
                    .get(at + 2)
                    .is_some_and(|next| next.is_ascii_digit() || *next == b'-'))
    }) && !["|&", "&>", "&|", "<<<"]
        .iter()
        .any(|syntax| source.contains(syntax))
        && !zsh_only_command(source)
}

/// Whether a word at command position is a zsh-only reserved word or
/// precommand modifier, or a `for`/`select` loop binds a name zsh ties to a
/// search path. Words are split at blanks and separators without regard to
/// quoting, so a quoted separator can only refuse more scripts.
fn zsh_only_command(source: &str) -> bool {
    let mut position = true;
    let mut binding = false;
    let mut word = String::new();
    for byte in source.bytes().chain(*b"\n") {
        // Groups and substitutions also start commands; scripts using them are
        // refused already, so splitting there only keeps this check local.
        let separator = b";|&\n{}()`".contains(&byte);
        if !(separator || byte == b' ' || byte == b'\t') {
            if !b"'\"\\".contains(&byte) {
                word.push(char::from(byte));
            }
            continue;
        }
        if !word.is_empty() {
            if binding
                && matches!(
                    word.as_str(),
                    "path" | "cdpath" | "fpath" | "manpath" | "module_path"
                )
            {
                return true;
            }
            binding = position && matches!(word.as_str(), "for" | "select");
            if position
                && matches!(
                    word.as_str(),
                    "repeat"
                        | "nocorrect"
                        | "noglob"
                        | "-"
                        | "foreach"
                        | "coproc"
                        | "float"
                        | "integer"
                )
            {
                return true;
            }
            position = position
                && (assignment(&word)
                    || matches!(
                        word.as_str(),
                        "if" | "then"
                            | "else"
                            | "elif"
                            | "do"
                            | "while"
                            | "until"
                            | "!"
                            | "time"
                            | "command"
                            | "builtin"
                            | "exec"
                    ));
            word.clear();
        }
        if separator {
            position = true;
            binding = false;
        }
    }
    false
}

pub(super) fn offset(words: &[structure::Word]) -> Option<Wrapper> {
    let name = words.first()?.literal.as_deref()?;
    let mut offset = 1;
    let mut cwd_unknown = false;
    match name {
        "nice" | "/usr/bin/nice" => offset = nice_offset(words)?,
        "nohup" | "/usr/bin/nohup" => offset = end_options(words, offset)?,
        "timeout" | "/usr/bin/timeout" | "gtimeout" => offset = timeout_offset(words)?,
        "builtin" => offset = builtin_offset(words, offset)?,
        "command" => {
            while words.get(offset)?.literal.as_deref() == Some("-p") {
                offset += 1;
            }
            offset = end_options(words, offset)?;
        }
        "exec" => {
            while let Some(value) = words.get(offset)?.literal.as_deref() {
                match value {
                    "-c" | "-l" => offset += 1,
                    "-a" => {
                        words.get(offset + 1)?.literal.as_ref()?;
                        offset += 2;
                    }
                    _ => break,
                }
            }
            offset = end_options(words, offset)?;
        }
        "sudo" | "/usr/bin/sudo" => {
            while let Some(value) = words.get(offset)?.literal.as_deref() {
                if matches!(
                    value,
                    "-u" | "--user"
                        | "-g"
                        | "--group"
                        | "-D"
                        | "--chdir"
                        | "-R"
                        | "--chroot"
                        | "-p"
                        | "--prompt"
                        | "-C"
                        | "--close-from"
                        | "-T"
                        | "--command-timeout"
                ) {
                    words.get(offset + 1)?.literal.as_ref()?;
                    cwd_unknown |= matches!(value, "-D" | "--chdir" | "-R" | "--chroot");
                    offset += 2;
                } else if matches!(
                    value,
                    "-n" | "--non-interactive"
                        | "-E"
                        | "--preserve-env"
                        | "-H"
                        | "--set-home"
                        | "-b"
                        | "--background"
                ) || assignment(value)
                {
                    offset += 1;
                } else {
                    break;
                }
            }
            offset = end_options(words, offset)?;
        }
        "env" | "/usr/bin/env" => {
            while let Some(value) = words.get(offset)?.literal.as_deref() {
                if value == "-i" || value == "--ignore-environment" || assignment(value) {
                    offset += 1;
                } else if matches!(value, "-u" | "--unset" | "-C" | "--chdir") {
                    words.get(offset + 1)?.literal.as_ref()?;
                    cwd_unknown |= matches!(value, "-C" | "--chdir");
                    offset += 2;
                } else if value.starts_with("--unset=") || value.starts_with("--chdir=") {
                    cwd_unknown |= value.starts_with("--chdir=");
                    offset += 1;
                } else {
                    break;
                }
            }
            offset = end_options(words, offset)?;
        }
        _ => return None,
    }
    let executable = words.get(offset)?.literal.as_deref()?;
    (!executable.starts_with('-')).then_some(Wrapper {
        offset,
        cwd_unknown,
    })
}

fn builtin_offset(words: &[structure::Word], mut offset: usize) -> Option<usize> {
    if words.get(offset)?.literal.as_deref() == Some("--") {
        offset += 1;
    }
    if !super::bindings::unknown_builtin(words.get(offset)?.literal.as_deref()?)
        && !matches!(
            words.get(offset)?.literal.as_deref()?,
            "eval"
                | "cd"
                | "export"
                | "local"
                | "set"
                | "shift"
                | "return"
                | "break"
                | "continue"
                | "exit"
                | "read"
                | "unset"
                | "readonly"
                | "declare"
                | "alias"
                | "unalias"
                | "trap"
                | "printf"
                | "echo"
                | "true"
                | "false"
                | ":"
        )
    {
        return None;
    }
    Some(offset)
}

fn nice_offset(words: &[structure::Word]) -> Option<usize> {
    let mut at = 1;
    while let Some(option) = words.get(at)?.literal.as_deref() {
        if matches!(option, "-n" | "--adjustment") {
            words.get(at + 1)?.literal.as_deref()?.parse::<i64>().ok()?;
            at += 2;
        } else if let Some(value) = option
            .strip_prefix("--adjustment=")
            .or_else(|| option.strip_prefix("-n").filter(|value| !value.is_empty()))
        {
            value.parse::<i64>().ok()?;
            at += 1;
        } else {
            break;
        }
    }
    end_options(words, at)
}

/// GNU `timeout [OPTION] DURATION COMMAND`: its options, then a literal
/// duration such as `5`, `2.5s` or `1m`, then the command it runs.
fn timeout_offset(words: &[structure::Word]) -> Option<usize> {
    let mut at = 1;
    while let Some(option) = words.get(at)?.literal.as_deref() {
        match option {
            "--preserve-status" | "--foreground" | "-v" | "--verbose" => at += 1,
            "-s" | "--signal" | "-k" | "--kill-after" => {
                words.get(at + 1)?.literal.as_ref()?;
                at += 2;
            }
            _ if option.starts_with("--signal=") || option.starts_with("--kill-after=") => {
                at += 1;
            }
            _ if (option.starts_with("-s") || option.starts_with("-k")) && option.len() > 2 => {
                at += 1;
            }
            _ => break,
        }
    }
    at = end_options(words, at)?;
    let duration = words.get(at)?.literal.as_deref()?;
    let number = duration
        .strip_suffix(['s', 'm', 'h', 'd'])
        .unwrap_or(duration);
    let (whole, fraction) = number.split_once('.').unwrap_or((number, "0"));
    let digits = |part: &str| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit());
    (digits(whole) && digits(fraction)).then_some(at + 1)
}

fn end_options(words: &[structure::Word], at: usize) -> Option<usize> {
    Some(at + usize::from(words.get(at)?.literal.as_deref() == Some("--")))
}
