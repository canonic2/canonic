//! File footprints for fixed text filters: the files they read, and for sed the
//! files it edits in place or names in its script. Program effects remain
//! explicitly bounded.
use super::{Descendants, Target, Targets};
mod jq;
use std::{fs, path::Path};

/// A recognized filter invocation.
struct Parsed {
    operands: Vec<String>,
    /// The backup suffix when sed edits its operands in place; empty for none.
    in_place: Option<String>,
    /// Files a sed script writes (`w`, `W`, the `w` flag) or reads (`r`, `R`).
    writes: Vec<String>,
    reads: Vec<String>,
    /// The sed script word, as the reading took it.
    script: Option<String>,
    /// The position of a sed `-i` followed by a separate nonempty word.
    separate_suffix: Option<usize>,
    /// BSD sed's reading of `-i WORD`, with WORD as the backup suffix, whose
    /// footprint joins this GNU reading's.
    alternative: Option<Box<Parsed>>,
    complete: bool,
}

impl Parsed {
    /// Each named file with whether the filter writes it, operands first,
    /// then the files only the alternative reading names.
    fn files(&self) -> Vec<(bool, String)> {
        let mut files = Vec::new();
        for (write, _, path) in self.entries(&|_| false) {
            if !files.contains(&(write, path.clone())) {
                files.push((write, path));
            }
        }
        files
    }

    /// Each named file with whether the filter writes it and whether that
    /// access follows a symbolic link at the name. An in-place edit renames
    /// its output, and the backup of its input, over the name, replacing a
    /// link there instead of writing through it; reads and script writes
    /// open the name and follow it.
    fn entries(&self, missing: &dyn Fn(&str) -> bool) -> Vec<(bool, bool, String)> {
        let mut files = Vec::new();
        for operand in self.present(missing) {
            files.push((false, true, operand.clone()));
            if let Some(suffix) = &self.in_place {
                files.push((true, false, operand.clone()));
                if !suffix.is_empty() {
                    files.push((true, false, format!("{operand}{suffix}")));
                }
            }
        }
        files.extend(self.reads.iter().map(|path| (false, true, path.clone())));
        files.extend(self.writes.iter().map(|path| (true, true, path.clone())));
        for file in self
            .alternative
            .iter()
            .flat_map(|other| other.entries(missing))
        {
            if !files.contains(&file) {
                files.push(file);
            }
        }
        files
    }

    /// The input operands this reading opens, without the in-place inputs
    /// `missing` reports. Only a reading that creates no file can skip them: a
    /// `w` file is created before input is read, and a backup can be a later
    /// operand, so either could create a missing input that sed then edits.
    /// Without both, sed only renames its output onto existing inputs, so a
    /// missing one is reported, not edited.
    fn present(&self, missing: &dyn Fn(&str) -> bool) -> Vec<&String> {
        let creates_nothing =
            self.writes.is_empty() && self.in_place.as_deref().is_some_and(str::is_empty);
        self.operands
            .iter()
            .filter(|operand| operand.as_str() != "-")
            .filter(|operand| !creates_nothing || !missing(operand))
            .collect()
    }

    /// Every input file either reading edits or reads.
    fn inputs(&self, missing: &dyn Fn(&str) -> bool) -> Vec<&String> {
        let mut inputs = self.present(missing);
        if let Some(other) = &self.alternative {
            inputs.extend(other.present(missing));
        }
        inputs
    }
}

/// The filter's footprint at `cwd`. When `alone`, the request runs no other
/// operation that could create a file first, so an in-place input that is
/// certainly absent names no target. Otherwise, as for any other lstat
/// failure or a present non-regular input, the footprint is incomplete.
pub(super) fn collect(
    executable: &str,
    arguments: &[String],
    cwd: &Path,
    alone: bool,
) -> Option<Targets> {
    let parsed = parse(executable, arguments)?;
    let mut complete = parsed.complete;
    let missing = |operand: &str| {
        alone
            && fs::symlink_metadata(cwd.join(operand))
                .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound)
    };
    complete &= parsed
        .inputs(&missing)
        .iter()
        .all(|operand| fs::metadata(cwd.join(operand)).is_ok_and(|m| m.is_file()));
    let items = parsed
        .entries(&missing)
        .into_iter()
        .map(|(write, follow, path)| Target {
            action: if write { "write" } else { "read" },
            role: if write { "destination" } else { "source" },
            path: cwd.join(path),
            recursive: false,
            descendants: Descendants::All,
            follow,
        })
        .collect();
    Some(Targets { items, complete })
}

