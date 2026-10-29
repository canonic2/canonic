//! Owns request-specific permission-change evidence over retained candidates.

use super::{CandidateEvaluation, Snapshot, evaluate};
use crate::policy::{self, Error, Request, Scope};
use serde::Serialize;

#[cfg(test)]
mod tests;

/// Decision direction for the supplied request only.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Change {
    /// The after candidate is less restrictive for this request.
    Expansion,
    /// The after candidate is more restrictive for this request.
    Restriction,
    /// The decisions agree; policies and explanations may still differ.
    Unchanged,
}

/// Unverified evidence for exactly one literal request, never a global policy proof.
#[derive(Debug, Serialize)]
pub struct Comparison {
    schema_version: u8,
    mode: &'static str,
    trust: &'static str,
    kind: &'static str,
    coverage: &'static str,
    overall_relation: &'static str,
    /// Request-specific direction, with no claim about unexamined requests.
    pub change: Change,
    /// Fingerprint of the complete comparison evidence, never an approval token.
    pub revision: String,
    /// Exact literal request and context used on both sides.
    pub request: Request,
    /// Complete before candidate metadata and matching-rule explanation.
    pub before: CandidateEvaluation,
    /// Complete after candidate metadata and matching-rule explanation.
    pub after: CandidateEvaluation,
}

/// Compare the same literal request against two complete explicit candidates.
/// Decisions are ordered allow < ask < deny. An expansion is evidence only for
/// this request/context; every result leaves the overall relation unknown.
/// No files, active state, authenticated scope, or approval are accessed.
///
/// # Errors
/// Rejects either invalid composition, invalid request, or serialization failure.
pub fn compare(
    before: &[(Scope, Snapshot)],
    after: &[(Scope, Snapshot)],
    request: &Request,
) -> Result<Comparison, Error> {
    let before = evaluate(before, request)?;
    let after = evaluate(after, request)?;
    let change = match after.decision.cmp(&before.decision) {
        std::cmp::Ordering::Less => Change::Expansion,
        std::cmp::Ordering::Greater => Change::Restriction,
        std::cmp::Ordering::Equal => Change::Unchanged,
    };
    let mut comparison = Comparison {
        schema_version: 1,
        mode: "preview",
        trust: "unverified",
        kind: "request-comparison",
        coverage: "single-literal-request",
        overall_relation: "unknown",
        change,
        revision: String::new(),
        request: request.clone(),
        before,
        after,
    };
    // Domain-separated canonical evidence with an empty revision field. It binds
    // direction, both full candidates/explanations, and every request field.
    let bytes = serde_json::to_vec(&comparison)
        .map_err(|_| Error::Contract("comparison serialization failed"))?;
    comparison.revision = policy::fingerprint(&bytes);
    Ok(comparison)
}
