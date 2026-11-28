//! Canonical physical-key identities derived from the pinned driver layout.
//!
//! The driver still produces native key events. This map only tracks aliases
//! for balanced release; it does not replace typing with DOM event dispatch.

use serde::Deserialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::OnceLock,
};

#[derive(Deserialize)]
struct Source {
    version: String,
    layout: BTreeMap<String, Definition>,
    aliases: BTreeMap<String, Vec<String>>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Definition {
    key: String,
    shift_key: Option<String>,
    #[serde(default)]
    location: u8,
}

fn layout() -> &'static BTreeMap<String, String> {
    static LAYOUT: OnceLock<BTreeMap<String, String>> = OnceLock::new();
    LAYOUT.get_or_init(|| {
        let source: Source = serde_json::from_str(include_str!("keyboard-layout.json"))
            .expect("Pinned keyboard data");
        assert_eq!(
            source.version,
            super::DRIVER_VERSION,
            "Pinned keyboard version"
        );
        let mut keys = BTreeMap::new();
        for (code, definition) in &source.layout {
            keys.insert(code.clone(), code.clone());
            if let Some(aliases) = source.aliases.get(code) {
                for alias in aliases {
                    keys.insert(alias.clone(), code.clone());
                }
            }
            if definition.location == 0 {
                if definition.key.chars().count() == 1 {
                    keys.insert(definition.key.clone(), code.clone());
                }
                if let Some(shifted) = &definition.shift_key {
                    keys.insert(shifted.clone(), code.clone());
                }
            }
        }
        keys
    })
}

pub(crate) fn code(key: &str) -> Result<&'static str, &'static str> {
    if key.is_empty() || key.len() > 128 {
        return Err("INVALID_BROWSER_KEY");
    }
    let key = if key == "ControlOrMeta" {
        if cfg!(target_os = "macos") {
            "Meta"
        } else {
            "Control"
        }
    } else {
        key
    };
    layout()
        .get(key)
        .map(String::as_str)
        .ok_or("INVALID_BROWSER_KEY")
}

/// Match the pinned driver's plus-token grammar, including literal '+' keys.
/// Resolve every token before dispatch so a bad final key cannot press modifiers.
pub(crate) fn press_keys(key: &str) -> Result<Vec<String>, &'static str> {
    if key.is_empty() || key.len() > 128 {
        return Err("INVALID_BROWSER_KEY");
    }
    let mut tokens = Vec::new();
    let mut current = String::new();
    for character in key.chars() {
        if character == '+' && !current.is_empty() {
            tokens.push(std::mem::take(&mut current));
        } else {
            current.push(character);
        }
    }
    tokens.push(current);
    if tokens.len() > 16 {
        return Err("INPUT_BOUNDS");
    }
    for token in &tokens {
        code(token)?;
    }
    Ok(tokens)
}
pub(crate) fn press_codes(key: &str) -> Result<BTreeSet<String>, &'static str> {
    press_keys(key)?
        .iter()
        .map(|token| code(token).map(str::to_owned))
        .collect()
}

/// Unsupported layout characters use the driver's text insertion path and
/// therefore reserve no physical key for cleanup.
pub(crate) fn text_codes(text: &str) -> BTreeSet<String> {
    text.chars()
        .filter_map(|character| code(&character.to_string()).ok().map(str::to_owned))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn physical_key_aliases_share_release_identity_and_chords_validate_before_dispatch() {
        assert_eq!(code("a"), code("A"));
        assert_eq!(code("a"), code("KeyA"));
        assert_eq!(code("Shift"), code("ShiftLeft"));
        assert_ne!(code("ShiftLeft"), code("ShiftRight"));
        assert_eq!(code("\n"), code("Enter"));
        assert_eq!(
            press_codes("Control++").unwrap(),
            BTreeSet::from(["ControlLeft".into(), "Equal".into()])
        );
        assert_eq!(press_codes("+").unwrap(), BTreeSet::from(["Equal".into()]));
        assert_eq!(
            text_codes("aAé🙂\n"),
            BTreeSet::from(["KeyA".into(), "Enter".into()])
        );
        assert!(press_codes("Shift+NotAKey").is_err());
        assert!(press_codes("Shift+").is_err());
        assert!(code("é").is_err());
    }
}
