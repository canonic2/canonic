//! Owns supplied status sections and notice lifecycle; hosts own operations, time and placement.

mod diagnostic;
mod input;
mod model;
mod view;
use crate::{event::Event, theme::Presentation};
pub use diagnostic::Diagnostic;
pub use model::{
    Action, Chrome, Entry, Glyph, Intent, Invalid, Kind, Mode, Notice, Outcome, Request, Row,
    Section, Severity, Snapshot, State, Target, Ticket,
};
use ratatui::{Frame, layout::Rect};
use std::{
    collections::{BTreeMap, BTreeSet},
    time::Duration,
};

/// Independent status/notice view with bounded data and exact effect acknowledgement.
pub struct StatusPanels {
    snapshot: Snapshot,
    mode: Mode,
    cursor: Option<Target>,
    notice_cursor: Option<String>,
    folded: BTreeSet<String>,
    aside: BTreeSet<String>,
    expired: BTreeMap<String, u64>,
    settled: BTreeMap<String, u64>,
    pending: BTreeMap<Target, Request>,
    failures: BTreeMap<Target, String>,
    intents: Vec<Intent>,
    now: Duration,
    offset: usize,
    details_offset: usize,
    viewport: usize,
    heights: BTreeMap<String, usize>,
}
impl StatusPanels {
    /// Construct without reading time or issuing host operations.
    /// # Errors
    /// Rejects duplicate identities, invalid state and bounded-data overflow.
    pub fn new(snapshot: Snapshot) -> Result<Self, Invalid> {
        model::validate(&snapshot)?;
        let cursor = snapshot.sections.first().map(|s| Target::Section {
            id: s.id.clone(),
            row: None,
        });
        Ok(Self {
            snapshot,
            mode: Mode::Closed,
            cursor,
            notice_cursor: None,
            folded: BTreeSet::new(),
            aside: BTreeSet::new(),
            expired: BTreeMap::new(),
            settled: BTreeMap::new(),
            pending: BTreeMap::new(),
            failures: BTreeMap::new(),
            intents: vec![],
            now: Duration::ZERO,
            offset: 0,
            details_offset: 0,
            viewport: 1,
            heights: BTreeMap::new(),
        })
    }
    #[must_use]
    pub fn snapshot(&self) -> &Snapshot {
        &self.snapshot
    }
    #[must_use]
    pub fn mode(&self) -> Mode {
        self.mode
    }
    #[must_use]
    pub fn cursor(&self) -> Option<Target> {
        if self.mode == Mode::Notices {
            self.notice_cursor.clone().map(Target::Notice)
        } else {
            self.cursor.clone()
        }
    }
    #[must_use]
    pub fn folded(&self, id: &str) -> bool {
        self.folded.contains(id)
    }
    #[must_use]
    pub fn pending(&self, r: &Request) -> bool {
        self.pending.get(&r.target) == Some(r)
    }
    #[must_use]
    pub fn failure(&self, target: &Target) -> Option<&str> {
        self.failures.get(target).map(String::as_str)
    }
    pub fn take_intents(&mut self) -> Vec<Intent> {
        std::mem::take(&mut self.intents)
    }
    /// Enter without touching the caller's editor, selection or scroll state.
    pub fn open(&mut self, mode: Mode) {
        self.mode = mode;
        self.offset = 0;
        self.details_offset = 0;
        if mode == Mode::Notices
            && self
                .notice_cursor
                .as_ref()
                .is_none_or(|id| !self.notices().iter().any(|n| &n.id == id))
        {
            self.notice_cursor = self.notices().first().map(|n| n.id.clone());
        }
    }
    /// Return focus; supplied facts and ongoing host work remain alive.
    pub fn close(&mut self) {
        if self.mode != Mode::Closed {
            self.mode = Mode::Closed;
            if !self.intents.contains(&Intent::Returned) {
                self.intents.push(Intent::Returned);
            }
        }
    }
    /// Advance an explicit monotonic host clock. Warning/error notices never expire.
    pub fn advance(&mut self, now: Duration) -> bool {
        if now < self.now {
            return false;
        }
        self.now = now;
        for n in &self.snapshot.notices {
            if n.severity < Severity::Warning
                && n.expires.is_some_and(|due| now >= due)
                && !self.pending.contains_key(&Target::Notice(n.id.clone()))
                && !self.failures.contains_key(&Target::Notice(n.id.clone()))
            {
                self.expired.insert(n.id.clone(), n.revision);
            }
        }
        self.repair();
        true
    }
    /// Accept an authoritative newer snapshot atomically. Material changes require new revisions.
    /// # Errors
    /// Returns invalid descriptors or non-monotonic snapshot/record revisions without mutation.
    pub fn replace(&mut self, snapshot: Snapshot) -> Result<(), Invalid> {
        self.validate_replacement(&snapshot)?;
        let unchanged: BTreeSet<_> = self
            .snapshot
            .sections
            .iter()
            .flat_map(|s| {
                let mut targets = vec![Target::Section {
                    id: s.id.clone(),
                    row: None,
                }];
                targets.extend(s.rows.iter().map(|r| Target::Section {
                    id: s.id.clone(),
                    row: Some(r.id.clone()),
                }));
                targets
            })
            .chain(
                self.snapshot
                    .notices
                    .iter()
                    .map(|n| Target::Notice(n.id.clone())),
            )
            .filter(|t| self.target_revision(t) == replacement_revision(&snapshot, t))
            .collect();
        self.failures.retain(|t, _| unchanged.contains(t));
        self.aside
            .retain(|id| unchanged.contains(&Target::Notice(id.clone())));
        let old_sections = &self.snapshot.sections;
        let old_cursor = self.cursor.clone();
        let old_notice = self.notice_cursor.clone();
        let old_index = old_cursor
            .as_ref()
            .and_then(|target| match target {
                Target::Section { id, .. } => old_sections.iter().position(|s| &s.id == id),
                Target::Notice(_) => None,
            })
            .unwrap_or(0);
        self.snapshot = snapshot;
        let sections = &self.snapshot.sections;
        self.folded
            .retain(|id| sections.iter().any(|s| &s.id == id));
        self.heights
            .retain(|id, _| sections.iter().any(|s| &s.id == id));
        let notices = &self.snapshot.notices;
        self.aside.retain(|id| notices.iter().any(|n| &n.id == id));
        self.expired
            .retain(|id, rev| notices.iter().any(|n| &n.id == id && n.revision == *rev));
        self.settled
            .retain(|id, _| notices.iter().any(|n| &n.id == id));
        self.cursor = old_cursor
            .filter(|target| self.target_revision(target).is_some())
            .or_else(|| {
                sections
                    .get(old_index.min(sections.len().saturating_sub(1)))
                    .map(|s| Target::Section {
                        id: s.id.clone(),
                        row: None,
                    })
            });
        self.notice_cursor = old_notice;
        let valid: BTreeSet<_> = self
            .pending
            .values()
            .filter(|r| self.target_revision(&r.target) == Some(r.revision))
            .map(|r| r.target.clone())
            .collect();
        self.pending.retain(|t, _| valid.contains(t));
        let pending = &self.pending;
        self.intents
            .retain(|i| !matches!(i,Intent::Request(r) if pending.get(&r.target)!=Some(r)));
        self.advance(self.now);
        Ok(())
    }
    fn validate_replacement(&self, s: &Snapshot) -> Result<(), Invalid> {
        model::validate(s)?;
        if s.revision <= self.snapshot.revision {
            return Err(Invalid::Revision);
        }
        for section in &s.sections {
            if let Some(old) = self
                .snapshot
                .sections
                .iter()
                .find(|old| old.id == section.id)
                && (section.revision < old.revision
                    || section.revision == old.revision && section != old)
            {
                return Err(Invalid::Revision);
            }
        }
        for notice in &s.notices {
            if let Some(old) = self.snapshot.notices.iter().find(|old| old.id == notice.id)
                && (notice.revision < old.revision
                    || notice.revision == old.revision && notice != old)
            {
                return Err(Invalid::Revision);
            }
        }
        Ok(())
    }
    fn target_revision(&self, target: &Target) -> Option<u64> {
        match target {
            Target::Section { id, row } => self
                .snapshot
                .sections
                .iter()
                .find(|s| {
                    &s.id == id
                        && row
                            .as_ref()
                            .is_none_or(|id| s.rows.iter().any(|r| &r.id == id))
                })
                .map(|s| s.revision),
            Target::Notice(id) => self
                .snapshot
                .notices
                .iter()
                .find(|n| &n.id == id && !self.hidden(n))
                .map(|n| n.revision),
        }
    }
    fn hidden(&self, n: &Notice) -> bool {
        self.expired.get(&n.id).is_some_and(|r| n.revision <= *r)
            || self.settled.get(&n.id).is_some_and(|r| n.revision <= *r)
    }
    /// Severity/newest ordering includes set-aside actionable errors.
    #[must_use]
    pub fn notices(&self) -> Vec<&Notice> {
        let mut notices: Vec<_> = self
            .snapshot
            .notices
            .iter()
            .filter(|n| !self.hidden(n))
            .collect();
        notices.sort_by_key(|n| {
            (
                std::cmp::Reverse(self.notice_severity(n)),
                std::cmp::Reverse(n.published),
            )
        });
        notices
    }
    #[must_use]
    pub fn current_notice(&self) -> Option<&Notice> {
        self.notices()
            .into_iter()
            .find(|n| !self.aside.contains(&n.id))
    }
    #[must_use]
    pub fn unresolved(&self) -> usize {
        self.notices()
            .iter()
            .filter(|n| self.notice_severity(n) == Severity::Error && n.action.is_some())
            .count()
    }
    fn repair(&mut self) {
        if self
            .notice_cursor
            .as_ref()
            .is_none_or(|id| !self.notices().iter().any(|n| &n.id == id))
        {
            self.notice_cursor = self.notices().first().map(|n| n.id.clone());
            self.details_offset = 0;
        }
    }
    /// Dismiss attention, retaining actionable errors in the list and badge.
    pub fn dismiss(&mut self, id: &str) -> bool {
        let Some(n) = self.notices().into_iter().find(|n| n.id == id) else {
            return false;
        };
        if self.notice_severity(n) == Severity::Error && n.action.is_some() {
            self.aside.insert(id.into());
        } else {
            self.settled.insert(id.into(), n.revision);
            self.pending.remove(&Target::Notice(id.into()));
        }
        let pending = &self.pending;
        self.intents
            .retain(|i| !matches!(i,Intent::Request(r) if pending.get(&r.target)!=Some(r)));
        self.repair();
        true
    }
    fn issue(&mut self, target: Target, action: Action) {
        if self.pending.contains_key(&target) || self.pending.len() >= 128 {
            return;
        }
        if let Some(reason) = action.disabled {
            self.record_failure(target, reason);
            return;
        }
        let Some(revision) = self.target_revision(&target) else {
            return;
        };
        let request = Request {
            ticket: Ticket::new(),
            target: target.clone(),
            revision,
            action: action.id,
        };
        self.pending.insert(target, request.clone());
        self.intents.push(Intent::Request(request));
    }
    /// Resolve exact requests once; stale/foreign/wrong target replies are inert.
    /// # Errors
    /// Invalid authoritative data or oversized failure reasons preserve pending work.
    pub fn complete(&mut self, r: &Request, outcome: Outcome) -> Result<bool, Invalid> {
        if !self.pending(r) || self.target_revision(&r.target) != Some(r.revision) {
            return Ok(false);
        }
        if let Outcome::Applied(s) = &outcome {
            self.validate_replacement(s)?;
        }
        if matches!(&outcome,Outcome::Failed(s) if s.len()>65536) {
            return Err(Invalid::Limit);
        }
        if matches!(&outcome, Outcome::Failed(s) if !self.can_record_failure(&r.target, s)) {
            return Err(Invalid::Limit);
        }
        self.pending.remove(&r.target);
        self.intents
            .retain(|i| !matches!(i,Intent::Request(request) if request==r));
        match outcome {
            Outcome::Applied(s) => self.replace(s)?,
            Outcome::Done => {
                if let Target::Notice(id) = &r.target {
                    self.settled.insert(id.clone(), r.revision);
                }
                self.failures.remove(&r.target);
            }
            Outcome::Failed(reason) => {
                self.failures.insert(r.target.clone(), reason);
                if let Target::Notice(id) = &r.target {
                    self.aside.remove(id);
                    self.expired.remove(id);
                }
            }
        }
        self.repair();
        Ok(true)
    }
    /// Process only the pane's bindings, with paste and modified/repeated effects inert.
    pub fn update(&mut self, event: &Event) -> bool {
        input::update(self, event)
    }
    /// Render the sections into a host-owned area; compact presentation preserves saved folds.
    pub fn render_sections(
        &mut self,
        f: &mut Frame<'_>,
        area: Rect,
        p: Presentation,
        compact: bool,
    ) {
        view::sections(self, f, area, p, compact);
    }
    pub fn render_notices(&mut self, f: &mut Frame<'_>, area: Rect, p: Presentation) {
        view::notices(self, f, area, p);
    }
    /// Compact host facts; unknown context/cost never becomes zero.
    #[must_use]
    pub fn summary(&self, width: u16) -> String {
        view::summary(self, width)
    }
    /// Notice activity text and severity, leaving actual activity-row precedence to the caller.
    #[must_use]
    pub fn activity(&self, p: Presentation) -> Option<(String, Severity)> {
        view::activity(self, p)
    }
    fn notice_severity(&self, n: &Notice) -> Severity {
        if self.failures.contains_key(&Target::Notice(n.id.clone())) {
            Severity::Error
        } else {
            n.severity
        }
    }
    fn can_record_failure(&self, target: &Target, reason: &str) -> bool {
        let total: usize = self
            .failures
            .iter()
            .filter(|(t, _)| *t != target)
            .map(|(_, s)| s.len())
            .sum();
        total.saturating_add(reason.len()) <= 4 * 1024 * 1024
    }
    fn record_failure(&mut self, target: Target, reason: String) {
        if self.can_record_failure(&target, &reason) {
            self.failures.insert(target, reason);
        }
    }
}

fn replacement_revision(snapshot: &Snapshot, target: &Target) -> Option<u64> {
    match target {
        Target::Section { id, row } => snapshot
            .sections
            .iter()
            .find(|s| {
                &s.id == id
                    && row
                        .as_ref()
                        .is_none_or(|id| s.rows.iter().any(|r| &r.id == id))
            })
            .map(|s| s.revision),
        Target::Notice(id) => snapshot
            .notices
            .iter()
            .find(|n| &n.id == id)
            .map(|n| n.revision),
    }
}
