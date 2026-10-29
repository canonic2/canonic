//! Pure scalar double-quote expansion with known bindings; no splitting or execution.

use super::{Dialect, Error, State, structure::Word};

pub(super) const DEFAULT_IFS: &str = " \t\n";

/// Recognize status-only uncertainty without inventing a literal runtime value.
pub(super) fn status_only(word: &Word, dialect: Dialect) -> Result<bool, Error> {
    let mut found = false;
    for fragment in &word.fragments {
        if matches!(fragment.raw.as_str(), "$?" | "${?}") {
            found = true;
        } else if fragment.kind == "double-quoted" {
            let Some(inner) = fragment
                .raw
                .strip_prefix('"')
                .and_then(|s| s.strip_suffix('"'))
            else {
                return Ok(false);
            };
            let mut chars = inner.chars().peekable();
            while let Some(ch) = chars.next() {
                match ch {
                    '\\' => {
                        chars.next();
                    }
                    '`' => return Ok(false),
                    '$' => {
                        match chars.next() {
                            Some('?') => {}
                            Some('{') if chars.next() == Some('?') && chars.next() == Some('}') => {
                            }
                            _ => return Ok(false),
                        }
                        found = true;
                    }
                    _ => {}
                }
            }
        } else if super::literal_words(&fragment.raw, dialect)?.is_none() {
            return Ok(false);
        }
    }
    Ok(found)
}

/// Unknown arithmetic and assigning parameters may alter the current shell.
pub(super) fn may_mutate(word: &Word, state: &State) -> bool {
    word.literal.is_none()
        && word.fragments.iter().any(|fragment| match fragment.kind {
            "arithmetic-expansion" => fragment
                .raw
                .strip_prefix("$((")
                .and_then(|value| value.strip_suffix("))"))
                .and_then(super::arithmetic::constant)
                .is_none(),
            "parameter-expansion" => fragment.raw.contains(['=', '[']),
            "double-quoted" => {
                fragment.raw.contains("${") && fragment.raw.contains(['=', '['])
                    || fragment.raw.contains("$((") && quoted_scalar(&fragment.raw, state).is_none()
            }
            _ => false,
        })
}

pub(super) fn scalar_word(
    word: &Word,
    state: &State,
    dialect: Dialect,
) -> Result<Option<String>, Error> {
    scalar(word, state, dialect, false)
}

/// Quoted positional vectors concatenate at their first/last field, never split.
pub(super) fn argument_words(
    word: &Word,
    state: &State,
    dialect: Dialect,
    checkpoint: &dyn Fn() -> Result<(), Error>,
) -> Result<Option<Vec<Option<String>>>, Error> {
    checkpoint()?;
    if word.raw.len() > 4096 {
        return Err(Error::Limit);
    }
    if word.fragments.is_empty() {
        return Ok(None);
    }
    if word.fragments.len() == 1
        && matches!(
            word.fragments[0].kind,
            "unquoted" | "parameter-expansion" | "arithmetic-expansion"
        )
    {
        return unquoted_word(word, state, checkpoint);
    }
    // Older Bash can discard adjacent empty quote fragments with zero `$@`.
    // Keep that version-sensitive shape opaque rather than inventing a field.
    if state.positionals.as_ref().is_some_and(Vec::is_empty)
        && word.fragments.len() > 1
        && word
            .fragments
            .iter()
            .any(|fragment| fragment.kind == "double-quoted" && fragment.raw.contains('@'))
    {
        return Ok(None);
    }
    let mut result = Vec::new();
    for fragment in &word.fragments {
        checkpoint()?;
        let values = if fragment.kind == "double-quoted" {
            quoted_arguments(&fragment.raw, state)
        } else {
            let mut scalar = word.clone();
            scalar.fragments = vec![fragment.clone()];
            scalar_word(&scalar, state, dialect)?.map(|value| vec![Some(value)])
        };
        let Some(values) = values else {
            return Ok(None);
        };
        concatenate(&mut result, values);
        if result.len() > 256 || result.iter().flatten().any(|value| value.len() > 4096) {
            return Err(Error::Limit);
        }
    }
    Ok(Some(result))
}

