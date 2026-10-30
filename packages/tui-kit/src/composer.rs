//! Owns versioned drafts and submission acknowledgements; hosts execute and persist.

mod codec;
mod completion;
mod editor_host;
mod handoff;
mod history;
mod lifecycle;
mod lists;
mod modes;
mod paste_burst;
mod payload;
pub use paste_burst::PasteBurst;
mod saved;
mod work;
pub use completion::{CompletionItem, CompletionKind, CompletionRequest};
pub use editor_host::{editor_command, run_editor};
pub use handoff::{EditorRequest, EditorResult};
pub use history::HistoryEntry;
pub use lifecycle::{InputMode, Intent, QueueSummary};
pub use modes::{BoxMode, DraftCheckpoint};
pub use saved::{SavedComposer, SavedDraft};
pub use work::{WorkAction, WorkRequest};

use crate::{
    editor::{Editor, EditorView, Outcome},
    event::{Event, KeyCode, KeyEventKind},
    text::clean,
};
use ratatui::{Frame, layout::Rect};
use std::{sync::Arc, time::Duration};

/// Opaque instance or operation identity. Clones retain identity; new values differ.
#[derive(Clone, Debug)]
pub struct Identity(Arc<()>);

impl Identity {
    fn new() -> Self {
        Self(Arc::new(()))
    }
}
impl PartialEq for Identity {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
impl Eq for Identity {}

/// A draft lifetime and monotonic text revision, independent of cursor movement.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Version {
    /// Instance-local lifetime; reset invalidates earlier acknowledgements.
    pub identity: Identity,
    /// Advances on editing, undo/redo and explicit host replacement.
    pub revision: u64,
}

/// Immutable-by-convention snapshot returned once per submission attempt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Submission {
    /// Attempt identity; retries of the same revision have distinct tickets.
    pub ticket: Identity,
    /// Submitted draft version.
    pub version: Version,
    /// Exact bounded editor text, including logical newlines.
    pub text: String,
    /// Message or shell command, selected explicitly before sending.
    pub mode: InputMode,
    /// Lossless recall payload, including opaque attachment references.
    pub document: crate::editor::Document,
}

/// Lifecycle display state. Acceptance means host acknowledgement, not execution.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum State {
    /// No outstanding submission.
    Ready,
    /// One snapshot awaiting its host response; editing remains enabled.
    Pending(Submission),
    /// Host accepted the fixed snapshot; the fresh draft is independent.
    Accepted,
    /// Host rejected a snapshot, retained for recovery even if the draft changed.
    Rejected {
        /// Original submitted text and identity.
        submission: Submission,
        /// Bounded, sanitized host explanation.
        reason: String,
    },
}

/// One editor plus its draft lifetime, pending snapshot and host availability.
pub struct Composer {
    editor: Editor,
    version: Version,
    state: State,
    blocked: Option<String>,
    submitted_box: Option<Version>,
    mode: BoxMode,
    set_aside: Option<lifecycle::StoredDraft>,
    stash: Vec<lifecycle::StoredDraft>,
    limit: usize,
    history: Vec<String>,
    history_browsing: bool,
    input_mode: InputMode,
    repository_label: String,
    shell_history: Vec<String>,
    history_documents: std::collections::BTreeMap<String, crate::editor::Document>,
    history_origins: std::collections::BTreeMap<String, String>,
    completion_items: Vec<CompletionItem>,
    completion: Option<completion::Completion>,
    inserted_hint: Option<(String, String)>,
    inserted_command: Option<(String, String)>,
    literal_command: Option<Version>,
    rewind: Option<Version>,
    queue: Vec<lifecycle::QueuedDraft>,
    editing_queue: Option<Identity>,
    working: bool,
    working_elapsed: Duration,
    motion: work::Motion,
    work_request: Option<WorkRequest>,
    background: Vec<Identity>,
    paused: bool,
    resume_waiting: Option<Identity>,
    intents: Vec<Intent>,
    leader: Option<Duration>,
    now: Duration,
    arm: Option<(lifecycle::Arm, Duration)>,
    returned: Option<Version>,
    list: Option<lists::List>,
    external: Option<EditorRequest>,
    cancelled_editor: Option<Identity>,
    conflict: Option<handoff::Conflict>,
    save_due: Option<Duration>,
    save_ticket: Option<(Identity, Version)>,
}

