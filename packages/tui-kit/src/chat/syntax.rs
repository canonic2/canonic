//! Supplies bounded lexical color hints with original text and plain unknown-language fallback.

use super::content::{Ink, Run};
pub(super) fn highlight(text: &str, language: &str) -> Vec<Run> {
    if !matches!(
        language,
        "ts" | "typescript"
            | "js"
            | "javascript"
            | "rs"
            | "rust"
            | "py"
            | "python"
            | "sh"
            | "bash"
            | "json"
    ) {
        return vec![Run {
            text: text.into(),
            ink: Ink::default(),
            generated: false,
        }];
    }
    let mut runs: Vec<Run> = Vec::new();
    let mut offset = 0;
    while offset < text.len() {
        let tail = &text[offset..];
        let first = tail.chars().next().unwrap_or(' ');
        let (length, ink) = if tail.starts_with("//")
            || (matches!(language, "sh" | "bash" | "py" | "python") && tail.starts_with('#'))
        {
            (
                tail.find('\n').unwrap_or(tail.len()),
                Ink {
                    muted: true,
                    ..Ink::default()
                },
            )
        } else if matches!(first, '\'' | '"' | '`') {
            let mut length = first.len_utf8();
            let mut escaped = false;
            for c in tail[first.len_utf8()..].chars() {
                length += c.len_utf8();
                if c == first && !escaped {
                    break;
                }
                escaped = c == '\\' && !escaped;
            }
            (
                length,
                Ink {
                    success: true,
                    ..Ink::default()
                },
            )
        } else if first.is_alphabetic() || first == '_' {
            let length = tail
                .char_indices()
                .find(|(_, c)| !c.is_alphanumeric() && *c != '_')
                .map_or(tail.len(), |(i, _)| i);
            let keyword = keyword(&tail[..length]);
            (
                length,
                Ink {
                    info: keyword,
                    ..Ink::default()
                },
            )
        } else {
            (first.len_utf8(), Ink::default())
        };
        if let Some(run) = runs.last_mut().filter(|run| run.ink == ink) {
            run.text.push_str(&tail[..length]);
        } else {
            runs.push(Run {
                text: tail[..length].into(),
                ink,
                generated: false,
            });
        }
        offset += length;
    }
    runs
}

fn keyword(word: &str) -> bool {
    matches!(
        word,
        "fn" | "pub"
            | "let"
            | "mut"
            | "impl"
            | "use"
            | "struct"
            | "enum"
            | "match"
            | "return"
            | "if"
            | "else"
            | "for"
            | "while"
            | "async"
            | "await"
            | "const"
            | "function"
            | "class"
            | "import"
            | "from"
            | "export"
            | "def"
            | "in"
            | "true"
            | "false"
            | "null"
            | "None"
    )
}
