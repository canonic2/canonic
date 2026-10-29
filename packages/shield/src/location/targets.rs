//! Fixed native operation models. Unknown flags and traversal retain partial coverage.
use super::{Descendants, Target, Targets};
use crate::policy::normalized::Input;
use std::{fs, path::Path};

/// `alone` reports that the request runs no other operation, which lets a
/// missing in-place input count as absent; see [`super::text::collect`].
pub(super) fn collect(input: &Input, cwd: &Path, alone: bool) -> Targets {
    match input {
        Input::File {
            action,
            sources,
            destination,
            recursive,
            ..
        } => resources(
            action,
            sources,
            destination.as_deref(),
            *recursive,
            false,
            false,
            cwd,
        ),
        Input::Argv {
            executable,
            arguments,
            ..
        } => argv(executable, arguments, cwd, alone),
        _ => Targets {
            items: vec![],
            complete: false,
        },
    }
}

fn argv(executable: &str, arguments: &[String], cwd: &Path, alone: bool) -> Targets {
    if let Some(file) = super::node_check_file(
        executable,
        &arguments.iter().map(String::as_str).collect::<Vec<_>>(),
    ) {
        return resources("read", &[file.into()], None, false, false, false, cwd);
    }
    if let Some(targets) = super::text::collect(executable, arguments, cwd, alone) {
        return targets;
    }
    let path = Path::new(executable);
    let leaf = if [
        "rm", "rmdir", "cp", "mv", "cat", "head", "tail", "wc", "ls", "grep", "rg", "find",
        "mkdir", "touch", "tee", "ln", "chmod", "chown",
    ]
    .contains(&executable)
    {
        executable
    } else if path.is_absolute()
        && [Path::new("/bin"), Path::new("/usr/bin")]
            .contains(&path.parent().unwrap_or(Path::new("")))
    {
        path.file_name()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
    } else {
        ""
    };
    if ["cat", "head", "tail", "wc", "ls"].contains(&leaf) {
        return read_argv(leaf, arguments, cwd);
    }
    if leaf == "rg" {
        return rg_argv(arguments, cwd);
    }
    if leaf == "grep" {
        return grep_argv(arguments, cwd);
    }
    if leaf == "find" {
        return find_argv(arguments, cwd);
    }
    if ["mkdir", "touch"].contains(&leaf) {
        return write_argv(leaf, arguments, cwd);
    }
    if let Some(targets) = super::writers::collect(leaf, arguments, cwd) {
        return targets;
    }
    destructive_argv(leaf, arguments, cwd)
}

fn destructive_argv(leaf: &str, arguments: &[String], cwd: &Path) -> Targets {
    let action = match leaf {
        "rm" | "rmdir" => "remove",
        "cp" => "copy",
        "mv" => "move",
        _ => {
            return Targets {
                items: vec![],
                complete: false,
            };
        }
    };
    let mut operands = Vec::new();
    let mut options = true;
    let mut recursive = false;
    let mut no_directory = false;
    let mut target_directory = None;
    let mut complete = cfg!(unix);
    let mut forced_copy = false;
    let mut position = 0;
    while position < arguments.len() {
        let argument = &arguments[position];
        if options && argument == "--" {
            options = false;
        } else if options && argument.starts_with('-') && argument != "-" {
            if cfg!(target_os = "linux")
                && ["--no-target-directory", "-T"].contains(&argument.as_str())
            {
                no_directory = true;
            } else if cfg!(target_os = "linux")
                && ["--target-directory", "-t"].contains(&argument.as_str())
            {
                position += 1;
                target_directory = arguments.get(position).cloned();
                complete &= target_directory.is_some();
            } else if cfg!(target_os = "linux") && argument.starts_with("--target-directory=") {
                target_directory = argument
                    .strip_prefix("--target-directory=")
                    .map(str::to_owned);
            } else if argument.starts_with('-') && !argument.starts_with("--") {
                for flag in argument[1..].bytes() {
                    match flag {
                        b'r' | b'R' if leaf != "mv" && leaf != "rmdir" => recursive = true,
                        b'f' if leaf != "rmdir" => forced_copy |= leaf == "cp",
                        b'i' | b'v' if leaf != "rmdir" => {}
                        b'n' if leaf == "cp" || leaf == "mv" => {}
                        _ => complete = false,
                    }
                }
            } else if cfg!(target_os = "linux") {
                match argument.as_str() {
                    "--recursive" if leaf == "rm" || leaf == "cp" => recursive = true,
                    "--force" if leaf != "rmdir" => forced_copy |= leaf == "cp",
                    "--verbose" | "--interactive" if leaf != "rmdir" => {}
                    "--preserve-root" | "--no-preserve-root" if leaf == "rm" => {}
                    _ => complete = false,
                }
            } else {
                complete = false;
            }
        } else {
            operands.push(argument.clone());
        }
        position += 1;
    }
    let destination = if action == "remove" {
        None
    } else {
        target_directory.or_else(|| operands.pop())
    };
    let mut result = resources(
        action,
        &operands,
        destination.as_deref(),
        recursive,
        no_directory,
        forced_copy,
        cwd,
    );
    result.complete &= complete;
    result
}

