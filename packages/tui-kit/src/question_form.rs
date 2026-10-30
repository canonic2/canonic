//! Staged structured answers and identified host responses; no execution or I/O.

mod input;
mod model;
mod view;

use crate::{dialog::Stack, editor::Editor, picker::ChoicePicker};
pub use model::{
    Answer, AnswerValue, Invalid, Kind, Question, Request, Response, Result, State, Ticket,
};
use std::{collections::BTreeSet, time::Duration};

/// Local intent; only an exact host acknowledgement establishes acceptance.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Intent {
    /// Submit an immutable answer snapshot, bound to this request and revision.
    Submit(Response),
    /// Close without submitting any answer.
    Cancelled { id: String, revision: u64 },
    /// Copy retained answers; the host owns clipboard access.
    Copy {
        id: String,
        revision: u64,
        ticket: Ticket,
        text: String,
    },
}

struct Stage {
    cursor: usize,
    marks: BTreeSet<usize>,
    field: Editor,
    other: bool,
    visited: bool,
}
/// Caller-owned form. Receives events, frames and elapsed time, never a terminal lease.
pub struct QuestionForm {
    request: Request,
    stages: Vec<Stage>,
    step: usize,
    editing: bool,
    from_review: bool,
    state: State,
    notice: String,
    pending: Option<Response>,
    receipt: Option<Response>,
    copy: Option<Ticket>,
    retained: String,
    elapsed: Duration,
    intents: Vec<Intent>,
    cancel: Stack<ChoicePicker, u8>,
    area: ratatui::layout::Rect,
    offset: usize,
    viewport: usize,
    review_row: Option<usize>,
}

