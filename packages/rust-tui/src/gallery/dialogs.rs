//! Composes caller-owned dialog bodies, host drafts and simulated late results.

use super::dialog_body::Body;
use crate::{
    dialog::{ClosePolicy, Dialog, Dispatch, Reason, Size, Stack, Ticket},
    editor::{Editor, EditorView, Outcome},
    event::{Event, KeyCode, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind},
    theme::Presentation,
};
use ratatui::{Frame, layout::Rect, widgets::Paragraph};
use std::fmt::Write;

const NAMES: [&str; 7] = [
    "prompt",
    "nested",
    "long",
    "locked",
    "outside",
    "removed-return",
    "error",
];
pub(super) struct DialogDemo {
    drafts: [Editor; 2],
    stack: Stack<Body, usize>,
    scenario: usize,
    next: usize,
    pending: Option<Ticket>,
    note: String,
    viewport: Rect,
}
impl DialogDemo {
    pub(super) fn new() -> Self {
        let mut drafts = std::array::from_fn(|_| Editor::default());
        drafts[0].set_text("Background draft one");
        drafts[1].set_text("Background draft two");
        let mut demo = Self {
            drafts,
            stack: Stack::new(vec![0, 1]).unwrap_or_else(|_| unreachable!("distinct targets")),
            scenario: 0,
            next: 10,
            pending: None,
            note: String::new(),
            viewport: Rect::default(),
        };
        demo.reset();
        demo
    }
    pub(super) fn input(&self, index: usize) -> Option<&str> {
        self.drafts.get(index).map(Editor::text)
    }
    pub(super) fn depth(&self) -> usize {
        self.stack.len()
    }
    pub(super) fn tick(&mut self, elapsed: std::time::Duration) {
        for draft in &mut self.drafts {
            draft.tick(elapsed);
        }
        if let Some(ticket) = self.stack.ticket().cloned()
            && let Some(body) = self.stack.body_mut(&ticket)
        {
            body.tick(elapsed);
        }
    }
    fn dialog(&mut self, body: Body, title: &str) -> Dialog<Body, usize> {
        let id = self.next;
        self.next += 1;
        Dialog {
            id: format!("dialog-{id}"),
            title: title.into(),
            targets: vec![id],
            body,
            size: Size::default(),
            close: ClosePolicy::default(),
        }
    }
    fn open(&mut self, body: Body, title: &str) {
        let dialog = self.dialog(body, title);
        match self.stack.push(dialog) {
            Ok(ticket) => self.pending = Some(ticket),
            Err(error) => self.note = error.to_string(),
        }
    }
    fn reset(&mut self) {
        let _dismissed = self.stack.clear();
        self.stack = Stack::new(vec![0, 1]).unwrap_or_else(|_| unreachable!("distinct targets"));
        self.note.clear();
        match self.scenario {
            1 => {
                self.open(
                    Body::prompt("Parent draft survives the child"),
                    "Parent prompt",
                );
                self.open(Body::choices(), "Nested choice");
            }
            2 => self.open(
                Body::text(
                    &(0..60).fold(String::new(), |mut text, i| {
                        writeln!(text, "Inspection row {i:02}: supplied local content")
                            .unwrap_or_else(|_| unreachable!("String formatting cannot fail"));
                        text
                    }),
                    true,
                ),
                "Long inspection · page/home/end",
            ),
            3 | 4 => {
                let mut dialog = self.dialog(
                    Body::prompt("Enter returns this local result"),
                    "Explicit close policy",
                );
                dialog.close = ClosePolicy {
                    escape: self.scenario != 3,
                    outside_click: self.scenario == 4,
                };
                self.pending = Some(
                    self.stack
                        .push(dialog)
                        .unwrap_or_else(|_| unreachable!("valid fixture")),
                );
            }
            6 => self.open(
                Body::text(
                    "Local source error. F6 retries with a replacement prompt; Enter does nothing.",
                    false,
                ),
                "Local error",
            ),
            _ => self.open(
                Body::prompt("Modal draft · Cafe\u{301} · 界 · 👩‍💻"),
                "Draft prompt · Enter returns / Ctrl-J newline",
            ),
        }
        if self.scenario == 5 {
            self.stack.remove_target(&0);
            self.note = "Return target 0 removed; close returns to draft two".into();
        }
    }
    fn dismissed(&mut self, id: &str, reason: Reason) {
        self.note = format!("Dismissed {id}: {reason:?} · no operation executed");
    }
    pub(super) fn update(&mut self, event: &Event) -> bool {
        if let Event::Key(key) = event {
            if key.kind == KeyEventKind::Release {
                return true;
            }
            if key.modifiers.contains(KeyModifiers::CONTROL)
                && matches!(key.code, KeyCode::Char('c' | 't'))
            {
                return false;
            }
            match key.code {
                KeyCode::F(4) => return false,
                KeyCode::F(2) => {
                    self.scenario = (self.scenario + 1) % NAMES.len();
                    self.reset();
                    return true;
                }
                KeyCode::F(3) => {
                    self.open(Body::prompt("Nested inspection draft"), "Nested prompt");
                    return true;
                }
                KeyCode::F(5) => {
                    self.stack.remove_target(&0);
                    self.note = "Return target 0 removed; fallback is draft two".into();
                    return true;
                }
                KeyCode::F(6) => {
                    let dialog =
                        self.dialog(Body::prompt("Replacement draft"), "Replacement prompt");
                    match self.stack.replace(dialog) {
                        Ok((_, old)) => self.dismissed(&old.id, old.reason),
                        Err(error) => self.note = error.to_string(),
                    }
                    return true;
                }
                KeyCode::F(7) => {
                    self.note = if self
                        .pending
                        .as_ref()
                        .is_some_and(|ticket| self.stack.accepts(ticket))
                    {
                        "Result ticket belongs to active opening"
                    } else {
                        "Rejected stale result; draft unchanged"
                    }
                    .into();
                    return true;
                }
                KeyCode::F(8) => {
                    let dismissed = self.stack.clear();
                    self.note = format!(
                        "Cleared {} layers; host cancels their work",
                        dismissed.len()
                    );
                    return true;
                }
                KeyCode::F(9) => {
                    let mouse = Event::Mouse(MouseEvent {
                        kind: MouseEventKind::Down(MouseButton::Left),
                        column: self.viewport.x,
                        row: self.viewport.y,
                        modifiers: KeyModifiers::NONE,
                    });
                    if let Dispatch::Dismissed(old) =
                        self.stack.dispatch(&mouse, self.viewport, |_, _, _| false)
                    {
                        self.dismissed(&old.id, old.reason);
                    }
                    return true;
                }
                _ => {}
            }
        }
        let ticket = self.stack.ticket().cloned();
        let mut result = None;
        match self.stack.dispatch(event, self.viewport, |body, _, event| {
            let (consumed, value) = body.update(event);
            result = value;
            consumed
        }) {
            Dispatch::Dismissed(old) => self.dismissed(&old.id, old.reason),
            Dispatch::Consumed => {
                if let (Some(ticket), Some(value)) = (ticket, result)
                    && let Some(old) = self.stack.close(&ticket, Reason::Submitted)
                {
                    self.note = format!("{value} · {} closed; fixture only", old.id);
                }
            }
            Dispatch::Background => {
                return self.background(event);
            }
        }
        true
    }
    fn background(&mut self, event: &Event) -> bool {
        if let Event::Key(key) = event
            && matches!(key.code, KeyCode::Tab | KeyCode::BackTab)
        {
            self.stack.advance(
                key.code == KeyCode::BackTab || key.modifiers.contains(KeyModifiers::SHIFT),
            );
            return true;
        }
        if let Some(&index) = self.stack.focused() {
            return self.drafts[index].update(event) != Outcome::Unhandled
                || !matches!(event,Event::Key(key) if key.code==KeyCode::Esc);
        }
        !matches!(event,Event::Key(key) if key.code==KeyCode::Esc)
    }
    pub(super) fn render(&mut self, frame: &mut Frame<'_>, area: Rect, p: Presentation) {
        self.viewport = Rect::new(area.x, area.y, area.width, area.height.saturating_sub(2));
        let half = self.viewport.height / 2;
        for (index, draft) in self.drafts.iter_mut().enumerate() {
            let rect = if index == 0 {
                Rect::new(area.x, area.y, area.width, half)
            } else {
                Rect::new(
                    area.x,
                    area.y + half,
                    area.width,
                    self.viewport.height - half,
                )
            };
            draft.render_with(
                frame,
                rect,
                EditorView {
                    title: if index == 0 {
                        "background one"
                    } else {
                        "background two"
                    },
                    presentation: p,
                    ..EditorView::default()
                },
                self.stack.is_empty() && self.stack.focused() == Some(&index),
            );
        }
        self.stack
            .render(frame, self.viewport, p, |body, frame, area, target| {
                body.render(frame, area, p, target.is_some());
            });
        if area.height >= 2 {
            let hint = format!(
                "#dialog:{} · F2 fixture · F3 nest · F5 remove · F6 replace · F7 late · F8 clear · F9 outside",
                NAMES[self.scenario]
            );
            frame.render_widget(
                Paragraph::new(hint).style(p.muted()),
                Rect::new(area.x, area.bottom() - 2, area.width, 1),
            );
            frame.render_widget(
                Paragraph::new(self.note.as_str()).style(p.accent()),
                Rect::new(area.x, area.bottom() - 1, area.width, 1),
            );
        }
    }
}
