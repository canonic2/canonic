//! Owns host intents, queue scheduling, stash protection and escape/leader routing.

use super::{BoxMode, Composer, Identity, State, Submission};
use crate::{
    editor::Editor,
    event::{Event, KeyCode, KeyEventKind, KeyModifiers},
};
use std::time::Duration;

/// Execution interpretation is always visible and carried to the host.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InputMode {
    #[default]
    Message,
    Shell,
}

/// Host operations emitted by explicit interaction or the autosave clock.
#[derive(Clone, Debug)]
pub enum Intent {
    /// Identified stop, interrupt-after-step or background request; host acknowledgement required.
    Work(super::WorkRequest),
    /// Stop waiting for this editor return; host editor/process ownership is unchanged.
    EditorCancelled(Identity),
    /// Ask the host to read a clipboard image, then call `insert_image_with_size`.
    ClipboardImage,
    /// Quit after persisting `saved()`; never discard a draft.
    Quit,
    /// Execute a host command by stable identity.
    Command(String),
    /// A later Composer Enter invoked a command with its typed argument text.
    CommandWithArguments { id: String, arguments: String },
    /// Tab requested another attempt for failed completion sources.
    RetryCompletion(super::CompletionRequest),
    /// Launch the host's configured editor with a versioned ticket.
    Editor(super::EditorRequest),
    /// Persist a snapshot; acknowledge this exact ticket with `acknowledge_save`.
    Save {
        ticket: Identity,
        state: Box<super::SavedComposer>,
    },
    /// Rewind to the host's last user turn before sending this new draft.
    Rewind,
}

#[derive(Clone)]
pub(super) struct StoredDraft {
    pub editor: Editor,
    pub mode: InputMode,
    pub at: Duration,
    pub age_seconds: u64,
}
pub(super) struct QueuedDraft {
    pub id: Identity,
    pub draft: StoredDraft,
}

