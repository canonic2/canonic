//! Composes docked commands, a full-screen keys view and delayed continuation hints.

mod fixtures;

use crate::{
    commands::{
        Commands,
        bindings::{Dispatcher, Outcome},
    },
    dialog::{ClosePolicy, Dialog, DialogView, Reason, Size, Stack},
    editor::{Editor, EditorView},
    event::{Event, KeyCode, KeyEventKind, KeyModifiers},
    picker::{Intent, PickerView},
    theme::Presentation,
    ui::Rule,
};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    text::{Line, Span},
    widgets::Paragraph,
};
use std::time::Duration;

pub(super) struct CommandDemo {
    view: Commands,
    dispatcher: Dispatcher,
    draft: Editor,
    open: bool,
    remapped: bool,
    fixture: usize,
    presentation_pending: bool,
    note: String,
    notice: Duration,
    recent: Vec<String>,
    dialogs: Stack<(), u8>,
    viewport: Rect,
}
impl CommandDemo {
    pub(super) fn take_presentation(&mut self, p: Presentation) -> Option<Presentation> {
        std::mem::take(&mut self.presentation_pending).then(|| fixtures::profile(self.fixture, p))
    }
    pub(super) fn new() -> Self {
        let view = fixtures::view(false, 0);
        let dispatcher = Dispatcher::new(view.descriptors(), Duration::from_secs(3))
            .expect("valid fixture bindings");
        let mut draft = Editor::default();
        draft.set_text("and bump the app version");
        let mut demo = Self {
            view,
            dispatcher,
            draft,
            open: true,
            remapped: false,
            fixture: 0,
            presentation_pending: false,
            note: String::new(),
            notice: Duration::ZERO,
            recent: vec!["handoff".into(), "lens".into()],
            dialogs: Stack::new_timed(vec![0]).expect("unique host target"),
            viewport: Rect::default(),
        };
        demo.sync_dialog();
        demo.dialogs.tick(Duration::from_millis(300));
        demo
    }
    fn sync_dialog(&mut self) {
        self.dialogs.clear();
        if self.open {
            self.dialogs
                .push(Dialog {
                    id: "commands".into(),
                    title: "commands".into(),
                    targets: vec![1],
                    body: (),
                    size: Size::default(),
                    close: ClosePolicy::default(),
                    view: DialogView::default(),
                })
                .expect("valid host dialog");
        }
    }
    fn rebuild(&mut self) {
        self.view = fixtures::view(self.remapped, self.fixture);
        self.view
            .set_recent(&self.recent.iter().map(String::as_str).collect::<Vec<_>>());
        self.dispatcher = Dispatcher::new(self.view.descriptors(), Duration::from_secs(3))
            .expect("valid effective bindings");
        self.note.clear();
        self.notice = Duration::ZERO;
        if matches!(self.fixture, 8..=10 | 23) {
            self.open = false;
            self.dispatcher
                .update(&Event::Key(crate::event::KeyEvent::new(
                    KeyCode::Char('x'),
                    KeyModifiers::CONTROL,
                )));
            if matches!(self.fixture, 9 | 23) {
                self.dispatcher.tick(Duration::from_millis(400));
            }
            if self.fixture == 10 {
                self.dispatcher
                    .update(&Event::Key(crate::event::KeyEvent::new(
                        KeyCode::Char('z'),
                        KeyModifiers::NONE,
                    )));
                self.note = "ctrl+x z isn't a command · F1 lists them".into();
                self.notice = Duration::from_secs(3);
            }
        }
        if self.fixture == 15 {
            self.open = false;
            self.note = "! stop session didn't run · acme had already stopped".into();
            self.notice = Duration::from_secs(3);
        }
        self.sync_dialog();
        self.dialogs.tick(Duration::from_millis(300));
    }
    pub(super) fn tick(&mut self, elapsed: Duration) {
        self.draft.tick(elapsed);
        self.dialogs.tick(elapsed);
        if self.dispatcher.tick(elapsed) {
            self.note.clear();
        }
        self.notice = self.notice.saturating_sub(elapsed);
        if self.notice.is_zero() && self.dispatcher.pending().is_empty() && !self.open {
            self.note.clear();
        }
    }
    pub(super) fn input(&self) -> &str {
        self.draft.text()
    }
    fn open(&mut self, keys: bool) {
        self.view.set_query("");
        self.view.set_keys(keys);
        self.open = true;
        self.dispatcher.reset();
        self.note.clear();
        self.sync_dialog();
    }
    pub(super) fn update(&mut self, event: &Event) -> bool {
        if let Event::Key(k) = event {
            if k.modifiers.contains(KeyModifiers::CONTROL)
                && (k.code == KeyCode::Char('c') && !self.open
                    || (k.code == KeyCode::Char('t') && self.fixture != 7))
                || k.code == KeyCode::F(4)
            {
                return false;
            }
            if k.kind == KeyEventKind::Press {
                match k.code {
                    KeyCode::F(1) => {
                        self.open(true);
                        return true;
                    }
                    KeyCode::F(2) => {
                        self.fixture = (self.fixture + 1) % fixtures::NAMES.len();
                        self.presentation_pending = true;
                        self.open = true;
                        self.rebuild();
                        return true;
                    }
                    KeyCode::F(3) => {
                        self.open(!self.view.is_keys());
                        return true;
                    }
                    KeyCode::F(5) => {
                        self.remapped = !self.remapped;
                        let keys = self.view.is_keys();
                        self.rebuild();
                        self.view.set_keys(keys);
                        self.note = "bindings replaced; pending cleared".into();
                        return true;
                    }
                    KeyCode::F(6) => {
                        self.open = !self.open;
                        self.dispatcher.reset();
                        self.note.clear();
                        self.sync_dialog();
                        return true;
                    }
                    KeyCode::Esc if !self.open && self.dispatcher.pending().is_empty() => {
                        return false;
                    }
                    KeyCode::Char('p' | 'o')
                        if k.modifiers == KeyModifiers::CONTROL
                            && k.code == KeyCode::Char(if self.remapped { 'o' } else { 'p' }) =>
                    {
                        self.open(false);
                        return true;
                    }
                    _ => {}
                }
            }
        }
        if self.open {
            self.update_overlay(event);
            return true;
        }
        match self.dispatcher.update(event) {
            Outcome::Invoke(id) => {
                self.record_recent(&id);
                self.note = format!("shortcut intent: {id}");
                self.notice = Duration::from_secs(3);
            }
            Outcome::Pending(_) => {}
            Outcome::Mismatch(note) => {
                self.note = note;
                self.notice = Duration::from_secs(3);
            }
            Outcome::Cancelled => {
                self.note.clear();
            }
            Outcome::Unhandled => {
                self.draft.update(event);
            }
        }
        true
    }
    fn update_overlay(&mut self, event: &Event) {
        let mut intent = Intent::None;
        self.dialogs.dispatch(event, self.viewport, |(), _, event| {
            intent = self.view.update(event);
            intent != Intent::Cancelled
        });
        if self.dialogs.is_empty() {
            self.open = false;
        }
        match intent {
            Intent::Selected(id) => {
                self.open = false;
                self.dispatcher.reset();
                if self.fixture == 15 && id == "stop" {
                    self.note = "! stop session didn't run · acme had already stopped".into();
                } else {
                    self.record_recent(&id);
                    self.note = format!("intent: {id} · no operation executed");
                }
                self.notice = Duration::from_secs(3);
                if let Some(ticket) = self.dialogs.ticket().cloned() {
                    self.dialogs.close(&ticket, Reason::Submitted);
                }
            }
            Intent::Inspect(id) => {
                if let Some(info) = self.view.key_info(&id) {
                    self.note = format!(
                        "open source intent: {}:{}",
                        info.origin.path, info.origin.line
                    );
                }
            }
            Intent::Cancelled => {
                self.open = false;
                self.dispatcher.reset();
                self.note.clear();
            }
            _ => {}
        }
    }
    fn record_recent(&mut self, id: &str) {
        self.recent.retain(|old| old != id);
        self.recent.insert(0, id.into());
        self.recent.truncate(3);
        self.view
            .set_recent(&self.recent.iter().map(String::as_str).collect::<Vec<_>>());
    }
    pub(super) fn render(&mut self, frame: &mut Frame<'_>, area: Rect, p: Presentation) {
        self.viewport = area;
        let area = self.stage(area);
        let [body, activity, draft, hints] = Layout::vertical([
            Constraint::Min(0),
            Constraint::Length(1),
            Constraint::Length(4),
            Constraint::Length(1),
        ])
        .areas(area);
        frame.render_widget(Paragraph::new("acme · 20:05\n  Both calls now send the app's language.\n\n  edit  example/account-deletion.rs   +6 −2 applied\n  run   example checks → 6 passed · 0.9s\n\n  ✓ done · 34s").style(p.muted()),body);
        if self.open && !self.view.is_keys() && p.colors != crate::theme::ColorMode::NoColor {
            frame.buffer_mut().set_style(area, p.faded());
        }
        if self.open {
            let view = PickerView {
                max_height: if self.view.is_keys() {
                    area.height.saturating_sub(1)
                } else {
                    12
                },
                presentation: p,
                ..PickerView::default()
            };
            let height = self.view.height(body.width, view).min(body.height);
            let width = if body.width <= 40 {
                body.width
            } else {
                body.width.saturating_sub(4).min(72)
            };
            let docked = Rect::new(
                body.x + body.width.saturating_sub(width) / 2,
                body.bottom().saturating_sub(height),
                width,
                height,
            );
            if self.view.is_keys() {
                self.view.render(
                    frame,
                    Rect::new(area.x, area.y, area.width, area.height.saturating_sub(1)),
                    view,
                );
            } else {
                self.view.render(frame, docked, view);
            }
        } else if self.dispatcher.hint_visible() {
            self.hints(frame, body, p);
        }
        if !self.open || !self.view.is_keys() {
            let pending = self.dispatcher.pending();
            let line = if !pending.is_empty() {
                Line::from(Span::styled(
                    format!("{pending} … · esc cancels"),
                    p.muted(),
                ))
            } else if self.open && self.dialogs.activity() != "! dialog · esc closes it" {
                Line::from(Span::styled(self.dialogs.activity(), p.warning()))
            } else if self.open {
                Line::from(vec![
                    Span::styled(
                        "! commands",
                        p.warning()
                            .add_modifier(crate::ratatui::style::Modifier::BOLD),
                    ),
                    Span::styled(
                        if self.view.query().is_empty() {
                            " · esc closes it"
                        } else {
                            " · esc clears the filter"
                        },
                        p.muted(),
                    ),
                ])
            } else {
                Line::from(Span::styled(
                    if self.note.is_empty() {
                        "ready"
                    } else {
                        &self.note
                    },
                    p.muted(),
                ))
            };
            frame.render_widget(Paragraph::new(line), activity);
            self.draft.render_with(
                frame,
                draft,
                EditorView {
                    title: "message",
                    presentation: p,
                    ..EditorView::default()
                },
                !self.open && pending.is_empty(),
            );
        }
        self.scenario_footer(frame, hints, p);
    }
    fn stage(&self, area: Rect) -> Rect {
        match self.fixture {
            17 => Rect::new(area.x, area.y, area.width.min(64), area.height.min(12)),
            18 => Rect::new(area.x, area.y, area.width.min(40), area.height),
            _ => area,
        }
    }
    fn scenario_footer(&self, frame: &mut Frame<'_>, hints: Rect, p: Presentation) {
        let note = if self.open
            && self.view.is_keys()
            && self.dialogs.activity() != "! dialog · esc closes it"
        {
            self.dialogs.activity()
        } else if self.view.is_keys() && self.note.starts_with("open source intent:") {
            self.note.clone()
        } else {
            format!(
                "#commands:{} · F2 fixture · F1 keys · F3 palette/keys · F5 remap · F6 focus",
                fixtures::NAMES[self.fixture]
            )
        };
        frame.render_widget(Paragraph::new(note).style(p.muted()), hints);
    }
    fn hints(&self, frame: &mut Frame<'_>, body: Rect, p: Presentation) {
        let rows = self.dispatcher.continuations();
        let columns = if body.width >= 64 { 2 } else { 1 };
        let desired = u16::try_from(rows.len().div_ceil(columns) + 1)
            .unwrap_or(u16::MAX)
            .min(body.height);
        let area = Rect::new(
            body.x,
            body.bottom().saturating_sub(desired),
            body.width,
            desired,
        );
        if area.is_empty() {
            return;
        }
        frame.render_widget(
            ratatui::widgets::Block::default().style(
                p.base()
                    .bg(p.tokens().surface.unwrap_or(ratatui::style::Color::Reset)),
            ),
            area,
        );
        frame.render_widget(
            Rule {
                left: Line::from(Span::styled(self.dispatcher.pending(), p.heading())),
                right: Line::default(),
                glyph: p.symbols.rule(),
                style: p.accent(),
            },
            Rect::new(area.x, area.y, area.width, 1),
        );
        for (i, (key, id)) in rows
            .iter()
            .take(usize::from(area.height - 1) * columns)
            .enumerate()
        {
            let label = self
                .view
                .descriptors()
                .iter()
                .find(|c| &c.id == id)
                .map_or(id.as_str(), |c| c.label.as_str());
            let width = area.width / u16::try_from(columns).unwrap_or(1);
            frame.render_widget(
                Paragraph::new(format!("  {key}  {label}")).style(p.base()),
                Rect::new(
                    area.x + u16::try_from(i % columns).unwrap_or(0) * width,
                    area.y + 1 + u16::try_from(i / columns).unwrap_or(0),
                    width,
                    1,
                ),
            );
        }
    }
}
