//! FFI adapter for the pinned standalone RegExp engine; no general JS execution.
use super::*;
use std::ffi::{c_int, c_void};

#[repr(C)]
#[derive(Clone, Copy)]
struct Span {
    data: *const u16,
    length: usize,
}
#[repr(C)]
struct Criterion {
    kind: c_int,
    exact: Span,
    pattern: *const u8,
    pattern_length: usize,
    flags: *const u8,
    flags_length: usize,
}
#[repr(C)]
struct Attributes {
    fields: [Span; 3],
}
unsafe extern "C" {
    fn tr_cookie_filter(
        criteria: *const Criterion,
        cookies: *const Attributes,
        count: usize,
        matches: *mut u8,
        stopped: extern "C" fn(*mut c_void) -> c_int,
        control: *mut c_void,
    ) -> c_int;
}
struct Control {
    deadline: Instant,
    cancelled: Arc<AtomicBool>,
}
extern "C" fn stopped(control: *mut c_void) -> c_int {
    // SAFETY: the synchronous native call only receives this live Control's address.
    let control = unsafe { &*control.cast::<Control>() };
    c_int::from(control.cancelled.load(Ordering::SeqCst) || Instant::now() >= control.deadline)
}
fn run(
    mut encoded: Vec<[Vec<u16>; 3]>,
    filter: &CookieFilter,
    deadline: Instant,
    cancelled: Arc<AtomicBool>,
) -> Result<Vec<bool>, Error> {
    let values = [
        filter.name.as_ref(),
        filter.domain.as_ref(),
        filter.path.as_ref(),
    ];
    // The upstream parser/string reader may inspect its trailing sentinel.
    // Logical lengths exclude these owned zero units, including embedded NULs.
    for fields in &mut encoded {
        for field in fields {
            field.push(0);
        }
    }
    let patterns: [Vec<u8>; 3] = std::array::from_fn(|index| {
        let mut bytes = match values[index] {
            Some(CookieMatch::Regex(regex)) => regex.regex.as_bytes().to_vec(),
            _ => vec![],
        };
        bytes.push(0);
        bytes
    });
    let exact: [Vec<u16>; 3] = std::array::from_fn(|index| match values[index] {
        Some(CookieMatch::Exact(value)) => value.encode_utf16().collect(),
        _ => vec![],
    });
    let criteria: [Criterion; 3] = std::array::from_fn(|index| {
        let mut criterion = Criterion {
            kind: 0,
            exact: Span {
                data: exact[index].as_ptr(),
                length: exact[index].len(),
            },
            pattern: std::ptr::null(),
            pattern_length: 0,
            flags: std::ptr::null(),
            flags_length: 0,
        };
        match values[index] {
            Some(CookieMatch::Exact(_)) => criterion.kind = 1,
            Some(CookieMatch::Regex(regex)) => {
                criterion.kind = 2;
                criterion.pattern = patterns[index].as_ptr();
                criterion.pattern_length = patterns[index].len() - 1;
                criterion.flags = regex.flags.as_ptr();
                criterion.flags_length = regex.flags.len();
            }
            None => {}
        }
        criterion
    });
    let attributes = encoded
        .iter()
        .map(|value| Attributes {
            fields: std::array::from_fn(|index| Span {
                data: value[index].as_ptr(),
                length: value[index].len() - 1,
            }),
        })
        .collect::<Vec<_>>();
    let control = Control {
        deadline,
        cancelled,
    };
    let mut matches = vec![0u8; attributes.len()];
    // SAFETY: every span points into owned, live UTF-16/UTF-8 buffers, and every
    // array has the declared length. Native code is synchronous, writes only
    // the result array and cannot retain these pointers or execute host I/O.
    let result = unsafe {
        tr_cookie_filter(
            criteria.as_ptr(),
            attributes.as_ptr(),
            attributes.len(),
            matches.as_mut_ptr(),
            stopped,
            (&control as *const Control).cast_mut().cast(),
        )
    };
    if control.cancelled.load(Ordering::SeqCst) {
        return Err("CANCELLED".into());
    }
    if Instant::now() >= deadline {
        return Err("HTTP_DEADLINE".into());
    }
    match result {
        0 => Ok(matches.into_iter().map(|value| value != 0).collect()),
        1 => Err("HTTP_COOKIE_REGEX_INVALID".into()),
        2 => Err("HTTP_COOKIE_FILTER_BOUNDS".into()),
        _ => Err("HTTP_COOKIE_FILTER_FAILED".into()),
    }
}
pub(super) fn evaluate(
    attributes: Vec<[String; 3]>,
    filter: CookieFilter,
    deadline: Instant,
    cancelled: Arc<AtomicBool>,
) -> Result<Vec<usize>, Error> {
    let encoded = attributes
        .iter()
        .map(|fields| {
            fields
                .each_ref()
                .map(|value| value.encode_utf16().collect())
        })
        .collect();
    Ok(run(encoded, &filter, deadline, cancelled)?
        .into_iter()
        .enumerate()
        .filter_map(|(index, matched)| matched.then_some(index))
        .collect())
}
#[cfg(all(test, feature = "acceptance-browser-public"))]
pub(super) fn classify_codepoints(
    pattern: &str,
    flags: &str,
    deadline: Instant,
) -> Result<Vec<bool>, Error> {
    unsafe extern "C" {
        fn tr_cookie_filter_codepoints(
            criteria: *const Criterion,
            matches: *mut u8,
            stopped: extern "C" fn(*mut c_void) -> c_int,
            control: *mut c_void,
        ) -> c_int;
    }
    let mut bytes = pattern.as_bytes().to_vec();
    bytes.push(0);
    let criteria: [Criterion; 3] = std::array::from_fn(|index| Criterion {
        kind: if index == 0 { 2 } else { 0 },
        exact: Span {
            data: std::ptr::null(),
            length: 0,
        },
        pattern: bytes.as_ptr(),
        pattern_length: bytes.len() - 1,
        flags: flags.as_ptr(),
        flags_length: flags.len(),
    });
    let control = Control {
        deadline,
        cancelled: Arc::new(AtomicBool::new(false)),
    };
    let mut matches = vec![0u8; 0x110000];
    // SAFETY: the reference entry point generates bounded UTF-16 spans itself
    // and writes exactly one result for each Unicode code point.
    let result = unsafe {
        tr_cookie_filter_codepoints(
            criteria.as_ptr(),
            matches.as_mut_ptr(),
            stopped,
            (&control as *const Control).cast_mut().cast(),
        )
    };
    if Instant::now() >= deadline {
        return Err("HTTP_DEADLINE".into());
    }
    match result {
        0 => Ok(matches.into_iter().map(|value| value != 0).collect()),
        1 => Err("HTTP_COOKIE_REGEX_INVALID".into()),
        2 => Err("HTTP_COOKIE_FILTER_BOUNDS".into()),
        _ => Err("HTTP_COOKIE_FILTER_FAILED".into()),
    }
}
