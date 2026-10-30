//! Stable tree navigation, host discovery and acknowledged actions without I/O.

mod input;
mod model;
mod view;
const CAPACITY: &str = "too many pending requests; wait for a host result";
use crate::{editor::Editor, picker::ChoicePicker};
pub use model::{
    Action, Children, Invalid, Kind, Node, Operation, Outcome, Placement, Request, Snapshot, State,
    Ticket,
};
use std::{
    collections::{HashMap, HashSet},
    time::Duration,
};

/// An addressable node or branch/root retry/page row.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Cursor {
    Node(String),
    Children(Option<String>),
}
/// Returned work or a caller focus/lifetime transition.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Intent {
    Request(Request),
    Opened(String),
    MainView,
    Returned,
}
struct Pending {
    request: Request,
    elapsed: Duration,
}
struct Menu {
    node: String,
    revision: u64,
    picker: ChoicePicker,
}
#[derive(Clone)]
struct Row {
    key: Option<Cursor>,
    parent: Option<String>,
    depth: usize,
    text: String,
}
/// Caller-owned reusable tree state. Rendering never performs host work.
pub struct TreeBrowser {
    snapshot: Snapshot,
    skipped: usize,
    folded: HashSet<String>,
    cascade: HashSet<String>,
    cursor: Option<Cursor>,
    opened: Option<String>,
    query: Editor,
    filtering: bool,
    focused: bool,
    placement: Placement,
    pending: HashMap<Option<String>, Pending>,
    failures: HashMap<Option<String>, String>,
    load_failures: HashMap<Option<String>, String>,
    attempts: HashMap<Option<String>, usize>,
    menu: Option<Menu>,
    intents: Vec<Intent>,
    notice: String,
    offset: usize,
    details_offset: usize,
    viewport: usize,
    closed: bool,
}
impl TreeBrowser {
    /// Validate and normalize a supplied forest.
    /// # Errors
    /// Rejects malformed material or resource overflow before constructing state.
    pub fn new(snapshot: Snapshot, placement: Placement) -> Result<Self, Invalid> {
        let (snapshot, skipped) = model::normalize(snapshot)?;
        let mut query = Editor::new(4096);
        query.set_reject_overflow(true);
        let mut tree = Self {
            snapshot,
            skipped,
            folded: HashSet::new(),
            cascade: HashSet::new(),
            cursor: None,
            opened: None,
            query,
            filtering: false,
            focused: true,
            placement,
            pending: HashMap::new(),
            failures: HashMap::new(),
            load_failures: HashMap::new(),
            attempts: HashMap::new(),
            menu: None,
            intents: vec![],
            notice: String::new(),
            offset: 0,
            details_offset: 0,
            viewport: 1,
            closed: false,
        };
        tree.repair(&[]);
        Ok(tree)
    }
    #[must_use]
    pub fn snapshot(&self) -> &Snapshot {
        &self.snapshot
    }
    #[must_use]
    pub fn cursor(&self) -> Option<&Cursor> {
        self.cursor.as_ref()
    }
    #[must_use]
    pub fn opened(&self) -> Option<&str> {
        self.opened.as_deref()
    }
    /// Whether this exact delivered request still belongs to the current view.
    #[must_use]
    pub fn pending(&self, request: &Request) -> bool {
        !self.closed
            && self
                .pending
                .get(&request.node)
                .is_some_and(|p| p.request == *request)
    }
    #[must_use]
    pub fn query(&self) -> &str {
        self.query.text()
    }
    #[must_use]
    pub fn skipped(&self) -> usize {
        self.skipped
    }
    #[must_use]
    pub fn expanded(&self, id: &str) -> bool {
        self.folded.contains(id)
    }
    #[must_use]
    pub fn focused(&self) -> bool {
        self.focused
    }
    /// The host routes events only to the focused view.
    pub fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }
    /// Host's currently inspected node; independent from keyboard selection.
    /// # Errors
    /// Unknown nodes cannot become the open mark.
    pub fn set_opened(&mut self, id: Option<&str>) -> Result<(), Invalid> {
        if id.is_some_and(|id| self.node(id).is_none()) {
            return Err(Invalid::Identity);
        }
        self.opened = id.map(str::to_owned);
        Ok(())
    }
    /// Select a visible address, without executing it.
    pub fn select(&mut self, cursor: Cursor) -> bool {
        if self.rows().iter().any(|r| r.key.as_ref() == Some(&cursor)) {
            self.cursor = Some(cursor);
            self.details_offset = 0;
            true
        } else {
            false
        }
    }
    /// Take undrained intents once.
    pub fn take_intents(&mut self) -> Vec<Intent> {
        std::mem::take(&mut self.intents)
    }
    /// Inject elapsed time; no internal clock or automatic retry exists.
    pub fn tick(&mut self, elapsed: Duration) {
        for p in self.pending.values_mut() {
            p.elapsed = p.elapsed.saturating_add(elapsed);
        }
    }
    /// Close and invalidate all work still owned by this view.
    pub fn close(&mut self) {
        self.closed = true;
        self.pending.clear();
        self.intents.clear();
        self.menu = None;
        self.cascade.clear();
    }
    fn node(&self, id: &str) -> Option<&Node> {
        self.snapshot.nodes.iter().find(|n| n.id == id)
    }
    fn selected_node(&self) -> Option<&Node> {
        match &self.cursor {
            Some(Cursor::Node(id)) => self.node(id),
            _ => None,
        }
    }
    fn ancestry(&self) -> Vec<String> {
        let mut id = match &self.cursor {
            Some(Cursor::Node(id)) => Some(id.as_str()),
            Some(Cursor::Children(id)) => id.as_deref(),
            None => None,
        };
        let mut out = vec![];
        while let Some(key) = id {
            out.push(key.to_owned());
            id = self.node(key).and_then(|n| n.parent.as_deref());
        }
        out
    }
    fn repair(&mut self, ancestors: &[String]) {
        let keys: Vec<_> = self.rows().into_iter().filter_map(|r| r.key).collect();
        if self.cursor.as_ref().is_some_and(|key| keys.contains(key)) {
            return;
        }
        self.cursor = ancestors
            .iter()
            .map(|id| Cursor::Node(id.clone()))
            .find(|key| keys.contains(key))
            .or_else(|| keys.first().cloned());
        self.details_offset = 0;
    }
    /// Adopt an authoritative refresh atomically and preserve surviving folds/IDs.
    /// # Errors
    /// Stale or malformed revisions leave data and tickets unchanged.
    pub fn replace(&mut self, snapshot: Snapshot) -> Result<(), Invalid> {
        let (snapshot, skipped) = model::normalize(snapshot)?;
        if self.closed || snapshot.revision <= self.snapshot.revision {
            return Err(Invalid::Revision);
        }
        for n in &snapshot.nodes {
            if let Some(old) = self.node(&n.id)
                && (n.revision < old.revision || (n.revision == old.revision && !n.material(old)))
            {
                return Err(Invalid::Revision);
            }
        }
        let ancestors = self.ancestry();
        let before = self.cursor.clone();
        let old = std::mem::replace(&mut self.snapshot, snapshot);
        self.skipped = skipped;
        let valid: HashSet<_> = self.snapshot.nodes.iter().map(|n| n.id.clone()).collect();
        self.folded.retain(|id| valid.contains(id));
        self.cascade.retain(|id| valid.contains(id));
        if self.opened.as_ref().is_some_and(|id| !valid.contains(id)) {
            self.opened = None;
        }
        self.pending.retain(|id, p| match id {
            None => false,
            Some(id) => self.snapshot.nodes.iter().any(|n| {
                n.id == *id
                    && n.revision == p.request.revision
                    && (!matches!(p.request.operation, Operation::Load { .. })
                        || old.nodes.iter().any(|prior| {
                            prior.id == *id && prior.children == n.children && prior.more == n.more
                        }))
            }),
        });
        self.attempts
            .retain(|id, _| id.as_ref().is_none_or(|id| valid.contains(id)));
        self.failures.retain(|id, _| {
            id.as_ref().is_some_and(|id| {
                self.snapshot.nodes.iter().any(|n| {
                    n.id == *id
                        && old
                            .nodes
                            .iter()
                            .any(|prior| prior.id == *id && prior.revision == n.revision)
                })
            })
        });
        self.load_failures.retain(|id, _| {
            id.as_ref().is_some_and(|id| {
                self.snapshot.nodes.iter().any(|n| {
                    n.id == *id
                        && old.nodes.iter().any(|prior| {
                            prior.id == *id
                                && prior.revision == n.revision
                                && prior.children == n.children
                                && prior.more == n.more
                        })
                })
            })
        });
        if self.menu.as_ref().is_some_and(|m| {
            !self
                .snapshot
                .nodes
                .iter()
                .any(|n| n.id == m.node && n.revision == m.revision)
        }) {
            self.menu = None;
        }
        self.retain_requests();
        self.repair(&ancestors);
        if before != self.cursor {
            self.notice = "node removed or hidden; moved to surviving ancestor".into();
        }
        self.expand_cascades();
        self.clear_capacity_notice();
        Ok(())
    }
    fn retain_requests(&mut self) {
        self.intents.retain(|i| match i {
            Intent::Request(r) => self.pending.get(&r.node).is_some_and(|p| p.request == *r),
            _ => true,
        });
    }
    fn clear_capacity_notice(&mut self) {
        if self.pending.len() < 256 && self.notice == CAPACITY {
            self.notice.clear();
        }
    }
    /// Apply an exact outstanding request once. Invalid answers remain pending.
    pub fn resolve(&mut self, request: &Request, outcome: Outcome) -> bool {
        if self.closed
            || !self
                .pending
                .get(&request.node)
                .is_some_and(|p| p.request == *request)
        {
            return false;
        }
        match outcome {
            Outcome::Loaded(snapshot) if matches!(request.operation, Operation::Load { .. }) => {
                let state = request.node.as_ref().map_or(Some(&snapshot.roots), |id| {
                    snapshot
                        .nodes
                        .iter()
                        .find(|n| n.id == *id)
                        .map(|n| &n.children)
                });
                if !state.is_some_and(|c| matches!(c, Children::Ready | Children::Failed(_))) {
                    return false;
                }
                if self.replace(snapshot).is_err() {
                    return false;
                }
            }
            Outcome::Applied(snapshot) if matches!(request.operation, Operation::Action(_)) => {
                if self.replace(snapshot).is_err() {
                    return false;
                }
            }
            Outcome::Opened => {
                let Some(id) = &request.node else {
                    return false;
                };
                let Operation::Action(action) = &request.operation else {
                    return false;
                };
                if !self
                    .node(id)
                    .is_some_and(|n| n.actions.iter().any(|a| a.id == *action && a.opens))
                {
                    return false;
                }
                self.opened = Some(id.clone());
                self.intents.push(Intent::Opened(id.clone()));
            }
            Outcome::Failed(reason) => {
                if reason.len() > 65536 {
                    return false;
                }
                if matches!(request.operation, Operation::Load { .. }) {
                    self.load_failures.insert(request.node.clone(), reason);
                } else {
                    self.failures.insert(request.node.clone(), reason);
                }
            }
            _ => return false,
        }
        self.pending.remove(&request.node);
        self.retain_requests();
        let ancestors = self.ancestry();
        self.repair(&ancestors);
        self.expand_cascades();
        self.clear_capacity_notice();
        true
    }
    fn request(&mut self, node: Option<String>, operation: Operation) {
        if self.closed || self.pending.contains_key(&node) {
            return;
        }
        if self.pending.len() >= 256 {
            self.notice = CAPACITY.into();
            return;
        }
        let revision = node
            .as_deref()
            .and_then(|id| self.node(id))
            .map_or(self.snapshot.revision, |n| n.revision);
        let r = Request {
            ticket: Ticket::new(),
            node: node.clone(),
            revision,
            operation,
        };
        if matches!(r.operation, Operation::Load { .. }) {
            self.load_failures.remove(&node);
        } else {
            self.failures.remove(&node);
        }
        self.pending.insert(
            node,
            Pending {
                request: r.clone(),
                elapsed: Duration::ZERO,
            },
        );
        self.intents.push(Intent::Request(r));
    }
    fn load(&mut self, id: Option<String>, cursor: Option<String>) {
        if self.pending.contains_key(&id) {
            return;
        }
        self.request(id.clone(), Operation::Load { cursor });
        if self.pending.contains_key(&id) {
            let attempts = self.attempts.entry(id).or_default();
            *attempts = attempts.saturating_add(1);
        }
    }
    fn action(&mut self, id: &str, action: &str) {
        if self.node(id).is_some_and(|n| {
            n.actions
                .iter()
                .any(|a| a.id == action && a.disabled.is_none())
        }) {
            self.request(Some(id.into()), Operation::Action(action.into()));
        }
    }
    fn open_branch(&mut self, id: &str) {
        let Some(n) = self.node(id) else {
            return;
        };
        if !n.has_children {
            return;
        }
        let unloaded = matches!(n.children, Children::Unloaded);
        self.folded.insert(id.into());
        if unloaded && !self.load_failures.contains_key(&Some(id.into())) {
            self.load(Some(id.into()), None);
        }
    }
    fn below(&self, id: &str, ancestor: &str) -> bool {
        let mut cursor = Some(id);
        while let Some(key) = cursor {
            if key == ancestor {
                return true;
            }
            cursor = self.node(key).and_then(|n| n.parent.as_deref());
        }
        false
    }
    fn collapse(&mut self, id: &str, all: bool) {
        let descendants: HashSet<_> = self
            .snapshot
            .nodes
            .iter()
            .filter(|n| self.below(&n.id, id))
            .map(|n| n.id.clone())
            .collect();
        let overlapping: HashSet<_> = self
            .cascade
            .iter()
            .filter(|root| self.below(id, root) || descendants.contains(*root))
            .cloned()
            .collect();
        self.cascade.retain(|key| !overlapping.contains(key));
        if all {
            self.folded.retain(|key| !descendants.contains(key));
        } else {
            self.folded.remove(id);
        }
        let hidden = match &self.cursor {
            Some(Cursor::Node(key)) => key != id && descendants.contains(key),
            Some(Cursor::Children(Some(key))) => descendants.contains(key),
            _ => false,
        };
        if hidden {
            self.cursor = Some(Cursor::Node(id.into()));
        }
    }
    fn expand_cascades(&mut self) {
        let branches: Vec<_> = self
            .snapshot
            .nodes
            .iter()
            .filter(|n| n.has_children && self.cascade.iter().any(|id| self.below(&n.id, id)))
            .map(|n| n.id.clone())
            .collect();
        for id in branches {
            self.open_branch(&id);
        }
    }
}
