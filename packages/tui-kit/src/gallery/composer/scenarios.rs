//! Additional composer states are produced by the same interactions as live input.

use super::ComposerDemo;
use crate::{
    composer::{EditorResult, HistoryEntry, InputMode},
    editor::Editor,
    event::{Event, KeyCode, KeyEvent, KeyModifiers},
};

fn key(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
}
impl ComposerDemo {
    pub(super) fn extended(index: usize) -> Self {
        let base = match index {
            29 | 43 => 11,
            30 => 12,
            32..=36 => 8,
            38 => 14,
            39 => 20,
            40..=42 => 15,
            44 => 28,
            48 => 22,
            50..=51 => 24,
            52 => 25,
            54 => 2,
            _ => 0,
        };
        let mut demo = Self::scenario(base);
        if index <= 39 {
            demo.recall_scenario(index);
        } else {
            demo.utility_scenario(index);
        }
        demo.scenario = index;
        demo
    }
    fn recall_scenario(&mut self, index: usize) {
        match index {
            29 => self.resolve(true),
            30 => {
                self.edit(&Event::Key(KeyEvent::new(
                    KeyCode::Char('d'),
                    KeyModifiers::CONTROL,
                )));
            }
            31 => {
                self.chat.composer_mut().set_history(Vec::new());
                self.edit(&key(KeyCode::Up));
            }
            32 => {
                self.chat.composer_mut().browse_history(4);
            }
            33 => {
                self.edit(&Event::Paste("[edited] ".into()));
            }
            34 => {
                self.edit(&key(KeyCode::Enter));
            }
            35 => {
                self.chat.composer_mut().restore_draft();
                let history = self
                    .chat
                    .composer()
                    .history()
                    .iter()
                    .enumerate()
                    .map(|(i, text)| {
                        let mut editor = Editor::default();
                        editor.set_text(text);
                        HistoryEntry {
                            document: editor.document(),
                            mode: InputMode::Message,
                            origin: (i >= 6).then(|| "yesterday".into()),
                        }
                    })
                    .collect();
                self.chat.composer_mut().set_history_entries(history);
                self.chat.composer_mut().browse_history(9);
            }
            36 => {
                self.chat.composer_mut().browse_history(12);
                self.edit(&key(KeyCode::Up));
            }
            37 => self.chat.composer_mut().stash_draft(),
            38 => {
                self.chat.set_input("");
                self.chat.composer_mut().restore_stash(1);
            }
            39 => {
                self.edit(&key(KeyCode::Char('d')));
            }
            _ => {}
        }
    }
    fn utility_scenario(&mut self, index: usize) {
        match index {
            40 => {
                self.update(&key(KeyCode::F(11)));
            }
            41 | 42 => {
                if let Some(request) = self.editor.take() {
                    self.chat.composer_mut().finish_editor(
                        &request.ticket,
                        if index == 41 {
                            EditorResult::Text(String::new())
                        } else {
                            EditorResult::Failed("Editor exited with status 1".into())
                        },
                    );
                }
            }
            43 | 44 => {
                self.edit(&key(KeyCode::Esc));
            }
            45 => {
                self.edit(&key(KeyCode::Esc));
                self.edit(&key(KeyCode::Esc));
            }
            46 => {
                self.edit(&Event::Key(KeyEvent::new(
                    KeyCode::Char('c'),
                    KeyModifiers::CONTROL,
                )));
            }
            47 => {
                self.edit(&Event::Paste(
                    "run the digest tests\nthen check the logs".into(),
                ));
            }
            48 => {
                self.edit(&key(KeyCode::Left));
            }
            49 => {
                self.edit(&Event::Paste("/var/log/digest/worker.log".into()));
            }
            50 => {
                self.edit(&key(KeyCode::Enter));
                self.resolve(true);
                if let Some(ticket) = &self.previous {
                    let output = (1..=24)
                        .map(|i| format!("Digest test {i}: passed"))
                        .collect::<Vec<_>>()
                        .join("\n");
                    self.chat.set_shell_output(ticket, &output);
                }
            }
            51 => {
                self.chat.set_input("");
                self.edit(&key(KeyCode::Backspace));
            }
            52 => {
                self.edit(&key(KeyCode::Tab));
            }
            53 => {
                self.chat.set_input("send it to sam");
                self.edit(&key(KeyCode::Char('@')));
                self.edit(&Event::Paste("ex".into()));
            }
            55 => {
                self.chat.set_input("check the digest\\");
                self.edit(&key(KeyCode::Enter));
            }
            56 => {
                self.edit(&key(KeyCode::F(1)));
            }
            _ => {}
        }
    }
}
