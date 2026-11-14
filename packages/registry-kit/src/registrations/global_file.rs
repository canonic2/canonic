//! Reviewed global registrations: immutable generations, recoverable publication
//! and a compatible human-readable projection at the caller's selected path.
use super::{RegistrationError, Registrations};
use serde_json::Value;
use std::path::{Path, PathBuf};

mod transaction;
pub use transaction::{MAX_RECEIPT_BYTES, Publication, Snapshot};

/// Maximum size of the global registrations projection.
pub const MAX_FILE_BYTES: u64 = 1_048_576;

/// A product's global discovery configuration and scoped publication journal.
#[derive(Clone, Debug)]
pub struct GlobalFile {
    path: PathBuf,
    builtin: String,
}

impl GlobalFile {
    /// Selects a configuration path and reserved built-in alias.
    #[must_use]
    pub fn new(path: impl Into<PathBuf>, builtin: &str) -> Self {
        Self {
            path: path.into(),
            builtin: builtin.to_owned(),
        }
    }

    /// The human-readable projection path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Reads committed registrations, or a legacy projection before the first
    /// generation. This read never publishes or repairs files.
    ///
    /// # Errors
    /// Refuses malformed, linked, oversized or unavailable required state.
    pub fn read(&self) -> Result<Registrations, RegistrationError> {
        Ok(self.snapshot()?.registrations().clone())
    }

    /// Captures the exact committed generation and projection reviewed by a caller.
    /// When [`Snapshot::projection_ready`] is false, publishing from this
    /// snapshot replaces the exact projection bytes it captured.
    ///
    /// # Errors
    /// Refuses invalid required state or a filesystem failure.
    pub fn snapshot(&self) -> Result<Snapshot, RegistrationError> {
        transaction::snapshot(self)
    }

    /// Parses the projection file on its own, without the active generation,
    /// so a caller can show a manual edit and adopt it by publishing it as
    /// `after`. Returns `None` when the file is missing.
    ///
    /// # Errors
    /// Refuses a linked, oversized or invalid projection or I/O failure.
    pub fn read_projection(&self) -> Result<Option<Registrations>, RegistrationError> {
        transaction::read_projection(self)
    }

    /// Reconciles an already approved interrupted publication. It never invents
    /// an approval or discards conflicting manual bytes. An uncommitted intent
    /// whose reviewed base changed can never commit and is discarded; a missing
    /// projection is rewritten from the active generation.
    ///
    /// # Errors
    /// Refuses live/unverifiable ownership, invalid state, or I/O failure
    /// before the commit point.
    pub fn recover(&self) -> Result<Option<Publication>, RegistrationError> {
        transaction::recover(self)
    }

    /// Reads the last committed receipt only when its logical operation matches.
    /// Older mutations are never revived by this lookup.
    ///
    /// # Errors
    /// Refuses invalid required state or I/O failure.
    pub fn receipt(&self, operation: &str) -> Result<Option<Publication>, RegistrationError> {
        transaction::receipt(self, operation)
    }

    /// Publishes a caller-reviewed generation with a bounded product-owned receipt.
    /// The snapshot binds its base generation, including equal-value ABA changes.
    ///
    /// # Errors
    /// Refuses stale review, invalid/bounded input, live/unverifiable ownership,
    /// or I/O before commitment. Post-commit problems are returned in Publication.
    pub fn publish(
        &self,
        before: &Snapshot,
        after: &Registrations,
        operation: &str,
        receipt: &Value,
    ) -> Result<Publication, RegistrationError> {
        transaction::publish(self, before, after, operation, receipt)
    }

    /// Conditionally replaces registrations through the generation transaction.
    /// Use snapshot and publish to retain a product receipt.
    ///
    /// # Errors
    /// Most errors leave the active generation unchanged. `NotDurable` and
    /// `CommittedRepair` explicitly mean the new generation was committed.
    /// `ProjectionConflict` refuses while the projection holds manual edits;
    /// resolve them through `snapshot` and `publish`.
    pub fn replace(
        &self,
        before: &Registrations,
        after: &Registrations,
    ) -> Result<(), RegistrationError> {
        let result = transaction::replace(self, before, after)?;
        if result.repair_needed {
            return Err(RegistrationError::CommittedRepair);
        }
        if !result.durable {
            return Err(RegistrationError::NotDurable(std::io::Error::other(
                "committed generation requires durability reconciliation",
            )));
        }
        Ok(())
    }
}
