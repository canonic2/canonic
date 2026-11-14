//! Explicit versioned metadata acquisition with one aggregate budget and deadline.
use super::{AcquisitionError, Fetcher};
use crate::{
    catalog::{Catalog, Entry},
    checkpoint::Checkpoints,
    digest,
    names::PackId,
    source::Source,
    trust::{self, Revocations, TrustError, TrustedRoot},
};
use std::{
    collections::BTreeMap,
    time::{Instant, SystemTime},
};

#[cfg(all(test, feature = "test-support"))]
mod tests;

/// Maximum sequential root successors admitted by one update.
pub const MAX_ROOT_STEPS: usize = 32;
/// Aggregate fetched payload and signature bytes, including catalog/revocations.
pub const MAX_UPDATE_BYTES: u64 = 4 * 1024 * 1024;

/// Immutable inputs captured by the caller before fetching. Adoption/review and
/// durable compare-and-swap remain the caller's responsibility.
#[derive(Clone, Copy)]
pub struct VersionedRequest<'a> {
    /// The original enrolled signed source, never replaced with a rotated key.
    pub source: &'a Source,
    /// Last admitted root and corresponding revocations; absence requires root1.
    pub previous: Option<(&'a TrustedRoot, &'a Revocations)>,
    /// Current catalog anti-rollback/replacement checkpoint value.
    pub checkpoints: &'a Checkpoints,
    /// Installed/pinned release digests retained on catalog admission.
    pub protected: &'a BTreeMap<String, String>,
    /// Fixed wall time for all freshness checks in this update.
    pub now: SystemTime,
}

/// Verified metadata and the next checkpoint value. No constructor accepts
/// caller-appointed signing keys. Persist it only if the captured authority
/// state and expected head still match; fetching alone grants no policy authority.
#[derive(Clone, Debug)]
pub struct VerifiedUpdate {
    root: TrustedRoot,
    revocations: Revocations,
    catalog: Catalog,
    checkpoints: Checkpoints,
    protected: BTreeMap<String, String>,
    expected_head: Option<String>,
}

impl VerifiedUpdate {
    /// Authenticated final root and original enrolled source binding.
    #[must_use]
    pub fn root(&self) -> &TrustedRoot {
        &self.root
    }
    /// Authenticated cumulative exact-release denials.
    #[must_use]
    pub fn revocations(&self) -> &Revocations {
        &self.revocations
    }
    /// Authenticated catalog, including records that may be revoked. Check each
    /// selected release with [`Self::check_release`] before using cached bytes.
    #[must_use]
    pub fn catalog(&self) -> &Catalog {
        &self.catalog
    }
    /// Next catalog checkpoint, admitted without mutating the caller's old value.
    #[must_use]
    pub fn checkpoints(&self) -> &Checkpoints {
        &self.checkpoints
    }
    /// Previously captured root+revocation revision, or None for first adoption.
    #[must_use]
    pub fn expected_head_revision(&self) -> Option<&str> {
        self.expected_head.as_deref()
    }
    /// Current root+revocation revision for caller-owned atomic publication.
    #[must_use]
    pub fn head_revision(&self) -> String {
        head(&self.root, &self.revocations)
    }
    /// Rejects forged catalog selections and exact known revocations, including
    /// bytes already present in a consumer cache. Offline denial has no clock.
    ///
    /// # Errors
    /// Returns [`AcquisitionError`] for absent/replaced entries or revoked bytes.
    pub fn check_release(&self, entry: &Entry) -> Result<(), AcquisitionError> {
        if !self.catalog.entries().contains(entry) {
            return Err(crate::catalog::CatalogError::InvalidEntry.into());
        }
        self.revocations.check_release(
            self.root.source().identity(),
            &entry.id,
            &entry.version,
            &entry.sha256,
        )?;
        Ok(())
    }

    /// Guarded network reads require the root, revocations and catalog to be
    /// fresh at `now`; retaining an update never extends any of them.
    fn check_fresh(&self, now: SystemTime) -> Result<(), AcquisitionError> {
        self.root.check_fresh(now)?;
        self.revocations.check_fresh(now)?;
        self.catalog.check_fresh(now)?;
        Ok(())
    }
}

