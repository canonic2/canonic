//! Stable-ID session navigation and acknowledged host requests; no storage or I/O.

mod input;
mod model;
mod view;
use crate::{
    dialog::{Confirmation, Stack},
    editor::Editor,
    picker::ChoicePicker,
};
pub use model::{
    Discovery, Invalid, Kind, Offered, Outcome, Request, Session, Snapshot, Status, Ticket, Tone,
};
use std::{collections::HashMap, time::Duration};

/// View and host operations remain separate; only actual results switch sessions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Intent {
    /// One identified row operation.
    Action(Request),
    /// Load/retry/page/search request. Hosts return full snapshots.
    Discover(Discovery),
    /// Return to the originating Chat without changing it.
    Returned(String),
    /// Actual Open acknowledgement; host selects this retained Chat instance.
    Opened(String),
}
struct Pending {
    request: Request,
    elapsed: Duration,
}
struct Failure {
    revision: u64,
    kind: Kind,
    value: String,
    reason: String,
}
struct Rename {
    id: String,
    revision: u64,
    editor: Editor,
}
enum MenuKind {
    Actions(Vec<Kind>),
    Move,
}
struct Menu {
    id: String,
    revision: u64,
    picker: ChoicePicker,
    kind: MenuKind,
}
struct DeleteCheck {
    id: String,
    revision: u64,
    text: String,
    delete: bool,
    confirmation: Confirmation,
    offset: usize,
    viewport: usize,
    total: usize,
}

