//! Maps arrival, closure, caution and edge frames to deterministic public dialog behavior.

use super::{Body, DialogDemo, Stack};
use crate::{
    dialog::DialogView,
    event::{Event, KeyCode, KeyEvent, KeyModifiers},
};
use std::time::Duration;

impl DialogDemo {
    pub(super) fn extended_reset(&mut self) {
        self.stack =
            Stack::new_timed(vec![2, 0, 1]).unwrap_or_else(|_| unreachable!("distinct targets"));
        match self.scenario {
            11 | 32 => {
                self.stack.typed();
                let dialog = self.dialog(Body::prompt("tracker-server"), "tracker server");
                self.stack
                    .arrive(dialog)
                    .unwrap_or_else(|_| unreachable!("valid arrival"));
                if self.scenario == 32 {
                    let next = self.dialog(Body::choices(), "second host request");
                    self.stack
                        .arrive(next)
                        .unwrap_or_else(|_| unreachable!("valid arrival"));
                    self.note = "+1 waiting · host order preserved".into();
                }
            }
            12 | 25 => self.open(Body::prompt("APP-22-invoice-export"), "rename branch"),
            13 => {
                self.stack.set_terminal_focused(false);
                self.open(Body::prompt("APP-22-invoice-export"), "rename branch");
                self.stack.tick(Duration::from_secs(120));
                let attention = self.stack.take_attention();
                self.note = format!(
                    "{} attention intent · notification and title simulated",
                    attention.len()
                );
            }
            14 => {
                self.open(Body::prompt("keep this input"), "rename branch");
                self.ready();
                self.local_key(KeyCode::Char('c'), KeyModifiers::CONTROL);
            }
            15 => {
                self.nested();
                self.stack.clear_with_reason("acme cancelled the rename");
            }
            16 => {
                self.nested();
                if let Some(ticket) = self.stack.ticket().cloned() {
                    self.stack.remove(&ticket, "origin no longer has it");
                }
            }
            17 => self.locked_fixture(),
            18 => {
                self.open(
                    Body::prompt("outside press leaves me intact"),
                    "rename branch",
                );
                self.ready();
                self.local_key(KeyCode::F(9), KeyModifiers::NONE);
            }
            19 => self.error_fixture(),
            20 | 21 => {
                self.open(Body::filtered("ma"), "request review");
                self.ready();
                if self.scenario == 21 {
                    self.local_key(KeyCode::Esc, KeyModifiers::NONE);
                }
            }
            22 => self.open(Body::choices(), "delete branch"),
            23 => self.open(
                Body::text(
                    &"commit message · supplied inspection row\n".repeat(30),
                    true,
                ),
                "commit message",
            ),
            24 => self.nested(),
            26 | 27 => {
                self.open(Body::choices(), "discard local draft");
                self.ready();
                self.local_key(KeyCode::Char('2'), KeyModifiers::NONE);
                self.local_key(KeyCode::Enter, KeyModifiers::NONE);
                if self.scenario == 27 {
                    self.tick(Duration::from_secs(3));
                    self.note = "Arming expired · enter arms again".into();
                }
            }
            28 | 29 => self.named_fixture(),
            30 => {
                self.open(Body::prompt("APP-22-invoice-export-"), "rename branch");
                self.local_key(KeyCode::Char('y'), KeyModifiers::NONE);
            }
            31 => {
                self.open(Body::choices(), "clean up");
                self.ready();
                self.local_key(KeyCode::Enter, KeyModifiers::NONE);
            }
            _ => unreachable!("registered extended fixture"),
        }
        if !matches!(self.scenario, 11 | 12 | 32) {
            self.ready();
        }
    }
    fn ready(&mut self) {
        self.stack.tick(Duration::from_millis(300));
    }
    fn locked_fixture(&mut self) {
        let mut dialog = self.dialog(Body::locked(), "answer to continue");
        dialog.close.escape = false;
        dialog.view.footer = "1 keep says no · 2 discard · enter pick".into();
        dialog.view.compact_footer = "enter pick".into();
        self.stack
            .push(dialog)
            .unwrap_or_else(|_| unreachable!("valid locked fixture"));
        self.ready();
        self.local_key(KeyCode::Esc, KeyModifiers::NONE);
    }
    fn error_fixture(&mut self) {
        self.open(Body::prompt("APP-22-invoice-export"), "rename branch");
        if let Some(ticket) = self.stack.ticket().cloned() {
            self.stack.update_view(
                &ticket,
                DialogView {
                    scope: "origin".into(),
                    message: "Name already exists on origin. Edit it and retry.".into(),
                    message_error: true,
                    footer: "enter retry · esc cancel".into(),
                    compact_footer: "enter retry · esc".into(),
                },
            );
        }
    }
    fn named_fixture(&mut self) {
        let mut dialog = self.dialog(
            Body::named(if self.scenario == 28 {
                "acme#2"
            } else {
                "acme#22"
            }),
            "delete session",
        );
        dialog.view.message =
            "Deletes this fixture's history and stashes. Type acme#22 to confirm.".into();
        self.stack
            .push(dialog)
            .unwrap_or_else(|_| unreachable!("valid named fixture"));
    }
    fn nested(&mut self) {
        self.open(Body::prompt("APP-22-invoice-export"), "rename branch");
        self.open(Body::choices(), "name taken");
    }
    fn local_key(&mut self, code: KeyCode, modifiers: KeyModifiers) {
        self.update(&Event::Key(KeyEvent::new(code, modifiers)));
    }
}
