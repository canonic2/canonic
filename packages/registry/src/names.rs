//! Text rules: identifiers for publishers, pack names and aliases, source
//! identity bounds, and safe relative artifact paths.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::{fmt, str::FromStr};

/// Maximum length of an identifier segment or alias.
pub const MAX_IDENTIFIER: usize = 64;
/// Maximum length of a source identity or other bounded text field.
pub const MAX_TEXT: usize = 4096;

/// Whether `value` is 1–64 lowercase ASCII letters, digits or hyphens.
///
/// Publishers, pack names and registry aliases use this form.
#[must_use]
pub fn is_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_IDENTIFIER
        && value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// Whether `value` is a non-blank source identity of at most 4096 bytes without NUL.
#[must_use]
pub fn is_source_identity(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= MAX_TEXT && !value.contains('\0')
}

/// Whether `path` is a safe, portable relative artifact path inside a source.
///
/// Segments use ASCII letters, digits and `-._~`. Empty, `.` and `..`
/// segments, segments ending in `.`, and Windows device names (`CON`, `PRN`,
/// `AUX`, `NUL`, `COM1`–`COM9`, `LPT1`–`LPT9`, with any extension) are refused,
/// so retained artifacts can be written on every platform.
#[must_use]
pub fn is_artifact_path(path: &str) -> bool {
    path.len() <= MAX_TEXT
        && is_safe_relative(path)
        && path.split('/').all(|part| {
            let stem = part
                .split('.')
                .next()
                .unwrap_or_default()
                .to_ascii_uppercase();
            !part.ends_with('.')
                && !matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
                && !(stem.len() == 4
                    && (stem.starts_with("COM") || stem.starts_with("LPT"))
                    && matches!(stem.as_bytes()[3], b'1'..=b'9'))
        })
}

/// Whether `path` is non-empty, uses only ASCII letters, digits and `-._~/`,
/// and has no empty, `.` or `..` segment. URL paths and artifact paths share it.
pub(crate) fn is_safe_relative(path: &str) -> bool {
    !path.is_empty()
        && path
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-._~/".contains(&b))
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

/// A fully qualified `publisher/name` pack identity.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PackId {
    publisher: String,
    name: String,
}

/// A malformed `publisher/name`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("pack identity must be publisher/name using lowercase letters, digits and hyphens")]
pub struct InvalidPackId;

impl PackId {
    /// Builds an identity from validated segments.
    ///
    /// # Errors
    /// Returns [`InvalidPackId`] when either segment is not an identifier.
    pub fn new(publisher: &str, name: &str) -> Result<Self, InvalidPackId> {
        if is_identifier(publisher) && is_identifier(name) {
            Ok(Self {
                publisher: publisher.to_owned(),
                name: name.to_owned(),
            })
        } else {
            Err(InvalidPackId)
        }
    }

    /// The publisher segment.
    #[must_use]
    pub fn publisher(&self) -> &str {
        &self.publisher
    }

    /// The name segment.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }
}

impl FromStr for PackId {
    type Err = InvalidPackId;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let (publisher, name) = value.split_once('/').ok_or(InvalidPackId)?;
        Self::new(publisher, name)
    }
}

impl fmt::Display for PackId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}/{}", self.publisher, self.name)
    }
}

impl Serialize for PackId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for PackId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifiers_are_bounded_lowercase_segments() {
        assert!(is_identifier("acme-2"));
        assert!(is_identifier(&"a".repeat(64)));
        for value in ["", "Acme", "acme_x", "acme/x", "é", &"a".repeat(65)] {
            assert!(!is_identifier(value), "{value}");
        }
    }

    #[test]
    fn artifact_paths_stay_relative_contained_and_portable() {
        assert!(is_artifact_path("packs/acme/git-1.0.0.yml"));
        assert!(is_artifact_path("packs/console/com0.yml"));
        for path in [
            "packs/CON",
            "packs/nul.yml",
            "com1/pack.yml",
            "LPT9.txt",
            "pack.",
            "a b",
            "",
            "../pack.yml",
            "/pack.yml",
            "a//b",
            "a/./b",
            "%2fpack",
            "a?x",
            "a#x",
            "a\\b",
            "https://other.example/pack",
        ] {
            assert!(!is_artifact_path(path), "{path}");
        }
    }

    #[test]
    fn pack_ids_need_exactly_publisher_and_name() {
        let id: PackId = "acme/git".parse().unwrap();
        assert_eq!((id.publisher(), id.name()), ("acme", "git"));
        assert_eq!(id.to_string(), "acme/git");
        for value in ["git", "acme/git/x", "/git", "acme/", "Acme/git"] {
            assert!(value.parse::<PackId>().is_err(), "{value}");
        }
        assert_eq!(serde_json::from_str::<PackId>("\"acme/git\"").unwrap(), id);
        assert!(serde_json::from_str::<PackId>("\"acme\"").is_err());
    }

    #[test]
    fn source_identities_reject_blank_oversized_and_nul_values() {
        assert!(is_source_identity("main"));
        for value in ["", "  ", "a\0b", &"a".repeat(4097)] {
            assert!(!is_source_identity(value));
        }
    }
}
