//! Lockfile entries for installed packs and their closure invariants.
//!
//! Products own their lockfile document and embed these entries in it. An
//! entry holds no machine paths, aliases or user identities, so a lockfile is
//! identical on every machine. Incoming constraints are derived from the
//! recorded edges rather than stored twice.

use crate::{digest, names, names::PackId, selector::InstalledSelector};
use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Maximum locked packs, matching the resolution closure bound.
pub const MAX_PACKS: usize = crate::resolution::MAX_INSTANCES;

/// One installed pack instance.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LockedPack {
    /// Stable selector for this instance.
    pub selector: InstalledSelector,
    /// Source identity the release came from.
    pub registry: String,
    /// Fully qualified publisher/name.
    pub id: PackId,
    /// Exact installed version.
    pub version: Version,
    /// Lowercase SHA-256 of the artifact bytes.
    pub digest: String,
    /// Whether an exact version was requested; updates skip pinned packs.
    pub pinned: bool,
    /// Whether the operator selected this pack, rather than a dependency.
    pub direct: bool,
    /// Dependency edges declared by this exact release.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dependencies: Vec<LockedEdge>,
}

/// One dependency edge of a locked release.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LockedEdge {
    /// Source identity of the target.
    pub registry: String,
    /// Target publisher/name.
    pub pack: PackId,
    /// Declared requirement, which the target's version satisfies.
    pub requirement: VersionReq,
}

/// A violated lockfile invariant.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LockError {
    /// More than [`MAX_PACKS`] entries.
    #[error("lockfile exceeds the pack limit")]
    Limit,
    /// A selector or source/pack pair appears twice.
    #[error("lockfile repeats {0}")]
    Duplicate(String),
    /// A digest or source identity is malformed.
    #[error("lockfile entry {0} is malformed")]
    Malformed(String),
    /// An edge points at a pack that is not locked.
    #[error("lockfile dependency of {0} is missing")]
    MissingDependency(String),
    /// A locked version does not satisfy an incoming requirement.
    #[error("lockfile dependency of {0} is unsatisfied")]
    Unsatisfied(String),
    /// A dependency entry is not reachable from any direct entry.
    #[error("lockfile entry {0} is unreachable from direct selections")]
    Unreachable(String),
}

impl LockedPack {
    fn key(&self) -> (&str, &PackId) {
        (&self.registry, &self.id)
    }

    fn label(&self) -> String {
        format!("{}@{}", self.id, self.version)
    }
}

/// Checks the closure invariants of a set of locked packs.
///
/// Selectors and source/pack pairs are unique, digests and identities are
/// well formed, every edge targets a locked pack whose version satisfies it,
/// and every dependency is reachable from a direct selection.
///
/// # Errors
/// Returns the first [`LockError`] found.
pub fn validate(packs: &[LockedPack]) -> Result<(), LockError> {
    if packs.len() > MAX_PACKS {
        return Err(LockError::Limit);
    }
    let mut selectors = BTreeSet::new();
    let mut by_key = BTreeMap::new();
    for pack in packs {
        if !selectors.insert(&pack.selector) {
            return Err(LockError::Duplicate(pack.selector.to_string()));
        }
        if by_key.insert(pack.key(), pack).is_some() {
            return Err(LockError::Duplicate(pack.id.to_string()));
        }
        if !digest::is_sha256_hex(&pack.digest)
            || !names::is_source_identity(&pack.registry)
            || pack.dependencies.iter().any(|e| {
                !names::is_source_identity(&e.registry)
                    || !crate::resolution::bounded_requirement(&e.requirement)
            })
        {
            return Err(LockError::Malformed(pack.label()));
        }
    }
    for pack in packs {
        for edge in &pack.dependencies {
            let target = by_key
                .get(&(edge.registry.as_str(), &edge.pack))
                .ok_or_else(|| LockError::MissingDependency(pack.label()))?;
            if !edge.requirement.matches(&target.version) {
                return Err(LockError::Unsatisfied(pack.label()));
            }
        }
    }
    let mut reached: BTreeSet<_> = packs
        .iter()
        .filter(|p| p.direct)
        .map(LockedPack::key)
        .collect();
    let mut frontier: Vec<_> = packs.iter().filter(|p| p.direct).collect();
    while let Some(pack) = frontier.pop() {
        for edge in &pack.dependencies {
            if let Some(target) = by_key.get(&(edge.registry.as_str(), &edge.pack))
                && reached.insert(target.key())
            {
                frontier.push(target);
            }
        }
    }
    if let Some(unreachable) = packs.iter().find(|p| !reached.contains(&p.key())) {
        return Err(LockError::Unreachable(unreachable.label()));
    }
    Ok(())
}