/// Whether the filter only reads its operands and names no other file.
pub(super) fn read_only(executable: &str, arguments: &[String]) -> bool {
    parse(executable, arguments).is_some_and(|parsed| {
        parsed.complete
            && parsed.in_place.is_none()
            && parsed.writes.is_empty()
            && parsed.reads.is_empty()
    })
}

/// Every file a modeled filter names, with whether it writes it; None when its
/// form is not modeled.
pub(super) fn files(executable: &str, arguments: &[String]) -> Option<Vec<(bool, String)>> {
    parse(executable, arguments)
        .filter(|parsed| parsed.complete)
        .map(|parsed| parsed.files())
}

/// A recognized sed in-place option.
enum InPlace {
    /// The backup suffix; empty for no backup.
    Suffix(String),
    /// A suffix using GNU's `*` form or holding NUL.
    Unmodeled,
}

/// sed's in-place option: `-i`, `-iSUFFIX`, `--in-place` or
/// `--in-place=SUFFIX`. This is GNU's reading of `-i WORD`, where WORD, even
/// an empty one, is the script; [`parse`] adds BSD's, where WORD is the backup
/// suffix.
fn in_place(argument: &str) -> Option<InPlace> {
    let suffix = if argument == "-i" {
        String::new()
    } else if let Some(suffix) = argument
        .strip_prefix("--in-place=")
        .or_else(|| argument.strip_prefix("-i"))
    {
        suffix.to_owned()
    } else if argument == "--in-place" {
        String::new()
    } else {
        return None;
    };
    // Without `*`, both seds append the suffix to the input's name as written,
    // so a `/` in it only names a path below that name.
    Some(if suffix.contains(['*', '\0']) {
        InPlace::Unmodeled
    } else {
        InPlace::Suffix(suffix)
    })
}

/// Both readings of sed's `-i WORD`. GNU sed takes WORD as the script, so an
/// empty WORD is an empty script and every later word an edited file; BSD sed
/// takes WORD as the backup suffix, like GNU's `-iWORD` (no backup when WORD is
/// empty), and the next word as the script. The footprint is their union and
/// complete only when both are. A BSD reading without input files refuses
/// before reading input, so only a script that can open files for writing
/// (`w`) at compile time can affect anything; a script outside the model counts
/// as possibly writing when it contains a `w`.
fn parse(executable: &str, arguments: &[String]) -> Option<Parsed> {
    let mut parsed = parse_reading(executable, arguments)?;
    let Some(at) = parsed.separate_suffix else {
        return Some(parsed);
    };
    let mut joined = arguments.to_vec();
    let suffix = joined.remove(at + 1);
    joined[at] = format!("--in-place={suffix}");
    let bsd = parse_reading(executable, &joined)?;
    let inert = bsd.operands.is_empty()
        && bsd.script.as_deref().is_none_or(|script| {
            sed(script).map_or(!script.contains('w'), |files| files.writes.is_empty())
        });
    if !inert {
        parsed.complete &= bsd.complete && bsd.separate_suffix.is_none();
        parsed.alternative = Some(Box::new(bsd));
    }
    Some(parsed)
}

