//! Owned CoreMedia descriptions and compressed HEVC muxer bindings.
use crate::error::{Error, Result};
use std::ffi::{CString, c_char, c_void};
unsafe extern "C" {
    fn sb_hevc_decoder_state(handle: *mut c_void, pts: *mut u64) -> i32;
    fn sb_hevc_decoder_open(format: *mut c_void, width: u32, height: u32) -> *mut c_void;
    fn sb_hevc_decoder_close(handle: *mut c_void);
    fn sb_hevc_decoder_append(
        handle: *mut c_void,
        format: *mut c_void,
        bytes: *const u8,
        length: usize,
        pts: u64,
    ) -> i32;
    fn sb_hevc_decoder_snapshot(
        handle: *mut c_void,
        bytes: *mut *mut u8,
        length: *mut usize,
        pts: *mut u64,
        width: *mut u32,
        height: *mut u32,
    ) -> i32;
    fn sb_hevc_format(
        v: *const u8,
        vn: usize,
        s: *const u8,
        sn: usize,
        p: *const u8,
        pn: usize,
        c: *mut *mut u8,
        n: *mut usize,
        w: *mut u32,
        h: *mut u32,
    ) -> *mut c_void;
    fn sb_hevc_format_release(f: *mut c_void);
    fn sb_hevc_bytes_release(p: *mut c_void);
    fn sb_hevc_recording_open(path: *const c_char, f: *mut c_void) -> *mut c_void;
    fn sb_hevc_recording_append(
        job: *mut c_void,
        f: *mut c_void,
        p: *const u8,
        n: usize,
        pts: u64,
        key: i32,
    ) -> i32;
    fn sb_hevc_recording_finish(job: *mut c_void) -> i32;
    fn sb_hevc_recording_cancel(job: *mut c_void);
    fn sb_hevc_recording_verify(
        path: *const c_char,
        frames: *mut u64,
        width: *mut u32,
        height: *mut u32,
        duration: *mut u64,
    ) -> i32;
}
pub(crate) struct Decoder(*mut c_void);
// SAFETY: the owner serializes all access; the native decoder synchronizes its callback image.
unsafe impl Send for Decoder {}
impl Drop for Decoder {
    fn drop(&mut self) {
        unsafe {
            sb_hevc_decoder_close(self.0);
        }
    }
}
pub(crate) struct Snapshot {
    pub bytes: Vec<u8>,
    pub pts: u64,
    pub width: u32,
    pub height: u32,
}
impl Decoder {
    pub(crate) fn latest_pts(&self) -> Option<u64> {
        let mut pts = 0;
        // SAFETY: the live native decoder synchronizes its callback state; pts remains writable.
        (unsafe { sb_hevc_decoder_state(self.0, &mut pts) } == 0).then_some(pts)
    }
    pub(crate) fn new(format: &Format, display: (u32, u32)) -> Result<Self> {
        // SAFETY: the format remains live for this synchronous construction; VideoToolbox retains it.
        let raw = unsafe { sb_hevc_decoder_open(format.raw, display.0, display.1) };
        if raw.is_null() {
            return Err(Error::new(
                "CAPABILITY_UNAVAILABLE",
                "Native HEVC decoder is unavailable",
            ));
        }
        Ok(Self(raw))
    }
    pub(crate) fn append(&mut self, format: &Format, payload: &[u8], pts: u64) -> Result<()> {
        // SAFETY: native code copies the payload before returning and retains callback state until close.
        let status = unsafe {
            sb_hevc_decoder_append(self.0, format.raw, payload.as_ptr(), payload.len(), pts)
        };
        if status != 0 {
            return Err(
                Error::new("CAPTURE_STALLED", "HEVC observation decode failed")
                    .details(serde_json::json!({"nativeStatus": status})),
            );
        }
        Ok(())
    }
    pub(crate) fn snapshot(&self) -> Result<Option<Snapshot>> {
        let mut bytes = std::ptr::null_mut();
        let mut length = 0;
        let mut pts = 0;
        let mut width = 0;
        let mut height = 0;
        // SAFETY: outputs remain writable; native code returns one owned allocation with its exact length.
        let status = unsafe {
            sb_hevc_decoder_snapshot(
                self.0,
                &mut bytes,
                &mut length,
                &mut pts,
                &mut width,
                &mut height,
            )
        };
        if status == 1 {
            return Ok(None);
        }
        if status != 0 {
            return Err(Error::new(
                "CAPTURE_STALLED",
                "Cannot encode video observation",
            ));
        }
        // SAFETY: successful snapshot allocated exactly length bytes; free through the paired native API.
        let data = unsafe {
            let data = std::slice::from_raw_parts(bytes, length).to_vec();
            sb_hevc_bytes_release(bytes.cast());
            data
        };
        Ok(Some(Snapshot {
            bytes: data,
            pts,
            width,
            height,
        }))
    }
}
pub struct Format {
    raw: *mut c_void,
    pub description: Vec<u8>,
    pub width: u32,
    pub height: u32,
}
// SAFETY: immutable retained CMFormatDescription is thread safe; drop owns its reference.
unsafe impl Send for Format {}
unsafe impl Sync for Format {}
impl Drop for Format {
    fn drop(&mut self) {
        unsafe {
            sb_hevc_format_release(self.raw);
        }
    }
}
impl Format {
    pub fn new(parameters: &[Vec<u8>; 3]) -> Result<Self> {
        let mut bytes = std::ptr::null_mut();
        let mut length = 0;
        let mut width = 0;
        let mut height = 0;
        // SAFETY: all slices and output pointers remain live for the synchronous call.
        let raw = unsafe {
            sb_hevc_format(
                parameters[0].as_ptr(),
                parameters[0].len(),
                parameters[1].as_ptr(),
                parameters[1].len(),
                parameters[2].as_ptr(),
                parameters[2].len(),
                &mut bytes,
                &mut length,
                &mut width,
                &mut height,
            )
        };
        if raw.is_null() {
            return Err(Error::new(
                "PLATFORM_PROTOCOL",
                "Invalid HEVC parameter sets",
            ));
        }
        // SAFETY: native function returns an owned allocation of exactly length bytes.
        let description = unsafe {
            let data = std::slice::from_raw_parts(bytes, length).to_vec();
            sb_hevc_bytes_release(bytes.cast());
            data
        };
        Ok(Self {
            raw,
            description,
            width,
            height,
        })
    }
    pub fn codec(&self) -> Result<String> {
        let c = &self.description;
        if c.len() < 13 {
            return Err(Error::new("PLATFORM_PROTOCOL", "Invalid hvcC description"));
        }
        let profile = c[1] & 31;
        let compatible = u32::from_be_bytes(
            c[2..6]
                .try_into()
                .map_err(|_| Error::new("PLATFORM_PROTOCOL", "Invalid hvcC flags"))?,
        )
        .reverse_bits();
        let tier = if c[1] & 32 != 0 { "H" } else { "L" };
        let constraints = c[6..12]
            .iter()
            .rev()
            .skip_while(|&&v| v == 0)
            .copied()
            .collect::<Vec<_>>();
        let suffix = constraints
            .iter()
            .rev()
            .map(|v| format!(".{v:X}"))
            .collect::<String>();
        Ok(format!(
            "hev1.{profile}.{compatible:X}.{tier}{}{suffix}",
            c[12]
        ))
    }
}
pub struct Writer {
    raw: *mut c_void,
}
// SAFETY: this owner serializes all AVAssetWriter calls; no handle is shared.
unsafe impl Send for Writer {}
impl Writer {
    pub fn open(path: &std::path::Path, format: &Format) -> Result<Self> {
        let path = CString::new(
            path.to_str()
                .ok_or_else(|| Error::invalid("Recording path must be UTF-8"))?,
        )
        .map_err(|_| Error::invalid("Invalid recording path"))?;
        let raw = unsafe { sb_hevc_recording_open(path.as_ptr(), format.raw) };
        if raw.is_null() {
            return Err(Error::new(
                "RECORDING_FAILED",
                "Cannot open HEVC MP4 writer",
            ));
        }
        Ok(Self { raw })
    }
    pub fn append(&mut self, format: &Format, bytes: &[u8], pts: u64, key: bool) -> Result<bool> {
        match unsafe {
            sb_hevc_recording_append(
                self.raw,
                format.raw,
                bytes.as_ptr(),
                bytes.len(),
                pts,
                i32::from(key),
            )
        } {
            0 => Ok(true),
            1 => Ok(false),
            _ => Err(Error::new(
                "RECORDING_FAILED",
                "Cannot append HEVC access unit",
            )),
        }
    }
    pub fn finish(mut self) -> Result<()> {
        let raw = std::mem::replace(&mut self.raw, std::ptr::null_mut());
        if unsafe { sb_hevc_recording_finish(raw) } == 0 {
            Ok(())
        } else {
            Err(Error::new("RECORDING_FAILED", "Cannot finalize HEVC MP4"))
        }
    }
}
impl Drop for Writer {
    fn drop(&mut self) {
        if !self.raw.is_null() {
            unsafe {
                sb_hevc_recording_cancel(self.raw);
            }
        }
    }
}
pub fn verify(path: &std::path::Path) -> Result<serde_json::Value> {
    let path = CString::new(
        path.to_str()
            .ok_or_else(|| Error::invalid("Video path must be UTF-8"))?,
    )
    .map_err(|_| Error::invalid("Invalid video path"))?;
    let mut frames = 0;
    let mut width = 0;
    let mut height = 0;
    let mut duration = 0;
    // SAFETY: path and writable outputs live for the synchronous decode verification.
    let status = unsafe {
        sb_hevc_recording_verify(
            path.as_ptr(),
            &mut frames,
            &mut width,
            &mut height,
            &mut duration,
        )
    };
    if status != 0 {
        return Err(Error::new(
            "RECORDING_FAILED",
            if status == -2 {
                "HEVC full-track verification exceeded eight seconds"
            } else {
                "HEVC MP4 failed full-track decode/timestamp verification"
            },
        )
        .details(serde_json::json!({"decodedFrames":frames})));
    }
    Ok(
        serde_json::json!({"decodedFrames":frames,"width":width,"height":height,"lastPtsNs":duration.to_string()}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_video_observation_survives_bursts_and_crops_to_the_calibrated_display() {
        let nals = include_str!("../../tests/fixtures/ios/hevc-keyframe.hex")
            .lines()
            .map(|line| {
                line.as_bytes()
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|b| u8::from_str_radix(std::str::from_utf8(b).unwrap(), 16).unwrap())
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        let format = Format::new(&[nals[0].clone(), nals[1].clone(), nals[2].clone()]).unwrap();
        assert!(Decoder::new(&format, (321, 568)).is_err());
        let mut decoder = Decoder::new(&format, (300, 550)).unwrap();
        let mut payload = (nals[3].len() as u32).to_be_bytes().to_vec();
        payload.extend(&nals[3]);
        decoder.append(&format, &payload, 123_000_000).unwrap();
        // A lone submission must complete without a later encoded frame. Native
        // construction is synchronous and precedes this callback deadline.
        let first_deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while decoder.latest_pts() != Some(123_000_000) {
            assert!(std::time::Instant::now() < first_deadline);
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        // A burst must apply backpressure without discarding the decoder's
        // reference frames while native callbacks are still warming up.
        for frame in 1..=128 {
            decoder
                .append(&format, &payload, 123_000_000 + frame * 16_666_667)
                .unwrap();
        }
        let expected_pts = 123_000_000 + 128 * 16_666_667;
        // This guards against a stalled decoder; it is not a throughput bound.
        // An Intel Xeon host took 1.9-4.2 s to decode the 128-frame burst.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let image = loop {
            if let Some(image) = decoder.snapshot().unwrap()
                && image.pts == expected_pts
            {
                break image;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "decoder stalled at {:?} of {expected_pts}",
                decoder.latest_pts()
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        };
        assert_eq!(
            (image.width, image.height, image.pts),
            (300, 550, expected_pts)
        );
        assert_eq!(
            crate::capture::dimensions(&image.bytes).unwrap(),
            (300, 550)
        );
        assert!(image.bytes.len() > 1000);
    }
    #[test]
    fn compressed_hevc_muxes_a_decodable_mp4_without_host_encoding() {
        // Synthetic testsrc2 320x568 HEVC parameter sets and one keyframe.
        let nals = include_str!("../../tests/fixtures/ios/hevc-keyframe.hex")
            .lines()
            .map(|line| {
                line.as_bytes()
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        let format = Format::new(&[nals[0].clone(), nals[1].clone(), nals[2].clone()]).unwrap();
        assert_eq!((format.width, format.height), (320, 568));
        assert!(format.codec().unwrap().starts_with("hev1."));
        let mut payload = Vec::new();
        payload.extend_from_slice(&(nals[3].len() as u32).to_be_bytes());
        payload.extend_from_slice(&nals[3]);
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("synthetic.mp4");
        let mut writer = Writer::open(&path, &format).unwrap();
        for seq in 0..3 {
            for _ in 0..100 {
                if writer
                    .append(&format, &payload, seq * 33_333_333, true)
                    .unwrap()
                {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }
        writer.finish().unwrap();
        let checked = crate::native::verify_video_track(&path).unwrap();
        assert_eq!(checked["width"], 320);
        assert_eq!(checked["height"], 568);
        assert_eq!(checked["decodedFrames"], 3);
        assert_eq!(verify(&path).unwrap()["decodedFrames"], 3);
    }
}
