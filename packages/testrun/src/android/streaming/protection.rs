//! Conservative native window admission; unknown or changing policy never exports retained video.
use crate::error::{Error, Result};
#[cfg(target_os = "macos")]
use std::time::Duration;
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Window {
    identity: String,
    allowed: bool,
}
impl Window {
    pub(super) fn admissible(&self) -> bool {
        self.allowed
    }
}
#[cfg(target_os = "macos")]
pub(super) async fn inspect(adapter: &super::super::Android, serial: &str) -> Result<Window> {
    // One transport for both native snapshots; the random separator keeps the
    // textual window dump separate from the binary layer protobuf.
    let separator = format!("\nTESTRUN_LAYERS_{}\n", uuid::Uuid::new_v4().simple());
    // Preserve confirmed window protection if the layer service fails. A zero
    // protobuf key makes failed/partial layer output unknown, never admissible.
    let script = format!(
        "dumpsys window || exit; printf '{separator}'; dumpsys SurfaceFlinger --proto || printf '\\000'"
    );
    let bytes = match adapter
        .script(serial, &script, Duration::from_secs(4), 8 * 1024 * 1024)
        .await
    {
        Ok(bytes) => bytes,
        Err(error) => {
            // A hung layer service must not hide confirmed window protection.
            // This extra call is restricted to failed combined snapshots.
            if let Ok(windows) = adapter
                .script(
                    serial,
                    "dumpsys window",
                    Duration::from_secs(2),
                    4 * 1024 * 1024,
                )
                .await
            {
                parse(&String::from_utf8_lossy(&windows))?;
            }
            return Err(error);
        }
    };
    snapshot(&bytes, &separator)
}
fn snapshot(bytes: &[u8], separator: &str) -> Result<Window> {
    let boundary = bytes
        .windows(separator.len())
        .position(|part| part == separator.as_bytes())
        .ok_or_else(|| {
            Error::new(
                "PLATFORM_PROTOCOL",
                "Android capture policy snapshot is incomplete",
            )
        })?;
    let mut window = parse(&String::from_utf8_lossy(&bytes[..boundary]))?;
    match composition(&bytes[boundary + separator.len()..]) {
        Ok(Some(identity)) => window.identity.push_str(&identity),
        Ok(None) => window.allowed = false,
        Err(error) => return Err(error),
    }
    Ok(window)
}
fn parse(text: &str) -> Result<Window> {
    let mut focus = None;
    let mut visible = false;
    let mut secure = false;
    let mut uncertain = false;
    let mut in_window = false;
    let mut flags_known = false;
    let mut identity = String::new();
    for line in text.lines().map(str::trim) {
        if let Some(value) = line.strip_prefix("mCurrentFocus=Window{") {
            focus = Some(value.to_owned());
        }
        if line.starts_with("Window #") {
            if visible && secure {
                return blocked();
            }
            if in_window && !flags_known {
                uncertain = true;
            }
            visible = false;
            secure = false;
            in_window = true;
            flags_known = false;
            identity.push_str(line);
        }
        if in_window && let Some(flags) = super::super::window::flags(line) {
            match flags {
                super::super::window::Flags::Secure => {
                    secure = true;
                    flags_known = true;
                }
                super::super::window::Flags::Clear => flags_known = true,
                super::super::window::Flags::Unknown => uncertain = true,
            }
            identity.push_str(line);
        }
        if line == "isVisible=true" || line.contains("isOnScreen=true") {
            visible = true;
        }
        // A visible secure surface is a full refusal, not a request to change backend.
        if line.contains("secure=true") || line.contains("isSecure=true") {
            secure = true;
        }
    }
    if visible && secure {
        return blocked();
    }
    if in_window && !flags_known {
        uncertain = true;
    }
    let Some(focus) = focus else {
        return Ok(Window {
            identity,
            allowed: false,
        });
    };
    identity.push_str(&focus);
    // Composed child layers are checked separately; a focused window alone is insufficient.
    Ok(Window {
        allowed: in_window && !uncertain && !focus.starts_with("null"),
        identity,
    })
}
// Android 12 LayersProto: repeated LayerProto field 1; layer id/name/flags/protection fields 1/2/22/36.
// https://android.googlesource.com/platform/frameworks/native/+/refs/heads/android12-release/services/surfaceflinger/layerproto/layers.proto
// Refuse all secure layers and protected buffers, including possibly hidden ones,
// rather than infer visibility.
fn composition(bytes: &[u8]) -> Result<Option<String>> {
    let Some(top) = fields(bytes) else {
        return Ok(None);
    };
    let mut identity = String::new();
    let mut count = 0;
    for (number, _, layer) in top {
        if number != 1 {
            return Ok(None);
        }
        let Some(layer) = fields(layer) else {
            return Ok(None);
        };
        let mut id = None;
        let mut named = false;
        let mut flags = 0;
        let mut protected = false;
        for (number, value, bytes) in layer {
            match number {
                1 => id = value,
                2 => named = !bytes.is_empty(),
                22 => flags = value.unwrap_or(u64::MAX),
                36 => match value {
                    Some(value) => protected = value != 0,
                    None => return Ok(None),
                },
                _ => {}
            }
        }
        if flags & 0x80 != 0 || protected {
            return blocked();
        }
        let Some(id) = id.filter(|_| named) else {
            return Ok(None);
        };
        use std::fmt::Write;
        let _ = write!(identity, "/{id}:{flags}");
        count += 1;
        if count > 4096 {
            return Ok(None);
        }
    }
    Ok((count > 0).then_some(identity))
}
type ProtoField<'a> = (u64, Option<u64>, &'a [u8]);
fn fields(bytes: &[u8]) -> Option<Vec<ProtoField<'_>>> {
    let mut remaining = bytes;
    let mut output = Vec::new();
    while !remaining.is_empty() {
        let key = varint(&mut remaining)?;
        if key >> 3 == 0 || output.len() >= 16384 {
            return None;
        }
        let (value, payload) = match key & 7 {
            0 => (Some(varint(&mut remaining)?), &[][..]),
            1 | 5 => {
                let size = if key & 7 == 1 { 8 } else { 4 };
                let (_, rest) = remaining.split_at_checked(size)?;
                remaining = rest;
                (None, &[][..])
            }
            2 => {
                let size = usize::try_from(varint(&mut remaining)?).ok()?;
                let (payload, rest) = remaining.split_at_checked(size)?;
                remaining = rest;
                (None, payload)
            }
            _ => return None,
        };
        output.push((key >> 3, value, payload));
    }
    Some(output)
}
fn varint(bytes: &mut &[u8]) -> Option<u64> {
    let mut value = 0;
    for shift in (0..70).step_by(7) {
        let (&byte, rest) = bytes.split_first()?;
        *bytes = rest;
        if shift == 63 && byte > 1 {
            return None;
        }
        value |= u64::from(byte & 127) << shift;
        if byte & 128 == 0 {
            return Some(value);
        }
    }
    None
}
fn blocked<T>() -> Result<T> {
    Err(Error::new(
        "CAPTURE_BLOCKED",
        "Android protects a visible window or surface; no image is available",
    ))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn combined_snapshot_preserves_protection_and_rejects_incomplete_framing() {
        let dump = include_str!("../../../tests/fixtures/android12-window-symbolic.txt");
        let separator = "\nTESTRUN_LAYERS_fixture\n";
        let joined = |window: &str, layers: &[u8]| {
            [window.as_bytes(), separator.as_bytes(), layers].concat()
        };
        // Layer-service failure cannot erase a confirmed secure window.
        assert_eq!(
            snapshot(&joined(dump, &[0]), separator).unwrap_err().code,
            "CAPTURE_BLOCKED"
        );
        let clear = dump.replace(" SECURE", "");
        assert!(
            !snapshot(&joined(&clear, &[0]), separator)
                .unwrap()
                .admissible()
        );
        assert!(
            snapshot(&joined(&clear, &[10, 5, 8, 1, 18, 1, b'x']), separator)
                .unwrap()
                .admissible()
        );
        assert_eq!(
            snapshot(clear.as_bytes(), separator).unwrap_err().code,
            "PLATFORM_PROTOCOL"
        );
    }
    #[test]
    fn visible_secure_windows_block_while_unknown_policy_is_not_admitted() {
        // AOSP-format fixture; real-device ordering and protection acceptance
        // remain separate from these parser cases.
        let dump = include_str!("../../../tests/fixtures/android12-window-symbolic.txt");
        for dump in [dump.to_owned(), dump.replace("\n      fl=", " fl=")] {
            assert_eq!(parse(&dump).unwrap_err().code, "CAPTURE_BLOCKED");
        }
        assert!(!parse("").unwrap().admissible());
        let clear = dump.replace(" SECURE", "");
        assert!(parse(&clear).unwrap().admissible());
        assert!(
            parse(&clear.replace("\n      fl=", " fl="))
                .unwrap()
                .admissible()
        );
        let numeric = dump.replace(
            "KEEP_SCREEN_ON SECURE LAYOUT_IN_SCREEN HARDWARE_ACCELERATED",
            "#2000",
        );
        assert_eq!(parse(&numeric).unwrap_err().code, "CAPTURE_BLOCKED");
        assert!(
            !parse(&clear.replace("fl=KEEP_SCREEN_ON", "pfl=KEEP_SCREEN_ON"))
                .unwrap()
                .admissible()
        );
        assert!(
            !parse(&clear.replace("fl=KEEP_SCREEN_ON", "fl=OEM_UNKNOWN"))
                .unwrap()
                .admissible()
        );
    }
    #[test]
    fn composed_secure_children_block_and_malformed_metadata_is_unknown() {
        let clear = [10, 5, 8, 1, 18, 1, b'x'];
        assert!(composition(&clear).unwrap().is_some());
        let secure = [10, 9, 8, 1, 18, 1, b'x', 176, 1, 128, 1];
        assert_eq!(composition(&secure).unwrap_err().code, "CAPTURE_BLOCKED");
        for bytes in [&[][..], &[10, 127][..], &[0][..], &[10, 1, 0][..]] {
            assert!(composition(bytes).unwrap().is_none());
        }
    }
    #[test]
    fn protected_buffer_changes_block_without_a_layer_or_flag_change() {
        // Layer 1 named x, with unchanged default flags and field 36 toggled.
        let layer = |protected| [10, 8, 8, 1, 18, 1, b'x', 160, 2, protected];
        let clear = composition(&layer(0)).unwrap().unwrap();
        assert_eq!(composition(&layer(1)).unwrap_err().code, "CAPTURE_BLOCKED");
        assert_eq!(composition(&layer(0)).unwrap(), Some(clear.clone()));
        // An omitted proto3 boolean is false, like an explicit zero.
        assert_eq!(
            composition(&[10, 5, 8, 1, 18, 1, b'x']).unwrap(),
            Some(clear)
        );
        // A non-varint protection field cannot establish an admissible source.
        assert!(
            composition(&[10, 8, 8, 1, 18, 1, b'x', 162, 2, 0])
                .unwrap()
                .is_none()
        );
    }
}