// Explicit portable read forms. No recursive listing, monitoring, option-file
// loading or helper execution is inferred from an unfamiliar option.
fn read_argv(leaf: &str, arguments: &[String], cwd: &Path) -> Targets {
    let mut operands = Vec::new();
    let mut options = true;
    let mut complete = cfg!(unix);
    let mut position = 0;
    while position < arguments.len() {
        let argument = &arguments[position];
        if options && argument == "--" {
            options = false;
        } else if position == 0
            && ["head", "tail"].contains(&leaf)
            && argument.len() > 1
            && argument.starts_with('-')
            && argument.bytes().skip(1).all(|b| b.is_ascii_digit())
        {
            // The historical `head -50` count, accepted only as the first argument.
        } else if options && argument.starts_with('-') && argument != "-" {
            let flags = match leaf {
                "cat" => "benstuvET",
                "head" | "tail" => "qv",
                "wc" => "clmw",
                "ls" => "aAdFhilmnpqrsSt1",
                _ => "",
            };
            complete &= !argument.starts_with("--");
            let mut short = argument.as_bytes()[1..].iter();
            while let Some(&flag) = short.next() {
                if ["head", "tail"].contains(&leaf) && b"nc".contains(&flag) {
                    let attached = short.as_slice();
                    let count = if attached.is_empty() {
                        position += 1;
                        arguments.get(position).map(String::as_bytes)
                    } else {
                        Some(attached)
                    };
                    // tail also counts from the start with `+N`.
                    let count = count.map(|v| match v {
                        [b'+', rest @ ..] if leaf == "tail" => rest,
                        _ => v,
                    });
                    complete &=
                        count.is_some_and(|v| !v.is_empty() && v.iter().all(u8::is_ascii_digit));
                    break;
                }
                complete &= flags.as_bytes().contains(&flag);
            }
        } else if argument != "-" || leaf == "ls" {
            operands.push(argument.clone());
        }
        position += 1;
    }
    if leaf == "ls" && operands.is_empty() {
        operands.push(".".into());
    }
    Targets {
        items: operands
            .into_iter()
            .map(|path| Target {
                action: "read",
                role: "source",
                path: cwd.join(path),
                recursive: false,
                descendants: Descendants::All,
                follow: true,
            })
            .collect(),
        complete,
    }
}

