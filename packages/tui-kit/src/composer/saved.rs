//! Owns validated restart snapshots and acknowledgement of debounced host autosaves.

use super::{
    Composer, Identity, InputMode,
    lifecycle::{Intent, QueuedDraft, StoredDraft},
};
use crate::editor::Editor;
use std::time::Duration;

/// Persistable text/cursor/mode. Hosts can serialize this without operation identities.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SavedDraft {
    pub text: String,
    pub cursor: usize,
    pub mode: InputMode,
    pub age_seconds: u64,
    pub atoms: std::collections::BTreeMap<char, crate::editor::InlineAtom>,
}
/// One session draft/queue plus repository history/stash; restoring never sends.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SavedComposer {
    pub draft: SavedDraft,
    pub queue: Vec<SavedDraft>,
    pub history: Vec<String>,
    pub shell_history: Vec<String>,
    pub stash: Vec<SavedDraft>,
    pub history_documents: std::collections::BTreeMap<String, crate::editor::Document>,
    pub history_origins: std::collections::BTreeMap<String, String>,
    /// Unacknowledged text is restored to stash, never sent automatically.
    pub pending: Option<SavedDraft>,
    /// A partially edited queue copy is protected separately from its original.
    pub queued_edit: Option<SavedDraft>,
    pub editor_return: Option<SavedDraft>,
}