impl Default for Composer {
    fn default() -> Self {
        Self::new(65_536)
    }
}

impl Composer {
    /// Create an independent empty draft with an explicit editor byte limit.
    #[must_use]
    pub fn new(limit: usize) -> Self {
        Self {
            editor: Editor::new(limit),
            version: Version {
                identity: Identity::new(),
                revision: 0,
            },
            state: State::Ready,
            blocked: None,
            submitted_box: None,
            mode: BoxMode::Draft,
            set_aside: None,
            stash: Vec::new(),
            limit,
            history: Vec::new(),
            history_browsing: false,
            input_mode: InputMode::Message,
            repository_label: "repository".into(),
            shell_history: Vec::new(),
            history_documents: std::collections::BTreeMap::new(),
            history_origins: std::collections::BTreeMap::new(),
            completion_items: Vec::new(),
            completion: None,
            inserted_hint: None,
            inserted_command: None,
            literal_command: None,
            rewind: None,
            queue: Vec::new(),
            editing_queue: None,
            working: false,
            working_elapsed: Duration::ZERO,
            motion: work::Motion::Animated,
            work_request: None,
            background: Vec::new(),
            paused: false,
            resume_waiting: None,
            intents: Vec::new(),
            leader: None,
            now: Duration::ZERO,
            arm: None,
            returned: None,
            list: None,
            external: None,
            cancelled_editor: None,
            conflict: None,
            save_due: None,
            save_ticket: None,
        }
    }

    /// Read-only editor state; all mutations pass through the revision owner.
    #[must_use]
    pub fn editor(&self) -> &Editor {
        &self.editor
    }
    /// Current version for host operations.
    #[must_use]
    pub fn version(&self) -> &Version {
        &self.version
    }
    /// Current acknowledgement/recovery state.
    #[must_use]
    pub fn state(&self) -> &State {
        &self.state
    }

    fn notify(&mut self, text: &str) {
        let message = if self.editor.notice() == Some("Stash limit · oldest entry dropped") {
            format!("{text} · oldest entry dropped")
        } else {
            text.into()
        };
        self.editor.notify(&message);
    }
    fn advance(&mut self) {
        self.literal_command = None;
        self.inserted_hint = None;
        if self.inserted_command.as_ref().is_some_and(|(_, name)| {
            self.editor
                .text()
                .strip_prefix('/')
                .and_then(|s| s.split_whitespace().next())
                != Some(name.as_str())
        }) {
            self.inserted_command = None;
        }
        self.save_due = Some(self.now + Duration::from_millis(500));
        self.returned = None;
        if let Some(next) = self.version.revision.checked_add(1) {
            self.version.revision = next;
        } else {
            self.version = Version {
                identity: Identity::new(),
                revision: 0,
            };
        }
    }

    /// Explicit host replacement within this lifetime; retains a pending snapshot.
    /// Returns whether the editor truncated the supplied text.
    pub fn set_text(&mut self, text: &str) -> bool {
        self.restore_draft();
        self.completion = None;
        self.inserted_command = None;
        let cut = self.editor.set_text(text);
        self.advance();
        cut
    }

    /// Start a different draft lifetime and invalidate outstanding/rejected results.
    /// Returns whether the initial text was truncated. Availability is retained.
    pub fn reset(&mut self, text: &str) -> bool {
        self.restore_draft();
        self.mode = BoxMode::Draft;
        self.version = Version {
            identity: Identity::new(),
            revision: 0,
        };
        self.state = State::Ready;
        self.submitted_box = None;
        self.external = None;
        self.cancelled_editor = None;
        self.conflict = None;
        self.list = None;
        self.completion = None;
        self.inserted_hint = None;
        self.inserted_command = None;
        self.save_ticket = None;
        self.arm = None;
        self.leader = None;
        self.rewind = None;
        self.work_request = None;
        self.intents
            .retain(|intent| !matches!(intent, Intent::Work(_)));
        self.background.clear();
        self.resume_waiting = None;
        self.editor.set_text(text)
    }

