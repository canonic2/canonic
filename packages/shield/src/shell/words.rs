//! Literal word and assignment decoding shared by parsing, expansion and wrappers.
use super::{Dialect, Error};

pub(super) fn assignment(value: &str) -> bool {
    value
        .split_once('=')
        .is_some_and(|(name, _)| identifier(name))
}

pub(in crate::shell) fn identifier(name: &str) -> bool {
    !name.is_empty()
        && name.bytes().enumerate().all(|(index, byte)| {
            byte == b'_' || byte.is_ascii_alphabetic() || (index > 0 && byte.is_ascii_digit())
        })
}

// Any unsupported token abandons the entire extraction, preventing a safe prefix
// from granting allowance. This is not a general shell syntax validator.
pub(super) fn literal_words(source: &str, dialect: Dialect) -> Result<Option<Vec<String>>, Error> {
    let mut chars = source.chars().peekable();
    let mut quote = None;
    let mut word = String::new();
    let mut started = false;
    let mut words = Vec::new();
    while let Some(ch) = chars.next() {
        match (quote, ch) {
            (Some('\''), '\'') | (Some('"'), '"') => quote = None,
            (Some('\''), _) => word.push(ch),
            (None, '\'' | '"') => {
                quote = Some(ch);
                started = true;
            }
            (None, ' ' | '\t') => {
                if started {
                    push_word(&mut words, &mut word)?;
                    started = false;
                }
            }
            (None, '$') if dialect == Dialect::Bash && chars.peek() == Some(&'\'') => {
                chars.next();
                let Some(value) = ansi_literal(&mut chars)? else {
                    return Ok(None);
                };
                word.push_str(&value);
                started = true;
            }
            (None | Some('"'), '\\') => {
                let next = chars
                    .next()
                    .ok_or(Error::Contract("unfinished shell escape"))?;
                if next != '\n' {
                    if quote == Some('"') && !matches!(next, '$' | '`' | '"' | '\\') {
                        word.push('\\');
                    }
                    word.push(next);
                    started = true;
                }
            }
            (Some('"'), '!') if dialect == Dialect::Bash => return Ok(None),
            // Tilde expansion is a word-prefix operation (or assignment value),
            // not an embedded character in ordinary arguments such as HEAD~3.
            (None, '~') if !word.is_empty() && !word.contains('=') => word.push(ch),
            // A standalone bracket cannot form a pathname pattern. In
            // particular, `[` is the test command, not an unknown executable.
            (None, '[' | ']')
                if !started && chars.peek().is_none_or(|next| matches!(next, ' ' | '\t')) =>
            {
                word.push(ch);
                started = true;
            }
            (Some('"'), '$' | '`')
            | (
                None,
                '\n' | ';' | '&' | '|' | '(' | ')' | '{' | '}' | '<' | '>' | '$' | '`' | '*' | '?'
                | '[' | ']' | '~' | '!',
            ) => return Ok(None),
            (None, '#') if !started => return Ok(None),
            _ => {
                word.push(ch);
                started = true;
            }
        }
        if word.len() > 4096 {
            return Err(Error::Limit);
        }
    }
    if quote.is_some() {
        return Err(Error::Contract("unfinished shell quote"));
    }
    if started {
        push_word(&mut words, &mut word)?;
    }
    if words.is_empty() {
        return Err(Error::Contract("empty shell invocation"));
    }
    Ok(Some(words))
}

fn ansi_literal(
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
) -> Result<Option<String>, Error> {
    let mut value = String::new();
    let mut truncated = false;
    while let Some(ch) = chars.next() {
        if truncated {
            match ch {
                '\'' => return Ok(Some(value)),
                '\\' => {
                    chars
                        .next()
                        .ok_or(Error::Contract("unfinished ANSI quote"))?;
                }
                _ => {}
            }
            continue;
        }
        match ch {
            '\'' => return Ok(Some(value)),
            '\\' => {
                let escape = chars
                    .next()
                    .ok_or(Error::Contract("unfinished ANSI quote"))?;
                match escape {
                    'a' => value.push('\u{7}'),
                    'b' => value.push('\u{8}'),
                    'e' | 'E' => value.push('\u{1b}'),
                    'f' => value.push('\u{c}'),
                    'n' => value.push('\n'),
                    '\n' | 'u' | 'U' => return Ok(None),
                    'c' => {
                        let next = chars
                            .next()
                            .ok_or(Error::Contract("unfinished control escape"))?;
                        let Some(control) = ansi_control(next) else {
                            return Ok(None);
                        };
                        if control == '\0' {
                            truncated = true;
                        } else {
                            value.push(control);
                        }
                    }
                    'r' => value.push('\r'),
                    't' => value.push('\t'),
                    'v' => value.push('\u{b}'),
                    '\\' | '\'' | '"' | '?' => value.push(escape),
                    'x' | '0'..='7' => {
                        let octal = escape.is_ascii_digit();
                        let radix = if octal { 8 } else { 16 };
                        let maximum = match escape {
                            'x' => 2,
                            _ => 3,
                        };
                        let mut digits = if octal {
                            escape.to_string()
                        } else {
                            String::new()
                        };
                        while digits.len() < maximum
                            && chars.peek().is_some_and(|ch| ch.is_digit(radix))
                        {
                            digits.push(chars.next().ok_or(Error::Contract("missing ANSI digit"))?);
                        }
                        let Some(code) = u32::from_str_radix(&digits, radix)
                            .ok()
                            .filter(|code| *code <= 127)
                        else {
                            // Non-ASCII byte/Unicode conversion depends on unobserved locale.
                            return Ok(None);
                        };
                        if code == 0 {
                            truncated = true;
                            continue;
                        }
                        value.push(
                            char::from_u32(code)
                                .ok_or(Error::Contract("invalid ANSI character"))?,
                        );
                    }
                    _ => {
                        value.push('\\');
                        value.push(escape);
                    }
                }
            }
            _ => value.push(ch),
        }
        if value.len() > 4096 {
            return Err(Error::Limit);
        }
    }
    Err(Error::Contract("unfinished ANSI quote"))
}

fn ansi_control(ch: char) -> Option<char> {
    let ch = ch.to_ascii_uppercase();
    // Other characters (notably '?') have version-specific decoding.
    (ch.is_ascii_uppercase() || matches!(ch, '@' | '[' | ']' | '^' | '_'))
        .then(|| {
            u8::try_from(u32::from(ch))
                .ok()
                .map(|code| char::from(code & 31))
        })
        .flatten()
}

fn push_word(words: &mut Vec<String>, word: &mut String) -> Result<(), Error> {
    if words.len() >= 257 {
        return Err(Error::Limit);
    }
    words.push(std::mem::take(word));
    Ok(())
}
