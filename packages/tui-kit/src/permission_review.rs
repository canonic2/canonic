//! Identified permission review without execution, policy authority or terminal ownership.

mod input;
mod model;
mod view;

use crate::{
    dialog::{Confirmation, Stack},
    editor::Editor,
};
pub use model::{Choice, Invalid, Kind, Request, Resource, Response, Result, State, Ticket};
use std::time::Duration;

/// Local host intent. A choice never establishes authority without acknowledgement.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Intent {
    /// One immutable response attempt.
    Respond(Response),
    /// Close without answering; distinct from Deny and Skip.
    Dismissed { id: String, revision: u64 },
}

struct ScopeCheck {
    scope: String,
    prompt: String,
    label: String,
    accept: bool,
    offset: usize,
    viewport: usize,
    total: usize,
    destructive: bool,
    confirmation: Confirmation,
}
/// Caller-owned reusable review with isolated input and injected time.
pub struct PermissionReview {
    request: Request,
    state: State,
    closed: bool,
    cursor: Option<usize>,
    previous: Option<usize>,
    details: bool,
    searching: bool,
    query: Editor,
    matches: Vec<usize>,
    match_index: usize,
    offset: usize,
    detail_total: usize,
    viewport: usize,
    choice_offset: usize,
    notice: String,
    pending: Option<Response>,
    failed: Option<Response>,
    receipt: Option<Response>,
    elapsed: Duration,
    intents: Vec<Intent>,
    scope: Stack<ScopeCheck, u8>,
    confirmation: Confirmation,
    area: ratatui::layout::Rect,
}
impl PermissionReview {
    /// Validate the complete descriptor before creating an opening.
    /// # Errors
    /// Invalid IDs/defaults, scopes and resource bounds leave no partial state.
    pub fn new(request: Request) -> std::result::Result<Self, Invalid> {
        model::validate(&request)?;
        let cursor = request
            .default
            .as_ref()
            .and_then(|id| request.choices.iter().position(|c| &c.id == id))
            .map(|i| i + 1);
        let mut query = Editor::new(4096);
        query.set_reject_overflow(true);
        Ok(Self {
            request,
            state: State::Open,
            closed: false,
            cursor,
            previous: None,
            details: false,
            searching: false,
            query,
            matches: vec![],
            match_index: 0,
            offset: 0,
            detail_total: 0,
            viewport: 1,
            choice_offset: 0,
            notice: String::new(),
            pending: None,
            failed: None,
            receipt: None,
            elapsed: Duration::ZERO,
            intents: vec![],
            scope: Stack::new(vec![0]).map_err(|_| Invalid::Identity)?,
            confirmation: Confirmation::default(),
            area: ratatui::layout::Rect::default(),
        })
    }
    /// Current material snapshot.
    #[must_use]
    pub fn request(&self) -> &Request {
        &self.request
    }
    /// Current lifecycle.
    #[must_use]
    pub fn state(&self) -> State {
        self.state
    }
    /// Whether the host may fold this panel into a receipt.
    #[must_use]
    pub fn closed(&self) -> bool {
        self.closed
    }
    /// Current choice, excluding the details row.
    #[must_use]
    pub fn selected(&self) -> Option<&Choice> {
        self.cursor
            .and_then(|i| i.checked_sub(1))
            .and_then(|i| self.request.choices.get(i))
    }
    /// True while exact-scope confirmation owns focus.
    #[must_use]
    pub fn confirming(&self) -> bool {
        !self.scope.is_empty()
    }
    /// True while safe loaded source is open.
    #[must_use]
    pub fn inspecting(&self) -> bool {
        self.details
    }
    /// Literal loaded-source query.
    #[must_use]
    pub fn query(&self) -> &str {
        self.query.text()
    }
    /// Local validation or actual host feedback.
    #[must_use]
    pub fn notice(&self) -> &str {
        &self.notice
    }
    /// Actual acknowledged immutable response, never an optimistic grant.
    #[must_use]
    pub fn receipt(&self) -> Option<&Response> {
        self.receipt.as_ref()
    }
    /// Drain effects once.
    pub fn take_intents(&mut self) -> Vec<Intent> {
        std::mem::take(&mut self.intents)
    }
    pub(crate) fn current_intent(&self, intent: &Intent) -> bool {
        match intent {
            Intent::Respond(r) => self.pending.as_ref().is_some_and(|p| p.ticket == r.ticket),
            Intent::Dismissed { id, revision } => {
                &self.request.id == id && self.request.revision == *revision
            }
        }
    }
    /// Consume exactly one current operation identity; stale/foreign replies are inert.
    pub fn acknowledge(&mut self, ticket: &Ticket, result: Result) -> bool {
        if self.pending.as_ref().is_none_or(|r| &r.ticket != ticket) {
            return false;
        }
        let Some(response) = self.pending.take() else {
            return false;
        };
        match result {
            Result::Applied => {
                let Some(kind) = self
                    .request
                    .choices
                    .iter()
                    .find(|c| c.id == response.choice)
                    .map(|c| c.kind)
                else {
                    return false;
                };
                self.state = match kind {
                    Kind::Allow => State::Allowed,
                    Kind::Deny => State::Denied,
                    Kind::Skip => State::Skipped,
                };
                self.receipt = Some(response);
                self.closed = true;
                self.notice.clear();
            }
            Result::Failed(reason) => {
                self.state = State::Failed;
                self.failed = Some(response);
                self.notice = crate::text::clean(&reason, 4096, true);
            }
            Result::Refused(reason) => {
                let reason = crate::text::clean(&reason, 4096, true);
                if let Some(choice) = self
                    .request
                    .choices
                    .iter_mut()
                    .find(|c| c.id == response.choice)
                {
                    choice.disabled = Some(reason.clone());
                }
                self.cursor = None;
                self.failed = None;
                self.state = State::Refused;
                self.notice = reason;
            }
            Result::Expired(reason) => self.expire(&reason),
            Result::Withdrawn(reason) => self.withdraw(&reason),
        }
        true
    }
    /// Host expiry invalidates pending work and keeps details inspectable until close.
    pub fn expire(&mut self, reason: &str) {
        self.invalidate(State::Expired, reason);
    }
    /// Active withdrawal keeps its explanation visible until explicit closure.
    pub fn withdraw(&mut self, reason: &str) {
        self.invalidate(State::Withdrawn, reason);
    }
    fn invalidate(&mut self, state: State, reason: &str) {
        if self.closed {
            return;
        }
        self.state = state;
        self.pending = None;
        self.failed = None;
        self.scope.clear();
        self.confirmation.disarm();
        self.cursor = None;
        self.notice = crate::text::clean(reason, 4096, true);
        self.intents.clear();
    }
    /// Higher material revisions clear choices, confirmations and all outstanding work.
    /// # Errors
    /// Invalid descriptors or stale/foreign revisions leave the current review intact.
    pub fn replace(&mut self, request: Request, reason: &str) -> std::result::Result<(), Invalid> {
        if self.closed || request.id != self.request.id || request.revision <= self.request.revision
        {
            return Err(Invalid::Revision);
        }
        let mut fresh = Self::new(request)?;
        fresh.cursor = None;
        fresh.state = State::Changed;
        fresh.notice = format!(
            "{} · your choice was cleared",
            crate::text::clean(reason, 4096, false)
        );
        *self = fresh;
        Ok(())
    }
    /// Inject elapsed time; never schedules or automatically retries/times out.
    pub fn tick(&mut self, elapsed: Duration) {
        self.elapsed = self.elapsed.saturating_add(elapsed);
        self.query.tick(elapsed);
        self.confirmation.tick(elapsed);
        if let Some(ticket) = self.scope.ticket().cloned()
            && let Some(body) = self.scope.body_mut(&ticket)
        {
            body.confirmation.tick(elapsed);
        }
        let armed = self
            .selected()
            .is_some_and(|c| self.confirmation.armed(&c.id))
            || self
                .scope
                .ticket()
                .and_then(|t| self.scope.body(t))
                .is_some_and(|b| b.confirmation.armed("scope"));
        if !armed && self.notice.starts_with("press enter again") {
            self.notice.clear();
        }
    }
    pub(super) fn locked(&self) -> bool {
        matches!(self.state, State::Expired | State::Withdrawn)
    }
    /// Distinct receipt words suitable for transcript projection.
    #[must_use]
    pub fn outcome(&self) -> &'static str {
        match self.state {
            State::Allowed => "allowed",
            State::Denied => "denied",
            State::Skipped => "skipped",
            State::Expired => "expired, nothing applied",
            State::Withdrawn => "withdrawn, nothing applied",
            _ => "dismissed",
        }
    }
}
