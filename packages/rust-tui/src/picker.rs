//! Owns searchable choice state and rendering, without performing selected actions.

use crate::text::clean;
use crate::{
    event::{Event, KeyCode, KeyEventKind, KeyModifiers},
    theme::{Presentation, Theme},
};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    widgets::{Block, List, ListItem, ListState, Paragraph},
};

/// Result of a picker interaction; indices refer to the original choices.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Continue the interaction.
    None,
    /// The user explicitly confirmed this original choice index.
    Selected(usize),
    /// The user cancelled; this does not select a choice.
    Cancelled,
}

/// A searchable single-choice view with an explicit caller-selected default.
pub struct Picker {
    title: String,
    choices: Vec<String>,
    selected: Option<usize>,
    query: String,
}

impl Picker {
    /// Create a view. Invalid defaults leave selection empty.
    #[must_use]
    pub fn new(title: impl Into<String>, choices: Vec<String>, default: Option<usize>) -> Self {
        let selected = default.filter(|index| *index < choices.len());
        Self {
            title: clean(&title.into(), 128, false),
            choices: choices
                .into_iter()
                .map(|text| clean(&text, 4096, false))
                .collect(),
            selected,
            query: String::new(),
        }
    }

    /// Original index of the highlighted item.
    #[must_use]
    pub fn selected(&self) -> Option<usize> {
        self.selected
    }

    /// Current search input.
    #[must_use]
    pub fn query(&self) -> &str {
        &self.query
    }

    fn matches(&self) -> Vec<usize> {
        let query = self.query.to_lowercase();
        self.choices
            .iter()
            .enumerate()
            .filter(|(_, text)| text.to_lowercase().contains(&query))
            .map(|(index, _)| index)
            .collect()
    }

    /// Handle a keyboard or paste event without acquiring terminal input.
    pub fn update(&mut self, event: &Event) -> Action {
        let mut search_changed = false;
        match event {
            Event::Key(key) if key.kind != KeyEventKind::Release => match key.code {
                KeyCode::Esc => return Action::Cancelled,
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    return Action::Cancelled;
                }
                KeyCode::Enter => return self.selected.map_or(Action::None, Action::Selected),
                KeyCode::Up | KeyCode::Down => {
                    let matches = self.matches();
                    let position = self
                        .selected
                        .and_then(|selected| matches.iter().position(|index| *index == selected));
                    let next = match (key.code, position) {
                        (KeyCode::Up, Some(position)) => position.saturating_sub(1),
                        (KeyCode::Down, Some(position)) => {
                            (position + 1).min(matches.len().saturating_sub(1))
                        }
                        _ => 0,
                    };
                    self.selected = matches.get(next).copied();
                }
                KeyCode::Backspace => {
                    self.query.pop();
                    search_changed = true;
                }
                KeyCode::Char(character)
                    if !key
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                {
                    if !character.is_control() && self.query.len() + character.len_utf8() <= 4096 {
                        self.query.push(character);
                    }
                    search_changed = true;
                }
                _ => {}
            },
            Event::Paste(text) => {
                self.query.push_str(&clean(
                    text,
                    4096_usize.saturating_sub(self.query.len()),
                    false,
                ));
                search_changed = true;
            }
            _ => {}
        }
        if search_changed {
            let matches = self.matches();
            if !self
                .selected
                .is_some_and(|selected| matches.contains(&selected))
            {
                self.selected = matches.first().copied();
            }
        }
        Action::None
    }

    /// Render into a caller-owned frame and area.
    pub fn render(&self, frame: &mut Frame<'_>, area: Rect, theme: Theme) {
        self.render_with(frame, area, theme.presentation());
    }

    /// Render using explicit terminal color capabilities.
    pub fn render_with(&self, frame: &mut Frame<'_>, area: Rect, theme: Presentation) {
        let block = Block::bordered()
            .title(self.title.as_str())
            .style(theme.base());
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let [search, list, hints] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .areas(inner);
        frame.render_widget(
            Paragraph::new(format!("Search: {}", self.query)).style(theme.muted()),
            search,
        );
        let matches = self.matches();
        if matches.is_empty() {
            frame.render_widget(
                Paragraph::new("No matching choices").style(theme.muted()),
                list,
            );
        } else {
            let items = matches
                .iter()
                .map(|index| ListItem::new(self.choices[*index].as_str()));
            let mut state = ListState::default().with_selected(
                self.selected
                    .and_then(|selected| matches.iter().position(|index| *index == selected)),
            );
            frame.render_stateful_widget(
                List::new(items)
                    .highlight_symbol("> ")
                    .highlight_style(theme.surface().patch(theme.focus())),
                list,
                &mut state,
            );
        }
        frame.render_widget(
            Paragraph::new("Up/Down move · Enter select · Esc cancel").style(theme.muted()),
            hints,
        );
    }
}
