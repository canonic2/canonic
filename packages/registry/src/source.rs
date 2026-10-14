//! Registry source identity, signing trust and location validation.
//!
//! A source is either a canonical local directory or a signed HTTPS origin.
//! Its identity is derived from what it is, not from the alias an operator
//! chose, so renaming or removing a registration never retargets installed packs.

use crate::{digest, names};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// One enrolled registry source.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "Stored", into = "Stored")]
pub struct Source {
    location: String,
    identity: String,
    key: Option<String>,
    registry_id: Option<String>,
}

/// Serialized form; deserialization revalidates every field.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Stored {
    location: String,
    identity: String,
    key: Option<String>,
    #[serde(default)]
    registry_id: Option<String>,
}

/// A source whose location, key or identity is malformed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SourceError {
    /// A local location is not an absolute UTF-8 path.
    #[error("local registry location must be an absolute UTF-8 path")]
    LocalPath,
    /// An HTTPS location is unsupported.
    #[error("HTTPS registry requires a safe https:// URL ending in /")]
    Url,
    /// The signing key is not 32 bytes of hexadecimal.
    #[error("registry signing key must be 64 hexadecimal characters")]
    Key,
    /// The registry ID or identity is blank, oversized or contains NUL.
    #[error("registry identity is invalid")]
    Identity,
    /// Signed sources need both a key and registry ID; local sources neither.
    #[error("registry trust fields are inconsistent with its location")]
    Trust,
}

impl Source {
    /// A local directory source. `location` must already be canonical.
    ///
    /// # Errors
    /// Returns [`SourceError::LocalPath`] for relative or non-UTF-8 paths.
    pub fn local(location: &Path) -> Result<Self, SourceError> {
        let location = location
            .to_str()
            .filter(|_| location.is_absolute())
            .ok_or(SourceError::LocalPath)?;
        Ok(Self {
            identity: digest::sha256_hex(location.as_bytes()),
            location: location.to_owned(),
            key: None,
            registry_id: None,
        })
    }

    /// A signed HTTPS source, identified by its location, key and registry ID.
    ///
    /// The location and key are stored in canonical form (see
    /// [`canonical_https_location`]; the key in lowercase), so every spelling
    /// of the same source has the same identity.
    ///
    /// # Errors
    /// Returns [`SourceError`] for an unsupported URL, key or registry ID.
    pub fn signed(location: &str, key: &str, registry_id: &str) -> Result<Self, SourceError> {
        let (location, key) = canonical_trust(location, key)?;
        Self::pinned(
            &location,
            &signed_identity(&location, &key, registry_id),
            &key,
            registry_id,
        )
    }

    /// A signed source with a fixed identity, for a product's built-in registry.
    ///
    /// The location and key are stored in canonical form, as for [`Source::signed`].
    ///
    /// # Errors
    /// Returns [`SourceError`] for an unsupported URL, key, identity or registry ID.
    pub fn pinned(
        location: &str,
        identity: &str,
        key: &str,
        registry_id: &str,
    ) -> Result<Self, SourceError> {
        let (location, key) = canonical_trust(location, key)?;
        let source = Self {
            location,
            identity: identity.to_owned(),
            key: Some(key),
            registry_id: Some(registry_id.to_owned()),
        };
        source.validate()?;
        Ok(source)
    }

    fn validate(&self) -> Result<(), SourceError> {
        if !names::is_source_identity(&self.identity) {
            return Err(SourceError::Identity);
        }
        match (&self.key, &self.registry_id) {
            (Some(key), Some(registry_id)) => {
                // Stored values must already be canonical, as constructors make them.
                if canonical_https_location(&self.location).as_deref() != Some(&self.location) {
                    return Err(SourceError::Url);
                }
                if digest::normalize_hex(key, 32).as_deref() != Some(key) {
                    return Err(SourceError::Key);
                }
                if !names::is_source_identity(registry_id) {
                    return Err(SourceError::Identity);
                }
                Ok(())
            }
            (None, None) => {
                if Path::new(&self.location).is_absolute() {
                    Ok(())
                } else {
                    Err(SourceError::LocalPath)
                }
            }
            _ => Err(SourceError::Trust),
        }
    }

    /// Canonical directory path or HTTPS base URL.
    #[must_use]
    pub fn location(&self) -> &str {
        &self.location
    }

