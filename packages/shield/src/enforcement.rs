//! Owns fail-closed agent handoff and mandatory operator/state access restrictions.
//!
//! This is an embedded integration boundary, not a launcher or installed hook.
//! Integrations classify the submitted operation and evaluate it under current
//! policy.
//! Unsupported requests use [`AgentAccess::Unclassified`].

use crate::{
    configuration::CandidateEvaluation,
    policy::{Decision, Request},
    runtime::{self, EvaluatedRequest, Project},
};

pub(crate) mod protocol;
pub(crate) mod read_text;

/// Host-established access classification; never deserialize agent authority claims.
///
/// The host owns classification of aliases, wrappers, shell effects and all write
/// tools. Submitted operator or authorization-state operations must use their
/// protected classifications. This enum does
/// not perform that verification or authenticate its caller.
///
/// ```compile_fail
/// let access: shield::enforcement::AgentAccess = serde_json::from_str("\"ordinary\"").unwrap();
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum AgentAccess {
    /// A supported ordinary request evaluated from the available host input.
    Ordinary,
    /// Approval, grant issuance, activation, enrollment or disabling protection.
    Operator,
    /// Direct mutation of retained policy, approvals, activation or their storage parents.
    AuthorizationStateWrite,
    /// An unsupported or ambiguous submitted operation.
    Unclassified,
}

/// A blocked handoff, distinct from any result returned by the host operation.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Current-state validation failed before invoking the host operation.
    #[error(transparent)]
    Runtime(#[from] runtime::Error),
    /// Policy denied the actual request; one-time approval cannot override it.
    #[error("active policy denied the request")]
    Denied,
    /// This generic gate has no execution grant route; no callback was invoked.
    #[error("operator execution approval is required")]
    ApprovalRequired,
    /// Mandatory operator/state separation, independent of configurable policy.
    #[error("agent access to operator or authorization-state mutation paths is blocked")]
    ProtectedAccess,
    /// The submitted operation is unsupported or ambiguous.
    #[error("unsupported or ambiguous operation is blocked")]
    Unclassified,
}

/// Handoff one owned request only when current policy allows ordinary access.
///
/// Mandatory access restrictions run independently of policy, including an allow
/// rule. For ordinary access the current enrollment and activation are revalidated
/// under the shared writer lock, then allow/ask/deny is handled before the callback.
/// The callback receives the immutable original request/evaluation and runs while
/// that lock is held. It must be short, must not acquire a review writer lock, and
/// must not wait for process completion. Its own return type, including failures,
/// is preserved separately from protection errors.
///
/// Consuming evidence prevents moving that object through another handoff. It is
/// not one-time approval: callers may evaluate new evidence, and no grant is
/// issued or consumed. Allow is a policy decision about the submitted request.
/// Operator CLI review continues through its separate existing entry points.
///
/// ```compile_fail
/// use shield::{enforcement::{self, AgentAccess}, runtime::{Project, EvaluatedRequest}};
/// fn replay(project: &Project, evidence: EvaluatedRequest) {
///     let _ = enforcement::handoff(project, AgentAccess::Unclassified, evidence, |_, _| ());
///     let _ = project.with_current(&evidence, |_, _| ()); // Evidence was moved.
/// }
/// ```
///
/// # Errors
/// Blocks protected or unclassified access, deny, ask and invalid/stale state
/// without invoking the callback. Runtime lock release can fail after a successful
/// callback; that error cannot undo an operation, so callers must not blindly retry.
#[expect(
    clippy::needless_pass_by_value,
    reason = "the handoff consumes immutable evidence so callers cannot replay that object"
)]
pub fn handoff<T>(
    project: &Project,
    access: AgentAccess,
    evidence: EvaluatedRequest,
    operation: impl FnOnce(&Request, &CandidateEvaluation) -> T,
) -> Result<T, Error> {
    verify_access(access)?;
    project.with_current(&evidence, |request, evaluation| match evaluation.decision {
        Decision::Allow => Ok(operation(request, evaluation)),
        Decision::Ask => Err(Error::ApprovalRequired),
        Decision::Deny => Err(Error::Denied),
    })?
}

pub(crate) fn verify_access(access: AgentAccess) -> Result<(), Error> {
    match access {
        AgentAccess::Operator | AgentAccess::AuthorizationStateWrite => {
            return Err(Error::ProtectedAccess);
        }
        AgentAccess::Unclassified => return Err(Error::Unclassified),
        AgentAccess::Ordinary => {}
    }
    Ok(())
}
