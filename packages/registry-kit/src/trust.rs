//! Explicit versioned registry trust, separate from immutable source identity.
//!
//! These opt-in metadata documents are an engineering protocol. Legacy pinned
//! catalogs do not consult them. No endpoint or production signing key is supplied
//! by this library; consumers own activation, durable checkpoints and review.
mod json;
mod revocations;
#[cfg(test)]
pub(crate) mod tests;

use crate::{catalog, digest, source::Source, timestamp};
pub use revocations::Revocations;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, time::SystemTime};

/// Maximum root payload size, before parsing or cryptography.
pub const MAX_ROOT_BYTES: usize = 64 * 1024;
/// Maximum detached signature document size.
pub const MAX_SIGNATURE_BYTES: usize = 16 * 1024;
/// Maximum revocation snapshot payload size.
pub const MAX_REVOCATION_BYTES: usize = 1024 * 1024;
/// Maximum keys and signatures per document.
pub const MAX_KEYS: usize = 16;
/// Maximum distinct signing keys assigned to one role.
pub const MAX_ROLE_KEYS: usize = 8;
/// Maximum revoked releases in the initial snapshot protocol.
pub const MAX_REVOCATIONS: usize = 4096;
/// Maximum encoded private root checkpoint, including JSON string escaping.
pub const MAX_CHECKPOINT_BYTES: usize = 512 * 1024;

