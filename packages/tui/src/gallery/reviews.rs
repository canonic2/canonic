//! Deterministic review host simulation. No agent execution, permission grants or clipboard I/O.

use crate::{
    chat::{Chat, Role, Turn},
    event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    picker::Choice,
    question_form::{self, Kind, Question, QuestionForm, Request, Result as Reply},
    review_queue::{Intent, Review},
    theme::Presentation,
};
use ratatui::{Frame, layout::Rect};
use std::time::Duration;

pub(super) struct ReviewDemo {
    chat: Chat,
    pending: Option<question_form::Response>,
    stale: Option<question_form::Ticket>,
    elapsed: Duration,
    delayed: bool,
    stream: bool,
    feedback: String,
    fixture: usize,
}
pub(super) fn question_request() -> Request {
    let mut users = Question::new(
        "users",
        "users",
        "Who should get timezone-aware digests?",
        Kind::Single,
    );
    users.required = true;
    users.other = true;
    users.default = Some("timezone".into());
    users.choices = vec![
        Choice::new("everyone", "Everyone"),
        Choice::new("timezone", "Users with a timezone set"),
        {
            let mut c = Choice::new("new", "New sign-ups only");
            c.disabled = Some("needs a migration first".into());
            c
        },
    ];
    users.choices[1].detail = "default".into();
    let mut channels = Question::new(
        "channels",
        "channels",
        "Which channels should follow it?",
        Kind::Multiple,
    );
    channels.required = true;
    channels.minimum = 1;
    channels.maximum = Some(3);
    channels.other = true;
    channels.description = "Pick 1 to 3.".into();
    channels.choices = [
        "digest emails",
        "weekly report",
        "push notifications",
        "in-app banner",
        "SMS",
    ]
    .iter()
    .enumerate()
    .map(|(i, s)| Choice::new(format!("channel-{i}"), *s))
    .collect();
    channels.choices[0].detail = "daily summary".into();
    channels.choices[4].disabled = Some("not set up for this project".into());
    let mut note = Question::new(
        "note",
        "note",
        "Anything to mention in the release note?",
        Kind::Text,
    );
    note.characters = Some(200);
    note.description = "Optional.".into();
    Request {
        id: "digest-rollout".into(),
        revision: 1,
        title: "digest rollout".into(),
        questions: vec![users, channels, note],
        partial: false,
    }
}
fn key(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
}
fn press(form: &mut QuestionForm, code: KeyCode) {
    form.update(&key(code));
}
fn answered(form: &mut QuestionForm) {
    press(form, KeyCode::Char('2'));
    press(form, KeyCode::Char(' '));
    press(form, KeyCode::Down);
    press(form, KeyCode::Char(' '));
    press(form, KeyCode::Enter);
}
impl ReviewDemo {
    pub(super) fn new(fixture: usize) -> Self {
        let name = super::scenarios::entries(super::Component::QuestionForm)[fixture].id;
        let form = prepare_form(name);
        let mut chat = Chat::new();
        chat.set_input("and bump the app version");
        chat.set_footer("acme-api#276 ~chat");
        for _ in 0..7 {
            chat.update(&key(KeyCode::Left));
        }
        chat.push_record(Turn::new(
            "question-turn",
            Role::Assistant,
            "acme",
            "Before I change the digest, three quick questions.",
        ));
        if name == "waiting-open" {
            chat.update(&key(KeyCode::Char('d')));
        }
        chat.reviews_mut()
            .push(Review::Questions(Box::new(form)))
            .expect("unique fixture review");
        if matches!(name, "queue" | "queue-reordered" | "queue-withdrawn") {
            let mut r = question_request();
            r.id = "second-request".into();
            r.title = "second request".into();
            chat.reviews_mut()
                .push(Review::Questions(Box::new(
                    QuestionForm::new(r).expect("valid fixture request"),
                )))
                .expect("distinct queue fixture");
            if name == "queue-reordered" {
                chat.reviews_mut()
                    .reorder(&["second-request", "digest-rollout"])
                    .expect("complete host order");
            }
            if name == "queue-withdrawn" {
                chat.reviews_mut().withdraw("second-request");
            }
        }
        let mut demo = Self {
            chat,
            pending: None,
            stale: None,
            elapsed: Duration::ZERO,
            delayed: name == "delayed-ack",
            stream: name == "streaming",
            feedback: String::new(),
            fixture,
        };
        demo.drain();
        demo.prepare_reply(name);
        demo.chat.tick(Duration::ZERO);
        demo
    }
    fn prepare_reply(&mut self, name: &str) {
        match name {
            "accepted" | "partial-accepted" | "draft-restored" => self.reply(Reply::Accepted),
            "failed" => self.reply(Reply::Failed("the server rejected the answers".into())),
            "refused" => self.reply(Reply::Refused {
                question: "channels".into(),
                reason: "weekly report isn't available for this plan".into(),
            }),
            "foreign-ack" => {
                let mut foreign =
                    QuestionForm::new(question_request()).expect("valid foreign fixture");
                answered(&mut foreign);
                press(&mut foreign, KeyCode::Tab);
                press(&mut foreign, KeyCode::Enter);
                if let Some(question_form::Intent::Submit(r)) = foreign.take_intents().pop()
                    && let Some(Review::Questions(f)) = self.chat.reviews_mut().active_mut()
                {
                    self.feedback = format!(
                        "foreign acknowledgement rejected: {}",
                        !f.acknowledge(&r.ticket, Reply::Accepted)
                    );
                }
            }
            "stale-ack" => {
                if let Some(r) = self.pending.take() {
                    self.stale = Some(r.ticket);
                }
                self.replace();
                self.stale_reply();
            }
            _ => {}
        }
    }
    fn drain(&mut self) {
        for intent in self.chat.reviews_mut().take_intents() {
            match intent {
                Intent::Permission(_) => {}
                Intent::Question(question_form::Intent::Submit(r)) => {
                    self.pending = Some(r);
                    self.elapsed = Duration::ZERO;
                }
                Intent::Question(question_form::Intent::Copy { ticket, .. }) => {
                    if let Some(Review::Questions(f)) = self.chat.reviews_mut().active_mut() {
                        let name = super::scenarios::entries(super::Component::QuestionForm)
                            [self.fixture]
                            .id;
                        f.acknowledge_copy(
                            &ticket,
                            if name == "copy-failed" {
                                Err("clipboard unavailable")
                            } else {
                                Ok(())
                            },
                        );
                    }
                }
                Intent::Question(question_form::Intent::Cancelled { .. }) => {
                    self.feedback = "cancelled · nothing sent".into();
                }
            }
        }
    }
    fn reply(&mut self, reply: Reply) {
        if let Some(r) = self.pending.take()
            && let Some(Review::Questions(f)) = self.chat.reviews_mut().get_mut(&r.id)
        {
            f.acknowledge(&r.ticket, reply);
            self.stale = Some(r.ticket);
        }
        self.chat.tick(Duration::ZERO);
    }
    fn replace(&mut self) {
        if let Some(Review::Questions(f)) = self.chat.reviews_mut().active_mut() {
            let mut r = question_request();
            r.revision = f.request().revision + 1;
            r.questions.truncate(1);
            f.replace(r).expect("increasing fixture revision");
        }
    }
    fn stale_reply(&mut self) {
        if let Some(t) = &self.stale
            && let Some(Review::Questions(f)) = self.chat.reviews_mut().active_mut()
        {
            self.feedback = format!(
                "stale acknowledgement rejected: {}",
                !f.acknowledge(t, Reply::Accepted)
            );
        }
    }
    pub(super) fn update(&mut self, event: &Event) -> bool {
        if let Event::Key(k) = event
            && k.kind == KeyEventKind::Press
        {
            match k.code {
                KeyCode::F(2) => {
                    *self = Self::new(
                        (self.fixture + 1)
                            % super::scenarios::entries(super::Component::QuestionForm).len(),
                    );
                    return true;
                }
                KeyCode::F(5) => {
                    self.reply(Reply::Accepted);
                    return true;
                }
                KeyCode::F(6) => {
                    self.reply(Reply::Failed("simulated host failure".into()));
                    return true;
                }
                KeyCode::F(7) => {
                    self.reply(Reply::Refused {
                        question: "channels".into(),
                        reason: "host refused this channel".into(),
                    });
                    return true;
                }
                KeyCode::F(8) => {
                    if let Some(Review::Questions(f)) = self.chat.reviews_mut().active_mut() {
                        f.expire("host stopped waiting");
                    }
                    return true;
                }
                KeyCode::F(9) => {
                    self.replace();
                    return true;
                }
                KeyCode::F(12) => {
                    self.stale_reply();
                    return true;
                }
                KeyCode::F(4) => return false,
                KeyCode::Char('t' | 'c') if k.modifiers == KeyModifiers::CONTROL => return false,
                KeyCode::Esc
                    if self.chat.reviews().active().is_none()
                        && self.chat.reviews().waiting_count() == 0 =>
                {
                    return false;
                }
                _ => {}
            }
        }
        self.chat.update(event);
        self.drain();
        true
    }
    pub(super) fn tick(&mut self, elapsed: Duration) {
        self.chat.tick(elapsed);
        self.elapsed = self.elapsed.saturating_add(elapsed);
        if self.stream && self.elapsed >= Duration::from_millis(800) {
            self.chat
                .append("\nStreaming background update while the draft and review remain intact.");
            self.elapsed = Duration::ZERO;
        }
        if self.delayed && self.pending.is_some() && self.elapsed >= Duration::from_secs(2) {
            self.reply(Reply::Accepted);
        }
        self.drain();
    }
    pub(super) fn render(&mut self, frame: &mut Frame<'_>, area: Rect, p: Presentation) {
        self.chat.render_docked_with(frame, area, p);
        if !self.feedback.is_empty() && area.height > 8 {
            frame.render_widget(
                ratatui::widgets::Paragraph::new(self.feedback.clone()).style(p.muted()),
                Rect::new(area.x, area.y, area.width, 1),
            );
        }
    }
}

