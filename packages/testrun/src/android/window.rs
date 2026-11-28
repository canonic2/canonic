//! Window-manager flag decoding shared by raw-capture refusals and stream admission.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Flags {
    Clear,
    Secure,
    Unknown,
}

/// Find the exact `fl=` field, including inside `mAttrs`, without confusing
/// `pfl=` or another field's SECURE text with window flags. Android 12's
/// LayoutParams.toString uses ViewDebug.flagsToString; OEMs also emit hex.
pub(super) fn flags(line: &str) -> Option<Flags> {
    let mut fields = line.split_whitespace();
    let first = fields.find_map(|field| field.strip_prefix("fl="))?;
    let mut secure = false;
    let mut unknown = false;
    for field in std::iter::once(first)
        .chain(fields)
        .take_while(|field| !field.contains('='))
    {
        let field = field.trim_end_matches('}');
        if field.is_empty() {
            continue;
        }
        if field == "SECURE" {
            secure = true;
        } else if !symbolic(field) {
            let hex = field
                .strip_prefix('#')
                .or_else(|| field.strip_prefix("0x"))
                .unwrap_or(field);
            match u32::from_str_radix(hex, 16) {
                Ok(bits) => secure |= bits & 0x2000 != 0,
                Err(_) => unknown = true,
            }
        }
    }
    Some(if secure {
        Flags::Secure
    } else if unknown {
        Flags::Unknown
    } else {
        Flags::Clear
    })
}

/// Android 12 LayoutParams.flags ExportedProperty names. Unknown OEM names
/// remain unknown unless another token positively establishes SECURE.
fn symbolic(value: &str) -> bool {
    matches!(
        value,
        "ALLOW_LOCK_WHILE_SCREEN_ON"
            | "DIM_BEHIND"
            | "BLUR_BEHIND"
            | "NOT_FOCUSABLE"
            | "NOT_TOUCHABLE"
            | "NOT_TOUCH_MODAL"
            | "TOUCHABLE_WHEN_WAKING"
            | "KEEP_SCREEN_ON"
            | "LAYOUT_IN_SCREEN"
            | "LAYOUT_NO_LIMITS"
            | "FULLSCREEN"
            | "FORCE_NOT_FULLSCREEN"
            | "DITHER"
            | "SCALED"
            | "IGNORE_CHEEK_PRESSES"
            | "LAYOUT_INSET_DECOR"
            | "ALT_FOCUSABLE_IM"
            | "WATCH_OUTSIDE_TOUCH"
            | "SHOW_WHEN_LOCKED"
            | "SHOW_WALLPAPER"
            | "TURN_SCREEN_ON"
            | "DISMISS_KEYGUARD"
            | "SPLIT_TOUCH"
            | "HARDWARE_ACCELERATED"
            | "LOCAL_FOCUS_MODE"
            | "TRANSLUCENT_STATUS"
            | "TRANSLUCENT_NAVIGATION"
            | "FLAG_SLIPPERY"
            | "FLAG_LAYOUT_ATTACHED_IN_DECOR"
            | "DRAWS_SYSTEM_BAR_BACKGROUNDS"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_flags_accept_symbolic_inline_and_numeric_forms() {
        for line in [
            "  fl=SECURE KEEP_SCREEN_ON",
            "mAttrs={(0,0)(fillxfill) ty=BASE_APPLICATION fl=KEEP_SCREEN_ON SECURE}",
            "fl=#2000",
            "fl=0x2000",
            "fl=81812100",
            "fl=#0 SECURE",
        ] {
            assert_eq!(flags(line), Some(Flags::Secure), "{line}");
        }
        for line in [
            "fl=KEEP_SCREEN_ON LAYOUT_IN_SCREEN",
            "fl=#0",
            "fl=0",
            "fl=}",
            "fl=",
        ] {
            assert_eq!(flags(line), Some(Flags::Clear), "{line}");
        }
        for line in ["pfl=SECURE", "title=SECURE", "mAttrs={ty=SECURE}"] {
            assert_eq!(flags(line), None, "{line}");
        }
        assert_eq!(flags("fl=KEEP_SCREEN_ON pfl=SECURE"), Some(Flags::Clear));
        assert_eq!(flags("fl=NOT_SECURE"), Some(Flags::Unknown));
        assert_eq!(flags("fl=#nothex"), Some(Flags::Unknown));
        assert_eq!(flags("fl=OEM_UNKNOWN SECURE"), Some(Flags::Secure));
    }
}
