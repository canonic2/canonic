//! Catalog schema, exact-byte authentication and release lookups.
//!
//! A catalog is `catalog.json`, version 1 or 2. Signed sources also serve a
//! detached hexadecimal Ed25519 `catalog.sig` over the exact catalog bytes;
//! the signature is checked before the bytes are parsed.

use crate::{
    digest,
    names::{self, PackId},
    source::Source,
    timestamp,
};
use semver::Version;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    time::SystemTime,
};

/// Maximum releases one catalog may list.
pub const MAX_ENTRIES: usize = 64;
/// Maximum command basenames advertised by one pack.
pub const MAX_COMMANDS: usize = 16;
/// Maximum size of a catalog document, enforced before it is verified or parsed.
pub const MAX_CATALOG_BYTES: u64 = 1_048_576;

/// One immutable release listed by a catalog.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    /// Fully qualified publisher/name.
    pub id: PackId,
    /// Exact release version.
    pub version: Version,
    /// Safe relative artifact path inside the source.
    pub path: String,
    /// Lowercase SHA-256 of the artifact bytes.
    pub sha256: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    schema_version: u8,
    packs: Vec<Entry>,
    #[serde(default)]
    registry_id: Option<String>,
    #[serde(default)]
    sequence: Option<u64>,
    #[serde(default)]
    expires_at: Option<String>,
    #[serde(default, deserialize_with = "present_commands")]
    commands: Option<BTreeMap<PackId, Vec<String>>>,
}

/// A parsed catalog whose bytes were authenticated against its source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Catalog {
    source: String,
    digest: String,
    entries: Vec<Entry>,
    registry_id: Option<String>,
    sequence: Option<u64>,
    expires_at: Option<String>,
    commands: BTreeMap<PackId, Vec<String>>,
}

/// Why catalog bytes were refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CatalogError {
    /// A signed source served no signature.
    #[error("registry signature missing")]
    SignatureMissing,
    /// The signature does not verify against the enrolled key.
    #[error("registry catalog signature invalid")]
    SignatureInvalid,
    /// The bytes are not a supported catalog document.
    #[error("invalid registry catalog")]
    Malformed,
    /// The catalog is larger than [`MAX_CATALOG_BYTES`] or lists more than
    /// [`MAX_ENTRIES`] releases.
    #[error("registry catalog exceeds limits")]
    Limit,
    /// A signed catalog names another registry.
    #[error("registry identity does not match its enrollment")]
    Identity,
    /// A signed catalog has no sequence number.
    #[error("signed registry catalog has no sequence")]
    SequenceMissing,
    /// The expiry is missing or not a whole-second UTC timestamp.
    #[error("registry catalog expiry is missing or invalid")]
    ExpiryInvalid,
    /// The signed catalog has expired.
    #[error("registry catalog has expired")]
    Expired,
    /// The same publisher/name and version appear twice.
    #[error("registry catalog repeats a release")]
    DuplicateRelease,
    /// A release has an unsafe path or malformed digest.
    #[error("registry release has an invalid path or digest")]
    InvalidEntry,
}

/// Lookup failure for a pack name within one catalog.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LookupError {
    /// No listed pack matches.
    #[error("pack not found in registry")]
    Missing,
    /// Several publishers list the name; the alternatives are fully qualified.
    #[error("pack name is ambiguous; use one of {0:?}")]
    Ambiguous(Vec<PackId>),
}

impl Catalog {
    /// Authenticates and parses catalog bytes for `source` at time `now`.
    ///
    /// Signed sources require a valid signature, their enrolled registry ID,
    /// a sequence and an unexpired `expires_at`. Local sources need none of
    /// them; a present expiry must still be well formed.
    ///
    /// # Errors
    /// Returns [`CatalogError`] for any authentication, schema or limit failure.
    pub fn verify(
        source: &Source,
        bytes: &[u8],
        signature: Option<&[u8]>,
        now: SystemTime,
    ) -> Result<Self, CatalogError> {
        // Bound the work before hashing or parsing, whoever supplied the bytes.
        if bytes.len() as u64 > MAX_CATALOG_BYTES {
            return Err(CatalogError::Limit);
        }
        if let Some(key) = source.key() {
            verify_signature(key, bytes, signature.ok_or(CatalogError::SignatureMissing)?)?;
        }
        let document: Document =
            serde_json::from_slice(bytes).map_err(|_| CatalogError::Malformed)?;
        if !matches!(document.schema_version, 1 | 2)
            || (document.schema_version == 1 && document.commands.is_some())
        {
            return Err(CatalogError::Malformed);
        }
        if document.packs.len() > MAX_ENTRIES {
            return Err(CatalogError::Limit);
        }
        if let Some(expiry) = &document.expires_at
            && !timestamp::is_utc_seconds(expiry)
        {
            return Err(CatalogError::ExpiryInvalid);
        }
        if source.is_signed() {
            if document.registry_id.as_deref() != source.registry_id() {
                return Err(CatalogError::Identity);
            }
            if document.sequence.is_none() {
                return Err(CatalogError::SequenceMissing);
            }
            let expiry = document
                .expires_at
                .as_deref()
                .ok_or(CatalogError::ExpiryInvalid)?;
            if timestamp::is_expired(expiry, now).ok_or(CatalogError::ExpiryInvalid)? {
                return Err(CatalogError::Expired);
            }
        }
        let mut seen = BTreeSet::new();
        let mut entries = document.packs;
        for entry in &mut entries {
            if !seen.insert((entry.id.clone(), entry.version.clone())) {
                return Err(CatalogError::DuplicateRelease);
            }
            if !names::is_artifact_path(&entry.path) {
                return Err(CatalogError::InvalidEntry);
            }
            entry.sha256 =
                digest::normalize_sha256(&entry.sha256).ok_or(CatalogError::InvalidEntry)?;
        }
        let commands = document.commands.unwrap_or_default();
        if commands.len() > MAX_ENTRIES {
            return Err(CatalogError::Limit);
        }
        for (id, names) in &commands {
            if names.len() > MAX_COMMANDS {
                return Err(CatalogError::Limit);
            }
            if names.is_empty()
                || !entries
                    .iter()
                    .any(|entry| entry.id == *id && entry.version.pre.is_empty())
                || names.iter().collect::<BTreeSet<_>>().len() != names.len()
                || names.iter().any(|name| !is_command(name))
            {
                return Err(CatalogError::InvalidEntry);
            }
        }
        Ok(Self {
            source: source.identity().to_owned(),
            digest: digest::sha256_hex(bytes),
            entries,
            registry_id: document.registry_id,
            sequence: document.sequence,
            expires_at: document.expires_at,
            commands,
        })
    }

