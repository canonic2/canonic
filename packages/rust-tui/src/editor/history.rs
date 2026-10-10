//! Owns bounded edit transactions and injected-time coalescing, without rendering.

use super::Draft;
use std::{collections::VecDeque, time::Duration};

const LIMIT: usize = 64;
const GROUP_PAUSE: Duration = Duration::from_secs(1);

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum EditKind {
    Typing,
    Newline,
    Paste,
    Delete,
    Backspace,
}

impl EditKind {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Typing => "typing",
            Self::Newline => "newline",
            Self::Paste => "paste",
            Self::Delete | Self::Backspace => "deletion",
        }
    }
}

#[derive(Clone)]
pub(super) struct Snapshot {
    pub draft: Draft,
    pub offset: usize,
}

struct Entry {
    snapshot: Snapshot,
    kind: EditKind,
}

#[derive(Default)]
pub(super) struct History {
    undo: VecDeque<Entry>,
    redo: VecDeque<Entry>,
    group: Option<(EditKind, Duration)>,
}

impl History {
    pub(super) fn break_group(&mut self) {
        self.group = None;
    }
    pub(super) fn record(
        &mut self,
        snapshot: Snapshot,
        kind: EditKind,
        now: Duration,
        selected: bool,
    ) {
        let grouped = !selected
            && !matches!(kind, EditKind::Paste | EditKind::Newline)
            && self.group.is_some_and(|(previous, at)| {
                previous == kind && now.saturating_sub(at) < GROUP_PAUSE
            });
        if !grouped {
            if self.undo.len() == LIMIT {
                self.undo.pop_front();
            }
            self.undo.push_back(Entry { snapshot, kind });
        }
        self.redo.clear();
        self.group = if matches!(kind, EditKind::Paste | EditKind::Newline) || selected {
            None
        } else {
            Some((kind, now))
        };
    }
    pub(super) fn restore(
        &mut self,
        current: Snapshot,
        redo: bool,
    ) -> Option<(Snapshot, EditKind)> {
        self.break_group();
        let entry = if redo {
            self.redo.pop_back()
        } else {
            self.undo.pop_back()
        }?;
        let kind = entry.kind;
        let reverse = Entry {
            snapshot: current,
            kind,
        };
        if redo {
            self.undo.push_back(reverse);
        } else {
            self.redo.push_back(reverse);
        }
        Some((entry.snapshot, kind))
    }
}