impl Composer {
    /// Report a host read/decode failure without changing the current draft or storage.
    /// A fresh session remains empty; a live draft is never erased by a failed restore.
    pub fn report_restore_failure(&mut self, reason: &str) {
        self.editor.notify(&format!(
            "✕ Saved draft could not be read · {}",
            crate::text::clean(reason, 256, false)
        ));
    }
    /// Snapshot everything required to resume. A recall copy never replaces the draft.
    #[must_use]
    pub fn saved(&self) -> SavedComposer {
        let draft = self.set_aside.as_ref().map_or_else(
            || self.saved_editor(&self.editor, self.input_mode, self.now),
            |draft| self.save_draft(draft),
        );
        SavedComposer {
            editor_return: self
                .conflict
                .as_ref()
                .map(|conflict| self.save_draft(&conflict.returned)),
            pending: match &self.state {
                super::State::Pending(submission) => Some(SavedDraft {
                    text: submission.document.text.clone(),
                    cursor: submission.document.cursor,
                    atoms: submission.document.atoms.clone(),
                    mode: submission.mode,
                    age_seconds: 0,
                }),
                _ => None,
            },
            queued_edit: self
                .editing_queue
                .as_ref()
                .map(|_| self.saved_editor(&self.editor, self.input_mode, self.now)),
            draft,
            queue: self
                .queue
                .iter()
                .map(|entry| self.save_draft(&entry.draft))
                .collect(),
            history: self.history.clone(),
            shell_history: self.shell_history.clone(),
            history_documents: self.history_documents.clone(),
            history_origins: self.history_origins.clone(),
            stash: self
                .stash
                .iter()
                .map(|draft| self.save_draft(draft))
                .collect(),
        }
    }
    fn saved_editor(&self, editor: &Editor, mode: InputMode, at: Duration) -> SavedDraft {
        SavedDraft {
            text: editor.text().into(),
            atoms: editor.document().atoms,
            cursor: editor.cursor(),
            mode,
            age_seconds: self.now.saturating_sub(at).as_secs(),
        }
    }
    fn save_draft(&self, draft: &StoredDraft) -> SavedDraft {
        let mut saved = self.saved_editor(&draft.editor, draft.mode, draft.at);
        saved.age_seconds = saved.age_seconds.saturating_add(draft.age_seconds);
        saved
    }
    fn load_draft(&self, saved: &SavedDraft) -> Option<StoredDraft> {
        let mut editor = Editor::new(self.limit);
        if !editor.restore_document(&crate::editor::Document {
            text: saved.text.clone(),
            cursor: saved.cursor,
            atoms: saved.atoms.clone(),
        }) {
            return None;
        }
        Some(StoredDraft {
            editor,
            mode: saved.mode,
            at: self.now,
            age_seconds: saved.age_seconds,
        })
    }
    /// Atomically load a host snapshot. Queue is always paused after restart.
    pub fn restore_saved(&mut self, saved: &SavedComposer) -> bool {
        let mut editor = Editor::new(self.limit);
        if saved.queue.len() > 128
            || saved.history.len() > 500
            || saved.shell_history.len() > 500
            || saved.stash.len() > 50
        {
            return false;
        }
        if saved.history_origins.len() > 1000
            || saved.history_origins.iter().any(|(text, origin)| {
                origin.len() > 64
                    || crate::text::clean(origin, 64, false) != *origin
                    || !saved.history.contains(text) && !saved.shell_history.contains(text)
            })
        {
            return false;
        }
        if saved.history_documents.len() > 1000
            || saved.history_documents.iter().any(|(text, document)| {
                !editor.restore_document(document)
                    || editor.expanded_text() != *text
                    || !saved.history.contains(text) && !saved.shell_history.contains(text)
            })
        {
            return false;
        }
        let recoveries = saved
            .pending
            .iter()
            .chain(&saved.queued_edit)
            .chain(&saved.editor_return)
            .map(|draft| self.load_draft(draft))
            .collect::<Option<Vec<_>>>();
        let Some(recoveries) = recoveries else {
            return false;
        };
        let Some(draft) = self.load_draft(&saved.draft) else {
            return false;
        };
        let Some(queue) = saved
            .queue
            .iter()
            .map(|draft| {
                self.load_draft(draft).map(|draft| QueuedDraft {
                    id: Identity::new(),
                    draft,
                })
            })
            .collect::<Option<Vec<_>>>()
        else {
            return false;
        };
        let Some(stash) = saved
            .stash
            .iter()
            .map(|draft| self.load_draft(draft))
            .collect::<Option<Vec<_>>>()
        else {
            return false;
        };
        if saved
            .history
            .iter()
            .chain(&saved.shell_history)
            .any(|text| editor.set_text(text) || editor.text() != text)
        {
            return false;
        }
        self.reset("");
        self.install_draft(draft);
        self.queue = queue;
        self.stash = stash;
        for recovery in recoveries {
            self.push_stash(recovery);
        }
        self.history_documents.clone_from(&saved.history_documents);
        self.history_origins.clone_from(&saved.history_origins);
        self.history.clone_from(&saved.history);
        self.shell_history.clone_from(&saved.shell_history);
        self.paused = true;
        self.working = false;
        self.list = None;
        self.external = None;
        self.conflict = None;
        self.leader = None;
        self.arm = None;
        self.intents.clear();
        self.save_ticket = None;
        self.save_due = None;
        self.notify("Draft restored · queue paused · nothing sent");
        true
    }
    pub(super) fn request_save(&mut self) {
        if self.save_ticket.is_some() {
            return;
        }
        let ticket = Identity::new();
        self.save_ticket = Some((ticket.clone(), self.version.clone()));
        self.save_due = None;
        self.intents.push(Intent::Save {
            ticket,
            state: Box::new(self.saved()),
        });
    }
    /// Acknowledge exact storage write. Failure keeps all text and schedules retry.
    pub fn acknowledge_save(&mut self, ticket: &Identity, result: Result<(), &str>) -> bool {
        let Some((pending, version)) = &self.save_ticket else {
            return false;
        };
        if pending != ticket {
            return false;
        }
        let changed = version != &self.version;
        self.save_ticket = None;
        if let Err(reason) = result {
            self.editor.notify(&format!(
                "Draft not saved · {} · retrying in 5s · text kept",
                crate::text::clean(reason, 256, false)
            ));
            self.save_due = Some(self.now + Duration::from_secs(5));
        } else if changed {
            self.save_due = Some(self.now + Duration::from_millis(500));
        }
        true
    }
}