fn unquoted_word(
    word: &Word,
    state: &State,
    checkpoint: &dyn Fn() -> Result<(), Error>,
) -> Result<Option<Vec<Option<String>>>, Error> {
    let raw = word.raw.as_str();
    if raw == "~" || raw.starts_with("~/") {
        let Some(home) = state
            .variables
            .get("HOME")
            .and_then(super::provenance::Binding::literal)
        else {
            return Ok(None);
        };
        let suffix = &raw[1..];
        if suffix.contains(['$', '`', '*', '?', '[', '\\', '"', '\'']) {
            return Ok(None);
        }
        let value = format!("{home}{suffix}");
        if value.len() > 4096 {
            return Err(Error::Limit);
        }
        return Ok(Some(vec![Some(value)]));
    }
    if !raw.starts_with('$') {
        return Ok(None);
    }
    let Some(ifs) = state
        .variables
        .get("IFS")
        .and_then(super::provenance::Binding::literal)
        .filter(|value| value.is_ascii())
    else {
        return Ok(None);
    };
    let values = if matches!(raw, "$@" | "${@}" | "$*" | "${*}") {
        let Some(args) = &state.positionals else {
            return Ok(None);
        };
        let Some(args) = args.iter().cloned().collect::<Option<Vec<_>>>() else {
            return Ok(None);
        };
        args
    } else {
        let Some(value) = quoted_scalar(&format!("\"{raw}\""), state) else {
            return Ok(None);
        };
        // Splitting applies to expansion results, not appended source literals.
        let whole = raw.starts_with("${") && raw.ends_with('}')
            || raw.starts_with("$((") && raw.ends_with("))")
            || raw.strip_prefix('$').is_some_and(|name| {
                let first = name.as_bytes().first().copied();
                if first.is_some_and(|ch| ch.is_ascii_digit() || ch == b'#') {
                    name.len() == 1
                } else {
                    first.is_some_and(|ch| ch == b'_' || ch.is_ascii_alphabetic())
                        && name
                            .chars()
                            .all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
                }
            });
        if !whole {
            return Ok(None);
        }
        vec![value]
    };
    let mut result = Vec::new();
    for value in values {
        checkpoint()?;
        if value.len() > 4096 {
            return Err(Error::Limit);
        }
        // Bash splits on the default IFS but zsh does not, so only values that
        // both shells leave as one field resolve.
        if value.contains(['*', '?', '['])
            || ifs == DEFAULT_IFS && value.contains([' ', '\t', '\n'])
        {
            return Ok(None);
        }
        result.extend(split_fields(&value, ifs, checkpoint)?.into_iter().map(Some));
        if result.len() > 256 {
            return Err(Error::Limit);
        }
    }
    Ok(Some(result))
}

fn split_fields(
    value: &str,
    ifs: &str,
    checkpoint: &dyn Fn() -> Result<(), Error>,
) -> Result<Vec<String>, Error> {
    let whitespace = |ch: char| matches!(ch, ' ' | '\t' | '\n') && ifs.contains(ch);
    let mut chars = value.trim_matches(whitespace).chars().peekable();
    let mut result = Vec::new();
    let mut field = String::new();
    let mut steps = 0;
    while let Some(ch) = chars.next() {
        steps += 1;
        if steps % 128 == 0 {
            checkpoint()?;
        }
        if !ifs.contains(ch) {
            field.push(ch);
            continue;
        }
        let mut nonwhite = !whitespace(ch);
        if !nonwhite {
            while chars.peek().is_some_and(|ch| whitespace(*ch)) {
                chars.next();
            }
            if chars.peek().is_some_and(|ch| ifs.contains(*ch)) {
                chars.next();
                nonwhite = true;
            }
        }
        while chars.peek().is_some_and(|ch| whitespace(*ch)) {
            chars.next();
        }
        if nonwhite || !field.is_empty() {
            result.push(std::mem::take(&mut field));
        }
    }
    if !field.is_empty() {
        result.push(field);
    }
    Ok(result)
}

fn concatenate(result: &mut Vec<Option<String>>, mut values: Vec<Option<String>>) {
    if result.is_empty() {
        result.append(&mut values);
    } else if !values.is_empty() {
        let first = values.remove(0);
        let last = result.last_mut().expect("nonempty result");
        *last = last
            .as_ref()
            .zip(first)
            .map(|(left, right)| format!("{left}{right}"));
        result.append(&mut values);
    }
}

