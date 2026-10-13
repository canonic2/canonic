//! Composes reusable views into a local gallery using simulated product activity.

mod commands;
mod completion;
mod composer;
mod conversation;
mod dialog_body;
mod dialogs;
mod editor;
mod inline;
mod inspection;
mod job_control;
mod option_picker;
mod permissions;
mod reviews;
mod scenarios;
mod sessions;
mod status;
mod tool_cards;
mod trees;

use crate::{
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
    widgets::{ListState, Paragraph},
};
use std::time::{Duration, Instant};

/// Local component gallery options.
#[derive(Parser)]
#[command(
    name = "tui-gallery",
    version,
    about = "Explore Canonic terminal components with local fixtures"
)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "independent command-line switches"
)]
pub struct Cli {
    /// List components without opening an interactive terminal.
    #[arg(long)]
    pub list: bool,
    /// Run the native inline composition host; ordinary gallery views remain full screen.
    #[arg(long)]
    pub inline: bool,
    /// Open a component's scenario picker; otherwise start at the component picker.
    #[arg(long, value_enum)]
    pub component: Option<Component>,
    /// Open a named scenario; requires --component. Use --list to see names.
    #[arg(long, requires = "component")]
    pub scenario: Option<String>,
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
    /// Command palette, keyboard help and scoped shortcut sequences.
    Commands,
    /// Draft submission with delayed host acceptance and rejection.
    Composer,
    /// Focus-preserving command and file suggestions.
    CompletionMenu,
    /// Identified tool activity, output, concurrency and acknowledged effects.
    ToolCallCard,
    /// Staged identified questions above a preserved Chat draft.
    QuestionForm,
    PermissionReview,
    /// Dedicated host-owned conversation browser.
    SessionBrowser,
    /// Lazy host-owned files, diff resources and task trees.
    TreeBrowser,
    /// Read-only host-supplied code and unified patch inspection.
    CodeDiffInspection,
    StatusPanelsAndNotices,
    InlineComposition,
}

#[derive(Clone, Copy)]
enum Screen {
    Catalog,
    Picker,
    Chat,
    Editor,
    Dialog,
    Commands,
    Composer,
    CompletionMenu,
    ToolCallCard,
    QuestionForm,
    PermissionReview,
    /// Dedicated host-owned conversation browser.
    SessionBrowser,
    TreeBrowser,
    CodeDiffInspection,
    StatusPanelsAndNotices,
    InlineComposition,
    Scenarios(Component),
    Design(Component, usize),
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
    catalog_viewport: ListState,
    picker: option_picker::PickerDemo,
    chat: conversation::ChatDemo,
    theme: Presentation,
    note: String,
    scenario: usize,
    editor: editor::EditorDemo,
    dialogs: dialogs::DialogDemo,
    commands: commands::CommandDemo,
    composer: composer::ComposerDemo,
    completion: completion::CompletionDemo,
    tools: tool_cards::ToolDemo,
    tool_scenario: usize,
    reviews: reviews::ReviewDemo,
    permissions: permissions::PermissionDemo,
    sessions: sessions::SessionDemo,
    trees: trees::TreeDemo,
    inspection: inspection::InspectionDemo,
    status: status::StatusDemo,
    inline: inline::InlineDemo,
    scenarios: [Picker; 15],
    scenario_viewports: [ListState; 15],
    scenario_return: Screen,
    preview_width: Option<u16>,
}

impl Default for Gallery {
    fn default() -> Self {
        Self::new()
    }
}

impl Gallery {
    fn active_component(&self) -> Option<Component> {
        match self.screen {
            Screen::Catalog => None,
            Screen::Picker => Some(Component::Picker),
            Screen::Chat => Some(Component::Chat),
            Screen::Editor => Some(Component::Editor),
            Screen::Dialog => Some(Component::Dialog),
            Screen::Commands => Some(Component::Commands),
            Screen::Composer => Some(Component::Composer),
            Screen::CompletionMenu => Some(Component::CompletionMenu),
            Screen::ToolCallCard => Some(Component::ToolCallCard),
            Screen::QuestionForm => Some(Component::QuestionForm),
            Screen::PermissionReview => Some(Component::PermissionReview),
            Screen::SessionBrowser => Some(Component::SessionBrowser),
            Screen::TreeBrowser => Some(Component::TreeBrowser),
            Screen::CodeDiffInspection => Some(Component::CodeDiffInspection),
            Screen::StatusPanelsAndNotices => Some(Component::StatusPanelsAndNotices),
            Screen::InlineComposition => Some(Component::InlineComposition),
            Screen::Scenarios(component) | Screen::Design(component, _) => Some(component),
        }
    }

