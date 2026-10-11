//! Simulates delayed host effects and the remaining transcript design states.

use super::{ChatDemo, PATCH, alt, key, markdown, table};
use crate::{
    chat::{Content, End, Intent, Part, Role, Turn},
    event::{Event, KeyCode},
};

impl ChatDemo {
    pub(super) fn configure_alignment(&mut self) {
        self.chat.set_input("Independent draft · preserved caret");
        match self.scenario {
            51..=53 => {
                self.replace(vec![markdown("Copy **the original Markdown**, including [the reference](https://example.com/reference).")], End::Done("12s".into()));
                self.chat.open_selection();
                self.chat.update(&key(KeyCode::Char('y')));
                self.effects();
                if self.scenario == 52 { self.control(KeyCode::F(6)); }
                if self.scenario == 53 { self.control(KeyCode::F(5)); }
            }
            54..=56 => {
                self.replace(vec![markdown("Read [the reference](https://example.com/reference).")], End::Done("12s".into()));
                self.chat.open_selection();
                self.chat.update(&key(KeyCode::Char('o')));
                self.effects();
                if self.scenario == 55 { self.control(KeyCode::F(6)); }
                if self.scenario == 56 { self.control(KeyCode::F(5)); }
            }
            57..=59 => {
                if self.scenario == 59 {
                    self.replace(vec![Part::new("patch", Content::Diff { file: "src/digest/settings.ts".into(), patch: PATCH.into(), summary: "host patch · full source available".into() })], End::Done("12s".into()));
                } else { self.replace(vec![table(6)], End::Done("12s".into())); }
                self.chat.open_selection();
                self.chat.update(&key(KeyCode::Char('o')));
                self.effects();
                if self.scenario == 58 { self.control(KeyCode::F(6)); }
            }
            60 | 61 => {
                self.long_history();
                self.chat.update(&alt(KeyCode::Home));
                self.chat.update(&alt(KeyCode::Home));
                self.effects();
                if self.scenario == 61 { self.control(KeyCode::F(6)); }
            }
            62..=64 => {
                self.chat.reset_transcript();
                for index in 0..100 {
                    self.chat.push_record(Turn::new(&format!("loaded-{index}"), Role::Assistant, "acme", &format!("Loaded message {index}.")));
                }
                self.chat.set_earlier_count(340);
                self.chat.update(&alt(KeyCode::Home));
                self.chat.update(&alt(KeyCode::Home));
                self.effects();
                self.control(KeyCode::F(5));
                self.chat.update(&alt(KeyCode::End));
                self.effects();
                if self.scenario == 63 { self.control(KeyCode::F(6)); }
                if self.scenario == 64 { self.control(KeyCode::F(5)); }
            }
            65 => {
                self.replace(vec![markdown("Partial output kept after failure.")], End::Failed("transport disconnected".into()));
                self.chat.set_retry_available(false);
            }
            66 => {
                self.chat.update(&Event::Key(crate::event::KeyEvent::new(KeyCode::Char('x'), crate::event::KeyModifiers::CONTROL)));
                self.chat.update(&key(KeyCode::Char('t')));
            }
            67 => {
                self.chat.reset_transcript();
                self.chat.push_record(Turn::new("note", Role::Note, "", "– cancelled · question dismissed"));
                self.chat.push_record(Turn::new("reply", Role::Assistant, "acme", "The draft stays in the message box."));
            }
            68 => self.replace(vec![Part::new("reasoning", Content::Reasoning { summary: "no reasoning text supplied".into(), text: String::new() }), markdown("Reasoning text is unavailable; the answer remains readable.")], End::Done("12s".into())),
            69 => self.replace(vec![Part::new("unknown", Content::Attachment { name: "artifact.bin".into(), size: "unknown kind · unknown size".into(), target: Some("artifact.bin".into()), unavailable: String::new() })], End::Done("12s".into())),
            70 => self.replace(vec![markdown("Raw: <details><summary>HTML</summary>body</details>\nEscape: \x1b[31mred\x1b[0m\nControls: \x07 \x00 \x7f\nUnicode: 界 e\u{301} 👩‍💻")], End::Done("12s".into())),
            71 => self.replace(vec![Part::new("oversize", Content::Code { language: "log".into(), file: String::new(), line: None, text: "source line not discarded silently\n".repeat(3000) })], End::Done("12s".into())),
            72..=74 => {
                self.chat.open_search();
                self.chat.update(&Event::Paste("timezone".into()));
            }
            75 => {
                self.chat.reset_transcript();
                self.chat.set_welcome(crate::chat::Welcome {
                    title: "canonic".into(), context: "acme · orbit-api".into(),
                    recent: vec!["Recent · digest headers · 20:03".into()],
                    suggestions: vec!["check the retry queue".into(), "review the digest changes".into()],
                });
            },
            _ => {}
        }
        self.chat.set_input("Independent draft · preserved caret");
    }
    pub(super) fn fail_effect(&mut self, intent: Intent) {
        match intent {
            Intent::Earlier { request, .. } => {
                self.chat.complete_earlier(
                    &request,
                    Err("offline (simulation)"),
                    self.chat.history_counts().0,
                );
            }
            Intent::Copy { request, .. } => {
                self.chat
                    .acknowledge(&request, Err("clipboard unavailable (simulation)"));
            }
            Intent::Open { request, .. } => {
                self.chat
                    .acknowledge(&request, Err("host refused the target (simulation)"));
            }
            Intent::Inspect { request, .. } => {
                self.chat
                    .acknowledge(&request, Err("full source unavailable (simulation)"));
            }
            Intent::Latest { request } | Intent::Retry { request, .. } => {
                self.chat.acknowledge(&request, Err("offline (simulation)"));
            }
            _ => {}
        }
    }
}
