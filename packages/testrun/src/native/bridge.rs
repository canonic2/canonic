//! Worker-local Apple FFI and native handle ownership.
use crate::error::{Error, Result};
use std::ffi::{CString, c_char, c_void};
unsafe extern "C" {
    #[cfg(test)]
    fn sb_simulator_process_identity(pid: i32, seconds: *mut u64, micros: *mut u64) -> i32;
    fn sb_simulator_open(
        developer: *const c_char,
        uuid: *const c_char,
        failure: *mut i32,
    ) -> *mut c_void;
    fn sb_simulator_close(handle: *mut c_void);
    fn sb_simulator_identity_open(developer: *const c_char, uuid: *const c_char) -> *mut c_void;
    fn sb_simulator_peer(
        handle: *mut c_void,
        pid: *mut i32,
        seconds: *mut u64,
        micros: *mut u64,
    ) -> i32;
    fn sb_simulator_peer_alive(pid: i32, seconds: u64, micros: u64) -> i32;
    fn sb_simulator_input_open(developer: *const c_char, uuid: *const c_char) -> *mut c_void;
    fn sb_simulator_input_gesture(
        handle: *mut c_void,
        x: f64,
        y: f64,
        end_x: f64,
        end_y: f64,
        duration: u32,
    ) -> i32;
    fn sb_simulator_input_keys(
        handle: *mut c_void,
        strokes: *const crate::keyboard::Stroke,
        count: usize,
    ) -> i32;
    fn sb_simulator_input_release(
        handle: *mut c_void,
        touch: i32,
        keys: *const u32,
        count: usize,
    ) -> i32;
    fn sb_simulator_device_path(handle: *mut c_void) -> *const c_char;
    fn sb_simulator_geometry(handle: *mut c_void, width: *mut u32, height: *mut u32) -> i32;
    fn sb_simulator_keyframe(handle: *mut c_void) -> i32;
    fn sb_simulator_source_size(handle: *mut c_void, width: *mut u32, height: *mut u32);
    fn sb_simulator_hardware_encoder(handle: *mut c_void) -> i32;
    fn sb_simulator_activity(handle: *mut c_void) -> u64;
    fn sb_simulator_callbacks(handle: *mut c_void) -> i32;
    fn sb_simulator_force_keyframe(handle: *mut c_void);
    fn sb_simulator_recording_start(handle: *mut c_void, path: *const c_char) -> i32;
    fn sb_simulator_recording_detach(handle: *mut c_void, drops: *mut u64) -> *mut c_void;
    fn sb_simulator_recording_complete(handle: *mut c_void) -> i32;
    fn sb_simulator_recording_dispose(handle: *mut c_void);
    fn sb_video_track_verify(
        path: *const c_char,
        frames: *mut u64,
        width: *mut u32,
        height: *mut u32,
        last_pts: *mut u64,
        budget_seconds: f64,
    ) -> i32;
    fn sb_simulator_frame(
        handle: *mut c_void,
        jpeg: i32,
        size: *mut usize,
        width: *mut u32,
        height: *mut u32,
        failure: *mut i32,
    ) -> *mut u8;
    fn sb_simulator_free(bytes: *mut c_void);
    fn sb_simulator_accessibility(
        developer: *const c_char,
        uuid: *const c_char,
        size: *mut usize,
        failure: *mut i32,
    ) -> *mut u8;
}
/// A worker-local native connection. This pointer is deliberately not Send.
pub struct Capture {
    handle: *mut c_void,
}
/// One worker-local retained HID connection, independently owned from capture.
pub(crate) struct Input {
    handle: *mut c_void,
}
impl Input {
    pub(crate) fn open(developer: &str, uuid: &str) -> Result<Self> {
        let developer = cstring(developer)?;
        let uuid = cstring(uuid)?;
        // SAFETY: live NUL-terminated identifiers; successful handle has one Drop owner.
        let handle = unsafe { sb_simulator_input_open(developer.as_ptr(), uuid.as_ptr()) };
        if handle.is_null() {
            return Err(Error::new(
                "INPUT_UNAVAILABLE",
                "Cannot open retained simulator input; check Xcode/runtime support and controller ownership",
            ));
        }
        Ok(Self { handle })
    }
    pub(crate) fn incarnation(
        &self,
        uuid: &str,
    ) -> Result<crate::simulator::incarnation::Incarnation> {
        handle_incarnation(self.handle, uuid)
    }
    pub(crate) fn geometry(&self) -> Result<(u32, u32)> {
        handle_geometry(self.handle)
    }
    pub(crate) fn perform(&self, action: &crate::simulator::input::protocol::Action) -> Result<()> {
        use crate::simulator::input::protocol::Action;
        action.validate()?;
        // SAFETY: exclusively owned live handle, validated scalars and bounded slices
        // outlive synchronous native calls. Native sends retain their completion state.
        let status = unsafe {
            match action {
                Action::Gesture {
                    start,
                    end,
                    duration,
                } => sb_simulator_input_gesture(
                    self.handle,
                    start[0],
                    start[1],
                    end[0],
                    end[1],
                    *duration,
                ),
                Action::Keys { strokes } => {
                    sb_simulator_input_keys(self.handle, strokes.as_ptr(), strokes.len())
                }
                Action::Release { touch, keys } => sb_simulator_input_release(
                    self.handle,
                    i32::from(*touch),
                    keys.as_ptr(),
                    keys.len(),
                ),
                Action::Close => 0,
            }
        };
        if status != 0 {
            return Err(native_error(status));
        }
        Ok(())
    }
}
impl Drop for Input {
    fn drop(&mut self) {
        // SAFETY: releases this sole bridge-retained object; does not touch a capture source.
        unsafe { sb_simulator_close(self.handle) };
    }
}
fn handle_incarnation(
    handle: *mut c_void,
    uuid: &str,
) -> Result<crate::simulator::incarnation::Incarnation> {
    // SAFETY: caller retains the worker-local native object during the path copy.
    let path = unsafe { sb_simulator_device_path(handle) };
    if path.is_null() {
        return Err(native_error(1));
    }
    // SAFETY: native-selected device owns the NUL-terminated path during this copy.
    let path = unsafe { std::ffi::CStr::from_ptr(path) }
        .to_str()
        .map_err(|_| native_error(1))?;
    let mut peer = crate::simulator::incarnation::Peer {
        pid: 0,
        seconds: 0,
        micros: 0,
    };
    // SAFETY: live object owns its XPC connection; initialized outputs are valid.
    if unsafe { sb_simulator_peer(handle, &mut peer.pid, &mut peer.seconds, &mut peer.micros) } != 0
    {
        return Err(native_error(7));
    }
    Ok(
        crate::simulator::incarnation::Incarnation::read(std::path::Path::new(path), uuid)?
            .with_peer(peer),
    )
}
pub(crate) fn identity_connection(
    developer: &str,
    uuid: &str,
) -> Result<crate::simulator::incarnation::Incarnation> {
    struct Binding(*mut c_void);
    impl Drop for Binding {
        fn drop(&mut self) {
            // SAFETY: the only owner releases its bridge-retained identity connection.
            unsafe { sb_simulator_close(self.0) };
        }
    }
    let developer = cstring(developer)?;
    let name = cstring(uuid)?;
    // SAFETY: identifiers outlive the call; no display, HID or AX resource is opened.
    let handle = unsafe { sb_simulator_identity_open(developer.as_ptr(), name.as_ptr()) };
    if handle.is_null() {
        return Err(native_error(8));
    }
    let binding = Binding(handle);
    handle_incarnation(binding.0, uuid)
}
pub(crate) fn peer_alive(peer: crate::simulator::incarnation::Peer) -> bool {
    // SAFETY: read-only kernel process-identity query with scalar values.
    unsafe { sb_simulator_peer_alive(peer.pid, peer.seconds, peer.micros) != 0 }
}
#[cfg(test)]
pub(crate) fn process_peer(pid: i32) -> crate::simulator::incarnation::Peer {
    let mut peer = crate::simulator::incarnation::Peer {
        pid,
        seconds: 0,
        micros: 0,
    };
    // SAFETY: read-only query for a test-owned process with initialized outputs.
    assert_ne!(
        unsafe { sb_simulator_process_identity(pid, &mut peer.seconds, &mut peer.micros) },
        0
    );
    peer
}
fn handle_geometry(handle: *mut c_void) -> Result<(u32, u32)> {
    let (mut w, mut h) = (0, 0);
    // SAFETY: caller retains the object; both output pointers are initialized.
    let status = unsafe { sb_simulator_geometry(handle, &mut w, &mut h) };
    if status != 0 || w == 0 || h == 0 || w > 8192 || h > 8192 {
        return Err(native_error(status));
    }
    Ok((w, h))
}
impl Capture {
    pub(crate) fn check_connection(&self) -> Result<()> {
        let (mut pid, mut seconds, mut micros) = (0, 0, 0);
        // SAFETY: the worker-local object retains the XPC connection; outputs are valid.
        if unsafe { sb_simulator_peer(self.handle, &mut pid, &mut seconds, &mut micros) } != 0 {
            return Err(native_error(7));
        }
        Ok(())
    }
    pub(crate) fn incarnation(
        &self,
        uuid: &str,
    ) -> Result<crate::simulator::incarnation::Incarnation> {
        handle_incarnation(self.handle, uuid)
    }
    pub(crate) fn geometry(&self) -> Result<(u32, u32)> {
        handle_geometry(self.handle)
    }
    pub fn open(developer: &str, uuid: &str) -> Result<Self> {
        let developer = cstring(developer)?;
        let uuid = cstring(uuid)?;
        let mut failure = 0;
        // SAFETY: the input strings outlive the call, the native bridge retains
        // its objects, and successful handles are released exactly once by Drop.
        let handle = unsafe { sb_simulator_open(developer.as_ptr(), uuid.as_ptr(), &mut failure) };
        if handle.is_null() {
            return Err(native_error(failure));
        }
        Ok(Self { handle })
    }
    pub fn frame(&self, jpeg: bool) -> Result<(Vec<u8>, u32, u32)> {
        self.frame_codec(i32::from(jpeg))
    }
    pub fn frame_codec(&self, codec: i32) -> Result<(Vec<u8>, u32, u32)> {
        let mut size = 0;
        let mut failure = 0;
        let mut width = 0;
        let mut height = 0;
        // SAFETY: the connection is live and worker-local; output pointers are
        // initialized by the bridge, whose allocation is bounded at 64 MiB.
        let bytes = unsafe {
            sb_simulator_frame(
                self.handle,
                codec,
                &mut size,
                &mut width,
                &mut height,
                &mut failure,
            )
        };
        if bytes.is_null() {
            return Err(native_error(failure));
        }
        // SAFETY: successful allocation contains exactly size initialized bytes.
        let result = unsafe { std::slice::from_raw_parts(bytes, size).to_vec() };
        // SAFETY: frees the same allocation through its originating allocator.
        unsafe { sb_simulator_free(bytes.cast()) };
        Ok((result, width, height))
    }
    pub fn keyframe(&self) -> bool {
        // SAFETY: the capture is live and this read is worker-local.
        unsafe { sb_simulator_keyframe(self.handle) != 0 }
    }
    pub fn source_size(&self) -> (u32, u32) {
        let mut width = 0;
        let mut height = 0;
        // SAFETY: the connection is live; both output pointers are valid.
        unsafe { sb_simulator_source_size(self.handle, &mut width, &mut height) };
        (width, height)
    }
    pub fn hardware_encoder(&self) -> Option<bool> {
        // SAFETY: this capture is live; the bridge owns the property value.
        match unsafe { sb_simulator_hardware_encoder(self.handle) } {
            0 => Some(false),
            1 => Some(true),
            _ => None,
        }
    }
    pub fn activity(&self) -> u64 {
        // SAFETY: this is a live, worker-local capture connection.
        unsafe { sb_simulator_activity(self.handle) }
    }
    pub fn callbacks(&self) -> bool {
        // SAFETY: this is a live, worker-local capture connection.
        unsafe { sb_simulator_callbacks(self.handle) != 0 }
    }
    pub fn force_keyframe(&self) {
        // SAFETY: this capture has one worker-local owner and no concurrent encoder calls.
        unsafe { sb_simulator_force_keyframe(self.handle) };
    }
    pub fn recording_start(&self, path: &std::path::Path) -> Result<()> {
        let path = cstring(
            path.to_str()
                .ok_or_else(|| Error::invalid("Recording path is not UTF-8"))?,
        )?;
        // SAFETY: the live connection retains a copy of this C string.
        let status = unsafe { sb_simulator_recording_start(self.handle, path.as_ptr()) };
        if status != 0 {
            return Err(Error::new(
                "RECORDING_FAILED",
                "Cannot initialize native recording",
            ));
        }
        Ok(())
    }
    pub fn recording_detach(&self) -> Result<(RecordingFinalizer, u64)> {
        let mut drops = 0;
        // SAFETY: all encoding callbacks have completed before this worker-local
        // finalization call; the output pointer is valid throughout the call.
        let handle = unsafe { sb_simulator_recording_detach(self.handle, &mut drops) };
        if handle.is_null() {
            return Err(Error::new(
                "RECORDING_FAILED",
                "Native MP4 writer could not be detached",
            ));
        }
        Ok((RecordingFinalizer(handle), drops))
    }
}
/// An exclusively owned writer detached from the worker-local capture/encoder.
pub struct RecordingFinalizer(*mut c_void);
// SAFETY: detachment removes every writer reference from Capture, after encoding
// callbacks finish. Only this owner uses the AVAssetWriter on its finalizer thread.
unsafe impl Send for RecordingFinalizer {}
impl RecordingFinalizer {
    pub fn finish(self) -> Result<()> {
        // SAFETY: this owner holds the bridge-retained writer until Drop; no
        // capture callback can append samples or access it after detachment.
        let status = unsafe { sb_simulator_recording_complete(self.0) };
        if status == 0 {
            Ok(())
        } else {
            Err(Error::new(
                "RECORDING_FAILED",
                format!("Native MP4 finalization failed ({status})"),
            ))
        }
    }
}
impl Drop for RecordingFinalizer {
    fn drop(&mut self) {
        // SAFETY: exactly one bridge-retained writer is released by this owner.
        unsafe { sb_simulator_recording_dispose(self.0) };
    }
}
impl Drop for Capture {
    fn drop(&mut self) {
        // SAFETY: this is the sole owner and the handle has not been released.
        unsafe { sb_simulator_close(self.handle) };
    }
}
fn cstring(value: &str) -> Result<CString> {
    CString::new(value).map_err(|_| Error::invalid("Native identifier contains a NUL byte"))
}
fn native_error(failure: i32) -> Error {
    if failure == 7 {
        return Error::new(
            "STALE_TARGET",
            "Simulator service connection changed; discover and reconnect without repeating uncertain input",
        );
    }
    if failure == 8 {
        return Error::new(
            "CAPABILITY_UNAVAILABLE",
            "Cannot verify simulator service identity on this Xcode/runtime; use a supported host/runtime",
        );
    }
    Error::new(
        "NATIVE_ADAPTER_UNAVAILABLE",
        format!("Native simulator adapter failed ({failure})"),
    )
}
pub fn accessibility(developer: &str, uuid: &str) -> Result<Vec<u8>> {
    let developer = cstring(developer)?;
    let uuid = cstring(uuid)?;
    let mut size = 0;
    let mut failure = 0;
    // SAFETY: strings and output pointers are live throughout this call;
    // successful responses are malloc-owned and limited to 2 MiB by the bridge.
    let bytes = unsafe {
        sb_simulator_accessibility(developer.as_ptr(), uuid.as_ptr(), &mut size, &mut failure)
    };
    if bytes.is_null() {
        return Err(native_error(failure));
    }
    // SAFETY: the bridge initialized exactly size bytes and retains ownership
    // until we call its matching deallocator after copying.
    let result = unsafe { std::slice::from_raw_parts(bytes, size).to_vec() };
    unsafe { sb_simulator_free(bytes.cast()) };
    Ok(result)
}
pub(crate) fn verify_video_track(path: &std::path::Path) -> Result<serde_json::Value> {
    let path = cstring(
        path.to_str()
            .ok_or_else(|| Error::invalid("Video path is not UTF-8"))?,
    )?;
    let mut width = 0;
    let mut height = 0;
    let mut frames = 0;
    let mut last_pts = 0;
    // SAFETY: the path and output pointers remain valid throughout this call.
    let status = unsafe {
        sb_video_track_verify(
            path.as_ptr(),
            &mut frames,
            &mut width,
            &mut height,
            &mut last_pts,
            60.0,
        )
    };
    if status != 0 {
        return Err(Error::new(
            if status == -2 {
                "TIMEOUT"
            } else {
                "INVALID_ARTIFACT"
            },
            if status == -2 {
                "Full-track video verification exhausted its budget"
            } else {
                "Native video artifact did not decode completely"
            },
        )
        .details(serde_json::json!({"nativeStatus":status,"decodedFrames":frames})));
    }
    Ok(
        serde_json::json!({"decodable":true,"verification":"full_track","width":width,"height":height,"decodedFrames":frames,"lastPtsNs":last_pts}),
    )
}
