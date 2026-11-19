//! Output paths for Git's diff and revision commands. Unverified argument
//! boundaries and possible direct file reads cannot establish write coverage.
use super::{Descendants, Target, Targets, join_operand};
use std::{
    fs,
    io::ErrorKind,
    path::{Component, Path},
};

/// Git opens each named file during option parsing. Resolve relative output
/// against the invocation directory after any policy-normalized -C.
pub(super) fn output(arguments: &[String], cwd: &Path, stable: bool) -> Targets {
    let arguments = match arguments {
        [option, rest @ ..] if matches!(option.as_str(), "--no-pager" | "-P") => rest,
        _ => arguments,
    };
    let Some((command, words)) = arguments.split_first() else {
        return Targets::default();
    };
    if !stable
        || !matches!(
            command.as_str(),
            "diff" | "show" | "log" | "diff-tree" | "diff-index" | "whatchanged"
        )
    {
        return Targets::default();
    }
    let diff = command == "diff";
    // Git looks for --no-index before parsing the diff options and their values.
    if diff && words.iter().any(|word| word == "--no-index") {
        return Targets::default();
    }
    let mut targets = Targets {
        complete: true,
        ..Targets::default()
    };
    let mut operands = Vec::new();
    let mut separated = false;
    let mut words = words.iter().map(String::as_str);
    while let Some(word) = words.next() {
        let (name, attached) = word
            .split_once('=')
            .map_or((word, None), |(name, value)| (name, Some(value)));
        if separated || !word.starts_with('-') || word == "-" {
            if separated && name == "--output" {
                targets.complete = false;
            }
            if diff && operands.len() < 2 {
                operands.push(word);
            }
            continue;
        }
        if word == "--" {
            separated = true;
            continue;
        }
        match arity(command, name) {
            Some(Arity::Value) => {
                let value = attached.or_else(|| words.next());
                // Some parsers look for the separator before parsing options,
                // so a standalone -- is never proof of an option's value.
                let Some(value) = value.filter(|value| *value != "--") else {
                    targets.complete = false;
                    break;
                };
                if name == "--output" {
                    if value.is_empty() {
                        targets.complete = false;
                        break;
                    }
                    targets.items.push(Target {
                        action: "write",
                        role: "destination",
                        path: join_operand(cwd, value, &mut targets.complete),
                        recursive: false,
                        descendants: Descendants::All,
                        follow: true,
                    });
                }
            }
            Some(Arity::OptionalAttached) => {}
            Some(Arity::Attached) if attached.is_some() => {}
            Some(Arity::Flag) if attached.is_none() => {}
            _ => {
                targets.complete = false;
                break;
            }
        }
    }
    // No-index can be implicit even with a valid output option. Two operands
    // can require file reads, including outside the repository. Keep the
    // revision-only case when neither can name existing input. Opening a new
    // output can only create an empty file before Git reads the inputs.
    if targets.complete && !targets.items.is_empty() && operands.len() == 2 {
        targets.complete = operands.iter().all(|word| missing_file(cwd, word));
    }
    targets
}

enum Arity {
    Flag,
    Value,
    Attached,
    OptionalAttached,
}

// Exact spellings with verified arity. Git's native protected-path scan also
// recognizes forms whose arity is approximate, which cannot prove coverage here.
fn arity(command: &str, name: &str) -> Option<Arity> {
    Some(match name {
        "--decorate-refs" | "--decorate-refs-exclude"
            if matches!(command, "log" | "show" | "whatchanged") =>
        {
            Arity::Value
        }
        "--output"
        | "--line-prefix"
        | "--src-prefix"
        | "--dst-prefix"
        | "--output-indicator-new"
        | "--output-indicator-old"
        | "--output-indicator-context"
        | "--word-diff-regex"
        | "--diff-algorithm"
        | "--diff-filter"
        | "--inter-hunk-context"
        | "--ignore-matching-lines"
        | "--author"
        | "--committer"
        | "--grep"
        | "--encoding"
        | "--date"
        | "--since"
        | "--until"
        | "--max-count"
        | "--skip"
        | "-n"
        | "-S"
        | "-G"
        | "-I" => Arity::Value,
        "--format" => Arity::Attached,
        "--stat" | "--dirstat" | "--dirstat-by-file" | "--color" | "--color-words"
        | "--word-diff" | "--abbrev" | "--relative" | "--pretty" | "--decorate" | "--unified"
        | "-U" | "-M" | "-C" | "-B" => Arity::OptionalAttached,
        "--cached"
        | "--staged"
        | "--patch"
        | "--no-patch"
        | "--raw"
        | "--patch-with-raw"
        | "--patch-with-stat"
        | "--name-only"
        | "--name-status"
        | "--numstat"
        | "--shortstat"
        | "--summary"
        | "--check"
        | "--binary"
        | "--full-index"
        | "--exit-code"
        | "--quiet"
        | "--text"
        | "--ignore-all-space"
        | "--ignore-space-change"
        | "--ignore-space-at-eol"
        | "--ignore-blank-lines"
        | "--no-color"
        | "--no-ext-diff"
        | "--no-textconv"
        | "--oneline"
        | "--graph"
        | "--no-decorate"
        | "--no-abbrev-commit"
        | "--abbrev-commit"
        | "--all"
        | "--first-parent"
        | "--reverse"
        | "--no-renames"
        | "--i-still-use-this"
        | "-p"
        | "-s"
        | "-u"
        | "-w"
        | "-b"
        | "-a"
        | "-R" => Arity::Flag,
        _ if name.starts_with('-')
            && name.len() > 1
            && name[1..].bytes().all(|byte| byte.is_ascii_digit()) =>
        {
            Arity::Flag
        }
        _ if name.starts_with('-')
            && name.len() > 2
            && b"nSGIUCBM".contains(&name.as_bytes()[1]) =>
        {
            Arity::OptionalAttached
        }
        _ => return None,
    })
}

fn missing_file(cwd: &Path, word: &str) -> bool {
    let path = Path::new(word);
    word != "-"
        && !word.is_empty()
        && word
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b".-_/~^@{}".contains(&byte))
        && path.components().all(ordinary_component)
        && fs::symlink_metadata(cwd.join(path))
            .is_err_and(|error| error.kind() == ErrorKind::NotFound)
}

fn ordinary_component(part: Component<'_>) -> bool {
    let Component::Normal(name) = part else {
        return false;
    };
    if cfg!(windows) {
        // Metadata absence does not prove that a DOS device name cannot open.
        let Some(name) = name.to_str() else {
            return false;
        };
        let stem = name.split('.').next().unwrap_or(name).to_ascii_uppercase();
        return !matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
            && !(stem.len() == 4
                && (stem.starts_with("COM") || stem.starts_with("LPT"))
                && stem.as_bytes()[3].is_ascii_digit());
    }
    true
}
