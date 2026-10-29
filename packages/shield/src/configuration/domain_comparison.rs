//! Owns bounded literal-request domain comparison and unverified evidence.

use super::{Comparison, Effective, Snapshot, compare, evaluation::Prepared};
use crate::policy::{self, Error, Request, Scope, partition};
use serde::Serialize;
mod profiles;
mod symbolic;

#[derive(Clone, Copy)]
enum Proof {
    Profile,
    Symbolic,
}

#[cfg(test)]
mod tests;

/// Relation only within the completed literal request domain described by coverage.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Relation {
    /// Both candidates make the same decision for all covered valid argv.
    Equivalent,
    /// At least one decision tightens and none relax in the covered domain.
    Restrictive,
    /// At least one decision relaxes and none tighten in the covered domain.
    Expansive,
    /// The completed domain contains both relaxations and restrictions.
    Mixed,
    /// The model relation is unproved; witnesses alone do not establish its bound.
    Unknown,
}

/// Read-only comparison over the current literal matcher model.
#[derive(Debug, Serialize)]
pub struct DomainComparison {
    schema_version: u8,
    kind: &'static str,
    mode: &'static str,
    trust: &'static str,
    coverage: &'static str,
    algorithm: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    decision_bound: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    symbolic_regions: Option<usize>,
    limits: partition::Limits,
    host_os: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    cwd: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    project: Option<String>,
    /// Whether the reported model relation was established by partitioning or proof.
    pub complete: bool,
    /// Decision relation within this model/context only, never an activation right.
    pub relation: Relation,
    /// Number of evaluated representatives, including any redundant truth vectors.
    pub evaluated_regions: usize,
    incomplete_reason: Option<partition::Incomplete>,
    /// First deterministic relaxation witness, when found.
    pub expansion: Option<Comparison>,
    /// First deterministic restriction witness, when found.
    pub restriction: Option<Comparison>,
    before: Effective,
    after: Effective,
    /// Fingerprint of the complete serialized report with this field empty.
    pub revision: String,
}

/// Compare all current valid literal argv matching regions at one fixed context.
/// A bounded complete result concerns model decisions only. Incomplete results
/// retain any observed witnesses and always report unknown relation.
///
/// # Errors
/// Rejects invalid candidates/context or shared evaluation/serialization failures.
pub fn compare_argv(
    before: &[(Scope, Snapshot)],
    after: &[(Scope, Snapshot)],
    cwd: &str,
    project: &str,
) -> Result<DomainComparison, Error> {
    comparison(before, after, Some((cwd, project)))
}

/// Compare all valid literal executable/argv/cwd/project matching regions on
/// this host. Context representatives are synthetic strings, without filesystem
/// discovery or canonicalization. Completion proves only model decision relation.
///
/// # Errors
/// Rejects invalid candidates or shared evaluation/serialization failures.
pub fn compare_policy(
    before: &[(Scope, Snapshot)],
    after: &[(Scope, Snapshot)],
) -> Result<DomainComparison, Error> {
    comparison(before, after, None)
}

