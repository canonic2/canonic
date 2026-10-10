//! Simulates host acknowledgements for one session draft without persistence or I/O.

mod scenarios;

use crate::{
    chat::Chat,
    composer::{
        CompletionItem, CompletionKind, EditorRequest, EditorResult, Identity, Intent,
        SavedComposer, Submission,
    },
    event::{Event, KeyCode, KeyEventKind, KeyModifiers},
    theme::Presentation,
};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    widgets::Paragraph,
};
use std::time::Duration;

pub(super) struct ComposerDemo {
    chat: Chat,
    ticket: Option<Identity>,
    previous: Option<Identity>,
    count: usize,
    note: String,
    blocked: bool,
    scenario: usize,
    saved: Option<SavedComposer>,
    editor: Option<EditorRequest>,
    quit: bool,
}

impl ComposerDemo {
    pub(super) fn scenario(index: usize) -> Self {
        if index >= 29 {
            return Self::extended(index);
        }
        let mut demo = Self::new();
        demo.scenario = index;
        match index {
            1..=4 => {
                demo.chat.set_input("Run the digest tests too.");
                demo.edit(&Event::Key(crate::event::KeyEvent::new(
                    KeyCode::Enter,
                    KeyModifiers::NONE,
                )));
                if index == 3 {
                    demo.chat.set_input("And bump the app version.");
                }
                if matches!(index, 2..=4) {
                    demo.resolve(index == 4);
                }
            }
            5 => {
                demo.blocked = true;
                demo.chat
                    .composer_mut()
                    .set_blocked(Some("Disconnected · submission unavailable"));
            }
            6 | 8 | 10 | 17 => {
                demo.chat
                    .set_input("and bump the app version\nthen tag a release candidate");
                demo.chat
                    .composer_mut()
                    .update(&Event::Key(crate::event::KeyEvent::new(
                        KeyCode::Left,
                        KeyModifiers::NONE,
                    )));
                if index == 8 {
                    demo.chat.composer_mut().browse_history(3);
                }
                if index == 10 {
                    let state = demo.chat.composer_mut().saved();
                    demo.chat.composer_mut().restore_saved(&state);
                }
            }
            7 | 11 | 12 | 18 | 19 => {
                demo.chat.composer_mut().set_working(true);
                for i in 1..=if index == 18 { 5 } else { 2 } {
                    demo.chat.set_input(&format!("Queued digest task {i}"));
                    demo.edit(&Event::Key(crate::event::KeyEvent::new(
                        KeyCode::Enter,
                        KeyModifiers::NONE,
                    )));
                }
                demo.chat.set_input("The session draft stays here");
                if matches!(index, 7 | 12) {
                    demo.chat.composer_mut().edit_queued(2);
                }
                if index == 19 {
                    demo.chat.composer_mut().turn_ended(false);
                }
            }
            9 | 15 | 16 => {
                demo.chat.set_input("Original editor draft");
                let request = demo.chat.composer_mut().begin_editor("nvim");
                if index == 16 {
                    demo.chat.composer_mut().set_text("Newer box draft");
                    if let Some(request) = request {
                        demo.chat.composer_mut().finish_editor(
                            &request.ticket,
                            EditorResult::Text("Returned editor text".into()),
                        );
                    }
                }
                demo.host_intents();
            }
            13 => {
                demo.chat.composer_mut().open_history_search();
            }
            14 | 20 | 21 => {
                for text in ["First protected draft", "Second protected draft"] {
                    demo.chat.set_input(text);
                    demo.chat.composer_mut().stash_draft();
                }
                demo.chat.set_input("Current draft");
                if index == 20 {
                    demo.chat.composer_mut().open_stash_list();
                }
                if index == 21 {
                    demo.chat.composer_mut().restore_stash(1);
                }
            }
            22..=28 => demo.content_scenario(index),
            _ => {}
        }
        demo
    }