fn parse_reading(executable: &str, arguments: &[String]) -> Option<Parsed> {
    let path = Path::new(executable);
    let leaf = path.file_name()?.to_str()?;
    if executable != leaf && ![Path::new("/bin"), Path::new("/usr/bin")].contains(&path.parent()?) {
        return None;
    }
    let (flags, values, long, max) = match leaf {
        "sed" => ("nEru", "", "", 256),
        "awk" => ("", "F", "", 256),
        "jq" => (
            "rceSMCasR",
            "",
            "--raw-output --compact-output --exit-status --sort-keys --monochrome-output --color-output --ascii-output --slurp --raw-input",
            256,
        ),
        "sort" => ("bdfghiMnrRsuV", "kt", "", 256),
        "uniq" => ("cdiu", "fs", "", 1),
        "cut" => ("ns", "bcfd", "", 256),
        "diff" => (
            "abBiqsuw",
            "U",
            "--text --brief --report-identical-files --ignore-case --ignore-all-space --ignore-space-change --ignore-blank-lines --unified",
            2,
        ),
        "cmp" => ("bcls", "", "--print-bytes --verbose --quiet --silent", 2),
        "nl" => ("p", "bdfhinsvw", "", 1),
        "comm" => ("123i", "", "", 2),
        "paste" => ("s", "d", "", 256),
        "column" => ("tx", "sco", "", 256),
        // Pagers are not fixed filters: helpers and interactive commands can
        // start subprocesses or access other files, even with readable operands.
        _ => return None,
    };
    let program = matches!(leaf, "sed" | "awk" | "jq");
    let total = arguments.len();
    let mut arguments = arguments.iter().peekable();
    let mut parsed = Parsed {
        operands: Vec::new(),
        in_place: None,
        writes: Vec::new(),
        reads: Vec::new(),
        script: None,
        separate_suffix: None,
        alternative: None,
        complete: cfg!(unix),
    };
    let mut options = true;
    let mut literal = false;
    let mut script = !program;
    while let Some(argument) = arguments.next() {
        if options && leaf == "sed" && argument == "-i" && arguments.peek().is_some() {
            // Only one ambiguous suffix is modeled.
            parsed.complete &= parsed.separate_suffix.is_none();
            parsed
                .separate_suffix
                .get_or_insert(total - arguments.len() - 1);
        }
        if !literal && argument == "--" {
            options = false;
            literal = true;
        } else if options
            && leaf == "sed"
            && let Some(option) = in_place(argument)
        {
            parsed.in_place = Some(match option {
                InPlace::Suffix(suffix) => suffix,
                InPlace::Unmodeled => {
                    parsed.complete = false;
                    String::new()
                }
            });
        } else if options && argument.starts_with('-') && argument != "-" {
            parsed.complete &= option(argument, flags, values, long, &mut arguments);
        } else if !script {
            parsed.script = Some(argument.clone());
            parsed.complete &= match leaf {
                // An empty script copies its input unchanged.
                "sed" if argument.is_empty() => true,
                "sed" => sed(argument).is_some_and(|files| {
                    parsed.writes = files.writes;
                    parsed.reads = files.reads;
                    true
                }),
                "awk" => awk(argument),
                "jq" => jq(argument),
                _ => false,
            };
            script = true;
            options = false;
        } else {
            parsed.complete &= literal || !argument.starts_with('-') || argument == "-";
            parsed.operands.push(argument.clone());
            options = false;
        }
    }
    parsed.complete &= script
        && parsed.operands.len() <= max
        && !(parsed.in_place.is_some() && parsed.operands.iter().any(|o| o.as_str() == "-"));
    Some(parsed)
}

fn option<'a>(
    argument: &str,
    flags: &str,
    values: &str,
    long: &str,
    arguments: &mut impl Iterator<Item = &'a String>,
) -> bool {
    if argument.starts_with("--") {
        // The only valued long option in these fixed forms is diff's context.
        if argument == "--unified" && long.contains("--unified") {
            return true;
        }
        if let Some(count) = argument.strip_prefix("--unified=") {
            return long.contains("--unified") && digits(count);
        }
        return long.split_whitespace().any(|flag| flag == argument);
    }
    let mut short = argument.as_bytes()[1..].iter();
    while let Some(&flag) = short.next() {
        if values.as_bytes().contains(&flag) {
            let value = if short.as_slice().is_empty() {
                arguments.next().map(String::as_str)
            } else {
                std::str::from_utf8(short.as_slice()).ok()
            };
            return value.is_some_and(|v| !v.is_empty());
        }
        if !flags.as_bytes().contains(&flag) {
            return false;
        }
    }
    true
}

fn digits(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit())
}

/// Files a sed script writes or reads by name.
#[derive(Default)]
struct SedFiles {
    writes: Vec<String>,
    reads: Vec<String>,
}

