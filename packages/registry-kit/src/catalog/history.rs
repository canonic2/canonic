//! Digest-bound, ordered version trees. A page describes one pack only.
use super::{CatalogError, Entry};
use crate::{
    digest,
    names::{self, PackId},
};
use semver::Version;
use serde::{Deserialize, Serialize};

/// Maximum entries or child references in one history page.
pub const PAGE_SIZE: usize = 64;
/// Maximum tree depth followed in one lookup.
pub const MAX_DEPTH: usize = 16;
/// Maximum pages read in one history query.
pub const MAX_PAGES: usize = 128;
/// Maximum history bytes read in one query.
pub const MAX_BYTES: u64 = 8 * 1_048_576;
/// Maximum releases materialized per selected pack in a query/catalog view.
pub const MAX_RELEASES: usize = 4096;

/// A signed-parent commitment to one immutable history page and its version range.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reference {
    /// Safe relative page path within the same registry.
    pub path: String,
    /// SHA-256 of the exact page bytes.
    pub sha256: String,
    /// Lowest version in the subtree, inclusive.
    pub first: Version,
    /// Highest version in the subtree, inclusive.
    pub last: Version,
}

impl Reference {
    pub(crate) fn validate(&self) -> Result<(), CatalogError> {
        if !names::is_artifact_path(&self.path)
            || !digest::is_sha256_hex(&self.sha256)
            || self.first > self.last
        {
            return Err(CatalogError::InvalidEntry);
        }
        Ok(())
    }
}

/// A verified history page; leaves carry releases, branches carry child commitments.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Page {
    schema_version: u8,
    id: PackId,
    /// Ordered releases in a leaf.
    #[serde(default)]
    pub entries: Vec<Entry>,
    /// Ordered, nonoverlapping child ranges in a branch.
    #[serde(default)]
    pub children: Vec<Reference>,
}

impl Page {
    /// Verifies the digest before parsing, then the pack identity, ranges and bounds.
    ///
    /// # Errors
    /// Returns [`CatalogError`] for tampering, malformed pages or unsafe references.
    pub fn verify(id: &PackId, reference: &Reference, bytes: &[u8]) -> Result<Self, CatalogError> {
        if bytes.len() as u64 > super::MAX_CATALOG_BYTES {
            return Err(CatalogError::Limit);
        }
        reference.validate()?;
        if digest::sha256_hex(bytes) != reference.sha256 {
            return Err(CatalogError::InvalidEntry);
        }
        let mut page: Self = serde_json::from_slice(bytes).map_err(|_| CatalogError::Malformed)?;
        if page.schema_version != 1
            || page.id != *id
            || page.entries.is_empty() == page.children.is_empty()
        {
            return Err(CatalogError::InvalidEntry);
        }
        if page.entries.len() > PAGE_SIZE || page.children.len() > PAGE_SIZE {
            return Err(CatalogError::Limit);
        }
        if page.entries.is_empty() {
            for child in &page.children {
                child.validate()?;
            }
            if page
                .children
                .windows(2)
                .any(|pair| pair[0].last >= pair[1].first)
                || page.children.first().map(|child| &child.first) != Some(&reference.first)
                || page.children.last().map(|child| &child.last) != Some(&reference.last)
            {
                return Err(CatalogError::InvalidEntry);
            }
        } else {
            for entry in &mut page.entries {
                if entry.id != *id || !names::is_artifact_path(&entry.path) {
                    return Err(CatalogError::InvalidEntry);
                }
                entry.sha256 =
                    digest::normalize_sha256(&entry.sha256).ok_or(CatalogError::InvalidEntry)?;
            }
            if page
                .entries
                .windows(2)
                .any(|pair| pair[0].version >= pair[1].version)
                || page.entries.first().map(|entry| &entry.version) != Some(&reference.first)
                || page.entries.last().map(|entry| &entry.version) != Some(&reference.last)
            {
                return Err(CatalogError::InvalidEntry);
            }
        }
        Ok(page)
    }
}

#[cfg(test)]
mod tests;
