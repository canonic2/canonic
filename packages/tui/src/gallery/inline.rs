//! Composes existing picker/reviews, retained Chat and borrowed views into a deterministic inline host.

mod native;
use crate::{
    chat::{Chat, Content, Intent as ChatIntent, Mode as ChatMode, Part as ChatPart, Role, Turn},
    event::{Event, KeyCode, KeyEventKind, KeyModifiers},
    inline_composition::{Part, PrintLedger},
    inspection::{Inspector, Origin},
    permission_review::{self, PermissionReview},
    picker::{Choice, ChoicePicker, Filter, Intent as PickIntent, PickerView, SelectionMode},
    question_form::{self, QuestionForm},
    session_browser::SessionBrowser,
    status_panels::{Chrome, Entry, Mode, StatusPanels},
    theme::Presentation,
    tree_browser::{self, Placement, TreeBrowser},
};
pub(super) use native::run;
use ratatui::{
    Frame,
    layout::Rect,
    widgets::{Block, Paragraph},
};
use std::{collections::VecDeque, time::Duration};

pub(super) const NAMES: &[&str] = &[
    "main",
    "picker",
    "question",
    "permission",
    "streaming",
    "out-of-order",
    "follow-up",
    "queued",
    "pending",
    "refused",
    "cancelled",
    "withdrawn",
    "host-stopped",
    "long-result",
    "empty",
    "long",
    "narrow",
    "short",
    "tiny",
    "plain",
    "no-anchor",
    "no-alternate",
    "return",
    "late-output",
    "redraw",
    "handoff",
    "ascii",
    "no-color",
    "ansi16",
    "light",
];