/// Queue display data in FIFO order, independent of mutable editors.
#[derive(Clone, Debug)]
pub struct QueueSummary {
    pub text: String,
    pub editing: bool,
    pub mode: InputMode,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Arm {
    Clear,
    Rewind,
    Quit(char),
}

pub(super) enum Route {
    Unhandled,
    Consumed(Option<Submission>),
}

impl Composer {
    /// Host-owned repository display name for the shell rule; performs no path lookup.
    pub fn set_repository_label(&mut self, label: &str) {
        self.repository_label = crate::text::clean(label, 128, false);
    }
    /// Read the visible input interpretation.
    #[must_use]
    pub fn input_mode(&self) -> InputMode {
        self.input_mode
    }
    /// Read host execution availability, including an unacknowledged submission.
    #[must_use]
    pub fn is_busy(&self) -> bool {
        self.working || self.work_request.is_some() || matches!(self.state, State::Pending(_))
    }
    /// Host reports whether an accepted turn is running.
    pub fn set_working(&mut self, working: bool) {
        if working && !self.working {
            self.working_elapsed = Duration::ZERO;
        }
        self.working = working;
        if !working {
            self.work_request = None;
            self.intents
                .retain(|intent| !matches!(intent, Intent::Work(_)));
        }
    }
    /// Host elapsed duration for accepted foreground work; repaint ticks do not infer it.
    pub fn set_working_elapsed(&mut self, elapsed: Duration) {
        self.working_elapsed = elapsed;
    }
    /// Keep activity dots static while host-reported elapsed time continues to change.
    pub fn set_reduced_motion(&mut self, reduced: bool) {
        self.motion = if reduced {
            super::work::Motion::Reduced
        } else {
            super::work::Motion::Animated
        };
    }
    /// A paused queue requires explicit Ctrl-X Enter to resume sending.
    #[must_use]
    pub fn queue_paused(&self) -> bool {
        self.paused
    }
    /// Read queued entries in send order.
    #[must_use]
    pub fn queue(&self) -> Vec<QueueSummary> {
        self.queue
            .iter()
            .map(|entry| QueueSummary {
                text: entry.draft.editor.document().display_text(),
                mode: entry.draft.mode,
                editing: self.editing_queue.as_ref() == Some(&entry.id),
            })
            .collect()
    }
    /// Drain host actions once. Submission remains the return value of `update`.
    pub fn take_intents(&mut self) -> Vec<Intent> {
        std::mem::take(&mut self.intents)
    }
    /// A completed turn sends one eligible queued entry, or pauses on failure.
    pub fn turn_ended(&mut self, success: bool) -> Option<Submission> {
        self.working = false;
        self.work_request = None;
        self.intents
            .retain(|intent| !matches!(intent, Intent::Work(_)));
        if !success {
            self.paused = true;
            self.advance();
            self.editor
                .notify("Queue paused · turn stopped with an error");
            return None;
        }
        if self.paused {
            return None;
        }
        self.send_next()
    }
    /// Explicitly send one queued message; hosts stop current work after its step.
    pub fn send_next_now(&mut self) -> Option<Submission> {
        if self.work_request.is_some() {
            return None;
        }
        if let State::Pending(submission) = &self.state {
            self.resume_waiting = Some(submission.ticket.clone());
            self.notify("Waiting for sending acknowledgement");
            return None;
        }
        if self.working {
            if !self.queue.is_empty() {
                self.request_work(super::WorkAction::InterruptAfterStep);
            }
            return None;
        }
        self.working = false;
        self.paused = false;
        self.send_next()
    }
    pub(super) fn send_next(&mut self) -> Option<Submission> {
        if self.blocked.is_some() || matches!(self.state, State::Pending(_)) {
            return None;
        }
        let index = self
            .queue
            .iter()
            .position(|entry| self.editing_queue.as_ref() != Some(&entry.id))?;
        let entry = self.queue.remove(index);
        let submission = Submission {
            ticket: Identity::new(),
            version: self.version.clone(),
            text: entry.draft.editor.expanded_text(),
            mode: entry.draft.mode,
            document: entry.draft.editor.document(),
        };
        self.state = State::Pending(submission.clone());
        self.submitted_box = None;
        self.refresh_queue_position();
        self.advance();
        Some(submission)
    }
    pub(super) fn enqueue(&mut self) {
        if self.queue.len() >= 128 {
            self.notify("! Queue full · 128 waiting · draft kept");
            return;
        }
        let draft = self.take_visible_draft();
        self.queue.push(QueuedDraft {
            id: Identity::new(),
            draft,
        });
        self.advance();
        self.notify("Queued · sent one per turn · up edits");
    }
    pub(super) fn take_visible_draft(&mut self) -> StoredDraft {
        self.completion = None;
        self.inserted_command = None;
        self.inserted_hint = None;
        StoredDraft {
            editor: std::mem::replace(&mut self.editor, Editor::new(self.limit)),
            mode: self.input_mode,
            at: self.now,
            age_seconds: 0,
        }
    }
    pub(super) fn install_draft(&mut self, draft: StoredDraft) {
        self.completion = None;
        self.inserted_command = None;
        self.inserted_hint = None;
        self.editor = draft.editor;
        self.input_mode = draft.mode;
    }
    pub(super) fn push_stash(&mut self, draft: StoredDraft) {
        if draft.editor.text().is_empty() {
            return;
        }
        if self.stash.len() == 50 {
            self.stash.remove(0);
            self.notify("Stash limit · oldest entry dropped");
        }
        self.stash.push(draft);
        self.save_due = Some(self.now + Duration::from_millis(500));
    }
    /// Stash the session draft, preserving cursor and undo; empty drafts are ignored.
    pub fn stash_draft(&mut self) {
        self.restore_draft();
        if self.editor.text().is_empty() {
            self.notify("Nothing to stash · box empty");
            return;
        }
        let draft = self.take_visible_draft();
        self.push_stash(draft);
        self.advance();
        self.notify("Stashed · ctrl+x u brings it back");
    }
    /// Restore a newest-first stash position; a nonempty current draft swaps with it.
    pub fn restore_stash(&mut self, position: usize) -> bool {
        if position == 0 || position > self.stash.len() {
            self.notify("Nothing stashed");
            return false;
        }
        self.restore_draft();
        let restored = self.stash.remove(self.stash.len() - position);
        let current = self.take_visible_draft();
        let swapped = !current.editor.text().is_empty();
        self.push_stash(current);
        self.install_draft(restored);
        self.advance();
        self.notify(if swapped {
            "Swapped · what you had is stashed"
        } else {
            "Restored"
        });
        true
    }
    /// Edit an actual FIFO queue position while holding its original entry.
    pub fn edit_queued(&mut self, position: usize) -> bool {
        let Some(entry) = position
            .checked_sub(1)
            .and_then(|index| self.queue.get(index))
        else {
            return false;
        };
        if self.mode != BoxMode::Draft {
            return false;
        }
        let editor = entry.draft.editor.clone();
        let input_mode = entry.draft.mode;
        let id = entry.id.clone();
        self.set_aside = Some(self.take_visible_draft());
        self.editor = editor;
        self.input_mode = input_mode;
        self.editing_queue = Some(id);
        self.mode = BoxMode::Queued {
            position,
            total: self.queue.len(),
        };
        true
    }
    pub(super) fn save_queued(&mut self) {
        let Some(index) = self
            .queue
            .iter()
            .position(|entry| Some(&entry.id) == self.editing_queue.as_ref())
        else {
            return;
        };
        if self.editor.text().trim().is_empty() {
            let original = self.queue.remove(index).draft;
            self.push_stash(original);
            self.restore_draft();
            self.advance();
            self.notify("Empty edit removed · original stashed");
            return;
        }
        let editor = self.editor.clone();
        self.queue[index].draft = StoredDraft {
            editor,
            mode: self.input_mode,
            at: self.now,
            age_seconds: 0,
        };
        self.restore_draft();
        self.advance();
        self.notify("Queued edit saved");
    }
    pub(super) fn remove_queued(&mut self) {
        let Some(index) = self
            .queue
            .iter()
            .position(|entry| Some(&entry.id) == self.editing_queue.as_ref())
        else {
            return;
        };
        self.queue.remove(index);
        let removed = self.take_visible_draft();
        self.push_stash(removed);
        self.restore_draft();
        self.advance();
        self.notify("Removed from queue · stashed");
    }
    fn refresh_queue_position(&mut self) {
        if let Some(index) = self
            .queue
            .iter()
            .position(|entry| Some(&entry.id) == self.editing_queue.as_ref())
        {
            self.mode = BoxMode::Queued {
                position: index + 1,
                total: self.queue.len(),
            };
        }
    }
    pub(super) fn lifecycle_tick(&mut self, elapsed: Duration) {
        if self.leader.is_some_and(|due| self.now >= due) {
            self.leader = None;
        }
        if self.arm.is_some_and(|(_, due)| self.now >= due) {
            self.arm = None;
        }
        if let Some(list) = &mut self.list {
            list.picker.tick(elapsed);
        }
        if self.save_ticket.is_none() && self.save_due.is_some_and(|due| self.now >= due) {
            self.request_save();
        }
    }
    pub(super) fn lifecycle_input(&mut self, event: &Event) -> Route {
        if self.completion.is_some() {
            return Route::Consumed(self.completion_input(event));
        }
        if self.list.is_some() {
            self.list_input(event);
            return Route::Consumed(None);
        }
        if self.conflict.is_some() {
            self.conflict_input(event);
            return Route::Consumed(None);
        }
        let Event::Key(key) = event else {
            if matches!(event, Event::Paste(_) | Event::Mouse(_)) {
                self.arm = None;
                self.leader = None;
            }
            return Route::Unhandled;
        };
        if key.kind != KeyEventKind::Press {
            return Route::Unhandled;
        }
        if key.code == KeyCode::F(1) && key.modifiers.is_empty() {
            self.open_key_help();
            return Route::Consumed(None);
        }
        if matches!(self.mode, BoxMode::Locked { .. })
            && key.code != KeyCode::Esc
            && !(key.code == KeyCode::Char('c') && key.modifiers == KeyModifiers::CONTROL)
        {
            return Route::Consumed(None);
        }
        if let Some(_due) = self.leader.take() {
            return Route::Consumed(match (key.code, key.modifiers) {
                (KeyCode::Char('s'), KeyModifiers::NONE) => {
                    self.stash_draft();
                    None
                }
                (KeyCode::Char('u'), KeyModifiers::NONE) => {
                    self.restore_stash(1);
                    None
                }
                (KeyCode::Char('e'), KeyModifiers::NONE) => {
                    self.begin_editor("editor");
                    None
                }
                (KeyCode::Char('b'), KeyModifiers::NONE) => {
                    self.background_shell();
                    None
                }
                (KeyCode::Enter, KeyModifiers::NONE) => self.send_next_now(),
                (KeyCode::Esc, KeyModifiers::NONE) => None,
                _ => {
                    self.notify(&format!(
                        "Unrecognized ctrl+x sequence · {}",
                        match key.code {
                            KeyCode::Char(c) => c.to_string(),
                            _ => format!("{:?}", key.code),
                        }
                    ));
                    None
                }
            });
        }
        if key.modifiers == KeyModifiers::CONTROL {
            match key.code {
                KeyCode::Char('x') => {
                    self.leader = Some(self.now + Duration::from_secs(3));
                    return Route::Consumed(None);
                }
                KeyCode::Char('r') => {
                    self.open_history_search();
                    return Route::Consumed(None);
                }
                KeyCode::Char('v') => {
                    self.intents.push(Intent::ClipboardImage);
                    return Route::Consumed(None);
                }
                KeyCode::Char('c') => {
                    self.cancel_key(true);
                    return Route::Consumed(None);
                }
                KeyCode::Char('d') => {
                    self.delete_key();
                    return Route::Consumed(None);
                }
                _ => {}
            }
        }
        if key.modifiers.is_empty() && key.code == KeyCode::Esc {
            self.cancel_key(false);
            return Route::Consumed(None);
        }
        self.arm = None;
        Route::Unhandled
    }