/// Versioned trust refusal. Failures never mutate an admitted head.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TrustError {
    /// Invalid JSON, unknown/duplicate fields, unsupported version or bad values.
    #[error("invalid registry trust metadata")]
    Malformed,
    /// An input exceeds its admitted resource bound.
    #[error("registry trust metadata exceeds limits")]
    Limit,
    /// Metadata names another source, origin, registry or root version.
    #[error("registry trust metadata identity mismatch")]
    Identity,
    /// A required distinct-key signature threshold was not met.
    #[error("registry trust signature threshold not met")]
    Signature,
    /// A root skipped a version, or a checkpoint sequence/digest went backwards.
    #[error("registry trust rollback or equivocation")]
    Rollback,
    /// The final root or revocation snapshot is expired.
    #[error("registry trust metadata expired")]
    Expired,
    /// A newer snapshot attempted to remove a known revocation.
    #[error("registry trust metadata removed a known revocation")]
    RevocationRemoved,
    /// A selected exact release is known revoked.
    #[error("registry release is revoked")]
    Revoked,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Role {
    key_ids: Vec<String>,
    threshold: usize,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Roles {
    root: Role,
    catalog: Role,
    revocations: Role,
}

/// Reviewable role authority: SHA-256 fingerprints of raw public keys and the
/// distinct-key signature threshold. These values confer no policy authority.
#[derive(Clone, Debug, Serialize)]
pub struct RoleSummary {
    /// Distinct raw-key SHA-256 fingerprints, as listed by the authenticated root.
    pub key_ids: Vec<String>,
    /// Number of distinct valid signatures required by this role.
    pub threshold: usize,
}

/// Bounded review facts from one authenticated root, for consumer opt-in UI.
#[derive(Clone, Debug, Serialize)]
pub struct RootSummary {
    /// Authenticated consecutive root version.
    pub version: u64,
    /// Final acquisition freshness deadline.
    pub expires_at: String,
    /// Authority allowed to authenticate the next root.
    pub root: RoleSummary,
    /// Authority allowed to authenticate catalogs.
    pub catalog: RoleSummary,
    /// Authority allowed to authenticate exact-release revocations.
    pub revocations: RoleSummary,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Root {
    schema_version: u32,
    kind: String,
    source_identity: String,
    location: String,
    registry_id: String,
    version: u64,
    expires_at: String,
    keys: BTreeMap<String, String>,
    roles: Roles,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Signature {
    key_id: String,
    signature: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Signatures {
    schema_version: u32,
    kind: String,
    #[serde(rename = "signatures")]
    entries: Vec<Signature>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Signed {
    payload: String,
    signatures: String,
}

/// An authenticated current root and constant-sized last-transition evidence.
/// Construction is possible only through bootstrap, advance or private-checkpoint
/// restoration. Source identity always remains the original enrolled anchor.
#[derive(Clone, Debug)]
pub struct TrustedRoot {
    source: Source,
    root: Root,
    signed: Signed,
    previous: Option<Signed>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Checkpoint {
    schema_version: u32,
    kind: String,
    source: Source,
    current: Signed,
    previous: Option<Signed>,
}

impl TrustedRoot {
    /// Adopts root version 1 under the source's enrolled Ed25519 anchor and its
    /// own new root threshold. Freshness is checked separately so an expired
    /// intermediate root can lead to an unexpired authenticated successor.
    ///
    /// # Errors
    /// Returns [`TrustError`] for malformed, oversized, foreign or unsigned roots.
    pub fn bootstrap(
        source: &Source,
        payload: &[u8],
        signatures: &[u8],
    ) -> Result<Self, TrustError> {
        bounded(payload, MAX_ROOT_BYTES)?;
        let signatures_document = signatures_document(signatures)?;
        let key = source.key().ok_or(TrustError::Identity)?;
        let id = key_id(key)?;
        let keys = BTreeMap::from([(id.clone(), key.to_owned())]);
        verify(
            payload,
            &signatures_document,
            &keys,
            &Role {
                key_ids: vec![id],
                threshold: 1,
            },
        )?;
        let root = root(source, payload)?;
        if root.version != 1 {
            return Err(TrustError::Rollback);
        }
        verify(payload, &signatures_document, &root.keys, &root.roles.root)?;
        Ok(Self {
            source: source.clone(),
            root,
            signed: signed(payload, signatures)?,
            previous: None,
        })
    }

    /// Verifies exactly the next root using both the old and new root roles.
    /// The old root may be expired only while reaching a fresh final root.
    ///
    /// # Errors
    /// Returns [`TrustError`] without changing this head on any refusal.
    pub fn advance(&self, payload: &[u8], signatures: &[u8]) -> Result<Self, TrustError> {
        bounded(payload, MAX_ROOT_BYTES)?;
        let signatures_document = signatures_document(signatures)?;
        verify(
            payload,
            &signatures_document,
            &self.root.keys,
            &self.root.roles.root,
        )?;
        let root = root(&self.source, payload)?;
        if self.root.version.checked_add(1) != Some(root.version) {
            return Err(TrustError::Rollback);
        }
        verify(payload, &signatures_document, &root.keys, &root.roles.root)?;
        Ok(Self {
            source: self.source.clone(),
            root,
            signed: signed(payload, signatures)?,
            previous: Some(self.signed.clone()),
        })
    }

    /// Checks the final head at the caller's fixed update-start time.
    ///
    /// # Errors
    /// Returns [`TrustError::Expired`] if the final root has expired.
    pub fn check_fresh(&self, now: SystemTime) -> Result<(), TrustError> {
        fresh(&self.root.expires_at, now)
    }

    /// Immutable enrolled source; rotating a role key never creates a new source.
    #[must_use]
    pub fn source(&self) -> &Source {
        &self.source
    }

    /// Current authenticated root version.
    #[must_use]
    pub fn version(&self) -> u64 {
        self.root.version
    }

    /// Exact authenticated role fingerprints and thresholds for operator review.
    #[must_use]
    pub fn summary(&self) -> RootSummary {
        let role = |role: &Role| RoleSummary {
            key_ids: role.key_ids.clone(),
            threshold: role.threshold,
        };
        RootSummary {
            version: self.root.version,
            expires_at: self.root.expires_at.clone(),
            root: role(&self.root.roles.root),
            catalog: role(&self.root.roles.catalog),
            revocations: role(&self.root.roles.revocations),
        }
    }

    /// Digest of the exact current signed payload, suitable for caller CAS.
    #[must_use]
    pub fn revision(&self) -> String {
        digest::sha256_hex(self.signed.payload.as_bytes())
    }

    /// Encodes a private caller-owned checkpoint, retaining the last predecessor
    /// instead of accumulating every root ever observed.
    ///
    /// # Errors
    /// Returns [`TrustError::Malformed`] if encoding fails.
    pub fn checkpoint(&self) -> Result<Vec<u8>, TrustError> {
        let bytes = serde_json::to_vec(&Checkpoint {
            schema_version: 1,
            kind: "registry-trust-checkpoint".into(),
            source: self.source.clone(),
            current: self.signed.clone(),
            previous: self.previous.clone(),
        })
        .map_err(|_| TrustError::Malformed)?;
        bounded(&bytes, MAX_CHECKPOINT_BYTES)?;
        Ok(bytes)
    }

    /// Restores a previously admitted private checkpoint. The caller's durable
    /// storage is trusted like catalog checkpoints; this is not a network seed.
    /// Revalidates source binding, payloads, signatures and last-transition order.
    ///
    /// # Errors
    /// Returns [`TrustError`] for invalid, foreign or internally inconsistent state.
    pub fn restore_checkpoint(source: &Source, bytes: &[u8]) -> Result<Self, TrustError> {
        let checkpoint: Checkpoint = parse(bytes, MAX_CHECKPOINT_BYTES)?;
        if checkpoint.schema_version != 1 || checkpoint.kind != "registry-trust-checkpoint" {
            return Err(TrustError::Malformed);
        }
        if &checkpoint.source != source {
            return Err(TrustError::Identity);
        }
        let current = &checkpoint.current;
        if let Some(previous) = &checkpoint.previous {
            let previous_root = root(source, previous.payload.as_bytes())?;
            let signatures = signatures_document(previous.signatures.as_bytes())?;
            verify(
                previous.payload.as_bytes(),
                &signatures,
                &previous_root.keys,
                &previous_root.roles.root,
            )?;
            let previous = Self {
                source: source.clone(),
                root: previous_root,
                signed: previous.clone(),
                previous: None,
            };
            previous.advance(current.payload.as_bytes(), current.signatures.as_bytes())
        } else {
            Self::bootstrap(
                source,
                current.payload.as_bytes(),
                current.signatures.as_bytes(),
            )
        }
    }

    pub(crate) fn verify_catalog(
        &self,
        payload: &[u8],
        signatures: &[u8],
        now: SystemTime,
    ) -> Result<(), TrustError> {
        self.check_fresh(now)?;
        bounded(
            payload,
            usize::try_from(crate::catalog::MAX_CATALOG_BYTES).map_err(|_| TrustError::Limit)?,
        )?;
        verify(
            payload,
            &signatures_document(signatures)?,
            &self.root.keys,
            &self.root.roles.catalog,
        )
    }
}

fn root(source: &Source, bytes: &[u8]) -> Result<Root, TrustError> {
    let root: Root = parse(bytes, MAX_ROOT_BYTES)?;
    if root.schema_version != 1
        || root.kind != "registry-root"
        || root.version == 0
        || !timestamp::is_utc_seconds(&root.expires_at)
    {
        return Err(TrustError::Malformed);
    }
    if root.source_identity != source.identity()
        || root.location != source.location()
        || Some(root.registry_id.as_str()) != source.registry_id()
        || !source.is_signed()
    {
        return Err(TrustError::Identity);
    }
    if root.keys.is_empty() || root.keys.len() > MAX_KEYS {
        return Err(TrustError::Limit);
    }
    for (id, key) in &root.keys {
        if key_id(key)? != *id {
            return Err(TrustError::Malformed);
        }
    }
    for role in [
        &root.roles.root,
        &root.roles.catalog,
        &root.roles.revocations,
    ] {
        validate_role(role, &root.keys)?;
    }
    Ok(root)
}

fn validate_role(role: &Role, keys: &BTreeMap<String, String>) -> Result<(), TrustError> {
    if role.key_ids.is_empty() || role.key_ids.len() > MAX_ROLE_KEYS {
        return Err(TrustError::Limit);
    }
    let mut seen = std::collections::BTreeSet::new();
    if role.threshold == 0
        || role.threshold > role.key_ids.len()
        || role
            .key_ids
            .iter()
            .any(|id| !keys.contains_key(id) || !seen.insert(id))
    {
        return Err(TrustError::Malformed);
    }
    Ok(())
}

fn signatures_document(bytes: &[u8]) -> Result<Signatures, TrustError> {
    let document: Signatures = parse(bytes, MAX_SIGNATURE_BYTES)?;
    if document.schema_version != 1 || document.kind != "registry-signatures" {
        return Err(TrustError::Malformed);
    }
    if document.entries.is_empty() || document.entries.len() > MAX_KEYS {
        return Err(TrustError::Limit);
    }
    let mut seen = std::collections::BTreeSet::new();
    if document.entries.iter().any(|signature| {
        !digest::is_sha256_hex(&signature.key_id)
            || !canonical_hex(&signature.signature, 64)
            || !seen.insert(&signature.key_id)
    }) {
        return Err(TrustError::Malformed);
    }
    Ok(document)
}

fn verify(
    bytes: &[u8],
    signatures: &Signatures,
    keys: &BTreeMap<String, String>,
    role: &Role,
) -> Result<(), TrustError> {
    let count = signatures
        .entries
        .iter()
        .filter(|signature| {
            role.key_ids.contains(&signature.key_id)
                && keys.get(&signature.key_id).is_some_and(|key| {
                    catalog::verify_signature(key, bytes, signature.signature.as_bytes()).is_ok()
                })
        })
        .count();
    if count < role.threshold {
        Err(TrustError::Signature)
    } else {
        Ok(())
    }
}

fn key_id(key: &str) -> Result<String, TrustError> {
    if !canonical_hex(key, 32) {
        return Err(TrustError::Malformed);
    }
    Ok(digest::sha256_hex(
        &digest::decode(key, 32).ok_or(TrustError::Malformed)?,
    ))
}

fn canonical_hex(value: &str, length: usize) -> bool {
    digest::normalize_hex(value, length).as_deref() == Some(value)
}

fn signed(payload: &[u8], signatures: &[u8]) -> Result<Signed, TrustError> {
    Ok(Signed {
        payload: std::str::from_utf8(payload)
            .map_err(|_| TrustError::Malformed)?
            .into(),
        signatures: std::str::from_utf8(signatures)
            .map_err(|_| TrustError::Malformed)?
            .into(),
    })
}

fn bounded(bytes: &[u8], limit: usize) -> Result<(), TrustError> {
    if bytes.len() > limit {
        Err(TrustError::Limit)
    } else {
        Ok(())
    }
}

fn parse<T: serde::de::DeserializeOwned>(bytes: &[u8], limit: usize) -> Result<T, TrustError> {
    bounded(bytes, limit)?;
    serde_json::from_value(json::parse(bytes)?).map_err(|_| TrustError::Malformed)
}

fn fresh(expires_at: &str, now: SystemTime) -> Result<(), TrustError> {
    match timestamp::is_expired(expires_at, now) {
        Some(false) => Ok(()),
        Some(true) => Err(TrustError::Expired),
        None => Err(TrustError::Malformed),
    }
}