/// Reusable dedicated screen receiving caller events, frames and elapsed time.
pub struct SessionBrowser {
    snapshot: Snapshot,
    remote: Option<Snapshot>,
    origin: String,
    selected: Option<String>,
    search_row: bool,
    query: Editor,
    searching: bool,
    search: Option<Discovery>,
    load: Option<Discovery>,
    search_error: String,
    load_cursor: String,
    load_failure: Option<String>,
    pending: HashMap<String, Pending>,
    failures: HashMap<String, Failure>,
    rename: Option<Rename>,
    menu: Option<Menu>,
    delete: Stack<DeleteCheck, u8>,
    intents: Vec<Intent>,
    notice: String,
    elapsed: Duration,
    offset: usize,
    viewport: usize,
    closed: bool,
}
impl SessionBrowser {
    /// Validate a whole snapshot and open on the caller's originating session ID.
    /// # Errors
    /// Invalid descriptors leave no partially constructed screen.
    pub fn new(snapshot: Snapshot, origin: &str) -> Result<Self, Invalid> {
        model::validate(&snapshot)?;
        if !model::identity(origin) {
            return Err(Invalid::Identity);
        }
        let selected = snapshot
            .rows
            .iter()
            .find(|r| r.id == origin)
            .or(snapshot.rows.first())
            .map(|r| r.id.clone());
        let mut query = Editor::new(4096);
        query.set_reject_overflow(true);
        Ok(Self {
            snapshot,
            remote: None,
            origin: origin.into(),
            selected,
            search_row: false,
            query,
            searching: false,
            search: None,
            load: None,
            search_error: String::new(),
            load_cursor: String::new(),
            load_failure: None,
            pending: HashMap::new(),
            failures: HashMap::new(),
            rename: None,
            menu: None,
            delete: Stack::new(vec![0]).map_err(|_| Invalid::Contract)?,
            intents: vec![],
            notice: String::new(),
            elapsed: Duration::ZERO,
            offset: 0,
            viewport: 1,
            closed: false,
        })
    }
    /// Authoritative loaded snapshot, without locally invented session facts.
    #[must_use]
    pub fn snapshot(&self) -> &Snapshot {
        self.remote.as_ref().unwrap_or(&self.snapshot)
    }
    /// Current stable row ID; None when empty or on Search all.
    #[must_use]
    pub fn selected(&self) -> Option<&str> {
        if self.search_row {
            None
        } else {
            self.selected.as_deref()
        }
    }
    /// Current filter text, independent of rename and Chat fields.
    #[must_use]
    pub fn query(&self) -> &str {
        self.query.text()
    }
    /// Whether a local return or acknowledged open closed this screen.
    #[must_use]
    pub fn closed(&self) -> bool {
        self.closed
    }
    /// Caller-owned informational feedback; this cannot complete an operation.
    pub fn set_notice(&mut self, text: &str) {
        self.notice = crate::text::clean(text, 4096, false);
    }
    /// Selected row's in-flight operation, if any.
    #[must_use]
    pub fn pending(&self, id: &str) -> Option<&Request> {
        self.pending.get(id).map(|p| &p.request)
    }
    /// Current full rename draft, including invalid over-limit text.
    #[must_use]
    pub fn rename_text(&self) -> Option<&str> {
        self.rename.as_ref().map(|r| r.editor.text())
    }
    /// Whether a delete Dialog is currently open.
    #[must_use]
    pub fn confirming(&self) -> bool {
        self.delete.ticket().is_some()
    }
    /// Exact visible ID order; None is the final Search all row.
    #[must_use]
    pub fn visible_ids(&self) -> Vec<Option<&str>> {
        let mut ids = self
            .visible()
            .iter()
            .map(|&i| Some(self.rows()[i].id.as_str()))
            .collect::<Vec<_>>();
        if self.search_available() {
            ids.push(None);
        }
        ids
    }
    fn rows(&self) -> &[Session] {
        self.remote
            .as_ref()
            .map_or(&self.snapshot.rows, |s| &s.rows)
    }
    fn row(&self, id: &str) -> Option<&Session> {
        self.rows()
            .iter()
            .find(|r| r.id == id)
            .or_else(|| self.snapshot.rows.iter().find(|r| r.id == id))
    }
    fn visible(&self) -> Vec<usize> {
        if self.remote.is_some() || self.query.text().is_empty() {
            return (0..self.rows().len()).collect();
        }
        let query = self.query.text().to_lowercase();
        let mut titles = vec![];
        let mut messages = vec![];
        for (i, r) in self.rows().iter().enumerate() {
            if r.title.to_lowercase().contains(&query) {
                titles.push(i);
            } else if r.messages.to_lowercase().contains(&query) {
                messages.push(i);
            }
        }
        titles.extend(messages);
        titles
    }
    fn search_available(&self) -> bool {
        self.snapshot.search_all && !self.query.text().is_empty() && self.remote.is_none()
    }
    fn material_revision(&self) -> u64 {
        self.remote.as_ref().map_or(self.snapshot.revision, |s| {
            s.revision.max(self.snapshot.revision)
        })
    }
    fn validate_new(&self, snapshot: &Snapshot) -> Result<(), Invalid> {
        model::validate(snapshot)?;
        for row in &snapshot.rows {
            if let Some(old) = self
                .row(&row.id)
                .or_else(|| self.snapshot.rows.iter().find(|r| r.id == row.id))
                && (row.revision < old.revision
                    || row.revision == old.revision && !model::same_material(old, row))
            {
                return Err(Invalid::Contract);
            }
        }
        Ok(())
    }
    fn end_remote(&mut self) {
        if let Some(remote) = self.remote.take() {
            self.snapshot.revision = self.snapshot.revision.max(remote.revision);
            for row in &mut self.snapshot.rows {
                if let Some(new) = remote.rows.iter().find(|r| r.id == row.id) {
                    row.clone_from(new);
                }
            }
        }
    }
    fn repair(&mut self, old: &[String]) {
        let visible = self
            .visible()
            .into_iter()
            .map(|i| self.rows()[i].id.clone())
            .collect::<Vec<_>>();
        if self
            .selected
            .as_ref()
            .is_some_and(|id| visible.contains(id))
        {
            return;
        }
        let previous = self.selected.take();
        let old_position = previous
            .as_ref()
            .and_then(|id| old.iter().position(|r| r == id));
        self.selected = old_position
            .and_then(|p| {
                old[p..]
                    .iter()
                    .find(|id| visible.contains(id))
                    .cloned()
                    .or_else(|| {
                        old[..p]
                            .iter()
                            .rev()
                            .find(|id| visible.contains(id))
                            .cloned()
                    })
            })
            .or_else(|| visible.first().cloned());
        self.search_row = self.selected.is_none() && self.search_available();
    }
    fn reconcile(&mut self, authoritative: bool) {
        let revisions = self
            .snapshot
            .rows
            .iter()
            .chain(self.remote.iter().flat_map(|s| s.rows.iter()))
            .map(|r| (r.id.clone(), r.revision))
            .collect::<HashMap<_, _>>();
        self.pending.retain(|id, p| {
            revisions
                .get(id)
                .map_or(!authoritative, |revision| *revision == p.request.revision)
        });
        self.failures
            .retain(|id, f| revisions.get(id) == Some(&f.revision));
        if self
            .rename
            .as_ref()
            .is_some_and(|r| revisions.get(&r.id) != Some(&r.revision))
        {
            self.rename = None;
            self.notice = "session changed · rename cancelled".into();
        }
        if self
            .menu
            .as_ref()
            .is_some_and(|m| revisions.get(&m.id) != Some(&m.revision))
        {
            self.menu = None;
        }
        if let Some(t) = self.delete.ticket().cloned() {
            let invalid = self
                .delete
                .body(&t)
                .is_some_and(|b| revisions.get(&b.id) != Some(&b.revision));
            if invalid {
                self.delete.clear();
                self.notice = "session changed · delete cancelled".into();
            }
        }
    }
    /// Adopt a newer authoritative projection, preserving IDs and invalidating material races.
    /// # Errors
    /// Invalid snapshots leave all old state/tickets intact. Older revisions return false.
    pub fn replace(&mut self, snapshot: Snapshot) -> Result<bool, Invalid> {
        self.validate_new(&snapshot)?;
        if self.closed || snapshot.revision <= self.material_revision() {
            return Ok(false);
        }
        let old = self
            .visible()
            .iter()
            .map(|&i| self.rows()[i].id.clone())
            .collect::<Vec<_>>();
        let removed = self
            .selected
            .as_ref()
            .filter(|id| !snapshot.rows.iter().any(|r| &r.id == *id))
            .cloned();
        self.snapshot = snapshot;
        self.remote = None;
        self.search = None;
        self.load = None;
        self.load_failure = None;
        self.search_error.clear();
        self.reconcile(true);
        self.repair(&old);
        if let Some(id) = removed {
            self.notice = format!("{id} was removed elsewhere · cursor moved on");
        }
        Ok(true)
    }
    /// Open a fresh view lifetime. Old tickets never become valid again.
    /// # Errors
    /// Invalid originating identity leaves the browser unchanged.
    pub fn reopen(&mut self, origin: &str) -> Result<(), Invalid> {
        if !model::identity(origin) {
            return Err(Invalid::Identity);
        }
        self.close_requests();
        self.intents.clear();
        self.origin = origin.into();
        self.closed = false;
        self.query.set_text("");
        self.searching = false;
        self.end_remote();
        self.notice.clear();
        self.failures.clear();
        self.selected = self
            .snapshot
            .rows
            .iter()
            .find(|r| r.id == origin)
            .or(self.snapshot.rows.first())
            .map(|r| r.id.clone());
        self.search_row = false;
        self.offset = 0;
        Ok(())
    }
    fn close_requests(&mut self) {
        self.pending.clear();
        self.search = None;
        self.load = None;
        self.load_cursor.clear();
        self.load_failure = None;
        self.rename = None;
        self.menu = None;
        self.delete.clear();
    }
    fn leave(&mut self, id: String, opened: bool) {
        self.closed = true;
        self.close_requests();
        self.intents.push(if opened {
            Intent::Opened(id)
        } else {
            Intent::Returned(id)
        });
    }
    /// Advance elapsed pending labels and destructive confirmation using the caller's clock.
    pub fn tick(&mut self, elapsed: Duration) {
        if self.closed {
            return;
        }
        self.elapsed = self.elapsed.saturating_add(elapsed);
        self.query.tick(elapsed);
        for p in self.pending.values_mut() {
            p.elapsed = p.elapsed.saturating_add(elapsed);
        }
        if let Some(r) = &mut self.rename {
            r.editor.tick(elapsed);
        }
        if let Some(t) = self.delete.ticket().cloned()
            && let Some(b) = self.delete.body_mut(&t)
        {
            b.confirmation.tick(elapsed);
        }
    }
    /// Drain current intents. Invalidated undelivered operations are removed.
    pub fn take_intents(&mut self) -> Vec<Intent> {
        std::mem::take(&mut self.intents)
            .into_iter()
            .filter(|i| match i {
                Intent::Action(r) => self.pending.get(&r.id).is_some_and(|p| p.request == *r),
                Intent::Discover(r) => {
                    self.search.as_ref() == Some(r) || self.load.as_ref() == Some(r)
                }
                Intent::Returned(_) | Intent::Opened(_) => true,
            })
            .collect()
    }
    /// Apply only one exact current row request; invalid result material is atomic.
    /// # Errors
    /// Malformed authoritative snapshots leave the operation pending for correction.
    pub fn complete(&mut self, request: &Request, outcome: Outcome) -> Result<bool, Invalid> {
        if self.closed
            || !self
                .pending
                .get(&request.id)
                .is_some_and(|p| p.request == *request)
        {
            return Ok(false);
        }
        match outcome {
            Outcome::Opened if request.kind == Kind::Open => {
                self.leave(request.id.clone(), true);
            }
            Outcome::Failed(reason) => {
                self.pending.remove(&request.id);
                self.failures.insert(
                    request.id.clone(),
                    Failure {
                        revision: request.revision,
                        kind: request.kind,
                        value: request.value.clone(),
                        reason: crate::text::clean(&reason, 4096, true),
                    },
                );
                if request.kind == Kind::Rename
                    && self.selected.as_deref() == Some(&request.id)
                    && self.rename.is_none()
                    && self.menu.is_none()
                    && !self.confirming()
                {
                    self.begin_rename(&request.id);
                }
            }
            Outcome::Applied(snapshot)
                if request.kind != Kind::Open && request.kind != Kind::Fork =>
            {
                self.validate_new(&snapshot)?;
                let row = snapshot.rows.iter().find(|r| r.id == request.id);
                if snapshot.revision <= self.material_revision()
                    || (request.kind == Kind::Delete && row.is_some())
                    || (request.kind != Kind::Delete
                        && row.is_none_or(|r| r.revision <= request.revision))
                {
                    return Ok(false);
                }
                self.pending.remove(&request.id);
                self.replace(snapshot)?;
                self.notice = format!(
                    "{} {}",
                    if request.kind == Kind::Delete {
                        "deleted"
                    } else {
                        "updated"
                    },
                    request.id
                );
            }
            Outcome::Forked { snapshot, id } if request.kind == Kind::Fork => {
                self.validate_new(&snapshot)?;
                if snapshot.revision <= self.material_revision()
                    || self.row(&id).is_some()
                    || !snapshot
                        .rows
                        .iter()
                        .any(|r| r.id == id && r.parent.as_deref() == Some(&request.id))
                {
                    return Ok(false);
                }
                self.pending.remove(&request.id);
                self.replace(snapshot)?;
                self.query.set_text("");
                self.searching = false;
                self.selected = Some(id);
                self.search_row = false;
                self.notice = "forked · rename it with r".into();
            }
            _ => return Ok(false),
        }
        Ok(true)
    }
    /// Complete exact current discovery. Newer queries, scopes and close invalidate it.
    /// # Errors
    /// Invalid descriptors leave current discovery pending.
    pub fn complete_discovery(
        &mut self,
        request: &Discovery,
        result: Result<Snapshot, String>,
    ) -> Result<bool, Invalid> {
        if self.closed
            || !(self.search.as_ref() == Some(request) || self.load.as_ref() == Some(request))
        {
            return Ok(false);
        }
        match result {
            Err(reason) => {
                if self.search.as_ref() == Some(request) {
                    self.search = None;
                    self.search_error = crate::text::clean(&reason, 4096, true);
                } else {
                    self.load = None;
                    self.load_failure = Some(crate::text::clean(&reason, 4096, true));
                }
            }
            Ok(snapshot) => {
                self.validate_new(&snapshot)?;
                if snapshot.revision <= self.material_revision() {
                    return Ok(false);
                }
                if request.search {
                    let old = self
                        .visible()
                        .iter()
                        .map(|&i| self.rows()[i].id.clone())
                        .collect::<Vec<_>>();
                    self.search = None;
                    if self.load.as_ref() == Some(request) {
                        self.load = None;
                    }
                    self.load_failure = None;
                    self.remote = Some(snapshot);
                    self.search_row = false;
                    self.reconcile(false);
                    self.repair(&old);
                } else {
                    self.replace(snapshot)?;
                    self.load_cursor.clear();
                }
            }
        }
        Ok(true)
    }
}