fn prepare_form(name: &str) -> QuestionForm {
    let mut request = question_request();
    if name == "single" {
        request.questions.truncate(1);
    }
    if matches!(name, "typed-required" | "resource-limit" | "single-typed") {
        let mut q = request.questions.remove(2);
        q.required = name != "single-typed";
        request.questions = vec![q];
    }
    if matches!(
        name,
        "partial-offered" | "partial-submitting" | "partial-accepted"
    ) {
        request.partial = true;
    }
    if name == "long-choices" {
        request.questions[0].choices = (0..100)
            .map(|i| Choice::new(format!("user-{i}"), format!("Audience {i} · 東京")))
            .collect();
        request.questions[0].default = None;
    }
    if name == "long-prompt" {
        request.questions[0].prompt = "Choose the audience for timezone-aware digests. ".repeat(30);
    }
    let mut form = QuestionForm::new(request).expect("valid gallery question request");
    match name {
        "multiple" | "limit-reached" | "required-missing" | "back" | "skip-required" => {
            press(&mut form, KeyCode::Char('2'));
            if name == "required-missing" {
                press(&mut form, KeyCode::Enter);
            } else {
                press(&mut form, KeyCode::Char(' '));
                press(&mut form, KeyCode::Down);
                press(&mut form, KeyCode::Char(' '));
                if name == "limit-reached" {
                    press(&mut form, KeyCode::Down);
                    press(&mut form, KeyCode::Char(' '));
                    press(&mut form, KeyCode::Down);
                    press(&mut form, KeyCode::Char(' '));
                }
                if name == "back" {
                    press(&mut form, KeyCode::BackTab);
                }
                if name == "skip-required" {
                    press(&mut form, KeyCode::Char(' '));
                    press(&mut form, KeyCode::Up);
                    press(&mut form, KeyCode::Char(' '));
                    press(&mut form, KeyCode::Tab);
                }
            }
        }
        "disabled" => {
            press(&mut form, KeyCode::Down);
            press(&mut form, KeyCode::Enter);
        }
        "something-else" | "other-field" | "other-cleared" | "other-undo" => {
            press(&mut form, KeyCode::End);
            form.update(&Event::Paste("Only EU users for now".into()));
            if name != "other-field" {
                press(&mut form, KeyCode::Esc);
            }
            if matches!(name, "other-cleared" | "other-undo") {
                press(&mut form, KeyCode::Delete);
            }
            if name == "other-undo" {
                form.update(&Event::Key(KeyEvent::new(
                    KeyCode::Char('z'),
                    KeyModifiers::CONTROL,
                )));
            }
        }
        "typed-required" => press(&mut form, KeyCode::Enter),
        "resource-limit" => {
            form.update(&Event::Paste("x".repeat(65_537)));
        }
        "single-typed" => {
            form.update(&Event::Paste("Use the profile timezone".into()));
        }
        "review-missing" => {
            for _ in 0..3 {
                press(&mut form, KeyCode::Tab);
            }
        }
        "main" | "single" | "waiting-open" | "long-prompt" | "long-choices" | "tiny" => {}
        _ => prepare_answered(&mut form, name),
    }
    form
}
fn prepare_answered(form: &mut QuestionForm, name: &str) {
    answered(form);
    if !matches!(
        name,
        "empty-field" | "partial-offered" | "partial-submitting" | "partial-accepted"
    ) {
        form.update(&Event::Paste(if name == "too-long" {
            "x".repeat(214)
        } else {
            "Digest times now follow each user's\ntimezone. Users without one keep UTC.".into()
        }));
    }
    if matches!(
        name,
        "review"
            | "review-edit"
            | "submitting"
            | "accepted"
            | "failed"
            | "refused"
            | "no-ack"
            | "delayed-ack"
            | "foreign-ack"
            | "stale-ack"
            | "cancel-with-answers"
            | "expired"
            | "replaced"
            | "queue"
            | "streaming"
            | "draft-restored"
            | "cancelled"
            | "narrow-review"
            | "copy-failed"
            | "expired-copy"
            | "partial-offered"
            | "partial-submitting"
            | "partial-accepted"
    ) {
        press(form, KeyCode::Tab);
    }
    if name == "review-edit" {
        press(form, KeyCode::Char('2'));
    }
    if name == "too-long" {
        press(form, KeyCode::Enter);
    }
    if matches!(
        name,
        "cancel-with-answers"
            | "partial-offered"
            | "partial-submitting"
            | "partial-accepted"
            | "cancelled"
    ) {
        press(form, KeyCode::Esc);
        if matches!(
            name,
            "partial-submitting" | "partial-accepted" | "cancelled"
        ) {
            press(form, KeyCode::Char('2'));
        }
    }
    if matches!(name, "expired" | "copy-failed" | "expired-copy") {
        form.expire("acme stopped waiting at 20:21");
        if name != "expired" {
            press(form, KeyCode::Char('y'));
        }
    }
    if name == "replaced" {
        let mut fresh = question_request();
        fresh.revision = 2;
        fresh.questions.truncate(1);
        form.replace(fresh).expect("new material revision");
    }
    if matches!(
        name,
        "submitting"
            | "accepted"
            | "failed"
            | "refused"
            | "no-ack"
            | "delayed-ack"
            | "foreign-ack"
            | "stale-ack"
            | "draft-restored"
    ) {
        press(form, KeyCode::Enter);
    }
}
