//! FFI adapter for compiled pinned ECMAScript RegExp programs used by URL
//! matching when a pattern needs backtracking features (lookaround,
//! backreferences, property escapes, `v`) or non-ASCII source text.
use std::ffi::{c_int, c_void};
use std::ptr::NonNull;
use std::time::{Duration, Instant};

#[repr(C)]
struct Program {
    _private: [u8; 0],
}
unsafe extern "C" {
    fn tr_regexp_compile(
        pattern: *const u8,
        pattern_length: usize,
        flags: *const u8,
        flags_length: usize,
        stopped: extern "C" fn(*mut c_void) -> c_int,
        control: *mut c_void,
        result: *mut *mut Program,
    ) -> c_int;
    fn tr_regexp_test(
        regexp: *const Program,
        subject: *const u16,
        length: usize,
        stopped: extern "C" fn(*mut c_void) -> c_int,
        control: *mut c_void,
    ) -> c_int;
    fn tr_regexp_free(regexp: *mut Program);
}
extern "C" fn stopped(control: *mut c_void) -> c_int {
    // SAFETY: each synchronous native call receives only a live Instant's address.
    let deadline = unsafe { &*control.cast::<Instant>() };
    c_int::from(Instant::now() >= *deadline)
}
const COMPILE_BUDGET: Duration = Duration::from_millis(500);
/// Per-URL budget; a test exceeding it or the engine's memory bound does not match.
pub(crate) const MATCH_BUDGET: Duration = Duration::from_millis(50);

pub(crate) struct Regexp(NonNull<Program>);
// SAFETY: the native program is immutable after compilation and every test
// allocates its captures and backtracking state in its own call-local runtime.
unsafe impl Send for Regexp {}
// SAFETY: see `Send`; concurrent tests only read the shared program.
unsafe impl Sync for Regexp {}
impl Regexp {
    pub(crate) fn compile(pattern: &str, flags: &str) -> Result<Self, &'static str> {
        // The upstream parser may inspect a trailing sentinel past the logical length.
        let mut source = pattern.as_bytes().to_vec();
        source.push(0);
        let deadline = Instant::now() + COMPILE_BUDGET;
        let mut result = std::ptr::null_mut();
        // SAFETY: both buffers are owned and live for this synchronous call, and
        // the native code writes only `result`.
        let status = unsafe {
            tr_regexp_compile(
                source.as_ptr(),
                pattern.len(),
                flags.as_ptr(),
                flags.len(),
                stopped,
                (&deadline as *const Instant).cast_mut().cast(),
                &mut result,
            )
        };
        match status {
            0 => NonNull::new(result).map(Self).ok_or("ROUTE_PATTERN_BOUNDS"),
            1 => Err("ROUTE_REGEX_INVALID"),
            _ => Err("ROUTE_PATTERN_BOUNDS"),
        }
    }
    pub(crate) fn is_match(&self, subject: &str) -> bool {
        let mut units: Vec<u16> = subject.encode_utf16().collect();
        let length = units.len();
        units.push(0);
        let deadline = Instant::now() + MATCH_BUDGET;
        // SAFETY: the program is live for `self`, the UTF-16 buffer is owned and
        // the call is synchronous without retaining either pointer.
        let status = unsafe {
            tr_regexp_test(
                self.0.as_ptr(),
                units.as_ptr(),
                length,
                stopped,
                (&deadline as *const Instant).cast_mut().cast(),
            )
        };
        status == 1
    }
}
impl Drop for Regexp {
    fn drop(&mut self) {
        // SAFETY: the program came from tr_regexp_compile and is freed once.
        unsafe { tr_regexp_free(self.0.as_ptr()) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn route_regex_ecma_runs_backtracking_features_and_bounds_catastrophic_matches() {
        let lookahead = Regexp::compile(r"^https://example\.com/(?!private)", "").unwrap();
        assert!(lookahead.is_match("https://example.com/public"));
        assert!(!lookahead.is_match("https://example.com/private"));
        let behind = Regexp::compile(r"(?<=/api/)v\d+$", "").unwrap();
        assert!(behind.is_match("https://example.com/api/v2"));
        let backreference = Regexp::compile(r"/(\w+)/\1$", "").unwrap();
        assert!(backreference.is_match("https://example.com/acme/acme"));
        assert!(!backreference.is_match("https://example.com/acme/other"));
        let property = Regexp::compile(r"\p{Script=Greek}", "u").unwrap();
        assert!(property.is_match("https://example.com/\u{3b1}"));
        let sets = Regexp::compile(r"[\p{L}--[a-z]]", "v").unwrap();
        assert!(sets.is_match("https://example.com/A"));
        assert!(!sets.is_match("https://example.com/a"));
        let unicode = Regexp::compile("東京", "").unwrap();
        assert!(unicode.is_match("https://example.com/東京"));
        let sticky = Regexp::compile("example", "y").unwrap();
        assert!(!sticky.is_match("https://example.com/"));
        assert_eq!(Regexp::compile("(", "").err(), Some("ROUTE_REGEX_INVALID"));
        assert_eq!(
            Regexp::compile("a", "uv").err(),
            Some("ROUTE_REGEX_INVALID")
        );
        let catastrophic = Regexp::compile("^(a+)+$", "").unwrap();
        let started = Instant::now();
        assert!(!catastrophic.is_match(&format!("{}!", "a".repeat(64))));
        assert!(started.elapsed() < MATCH_BUDGET * 4);
    }
}
