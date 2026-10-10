//! Composes reusable views into a local gallery using simulated product activity.

mod dialog_body;
mod dialogs;
mod editor;
mod option_picker;

use crate::{
    chat::{self, Chat},
    event::{Event, KeyCode, KeyEventKind, KeyModifiers},
    picker::{self, Picker},
    runtime::TerminalSession,
    theme::{ColorMode, Presentation, Symbols, Theme},
};
use anyhow::Result;
use clap::{Parser, ValueEnum};
use crossterm::style::{Colored, force_color_output};
use ratatui::{
    Frame,
    layout::{Constraint, Layout},
    widgets::Paragraph,
};
use std::time::{Duration, Instant};

/// Local component gallery options.
#[derive(Parser)]
#[command(
    name = "tui-gallery",
    version,
    about = "Explore Canonic terminal components with local fixtures"
)]
pub struct Cli {
    /// List components without opening an interactive terminal.
    #[arg(long)]
    pub list: bool,
    /// Open a component directly; otherwise start at the component selector.
    #[arg(long, value_enum)]
    pub component: Option<Component>,
    /// Color capability; auto respects `NO_COLOR`, `COLORTERM` and `TERM`.
    #[arg(long, value_enum, default_value = "auto")]
    pub colors: ColorChoice,
    /// Use ASCII decorations; preserves Unicode draft text.
    #[arg(long)]
    pub ascii: bool,
    /// Use light RGB tokens; body colors remain terminal defaults.
    #[arg(long)]
    pub light: bool,
}

/// Explicit gallery color profiles.
#[derive(Clone, Copy, ValueEnum)]
pub enum ColorChoice {
    /// Resolve host environment once before opening the terminal.
    Auto,
    /// Terminal-native sixteen-color palette.
    Ansi16,
    /// Indexed palette.
    Ansi256,
    /// RGB palette.
    Truecolor,
    /// Attributes only.
    None,
}

impl ColorChoice {
    fn resolve(self) -> ColorMode {
        match self {
            Self::Ansi16 => ColorMode::Ansi16,
            Self::Ansi256 => ColorMode::Ansi256,
            Self::Truecolor => ColorMode::TrueColor,
            Self::None => ColorMode::NoColor,
            Self::Auto => {
                if std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty()) {
                    return ColorMode::NoColor;
                }
                let color = std::env::var("COLORTERM").unwrap_or_default();
                if color == "truecolor" || color == "24bit" {
                    ColorMode::TrueColor
                } else if std::env::var("TERM").is_ok_and(|v| v.contains("256color")) {
                    ColorMode::Ansi256
                } else {
                    ColorMode::Ansi16
                }
            }
        }
    }
}

/// Components available in the first-screen selector.
#[derive(Clone, Copy, ValueEnum)]
pub enum Component {
    /// Searchable option picker.
    Picker,
    /// Full-screen conversation and composer.
    Chat,
    /// Multiline editors and modal focus isolation.
    Editor,
    /// Nested caller-owned dialogs and host drafts.
    Dialog,
}

enum Screen {
    Catalog,
    Picker,
    Chat,
    Editor,
    Dialog,
}

// The host's resolved presentation owns color policy while the gallery runs.
// Crossterm otherwise independently suppresses explicit profiles under NO_COLOR.
struct ColorOutput(bool);

impl ColorOutput {
    fn acquire() -> Self {
        let previous = Colored::ansi_color_disabled_memoized();
        force_color_output(true);
        Self(previous)
    }
}

impl Drop for ColorOutput {
    fn drop(&mut self) {
        force_color_output(!self.0);
    }
}

/// Gallery state independent of a terminal, clock, agent or product executable.
pub struct Gallery {
    screen: Screen,
    catalog: Picker,
    picker: option_picker::PickerDemo,
    chat: Chat,
    theme: Presentation,
    note: String,
    scenario: usize,
    stream: std::collections::VecDeque<String>,
    editor: editor::EditorDemo,
    dialogs: dialogs::DialogDemo,
}

impl Default for Gallery {
    fn default() -> Self {
        Self::new()
    }
}

impl Gallery {
    /// Start at the component selector with safe local fixtures.
    #[must_use]
    pub fn new() -> Self {
        let mut gallery = Self {
            screen: Screen::Catalog,
            catalog: Picker::new(
                "Choose a component",
                vec![
                    "Option picker".into(),
                    "Full-screen chat".into(),
                    "Multiline editor".into(),
                    "Dialogs".into(),
                ],
                Some(0),
            ),
            picker: option_picker::PickerDemo::new(),
            chat: Chat::new(),
            theme: Presentation::default(),
            note: String::new(),
            scenario: 0,
            stream: std::collections::VecDeque::new(),
            editor: editor::EditorDemo::new(),
            dialogs: dialogs::DialogDemo::new(),
        };
        gallery.reset_chat();
        gallery
    }

