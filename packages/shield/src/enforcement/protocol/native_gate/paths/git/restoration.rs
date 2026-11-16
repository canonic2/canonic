//! Complete literal Git restoration forms eligible for exact human review.
use super::Invocation;

pub(super) fn paths<'a>(invocation: &Invocation<'a>) -> Option<&'a [&'a str]> {
    if !invocation.ordinary || invocation.execution_override || invocation.elsewhere {
        return None;
    }
    let separator = invocation.rest.iter().position(|word| *word == "--")?;
    let (options, paths) = invocation.rest.split_at(separator);
    let paths = &paths[1..];
    if paths.is_empty() || paths.iter().any(|path| path.is_empty()) {
        return None;
    }
    let mut words = options.iter().copied();
    let mut revisions = 0;
    while let Some(word) = words.next() {
        let supported = match invocation.subcommand {
            "checkout" => {
                if ["--ours", "--theirs", "--force", "-f", "--quiet", "-q"].contains(&word) {
                    true
                } else if !word.starts_with('-') && revisions == 0 {
                    revisions += 1;
                    true
                } else {
                    false
                }
            }
            "restore" => {
                if [
                    "--ours",
                    "--theirs",
                    "--staged",
                    "-S",
                    "--worktree",
                    "-W",
                    "--quiet",
                    "-q",
                ]
                .contains(&word)
                {
                    true
                } else {
                    let source = if ["--source", "-s"].contains(&word) {
                        words.next()
                    } else {
                        word.strip_prefix("--source=")
                            .or_else(|| word.strip_prefix("-s"))
                    };
                    source.is_some_and(|value| !value.is_empty() && !value.starts_with('-'))
                }
            }
            "checkout-index" => [
                "--force",
                "-f",
                "--quiet",
                "-q",
                "--stage=1",
                "--stage=2",
                "--stage=3",
            ]
            .contains(&word),
            _ => false,
        };
        if !supported {
            return None;
        }
    }
    ["checkout", "restore", "checkout-index"]
        .contains(&invocation.subcommand)
        .then_some(paths)
}
