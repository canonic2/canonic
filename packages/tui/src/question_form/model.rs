//! Validates host descriptors and projects staged values without presentation authority.

use super::Stage;
use crate::picker::Choice;
pub use crate::review_ticket::Ticket;
use std::{collections::HashSet, fmt};
/// Question input kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// One choice or a typed alternate.
    Single,
    /// An ordered set, optionally including a typed alternate.
    Multiple,
    /// A standalone Editor field.
    Text,
}
/// Identified host question and local validation constraints.
#[derive(Clone, Debug)]
pub struct Question {
    /// Unique identity within the request.
    pub id: String,
    /// Short step/receipt label.
    pub label: String,
    /// Main wrapped prompt.
    pub prompt: String,
    /// Optional explanation.
    pub description: String,
    /// Answer kind.
    pub kind: Kind,
    /// Nonempty response required.
    pub required: bool,
    /// Source-ordered choices; disabled rows remain navigable.
    pub choices: Vec<Choice>,
    /// Optional highlighted single-choice ID; never an answer.
    pub default: Option<String>,
    /// Offer a last Something else field on choice questions.
    pub other: bool,
    /// Minimum marks for a nonempty multiple-choice answer.
    pub minimum: usize,
    /// Maximum marks, including a nonempty typed alternate.
    pub maximum: Option<usize>,
    /// Unicode scalar limit, including LF. Retained text is never truncated.
    pub characters: Option<usize>,
}
impl Question {
    /// Construct an optional question with no choices or limits.
    #[must_use]
    pub fn new(id: &str, label: &str, prompt: &str, kind: Kind) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            prompt: prompt.into(),
            description: String::new(),
            kind,
            required: false,
            choices: vec![],
            default: None,
            other: false,
            minimum: 0,
            maximum: None,
            characters: None,
        }
    }
}
/// Host-owned request lifetime and material identity.
#[derive(Clone, Debug)]
pub struct Request {
    /// Unique among all queued question and permission requests.
    pub id: String,
    /// Increasing material revision.
    pub revision: u64,
    /// Form heading.
    pub title: String,
    /// Source order, one to thirty-two questions.
    pub questions: Vec<Question>,
    /// Explicitly offer Send answers so far when cancelling.
    pub partial: bool,
}
/// Immutable staged response data; choice IDs retain host source order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AnswerValue {
    /// Typed standalone response.
    Text(String),
    /// Stable choice IDs and optional typed alternate.
    Choices {
        /// Source order.
        ids: Vec<String>,
        /// Something else value.
        other: Option<String>,
    },
}
/// One answered question.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Answer {
    /// Host question ID.
    pub question: String,
    /// Immutable value.
    pub value: AnswerValue,
}
/// One immutable identified response attempt, accepted only by the host.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Response {
    /// Request ID.
    pub id: String,
    /// Exact material revision.
    pub revision: u64,
    /// Single-use response acknowledgement ticket.
    pub ticket: Ticket,
    /// Answered questions in source order.
    pub answers: Vec<Answer>,
    /// Explicit partial submission.
    pub partial: bool,
}
/// Host acknowledgement; local validation does not establish authority.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Result {
    /// Host accepted the submitted snapshot.
    Accepted,
    /// Retry is a new deliberate confirmation.
    Failed(String),
    /// Reopen an identified question with all answers retained.
    Refused {
        /// Existing question ID.
        question: String,
        /// Host explanation.
        reason: String,
    },
}
/// Observable lifecycle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    /// Local staged input.
    Answering,
    /// Immutable attempt pending; all input locked.
    Submitting,
    /// Actual host acceptance.
    Accepted,
    /// Attempt failed, answers retained.
    Failed,
    /// Host refused an answer, fields retained.
    Refused,
    /// Host expiry, copy/close only.
    Expired,
    /// Closed without submission.
    Cancelled,
}
/// Atomic request validation error.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Invalid {
    /// Empty/duplicate identity or invalid default.
    Identity,
    /// Descriptor count or bytes exceed bounds.
    Limit,
    /// Inconsistent kind, choices or validation bounds.
    Question,
    /// Replacement identity/revision is invalid.
    Revision,
}
impl fmt::Display for Invalid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid question request: {self:?}")
    }
}
impl std::error::Error for Invalid {}

