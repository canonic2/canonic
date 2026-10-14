//! Bounded, pure version and level resolution over supplied release metadata.
//!
//! Callers supply authenticated, validated release metadata and the set of
//! enrolled source identities. This module performs no acquisition,
//! enrollment, artifact validation, locking or activation.
//!
//! A product may give installed packs an ordered [`Level`], such as a
//! protection profile, that dependencies inherit or map from their parent.
//! Products without levels use `()`.

use crate::names::{self, PackId};
use semver::{Op, Version, VersionReq};
use std::collections::{BTreeMap, BTreeSet};

#[cfg(test)]
mod tests;

/// Maximum explicit roots, and maximum reachable instances.
pub const MAX_INSTANCES: usize = 64;
/// Maximum supplied releases and total dependency edges.
pub const MAX_RELEASES: usize = 4096;
/// Maximum enrolled source identities: more than a full global scope, a full
/// project scope and the built-in source together.
pub const MAX_SOURCES: usize = 256;
/// Deterministic search-state budget.
pub const MAX_STATES: usize = 10_000;

/// An ordered selection level; greater values are stronger.
///
/// [`Level::all`] lists every value in ascending order. A dependency's level
/// mapping holds one required level per parent level, in that order.
pub trait Level: Copy + Ord + std::fmt::Debug + 'static {
    /// Every level, weakest first.
    fn all() -> &'static [Self];
}

impl Level for () {
    fn all() -> &'static [Self] {
        &[()]
    }
}

/// A registry-qualified pack; aliases and the current default play no role here.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Instance {
    /// Captured, enrolled source identity.
    pub registry: String,
    /// Fully qualified publisher/name.
    pub pack: PackId,
}

/// One explicit selection.
#[derive(Clone, Debug)]
pub struct Root<L> {
    /// Registry-qualified identity.
    pub instance: Instance,
    /// Allowed versions for this resolution.
    pub requirement: VersionReq,
    /// Optional exact retained version, including prerelease pins.
    ///
    /// A prerelease pin satisfies a prerelease-free root range by its numeric
    /// version; incoming dependency ranges still require an exact prerelease.
    pub pin: Option<Version>,
    /// Explicit level, which dependencies cannot silently strengthen.
    pub level: L,
}

/// A dependency declared by one exact release.
#[derive(Clone, Debug)]
pub struct Dependency<L> {
    /// Fully qualified publisher/name.
    pub pack: PackId,
    /// Foreign enrolled identity; omission means the parent's source.
    pub registry: Option<String>,
    /// Allowed versions; prereleases require an exact prerelease comparator.
    pub requirement: VersionReq,
    /// Monotonic required level per parent level; omission inherits the parent's.
    pub levels: Option<Vec<L>>,
}

/// Exact metadata supplied by the acquisition owner, without trust claims here.
#[derive(Clone, Debug)]
pub struct Release<L> {
    /// Registry-qualified identity.
    pub instance: Instance,
    /// Immutable release version.
    pub version: Version,
    /// Exact release edges.
    pub dependencies: Vec<Dependency<L>>,
}

/// One selected release with its effective level.
#[derive(Debug)]
pub struct Selection<'a, L> {
    /// Original supplied metadata, never rewritten by resolution.
    pub release: &'a Release<L>,
    /// Explicit root or strongest required level.
    pub level: L,
    /// Whether the caller selected this instance directly.
    pub direct: bool,
}

/// A successful closure with the candidate attempts rejected during its search.
#[derive(Debug)]
pub struct ResolutionReport<'a, L> {
    /// The same selections returned by [`resolve`].
    pub selections: Vec<Selection<'a, L>>,
    /// Failed attempts in deterministic search order, bounded by [`MAX_STATES`].
    ///
    /// These describe branches, not globally unusable releases. A release may
    /// appear more than once, or be selected under a different set of choices.
    /// Candidates excluded by version requirements or pins are not attempted.
    pub rejected: Vec<RejectedCandidate<'a, L>>,
}

/// One candidate whose attempted dependency closure failed during backtracking.
#[derive(Debug)]
pub struct RejectedCandidate<'a, L> {
    /// The candidate being tried when its branch failed.
    pub release: &'a Release<L>,
    /// The failure returned by that branch, which may come from a dependency.
    pub reason: Error,
}

