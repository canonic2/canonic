//! Fixed native operation models. Unknown flags and traversal retain partial coverage.
use super::{Descendants, Target, Targets};
use crate::policy::resolution::normalized::Input;
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
            ..Targets::default()
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
    if leaf == "touch" {
        let words: Vec<_> = arguments.iter().map(String::as_str).collect();
        let footprint = super::touch::parse("touch", &words).expect("known touch name");
        let mut complete = footprint.complete;
        let mut items = Vec::new();
        for (action, role, path) in footprint
            .reference
            .into_iter()
            .map(|path| ("read", "source", path))
            .chain(
                footprint
                    .destinations
                    .into_iter()
                    .map(|path| ("write", "destination", path)),
            )
        {
            items.push(Target {
                action,
                role,
                path: super::join_operand(cwd, path, &mut complete),
                recursive: false,
                descendants: Descendants::All,
                follow: true,
            });
        }
        return Targets {
            items,
            complete,
            ..Targets::default()
        };
    }
    if leaf == "mkdir" {
        return mkdir_argv(arguments, cwd);
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
                ..Targets::default()
            };
        }
    };
    let mut operands = Vec::new();
    let mut options = true;
    let mut recursive = false;
    let mut no_directory = false;
    let mut target_directory = None;
    let mut complete = cfg!(any(unix, windows));
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
    let mut complete = cfg!(any(unix, windows));
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
                path: super::join_operand(cwd, path, &mut complete),
                recursive: false,
                descendants: Descendants::All,
                follow: true,
            })
            .collect(),
        complete,
        ..Targets::default()
    }
}

// grep reads explicit regular files; explicit operands follow their leaf links.
// A recursive grep (-r) reads every existing descendant, hidden ones too, since
// grep honours no ignore files; GNU and BSD walks follow no descendant links.
// Include/exclude filters only narrow it and receive no credit. -R follows
// links, and a recursive grep without a path differs between GNU and BSD, so
// both stay unmodeled. A pattern given with -e makes every operand a path.
// GNU and BSD grep move options written after operands to the front, unless
// POSIXLY_CORRECT makes them operands: a later option naming an existing path,
// or taking its value from the next word, stays unmodeled.
fn grep_argv(arguments: &[String], cwd: &Path) -> Targets {
    const FILTERS: &str = "--include --exclude --exclude-dir";
    let listed = |list: &str, word: &str| list.split_whitespace().any(|item| item == word);
    let mut operands = Vec::new();
    let mut options = true;
    let mut recursive = false;
    let mut pattern_given = false;
    let mut complete = cfg!(any(unix, windows));
    let mut arguments = arguments.iter();
    while let Some(argument) = arguments.next() {
        let long = argument
            .split_once('=')
            .map_or(argument.as_str(), |(name, _)| name);
        if !options || argument == "-" || !argument.starts_with('-') {
            operands.push(argument);
            continue;
        }
        let late = !operands.is_empty();
        complete &= !late || cwd.join(argument).symlink_metadata().is_err();
        if argument == "--" {
            options = false;
        } else if argument == "--recursive" {
            recursive = true;
        } else if argument == "-e" || argument == "--regexp" {
            pattern_given = true;
            complete &= !late && arguments.next().is_some();
        } else if long == "--regexp" || (argument.starts_with("-e") && argument.len() > 2) {
            pattern_given = true;
        } else if listed(FILTERS, argument) {
            complete &= !late && arguments.next().is_some();
        } else if long == argument || !listed(FILTERS, long) {
            recursive |= !argument.starts_with("--") && argument.contains('r');
            complete &= portable_flags(argument, "EFGinlcqvwxHhr");
        }
    }
    let paths: Vec<&String> = operands
        .into_iter()
        .skip(usize::from(!pattern_given))
        .collect();
    complete &= !paths.is_empty();
    let items = paths
        .into_iter()
        .map(|operand| {
            let path = super::join_operand(cwd, operand, &mut complete);
            let metadata = fs::metadata(&path);
            let directory = recursive && metadata.as_ref().is_ok_and(std::fs::Metadata::is_dir);
            complete &= operand != "-" && (directory || metadata.is_ok_and(|m| m.is_file()));
            Target {
                action: "read",
                role: "source",
                path,
                recursive: directory,
                descendants: if directory {
                    Descendants::Present
                } else {
                    Descendants::Visible
                },
                follow: true,
            }
        })
        .collect();
    Targets {
        items,
        complete,
        ..Targets::default()
    }
}

