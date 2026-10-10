//! Supplies dialog fixture bodies through public component APIs, without host operations.

use crate::{
    editor::{Editor, EditorView, Outcome},
    event::{Event, KeyCode, KeyEventKind},
    picker::{Choice, ChoicePicker, Filter, Intent, PickerView, SelectionMode},
    theme::Presentation,
};
use ratatui::{Frame, layout::Rect, widgets::Paragraph};

pub(super) enum Body {
    Prompt(Editor),
    Choices(ChoicePicker),
    Text {
        text: String,
        offset: usize,
        height: usize,
        acknowledge: bool,
    },
}
impl Body {
    pub(super) fn prompt(text: &str) -> Self {
        let mut editor = Editor::default();
        editor.set_text(text);
        Self::Prompt(editor)
    }
    pub(super) fn choices() -> Self {
        Self::Choices(
            ChoicePicker::new(
                vec![
                    Choice::new("save", "Keep this draft"),
                    Choice::new("cancel", "Cancel"),
                ],
                Some("cancel"),
                SelectionMode::Single,
                Filter::Off,
            )
            .unwrap_or_else(|_| unreachable!("fixture IDs are unique")),
        )
    }
    pub(super) fn text(text: &str, acknowledge: bool) -> Self {
        Self::Text {
            text: crate::text::clean(text, 64 * 1024, true),
            offset: 0,
            height: 1,
            acknowledge,
        }
    }
    pub(super) fn tick(&mut self, elapsed: std::time::Duration) {
        match self {
            Self::Prompt(e) => e.tick(elapsed),
            Self::Choices(p) => p.tick(elapsed),
            Self::Text { .. } => {}
        }
    }
    pub(super) fn update(&mut self, event: &Event) -> (bool, Option<String>) {
        let enter = matches!(event,Event::Key(k) if k.code==KeyCode::Enter && k.kind==KeyEventKind::Press && k.modifiers.is_empty());
        match self {
            Self::Prompt(editor) => {
                if enter {
                    return (
                        true,
                        Some(format!(
                            "Prompt result: {}",
                            crate::text::clean(editor.text(), 4096, false)
                        )),
                    );
                }
                (editor.update(event) != Outcome::Unhandled, None)
            }
            Self::Choices(picker) => match picker.update(event) {
                Intent::Selected(id) => (true, Some(format!("Choice result: {id}"))),
                Intent::Cancelled => (false, None),
                _ => (true, None),
            },
            Self::Text {
                text,
                offset,
                height,
                acknowledge,
            } => {
                if enter && *acknowledge {
                    return (true, Some("Acknowledged inspection".into()));
                }
                if let Event::Key(k) = event {
                    match k.code {
                        KeyCode::Down => *offset = offset.saturating_add(1),
                        KeyCode::Up => *offset = offset.saturating_sub(1),
                        KeyCode::PageDown => *offset = offset.saturating_add(*height),
                        KeyCode::PageUp => *offset = offset.saturating_sub(*height),
                        KeyCode::Home => *offset = 0,
                        KeyCode::End => *offset = text.len(),
                        _ => return (false, None),
                    }
                    return (true, None);
                }
                (false, None)
            }
        }
    }
    pub(super) fn render(
        &mut self,
        frame: &mut Frame<'_>,
        area: Rect,
        p: Presentation,
        focused: bool,
    ) {
        match self {
            Self::Prompt(e) => e.render_with(
                frame,
                area,
                EditorView {
                    placeholder: "Type a local draft",
                    chrome: false,
                    presentation: p,
                    ..EditorView::default()
                },
                focused,
            ),
            Self::Choices(choices) => {
                choices.render(
                    frame,
                    area,
                    PickerView {
                        title: "choose",
                        max_height: area.height,
                        presentation: p,
                        ..PickerView::default()
                    },
                    focused,
                );
            }
            Self::Text {
                text,
                offset,
                height,
                ..
            } => {
                let rows = crate::text::wrapped(text, area.width);
                *height = usize::from(area.height);
                *offset = (*offset).min(rows.len().saturating_sub((*height).max(1)));
                frame.render_widget(
                    Paragraph::new(
                        rows.into_iter()
                            .skip(*offset)
                            .take(*height)
                            .collect::<Vec<_>>()
                            .join("\n"),
                    )
                    .style(p.base()),
                    area,
                );
            }
        }
    }
}
