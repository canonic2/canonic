//! Declared directory options: tool facts saying which options only choose the
//! folder a tool works in, such as `npm --prefix DIR` or `git -C DIR`. A request
//! using one is matched as the same command without it, run from that folder,
//! so one rule covers every way the tool is told where to work.
use super::{Error, Request, portability::lexical};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

/// The most declarations one document may hold.
const MAX_DECLARATIONS: usize = 1024;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct DirectoryOption {
    /// The executable: one name, or `one_of` several, as rules write it.
    pub(crate) executable: Executable,
    /// The option name, such as `--prefix` or `-C`.
    pub(crate) option: String,
    /// How the value is written. Long options default to a separate argument
    /// or `--name=value`; short options to a separate or attached value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    forms: Option<Vec<Form>>,
    /// Where the option is recognized.
    #[serde(default, skip_serializing_if = "is_default")]
    position: Position,
    /// What the value names.
    #[serde(default, skip_serializing_if = "is_default")]
    value: Names,
    /// For `value: file`, the file names the tool finds in its folder without
    /// the option, such as `Cargo.toml`. Only a value naming one of them
    /// chooses just a folder; another file is a different project.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    files: Vec<String>,
    /// How repeated occurrences combine.
    #[serde(default, skip_serializing_if = "is_default")]
    repeat: Repeat,
    /// Whether the tool resolves other relative paths against the chosen folder.
    paths: Paths,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(untagged)]
pub(crate) enum Executable {
    Name(String),
    OneOf { one_of: Vec<String> },
}

impl Executable {
    fn names(&self) -> &[String] {
        match self {
            Self::Name(name) => std::slice::from_ref(name),
            Self::OneOf { one_of } => one_of,
        }
    }
}

