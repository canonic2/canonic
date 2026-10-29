//! Strict JSON values shared by policy preparation and transport boundaries.

use serde::{
    Deserialize, Deserializer,
    de::{self, MapAccess, SeqAccess, Visitor},
};
use serde_json::{Map, Number, Value};
use std::fmt;

/// Inspect decoded strings and object keys, without confusing escaped text with NUL.
pub(crate) fn contains_nul(value: &Value) -> bool {
    match value {
        Value::String(text) => text.contains('\0'),
        Value::Array(values) => values.iter().any(contains_nul),
        Value::Object(fields) => fields
            .iter()
            .any(|(key, value)| key.contains('\0') || contains_nul(value)),
        _ => false,
    }
}

struct Strict(Value);

impl<'de> Deserialize<'de> for Strict {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(StrictVisitor)
    }
}

struct StrictVisitor;

impl<'de> Visitor<'de> for StrictVisitor {
    type Value = Strict;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("strict JSON value")
    }
    fn visit_bool<E: de::Error>(self, value: bool) -> Result<Strict, E> {
        Ok(Strict(Value::Bool(value)))
    }
    fn visit_i64<E: de::Error>(self, value: i64) -> Result<Strict, E> {
        Ok(Strict(Value::Number(value.into())))
    }
    fn visit_u64<E: de::Error>(self, value: u64) -> Result<Strict, E> {
        Ok(Strict(Value::Number(value.into())))
    }
    fn visit_f64<E: de::Error>(self, value: f64) -> Result<Strict, E> {
        Number::from_f64(value)
            .map(|value| Strict(Value::Number(value)))
            .ok_or_else(|| E::custom("invalid JSON number"))
    }
    fn visit_str<E: de::Error>(self, value: &str) -> Result<Strict, E> {
        Ok(Strict(Value::String(value.into())))
    }
    fn visit_none<E: de::Error>(self) -> Result<Strict, E> {
        Ok(Strict(Value::Null))
    }
    fn visit_unit<E: de::Error>(self) -> Result<Strict, E> {
        Ok(Strict(Value::Null))
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<Strict, A::Error> {
        let mut items = Vec::new();
        while let Some(Strict(value)) = sequence.next_element()? {
            items.push(value);
        }
        Ok(Strict(Value::Array(items)))
    }
    fn visit_map<A: MapAccess<'de>>(self, mut access: A) -> Result<Strict, A::Error> {
        let mut object = Map::new();
        while let Some(key) = access.next_key::<String>()? {
            if object.contains_key(&key) {
                return Err(de::Error::custom("duplicate JSON key"));
            }
            let Strict(value) = access.next_value()?;
            object.insert(key, value);
        }
        Ok(Strict(Value::Object(object)))
    }
}

pub(crate) enum Error {
    Syntax,
    Trailing,
}

pub(crate) fn parse(bytes: &[u8]) -> Result<Value, Error> {
    let mut input = serde_json::Deserializer::from_slice(bytes);
    let Strict(value) = Strict::deserialize(&mut input).map_err(|_| Error::Syntax)?;
    input.end().map_err(|_| Error::Trailing)?;
    Ok(value)
}