    fn navigate_scenarios(&mut self, event: &Event) -> bool {
        if matches!(event, Event::Key(key) if key.code == KeyCode::F(10)) {
            if matches!(event, Event::Key(key) if key.kind == KeyEventKind::Press)
                && let Some(component) = self.active_component()
                && !matches!(self.screen, Screen::Scenarios(_))
            {
                self.open_scenarios(component);
            } else if matches!(event, Event::Key(key) if key.kind == KeyEventKind::Press)
                && matches!(self.screen, Screen::Scenarios(_))
            {
                self.screen = self.scenario_return;
                self.note.clear();
            }
            return true;
        }
        let Screen::Scenarios(component) = self.screen else {
            return false;
        };
        if let Event::Key(key) = event {
            if key.kind == KeyEventKind::Release {
                return true;
            }
            if key.code == KeyCode::Esc {
                self.screen = Screen::Catalog;
                self.note.clear();
                return true;
            }
            if key.code == KeyCode::F(4)
                || (key.modifiers.contains(KeyModifiers::CONTROL)
                    && matches!(key.code, KeyCode::Char('c' | 't')))
            {
                return false;
            }
        }
        if let picker::Action::Selected(index) =
            self.scenarios[scenarios::index(component)].update(event)
        {
            let entries = scenarios::entries(component);
            let id = entries.get(index).map_or_else(
                || scenarios::PROFILES[index - entries.len()],
                |entry| entry.id,
            );
            self.open_scenario(component, id)
                .unwrap_or_else(|_| unreachable!("catalog IDs are valid"));
        }
        true
    }

    fn open_scenarios(&mut self, component: Component) {
        self.scenario_return = self.screen;
        self.screen = Screen::Scenarios(component);
        self.note = scenarios::board(component).into();
    }