/// Scripts of `p`, `d`, `s`, `y`, `w`, `W`, `r` and `R` commands, each with an
/// optional line address, and the files they name. `e` and the `e` flag run
/// programs, so scripts using them, regex addresses or blocks are not modeled.
fn sed(script: &str) -> Option<SedFiles> {
    let mut files = SedFiles::default();
    let mut rest = script;
    let mut commands = 0;
    loop {
        rest = rest.trim_start_matches([' ', '\t', '\n', ';']);
        if rest.is_empty() {
            return (commands > 0).then_some(files);
        }
        let end = rest
            .find(|c: char| !(c.is_ascii_digit() || c == '$' || c == ','))
            .unwrap_or(rest.len());
        let (address, tail) = rest.split_at(end);
        if !address.is_empty() {
            let parts: Vec<_> = address.split(',').collect();
            if parts.len() > 2 || !parts.iter().all(|part| *part == "$" || digits(part)) {
                return None;
            }
        }
        let mut chars = tail.chars();
        let after = match chars.next()? {
            'p' | 'd' => chars.as_str(),
            'y' => sed_replacement(chars.as_str(), false)?,
            's' => {
                let after = sed_replacement(chars.as_str(), true)?;
                match after.strip_prefix('w') {
                    Some(name) => sed_file(name, &mut files.writes)?,
                    None => after,
                }
            }
            'w' | 'W' => sed_file(chars.as_str(), &mut files.writes)?,
            'r' | 'R' => sed_file(chars.as_str(), &mut files.reads)?,
            _ => return None,
        };
        let after = after.trim_start_matches([' ', '\t']);
        if !(after.is_empty() || after.starts_with([';', '\n'])) {
            return None;
        }
        rest = after;
        commands += 1;
    }
}

/// A file name runs to the end of the line, `;` included, as sed reads it.
/// The standard streams are not files.
fn sed_file<'a>(text: &'a str, files: &mut Vec<String>) -> Option<&'a str> {
    let text = text.trim_start_matches([' ', '\t']);
    let (name, after) = text.split_at(text.find('\n').unwrap_or(text.len()));
    if name.is_empty() {
        return None;
    }
    if !["/dev/stdout", "/dev/stderr"].contains(&name) {
        files.push(name.to_owned());
    }
    Some(after)
}

