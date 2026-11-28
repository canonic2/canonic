//! Explicit-origin access to the host system's text clipboard through the owned page.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    Read {},
    Write { text: String },
}
pub(crate) fn validate(origin: &str, action: &Action) -> Result<(), &'static str> {
    super::settings::validate_origin(origin)?;
    if origin.ends_with('/') {
        return Err("INVALID_PERMISSION_ORIGIN");
    }
    if matches!(action,Action::Write{text} if text.len()>16384) {
        return Err("CLIPBOARD_BOUNDS");
    }
    Ok(())
}
pub(crate) const SCRIPT: &str = include_str!("reading/clipboard.js");
pub(crate) fn result(value: serde_json::Value) -> Result<serde_json::Value, super::session::Error> {
    let Some(code) = value["error"].as_str() else {
        return Ok(value);
    };
    Err(match code {
        "CLIPBOARD_ORIGIN_MISMATCH" => "CLIPBOARD_ORIGIN_MISMATCH",
        "CLIPBOARD_UNAVAILABLE" => "CLIPBOARD_UNAVAILABLE",
        "CLIPBOARD_BOUNDS" => "CLIPBOARD_BOUNDS",
        _ => "CLIPBOARD_PERMISSION_REQUIRED",
    }
    .into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn clipboard_requires_an_exact_origin_and_bounded_text() {
        assert!(validate("https://example.com", &Action::Read {}).is_ok());
        for origin in [
            "https://example.com/path",
            "https://user@example.com",
            "file:///tmp/test",
            "https://example.com/",
        ] {
            assert!(validate(origin, &Action::Read {}).is_err(), "{origin}");
        }
        assert_eq!(
            validate(
                "https://example.com",
                &Action::Write {
                    text: "x".repeat(16385)
                }
            ),
            Err("CLIPBOARD_BOUNDS")
        );
    }
}