    /// Host availability. A reason prevents submission while preserving editing.
    pub fn set_blocked(&mut self, reason: Option<&str>) {
        self.blocked = reason.map(|text| clean(text, 256, false));
    }

    /// Consume editor input, or return one snapshot for a plain Enter press.
    /// Modified Enter/paste never submit. Further Enter while pending returns none.
    pub fn update(&mut self, event: &Event) -> Option<Submission> {
        if let lifecycle::Route::Consumed(result) = self.lifecycle_input(event) {
            return result;
        }
        if self.payload_input(event) {
            return None;
        }
        if self.mode_input(event) {
            return None;
        }
        if matches!(event, Event::Key(key) if key.code == KeyCode::Enter
            && key.modifiers.is_empty() && key.kind == KeyEventKind::Press)
        {
            return self.submit();
        }
        let cursor = self.editor.cursor();
        if self.editor.update(event) == Outcome::Changed {
            self.adopt_recall();
            self.advance();
        } else if cursor != self.editor.cursor() {
            self.save_due = Some(self.now + Duration::from_millis(500));
        }
        None
    }

    /// Fix the submitted text and clear the editor immediately; one operation may be pending.
    pub fn submit(&mut self) -> Option<Submission> {
        self.submit_with_commands(true)
    }
    fn submit_with_commands(&mut self, commands: bool) -> Option<Submission> {
        if matches!(self.mode, BoxMode::Locked { .. }) {
            return None;
        }
        if self.clean_before_send() {
            return None;
        }
        if matches!(self.mode, BoxMode::Queued { .. }) && self.editing_queue.is_some() {
            self.save_queued();
            return None;
        }
        if self.prepare_enter() {
            return None;
        }
        if commands && self.invoke_command() {
            return None;
        }
        if self.history_browsing && matches!(self.mode, BoxMode::History { .. }) {
            if self.blocked.is_some() || self.editor.text().trim().is_empty() {
                return None;
            }
            self.adopt_recall();
        }
        if self.is_busy()
            && matches!(self.mode, BoxMode::Draft)
            && self.blocked.is_none()
            && !self.editor.text().trim().is_empty()
        {
            self.enqueue();
            return None;
        }
        if !matches!(self.mode, BoxMode::Draft)
            || matches!(self.state, State::Pending(_))
            || self.blocked.is_some()
            || self.editor.text().trim().is_empty()
        {
            return None;
        }
        let submission = Submission {
            ticket: Identity::new(),
            version: self.version.clone(),
            text: self.editor.expanded_text(),
            mode: self.input_mode,
            document: self.editor.document(),
        };
        if self.rewind.take().is_some() {
            self.intents.push(Intent::Rewind);
        }
        self.editor.set_text("");
        self.advance();
        self.submitted_box = Some(self.version.clone());
        self.state = State::Pending(submission.clone());
        Some(submission)
    }

    /// Resolve the exact pending ticket. `Ok(())` accepts; `Err(reason)` rejects.
    /// False means stale, duplicate or foreign response; it changes no state.
    pub fn acknowledge(&mut self, ticket: &Identity, result: Result<(), &str>) -> bool {
        let State::Pending(submission) = &self.state else {
            return false;
        };
        if &submission.ticket != ticket {
            return false;
        }
        if result.is_ok() {
            let text = submission.text.clone();
            let mode = submission.mode;
            let document = submission.document.clone();
            self.history_documents.insert(text.clone(), document);
            self.remember_in_mode(text, mode);
        }
        let State::Pending(submission) = &self.state else {
            return false;
        };
        self.state = match result {
            Ok(()) => State::Accepted,
            Err(reason) => {
                let submission = submission.clone();
                if matches!(self.mode, BoxMode::Draft | BoxMode::Locked { .. })
                    && self.editor.text().is_empty()
                    && self.submitted_box.as_ref() == Some(&self.version)
                {
                    self.editor.restore_document(&submission.document);
                    self.input_mode = submission.mode;
                    self.advance();
                    self.returned = Some(self.version.clone());
                } else {
                    let mut editor = Editor::new(self.limit);
                    editor.restore_document(&submission.document);
                    self.push_stash(lifecycle::StoredDraft {
                        editor,
                        mode: submission.mode,
                        at: self.now,
                        age_seconds: 0,
                    });
                    self.editor
                        .notify("Not sent text stashed · your newer draft is kept");
                }
                self.paused = true;
                State::Rejected {
                    submission,
                    reason: clean(reason, 256, false),
                }
            }
        };
        self.submitted_box = None;
        if self.resume_waiting.take().is_some() && result.is_ok() && !self.queue.is_empty() {
            self.request_work(WorkAction::InterruptAfterStep);
        }
        true
    }