    /// Advisory command basenames by pack ID from this authenticated catalog.
    /// These associations carry no permission authority.
    #[must_use]
    pub fn commands(&self) -> &BTreeMap<PackId, Vec<String>> {
        &self.commands
    }

    /// Identity of the source this catalog was authenticated against.
    #[must_use]
    pub fn source_identity(&self) -> &str {
        &self.source
    }

    /// SHA-256 of the exact authenticated catalog bytes.
    #[must_use]
    pub fn digest(&self) -> &str {
        &self.digest
    }

    /// Listed releases in catalog order.
    #[must_use]
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// The registry ID, required for signed sources.
    #[must_use]
    pub fn registry_id(&self) -> Option<&str> {
        self.registry_id.as_deref()
    }

    /// Monotonic publication sequence, required for signed sources.
    #[must_use]
    pub fn sequence(&self) -> Option<u64> {
        self.sequence
    }

    /// Whole-second UTC expiry, required for signed sources.
    #[must_use]
    pub fn expires_at(&self) -> Option<&str> {
        self.expires_at.as_deref()
    }

    /// Releases of one pack, highest version first.
    #[must_use]
    pub fn releases(&self, id: &PackId) -> Vec<&Entry> {
        let mut releases: Vec<_> = self.entries.iter().filter(|e| e.id == *id).collect();
        releases.sort_by(|a, b| b.version.cmp(&a.version));
        releases
    }

    /// The exact release of `id` at `version`.
    #[must_use]
    pub fn release(&self, id: &PackId, version: &Version) -> Option<&Entry> {
        self.entries
            .iter()
            .find(|e| e.id == *id && e.version == *version)
    }

    /// The release an operator selection means: the exact `version` when
    /// given, otherwise the highest stable (non-prerelease) version.
    #[must_use]
    pub fn select(&self, id: &PackId, version: Option<&Version>) -> Option<&Entry> {
        match version {
            Some(version) => self.release(id, version),
            None => self
                .entries
                .iter()
                .filter(|e| e.id == *id && e.version.pre.is_empty())
                .max_by(|a, b| a.version.cmp(&b.version)),
        }
    }

    /// Releases whose `publisher/name` contains `query`, in catalog order.
    #[must_use]
    pub fn search(&self, query: &str) -> Vec<&Entry> {
        self.entries
            .iter()
            .filter(|e| e.id.to_string().contains(query))
            .collect()
    }

    /// Resolves `publisher/name` or a bare name to exactly one listed pack.
    ///
    /// # Errors
    /// Returns [`LookupError::Missing`] when nothing matches and
    /// [`LookupError::Ambiguous`] when a bare name has several publishers.
    pub fn lookup(&self, reference: &str) -> Result<PackId, LookupError> {
        let matches: BTreeSet<_> = self
            .entries
            .iter()
            .filter(|e| {
                if reference.contains('/') {
                    e.id.to_string() == reference
                } else {
                    e.id.name() == reference
                }
            })
            .map(|e| e.id.clone())
            .collect();
        let mut matches = matches.into_iter();
        match (matches.next(), matches.len()) {
            (None, _) => Err(LookupError::Missing),
            (Some(id), 0) => Ok(id),
            (Some(first), _) => Err(LookupError::Ambiguous(
                std::iter::once(first).chain(matches).collect(),
            )),
        }
    }
}

fn is_command(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name.as_bytes()[0].is_ascii_alphanumeric()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._+-".contains(&byte))
}

fn present_commands<'de, D: serde::Deserializer<'de>>(
    input: D,
) -> Result<Option<BTreeMap<PackId, Vec<String>>>, D::Error> {
    BTreeMap::deserialize(input).map(Some)
}

/// Verifies a hexadecimal Ed25519 `signature` over the exact `bytes`.
pub(crate) fn verify_signature(
    key: &str,
    bytes: &[u8],
    signature: &[u8],
) -> Result<(), CatalogError> {
    let signature = std::str::from_utf8(signature)
        .ok()
        .and_then(|s| digest::decode(s, 64))
        .ok_or(CatalogError::SignatureInvalid)?;
    let key = digest::decode(key, 32).ok_or(CatalogError::SignatureInvalid)?;
    ring::signature::UnparsedPublicKey::new(&ring::signature::ED25519, key)
        .verify(bytes, &signature)
        .map_err(|_| CatalogError::SignatureInvalid)
}

#[cfg(test)]
mod tests;
