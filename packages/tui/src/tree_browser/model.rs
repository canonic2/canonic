//! Host descriptors, bounded validation and safe topology normalization.

pub use crate::review_ticket::Ticket;
use std::collections::{HashMap, HashSet};

/// Supplied branch/root discovery state.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Children {
    /// No child discovery has happened.
    Unloaded,
    /// Caller is discovering children independently.
    Loading(String),
    /// Loaded children, optionally followed by another page.
    #[default]
    Ready,
    /// Explicitly retryable discovery failure.
    Failed(String),
}
/// Semantic glyph family; names and facts remain host-owned.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Kind {
    #[default]
    File,
    Directory,
    Task,
}
/// Host's state, never inferred from descendants.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum State {
    #[default]
    Plain,
    Modified,
    Added,
    Deleted,
    Renamed,
    Untracked,
    Ignored,
    Working,
    Done,
    Failed,
    Queued,
    Cancelled,
    Interrupted,
}
/// One supplied action. ID and shortcut are local to its node.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Action {
    pub id: String,
    pub label: String,
    pub pending: String,
    pub shortcut: Option<char>,
    pub disabled: Option<String>,
    /// Open/Select acknowledgement may move the main-view mark.
    pub opens: bool,
}
impl Action {
    #[must_use]
    pub fn new(id: &str, label: &str) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            pending: label.into(),
            shortcut: None,
            disabled: None,
            opens: false,
        }
    }
}
/// One stable host node. Flat input order determines sibling order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Node {
    pub id: String,
    pub revision: u64,
    pub parent: Option<String>,
    pub name: String,
    pub kind: Kind,
    pub state: State,
    pub detail: String,
    pub has_children: bool,
    pub children: Children,
    pub more: Option<String>,
    pub actions: Vec<Action>,
    /// Action used by Enter; absent means no primary operation.
    pub primary: Option<String>,
}
impl Node {
    #[must_use]
    pub fn new(id: &str, name: &str, parent: Option<&str>) -> Self {
        Self {
            id: id.into(),
            revision: 1,
            parent: parent.map(str::to_owned),
            name: name.into(),
            kind: Kind::File,
            state: State::Plain,
            detail: String::new(),
            has_children: false,
            children: Children::Ready,
            more: None,
            actions: vec![],
            primary: None,
        }
    }
    pub(super) fn material(&self, other: &Self) -> bool {
        self.parent == other.parent
            && self.name == other.name
            && self.kind == other.kind
            && self.has_children == other.has_children
            && self.actions == other.actions
            && self.primary == other.primary
    }
}
/// Whole authoritative tree snapshot, including the root discovery state.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub revision: u64,
    pub title: String,
    pub summary: String,
    pub nodes: Vec<Node>,
    pub roots: Children,
    pub more: Option<String>,
}
/// Explicit placement for focus/return intents; host determines geometry.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Placement {
    #[default]
    FullScreen,
    Panel,
}
/// Discovery or a supplied node action.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Operation {
    Load { cursor: Option<String> },
    Action(String),
}
/// Exact request identity; clone it to answer once.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    pub ticket: Ticket,
    pub node: Option<String>,
    pub revision: u64,
    pub operation: Operation,
}
/// Host result. A generic action cannot move the open mark.
#[derive(Clone, Debug)]
pub enum Outcome {
    Loaded(Snapshot),
    Applied(Snapshot),
    Opened,
    Failed(String),
}
/// Invalid material leaves the previous tree intact.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Invalid {
    Limit,
    Identity,
    Action,
    Revision,
}
impl std::fmt::Display for Invalid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid tree descriptor: {self:?}")
    }
}
impl std::error::Error for Invalid {}
pub(super) fn identity(s: &str) -> bool {
    !s.is_empty() && s.len() <= 128 && !s.chars().any(char::is_control)
}
fn text(s: &str, max: usize, total: &mut usize) -> Result<(), Invalid> {
    *total = total.saturating_add(s.len());
    if s.len() > max || *total > 8 * 1024 * 1024 {
        Err(Invalid::Limit)
    } else {
        Ok(())
    }
}
fn children(c: &Children, total: &mut usize) -> Result<(), Invalid> {
    if let Children::Loading(s) | Children::Failed(s) = c {
        text(s, 65536, total)?;
    }
    Ok(())
}
/// Returns a normalized forest and the number of skipped host nodes.
pub(super) fn normalize(mut s: Snapshot) -> Result<(Snapshot, usize), Invalid> {
    if s.nodes.len() > 4096 {
        return Err(Invalid::Limit);
    }
    let mut total = 0;
    text(&s.title, 4096, &mut total)?;
    text(&s.summary, 65536, &mut total)?;
    children(&s.roots, &mut total)?;
    if let Some(cursor) = &s.more {
        text(cursor, 4096, &mut total)?;
    }
    for n in &s.nodes {
        if !identity(&n.id) || n.parent.as_deref().is_some_and(|id| !identity(id)) {
            return Err(Invalid::Identity);
        }
        text(&n.id, 128, &mut total)?;
        text(&n.name, 4096, &mut total)?;
        text(&n.detail, 65536, &mut total)?;
        if let Some(parent) = &n.parent {
            text(parent, 128, &mut total)?;
        }
        if let Some(cursor) = &n.more {
            text(cursor, 4096, &mut total)?;
        }
        children(&n.children, &mut total)?;
        if n.actions.len() > 64 {
            return Err(Invalid::Limit);
        }
        let mut ids = HashSet::new();
        let mut keys = HashSet::new();
        for a in &n.actions {
            if !identity(&a.id)
                || !ids.insert(&a.id)
                || a.shortcut
                    .is_some_and(|c| !c.is_ascii_lowercase() || c == 'a' || !keys.insert(c))
            {
                return Err(Invalid::Action);
            }
            text(&a.label, 4096, &mut total)?;
            text(&a.id, 128, &mut total)?;
            text(&a.pending, 4096, &mut total)?;
            if let Some(reason) = &a.disabled {
                text(reason, 65536, &mut total)?;
            }
        }
        if n.primary.as_ref().is_some_and(|id| !ids.contains(id)) {
            return Err(Invalid::Action);
        }
        if let Some(primary) = &n.primary {
            text(primary, 128, &mut total)?;
        }
        if !n.has_children && (!matches!(n.children, Children::Ready) || n.more.is_some()) {
            return Err(Invalid::Action);
        }
    }
    let original = s.nodes.len();
    let mut seen = HashSet::new();
    s.nodes.retain(|n| seen.insert(n.id.clone()));
    let map: HashMap<_, _> = s.nodes.iter().map(|n| (n.id.as_str(), n)).collect();
    let valid: HashSet<_> = s
        .nodes
        .iter()
        .filter(|n| {
            let mut cursor = Some(n.id.as_str());
            let mut path = HashSet::new();
            while let Some(id) = cursor {
                if path.len() >= 256 || !path.insert(id) {
                    return false;
                }
                let Some(row) = map.get(id) else {
                    return false;
                };
                if id != n.id && !row.has_children {
                    return false;
                }
                cursor = row.parent.as_deref();
            }
            true
        })
        .map(|n| n.id.clone())
        .collect();
    s.nodes.retain(|n| valid.contains(&n.id));
    let skipped = original - s.nodes.len();
    Ok((s, skipped))
}