/// The text after an `s` or `y` command's three delimiters and, for `s`, its
/// global, print, occurrence, case and multiline flags.
fn sed_replacement(text: &str, flags: bool) -> Option<&str> {
    let mut chars = text.char_indices();
    let (_, delimiter) = chars.next()?;
    // A letter, digit or separator as delimiter would make flags ambiguous.
    if delimiter.is_ascii_alphanumeric()
        || delimiter.is_whitespace()
        || ['\\', ';', '{', '}'].contains(&delimiter)
    {
        return None;
    }
    let mut delimiters = 0;
    let mut escaped = false;
    let mut end = None;
    for (index, c) in chars {
        if c == '\n' {
            return None;
        }
        if escaped {
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else if c == delimiter {
            delimiters += 1;
            if delimiters == 2 {
                end = Some(index + c.len_utf8());
                break;
            }
        }
    }
    let rest = &text[end?..];
    if !flags {
        return Some(rest);
    }
    let length = rest
        .find(|c: char| !(c.is_ascii_digit() || "gpiImM".contains(c)))
        .unwrap_or(rest.len());
    Some(&rest[length..])
}

fn awk(script: &str) -> bool {
    let Some(body) = script
        .trim()
        .strip_prefix('{')
        .and_then(|s| s.strip_suffix('}'))
    else {
        return false;
    };
    let Some(fields) = body
        .trim()
        .strip_prefix("print")
        .filter(|s| s.starts_with(char::is_whitespace))
    else {
        return false;
    };
    fields
        .split(',')
        .all(|field| field.trim().strip_prefix('$').is_some_and(digits))
}

fn jq(script: &str) -> bool {
    if jq::read_only(script) {
        return true;
    }
    script.split('|').all(|part| {
        let part = part.trim();
        matches!(
            part,
            "keys"
                | "keys_unsorted"
                | "length"
                | "type"
                | "sort"
                | "unique"
                | "values"
                | "tostring"
        ) || jq_path(part)
    })
}

fn jq_path(value: &str) -> bool {
    let Some(mut rest) = value.strip_prefix('.') else {
        return false;
    };
    if rest.is_empty() {
        return true;
    }
    loop {
        if let Some(bracket) = rest.strip_prefix('[') {
            let Some((key, tail)) = bracket.split_once(']') else {
                return false;
            };
            let quoted = key.strip_prefix('"').and_then(|s| s.strip_suffix('"'));
            if !(key.is_empty()
                || digits(key)
                || quoted.is_some_and(|s| {
                    !s.is_empty()
                        && s.bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
                }))
            {
                return false;
            }
            rest = tail;
        } else {
            if !rest.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_') {
                return false;
            }
            rest = rest.trim_start_matches(|c: char| c.is_ascii_alphanumeric() || c == '_');
        }
        if rest.is_empty() {
            return true;
        }
        if let Some(tail) = rest.strip_prefix('.') {
            rest = tail;
            if rest.is_empty() {
                return false;
            }
        } else if !rest.starts_with('[') {
            return false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_programs_admit_reads_and_reject_file_helpers_and_environment_access() {
        for script in [
            "p",
            "1,200p",
            "200,$p",
            "$p",
            "s/acme/example/",
            "s|/acme|/example|g",
            "1,5s/a/b/2p",
            "s/(\"[^\"]*(key|token)[^\"]*\"\\s*:\\s*\")[^\"]+/\\1<redacted>/Ig",
            "s/a\\/b/c/; 3d; y/abc/xyz/",
            "s/;/,/g\n$p",
            "s/a/b/w /dev/stdout",
        ] {
            let files = sed(script).unwrap_or_else(|| panic!("{script}"));
            assert!(
                files.writes.is_empty() && files.reads.is_empty(),
                "{script}"
            );
        }
        // Scripts name the files they write or read, to the end of the line.
        for (script, writes, reads) in [
            ("w output", vec!["output"], vec![]),
            ("s/a/b/gw out; p", vec!["out; p"], vec![]),
            (
                "1r notes.txt\n$W tail.txt",
                vec!["tail.txt"],
                vec!["notes.txt"],
            ),
            ("s/a/b/w one\ns/c/d/w two", vec!["one", "two"], vec![]),
        ] {
            let files = sed(script).unwrap_or_else(|| panic!("{script}"));
            assert_eq!(files.writes, writes, "{script}");
            assert_eq!(files.reads, reads, "{script}");
        }
        for script in [
            "",
            "1,2p; e id",
            "1,2p\ne id",
            "s/a/b/e",
            "s/a/b/ew out",
            "w",
            "s/a/b",
            "s/a\nb/c/",
            "sgagbg",
            "s a b ",
            "y/a/b/g",
            "/acme/p",
            "1{p}",
            "1,2,3p",
        ] {
            assert!(sed(script).is_none(), "{script}");
        }
    }

    #[test]
    fn sed_in_place_edits_write_each_input_and_its_backup() {
        let arguments = |words: &[&str]| words.iter().map(|w| (*w).to_owned()).collect::<Vec<_>>();
        // In-place edits write each input, plus its backup when a suffix is given.
        for (words, written) in [
            (vec!["-i", "s/a/b/", "a.txt"], vec!["a.txt"]),
            // GNU reads the empty word as the script and edits `s/a/b/` too.
            (vec!["-i", "", "s/a/b/", "a.txt"], vec!["s/a/b/", "a.txt"]),
            (
                vec!["-E", "-i.bak", "s/a/b/", "a.txt"],
                vec!["a.txt", "a.txt.bak"],
            ),
            (
                vec!["--in-place=~", "-n", "1p", "a.txt"],
                vec!["a.txt", "a.txt~"],
            ),
            (vec!["-n", "1w out.txt", "a.txt"], vec!["out.txt"]),
            // Without `*`, a suffix is appended to the name as written.
            (
                vec!["-i../x", "s/a/b/", "a.txt"],
                vec!["a.txt", "a.txt../x"],
            ),
        ] {
            let found = files("sed", &arguments(&words)).unwrap_or_else(|| panic!("{words:?}"));
            let writes: Vec<_> = found
                .iter()
                .filter(|(w, _)| *w)
                .map(|(_, p)| p.as_str())
                .collect();
            assert_eq!(writes, written, "{words:?}");
            assert!(!read_only("sed", &arguments(&words)), "{words:?}");
        }
        for words in [
            vec!["-i*.bak", "s/a/b/", "a.txt"],
            vec!["-i", "s/a/b/", "-"],
            vec!["-n", "1e id", "a.txt"],
        ] {
            assert!(files("sed", &arguments(&words)).is_none(), "{words:?}");
        }
        assert!(read_only("sed", &arguments(&["-n", "s/a/b/p", "a.txt"])));
    }

    #[test]
    fn sed_in_place_with_a_separate_word_joins_gnu_and_bsd_readings() {
        let arguments = |words: &[&str]| words.iter().map(|w| (*w).to_owned()).collect::<Vec<_>>();
        for (words, written) in [
            // BSD reads `1p` as the suffix and `w /tmp/x` as the script.
            (
                vec!["-i", "1p", "w /tmp/x", "a.txt"],
                vec!["w /tmp/x", "a.txt", "a.txt1p", "/tmp/x"],
            ),
            (vec!["-i", "-n", "p", "a.txt"], vec!["a.txt", "a.txt-n"]),
            (
                vec!["-i", "--", "s/a/b/", "a.txt"],
                vec!["a.txt", "a.txt--"],
            ),
            // Without input files BSD sed refuses an in-place edit, and its
            // script cannot open a file for writing.
            (vec!["-i", "s/a/b/", "a.txt"], vec!["a.txt"]),
            // BSD takes `s/a/b/` as the suffix and `w out` as the script,
            // which opens `out` even without input files.
            (vec!["-i", "s/a/b/", "w out"], vec!["w out", "out"]),
        ] {
            let found = files("sed", &arguments(&words)).unwrap_or_else(|| panic!("{words:?}"));
            let writes: Vec<_> = found
                .iter()
                .filter(|(w, _)| *w)
                .map(|(_, p)| p.as_str())
                .collect();
            assert_eq!(writes, written, "{words:?}");
        }
        for words in [
            // GNU reads `.bak` as the script, which is not modeled.
            vec!["-i", ".bak", "s/a/b/", "a.txt"],
            // BSD reads `a.txt` as a script outside the model.
            vec!["-i", "s/a/b/", "a.txt", "b.txt"],
            vec!["-i", "x", "-i", "y", "p", "a.txt"],
        ] {
            assert!(files("sed", &arguments(&words)).is_none(), "{words:?}");
        }
        let root = assert_fs::TempDir::new().unwrap();
        std::fs::write(root.path().join("a.txt"), "a").unwrap();
        let request = arguments(&["-i", "1p", "w /tmp/x", "a.txt"]);
        // Another operation in the request could create GNU's missing input.
        let targets = collect("sed", &request, root.path(), false).unwrap();
        assert!(!targets.complete, "the GNU reading edits a missing file");
        // Alone, that input is reported, not edited; BSD's script write, edit
        // and backup remain.
        let targets = collect("sed", &request, root.path(), true).unwrap();
        assert!(targets.complete);
        let writes: Vec<_> = targets
            .items
            .iter()
            .filter(|t| t.action == "write")
            .map(|t| t.path.clone())
            .collect();
        assert_eq!(
            writes,
            [
                root.path().join("a.txt"),
                root.path().join("a.txt1p"),
                std::path::PathBuf::from("/tmp/x"),
            ]
        );
        // Any other unusable input, such as a dangling link, stays incomplete.
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("absent.txt", root.path().join("dangling.txt")).unwrap();
            let targets = collect(
                "sed",
                &arguments(&["-i", "", "s/a/b/", "dangling.txt"]),
                root.path(),
                true,
            )
            .unwrap();
            assert!(!targets.complete);
        }
    }

    #[test]
    fn a_sole_bsd_style_sed_edit_writes_only_its_existing_input() {
        let root = assert_fs::TempDir::new().unwrap();
        std::fs::write(root.path().join("notes.txt"), "a").unwrap();
        let words: Vec<String> = ["-i", "", "s/a/b/", "notes.txt"]
            .iter()
            .map(|w| (*w).to_owned())
            .collect();
        // GNU's input `s/a/b/` is absent; only a request without other
        // operations may treat it as such.
        assert!(!collect("sed", &words, root.path(), false).unwrap().complete);
        let targets = collect("sed", &words, root.path(), true).unwrap();
        assert!(targets.complete);
        let entries: Vec<_> = targets
            .items
            .iter()
            .map(|t| (t.action, t.path.clone()))
            .collect();
        let notes = root.path().join("notes.txt");
        assert_eq!(entries, [("read", notes.clone()), ("write", notes)]);
    }

    #[test]
    fn missing_inputs_stay_unknown_when_the_edit_can_create_them() {
        let root = assert_fs::TempDir::new().unwrap();
        std::fs::write(root.path().join("a.txt"), "a").unwrap();
        std::fs::create_dir(root.path().join("b.txtx")).unwrap();
        let arguments = |words: &[&str]| words.iter().map(|w| (*w).to_owned()).collect::<Vec<_>>();
        for words in [
            // The `w` file is created first, then edited as an input, and its
            // backup lands outside the project.
            &["-ix/../../.zshrc", "w b.txt", "b.txt"][..],
            &["-i", "w b.txt", "b.txt"],
            // The first input's backup is the second input, then edited too.
            &["-i.bak", "s/a/b/", "a.txt", "a.txt.bak"],
        ] {
            let targets = collect("sed", &arguments(words), root.path(), true).unwrap();
            assert!(!targets.complete, "{words:?}");
        }
        // Without a backup or a `w` file, sed creates nothing a later input
        // could name, so a missing input is only reported. (`-i` before a
        // separate word would add BSD's reading, whose backup suffix is it.)
        let words = arguments(&["--in-place", "s/a/b/", "a.txt", "gone.txt"]);
        let targets = collect("sed", &words, root.path(), true).unwrap();
        assert!(targets.complete);
        assert!(targets.items.iter().all(|t| !t.path.ends_with("gone.txt")));
    }

    #[test]
    fn sed_in_place_with_an_empty_word_joins_gnu_and_bsd_readings() {
        let arguments = |words: &[&str]| words.iter().map(|w| (*w).to_owned()).collect::<Vec<_>>();
        // GNU reads `''` as an empty script and edits `p` as a file; BSD reads
        // it as no backup suffix and `p` as the script.
        let found = files("sed", &arguments(&["-n", "-i", "", "p", "notes.txt"])).unwrap();
        assert_eq!(
            found,
            [
                (false, "p".to_owned()),
                (true, "p".to_owned()),
                (false, "notes.txt".to_owned()),
                (true, "notes.txt".to_owned()),
            ]
        );
        // Without later input files the BSD reading refuses, leaving GNU's.
        let found = files("sed", &arguments(&["-i", "", "s/a/b/"])).unwrap();
        assert_eq!(
            found,
            [(false, "s/a/b/".to_owned()), (true, "s/a/b/".to_owned())]
        );
        // An empty script reads its input and names no other file.
        assert!(read_only("sed", &arguments(&["", "notes.txt"])));
    }

    #[cfg(unix)]
    #[test]
    fn sed_in_place_edits_replace_a_linked_input_and_backup_and_read_through_it() {
        let root = assert_fs::TempDir::new().unwrap();
        std::fs::write(root.path().join("target.txt"), "a").unwrap();
        std::os::unix::fs::symlink("target.txt", root.path().join("a.txt")).unwrap();
        let words: Vec<String> = ["-i.bak", "s/a/b/", "a.txt"]
            .iter()
            .map(|w| (*w).to_owned())
            .collect();
        let targets = collect("sed", &words, root.path(), false).unwrap();
        let entries: Vec<_> = targets
            .items
            .iter()
            .map(|t| (t.action, t.follow, t.path.clone()))
            .collect();
        assert_eq!(
            entries,
            [
                ("read", true, root.path().join("a.txt")),
                ("write", false, root.path().join("a.txt")),
                ("write", false, root.path().join("a.txt.bak")),
            ]
        );
        // A script's own write opens the name and follows a link there.
        let words: Vec<String> = ["-n", "w a.txt", "notes.txt"]
            .iter()
            .map(|w| (*w).to_owned())
            .collect();
        let targets = collect("sed", &words, root.path(), false).unwrap();
        assert!(
            targets
                .items
                .iter()
                .any(|t| t.action == "write" && t.follow && t.path == root.path().join("a.txt"))
        );
    }

    #[test]
    fn line_and_column_readers_read_their_operands_with_listed_options() {
        let arguments = |words: &[&str]| words.iter().map(|w| (*w).to_owned()).collect::<Vec<_>>();
        for (leaf, words, read) in [
            ("nl", vec!["-ba", "-w", "4", "a.txt"], vec!["a.txt"]),
            (
                "comm",
                vec!["-12", "a.txt", "b.txt"],
                vec!["a.txt", "b.txt"],
            ),
            ("comm", vec!["-3", "-", "b.txt"], vec!["b.txt"]),
            (
                "paste",
                vec!["-sd,", "a.txt", "b.txt"],
                vec!["a.txt", "b.txt"],
            ),
            ("column", vec!["-t", "-s", ",", "a.csv"], vec!["a.csv"]),
            ("nl", vec!["-ba"], vec![]),
        ] {
            let found =
                files(leaf, &arguments(&words)).unwrap_or_else(|| panic!("{leaf} {words:?}"));
            assert!(found.iter().all(|(write, _)| !write), "{leaf} {words:?}");
            let paths: Vec<_> = found.iter().map(|(_, p)| p.as_str()).collect();
            assert_eq!(paths, read, "{leaf} {words:?}");
            assert!(read_only(leaf, &arguments(&words)), "{leaf} {words:?}");
        }
        for (leaf, words) in [
            ("nl", vec!["a.txt", "b.txt"]),
            ("comm", vec!["a.txt", "b.txt", "c.txt"]),
            ("column", vec!["--output-width", "80", "a.txt"]),
            ("less", vec!["-o", "log.txt", "a.txt"]),
            ("less", vec!["a.txt", "-N"]),
            ("more", vec!["-n", "5", "a.txt"]),
        ] {
            assert!(!read_only(leaf, &arguments(&words)), "{leaf} {words:?}");
        }
    }

    #[test]
    fn pagers_never_establish_read_only_or_complete_file_effects() {
        for executable in [
            "less",
            "more",
            "/bin/less",
            "/usr/bin/less",
            "/bin/more",
            "/usr/bin/more",
        ] {
            for words in [
                vec!["a.txt"],
                vec!["-N", "a.txt"],
                vec!["--", "a.txt"],
                vec!["+!helper", "a.txt"],
            ] {
                let arguments: Vec<String> = words.iter().map(|word| (*word).into()).collect();
                assert!(!read_only(executable, &arguments));
                assert!(files(executable, &arguments).is_none());
                assert!(collect(executable, &arguments, Path::new("/"), false).is_none());
            }
        }
    }

    #[test]
    fn fixed_awk_and_jq_programs_admit_reads_and_reject_helpers_and_environment_access() {
        for script in ["{print $1}", "{ print $1, $2 }", "{print $0}"] {
            assert!(awk(script));
        }
        for script in [
            "{print ENVIRON}",
            "{print $1 > \"out\"}",
            "{system(\"id\")}",
            "{print $1; getline}",
        ] {
            assert!(!awk(script));
        }
        for script in [
            ".",
            ".name",
            ".items[].name",
            ".items[0]",
            ".[\"some-key\"]",
            ".items | length",
            "keys",
        ] {
            assert!(jq(script), "{script}");
        }
        for script in [
            "env",
            "$ENV",
            "input",
            ". | inputs",
            "include \"helper\"; .",
            ".[env.KEY]",
            ".[env:]",
            ".[input:]",
            ".foo..",
            ".foo | debug",
        ] {
            assert!(!jq(script), "{script}");
        }
    }
}
