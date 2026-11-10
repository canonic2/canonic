//! Common-version native PowerShell literals, sequences and call operators.
//! Cmdlets, aliases, expansions and version-specific syntax stay opaque.
use super::structure::Node;
use crate::policy::Error;
mod literal;

pub(super) fn parse(
    source: &str,
    checkpoint: &dyn Fn() -> Result<(), Error>,
) -> Result<Option<Node>, Error> {
    literal::parse(source, checkpoint)
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
            "git 'a'joined",
            "git --foo='x'",
            r#"git "a""b""#,
            r#"git 'a"b'"#,
            r"git 'C:\Acme Tools\'",
            "git \u{201c}status\u{201d}",
            "git status '\u{201a}; Remove-Item -Recurse C:\\acme; \u{201a}'",
            "git status '\u{201b}; Remove-Item -Recurse C:\\acme; \u{201b}'",
            "git status \"\u{201e}; Remove-Item -Recurse C:\\acme; \u{201e}\"",
            "rg 'café' notes.txt",
            "git status # comment",
            "git (status)",
            "git status &",
            "'git' status",
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
        assert!(matches!(
            parse("git 'unterminated", &|| Ok(())),
            Err(Error::Contract(_))
        ));
    }

    #[test]
    fn native_argument_scanning_checks_cancellation_and_bounds_argument_count() {
        let calls = std::cell::Cell::new(0);
        let result = parse(&format!("rg '{}'", "x".repeat(16_000)), &|| {
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
