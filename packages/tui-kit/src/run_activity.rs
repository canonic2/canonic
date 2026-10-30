//! Whole-run activity and identified cancellation without transport or terminal ownership.

use crate::theme::{Presentation, Symbols};
use ratatui::{Frame, layout::Rect, widgets::Paragraph};
use std::{sync::Arc, time::Duration};

/// Authoritative run lifecycle; connectivity is independent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum State {
    /// No run is active.
    Idle,
    /// Host accepted a message and is preparing output.
    Preparing,
    /// Host is executing the run.
    Running,
    /// Host awaits supplied questions or another named input.
    Waiting { questions: usize, subject: String },
    /// Host-confirmed completion.
    Completed,
    /// Host-confirmed cancellation.
    Cancelled,
    /// Host reports termination without a result.
    Interrupted(String),
    /// Host-confirmed run failure, independent of tool/connection failures.
    Failed(String),
}
impl State {
    /// Whether this run has a final outcome.
    #[must_use]
    pub fn terminal(&self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Cancelled | Self::Interrupted(_) | Self::Failed(_)
        )
    }
    fn active(&self) -> bool {
        matches!(self, Self::Preparing | Self::Running | Self::Waiting { .. })
    }
}

/// Safe transport-independent host snapshot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    /// Stable nonempty identity, at most 128 bytes without controls.
    pub id: String,
    /// Strictly increasing revision for replacements.
    pub revision: u64,
    /// Host run state.
    pub state: State,
    /// Host elapsed duration, not inferred from repaint time.
    pub elapsed: Duration,
    /// Whether a deliberate Escape may request cancellation.
    pub cancel_available: bool,
    /// Whether Ctrl+G may ask the host to open allowed options.
    pub options_available: bool,
}
impl Snapshot {
    /// Create an idle run snapshot with no enabled host effects.
    #[must_use]
    pub fn new(id: &str) -> Self {
        Self {
            id: id.into(),
            revision: 0,
            state: State::Idle,
            elapsed: Duration::ZERO,
            cancel_available: false,
            options_available: false,
        }
    }
    fn valid(&self) -> bool {
        !self.id.is_empty()
            && self.id.len() <= 128
            && !self.id.chars().any(char::is_control)
            && match &self.state {
                State::Waiting { subject, .. }
                | State::Interrupted(subject)
                | State::Failed(subject) => subject.len() <= 4096,
                _ => true,
            }
    }
    fn sanitize(&mut self) {
        match &mut self.state {
            State::Waiting { subject, .. }
            | State::Interrupted(subject)
            | State::Failed(subject) => {
                *subject = crate::text::clean(subject, 4096, false);
            }
            _ => {}
        }
    }
}

/// Independent connection indication; no run outcome is inferred from it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Connection {
    /// The host transport is online.
    #[default]
    Online,
    /// Retry timing is supplied by the host and may be unknown.
    Offline { retry_in: Option<Duration> },
}

/// Opaque instance-scoped single-use acknowledgement ticket.
#[derive(Clone, Debug)]
pub struct Ticket {
    owner: Arc<()>,
    serial: u64,
}
impl PartialEq for Ticket {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.owner, &other.owner) && self.serial == other.serial
    }
}
impl Eq for Ticket {}

/// Explicit host effect, never executed by the component.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effect {
    /// Request cancellation; this is not a confirmed outcome.
    Cancel,
    /// Open only the options supplied by the host.
    Options,
}
/// Identified operation consumed and acknowledged by the host.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Intent {
    /// Instance-scoped ticket.
    pub ticket: Ticket,
    /// Stable run identity.
    pub run: String,
    /// Revision at the time of the request.
    pub revision: u64,
    /// Requested operation.
    pub effect: Effect,
}

