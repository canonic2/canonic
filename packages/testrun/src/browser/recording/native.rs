//! Exclusive native H.264 writer capsule; unsafe FFI stays inside this adapter.

#[cfg(not(windows))]
use crate::error::{Error, Result};
#[cfg(target_os = "macos")]
use std::ffi::{CString, c_char, c_void};
#[cfg(not(windows))]
use std::path::Path;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub(in crate::browser) use windows::available;
#[cfg(windows)]
pub(super) use windows::{Writer, verify};

#[cfg(not(windows))]
pub(super) fn verify(
    path: &Path,
    _width: u32,
    _height: u32,
    _expected: u64,
) -> Result<serde_json::Value> {
    #[cfg(target_os = "macos")]
    return crate::native::verify_video_track(path);
    #[cfg(not(target_os = "macos"))]
    {
        let _ = path;
        Err(Error::new(
            "CAPABILITY_UNAVAILABLE",
            "Browser recording is unavailable on this host",
        ))
    }
}

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn sb_browser_recording_open(path: *const c_char, width: u32, height: u32) -> *mut c_void;
    fn sb_browser_recording_append(
        handle: *mut c_void,
        bytes: *const u8,
        length: usize,
        pts: u64,
    ) -> i32;
    fn sb_browser_recording_finish(handle: *mut c_void, end: u64) -> i32;
    fn sb_browser_recording_cancel(handle: *mut c_void);
    #[cfg(test)]
    fn sb_browser_recording_corners(path: *const c_char, rgb: *mut u8) -> i32;
}

#[cfg(all(test, target_os = "macos"))]
pub(super) fn corners(path: &Path) -> [u8; 12] {
    let path = CString::new(path.to_str().unwrap()).unwrap();
    let mut rgb = [0; 12];
    // SAFETY: the path and twelve-byte output buffer remain live for the call.
    assert_eq!(
        unsafe { sb_browser_recording_corners(path.as_ptr(), rgb.as_mut_ptr()) },
        0
    );
    rgb
}

#[cfg(not(windows))]
pub(super) struct Writer {
    #[cfg(target_os = "macos")]
    handle: *mut c_void,
}
#[cfg(not(windows))]
impl Writer {
    pub fn open(path: &Path, width: u32, height: u32) -> Result<Self> {
        #[cfg(target_os = "macos")]
        {
            let path = CString::new(
                path.to_str()
                    .ok_or_else(|| Error::invalid("Recording path is not UTF-8"))?,
            )
            .map_err(|_| Error::invalid("Invalid recording path"))?;
            // SAFETY: the path remains valid for this call; returned ownership
            // is consumed exactly once by finish or Drop.
            let handle = unsafe { sb_browser_recording_open(path.as_ptr(), width, height) };
            if handle.is_null() {
                return Err(Error::new(
                    "RECORDING_UNAVAILABLE",
                    "Cannot open the native browser encoder",
                ));
            }
            Ok(Self { handle })
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (path, width, height);
            Err(Error::new(
                "CAPABILITY_UNAVAILABLE",
                "Browser recording is unavailable on this host",
            ))
        }
    }
    pub fn append(&mut self, jpeg: &[u8], pts: u64) -> Result<bool> {
        #[cfg(target_os = "macos")]
        {
            // SAFETY: the exclusive capsule and JPEG bytes outlive the
            // synchronous decode/append; the native writer retains pixels only.
            match unsafe {
                sb_browser_recording_append(self.handle, jpeg.as_ptr(), jpeg.len(), pts)
            } {
                0 => Ok(true),
                1 => Ok(false),
                _ => Err(Error::new(
                    "RECORDING_FAILED",
                    "The browser encoder could not append a frame",
                )),
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (jpeg, pts);
            Err(Error::new(
                "CAPABILITY_UNAVAILABLE",
                "Browser recording is unavailable on this host",
            ))
        }
    }
    pub fn finish(mut self, end: u64) -> Result<()> {
        #[cfg(target_os = "macos")]
        {
            let handle = std::mem::replace(&mut self.handle, std::ptr::null_mut());
            // SAFETY: ownership is transferred once and this capsule's Drop
            // sees null even when native finalization fails.
            if unsafe { sb_browser_recording_finish(handle, end) } != 0 {
                return Err(Error::new(
                    "RECORDING_FAILED",
                    "Native browser recording finalization failed",
                ));
            }
            Ok(())
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (&mut self, end);
            Err(Error::new(
                "CAPABILITY_UNAVAILABLE",
                "Browser recording is unavailable on this host",
            ))
        }
    }
}
#[cfg(not(windows))]
impl Drop for Writer {
    fn drop(&mut self) {
        #[cfg(target_os = "macos")]
        if !self.handle.is_null() {
            // SAFETY: Drop still owns this unfinalized capsule exclusively.
            unsafe { sb_browser_recording_cancel(self.handle) };
        }
    }
}