fn is_default<T: Default + PartialEq>(value: &T) -> bool {
    *value == T::default()
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
enum Form {
    /// `--prefix DIR`
    Separate,
    /// `--prefix=DIR`
    Equals,
    /// `-CDIR`
    Attached,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Position {
    /// Before the first operand, such as a subcommand.
    #[default]
    Leading,
    /// Anywhere before `--`.
    Anywhere,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Names {
    /// The working folder itself.
    #[default]
    Directory,
    /// A file whose folder is the working folder, such as a manifest.
    File,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Repeat {
    /// A repeated option leaves the request as submitted.
    #[default]
    Refuse,
    /// The last occurrence wins.
    Last,
    /// Each relative value resolves against the previous one, as `git -C` does.
    Chain,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Paths {
    /// Relative paths in the other arguments resolve against the chosen folder.
    Follow,
    /// They resolve against the original folder, so the request is normalized
    /// only when the chosen folder is that folder.
    Unchanged,
}

impl DirectoryOption {
    fn forms(&self) -> Vec<Form> {
        self.forms.clone().unwrap_or_else(|| {
            if self.option.starts_with("--") {
                vec![Form::Separate, Form::Equals]
            } else {
                vec![Form::Separate, Form::Attached]
            }
        })
    }

    /// This declaration for one executable, with its forms spelled out, so
    /// declarations written differently compare by what they mean.
    fn meaning(&self, name: &str) -> Self {
        let mut forms = self.forms();
        forms.sort();
        let mut files = self.files.clone();
        files.sort();
        Self {
            executable: Executable::Name(name.to_owned()),
            forms: Some(forms),
            files,
            ..self.clone()
        }
    }

    fn validate(&self) -> Result<(), Error> {
        let option = &self.option;
        let names = self.executable.names();
        let mut unique: Vec<_> = names.iter().collect();
        unique.sort();
        unique.dedup();
        if names.is_empty()
            || names.len() > 256
            || unique.len() != names.len()
            || names
                .iter()
                .any(|name| name.is_empty() || name.len() > 4096 || name.contains('\0'))
        {
            return Err(Error::Contract("invalid directory option executable"));
        }
        if !option.starts_with('-')
            || option == "-"
            || option == "--"
            || option.len() > 128
            || option.contains(|c: char| c == '=' || c.is_whitespace() || c == '\0')
        {
            return Err(Error::Contract("invalid directory option name"));
        }
        let forms = self.forms();
        let mut unique = forms.clone();
        unique.sort();
        unique.dedup();
        if forms.is_empty() || unique.len() != forms.len() {
            return Err(Error::Contract("invalid directory option forms"));
        }
        // Only a single-letter short option can take an attached value.
        if forms.contains(&Form::Attached) && (option.starts_with("--") || option.len() != 2) {
            return Err(Error::Contract(
                "only a short directory option takes an attached value",
            ));
        }
        let plain = |name: &String| {
            !name.is_empty()
                && name.len() <= 255
                && name != "."
                && name != ".."
                && !name.contains(['/', '\\', '\0'])
        };
        match self.value {
            Names::File if self.files.is_empty() || self.files.len() > 16 => {
                return Err(Error::Contract(
                    "a file directory option names the files its tool finds by default",
                ));
            }
            Names::Directory if !self.files.is_empty() => {
                return Err(Error::Contract("only a file directory option names files"));
            }
            _ if !self.files.iter().all(plain) => {
                return Err(Error::Contract("invalid directory option file name"));
            }
            _ => {}
        }
        Ok(())
    }

    /// The value this argument gives the option, and how many arguments it
    /// uses, or `None` when the argument is not this option.
    fn value<'a>(&self, arguments: &'a [String], index: usize) -> Option<(Option<&'a str>, usize)> {
        let word = arguments[index].as_str();
        let forms = self.forms();
        if word == self.option && forms.contains(&Form::Separate) {
            return Some((arguments.get(index + 1).map(String::as_str), 2));
        }
        if let Some(value) = word
            .strip_prefix(self.option.as_str())
            .and_then(|rest| rest.strip_prefix('='))
            && forms.contains(&Form::Equals)
        {
            return Some((Some(value), 1));
        }
        if forms.contains(&Form::Attached)
            && word.len() > self.option.len()
            && let Some(value) = word.strip_prefix(self.option.as_str())
        {
            return Some((Some(value), 1));
        }
        None
    }
}

/// Validate one document's declarations: each well formed, each option
/// declared once per executable.
pub(crate) fn validate(declarations: &[DirectoryOption]) -> Result<(), Error> {
    if declarations.len() > MAX_DECLARATIONS {
        return Err(Error::Limit);
    }
    let mut seen = BTreeSet::new();
    for declaration in declarations {
        declaration.validate()?;
        for name in declaration.executable.names() {
            if !seen.insert((name, &declaration.option)) {
                return Err(Error::Contract("duplicate directory option declaration"));
            }
        }
    }
    Ok(())
}

/// The document that declared a directory option, as a conflict names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Source<'a> {
    /// A selected pack, by its validated namespaced id.
    Pack(&'a str),
    /// The project's custom rules, shared and local layers joined.
    Project,
}

impl std::fmt::Display for Source<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pack(id) => write!(f, "pack {}", bounded(id)),
            Self::Project => f.write_str("the project rules"),
        }
    }
}

/// A name shortened for a message, keeping at most 64 characters.
pub(super) fn bounded(text: &str) -> String {
    const LIMIT: usize = 64;
    if text.chars().count() <= LIMIT {
        text.to_owned()
    } else {
        let mut short: String = text.chars().take(LIMIT - 1).collect();
        short.push('…');
        short
    }
}

/// Combine the declarations of every selected document, one per executable
/// name. The same option of the same executable may be declared by several
/// documents only identically.
///
/// # Errors
/// Rejects two documents declaring one option of one executable differently,
/// naming the option, the executable's file name and both sources.
pub(crate) fn combine<'a>(
    documents: impl IntoIterator<Item = (Source<'a>, &'a [DirectoryOption])>,
) -> Result<Vec<DirectoryOption>, Error> {
    let mut combined: BTreeMap<(String, String), (Source<'a>, DirectoryOption)> = BTreeMap::new();
    for (source, declarations) in documents {
        for declaration in declarations {
            for name in declaration.executable.names() {
                let single = declaration.meaning(name);
                match combined.get(&(name.clone(), declaration.option.clone())) {
                    Some((first, known)) if *known != single => {
                        // The file name alone keeps machine folders out of the message.
                        let tool = name.rsplit(['/', '\\']).next().unwrap_or(name);
                        return Err(Error::DirectoryOptionConflict(format!(
                            "directory option {} of {} is declared differently by {first} and {source}",
                            bounded(&declaration.option),
                            bounded(tool),
                        )));
                    }
                    Some(_) => {}
                    None => {
                        combined
                            .insert((name.clone(), declaration.option.clone()), (source, single));
                    }
                }
            }
        }
    }
    Ok(combined
        .into_values()
        .map(|(_, declaration)| declaration)
        .collect())
}

/// Shared custom declarations with the local layer's joined. A local
/// declaration that means the same as a shared one for an executable adds
/// nothing for it, however either is written; one that differs fails.
///
/// # Errors
/// Rejects a local declaration that differs from the shared one for the same
/// executable and option.
pub(crate) fn join(
    shared: &[DirectoryOption],
    local: &[DirectoryOption],
) -> Result<Vec<DirectoryOption>, Error> {
    let mut joined = shared.to_vec();
    for declaration in local {
        let mut added = Vec::new();
        for name in declaration.executable.names() {
            match shared.iter().find(|known| {
                known.option == declaration.option && known.executable.names().contains(name)
            }) {
                Some(known) if known.meaning(name) != declaration.meaning(name) => {
                    return Err(Error::Contract(
                        "a directory option in the local rules differs from the shared rules' declaration of the same option",
                    ));
                }
                Some(_) => {}
                None => added.push(name.clone()),
            }
        }
        match added.as_slice() {
            [] => {}
            [name] => joined.push(DirectoryOption {
                executable: Executable::Name(name.clone()),
                ..declaration.clone()
            }),
            _ => joined.push(DirectoryOption {
                executable: Executable::OneOf { one_of: added },
                ..declaration.clone()
            }),
        }
    }
    Ok(joined)
}

/// The combined declarations of selected packs and custom rules, as a compiled
/// plan holds them.
///
/// # Errors
/// Rejects conflicting declarations.
pub(crate) fn selected(
    packs: &[(&super::Pack, super::Profile)],
    customs: &[&super::CustomRules],
) -> Result<Vec<DirectoryOption>, Error> {
    combine(
        packs
            .iter()
            .map(|(pack, _)| {
                (
                    Source::Pack(&pack.document.id),
                    pack.document.directory_options.as_slice(),
                )
            })
            .chain(customs.iter().map(|custom| {
                (
                    Source::Project,
                    custom.document.directory_options.as_slice(),
                )
            })),
    )
}

/// The request as the same command without its declared directory option,
/// run from the folder the option chose. `None` leaves the request as
/// submitted: no declared option, an option used in an undeclared way, a value
/// that is missing, not a plain path or unresolvable, or a folder that other
/// relative paths would not follow.
pub(crate) fn normalize(request: &Request, declarations: &[DirectoryOption]) -> Option<Request> {
    let declared: Vec<_> = declarations
        .iter()
        .filter(|declaration| declaration.executable.names().contains(&request.executable))
        .collect();
    if declared.is_empty() {
        return None;
    }
    let arguments = &request.arguments;
    let mut kept = Vec::new();
    let mut found: Vec<(&DirectoryOption, &str)> = Vec::new();
    let mut operand = false;
    let mut index = 0;
    while index < arguments.len() {
        let word = arguments[index].as_str();
        if word == "--" {
            kept.extend(arguments[index..].iter().cloned());
            break;
        }
        let matched = declared.iter().find_map(|declaration| {
            (declaration.position == Position::Anywhere || !operand)
                .then(|| declaration.value(arguments, index))
                .flatten()
                .map(|(value, used)| (*declaration, value, used))
        });
        if let Some((declaration, value, used)) = matched {
            found.push((declaration, value?));
            index += used;
            continue;
        }
        operand |= !word.starts_with('-');
        kept.push(arguments[index].clone());
        index += 1;
    }
    let (declaration, _) = *found.first()?;
    if found.iter().any(|(other, _)| *other != declaration)
        || (found.len() > 1 && declaration.repeat == Repeat::Refuse)
    {
        return None;
    }
    let start = lexical(&request.cwd).filter(|cwd| cwd.is_absolute())?;
    let values: Vec<&str> = if declaration.repeat == Repeat::Last {
        found.last().map(|(_, value)| *value).into_iter().collect()
    } else {
        found.iter().map(|(_, value)| *value).collect()
    };
    let mut folder = start.clone();
    for value in values {
        folder = resolve(&folder, value, declaration)?;
    }
    if declaration.paths == Paths::Unchanged && folder != start {
        return None;
    }
    Some(Request {
        executable: request.executable.clone(),
        arguments: kept,
        cwd: folder.to_str()?.to_owned(),
        project: request.project.clone(),
    })
}

/// The folder a value chooses from `base`. Values the shell left unexpanded,
/// such as `~` or `$NAME`, and values that look like options are not paths.
/// `..` stays in the folder: after a symbolic link it leaves the link's
/// target, not the folder holding the link, so only location checks, which
/// follow links first, can tell where it leads. A file value chooses its
/// folder only when it names a file the tool finds there by default.
fn resolve(base: &Path, value: &str, declaration: &DirectoryOption) -> Option<PathBuf> {
    let file = declaration.value == Names::File;
    if value.is_empty()
        || value.starts_with(['-', '~'])
        || value.contains(['$', '\0', '`'])
        || (file && value.ends_with('/'))
    {
        return None;
    }
    // Joining drops `.` components and keeps `..`.
    let path: PathBuf = base.join(value).components().collect();
    if !file {
        return Some(path);
    }
    let name = path.file_name()?.to_str()?;
    declaration
        .files
        .iter()
        .any(|known| known == name)
        .then(|| path.parent().map(Path::to_path_buf))
        .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn declarations() -> Vec<DirectoryOption> {
        serde_json::from_value(json!([
            {"executable":{"one_of":["npm","/usr/bin/npm"]},"option":"--prefix","position":"anywhere","paths":"follow"},
            {"executable":"git","option":"-C","repeat":"chain","paths":"follow"},
            {"executable":"cargo","option":"--manifest-path","position":"anywhere","value":"file","files":["Cargo.toml"],"paths":"unchanged"},
            {"executable":"make","option":"-C","repeat":"chain","paths":"follow"},
            {"executable":"terraform","option":"-chdir","forms":["equals"],"paths":"follow"}
        ]))
        .unwrap()
    }

    fn request(command: &str, cwd: &str) -> Request {
        let mut words = command.split(' ').map(str::to_owned);
        Request {
            executable: words.next().unwrap(),
            arguments: words.collect(),
            cwd: cwd.into(),
            project: "/srv/acme".into(),
        }
    }

    fn normalized(command: &str, cwd: &str) -> Option<(String, String)> {
        let result = normalize(&request(command, cwd), &declarations())?;
        Some((
            std::iter::once(result.executable)
                .chain(result.arguments)
                .collect::<Vec<_>>()
                .join(" "),
            result.cwd,
        ))
    }

    #[expect(
        clippy::unnecessary_wraps,
        reason = "the expected value of the optional normalization"
    )]
    fn pair(command: &str, cwd: &str) -> Option<(String, String)> {
        Some((command.into(), cwd.into()))
    }

    #[test]
    fn every_form_of_a_directory_option_becomes_the_plain_command_in_that_folder() {
        for (command, cwd) in [
            ("npm --prefix /srv/acme start", "/srv/other"),
            ("npm --prefix=/srv/acme start", "/srv/other"),
            ("npm --prefix . start", "/srv/acme"),
            ("npm --prefix ./ start", "/srv/acme"),
        ] {
            assert_eq!(
                normalized(command, cwd),
                pair("npm start", "/srv/acme"),
                "{command}"
            );
        }
        // `..` is left for location checks, which follow symbolic links first.
        assert_eq!(
            normalized("npm start --prefix ../acme", "/srv/other"),
            pair("npm start", "/srv/other/../acme")
        );
        assert_eq!(
            normalized("npm --prefix /srv/acme/web/.. start", "/"),
            pair("npm start", "/srv/acme/web/..")
        );
        assert_eq!(
            normalized("/usr/bin/npm --prefix /srv/acme start", "/"),
            pair("/usr/bin/npm start", "/srv/acme")
        );
        assert_eq!(
            normalized("git -C ../acme status", "/srv/other"),
            pair("git status", "/srv/other/../acme")
        );
        assert_eq!(
            normalized("git -C/srv/acme status", "/"),
            pair("git status", "/srv/acme")
        );
        assert_eq!(
            normalized("terraform -chdir=infra plan", "/srv/acme"),
            pair("terraform plan", "/srv/acme/infra")
        );
    }

    #[test]
    fn repeated_options_follow_their_declared_rule() {
        assert_eq!(
            normalized("git -C /srv -C acme status", "/"),
            pair("git status", "/srv/acme")
        );
        assert_eq!(normalized("npm --prefix a --prefix b start", "/srv"), None);
    }

    #[test]
    fn a_folder_other_paths_do_not_follow_is_normalized_only_in_place() {
        assert_eq!(
            normalized(
                "cargo build --manifest-path /srv/acme/Cargo.toml",
                "/srv/acme"
            ),
            pair("cargo build", "/srv/acme")
        );
        assert_eq!(
            normalized("cargo build --manifest-path Cargo.toml", "/srv/acme"),
            pair("cargo build", "/srv/acme")
        );
        assert_eq!(
            normalized("cargo build --manifest-path /srv/acme/Cargo.toml", "/srv"),
            None
        );
        assert_eq!(
            normalized("cargo build --manifest-path /srv/acme/", "/srv/acme"),
            None
        );
        // Another manifest is another project, not this folder's build.
        assert_eq!(
            normalized("cargo build --manifest-path other.toml", "/srv/acme"),
            None
        );
    }

    #[test]
    fn undeclared_or_unclear_uses_stay_as_submitted() {
        for (command, cwd) in [
            ("npm start", "/srv/acme"),
            ("pnpm -C /srv/acme start", "/"),
            ("npm --prefix", "/srv/acme"),
            ("npm --prefix ~/acme start", "/srv"),
            ("npm --prefix $ACME start", "/srv"),
            ("npm --prefix --global start", "/srv"),
            ("npm run acme -- --prefix /srv/acme", "/srv"),
            ("terraform -chdir infra plan", "/srv/acme"),
            ("npm --prefix /srv/acme start", "relative"),
        ] {
            assert_eq!(normalized(command, cwd), None, "{command}");
        }
        // A leading option after the subcommand belongs to the subcommand.
        assert_eq!(normalized("git log -C acme", "/srv"), None);
        assert_eq!(normalized("make build -C acme", "/srv"), None);
    }

    #[test]
    fn declarations_validate_and_combine_only_when_they_agree() {
        assert!(validate(&declarations()).is_ok());
        let invalid = |value: serde_json::Value| {
            serde_json::from_value::<Vec<DirectoryOption>>(value)
                .map_err(|_| ())
                .and_then(|declarations| validate(&declarations).map_err(|_| ()))
                .is_err()
        };
        assert!(invalid(
            json!([{"executable":"npm","option":"prefix","paths":"follow"}])
        ));
        assert!(invalid(
            json!([{"executable":"npm","option":"--prefix","forms":["attached"],"paths":"follow"}])
        ));
        assert!(invalid(json!([{"executable":"npm","option":"--prefix"}])));
        for files in [
            json!(null),
            json!([]),
            json!(["dir/Cargo.toml"]),
            json!([".."]),
        ] {
            let mut declaration = json!({"executable":"cargo","option":"--manifest-path","value":"file","paths":"unchanged"});
            if !files.is_null() {
                declaration["files"] = files.clone();
            }
            assert!(invalid(json!([declaration])), "{files}");
        }
        assert!(invalid(
            json!([{"executable":"npm","option":"--prefix","files":["package.json"],"paths":"follow"}])
        ));
        assert!(invalid(json!([
            {"executable":"npm","option":"--prefix","paths":"follow"},
            {"executable":"npm","option":"--prefix","paths":"follow"}
        ])));
        assert!(invalid(json!([
            {"executable":{"one_of":["npm","/usr/bin/npm"]},"option":"--prefix","paths":"follow"},
            {"executable":"npm","option":"--prefix","paths":"follow"}
        ])));
        // One declaration per executable name: the pack's two npm names and
        // the four single names.
        let pack = declarations();
        let same: Vec<DirectoryOption> = serde_json::from_value(json!([
            {"executable":"npm","option":"--prefix","position":"anywhere","paths":"follow"}
        ]))
        .unwrap();
        let acme = Source::Pack("acme/tools");
        assert_eq!(
            combine([(acme, &pack[..]), (Source::Project, &same[..])])
                .unwrap()
                .len(),
            pack.len() + 1
        );
        let mut different = same[0].clone();
        different.paths = Paths::Unchanged;
        assert!(
            combine([
                (acme, &pack[..]),
                (Source::Project, std::slice::from_ref(&different))
            ])
            .is_err()
        );
        // Defaults written out mean the same as defaults left out.
        let spelled: Vec<DirectoryOption> = serde_json::from_value(json!([
            {"executable":"npm","option":"--prefix","position":"anywhere","forms":["equals","separate"],"value":"directory","repeat":"refuse","paths":"follow"}
        ]))
        .unwrap();
        assert_eq!(
            combine([(acme, &pack[..]), (Source::Project, &spelled[..])])
                .unwrap()
                .len(),
            pack.len() + 1
        );
    }

    #[test]
    fn a_conflict_names_the_option_and_both_sources() {
        let parse = |value: serde_json::Value| -> Vec<DirectoryOption> {
            serde_json::from_value(value).unwrap()
        };
        let follow = parse(json!([
            {"executable":{"one_of":["npm","/opt/acme/bin/npm"]},"option":"--prefix","paths":"follow"}
        ]));
        let unchanged = parse(json!([
            {"executable":"/opt/acme/bin/npm","option":"--prefix","paths":"unchanged"}
        ]));
        let message = |documents: [(Source<'_>, &[DirectoryOption]); 2]| match combine(documents) {
            Err(Error::DirectoryOptionConflict(message)) => message,
            other => panic!("expected a conflict, got {other:?}"),
        };
        // Two packs: each named by its id, in selection order.
        assert_eq!(
            message([
                (Source::Pack("acme/node"), &follow),
                (Source::Pack("acme/web"), &unchanged),
            ]),
            "directory option --prefix of npm is declared differently by pack acme/node and pack acme/web"
        );
        // A pack and the project rules; the executable's folder stays out.
        let text = message([
            (Source::Pack("acme/node"), &follow),
            (Source::Project, &unchanged),
        ]);
        assert_eq!(
            text,
            "directory option --prefix of npm is declared differently by pack acme/node and the project rules"
        );
        assert!(!text.contains("/opt"));
        // Long names are shortened.
        let long = "acme/".to_owned() + &"x".repeat(200);
        let text = message([
            (Source::Pack(&long), &follow),
            (Source::Project, &unchanged),
        ]);
        assert!(text.len() < 200, "{text}");
        assert!(text.ends_with("… and the project rules"), "{text}");
    }

    #[test]
    fn local_declarations_join_shared_ones_by_meaning() {
        let parse = |value: serde_json::Value| -> Vec<DirectoryOption> {
            serde_json::from_value(value).unwrap()
        };
        let shared = parse(json!([
            {"executable":{"one_of":["npm","pnpm"]},"option":"--prefix","position":"anywhere","paths":"follow"}
        ]));
        // The same meaning, written another way, for one of the names.
        let repeat = parse(json!([
            {"executable":"npm","option":"--prefix","position":"anywhere","forms":["separate","equals"],"repeat":"refuse","paths":"follow"}
        ]));
        assert_eq!(join(&shared, &repeat).unwrap(), shared);
        // Names the shared declaration lacks are added alone.
        let more = parse(json!([
            {"executable":{"one_of":["npm","yarn"]},"option":"--prefix","position":"anywhere","paths":"follow"}
        ]));
        let joined = join(&shared, &more).unwrap();
        assert_eq!(joined.len(), 2);
        assert_eq!(joined[1].executable, Executable::Name("yarn".into()));
        let leading = parse(json!([
            {"executable":"pnpm","option":"--prefix","position":"leading","paths":"follow"}
        ]));
        assert!(join(&shared, &leading).is_err());
        validate(&joined).unwrap();
    }
}
