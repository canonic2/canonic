//! Preserve policy document structural budgets for the strict JSON parsing path.

use super::Error;
use serde_json::Value;

pub(super) fn check(value: &Value) -> Result<(), Error> {
    // YAML stream/document start and end contribute four events.
    visit(value, 0, &mut 0, &mut 4)
}

fn visit(value: &Value, depth: usize, nodes: &mut usize, events: &mut usize) -> Result<(), Error> {
    *nodes += 1;
    *events += 1;
    if *nodes > 16_000_000 || *events > 32_000_000 {
        return Err(Error::Limit);
    }
    if value.is_array() || value.is_object() {
        if depth >= 64 {
            return Err(Error::Limit);
        }
        // Every container also produces an end event.
        *events += 1;
        if *events > 32_000_000 {
            return Err(Error::Limit);
        }
    }
    match value {
        Value::Array(items) => {
            for item in items {
                visit(item, depth + 1, nodes, events)?;
            }
        }
        Value::Object(items) => {
            for value in items.values() {
                // Mapping keys are scalar nodes/events, too.
                *nodes += 1;
                *events += 1;
                visit(value, depth + 1, nodes, events)?;
            }
        }
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn containers_and_mapping_keys_count_toward_resource_limits() {
        let pair = serde_json::json!({"key":0});
        assert!(visit(&pair, 0, &mut 15_999_997, &mut 4).is_ok());
        assert!(matches!(
            visit(&pair, 0, &mut 15_999_998, &mut 4),
            Err(Error::Limit)
        ));
        assert!(visit(&Value::Null, 0, &mut 0, &mut 31_999_999).is_ok());
        assert!(matches!(
            visit(&serde_json::json!([]), 0, &mut 0, &mut 31_999_999),
            Err(Error::Limit)
        ));
        assert!(matches!(
            visit(&pair, 64, &mut 0, &mut 4),
            Err(Error::Limit)
        ));
    }
}