impl QuestionForm {
    /// Validate the whole request before constructing a form.
    /// # Errors
    /// Returns descriptor/identity/limit errors without any UI or host effects.
    pub fn new(request: Request) -> std::result::Result<Self, Invalid> {
        model::validate(&request)?;
        let stages = request
            .questions
            .iter()
            .map(|q| {
                let mut field = Editor::new(65_536);
                field.set_reject_overflow(true);
                Stage {
                    cursor: q
                        .default
                        .as_ref()
                        .and_then(|id| q.choices.iter().position(|c| &c.id == id))
                        .unwrap_or(0),
                    marks: BTreeSet::new(),
                    field,
                    other: false,
                    visited: false,
                }
            })
            .collect();
        let editing = request.questions[0].kind == Kind::Text;
        Ok(Self {
            request,
            stages,
            step: 0,
            editing,
            from_review: false,
            state: State::Answering,
            notice: String::new(),
            pending: None,
            receipt: None,
            copy: None,
            retained: String::new(),
            elapsed: Duration::ZERO,
            intents: vec![],
            cancel: Stack::new(vec![0]).map_err(|_| Invalid::Identity)?,
            area: ratatui::layout::Rect::default(),
            offset: 0,
            viewport: 1,
            review_row: None,
        })
    }
    /// Current material host snapshot.
    #[must_use]
    pub fn request(&self) -> &Request {
        &self.request
    }
    /// Local lifecycle state.
    #[must_use]
    pub fn state(&self) -> State {
        self.state
    }
    /// Current zero-based question; question count means Review.
    #[must_use]
    pub fn step(&self) -> usize {
        self.step
    }
    /// Editor inspection for retained cursor, selection and undo verification.
    #[must_use]
    pub fn field(&self, question: usize) -> Option<&Editor> {
        self.stages.get(question).map(|s| &s.field)
    }
    /// True while cancellation owns the top focus scope.
    #[must_use]
    pub fn cancelling(&self) -> bool {
        !self.cancel.is_empty()
    }
    /// Current validation or host feedback.
    #[must_use]
    pub fn notice(&self) -> &str {
        &self.notice
    }
    /// Staged supplied answers in source order, omitting empty optional questions.
    #[must_use]
    pub fn answers(&self) -> Vec<Answer> {
        self.request
            .questions
            .iter()
            .zip(&self.stages)
            .filter_map(|(q, s)| model::answer(q, s))
            .collect()
    }
    /// Drain host intents once. Submitting never executes host work.
    pub fn take_intents(&mut self) -> Vec<Intent> {
        std::mem::take(&mut self.intents)
    }
    pub(crate) fn current_intent(&self, intent: &Intent) -> bool {
        match intent {
            Intent::Submit(r) => self.pending.as_ref().is_some_and(|p| p.ticket == r.ticket),
            Intent::Copy { ticket, .. } => self.copy.as_ref() == Some(ticket),
            Intent::Cancelled { id, revision } => {
                &self.request.id == id && self.request.revision == *revision
            }
        }
    }
    /// Consume a single-use exact response ticket. Foreign/stale/duplicate replies are inert.
    pub fn acknowledge(&mut self, ticket: &Ticket, result: Result) -> bool {
        if self.pending.as_ref().is_none_or(|p| &p.ticket != ticket) {
            return false;
        }
        if let Result::Refused { question, .. } = &result
            && !self.request.questions.iter().any(|q| &q.id == question)
        {
            return false;
        }
        let response = self.pending.take();
        match result {
            Result::Accepted => {
                self.receipt = response;
                self.state = State::Accepted;
                self.notice.clear();
            }
            Result::Failed(reason) => {
                self.state = State::Failed;
                self.notice = crate::text::clean(&reason, 4096, false);
            }
            Result::Refused { question, reason } => {
                self.state = State::Refused;
                self.go(self
                    .request
                    .questions
                    .iter()
                    .position(|q| q.id == question)
                    .unwrap_or(0));
                self.notice = crate::text::clean(&reason, 4096, false);
            }
        }
        true
    }
    /// Copy result does not submit, discard or modify any answer.
    pub fn acknowledge_copy(
        &mut self,
        ticket: &Ticket,
        result: std::result::Result<(), &str>,
    ) -> bool {
        if self.copy.as_ref() != Some(ticket) {
            return false;
        }
        self.copy = None;
        self.notice = result.map_or_else(
            |r| format!("could not copy · {}", crate::text::clean(r, 4096, false)),
            |()| "answers copied".into(),
        );
        true
    }
    /// Host expiry locks answers and invalidates all outstanding tickets.
    pub fn expire(&mut self, reason: &str) {
        if self.closed() {
            return;
        }
        self.pending = None;
        self.copy = None;
        self.cancel.clear();
        self.state = State::Expired;
        self.notice = crate::text::clean(reason, 4096, false);
        self.intents.clear();
    }
    /// A higher material revision starts fresh; old answers survive solely for copying.
    /// # Errors
    /// Invalid snapshots or non-increasing/different identities leave the form intact.
    pub fn replace(&mut self, request: Request) -> std::result::Result<(), Invalid> {
        if self.closed()
            || request.id != self.request.id
            || request.revision <= self.request.revision
        {
            return Err(Invalid::Revision);
        }
        let mut fresh = Self::new(request)?;
        fresh.retained = self.answer_text();
        fresh.notice = "replaced the earlier questions · y copies old answers".into();
        *self = fresh;
        Ok(())
    }
    /// Advance field clocks and the visible submission duration; owns no timers.
    pub fn tick(&mut self, elapsed: Duration) {
        self.elapsed = self.elapsed.saturating_add(elapsed);
        for stage in &mut self.stages {
            stage.field.tick(elapsed);
        }
    }
    /// Whether Chat may close this opening and restore its saved focus.
    #[must_use]
    pub fn closed(&self) -> bool {
        matches!(self.state, State::Accepted | State::Cancelled)
    }
    /// Accepted immutable response for host receipt projection.
    #[must_use]
    pub fn receipt(&self) -> Option<&Response> {
        self.receipt.as_ref()
    }
    /// Receipt body uses readable labels, omits skipped answers and retains typed text.
    #[must_use]
    pub fn answer_text(&self) -> String {
        self.answers()
            .iter()
            .filter_map(|a| {
                let q = self.request.questions.iter().find(|q| q.id == a.question)?;
                Some(format!("{}: {}", q.label, model::display(q, &a.value)))
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}
