//! Owns safe tool activity, local previews and identified host intents without I/O.

mod model;
pub(crate) mod view;

pub use model::{Artifact, Call, Capabilities, ChildTask, Output, Progress, State, ToolGroup};
use std::{sync::Arc, time::Duration};

/// Opaque single-use ticket, scoped to one card instance and snapshot revision.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ticket {
    owner: Owner,
    serial: u64,
}
#[derive(Clone, Debug)]
struct Owner(Arc<()>);
impl PartialEq for Owner {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
impl Eq for Owner {}

/// Deliberate command from a host's selected-card focus scope.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    /// Ask the host to open the artifact at this supplied index.
    OpenArtifact(usize),
    /// Local bounded preview expansion, without a host effect.
    Toggle,
    /// Ask the host to open full output.
    Details,
    /// Ask the host for a new successor, retaining this historical call.
    Retry,
    /// Copy semantic call and all retained source.
    CopyCall,
    /// Copy all retained source only.
    CopyOutput,
}
/// Identified effect. Hosts own execution and acknowledge actual results.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Intent {
    /// Single-use instance ticket.
    pub ticket: Ticket,
    /// Host call ID.
    pub call: String,
    /// Snapshot revision requested.
    pub revision: u64,
    /// Requested effect.
    pub effect: Effect,
}
/// Effects never run from rendering or ordinary Composer typing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Effect {
    /// Open a host-owned child task.
    OpenTask { task: String },
    /// Open a supplied artifact through the host.
    OpenArtifact { artifact: String, target: String },
    /// Open the host's full source; retained output is a useful local fallback.
    Details,
    /// Create a new identified call below this one.
    Retry,
    /// Clean semantic text, without display numbers/chrome.
    Copy { text: String, output_only: bool },
}
/// Local preview policy. Automatic failure preview is three initial error lines.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Expansion {
    /// Three initial error lines, open waiting, otherwise a three-line text tail.
    #[default]
    Automatic,
    /// Hide failed/waiting and code/diff bodies; text keeps its bounded tail.
    Collapsed,
    /// First ten and last ten source lines, with precise omitted-line counts.
    Expanded,
}

