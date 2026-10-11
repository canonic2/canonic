//! Owns optional injected-clock arrival, opening guards, notices and host attention intents.

use super::{Dialog, Dismissed, Dispatch, Invalid, Reason, Stack, Ticket};
use crate::event::{Event, KeyCode, KeyEventKind, KeyModifiers};
use std::{collections::VecDeque, time::Duration};

/// Result of a validated asynchronous arrival.
#[derive(Clone, Debug)]
pub enum Arrival {
    /// The root opened immediately.
    Opened(Ticket),
    /// The request waits in host order; no focus changed.
    Waiting,
}
/// Explicit host notification request; never emitted by rendering.
#[derive(Clone, Debug)]
pub struct Attention {
    /// Unique opening for deduplication and stale-result rejection.
    pub ticket: Ticket,
    /// Safe task title for a host-owned notification.
    pub subject: String,
    /// Suggested terminal title. The host owns OSC output and restoration.
    pub title: String,
}
pub(super) struct Timing<B, T> {
    pub(super) queue: VecDeque<Dialog<B, T>>,
    clock: Duration,
    typed_at: Option<Duration>,
    opened_at: Duration,
    armed: bool,
    focused: bool,
    pub(super) notice: Option<(String, Duration)>,
    attention: Vec<Attention>,
}
impl<B, T> Default for Timing<B, T> {
    fn default() -> Self {
        Self {
            queue: VecDeque::new(),
            clock: Duration::ZERO,
            typed_at: None,
            opened_at: Duration::ZERO,
            armed: false,
            focused: true,
            notice: None,
            attention: Vec::new(),
        }
    }
}
impl<B, T: Clone + Eq> Stack<B, T> {
    /// Create a stack with typing-aware arrival and a 300 ms opening guard.
    /// Hosts advance time with `tick` and report background typing with `typed`.
    /// The low-level `new` constructor leaves these host policies disabled.
    /// # Errors
    /// Rejects duplicate base targets.
    pub fn new_timed(targets: Vec<T>) -> Result<Self, Invalid> {
        let mut stack = Self::new(targets)?;
        stack.timing = Some(Timing::default());
        Ok(stack)
    }
    /// Record actual background typing/paste, without reading input or a clock.
    pub fn typed(&mut self) {
        if let Some(timing) = &mut self.timing {
            timing.typed_at = Some(timing.clock);
        }
    }
    /// Supply terminal focus without performing notification or title I/O.
    pub fn set_terminal_focused(&mut self, focused: bool) {
        if let Some(timing) = &mut self.timing {
            timing.focused = focused;
        }
    }
    /// Take pending attention requests once; closed openings are discarded.
    pub fn take_attention(&mut self) -> Vec<Attention> {
        let Some(timing) = &mut self.timing else {
            return Vec::new();
        };
        std::mem::take(&mut timing.attention)
            .into_iter()
            .filter(|intent| {
                self.layers
                    .iter()
                    .any(|layer| layer.ticket == intent.ticket)
            })
            .collect()
    }
    /// Number of asynchronous arrivals waiting in the host's FIFO order.
    #[must_use]
    pub fn waiting_len(&self) -> usize {
        self.timing.as_ref().map_or(0, |timing| timing.queue.len())
    }
    /// Withdraw an unopened host request, returning its body without touching focus.
    pub fn withdraw_waiting(&mut self, id: &str) -> Option<Dialog<B, T>> {
        let timing = self.timing.as_mut()?;
        let index = timing.queue.iter().position(|dialog| dialog.id == id)?;
        timing.queue.remove(index)
    }
    /// Validate and enqueue an asynchronous root, preserving every existing body.
    /// Without timing enabled, this behaves as an explicit `push`.
    /// # Errors
    /// Rejects duplicate open/waiting IDs, invalid targets or more than sixteen arrivals.
    pub fn arrive(&mut self, dialog: Dialog<B, T>) -> Result<Arrival, Invalid> {
        self.validate(&dialog, false)?;
        let Some(timing) = &mut self.timing else {
            return self.push(dialog).map(Arrival::Opened);
        };
        if timing.queue.iter().any(|waiting| waiting.id == dialog.id) {
            return Err(Invalid::DuplicateId);
        }
        if timing.queue.len() >= 16 {
            return Err(Invalid::QueueLimit);
        }
        timing.queue.push_back(dialog);
        Ok(self
            .open_waiting(false)
            .map_or(Arrival::Waiting, Arrival::Opened))
    }
    /// Advance host time; opens at most one queued root after a one-second pause.
    pub fn tick(&mut self, elapsed: Duration) {
        if let Some(timing) = &mut self.timing {
            timing.clock = timing.clock.saturating_add(elapsed);
            if timing
                .notice
                .as_ref()
                .is_some_and(|(_, until)| timing.clock >= *until)
            {
                timing.notice = None;
            }
        }
        self.open_waiting(false);
    }
    /// Explicitly open the next waiting root. Active/nested dialogs remain untouched.
    pub fn open_waiting(&mut self, force: bool) -> Option<Ticket> {
        if !self.is_empty() {
            return None;
        }
        let timing = self.timing.as_mut()?;
        if !force
            && timing
                .typed_at
                .is_some_and(|typed| timing.clock.saturating_sub(typed) < Duration::from_secs(1))
        {
            return None;
        }
        let dialog = timing.queue.pop_front()?;
        Some(self.insert(dialog))
    }
    pub(super) fn opened(&mut self, ticket: &Ticket) {
        if let Some(timing) = &mut self.timing {
            timing.opened_at = timing.clock;
            timing.armed = false;
            timing.notice = None;
            timing.attention.retain(|intent| {
                self.layers
                    .iter()
                    .any(|layer| layer.ticket == intent.ticket)
            });
            if !timing.focused {
                timing.attention.push(Attention {
                    ticket: ticket.clone(),
                    subject: self
                        .layers
                        .last()
                        .map_or_else(String::new, |layer| layer.title.clone()),
                    title: "! canonic · needs you".into(),
                });
            }
        }
    }
    pub(super) fn returned(&mut self) {
        if let Some(timing) = &mut self.timing {
            timing.armed = false;
            timing.opened_at = timing.clock;
            timing.notice = None;
        }
    }
    pub(super) fn notice(&mut self, text: &str) {
        if let Some(timing) = &mut self.timing {
            timing.notice = Some((
                crate::text::clean(text, 512, false),
                timing.clock.saturating_add(Duration::from_secs(3)),
            ));
        }
    }
    /// Remove an active host request and show its safe reason for three seconds.
    pub fn remove(&mut self, ticket: &Ticket, reason: &str) -> Option<Dismissed<B>> {
        let title = self
            .layers
            .last()
            .filter(|layer| &layer.ticket == ticket)?
            .title
            .clone();
        let closed = self.close(ticket, Reason::Cancelled)?;
        self.notice(&if self.is_empty() {
            format!("dialog closed · {reason}")
        } else {
            format!("! dialog · {title} closed · {reason}")
        });
        Some(closed)
    }
    /// Clear all layers and waiting arrivals, then show the host's reason for three seconds.
    pub fn clear_with_reason(&mut self, reason: &str) -> Vec<Dismissed<B>> {
        let closed = self.clear();
        self.notice(&format!("{} dialogs closed · {reason}", closed.len()));
        closed
    }
    /// Update labels for the active opening without replacing input or its ticket.
    pub fn update_view(&mut self, ticket: &Ticket, view: super::DialogView) -> bool {
        let Some(layer) = self
            .layers
            .last_mut()
            .filter(|layer| &layer.ticket == ticket)
        else {
            return false;
        };
        layer.view = view.sanitized();
        true
    }
    /// Current activity text; hosts paint it only when a panel does not cover it.
    #[must_use]
    pub fn activity(&self) -> String {
        if let Some(timing) = &self.timing {
            if let Some((notice, _)) = &timing.notice {
                return notice.clone();
            }
            if self.is_empty()
                && let Some(dialog) = timing.queue.front()
            {
                return format!(
                    "! dialog waiting · {} · ctrl+g opens it",
                    crate::text::clean(&dialog.title, 128, false)
                );
            }
            if !self.is_empty() {
                if timing.armed {
                    return "! dialog · ctrl+c again closes it".into();
                }
                if !timing.focused {
                    return format!(
                        "! dialog · waiting for you · {}s",
                        timing.clock.saturating_sub(timing.opened_at).as_secs()
                    );
                }
                if timing.clock.saturating_sub(timing.opened_at) < Duration::from_millis(300) {
                    return "! dialog · keys work in a moment".into();
                }
            }
        }
        if self.is_empty() {
            "ready".into()
        } else if self.len() > 1 {
            format!("! dialog {} of {} · esc goes back", self.len(), self.len())
        } else if self.close_policy().is_some_and(|policy| policy.escape) {
            "! dialog · esc closes it".into()
        } else {
            "! dialog · pick one to go on".into()
        }
    }
    pub(super) fn timed_dispatch(&mut self, event: &Event) -> Option<Dispatch<B>> {
        let timing = self.timing.as_mut()?;
        let Event::Key(key) = event else { return None };
        if self.layers.is_empty() {
            if key.code == KeyCode::Char('g')
                && key.modifiers == KeyModifiers::CONTROL
                && key.kind == KeyEventKind::Press
                && !timing.queue.is_empty()
            {
                self.open_waiting(true);
                return Some(Dispatch::Consumed);
            }
            return None;
        }
        let close =
            matches!(key.code, KeyCode::Char('c' | 'd')) && key.modifiers == KeyModifiers::CONTROL;
        if close {
            if key.kind != KeyEventKind::Press {
                return Some(Dispatch::Consumed);
            }
            if timing.armed {
                return Some(
                    self.pop(Reason::Cancelled)
                        .map_or(Dispatch::Consumed, Dispatch::Dismissed),
                );
            }
            timing.armed = true;
            timing.notice = None;
            return Some(Dispatch::Consumed);
        }
        if key.kind != KeyEventKind::Release {
            timing.armed = false;
        }
        if timing.clock.saturating_sub(timing.opened_at) < Duration::from_millis(300)
            && (key.code == KeyCode::Enter || matches!(key.code, KeyCode::Char('0'..='9')))
        {
            return Some(Dispatch::Consumed);
        }
        None
    }
}
