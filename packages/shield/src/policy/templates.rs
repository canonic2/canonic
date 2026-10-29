//! `{{name}}` placeholders in pack reasons and guidance, rendered during preparation.
use super::{Error, variables};

/// Whether text contains anything a template parser would read.
pub(crate) fn templated(text: &str) -> bool {
    text.contains("{{") || text.contains("}}")
}

/// Render placeholders through `lookup`; None when a referenced value is unset.
pub(crate) fn render(
    text: &str,
    lookup: &mut dyn FnMut(&str) -> Result<Option<String>, Error>,
) -> Result<Option<String>, Error> {
    let mut output = String::with_capacity(text.len());
    let mut rest = text;
    let mut complete = true;
    while let Some(open) = rest.find("{{") {
        let (literal, tail) = rest.split_at(open);
        if literal.contains("}}") {
            return Err(Error::Contract("unbalanced template braces"));
        }
        output.push_str(literal);
        let tail = &tail[2..];
        let close = tail
            .find("}}")
            .ok_or(Error::Contract("unbalanced template braces"))?;
        let name = &tail[..close];
        if !variables::valid_name(name) {
            return Err(Error::Contract("invalid template placeholder"));
        }
        match lookup(name)? {
            Some(value) => output.push_str(&value),
            None => complete = false,
        }
        rest = &tail[close + 2..];
    }
    if rest.contains("}}") {
        return Err(Error::Contract("unbalanced template braces"));
    }
    output.push_str(rest);
    Ok(complete.then_some(output))
}
