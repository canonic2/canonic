//! One host-ordered review queue, with injected arrival timing and isolated active input.

use crate::{
    event::{Event, KeyCode, KeyEventKind, KeyModifiers},
    permission_review::{self, PermissionReview},
    question_form::{self, QuestionForm},
    theme::Presentation,
};
use ratatui::{Frame, layout::Rect};
use std::{collections::HashSet, fmt, time::Duration};

/// A reusable human review panel. Hosts own lifetime and authority.
pub enum Review {
    /// Structured staged answers.
    Questions(Box<QuestionForm>),
    /// Explicit permission responses; authority remains host-owned.
    Permission(Box<PermissionReview>),
}
impl Review {
    /// Stable host request ID, unique across the whole queue.
    #[must_use]
    pub fn id(&self) -> &str {
        match self {
            Self::Questions(f) => &f.request().id,
            Self::Permission(f) => &f.request().id,
        }
    }
    /// Exact current material revision.
    #[must_use]
    pub fn revision(&self) -> u64 {
        match self {
            Self::Questions(f) => f.request().revision,
            Self::Permission(f) => f.request().revision,
        }
    }
    /// Whether the panel has closed from host acceptance or explicit dismissal.
    #[must_use]
    pub fn closed(&self) -> bool {
        match self {
            Self::Questions(f) => f.closed(),
            Self::Permission(f) => f.closed(),
        }
    }
    fn update(&mut self, event: &Event) {
        match self {
            Self::Questions(f) => f.update(event),
            Self::Permission(f) => f.update(event),
        }
    }
    fn tick(&mut self, elapsed: Duration) {
        match self {
            Self::Questions(f) => f.tick(elapsed),
            Self::Permission(f) => f.tick(elapsed),
        }
    }
    /// Panel's preferred height, still clamped by the caller.
    #[must_use]
    pub fn height(&self, width: u16, p: Presentation) -> u16 {
        match self {
            Self::Questions(f) => f.height(width, p),
            Self::Permission(f) => f.height(width, p),
        }
    }
    /// Panel-specific footer keys.
    #[must_use]
    pub fn hint(&self) -> String {
        match self {
            Self::Questions(f) => f.hint().into(),
            Self::Permission(f) => f.hint(),
        }
    }
    /// Narrow footer keys prioritize Enter and Escape.
    #[must_use]
    pub fn hint_for(&self, width: u16) -> String {
        match self {
            Self::Questions(f) => f.hint_for(width).into(),
            Self::Permission(f) => f.hint_for(width),
        }
    }
    fn render(&mut self, frame: &mut Frame<'_>, area: Rect, p: Presentation, waiting: usize) {
        match self {
            Self::Questions(f) => f.render_with(frame, area, p, waiting),
            Self::Permission(f) => f.render_with(frame, area, p, waiting),
        }
    }
}
/// Queue envelope retains component-specific intent and request identity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Intent {
    /// Identified question operation.
    Question(question_form::Intent),
    /// Identified permission operation or distinct dismissal.
    Permission(permission_review::Intent),
}
/// Atomic queue update error.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Invalid {
    /// IDs are empty, duplicate, missing or foreign.
    Identity,
    /// Sixteen later reviews already wait.
    Limit,
}
impl fmt::Display for Invalid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid review queue: {self:?}")
    }
}
impl std::error::Error for Invalid {}
/// Caller-owned order, timing and focus. Never executes responses or owns persistence.
#[derive(Default)]
pub struct ReviewQueue {
    active: Option<Review>,
    waiting: Vec<Review>,
    finished: Vec<Review>,
    intents: Vec<Intent>,
    now: Duration,
    typed: Option<Duration>,
}
impl ReviewQueue {
    /// Append in host order without replacing an active opening.
    /// # Errors
    /// Duplicate/empty identities and capacity overflow leave the queue intact.
    pub fn push(&mut self, review: Review) -> Result<(), Invalid> {
        if review.id().is_empty()
            || self.active.as_ref().is_some_and(|r| r.id() == review.id())
            || self.waiting.iter().any(|r| r.id() == review.id())
        {
            return Err(Invalid::Identity);
        }
        if self.waiting.len() >= 16 {
            return Err(Invalid::Limit);
        }
        self.waiting.push(review);
        self.open_if_ready();
        Ok(())
    }
    /// Inspect the active focused opening, never a waiting request.
    #[must_use]
    pub fn active(&self) -> Option<&Review> {
        self.active.as_ref()
    }
    /// Host updates to active material/lifetime, with component ticket validation.
    pub fn active_mut(&mut self) -> Option<&mut Review> {
        self.active.as_mut()
    }
    /// Address active or waiting material updates by host identity.
    pub fn get_mut(&mut self, id: &str) -> Option<&mut Review> {
        if self.active.as_ref().is_some_and(|r| r.id() == id) {
            self.active.as_mut()
        } else {
            self.waiting.iter_mut().find(|r| r.id() == id)
        }
    }
    /// Count of later reviews; an unopened first arrival is included.
    #[must_use]
    pub fn waiting_count(&self) -> usize {
        self.waiting.len()
    }
    /// Host reorder must name every pending identity once; active focus is preserved.
    /// # Errors
    /// Invalid orders are rejected atomically.
    pub fn reorder(&mut self, ids: &[&str]) -> Result<(), Invalid> {
        let current: HashSet<_> = self
            .active
            .iter()
            .chain(&self.waiting)
            .map(Review::id)
            .collect();
        if ids.len() != current.len() || ids.iter().copied().collect::<HashSet<_>>() != current {
            return Err(Invalid::Identity);
        }
        self.waiting
            .sort_by_key(|r| ids.iter().position(|id| *id == r.id()).unwrap_or(0));
        Ok(())
    }
    /// Withdraw without producing a response. Removed panels have invalidated tickets.
    /// An active Permission Review stays visible as withdrawn until Escape and returns None.
    pub fn withdraw(&mut self, id: &str) -> Option<Review> {
        self.intents.retain(|intent| intent_id(intent) != id);
        if let Some(Review::Permission(review)) = &mut self.active
            && review.request().id == id
        {
            review.withdraw("host no longer needs this operation");
            return None;
        }
        if self.active.as_ref().is_some_and(|r| r.id() == id) {
            let mut removed = self.active.take();
            if let Some(review) = &mut removed {
                invalidate(review);
            }
            self.open_if_ready();
            removed
        } else {
            let index = self.waiting.iter().position(|r| r.id() == id)?;
            let mut removed = self.waiting.remove(index);
            invalidate(&mut removed);
            Some(removed)
        }
    }
    /// Record actual base typing; deferred opening waits for one second of quiet.
    pub fn note_typing(&mut self) {
        self.typed = Some(self.now);
    }
    /// Advance all visible component clocks and delayed opening.
    pub fn tick(&mut self, elapsed: Duration) {
        self.now = self.now.saturating_add(elapsed);
        if let Some(active) = &mut self.active {
            active.tick(elapsed);
        }
        self.synchronize();
        self.open_if_ready();
    }
    fn open_if_ready(&mut self) {
        if self.active.is_none()
            && !self.waiting.is_empty()
            && self
                .typed
                .is_none_or(|t| self.now.saturating_sub(t) >= Duration::from_secs(1))
        {
            self.active = Some(self.waiting.remove(0));
        }
    }
    /// Consume only focused review events. Ctrl-G opens a waiting first arrival.
    pub fn update(&mut self, event: &Event) -> bool {
        self.synchronize();
        if let Some(active) = &mut self.active {
            active.update(event);
            self.synchronize();
            return true;
        }
        if !self.waiting.is_empty()
            && matches!(event, Event::Key(k) if k.code == KeyCode::Char('g') && k.modifiers == KeyModifiers::CONTROL && k.kind == KeyEventKind::Press)
        {
            self.typed = None;
            self.active = Some(self.waiting.remove(0));
            return true;
        }
        false
    }
    /// Drain current and just-closed identified intents exactly once.
    pub fn take_intents(&mut self) -> Vec<Intent> {
        self.synchronize();
        let active = self.active.as_ref();
        std::mem::take(&mut self.intents)
            .into_iter()
            .filter(|intent| match (intent, active) {
                (
                    Intent::Question(question_form::Intent::Cancelled { .. })
                    | Intent::Permission(permission_review::Intent::Dismissed { .. }),
                    _,
                ) => true,
                (Intent::Question(intent), Some(Review::Questions(form))) => {
                    form.current_intent(intent)
                }
                (Intent::Permission(intent), Some(Review::Permission(review))) => {
                    review.current_intent(intent)
                }
                _ => false,
            })
            .collect()
    }
    /// Return closed panels for host receipt/focus projection; no external effect.
    pub fn take_finished(&mut self) -> Vec<Review> {
        self.synchronize();
        std::mem::take(&mut self.finished)
    }
    fn synchronize(&mut self) {
        if let Some(Review::Questions(f)) = &mut self.active {
            self.intents
                .extend(f.take_intents().into_iter().map(Intent::Question));
        }
        if let Some(Review::Permission(f)) = &mut self.active {
            self.intents
                .extend(f.take_intents().into_iter().map(Intent::Permission));
        }
        if self.active.as_ref().is_some_and(Review::closed) {
            if let Some(panel) = self.active.take() {
                self.finished.push(panel);
            }
            self.open_if_ready();
        }
    }
    /// Waiting activity text; use in the host's fixed activity row.
    #[must_use]
    pub fn waiting_label(&self) -> String {
        self.waiting.first().map_or_else(String::new, |r| {
            format!(
                "? {} · {} waiting · ctrl+g opens",
                title(r),
                self.waiting.len()
            )
        })
    }
    /// Render the focused panel, with the count of later host-ordered requests.
    pub fn render_with(&mut self, frame: &mut Frame<'_>, area: Rect, p: Presentation) {
        let waiting = self.waiting.len();
        if let Some(active) = &mut self.active {
            active.render(frame, area, p, waiting);
            if waiting > 0
                && area.height > 1
                && !matches!(active, Review::Permission(f) if f.state() == permission_review::State::Sending)
            {
                frame.render_widget(
                    ratatui::widgets::Paragraph::new(self.next_label()).style(p.muted()),
                    Rect::new(area.x, area.bottom() - 1, area.width, 1),
                );
            }
        }
    }
    /// Host order disclosure, with no automatic reordering.
    #[must_use]
    pub fn next_label(&self) -> String {
        let next = self
            .waiting
            .iter()
            .take(2)
            .map(title)
            .collect::<Vec<_>>()
            .join(" · then ");
        if next.is_empty() {
            next
        } else {
            format!("next: {next}")
        }
    }
    /// Render the active review's modal layer in a separately caller-owned viewport.
    pub fn render_overlay(&mut self, frame: &mut Frame<'_>, area: Rect, p: Presentation) {
        if let Some(Review::Questions(form)) = &mut self.active {
            form.render_overlay(frame, area, p);
        }
        if let Some(Review::Permission(review)) = &mut self.active {
            review.render_overlay(frame, area, p);
        }
    }
}

fn title(review: &Review) -> &str {
    match review {
        Review::Questions(f) => &f.request().title,
        Review::Permission(f) => &f.request().title,
    }
}
fn invalidate(review: &mut Review) {
    match review {
        Review::Questions(f) => f.expire("withdrawn by host"),
        Review::Permission(f) => f.withdraw("withdrawn by host"),
    }
}
fn intent_id(intent: &Intent) -> &str {
    match intent {
        Intent::Question(question_form::Intent::Submit(r)) => &r.id,
        Intent::Question(
            question_form::Intent::Cancelled { id, .. } | question_form::Intent::Copy { id, .. },
        )
        | Intent::Permission(permission_review::Intent::Dismissed { id, .. }) => id,
        Intent::Permission(permission_review::Intent::Respond(r)) => &r.id,
    }
}
