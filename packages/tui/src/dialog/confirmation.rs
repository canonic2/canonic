//! Pure caution policies; qualifying an action never executes or authorizes it.

use crate::event::{Event, KeyCode, KeyEventKind};
use std::time::Duration;

/// Host-selected caution for a stable action and its current details.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Caution {
    /// One deliberate Enter qualifies the action.
    Normal,
    /// A second deliberate Enter must follow within three seconds.
    Irreversible,
    /// Input must equal this exact host name; no trimming or case folding.
    TypeName(String),
}
/// Result of a deliberate confirmation key, without any external effect.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    /// Input cannot qualify an action.
    None,
    /// The irreversible action awaits a second deliberate Enter.
    Armed,
    /// The name does not match exactly.
    NameMismatch,
    /// The host may consume its separately identified action intent.
    Confirm,
}
/// Instance-local, injected-clock destructive confirmation.
#[derive(Default)]
pub struct Confirmation {
    clock: Duration,
    armed: Option<(String, Duration)>,
}
impl Confirmation {
    /// Advance host time; an expired action must be armed again.
    pub fn tick(&mut self, elapsed: Duration) {
        self.clock = self.clock.saturating_add(elapsed);
        if self
            .armed
            .as_ref()
            .is_some_and(|(_, until)| self.clock >= *until)
        {
            self.disarm();
        }
    }
    /// Clear on changed selection/details, Escape, replacement or destruction.
    pub fn disarm(&mut self) {
        self.armed = None;
    }
    /// Whether this stable action identity is currently armed.
    #[must_use]
    pub fn armed(&self, action: &str) -> bool {
        self.armed
            .as_ref()
            .is_some_and(|(id, until)| id == action && self.clock < *until)
    }
    /// Qualify only an unmodified Enter press. Numbers/paste/repeats never qualify.
    /// Use an identity incorporating the host revision when action details change.
    pub fn update(
        &mut self,
        event: &Event,
        action: &str,
        caution: &Caution,
        input: &str,
    ) -> Decision {
        let Event::Key(key) = event else {
            return Decision::None;
        };
        if key.code == KeyCode::Esc && key.kind == KeyEventKind::Press {
            self.disarm();
        }
        if key.code != KeyCode::Enter
            || key.kind != KeyEventKind::Press
            || !key.modifiers.is_empty()
        {
            return Decision::None;
        }
        match caution {
            Caution::Normal => {
                self.disarm();
                Decision::Confirm
            }
            Caution::TypeName(name) => {
                self.disarm();
                if input == name {
                    Decision::Confirm
                } else {
                    Decision::NameMismatch
                }
            }
            Caution::Irreversible if self.armed(action) => {
                self.disarm();
                Decision::Confirm
            }
            Caution::Irreversible => {
                self.armed = Some((
                    action.into(),
                    self.clock.saturating_add(Duration::from_secs(3)),
                ));
                Decision::Armed
            }
        }
    }
}
