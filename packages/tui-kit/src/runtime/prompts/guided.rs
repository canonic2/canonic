//! Shared guided prompt hosting; consumers supply facts and validate intent.
use super::{open, presentation, suspend};
use crate::{
    editor::{Editor, EditorView},
    event::{Event, KeyCode, KeyEventKind, KeyModifiers},
    permission_review::{self, PermissionReview},
    picker::{Choice, ChoicePicker, Filter, Intent, PickerView, SelectionMode},
    question_form::{self as form, AnswerValue, QuestionForm},
    ratatui::{
        Frame,
        layout::{Constraint, Layout, Rect},
        widgets::Paragraph,
    },
    runtime::InlineSession,
};
use std::{
    io,
    time::{Duration, Instant},
};

/// A submitted answer, a previous-step request, or cancellation of the flow.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome<T> {
    /// Deliberate input confirmed the supplied value.
    Submitted(T),
    /// Escape left the step without submitting it.
    Back,
    /// Ctrl-C/Ctrl-D or a host checkpoint interrupted the flow.
    Cancelled,
}

/// A picker and its shared inline presentation. Keep this value to preserve
/// filter, highlight and marks when revisiting a step.
pub struct PickerPrompt<'a> {
    picker: ChoicePicker,
    title: &'a str,
    context: &'a str,
    escape: &'a str,
    empty: &'a str,
    mode: SelectionMode,
}
impl<'a> PickerPrompt<'a> {
    /// A single stable-ID choice. An absent default leaves no highlight.
    /// # Errors
    /// Duplicate IDs are refused before terminal acquisition.
    pub fn single(
        title: &'a str,
        context: &'a str,
        choices: Vec<Choice>,
        default: Option<&str>,
    ) -> io::Result<Self> {
        let mut picker = ChoicePicker::new(choices, default, SelectionMode::Single, Filter::Auto)
            .map_err(io::Error::other)?;
        if !picker.highlight(default) {
            picker.highlight(None);
        }
        Ok(Self {
            picker,
            title,
            context,
            escape: "cancel",
            empty: "",
            mode: SelectionMode::Single,
        })
    }
    /// Several stable IDs; Space marks and Enter confirms. Empty answers use
    /// `empty` in the footer. Enter below `minimum` keeps the list open.
    /// # Errors
    /// Duplicate IDs and impossible minimums are refused before acquiring a terminal.
    pub fn multiple(
        title: &'a str,
        context: &'a str,
        choices: Vec<Choice>,
        marked: &[&str],
        minimum: usize,
        empty: &'a str,
    ) -> io::Result<Self> {
        let enabled = choices.iter().filter(|c| c.disabled.is_none()).count();
        if minimum > enabled {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "At least {minimum} choices are required, but only {enabled} are available"
                ),
            ));
        }
        let mut picker = ChoicePicker::new(choices, None, SelectionMode::Multiple, Filter::Auto)
            .map_err(io::Error::other)?;
        picker.set_selection_limits(minimum, None);
        picker.set_marked(marked);
        Ok(Self {
            picker,
            title,
            context,
            escape: "cancel",
            empty,
            mode: SelectionMode::Multiple,
        })
    }
    /// Name Escape as back when an earlier step exists. Results still distinguish
    /// Escape from interruption; the consumer chooses its first-step behavior.
    #[must_use]
    pub fn back(mut self, back: bool) -> Self {
        self.escape = if back { "back" } else { "cancel" };
        self
    }
    fn view(&self, height: u16) -> PickerView<'_> {
        PickerView {
            title: self.title,
            context: self.context,
            escape: self.escape,
            empty: self.empty,
            max_height: height,
            presentation: presentation(),
            ..PickerView::default()
        }
    }
    fn height(&self, width: u16) -> u16 {
        let notices = u16::from(self.mode == SelectionMode::Multiple);
        self.picker
            .height(width, self.view(16))
            .saturating_add(1 + notices)
            .clamp(if self.context.is_empty() { 5 } else { 8 } + notices, 16)
    }
    fn render(&mut self, frame: &mut Frame<'_>) {
        let area = frame.area();
        if area.height < 5 || area.width < 20 {
            frame.render_widget(Paragraph::new("esc cancels; enlarge terminal"), area);
            return;
        }
        let list = Rect {
            height: area.height - u16::from(self.mode == SelectionMode::Multiple),
            ..area
        };
        let view = PickerView {
            title: self.title,
            context: if list.height >= 8 { self.context } else { "" },
            escape: self.escape,
            empty: self.empty,
            max_height: 16,
            presentation: presentation(),
            ..PickerView::default()
        };
        let used = self.picker.render(frame, list, view, true);
        if let Some(notice) = self.picker.notice() {
            frame.render_widget(
                Paragraph::new(crate::text::clean(notice, 256, false))
                    .style(presentation().warning()),
                Rect::new(area.x, used.bottom(), area.width, 1).intersection(area),
            );
        }
    }
    fn update(&mut self, event: &Event, available: bool) -> Option<Outcome<Vec<String>>> {
        picker_event(&mut self.picker, event, available)
    }
}

