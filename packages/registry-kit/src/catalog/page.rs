//! Typed keyset pages over authenticated, already loaded catalog entries.

use super::{Catalog, Entry};
use crate::names::PackId;
use semver::Version;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, ops::Bound};

/// Default number of entries returned by a catalog page.
pub const DEFAULT_LIMIT: usize = 50;
/// Largest accepted catalog page limit.
pub const MAX_LIMIT: usize = 100;
/// Budget for the compact JSON items array, including delimiters and separators.
/// An entry exceeding this budget is returned alone; products enforce their
/// transport limits when encoding the response.
pub const MAX_PAGE_BYTES: usize = 256 * 1024;

/// A validated page size, from 1 through 100; defaults to 50.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PageLimit(usize);

/// A page size outside 1 through 100.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("catalog page limit must be between 1 and 100")]
pub struct InvalidPageLimit;

impl PageLimit {
    /// Validates a page size without clamping.
    ///
    /// # Errors
    /// Returns [`InvalidPageLimit`] for zero or a value above [`MAX_LIMIT`].
    ///
    /// ```
    /// use canonic_registry_kit::catalog::PageLimit;
    /// assert_eq!(PageLimit::default().get(), 50);
    /// assert_eq!(PageLimit::new(100)?.get(), 100);
    /// assert!(PageLimit::new(101).is_err());
    /// # Ok::<(), canonic_registry_kit::catalog::InvalidPageLimit>(())
    /// ```
    pub fn new(value: usize) -> Result<Self, InvalidPageLimit> {
        if (1..=MAX_LIMIT).contains(&value) {
            Ok(Self(value))
        } else {
            Err(InvalidPageLimit)
        }
    }

    /// The validated number of entries requested.
    #[must_use]
    pub fn get(self) -> usize {
        self.0
    }
}

impl Default for PageLimit {
    fn default() -> Self {
        Self(DEFAULT_LIMIT)
    }
}

impl TryFrom<usize> for PageLimit {
    type Error = InvalidPageLimit;

    fn try_from(value: usize) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

/// Last returned identity and exact version, including prerelease/build metadata.
///
/// Products encode this typed sort key into their own cursors, binding the
/// source, operation and query there. Reuse it only with the same listing/query.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntryPosition {
    /// Fully qualified pack identity.
    pub id: PackId,
    /// Exact release version, the unique tie-breaker within a pack.
    pub version: Version,
}

/// A bounded selection of borrowed entries and an exclusive continuation key.
///
/// Pages stop at the item limit or [`MAX_PAGE_BYTES`] of compact JSON items,
/// whichever comes first. A single oversized entry is returned alone.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Page<'a, P> {
    /// At most the requested limit of entries.
    pub items: Vec<&'a Entry>,
    /// Last returned item's sort key, absent when no further match exists.
    pub next_position: Option<P>,
}

pub(super) type Index = BTreeMap<PackId, BTreeMap<Version, usize>>;

pub(super) fn index(entries: &[Entry]) -> Index {
    let mut index = Index::new();
    for (offset, entry) in entries.iter().enumerate() {
        index
            .entry(entry.id.clone())
            .or_default()
            .insert(entry.version.clone(), offset);
    }
    index
}