    fn content_scenario(&mut self, index: usize) {
        let demo = self;
        match index {
            22 => {
                demo.edit(&Event::Paste("one\ntwo\nthree\nfour\nfive".into()));
            }
            23 => {
                demo.chat.composer_mut().insert_image("acme-image-1");
            }
            24 => {
                demo.edit(&Event::Key(crate::event::KeyEvent::new(
                    KeyCode::Char('!'),
                    KeyModifiers::NONE,
                )));
                demo.chat.set_input("cargo test");
            }
            25 => {
                demo.edit(&Event::Key(crate::event::KeyEvent::new(
                    KeyCode::Char('@'),
                    KeyModifiers::NONE,
                )));
            }
            26 => {
                demo.edit(&Event::Key(crate::event::KeyEvent::new(
                    KeyCode::Char('/'),
                    KeyModifiers::NONE,
                )));
            }
            27 => {
                demo.chat.set_input("Hidden\u{200b} text");
            }
            28 => {
                demo.chat.set_input("A draft to clear");
                demo.edit(&Event::Key(crate::event::KeyEvent::new(
                    KeyCode::Esc,
                    KeyModifiers::NONE,
                )));
            }
            _ => {}
        }
    }
    pub(super) fn new() -> Self {
        let mut chat = Chat::new();
        chat.composer_mut().set_repository_label("acme-api");
        chat.composer_mut().set_completions(vec![
            CompletionItem {
                id: "stash".into(),
                value: "stash".into(),
                detail: "Restore a protected draft".into(),
                kind: CompletionKind::Command,
                disabled: None,
                touched: false,
            },
            CompletionItem {
                id: "help".into(),
                value: "help".into(),
                detail: "Host help command".into(),
                kind: CompletionKind::Command,
                disabled: None,
                touched: false,
            },
            CompletionItem {
                id: "readme".into(),
                value: "README.md".into(),
                detail: "Repository file".into(),
                kind: CompletionKind::File,
                disabled: None,
                touched: false,
            },
            CompletionItem {
                id: "digest".into(),
                value: "src/digest.rs".into(),
                detail: "Touched file".into(),
                kind: CompletionKind::File,
                disabled: None,
                touched: false,
            },
        ]);
        chat.push_turn("acme", "  Both digest calls now send the user’s timezone, same\n  as profile sync. 14 tests pass.", "20:09");
        chat.set_status("acme-api#276 ~chat");
        chat.composer_mut().set_history([
            "run the digest tests against staging data",
            "check the digest timezone conversion",
            "why does the digest skip DST weeks?",
            "check the weekly digest\nfix the 2026-03-29 switch\nadd a regression test\nrun all digest tests",
            "move digest settings behind the flag",
            "check the profile sync timezone",
            "add digest retry logging",
            "review the notification payload",
            "verify the staging configuration",
            "document the digest schedule",
            "add the weekly digest tests",
            "set up the digest worker",
        ].into_iter().map(str::to_owned).collect());
        Self {
            chat,
            ticket: None,
            previous: None,
            count: 0,
            note: "Enter sends; F5 accepts and F6 rejects the fixed message".into(),
            blocked: false,
            scenario: 0,
            saved: None,
            editor: None,
            quit: false,
        }
    }
    pub(super) fn input(&self, index: usize) -> Option<&str> {
        (index == 0).then(|| self.chat.input())
    }
    fn resolve(&mut self, accepted: bool) {
        let Some(ticket) = self.ticket.take() else {
            if let Some(submission) = self.chat.composer_mut().turn_ended(accepted) {
                self.sent(&submission);
            }
            return;
        };
        self.chat.acknowledge_submission(
            &ticket,
            if accepted {
                Ok(())
            } else {
                Err("Simulated host failure")
            },
        );
        self.previous = Some(ticket);
        self.chat.composer_mut().set_working(accepted);
        if accepted && let Some(ticket) = &self.previous {
            self.chat
                .set_shell_output(ticket, "14 passed · 2.1s\nOutput joins the conversation");
        }
        self.note = if accepted {
            "Host accepted the fixed message; fresh draft preserved"
        } else {
            "Host rejected; failed message retained and fresh draft preserved"
        }
        .into();
    }
    pub(super) fn update(&mut self, event: &Event) -> bool {
        if let Event::Key(key) = event {
            if key.kind == KeyEventKind::Release {
                return true;
            }
            if matches!(key.code, KeyCode::F(2..=3 | 5..=9 | 11..=12))
                && key.kind != KeyEventKind::Press
            {
                return true;
            }
            match key.code {
                KeyCode::F(2) => *self = Self::scenario((self.scenario + 1) % 57),
                KeyCode::F(3) => {
                    if let Some(ticket) = &self.previous {
                        self.chat.toggle_shell_output(ticket);
                    }
                }
                KeyCode::F(5) => self.resolve(true),
                KeyCode::F(6) => self.resolve(false),
                KeyCode::F(7) => {
                    if let Some(ticket) = self.ticket.take() {
                        self.chat
                            .acknowledge_submission(&ticket, Err("Session reset"));
                        self.previous = Some(ticket);
                    }
                    self.chat.composer_mut().reset("Replacement draft");
                    self.note = "New draft lifetime; old response is invalid".into();
                }
                KeyCode::F(8) => {
                    let applied = self
                        .previous
                        .as_ref()
                        .is_some_and(|ticket| self.chat.acknowledge_submission(ticket, Ok(())));
                    self.note = if applied {
                        "Unexpected stale response applied"
                    } else {
                        "Stale response ignored; fresh draft and pending state preserved"
                    }
                    .into();
                }
                KeyCode::F(9) => {
                    self.blocked = !self.blocked;
                    self.chat.composer_mut().set_blocked(
                        self.blocked
                            .then_some("Disconnected · submission unavailable"),
                    );
                }
                KeyCode::F(11) => {
                    if let Some(request) = self.editor.take() {
                        self.chat.composer_mut().finish_editor(
                            &request.ticket,
                            EditorResult::Text(format!("{}\nEdited outside the box", request.text)),
                        );
                    }
                }
                KeyCode::F(12) => {
                    let state = self.chat.composer_mut().saved();
                    self.chat.composer_mut().restore_saved(&state);
                    self.ticket = None;
                }
                _ => return self.edit(event),
            }
            return true;
        }
        self.edit(event)
    }
    fn edit(&mut self, event: &Event) -> bool {
        if !self.overlay_open()
            && matches!(event, Event::Key(key) if key.modifiers == KeyModifiers::ALT && matches!(key.code, KeyCode::PageUp | KeyCode::PageDown | KeyCode::End))
        {
            self.chat.update(event);
            return true;
        }
        if matches!(event, Event::Key(key) if key.code == KeyCode::F(4)
            || (matches!(key.code, KeyCode::Char('t')) && key.modifiers.contains(KeyModifiers::CONTROL)))
        {
            return false;
        }
        if let Some(submission) = self.chat.composer_mut().update(event) {
            self.sent(&submission);
        }
        self.host_intents();
        true
    }
    fn sent(&mut self, submission: &Submission) {
        self.count += 1;
        self.ticket = Some(submission.ticket.clone());
        self.chat.push_submission(submission, "20:10");
        self.note = "Message fixed; fresh box ready".into();
    }
    fn host_intents(&mut self) {
        for intent in self.chat.composer_mut().take_intents() {
            match intent {
                Intent::Save { ticket, state } => {
                    self.saved = Some(*state);
                    self.chat.composer_mut().acknowledge_save(&ticket, Ok(()));
                }
                Intent::Editor(request) => {
                    self.editor = Some(request);
                    self.note = "Simulated editor open · F11 returns text".into();
                }
                Intent::Quit => self.quit = true,
                Intent::Stop | Intent::InterruptAfterStep => {
                    if let Some(ticket) = self.ticket.take() {
                        self.chat.acknowledge_submission(&ticket, Err("Cancelled"));
                    }
                    self.chat.composer_mut().set_working(false);
                    self.note = "Host stop requested · queue paused".into();
                }
                Intent::Command(id) => self.note = format!("Host command: {id}"),
                Intent::Background => self.note = "Host shell background requested".into(),
                Intent::ClipboardImage => {
                    self.chat
                        .composer_mut()
                        .insert_image_with_size("acme-clipboard-image", Some((1280, 720)));
                }
                Intent::Rewind => self.note = "Host rewind requested before submission".into(),
            }
        }
    }
    pub(super) fn should_quit(&self) -> bool {
        self.quit
    }
    pub(super) fn overlay_open(&self) -> bool {
        self.chat.composer().overlay_open()
    }
    pub(super) fn tick(&mut self, elapsed: Duration) {
        self.chat.tick(elapsed);
        self.host_intents();
    }
    pub(super) fn render(&mut self, frame: &mut Frame<'_>, area: Rect, theme: Presentation) {
        let [chat, note, controls] = Layout::vertical([
            Constraint::Min(0),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .areas(area);
        self.chat.render_docked_with(frame, chat, theme);
        frame.render_widget(
            Paragraph::new(format!(
                "Harness: {} submission intents · {}",
                self.count, self.note
            ))
            .style(theme.muted()),
            note,
        );
        frame.render_widget(
            Paragraph::new("F5 accept/end · F6 refuse/error · F7 reset · F8 stale · F9 offline · F11 editor · F12 restart")
                .style(theme.muted()),
            controls,
        );
    }
}