// grep reads explicit regular files; explicit operands follow their leaf links.
fn grep_argv(arguments: &[String], cwd: &Path) -> Targets {
    let mut operands = Vec::new();
    let mut options = true;
    let mut terminated = false;
    let mut complete = cfg!(unix);
    for argument in arguments {
        if options && argument == "--" {
            options = false;
            terminated = true;
        } else if options && argument.starts_with('-') && argument != "-" {
            complete &= portable_flags(argument, "EFGinlcqvwxHh");
        } else {
            complete &= terminated || !argument.starts_with('-') || argument == "-";
            options = false;
            operands.push(argument);
        }
    }
    complete &= operands.len() >= 2;
    let items = operands
        .into_iter()
        .skip(1)
        .map(|operand| {
            let path = cwd.join(operand);
            let metadata = fs::metadata(&path);
            complete &= operand != "-" && metadata.is_ok_and(|m| m.is_file());
            Target {
                action: "read",
                role: "source",
                path,
                recursive: false,
                descendants: Descendants::Visible,
                follow: true,
            }
        })
        .collect();
    Targets { items, complete }
}

// Ambient ripgrep configuration is trusted; --no-config is optional. Explicit
// helper and link-following flags remain outside this model.
// Ignore-file negations and -g globs can include hidden entries, so
// their footprints conservatively include every existing descendant. --files
// lists names only: without --no-ignore, every existing descendant counts, so
// an existing excluded directory still blocks it. Without a path, ripgrep walks
// the working directory. Explicit --hidden is modeled only for filename listings.
// Types (-t) select after hidden entries are skipped in content searches.
fn rg_argv(arguments: &[String], cwd: &Path) -> Targets {
    rg_model(arguments, cwd).0
}

/// A modeled content search naming only its pattern, which ripgrep runs on
/// its standard input when that is a pipe instead of walking the directory.
pub(super) fn rg_pattern_only(arguments: &[String], cwd: &Path) -> bool {
    let (targets, listing) = rg_model(arguments, cwd);
    targets.complete && !listing && rg_operands(arguments) == Some(1)
}

/// Operands after ripgrep's modeled options, or None when an option is not
/// modeled. Valued options consume their value, as in `rg_model`.
fn rg_operands(arguments: &[String]) -> Option<usize> {
    const VALUED: &str = "-g -t -T -C -A -B -m -d --glob --type --type-not --context         --after-context --before-context --max-count --max-depth";
    let mut arguments = arguments.iter();
    while let Some(argument) = arguments.next() {
        if argument == "--" {
            return Some(arguments.count());
        }
        if argument == "-" || !argument.starts_with('-') {
            return Some(1 + arguments.count());
        }
        if VALUED.split_whitespace().any(|item| item == argument) {
            arguments.next()?;
        }
    }
    Some(0)
}

