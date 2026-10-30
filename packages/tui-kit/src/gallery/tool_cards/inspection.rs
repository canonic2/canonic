//! Full retained-output viewer with local literal search and simulated copy-all.

use crate::{
    editor::{Editor, EditorView},
    event::{Event, KeyCode, KeyEventKind, KeyModifiers},
    theme::Presentation,
    tool_call_card::Call,
};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    widgets::Paragraph,
};

pub(super) struct Inspection {
    patch: Option<crate::inspection::Inspector>,
    title: String,
    text: String,
    row: usize,
    query: Editor,
    searching: bool,
    notice: String,
    next: usize,
    width: u16,
    lines: Vec<String>,
    starts: Vec<usize>,
}
impl Inspection {
    pub(super) fn new(call: &Call) -> Self {
        Self {
            patch: if let crate::tool_call_card::Output::Diff(text) = &call.output {
                let mut item = crate::inspection::Item::new(&call.id, &call.target);
                item.summary.clone_from(&call.summary);
                Some(
                    crate::inspection::Inspector::new(
                        item.clone(),
                        crate::inspection::State::Ready(crate::inspection::Content::Diff {
                            patch: text.clone(),
                            new_file: None,
                        }),
                    )
                    .unwrap_or_else(|error| {
                        crate::inspection::Inspector::new(
                            item,
                            crate::inspection::State::Failed(error.to_string()),
                        )
                        .expect("bounded fixture")
                    }),
                )
            } else {
                None
            },
            title: format!(
                "orbit-api#276 · {} {} · {}",
                call.verb, call.target, call.summary
            ),
            text: call.output.text().into(),
            row: 0,
            query: Editor::new(1024),
            searching: false,
            notice: String::new(),
            next: 0,
            width: 0,
            lines: Vec::new(),
            starts: Vec::new(),
        }
    }
    pub(super) fn searching(&self) -> bool {
        self.patch.is_some() || self.searching
    }
    pub(super) fn finished(&self) -> bool {
        self.patch.as_ref().is_some_and(|v| !v.active())
    }
    pub(super) fn update(&mut self, event: &Event) {
        if let Some(viewer) = &mut self.patch {
            viewer.update(event);
            for intent in viewer.take_intents() {
                if let crate::inspection::Intent::Request(r) = intent {
                    let outcome = match r.operation {
                        crate::inspection::Operation::Copy { .. } => {
                            crate::inspection::Outcome::Copied
                        }
                        crate::inspection::Operation::Editor { .. } => {
                            crate::inspection::Outcome::Opened
                        }
                        crate::inspection::Operation::Load => crate::inspection::Outcome::Failed(
                            "patch unavailable in this fixture".into(),
                        ),
                    };
                    let _ = viewer.complete(&r, outcome);
                }
            }
            return;
        }
        if let Event::Key(k) = event {
            if k.kind != KeyEventKind::Press {
                return;
            }
            if self.searching {
                match k.code {
                    KeyCode::Esc => self.searching = false,
                    KeyCode::Enter => {
                        self.searching = false;
                        self.find();
                    }
                    _ => {
                        self.query.update(event);
                        self.next = 0;
                        self.find();
                    }
                }
                return;
            }
            if k.modifiers != KeyModifiers::NONE {
                return;
            }
            match k.code {
                KeyCode::Char('/') => {
                    self.searching = true;
                    self.query.set_text("");
                    self.next = 0;
                }
                KeyCode::Char('n') => self.find(),
                KeyCode::Char('y') => {
                    self.notice = format!(
                        "✓ copied all · {} lines · simulated",
                        self.text.lines().count()
                    );
                }
                KeyCode::Up => self.row = self.row.saturating_sub(1),
                KeyCode::Down => self.row = self.row.saturating_add(1),
                KeyCode::PageUp => self.row = self.row.saturating_sub(10),
                KeyCode::PageDown => self.row = self.row.saturating_add(10),
                KeyCode::Home => self.row = 0,
                KeyCode::End => self.row = self.lines.len().saturating_sub(1),
                _ => {}
            }
        } else if self.searching {
            self.query.update(event);
            self.next = 0;
            self.find();
        }
    }
    fn find(&mut self) {
        let query = self.query.text().to_lowercase();
        if query.is_empty() {
            return;
        }
        let matches: Vec<_> = self
            .text
            .lines()
            .enumerate()
            .filter(|(_, line)| line.to_lowercase().contains(&query))
            .map(|(i, _)| i)
            .collect();
        if matches.is_empty() {
            self.notice = "no matches in full output".into();
        } else {
            let index = self.next % matches.len();
            self.row = self.starts.get(matches[index]).copied().unwrap_or(0);
            self.next = index + 1;
            self.notice = format!("{} of {} matches", index + 1, matches.len());
        }
    }
    pub(super) fn render(&mut self, frame: &mut Frame<'_>, area: Rect, theme: Presentation) {
        if let Some(viewer) = &mut self.patch {
            viewer.render(frame, area, theme);
            return;
        }
        let [title, body, notice, query, footer] = Layout::vertical([
            Constraint::Length(2),
            Constraint::Min(0),
            Constraint::Length(1),
            Constraint::Length(u16::from(self.searching)),
            Constraint::Length(1),
        ])
        .areas(area);
        frame.render_widget(
            Paragraph::new(self.title.as_str()).style(theme.heading()),
            title,
        );
        if self.width != body.width {
            let source = self
                .starts
                .partition_point(|start| *start <= self.row)
                .saturating_sub(1);
            self.lines.clear();
            self.starts.clear();
            for line in self.text.lines() {
                self.starts.push(self.lines.len());
                self.lines
                    .extend(crate::text::wrapped(line, body.width.max(1)));
            }
            self.row = self.starts.get(source).copied().unwrap_or(0);
            self.width = body.width;
        }
        self.row = self.row.min(self.lines.len().saturating_sub(1));
        let text = self
            .lines
            .iter()
            .skip(self.row)
            .take(usize::from(body.height))
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join("\n");
        frame.render_widget(Paragraph::new(text).style(theme.base()), body);
        frame.render_widget(
            Paragraph::new(self.notice.as_str()).style(theme.muted()),
            notice,
        );
        if self.searching {
            self.query.render_with(
                frame,
                query,
                EditorView {
                    chrome: false,
                    presentation: theme,
                    ..EditorView::default()
                },
                true,
            );
        }
        frame.render_widget(
            Paragraph::new("/ find · n next · y copy all · esc back to chat").style(theme.muted()),
            footer,
        );
    }
}
