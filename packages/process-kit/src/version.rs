//! Semantic release precedence, independent of protocol compatibility and build metadata.

use crate::{Error, Result};

/// A validated release version. Build metadata does not establish upgrade order.
#[derive(Debug, Clone)]
pub struct Version(semver::Version);

impl Version {
    /// Parse a complete semantic version such as `1.10.0` or `2.0.0-rc.1`.
    ///
    /// # Errors
    /// Returns `InvalidVersion` for malformed versions or labels over 128 bytes.
    pub fn parse(value: &str) -> Result<Self> {
        if value.len() > 128 {
            return Err(Error::InvalidVersion);
        }
        semver::Version::parse(value)
            .map(Self)
            .map_err(|_| Error::InvalidVersion)
    }

    /// Compare release precedence, ignoring `+build` metadata.
    #[must_use]
    pub fn is_newer_than(&self, running: &Self) -> bool {
        self.0.cmp_precedence(&running.0).is_gt()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_precedence_does_not_use_string_or_build_metadata_order() {
        for (running, candidate, newer) in [
            ("1.9.0", "1.10.0", true),
            ("1.10.0", "1.9.0", false),
            ("1.0.0-rc.2", "1.0.0-rc.10", true),
            ("1.0.0-rc.10", "1.0.0", true),
            ("1.0.0", "1.0.0-rc.10", false),
            ("1.0.0+acme.1", "1.0.0+acme.2", false),
            ("1.0.0", "1.0.0", false),
        ] {
            assert_eq!(
                Version::parse(candidate)
                    .unwrap()
                    .is_newer_than(&Version::parse(running).unwrap()),
                newer
            );
        }
    }

    #[test]
    fn unknown_and_unbounded_labels_are_not_ordered() {
        for label in [
            "acme-development".to_owned(),
            "v1.0.0".into(),
            "1.0".into(),
            "1.0.0+".to_owned() + &"x".repeat(128),
        ] {
            assert!(matches!(Version::parse(&label), Err(Error::InvalidVersion)));
        }
    }
}
