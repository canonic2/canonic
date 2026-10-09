//! Defines shared semantic styling; products retain their own labels and behavior.

use ratatui::style::{Color, Modifier, Style};

/// Colours used by the common views.
#[derive(Clone, Copy, Debug, Default)]
pub enum Theme {
    /// Dark terminal palette.
    #[default]
    Dark,
    /// Light terminal palette.
    Light,
}

impl Theme {
    /// Toggle the preview palette.
    #[must_use]
    pub fn toggled(self) -> Self {
        match self {
            Self::Dark => Self::Light,
            Self::Light => Self::Dark,
        }
    }

    /// Base text and background.
    #[must_use]
    pub fn base(self) -> Style {
        match self {
            Self::Dark => Style::default().fg(Color::Gray).bg(Color::Black),
            Self::Light => Style::default().fg(Color::Black).bg(Color::White),
        }
    }

    /// Focused or selected content.
    #[must_use]
    pub fn focus(self) -> Style {
        self.base()
            .fg(match self {
                Self::Dark => Color::Cyan,
                Self::Light => Color::Blue,
            })
            .add_modifier(Modifier::BOLD)
    }

    /// Secondary text such as keyboard hints.
    #[must_use]
    pub fn muted(self) -> Style {
        self.base().fg(Color::DarkGray)
    }
}