fn quoted_arguments(raw: &str, state: &State) -> Option<Vec<Option<String>>> {
    let inner = raw.strip_prefix('"')?.strip_suffix('"')?;
    let mut result = Vec::new();
    let mut offset = 0;
    let mut start = 0;
    let mut vector = false;
    while offset < inner.len() {
        let rest = &inner[offset..];
        if rest.starts_with('\\') {
            offset += 1;
            offset += inner[offset..].chars().next()?.len_utf8();
            continue;
        }
        let token = ["$@", "${@}", "$*", "${*}"]
            .into_iter()
            .find(|token| rest.starts_with(token));
        let Some(token) = token else {
            offset += rest.chars().next()?.len_utf8();
            continue;
        };
        let prefix = quoted_scalar(&format!("\"{}\"", &inner[start..offset]), state)?;
        if !prefix.is_empty() {
            concatenate(&mut result, vec![Some(prefix)]);
        }
        let arguments = state.positionals.as_ref()?;
        let values = if token.contains('@') {
            arguments.clone()
        } else {
            let args: Option<Vec<_>> = arguments.iter().cloned().collect();
            let args = args?;
            let separator = if args.len() < 2 {
                String::new()
            } else {
                let ifs = state.variables.get("IFS")?.literal()?;
                if !ifs.is_ascii() {
                    return None;
                }
                ifs.chars()
                    .next()
                    .map_or_else(String::new, |ch| ch.to_string())
            };
            vec![Some(args.join(&separator))]
        };
        concatenate(&mut result, values);
        offset += token.len();
        start = offset;
        vector = true;
    }
    let suffix = quoted_scalar(&format!("\"{}\"", &inner[start..]), state)?;
    if !vector || !suffix.is_empty() {
        concatenate(&mut result, vec![Some(suffix)]);
    }
    Some(result)
}

pub(super) fn assignment_word(
    word: &Word,
    state: &State,
    dialect: Dialect,
) -> Result<Option<String>, Error> {
    if let Some(value) = &word.literal {
        return Ok(Some(value.clone()));
    }
    scalar(word, state, dialect, true)
}

fn scalar(
    word: &Word,
    state: &State,
    dialect: Dialect,
    assignment: bool,
) -> Result<Option<String>, Error> {
    if word.fragments.is_empty() {
        return Ok(None);
    }
    let mut result = String::new();
    for fragment in &word.fragments {
        let value = if fragment.kind == "double-quoted" {
            quoted_scalar(&fragment.raw, state)
        } else if assignment
            && (fragment.kind == "parameter-expansion"
                || fragment.kind == "unquoted" && fragment.raw.starts_with('$'))
        {
            // Assignment values do not undergo field splitting or globbing.
            quoted_scalar(&format!("\"{}\"", fragment.raw), state)
        } else if fragment.kind == "arithmetic-expansion" {
            fragment
                .raw
                .strip_prefix("$((")
                .and_then(|value| value.strip_suffix("))"))
                .and_then(super::arithmetic::constant)
        } else {
            super::literal_words(&fragment.raw, dialect)?.and_then(|mut words| {
                if words.len() == 1 { words.pop() } else { None }
            })
        };
        let Some(value) = value else { return Ok(None) };
        result.push_str(&value);
        if result.len() > 4096 {
            return Err(Error::Limit);
        }
    }
    Ok(Some(result))
}

pub(super) fn quoted_scalar(raw: &str, state: &State) -> Option<String> {
    let inner = raw.strip_prefix('"')?.strip_suffix('"')?;
    let mut chars = inner.chars().peekable();
    let mut result = String::new();
    while let Some(ch) = chars.next() {
        match ch {
            '"' | '`' => return None,
            '\\' => {
                let next = chars.next()?;
                match next {
                    '\n' => {}
                    '$' | '`' | '"' | '\\' => result.push(next),
                    _ => {
                        result.push('\\');
                        result.push(next);
                    }
                }
            }
            '$' => {
                let Some(next) = chars.peek().copied() else {
                    result.push('$');
                    continue;
                };
                let mut name = String::new();
                if next == '{' {
                    chars.next();
                    loop {
                        let next = chars.next()?;
                        if next == '}' {
                            break;
                        }
                        name.push(next);
                    }
                } else if next.is_ascii_digit() || next == '#' {
                    name.push(chars.next()?);
                } else if next == '_' || next.is_ascii_alphabetic() {
                    while chars
                        .peek()
                        .is_some_and(|ch| *ch == '_' || ch.is_ascii_alphanumeric())
                    {
                        name.push(chars.next()?);
                    }
                } else if next == '(' {
                    result.push_str(&arithmetic_inline(&mut chars)?);
                    continue;
                } else if matches!(next, '@' | '*' | '?' | '$' | '!' | '-') {
                    return None;
                } else {
                    result.push('$');
                    continue;
                }
                result.push_str(&parameter(&name, state)?);
            }
            _ => result.push(ch),
        }
        if result.len() > 4096 {
            return None;
        }
    }
    Some(result)
}

