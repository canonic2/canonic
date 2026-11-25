//! Shared base64 encoding and bounded canonical decoding.

pub(crate) enum BinaryError {
    Invalid,
    TooLarge,
}

pub(crate) fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut text = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let triple = chunk
            .iter()
            .enumerate()
            .fold(0_u32, |triple, (index, byte)| {
                triple | u32::from(*byte) << (16 - 8 * index)
            });
        for index in 0..4 {
            if index <= chunk.len() {
                let sextet = (triple >> (18 - 6 * index)) & 0x3f;
                text.push(char::from(ALPHABET[sextet as usize]));
            } else {
                text.push('=');
            }
        }
    }
    text
}

/// Encodes unpadded base64url.
pub(crate) fn base64url(bytes: &[u8]) -> String {
    base64(bytes)
        .trim_end_matches('=')
        .replace('+', "-")
        .replace('/', "_")
}

pub(crate) fn decoded_size(data: &str, limit: usize) -> Result<usize, BinaryError> {
    if data.len() > limit.div_ceil(3) * 4 {
        return Err(BinaryError::TooLarge);
    }
    if !data.len().is_multiple_of(4) {
        return Err(BinaryError::Invalid);
    }
    let padding = data.bytes().rev().take_while(|byte| *byte == b'=').count();
    if padding > 2 {
        return Err(BinaryError::Invalid);
    }
    let mut last = 0;
    for byte in data.bytes().take(data.len() - padding) {
        last = sextet(byte, *b"+/").ok_or(BinaryError::Invalid)?;
    }
    if (padding == 1 && last & 3 != 0) || (padding == 2 && last & 15 != 0) {
        return Err(BinaryError::Invalid);
    }
    let raw_bytes = data.len() / 4 * 3 - padding;
    if raw_bytes > limit {
        return Err(BinaryError::TooLarge);
    }
    Ok(raw_bytes)
}

/// Decodes canonical padded standard base64 of at most `limit` decoded bytes.
pub(crate) fn decode(data: &str, limit: usize) -> Result<Vec<u8>, BinaryError> {
    let size = decoded_size(data, limit)?;
    decode_unpadded(data.trim_end_matches('='), size, *b"+/")
}

/// Decodes canonical unpadded base64url of at most `limit` decoded bytes.
pub(crate) fn decode_base64url(data: &str, limit: usize) -> Option<Vec<u8>> {
    let remainder = data.len() % 4;
    let size = data.len() / 4 * 3 + remainder * 3 / 4;
    if remainder == 1 || size > limit {
        return None;
    }
    decode_unpadded(data, size, *b"-_").ok()
}

fn decode_unpadded(data: &str, size: usize, tail: [u8; 2]) -> Result<Vec<u8>, BinaryError> {
    let mut bytes = Vec::with_capacity(size);
    let mut buffer = 0_u32;
    let mut bits = 0;
    for byte in data.bytes() {
        buffer = (buffer << 6) | u32::from(sextet(byte, tail).ok_or(BinaryError::Invalid)?);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            bytes.push(u8::try_from(buffer >> bits).map_err(|_| BinaryError::Invalid)?);
            buffer &= (1 << bits) - 1;
        }
    }
    if buffer == 0 {
        Ok(bytes)
    } else {
        Err(BinaryError::Invalid)
    }
}

fn sextet(byte: u8, tail: [u8; 2]) -> Option<u8> {
    match byte {
        b'A'..=b'Z' => Some(byte - b'A'),
        b'a'..=b'z' => Some(byte - b'a' + 26),
        b'0'..=b'9' => Some(byte - b'0' + 52),
        byte if byte == tail[0] => Some(62),
        byte if byte == tail[1] => Some(63),
        _ => None,
    }
}
