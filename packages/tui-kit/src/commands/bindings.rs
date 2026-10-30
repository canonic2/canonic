//! Owns validated key sequences, explicit timeout and scope cancellation.

use super::{Command, Error};
use crate::event::{Event, KeyCode, KeyEventKind, KeyModifiers};
use std::{collections::HashSet, time::Duration};

/// Exact key and modifiers, independent of terminal reporting modes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stroke {
    /// Normalized key code supplied by the event backend.
    pub code: KeyCode,
    /// Exact modifier set.
    pub modifiers: KeyModifiers,
}
impl Stroke {
    /// Construct an exact key stroke.
    #[must_use]
    pub fn new(mut code: KeyCode, mut modifiers: KeyModifiers) -> Self {
        if let KeyCode::Char(c) = code
            && c.is_ascii_alphabetic()
        {
            if c.is_ascii_uppercase() {
                modifiers.insert(KeyModifiers::SHIFT);
            }
            code = KeyCode::Char(c.to_ascii_lowercase());
        }
        if code == KeyCode::BackTab {
            modifiers.insert(KeyModifiers::SHIFT);
        }
        Self { code, modifiers }
    }
    /// Design-system key spelling, independent of platform modifier icons.
    #[must_use]
    pub fn label(self) -> String {
        let mut text = String::new();
        for (flag, name) in [
            (KeyModifiers::CONTROL, "ctrl+"),
            (KeyModifiers::ALT, "alt+"),
            (KeyModifiers::SHIFT, "shift+"),
        ] {
            if self.modifiers.contains(flag)
                && !(flag == KeyModifiers::SHIFT
                    && (self.code == KeyCode::BackTab
                        || matches!(self.code, KeyCode::Char(c) if c.is_ascii_alphabetic())))
            {
                text.push_str(name);
            }
        }
        let key = match self.code {
            KeyCode::Char(c)
                if c.is_ascii_alphabetic() && self.modifiers.contains(KeyModifiers::SHIFT) =>
            {
                c.to_uppercase().to_string()
            }
            KeyCode::Char(' ') => "space".into(),
            KeyCode::Char(c) => c.to_string(),
            KeyCode::F(n) => format!("F{n}"),
            KeyCode::Up => "↑".into(),
            KeyCode::Down => "↓".into(),
            KeyCode::Left => "←".into(),
            KeyCode::Right => "→".into(),
            KeyCode::BackTab => "shift+tab".into(),
            code => format!("{code:?}").to_lowercase(),
        };
        text.push_str(&key);
        text
    }
}

