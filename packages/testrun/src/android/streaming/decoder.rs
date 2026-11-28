//! AVC access units and owned macOS video bindings; no native handles escape this adapter.
use crate::error::{Error, Result};
use std::ffi::c_void;
unsafe extern "C" {
    fn sb_android_h264_format(
        s: *const u8,
        sn: usize,
        p: *const u8,
        pn: usize,
        width: u32,
        height: u32,
    ) -> *mut c_void;
    fn sb_hevc_format_release(format: *mut c_void);
    fn sb_hevc_decoder_open(format: *mut c_void, width: u32, height: u32) -> *mut c_void;
    fn sb_hevc_decoder_close(decoder: *mut c_void);
    fn sb_hevc_decoder_append(
        decoder: *mut c_void,
        format: *mut c_void,
        bytes: *const u8,
        length: usize,
        sequence: u64,
    ) -> i32;
    fn sb_android_decoder_state(decoder: *mut c_void, sequence: *mut u64) -> i32;
    fn sb_hevc_decoder_snapshot(
        decoder: *mut c_void,
        bytes: *mut *mut u8,
        length: *mut usize,
        sequence: *mut u64,
        width: *mut u32,
        height: *mut u32,
    ) -> i32;
    fn sb_hevc_bytes_release(bytes: *mut c_void);
}
pub(super) struct Decoder {
    format: *mut c_void,
    decoder: *mut c_void,
}
impl Decoder {
    pub(super) fn new(config: &[u8], size: (u32, u32)) -> Result<Self> {
        let units = units(config)?;
        let parameter = |kind| {
            units
                .iter()
                .find(|unit| unit[0] & 31 == kind)
                .copied()
                .ok_or_else(|| Error::new("PLATFORM_PROTOCOL", "AVC parameters missing"))
        };
        let (s, p) = (parameter(7)?, parameter(8)?);
        // SAFETY: validated slices remain live for format creation, which copies them.
        let format = unsafe {
            sb_android_h264_format(s.as_ptr(), s.len(), p.as_ptr(), p.len(), size.0, size.1)
        };
        if format.is_null() {
            return Err(failed());
        }
        // SAFETY: live owned format, bounded display dimensions; constructor retains its format.
        let decoder = unsafe { sb_hevc_decoder_open(format, size.0, size.1) };
        if decoder.is_null() {
            unsafe {
                sb_hevc_format_release(format);
            }
            return Err(failed());
        }
        Ok(Self { format, decoder })
    }
    pub(super) fn append(&mut self, bytes: &[u8], sequence: u64) -> Result<()> {
        let mut avcc = Vec::new();
        for unit in units(bytes)? {
            avcc.extend((unit.len() as u32).to_be_bytes());
            avcc.extend(unit);
        }
        // SAFETY: the bridge copies this bounded access unit; callbacks synchronize retained image state.
        if unsafe {
            sb_hevc_decoder_append(
                self.decoder,
                self.format,
                avcc.as_ptr(),
                avcc.len(),
                sequence,
            )
        } != 0
        {
            return Err(failed());
        }
        Ok(())
    }
    pub(super) fn sequence(&self) -> Result<Option<u64>> {
        let mut sequence = 0;
        // SAFETY: live decoder; the bridge synchronizes its callback state.
        match unsafe { sb_android_decoder_state(self.decoder, &mut sequence) } {
            0 => Ok(Some(sequence)),
            1 => Ok(None),
            _ => Err(failed()),
        }
    }
    pub(super) fn snapshot(&self) -> Result<(Vec<u8>, u64)> {
        let (mut bytes, mut length, mut sequence, mut width, mut height) =
            (std::ptr::null_mut(), 0, 0, 0, 0);
        // SAFETY: writable outputs; native API returns one allocation with an exact length.
        let status = unsafe {
            sb_hevc_decoder_snapshot(
                self.decoder,
                &mut bytes,
                &mut length,
                &mut sequence,
                &mut width,
                &mut height,
            )
        };
        if status != 0 {
            return Err(failed());
        }
        if bytes.is_null() || length > 2 * 1024 * 1024 {
            if !bytes.is_null() {
                unsafe {
                    sb_hevc_bytes_release(bytes.cast());
                }
            }
            return Err(failed());
        }
        // SAFETY: successful bridge allocation; free with its paired allocator.
        let image = unsafe {
            let image = std::slice::from_raw_parts(bytes, length).to_vec();
            sb_hevc_bytes_release(bytes.cast());
            image
        };
        Ok((image, sequence))
    }
}
impl Drop for Decoder {
    fn drop(&mut self) {
        // SAFETY: this serialized worker owns both handles, and close drains callbacks.
        unsafe {
            sb_hevc_decoder_close(self.decoder);
            sb_hevc_format_release(self.format);
        }
    }
}
fn failed() -> Error {
    Error::new(
        "CAPTURE_STALLED",
        "Android video decode or compact encoding failed",
    )
}
fn units(bytes: &[u8]) -> Result<Vec<&[u8]>> {
    let mut starts = Vec::new();
    let mut at = 0;
    while at + 3 <= bytes.len() {
        let prefix = if bytes[at..].starts_with(&[0, 0, 0, 1]) {
            4
        } else if bytes[at..].starts_with(&[0, 0, 1]) {
            3
        } else {
            0
        };
        if prefix > 0 {
            starts.push((at, prefix));
            at += prefix;
        } else {
            at += 1;
        }
    }
    if starts.first().is_none_or(|(start, _)| *start != 0) {
        return Err(failed());
    }
    let mut output = Vec::new();
    for (index, (start, prefix)) in starts.iter().enumerate() {
        let end = starts
            .get(index + 1)
            .map_or(bytes.len(), |(start, _)| *start);
        if end <= start + prefix {
            return Err(failed());
        }
        output.push(&bytes[start + prefix..end]);
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn annex_b_accepts_complete_units_and_rejects_missing_or_empty_prefixes() {
        let bytes = [0, 0, 0, 1, 0x67, 2, 0, 0, 1, 0x68, 3];
        assert_eq!(units(&bytes).unwrap(), vec![&[0x67, 2][..], &[0x68, 3][..]]);
        for bytes in [
            &[][..],
            &[1, 2, 3][..],
            &[0, 0, 1][..],
            &[0, 0, 1, 0, 0, 1, 5][..],
        ] {
            assert!(units(bytes).is_err());
        }
    }
}