// Ripgrep 15 accepts options anywhere before `--`: short clusters (`-inw`),
// attached values (`-g*.rs`, `-A3`) and `--name=VALUE`. Ambient configuration
// is trusted; --no-config is optional. Helper programs (--pre, --pre-glob,
// -z/--search-zip, --hostname-bin), link following (-L/--follow) and options
// ripgrep does not define stay outside this model. Pattern and ignore files
// (-f, --ignore-file) are read like explicit files. Ignore-file negations,
// -g/--iglob globs and --hidden can include hidden entries, so their footprints
// include every existing descendant. --files lists names only: without
// --no-ignore, every existing descendant counts, so an existing excluded
// directory still blocks it. Without a path, ripgrep walks the working
// directory. Types (-t) select after hidden entries are skipped.
fn rg_argv(arguments: &[String], cwd: &Path) -> Targets {
    rg_model(arguments, cwd).0
}

/// A modeled content search naming no path, which ripgrep runs on its
/// standard input when that is a pipe instead of walking the directory. It
/// still reads its pattern and ignore files; see [`rg_auxiliary_reads`].
/// With `-f -` the patterns take the standard input, so ripgrep walks the
/// working directory instead.
pub(super) fn rg_pattern_only(arguments: &[String], cwd: &Path) -> bool {
    let (targets, listing) = rg_model(arguments, cwd);
    let request = rg_parse(arguments);
    targets.complete
        && !listing
        && request.pattern != RgPattern::Input
        && request.paths().is_empty()
}

/// The files ripgrep reads besides its search paths: `-f` pattern files and
/// `--ignore-file` files, as submitted.
pub(super) fn rg_auxiliary_reads(arguments: &[String]) -> Vec<String> {
    rg_parse(arguments)
        .reads
        .into_iter()
        .map(str::to_owned)
        .collect()
}

/// Ripgrep's short flags that change neither what it reads nor which program runs.
const RG_SHORT_FLAGS: &str = "sivxFUPSawbnN0opqHIcl";
/// Ripgrep's short options taking a value, with their long names.
const RG_SHORT_VALUED: [(char, &str); 14] = [
    ('e', "regexp"),
    ('f', "file"),
    ('E', "encoding"),
    ('m', "max-count"),
    ('j', "threads"),
    ('g', "glob"),
    ('d', "max-depth"),
    ('t', "type"),
    ('T', "type-not"),
    ('A', "after-context"),
    ('B', "before-context"),
    ('C', "context"),
    ('M', "max-columns"),
    ('r', "replace"),
];
/// Ripgrep's long options taking a value.
const RG_LONG_VALUED: &str = "regexp file pre pre-glob dfa-size-limit encoding engine max-count \
    regex-size-limit threads glob iglob ignore-file max-depth max-filesize type type-not type-add \
    type-clear after-context before-context color colors context context-separator \
    field-context-separator field-match-separator hostname-bin hyperlink-format max-columns \
    path-separator replace sort sortr generate";
/// Ripgrep's long flags that change neither what it reads nor which program
/// runs. Their `--no-` negations, and the positive forms of `--no-` flags here,
/// only restore defaults.
const RG_LONG_FLAGS: &str = "case-sensitive crlf fixed-strings ignore-case invert-match \
    line-regexp mmap multiline multiline-dotall no-unicode null-data pcre2 smart-case \
    stop-on-nonmatch text word-regexp auto-hybrid-regex no-pcre2-unicode binary \
    glob-case-insensitive ignore-file-case-insensitive no-ignore-dot no-ignore-exclude \
    no-ignore-files no-ignore-global no-ignore-parent no-ignore-vcs no-require-git \
    one-file-system block-buffered byte-offset column heading include-zero line-buffered \
    line-number no-line-number max-columns-preview null only-matching passthru pretty quiet \
    trim vimgrep with-filename no-filename sort-files count count-matches files-with-matches \
    files-without-match json debug no-ignore-messages no-messages stats trace no-config";
/// Ripgrep's options that run other programs, follow links or print
/// information without searching, which no target describes.
const RG_UNMODELED: &str =
    "pre pre-glob hostname-bin follow search-zip help version type-list pcre2-version generate";

/// The filters that decide which descendants a ripgrep walk can reach.
#[derive(Default)]
struct RgFilters {
    hidden: bool,
    no_ignore: bool,
    /// Glob selections override ripgrep's other filters, hidden ones too.
    selects: bool,
}