/// One host-supplied shortcut, from one to four strokes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Binding(pub Vec<Stroke>);
impl Binding {
    /// Effective shortcut text for both help and pending hints.
    #[must_use]
    pub fn label(&self) -> String {
        self.0
            .iter()
            .map(|s| s.label())
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// Result of scoped dispatch. All outcomes except `Unhandled` consume input.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The focused component may handle this event.
    Unhandled,
    /// Exactly one host invocation; no operation is executed here.
    Invoke(String),
    /// Waiting for the next key, with effective sequence text.
    Pending(String),
    /// The pending sequence was dismissed, expired or did not match.
    Cancelled,
    /// A pending continuation did not match; the host may show this for three seconds.
    Mismatch(String),
}

/// A map for one focus scope. Hosts reset it whenever that scope changes/closes.
pub struct Dispatcher {
    entries: Vec<(String, Binding)>,
    pending: Vec<Stroke>,
    elapsed: Duration,
    timeout: Duration,
}
impl Dispatcher {
    /// Validate effective shortcuts; duplicate and prefix conflicts are rejected.
    ///
    /// # Errors
    /// Rejects duplicate IDs, empty/long sequences and reserved newline/exit keys.
    pub fn new(commands: &[Command], timeout: Duration) -> Result<Self, Error> {
        let mut ids = HashSet::new();
        let mut entries: Vec<(String, Binding)> = Vec::new();
        for command in commands {
            if !ids.insert(&command.id) {
                return Err(Error::Duplicate(command.id.clone()));
            }
            for binding in &command.bindings {
                if timeout.is_zero()
                    || binding.0.is_empty()
                    || binding.0.len() > 4
                    || binding.0.iter().any(|s| {
                        matches!(s.code, KeyCode::Enter | KeyCode::Esc)
                            || (s.modifiers.contains(KeyModifiers::CONTROL)
                                && matches!(s.code, KeyCode::Char('c' | 'j')))
                    })
                {
                    return Err(Error::Binding(binding.label()));
                }
                if entries
                    .iter()
                    .any(|(_, b)| b.0.starts_with(&binding.0) || binding.0.starts_with(&b.0))
                {
                    return Err(Error::Binding(binding.label()));
                }
                // Disabled entries still reserve their shortcuts, but cannot invoke.
                entries.push((command.id.clone(), binding.clone()));
            }
        }
        entries.retain(|(id, _)| commands.iter().any(|c| &c.id == id && c.disabled.is_none()));
        Ok(Self {
            entries,
            pending: Vec::new(),
            elapsed: Duration::ZERO,
            timeout,
        })
    }
    /// Cancel on focus changes, modal opening/closing or descriptor replacement.
    pub fn reset(&mut self) {
        self.pending.clear();
        self.elapsed = Duration::ZERO;
    }
    /// Advance the caller's clock; returns whether a pending sequence expired.
    pub fn tick(&mut self, elapsed: Duration) -> bool {
        if self.pending.is_empty() {
            return false;
        }
        self.elapsed = self.elapsed.saturating_add(elapsed);
        if self.elapsed >= self.timeout {
            self.reset();
            true
        } else {
            false
        }
    }
    /// Effective pending hint, empty when no sequence owns the next key.
    #[must_use]
    pub fn pending(&self) -> String {
        Binding(self.pending.clone()).label()
    }
    /// Whether the design's delayed continuation strip should be visible.
    #[must_use]
    pub fn hint_visible(&self) -> bool {
        !self.pending.is_empty() && self.elapsed >= Duration::from_millis(400)
    }
    /// Remaining sequences and invocation IDs, sorted by their effective key.
    #[must_use]
    pub fn continuations(&self) -> Vec<(String, String)> {
        if self.pending.is_empty() {
            return vec![];
        }
        let mut rows: Vec<_> = self
            .entries
            .iter()
            .filter(|(_, b)| b.0.starts_with(&self.pending))
            .map(|(id, b)| {
                (
                    Binding(b.0[self.pending.len()..].to_vec()).label(),
                    id.clone(),
                )
            })
            .collect();
        rows.sort();
        rows
    }
    /// Dispatch press events only. Pending paste/mismatches are consumed.
    pub fn update(&mut self, event: &Event) -> Outcome {
        let Event::Key(key) = event else {
            if matches!(
                event,
                Event::Paste(_) | Event::FocusLost | Event::FocusGained
            ) && !self.pending.is_empty()
            {
                self.reset();
                return Outcome::Cancelled;
            }
            return Outcome::Unhandled;
        };
        if key.kind != KeyEventKind::Press {
            return if self.pending.is_empty() {
                Outcome::Unhandled
            } else {
                Outcome::Pending(self.pending())
            };
        }
        // Emergency exit always returns to the application, even during a leader.
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.reset();
            return Outcome::Unhandled;
        }
        let had_pending = !self.pending.is_empty();
        if had_pending && key.code == KeyCode::Esc {
            self.reset();
            return Outcome::Cancelled;
        }
        self.pending.push(Stroke::new(key.code, key.modifiers));
        if let Some((id, _)) = self.entries.iter().find(|(_, b)| b.0 == self.pending) {
            let id = id.clone();
            self.reset();
            return Outcome::Invoke(id);
        }
        if self
            .entries
            .iter()
            .any(|(_, b)| b.0.starts_with(&self.pending))
        {
            self.elapsed = Duration::ZERO;
            return Outcome::Pending(self.pending());
        }
        let failed = self.pending();
        self.reset();
        if had_pending {
            Outcome::Mismatch(format!("{failed} isn't a command · F1 lists them"))
        } else {
            Outcome::Unhandled
        }
    }
}
