//! Owns semantic color, symbol and cell tokens independently of components.

use ratatui::style::Color;

/// Host-supplied terminal color capability.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ColorMode {
    /// Follow the terminal's sixteen color slots and default body colors.
    #[default]
    Ansi16,
    /// Indexed palette.
    Ansi256,
    /// RGB palette.
    TrueColor,
    /// Attributes only, without color overrides.
    NoColor,
}

/// Decoration alphabet; document text is never transliterated.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Symbols {
    /// Unicode rules and arrows.
    #[default]
    Unicode,
    /// ASCII rules and direction words.
    Ascii,
}

impl Symbols {
    /// One-cell rule character.
    #[must_use]
    pub const fn rule(self) -> char {
        match self {
            Self::Unicode => '─',
            Self::Ascii => '-',
        }
    }
    /// Scroll direction.
    #[must_use]
    pub const fn more(self, above: bool) -> &'static str {
        match (self, above) {
            (Self::Unicode, true) => "↑",
            (Self::Unicode, false) => "↓",
            (Self::Ascii, true) => "up",
            (Self::Ascii, false) => "down",
        }
    }
    /// Metadata separator.
    #[must_use]
    pub const fn separator(self) -> &'static str {
        match self {
            Self::Unicode => " · ",
            Self::Ascii => " | ",
        }
    }
    /// Truncated-label marker.
    #[must_use]
    pub const fn ellipsis(self) -> char {
        match self {
            Self::Unicode => '⋯',
            Self::Ascii => '.',
        }
    }
}

/// Semantic colors. `None` preserves the user's terminal color.
#[derive(Clone, Copy, Debug)]
pub struct Colors {
    /// Body text.
    pub foreground: Option<Color>,
    /// Body background.
    pub background: Option<Color>,
    /// Secondary text and inactive rules.
    pub dim: Option<Color>,
    /// Obscured content behind an active modal, distinct from secondary text.
    pub faded: Option<Color>,
    /// Focus and current identity.
    pub accent: Option<Color>,
    /// Successful results.
    pub success: Option<Color>,
    /// Limits and attention.
    pub warning: Option<Color>,
    /// Failed results.
    pub error: Option<Color>,
    /// Paths and references.
    pub info: Option<Color>,
    /// Selected list row background; reverse when absent.
    pub surface: Option<Color>,
    /// Editor selection background; reverse when absent.
    pub selection: Option<Color>,
}

/// Shared dimensions, measured in terminal cells.
pub mod spacing {
    /// Body indentation.
    pub const INDENT: u16 = 2;
    /// Gap between independent views.
    pub const GAP: u16 = 1;
    /// Editor title/footer rows.
    pub const EDITOR_CHROME: u16 = 2;
    /// Tab stops.
    pub const TAB_STOP: usize = 4;
    /// Caret context rows.
    pub const CURSOR_CONTEXT: usize = 1;
    /// Default maximum height for a compact picker, including chrome.
    pub const PICKER_HEIGHT: u16 = 9;
    /// Minimum useful second-column width before details disappear.
    pub const PICKER_DETAIL: usize = 12;
    /// Clear separation between picker labels and aligned detail metadata.
    pub const PICKER_GAP: usize = 2;
    /// Default preferred dialog width; host area always clamps it.
    pub const DIALOG_WIDTH: u16 = 72;
    /// Default preferred dialog height, including chrome.
    pub const DIALOG_HEIGHT: u16 = 14;
    /// Prompt field growth limit before scrolling.
    pub const DIALOG_FIELD_ROWS: usize = 6;
    /// Width below which optional scope and verbose footer labels drop.
    pub const DIALOG_SCOPE_WIDTH: u16 = 48;
    /// Vertical breathing room around full-screen panels.
    pub const DIALOG_PANEL_GAP: u16 = 4;
    /// Persistent activity, composer and context rows in the dialog gallery dock.
    pub const DIALOG_DOCK_HEIGHT: u16 = 7;
}