impl RgFilters {
    /// Without ignore files, globs or hidden entries, the walk skips hidden
    /// descendants; otherwise it can reach every existing one.
    fn descendants(&self) -> Descendants {
        if self.no_ignore && !self.selects && !self.hidden {
            Descendants::Visible
        } else {
            Descendants::Present
        }
    }
}

/// Where a ripgrep search takes its patterns from.
#[derive(Default, PartialEq)]
enum RgPattern {
    /// The first operand.
    #[default]
    Operand,
    /// `-e` or a `-f` file, so every operand is a path.
    Option,
    /// `-f -`: the standard input holds patterns, not searched data.
    Input,
}

/// A ripgrep request parsed as ripgrep does. `modeled` is false for an
/// option outside the model or a missing value.
#[derive(Default)]
struct RgRequest<'a> {
    operands: Vec<&'a str>,
    pattern: RgPattern,
    files: bool,
    filters: RgFilters,
    reads: Vec<&'a str>,
    unrestricted: u8,
    modeled: bool,
}

impl<'a> RgRequest<'a> {
    /// The searched paths: every operand for a listing or an option-given
    /// pattern, otherwise the operands after the pattern.
    fn paths(&self) -> &[&'a str] {
        if self.files || self.pattern != RgPattern::Operand {
            &self.operands
        } else {
            self.operands.get(1..).unwrap_or_default()
        }
    }

    fn value(&mut self, name: &str, value: Option<&'a str>) {
        let Some(value) = value else {
            self.modeled = false;
            return;
        };
        match name {
            "regexp" if self.pattern == RgPattern::Operand => self.pattern = RgPattern::Option,
            "regexp" => {}
            "file" if value == "-" => self.pattern = RgPattern::Input,
            "file" => {
                if self.pattern == RgPattern::Operand {
                    self.pattern = RgPattern::Option;
                }
                self.reads.push(value);
            }
            // --no-ignore leaves these files in force, and their `!` lines can
            // include hidden entries, as globs can.
            "ignore-file" => {
                self.reads.push(value);
                self.filters.selects = true;
            }
            "glob" | "iglob" => self.filters.selects = true,
            _ if listed(RG_UNMODELED, name) => self.modeled = false,
            _ => {}
        }
    }

    fn flag(&mut self, name: &str) {
        let known = |name: &str| listed(RG_LONG_FLAGS, name) || listed(RG_LONG_VALUED, name);
        match name {
            "hidden" => self.filters.hidden = true,
            "no-hidden" => self.filters.hidden = false,
            "no-ignore" => self.filters.no_ignore = true,
            "ignore" => self.filters.no_ignore = false,
            "files" => self.files = true,
            "unrestricted" => self.unrestricted(),
            _ if listed(RG_UNMODELED, name) => self.modeled = false,
            _ if listed(RG_LONG_FLAGS, name) => {}
            _ if name
                .strip_prefix("no-")
                .is_some_and(|base| known(base) || listed(RG_UNMODELED, base)) => {}
            _ if known(&format!("no-{name}")) => {}
            _ => self.modeled = false,
        }
    }

    /// Each -u lifts one filter level: ignore files, then hidden entries,
    /// then binary files.
    fn unrestricted(&mut self) {
        self.unrestricted = self.unrestricted.saturating_add(1);
        self.filters.no_ignore = true;
        self.filters.hidden |= self.unrestricted >= 2;
    }
}

fn listed(list: &str, word: &str) -> bool {
    list.split_whitespace().any(|item| item == word)
}

fn rg_parse(arguments: &[String]) -> RgRequest<'_> {
    let mut request = RgRequest {
        modeled: true,
        ..RgRequest::default()
    };
    let mut options = true;
    let mut words = arguments.iter().map(String::as_str);
    while let Some(word) = words.next() {
        if !options || word == "-" || !word.starts_with('-') {
            request.operands.push(word);
        } else if word == "--" {
            options = false;
        } else if let Some(long) = word.strip_prefix("--") {
            let (name, attached) = long
                .split_once('=')
                .map_or((long, None), |(name, value)| (name, Some(value)));
            if listed(RG_LONG_VALUED, name) {
                let value = attached.or_else(|| words.next());
                request.value(name, value);
            } else if attached.is_some() {
                request.modeled = false;
            } else {
                request.flag(name);
            }
        } else {
            for (index, flag) in word.char_indices().skip(1) {
                if let Some((_, name)) = RG_SHORT_VALUED.iter().find(|(short, _)| *short == flag) {
                    let attached = &word[index + flag.len_utf8()..];
                    // ripgrep drops one `=` before an attached value: `-f=x` reads `x`.
                    let attached = attached.strip_prefix('=').unwrap_or(attached);
                    let value = if attached.is_empty() {
                        words.next()
                    } else {
                        Some(attached)
                    };
                    request.value(name, value);
                    break;
                }
                match flag {
                    '.' => request.filters.hidden = true,
                    'u' => request.unrestricted(),
                    _ if RG_SHORT_FLAGS.contains(flag) => {}
                    _ => request.modeled = false,
                }
            }
        }
    }
    request
}