pub(super) fn validate(r: &Request) -> std::result::Result<(), Invalid> {
    if r.id.is_empty() {
        return Err(Invalid::Identity);
    }
    if r.questions.is_empty() || r.questions.len() > 32 {
        return Err(Invalid::Limit);
    }
    let mut bytes = r.id.len() + r.title.len();
    let mut questions = HashSet::new();
    for q in &r.questions {
        if q.id.is_empty() || !questions.insert(&q.id) {
            return Err(Invalid::Identity);
        }
        if q.choices.len() > 128 {
            return Err(Invalid::Limit);
        }
        if (q.kind == Kind::Text && (!q.choices.is_empty() || q.other || q.default.is_some()))
            || (q.kind != Kind::Text && q.choices.is_empty() && !q.other)
            || q.maximum.is_some_and(|m| q.minimum > m)
            || (q.kind != Kind::Multiple && (q.minimum != 0 || q.maximum.is_some()))
        {
            return Err(Invalid::Question);
        }
        let mut choices = HashSet::new();
        for c in &q.choices {
            if c.id.is_empty() || !choices.insert(&c.id) {
                return Err(Invalid::Identity);
            }
            bytes += c.id.len()
                + c.label.len()
                + c.detail.len()
                + c.group.len()
                + c.disabled.as_ref().map_or(0, String::len);
        }
        if let Some(id) = &q.default
            && (q.kind != Kind::Single
                || !q
                    .choices
                    .iter()
                    .any(|c| &c.id == id && c.disabled.is_none()))
        {
            return Err(Invalid::Identity);
        }
        bytes += q.id.len()
            + q.label.len()
            + q.prompt.len()
            + q.description.len()
            + q.default.as_ref().map_or(0, String::len);
        if bytes > 65_536 {
            return Err(Invalid::Limit);
        }
    }
    Ok(())
}
pub(super) fn answer(q: &Question, s: &Stage) -> Option<Answer> {
    let text = s.field.text();
    let value = if q.kind == Kind::Text {
        if text.trim().is_empty() {
            return None;
        }
        AnswerValue::Text(text.into())
    } else {
        let ids: Vec<_> = s.marks.iter().map(|i| q.choices[*i].id.clone()).collect();
        let other = (s.other && !text.trim().is_empty()).then(|| text.into());
        if ids.is_empty() && other.is_none() {
            return None;
        }
        AnswerValue::Choices { ids, other }
    };
    Some(Answer {
        question: q.id.clone(),
        value,
    })
}
pub(super) fn validation(q: &Question, s: &Stage) -> Option<String> {
    let a = answer(q, s);
    if a.is_none() {
        return q.required.then(|| {
            if q.kind == Kind::Text {
                "type an answer".into()
            } else {
                "pick at least one".into()
            }
        });
    }
    if (q.kind == Kind::Text || s.other)
        && q.characters
            .is_some_and(|limit| s.field.text().chars().count() > limit)
    {
        return Some(format!(
            "{} of {} characters · shorten it to go on",
            s.field.text().chars().count(),
            q.characters.unwrap_or(0)
        ));
    }
    let count = s.marks.len() + usize::from(s.other && !s.field.text().trim().is_empty());
    if q.kind == Kind::Multiple && (count < q.minimum || q.maximum.is_some_and(|m| count > m)) {
        return Some(format!(
            "pick {} to {}",
            q.minimum,
            q.maximum.unwrap_or(q.choices.len() + usize::from(q.other))
        ));
    }
    None
}
pub(super) fn display(q: &Question, value: &AnswerValue) -> String {
    match value {
        AnswerValue::Text(text) => text.clone(),
        AnswerValue::Choices { ids, other } => ids
            .iter()
            .filter_map(|id| {
                q.choices
                    .iter()
                    .find(|c| &c.id == id)
                    .map(|c| c.label.clone())
            })
            .chain(other.iter().cloned())
            .collect::<Vec<_>>()
            .join(", "),
    }
}