/// Host-owned signal checks may pause/resume the supplied lease or return an
/// `Interrupted` error. They must not acquire a second lease or write around it.
pub type Checkpoint<'a> = dyn FnMut(InlineSession) -> io::Result<InlineSession> + 'a;

/// One shared prompt lease. All ordinary CLI prompts use this host; product
/// adapters only supply data, validation and optional native signal checks.
pub struct Host<'a> {
    session: Option<InlineSession>,
    checkpoint: Option<Box<Checkpoint<'a>>>,
}
impl Host<'static> {
    /// Open an inline prompt with shared capability checks and presentation.
    /// # Errors
    /// Unsupported terminals and acquisition failures return without an answer.
    pub fn open(height: u16) -> io::Result<Self> {
        Ok(Self {
            session: Some(open(height)?),
            checkpoint: None,
        })
    }
}
impl<'a> Host<'a> {
    /// Attach the consumer's existing cooperative signal owner. UI behavior,
    /// terminal sizing, cleanup and key handling remain shared.
    #[must_use]
    pub fn with_checkpoint(
        mut self,
        checkpoint: impl FnMut(InlineSession) -> io::Result<InlineSession> + 'a,
    ) -> Self {
        self.checkpoint = Some(Box::new(checkpoint));
        self
    }
    fn session(&mut self) -> io::Result<&mut InlineSession> {
        if let Some(checkpoint) = &mut self.checkpoint {
            let session = self
                .session
                .take()
                .ok_or_else(|| io::Error::other("Terminal is closed"))?;
            self.session = Some(checkpoint(session)?);
        }
        self.session
            .as_mut()
            .ok_or_else(|| io::Error::other("Terminal is closed"))
    }
    fn poll(&mut self, plain_suspend: bool) -> io::Result<Option<Event>> {
        let event = self.session()?.poll(Duration::from_millis(100))?;
        if event
            .as_ref()
            .is_some_and(|event| suspend::requested(event, plain_suspend))
        {
            suspend::suspend(&mut self.session)?;
            return Ok(None);
        }
        Ok(event)
    }
    fn finish_record<T>(
        self,
        result: io::Result<Outcome<T>>,
        record: &str,
    ) -> io::Result<Outcome<T>> {
        let result = match result {
            Err(error) if error.kind() == io::ErrorKind::Interrupted => Ok(Outcome::Cancelled),
            other => other,
        };
        super::finish_record(self.session, result, record)
    }
    /// Run or revisit a picker. The prompt retains its filter, highlight and marks.
    /// # Errors
    /// Terminal/checkpoint failures are reported after attempting restoration.
    pub fn pick(mut self, prompt: &mut PickerPrompt<'_>) -> io::Result<Outcome<Vec<String>>> {
        let result = self.pick_loop(prompt);
        let record = match &result {
            Ok(Outcome::Submitted(ids)) => {
                let label = if ids.is_empty() {
                    prompt.empty.to_owned()
                } else if ids.len() > 4 {
                    format!("{} of {} marked", ids.len(), prompt.picker.choices().len())
                } else {
                    ids.iter()
                        .map(|id| {
                            prompt
                                .picker
                                .choices()
                                .iter()
                                .find(|choice| &choice.id == id)
                                .map_or(id.as_str(), |choice| choice.label.as_str())
                        })
                        .collect::<Vec<_>>()
                        .join(", ")
                };
                format!("{} → {label}", prompt.title)
            }
            Ok(Outcome::Back) => format!(
                "{} → {}",
                prompt.title,
                if prompt.escape == "back" {
                    "back"
                } else {
                    "cancelled"
                }
            ),
            Ok(Outcome::Cancelled) => format!("{} → cancelled", prompt.title),
            Err(_) => String::new(),
        };
        self.finish_record(result, &record)
    }
    fn pick_loop(&mut self, prompt: &mut PickerPrompt<'_>) -> io::Result<Outcome<Vec<String>>> {
        loop {
            let session = self.session()?;
            session.set_height(prompt.height(session.area().width))?;
            session.draw(|frame| prompt.render(frame))?;
            let Some(event) = self.poll(!prompt.picker.filtering())? else {
                continue;
            };
            let session = self.session()?;
            let available = session.confirmation_available() && session.area().height >= 5;
            if let Some(outcome) = prompt.update(&event, available) {
                return Ok(outcome);
            }
        }
    }
    /// Edit bounded multiline text. No submitted text is printed or stored by the host.
    /// # Errors
    /// Terminal/checkpoint failures are reported after attempting restoration.
    pub fn edit(mut self, title: &str, limit: usize) -> io::Result<Outcome<String>> {
        let mut editor = Editor::new(limit);
        editor.set_reject_overflow(true);
        let result = self.edit_loop(title, &mut editor);
        let record = if matches!(result, Ok(Outcome::Submitted(_))) {
            "Input → received"
        } else {
            "Input → cancelled"
        };
        self.finish_record(result, record)
    }
    fn edit_loop(&mut self, title: &str, editor: &mut Editor) -> io::Result<Outcome<String>> {
        loop {
            self.session()?.draw(|frame| {
                editor.render_with(
                    frame,
                    frame.area(),
                    EditorView {
                        title,
                        footer: "Enter continue · Ctrl-J newline · Esc cancel",
                        presentation: presentation(),
                        ..EditorView::default()
                    },
                    true,
                );
            })?;
            let Some(event) = self.poll(false)? else {
                continue;
            };
            if interrupted(&event) {
                return Ok(Outcome::Cancelled);
            }
            if escaped(&event) {
                return Ok(Outcome::Back);
            }
            if entered(&event) && self.session()?.confirmation_available() {
                return Ok(Outcome::Submitted(editor.text().to_owned()));
            }
            editor.update(&event);
        }
    }
    /// Run a supplied permission review; dismissal returns Back. This returns
    /// intent only and never applies a grant or product operation.
    /// # Errors
    /// Invalid requests and terminal/checkpoint failures are returned.
    pub fn review(
        mut self,
        request: permission_review::Request,
        waiting: usize,
    ) -> io::Result<Outcome<String>> {
        let title = request.title.clone();
        let summary = request.summary.clone();
        let labels: Vec<_> = request
            .choices
            .iter()
            .map(|choice| (choice.id.clone(), choice.label.clone()))
            .collect();
        let result = PermissionReview::new(request)
            .map_err(io::Error::other)
            .and_then(|mut review| self.review_loop(&mut review, waiting));
        let record = match &result {
            Ok(Outcome::Submitted(id)) => format!(
                "{title} → {} · {summary}",
                labels
                    .iter()
                    .find(|(key, _)| key == id)
                    .map_or(id.as_str(), |(_, label)| label)
            ),
            Ok(Outcome::Back) => format!("{title} → left pending · {summary}"),
            Ok(Outcome::Cancelled) => format!("{title} → cancelled"),
            Err(_) => String::new(),
        };
        self.finish_record(result, &record)
    }
    fn review_loop(
        &mut self,
        review: &mut PermissionReview,
        waiting: usize,
    ) -> io::Result<Outcome<String>> {
        let mut last_tick = Instant::now();
        loop {
            let session = self.session()?;
            let now = Instant::now();
            review.tick(now.duration_since(last_tick));
            last_tick = now;
            let check = if review.confirming() { 8 } else { 1 };
            session.set_height(
                review
                    .height(session.area().width, presentation())
                    .saturating_add(check),
            )?;
            session.draw(|frame| {
                review.render_with(frame, frame.area(), presentation(), waiting);
                review.render_overlay(frame, frame.area(), presentation());
            })?;
            let Some(event) = self.poll(false)? else {
                continue;
            };
            if interrupted(&event) {
                return Ok(Outcome::Cancelled);
            }
            let available = self.session()?.confirmation_available();
            // Polling and checkpoints may suspend the host. Expire the old
            // confirmation before any resumed or continuously arriving input.
            let now = Instant::now();
            review.tick(now.duration_since(last_tick));
            last_tick = now;
            if entered(&event) && !available {
                continue;
            }
            review.update(&event);
            if let Some(intent) = review.take_intents().into_iter().next() {
                return Ok(match intent {
                    permission_review::Intent::Respond(response) => {
                        Outcome::Submitted(response.choice)
                    }
                    permission_review::Intent::Dismissed { .. } => Outcome::Back,
                });
            }
        }
    }
    /// Ask one structured question. Validation refusals preserve typed answers.
    /// # Errors
    /// Invalid questions (including multiple choices with a typed alternative)
    /// and terminal/checkpoint failures are returned.
    pub fn question(
        mut self,
        question: &Question,
        check: &mut Check<'_>,
    ) -> io::Result<Outcome<Reply>> {
        let result = setup_form(question).and_then(|mut form| self.question_loop(&mut form, check));
        let record = match &result {
            Ok(Outcome::Submitted(Reply::Typed(text))) => format!("{} → {text}", question.title),
            Ok(Outcome::Submitted(Reply::Choices(ids))) => {
                let summary = if ids.is_empty() {
                    "none".to_owned()
                } else {
                    ids.iter()
                        .map(|id| {
                            question
                                .choices
                                .iter()
                                .find(|(key, _, _)| key == id)
                                .map_or(id.as_str(), |(_, label, _)| label.as_str())
                        })
                        .collect::<Vec<_>>()
                        .join(", ")
                };
                format!("{} → {summary}", question.title)
            }
            Ok(Outcome::Back) => format!("{} → back", question.title),
            Ok(Outcome::Cancelled) => format!("{} → cancelled", question.title),
            Err(_) => String::new(),
        };
        self.finish_record(result, &record)
    }
    fn question_loop(
        &mut self,
        questions: &mut QuestionForm,
        check: &mut Check<'_>,
    ) -> io::Result<Outcome<Reply>> {
        let mut last_tick = Instant::now();
        loop {
            let session = self.session()?;
            let now = Instant::now();
            questions.tick(now.duration_since(last_tick));
            last_tick = now;
            session.set_height(
                questions
                    .height(session.area().width, presentation())
                    .max(8)
                    .saturating_add(1),
            )?;
            session.draw(|frame| render_question(questions, frame, frame.area()))?;
            let Some(event) = self.poll(false)? else {
                continue;
            };
            if interrupted(&event) {
                return Ok(Outcome::Cancelled);
            }
            let session = self.session()?;
            let available =
                session.confirmation_available() && question_fits(questions, session.area());
            let now = Instant::now();
            questions.tick(now.duration_since(last_tick));
            last_tick = now;
            question_event(questions, &event, available);
            for intent in questions.take_intents() {
                match intent {
                    form::Intent::Submit(response) => {
                        let reply = reply(&response);
                        match check(&reply) {
                            Ok(()) => {
                                questions.acknowledge(&response.ticket, form::Result::Accepted);
                                return Ok(Outcome::Submitted(reply));
                            }
                            Err(reason) => {
                                questions.acknowledge(
                                    &response.ticket,
                                    form::Result::Refused {
                                        question: "answer".into(),
                                        reason,
                                    },
                                );
                            }
                        }
                    }
                    form::Intent::Cancelled { .. } => return Ok(Outcome::Back),
                    form::Intent::Copy { .. } => {}
                }
            }
        }
    }
}

