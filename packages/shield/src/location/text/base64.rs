//! GNU and macOS Base64 option arity and explicit file effects. Encoding,
//! decoding, wrapping and garbage handling never execute the decoded bytes.
use super::Parsed;

pub(super) fn parse(arguments: &[String]) -> Parsed {
    parse_with(arguments, cfg!(target_os = "macos"))
}

fn parse_with(arguments: &[String], macos: bool) -> Parsed {
    let mut parsed = Parsed {
        complete: cfg!(unix),
        stream: true,
        ..Parsed::default()
    };
    let mut words = arguments.iter().map(String::as_str);
    let mut options = true;
    while let Some(word) = words.next() {
        if options && word == "--" {
            options = false;
        } else if !options || word == "-" || !word.starts_with('-') {
            parsed.operands.push(word.into());
        } else if let Some(long) = word.strip_prefix("--") {
            let (name, attached) = long
                .split_once('=')
                .map_or((long, None), |(k, v)| (k, Some(v)));
            match name {
                "input" | "output" | "break" | "wrap" => {
                    value(&mut parsed, name, attached.or_else(|| words.next()));
                }
                "decode" | "help" | "version" | "ignore-garbage" if attached.is_none() => {}
                _ => parsed.complete = false,
            }
        } else {
            for (index, flag) in word.char_indices().skip(1) {
                let name = match flag {
                    // GNU -i ignores garbage; macOS -i takes an input file.
                    'i' if !macos => continue,
                    'i' => "input",
                    'o' => "output",
                    'b' => "break",
                    'w' => "wrap",
                    'd' | 'D' | 'h' => continue,
                    _ => {
                        parsed.complete = false;
                        continue;
                    }
                };
                let attached = &word[index + flag.len_utf8()..];
                value(
                    &mut parsed,
                    name,
                    if attached.is_empty() {
                        words.next()
                    } else {
                        Some(attached)
                    },
                );
                break;
            }
        }
    }
    // Neither implementation processes multiple positional input files.
    parsed.complete &= parsed.operands.len() <= 1;
    parsed
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn base64_gnu_and_macos_option_arity_preserves_every_file() {
        for (macos, words, expected) in [
            (false, vec!["-di", "input"], vec![(false, "input")]),
            (false, vec!["-i"], vec![]),
            (
                true,
                vec!["-diinput", "-oout"],
                vec![(false, "input"), (true, "out")],
            ),
            (
                true,
                vec!["-i", "input", "--output=out"],
                vec![(false, "input"), (true, "out")],
            ),
            (true, vec!["-i", "-", "-o", "-"], vec![]),
            (
                true,
                vec!["-b72", "-oone", "-otwo"],
                vec![(true, "one"), (true, "two")],
            ),
        ] {
            let arguments = words.iter().map(|w| (*w).into()).collect::<Vec<_>>();
            let parsed = parse_with(&arguments, macos);
            assert!(parsed.complete, "{macos} {words:?}");
            assert_eq!(
                parsed.files(),
                expected
                    .iter()
                    .map(|(write, path)| (*write, (*path).into()))
                    .collect::<Vec<_>>()
            );
        }
        assert!(!parse_with(&["-i".into()], true).complete);
    }
}

fn value(parsed: &mut Parsed, name: &str, value: Option<&str>) {
    let Some(value) = value.filter(|v| !v.is_empty()) else {
        parsed.complete = false;
        return;
    };
    match name {
        "input" if value != "-" => parsed.reads.push(value.into()),
        "output" if value != "-" => parsed.writes.push(value.into()),
        "input" | "output" => {}
        _ => parsed.complete &= value.bytes().all(|b| b.is_ascii_digit()),
    }
}
