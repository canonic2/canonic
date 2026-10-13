//! Deterministic status/notice host, retained Chat focus and responsive dialog placement.

use crate::{
    chat::{Chat, Intent as ChatIntent, Role, Turn},
    dialog::{ClosePolicy, Dialog, DialogView, Size, Stack},
    event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    status_panels::{
        Action, Chrome, Entry, Glyph, Intent, Kind, Mode, Notice, Outcome, Request, Row, Section,
        Severity, Snapshot, State, StatusPanels,
    },
    theme::Presentation,
};
use ratatui::{
    Frame,
    layout::Rect,
    widgets::{Clear, Paragraph},
};
use std::time::Duration;

pub(super) const NAMES: &[&str] = &[
    "main",
    "unknown",
    "loading",
    "empty",
    "error",
    "folded",
    "warning",
    "notices",
    "info",
    "success",
    "set-aside",
    "pending",
    "action-failed",
    "resolved",
    "expired",
    "long-reason",
    "disabled",
    "return-chat",
    "medium",
    "narrow",
    "tiny",
    "ansi16",
    "no-color",
    "ascii",
    "light",
    "reduced-motion",
    "review",
];

fn key(code: KeyCode, modifiers: KeyModifiers) -> Event {
    Event::Key(KeyEvent::new(code, modifiers))
}

pub(super) fn facts() -> Snapshot {
    let mut sections = vec![];
    for (id, title, kind, summary, detail) in [
        (
            "context",
            "Context",
            Kind::Context,
            "42%",
            "84k / 200k tokens",
        ),
        (
            "cost",
            "Cost",
            Kind::Cost,
            "$0.42",
            "session $0.42 · turn $0.08",
        ),
        ("todos", "To-dos", Kind::Todos, "1/3", ""),
        ("files", "Changed files", Kind::Files, "2", ""),
        ("connections", "Connections", Kind::Connections, "2/3", ""),
        (
            "activity",
            "Child activity",
            Kind::Activity,
            "1 working",
            "",
        ),
    ] {
        let mut s = Section::new(id, title);
        s.kind = kind;
        s.state = State::Ready;
        s.summary = summary.into();
        s.detail = detail.into();
        sections.push(s);
    }
    sections[0].bar = Some(4);
    sections[0].action = Some(Action::new("compact", "compact"));
    for (index, rows) in [
        (
            2,
            vec![
                ("design", "Review design", Glyph::Done),
                ("build", "Implement status", Glyph::Working),
                ("test", "Verify handoff", Glyph::Queued),
            ],
        ),
        (
            3,
            vec![
                ("settings", "src/settings.ts", Glyph::Modified),
                ("tests", "tests/settings.ts", Glyph::Added),
            ],
        ),
        (
            4,
            vec![
                ("model", "Model · connected", Glyph::Connected),
                ("tools", "Tools · connected", Glyph::Connected),
                ("local", "Local · disconnected", Glyph::Failed),
            ],
        ),
        (5, vec![("child", "Acme · working", Glyph::Working)]),
    ] {
        sections[index].rows = rows
            .into_iter()
            .map(|(id, label, glyph)| {
                let mut row = Row::new(id, label);
                row.glyph = glyph;
                row.action = Some(Action::new("inspect", "inspect"));
                row
            })
            .collect();
    }
    let mut error = Notice::new("connection", Severity::Error, "Local connection failed");
    error.detail =
        "The host reported a disconnected local service. Retry reconnects through the host.".into();
    error.action = Some(Action::new("reconnect", "retry"));
    let mut warning = Notice::new("usage", Severity::Warning, "Context nearing limit");
    warning.detail = "Usage and threshold are supplied by the host.".into();
    let mut success = Notice::new("saved", Severity::Success, "Changes saved");
    success.expires = Some(Duration::from_secs(8));
    let mut info = Notice::new("update", Severity::Info, "Session updated");
    info.expires = Some(Duration::from_secs(5));
    Snapshot {
        revision: 1,
        sections,
        notices: vec![info, success, warning, error],
    }
}