impl Catalog {
    /// Loaded releases ordered by publisher, then name, then version ascending.
    /// Version uses `semver::Version::Ord`, including build metadata as a final
    /// tie-breaker. Seeks strictly after `position`, even if it no longer exists.
    /// No history is fetched; results borrow this catalog view.
    #[must_use]
    pub fn entries_page(
        &self,
        limit: PageLimit,
        position: Option<&EntryPosition>,
    ) -> Page<'_, EntryPosition> {
        identity_page(&self.entries, &self.entries_by_id, limit, position, |_| {
            true
        })
    }

    /// Discovery releases, excluding loaded history, in the same order as
    /// [`Self::entries_page`]. Flat v1/v2 discovery may contain several versions
    /// of a pack; the version tie-breaker keeps every release addressable.
    #[must_use]
    pub fn heads_page(
        &self,
        limit: PageLimit,
        position: Option<&EntryPosition>,
    ) -> Page<'_, EntryPosition> {
        identity_page(&self.heads, &self.heads_by_id, limit, position, |_| true)
    }

    /// Case-sensitive substring search of `publisher/name` over loaded releases.
    /// All matches have equal relevance; ties use the identity/version order of
    /// [`Self::entries_page`]. Empty queries match all releases. Retains only a
    /// page of references while scanning candidates after the position.
    #[must_use]
    pub fn search_page(
        &self,
        query: &str,
        limit: PageLimit,
        position: Option<&EntryPosition>,
    ) -> Page<'_, EntryPosition> {
        let qualified = query.split_once('/');
        identity_page(&self.entries, &self.entries_by_id, limit, position, |id| {
            if let Some((publisher, name)) = qualified {
                id.publisher().ends_with(publisher) && id.name().starts_with(name)
            } else {
                id.publisher().contains(query) || id.name().contains(query)
            }
        })
    }

    /// Loaded releases for `id`, in descending `semver::Version::Ord` order,
    /// including build metadata as a final tie-breaker. Continues strictly below
    /// the last version; no history is fetched. Missing packs yield an empty page.
    #[must_use]
    pub fn releases_page(
        &self,
        id: &PackId,
        limit: PageLimit,
        position: Option<&Version>,
    ) -> Page<'_, Version> {
        let versions = self.entries_by_id.get(id).into_iter().flat_map(|versions| {
            versions
                .range((
                    Bound::Unbounded,
                    position.map_or(Bound::Unbounded, Bound::Excluded),
                ))
                .rev()
                .map(|(_, offset)| indexed_entry(&self.entries, *offset))
        });
        collect_page(versions, limit, |entry| entry.version.clone())
    }
}

fn identity_page<'a>(
    entries: &'a [Entry],
    index: &'a Index,
    limit: PageLimit,
    position: Option<&EntryPosition>,
    matches: impl Fn(&PackId) -> bool,
) -> Page<'a, EntryPosition> {
    let start = position.map_or(Bound::Unbounded, |position| Bound::Included(&position.id));
    let candidates = index
        .range((start, Bound::Unbounded))
        .filter(|(id, _)| matches(id))
        .flat_map(|(id, versions)| {
            let start = position
                .filter(|position| position.id == *id)
                .map_or(Bound::Unbounded, |position| {
                    Bound::Excluded(&position.version)
                });
            versions
                .range((start, Bound::Unbounded))
                .map(|(_, offset)| indexed_entry(entries, *offset))
        });
    collect_page(candidates, limit, |entry| EntryPosition {
        id: entry.id.clone(),
        version: entry.version.clone(),
    })
}

fn indexed_entry(entries: &[Entry], offset: usize) -> &Entry {
    #[cfg(test)]
    tests::ENTRY_READS.with(|reads| reads.set(reads.get() + 1));
    &entries[offset]
}

fn collect_page<'a, P>(
    candidates: impl Iterator<Item = &'a Entry>,
    limit: PageLimit,
    position: impl FnOnce(&Entry) -> P,
) -> Page<'a, P> {
    let mut items = Vec::new();
    let mut bytes = 2_usize; // Opening and closing array delimiters.
    let mut more = false;
    for entry in candidates {
        if items.len() == limit.get() {
            more = true;
            break;
        }
        let mut encoded = JsonBytes(0);
        // Entry contains only string-serializing types and this writer never
        // fails. Conservatively isolate the entry if serialization ever fails.
        let size = serde_json::to_writer(&mut encoded, entry).map_or(usize::MAX, |()| encoded.0);
        let next_bytes = bytes
            .saturating_add(usize::from(!items.is_empty()))
            .saturating_add(size);
        if !items.is_empty() && next_bytes > MAX_PAGE_BYTES {
            more = true;
            break;
        }
        items.push(entry);
        bytes = next_bytes;
    }
    let next_position = if more {
        items.last().map(|entry| position(entry))
    } else {
        None
    };
    Page {
        items,
        next_position,
    }
}

/// Counts JSON output without allocating an encoded entry or page.
struct JsonBytes(usize);

impl std::io::Write for JsonBytes {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0 = self.0.saturating_add(bytes.len());
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests;