    /// Current view name, useful for host diagnostics and fixture tests.
    #[must_use]
    pub fn current(&self) -> &'static str {
        match self.screen {
            Screen::Catalog => "catalog",
            Screen::Picker => "picker",
            Screen::Chat => "chat",
            Screen::Editor => "editor",
            Screen::Dialog => "dialog",
        }
    }

    /// Inspect a draft in the independent editor fixture; invalid indices return none.
    #[must_use]
    pub fn editor_input(&self, index: usize) -> Option<&str> {
        self.editor.input(index)
    }
    /// Independent host draft in the rich picker fixture.
    #[must_use]
    pub fn picker_input(&self) -> &str {
        self.picker.input()
    }
    /// Host drafts underneath the dialog fixture.
    #[must_use]
    pub fn dialog_input(&self, index: usize) -> Option<&str> {
        self.dialogs.input(index)
    }
    /// Open layers in the dialog fixture.
    #[must_use]
    pub fn dialog_depth(&self) -> usize {
        self.dialogs.depth()
    }

    fn reset_chat(&mut self) {
        self.chat = Chat::new();
        self.stream.clear();
        match self.scenario {
            1 => self.chat.set_status("Empty conversation"),
            2 => {
                self.chat.push("Acme agent", "");
                self.stream = "This is simulated streaming output. Type while it arrives, or press Ctrl-X to interrupt.".split_inclusive(' ').map(str::to_owned).collect();
                self.chat.set_status("Streaming fixture");
            }
            3 => {
                self.chat.push(
                    "Activity",
                    "Simulated tool failure: example.com could not be reached.",
                );
                self.chat.set_status("Error fixture");
            }
            4 => {
                self.chat
                    .push("Activity", "The simulated agent connection is unavailable.");
                self.chat.set_status("Disconnected fixture");
            }
            5 => {
                self.chat.push(
                    "Acme agent",
                    &"Line: Resize, scroll, and inspect wrapping.\n".repeat(150),
                );
                self.chat.set_status("Long transcript fixture");
            }
            _ => {
                self.chat.push("Acme agent", "Welcome to the full-screen chat component. This is a local simulation; no AI account is used.");
                self.chat
                    .push("Tool activity", "example_lookup completed · example.com");
                self.chat.set_status("Ready fixture");
            }
        }
    }

    /// Advance one deterministic simulated streaming step.
    pub fn tick(&mut self) {
        self.tick_elapsed(Duration::from_millis(80));
    }

    fn tick_elapsed(&mut self, elapsed: Duration) {
        self.editor.tick(elapsed);
        self.picker.tick(elapsed);
        self.dialogs.tick(elapsed);
        self.chat.tick(elapsed);
        if let Some(fragment) = self.stream.pop_front() {
            self.chat.append(&fragment);
            if self.stream.is_empty() {
                self.chat.set_status("Ready fixture");
            }
        }
    }

    /// Apply an input event. Returns true when the gallery should exit.
    pub fn update(&mut self, event: &Event) -> bool {
        if matches!(self.screen, Screen::Editor) && self.editor.update(event) {
            return false;
        }
        if matches!(self.screen, Screen::Picker) && self.picker.update(event) {
            return false;
        }
        if matches!(self.screen, Screen::Dialog) && self.dialogs.update(event) {
            return false;
        }
        if let Event::Key(key) = event {
            if key.kind == KeyEventKind::Release {
                return false;
            }
            if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
                return true;
            }
            if key.code == KeyCode::Char('t') && key.modifiers.contains(KeyModifiers::CONTROL) {
                self.theme.theme = self.theme.theme.toggled();
                return false;
            }
            if key.code == KeyCode::F(4) {
                self.next_colors();
                return false;
            }
            if key.code == KeyCode::Esc {
                if matches!(self.screen, Screen::Chat) && self.chat.dismiss_selection() {
                    return false;
                }
                if matches!(self.screen, Screen::Catalog) {
                    return true;
                }
                self.screen = Screen::Catalog;
                self.stream.clear();
                self.note.clear();
                return false;
            }
            if key.code == KeyCode::F(2) {
                if matches!(self.screen, Screen::Chat) {
                    self.scenario = (self.scenario + 1) % 6;
                    self.reset_chat();
                }
                return false;
            }
        }
        match self.screen {
            Screen::Catalog => {
                if let picker::Action::Selected(index) = self.catalog.update(event) {
                    self.screen = match index {
                        0 => Screen::Picker,
                        1 => Screen::Chat,
                        2 => Screen::Editor,
                        _ => Screen::Dialog,
                    };
                    self.editor = editor::EditorDemo::new();
                    self.picker = option_picker::PickerDemo::new();
                    self.dialogs = dialogs::DialogDemo::new();
                    if matches!(self.screen, Screen::Chat) {
                        self.reset_chat();
                    } else {
                        self.stream.clear();
                    }
                }
            }
            Screen::Chat => match self.chat.update(event) {
                chat::Action::Submit(text) => {
                    if self.scenario == 4 {
                        self.chat.set_input(&text);
                        self.chat
                            .set_status("Disconnected fixture · message not executed");
                    } else if !self.stream.is_empty() {
                        self.chat.set_input(&text);
                        self.chat.set_status("Busy fixture · message not executed");
                    } else {
                        self.chat.push("You", &text);
                        self.chat.push("Acme agent", "");
                        self.stream = "Simulated response: your composer and conversation view work independently of an agent backend.".split_inclusive(' ').map(str::to_owned).collect();
                        self.chat.set_status("Streaming fixture");
                    }
                }
                chat::Action::Cancel => {
                    self.stream.clear();
                    self.chat.set_status("Interrupted fixture");
                }
                chat::Action::None => {}
            },
            Screen::Picker | Screen::Editor | Screen::Dialog => {}
        }
        false
    }

    fn next_colors(&mut self) {
        self.theme.colors = match self.theme.colors {
            ColorMode::Ansi16 => ColorMode::Ansi256,
            ColorMode::Ansi256 => ColorMode::TrueColor,
            ColorMode::TrueColor => ColorMode::NoColor,
            ColorMode::NoColor => ColorMode::Ansi16,
        };
    }

    /// Render the active component using the same public views as consumers.
    pub fn render(&mut self, frame: &mut Frame<'_>) {
        let area = frame.area();
        frame.render_widget(
            ratatui::widgets::Block::default().style(self.theme.base()),
            area,
        );
        let [header, body, note, footer] = Layout::vertical([
            Constraint::Length(2),
            Constraint::Min(0),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .areas(area);
        frame.render_widget(
            Paragraph::new("Canonic TUI · Component gallery").style(self.theme.focus()),
            header,
        );
        match self.screen {
            Screen::Catalog => self.catalog.render_with(frame, body, self.theme),
            Screen::Picker => self.picker.render(frame, body, self.theme),
            Screen::Chat => self.chat.render_with(frame, body, self.theme),
            Screen::Editor => self.editor.render(frame, body, self.theme),
            Screen::Dialog => self.dialogs.render(frame, body, self.theme),
        }
        frame.render_widget(
            Paragraph::new(self.note.as_str()).style(self.theme.focus()),
            note,
        );
        frame.render_widget(
            Paragraph::new(
                "Esc back/quit · Ctrl-C quit · Ctrl-T dark/light · F2 fixture · F4 colors",
            )
            .style(self.theme.muted()),
            footer,
        );
    }
}

/// Run the component gallery or plain component listing.
///
/// # Errors
/// Returns errors for noninteractive terminals or terminal I/O failures.
pub fn run(cli: &Cli) -> Result<()> {
    if cli.list {
        println!(
            "picker  Option picker\nchat    Full-screen chat\neditor  Multiline editor\ndialog  Dialogs"
        );
        return Ok(());
    }
    let mut gallery = Gallery::new();
    gallery.theme = Presentation {
        theme: if cli.light { Theme::Light } else { Theme::Dark },
        colors: cli.colors.resolve(),
        symbols: if cli.ascii {
            Symbols::Ascii
        } else {
            Symbols::Unicode
        },
    };
    if let Some(component) = cli.component {
        gallery.screen = match component {
            Component::Picker => Screen::Picker,
            Component::Chat => Screen::Chat,
            Component::Editor => Screen::Editor,
            Component::Dialog => Screen::Dialog,
        };
    }
    // Declared first so the terminal restores its colors before policy is restored.
    let _color_output = ColorOutput::acquire();
    let mut terminal = TerminalSession::open()?;
    let mut last_tick = Instant::now();
    loop {
        terminal.draw(|frame| gallery.render(frame))?;
        if let Some(event) = terminal.poll(Duration::from_millis(50))?
            && gallery.update(&event)
        {
            break;
        }
        if last_tick.elapsed() >= Duration::from_millis(80) {
            gallery.tick_elapsed(last_tick.elapsed());
            last_tick = Instant::now();
        }
    }
    Ok(())
}