fn comparison(
    before: &[(Scope, Snapshot)],
    after: &[(Scope, Snapshot)],
    fixed: Option<(&str, &str)>,
) -> Result<DomainComparison, Error> {
    let first = Prepared::new(before)?;
    let second = Prepared::new(after)?;
    let (cwd, project) = fixed.unwrap_or((partition::absolute_root(), partition::absolute_root()));
    let context = Request {
        executable: "shield-context-validation".into(),
        arguments: Vec::new(),
        cwd: cwd.into(),
        project: project.into(),
    };
    // Validate both complete selected policies even if construction hits a budget.
    let first_validation = first.policy(&context)?;
    let second_validation = second.policy(&context)?;
    let packs: Vec<_> = first.packs.iter().chain(&second.packs).copied().collect();
    let customs: Vec<_> = first.custom.iter().chain(&second.custom).copied().collect();
    if has_normalized(&first, &second) {
        return normalized_comparison(first.effective, second.effective, fixed);
    }

    let mut regions = if fixed.is_some() {
        partition::build(&packs, &customs, cwd, project)?
    } else {
        partition::build_all(&packs, &customs)?
    };
    // Equality of every selected artifact byte fingerprint and profile proves
    // the same deterministic decision function without partitioning effects.
    // This is a model proof; it cannot authenticate artifacts or tool semantics.
    let identical = first_validation.revision == second_validation.revision
        && first.effective.access_paths() == second.effective.access_paths();
    let semantic_identity =
        regions.incomplete == Some(partition::Incomplete::UnsupportedSemanticDomain) && identical;
    if semantic_identity {
        regions.incomplete = None;
    }
    let profile_bound = profiles::prepare(&first, &second, fixed, &mut regions)?;
    let symbolic = if profile_bound.is_none() {
        symbolic::prepare(&first, &second, fixed, &mut regions)?
    } else {
        None
    };
    let decision_bound = profile_bound.or(symbolic.map(|(bound, _)| bound));
    // Preconditions or other rule domains leave no complete partition; the
    // command rules still give example requests, decided by both full
    // policies, so the review can show real changes. The result stays incomplete.
    if regions.incomplete == Some(partition::Incomplete::UnsupportedSemanticDomain)
        && regions.requests.is_empty()
    {
        regions.requests = examples(&packs, &customs, fixed);
    }
    let (expansion, restriction) = witnesses(&first, &second, before, after, &regions.requests)?;
    if profiles::finish(decision_bound, expansion.is_some(), restriction.is_some())? {
        regions.incomplete = None;
    }
    // Partitions and bounds cover commands as written. A directory option
    // decides some commands as their rewritten form, so only identical
    // policies keep a proved relation; witnesses above still count.
    if regions.incomplete.is_none()
        && !identical
        && !(first.directory_options.is_empty() && second.directory_options.is_empty())
    {
        regions.incomplete = Some(partition::Incomplete::DirectoryOptions);
    }
    let complete = regions.incomplete.is_none();
    let relation = if complete {
        match (expansion.is_some(), restriction.is_some()) {
            (false, false) => Relation::Equivalent,
            (false, true) => Relation::Restrictive,
            (true, false) => Relation::Expansive,
            (true, true) => Relation::Mixed,
        }
    } else {
        Relation::Unknown
    };
    let (coverage, algorithm) = report_model(
        profile_bound
            .map(|_| Proof::Profile)
            .or(symbolic.map(|_| Proof::Symbolic)),
        semantic_identity,
        regions.semantic,
        fixed.is_some(),
    );
    let mut result = DomainComparison {
        schema_version: if regions.semantic { 2 } else { 1 },
        kind: if fixed.is_some() {
            "argv-domain-comparison"
        } else {
            "policy-domain-comparison"
        },
        mode: "preview",
        trust: "unverified",
        coverage,
        algorithm,
        limits: partition::limits(),
        decision_bound: decision_bound.map(profiles::label),
        symbolic_regions: symbolic.map(|(_, count)| count),
        host_os: std::env::consts::OS,
        cwd: fixed.map(|(cwd, _)| cwd.into()),
        project: fixed.map(|(_, project)| project.into()),
        complete,
        relation,
        evaluated_regions: regions.requests.len(),
        incomplete_reason: regions.incomplete,
        expansion,
        restriction,
        before: first.effective,
        after: second.effective,
        revision: String::new(),
    };
    let bytes = serde_json::to_vec(&result)
        .map_err(|_| Error::Contract("domain comparison serialization failed"))?;
    result.revision = policy::fingerprint(&bytes);
    Ok(result)
}

/// Example requests from the command rules. Examples prove nothing about
fn has_normalized(first: &Prepared<'_>, second: &Prepared<'_>) -> bool {
    !first.effective.mcp_connections().is_empty()
        || !second.effective.mcp_connections().is_empty()
        || first
            .custom
            .iter()
            .chain(&second.custom)
            .any(|c| policy::has_normalized(c))
}

/// Example requests from the command rules. Examples prove nothing about
/// completeness, so a small sample keeps a proposal within its tool deadline
/// even with large packs.
fn examples(
    packs: &[(&policy::Pack, policy::Profile)],
    customs: &[&policy::CustomRules],
    fixed: Option<(&str, &str)>,
) -> Vec<Request> {
    const MAX_EXAMPLES: usize = 256;
    let mut requests = partition::examples(packs, customs, fixed);
    requests.truncate(MAX_EXAMPLES);
    let budget = partition::witness_budget(packs, customs).min(MAX_EXAMPLES);
    profiles::augment(&mut requests, budget);
    requests.truncate(MAX_EXAMPLES);
    requests
}

