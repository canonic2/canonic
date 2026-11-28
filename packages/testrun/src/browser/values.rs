//! Bounded JSON argument conversion for the pinned Playwright value codec.
//!
//! Non-JSON values and cyclic evaluation results remain scoped handles. This
//! converter does not claim JavaScript structured-clone or Map/Set conversion.

use serde_json::{Value, json};

/// Decodes only lossless JSON results; special values require a handle API.
pub(crate) fn result(value: &Value) -> Result<Value, &'static str> {
    let mut nodes = 0;
    decode(value, 0, &mut nodes)
}
fn decode(value: &Value, depth: usize, nodes: &mut usize) -> Result<Value, &'static str> {
    *nodes += 1;
    if depth > 64 || *nodes > 100_000 {
        return Err("VALUE_BOUNDS");
    }
    if value["ref"].is_number() {
        return Err("NON_JSON_RESULT");
    }
    for field in ["s", "b", "n"] {
        if let Some(value) = value.get(field) {
            return Ok(value.clone());
        }
    }
    if value["v"] == "null" {
        return Ok(Value::Null);
    }
    if let Some(array) = value["a"].as_array() {
        return array
            .iter()
            .map(|v| decode(v, depth + 1, nodes))
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array);
    }
    if let Some(entries) = value["o"].as_array() {
        let mut result = serde_json::Map::new();
        for entry in entries {
            let key = entry["k"].as_str().ok_or("NON_JSON_RESULT")?;
            result.insert(key.into(), decode(&entry["v"], depth + 1, nodes)?);
        }
        return Ok(Value::Object(result));
    }
    Err("NON_JSON_RESULT")
}

pub(crate) fn argument(value: &Value) -> Result<Value, &'static str> {
    let mut nodes = 0;
    let result = encode(value, 0, &mut nodes)?;
    if serde_json::to_vec(&result)
        .map_err(|_| "VALUE_BOUNDS")?
        .len()
        > 1024 * 1024
    {
        return Err("VALUE_BOUNDS");
    }
    Ok(json!({"value":result,"handles":[]}))
}
fn encode(value: &Value, depth: usize, nodes: &mut usize) -> Result<Value, &'static str> {
    *nodes += 1;
    if depth > 64 || *nodes > 100_000 {
        return Err("VALUE_BOUNDS");
    }
    Ok(match value {
        Value::Null => json!({"v":"null"}),
        Value::Bool(value) => json!({"b":value}),
        Value::Number(value) => {
            if value
                .as_i64()
                .is_some_and(|v| v.unsigned_abs() > 9_007_199_254_740_991)
                || value.as_u64().is_some_and(|v| v > 9_007_199_254_740_991)
            {
                return Err("UNSAFE_INTEGER");
            }
            json!({"n":value})
        }
        Value::String(value) => json!({"s":value}),
        Value::Array(values) => {
            json!({"a":values.iter().map(|v| encode(v,depth+1,nodes)).collect::<Result<Vec<_>,_>>()?})
        }
        Value::Object(values) => {
            if values.contains_key("__proto__") {
                return Err("INVALID_VALUE_KEY");
            }
            json!({"o":values.iter().map(|(k,v)| Ok(json!({"k":k,"v":encode(v,depth+1,nodes)?}))).collect::<Result<Vec<_>,&'static str>>()?})
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn json_values_preserve_null_and_bound_integer_and_recursive_inputs() {
        assert_eq!(
            argument(&json!({"items":[null,true,4,"Acme"]})).unwrap()["value"]["o"][0]["v"]["a"][0],
            json!({"v":"null"})
        );
        assert!(argument(&json!(u64::MAX)).is_err());
        let mut deep = Value::Null;
        for _ in 0..66 {
            deep = json!([deep]);
        }
        assert!(argument(&deep).is_err());
    }
}