/// One setup question with optional typed alternative and retained validation.
pub struct Question {
    /// Short page title.
    pub title: String,
    /// The question to answer.
    pub prompt: String,
    /// Supporting nonsecret context.
    pub context: String,
    /// Stable ID, label and detail, in display order.
    pub choices: Vec<(String, String, String)>,
    /// Accept a set of marked choices instead of a single answer.
    pub multiple: bool,
    /// Typed alternative label and placeholder; unsupported when `multiple` is true.
    pub typed: Option<(String, Option<String>)>,
    /// Highlight this stable ID on single-choice questions without submitting it.
    pub default: Option<String>,
    /// Name Escape as Back because a previous step exists.
    pub back: bool,
}
/// A typed answer or a set of stable choice IDs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reply {
    /// Confirmed stable IDs, possibly empty for an optional multiple choice.
    Choices(Vec<String>),
    /// Trimmed nonsecret typed input.
    Typed(String),
}
/// A refusal keeps the same question open with the supplied reason.
pub type Check<'a> = dyn FnMut(&Reply) -> Result<(), String> + 'a;

fn entered(event: &Event) -> bool {
    matches!(event, Event::Key(k) if k.kind == KeyEventKind::Press && k.code == KeyCode::Enter && k.modifiers.is_empty())
}
fn picker_event(
    picker: &mut ChoicePicker,
    event: &Event,
    available: bool,
) -> Option<Outcome<Vec<String>>> {
    if interrupted(event) {
        return Some(Outcome::Cancelled);
    }
    match picker.update(event) {
        Intent::Selected(id) if entered(event) && available => Some(Outcome::Submitted(vec![id])),
        Intent::Confirmed(ids) if entered(event) && available => Some(Outcome::Submitted(ids)),
        Intent::Cancelled => Some(Outcome::Back),
        _ => None,
    }
}
fn escaped(event: &Event) -> bool {
    matches!(event, Event::Key(k) if k.kind != KeyEventKind::Release && k.code == KeyCode::Esc)
}
fn interrupted(event: &Event) -> bool {
    matches!(event, Event::Key(k) if k.kind != KeyEventKind::Release && matches!(k.code, KeyCode::Char('c' | 'd')) && k.modifiers.contains(KeyModifiers::CONTROL))
}
fn question_event(questions: &mut QuestionForm, event: &Event, available: bool) {
    // Question Form's numbered single choices can advance immediately. Gate
    // them as well as Enter while the terminal cannot show the question.
    let confirms = matches!(event, Event::Key(key) if key.kind == KeyEventKind::Press
        && (key.code == KeyCode::Enter || matches!(key.code, KeyCode::Char('1'..='9'))));
    if available || !confirms {
        questions.update(event);
    }
}
fn render_question(questions: &mut QuestionForm, frame: &mut Frame<'_>, area: Rect) {
    let [body, footer] = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(area);
    questions.render_with(frame, body, presentation(), 0);
    frame.render_widget(
        Paragraph::new(if question_fits(questions, area) {
            questions.hint_for(area.width)
        } else {
            "esc cancels; enlarge terminal"
        })
        .style(presentation().muted()),
        footer,
    );
    questions.render_overlay(frame, area, presentation());
}
fn question_fits(questions: &QuestionForm, area: Rect) -> bool {
    area.width >= 20
        && area.height
            >= questions
                .minimum_height(area.width, presentation())
                .saturating_add(1)
}
fn setup_form(question: &Question) -> io::Result<QuestionForm> {
    if question.multiple && question.typed.is_some() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "multiple-choice questions cannot include a typed alternative",
        ));
    }
    let shape = match (question.multiple, question.choices.is_empty()) {
        (true, _) => form::Kind::Multiple,
        (false, true) => form::Kind::Text,
        (false, false) => form::Kind::Single,
    };
    let mut asked = form::Question::new("answer", &question.title, &question.prompt, shape);
    asked.description.clone_from(&question.context);
    asked.required = shape != form::Kind::Multiple;
    asked.choices = question
        .choices
        .iter()
        .map(|(id, label, detail)| {
            let mut choice = Choice::new(id, label);
            choice.detail.clone_from(detail);
            choice
        })
        .collect();
    asked.default = question
        .default
        .clone()
        .filter(|_| shape == form::Kind::Single);
    if let Some((label, placeholder)) = &question.typed {
        asked.placeholder.clone_from(placeholder);
        if shape != form::Kind::Text {
            asked.other = true;
            asked.other_label = Some(label.clone());
        }
    }
    QuestionForm::new(form::Request {
        id: "setup".into(),
        revision: 1,
        title: question.title.clone(),
        questions: vec![asked],
        partial: false,
        back: question.back,
    })
    .map_err(io::Error::other)
}
fn reply(response: &form::Response) -> Reply {
    match response.answers.first().map(|a| &a.value) {
        Some(
            AnswerValue::Text(text)
            | AnswerValue::Choices {
                other: Some(text), ..
            },
        ) => Reply::Typed(text.trim().into()),
        Some(AnswerValue::Choices { ids, other: None }) => Reply::Choices(ids.clone()),
        None => Reply::Choices(vec![]),
    }
}

#[cfg(test)]
mod tests;
