//! Composes reusable views into a local gallery using simulated product activity.

use crate::{
    chat::{self, Chat},
    event::{Event, KeyCode, KeyEventKind, KeyModifiers},
    picker::{self, Picker},
    runtime::TerminalSession,
    theme::Theme,
};
use anyhow::Result;
use clap::{Parser, ValueEnum};
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
}

/// Components available in the first-screen selector.
#[derive(Clone, Copy, ValueEnum)]
pub enum Component {
    /// Searchable option picker.
    Picker,
    /// Full-screen conversation and composer.
    Chat,
}

enum Screen {
    Catalog,
    Picker,
    Chat,
}

/// Gallery state independent of a terminal, clock, agent or product executable.
pub struct Gallery {
    screen: Screen,
    catalog: Picker,
    picker: Picker,
    chat: Chat,
    theme: Theme,
    note: String,
    scenario: usize,
    stream: std::collections::VecDeque<String>,
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
                vec!["Option picker".into(), "Full-screen chat".into()],
                Some(0),
            ),
            picker: Picker::new(
                "Option picker",
                vec![
                    "Use this server".into(),
                    "Use all project servers".into(),
                    "Continue without this server".into(),
                ],
                Some(2),
            ),
            chat: Chat::new(),
            theme: Theme::Dark,
            note: String::new(),
            scenario: 0,
            stream: std::collections::VecDeque::new(),
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
        }
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
        if let Some(fragment) = self.stream.pop_front() {
            self.chat.append(&fragment);
            if self.stream.is_empty() {
                self.chat.set_status("Ready fixture");
            }
        }
    }

    /// Apply an input event. Returns true when the gallery should exit.
    pub fn update(&mut self, event: &Event) -> bool {
        if let Event::Key(key) = event {
            if key.kind == KeyEventKind::Release {
                return false;
            }
            if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
                return true;
            }
            if key.code == KeyCode::Char('t') && key.modifiers.contains(KeyModifiers::CONTROL) {
                self.theme = self.theme.toggled();
                return false;
            }
            if key.code == KeyCode::Esc {
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
                } else if matches!(self.screen, Screen::Picker) {
                    self.picker = Picker::new("Empty picker fixture", Vec::new(), None);
                }
                return false;
            }
        }
        match self.screen {
            Screen::Catalog => {
                if let picker::Action::Selected(index) = self.catalog.update(event) {
                    self.screen = if index == 0 {
                        Screen::Picker
                    } else {
                        Screen::Chat
                    };
                    self.picker = Picker::new(
                        "Option picker",
                        vec![
                            "Use this server".into(),
                            "Use all project servers".into(),
                            "Continue without this server".into(),
                        ],
                        Some(2),
                    );
                    self.reset_chat();
                }
            }
            Screen::Picker => {
                if let picker::Action::Selected(index) = self.picker.update(event) {
                    self.note = format!(
                        "Selected original choice {} · fixture only; no settings changed",
                        index + 1
                    );
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
        }
        false
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
            Screen::Catalog => self.catalog.render(frame, body, self.theme),
            Screen::Picker => self.picker.render(frame, body, self.theme),
            Screen::Chat => self.chat.render(frame, body, self.theme),
        }
        frame.render_widget(
            Paragraph::new(self.note.as_str()).style(self.theme.focus()),
            note,
        );
        frame.render_widget(
            Paragraph::new(
                "Esc back/quit · Ctrl-C quit · Ctrl-T theme · F2 next fixture · resize freely",
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
        println!("picker  Option picker\nchat    Full-screen chat");
        return Ok(());
    }
    let mut gallery = Gallery::new();
    if let Some(component) = cli.component {
        gallery.screen = match component {
            Component::Picker => Screen::Picker,
            Component::Chat => Screen::Chat,
        };
    }
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
            gallery.tick();
            last_tick = Instant::now();
        }
    }
    Ok(())
}
