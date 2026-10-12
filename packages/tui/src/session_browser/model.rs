//! Host session facts, operation payloads and atomic snapshot validation.

use crate::picker::Choice;
pub use crate::review_ticket::Ticket;
use std::{
    collections::{HashMap, HashSet},
    fmt,
};

/// Offered row operation; UI never performs it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    /// Resume the selected conversation.
    Open,
    /// Change its title.
    Rename,
    /// Create a host-owned fork.
    Fork,
    /// Move to a supplied destination.
    Move,
    /// Pin or unpin in host order.
    Pin,
    /// Delete after safe confirmation.
    Delete,
}
impl Kind {
    /// Pending label, never an assertion of success.
    #[must_use]
    pub const fn pending(self) -> &'static str {
        match self {
            Self::Open => "opening",
            Self::Rename => "renaming",
            Self::Fork => "forking",
            Self::Move => "moving",
            Self::Pin => "pinning",
            Self::Delete => "deleting",
        }
    }
    /// Discoverable shortcut.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Open => "enter",
            Self::Rename => "r",
            Self::Fork => "f",
            Self::Move => "m",
            Self::Pin => "p",
            Self::Delete => "d",
        }
    }
}
/// One offered operation, including an optional disabled reason.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Offered {
    /// Operation identity.
    pub kind: Kind,
    /// Host wording for menus and hints.
    pub label: String,
    /// Readable reason, never executable while set.
    pub disabled: Option<String>,
    /// Full host loss wording, required for Delete.
    pub confirmation: String,
}
impl Offered {
    /// Enabled operation with an explicit host label.
    #[must_use]
    pub fn new(kind: Kind, label: &str) -> Self {
        Self {
            kind,
            label: label.into(),
            disabled: None,
            confirmation: String::new(),
        }
    }
}
/// Semantic color for host state wording, independent of that wording.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tone {
    /// Ordinary supplied fact.
    #[default]
    Normal,
    /// Work is in progress.
    Working,
    /// Human action is needed.
    Attention,
    /// Host reported failure.
    Failed,
    /// Host reported completion.
    Done,
}
/// Stable host session row. Storage and actual Chat instances remain caller-owned.
#[derive(Clone, Debug)]
pub struct Session {
    /// Stable unique ID.
    pub id: String,
    /// Material revision; a changed row invalidates its outstanding operation.
    pub revision: u64,
    /// Session title.
    pub title: String,
    /// Project display label.
    pub project: String,
    /// Issue/PR/ref display label.
    pub reference: String,
    /// Host group; order is exactly the supplied order.
    pub group: String,
    /// Optional fork parent. One fork level is drawn.
    pub parent: Option<String>,
    /// Host pin state, never inferred from group wording.
    pub pinned: bool,
    /// Last activity summary.
    pub activity: String,
    /// Host state text.
    pub state: String,
    /// Semantic state role.
    pub tone: Tone,
    /// Host age/date text, not a computed clock.
    pub age: String,
    /// Read-only loaded preview.
    pub preview: String,
    /// Loaded message text used for literal local search.
    pub messages: String,
    /// Offered actions, in menu order.
    pub actions: Vec<Offered>,
    /// Host-supplied move destinations.
    pub destinations: Vec<Choice>,
    /// Rename character limit; resource limit remains 4 KiB.
    pub rename_limit: usize,
}
impl Session {
    /// Minimal row; no implicit operation is offered.
    #[must_use]
    pub fn new(id: &str, title: &str) -> Self {
        Self {
            id: id.into(),
            revision: 0,
            title: title.into(),
            project: String::new(),
            reference: String::new(),
            group: String::new(),
            parent: None,
            pinned: false,
            activity: String::new(),
            state: String::new(),
            tone: Tone::Normal,
            age: String::new(),
            preview: String::new(),
            messages: String::new(),
            actions: vec![],
            destinations: vec![],
            rename_limit: 80,
        }
    }
}
/// Host loading state; retained rows remain usable.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Status {
    /// Loaded, including a genuinely empty scope.
    #[default]
    Ready,
    /// Waiting for host discovery.
    Loading,
    /// Failed discovery or paging, explicit retry only.
    Failed(String),
}
/// Whole authoritative host projection.
#[derive(Clone, Debug)]
pub struct Snapshot {
    /// Higher revisions replace earlier snapshots atomically.
    pub revision: u64,
    /// Display scope.
    pub scope: String,
    /// Rows and groups in host order.
    pub rows: Vec<Session>,
    /// Host load state.
    pub status: Status,
    /// Host empty explanation.
    pub empty: String,
    /// Whether the final Search all row is available.
    pub search_all: bool,
    /// Opaque next-page cursor. The UI returns it unchanged.
    pub more: Option<String>,
}
impl Snapshot {
    /// Ready projection with no implicit remote capabilities.
    #[must_use]
    pub fn new(revision: u64, scope: &str, rows: Vec<Session>) -> Self {
        Self {
            revision,
            scope: scope.into(),
            rows,
            status: Status::Ready,
            empty: format!("No conversations in {scope}. Return to Chat to start one."),
            search_all: false,
            more: None,
        }
    }
}
/// Immutable operation submitted once to the host.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    /// Exact request instance.
    pub ticket: Ticket,
    /// Stable target session.
    pub id: String,
    /// Target material revision.
    pub revision: u64,
    /// Typed operation.
    pub kind: Kind,
    /// Rename text, destination ID or `true`/`false` pin value; empty otherwise.
    pub value: String,
}
/// Discovery request, independent of row operations.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Discovery {
    /// Exact request instance.
    pub ticket: Ticket,
    /// Snapshot revision at request time.
    pub revision: u64,
    /// Current query, empty for ordinary loading.
    pub query: String,
    /// Supplied paging cursor, empty for retry/search.
    pub cursor: String,
    /// True only for remote search.
    pub search: bool,
}
/// Actual host outcome. Invalid combinations never consume a pending ticket.
pub enum Outcome {
    /// Host opened this session; caller may switch retained Chat instances.
    Opened,
    /// Host changed rows/order; supplies a complete higher revision.
    Applied(Snapshot),
    /// Host created a fork and supplies its actual stable ID.
    Forked { snapshot: Snapshot, id: String },
    /// Failed/refused action with safe display reason.
    Failed(String),
}
/// Atomic descriptor error.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Invalid {
    /// Missing, duplicate or controlled identity.
    Identity,
    /// Parent cycle.
    Parent,
    /// Invalid action/default or rename limit.
    Contract,
    /// Bounded resource budget exceeded.
    Limit,
}
impl fmt::Display for Invalid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid session snapshot: {self:?}")
    }
}
impl std::error::Error for Invalid {}
pub(super) fn identity(id: &str) -> bool {
    !id.is_empty() && id.len() <= 128 && !id.chars().any(char::is_control)
}
pub(super) fn same_material(a: &Session, b: &Session) -> bool {
    a.title == b.title
        && a.project == b.project
        && a.parent == b.parent
        && a.pinned == b.pinned
        && a.rename_limit == b.rename_limit
        && a.actions == b.actions
        && a.destinations.len() == b.destinations.len()
        && a.destinations.iter().zip(&b.destinations).all(|(a, b)| {
            a.id == b.id
                && a.label == b.label
                && a.detail == b.detail
                && a.disabled == b.disabled
                && a.danger == b.danger
                && a.current == b.current
                && a.group == b.group
        })
}
pub(super) fn validate(s: &Snapshot) -> Result<(), Invalid> {
    if s.rows.len() > 4096 {
        return Err(Invalid::Limit);
    }
    let mut ids = HashSet::new();
    let mut bytes = s.scope.len() + s.empty.len() + s.more.as_ref().map_or(0, String::len);
    if let Status::Failed(reason) = &s.status {
        bytes += reason.len();
    }
    for r in &s.rows {
        if !identity(&r.id)
            || !ids.insert(&r.id)
            || r.parent.as_ref().is_some_and(|id| !identity(id))
        {
            return Err(Invalid::Identity);
        }
        if r.rename_limit == 0 || r.rename_limit > 4096 {
            return Err(Invalid::Contract);
        }
        if r.preview.len() > 65536
            || r.messages.len() > 65536
            || r.title.len() > 4096
            || r.destinations.len() > 256
        {
            return Err(Invalid::Limit);
        }
        bytes += r.id.len()
            + r.title.len()
            + r.project.len()
            + r.reference.len()
            + r.group.len()
            + r.parent.as_ref().map_or(0, String::len)
            + r.activity.len()
            + r.state.len()
            + r.age.len()
            + r.preview.len()
            + r.messages.len();
        let mut kinds = HashSet::new();
        for a in &r.actions {
            if a.confirmation
                .chars()
                .any(|c| c.is_control() && c != '\n' && c != '\t')
            {
                return Err(Invalid::Contract);
            }
            if !kinds.insert(a.kind)
                || a.label.is_empty()
                || (a.kind == Kind::Delete && a.confirmation.is_empty())
            {
                return Err(Invalid::Contract);
            }
            if a.confirmation.len() > 65536 {
                return Err(Invalid::Limit);
            }
            bytes +=
                a.label.len() + a.confirmation.len() + a.disabled.as_ref().map_or(0, String::len);
        }
        let mut destinations = HashSet::new();
        for c in &r.destinations {
            if !identity(&c.id) || !destinations.insert(&c.id) {
                return Err(Invalid::Identity);
            }
            bytes += c.id.len()
                + c.label.len()
                + c.detail.len()
                + c.group.len()
                + c.disabled.as_ref().map_or(0, String::len);
        }
        if bytes > 8 * 1024 * 1024 {
            return Err(Invalid::Limit);
        }
    }
    let by_id = s
        .rows
        .iter()
        .map(|r| (r.id.as_str(), r))
        .collect::<HashMap<_, _>>();
    let mut finished = HashSet::new();
    for r in &s.rows {
        let mut seen = HashSet::new();
        let mut row = Some(r);
        while let Some(r) = row {
            if finished.contains(r.id.as_str()) {
                break;
            }
            if !seen.insert(&r.id) {
                return Err(Invalid::Parent);
            }
            row = r
                .parent
                .as_ref()
                .and_then(|id| by_id.get(id.as_str()).copied());
        }
        finished.extend(seen.into_iter().map(String::as_str));
    }
    if bytes > 8 * 1024 * 1024 {
        return Err(Invalid::Limit);
    }
    Ok(())
}
