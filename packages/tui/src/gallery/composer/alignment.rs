//! Exercises recovery and work controls through deterministic public host boundaries.

use super::ComposerDemo;
use crate::{
    composer::{EditorResult, HistoryEntry, InputMode, Intent},
    editor::Editor,
    event::{Event, KeyCode, KeyEvent, KeyModifiers},
    theme::{ColorMode, Presentation, Symbols, Theme},
};
use std::time::Duration;

fn key(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
}
fn ctrl(code: char) -> Event {
    Event::Key(KeyEvent::new(KeyCode::Char(code), KeyModifiers::CONTROL))
}

impl ComposerDemo {
    pub(super) fn aligned(index: usize) -> Self {
        let mut demo = Self::new();
        demo.scenario = index;
        match index {
            57..=65 => demo.recovery_frame(index),
            66..=71 => demo.editor_frame(index),
            72..=76 => demo.shell_frame(index),
            93..=96 => demo.recall_edge_frame(index),
            _ => demo.input_frame(index),
        }
        demo
    }
    fn queue_fixture(&mut self, count: usize) {
        self.chat.composer_mut().set_working(true);
        for index in 1..=count {
            self.chat.set_input(&format!("Digest task {index}"));
            self.edit(&key(KeyCode::Enter));
        }
        self.chat.set_input("Independent draft · caret stays here");
        self.elapsed = Duration::from_secs(14);
        self.chat.composer_mut().set_working_elapsed(self.elapsed);
    }
    fn recovery_frame(&mut self, index: usize) {
        match index {
            57 => {
                self.queue_fixture(128);
                self.edit(&key(KeyCode::Enter));
            }
            58 => {
                self.queue_fixture(2);
                self.chat.composer_mut().edit_queued(2);
                self.edit(&ctrl('g'));
                self.edit(&key(KeyCode::Backspace));
                self.edit(&key(KeyCode::Enter));
            }
            59 => {
                self.queue_fixture(2);
                self.chat.composer_mut().turn_ended(false);
                if let Some(submission) = self.chat.composer_mut().send_next_now() {
                    self.sent(&submission);
                }
            }
            60 | 61 => {
                self.chat.set_input("Version 0.9.4");
                self.edit(&key(KeyCode::Enter));
                for text in ["Then update changelog", "Then run full suite"] {
                    self.chat.set_input(text);
                    self.edit(&key(KeyCode::Enter));
                }
                if index == 60 {
                    self.chat.composer_mut().send_next_now();
                } else {
                    self.resolve(false);
                }
            }
            62 => {
                let mut saved = self.chat.composer_mut().saved();
                saved.draft.cursor = usize::MAX;
                assert!(!self.chat.composer_mut().restore_saved(&saved));
                self.chat
                    .composer_mut()
                    .report_restore_failure("file is truncated");
            }
            63 => {
                self.chat
                    .set_input("Draft kept when the simulated disk is full");
                self.chat.composer_mut().tick(Duration::from_millis(500));
                for intent in self.chat.composer_mut().take_intents() {
                    if let Intent::Save { ticket, .. } = intent {
                        self.chat
                            .composer_mut()
                            .acknowledge_save(&ticket, Err("disk full"));
                    }
                }
            }
            64 => {
                for index in 1..=51 {
                    self.chat.set_input(&format!("Protected draft {index}"));
                    self.chat.composer_mut().stash_draft();
                }
            }
            65 => {
                self.chat.set_input("Earlier draft");
                self.chat.composer_mut().stash_draft();
                self.chat.set_input("Current draft");
                self.chat.composer_mut().restore_stash(1);
                self.chat.composer_mut().restore_stash(1);
            }
            _ => {}
        }
    }
    fn editor_frame(&mut self, index: usize) {
        self.chat.set_input("Original editor draft");
        if index == 71 {
            self.chat
                .composer_mut()
                .insert_image_with_size("acme-image", Some((1280, 720)));
            self.edit(&Event::Paste("one\ntwo\nthree\nfour".into()));
            self.edit(&Event::Key(KeyEvent::new(
                KeyCode::Home,
                KeyModifiers::CONTROL,
            )));
        }
        let request = self
            .chat
            .composer_mut()
            .begin_editor("nvim")
            .expect("fixture draft permits editor handoff");
        self.host_intents();
        match index {
            66..=68 => {
                self.editor = None;
                self.chat.composer_mut().finish_editor(
                    &request.ticket,
                    match index {
                        66 => EditorResult::Failed("No editor · set VISUAL or EDITOR".into()),
                        67 => EditorResult::Text("x".repeat(82 * 1024)),
                        _ => EditorResult::Failed("Couldn't read the file back".into()),
                    },
                );
            }
            69 | 70 => {
                self.edit(&key(KeyCode::Esc));
                if index == 70 {
                    self.chat.composer_mut().finish_editor(
                        &request.ticket,
                        EditorResult::Text("Late text must not replace draft".into()),
                    );
                    self.editor = None;
                }
            }
            71 => {
                self.editor = None;
                self.chat
                    .composer_mut()
                    .finish_editor(&request.ticket, EditorResult::Text(request.text));
                self.edit(&Event::Key(KeyEvent::new(
                    KeyCode::Home,
                    KeyModifiers::CONTROL,
                )));
            }
            _ => {}
        }
    }
    fn shell_frame(&mut self, index: usize) {
        if index == 72 {
            *self = Self::scenario(50);
            self.scenario = index;
            if let Some(ticket) = &self.previous {
                self.chat.toggle_shell_output(ticket);
            }
            return;
        }
        self.edit(&key(KeyCode::Char('!')));
        if index == 76 {
            let mut editor = Editor::default();
            editor.set_text("npm test -- digest --watch=false");
            self.chat
                .composer_mut()
                .set_history_entries(vec![HistoryEntry {
                    document: editor.document(),
                    mode: InputMode::Shell,
                    origin: Some("yesterday".into()),
                }]);
            self.edit(&key(KeyCode::Up));
            return;
        }
        self.chat.composer_mut().set_working(true);
        if index == 73 {
            self.chat.set_input("npm run lint -- digest");
            self.edit(&key(KeyCode::Enter));
        } else {
            self.edit(&ctrl('x'));
            self.edit(&key(KeyCode::Char('b')));
            if index == 75 {
                self.resolve(true);
            }
        }
    }
    fn input_frame(&mut self, index: usize) {
        self.chat.set_input("and bump the app version");
        match index {
            77 | 78 => {
                self.edit(&ctrl('x'));
                if index == 78 {
                    self.edit(&key(KeyCode::Char('q')));
                }
            }
            79 | 80 => {
                self.chat.set_input("");
                self.edit(&key(KeyCode::Char('/')));
                self.edit(&key(KeyCode::Char('h')));
                self.edit(&key(KeyCode::Char('e')));
                self.edit(&key(KeyCode::Char('l')));
                self.edit(&key(KeyCode::Char('p')));
                self.edit(&key(KeyCode::Tab));
                self.edit(&Event::Paste("src/digest --strict".into()));
                if index == 80 {
                    self.edit(&key(KeyCode::Enter));
                    self.chat.push_turn(
                        "/help src/digest --strict",
                        "Reviewing four files · simulated host command",
                        "20:15",
                    );
                    self.chat.composer_mut().set_working(true);
                }
            }
            81 => {
                self.chat.set_input("one\ntwo\nthree\nfour\nfive");
            }
            82 => {
                self.edit(&ctrl('g'));
                self.edit(&key(KeyCode::Esc));
            }
            83 | 84 => {
                self.chat
                    .composer_mut()
                    .insert_image_with_size("acme-image-1", (index == 83).then_some((1280, 720)));
                if index == 83 {
                    self.chat
                        .composer_mut()
                        .insert_image_with_size("acme-image-2", Some((640, 480)));
                    self.edit(&Event::Paste("one\ntwo\nthree\nfour".into()));
                }
            }
            85..=88 => {
                self.queue_fixture(4);
                if index == 85 {
                    self.width = Some(40);
                }
                self.profile = Some(Presentation {
                    colors: match index {
                        86 => ColorMode::Ansi16,
                        87 => ColorMode::NoColor,
                        _ => ColorMode::TrueColor,
                    },
                    symbols: if index == 88 {
                        Symbols::Ascii
                    } else {
                        Symbols::Unicode
                    },
                    ..Presentation::default()
                });
            }
            89 => {
                *self = Self::scenario(16);
                self.scenario = index;
                self.profile = Some(Presentation {
                    colors: ColorMode::Ansi16,
                    ..Presentation::default()
                });
            }
            90 => {
                self.profile = Some(Presentation {
                    theme: Theme::Light,
                    colors: ColorMode::TrueColor,
                    ..Presentation::default()
                });
            }
            91 | 92 => {
                self.queue_fixture(2);
                self.edit(&key(KeyCode::Esc));
                if index == 92 {
                    self.resolve(false);
                }
            }
            _ => {}
        }
    }
    fn recall_edge_frame(&mut self, index: usize) {
        self.chat.set_input("Independent draft");
        if index <= 94 {
            self.chat.composer_mut().open_history_search();
            if index == 93 {
                self.edit(&Event::Paste("dst rollback".into()));
            } else {
                self.edit(&key(KeyCode::Enter));
            }
        } else {
            self.queue_fixture(2);
            self.chat.composer_mut().edit_queued(2);
            if index == 95 {
                self.edit(&key(KeyCode::Up));
                self.edit(&key(KeyCode::Up));
                self.edit(&key(KeyCode::Up));
            } else if let Some(submission) = self.chat.composer_mut().turn_ended(true) {
                self.sent(&submission);
            }
        }
    }
}
