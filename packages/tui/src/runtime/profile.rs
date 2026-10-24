//! Resolve host environment hints without querying or acquiring a terminal.
use crate::theme::{ColorMode, Presentation, Symbols};

/// Resolve the current environment; callers still decide whether output is a TTY.
#[must_use]
pub fn detect() -> Presentation {
    let locale = ["LC_ALL", "LC_CTYPE", "LANG"]
        .iter()
        .find_map(|key| std::env::var(key).ok().filter(|value| !value.is_empty()));
    resolve(
        &std::env::var("TERM").unwrap_or_default(),
        &std::env::var("COLORTERM").unwrap_or_default(),
        locale.as_deref(),
        std::env::var_os("NO_COLOR").is_some(),
    )
}

fn resolve(term: &str, colorterm: &str, locale: Option<&str>, no_color: bool) -> Presentation {
    let colors = if no_color || term.is_empty() || term == "dumb" {
        ColorMode::NoColor
    } else if matches!(
        colorterm.to_ascii_lowercase().as_str(),
        "truecolor" | "24bit"
    ) {
        ColorMode::TrueColor
    } else if term.contains("256color") {
        ColorMode::Ansi256
    } else {
        ColorMode::Ansi16
    };
    let symbols = if locale.is_some_and(|locale| {
        let locale = locale.to_ascii_lowercase();
        !locale.contains("utf-8") && !locale.contains("utf8")
    }) {
        Symbols::Ascii
    } else {
        Symbols::Unicode
    };
    Presentation {
        colors,
        symbols,
        ..Presentation::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn profiles_follow_capabilities_and_explicit_color_refusal() {
        assert_eq!(
            resolve("xterm", "truecolor", Some("en_US.UTF-8"), false).colors,
            ColorMode::TrueColor
        );
        assert_eq!(
            resolve("xterm-256color", "", None, false).colors,
            ColorMode::Ansi256
        );
        assert_eq!(resolve("xterm", "", None, false).colors, ColorMode::Ansi16);
        for term in ["", "dumb", "xterm-256color"] {
            assert_eq!(
                resolve(term, "truecolor", None, true).colors,
                ColorMode::NoColor
            );
        }
        assert_eq!(
            resolve("xterm", "", Some("C"), false).symbols,
            Symbols::Ascii
        );
        assert_eq!(
            resolve("xterm", "", Some("C.UTF-8"), false).symbols,
            Symbols::Unicode
        );
    }
}