pub(super) fn rg_model(arguments: &[String], cwd: &Path) -> (Targets, bool) {
    let request = rg_parse(arguments);
    let mut complete = cfg!(any(unix, windows)) && request.modeled;
    // A content search needs a pattern, positional or given by -e/-f.
    complete &=
        request.files || request.pattern != RgPattern::Operand || !request.operands.is_empty();
    let paths = match request.paths() {
        [] => &["."][..],
        paths => paths,
    };
    let descendants = request.filters.descendants();
    let mut unavailable = None;
    let mut items = Vec::new();
    for (operand, searched) in paths
        .iter()
        .map(|path| (*path, true))
        .chain(request.reads.iter().map(|path| (*path, false)))
    {
        let path = super::join_operand(cwd, operand, &mut complete);
        let metadata = fs::metadata(&path);
        let directory = searched && metadata.as_ref().is_ok_and(std::fs::Metadata::is_dir);
        if let Some(code) = rg_metadata_issue(&metadata) {
            unavailable.get_or_insert_with(|| (path.clone(), code));
        }
        complete &= operand != "-" && (directory || metadata.is_ok_and(|m| m.is_file()));
        items.push(Target {
            action: "read",
            role: "source",
            path,
            recursive: directory,
            descendants,
            follow: true,
        });
    }
    (
        Targets {
            items,
            complete,
            unavailable,
        },
        request.files,
    )
}

fn rg_metadata_issue(metadata: &std::io::Result<fs::Metadata>) -> Option<&'static str> {
    match metadata {
        Ok(m) if m.is_dir() || m.is_file() => None,
        Ok(_) => Some("unsupported_file_type"),
        Err(error) => Some(match error.kind() {
            std::io::ErrorKind::NotFound => "not_found",
            std::io::ErrorKind::PermissionDenied => "permission_denied",
            std::io::ErrorKind::NotADirectory => "not_directory",
            _ => "unresolved",
        }),
    }
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
    let mut complete = cfg!(any(unix, windows));
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
                path: super::join_operand(cwd, start, &mut complete),
                recursive: true,
                descendants: Descendants::Present,
                follow: true,
            })
            .collect(),
        complete,
        ..Targets::default()
    }
}

// mkdir's missing parents stay below its resolved operand. Modes are unmodeled.
fn mkdir_argv(arguments: &[String], cwd: &Path) -> Targets {
    let mut items = Vec::new();
    let mut options = true;
    let mut terminated = false;
    let mut complete = cfg!(any(unix, windows));
    for argument in arguments {
        if options && argument == "--" {
            options = false;
            terminated = true;
        } else if options && argument.starts_with('-') && argument != "-" {
            complete &= portable_flags(argument, "p");
        } else {
            complete &= terminated || !argument.starts_with('-') || argument == "-";
            options = false;
            complete &= !Path::new(argument)
                .components()
                .any(|c| c == std::path::Component::ParentDir);
            items.push(Target {
                action: "write",
                role: "destination",
                path: super::join_operand(cwd, argument, &mut complete),
                recursive: false,
                descendants: Descendants::All,
                follow: true,
            });
        }
    }
    complete &= !items.is_empty();
    Targets {
        items,
        complete,
        ..Targets::default()
    }
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
        complete: cfg!(any(unix, windows)),
        ..Targets::default()
    };
    let destination = destination.map(|p| super::join_operand(cwd, p, &mut result.complete));
    let directory = destination
        .as_ref()
        .is_some_and(|p| !no_directory && fs::metadata(p).is_ok_and(|m| m.is_dir()));
    if ["copy", "move"].contains(&action)
        && (sources.is_empty() || (sources.len() > 1 && !directory))
    {
        result.complete = false;
    }
    for source in sources {
        let source = super::join_operand(cwd, source, &mut result.complete);
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