/// Resolution refusal; no failure returns a partial selection.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    /// Malformed identities, duplicate roots or releases, or nonmonotonic mappings.
    #[error("invalid dependency metadata")]
    Invalid,
    /// Input, closure or deterministic search budget exhausted.
    #[error("dependency resolution limit exceeded")]
    Limit,
    /// A reachable dependency names a source that is not enrolled.
    #[error("dependency source is not enrolled: {0}")]
    Unenrolled(String),
    /// No supplied release closure satisfies versions, cycles and levels.
    #[error("no compatible dependency closure")]
    Conflict,
    /// An explicit level is weaker than a reachable dependency requirement.
    #[error("explicit level conflicts with dependency requirements: {0:?}")]
    LevelConflict(Instance),
    /// The selected release graph contains a cycle.
    #[error("cyclic dependency closure")]
    Cycle,
}

impl<L: Level> Dependency<L> {
    /// Parses and validates a declared dependency.
    ///
    /// # Errors
    /// Returns [`Error::Invalid`] for a malformed pack, source or mapping, or
    /// a malformed or oversized requirement.
    pub fn parse(
        pack: &str,
        requirement: &str,
        registry: Option<&str>,
        levels: Option<Vec<L>>,
    ) -> Result<Self, Error> {
        if requirement.len() > names::MAX_TEXT {
            return Err(Error::Invalid);
        }
        let dependency = Self {
            pack: pack.parse().map_err(|_| Error::Invalid)?,
            registry: registry.map(str::to_owned),
            requirement: requirement.parse().map_err(|_| Error::Invalid)?,
            levels,
        };
        if !bounded_requirement(&dependency.requirement)
            || dependency
                .registry
                .as_deref()
                .is_some_and(|r| !names::is_source_identity(r))
            || !monotonic(dependency.levels.as_deref())
        {
            return Err(Error::Invalid);
        }
        Ok(dependency)
    }
}

type Assigned<'a, L> = BTreeMap<Instance, &'a Release<L>>;
type Requirements<'a> = BTreeMap<Instance, Vec<(&'a VersionReq, bool)>>;

/// Resolves supplied releases deterministically, highest versions first in
/// source/pack identity order, backtracking when a release has no valid closure.
/// Only reachable instances are selected.
///
/// Enrolled identities are explicit inputs, never inferred from pack metadata.
/// Roots must be unique. Unenrolled dependencies, cycles and root-level conflicts
/// reject a candidate, allowing lower versions to be tried. Exact pins never
/// fall back to another version. Use [`resolve_with_diagnostics`] to explain
/// failed candidate attempts when reviewing the selected closure.
///
/// # Errors
/// Returns [`Error`] for invalid metadata, unavailable or incompatible closures,
/// unenrolled reachable sources or exhausted bounds. Performs no side effects.
pub fn resolve<'a, L: Level>(
    roots: &[Root<L>],
    releases: &'a [Release<L>],
    enrolled: &BTreeSet<String>,
) -> Result<Vec<Selection<'a, L>>, Error> {
    validate(roots, releases, enrolled)?;
    let mut budget = MAX_STATES;
    search(
        roots,
        releases,
        enrolled,
        &BTreeMap::new(),
        &mut budget,
        &mut None,
    )
}

/// Resolves as [`resolve`], also retaining rejected candidate attempts for review.
///
/// Reporting does not change selection, enrollment, pins or the search budget.
/// Diagnostics are returned only with a complete successful closure. Rejections
/// describe failed branches rather than proving a release is always unusable.
/// No I/O or operator review is performed here; callers present the report.
///
/// ```
/// use canonic_registry::resolution::{
///     self, Dependency, Error, Instance, Release, Root,
/// };
/// use std::collections::BTreeSet;
///
/// let instance = Instance {
///     registry: "acme-source".into(), pack: "acme/app".parse().unwrap(),
/// };
/// let releases: Vec<Release<()>> = vec![
///     Release {
///         instance: instance.clone(), version: "2.0.0".parse().unwrap(),
///         dependencies: vec![
///             Dependency::parse("acme/tokens", "*", Some("foreign"), None).unwrap(),
///         ],
///     },
///     Release {
///         instance: instance.clone(), version: "1.8.0".parse().unwrap(),
///         dependencies: vec![],
///     },
/// ];
/// let roots = [Root {
///     instance, requirement: "*".parse().unwrap(), pin: None, level: (),
/// }];
/// let report = resolution::resolve_with_diagnostics(
///     &roots, &releases, &BTreeSet::from(["acme-source".into()]),
/// ).unwrap();
/// assert_eq!(report.selections[0].release.version.to_string(), "1.8.0");
/// assert_eq!(report.rejected[0].release.version.to_string(), "2.0.0");
/// assert_eq!(report.rejected[0].reason, Error::Unenrolled("foreign".into()));
/// ```
///
/// # Errors
/// Returns the same [`Error`] as [`resolve`], without a partial closure or report.
pub fn resolve_with_diagnostics<'a, L: Level>(
    roots: &[Root<L>],
    releases: &'a [Release<L>],
    enrolled: &BTreeSet<String>,
) -> Result<ResolutionReport<'a, L>, Error> {
    validate(roots, releases, enrolled)?;
    let mut budget = MAX_STATES;
    let mut rejected = Vec::new();
    let selections = search(
        roots,
        releases,
        enrolled,
        &BTreeMap::new(),
        &mut budget,
        &mut Some(&mut rejected),
    )?;
    Ok(ResolutionReport {
        selections,
        rejected,
    })
}

