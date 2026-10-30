//! Host facts and bounded descriptor validation; no discovery, prices or connection probes.

pub use crate::review_ticket::Ticket;
use std::{collections::HashSet, time::Duration};

/// Semantic state of a supplied section.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum State {
    #[default]
    Unknown,
    Loading,
    Ready,
    Empty,
    Error,
}
/// Meaning used for compact summaries; identity and order remain host-owned.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Kind {
    Context,
    Cost,
    Todos,
    Files,
    Connections,
    Activity,
    #[default]
    Other,
}
/// Opt-in Chat routing after higher-priority review/run attention handling.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Entry {
    Status,
    Notices,
    Attention,
}
/// Derived presentation only; mutable fact/notice state stays in `StatusPanels`.
#[derive(Clone, Debug, Default)]
pub struct Chrome {
    pub summary: String,
    pub activity: Option<(String, Severity)>,
    pub focused: bool,
}
/// Decoration family for supplied rows; status words remain host facts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Glyph {
    #[default]
    Plain,
    Done,
    Working,
    Queued,
    Connected,
    Transition,
    Failed,
    Modified,
    Added,
    Deleted,
}
/// Host-offered action. A descriptor grants no execution authority.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Action {
    pub id: String,
    pub label: String,
    pub pending: String,
    pub disabled: Option<String>,
}
impl Action {
    #[must_use]
    pub fn new(id: &str, label: &str) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            pending: label.into(),
            disabled: None,
        }
    }
}
/// One section row, with identity scoped to its section.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub id: String,
    pub label: String,
    pub detail: String,
    pub glyph: Glyph,
    pub action: Option<Action>,
}
impl Row {
    #[must_use]
    pub fn new(id: &str, label: &str) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            detail: String::new(),
            glyph: Glyph::Plain,
            action: None,
        }
    }
}
/// Supplied facts and state. Increment revision whenever any material changes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Section {
    pub kind: Kind,
    pub id: String,
    pub revision: u64,
    pub title: String,
    pub summary: String,
    pub state: State,
    pub detail: String,
    pub rows: Vec<Row>,
    pub action: Option<Action>,
    pub retry: Option<Action>,
    /// Supplied filled cells in the ten-cell usage bar, never calculated from prices/usage.
    pub bar: Option<u8>,
    pub warning: bool,
}
impl Section {
    #[must_use]
    pub fn new(id: &str, title: &str) -> Self {
        Self {
            kind: Kind::Other,
            id: id.into(),
            revision: 1,
            title: title.into(),
            summary: String::new(),
            state: State::Unknown,
            detail: "not reported".into(),
            rows: vec![],
            action: None,
            retry: None,
            bar: None,
            warning: false,
        }
    }
}
/// Severity determines priority; newest supplied timestamp wins within a severity.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    #[default]
    Info,
    Success,
    Warning,
    Error,
}
/// A notice is active until removal, resolution or its eligible explicit deadline.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notice {
    pub id: String,
    pub revision: u64,
    pub severity: Severity,
    pub label: String,
    pub detail: String,
    pub timestamp: String,
    pub published: Duration,
    pub expires: Option<Duration>,
    pub action: Option<Action>,
}
impl Notice {
    #[must_use]
    pub fn new(id: &str, severity: Severity, label: &str) -> Self {
        Self {
            id: id.into(),
            revision: 1,
            severity,
            label: label.into(),
            detail: String::new(),
            timestamp: String::new(),
            published: Duration::ZERO,
            expires: None,
            action: None,
        }
    }
}
/// Authoritative state in supplied order. Removed notices are resolved by the host.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub revision: u64,
    pub sections: Vec<Section>,
    pub notices: Vec<Notice>,
}
/// Stable selected/focus/effect identity, independent of visual rows or resize.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Target {
    Section { id: String, row: Option<String> },
    Notice(String),
}
/// Exact action request. Clone it to acknowledge on its issuing instance once.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    pub ticket: Ticket,
    pub target: Target,
    pub revision: u64,
    pub action: String,
}
/// Success, authoritative update or a retryable host failure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Done,
    Applied(Snapshot),
    Failed(String),
}
/// Returned host work and restoration of caller-owned focus.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Intent {
    Request(Request),
    Returned,
}
/// Pane focus; opening does not clone or modify the caller's draft.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    #[default]
    Closed,
    Status,
    Notices,
}
/// Invalid data leaves the accepted view and outstanding work intact.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Invalid {
    Identity,
    Limit,
    Revision,
    State,
}
impl std::fmt::Display for Invalid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid status descriptor: {self:?}")
    }
}
impl std::error::Error for Invalid {}
fn id(s: &str) -> Result<(), Invalid> {
    if s.is_empty() || s.len() > 128 || s.chars().any(char::is_control) {
        Err(Invalid::Identity)
    } else {
        Ok(())
    }
}
fn text(s: &str, limit: usize, total: &mut usize) -> Result<(), Invalid> {
    *total = total.saturating_add(s.len());
    if s.len() > limit || *total > 4 * 1024 * 1024 {
        Err(Invalid::Limit)
    } else {
        Ok(())
    }
}
fn action(a: Option<&Action>, total: &mut usize) -> Result<(), Invalid> {
    if let Some(a) = a {
        id(&a.id)?;
        text(&a.id, 128, total)?;
        text(&a.label, 4096, total)?;
        text(&a.pending, 4096, total)?;
        if let Some(s) = &a.disabled {
            text(s, 65536, total)?;
        }
    }
    Ok(())
}
pub(super) fn validate(s: &Snapshot) -> Result<(), Invalid> {
    if s.sections.len() > 32
        || s.notices.len() > 128
        || s.sections.iter().map(|s| s.rows.len()).sum::<usize>() > 2048
    {
        return Err(Invalid::Limit);
    }
    let mut total = 0;
    let mut ids = HashSet::new();
    for section in &s.sections {
        id(&section.id)?;
        if !ids.insert(&section.id) {
            return Err(Invalid::Identity);
        }
        text(&section.id, 128, &mut total)?;
        text(&section.title, 4096, &mut total)?;
        text(&section.summary, 4096, &mut total)?;
        text(&section.detail, 65536, &mut total)?;
        if section.bar.is_some_and(|n| n > 10)
            || section.state != State::Ready && !section.rows.is_empty()
        {
            return Err(Invalid::State);
        }
        action(section.action.as_ref(), &mut total)?;
        action(section.retry.as_ref(), &mut total)?;
        let mut rows = HashSet::new();
        for r in &section.rows {
            id(&r.id)?;
            if !rows.insert(&r.id) {
                return Err(Invalid::Identity);
            }
            text(&r.id, 128, &mut total)?;
            text(&r.label, 4096, &mut total)?;
            text(&r.detail, 65536, &mut total)?;
            action(r.action.as_ref(), &mut total)?;
        }
    }
    let mut notices = HashSet::new();
    for n in &s.notices {
        id(&n.id)?;
        if !notices.insert(&n.id) {
            return Err(Invalid::Identity);
        }
        text(&n.id, 128, &mut total)?;
        text(&n.label, 4096, &mut total)?;
        text(&n.detail, 65536, &mut total)?;
        text(&n.timestamp, 4096, &mut total)?;
        action(n.action.as_ref(), &mut total)?;
    }
    Ok(())
}
