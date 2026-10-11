//! Resolves semantic tokens and text styles; hosts choose terminal capabilities.

pub mod tokens;

use ratatui::style::{Color, Modifier, Style};
pub use tokens::{ColorMode, Colors, Symbols};

/// Background polarity for RGB tokens; ANSI modes follow the terminal palette.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Theme {
    /// Dark palette.
    #[default]
    Dark,
    /// Light palette.
    Light,
}

/// Explicit design-system inputs; performs no environment reads or queries.
#[derive(Clone, Copy, Debug, Default)]
pub struct Presentation {
    /// Background polarity.
    pub theme: Theme,
    /// Color capability.
    pub colors: ColorMode,
    /// Decoration alphabet.
    pub symbols: Symbols,
}

impl Theme {
    /// Toggle background polarity.
    #[must_use]
    pub const fn toggled(self) -> Self {
        match self {
            Self::Dark => Self::Light,
            Self::Light => Self::Dark,
        }
    }
    /// Terminal-native presentation.
    #[must_use]
    pub fn presentation(self) -> Presentation {
        Presentation {
            theme: self,
            ..Presentation::default()
        }
    }
    /// Body style.
    #[must_use]
    pub fn base(self) -> Style {
        self.presentation().base()
    }
    /// Focused heading.
    #[must_use]
    pub fn focus(self) -> Style {
        self.presentation().focus()
    }
    /// Secondary text.
    #[must_use]
    pub fn muted(self) -> Style {
        self.presentation().muted()
    }
}

