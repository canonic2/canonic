//! Validated USB HID key vocabulary and explicit US ASCII text mapping.
use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
#[derive(Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Request {
    Key {
        name: String,
        #[serde(default)]
        modifiers: Vec<String>,
    },
    Text {
        text: String,
        #[serde(default)]
        replace: bool,
    },
}
#[repr(C)]
#[derive(Clone, Copy, Serialize, Deserialize)]
pub struct Stroke {
    pub usage: u32,
    pub modifiers: u32,
}
impl Request {
    pub(crate) fn strokes(&self) -> Result<Vec<Stroke>> {
        match self {
            Self::Key { name, modifiers } => Ok(vec![key(name, modifiers)?]),
            Self::Text {
                text: value,
                replace,
            } => {
                let mut strokes = text(value)?;
                if *replace {
                    strokes.insert(0, key("a", &["meta".into()])?);
                }
                Ok(strokes)
            }
        }
    }
}
pub fn key(name: &str, modifiers: &[String]) -> Result<Stroke> {
    let usage = match name {
        "enter" => 40,
        "escape" => 41,
        "backspace" => 42,
        "tab" => 43,
        "space" => 44,
        "delete" => 76,
        "home" => 74,
        "end" => 77,
        "page_up" => 75,
        "page_down" => 78,
        "arrow_right" => 79,
        "arrow_left" => 80,
        "arrow_down" => 81,
        "arrow_up" => 82,
        _ => {
            let bytes = name.as_bytes();
            if bytes.len() == 1 && bytes[0].is_ascii_alphabetic() {
                u32::from(bytes[0].to_ascii_lowercase() - b'a') + 4
            } else {
                return Err(Error::new(
                    "CAPABILITY_UNAVAILABLE",
                    "Key is not in the supported HID catalog",
                ));
            }
        }
    };
    if modifiers.len() > 4 {
        return Err(Error::invalid("A key chord has at most four modifiers"));
    }
    let mut bits = 0;
    for modifier in modifiers {
        let bit = match modifier.as_str() {
            "shift" => 1,
            "control" => 2,
            "alt" => 4,
            "meta" => 8,
            _ => return Err(Error::invalid("Unknown key modifier")),
        };
        if bits & bit != 0 {
            return Err(Error::invalid("Duplicate key modifier"));
        }
        bits |= bit;
    }
    Ok(Stroke {
        usage,
        modifiers: bits,
    })
}
pub fn text(text: &str) -> Result<Vec<Stroke>> {
    if text.len() > 65536 {
        return Err(Error::invalid("text exceeds 64 KiB"));
    }
    let plain = b"1234567890-=\x5b\x5d\\;\x27`,./";
    let shifted = b"!@#$%^&*()_+{}|:\x22~<>?";
    let usages = [
        30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 45, 46, 47, 48, 49, 51, 52, 53, 54, 55, 56,
    ];
    text.bytes()
        .map(|character| {
            let (usage, modifiers) = match character {
                b'a'..=b'z' => (u32::from(character - b'a') + 4, 0),
                b'A'..=b'Z' => (u32::from(character - b'A') + 4, 1),
                b' ' => (44, 0),
                b'\n' => (40, 0),
                b'\t' => (43, 0),
                _ => {
                    if let Some(index) = plain.iter().position(|c| *c == character) {
                        (usages[index], 0)
                    } else if let Some(index) = shifted.iter().position(|c| *c == character) {
                        (usages[index], 1)
                    } else {
                        return Err(Error::new(
                            "CAPABILITY_UNAVAILABLE",
                            "Native text input currently supports the US ASCII HID layout",
                        ));
                    }
                }
            };
            Ok(Stroke { usage, modifiers })
        })
        .collect()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn text_mapping_is_explicit_and_rejects_unsupported_characters() {
        let typed = text("Acme!\n").unwrap();
        assert_eq!(typed.len(), 6);
        assert_eq!(typed[0].usage, 4);
        assert_eq!(typed[0].modifiers, 1);
        assert_eq!(typed[4].usage, 30);
        assert_eq!(typed[4].modifiers, 1);
        assert!(text("é").is_err());
        assert!(key("a", &["meta".into(), "meta".into()]).is_err());
    }
}
