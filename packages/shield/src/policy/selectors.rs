//! Strict, reusable selectors. Patterns are prepared by the owning policy snapshot.
use super::{Error, patterns::Provider};
use serde_json::Value;
use std::collections::BTreeSet;

pub(super) fn fields(value: &Value, allowed: &[&str]) -> Result<(), Error> {
    if value
        .as_object()
        .is_none_or(|v| v.keys().any(|k| !allowed.contains(&k.as_str())))
    {
        return Err(Error::Contract("unknown predicate fields"));
    }
    Ok(())
}

/// The selector values of an `environment` predicate. Tree walkers descend
/// only into these: `fields` keys are variable names, never predicate keys,
/// and `exact` holds literal data.
pub(super) fn environment_selectors(environment: &Value) -> impl Iterator<Item = &Value> {
    environment
        .get("fields")
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(serde_json::Map::values)
}

/// [`environment_selectors`] for walkers that rewrite selectors in place.
pub(super) fn environment_selectors_mut(
    environment: &mut Value,
) -> impl Iterator<Item = &mut Value> {
    environment
        .get_mut("fields")
        .and_then(Value::as_object_mut)
        .into_iter()
        .flat_map(serde_json::Map::values_mut)
}

pub(super) fn text(value: &Value, nonempty: bool) -> Result<(), Error> {
    if let Some(s) = value.as_str() {
        return literal(s, nonempty);
    }
    fields(
        value,
        &["exact", "prefix", "one_of", "regex", "glob", "not"],
    )?;
    let object = value.as_object().ok_or(Error::Syntax)?;
    if object.len() != 1 {
        return Err(Error::Contract("one text selector required"));
    }
    let (kind, operand) = object.iter().next().ok_or(Error::Syntax)?;
    if kind == "not" {
        if operand.get("not").is_some() {
            return Err(Error::Contract("not cannot contain another not"));
        }
        text(operand, nonempty)?;
    } else if kind == "one_of" {
        strings(operand, true, nonempty)?;
    } else {
        literal(
            operand.as_str().ok_or(Error::Syntax)?,
            nonempty || kind == "prefix",
        )?;
    }
    Ok(())
}

pub(super) fn literal(value: &str, nonempty: bool) -> Result<(), Error> {
    if value.len() > 4096 {
        return Err(Error::Limit);
    }
    if value.contains('\0') || (nonempty && value.is_empty()) {
        return Err(Error::Contract("invalid selector text"));
    }
    Ok(())
}

pub(super) fn strings(value: &Value, nonempty: bool, members_nonempty: bool) -> Result<(), Error> {
    let values = value.as_array().ok_or(Error::Syntax)?;
    if values.len() > 256 {
        return Err(Error::Limit);
    }
    if nonempty && values.is_empty() {
        return Err(Error::Contract("nonempty list required"));
    }
    let mut seen = BTreeSet::new();
    for v in values {
        let s = v.as_str().ok_or(Error::Syntax)?;
        literal(s, members_nonempty)?;
        if !seen.insert(s) {
            return Err(Error::Contract("duplicate selector member"));
        }
    }
    Ok(())
}

pub(super) fn exact(value: &Value) -> Option<&str> {
    value
        .as_str()
        .or_else(|| value.get("exact").and_then(Value::as_str))
}

pub(crate) fn matches(
    selector: &Value,
    value: &str,
    patterns: &dyn Provider,
) -> Result<bool, Error> {
    // A negation decides only a present value; missing evidence never reaches
    // here, so `not` cannot turn an unknown field into a match.
    if let Some(inner) = selector.get("not") {
        return Ok(!matches(inner, value, patterns)?);
    }
    if let Some(literal) = exact(selector) {
        return Ok(literal == value);
    }
    if let Some(prefix) = selector["prefix"].as_str() {
        return Ok(value.starts_with(prefix));
    }
    if let Some(items) = selector["one_of"].as_array() {
        return Ok(items.iter().any(|v| v.as_str() == Some(value)));
    }
    for kind in ["regex", "glob"] {
        if let Some(source) = selector[kind].as_str() {
            return patterns.matches(&format!("{kind}:{source}"), value);
        }
    }
    Err(Error::Contract("invalid prepared text selector"))
}

pub(super) fn field_map(value: &Value) -> Result<(), Error> {
    let fields = value.as_object().ok_or(Error::Syntax)?;
    if fields.is_empty() || fields.len() > 256 {
        return Err(Error::Limit);
    }
    for (pointer, predicate) in fields {
        pointer_validate(pointer)?;
        self::fields(predicate, &["exact", "text"])?;
        if predicate.as_object().is_none_or(|v| v.len() != 1) {
            return Err(Error::Syntax);
        }
        if let Some(selector) = predicate.get("text") {
            text(selector, false)?;
        }
    }
    Ok(())
}

fn pointer_validate(pointer: &str) -> Result<(), Error> {
    if pointer.len() > 1024 {
        return Err(Error::Limit);
    }
    if (!pointer.is_empty() && !pointer.starts_with('/')) || pointer.contains('\0') {
        return Err(Error::Contract("invalid JSON pointer"));
    }
    for part in pointer.split('/').skip(1) {
        let mut chars = part.chars();
        while let Some(c) = chars.next() {
            if c == '~' && !matches!(chars.next(), Some('0' | '1')) {
                return Err(Error::Contract("invalid JSON pointer escape"));
            }
        }
    }
    Ok(())
}

pub(super) fn fields_match(
    fields: &Value,
    input: &Value,
    patterns: &dyn Provider,
) -> Result<Option<&'static str>, Error> {
    for (pointer, predicate) in fields.as_object().ok_or(Error::Syntax)? {
        let Some(value) = input.pointer(pointer) else {
            return Ok(Some("field_missing"));
        };
        if let Some(expected) = predicate.get("exact") {
            if expected != value {
                return Ok(Some("predicate_mismatch"));
            }
        } else if let Some(selector) = predicate.get("text") {
            let Some(value) = value.as_str() else {
                return Ok(Some("predicate_mismatch"));
            };
            if !matches(selector, value, patterns)? {
                return Ok(Some("predicate_mismatch"));
            }
        } else {
            return Err(Error::Syntax);
        }
    }
    Ok(None)
}