impl Fetcher {
    /// Explicitly acquires versioned trust metadata, with no fallback to legacy
    /// pinned catalogs after any missing, stale, invalid or unavailable sidecar.
    /// Root probes use `trust/roots/N.json` and `.sig`; revocations use
    /// `trust/revocations.json` and `.sig`; catalog names remain unchanged.
    /// Only an unencoded 404 of the next root document ends a validated chain.
    /// All reads, parsing and verification share one deadline/byte/step budget.
    ///
    /// # Errors
    /// Returns [`AcquisitionError`] without modifying input heads/checkpoints on
    /// cancellation, transport, trust, freshness, budget or continuity refusal.
    pub fn versioned_catalog(
        &self,
        request: VersionedRequest<'_>,
        checkpoint: &dyn Fn() -> Result<(), AcquisitionError>,
    ) -> Result<VerifiedUpdate, AcquisitionError> {
        let VersionedRequest {
            source,
            previous,
            checkpoints,
            protected,
            now,
        } = request;
        if !source.is_signed() {
            return Err(TrustError::Identity.into());
        }
        if previous.is_some_and(|(root, revocations)| {
            root.source() != source || !revocations.belongs_to(root)
        }) {
            return Err(TrustError::Identity.into());
        }
        let mut budget = Budget {
            fetcher: self,
            source,
            deadline: self.client.deadline(),
            bytes: 0,
            checkpoint,
        };
        let mut root = previous.map(|(root, _)| root.clone());
        let mut steps = 0;
        loop {
            let version = root
                .as_ref()
                .map_or(Some(1), |root| root.version().checked_add(1))
                .ok_or(TrustError::Rollback)?;
            let path = format!("trust/roots/{version}.json");
            let Some(payload) = budget.read(&path, trust::MAX_ROOT_BYTES as u64, true)? else {
                if root.is_none() {
                    return Err(TrustError::Malformed.into());
                }
                break;
            };
            if steps >= MAX_ROOT_STEPS {
                return Err(TrustError::Limit.into());
            }
            let signatures = budget.required(
                &format!("trust/roots/{version}.sig"),
                trust::MAX_SIGNATURE_BYTES as u64,
            )?;
            root = Some(match root {
                Some(root) => root.advance(&payload, &signatures)?,
                None => TrustedRoot::bootstrap(source, &payload, &signatures)?,
            });
            steps += 1;
            budget.check()?;
        }
        let root = root.ok_or(TrustError::Malformed)?;
        root.check_fresh(now)?;
        let payload =
            budget.required("trust/revocations.json", trust::MAX_REVOCATION_BYTES as u64)?;
        let signatures =
            budget.required("trust/revocations.sig", trust::MAX_SIGNATURE_BYTES as u64)?;
        let revocations = Revocations::verify(
            &root,
            &payload,
            &signatures,
            previous.map(|(_, revocations)| revocations),
            now,
        )?;
        let payload = budget.required("catalog.json", crate::catalog::MAX_CATALOG_BYTES)?;
        let signatures = budget.required("catalog.sig", trust::MAX_SIGNATURE_BYTES as u64)?;
        let catalog = Catalog::verify_versioned(&root, &payload, &signatures, now)?;
        let mut next = checkpoints.clone();
        next.admit_retaining(&catalog, protected)?;
        budget.check()?;
        Ok(VerifiedUpdate {
            root,
            revocations,
            catalog,
            checkpoints: next,
            protected: protected.clone(),
            expected_head: previous.map(|(root, revocations)| head(root, revocations)),
        })
    }

    /// Loads authenticated history under an already admitted, fresh versioned
    /// head. The extended catalog/checkpoint update is all-or-nothing. Historical
    /// records may remain visible for diagnostics; selected bytes must pass
    /// [`VerifiedUpdate::check_release`].
    ///
    /// # Errors
    /// Returns [`AcquisitionError`] for stale trust, invalid history, cancellation
    /// or catalog continuity refusal, preserving the original update.
    pub fn versioned_history(
        &self,
        update: &mut VerifiedUpdate,
        id: &PackId,
        version: Option<&semver::Version>,
        now: SystemTime,
        checkpoint: &dyn Fn() -> Result<(), AcquisitionError>,
    ) -> Result<(), AcquisitionError> {
        checkpoint()?;
        update.check_fresh(now)?;
        let mut catalog = update.catalog.clone();
        self.history(update.root.source(), &mut catalog, id, version)?;
        let mut checkpoints = update.checkpoints.clone();
        checkpoints.admit_retaining(&catalog, &update.protected)?;
        checkpoint()?;
        update.catalog = catalog;
        update.checkpoints = checkpoints;
        Ok(())
    }

    /// Acquires exact catalog bytes only after checking current freshness and
    /// known revocations. Legacy artifact acquisition is a separate API and
    /// must not be used by a consumer that adopted versioned trust.
    ///
    /// # Errors
    /// Returns [`AcquisitionError`] for stale trust, revoked/foreign entry, digest
    /// mismatch or bounded artifact transport refusal.
    pub fn versioned_artifact(
        &self,
        update: &VerifiedUpdate,
        entry: &Entry,
        max_bytes: u64,
        now: SystemTime,
    ) -> Result<Vec<u8>, AcquisitionError> {
        update.check_fresh(now)?;
        update.check_release(entry)?;
        self.artifact(update.root.source(), entry, max_bytes)
    }
}

fn head(root: &TrustedRoot, revocations: &Revocations) -> String {
    digest::sha256_hex(format!("{}:{}", root.revision(), revocations.revision()).as_bytes())
}

struct Budget<'a> {
    fetcher: &'a Fetcher,
    source: &'a Source,
    deadline: Instant,
    bytes: u64,
    checkpoint: &'a dyn Fn() -> Result<(), AcquisitionError>,
}
impl Budget<'_> {
    fn check(&self) -> Result<(), AcquisitionError> {
        (self.checkpoint)()?;
        if Instant::now() >= self.deadline {
            Err(AcquisitionError::Timeout)
        } else {
            Ok(())
        }
    }
    fn required(&mut self, path: &str, limit: u64) -> Result<Vec<u8>, AcquisitionError> {
        self.read(path, limit, false)?
            .ok_or(TrustError::Malformed.into())
    }
    fn read(
        &mut self,
        path: &str,
        limit: u64,
        next_root: bool,
    ) -> Result<Option<Vec<u8>>, AcquisitionError> {
        self.check()?;
        let remaining = MAX_UPDATE_BYTES
            .checked_sub(self.bytes)
            .ok_or(TrustError::Limit)?;
        if remaining == 0 {
            return Err(TrustError::Limit.into());
        }
        let bytes = if next_root {
            self.fetcher.client.next_root_until(
                self.source.location(),
                path,
                limit.min(remaining),
                self.deadline,
            )?
        } else {
            Some(self.fetcher.client.read_until(
                self.source.location(),
                path,
                limit.min(remaining),
                self.deadline,
            )?)
        };
        if let Some(bytes) = &bytes {
            self.bytes = self
                .bytes
                .checked_add(bytes.len() as u64)
                .ok_or(TrustError::Limit)?;
        }
        self.check()?;
        Ok(bytes)
    }
}