pub(super) struct StatusDemo {
    panels: StatusPanels,
    chat: Chat,
    dialog: Stack<(), usize>,
    now: Duration,
    request: Option<Request>,
    leader: Option<Duration>,
    log: String,
}
impl StatusDemo {
    pub(super) fn new(index: usize) -> Self {
        let name = NAMES[index % NAMES.len()];
        let mut snapshot = facts();
        configure(&mut snapshot, name);
        let mut chat = Chat::new();
        chat.set_input("and bump the app version");
        chat.update(&key(KeyCode::Left, KeyModifiers::NONE));
        chat.set_status_available(true);
        chat.push_record(Turn::new("status-turn", Role::Assistant, "acme", "Sending the user's timezone with the digest now. Host facts and notices stay beside this conversation."));
        let mut demo = Self {
            panels: StatusPanels::new(snapshot).expect("valid supplied status fixture"),
            chat,
            dialog: Stack::new(vec![0]).expect("one caller target"),
            now: Duration::ZERO,
            request: None,
            leader: None,
            log: String::new(),
        };
        if !matches!(
            name,
            "main"
                | "info"
                | "success"
                | "warning"
                | "ansi16"
                | "no-color"
                | "ascii"
                | "light"
                | "reduced-motion"
                | "review"
        ) {
            demo.open(
                if matches!(
                    name,
                    "notices"
                        | "set-aside"
                        | "pending"
                        | "action-failed"
                        | "resolved"
                        | "expired"
                        | "long-reason"
                        | "disabled"
                ) {
                    Mode::Notices
                } else {
                    Mode::Status
                },
            );
        }
        if name == "folded" {
            demo.panels.update(&key(KeyCode::Left, KeyModifiers::NONE));
        }
        if name == "set-aside" {
            demo.panels.dismiss("connection");
        }
        if matches!(name, "pending" | "action-failed" | "resolved") {
            demo.panels.update(&key(KeyCode::Enter, KeyModifiers::NONE));
            demo.drain();
            if name == "action-failed" {
                demo.ack(false);
            } else if name == "resolved" {
                demo.ack(true);
            }
        }
        if name == "expired" {
            demo.now = Duration::from_secs(10);
            demo.panels.advance(demo.now);
        }
        if name == "return-chat" {
            demo.panels.close();
        }
        if name == "review" {
            demo.chat
                .push_review(crate::review_queue::Review::Questions(Box::new(
                    crate::question_form::QuestionForm::new(super::reviews::question_request())
                        .expect("valid question fixture"),
                )))
                .expect("one review");
        }
        demo
    }
    fn open(&mut self, mode: Mode) {
        self.leader = None;
        self.panels.open(mode);
        self.dialog.clear();
        self.dialog
            .push(Dialog {
                id: "status".into(),
                title: if mode == Mode::Notices {
                    "Notices"
                } else {
                    "Status"
                }
                .into(),
                body: (),
                targets: vec![1],
                size: Size {
                    width: 64,
                    height: 24,
                },
                close: ClosePolicy::default(),
                view: DialogView::default(),
            })
            .expect("one dialog");
    }
    fn drain(&mut self) {
        for intent in self.panels.take_intents() {
            if let Intent::Request(r) = intent {
                self.request = Some(r);
            }
        }
    }
    fn ack(&mut self, success: bool) {
        if let Some(request) = self.request.take() {
            let outcome = if success {
                Outcome::Done
            } else {
                Outcome::Failed(
                    "Host could not reconnect. Check the local service, then retry.".into(),
                )
            };
            self.panels
                .complete(&request, outcome)
                .expect("bounded host result");
            self.log = if success {
                "Host acknowledged the action"
            } else {
                "Host action failed; retry available"
            }
            .into();
        }
    }
    pub(super) fn tick(&mut self, elapsed: Duration) {
        self.now = self.now.saturating_add(elapsed);
        if self.leader.is_some_and(|due| self.now >= due) {
            self.leader = None;
        }
        self.panels.advance(self.now);
        self.chat.tick(elapsed);
    }
    pub(super) fn update(&mut self, event: &Event) -> bool {
        if matches!(event, Event::Key(k) if k.code == KeyCode::Char('c') && k.modifiers == KeyModifiers::CONTROL)
        {
            return false;
        }
        if let Event::Key(k) = event
            && k.kind == KeyEventKind::Press
            && k.modifiers == KeyModifiers::NONE
        {
            match k.code {
                KeyCode::F(5) => {
                    self.ack(true);
                    return true;
                }
                KeyCode::F(6) => {
                    self.ack(false);
                    return true;
                }
                KeyCode::F(7) => {
                    self.tick(Duration::from_secs(10));
                    return true;
                }
                _ => {}
            }
        }
        if self.chat.reviews().active().is_some() {
            self.panels.close();
            self.chat.update(event);
            return true;
        }
        if self.panels.mode() == Mode::Closed {
            self.chat.update(event);
            for intent in self.chat.take_intents() {
                if let ChatIntent::Status { entry, .. } = intent {
                    match entry {
                        Entry::Status => self.open(Mode::Status),
                        Entry::Notices => self.open(Mode::Notices),
                        Entry::Attention => {
                            self.panels
                                .update(&key(KeyCode::Char('g'), KeyModifiers::CONTROL));
                        }
                    }
                }
            }
        } else {
            self.update_pane(event);
        }
        self.drain();
        true
    }
    pub(super) fn render(&mut self, frame: &mut Frame<'_>, area: Rect, p: Presentation) {
        let review = self.chat.reviews().active().is_some();
        let sidebar = area.width >= 120 && area.height >= 16;
        let chat_area = Rect {
            width: if sidebar { area.width - 33 } else { area.width },
            ..area
        };
        self.chat.set_status_chrome(Some(Chrome {
            summary: self.panels.summary(area.width),
            activity: self.panels.activity(p),
            focused: self.panels.mode() != Mode::Closed && !review,
        }));
        self.chat.render_docked_with(frame, chat_area, p);
        let stage = Rect {
            height: area.height.saturating_sub(8),
            ..area
        };
        if sidebar && self.panels.mode() != Mode::Notices {
            self.panels.render_sections(
                frame,
                Rect::new(area.right() - 32, area.y, 32, stage.height),
                p,
                review,
            );
        } else if self.panels.mode() != Mode::Closed && !review {
            if area.width < 50 {
                frame.render_widget(Clear, stage);
                if stage.height > 1 {
                    frame.render_widget(
                        Paragraph::new(if self.panels.mode() == Mode::Notices {
                            "Notices"
                        } else {
                            "Status"
                        })
                        .style(p.heading()),
                        Rect::new(stage.x, stage.y, stage.width, 1),
                    );
                    self.render_pane(
                        frame,
                        Rect::new(stage.x, stage.y + 1, stage.width, stage.height - 1),
                        p,
                    );
                } else {
                    self.render_pane(frame, stage, p);
                }
            } else {
                let panels = &mut self.panels;
                self.dialog.render_with(
                    frame,
                    stage,
                    p,
                    |(), _, _| 20,
                    |(), f, a, _, _| {
                        if panels.mode() == Mode::Notices {
                            panels.render_notices(f, a, p);
                        } else {
                            panels.render_sections(f, a, p, false);
                        }
                        None
                    },
                );
            }
        }
        if !self.log.is_empty() && stage.height > 0 && self.panels.mode() == Mode::Closed {
            frame.render_widget(
                Paragraph::new(self.log.clone()).style(p.muted()),
                Rect::new(area.x, area.y, chat_area.width, 1),
            );
        }
    }
    fn render_pane(&mut self, frame: &mut Frame<'_>, area: Rect, p: Presentation) {
        if self.panels.mode() == Mode::Notices {
            self.panels.render_notices(frame, area, p);
        } else {
            self.panels.render_sections(frame, area, p, false);
        }
    }
}
fn configure(snapshot: &mut Snapshot, name: &str) {
    let state = match name {
        "unknown" => Some(State::Unknown),
        "loading" => Some(State::Loading),
        "empty" => Some(State::Empty),
        "error" => Some(State::Error),
        _ => None,
    };
    if let Some(state) = state {
        for s in &mut snapshot.sections {
            s.state = state.clone();
            s.rows.clear();
            s.bar = None;
            s.detail = match state {
                State::Unknown => "not reported",
                State::Loading => "waiting for host facts",
                State::Empty => "no entries reported",
                _ => "host unavailable",
            }
            .into();
            if state == State::Error {
                s.retry = Some(Action::new("retry", "retry"));
            }
        }
    }
    if name == "warning" {
        snapshot.sections[0].warning = true;
        snapshot.sections[0].summary = "92%".into();
        snapshot.sections[0].bar = Some(9);
    }
    if matches!(name, "info" | "success") {
        snapshot.notices.retain(|n| {
            n.severity
                == if name == "info" {
                    Severity::Info
                } else {
                    Severity::Success
                }
        });
    }
    if name == "long-reason" {
        snapshot.notices[3].detail = format!(
            "{} END OF HOST REASON",
            "Long host explanation. ".repeat(80)
        );
    }
    if name == "disabled" {
        snapshot.notices[3]
            .action
            .as_mut()
            .expect("error action")
            .disabled = Some("Host policy currently prevents reconnect".into());
    }
}

impl StatusDemo {
    fn update_pane(&mut self, event: &Event) {
        if self.leader.is_some() {
            if matches!(event, Event::Key(k) if k.kind != KeyEventKind::Press) {
                return;
            }
            self.leader = None;
            if matches!(event, Event::Key(k) if k.kind == KeyEventKind::Press && k.modifiers == KeyModifiers::NONE && k.code == KeyCode::Char('n'))
            {
                self.open(Mode::Notices);
            }
            if matches!(event, Event::Key(k) if k.kind == KeyEventKind::Press && k.modifiers == KeyModifiers::NONE && k.code == KeyCode::Char('i'))
            {
                self.open(Mode::Status);
            }
        } else if matches!(event, Event::Key(k) if k.kind == KeyEventKind::Press && k.modifiers == KeyModifiers::CONTROL && k.code == KeyCode::Char('x'))
        {
            self.leader = Some(self.now.saturating_add(Duration::from_secs(1)));
        } else {
            self.panels.update(event);
        }
    }
}