    /// Stable identity used by installed packs, checkpoints and dependencies.
    #[must_use]
    pub fn identity(&self) -> &str {
        &self.identity
    }

    /// Hexadecimal Ed25519 public key for signed sources.
    #[must_use]
    pub fn key(&self) -> Option<&str> {
        self.key.as_deref()
    }

    /// Registry ID every signed catalog from this source must carry.
    #[must_use]
    pub fn registry_id(&self) -> Option<&str> {
        self.registry_id.as_deref()
    }

    /// Whether catalogs must be signed, current and sequenced.
    #[must_use]
    pub fn is_signed(&self) -> bool {
        self.key.is_some()
    }

    /// Whether the identity is the one derived from the location (local) or
    /// from the location, key and registry ID (signed). Only pinned built-in
    /// sources carry an identity chosen by the product.
    #[must_use]
    pub fn has_derived_identity(&self) -> bool {
        let derived = match (&self.key, &self.registry_id) {
            (Some(key), Some(registry_id)) => signed_identity(&self.location, key, registry_id),
            _ => digest::sha256_hex(self.location.as_bytes()),
        };
        self.identity == derived
    }
}

impl TryFrom<Stored> for Source {
    type Error = SourceError;

    fn try_from(stored: Stored) -> Result<Self, Self::Error> {
        let source = Self {
            location: stored.location,
            identity: stored.identity,
            key: stored.key,
            registry_id: stored.registry_id,
        };
        source.validate()?;
        Ok(source)
    }
}

impl From<Source> for Stored {
    fn from(source: Source) -> Self {
        Self {
            location: source.location,
            identity: source.identity,
            key: source.key,
            registry_id: source.registry_id,
        }
    }
}

/// The identity of a signed source: SHA-256 of `[location, key, registry_id]` as JSON.
fn signed_identity(location: &str, key: &str, registry_id: &str) -> String {
    digest::sha256_hex(
        serde_json::json!([location, key, registry_id])
            .to_string()
            .as_bytes(),
    )
}

/// The canonical location and lowercase key of a signed source.
fn canonical_trust(location: &str, key: &str) -> Result<(String, String), SourceError> {
    Ok((
        canonical_https_location(location).ok_or(SourceError::Url)?,
        digest::normalize_hex(key, 32).ok_or(SourceError::Key)?,
    ))
}

/// Whether `value` is a supported HTTPS registry base URL.
///
/// It must use `https://`, name a host without user information, end in `/`,
/// and carry no query, fragment, percent-encoding or dot segments.
#[must_use]
pub fn is_https_location(value: &str) -> bool {
    canonical_https_location(value).is_some()
}