/// Whether a requirement is within the comparator and text bounds.
pub(crate) fn bounded_requirement(value: &VersionReq) -> bool {
    value.comparators.len() <= 64 && value.to_string().len() <= names::MAX_TEXT
}

/// A level's position in [`Level::all`]. [`validate`] has already refused
/// levels that are not listed, so the fallback is never used.
fn rank<L: Level>(level: L) -> usize {
    L::all().iter().position(|l| *l == level).unwrap_or(0)
}

/// Whether `level` is listed by a well-formed [`Level::all`]: non-empty and
/// strictly ascending.
fn known<L: Level>(level: L) -> bool {
    let all = L::all();
    !all.is_empty() && all.windows(2).all(|w| w[0] < w[1]) && all.contains(&level)
}

fn monotonic<L: Level>(levels: Option<&[L]>) -> bool {
    levels.is_none_or(|l| {
        l.len() == L::all().len()
            && l.iter().all(|level| known(*level))
            && l.windows(2).all(|w| w[0] <= w[1])
    })
}

fn validate<L: Level>(
    roots: &[Root<L>],
    releases: &[Release<L>],
    enrolled: &BTreeSet<String>,
) -> Result<(), Error> {
    if roots.len() > MAX_INSTANCES
        || releases.len() > MAX_RELEASES
        || enrolled.len() > MAX_SOURCES
        || releases.iter().map(|r| r.dependencies.len()).sum::<usize>() > MAX_RELEASES
    {
        return Err(Error::Limit);
    }
    if enrolled.iter().any(|s| !names::is_source_identity(s)) {
        return Err(Error::Invalid);
    }
    let mut seen = BTreeSet::new();
    for root in roots {
        if !names::is_source_identity(&root.instance.registry)
            || !seen.insert(&root.instance)
            || !known(root.level)
            || !bounded_requirement(&root.requirement)
            || root
                .pin
                .as_ref()
                .is_some_and(|v| v.to_string().len() > names::MAX_TEXT)
        {
            return Err(Error::Invalid);
        }
        if !enrolled.contains(&root.instance.registry) {
            return Err(Error::Unenrolled(root.instance.registry.clone()));
        }
    }
    let mut seen = BTreeSet::new();
    for release in releases {
        if !names::is_source_identity(&release.instance.registry)
            || !seen.insert((&release.instance, &release.version))
            || release.version.to_string().len() > names::MAX_TEXT
        {
            return Err(Error::Invalid);
        }
        let mut edges = BTreeSet::new();
        for edge in &release.dependencies {
            let target = target(release, edge);
            if !names::is_source_identity(&target.registry)
                || !edges.insert(target)
                || !bounded_requirement(&edge.requirement)
                || !monotonic(edge.levels.as_deref())
            {
                return Err(Error::Invalid);
            }
        }
    }
    Ok(())
}

fn target<L>(release: &Release<L>, edge: &Dependency<L>) -> Instance {
    Instance {
        registry: edge
            .registry
            .clone()
            .unwrap_or_else(|| release.instance.registry.clone()),
        pack: edge.pack.clone(),
    }
}

fn requirements<'a, L>(
    roots: &'a [Root<L>],
    assigned: &'a Assigned<'_, L>,
    enrolled: &BTreeSet<String>,
) -> Result<Requirements<'a>, Error> {
    let mut required: Requirements<'_> = BTreeMap::new();
    for root in roots {
        required
            .entry(root.instance.clone())
            .or_default()
            .push((&root.requirement, true));
    }
    for release in assigned.values() {
        for edge in &release.dependencies {
            let target = target(release, edge);
            if !enrolled.contains(&target.registry) {
                return Err(Error::Unenrolled(target.registry));
            }
            required
                .entry(target)
                .or_default()
                .push((&edge.requirement, false));
        }
    }
    if required.len() > MAX_INSTANCES {
        return Err(Error::Limit);
    }
    Ok(required)
}

