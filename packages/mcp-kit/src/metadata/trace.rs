//! Bounded W3C propagation syntax, preserving exact strings without telemetry I/O.
use super::{Metadata, MetadataError};
use serde_json::{Map, Value};
use std::collections::HashSet;

/// Validated request-local propagation strings. Clones share metadata storage.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TraceContext(pub(super) Metadata);
impl TraceContext {
    pub(crate) fn from_metadata(meta: Metadata) -> Self {
        Self(meta)
    }
    /// Exact incoming traceparent, with a validated common prefix.
    #[must_use]
    pub fn traceparent(&self) -> Option<&str> {
        self.0.as_map().get("traceparent").and_then(Value::as_str)
    }
    /// Exact vendor state, exposed for tracing only when traceparent is present.
    #[must_use]
    pub fn tracestate(&self) -> Option<&str> {
        self.traceparent()
            .and_then(|_| self.0.as_map().get("tracestate").and_then(Value::as_str))
    }
    /// Exact baggage, independently of traceparent; never authority.
    #[must_use]
    pub fn baggage(&self) -> Option<&str> {
        self.0.as_map().get("baggage").and_then(Value::as_str)
    }
}
pub(super) fn validate(map: &Map<String, Value>) -> Result<(), MetadataError> {
    for (key, check) in [
        ("traceparent", parent as fn(&str) -> bool),
        ("tracestate", state),
        ("baggage", baggage),
    ] {
        if map
            .get(key)
            .is_some_and(|v| v.as_str().is_none_or(|s| !check(s)))
        {
            return Err(MetadataError::Field(key));
        }
    }
    Ok(())
}
fn parent(value: &str) -> bool {
    let b = value.as_bytes();
    let hex = |s: &[u8]| {
        s.iter()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(b))
    };
    b.len() >= 55
        && b.len() <= 256
        && b.is_ascii()
        && hex(&b[..2])
        && &b[..2] != b"ff"
        && b[2] == b'-'
        && b[35] == b'-'
        && b[52] == b'-'
        && hex(&b[3..35])
        && b[3..35].iter().any(|b| *b != b'0')
        && hex(&b[36..52])
        && b[36..52].iter().any(|b| *b != b'0')
        && hex(&b[53..55])
        && (b.len() == 55
            || (&b[..2] != b"00"
                && b[55] == b'-'
                && b[56..].iter().all(|b| (0x21..=0x7e).contains(b))))
}
fn ows(s: &str) -> &str {
    s.trim_matches([' ', '\t'])
}
fn state_key(key: &str) -> bool {
    let word = |s: &str, max: usize, digit: bool| {
        !s.is_empty()
            && s.len() <= max
            && s.as_bytes()
                .first()
                .is_some_and(|b| b.is_ascii_lowercase() || (digit && b.is_ascii_digit()))
            && s.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_-*/".contains(&b))
    };
    key.split_once('@').map_or_else(
        || word(key, 256, false),
        |(tenant, system)| word(tenant, 241, true) && word(system, 14, false),
    )
}
fn state(value: &str) -> bool {
    if value.len() > 512 {
        return false;
    }
    let mut seen = HashSet::new();
    value.split(',').enumerate().all(|(i, part)| {
        let part = ows(part);
        i < 32
            && (part.is_empty()
                || part.split_once('=').is_some_and(|(key, value)| {
                    state_key(key)
                        && seen.insert(key)
                        && !value.is_empty()
                        && value.len() <= 256
                        && value
                            .bytes()
                            .all(|b| (0x20..=0x7e).contains(&b) && b != b'=' && b != b',')
                        && !value.ends_with(' ')
                }))
    })
}
fn token(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b))
}
fn baggage_value(value: &str) -> bool {
    let b = value.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            if i + 2 >= b.len() || !b[i + 1].is_ascii_hexdigit() || !b[i + 2].is_ascii_hexdigit() {
                return false;
            }
            i += 3;
        } else {
            if !(0x21..=0x7e).contains(&b[i]) || b"\",;\\".contains(&b[i]) {
                return false;
            }
            i += 1;
        }
    }
    true
}
fn baggage(value: &str) -> bool {
    if value.len() > 8192 {
        return false;
    }
    if ows(value).is_empty() {
        return true;
    }
    value.split(',').enumerate().all(|(i, part)| {
        let mut parts = part.split(';');
        i < 180
            && parts
                .next()
                .and_then(|part| part.split_once('='))
                .is_some_and(|(k, v)| token(ows(k)) && baggage_value(ows(v)))
            && parts.all(|property| {
                let property = ows(property);
                property.split_once('=').map_or_else(
                    || token(property),
                    |(k, v)| token(ows(k)) && baggage_value(ows(v)),
                )
            })
    })
}