impl Presentation {
    /// Solid draft rule, distinct from its brighter label.
    #[must_use]
    pub fn message_line(self) -> Style {
        Self::foreground(self.tokens().line).add_modifier(Modifier::DIM)
    }
    /// `line-info` (#495F81 / indexed 60) for recalled content.
    #[must_use]
    pub fn info_line(self) -> Style {
        let style = Self::foreground(self.tokens().line_info);
        if self.colors == ColorMode::Ansi16 {
            style
        } else {
            style.add_modifier(Modifier::DIM)
        }
    }
    /// Resolve the semantic palette for this profile.
    #[must_use]
    pub fn tokens(self) -> Colors {
        let roles = match self.colors {
            ColorMode::NoColor => [None; 8],
            ColorMode::Ansi16 => [
                Some(Color::DarkGray),
                Some(Color::Cyan),
                Some(Color::Green),
                Some(Color::Yellow),
                Some(Color::Red),
                Some(Color::Blue),
                None,
                None,
            ],
            ColorMode::Ansi256 => {
                [245, 80, 114, 179, 210, 111, 235, 23].map(|n| Some(Color::Indexed(n)))
            }
            ColorMode::TrueColor => {
                let rgb = if self.theme == Theme::Dark {
                    [
                        0x0085_878c_u32,
                        0x005c_cfd6,
                        0x0086_c77f,
                        0x00eb_c56b,
                        0x00ec_7b82,
                        0x0082_aeef,
                        0x001c_1f24,
                        0x0016_3236,
                    ]
                } else {
                    [
                        0x006b_6e73_u32,
                        0x0012_7c83,
                        0x002e_7d32,
                        0x008a_6100,
                        0x00b3_261e,
                        0x0025_57b8,
                        0x00ee_eeea,
                        0x00d5_eef0,
                    ]
                };
                rgb.map(|n| {
                    let [_, r, g, b] = n.to_be_bytes();
                    Some(Color::Rgb(r, g, b))
                })
            }
        };
        Colors {
            foreground: None,
            background: None,
            dim: if self.colors == ColorMode::Ansi16 {
                None
            } else {
                roles[0]
            },
            faded: match self.colors {
                ColorMode::NoColor => None,
                ColorMode::Ansi16 => Some(Color::DarkGray),
                ColorMode::Ansi256 => Some(Color::Indexed(240)),
                ColorMode::TrueColor if self.theme == Theme::Dark => Some(Color::Rgb(94, 96, 100)),
                ColorMode::TrueColor => Some(Color::Rgb(154, 156, 159)),
            },
            accent: roles[1],
            line: match self.colors {
                ColorMode::TrueColor => Some(Color::Rgb(46, 116, 121)),
                ColorMode::Ansi256 => Some(Color::Indexed(30)),
                ColorMode::Ansi16 => Some(Color::Cyan),
                ColorMode::NoColor => None,
            },
            line_info: match self.colors {
                ColorMode::TrueColor => Some(Color::Rgb(73, 95, 129)),
                ColorMode::Ansi256 => Some(Color::Indexed(60)),
                ColorMode::Ansi16 => Some(Color::Blue),
                ColorMode::NoColor => None,
            },
            success: roles[2],
            warning: roles[3],
            error: roles[4],
            info: roles[5],
            surface: roles[6],
            selection: roles[7],
        }
    }
    fn foreground(color: Option<Color>) -> Style {
        color.map_or_else(Style::default, |color| Style::default().fg(color))
    }
    /// Body text preserves terminal foreground and background.
    #[must_use]
    pub fn base(self) -> Style {
        Style::default()
    }
    /// Readable secondary text; ANSI 16 uses default foreground with faint.
    #[must_use]
    pub fn muted(self) -> Style {
        if self.colors == ColorMode::Ansi16 {
            self.base().fg(Color::Reset).add_modifier(Modifier::DIM)
        } else {
            Self::foreground(self.tokens().dim)
        }
    }
    /// Obscured live content; attribute-only hosts fade without color overrides.
    #[must_use]
    pub fn faded(self) -> Style {
        if self.colors == ColorMode::NoColor {
            self.base()
        } else if self.colors == ColorMode::Ansi16 {
            self.muted()
        } else {
            Self::foreground(self.tokens().faded)
        }
    }
    /// Unavailable controls use faded styling; hosts retain a textual disabled
    /// marker so availability remains explicit without color.
    #[must_use]
    pub fn disabled(self) -> Style {
        if self.colors == ColorMode::Ansi16 {
            self.muted()
        } else {
            self.faded()
        }
    }
    /// Accent without emphasis.
    #[must_use]
    pub fn accent(self) -> Style {
        Self::foreground(self.tokens().accent)
    }
    /// Container heading, without the reverse emphasis reserved for focused fields.
    #[must_use]
    pub fn heading(self) -> Style {
        self.accent()
            .add_modifier(Modifier::BOLD)
            .remove_modifier(Modifier::DIM | Modifier::REVERSED)
    }
    /// Focused heading; reverse emphasis in no-color mode.
    #[must_use]
    pub fn focus(self) -> Style {
        let style = self.accent().add_modifier(Modifier::BOLD);
        if self.colors == ColorMode::NoColor {
            style.add_modifier(Modifier::REVERSED)
        } else {
            style
        }
    }
    /// Warning text, meaningful without color.
    #[must_use]
    pub fn warning(self) -> Style {
        Self::foreground(self.tokens().warning).add_modifier(Modifier::BOLD)
    }
    /// Failed or dangerous content, preserving defaults in no-color mode.
    #[must_use]
    pub fn error(self) -> Style {
        Self::foreground(self.tokens().error)
    }
    /// Informational controls and source metadata, including plain ANSI 16 blue.
    #[must_use]
    pub fn info(self) -> Style {
        Self::foreground(self.tokens().info)
    }
    /// Selection tint or reverse fallback.
    #[must_use]
    pub fn selection(self) -> Style {
        self.tokens().selection.map_or_else(
            || self.base().add_modifier(Modifier::REVERSED),
            |color| self.base().bg(color),
        )
    }

    /// Selected list row, with reverse fallback when tint is unavailable.
    #[must_use]
    pub fn surface(self) -> Style {
        self.tokens().surface.map_or_else(
            || self.base().add_modifier(Modifier::REVERSED),
            |color| self.base().bg(color),
        )
    }
}