pub(super) fn rg_model(arguments: &[String], cwd: &Path) -> (Targets, bool) {
    const FLAGS: &str = "inlcqFwxSH";
    const LONG: &str = "--no-config --no-ignore --no-follow --no-hidden --files --smart-case         --ignore-case --line-number --files-with-matches --count --fixed-strings --word-regexp         --no-heading --with-filename";
    const VALUED: &str = "-g -t -T -C -A -B -m -d --glob --type --type-not --context         --after-context --before-context --max-count --max-depth";
    const SELECTING: &str = "-g --glob";
    let listed = |list: &str, word: &str| list.split_whitespace().any(|item| item == word);
    let mut operands = Vec::new();
    let mut options = true;
    let mut terminated = false;
    let mut no_ignore = false;
    let mut files = false;
    let mut hidden = false;
    // Glob and type selections override ripgrep's other filters, hidden ones too.
    let mut selects = false;
    let mut complete = cfg!(unix);
    let mut arguments = arguments.iter();
    while let Some(argument) = arguments.next() {
        let long = argument
            .split_once('=')
            .map_or(argument.as_str(), |(name, _)| name);
        // Options precede operands; a later option-like word is unmodeled.
        if !options {
            complete &= terminated || !argument.starts_with('-');
            operands.push(argument);
        } else if argument == "--" {
            options = false;
            terminated = true;
        } else if argument == "-" || !argument.starts_with('-') {
            options = false;
            operands.push(argument);
        } else if argument == "--hidden" {
            hidden = true;
        } else if listed(LONG, argument) {
            no_ignore |= argument == "--no-ignore";
            files |= argument == "--files";
        } else if listed(VALUED, argument) {
            selects |= listed(SELECTING, argument);
            complete &= arguments.next().is_some();
        } else if argument.starts_with("--") && listed(VALUED, long) && long != argument {
            selects |= listed(SELECTING, long);
        } else if !argument.starts_with("--")
            && argument.len() > 2
            && listed(VALUED, &argument[..2])
        {
            selects |= listed(SELECTING, &argument[..2]);
        } else {
            complete &= portable_flags(argument, FLAGS);
        }
    }
    complete &= !hidden || files;
    let paths: Vec<&String> = if files {
        operands
    } else {
        complete &= !operands.is_empty();
        operands.into_iter().skip(1).collect()
    };
    let current = ".".to_owned();
    let paths = if paths.is_empty() {
        vec![&current]
    } else {
        paths
    };
    let items = paths
        .into_iter()
        .map(|operand| {
            let path = cwd.join(operand);
            let metadata = fs::metadata(&path);
            let directory = metadata.as_ref().is_ok_and(std::fs::Metadata::is_dir);
            complete &= operand != "-" && (directory || metadata.is_ok_and(|m| m.is_file()));
            Target {
                action: "read",
                role: "source",
                path,
                recursive: directory,
                descendants: if no_ignore && !selects && !hidden {
                    Descendants::Visible
                } else {
                    Descendants::Present
                },
                follow: true,
            }
        })
        .collect();
    (Targets { items, complete }, files)
}

// find's default physical walk lists existing entries without following
// descendant links. Start points follow their own links, which can only widen
// the checked footprint. Actions, output files, link-following options and
// reference-file tests are unmodeled.
fn find_argv(arguments: &[String], cwd: &Path) -> Targets {
    const NULLARY: &str = "! ( ) , -a -and -o -or -not -print -print0 -prune -empty -true -false -depth \
        -xdev -mount -quit -nouser -nogroup -readable -writable -executable -ls -noleaf -daystart \
        -nowarn -warn";
    const VALUED: &str = "-name -iname -path -ipath -wholename -iwholename -regex -iregex -lname -ilname \
        -type -maxdepth -mindepth -size -mtime -mmin -atime -amin -ctime -cmin -Btime -Bmin -perm \
        -user -group -uid -gid -links -inum -regextype -fstype -printf -flags";
    let mut complete = cfg!(unix);
    let mut rest = arguments.iter().peekable();
    while let Some(argument) = rest.next_if(|a| {
        a.as_str() == "--"
            || (a.len() > 1
                && a.starts_with('-')
                && a[1..].bytes().all(|b| b"EHLPXdsx".contains(&b)))
    }) {
        complete &= !argument.contains(['H', 'L']);
    }
    let mut starts = Vec::new();
    while let Some(start) =
        rest.next_if(|a| !a.starts_with('-') && !["!", "(", ")", ","].contains(&a.as_str()))
    {
        starts.push(start.as_str());
    }
    let listed = |list: &str, argument: &str| list.split_whitespace().any(|p| p == argument);
    while let Some(argument) = rest.next() {
        if listed(VALUED, argument) {
            complete &= rest.next().is_some();
        } else {
            complete &= listed(NULLARY, argument);
        }
    }
    if starts.is_empty() {
        starts.push(".");
    }
    Targets {
        items: starts
            .into_iter()
            .map(|start| Target {
                action: "read",
                role: "source",
                path: cwd.join(start),
                recursive: true,
                descendants: Descendants::Present,
                follow: true,
            })
            .collect(),
        complete,
    }
}

