//! Composes caller-owned dialog bodies, host drafts and simulated late results.

use super::dialog_body::Body;
use crate::{
    dialog::{ClosePolicy, Dialog, DialogView, Dispatch, Reason, Size, Stack, Ticket},
    editor::{Editor, EditorView, Outcome},
    event::{Event, KeyCode, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind},
    theme::Presentation,
};
use ratatui::{
    Frame,
    layout::Rect,
    text::{Line, Span},
    widgets::Paragraph,
};
use std::fmt::Write;

const NAMES: [&str; 11] = [
    "prompt",
    "nested",
    "long",
    "locked",
    "outside",
    "removed-return",
    "error",
    "confirm",
    "notice",
    "prompt-long",
    "rename",
];
pub(super) struct DialogDemo {
    drafts: [Editor; 2],
    composer: Editor,
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
        let mut composer = Editor::default();
        composer.set_text("and then open the PR");
        let mut demo = Self {
            drafts,
            composer,
            stack: Stack::new(vec![0, 1, 2]).unwrap_or_else(|_| unreachable!("distinct targets")),
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
        self.composer.tick(elapsed);
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
            size: Size::default(),
            close: ClosePolicy::default(),
            view: DialogView {
                footer: match &body {
                    Body::Prompt(_) => "enter done · ctrl+j newline · esc cancel",
                    Body::Choices(_) => "1–2 · enter · esc keep",
                    Body::Text {
                        acknowledge: true, ..
                    } => "enter ok · esc cancel",
                    Body::Text { .. } => "esc cancel",
                }
                .into(),
                compact_footer: match &body {
                    Body::Text {
                        acknowledge: false, ..
                    } => "esc",
                    _ => "enter · esc",
                }
                .into(),
                ..DialogView::default()
            },
            body,
        }
    }
    fn open(&mut self, body: Body, title: &str) {
        let mut dialog = self.dialog(body, title);
        if !self.stack.is_empty() {
            dialog.view.footer = dialog.view.footer.replace("esc cancel", "esc back");
        }
        match self.stack.push(dialog) {
            Ok(ticket) => self.pending = Some(ticket),
            Err(error) => self.note = error.to_string(),
        }
    }
    fn reset(&mut self) {
        let _dismissed = self.stack.clear();
        self.stack = Stack::new(vec![0, 1, 2]).unwrap_or_else(|_| unreachable!("distinct targets"));
        self.note.clear();
        match self.scenario {
            1 => {
                self.open(
                    Body::prompt("Parent draft survives the child"),
                    "parent prompt",
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
                "Long inspection",
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
                if self.scenario == 3 {
                    dialog.view.footer = "enter done · ctrl+j newline".into();
                    dialog.view.compact_footer = "enter".into();
                }
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
            7 => {
                let mut dialog = self.dialog(Body::choices(), "Discard draft");
                dialog.view.message = "This draft is local. Discarding it cannot be undone in this fixture.".into();
                dialog.view.scope = "example project".into();
                self.pending = Some(self.stack.push(dialog).unwrap_or_else(|_| unreachable!("valid fixture")));
            }
            8 => self.open(Body::text("Your example token expired. The fixture will ask again when it needs one.", true), "Signed out"),
            9 => self.open(Body::prompt("Modal draft · Café · 界 · 🧑\nSecond line from Ctrl-J\nThird line\nFourth line\nFifth line\nSixth line\nSeventh line\nEighth line"), "draft prompt"),
            10 => {
                let mut dialog = self.dialog(Body::prompt("EXECDEV-22-example"), "Rename branch");
                dialog.view.message = "Renames EXECDEV-22 here and on origin. Open pull requests follow the new name.".into();
                dialog.view.scope = "example project".into();
                dialog.view.footer = "enter rename · esc cancel".into();
                self.pending = Some(self.stack.push(dialog).unwrap_or_else(|_| unreachable!("valid fixture")));
            }
            _ => self.open(
                Body::prompt("Modal draft · Cafe\u{301} · 界 · 👩‍💻"),
                "draft prompt",
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
                    self.open(Body::prompt("Nested inspection draft"), "nested prompt");
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
            let draft = if index == 2 {
                &mut self.composer
            } else {
                &mut self.drafts[index]
            };
            return draft.update(event) != Outcome::Unhandled
                || !matches!(event,Event::Key(key) if key.code==KeyCode::Esc);
        }
        !matches!(event,Event::Key(key) if key.code==KeyCode::Esc)
    }
    pub(super) fn render(&mut self, frame: &mut Frame<'_>, area: Rect, p: Presentation) {
        let dock_y = area
            .bottom()
            .saturating_sub(crate::theme::tokens::spacing::DIALOG_DOCK_HEIGHT + 2)
            .max(area.y);
        self.viewport = area;
        for (index, draft) in self.drafts.iter_mut().enumerate() {
            let y = area
                .y
                .saturating_add(u16::try_from(index).unwrap_or(0) * 4)
                .min(dock_y);
            let rect = Rect::new(area.x, y, area.width, dock_y.saturating_sub(y).min(3));
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
        self.composer.render_with(
            frame,
            Rect::new(
                area.x,
                dock_y.saturating_add(1).min(area.bottom()),
                area.width,
                area.bottom().saturating_sub(dock_y + 1).min(5),
            ),
            EditorView {
                title: "message",
                presentation: p,
                ..EditorView::default()
            },
            self.stack.is_empty() && self.stack.focused() == Some(&2),
        );
        self.render_activity(frame, area, dock_y, p);
        if dock_y + 6 < area.bottom().saturating_sub(2) {
            frame.render_widget(
                Paragraph::new("example project · chat").style(p.muted()),
                Rect::new(area.x, dock_y + 6, area.width, 1),
            );
        }
        self.render_hints(frame, area, p);
        self.stack.render_with(
            frame,
            self.viewport,
            p,
            |body, width, boxed| body.height(width, boxed, p),
            |body, frame, area, target, boxed| body.render(frame, area, p, target.is_some(), boxed),
        );
        if self
            .stack
            .area(self.viewport)
            .is_none_or(|panel| dock_y < panel.y || dock_y >= panel.bottom())
        {
            self.render_activity(frame, area, dock_y, p);
        }
    }
    fn render_activity(&self, frame: &mut Frame<'_>, area: Rect, dock_y: u16, p: Presentation) {
        if dock_y < area.bottom() {
            let (status, hint) = if self.stack.is_empty() {
                ("ready".to_owned(), String::new())
            } else if self.stack.len() > 1 {
                (
                    format!("! dialog {} of {}", self.stack.len(), self.stack.len()),
                    " · esc goes back".to_owned(),
                )
            } else {
                (
                    "! dialog".to_owned(),
                    if self
                        .stack
                        .close_policy()
                        .is_some_and(|policy| policy.escape)
                    {
                        " · esc closes it"
                    } else {
                        " · enter finishes it"
                    }
                    .to_owned(),
                )
            };
            frame.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled(
                        status,
                        if self.stack.is_empty() {
                            p.muted()
                        } else {
                            p.warning()
                        },
                    ),
                    Span::styled(hint, p.muted()),
                ]))
                .style(p.base().remove_modifier(ratatui::style::Modifier::DIM)),
                Rect::new(area.x, dock_y, area.width, 1),
            );
        }
    }
    fn render_hints(&self, frame: &mut Frame<'_>, area: Rect, p: Presentation) {
        if area.height >= 2 {
            let hint = format!(
                "#dialog:{} · F2 fixture · F3 nest · F6 replace · F8 clear",
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
