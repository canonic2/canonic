//! SHA-256 digests and fixed-length lowercase hexadecimal encoding.

const HEX: &[u8; 16] = b"0123456789abcdef";

/// Returns the lowercase hexadecimal SHA-256 digest of `bytes`.
///
/// ```
/// assert_eq!(
///     canonic_registry::digest::sha256_hex(b""),
///     "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
/// );
/// ```
#[must_use]
pub fn sha256_hex(bytes: &[u8]) -> String {
    encode(ring::digest::digest(&ring::digest::SHA256, bytes).as_ref())
}

/// Encodes bytes as lowercase hexadecimal.
#[must_use]
pub fn encode(bytes: &[u8]) -> String {
    bytes
        .iter()
        .flat_map(|byte| {
            [
                char::from(HEX[usize::from(byte >> 4)]),
                char::from(HEX[usize::from(byte & 15)]),
            ]
        })
        .collect()
}

/// Decodes exactly `length` bytes of hexadecimal, ignoring surrounding whitespace.
///
/// Either case is accepted. Returns `None` for any other length or character.
#[must_use]
pub fn decode(value: &str, length: usize) -> Option<Vec<u8>> {
    let value = value.trim();
    if value.len() != length.checked_mul(2)? || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    (0..length)
        .map(|i| u8::from_str_radix(&value[i * 2..i * 2 + 2], 16).ok())
        .collect()
}

/// `value` as lowercase hexadecimal, if it encodes exactly `length` bytes in
/// either case, with no surrounding whitespace.
#[must_use]
pub fn normalize_hex(value: &str, length: usize) -> Option<String> {
    (Some(value.len()) == length.checked_mul(2) && value.bytes().all(|b| b.is_ascii_hexdigit()))
        .then(|| value.to_ascii_lowercase())
}

/// `value` as a canonical lowercase SHA-256 digest, if it is exactly 64
/// hexadecimal characters in either case, with no surrounding whitespace.
#[must_use]
pub fn normalize_sha256(value: &str) -> Option<String> {
    normalize_hex(value, 32)
}

/// Whether `value` is a lowercase 64-character SHA-256 digest.
#[must_use]
pub fn is_sha256_hex(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn decoding_requires_the_exact_length_and_hex_digits() {
        assert_eq!(decode(" 0aFf \n", 2), Some(vec![0x0a, 0xff]));
        assert_eq!(decode("0aff", 3), None);
        assert_eq!(decode("0g", 1), None);
        assert_eq!(decode("", 0), Some(Vec::new()));
        assert_eq!(decode("00", usize::MAX), None);
    }

    #[test]
    fn digests_are_canonical_lowercase() {
        let digest = sha256_hex(b"acme");
        assert!(is_sha256_hex(&digest));
        assert!(!is_sha256_hex(&digest.to_ascii_uppercase()));
        assert!(!is_sha256_hex(&digest[1..]));
        assert_eq!(
            normalize_sha256(&digest.to_ascii_uppercase()),
            Some(digest.clone())
        );
        assert_eq!(normalize_sha256(&format!(" {}", &digest[1..])), None);
        assert_eq!(normalize_sha256(&format!("{digest} ")), None);
        assert_eq!(normalize_hex("0aFF", 2).as_deref(), Some("0aff"));
        assert_eq!(normalize_hex("0aff", 3), None);
        assert_eq!(normalize_hex("00", usize::MAX), None);
    }

    proptest! {
        #[test]
        fn encoding_round_trips(bytes in proptest::collection::vec(any::<u8>(), 0..64)) {
            prop_assert_eq!(decode(&encode(&bytes), bytes.len()), Some(bytes));
        }
    }
}
