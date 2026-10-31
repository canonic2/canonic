//! Shared standard base64 encoding and bounded canonical validation.

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
        last = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return Err(BinaryError::Invalid),
        };
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