fn arithmetic_inline(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> Option<String> {
    if chars.next()? != '(' || chars.next()? != '(' {
        return None;
    }
    let mut depth = 1usize;
    let mut source = String::new();
    for ch in chars.by_ref() {
        if ch == '(' {
            depth += 1;
        }
        if ch == ')' {
            depth -= 1;
        }
        if depth == 0 {
            return (chars.next()? == ')')
                .then(|| super::arithmetic::constant(&source))
                .flatten();
        }
        if depth > 32 || source.len() > 4096 {
            return None;
        }
        source.push(ch);
    }
    None
}

fn parameter(name: &str, state: &State) -> Option<String> {
    if let Some(name) = name.strip_prefix('#') {
        if name.contains(['#', '%']) {
            return None;
        }
        let value = parameter(name, state)?;
        return value.is_ascii().then(|| value.len().to_string());
    }
    for operator in ["%%", "%", "##", "#"] {
        if let Some((name, pattern)) = name.split_once(operator) {
            if pattern.contains(['$', '`', '\\', '"', '\'', '*', '?', '[', ']']) {
                return None;
            }
            let value = parameter(name, state)?;
            return Some(if operator.starts_with('%') {
                value.strip_suffix(pattern).unwrap_or(&value).to_owned()
            } else {
                value.strip_prefix(pattern).unwrap_or(&value).to_owned()
            });
        }
    }
    if let Some((name, operator, fallback)) = parameter_operator(name) {
        if fallback.contains(['$', '`', '\\', '"', '\'', '{', '}']) {
            return None;
        }
        let (set, value) = if name.bytes().all(|byte| byte.is_ascii_digit()) && name != "0" {
            let index = name.parse::<usize>().ok()?.checked_sub(1)?;
            match state.positionals.as_ref()?.get(index) {
                Some(value) => (true, value.clone()?),
                None => (false, String::new()),
            }
        } else {
            (true, parameter(name, state)?)
        };
        let present = set && (!operator.starts_with(':') || !value.is_empty());
        return Some(if operator.ends_with('-') {
            if present { value } else { fallback.into() }
        } else if present {
            fallback.into()
        } else {
            String::new()
        });
    }
    match name {
        "0" => state.arg0.clone(),
        "#" => state
            .positionals
            .as_ref()
            .map(|args| args.len().to_string()),
        value if !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()) => {
            let index = value.parse::<usize>().ok()?.checked_sub(1)?;
            state
                .positionals
                .as_ref()?
                .get(index)
                .cloned()
                .unwrap_or(Some(String::new()))
        }
        value
            if value.bytes().enumerate().all(|(index, byte)| {
                byte == b'_' || byte.is_ascii_alphabetic() || (index > 0 && byte.is_ascii_digit())
            }) && !value.is_empty() =>
        {
            (!super::bindings::dynamic_variable(value))
                .then(|| {
                    state
                        .variables
                        .get(value)
                        .and_then(super::provenance::Binding::literal)
                        .map(str::to_owned)
                })
                .flatten()
        }
        _ => None,
    }
}

fn parameter_operator(value: &str) -> Option<(&str, &str, &str)> {
    for operator in [":-", ":+", "-", "+"] {
        if let Some((name, fallback)) = value.split_once(operator) {
            let valid = if name
                .bytes()
                .next()
                .is_some_and(|byte| byte.is_ascii_digit())
            {
                name.bytes().all(|byte| byte.is_ascii_digit())
            } else {
                !name.is_empty()
                    && name.bytes().enumerate().all(|(index, byte)| {
                        byte == b'_'
                            || byte.is_ascii_alphabetic()
                            || index > 0 && byte.is_ascii_digit()
                    })
            };
            if !valid {
                return None;
            }
            return Some((name, operator, fallback));
        }
    }
    None
}
