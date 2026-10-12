//! Validated host permission descriptors and immutable response data.

pub use crate::review_ticket::Ticket;
use crate::tool_call_card::Output;
use std::{collections::HashSet, fmt};

/// Host semantics used to label an acknowledged result, never to enforce policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Host-supplied affirmative response.
    Allow,
    /// Explicit negative answer.
    Deny,
    /// Explicitly continue without this operation.
    Skip,
}
/// One immutable host response option.
#[derive(Clone, Debug)]
pub struct Choice {
    /// Unique response identity.
    pub id: String,
    /// Human-readable action.
    pub label: String,
    /// Short scope description beside/below the choice.
    pub detail: String,
    /// Exact supplied scope, preserved in confirmation and response.
    pub scope: String,
    /// Response semantics.
    pub kind: Kind,
    /// Unavailable reason; disabled rows remain inspectable.
    pub disabled: Option<String>,
    /// Requires a second check quoting the full scope.
    pub wider: bool,
    /// Requires two deliberate Enter presses within three seconds.
    pub destructive: bool,
}
impl Choice {
    /// Construct an enabled, ordinary response with no invented scope.
    #[must_use]
    pub fn new(id: &str, label: &str, kind: Kind) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            detail: String::new(),
            scope: String::new(),
            kind,
            disabled: None,
            wider: false,
            destructive: false,
        }
    }
}
/// Host-supplied affected target and counts.
#[derive(Clone, Debug)]
pub struct Resource {
    /// Resource name/path.
    pub name: String,
    /// Supplied counts or context; UI does not calculate external effects.
    pub detail: String,
}
/// Host-owned permission lifetime and material revision.
#[derive(Clone, Debug)]
pub struct Request {
    /// Queue-unique identity.
    pub id: String,
    /// Increasing material revision.
    pub revision: u64,
    /// Host run label.
    pub run: String,
    /// Operation heading.
    pub title: String,
    /// Bold verb/target description.
    pub summary: String,
    /// Explanation.
    pub reason: String,
    /// Ordered affected resources.
    pub resources: Vec<Resource>,
    /// Inspection description beside the details row.
    pub details_label: String,
    /// Already redacted host source, rendered with tool output semantics.
    pub details: Output,
    /// Supplied response options.
    pub choices: Vec<Choice>,
    /// Optional enabled, non-destructive Deny/Skip ID.
    pub default: Option<String>,
}
/// Immutable identified attempt. Sending this does not grant permission.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Response {
    /// Host request ID.
    pub id: String,
    /// Exact material revision.
    pub revision: u64,
    /// Supplied choice ID, never its label.
    pub choice: String,
    /// Exact confirmed host scope.
    pub scope: String,
    /// Fresh opaque acknowledgement identity.
    pub ticket: Ticket,
    /// Zero for the original attempt; increments only on explicit retry.
    pub retry: u32,
}
/// Host acknowledgement of what actually happened.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Result {
    /// Host applied this exact response; kind derives from the sent choice.
    Applied,
    /// Host could not apply it; explicit R retry remains available.
    Failed(String),
    /// Host rejects the response and disables that choice for this revision.
    Refused(String),
    /// Host stopped waiting; wins over an outstanding response.
    Expired(String),
    /// Host no longer needs the operation.
    Withdrawn(String),
}
/// Observable lifecycle, including distinct non-response outcomes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    /// Choosing or inspecting.
    Open,
    /// Immutable attempt waiting for host acknowledgement.
    Sending,
    /// Actual affirmative host acknowledgement.
    Allowed,
    /// Actual negative host acknowledgement.
    Denied,
    /// Actual skip acknowledgement.
    Skipped,
    /// Host application failed; R only retries.
    Failed,
    /// Host refused and disabled a choice.
    Refused,
    /// Host material replacement; no selection.
    Changed,
    /// Host expiry; inspection/close only.
    Expired,
    /// Host withdrawal; inspection/close only.
    Withdrawn,
    /// Closed without answering.
    Dismissed,
}
/// Atomic descriptor error.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Invalid {
    /// Empty or duplicate ID, or unsafe/unknown default.
    Identity,
    /// Whole request exceeds a documented bound.
    Limit,
    /// Inconsistent scope or prohibited scope controls.
    Scope,
    /// Different identity or non-increasing material revision.
    Revision,
}
impl fmt::Display for Invalid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid permission request: {self:?}")
    }
}
impl std::error::Error for Invalid {}

pub(super) fn validate(r: &Request) -> std::result::Result<(), Invalid> {
    if r.id.is_empty() {
        return Err(Invalid::Identity);
    }
    if r.choices.is_empty() || r.choices.len() > 32 || r.resources.len() > 128 {
        return Err(Invalid::Limit);
    }
    let mut bytes = r.id.len()
        + r.run.len()
        + r.title.len()
        + r.summary.len()
        + r.reason.len()
        + r.details_label.len()
        + r.default.as_ref().map_or(0, String::len);
    let mut ids = HashSet::new();
    for c in &r.choices {
        if c.id.is_empty() || !ids.insert(&c.id) {
            return Err(Invalid::Identity);
        }
        if c.scope.len() > 4096 {
            return Err(Invalid::Limit);
        }
        if c.scope.chars().any(|c| c.is_control() && c != '\n')
            || (c.wider && (c.kind != Kind::Allow || c.scope.trim().is_empty()))
        {
            return Err(Invalid::Scope);
        }
        bytes += c.id.len()
            + c.label.len()
            + c.detail.len()
            + c.scope.len()
            + c.disabled.as_ref().map_or(0, String::len);
    }
    for resource in &r.resources {
        bytes += resource.name.len() + resource.detail.len();
    }
    let details = match &r.details {
        Output::Unavailable(reason) => reason,
        other => other.text(),
    };
    if bytes > 65_536 || details.len() > 262_144 {
        return Err(Invalid::Limit);
    }
    if let Some(default) = &r.default
        && !r.choices.iter().any(|c| {
            &c.id == default && c.kind != Kind::Allow && !c.destructive && c.disabled.is_none()
        })
    {
        return Err(Invalid::Identity);
    }
    Ok(())
}
