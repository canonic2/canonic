//! Typed ownership of the native, fixed-profile AAC-ELD decoder.
use crate::{error::Result, playback::probe::failed};
use std::ptr::NonNull;
unsafe extern "C" {
    fn sb_audio_decoder_open() -> *mut std::ffi::c_void;
    fn sb_audio_decoder_decode(
        decoder: *mut std::ffi::c_void,
        bytes: *const u8,
        length: u32,
        pcm: *mut i16,
    ) -> i32;
    fn sb_audio_decoder_close(decoder: *mut std::ffi::c_void);
}
pub(super) struct Decoder(NonNull<std::ffi::c_void>);
// SAFETY: the converter and its input callback are exclusively accessed by the probe task.
unsafe impl Send for Decoder {}
impl Decoder {
    pub(super) fn new() -> Result<Self> {
        // SAFETY: no parameters; returns an exclusively owned converter or null.
        NonNull::new(unsafe { sb_audio_decoder_open() })
            .map(Self)
            .ok_or_else(|| failed("decoder_initialization"))
    }
    pub(super) fn decode(&mut self, bytes: &[u8]) -> Result<[i16; 960]> {
        let mut pcm = [0; 960];
        if bytes.is_empty() || bytes.len() > 65536 {
            return Err(failed("aac_payload_size"));
        }
        // SAFETY: pointers live for the call; the native adapter writes at most 960 i16 samples.
        let status = unsafe {
            sb_audio_decoder_decode(
                self.0.as_ptr(),
                bytes.as_ptr(),
                bytes.len() as u32,
                pcm.as_mut_ptr(),
            )
        };
        if status != 0 {
            return Err(failed("aac_decode")
                .details(serde_json::json!({"phase":"aac_decode","nativeStatus":status})));
        }
        Ok(pcm)
    }
}
impl Drop for Decoder {
    fn drop(&mut self) {
        // SAFETY: self owns this converter, disposed once after the last decode.
        unsafe {
            sb_audio_decoder_close(self.0.as_ptr());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_eld_silence_decodes_exact_frames_and_invalid_input_is_not_pcm() {
        let mut decoder = Decoder::new().unwrap();
        // Digital-silence access unit from the owned Acme fixture's mode-8 lab stream.
        let pcm = decoder.decode(&[0x00, 0x68, 0x34, 0x00]).unwrap();
        assert_eq!(pcm.len(), 960);
        assert!(pcm.iter().all(|s| *s == 0));
        assert!(decoder.decode(&[]).is_err());
        assert!(decoder.decode(&[0xff; 8]).is_err());
    }
    #[test]
    fn native_eld_fixture_tone_produces_nonzero_pcm() {
        // Ten contiguous AAC-ELD units at the owned 440 Hz fixture's onset.
        // The first three decode to zero because of encoder priming; keep that
        // interval rather than confusing packet size with signal amplitude.
        let mut decoder = Decoder::new().unwrap();
        let mut samples = Vec::new();
        for packet in include_str!("decoder-tone.hex").lines() {
            let bytes: Vec<u8> = packet
                .as_bytes()
                .as_chunks::<2>()
                .0
                .iter()
                .map(|v| u8::from_str_radix(std::str::from_utf8(v).unwrap(), 16).unwrap())
                .collect();
            samples.extend(decoder.decode(&bytes).unwrap());
        }
        assert_eq!(samples.len(), 9600);
        assert!(samples[..2880].iter().all(|s| *s == 0));
        assert!(samples[2880..].iter().any(|s| s.unsigned_abs() > 1000));
    }
}