fn matches(requirement: &VersionReq, version: &Version) -> bool {
    let explicit_pre = requirement.comparators.len() == 1
        && requirement.comparators[0].op == Op::Exact
        && requirement.comparators[0].minor.is_some()
        && requirement.comparators[0].patch.is_some()
        && !requirement.comparators[0].pre.is_empty();
    (version.pre.is_empty() || explicit_pre) && requirement.matches(version)
}

fn search<'a, L: Level>(
    roots: &[Root<L>],
    releases: &'a [Release<L>],
    enrolled: &BTreeSet<String>,
    assigned: &Assigned<'a, L>,
    budget: &mut usize,
    diagnostics: &mut Option<&mut Vec<RejectedCandidate<'a, L>>>,
) -> Result<Vec<Selection<'a, L>>, Error> {
    *budget = budget.checked_sub(1).ok_or(Error::Limit)?;
    let required = requirements(roots, assigned, enrolled)?;
    let compatible = |id: &Instance, release: &Release<L>| {
        let root = roots.iter().find(|r| r.instance == *id);
        let pin = root.and_then(|r| r.pin.as_ref());
        pin.is_none_or(|pin| *pin == release.version)
            && required[id].iter().all(|(req, direct)| {
                if pin.is_some() && *direct {
                    let mut stable = release.version.clone();
                    stable.pre = semver::Prerelease::EMPTY;
                    req.matches(&release.version)
                        || (req.comparators.iter().all(|c| c.pre.is_empty())
                            && req.matches(&stable))
                } else {
                    matches(req, &release.version)
                }
            })
    };
    if assigned
        .iter()
        .any(|(id, release)| !compatible(id, release))
    {
        return Err(Error::Conflict);
    }
    let Some(id) = required.keys().find(|id| !assigned.contains_key(*id)) else {
        return levels(roots, assigned);
    };
    let mut candidates: Vec<_> = releases
        .iter()
        .filter(|r| r.instance == *id && compatible(id, r))
        .collect();
    candidates.sort_by(|a, b| b.version.cmp(&a.version));
    let mut failure = Error::Conflict;
    for release in candidates {
        let mut next = assigned.clone();
        next.insert(id.clone(), release);
        match search(roots, releases, enrolled, &next, budget, diagnostics) {
            Ok(result) => return Ok(result),
            Err(error @ (Error::Limit | Error::Invalid)) => return Err(error),
            Err(error) => {
                if let Some(rejected) = diagnostics {
                    rejected.push(RejectedCandidate {
                        release,
                        reason: error.clone(),
                    });
                }
                failure = error;
            }
        }
    }
    Err(failure)
}

fn levels<'a, L: Level>(
    roots: &[Root<L>],
    assigned: &Assigned<'a, L>,
) -> Result<Vec<Selection<'a, L>>, Error> {
    // Topological traversal rejects cycles and propagates the strongest incoming
    // level only after all parents contributed, independent of input ordering.
    let mut indegree: BTreeMap<_, usize> = assigned.keys().map(|id| (id.clone(), 0)).collect();
    for release in assigned.values() {
        for edge in &release.dependencies {
            *indegree
                .get_mut(&target(release, edge))
                .ok_or(Error::Conflict)? += 1;
        }
    }
    let mut ready: BTreeSet<_> = indegree
        .iter()
        .filter(|(_, n)| **n == 0)
        .map(|(id, _)| id.clone())
        .collect();
    let mut chosen: BTreeMap<_, _> = roots
        .iter()
        .map(|r| (r.instance.clone(), r.level))
        .collect();
    let mut visited = 0;
    while let Some(id) = ready.pop_first() {
        visited += 1;
        let level = *chosen.get(&id).ok_or(Error::Conflict)?;
        let release = assigned[&id];
        for edge in &release.dependencies {
            let target = target(release, edge);
            let requested = edge.levels.as_ref().map_or(level, |l| l[rank(level)]);
            if let Some(root) = roots.iter().find(|r| r.instance == target)
                && root.level < requested
            {
                return Err(Error::LevelConflict(target));
            }
            chosen
                .entry(target.clone())
                .and_modify(|l| {
                    if *l < requested {
                        *l = requested;
                    }
                })
                .or_insert(requested);
            let count = indegree.get_mut(&target).ok_or(Error::Conflict)?;
            *count -= 1;
            if *count == 0 {
                ready.insert(target);
            }
        }
    }
    if visited != assigned.len() {
        return Err(Error::Cycle);
    }
    Ok(assigned
        .iter()
        .map(|(id, release)| Selection {
            release,
            level: chosen[id],
            direct: roots.iter().any(|r| r.instance == *id),
        })
        .collect())
}
