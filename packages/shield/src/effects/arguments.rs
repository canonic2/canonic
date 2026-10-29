//! Owns explicit global-option arity and retained option boundaries, without launch I/O.

use serde::Serialize;

#[derive(Debug, Serialize)]
pub(super) struct OptionValue {
    pub(super) index: usize,
    pub(super) name: String,
    pub(super) value: Option<String>,
}

pub(super) struct Globals {
    pub(super) command_at: usize,
    pub(super) options: Vec<OptionValue>,
    pub(super) unresolved: Option<&'static str>,
    pub(super) query: bool,
    end_of_options: bool,
}

/// Explicit option grammar; Git flags do not use Cobra boolean values/bundles.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Syntax {
    Git,
    GitCommand,
    DockerGlobals,
    DockerRuntime,
}

pub(super) fn parse(args: &[String], valued: &[&str], flags: &[&str], syntax: Syntax) -> Globals {
    let git = matches!(syntax, Syntax::Git | Syntax::GitCommand);
    let mut result = Globals {
        command_at: 0,
        options: Vec::new(),
        unresolved: None,
        query: false,
        end_of_options: false,
    };
    while let Some(arg) = args.get(result.command_at) {
        if !arg.starts_with('-') {
            break;
        }
        let index = result.command_at;
        if matches!(syntax, Syntax::DockerRuntime | Syntax::GitCommand) && arg == "--" {
            result.command_at += 1;
            result.end_of_options = true;
            break;
        }
        let (name, attached) = arg
            .split_once('=')
            .map_or((arg.as_str(), None), |(name, value)| (name, Some(value)));
        if syntax == Syntax::Git && matches!(arg.as_str(), "--help" | "-h" | "--version" | "-v") {
            break;
        }
        if syntax == Syntax::Git && git_query(name, attached) {
            result.query = true;
            result.options.push(OptionValue {
                index,
                name: name.into(),
                value: attached.map(str::to_owned),
            });
            break;
        }
        if valued.contains(&name)
            && !(syntax == Syntax::Git
                && attached.is_some()
                && (name.len() == 2 || name == "--shallow-file"))
        {
            let value = if let Some(value) = attached {
                value
            } else {
                result.command_at += 1;
                let Some(value) = args.get(result.command_at) else {
                    result.unresolved = Some("missing-global-option-value");
                    break;
                };
                value
            };
            result.options.push(OptionValue {
                index,
                name: name.into(),
                value: Some(value.into()),
            });
        } else if flags.contains(&name)
            && attached.is_none_or(|value| !git && matches!(value, "true" | "false"))
        {
            result.options.push(OptionValue {
                index,
                name: name.into(),
                value: attached.map(str::to_owned),
            });
        } else if let Some(short) = valued.iter().find(|name| {
            !git && name.len() == 2
                && !name.starts_with("--")
                && arg.starts_with(**name)
                && arg.len() > 2
        }) {
            result.options.push(OptionValue {
                index,
                name: (*short).into(),
                value: Some(arg[2..].into()),
            });
        } else if !git
            && arg.starts_with('-')
            && !arg.starts_with("--")
            && arg.len() > 2
            && arg[1..]
                .chars()
                .all(|ch| flags.contains(&format!("-{ch}").as_str()))
        {
            result
                .options
                .extend(arg[1..].chars().map(|ch| OptionValue {
                    index,
                    name: format!("-{ch}"),
                    value: None,
                }));
        } else {
            result.unresolved = Some("unknown-global-option");
            break;
        }
        result.command_at += 1;
    }
    result
}

fn git_query(name: &str, attached: Option<&str>) -> bool {
    (attached.is_none()
        && matches!(
            name,
            "--exec-path" | "--html-path" | "--man-path" | "--info-path"
        ))
        || (name == "--list-cmds" && attached.is_some())
}

/// Cobra accepts flags between operands; unknown arity stops interpretation.
pub(super) fn interspersed(args: &[String], valued: &[&str], flags: &[&str]) -> Globals {
    let mut result = parse(&[], valued, flags, Syntax::DockerRuntime);
    while result.command_at < args.len() {
        let start = result.command_at;
        let mut segment = parse(&args[start..], valued, flags, Syntax::DockerRuntime);
        for option in &mut segment.options {
            option.index += start;
        }
        result.options.extend(segment.options);
        result.command_at += segment.command_at;
        result.unresolved = segment.unresolved;
        result.end_of_options = segment.end_of_options;
        if result.unresolved.is_some() || result.end_of_options {
            break;
        }
        // One non-option operand; its bytes never become option values or code.
        result.command_at += 1;
    }
    result
}

/// Git alias tokenization produces argv, never shell control flow or expansions.
pub(super) fn alias_words(source: &str) -> Option<Vec<String>> {
    let mut chars = source.chars();
    let mut quote = None;
    let mut word = String::new();
    let mut started = false;
    let mut result = Vec::new();
    while let Some(ch) = chars.next() {
        if quote == Some(ch) {
            quote = None;
        } else if quote.is_none() && matches!(ch, '\'' | '"') {
            quote = Some(ch);
            started = true;
        } else if ch == '\\' && quote != Some('\'') {
            word.push(chars.next()?);
            started = true;
        } else if quote.is_none() && ch.is_ascii_whitespace() {
            if started {
                result.push(std::mem::take(&mut word));
                started = false;
            }
        } else {
            word.push(ch);
            started = true;
        }
        if word.len() > 4096 || result.len() > 256 {
            return None;
        }
    }
    if quote.is_some() {
        return None;
    }
    if started {
        result.push(word);
    }
    if result.is_empty() || result.len() > 256 {
        None
    } else {
        Some(result)
    }
}
