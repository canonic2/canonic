//! Fixed native models for commands that write named files or change their
//! metadata: tee's output files, ln's links and chmod/chown's mode and owner
//! changes. Unknown options and unresolved operands retain partial coverage.
use super::{Descendants, Target, Targets};
use std::{fs, path::Path};

/// The model for a leaf this module owns, or None for another command.
pub(super) fn collect(leaf: &str, arguments: &[String], cwd: &Path) -> Option<Targets> {
    match leaf {
        "tee" => Some(tee(arguments, cwd)),
        "ln" => Some(link(arguments, cwd)),
        "chmod" | "chown" => Some(change(leaf, arguments, cwd)),
        _ => None,
    }
}

fn write(path: std::path::PathBuf, recursive: bool, follow: bool) -> Target {
    Target {
        action: "write",
        role: "destination",
        path,
        recursive,
        descendants: Descendants::All,
        follow,
    }
}

/// Options before operands, then operands; a later option-like word needs `--`.
struct Words<'a> {
    flags: Vec<u8>,
    operands: Vec<&'a String>,
    complete: bool,
}

fn words<'a>(arguments: &'a [String], flags: &str, mode: bool) -> Words<'a> {
    let mut parsed = Words {
        flags: Vec::new(),
        operands: Vec::new(),
        complete: cfg!(unix),
    };
    let mut options = true;
    let mut terminated = false;
    for argument in arguments {
        // chmod's leading mode, as in `chmod -x FILE`, is its first operand.
        let dash_mode = mode && parsed.operands.is_empty() && symbolic_mode(argument);
        if options && argument == "--" {
            options = false;
            terminated = true;
        } else if options && argument.starts_with('-') && argument != "-" && !dash_mode {
            parsed.complete &= !argument.starts_with("--")
                && argument[1..]
                    .bytes()
                    .all(|flag| flags.as_bytes().contains(&flag));
            parsed.flags.extend(argument[1..].bytes());
        } else {
            parsed.complete &=
                terminated || dash_mode || !argument.starts_with('-') || argument == "-";
            parsed.operands.push(argument);
            options &= dash_mode;
        }
    }
    parsed
}

/// A symbolic chmod mode such as `-x`, `u+rw,go-w` or `a=r`.
fn symbolic_mode(argument: &str) -> bool {
    argument.split(',').all(|clause| {
        let actions = clause.trim_start_matches(['u', 'g', 'o', 'a']);
        !actions.is_empty()
            && actions.starts_with(['+', '-', '='])
            && actions.bytes().all(|b| b"+-=rwxXstugo".contains(&b))
    })
}

/// tee writes each named file and copies its input to standard output.
fn tee(arguments: &[String], cwd: &Path) -> Targets {
    let parsed = words(arguments, "ai", false);
    let mut complete = parsed.complete;
    let items = parsed
        .operands
        .iter()
        .map(|operand| {
            complete &= operand.as_str() != "-";
            write(cwd.join(operand), false, true)
        })
        .collect();
    Targets { items, complete }
}

/// ln creates links: `ln SOURCE`, `ln SOURCE DEST` or `ln SOURCE... DIRECTORY`.
/// A hard link shares its source's contents, so the source is read; a symbolic
/// link stores only text, and its later readers resolve it themselves. `-f`
/// replaces an existing entry, so its removal is checked whether or not one
/// exists now.
fn link(arguments: &[String], cwd: &Path) -> Targets {
    let parsed = words(arguments, "sfnhvi", false);
    let mut complete = parsed.complete && !parsed.operands.is_empty();
    let symbolic = parsed.flags.contains(&b's');
    let replace = parsed.flags.contains(&b'f');
    let no_dereference = parsed.flags.iter().any(|flag| b"nh".contains(flag));
    let (sources, destination) = match parsed.operands.as_slice() {
        [] => (&[][..], None),
        [source] => (std::slice::from_ref(source), None),
        [sources @ .., last] => (sources, Some(cwd.join(last))),
    };
    let directory = match &destination {
        None => Some(cwd.to_path_buf()),
        Some(path) => {
            let metadata = if no_dereference {
                fs::symlink_metadata(path)
            } else {
                fs::metadata(path)
            };
            metadata.is_ok_and(|m| m.is_dir()).then(|| path.clone())
        }
    };
    complete &= sources.len() == 1 || directory.is_some();
    let mut items = Vec::new();
    for source in sources {
        if !symbolic {
            items.push(Target {
                action: "read",
                role: "source",
                path: cwd.join(source),
                recursive: false,
                descendants: Descendants::All,
                follow: true,
            });
        }
        let effective = match (&directory, &destination) {
            (Some(folder), _) => {
                let Some(name) = Path::new(source.as_str()).file_name() else {
                    complete = false;
                    continue;
                };
                folder.join(name)
            }
            (None, Some(path)) => path.clone(),
            (None, None) => continue,
        };
        if replace {
            items.push(Target {
                action: "remove",
                role: "replacement",
                path: effective.clone(),
                recursive: false,
                descendants: Descendants::All,
                follow: false,
            });
        }
        items.push(write(effective, false, false));
    }
    Targets { items, complete }
}

