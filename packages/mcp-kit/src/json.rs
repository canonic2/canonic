//! Strict JSON input and bounded, interruptible JSON encoding.
//!
//! Input refuses duplicate object keys and trailing data, so one message has
//! one meaning. Encoding stops at a byte limit or when a checkpoint fails,
//! instead of building an unbounded buffer.

use crate::control::Interrupted;
use serde::{
    Deserialize, Deserializer, Serialize,
    de::{self, MapAccess, SeqAccess, Visitor},
};
use serde_json::{Map, Number, Value};
use std::{fmt, io};

/// Why bytes are not one strict JSON value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ParseError {
    /// The input is longer than the caller's limit.
    #[error("message exceeds {limit} bytes")]
    TooLarge {
        /// The byte limit that was exceeded.
        limit: usize,
    },
    /// The input is not JSON or repeats an object key.
    #[error("invalid JSON or duplicate object key")]
    Syntax,
    /// Non-whitespace data follows the value.
    #[error("data follows the JSON value")]
    Trailing,
}

/// Why a value could not be encoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum EncodeError {
    /// The encoding would exceed the caller's limit.
    #[error("encoded value exceeds {limit} bytes")]
    TooLarge {
        /// The byte limit that was exceeded.
        limit: usize,
    },
    /// The checkpoint stopped encoding.
    #[error("encoding was interrupted")]
    Interrupted,
    /// The value cannot be represented as JSON, such as a map with non-string keys.
    #[error("value cannot be encoded as JSON")]
    Invalid,
}

/// Parses one strict JSON value of at most `limit` bytes.
///
/// # Errors
///
/// Returns [`ParseError::TooLarge`] for oversized input, [`ParseError::Syntax`]
/// for invalid JSON or a repeated object key, and [`ParseError::Trailing`] when
/// data follows the value.
pub fn parse(bytes: &[u8], limit: usize) -> Result<Value, ParseError> {
    if bytes.len() > limit {
        return Err(ParseError::TooLarge { limit });
    }
    let mut input = serde_json::Deserializer::from_slice(bytes);
    let Strict(value) = Strict::deserialize(&mut input).map_err(|_| ParseError::Syntax)?;
    input.end().map_err(|_| ParseError::Trailing)?;
    Ok(value)
}

/// Encodes `value` in at most `limit` bytes.
///
/// # Errors
///
/// Returns [`EncodeError::TooLarge`] past the limit or [`EncodeError::Invalid`]
/// for values JSON cannot represent.
pub fn encode(value: &(impl Serialize + ?Sized), limit: usize) -> Result<Vec<u8>, EncodeError> {
    encode_checked(value, limit, &|| Ok(()))
}

/// Encodes `value` in at most `limit` bytes, calling `check` before each write.
///
/// # Errors
///
/// Returns [`EncodeError::Interrupted`] when `check` fails, otherwise as [`encode`].
pub fn encode_checked(
    value: &(impl Serialize + ?Sized),
    limit: usize,
    check: &dyn Fn() -> Result<(), Interrupted>,
) -> Result<Vec<u8>, EncodeError> {
    let mut output = Bounded {
        bytes: Vec::new(),
        length: 0,
        limit,
        keep: true,
        check,
        failure: None,
    };
    write(value, &mut output)?;
    Ok(output.bytes)
}

/// Returns the encoded length of `value` without keeping the bytes.
///
/// # Errors
///
/// As [`encode_checked`].
pub fn measure(
    value: &(impl Serialize + ?Sized),
    limit: usize,
    check: &dyn Fn() -> Result<(), Interrupted>,
) -> Result<usize, EncodeError> {
    let mut output = Bounded {
        bytes: Vec::new(),
        length: 0,
        limit,
        keep: false,
        check,
        failure: None,
    };
    write(value, &mut output)?;
    Ok(output.length)
}

fn write(value: &(impl Serialize + ?Sized), output: &mut Bounded<'_>) -> Result<(), EncodeError> {
    serde_json::to_writer(&mut *output, value)
        .map_err(|_| output.failure.unwrap_or(EncodeError::Invalid))
}

struct Bounded<'a> {
    bytes: Vec<u8>,
    length: usize,
    limit: usize,
    keep: bool,
    check: &'a dyn Fn() -> Result<(), Interrupted>,
    failure: Option<EncodeError>,
}

impl io::Write for Bounded<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if (self.check)().is_err() {
            self.failure = Some(EncodeError::Interrupted);
            return Err(io::Error::other("encoding interrupted"));
        }
        let Some(length) = self
            .length
            .checked_add(bytes.len())
            .filter(|length| *length <= self.limit)
        else {
            self.failure = Some(EncodeError::TooLarge { limit: self.limit });
            return Err(io::Error::other("encoding limit"));
        };
        self.length = length;
        if self.keep {
            self.bytes.extend_from_slice(bytes);
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::cell::Cell;

    #[test]
    fn parse_refuses_duplicate_keys_trailing_data_and_oversized_input() {
        assert_eq!(
            parse(br#"{"a":1,"b":[true,null]}"#, 64),
            Ok(json!({"a":1,"b":[true,null]}))
        );
        assert_eq!(parse(br#"{"a":1,"a":2}"#, 64), Err(ParseError::Syntax));
        assert_eq!(
            parse(br#"{"a":{"b":1,"b":1}}"#, 64),
            Err(ParseError::Syntax)
        );
        assert_eq!(parse(br#"{"a":1} {}"#, 64), Err(ParseError::Trailing));
        assert_eq!(parse(b"{\"a\":1}\n", 64), Ok(json!({"a":1})));
        assert_eq!(parse(b"[1,2,3]", 6), Err(ParseError::TooLarge { limit: 6 }));
    }

    #[test]
    fn encoding_stops_at_the_limit_or_a_failed_checkpoint() {
        let value = json!({"text":"x".repeat(100)});
        let full = encode(&value, 1024).unwrap();
        assert_eq!(serde_json::from_slice::<Value>(&full).unwrap(), value);
        assert_eq!(measure(&value, 1024, &|| Ok(())), Ok(full.len()));
        assert_eq!(
            encode(&value, full.len() - 1),
            Err(EncodeError::TooLarge {
                limit: full.len() - 1
            })
        );

        let calls = Cell::new(0);
        let check = || {
            calls.set(calls.get() + 1);
            if calls.get() > 2 {
                Err(Interrupted)
            } else {
                Ok(())
            }
        };
        assert_eq!(
            encode_checked(&value, 1024, &check),
            Err(EncodeError::Interrupted)
        );
    }
}