    /// Lifecycle label suitable for a host's persistent activity row.
    #[must_use]
    pub fn status(&self) -> String {
        if let Some(request) = &self.work_request {
            return match request.action {
                WorkAction::Stop => "cancelling… · draft kept".into(),
                WorkAction::InterruptAfterStep => "waiting for current step · queue held".into(),
                WorkAction::Background => "moving to background…".into(),
            };
        }
        if self.resume_waiting.is_some() {
            return "resuming · waiting for sending acknowledgement".into();
        }
        if self.leader.is_some() {
            return "ctrl+x · s stash · u restore · e editor · b background · enter sends queued · esc cancels · 3s".into();
        }
        if !matches!(self.state, State::Pending(_))
            && let Some(reason) = &self.blocked
        {
            return reason.clone();
        }
        if let Some(notice) = self.editor.notice() {
            return notice.into();
        }
        if matches!(self.mode, BoxMode::Locked { .. }) {
            return "Editing in external editor · ctrl+c cancels waiting".into();
        }
        if self.working {
            let dots = if self.motion == work::Motion::Reduced {
                "···"
            } else {
                ["·  ", "·· ", "···"][(self.now.as_millis() / 140 % 3) as usize]
            };
            let prefix = format!(
                "{dots} working · {}s · esc stops",
                self.working_elapsed.as_secs()
            );
            return if self.queue.is_empty() {
                prefix
            } else {
                format!("{prefix} · {} queued · up edits", self.queue.len())
            };
        }
        if !self.queue.is_empty() {
            return if self.paused {
                "Queue paused · ctrl+x enter sends next".into()
            } else {
                format!("{} queued · sent one per turn · up edits", self.queue.len())
            };
        }
        match &self.state {
            State::Rejected { reason, .. } => format!("Not sent · {reason} · draft kept"),
            State::Pending(_) | State::Accepted | State::Ready => self
                .inserted_hint
                .as_ref()
                .map_or_else(String::new, |(hint, _)| hint.clone()),
        }
    }

    /// Compact activity text preserves the local exit/cancel key at narrow widths.
    #[must_use]
    pub fn status_for(&self, width: u16) -> String {
        if width >= 50 {
            return self.status();
        }
        if let Some(request) = &self.work_request {
            return match request.action {
                WorkAction::Stop => "cancelling… · esc waits".into(),
                WorkAction::InterruptAfterStep => "waiting for step · esc waits".into(),
                WorkAction::Background => "backgrounding · esc waits".into(),
            };
        }
        if self.leader.is_some() {
            return "ctrl+x · esc cancels".into();
        }
        if self.working && self.editor.notice().is_none() {
            return if self.queue.is_empty() {
                "working · esc stops".into()
            } else {
                format!("working · {} queued · esc stops", self.queue.len())
            };
        }
        self.status()
    }

    /// Render only the editor; the host owns layout and the activity row.
    pub fn render_with(
        &mut self,
        frame: &mut Frame<'_>,
        area: Rect,
        view: EditorView<'_>,
        focused: bool,
    ) {
        self.render_mode(frame, area, view, focused);
        if focused && let Some((_, placeholder)) = &self.inserted_hint {
            self.editor.render_ghost(frame, area, view, placeholder);
        }
    }
    /// Advance editor notices and undo grouping using the host clock.
    pub fn tick(&mut self, elapsed: Duration) {
        self.now = self.now.saturating_add(elapsed);
        self.editor.tick(elapsed);
        if let Some(completion) = &mut self.completion {
            completion.menu.tick(elapsed);
        }
        self.lifecycle_tick(elapsed);
    }
}