/// Sorts entries by source identity and publisher/name for reproducible output.
pub fn sort(packs: &mut [LockedPack]) {
    packs.sort_by(|a, b| a.key().cmp(&b.key()));
}

/// Locked packs that depend on `registry`/`id`, with the edge each declares.
#[must_use]
pub fn incoming<'a>(
    packs: &'a [LockedPack],
    registry: &str,
    id: &PackId,
) -> Vec<(&'a LockedPack, &'a LockedEdge)> {
    packs
        .iter()
        .flat_map(|pack| pack.dependencies.iter().map(move |edge| (pack, edge)))
        .filter(|(_, edge)| edge.registry == registry && edge.pack == *id)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pack(name: &str, version: &str, direct: bool, dependencies: &[(&str, &str)]) -> LockedPack {
        LockedPack {
            selector: InstalledSelector::from_seed(name.as_bytes()),
            registry: "acme-source".into(),
            id: format!("acme/{name}").parse().unwrap(),
            version: version.parse().unwrap(),
            digest: digest::sha256_hex(name.as_bytes()),
            pinned: false,
            direct,
            dependencies: dependencies
                .iter()
                .map(|(target, requirement)| LockedEdge {
                    registry: "acme-source".into(),
                    pack: format!("acme/{target}").parse().unwrap(),
                    requirement: requirement.parse().unwrap(),
                })
                .collect(),
        }
    }

    #[test]
    fn complete_closures_validate_and_sort_reproducibly() {
        let mut packs = vec![
            pack("skills", "1.0.0", true, &[("rules", "^1")]),
            pack("rules", "1.2.0", false, &[]),
        ];
        validate(&packs).unwrap();
        sort(&mut packs);
        assert_eq!(packs[0].id.to_string(), "acme/rules");
        let incoming = incoming(&packs, "acme-source", &"acme/rules".parse().unwrap());
        assert_eq!(incoming.len(), 1);
        assert_eq!(incoming[0].0.id.to_string(), "acme/skills");
        let json = serde_json::to_string(&packs).unwrap();
        assert!(!json.contains("\"dependencies\":[]"));
        assert_eq!(
            serde_json::from_str::<Vec<LockedPack>>(&json).unwrap(),
            packs
        );
    }

    #[test]
    fn broken_closures_refuse() {
        let base = || {
            vec![
                pack("skills", "1.0.0", true, &[("rules", "^1")]),
                pack("rules", "1.2.0", false, &[]),
            ]
        };
        let mut missing = base();
        missing.pop();
        assert!(matches!(
            validate(&missing),
            Err(LockError::MissingDependency(_))
        ));
        let mut unsatisfied = base();
        unsatisfied[1].version = "2.0.0".parse().unwrap();
        assert!(matches!(
            validate(&unsatisfied),
            Err(LockError::Unsatisfied(_))
        ));
        let mut orphan = base();
        orphan.push(pack("orphan", "1.0.0", false, &[]));
        assert!(matches!(validate(&orphan), Err(LockError::Unreachable(_))));
        let mut duplicate = base();
        duplicate.push(pack("rules", "1.3.0", true, &[]));
        assert!(matches!(validate(&duplicate), Err(LockError::Duplicate(_))));
        let mut malformed = base();
        malformed[0].digest = "ABC".into();
        assert!(matches!(validate(&malformed), Err(LockError::Malformed(_))));
        let mut oversized = base();
        let comparator: semver::Comparator = ">=1.0.0".parse().unwrap();
        oversized[0].dependencies[0].requirement = VersionReq {
            comparators: vec![comparator; 65],
        };
        assert!(matches!(validate(&oversized), Err(LockError::Malformed(_))));
        let many: Vec<_> = (0..=MAX_PACKS)
            .map(|i| pack(&format!("p{i}"), "1.0.0", true, &[]))
            .collect();
        assert_eq!(validate(&many), Err(LockError::Limit));
    }

    #[test]
    fn unknown_fields_and_machine_paths_are_not_accepted() {
        let mut value = serde_json::to_value(pack("skills", "1.0.0", true, &[])).unwrap();
        value["location"] = serde_json::json!("/Users/acme/registry");
        assert!(serde_json::from_value::<LockedPack>(value).is_err());
    }
}