/// Reusable one-row activity with injected animation and explicit host effects.
pub struct RunActivity {
    snapshot: Snapshot,
    connection: Connection,
    owner: Arc<()>,
    serial: u64,
    cancel: Option<Intent>,
    options: Option<Intent>,
    already_asked: bool,
    failure: Option<String>,
    clock: Duration,
    requested_at: Duration,
    reduced_motion: bool,
}
impl RunActivity {
    /// Validate a host snapshot without acquiring external resources.
    ///
    /// # Errors
    /// Rejects invalid identities or metadata exceeding 4096 bytes.
    pub fn new(mut snapshot: Snapshot) -> Result<Self, &'static str> {
        if !snapshot.valid() {
            return Err("invalid run snapshot");
        }
        snapshot.sanitize();
        Ok(Self {
            snapshot,
            connection: Connection::Online,
            owner: Arc::new(()),
            serial: 0,
            cancel: None,
            options: None,
            already_asked: false,
            failure: None,
            clock: Duration::ZERO,
            requested_at: Duration::ZERO,
            reduced_motion: false,
        })
    }
    /// Last authoritative run snapshot.
    #[must_use]
    pub fn snapshot(&self) -> &Snapshot {
        &self.snapshot
    }
    /// Independent transport state.
    #[must_use]
    pub fn connection(&self) -> Connection {
        self.connection
    }
    /// Set transport status without changing the run outcome or elapsed time.
    pub fn set_connection(&mut self, connection: Connection) {
        self.connection = connection;
    }
    /// Keep animation static while host durations continue to update.
    pub fn set_reduced_motion(&mut self, reduced: bool) {
        self.reduced_motion = reduced;
    }
    /// Advance repaint time, never run elapsed time or acknowledgement deadlines.
    pub fn tick(&mut self, elapsed: Duration) {
        self.clock = self.clock.saturating_add(elapsed);
    }
    /// Replace a newer snapshot atomically; final outcomes cannot resurrect.
    pub fn replace(&mut self, mut snapshot: Snapshot) -> bool {
        if !snapshot.valid()
            || snapshot.id != self.snapshot.id
            || snapshot.revision <= self.snapshot.revision
            || (self.snapshot.state.terminal() && snapshot.state != self.snapshot.state)
        {
            return false;
        }
        snapshot.sanitize();
        if snapshot.state.terminal() {
            self.cancel = None;
            self.options = None;
            self.failure = None;
            self.already_asked = false;
        }
        self.snapshot = snapshot;
        true
    }
    /// Begin a distinct host run, clearing every prior pending effect.
    pub fn begin(&mut self, snapshot: Snapshot) -> bool {
        if snapshot.id == self.snapshot.id {
            return false;
        }
        let Ok(mut next) = Self::new(snapshot) else {
            return false;
        };
        next.connection = self.connection;
        next.reduced_motion = self.reduced_motion;
        *self = next;
        true
    }
    fn intent(&mut self, effect: Effect) -> Intent {
        self.serial = self.serial.saturating_add(1);
        Intent {
            ticket: Ticket {
                owner: Arc::clone(&self.owner),
                serial: self.serial,
            },
            run: self.snapshot.id.clone(),
            revision: self.snapshot.revision,
            effect,
        }
    }
    /// Request cancellation once; repeated Escape changes only the local label.
    pub fn cancel(&mut self) -> Option<Intent> {
        if self.cancel.is_some() {
            self.already_asked = true;
            return None;
        }
        if !self.snapshot.state.active()
            || !self.snapshot.cancel_available
            || self.connection != Connection::Online
        {
            return None;
        }
        let intent = self.intent(Effect::Cancel);
        self.cancel = Some(intent.clone());
        self.already_asked = false;
        self.failure = None;
        self.requested_at = self.clock;
        Some(intent)
    }
    /// Ask the host to open supplied options without taking a run action.
    pub fn options(&mut self) -> Option<Intent> {
        if !self.snapshot.options_available || self.options.is_some() {
            return None;
        }
        let intent = self.intent(Effect::Options);
        self.options = Some(intent.clone());
        Some(intent)
    }
    /// Acknowledge a cancel failure. Success must arrive as an authoritative snapshot.
    pub fn cancel_failed(&mut self, ticket: &Ticket, reason: &str) -> bool {
        if self.cancel.as_ref().is_none_or(|i| &i.ticket != ticket) {
            return false;
        }
        self.cancel = None;
        self.already_asked = false;
        self.failure = Some(crate::text::clean(reason, 4096, false));
        true
    }
    /// Consume the host's acknowledgement that its options view opened or failed.
    pub fn acknowledge_options(&mut self, ticket: &Ticket) -> bool {
        if self.options.as_ref().is_none_or(|i| &i.ticket != ticket) {
            return false;
        }
        self.options = None;
        true
    }
    /// Whether cancellation awaits an actual host outcome.
    #[must_use]
    pub fn cancelling(&self) -> bool {
        self.cancel.is_some()
    }
    /// Generated connection marker for the host's right status slot.
    #[must_use]
    pub fn connection_label(&self, symbols: Symbols) -> String {
        match self.connection {
            Connection::Online => String::new(),
            Connection::Offline { retry_in } => {
                let glyph = if symbols == Symbols::Ascii {
                    "o"
                } else {
                    "○"
                };
                let retry =
                    retry_in.map_or_else(String::new, |d| format!(" · retry in {}s", d.as_secs()));
                alphabet(format!("{glyph} offline{retry}"), symbols)
            }
        }
    }
    /// Bounded row text; rendering clips it to the caller's area.
    #[must_use]
    pub fn label(&self, width: u16, symbols: Symbols) -> String {
        if width < 32
            && self.connection == Connection::Online
            && self.cancel.is_none()
            && self.failure.is_none()
            && matches!(self.snapshot.state, State::Preparing | State::Running)
        {
            return alphabet(
                format!(
                    "{} {}{}",
                    self.dots(),
                    if self.snapshot.state == State::Preparing {
                        "preparing"
                    } else {
                        "working"
                    },
                    if self.snapshot.cancel_available {
                        " · esc stops"
                    } else {
                        ""
                    }
                ),
                symbols,
            );
        }
        let wide = width >= 50;
        let text = if self.connection != Connection::Online && self.snapshot.state.active() {
            "! disconnected · run state unknown".into()
        } else if !wide && self.failure.is_some() {
            format!(
                "esc retry{} · ✕ cancel failed",
                if self.snapshot.options_available {
                    " · ctrl+g"
                } else {
                    ""
                }
            )
        } else if let Some(reason) = &self.failure {
            format!(
                "✕ couldn't cancel · {reason} · esc try again{}",
                if wide && self.snapshot.options_available {
                    " · ctrl+g options"
                } else {
                    ""
                }
            )
        } else if self.cancel.is_some() {
            if self.already_asked {
                format!(
                    "{} cancelling… · already asked · {}s",
                    self.dots(),
                    self.clock.saturating_sub(self.requested_at).as_secs()
                )
            } else {
                format!("{} cancelling… · waiting for host to confirm", self.dots())
            }
        } else {
            self.state_label(wide)
        };
        alphabet(text, symbols)
    }
    fn dots(&self) -> &'static str {
        if self.reduced_motion {
            return "···";
        }
        match (self.clock.as_millis() / 140) % 3 {
            0 => "·  ",
            1 => "·· ",
            _ => "···",
        }
    }
    fn state_label(&self, wide: bool) -> String {
        let elapsed = if self.snapshot.elapsed.as_secs() == 0 {
            String::new()
        } else {
            format!(" · {}s", self.snapshot.elapsed.as_secs())
        };
        match &self.snapshot.state {
            State::Idle | State::Completed => "ready".into(),
            State::Preparing | State::Running => format!(
                "{} {}{elapsed}{}",
                self.dots(),
                if self.snapshot.state == State::Preparing {
                    "preparing"
                } else {
                    "working"
                },
                if self.snapshot.cancel_available {
                    " · esc stops"
                } else {
                    ""
                }
            ),
            State::Waiting { questions, subject } => {
                let input = if *questions == 0 {
                    "input".into()
                } else {
                    format!(
                        "{questions} question{}",
                        if *questions == 1 { "" } else { "s" }
                    )
                };
                if wide {
                    format!(
                        "? {input} · {subject}{elapsed}{}",
                        if self.snapshot.options_available {
                            " · ctrl+g opens"
                        } else {
                            ""
                        }
                    )
                } else if self.snapshot.options_available {
                    format!("ctrl+g opens · ? {input}")
                } else if self.snapshot.cancel_available {
                    format!("esc stops · ? {input}")
                } else {
                    format!("? {input}")
                }
            }
            State::Cancelled => "– cancelled · work so far kept".into(),
            State::Interrupted(reason) => format!("! interrupted · {reason}"),
            State::Failed(reason) => format!("✕ run failed · {reason}"),
        }
    }
    /// Render one caller-owned row without acquiring terminal modes or input.
    pub fn render_with(&self, frame: &mut Frame<'_>, area: Rect, theme: Presentation) {
        let style = if self.connection != Connection::Online && self.snapshot.state.active() {
            theme.warning()
        } else if self.failure.is_some() || matches!(self.snapshot.state, State::Failed(_)) {
            theme.error().add_modifier(ratatui::style::Modifier::BOLD)
        } else if (self.connection != Connection::Online && self.snapshot.state.active())
            || matches!(
                self.snapshot.state,
                State::Waiting { .. } | State::Interrupted(_)
            )
        {
            theme.warning()
        } else if self.cancel.is_some()
            || matches!(self.snapshot.state, State::Preparing | State::Running)
        {
            theme.accent()
        } else {
            theme.muted()
        };
        frame.render_widget(
            Paragraph::new(self.label(area.width, theme.symbols)).style(style),
            area,
        );
    }
}
fn alphabet(text: String, symbols: Symbols) -> String {
    if symbols == Symbols::Ascii {
        text.replace('·', ".")
            .replace('✕', "x")
            .replace('–', "-")
            .replace('…', "...")
    } else {
        text
    }
}
