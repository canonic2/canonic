//! Windows recording backend; its capsule stays on the recording worker.
use crate::error::{Error, Result};
use serde_json::{Value, json};
use std::{ffi::c_void, os::windows::ffi::OsStrExt, path::Path, ptr::NonNull, sync::OnceLock};

unsafe extern "C" {
    fn sb_browser_mf_available() -> i32;
    fn sb_browser_mf_open(path: *const u16, width: u32, height: u32, out: *mut *mut c_void) -> i32;
    fn sb_browser_mf_append(handle: *mut c_void, data: *const u8, size: usize, pts: i64) -> i32;
    fn sb_browser_mf_finish(handle: *mut c_void, end: i64) -> i32;
    fn sb_browser_mf_close(handle: *mut c_void);
    fn sb_browser_mf_verify(
        path: *const u16,
        width: u32,
        height: u32,
        expected: u64,
        frames: *mut u64,
        last_pts: *mut u64,
        phase: *mut u32,
    ) -> i32;
}

pub(in crate::browser) fn available() -> bool {
    static AVAILABLE: OnceLock<bool> = OnceLock::new();
    // SAFETY: this only checks the system-directory library exports and holds no
    // native interfaces or caller-owned memory.
    *AVAILABLE.get_or_init(|| unsafe { sb_browser_mf_available() != 0 })
}

fn wide_path(path: &Path) -> Result<Vec<u16>> {
    let mut path: Vec<_> = path.as_os_str().encode_wide().collect();
    if path.contains(&0) {
        return Err(Error::invalid("Invalid recording path"));
    }
    path.push(0);
    Ok(path)
}

pub(in crate::browser::recording) struct Writer {
    handle: NonNull<c_void>,
    width: u32,
    height: u32,
}

impl Writer {
    pub fn open(path: &Path, width: u32, height: u32) -> Result<Self> {
        let path = wide_path(path)?;
        let mut handle = std::ptr::null_mut();
        // SAFETY: the wide path is NUL terminated, and handle is an out pointer
        // to writable storage for this call.
        let status = unsafe { sb_browser_mf_open(path.as_ptr(), width, height, &mut handle) };
        let handle = NonNull::new(handle).ok_or_else(|| {
            Error::new(
                "RECORDING_UNAVAILABLE",
                "Cannot open the Windows browser encoder",
            )
            .details(json!({"nativeStatus":status}))
        })?;
        Ok(Self {
            handle,
            width,
            height,
        })
    }

    pub fn append(&mut self, jpeg: &[u8], pts: u64) -> Result<bool> {
        let pts = i64::try_from(pts).map_err(|_| Error::invalid("Invalid recording timestamp"))?;
        let pixels = super::super::pixels::nv12(jpeg, self.width, self.height)?;
        // SAFETY: the capsule is exclusive to this worker and the pixels
        // remain valid until the native writer has copied them.
        let status = unsafe {
            sb_browser_mf_append(self.handle.as_ptr(), pixels.as_ptr(), pixels.len(), pts)
        };
        check(status, "Windows browser encoder could not append a frame")?;
        Ok(true)
    }

    pub fn finish(self, end: u64) -> Result<()> {
        let end = i64::try_from(end).map_err(|_| Error::invalid("Invalid recording timestamp"))?;
        // SAFETY: finish and Drop run on the apartment that owns the capsule.
        let status = unsafe { sb_browser_mf_finish(self.handle.as_ptr(), end) };
        check(status, "Windows browser recording finalization failed")
    }
}

impl Drop for Writer {
    fn drop(&mut self) {
        // SAFETY: this is the sole owner of the live native capsule.
        unsafe { sb_browser_mf_close(self.handle.as_ptr()) };
    }
}

fn check(status: i32, message: &str) -> Result<()> {
    if status < 0 {
        return Err(Error::new("RECORDING_FAILED", message).details(json!({"nativeStatus":status})));
    }
    Ok(())
}

pub(in crate::browser::recording) fn verify(
    path: &Path,
    width: u32,
    height: u32,
    expected: u64,
) -> Result<Value> {
    let path = wide_path(path)?;
    let mut frames = 0;
    let mut last_pts = 0;
    let mut phase = 0;
    // SAFETY: path is NUL-terminated, and both output integers are live and
    // writable until the synchronous decode returns.
    let status = unsafe {
        sb_browser_mf_verify(
            path.as_ptr(),
            width,
            height,
            expected,
            &mut frames,
            &mut last_pts,
            &mut phase,
        )
    };
    // HRESULT_FROM_WIN32(ERROR_TIMEOUT)
    if status == 0x8007_05b4_u32.cast_signed() {
        return Err(Error::new(
            "TIMEOUT",
            "Full-track video verification exhausted its budget",
        )
        .details(json!({"nativeStatus":status,"decodedFrames":frames,"nativePhase":phase})));
    }
    if status < 0 {
        return Err(Error::new(
            "INVALID_ARTIFACT",
            "Windows browser movie failed full-track decoding",
        )
        .details(json!({"nativeStatus":status,"decodedFrames":frames,"nativePhase":phase})));
    }
    Ok(
        json!({"decodable":true,"verification":"full_track","width":width,"height":height,
        "decodedFrames":frames,"lastPtsNs":last_pts}),
    )
}

#[cfg(test)]
#[test]
fn media_foundation_round_trip_preserves_a_non_macroblock_viewport() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("Acme viewport.mp4");
    let image = image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
        640,
        360,
        image::Rgb([255, 0, 0]),
    ));
    let mut jpeg = std::io::Cursor::new(Vec::new());
    image.write_to(&mut jpeg, image::ImageFormat::Jpeg).unwrap();
    let mut writer = Writer::open(&path, 640, 360).unwrap();
    writer.append(jpeg.get_ref(), 0).unwrap();
    writer.append(jpeg.get_ref(), 50_000_000).unwrap();
    writer.finish(100_000_000).unwrap();
    let report = verify(&path, 640, 360, 2).unwrap();
    assert_eq!(report["decodedFrames"], 2);
    assert_eq!(report["width"], 640);
    assert_eq!(report["height"], 360);
}
