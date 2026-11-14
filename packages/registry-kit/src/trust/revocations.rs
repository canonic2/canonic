//! Authenticated cumulative exact-release revocations and offline deny checks.
use super::{
    MAX_REVOCATION_BYTES, MAX_REVOCATIONS, Signed, SystemTime, TrustError, TrustedRoot, bounded,
    digest, fresh, parse, signatures_document, signed, timestamp, verify,
};
use crate::names::PackId;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
struct Release {
    id: PackId,
    version: semver::Version,
    sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Document {
    schema_version: u32,
    kind: String,
    source_identity: String,
    location: String,
    registry_id: String,
    root_version: u64,
    sequence: u64,
    expires_at: String,
    releases: Vec<Release>,
}

/// Verified revocations. Expiry gates new acquisition, never removal of an
/// already-known offline deny. The full signed snapshot is deliberately bounded.
#[derive(Clone, Debug)]
pub struct Revocations {
    document: Document,
    signed: Signed,
    index: BTreeSet<(PackId, semver::Version, String)>,
}

impl Revocations {
    pub(crate) fn belongs_to(&self, root: &TrustedRoot) -> bool {
        self.document.source_identity == root.source.identity()
            && self.document.location == root.source.location()
            && Some(self.document.registry_id.as_str()) == root.source.registry_id()
            && self.document.root_version == root.version()
    }

    /// Checks freshness for a new acquisition without removing offline denials.
    ///
    /// # Errors
    /// Returns [`TrustError::Expired`] for expired snapshot metadata.
    pub fn check_fresh(&self, now: SystemTime) -> Result<(), TrustError> {
        fresh(&self.document.expires_at, now)
    }
    /// Verifies a current snapshot and its continuity with the last admitted one.
    ///
    /// # Errors
    /// Returns [`TrustError`] on signature, identity, expiry, rollback, duplicate,
    /// bound or removed-revocation refusal. `previous` is never modified.
    pub fn verify(
        root: &TrustedRoot,
        payload: &[u8],
        signatures: &[u8],
        previous: Option<&Self>,
        now: SystemTime,
    ) -> Result<Self, TrustError> {
        root.check_fresh(now)?;
        let value = Self::restore_checkpoint(root, payload, signatures)?;
        value.check_fresh(now)?;
        if let Some(previous) = previous {
            value.check_previous(previous)?;
        }
        Ok(value)
    }

    /// Restores exact previously admitted signed bytes for offline deny checks.
    /// Signature, identity, syntax and bounds are revalidated; elapsed freshness
    /// does not erase a known revocation. This does not admit new acquisitions.
    ///
    /// # Errors
    /// Returns [`TrustError`] for malformed, foreign or unauthenticated snapshots.
    pub fn restore_checkpoint(
        root: &TrustedRoot,
        payload: &[u8],
        signatures: &[u8],
    ) -> Result<Self, TrustError> {
        bounded(payload, MAX_REVOCATION_BYTES)?;
        verify(
            payload,
            &signatures_document(signatures)?,
            &root.root.keys,
            &root.root.roles.revocations,
        )?;
        let document: Document = parse(payload, MAX_REVOCATION_BYTES)?;
        if document.schema_version != 1
            || document.kind != "registry-revocations"
            || document.sequence == 0
        {
            return Err(TrustError::Malformed);
        }
        if document.source_identity != root.source.identity()
            || document.location != root.source.location()
            || Some(document.registry_id.as_str()) != root.source.registry_id()
            || document.root_version != root.version()
        {
            return Err(TrustError::Identity);
        }
        if !timestamp::is_utc_seconds(&document.expires_at) {
            return Err(TrustError::Malformed);
        }
        if document.releases.len() > MAX_REVOCATIONS {
            return Err(TrustError::Limit);
        }
        let mut index = BTreeSet::new();
        for release in &document.releases {
            if !digest::is_sha256_hex(&release.sha256)
                || release.reason.as_ref().is_some_and(|reason| {
                    reason.len() > 1024 || reason.chars().any(char::is_control)
                })
            {
                return Err(TrustError::Malformed);
            }
            let key = (
                release.id.clone(),
                release.version.clone(),
                release.sha256.clone(),
            );
            if index.last().is_some_and(|last| last >= &key) || !index.insert(key) {
                return Err(TrustError::Malformed);
            }
        }
        let signed = signed(payload, signatures)?;
        Ok(Self {
            document,
            signed,
            index,
        })
    }

    fn check_previous(&self, previous: &Self) -> Result<(), TrustError> {
        let document = &self.document;
        let signed = &self.signed;
        let index = &self.index;
        if previous.document.source_identity != document.source_identity
            || previous.document.location != document.location
            || previous.document.registry_id != document.registry_id
        {
            return Err(TrustError::Identity);
        }
        if document.sequence < previous.document.sequence
            || (document.sequence == previous.document.sequence
                && signed.payload != previous.signed.payload)
        {
            return Err(TrustError::Rollback);
        }
        if !previous.index.is_subset(index) {
            return Err(TrustError::RevocationRemoved);
        }
        Ok(())
    }

    /// Exact release denial independent of network availability or later expiry.
    ///
    /// # Errors
    /// Returns [`TrustError::Identity`] for another source, or
    /// [`TrustError::Revoked`] for a known exact release, or
    /// [`TrustError::Malformed`] for a noncanonical artifact digest.
    pub fn check_release(
        &self,
        source: &str,
        id: &PackId,
        version: &semver::Version,
        sha256: &str,
    ) -> Result<(), TrustError> {
        if source != self.document.source_identity {
            return Err(TrustError::Identity);
        }
        if !digest::is_sha256_hex(sha256) {
            return Err(TrustError::Malformed);
        }
        if self
            .index
            .contains(&(id.clone(), version.clone(), sha256.into()))
        {
            Err(TrustError::Revoked)
        } else {
            Ok(())
        }
    }

    /// The admitted snapshot sequence.
    #[must_use]
    pub fn sequence(&self) -> u64 {
        self.document.sequence
    }

    /// Exact snapshot digest for caller checkpoint/CAS binding.
    #[must_use]
    pub fn revision(&self) -> String {
        digest::sha256_hex(self.signed.payload.as_bytes())
    }

    /// Signed payload/signature bytes for caller-owned persistence. Restore by
    /// verification under the corresponding admitted root, not raw deserialization.
    #[must_use]
    pub fn signed_bytes(&self) -> (&[u8], &[u8]) {
        (
            self.signed.payload.as_bytes(),
            self.signed.signatures.as_bytes(),
        )
    }
}