/// The canonical form of a supported HTTPS registry base URL
/// ([`is_https_location`]): the host in lowercase, and the port omitted when
/// it is the default 443. The path keeps its case.
#[must_use]
pub fn canonical_https_location(value: &str) -> Option<String> {
    if value.len() > names::MAX_TEXT
        || !value.starts_with("https://")
        || !value.ends_with('/')
        || value.contains('%')
        || value.contains('#')
    {
        return None;
    }
    let uri = value.parse::<ureq::http::Uri>().ok()?;
    let authority = uri.authority()?;
    let host = authority.host();
    if uri.scheme_str() != Some("https")
        || authority.as_str().contains('@')
        || host.is_empty()
        || uri.query().is_some()
        || !uri.path().strip_prefix('/').is_some_and(|path| {
            path.is_empty() || path.strip_suffix('/').is_some_and(names::is_safe_relative)
        })
    {
        return None;
    }
    let port = match &authority.as_str()[host.len()..] {
        "" => None,
        suffix => Some(
            suffix
                .strip_prefix(':')?
                .parse::<u16>()
                .ok()
                .filter(|&port| port != 0)?,
        ),
    };
    let port = port
        .filter(|&port| port != 443)
        .map_or_else(String::new, |port| format!(":{port}"));
    Some(format!(
        "https://{}{port}{}",
        host.to_ascii_lowercase(),
        uri.path()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::EXAMPLE_KEY as KEY;

    #[test]
    fn unsupported_urls_refuse() {
        assert!(is_https_location("https://example.com/registry/"));
        assert!(is_https_location("https://[::1]:8443/registry/"));
        for url in [
            "http://example.com/",
            "https://user@example.com/",
            "https://example.com/r",
            "https://example.com/a/../",
            "https://example.com/%2e%2e/",
            "https://example.com/?secret=x",
            "https://example.com/#x",
            "https://example.com//a/",
            "https://example.com/a\\b/",
            "https:///a/",
            "https://example.com:65536/",
            "https://example.com:/",
            "https://example.com:0/",
            "https://example.com:invalid/",
        ] {
            assert!(!is_https_location(url), "{url}");
        }
    }

    #[test]
    fn identities_bind_location_key_and_registry_id() {
        let signed = Source::signed("https://example.com/r/", KEY, "acme").unwrap();
        assert!(signed.is_signed());
        assert_ne!(
            signed.identity(),
            Source::signed("https://example.com/s/", KEY, "acme")
                .unwrap()
                .identity()
        );
        assert_ne!(
            signed.identity(),
            Source::signed("https://example.com/r/", KEY, "other")
                .unwrap()
                .identity()
        );
        let local = Source::local(Path::new("/srv/acme-registry")).unwrap();
        assert!(!local.is_signed());
        assert_eq!(local.identity(), digest::sha256_hex(b"/srv/acme-registry"));
        assert_eq!(
            Source::local(Path::new("relative")),
            Err(SourceError::LocalPath)
        );
        assert_eq!(
            Source::signed("https://example.com/r/", "00", "acme"),
            Err(SourceError::Key)
        );
        assert_eq!(
            Source::pinned("https://example.com/r/", "", KEY, "acme"),
            Err(SourceError::Identity)
        );
    }

    #[test]
    fn every_spelling_of_a_signed_source_has_one_identity() {
        let canonical = Source::signed("https://example.com/Registry/", KEY, "acme").unwrap();
        for (location, key) in [
            ("https://EXAMPLE.com/Registry/", KEY.to_owned()),
            ("https://example.com:443/Registry/", KEY.to_owned()),
            (
                "https://Example.COM:0443/Registry/",
                KEY.to_ascii_uppercase(),
            ),
        ] {
            let spelled = Source::signed(location, &key, "acme").unwrap();
            assert_eq!(spelled, canonical, "{location}");
            assert!(spelled.has_derived_identity());
        }
        let pinned = Source::pinned(
            "https://EXAMPLE.com/r/",
            "main",
            &KEY.to_ascii_uppercase(),
            "acme",
        )
        .unwrap();
        assert_eq!(pinned.location(), "https://example.com/r/");
        assert_eq!(pinned.key(), Some(KEY));
        // The path is case-sensitive, and other ports are other origins.
        for other in [
            "https://example.com/registry/",
            "https://example.com:8443/Registry/",
        ] {
            assert_ne!(
                Source::signed(other, KEY, "acme").unwrap().identity(),
                canonical.identity()
            );
        }
        assert_eq!(
            canonical_https_location("https://[::1]:8443/r/").as_deref(),
            Some("https://[::1]:8443/r/")
        );
        // Keys must be exactly 64 hexadecimal characters, without whitespace.
        assert_eq!(
            Source::signed("https://example.com/r/", &format!(" {KEY}"), "acme"),
            Err(SourceError::Key)
        );
    }

    #[test]
    fn stored_sources_are_revalidated() {
        let source = Source::pinned("https://example.com/r/", "main", KEY, "acme").unwrap();
        let value = serde_json::to_value(&source).unwrap();
        assert_eq!(
            serde_json::from_value::<Source>(value.clone()).unwrap(),
            source
        );
        for (field, replacement) in [
            ("location", serde_json::json!("http://example.com/")),
            ("key", serde_json::json!(null)),
            ("identity", serde_json::json!(" ")),
            // Stored values must already be canonical.
            ("location", serde_json::json!("https://EXAMPLE.com/r/")),
            ("key", serde_json::json!(KEY.to_ascii_uppercase())),
        ] {
            let mut altered = value.clone();
            altered[field] = replacement;
            assert!(
                serde_json::from_value::<Source>(altered).is_err(),
                "{field}"
            );
        }
        let mut extra = value;
        extra["installer"] = serde_json::json!("x");
        assert!(serde_json::from_value::<Source>(extra).is_err());
    }
}
