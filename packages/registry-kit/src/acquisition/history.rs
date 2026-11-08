//! Request-scoped traversal of signed-parent history commitments.
use super::{AcquisitionError, Fetcher};
use crate::{
    catalog::{
        Catalog, CatalogError,
        history::{self, Page},
    },
    names::PackId,
    source::Source,
};
use semver::Version;
use std::{collections::BTreeSet, time::Instant};

impl Fetcher {
    /// Loads one pack's authenticated history into a catalog view.
    ///
    /// With `version`, follows only the matching branch. Without it, loads the
    /// selected pack's history within query budgets. Flat catalogs require no I/O.
    /// The caller must admit the resulting view through its retained checkpoints.
    /// Failures leave `catalog` unchanged. All reads share one absolute deadline.
    ///
    /// # Errors
    /// Returns [`AcquisitionError`] for tampering, invalid pages, source mismatch,
    /// acquisition failure or exhausted query budgets.
    pub fn history(
        &self,
        source: &Source,
        catalog: &mut Catalog,
        id: &PackId,
        version: Option<&Version>,
    ) -> Result<(), AcquisitionError> {
        if source.identity() != catalog.source_identity() {
            return Err(CatalogError::Identity.into());
        }
        let Some(root) = catalog.history(id) else {
            return Ok(());
        };
        if catalog.history_loaded(id) || version.is_some_and(|v| catalog.release(id, v).is_some()) {
            return Ok(());
        }
        let deadline = self.client.deadline();
        let mut pending = vec![(root.clone(), 1usize)];
        let mut seen = BTreeSet::new();
        let mut size = 0u64;
        let mut entries = Vec::new();
        while let Some((reference, depth)) = pending.pop() {
            if version.is_some_and(|v| v < &reference.first || v > &reference.last) {
                continue;
            }
            if Instant::now() >= deadline {
                return Err(AcquisitionError::Timeout);
            }
            if depth > history::MAX_DEPTH
                || seen.len() >= history::MAX_PAGES
                || !seen.insert(reference.path.clone())
            {
                return Err(CatalogError::Limit.into());
            }
            let remaining = history::MAX_BYTES
                .checked_sub(size)
                .ok_or(CatalogError::Limit)?;
            let limit = super::MAX_CATALOG_BYTES.min(remaining);
            let bytes = if source.is_signed() {
                self.client
                    .read_until(source.location(), &reference.path, limit, deadline)?
            } else {
                self.read(source, &reference.path, limit)?
            };
            size += bytes.len() as u64;
            if Instant::now() >= deadline {
                return Err(AcquisitionError::Timeout);
            }
            let page = Page::verify(id, &reference, &bytes)?;
            entries.extend(page.entries);
            if entries.len() > history::MAX_RELEASES {
                return Err(CatalogError::Limit.into());
            }
            pending.extend(page.children.into_iter().map(|child| (child, depth + 1)));
        }
        catalog.extend_history(id, entries, version.is_none())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
