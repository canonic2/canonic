//! Bounded big-endian length-prefixed byte frames and JSON convenience.

use crate::{Error, Result};
use serde::{Serialize, de::DeserializeOwned};
use std::io::{Read, Write};

/// Default maximum payload: one MiB, excluding the four-byte prefix.
pub const DEFAULT_LIMIT: usize = 1024 * 1024;

/// Read exactly one frame, refusing the advertised length before allocation.
///
/// # Errors
/// Returns size-limit or I/O errors, including incomplete prefixes and bodies.
pub fn read(input: &mut impl Read, limit: usize) -> Result<Vec<u8>> {
    let mut prefix = [0; 4];
    input.read_exact(&mut prefix)?;
    let length = u32::from_be_bytes(prefix) as usize;
    if length > limit {
        return Err(Error::FrameTooLarge);
    }
    let mut bytes = vec![0; length];
    input.read_exact(&mut bytes)?;
    Ok(bytes)
}

/// Write one complete frame. Oversized payloads write no prefix or bytes.
///
/// # Errors
/// Returns size-limit or I/O errors.
pub fn write(output: &mut impl Write, payload: &[u8], limit: usize) -> Result<()> {
    let length = u32::try_from(payload.len()).map_err(|_| Error::FrameTooLarge)?;
    if payload.len() > limit {
        return Err(Error::FrameTooLarge);
    }
    output.write_all(&length.to_be_bytes())?;
    output.write_all(payload)?;
    Ok(())
}

/// Decode a JSON frame without leaking its contents in an error.
///
/// # Errors
/// Returns framing errors or [`Error::Envelope`] for invalid JSON.
pub fn read_json<T: DeserializeOwned>(input: &mut impl Read, limit: usize) -> Result<T> {
    serde_json::from_slice(&read(input, limit)?).map_err(|_| Error::Envelope)
}

/// Encode a JSON frame.
///
/// # Errors
/// Returns encoding, size-limit or I/O errors.
pub fn write_json(output: &mut impl Write, value: &impl Serialize, limit: usize) -> Result<()> {
    let bytes = encode_json(value, limit)?;
    write(output, &bytes, limit)
}

/// Encode JSON without allocating beyond the frame budget.
///
/// # Errors
/// Returns `FrameTooLarge` or `Envelope`; no partial bytes are returned.
pub fn encode_json(value: &impl Serialize, limit: usize) -> Result<Vec<u8>> {
    struct Bounded {
        bytes: Vec<u8>,
        limit: usize,
        exceeded: bool,
    }
    impl Write for Bounded {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if bytes.len() > self.limit.saturating_sub(self.bytes.len()) {
                self.exceeded = true;
                return Err(std::io::Error::other("frame limit"));
            }
            self.bytes.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut output = Bounded {
        bytes: Vec::new(),
        limit,
        exceeded: false,
    };
    let encoded = serde_json::to_writer(&mut output, value);
    if output.exceeded {
        return Err(Error::FrameTooLarge);
    }
    encoded.map_err(|_| Error::Envelope)?;
    Ok(output.bytes)
}
