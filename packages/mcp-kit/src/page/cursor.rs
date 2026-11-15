//! Versioned bounded base64url continuation codec and query/revision binding.

use super::{Error, MAX_CURSOR_BYTES, MAX_KEY_BYTES};
use sha2::{Digest, Sha256};

const VERSION: u8 = 1;
const HEADER: usize = 66;
const CHECKSUM: usize = 16;

/// SHA-256 of canonical query bytes or immutable content. Callers define a
/// deterministic encoding including every effective filter, ordering and selector.
#[must_use]
pub fn digest(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

/// Typed store position. Key bytes include the unique ordering tie-breaker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Position {
    /// Resume strictly after this item's sort key.
    Keyset(Vec<u8>),
    /// Next unread unit of immutable, revision-bound content.
    Offset(u64),
}

/// Stateless list/query binding. Cursors are checksummed, not signed: they
/// detect accidental edits but confer no authority. Never trust a decoded key
/// for access control; validate it against the store's key encoding as well.
#[derive(Debug, Clone)]
pub struct Scope {
    list: [u8; 32],
    query: [u8; 32],
    revision: Option<[u8; 32]>,
}

impl Scope {
    /// A mutable list using last-item keyset positions.
    #[must_use]
    pub fn new(list: &str, query_digest: [u8; 32]) -> Self {
        Self {
            list: digest(list.as_bytes()),
            query: query_digest,
            revision: None,
        }
    }

    /// Immutable content using next-unread offsets, bound to its revision digest.
    #[must_use]
    pub fn offset(list: &str, query_digest: [u8; 32], revision: [u8; 32]) -> Self {
        Self {
            revision: Some(revision),
            ..Self::new(list, query_digest)
        }
    }

    /// Encodes a position. Keyset keys have at most 110 bytes; larger composite
    /// keys need a compact, reversible store key, never a lossy key hash.
    ///
    /// # Errors
    /// Returns invalid position for the wrong kind or an oversized key.
    pub fn encode(&self, position: &Position) -> Result<String, Error> {
        if matches!(position, Position::Keyset(key) if key.len() > MAX_KEY_BYTES) {
            return Err(Error::InvalidPosition("sort key exceeds 110 bytes".into()));
        }
        let mut bytes = vec![VERSION];
        bytes.extend(self.list);
        bytes.extend(self.query);
        match (position, self.revision) {
            (Position::Keyset(key), None) => {
                bytes.push(0);
                bytes.extend(key);
            }
            (Position::Offset(offset), Some(revision)) => {
                bytes.push(1);
                bytes.extend(revision);
                bytes.extend(offset.to_be_bytes());
            }
            _ => {
                return Err(Error::InvalidPosition(
                    "position kind differs from scope".into(),
                ));
            }
        }
        let checksum = digest(&bytes);
        bytes.extend(&checksum[..CHECKSUM]);
        let text = crate::content::base64(&bytes)
            .trim_end_matches('=')
            .replace('+', "-")
            .replace('/', "_");
        if text.len() > MAX_CURSOR_BYTES {
            return Err(Error::InvalidPosition("sort key exceeds 110 bytes".into()));
        }
        Ok(text)
    }

    /// Checks the complete scope before returning a typed store position.
    ///
    /// # Errors
    /// Returns invalid cursor for malformed, oversized, foreign, stale,
    /// query-mismatched or wrong-version/wrong-position-kind cursors.
    pub fn decode(&self, cursor: &str) -> Result<Position, Error> {
        let bytes = decode(cursor)?;
        if bytes[1..33] != self.list || bytes[33..65] != self.query {
            return Err(Error::InvalidCursor);
        }
        let payload = &bytes[HEADER..bytes.len() - CHECKSUM];
        match (bytes[65], self.revision) {
            (0, None) => Ok(Position::Keyset(payload.to_vec())),
            (1, Some(revision)) if payload.len() == 40 && payload[..32] == revision => {
                Ok(Position::Offset(u64::from_be_bytes(
                    payload[32..].try_into().map_err(|_| Error::InvalidCursor)?,
                )))
            }
            _ => Err(Error::InvalidCursor),
        }
    }
}

pub(super) fn decode(cursor: &str) -> Result<Vec<u8>, Error> {
    if cursor.is_empty()
        || cursor.len() > MAX_CURSOR_BYTES
        || !cursor
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err(Error::InvalidCursor);
    }
    let mut padded = cursor.replace('-', "+").replace('_', "/");
    match padded.len() % 4 {
        0 => {}
        2 => padded.push_str("=="),
        3 => padded.push('='),
        _ => return Err(Error::InvalidCursor),
    }
    let bytes = crate::content::decode_base64(&padded, 192).map_err(|_| Error::InvalidCursor)?;
    if bytes.len() < HEADER + CHECKSUM || bytes[0] != VERSION || bytes[65] > 1 {
        return Err(Error::InvalidCursor);
    }
    let end = bytes.len() - CHECKSUM;
    if digest(&bytes[..end])[..CHECKSUM] != bytes[end..] {
        return Err(Error::InvalidCursor);
    }
    Ok(bytes)
}