enum Region {
    Picker(Box<ChoicePicker>),
    Questions(Box<QuestionForm>),
    Permission(Box<PermissionReview>),
    Dock,
}
enum Overlay {
    Transcript,
    Sessions(Box<SessionBrowser>),
    Tree(Box<TreeBrowser>),
    Inspection(Box<Inspector>),
    Status,
}
pub(super) struct InlineDemo {
    region: Region,
    waiting: VecDeque<Region>,
    overlay: Option<Overlay>,
    chat: Chat,
    status: StatusPanels,
    ledger: PrintLedger,
    parts: Vec<Part>,
    pub(super) printed: Vec<String>,
    output: VecDeque<String>,
    pub(super) result: Option<String>,
    now: Duration,
    guard: Duration,
    pending_question: Option<question_form::Response>,
    pending_permission: Option<permission_review::Response>,
    tiny: bool,
    plain: bool,
    no_alternate: bool,
    notice: String,
}
fn picker(long: bool, empty: bool) -> Region {
    let mut choices = vec![
        Choice::new("deploy", "Deploy"),
        Choice::new("api", "Deploy API only"),
        Choice::new("cancel", "Cancel"),
    ];
    choices[0].detail = "all 3 services · fixture only".into();
    choices[1].detail = "skip worker, digest".into();
    if long {
        choices.extend(
            (4..=40).map(|i| Choice::new(format!("service-{i}"), format!("Acme service {i}"))),
        );
    }
    if empty {
        choices.clear();
    }
    Region::Picker(Box::new(
        ChoicePicker::new(choices, Some("deploy"), SelectionMode::Single, Filter::Off)
            .expect("unique fixture choices"),
    ))
}
fn questions() -> Region {
    Region::Questions(Box::new(
        QuestionForm::new(super::reviews::question_request()).expect("valid questions"),
    ))
}
fn permission() -> Region {
    Region::Permission(Box::new(
        PermissionReview::new(super::permissions::request()).expect("valid review"),
    ))
}
impl InlineDemo {
    pub(super) fn new(index: usize) -> Self {
        let name = NAMES[index % NAMES.len()];
        let dock = matches!(
            name,
            "streaming"
                | "out-of-order"
                | "follow-up"
                | "return"
                | "late-output"
                | "no-alternate"
                | "handoff"
        );
        let region = if name == "question" {
            questions()
        } else if matches!(name, "permission" | "pending" | "refused") {
            permission()
        } else if dock {
            Region::Dock
        } else {
            picker(name == "long", name == "empty")
        };
        let mut chat = Chat::new();
        chat.set_input("and bump the app version");
        chat.update(&Event::Key(crate::event::KeyEvent::new(
            KeyCode::Left,
            KeyModifiers::NONE,
        )));
        chat.set_footer("acme-api#276 ~chat");
        chat.set_status_available(true);
        chat.set_files_available(true);
        chat.set_sessions_available(true);
        let mut demo = Self {
            region,
            waiting: VecDeque::new(),
            overlay: None,
            chat,
            status: StatusPanels::new(super::status::facts()).expect("valid status"),
            ledger: PrintLedger::new(),
            parts: vec![],
            printed: vec![
                "~/acme-api $ acme preview".into(),
                "plan · 3 services · fixture only".into(),
            ],
            output: VecDeque::new(),
            result: None,
            now: Duration::ZERO,
            guard: Duration::ZERO,
            pending_question: None,
            pending_permission: None,
            tiny: false,
            plain: matches!(name, "plain" | "no-anchor"),
            no_alternate: name == "no-alternate",
            notice: String::new(),
        };
        if name == "queued" {
            demo.waiting.push_back(questions());
        }
        demo.configure(name, dock);
        demo
    }
    fn configure(&mut self, name: &str, dock: bool) {
        let demo = self;
        if matches!(name, "pending" | "refused") {
            for code in [KeyCode::Char('1'), KeyCode::Enter] {
                demo.update(&Event::Key(crate::event::KeyEvent::new(
                    code,
                    KeyModifiers::NONE,
                )));
            }
            if name == "refused" {
                demo.refuse();
            }
        }
        if dock {
            demo.parts = vec![
                Part {
                    id: "prose".into(),
                    revision: 1,
                    label: "acme".into(),
                    text: "acme · Adding the timezone and a test.".into(),
                    settled: true,
                },
                Part {
                    id: "edit".into(),
                    revision: 1,
                    label: "edit src/settings.ts".into(),
                    text: "edit src/settings.ts ··· writing".into(),
                    settled: false,
                },
                Part {
                    id: "tests".into(),
                    revision: 1,
                    label: "run tests".into(),
                    text: if name == "out-of-order" {
                        "run tests → done · waits for earlier card".into()
                    } else {
                        "run tests ··· waiting".into()
                    },
                    settled: name == "out-of-order",
                },
            ];
            demo.sync_parts();
        }
        if name == "follow-up" {
            demo.settle();
            demo.settle();
            demo.correct();
        }
        if matches!(name, "cancelled" | "withdrawn" | "host-stopped") {
            demo.result = Some(format!(
                "preview → {} · nothing executed",
                if name == "cancelled" {
                    "cancelled"
                } else if name == "withdrawn" {
                    "withdrawn by host"
                } else {
                    "host stopped"
                }
            ));
        }
        if name == "long-result" {
            demo.result = Some(format!(
                "preview → {}",
                "A host supplied result that wraps without truncation. ".repeat(8)
            ));
        }
    }
    fn sync_parts(&mut self) {
        self.ledger
            .replace(self.parts.clone())
            .expect("monotonic fixture parts");
        let mut turn = Turn::new("inline-turn", Role::Assistant, "acme", "");
        turn.parts = self
            .parts
            .iter()
            .map(|p| {
                let mut part = ChatPart::new(&p.id, Content::Markdown(p.text.clone()));
                part.revision = p.revision;
                part
            })
            .collect();
        if self.chat.turn("inline-turn").is_none() {
            self.chat.push_record(turn);
        } else {
            for part in turn.parts {
                if let Some(old) = self
                    .chat
                    .turn("inline-turn")
                    .and_then(|t| t.parts.iter().find(|p| p.id == part.id))
                    && part.revision == old.revision.saturating_add(1)
                {
                    self.chat.apply(crate::chat::Update {
                        conversation: self.chat.conversation(),
                        message: "inline-turn".into(),
                        part: part.id,
                        expected: old.revision,
                        revision: part.revision,
                        change: crate::chat::Change::Replace(part.content),
                    });
                }
            }
        }
        if let Some(batch) = self.ledger.ready() {
            for line in &batch.lines {
                self.printed.push(line.clone());
                self.output.push_back(line.clone());
            }
            self.ledger.acknowledge(&batch);
        }
    }
    fn settle(&mut self) {
        if let Some(p) = self.parts.iter_mut().find(|p| !p.settled) {
            p.revision += 1;
            p.settled = true;
            p.text = format!("{} → done · host", p.label);
            self.sync_parts();
        }
    }
    fn correct(&mut self) {
        if let Some(p) = self.parts.iter_mut().find(|p| p.settled && p.id != "prose") {
            p.revision += 1;
            p.text = "finished while disconnected · host".into();
            self.sync_parts();
        }
    }
    pub(super) fn tick(&mut self, elapsed: Duration) {
        self.now = self.now.saturating_add(elapsed);
        self.status.advance(self.now);
        self.chat.tick(elapsed);
        match &mut self.region {
            Region::Picker(p) => p.tick(elapsed),
            Region::Questions(q) => q.tick(elapsed),
            Region::Permission(p) => p.tick(elapsed),
            Region::Dock => {}
        }
    }
    pub(super) fn alternate(&self) -> bool {
        self.overlay.is_some()
    }
    pub(super) fn take_output(&mut self) -> Vec<String> {
        self.output.drain(..).collect()
    }
    pub(super) fn no_alternate(&self) -> bool {
        self.no_alternate
    }
    pub(super) fn plain(&self) -> bool {
        self.plain
    }
    pub(super) fn height(&self, width: u16, p: Presentation) -> u16 {
        let height = match &self.region {
            Region::Picker(picker) => picker.height(width, picker_view(p, u16::MAX)),
            Region::Questions(q) => q.height(width, p),
            Region::Permission(r) => r.height(width, p),
            Region::Dock => {
                let live = self
                    .ledger
                    .live()
                    .iter()
                    .map(|part| crate::text::wrapped(&part.text, width).len())
                    .sum::<usize>();
                self.chat
                    .inline_height(width, p)
                    .saturating_add(u16::try_from(live).unwrap_or(u16::MAX))
            }
        };
        height
            .max(3)
            .saturating_add(u16::from(!self.notice.is_empty()))
    }
    fn next(&mut self, line: String) {
        if let Some(region) = self.waiting.pop_front() {
            self.printed.push(line.clone());
            self.output.push_back(line);
            self.region = region;
            self.guard = self.now.saturating_add(Duration::from_millis(300));
        } else {
            self.result = Some(line);
        }
    }
    pub(super) fn update(&mut self, event: &Event) -> bool {
        if self.result.is_some() {
            return false;
        }
        if let Event::Key(k) = event
            && k.kind == KeyEventKind::Press
            && k.modifiers == KeyModifiers::NONE
        {
            match k.code {
                KeyCode::F(5) => {
                    self.accept();
                    self.settle();
                    return true;
                }
                KeyCode::F(6) => {
                    self.refuse();
                    return true;
                }
                KeyCode::F(7) => {
                    self.correct();
                    return true;
                }
                KeyCode::F(8) => {
                    self.waiting.push_back(questions());
                    self.notice = "one request waiting".into();
                    return true;
                }
                KeyCode::F(9) => {
                    self.output
                        .push_back("host: background output arrived".into());
                    return true;
                }
                _ => {}
            }
        }
        if self.overlay.is_some() {
            self.update_overlay(event);
            return true;
        }
        if self.tiny {
            if cancel(event) {
                self.next("preview → cancelled · nothing executed".into());
            }
            return true;
        }
        if self.now < self.guard
            && matches!(event,Event::Key(k) if k.code==KeyCode::Enter || matches!(k.code,KeyCode::Char('0'..='9')))
        {
            return true;
        }
        if cancel(event) && !matches!(self.region, Region::Dock) {
            self.next("preview → cancelled · nothing executed".into());
            return true;
        }
        self.update_region(event);
        true
    }
    fn update_region(&mut self, event: &Event) {
        match &mut self.region {
            Region::Picker(p) => {
                if let PickIntent::Selected(id) = p.update(event) {
                    let label = p
                        .choices()
                        .iter()
                        .find(|c| c.id == id)
                        .map_or("choice", |c| c.label.as_str())
                        .to_owned();
                    self.next(if id == "cancel" {
                        "preview → cancelled · nothing executed".into()
                    } else {
                        format!("preview → {label} · fixture only")
                    });
                }
            }
            Region::Questions(q) => {
                q.update(event);
                for intent in q.take_intents() {
                    if let question_form::Intent::Submit(r) = intent {
                        self.pending_question = Some(r);
                    }
                }
                if q.closed() {
                    self.next("question → cancelled · nothing sent".into());
                }
            }
            Region::Permission(p) => {
                p.update(event);
                for intent in p.take_intents() {
                    if let permission_review::Intent::Respond(r) = intent {
                        self.pending_permission = Some(r);
                    }
                }
                if p.closed() {
                    self.next("review → cancelled · nothing applied".into());
                }
            }
            Region::Dock => {
                if matches!(event, Event::Key(k) if k.kind == KeyEventKind::Press && k.code == KeyCode::PageUp && k.modifiers == KeyModifiers::ALT)
                {
                    self.overlay = Some(Overlay::Transcript);
                }
                if let crate::chat::Action::Submit(submission) = self.chat.update(event) {
                    self.output.push_back(format!("you → {}", submission.text));
                    self.chat.acknowledge_submission(&submission.ticket, Ok(()));
                }
                for intent in self.chat.composer_mut().take_intents() {
                    match intent {
                        crate::composer::Intent::Quit => {
                            self.result = Some("acme → closed · host keeps saved draft".into());
                        }
                        crate::composer::Intent::Rewind => self.output.push_back(
                            "↳ rewind · draft restored by composer · printed history retained"
                                .into(),
                        ),
                        _ => {}
                    }
                }
                self.route_chat();
            }
        }
    }
    fn route_chat(&mut self) {
        for i in self.chat.take_intents() {
            match i {
                ChatIntent::Sessions(_) => {
                    self.overlay = Some(Overlay::Sessions(Box::new(
                        SessionBrowser::new(super::sessions::snapshot(), "inline")
                            .expect("valid sessions"),
                    )));
                }
                ChatIntent::Files(_) => {
                    self.overlay = Some(Overlay::Tree(Box::new(
                        TreeBrowser::new(super::trees::snapshot(false), Placement::FullScreen)
                            .expect("valid tree"),
                    )));
                }
                ChatIntent::Status { entry, .. } => match entry {
                    Entry::Status => {
                        self.status.open(Mode::Status);
                        self.overlay = Some(Overlay::Status);
                    }
                    Entry::Notices => {
                        self.status.open(Mode::Notices);
                        self.overlay = Some(Overlay::Status);
                    }
                    Entry::Attention => {
                        self.status.update(&Event::Key(crate::event::KeyEvent::new(
                            KeyCode::Char('g'),
                            KeyModifiers::CONTROL,
                        )));
                    }
                },
                _ => {}
            }
        }
        if self.chat.mode() != ChatMode::Compose
            || self.chat.composer().overlay_open()
                && self.chat.composer().completion_menu().is_none()
        {
            self.overlay = Some(Overlay::Transcript);
        }
        if self.no_alternate && self.overlay.is_some() {
            if matches!(self.overlay, Some(Overlay::Transcript))
                && !self.chat.composer().overlay_open()
            {
                self.chat.dismiss_selection();
                self.chat.update(&Event::Key(crate::event::KeyEvent::new(
                    KeyCode::End,
                    KeyModifiers::ALT,
                )));
            }
            self.overlay = None;
            self.notice = "view needs alternate screen · use the host plain list".into();
        }
    }
    fn accept(&mut self) {
        if let Region::Questions(q) = &mut self.region
            && let Some(r) = self.pending_question.take()
        {
            q.acknowledge(&r.ticket, question_form::Result::Accepted);
            let answer = q.answer_text();
            self.next(format!("question → {answer}"));
        }
        if let Region::Permission(p) = &mut self.region
            && let Some(r) = self.pending_permission.take()
        {
            p.acknowledge(&r.ticket, permission_review::Result::Applied);
            self.next(format!("review → {} · simulated host receipt", r.choice));
        }
    }
    fn refuse(&mut self) {
        if let Region::Questions(q) = &mut self.region
            && let Some(r) = self.pending_question.take()
        {
            q.acknowledge(
                &r.ticket,
                question_form::Result::Failed("host plan changed; retry explicitly".into()),
            );
        }
        if let Region::Permission(p) = &mut self.region
            && let Some(r) = self.pending_permission.take()
        {
            p.acknowledge(
                &r.ticket,
                permission_review::Result::Failed("host plan changed; retry explicitly".into()),
            );
        }
    }
    fn update_overlay(&mut self, event: &Event) {
        let mut close = false;
        if let Some(o) = &mut self.overlay {
            match o {
                Overlay::Transcript => {
                    let modal = self.chat.mode() != ChatMode::Compose
                        || self.chat.composer().overlay_open();
                    let composer_modal = self.chat.composer().overlay_open();
                    if !modal && cancel(event) {
                        close = true;
                    } else {
                        self.chat.update(event);
                        close = (cancel(event) || composer_modal)
                            && self.chat.mode() == ChatMode::Compose
                            && !self.chat.composer().overlay_open();
                    }
                }
                Overlay::Sessions(s) => {
                    s.update(event);
                    close = s.closed();
                }
                Overlay::Tree(t) => {
                    t.update(event);
                    close = t
                        .take_intents()
                        .iter()
                        .any(|i| matches!(i, tree_browser::Intent::Returned));
                }
                Overlay::Inspection(i) => {
                    i.update(event);
                    close = !i.active();
                }
                Overlay::Status => {
                    self.status.update(event);
                    close = self.status.mode() == Mode::Closed;
                }
            }
        }
        if close {
            self.overlay = None;
        }
    }
    pub(super) fn open_inspection(&mut self) {
        if self.no_alternate {
            self.notice = "inspection needs alternate screen · use the host plain list".into();
            return;
        }
        self.overlay = Some(Overlay::Inspection(Box::new(super::inspection::viewer(
            "inline-file",
            Origin::Chat,
        ))));
    }
    pub(super) fn render_live(&mut self, frame: &mut Frame<'_>, area: Rect, p: Presentation) {
        let notice_height = u16::from(self.overlay.is_none() && !self.notice.is_empty());
        self.tiny = area.width < 20 || area.height.saturating_sub(notice_height) < 3;
        frame.render_widget(Block::default().style(p.base()), area);
        if self.tiny {
            frame.render_widget(
                Paragraph::new(if area.width < 18 {
                    "esc cancel"
                } else {
                    "small | esc cancel"
                })
                .style(p.warning()),
                area,
            );
            return;
        }
        if let Some(o) = &mut self.overlay {
            match o {
                Overlay::Transcript => self.chat.render_docked_with(frame, area, p),
                Overlay::Sessions(s) => s.render_with(frame, area, p),
                Overlay::Tree(t) => t.render(frame, area, p),
                Overlay::Inspection(i) => i.render(frame, area, p),
                Overlay::Status => {
                    self.chat.render_docked_with(frame, area, p);
                    let pane = Rect {
                        height: area.height.saturating_sub(8),
                        ..area
                    };
                    if self.status.mode() == Mode::Notices {
                        self.status.render_notices(frame, pane, p);
                    } else {
                        self.status.render_sections(frame, pane, p, false);
                    }
                }
            }
            return;
        }
        if self.plain {
            frame.render_widget(Paragraph::new("Deploy image to staging?\n  1  Deploy\n  2  Deploy API only\n  3  Cancel\nchoose 1–3, no default:").style(p.base()),area);
            return;
        }
        let content = if self.notice.is_empty() {
            area
        } else {
            frame.render_widget(
                Paragraph::new(self.notice.clone()).style(p.warning()),
                Rect { height: 1, ..area },
            );
            Rect::new(
                area.x,
                area.y.saturating_add(1),
                area.width,
                area.height.saturating_sub(1),
            )
        };
        let area = content;
        match &mut self.region {
            Region::Picker(picker) => {
                let mut view = picker_view(p, area.height);
                view.source = if self.waiting.is_empty() {
                    ""
                } else {
                    "1 more waiting"
                };
                picker.render(frame, area, view, true);
            }
            Region::Questions(q) => q.render_with(frame, area, p, self.waiting.len()),
            Region::Permission(r) => r.render_with(frame, area, p, self.waiting.len()),
            Region::Dock => {
                self.chat.set_status_chrome(Some(Chrome {
                    summary: self.status.summary(area.width),
                    activity: self.status.activity(p),
                    focused: false,
                }));
                self.chat.render_inline_live(frame, area, p);
                let live = self
                    .ledger
                    .live()
                    .iter()
                    .flat_map(|part| crate::text::wrapped(&part.text, area.width))
                    .collect::<Vec<_>>()
                    .join("\n");
                frame.render_widget(
                    Paragraph::new(live).style(p.base()),
                    Rect {
                        height: area
                            .height
                            .saturating_sub(self.chat.inline_height(area.width, p)),
                        ..area
                    },
                );
            }
        }
    }
    pub(super) fn render(&mut self, frame: &mut Frame<'_>, area: Rect, p: Presentation) {
        let print_height = u16::try_from(self.printed.len() + 2)
            .unwrap_or(u16::MAX)
            .min(area.height.saturating_sub(3));
        frame.render_widget(
            Paragraph::new(self.printed.join("\n")).style(p.base()),
            Rect {
                height: print_height,
                ..area
            },
        );
        let live_height = area.height.saturating_sub(print_height);
        let preferred = self.result.as_ref().map_or_else(
            || self.height(area.width, p),
            |result| {
                u16::try_from(crate::text::wrapped(result, area.width).len()).unwrap_or(u16::MAX)
            },
        );
        let live = Rect::new(
            area.x,
            area.y + print_height,
            area.width,
            if self.overlay.is_some() {
                live_height
            } else {
                preferred.min(live_height)
            },
        );
        if let Some(result) = &self.result {
            frame.render_widget(
                Paragraph::new(crate::text::wrapped(result, area.width).join("\n")).style(p.base()),
                live,
            );
        } else {
            self.render_live(frame, live, p);
        }
    }
}
fn cancel(event: &Event) -> bool {
    matches!(event,Event::Key(k) if k.kind==KeyEventKind::Press && (k.code==KeyCode::Esc && k.modifiers==KeyModifiers::NONE || k.code==KeyCode::Char('c') && k.modifiers==KeyModifiers::CONTROL))
}
fn picker_view(p: Presentation, max_height: u16) -> PickerView<'static> {
    PickerView {
        title: "deploy to staging",
        source: "",
        context: "Deploy the fixture image to 3 services?",
        presentation: p,
        max_height,
    }
}