/// The first request each policy decides more permissively than the other.
fn witnesses(
    first: &Prepared<'_>,
    second: &Prepared<'_>,
    before: &[(Scope, Snapshot)],
    after: &[(Scope, Snapshot)],
    requests: &[Request],
) -> Result<(Option<Comparison>, Option<Comparison>), Error> {
    let mut expansion = None;
    let mut restriction = None;
    for request in requests {
        // Later requests cannot change a report that holds both witnesses.
        if expansion.is_some() && restriction.is_some() {
            break;
        }
        let previous = first.policy(request)?.decision;
        let next = second.policy(request)?.decision;
        if next < previous && expansion.is_none() {
            expansion = Some(compare(before, after, request)?);
        } else if next > previous && restriction.is_none() {
            restriction = Some(compare(before, after, request)?);
        }
    }
    Ok((expansion, restriction))
}

fn report_model(
    proof: Option<Proof>,
    identity: bool,
    semantic: bool,
    fixed: bool,
) -> (&'static str, &'static str) {
    match (proof, identity, semantic, fixed) {
        (Some(Proof::Symbolic), _, _, true) => (
            "argv-v1-symbolic-bound-possible-effects-v2-fixed-context",
            "argv-symbolic-possible-effects-bound-v1",
        ),
        (Some(Proof::Symbolic), _, _, false) => (
            "argv-v1-symbolic-bound-possible-effects-v2-all-contexts",
            "argv-symbolic-possible-effects-bound-v1",
        ),
        (Some(Proof::Profile), _, _, true) => (
            "argv-v1-profile-bound-possible-effects-v2-fixed-context",
            "selected-artifact-profile-bound-v1",
        ),
        (Some(Proof::Profile), _, _, false) => (
            "argv-v1-profile-bound-possible-effects-v2-all-contexts",
            "selected-artifact-profile-bound-v1",
        ),
        (_, true, _, true) => (
            "argv-v1-identical-selected-policy-possible-effects-v2-fixed-context",
            "selected-artifact-identity-v1",
        ),
        (_, true, _, false) => (
            "argv-v1-identical-selected-policy-possible-effects-v2-all-contexts",
            "selected-artifact-identity-v1",
        ),
        (_, _, true, true) => (
            "argv-v1-exact-possible-effects-v2-fixed-context",
            "argv-exact-effects-trie-v2",
        ),
        (_, _, true, false) => (
            "argv-v1-exact-possible-effects-v2-all-contexts",
            "context-argv-exact-effects-trie-v2",
        ),
        (_, _, _, true) => ("argv-v1-fixed-context", "argv-trie-v1"),
        _ => ("argv-v1-all-contexts", "context-argv-trie-v1"),
    }
}

fn normalized_comparison(
    before: Effective,
    after: Effective,
    fixed: Option<(&str, &str)>,
) -> Result<DomainComparison, Error> {
    let mut result = DomainComparison {
        schema_version: 1,
        kind: "policy-domain-comparison",
        mode: "preview",
        trust: "unverified",
        coverage: "normalized-request-conditions",
        algorithm: "unsupported-normalized-domain",
        decision_bound: None,
        symbolic_regions: None,
        limits: partition::limits(),
        host_os: std::env::consts::OS,
        cwd: fixed.map(|(cwd, _)| cwd.into()),
        project: fixed.map(|(_, project)| project.into()),
        complete: false,
        relation: Relation::Unknown,
        evaluated_regions: 0,
        incomplete_reason: Some(partition::Incomplete::UnsupportedSemanticDomain),
        expansion: None,
        restriction: None,
        before,
        after,
        revision: String::new(),
    };
    result.revision = policy::fingerprint(
        serde_json::to_vec(&result)
            .map_err(|_| Error::Contract("comparison encoding"))?
            .as_slice(),
    );
    Ok(result)
}
