//! Supplies dialog fixture bodies through public component APIs, without host operations.

use crate::{
    dialog::{Caution, Confirmation, Decision},
    editor::{Editor, EditorView, Outcome},
    event::{Event, KeyCode, KeyEventKind},
    picker::{Choice, ChoicePicker, Filter, Intent, SelectionMode},
    theme::Presentation,
};
use ratatui::{
    Frame,
    layout::Rect,
    style::Color,
    widgets::{Block, Paragraph},
};

pub(super) enum Body {
    Prompt(Box<Editor>),
    Choices(Box<ChoicePicker>, Confirmation),
    Named {
        editor: Box<Editor>,
        confirmation: Confirmation,
        name: String,
    },
    Text {
        text: String,
        offset: usize,
        height: usize,
        acknowledge: bool,
    },
}
impl Body {
    pub(super) fn prompt(text: &str) -> Self {
        let mut editor = Editor::new(2 * 1024);
        editor.set_text(text);
        Self::Prompt(Box::new(editor))
    }
    pub(super) fn choices() -> Self {
        Self::Choices(
            Box::new(
                ChoicePicker::new(
                    vec![Choice::new("cancel", "Keep this draft"), {
                        let mut c = Choice::new("delete", "Discard this draft");
                        c.danger = true;
                        c
                    }],
                    Some("cancel"),
                    SelectionMode::Single,
                    Filter::Off,
                )
                .unwrap_or_else(|_| unreachable!("fixture IDs are unique")),
            ),
            Confirmation::default(),
        )
    }
    pub(super) fn named(text: &str) -> Self {
        let mut editor = Editor::new(2 * 1024);
        editor.set_text(text);
        Self::Named {
            editor: Box::new(editor),
            confirmation: Confirmation::default(),
            name: "acme#22".into(),
        }
    }
    pub(super) fn locked() -> Self {
        let mut body = Self::choices();
        if let Self::Choices(picker, _) = &mut body {
            let mut choices = picker.choices().to_vec();
            choices[0].label = "No · keep this draft".into();
            choices[1].label = "Yes · discard this draft".into();
            picker
                .replace_choices(choices)
                .unwrap_or_else(|_| unreachable!("unchanged IDs"));
        }
        body
    }
    pub(super) fn filtered(query: &str) -> Self {
        let mut picker = ChoicePicker::new(
            vec![
                Choice::new("ada", "ada.k"),
                Choice::new("maya", "maya.lin"),
                Choice::new("marco", "marco.r"),
                Choice::new("tomas", "tomas.v"),
            ],
            Some("maya"),
            SelectionMode::Single,
            Filter::On,
        )
        .unwrap_or_else(|_| unreachable!("distinct fixture IDs"));
        picker.set_query(query);
        Self::Choices(Box::new(picker), Confirmation::default())
    }
    pub(super) fn activity(&self) -> Option<&str> {
        match self {
            Self::Choices(picker, confirm)
                if picker.selected().is_some_and(|id| confirm.armed(id)) =>
            {
                Some("! dialog · enter again deletes · esc cancels")
            }
            Self::Choices(picker, _) if !picker.query().is_empty() => {
                Some("! dialog · esc clears filter")
            }
            _ => None,
        }
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
            Self::Choices(p, confirm) => {
                p.tick(elapsed);
                confirm.tick(elapsed);
                refresh_choice(p, confirm);
            }
            Self::Named {
                editor,
                confirmation,
                ..
            } => {
                editor.tick(elapsed);
                confirmation.tick(elapsed);
            }
            Self::Text { .. } => {}
        }
    }
    pub(super) fn height(&self, width: u16, boxed: bool, p: Presentation) -> u16 {
        let rows = match self {
            Self::Prompt(e) | Self::Named { editor: e, .. } => {
                e.visual_rows(width, p.symbols)
                    .clamp(1, crate::theme::tokens::spacing::DIALOG_FIELD_ROWS)
                    + usize::from(boxed) * 2
            }
            Self::Choices(c, _) => c.choices().len().max(1) + usize::from(c.filtering()),
            Self::Text { text, .. } => crate::text::wrapped(text, width.max(1)).len().max(1),
        };
        u16::try_from(rows).unwrap_or(u16::MAX)
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
            Self::Choices(picker, confirmation) => choice_update(picker, confirmation, event),
            Self::Named {
                editor,
                confirmation,
                name,
            } => {
                if enter {
                    let decision = confirmation.update(
                        event,
                        "delete-session",
                        &Caution::TypeName(name.clone()),
                        editor.text(),
                    );
                    return (
                        true,
                        (decision == Decision::Confirm)
                            .then(|| "Deleted named session · fixture only".into()),
                    );
                }
                (editor.update(event) != Outcome::Unhandled, None)
            }
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
        boxed: bool,
    ) -> Option<String> {
        match self {
            Self::Prompt(e) | Self::Named { editor: e, .. } => {
                frame.render_widget(
                    Block::default().style(p.base().fg(Color::Reset).bg(Color::Reset)),
                    area,
                );
                e.render_with(
                    frame,
                    area,
                    EditorView {
                        placeholder: "Type a local draft",
                        title: if boxed { "name" } else { "" },
                        chrome: boxed && area.height >= 3,
                        presentation: p,
                        ..EditorView::default()
                    },
                    focused,
                );
                (!boxed && e.first_visible_row() > 0)
                    .then(|| format!("{} {} more", p.symbols.more(true), e.first_visible_row()))
            }
            Self::Choices(choices, _) => {
                choices.render_list(frame, area, p, focused);
                None
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
                (*offset > 0).then(|| format!("{} {} more", p.symbols.more(true), offset))
            }
        }
    }
}

fn choice_update(
    picker: &mut ChoicePicker,
    confirm: &mut Confirmation,
    event: &Event,
) -> (bool, Option<String>) {
    let previous = picker.selected().map(str::to_owned);
    let intent = picker.update(event);
    if previous.as_deref() != picker.selected()
        || matches!(event, Event::Key(key) if key.code == KeyCode::Esc)
    {
        confirm.disarm();
    }
    let result = match intent {
        Intent::Selected(id) => {
            let danger = picker
                .choices()
                .iter()
                .any(|choice| choice.id == id && choice.danger);
            if danger {
                (confirm.update(event, &id, &Caution::Irreversible, "") == Decision::Confirm)
                    .then(|| format!("Choice result: {id}"))
            } else {
                Some(format!("Choice result: {id}"))
            }
        }
        Intent::Cancelled => return (false, None),
        _ => None,
    };
    refresh_choice(picker, confirm);
    (true, result)
}
fn refresh_choice(picker: &mut ChoicePicker, confirm: &Confirmation) {
    let mut choices = picker.choices().to_vec();
    let mut changed = false;
    for choice in &mut choices {
        if choice.danger {
            let detail = if confirm.armed(&choice.id) {
                "enter again"
            } else {
                ""
            };
            if choice.detail != detail {
                choice.detail = detail.into();
                changed = true;
            }
        }
    }
    if changed {
        picker
            .replace_choices(choices)
            .unwrap_or_else(|_| unreachable!("unchanged fixture IDs"));
    }
}
