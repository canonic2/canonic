//! Owns identified foreground controls; only host acknowledgement releases work.

use super::{Composer, Identity, InputMode, Intent, Submission};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Motion {
    Animated,
    Reduced,
}

/// Explicit host operation on the foreground work.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkAction {
    Stop,
    InterruptAfterStep,
    Background,
}

/// One attempt identity. Duplicate and late acknowledgements are inert.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkRequest {
    pub ticket: Identity,
    pub action: WorkAction,
}

impl Composer {
    /// Pending control, including after a compatibility Chat cancel action.
    #[must_use]
    pub fn work_request(&self) -> Option<&WorkRequest> {
        self.work_request.as_ref()
    }

    pub(super) fn request_work(&mut self, action: WorkAction) {
        if self.work_request.is_some() {
            return;
        }
        let request = WorkRequest {
            ticket: Identity::new(),
            action,
        };
        if action != WorkAction::Background {
            self.paused = true;
            self.resume_waiting = None;
        }
        self.work_request = Some(request.clone());
        self.intents.push(Intent::Work(request));
        self.advance();
    }

    /// Acknowledge an exact work control. Interrupt acceptance may send one queue entry.
    /// A false first value means stale/foreign; failure leaves work and drafts intact.
    pub fn acknowledge_work(
        &mut self,
        ticket: &Identity,
        result: Result<(), &str>,
    ) -> (bool, Option<Submission>) {
        let Some(request) = &self.work_request else {
            return (false, None);
        };
        if &request.ticket != ticket {
            return (false, None);
        }
        let action = request.action;
        self.work_request = None;
        self.intents
            .retain(|intent| !matches!(intent, Intent::Work(request) if &request.ticket == ticket));
        if let Err(reason) = result {
            self.notify(&format!(
                "✕ control failed · {} · work continues",
                crate::text::clean(reason, 256, false)
            ));
            return (true, None);
        }
        self.working = false;
        let submission = match action {
            WorkAction::Stop => {
                self.paused = true;
                self.notify("– stopped · queue paused · draft kept");
                None
            }
            WorkAction::InterruptAfterStep => {
                self.paused = false;
                self.send_next()
            }
            WorkAction::Background => {
                self.background.push(ticket.clone());
                self.notify("Moved to background · box is free");
                None
            }
        };
        (true, submission)
    }

    /// Count host-confirmed background commands independently of foreground work.
    #[must_use]
    pub fn background_count(&self) -> usize {
        self.background.len()
    }

    /// Report completion of an identified background command once.
    pub fn finish_background(&mut self, ticket: &Identity) -> bool {
        let Some(index) = self.background.iter().position(|id| id == ticket) else {
            return false;
        };
        self.background.remove(index);
        true
    }

    pub(super) fn background_shell(&mut self) {
        if self.background.len() >= 128 {
            self.notify("! Background limit · 128 commands still running");
            return;
        }
        if self.input_mode == InputMode::Shell && self.is_busy() {
            self.request_work(WorkAction::Background);
        } else {
            self.notify("No running shell command to background");
        }
    }
}
