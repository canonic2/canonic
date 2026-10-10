//! Optional host decoder for terminals without bracketed paste.

use crate::event::{Event, KeyCode, KeyEventKind};
use std::time::Duration;

const GAP: Duration = Duration::from_millis(8);
const IDLE: Duration = Duration::from_millis(20);

/// Buffers plain character presses briefly. Four rapid characters form a paste;
/// newlines in that burst are inserted, never submitted. Explicit paste bypasses it.
/// Feed physical events before dispatch and flush with the host's monotonic clock.
#[derive(Default)]
pub struct PasteBurst {
    pending: Vec<Event>,
    last: Duration,
}

impl PasteBurst {
    /// Accept a decoded terminal event; output preserves order and modifier semantics.
    pub fn push(&mut self, event: Event, now: Duration) -> Vec<Event> {
        let mut output = if now.saturating_sub(self.last) > GAP {
            self.flush()
        } else {
            Vec::new()
        };
        let character = matches!(&event, Event::Key(key) if key.kind == KeyEventKind::Press && key.modifiers.is_empty() && matches!(key.code, KeyCode::Char(_)));
        let newline = matches!(&event, Event::Key(key) if key.kind == KeyEventKind::Press && key.modifiers.is_empty() && key.code == KeyCode::Enter)
            && self.pending.len() >= 4;
        if character || newline {
            self.pending.push(event);
            self.last = now;
            if self.pending.len() >= 65_536 {
                output.extend(self.flush());
            }
        } else {
            output.extend(self.flush());
            output.push(event);
        }
        output
    }
    /// Flush after twenty milliseconds without input; ordinary short typing stays keys.
    pub fn flush_idle(&mut self, now: Duration) -> Vec<Event> {
        if now.saturating_sub(self.last) >= IDLE {
            self.flush()
        } else {
            Vec::new()
        }
    }
    /// Flush before changing focus or closing the host.
    pub fn flush(&mut self) -> Vec<Event> {
        let events = std::mem::take(&mut self.pending);
        if events.len() < 4 {
            return events;
        }
        let text: String = events
            .iter()
            .filter_map(|event| match event {
                Event::Key(key) => match key.code {
                    KeyCode::Char(c) => Some(c),
                    KeyCode::Enter => Some('\n'),
                    _ => None,
                },
                _ => None,
            })
            .collect();
        vec![Event::Paste(text)]
    }
}