/// Reusable single-card state; the caller owns placement, input dispatch and clock.
pub struct ToolCallCard {
    call: Call,
    displayed: Output,
    expansion: Expansion,
    owner: Owner,
    serial: u64,
    pending: Option<Intent>,
    elapsed: Duration,
    painted: Duration,
    dirty: bool,
    notice: String,
    reduced_motion: bool,
}
impl ToolCallCard {
    /// Validate and sanitize a host snapshot. Does not acquire terminal resources.
    ///
    /// # Errors
    /// Returns a readable error for invalid identity or oversized metadata.
    pub fn new(mut call: Call) -> Result<Self, &'static str> {
        call.validate()?;
        call.sanitize();
        Ok(Self {
            displayed: call.output.clone(),
            call,
            expansion: Expansion::Automatic,
            owner: Owner(Arc::new(())),
            serial: 0,
            pending: None,
            elapsed: Duration::ZERO,
            painted: Duration::ZERO,
            dirty: false,
            notice: String::new(),
            reduced_motion: false,
        })
    }
    /// Current authoritative retained snapshot, including not-yet-painted output.
    #[must_use]
    pub fn call(&self) -> &Call {
        &self.call
    }
    /// Current local preview policy.
    #[must_use]
    pub fn expansion(&self) -> Expansion {
        self.expansion
    }
    /// Last acknowledged effect result; empty before acknowledgement.
    #[must_use]
    pub fn notice(&self) -> &str {
        &self.notice
    }
    /// Replace a newer snapshot of the same call, preserving local expansion.
    /// Invalid transitions, duplicate or foreign revisions are rejected atomically.
    pub fn replace(&mut self, mut call: Call) -> bool {
        if call.validate().is_err()
            || call.id != self.call.id
            || call.revision <= self.call.revision
            || !self.call.state.allows(&call.state)
            || (self.call.retried.is_some() && call.retried != self.call.retried)
        {
            return false;
        }
        call.sanitize();
        self.pending = None;
        self.notice.clear();
        let flush = call.state.terminal() || self.displayed.text().is_empty();
        self.call = call;
        self.dirty = true;
        if flush || self.elapsed.saturating_sub(self.painted) >= Duration::from_millis(100) {
            self.flush();
        }
        true
    }
    fn flush(&mut self) {
        self.displayed = self.call.output.clone();
        self.painted = self.elapsed;
        self.dirty = false;
    }
    pub(crate) fn pending_retry(&self, ticket: &Ticket) -> bool {
        self.pending
            .as_ref()
            .is_some_and(|i| &i.ticket == ticket && i.effect == Effect::Retry)
    }
    /// Advance the injected clock; returns whether the visible preview changed.
    pub fn tick(&mut self, elapsed: Duration) -> bool {
        let previous = self.elapsed.as_millis() / 140;
        self.elapsed = self.elapsed.saturating_add(elapsed);
        if self.dirty && self.elapsed.saturating_sub(self.painted) >= Duration::from_millis(100) {
            self.flush();
            true
        } else {
            !self.reduced_motion
                && previous != self.elapsed.as_millis() / 140
                && matches!(self.call.state, State::Running | State::Cancelling)
        }
    }
    /// Keep working dots static; duration remains supplied by the host.
    pub fn set_reduced_motion(&mut self, reduced: bool) {
        self.reduced_motion = reduced;
    }
    /// Whether a command currently has an observable effect.
    #[must_use]
    pub fn available(&self, command: Command) -> bool {
        if command == Command::Toggle {
            return (!self.call.output.text().is_empty() || self.call.child_task.is_some())
                && self.call.retried.is_none();
        }
        if self.pending.is_some() {
            return false;
        }
        match command {
            Command::OpenArtifact(index) => self
                .call
                .artifacts
                .get(index)
                .is_some_and(|a| a.open_available),
            Command::Details => self.call.capabilities.details,
            Command::Retry => {
                self.call.capabilities.retry
                    && self.call.retried.is_none()
                    && matches!(
                        self.call.state,
                        State::Failed | State::Cancelled(_) | State::Interrupted(_)
                    )
            }
            Command::CopyCall => self.call.capabilities.copy,
            Command::CopyOutput => {
                self.call.capabilities.copy && !self.call.output.text().is_empty()
            }
            Command::Toggle => unreachable!("handled above"),
        }
    }
    /// Consume a deliberate selected-scope command. Local expansion emits no effect.
    pub fn command(&mut self, command: Command) -> Option<Intent> {
        if !self.available(command) {
            return None;
        }
        if command == Command::Toggle {
            self.expansion = if self.expansion == Expansion::Expanded {
                Expansion::Collapsed
            } else {
                Expansion::Expanded
            };
            return None;
        }
        let effect = match command {
            Command::Details => self
                .call
                .child_task
                .as_ref()
                .map_or(Effect::Details, |c| Effect::OpenTask { task: c.id.clone() }),
            Command::OpenArtifact(index) => {
                let artifact = &self.call.artifacts[index];
                Effect::OpenArtifact {
                    artifact: artifact.id.clone(),
                    target: artifact.target.clone(),
                }
            }
            Command::Retry => Effect::Retry,
            Command::CopyCall | Command::CopyOutput => Effect::Copy {
                text: if command == Command::CopyOutput {
                    self.call.output.text().into()
                } else {
                    self.call.copy_text()
                },
                output_only: command == Command::CopyOutput,
            },
            Command::Toggle => unreachable!("handled above"),
        };
        self.serial = self.serial.saturating_add(1);
        let intent = Intent {
            ticket: Ticket {
                owner: self.owner.clone(),
                serial: self.serial,
            },
            call: self.call.id.clone(),
            revision: self.call.revision,
            effect,
        };
        self.pending = Some(intent.clone());
        Some(intent)
    }
    /// Single-use acknowledgement; revision changes invalidate old tickets.
    pub fn acknowledge(&mut self, ticket: &Ticket, result: Result<&str, &str>) -> bool {
        if self.pending.as_ref().is_none_or(|i| &i.ticket != ticket) {
            return false;
        }
        let Some(intent) = self.pending.take() else {
            return false;
        };
        self.notice = match result {
            Ok("") => match intent.effect {
                Effect::Copy { output_only, text } => format!(
                    "✓ copied {} · {} lines",
                    if output_only { "output" } else { "call" },
                    text.lines().count()
                ),
                Effect::Details => "→ opened details".into(),
                Effect::OpenTask { .. } => "→ opened task".into(),
                Effect::OpenArtifact { .. } => "→ opened artifact".into(),
                Effect::Retry => "retry accepted".into(),
            },
            Ok(label) => model::label(label),
            Err(reason) => format!("failed · {}", model::label(reason)),
        };
        true
    }
    /// Actions for a selected card, gated by capabilities and pending effects.
    #[must_use]
    pub fn hint(&self) -> String {
        let mut hints = Vec::new();
        if self
            .call
            .artifacts
            .iter()
            .enumerate()
            .any(|(i, _)| self.available(Command::OpenArtifact(i)))
        {
            hints.push("a artifacts");
        }
        for (command, label) in [
            (Command::Retry, "r retry"),
            (
                Command::Details,
                if self.call.child_task.is_some() {
                    "o open task"
                } else {
                    "o details"
                },
            ),
            (Command::CopyCall, "y copy call"),
            (Command::CopyOutput, "c copy output"),
        ] {
            if self.available(command) {
                hints.push(label);
            }
        }
        if self.available(Command::Toggle) {
            hints.push(if self.expansion == Expansion::Expanded {
                "enter collapse"
            } else {
                "enter expand"
            });
        }
        hints.push("esc done");
        hints.join(" · ")
    }
    /// Compact hints keep expansion/escape and enabled effect keys readable at 40 cells.
    #[must_use]
    pub fn hint_for(&self, width: u16) -> String {
        if width >= 50 {
            return self.hint();
        }
        let mut hints = Vec::new();
        if self
            .call
            .artifacts
            .iter()
            .enumerate()
            .any(|(i, _)| self.available(Command::OpenArtifact(i)))
        {
            hints.push("a");
        }
        if self.available(Command::Retry) {
            hints.push("r retry");
        }
        if self.available(Command::Toggle) {
            hints.push(if self.expansion == Expansion::Expanded {
                "enter fold"
            } else {
                "enter expand"
            });
        }
        if self.available(Command::Details) {
            hints.push("o");
        }
        if self.available(Command::CopyOutput) {
            hints.push("y/c");
        } else if self.available(Command::CopyCall) {
            hints.push("y");
        }
        hints.push("esc");
        hints.join(" · ")
    }
}

