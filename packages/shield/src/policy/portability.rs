//! Whether a custom rule names this machine's folders: the project checkout,
//! the home folder, temporary folders or agent configuration folders. Pure: the
//! caller discovers the roots.
use serde_json::Value;
use std::path::{Component, Path, PathBuf};

/// What a machine folder is, which decides how a rule can do without it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Project,
    AgentConfiguration,
    Temporary,
    Home,
}

/// One machine folder, with every spelling that names it, such as `/tmp` and
/// `/private/tmp` on macOS.
#[derive(Clone, Debug)]
pub(crate) struct Root {
    pub(crate) kind: Kind,
    pub(crate) paths: Vec<PathBuf>,
}

/// The root kinds the check covered, for the refusal.
const CHECKED: &str = "Checked roots: project, agent configuration, temporary and home folders.";

/// The longest path shown in a refusal.
const SHOWN: usize = 200;

/// Refuses a rule whose conditions or requirements contain an absolute path
/// inside one of `roots`, naming the path, its folder and what to write
/// instead. Roots are tried in order, so the most specific kind comes first.
/// Exact-request rules bind one complete request, including its cwd, by
/// design; only their requirements are checked.
pub(crate) fn check(rule: &Value, roots: &[Root]) -> Result<(), String> {
    let mut fields = vec![&rule["requirements"]];
    if rule["conditions"]["domain"] != "exact_request" {
        fields.push(&rule["conditions"]);
    }
    let id = rule["id"].as_str().unwrap_or("rule");
    check_fields(&fields, roots, &format!("Rule {id} names"))
}

/// Warns about machine-specific command evidence without inventing a rule ID.
pub(crate) fn check_request(request: &Value, roots: &[Root]) -> Result<(), String> {
    check_fields(&[request], roots, "The request names")
}

fn check_fields(fields: &[&Value], roots: &[Root], subject: &str) -> Result<(), String> {
    let mut found = None;
    for field in fields {
        visit(field, &mut |text| {
            if found.is_none() {
                found = machine_path(&unescaped(text), roots);
            }
        });
    }
    let Some((path, kind)) = found else {
        return Ok(());
    };
    let shown: String = path.chars().take(SHOWN).collect();
    let fix = match kind {
        Kind::Project => {
            "the project folder makes this checkout-specific. Drop directory options and use cwd within {root: project}; express path arguments with argument_paths or project-relative cwd conditions."
        }
        Kind::AgentConfiguration => {
            "an agent's configuration folder differs across machines. Omit the path; use the agent's existing working-file access."
        }
        Kind::Temporary => {
            "the temporary folder differs across machines. Omit the path; use the agent's existing temporary-file access."
        }
        Kind::Home => "the home folder differs across machines. Omit this path.",
    };
    Err(format!("{subject} `{shown}`: {fix} {CHECKED}"))
}

fn visit<'a>(value: &'a Value, found: &mut impl FnMut(&'a str)) {
    match value {
        Value::String(text) => found(text),
        Value::Array(items) => items.iter().for_each(|item| visit(item, found)),
        Value::Object(fields) => fields.values().for_each(|item| visit(item, found)),
        _ => {}
    }
}

/// The first absolute path in `text` inside a root, with that root's kind.
/// Paths start at a `/` that does not continue a word or another path, and end
/// at whitespace, quoting or pattern syntax, so arguments such as
/// `--prefix=/srv/acme`, globs and regular expressions are all read.
fn machine_path(text: &str, roots: &[Root]) -> Option<(String, Kind)> {
    let bytes = text.as_bytes();
    let mut start = 0;
    while let Some(offset) = text[start..].find('/') {
        let begin = start + offset;
        let end = text[begin..]
            .find(|c: char| c.is_whitespace() || "\"'`,;=()|^$[]{}*?+\\<>".contains(c))
            .map_or(text.len(), |length| begin + length);
        start = end.max(begin + 1);
        if begin > 0 && continues_path(bytes[begin - 1]) {
            continue;
        }
        let literal = &text[begin..end];
        let Some(path) = lexical(literal) else {
            continue;
        };
        if let Some(root) = roots.iter().find(|root| {
            root.paths
                .iter()
                .any(|base| base.parent().is_some() && path.starts_with(base))
        }) {
            return Some((literal.to_owned(), root.kind));
        }
    }
    None
}

/// `text` with backslash escapes of punctuation removed, so a pattern such as
/// `^\/srv\/acme\-app` reads as the path it matches. Escapes such as `\d` stay
/// and end a path.
fn unescaped(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut characters = text.chars().peekable();
    while let Some(character) = characters.next() {
        match characters.peek() {
            Some(next) if character == '\\' && next.is_ascii_punctuation() => {
                result.push(*next);
                characters.next();
            }
            _ => result.push(character),
        }
    }
    result
}

fn continues_path(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || b"/._-~".contains(&byte)
}