// mkdir's missing parents stay below its resolved operand; touch follows leaf
// links. Options loading dates/reference files or changing modes are unmodeled.
fn write_argv(leaf: &str, arguments: &[String], cwd: &Path) -> Targets {
    let mut items = Vec::new();
    let mut options = true;
    let mut terminated = false;
    let mut complete = cfg!(unix);
    for argument in arguments {
        if options && argument == "--" {
            options = false;
            terminated = true;
        } else if options && argument.starts_with('-') && argument != "-" {
            complete &= portable_flags(argument, if leaf == "mkdir" { "p" } else { "amc" });
        } else {
            complete &= terminated || !argument.starts_with('-') || argument == "-";
            options = false;
            complete &= !(leaf == "touch" && argument == "-"
                || leaf == "mkdir"
                    && Path::new(argument)
                        .components()
                        .any(|c| c == std::path::Component::ParentDir));
            items.push(Target {
                action: "write",
                role: "destination",
                path: cwd.join(argument),
                recursive: false,
                descendants: Descendants::All,
                follow: true,
            });
        }
    }
    complete &= !items.is_empty();
    Targets { items, complete }
}

fn portable_flags(argument: &str, flags: &str) -> bool {
    !argument.starts_with("--")
        && argument[1..]
            .bytes()
            .all(|flag| flags.as_bytes().contains(&flag))
}

fn resources(
    action: &str,
    sources: &[String],
    destination: Option<&str>,
    recursive: bool,
    no_directory: bool,
    forced_copy: bool,
    cwd: &Path,
) -> Targets {
    let mut result = Targets {
        items: vec![],
        complete: cfg!(unix),
    };
    let destination = destination.map(|p| cwd.join(p));
    let directory = destination
        .as_ref()
        .is_some_and(|p| !no_directory && fs::metadata(p).is_ok_and(|m| m.is_dir()));
    if ["copy", "move"].contains(&action)
        && (sources.is_empty() || (sources.len() > 1 && !directory))
    {
        result.complete = false;
    }
    for source in sources {
        let source = cwd.join(source);
        let remove = ["remove", "move"].contains(&action);
        let metadata = if remove {
            fs::symlink_metadata(&source)
        } else {
            fs::metadata(&source)
        };
        let directory_source = metadata.as_ref().is_ok_and(std::fs::Metadata::is_dir);
        let footprint = recursive || (action == "move" && directory_source);
        result.items.push(Target {
            action: if remove { "remove" } else { "read" },
            role: "source",
            path: source.clone(),
            recursive: footprint,
            // A read reaches only entries that exist; removal and moves also
            // cover protected names their footprint could hold.
            descendants: if action == "read" {
                Descendants::Present
            } else {
                Descendants::All
            },
            follow: !remove,
        });
        // A recursive copy can follow links below the root. Proving that graph
        // would require traversal; ordinary hooks retain it as unknown.
        if action == "copy" && (recursive || directory_source) {
            result.complete = false;
        }
        if source.to_string_lossy().ends_with('/')
            && metadata.as_ref().is_ok_and(|m| m.file_type().is_symlink())
        {
            result.complete = false;
        }
        if let Some(destination) = &destination {
            let effective = if directory {
                if let Some(name) = source.file_name() {
                    destination.join(name)
                } else {
                    result.complete = false;
                    destination.clone()
                }
            } else {
                destination.clone()
            };
            if (action == "move" || forced_copy) && fs::symlink_metadata(&effective).is_ok() {
                let subtree = fs::symlink_metadata(&effective).is_ok_and(|m| m.is_dir());
                result.items.push(Target {
                    action: "remove",
                    role: "replacement",
                    path: effective.clone(),
                    recursive: subtree,
                    descendants: Descendants::All,
                    follow: false,
                });
            }
            result.items.push(Target {
                action: "write",
                role: "destination",
                path: effective,
                recursive: footprint,
                descendants: Descendants::All,
                follow: action != "move",
            });
        }
    }
    if action == "write" {
        if let Some(destination) = destination {
            result.items.push(Target {
                action: "write",
                role: "destination",
                path: destination,
                recursive: false,
                descendants: Descendants::All,
                follow: true,
            });
        } else {
            result.complete = false;
        }
    }
    result
}
