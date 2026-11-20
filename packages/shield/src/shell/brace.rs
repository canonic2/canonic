//! Pure bounded Bash literal brace lists and unpadded sequences.

use super::Error;

pub(super) fn expand(
    source: &str,
    checkpoint: &dyn Fn() -> Result<(), Error>,
) -> Result<Option<Vec<String>>, Error> {
    let mut output = Vec::new();
    if expand_into(source, 0, &mut output, checkpoint)? {
        Ok(Some(output))
    } else {
        Ok(None)
    }
}

fn expand_into(
    source: &str,
    depth: usize,
    output: &mut Vec<String>,
    checkpoint: &dyn Fn() -> Result<(), Error>,
) -> Result<bool, Error> {
    checkpoint()?;
    if depth > 32 || source.len() > 4096 {
        return Err(Error::Limit);
    }
    let Some(start) = source.find('{') else {
        if output.len() >= 256 {
            return Err(Error::Limit);
        }
        output.push(source.into());
        return Ok(true);
    };
    let mut level = 0;
    let mut end = None;
    let mut separators = Vec::new();
    for (index, byte) in source.bytes().enumerate().skip(start) {
        if index % 128 == 0 {
            checkpoint()?;
        }
        match byte {
            b'{' => level += 1,
            b'}' => {
                level -= 1;
                if level == 0 {
                    end = Some(index);
                    break;
                }
            }
            b',' if level == 1 => separators.push(index),
            _ => {}
        }
    }
    let Some(end) = end else {
        return Ok(false);
    };
    let alternatives = if separators.is_empty() {
        let inner = &source[start + 1..end];
        match sequence(inner)? {
            Some(range) => range,
            // Bash leaves a pair that is neither a list nor a sequence literal,
            // as in find's `{}` and Git's `stash@{0}` and `@{u}`; braces nested
            // in it still expand. Unsupported sequence forms stay unresolved.
            None if !inner.contains("..") => {
                return literal(source, start, end, depth, output, checkpoint);
            }
            None => return Ok(false),
        }
    } else {
        let mut alternatives = Vec::new();
        let mut cursor = start + 1;
        for separator in separators.into_iter().chain(std::iter::once(end)) {
            if cursor == separator {
                return Ok(false);
            }
            alternatives.push(source[cursor..separator].to_owned());
            cursor = separator + 1;
        }
        alternatives
    };
    for alternative in alternatives {
        let word = format!("{}{alternative}{}", &source[..start], &source[end + 1..]);
        if !expand_into(&word, depth + 1, output, checkpoint)? {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Whether brace expansion leaves the unquoted word `raw` as written, as for
/// `{}` and `stash@{0}`, so braceexpand being off would not change it.
pub(super) fn unchanged(
    raw: &str,
    checkpoint: &dyn Fn() -> Result<(), Error>,
) -> Result<bool, Error> {
    let Some(words) = expand(raw, checkpoint)? else {
        return Ok(false);
    };
    let [word] = words.as_slice() else {
        return Ok(false);
    };
    Ok(super::words::literal_words(word, super::Dialect::Bash)? == Some(vec![raw.to_owned()]))
}

/// Keeps the braces at `start` and `end` literal and expands the words inside
/// and after them. Output remains shell source, so only the braces are quoted.
fn literal(
    source: &str,
    start: usize,
    end: usize,
    depth: usize,
    output: &mut Vec<String>,
    checkpoint: &dyn Fn() -> Result<(), Error>,
) -> Result<bool, Error> {
    let (mut inner, mut rest) = (Vec::new(), Vec::new());
    if !expand_into(&source[start + 1..end], depth + 1, &mut inner, checkpoint)?
        || !expand_into(&source[end + 1..], depth + 1, &mut rest, checkpoint)?
    {
        return Ok(false);
    }
    let prefix = &source[..start];
    for word in &inner {
        for tail in &rest {
            checkpoint()?;
            if output.len() >= 256 {
                return Err(Error::Limit);
            }
            output.push(format!("{prefix}'{{'{word}'}}'{tail}"));
        }
    }
    Ok(true)
}

fn sequence(source: &str) -> Result<Option<Vec<String>>, Error> {
    let Some((from, to)) = source.split_once("..") else {
        return Ok(None);
    };
    let numbers = from.parse::<i32>().ok().zip(to.parse::<i32>().ok());
    let (from_value, to_value, letters) = if let Some((a, b)) = numbers {
        if from != a.to_string() || to != b.to_string() {
            return Ok(None);
        }
        (i64::from(a), i64::from(b), false)
    } else if from.len() == 1
        && to.len() == 1
        && from.as_bytes()[0].is_ascii_alphabetic()
        && to.as_bytes()[0].is_ascii_alphabetic()
        && from.as_bytes()[0].is_ascii_uppercase() == to.as_bytes()[0].is_ascii_uppercase()
    {
        (
            i64::from(from.as_bytes()[0]),
            i64::from(to.as_bytes()[0]),
            true,
        )
    } else {
        return Ok(None);
    };
    if (to_value - from_value).abs() >= 256 {
        return Err(Error::Limit);
    }
    let step = if from_value <= to_value { 1 } else { -1 };
    let mut result = Vec::new();
    let mut value = from_value;
    loop {
        result.push(if letters {
            char::from(u8::try_from(value).map_err(|_| Error::Limit)?).to_string()
        } else {
            value.to_string()
        });
        if value == to_value {
            break;
        }
        value += step;
    }
    Ok(Some(result))
}