/// Concurrent-card composition, preserving host start order and member view state.
pub struct ToolGroupCard {
    group: ToolGroup,
    cards: Vec<ToolCallCard>,
    expanded: bool,
    slots: Vec<String>,
}
impl ToolGroupCard {
    /// Build a validated group without execution or terminal resources.
    ///
    /// # Errors
    /// Rejects invalid/duplicate call IDs and more than 128 calls.
    pub fn new(mut group: ToolGroup) -> Result<Self, &'static str> {
        group.validate()?;
        for call in &mut group.calls {
            call.sanitize();
        }
        let cards = group
            .calls
            .iter()
            .cloned()
            .map(ToolCallCard::new)
            .collect::<Result<_, _>>()?;
        let slots = group.calls.iter().map(|c| c.id.clone()).collect();
        Ok(Self {
            group,
            cards,
            expanded: false,
            slots,
        })
    }
    /// Inspect a member by host call ID.
    #[must_use]
    pub fn card(&self, id: &str) -> Option<&ToolCallCard> {
        self.cards.iter().find(|card| card.call.id == id)
    }
    pub(crate) fn index(&self, id: &str) -> Option<usize> {
        self.cards.iter().position(|c| c.call.id == id)
    }
    pub(crate) fn slot(&self, id: &str) -> usize {
        self.slots.iter().position(|slot| slot == id).unwrap_or(0) + 1
    }
    /// Dispatch a command/acknowledgement to one independently owned member.
    pub fn card_mut(&mut self, id: &str) -> Option<&mut ToolCallCard> {
        self.cards.iter_mut().find(|card| card.call.id == id)
    }
    /// Toggle the group, revealing all members in start order.
    pub fn toggle(&mut self) {
        self.expanded = !self.expanded;
    }
    /// Expanded groups show every member; automatic groups retain failures.
    #[must_use]
    pub fn expanded(&self) -> bool {
        self.expanded
    }
    /// Validate a newer group snapshot atomically and preserve surviving view state.
    pub fn replace(&mut self, mut group: ToolGroup) -> bool {
        if group.validate().is_err()
            || group.id != self.group.id
            || group.revision <= self.group.revision
        {
            return false;
        }
        for call in &mut group.calls {
            call.sanitize();
        }
        // Host order is authoritative; validate surviving identities before moving views.
        if self.cards.iter().any(|card| {
            let old = card.call();
            let Some(new) = group.calls.iter().find(|new| new.id == old.id) else {
                return true;
            };
            new.revision < old.revision
                || (new.revision == old.revision && old != new)
                || !old.state.allows(&new.state)
                || (old.retried.is_some() && old.retried != new.retried)
        }) {
            return false;
        }
        let mut cards = std::mem::take(&mut self.cards);
        for call in &group.calls {
            if let Some(index) = cards.iter().position(|card| card.call.id == call.id) {
                let mut card = cards.remove(index);
                if call.revision > card.call.revision {
                    card.replace(call.clone());
                }
                self.cards.push(card);
            } else if let Ok(card) = ToolCallCard::new(call.clone()) {
                self.slots.push(call.id.clone());
                self.cards.push(card);
            }
        }
        self.group = group;
        true
    }
    /// Advance all cards with the same injected clock.
    pub fn tick(&mut self, elapsed: Duration) -> bool {
        let mut changed = false;
        for card in &mut self.cards {
            changed |= card.tick(elapsed);
        }
        changed
    }
}