    /// Reset one component to a named design scenario or main-view profile.
    /// Pending designs show their contract without simulating unsupported behavior.
    ///
    /// # Errors
    /// Returns an error for an unknown scenario before changing gallery state.
    #[expect(
        clippy::too_many_lines,
        reason = "One scenario opening applies the shared profile once and preserves all other component states"
    )]
    pub fn open_scenario(&mut self, component: Component, id: &str) -> Result<()> {
        let id = match (component, id) {
            (Component::Picker, "default")
            | (Component::Editor, "focused")
            | (Component::Dialog, "prompt")
            | (Component::Commands, "palette") => "main",
            (Component::Dialog, "nest") => "nested",
            _ => id,
        };
        let profile = scenarios::PROFILES.iter().position(|name| *name == id);
        let index = if profile.is_some() {
            0
        } else {
            scenarios::entries(component)
                .iter()
                .position(|entry| entry.id == id)
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "Unknown {} scenario: {id}. Use --list.",
                        scenarios::name(component)
                    )
                })?
        };
        let entry = &scenarios::entries(component)[index];
        self.preview_width = None;
        self.note = if matches!(component, Component::Composer) {
            format!("composer / {id}")
        } else if entry.detail == "Interactive fixture" {
            String::new()
        } else {
            format!("{} / {} · {}", scenarios::name(component), id, entry.detail)
        };
        let Some(fixture) = entry.fixture else {
            self.screen = Screen::Design(component, index);
            return Ok(());
        };
        self.reset_component(component, fixture);
        if matches!(component, Component::ToolCallCard) {
            self.preview_width = if matches!(fixture, 28..=30) {
                Some(40)
            } else {
                None
            };
            if fixture == 31 {
                self.theme.colors = ColorMode::Ansi16;
            }
            if matches!(fixture, 32 | 33) {
                self.theme.colors = ColorMode::NoColor;
            }
        }
        if matches!(
            component,
            Component::SessionBrowser
                | Component::TreeBrowser
                | Component::CodeDiffInspection
                | Component::StatusPanelsAndNotices
                | Component::InlineComposition
        ) {
            match id {
                "ansi16" => self.theme.colors = ColorMode::Ansi16,
                "no-color" | "no-color-pending" => self.theme.colors = ColorMode::NoColor,
                "ascii" => self.theme.symbols = Symbols::Ascii,
                "light" => {
                    self.theme.colors = ColorMode::TrueColor;
                    self.theme.theme = Theme::Light;
                }
                _ => {}
            }
        }
        for code in entry.keys {
            self.fixture_event(
                component,
                &Event::Key(crate::event::KeyEvent::new(*code, KeyModifiers::NONE)),
            );
        }
        match profile {
            Some(0) => self.preview_width = Some(40),
            Some(1) => self.theme.colors = ColorMode::Ansi16,
            Some(2) => {
                self.theme.colors = ColorMode::NoColor;
                self.theme.symbols = Symbols::Unicode;
            }
            Some(3) => self.theme.symbols = Symbols::Ascii,
            Some(4) => {
                self.theme.colors = ColorMode::TrueColor;
                self.theme.theme = Theme::Light;
                self.theme.symbols = Symbols::Unicode;
            }
            Some(5) => {
                self.theme.colors = ColorMode::TrueColor;
                self.theme.theme = Theme::Dark;
                self.theme.symbols = Symbols::Unicode;
            }
            Some(6) => self.theme.colors = ColorMode::Ansi256,
            Some(7) => {
                self.theme.colors = ColorMode::NoColor;
                self.theme.symbols = Symbols::Ascii;
            }
            _ => {
                self.preview_width = match (component, id) {
                    (Component::CodeDiffInspection, "narrow" | "narrow-code")
                    | (
                        Component::StatusPanelsAndNotices | Component::InlineComposition,
                        "narrow",
                    ) => Some(40),
                    (Component::TreeBrowser, "narrow" | "narrow-filter")
                    | (Component::SessionBrowser, "narrow" | "narrow-search" | "narrow-delete")
                    | (Component::Chat, "narrow" | "search-narrow-ascii")
                    | (Component::QuestionForm, "narrow" | "narrow-review" | "narrow-field")
                    | (
                        Component::PermissionReview,
                        "narrow" | "narrow-details" | "narrow-scope" | "short",
                    )
                    | (Component::ToolCallCard, "narrow" | "narrow-diff" | "narrow-failed") => {
                        Some(40)
                    }
                    (Component::StatusPanelsAndNotices | Component::TreeBrowser, "medium")
                    | (Component::Chat, "pane") => Some(64),
                    (
                        Component::QuestionForm
                        | Component::PermissionReview
                        | Component::StatusPanelsAndNotices
                        | Component::InlineComposition,
                        "tiny",
                    ) => Some(12),
                    _ => None,
                };
                self.apply_chat_profile(component, id);
                self.apply_tool_profile(component, id);
            }
        }
        self.apply_fixture_profile(component);
        Ok(())
    }

    fn apply_tool_profile(&mut self, component: Component, id: &str) {
        if matches!(component, Component::ToolCallCard) {
            if id == "card-ascii" {
                self.theme.symbols = Symbols::Ascii;
            }
            if matches!(id, "card-light" | "child-light-selected") {
                self.theme.theme = Theme::Light;
                self.theme.colors = ColorMode::TrueColor;
            }
        }
    }

    fn apply_chat_profile(&mut self, component: Component, id: &str) {
        if matches!(component, Component::Chat) {
            match id {
                "search-ansi16" => self.theme.colors = ColorMode::Ansi16,
                "search-no-color" => self.theme.colors = ColorMode::NoColor,
                "search-narrow-ascii" => self.theme.symbols = Symbols::Ascii,
                _ => {}
            }
        }
    }

    fn apply_fixture_profile(&mut self, component: Component) {
        let presentation = match component {
            Component::Composer => self.composer.take_presentation(),
            Component::Commands => self.commands.take_presentation(self.theme),
            Component::CompletionMenu => self.completion.take_presentation(self.theme),
            Component::Dialog => self.dialogs.take_presentation(self.theme),
            _ => None,
        };
        if let Some(presentation) = presentation {
            self.theme = presentation;
        }
    }

    fn reset_component(&mut self, component: Component, fixture: usize) {
        self.screen = match component {
            Component::Picker => {
                self.picker = option_picker::PickerDemo::new();
                Screen::Picker
            }
            Component::Chat => {
                self.scenario = fixture;
                self.reset_chat();
                Screen::Chat
            }
            Component::Editor => {
                self.editor = editor::EditorDemo::new();
                Screen::Editor
            }
            Component::Dialog => {
                self.dialogs = dialogs::DialogDemo::new();
                Screen::Dialog
            }
            Component::Commands => {
                self.commands = commands::CommandDemo::new();
                Screen::Commands
            }
            Component::Composer => {
                self.composer = composer::ComposerDemo::scenario(fixture);
                Screen::Composer
            }
            Component::CompletionMenu => {
                self.completion = completion::CompletionDemo::scenario(fixture);
                Screen::CompletionMenu
            }
            Component::ToolCallCard => {
                self.tool_scenario = fixture;
                self.tools = tool_cards::ToolDemo::new(fixture);
                Screen::ToolCallCard
            }
            Component::QuestionForm => {
                self.reviews = reviews::ReviewDemo::new(fixture);
                Screen::QuestionForm
            }
            Component::SessionBrowser => {
                self.sessions = sessions::SessionDemo::new(fixture);
                Screen::SessionBrowser
            }
            Component::TreeBrowser => {
                self.trees = trees::TreeDemo::new(fixture);
                Screen::TreeBrowser
            }
            Component::CodeDiffInspection => {
                self.inspection = inspection::InspectionDemo::new(fixture);
                Screen::CodeDiffInspection
            }
            Component::StatusPanelsAndNotices => {
                self.status = status::StatusDemo::new(fixture);
                Screen::StatusPanelsAndNotices
            }
            Component::InlineComposition => {
                self.inline = inline::InlineDemo::new(fixture);
                Screen::InlineComposition
            }
            Component::PermissionReview => {
                self.permissions = permissions::PermissionDemo::new(fixture);
                Screen::PermissionReview
            }
        };
        if !matches!(
            component,
            Component::Chat
                | Component::Composer
                | Component::CompletionMenu
                | Component::ToolCallCard
                | Component::QuestionForm
                | Component::PermissionReview
                | Component::SessionBrowser
                | Component::TreeBrowser
                | Component::CodeDiffInspection
                | Component::StatusPanelsAndNotices
                | Component::InlineComposition
        ) {
            for _ in 0..fixture {
                self.fixture_event(
                    component,
                    &Event::Key(crate::event::KeyEvent::new(
                        KeyCode::F(2),
                        KeyModifiers::NONE,
                    )),
                );
            }
        }
    }

    fn fixture_event(&mut self, component: Component, event: &Event) {
        match component {
            Component::Picker => {
                self.picker.update(event);
            }
            Component::Editor => {
                self.editor.update(event);
            }
            Component::Dialog => {
                self.dialogs.update(event);
            }
            Component::Commands => {
                self.commands.update(event);
            }
            Component::Composer => {
                self.composer.update(event);
            }
            Component::Chat => self.update_chat(event),
            Component::CompletionMenu => {
                self.completion.update(event);
            }
            Component::ToolCallCard => {
                self.tools.update(event);
            }
            Component::QuestionForm => {
                self.reviews.update(event);
            }
            Component::PermissionReview => {
                self.permissions.update(event);
            }
            Component::SessionBrowser => {
                self.sessions.update(event);
            }
            Component::TreeBrowser => {
                self.trees.update(event);
            }
            Component::CodeDiffInspection => {
                self.inspection.update(event);
            }
            Component::StatusPanelsAndNotices => {
                self.status.update(event);
            }
            Component::InlineComposition => {
                self.inline.update(event);
            }
        }
    }

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
                    "Commands and bindings".into(),
                    "Composer".into(),
                    "Completion Menu".into(),
                    "Tool Call Card".into(),
                    "Question Form".into(),
                    "Permission Review".into(),
                    "Session Browser".into(),
                    "Tree Browser".into(),
                    "Code & diff inspection".into(),
                    "Status Panels & Notices".into(),
                    "Inline composition".into(),
                ],
                Some(0),
            ),
            catalog_viewport: ListState::default(),
            picker: option_picker::PickerDemo::new(),
            chat: conversation::ChatDemo::new(0),
            theme: Presentation::default(),
            note: String::new(),
            scenario: 0,
            editor: editor::EditorDemo::new(),
            dialogs: dialogs::DialogDemo::new(),
            commands: commands::CommandDemo::new(),
            composer: composer::ComposerDemo::new(),
            completion: completion::CompletionDemo::scenario(0),
            tools: tool_cards::ToolDemo::new(0),
            tool_scenario: 0,
            reviews: reviews::ReviewDemo::new(0),
            permissions: permissions::PermissionDemo::new(0),
            sessions: sessions::SessionDemo::new(0),
            trees: trees::TreeDemo::new(0),
            inspection: inspection::InspectionDemo::new(0),
            status: status::StatusDemo::new(0),
            inline: inline::InlineDemo::new(0),
            scenarios: std::array::from_fn(|index| scenarios::picker(scenarios::COMPONENTS[index])),
            scenario_viewports: std::array::from_fn(|_| ListState::default()),
            scenario_return: Screen::Catalog,
            preview_width: None,
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
            Screen::Commands => "commands",
            Screen::Composer => "composer",
            Screen::CompletionMenu => "completion-menu",
            Screen::ToolCallCard => "tool-call-card",
            Screen::QuestionForm => "question-form",
            Screen::PermissionReview => "permission-review",
            Screen::SessionBrowser => "session-browser",
            Screen::TreeBrowser => "tree-browser",
            Screen::CodeDiffInspection => "code-diff-inspection",
            Screen::StatusPanelsAndNotices => "status-panels-and-notices",
            Screen::InlineComposition => "inline-composition",
            Screen::Scenarios(_) => "scenarios",
            Screen::Design(_, _) => "design",
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

    /// Independent host draft in the command fixture.
    #[must_use]
    pub fn command_input(&self) -> &str {
        self.commands.input()
    }

    /// Independent draft in the delayed-acknowledgement fixture.
    #[must_use]
    pub fn composer_input(&self, index: usize) -> Option<&str> {
        self.composer.input(index)
    }
    /// Independent draft in the Completion Menu host.
    #[must_use]
    pub fn completion_input(&self) -> String {
        self.completion.input()
    }
    /// Composer draft in the Chat scenario, preserved while searching/selecting.
    #[must_use]
    pub fn chat_input(&self) -> &str {
        self.chat.input()
    }
    /// Composer draft preserved by Tool Call Card selection and details.
    #[must_use]
    pub fn tool_input(&self) -> &str {
        self.tools.input()
    }

    fn reset_chat(&mut self) {
        self.chat = conversation::ChatDemo::new(self.scenario);
    }

    /// Advance one deterministic simulated streaming step.
    pub fn tick(&mut self) {
        self.tick_elapsed(Duration::from_millis(80));
    }

    fn tick_elapsed(&mut self, elapsed: Duration) {
        if matches!(self.screen, Screen::Scenarios(_) | Screen::Design(_, _)) {
            return;
        }
        if matches!(self.screen, Screen::StatusPanelsAndNotices) {
            self.status.tick(elapsed);
        }
        if matches!(self.screen, Screen::InlineComposition) {
            self.inline.tick(elapsed);
        }
        self.editor.tick(elapsed);
        self.picker.tick(elapsed);
        self.dialogs.tick(elapsed);
        self.commands.tick(elapsed);
        self.composer.tick(elapsed);
        self.completion.tick(elapsed);
        self.chat.tick(elapsed);
        if matches!(self.screen, Screen::ToolCallCard) {
            self.tools.tick(elapsed);
        }
        if matches!(self.screen, Screen::QuestionForm) {
            self.reviews.tick(elapsed);
        }
        if matches!(self.screen, Screen::PermissionReview) {
            self.permissions.tick(elapsed);
        }
        if matches!(self.screen, Screen::SessionBrowser) {
            self.sessions.tick(elapsed);
        }
        if matches!(self.screen, Screen::TreeBrowser) {
            self.trees.tick(elapsed);
        }
    }

    /// Apply an input event. Returns true when the gallery should exit.
    pub fn update(&mut self, event: &Event) -> bool {
        if self.navigate_scenarios(event) {
            return false;
        }
        if matches!(self.screen, Screen::Chat) && self.chat.update(event) {
            return false;
        }
        if matches!(self.screen, Screen::ToolCallCard) && self.tools.update(event) {
            return false;
        }
        if self.update_browsing(event) {
            return false;
        }
        if matches!(self.screen, Screen::Composer) && self.composer.update(event) {
            self.apply_fixture_profile(Component::Composer);
            return self.composer.should_quit();
        }
        if matches!(self.screen, Screen::CompletionMenu) && self.completion.update(event) {
            self.apply_fixture_profile(Component::CompletionMenu);
            return false;
        }
        if matches!(self.screen, Screen::Commands) && self.commands.update(event) {
            self.apply_fixture_profile(Component::Commands);
            return false;
        }
        if matches!(self.screen, Screen::Editor) && self.editor.update(event) {
            return false;
        }
        if matches!(self.screen, Screen::Picker) && self.picker.update(event) {
            return false;
        }
        if matches!(self.screen, Screen::Dialog) && self.dialogs.update(event) {
            self.apply_fixture_profile(Component::Dialog);
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
                if matches!(self.screen, Screen::Catalog) {
                    return true;
                }
                if let Some(component) = self.active_component() {
                    self.open_scenarios(component);
                }
                return false;
            }
            if key.code == KeyCode::F(2) {
                if matches!(self.screen, Screen::ToolCallCard) {
                    self.tool_scenario = (self.tool_scenario + 1)
                        % scenarios::entries(Component::ToolCallCard).len();
                    self.tools = tool_cards::ToolDemo::new(self.tool_scenario);
                }
                if matches!(self.screen, Screen::Chat) {
                    self.scenario = (self.scenario + 1) % scenarios::entries(Component::Chat).len();
                    self.reset_chat();
                }
                return false;
            }
        }
        match self.screen {
            Screen::Catalog => {
                if let picker::Action::Selected(index) = self.catalog.update(event) {
                    self.open_scenarios(scenarios::COMPONENTS[index]);
                }
            }
            Screen::Chat => self.update_chat(event),
            Screen::Picker
            | Screen::Editor
            | Screen::Dialog
            | Screen::Commands
            | Screen::Composer
            | Screen::CompletionMenu
            | Screen::ToolCallCard
            | Screen::QuestionForm
            | Screen::PermissionReview
            | Screen::SessionBrowser
            | Screen::TreeBrowser
            | Screen::CodeDiffInspection
            | Screen::StatusPanelsAndNotices
            | Screen::InlineComposition
            | Screen::Scenarios(_)
            | Screen::Design(_, _) => {}
        }
        false
    }

    fn update_browsing(&mut self, event: &Event) -> bool {
        match self.screen {
            Screen::QuestionForm => self.reviews.update(event),
            Screen::PermissionReview => self.permissions.update(event),
            Screen::SessionBrowser => self.sessions.update(event),
            Screen::TreeBrowser => self.trees.update(event),
            Screen::CodeDiffInspection => self.inspection.update(event),
            Screen::StatusPanelsAndNotices => self.status.update(event),
            Screen::InlineComposition => self.inline.update(event),
            _ => false,
        }
    }

    fn update_chat(&mut self, event: &Event) {
        self.chat.update(event);
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
        let [header, mut body, note, footer] = Layout::vertical([
            Constraint::Length(2),
            Constraint::Min(0),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .areas(area);
        if !matches!(
            self.screen,
            Screen::Catalog | Screen::Scenarios(_) | Screen::Design(_, _)
        ) {
            body.width = self
                .preview_width
                .map_or(body.width, |width| width.min(body.width));
        }
        frame.render_widget(
            Paragraph::new("Canonic TUI · Component gallery").style(self.theme.heading()),
            header,
        );
        match self.screen {
            Screen::Catalog => {
                self.catalog
                    .render_with_state(frame, body, self.theme, &mut self.catalog_viewport);
            }
            Screen::Picker => self.picker.render(frame, body, self.theme),
            Screen::Chat => self.chat.render(frame, body, self.theme),
            Screen::Editor => self.editor.render(frame, body, self.theme),
            Screen::Dialog => self.dialogs.render(frame, body, self.theme),
            Screen::Commands => self.commands.render(frame, body, self.theme),
            Screen::Composer => self.composer.render(frame, body, self.theme),
            Screen::CompletionMenu => self.completion.render(frame, body, self.theme),
            Screen::ToolCallCard => self.tools.render(frame, body, self.theme),
            Screen::QuestionForm => self.reviews.render(frame, body, self.theme),
            Screen::PermissionReview => self.permissions.render(frame, body, self.theme),
            Screen::SessionBrowser => self.sessions.render(frame, body, self.theme),
            Screen::TreeBrowser => self.trees.render(frame, body, self.theme),
            Screen::CodeDiffInspection => self.inspection.render(frame, body, self.theme),
            Screen::StatusPanelsAndNotices => self.status.render(frame, body, self.theme),
            Screen::InlineComposition => self.inline.render(frame, body, self.theme),
            Screen::Scenarios(component) => {
                let index = scenarios::index(component);
                self.scenarios[index].render_with_state(
                    frame,
                    body,
                    self.theme,
                    &mut self.scenario_viewports[index],
                );
            }
            Screen::Design(component, index) => {
                let scenario = &scenarios::entries(component)[index];
                frame.render_widget(
                    Paragraph::new(format!(
                        "{} / {} · design pending\n\n{}\n\n{}\n\nEsc scenarios · Ctrl-C quit",
                        scenarios::name(component),
                        scenario.id,
                        scenario.detail,
                        scenarios::board(component)
                    ))
                    .wrap(ratatui::widgets::Wrap { trim: false })
                    .style(self.theme.base()),
                    body,
                );
            }
        }
        frame.render_widget(
            Paragraph::new(self.note.as_str()).style(self.theme.heading()),
            note,
        );
        frame.render_widget(
            Paragraph::new(
                if matches!(self.screen, Screen::Composer) {
                    "F10 scenarios · Esc local · Ctrl-C twice quits · F2 next · F4 colors · Ctrl-T theme"
                } else {
                    "Esc back · Ctrl-C quit · F10 scenarios · F2 next · F4 colors · Ctrl-T theme"
                },
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
            "picker  Option picker\nchat    Full-screen chat\neditor  Multiline editor\ndialog  Dialogs\ncommands Commands and bindings\ncomposer Composer\ncompletion-menu Completion Menu\ntool-call-card Tool Call Card"
        );
        for component in scenarios::COMPONENTS {
            println!(
                "\n{} scenarios ({})",
                scenarios::name(component),
                scenarios::board(component)
            );
            for entry in scenarios::entries(component) {
                println!(
                    "  {:<18} {} · {}",
                    entry.id,
                    if entry.fixture.is_some() {
                        "interactive"
                    } else {
                        "design pending"
                    },
                    entry.detail
                );
            }
            println!("  {} · main view profiles", scenarios::PROFILES.join(" / "));
        }
        return Ok(());
    }
    if cli.inline {
        if cli
            .component
            .is_some_and(|c| !matches!(c, Component::InlineComposition))
        {
            anyhow::bail!("--inline requires --component inline-composition");
        }
        return inline::run(
            cli.scenario.as_deref(),
            Presentation {
                theme: if cli.light { Theme::Light } else { Theme::Dark },
                colors: cli.colors.resolve(),
                symbols: if cli.ascii {
                    Symbols::Ascii
                } else {
                    Symbols::Unicode
                },
            },
        );
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
        if let Some(scenario) = &cli.scenario {
            gallery.open_scenario(component, scenario)?;
        } else {
            gallery.open_scenarios(component);
        }
    }
    interactive(&mut gallery)
}

fn interactive(gallery: &mut Gallery) -> Result<()> {
    // Declared first so the terminal restores its colors before policy is restored.
    let _color_output = ColorOutput::acquire();
    let signals = job_control::Suspension::install()?;
    let mut terminal = TerminalSession::open()?;
    let mut last_tick = Instant::now();
    let input_clock = Instant::now();
    let mut burst = crate::composer::PasteBurst::default();
    loop {
        if signals.requested() {
            terminal = job_control::full(terminal)?;
            last_tick = Instant::now();
        }
        terminal.draw(|frame| gallery.render(frame))?;
        let event = terminal.poll(Duration::from_millis(20))?;
        if signals.requested() {
            terminal = job_control::full(terminal)?;
            last_tick = Instant::now();
            terminal.draw(|frame| gallery.render(frame))?;
        }
        if event.as_ref().is_some_and(job_control::key) {
            if let Err(error) = job_control::available() {
                gallery.note = format!("suspend unavailable: {error}");
            } else {
                terminal = job_control::full(terminal)?;
                last_tick = Instant::now();
            }
            continue;
        }
        let mut events = burst.flush_idle(input_clock.elapsed());
        if let Some(event) = event {
            if matches!(gallery.screen, Screen::Composer) && !gallery.composer.overlay_open() {
                events.extend(burst.push(event, input_clock.elapsed()));
            } else {
                events.extend(burst.flush());
                events.push(event);
            }
        }
        if events.iter().any(|event| gallery.update(event)) {
            break;
        }
        if last_tick.elapsed() >= Duration::from_millis(80) {
            gallery.tick_elapsed(last_tick.elapsed());
            last_tick = Instant::now();
        }
    }
    Ok(())
}
