//! Closed PowerShell argument grammar for a single known native command.
//! Cmdlets, aliases, expressions, expansions and compound source remain opaque.
use super::structure::{Node, Span, Word};
use crate::policy::Error;

pub(super) fn parse(
    source: &str,
    checkpoint: &dyn Fn() -> Result<(), Error>,
) -> Result<Option<Node>, Error> {
    let mut input = source.char_indices().peekable();
    let mut words = Vec::new();
    while let Some(&(start, character)) = input.peek() {
        checkpoint()?;
        if matches!(character, ' ' | '\t') {
            input.next();
            continue;
        }
        let quote = matches!(character, '\'' | '"').then_some(character);
        if quote.is_some() {
            input.next();
        }
        let mut literal = String::new();
        let mut end = source.len();
        let mut closed = quote.is_none();
        let mut scanned = 0_usize;
        while let Some((at, character)) = input.next() {
            if scanned.is_multiple_of(128) {
                checkpoint()?;
            }
            scanned += 1;
            if Some(character) == quote {
                if quote == Some('\'') && input.peek().is_some_and(|(_, next)| *next == '\'') {
                    input.next();
                    literal.push('\'');
                    continue;
                }
                end = at + character.len_utf8();
                closed = true;
                break;
            }
            if quote.is_none() && matches!(character, ' ' | '\t') {
                end = at;
                break;
            }
            // PowerShell recognizes curly quotes as delimiters too. Refuse all
            // controls and native double quotes (legacy argv marshalling strips
            // or rearranges them). Double-quoted strings must not expand.
            if character.is_control()
                || matches!(
                    character,
                    '"' | '\u{2018}' | '\u{2019}' | '\u{201c}' | '\u{201d}'
                )
                || (quote == Some('"') && matches!(character, '$' | '`'))
                || (quote.is_none()
                    && !(character.is_ascii_alphanumeric() || "._:/\\=-+".contains(character)))
            {
                return Ok(None);
            }
            literal.push(character);
        }
        // Empty arguments and a whitespace-containing argument ending in a
        // backslash have version-dependent native marshalling. Adjacent quoted
        // and unquoted fragments are outside this grammar as well.
        if !closed
            || literal.is_empty()
            || literal == "--%"
            || (literal.contains(' ') && literal.ends_with('\\'))
            || (quote.is_some()
                && input
                    .peek()
                    .is_some_and(|(_, next)| !matches!(next, ' ' | '\t')))
        {
            return Ok(None);
        }
        words.push(Word {
            span: Span { start, end },
            raw: source[start..end].into(),
            literal: Some(literal),
            substitutions: Vec::new(),
            fragments: Vec::new(),
            resolved: true,
        });
        if words.len() > 257 {
            return Err(Error::Limit);
        }
    }
    // These are native programs in the supported tool contracts. In particular,
    // POSIX names such as cat, echo, mv and ls are PowerShell aliases, while
    // npm/pnpm can resolve to .ps1 wrappers with parameter binding of their own.
    let Some(executable) = words.first() else {
        return Ok(None);
    };
    if executable.raw != executable.literal.as_deref().unwrap_or_default()
        || !matches!(
            executable.literal.as_deref(),
            Some("git" | "rg" | "node" | "rustc" | "cargo" | "make" | "just" | "shield")
        )
    {
        return Ok(None);
    }
    Ok(Some(Node::Command {
        span: Span {
            start: 0,
            end: source.len(),
        },
        words,
        redirections: Vec::new(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_native_arguments_keep_powershell_quoting_and_windows_separators() {
        for (source, expected) in [
            ("git status --short", vec!["git", "status", "--short"]),
            (
                r"git -C 'C:\Acme Tools\project' status",
                vec!["git", "-C", r"C:\Acme Tools\project", "status"],
            ),
            (
                r#"rg 'don''t' "notes here.txt""#,
                vec!["rg", "don't", "notes here.txt"],
            ),
            (
                "rg '$HOME; $(whoami)' notes.txt",
                vec!["rg", "$HOME; $(whoami)", "notes.txt"],
            ),
            ("node --version\t", vec!["node", "--version"]),
        ] {
            let Node::Command { words, .. } = parse(source, &|| Ok(())).unwrap().unwrap() else {
                panic!("command")
            };
            assert_eq!(
                words
                    .iter()
                    .map(|word| word.literal.as_deref().unwrap())
                    .collect::<Vec<_>>(),
                expected,
                "{source}"
            );
        }
    }

    #[test]
    fn unsupported_source_never_returns_a_safe_prefix() {
        for source in [
            "git status; Remove-Item .shield",
            "git status\nRemove-Item .shield",
            "git status | node -e dangerous",
            "git status && git diff",
            "git status > out.txt",
            "git $(whoami)",
            "git $HOME",
            "git `nstatus",
            "git @args",
            "git @{x=1}",
            "git --% $HOME",
            "git ''",
            "git 'unterminated",
            "git 'a'joined",
            "git --foo='x'",
            r#"git "a""b""#,
            r#"git 'a"b'"#,
            r"git 'C:\Acme Tools\'",
            "git \u{201c}status\u{201d}",
            "git status # comment",
            "git (status)",
            "git status &",
            "'git' status",
            "& git status",
            "Get-Content notes.txt",
            "cat notes.txt",
            "echo hello",
            "npm test",
            "pnpm test",
            "Git status",
            "git.exe status",
            "function git { dangerous }; git status",
        ] {
            assert!(parse(source, &|| Ok(())).unwrap().is_none(), "{source}");
        }
    }

    #[test]
    fn native_argument_scanning_checks_cancellation_and_bounds_argument_count() {
        let calls = std::cell::Cell::new(0);
        let result = parse(&format!("rg '{}'", "é".repeat(16_000)), &|| {
            calls.set(calls.get() + 1);
            if calls.get() == 8 {
                Err(Error::Interrupted)
            } else {
                Ok(())
            }
        });
        assert!(matches!(result, Err(Error::Interrupted)));
        assert!(matches!(
            parse(&format!("git {}", "arg ".repeat(257)), &|| Ok(())),
            Err(Error::Limit)
        ));
    }
}