/// `chmod MODE FILE...` and `chown OWNER[:GROUP] FILE...` change each file's
/// metadata, and with -R everything below it. Reference files and
/// link-following traversal options are not modeled.
fn change(leaf: &str, arguments: &[String], cwd: &Path) -> Targets {
    let parsed = words(arguments, "Rfvh", leaf == "chmod");
    let mut complete = parsed.complete && parsed.operands.len() >= 2;
    let recursive = parsed.flags.contains(&b'R');
    let follow = !parsed.flags.contains(&b'h');
    let mut operands = parsed.operands.iter();
    if let Some(spec) = operands.next() {
        complete &= if leaf == "chmod" {
            (!spec.is_empty()
                && spec.bytes().all(|b| (b'0'..=b'7').contains(&b))
                && spec.len() <= 4)
                || symbolic_mode(spec)
        } else {
            !spec.is_empty()
                && spec
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_.:-".contains(&b))
        };
    }
    let items = operands
        .map(|operand| {
            complete &= operand.as_str() != "-";
            write(cwd.join(operand), recursive, follow)
        })
        .collect();
    Targets { items, complete }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arguments(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| (*w).to_owned()).collect()
    }

    fn found(leaf: &str, words: &[&str], cwd: &Path) -> (Vec<(&'static str, String, bool)>, bool) {
        let targets = collect(leaf, &arguments(words), cwd).unwrap();
        let items = targets
            .items
            .iter()
            .map(|t| {
                (
                    t.action,
                    t.path.strip_prefix(cwd).unwrap().display().to_string(),
                    t.recursive,
                )
            })
            .collect();
        (items, targets.complete)
    }

    #[test]
    fn tee_writes_each_named_file() {
        let cwd = Path::new("/acme");
        assert_eq!(
            found("tee", &["-a", "log.txt", "copy.txt"], cwd),
            (
                vec![
                    ("write", "log.txt".into(), false),
                    ("write", "copy.txt".into(), false)
                ],
                true
            )
        );
        assert_eq!(found("tee", &[], cwd), (vec![], true));
        for words in [
            &["-p", "out"][..],
            &["--append", "out"],
            &["-"],
            &["out", "-a"],
        ] {
            assert!(!found("tee", words, cwd).1, "{words:?}");
        }
    }

    #[test]
    fn hard_links_read_their_source_and_symbolic_links_do_not() {
        let root = assert_fs::TempDir::new().unwrap();
        let cwd = root.path();
        fs::create_dir(cwd.join("dir")).unwrap();
        fs::write(cwd.join("old"), "Acme").unwrap();
        assert_eq!(
            found("ln", &["a.txt", "b.txt"], cwd),
            (
                vec![
                    ("read", "a.txt".into(), false),
                    ("write", "b.txt".into(), false)
                ],
                true
            )
        );
        assert_eq!(
            found("ln", &["-s", "../outside/a", "b"], cwd),
            (vec![("write", "b".into(), false)], true)
        );
        // A directory destination receives each source's name; one operand links into cwd.
        assert_eq!(
            found("ln", &["-s", "x/a", "y/b", "dir"], cwd),
            (
                vec![
                    ("write", "dir/a".into(), false),
                    ("write", "dir/b".into(), false)
                ],
                true
            )
        );
        assert_eq!(
            found("ln", &["-s", "x/a"], cwd),
            (vec![("write", "a".into(), false)], true)
        );
        // -f replaces an existing entry; its removal is checked even before one exists.
        for destination in ["old", "new"] {
            assert_eq!(
                found("ln", &["-sf", "a", destination], cwd),
                (
                    vec![
                        ("remove", destination.into(), false),
                        ("write", destination.into(), false)
                    ],
                    true
                )
            );
        }
        for words in [
            &[][..],
            &["a", "b", "missing"],
            &["-F", "a", "b"],
            &["-s", "/", "dir"],
        ] {
            assert!(!found("ln", words, cwd).1, "{words:?}");
        }
    }

    #[test]
    fn mode_and_owner_changes_write_each_target_and_recursive_trees() {
        let cwd = Path::new("/acme");
        for words in [
            &["+x", "run.sh"][..],
            &["-x", "run.sh"],
            &["u+rw,go-w", "run.sh"],
            &["755", "run.sh"],
        ] {
            assert_eq!(
                found("chmod", words, cwd),
                (vec![("write", "run.sh".into(), false)], true),
                "{words:?}"
            );
        }
        assert_eq!(
            found("chmod", &["-R", "u+w", "build", "dist"], cwd),
            (
                vec![
                    ("write", "build".into(), true),
                    ("write", "dist".into(), true)
                ],
                true
            )
        );
        assert_eq!(
            found("chown", &["-R", "acme:staff", "build"], cwd),
            (vec![("write", "build".into(), true)], true)
        );
        for (leaf, words) in [
            ("chmod", &["+x"][..]),
            ("chmod", &["--reference=a", "b"]),
            ("chmod", &["-R", "-L", "u+w", "build"]),
            ("chmod", &["8", "run.sh"]),
            ("chmod", &["u+z", "run.sh"]),
            ("chown", &["acme;id", "run.sh"]),
            ("chown", &["acme", "-"]),
        ] {
            assert!(!found(leaf, words, cwd).1, "{leaf} {words:?}");
        }
    }
}