/// The path with `.` and `..` components removed, without reading the
/// filesystem; `..` at the filesystem root stays there, as it does on disk.
pub(crate) fn lexical(literal: &str) -> Option<PathBuf> {
    let mut path = PathBuf::new();
    for component in Path::new(literal).components() {
        match component {
            Component::RootDir => path.push("/"),
            Component::Normal(name) => path.push(name),
            Component::CurDir => {}
            Component::ParentDir => {
                path.pop();
            }
            Component::Prefix(_) => return None,
        }
    }
    Some(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn roots() -> Vec<Root> {
        let root = |kind, paths: &[&str]| Root {
            kind,
            paths: paths.iter().map(PathBuf::from).collect(),
        };
        vec![
            root(Kind::Project, &["/home/acme/code/acme-app"]),
            root(Kind::AgentConfiguration, &["/home/acme/.claude"]),
            root(Kind::Temporary, &["/tmp", "/private/tmp"]),
            root(Kind::Home, &["/home/acme"]),
        ]
    }

    fn argv(arguments: &[&str]) -> Value {
        json!({"id":"acme-start","decision":"allow","conditions":{"domain":"argv",
            "executable":"npm","arguments":{"kind":"exact","values":arguments}}})
    }

    fn refusal(rule: &Value) -> Option<Kind> {
        let message = check(rule, &roots()).err()?;
        [
            ("project folder", Kind::Project),
            ("configuration folder", Kind::AgentConfiguration),
            ("temporary folder", Kind::Temporary),
            ("home folder", Kind::Home),
        ]
        .into_iter()
        .find(|(phrase, _)| message.contains(phrase))
        .map(|(_, kind)| kind)
    }

    #[test]
    fn checkout_paths_are_refused_in_every_spelling() {
        for arguments in [
            &["--prefix", "/home/acme/code/acme-app", "start"][..],
            &["--prefix=/home/acme/code/acme-app", "start"],
            &["--prefix", "/home/acme/code/acme-app/", "start"],
            &["--prefix", "/home/acme/code/other/../acme-app/web", "start"],
            &["--prefix", "/home/acme/code/./acme-app", "start"],
        ] {
            assert_eq!(
                refusal(&argv(arguments)),
                Some(Kind::Project),
                "{arguments:?}"
            );
        }
        let message = check(
            &argv(&["--prefix", "/home/acme/code/acme-app", "start"]),
            &roots(),
        )
        .unwrap_err();
        assert!(message.contains("`/home/acme/code/acme-app`"), "{message}");
        assert!(message.contains("cwd within {root: project}"), "{message}");
    }

    #[test]
    fn paths_in_patterns_and_location_predicates_are_found() {
        let regex = json!({"id":"r","decision":"allow","conditions":{"domain":"argv",
            "executable":"acme","arguments":{"kind":"regex","pattern":"^/home/acme/code/acme-app(/.*)?$"}}});
        assert_eq!(refusal(&regex), Some(Kind::Project));
        // Escaped separators and punctuation still name the checkout.
        for pattern in [
            r"^\/home\/acme\/code\/acme-app(\/.*)?$",
            r"^/home/acme/code/acme\-app\/web$",
        ] {
            let escaped = json!({"id":"r","decision":"allow","conditions":{"domain":"argv",
                "executable":"acme","arguments":{"kind":"regex","pattern":pattern}}});
            assert_eq!(refusal(&escaped), Some(Kind::Project), "{pattern}");
        }
        // Escapes that are not punctuation still end a path.
        assert_eq!(unescaped(r"\/srv\d\-x\\"), "/srv\\d-x\\");
        let location = json!({"id":"l","decision":"allow","conditions":{"domain":"argv",
            "executable":"acme","arguments":{"kind":"any"}},
            "requirements":{"cwd":{"exact":{"root":"absolute","path":"/tmp/acme"}}}});
        assert_eq!(refusal(&location), Some(Kind::Temporary));
        let aliased = argv(&["--cache", "/private/tmp/acme"]);
        assert_eq!(refusal(&aliased), Some(Kind::Temporary));
    }

    #[test]
    fn the_most_specific_folder_names_the_fix() {
        assert_eq!(
            refusal(&argv(&["--config", "/home/acme/.claude/settings.json"])),
            Some(Kind::AgentConfiguration)
        );
        assert_eq!(
            refusal(&argv(&["--config", "/home/acme/.npmrc"])),
            Some(Kind::Home)
        );
        assert_eq!(
            refusal(&argv(&["--config", "/../home/acme/.npmrc"])),
            Some(Kind::Home)
        );
    }

    #[test]
    fn portable_rules_and_unrelated_paths_pass() {
        for arguments in [
            &["start"][..],
            &["--prefix", "web", "start"],
            &["--prefix", "/srv/acme", "start"],
            &["--prefix", "/home/acme-other/app", "start"],
            &["--prefix", "/home/acmes", "start"],
            &["--prefix", "./home/acme", "start"],
            &["--prefix", "web/home/acme", "start"],
            &["--prefix", "/home/acme/../../tmpdata", "start"],
        ] {
            assert_eq!(check(&argv(arguments), &roots()), Ok(()), "{arguments:?}");
        }
        let portable = json!({"id":"p","decision":"allow","conditions":{"domain":"argv",
            "executable":"npm","arguments":{"kind":"exact","values":["start"]},
            "cwd":{"within":{"root":"project"}}}});
        assert_eq!(check(&portable, &roots()), Ok(()));
    }

    #[test]
    fn exact_requests_keep_their_bound_cwd_but_not_their_requirements() {
        let exact = json!({"id":"e","decision":"allow","conditions":{"domain":"exact_request",
            "input":{"kind":"argv","executable":"acme","arguments":[]},
            "context":{"cwd":"/home/acme/code/acme-app"},"executable":null}});
        assert_eq!(check(&exact, &roots()), Ok(()));
        let mut required = exact;
        required["requirements"] = json!({"cwd":{"exact":{"root":"absolute","path":"/home/acme"}}});
        assert_eq!(refusal(&required), Some(Kind::Home));
    }

    #[test]
    fn the_filesystem_root_is_never_a_machine_folder() {
        let roots = [Root {
            kind: Kind::Home,
            paths: vec![PathBuf::from("/")],
        }];
        assert_eq!(check(&argv(&["--prefix", "/srv/acme"]), &roots), Ok(()));
    }
}
