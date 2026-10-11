//! Composes host run state without touching drafts, transcript anchors or terminal modes.

use super::{Chat, Intent};
use crate::{
    event::{Event, KeyCode, KeyEventKind, KeyModifiers},
    run_activity::{Connection, RunActivity, Snapshot, State, Ticket},
};

impl Chat {
    /// Attach a distinct host run. Pending effects from the previous run become inert.
    pub fn begin_run(&mut self, snapshot: Snapshot) -> bool {
        let accepted = if let Some(run) = &mut self.run {
            run.begin(snapshot)
        } else if let Ok(run) = RunActivity::new(snapshot) {
            self.run = Some(run);
            true
        } else {
            false
        };
        if accepted {
            self.sync_run_availability();
        }
        accepted
    }
    /// Replace a newer authoritative run snapshot, preserving a pending cancel.
    pub fn update_run(&mut self, snapshot: Snapshot) -> bool {
        let accepted = self.run.as_mut().is_some_and(|run| run.replace(snapshot));
        if accepted {
            self.sync_run_availability();
        }
        accepted
    }
    /// Read the attached run independently of tool and turn lifecycles.
    #[must_use]
    pub fn run_activity(&self) -> Option<&RunActivity> {
        self.run.as_ref()
    }
    /// Supply transport state. Offline changes submission availability, not outcomes.
    pub fn set_run_connection(&mut self, connection: Connection) {
        if let Some(run) = &mut self.run {
            run.set_connection(connection);
            self.sync_run_availability();
        }
    }
    /// Set static working dots while host elapsed seconds continue to update.
    pub fn set_run_reduced_motion(&mut self, reduced: bool) {
        if let Some(run) = &mut self.run {
            run.set_reduced_motion(reduced);
        }
    }
    /// Report the host's cancel failure without inventing a stopped outcome.
    pub fn fail_run_cancel(&mut self, ticket: &Ticket, reason: &str) -> bool {
        self.run
            .as_mut()
            .is_some_and(|run| run.cancel_failed(ticket, reason))
    }
    /// Acknowledge that the host opened or failed to open its options view.
    pub fn acknowledge_run_options(&mut self, ticket: &Ticket) -> bool {
        self.run
            .as_mut()
            .is_some_and(|run| run.acknowledge_options(ticket))
    }
    fn sync_run_availability(&mut self) {
        if let Some(run) = &self.run {
            self.input.set_working(
                matches!(
                    run.snapshot().state,
                    State::Preparing | State::Running | State::Waiting { .. }
                ) || run.connection() != Connection::Online,
            );
        }
    }
    pub(super) fn run_event(&mut self, event: &Event) -> bool {
        if self.input.overlay_open()
            || matches!(self.input.mode(), crate::composer::BoxMode::Locked { .. })
        {
            return false;
        }
        let Some(run) = &mut self.run else {
            return false;
        };
        let Event::Key(key) = event else {
            return false;
        };
        if key.code == KeyCode::Esc
            && key.modifiers.is_empty()
            && self.input.editor().selection().is_some()
        {
            return false;
        }
        let cancel = key.code == KeyCode::Esc && key.modifiers.is_empty()
            || key.code == KeyCode::Char('c') && key.modifiers == KeyModifiers::CONTROL;
        if cancel
            && matches!(
                run.snapshot().state,
                State::Preparing | State::Running | State::Waiting { .. }
            )
        {
            if key.kind == KeyEventKind::Press
                && let Some(intent) = run.cancel()
            {
                self.intents.push(Intent::Run(intent));
            }
            return true;
        }
        if key.code == KeyCode::Char('g')
            && key.modifiers == KeyModifiers::CONTROL
            && run.snapshot().options_available
        {
            if key.kind == KeyEventKind::Press
                && let Some(intent) = run.options()
            {
                self.intents.push(Intent::Run(intent));
            }
            return true;
        }
        false
    }
}