    fn delete_key(&mut self) {
        if self.editing_queue.is_some() {
            self.remove_queued();
            return;
        }
        if self.editor.text().is_empty() {
            self.arm_quit('d');
            return;
        }
        self.arm = None;
        let event = Event::Key(crate::event::KeyEvent::new(
            KeyCode::Delete,
            KeyModifiers::NONE,
        ));
        if self.editor.update(&event) == crate::editor::Outcome::Changed {
            self.adopt_recall();
            self.advance();
        }
    }
    pub(crate) fn take_stop(&mut self) -> bool {
        if let Some(index) = self
            .intents
            .iter()
            .position(|intent| matches!(intent, Intent::Work(request) if request.action == super::WorkAction::Stop))
        {
            self.intents.remove(index);
            true
        } else {
            false
        }
    }
    fn cancel_key(&mut self, ctrl_c: bool) {
        if self.restore_draft() {
            return;
        }
        if matches!(self.mode, BoxMode::Locked { .. }) {
            self.cancel_editor();
            return;
        }
        if !ctrl_c && self.editor.selection().is_some() {
            self.editor.update(&Event::Key(crate::event::KeyEvent::new(
                KeyCode::Esc,
                KeyModifiers::NONE,
            )));
            return;
        }
        if self.is_busy() {
            self.request_work(super::WorkAction::Stop);
            return;
        }
        if self.input_mode == InputMode::Shell {
            self.input_mode = InputMode::Message;
            self.advance();
            return;
        }
        if !self.editor.text().is_empty() {
            if ctrl_c || self.arm.is_some_and(|(arm, _)| arm == Arm::Clear) {
                let text = self.editor.expanded_text();
                self.history_documents
                    .insert(text.clone(), self.editor.document());
                self.remember_sent(text);
                self.editor.set_text("");
                self.advance();
                self.arm = None;
                self.notify("Cleared · up brings it back");
            } else {
                self.arm = Some((Arm::Clear, self.now + Duration::from_millis(1500)));
            }
        } else if ctrl_c {
            self.arm_quit('c');
        } else if self.arm.is_some_and(|(arm, _)| arm == Arm::Rewind) {
            self.arm = None;
            if self.browse_history(1) {
                self.rewind = Some(self.version.clone());
            }
        } else {
            self.arm = Some((Arm::Rewind, self.now + Duration::from_millis(1500)));
        }
    }
    fn arm_quit(&mut self, key: char) {
        if self.arm.is_some_and(|(arm, _)| arm == Arm::Quit(key)) {
            self.arm = None;
            self.save_ticket = None;
            self.request_save();
            self.intents.push(Intent::Quit);
        } else {
            self.arm = Some((Arm::Quit(key), self.now + Duration::from_millis(1500)));
        }
    }
    pub(super) fn prepare_enter(&mut self) -> bool {
        if self.mode == BoxMode::Draft && self.editor.text().trim() == "/stash" {
            self.editor.set_text("");
            self.open_stash_list();
            return true;
        }
        let cursor = self.editor.cursor();
        if self.editor.text()[..cursor].ends_with('\\')
            && (cursor == self.editor.text().len()
                || self.editor.text()[cursor..].starts_with('\n'))
        {
            self.editor.replace_before_cursor(1, "\n");
            self.advance();
            return true;
        }
        false
    }
}
