//! Strict JSON values shared by policy preparation and transport boundaries.

use serde::{
    Deserialize, Deserializer,
    de::{self, MapAccess, SeqAccess, Visitor},
};
use serde_json::{Map, Number, Value};
use std::{
    fmt,
    io::{self, Write},
};

/// Measure the same compact UTF-8 encoding used on disk, without retaining it.
pub(crate) fn encoded_len(value: &impl serde::Serialize) -> Result<usize, serde_json::Error> {
    let mut counter = Counter {
        bytes: 0,
        limit: usize::MAX,
    };
    serde_json::to_writer(&mut counter, value)?;
    Ok(counter.bytes)
}

/// Refuse oversized evidence as soon as its encoded size crosses the bound.
pub(crate) fn fits(value: &impl serde::Serialize, limit: usize) -> bool {
    serde_json::to_writer(Counter { bytes: 0, limit }, value).is_ok()
}

struct Counter {
    bytes: usize,
    limit: usize,
}
impl Write for Counter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.bytes = self
            .bytes
            .checked_add(bytes.len())
            .filter(|size| *size <= self.limit)
            .ok_or_else(|| io::Error::other("encoded JSON exceeds limit"))?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Hash compact JSON with bounded scratch space, preserving existing source digests.
pub(crate) fn fingerprint(value: &impl serde::Serialize) -> Result<String, serde_json::Error> {
    struct Digest(ring::digest::Context);
    impl Write for Digest {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0.update(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    // Buffer small serializer writes before hashing; never buffer the whole policy.
    let mut writer = io::BufWriter::new(Digest(ring::digest::Context::new(&ring::digest::SHA256)));
    serde_json::to_writer(&mut writer, value)?;
    let digest = writer
        .into_inner()
        .map_err(|error| serde_json::Error::io(error.into_error()))?
        .0
        .finish();
    Ok(canonic_registry_kit::digest::encode(digest.as_ref()))
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn size_refusal_stops_before_serializing_later_evidence() {
        struct Later<'a>(&'a std::cell::Cell<bool>);
        impl serde::Serialize for Later<'_> {
            fn serialize<S: serde::Serializer>(&self, output: S) -> Result<S::Ok, S::Error> {
                self.0.set(true);
                serde::Serialize::serialize(&true, output)
            }
        }
        let reached = std::cell::Cell::new(false);
        assert!(!fits(&("x".repeat(1000), Later(&reached)), 8));
        assert!(!reached.get());
    }

    #[test]
    fn measured_sizes_and_digests_preserve_escaped_utf8_and_map_bindings() {
        for value in [
            Value::Null,
            json!(0),
            json!(-0.25),
            json!({"é":"\n\t\\\"\0", "arrays":[false, "😃"]}),
            json!({"pack":"quoted \"source\"\n".repeat(10_000)}),
        ] {
            let encoded = value.to_string();
            assert_eq!(encoded_len(&value).unwrap(), encoded.len());
            assert!(fits(&value, encoded.len()));
            assert!(!fits(&value, encoded.len() - 1));
            assert_eq!(
                fingerprint(&value).unwrap(),
                crate::policy::fingerprint(encoded.as_bytes())
            );
            let borrowed = std::collections::BTreeMap::from([("project", &value)]);
            assert_eq!(
                fingerprint(&borrowed).unwrap(),
                crate::policy::fingerprint(json!({"project":value}).to_string().as_bytes())
            );
        }
    }
}
