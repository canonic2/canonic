//! Owns searchable history/stash pickers above the fixed dock and undoable drops.

use super::{Composer, lifecycle::StoredDraft};
use crate::{
    event::{Event, KeyCode, KeyModifiers},
    picker::{Choice, ChoicePicker, Filter, Intent as PickerIntent, PickerView, SelectionMode},
    theme::Presentation,
};
use ratatui::{Frame, layout::Rect};

#[derive(Clone, Copy)]
pub(super) enum ListKind {
    History,
    Stash,
    Keys,
}
pub(super) struct List {
    pub kind: ListKind,
    pub picker: ChoicePicker,
    drops: Vec<(usize, StoredDraft)>,
}

impl Composer {
    /// Show composer bindings using the shared picker above the dock.
    pub fn open_key_help(&mut self) {
        self.open_list(ListKind::Keys);
    }
    /// Open a searchable sent-history picker. Enter recalls without sending.
    pub fn open_history_search(&mut self) {
        self.restore_draft();
        self.open_list(ListKind::History);
    }
    /// Open the stash picker; d drops, u undoes, Enter restores/swaps.
    pub fn open_stash_list(&mut self) {
        self.restore_draft();
        self.open_list(ListKind::Stash);
    }
    fn list_choices(&self, kind: ListKind) -> Vec<Choice> {
        match kind {
            ListKind::Keys => [
                ("Enter", "send · queues while busy · saves queued edit"),
                ("Ctrl-J / Alt-Enter", "new line"),
                ("Up", "edit lines, then queued messages, then history"),
                ("Ctrl-R", "search sent history"),
                ("Ctrl-X s / u", "stash / restore or swap"),
                ("Ctrl-X e", "external editor"),
                ("Ctrl-X Enter", "resume queue and send one"),
                ("Ctrl-X b", "background running shell"),
                ("Ctrl-V", "host clipboard image"),
                ("Esc", "close · recall back · stop · arm clear or rewind"),
                ("Ctrl-C", "stop · clear · twice to quit"),
                ("Ctrl-D", "remove queued edit · delete · twice to quit"),
                ("Ctrl-Z / Ctrl-Shift-Z / Alt-Z", "undo / redo"),
                ("Ctrl-Y / Alt-Y", "yank / cycle kills"),
                ("! / @ / /", "shell / files / commands when typed"),
                (
                    "F1",
                    "Composer keys · editor keys are in the Editor reference",
                ),
            ]
            .into_iter()
            .enumerate()
            .map(|(i, (key, detail))| {
                let mut choice = Choice::new(i.to_string(), key);
                choice.detail = detail.into();
                choice
            })
            .collect(),
            ListKind::History => self
                .active_history()
                .iter()
                .enumerate()
                .map(|(i, text)| {
                    let mut choice = Choice::new(i.to_string(), text.lines().next().unwrap_or(""));
                    choice.detail = format!("{} lines", text.lines().count());
                    choice
                })
                .collect(),
            ListKind::Stash => self
                .stash
                .iter()
                .rev()
                .enumerate()
                .map(|(i, draft)| {
                    let mut choice = Choice::new(
                        i.to_string(),
                        draft
                            .editor
                            .document()
                            .display_text()
                            .lines()
                            .next()
                            .unwrap_or(""),
                    );
                    let age = self
                        .now
                        .saturating_sub(draft.at)
                        .as_secs()
                        .saturating_add(draft.age_seconds);
                    choice.detail = if age < 60 {
                        "now".into()
                    } else {
                        format!("{}m", age / 60)
                    };
                    choice
                })
                .collect(),
        }
    }
    fn open_list(&mut self, kind: ListKind) {
        if let Ok(picker) = ChoicePicker::new(
            self.list_choices(kind),
            None,
            SelectionMode::Single,
            Filter::On,
        ) {
            self.list = Some(List {
                kind,
                picker,
                drops: Vec::new(),
            });
        }
    }
    /// Whether a picker or conflict dialog needs input precedence.
    #[must_use]
    pub fn overlay_open(&self) -> bool {
        self.list.is_some() || self.conflict.is_some() || self.completion.is_some()
    }
    pub(super) fn list_input(&mut self, event: &Event) {
        if matches!(event, Event::Key(key) if key.kind == crate::event::KeyEventKind::Press && (key.code == KeyCode::Esc && key.modifiers.is_empty() || key.code == KeyCode::Char('c') && key.modifiers == KeyModifiers::CONTROL))
        {
            self.list = None;
            return;
        }
        let Some(list) = &mut self.list else {
            return;
        };
        let kind = list.kind;
        if matches!(kind, ListKind::Stash)
            && matches!(event, Event::Key(key) if key.kind == crate::event::KeyEventKind::Press && key.modifiers.is_empty() && matches!(key.code,KeyCode::Char('d'|'u')))
        {
            let selected = list
                .picker
                .selected()
                .and_then(|id| id.parse::<usize>().ok());
            if matches!(event, Event::Key(key) if key.code == KeyCode::Char('u')) {
                if let Some((index, draft)) = list.drops.pop() {
                    self.stash.insert(index.min(self.stash.len()), draft);
                    self.editor.notify("Drop undone");
                }
            } else if let Some(position) = selected
                && position < self.stash.len()
            {
                let index = self.stash.len() - 1 - position;
                list.drops.push((index, self.stash.remove(index)));
                self.editor.notify("Dropped · u undoes until list closes");
            }
            let choices = self.list_choices(kind);
            if let Some(list) = &mut self.list {
                let _ = list.picker.replace_choices(choices);
            }
            self.advance();
            return;
        }
        let result = if matches!(kind, ListKind::History)
            && matches!(event, Event::Key(key) if key.kind == crate::event::KeyEventKind::Press && key.code == KeyCode::Char('r') && key.modifiers == KeyModifiers::CONTROL)
        {
            list.picker.update(&Event::Key(crate::event::KeyEvent::new(
                KeyCode::Down,
                KeyModifiers::NONE,
            )))
        } else {
            list.picker.update(event)
        };
        match result {
            PickerIntent::Cancelled => self.list = None,
            PickerIntent::Selected(id) => {
                let position = id.parse::<usize>().ok().map(|i| i + 1);
                self.list = None;
                if let Some(position) = position {
                    match kind {
                        ListKind::Keys => {}
                        ListKind::History => {
                            self.browse_history(position);
                        }
                        ListKind::Stash => {
                            self.restore_stash(position);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    /// Render queue/list/dialog content above the caller's fixed message dock.
    pub fn render_above(&mut self, frame: &mut Frame<'_>, area: Rect, presentation: Presentation) {
        if self.completion.is_some() {
            self.render_completion(frame, area, presentation);
            return;
        }

        if let Some(list) = &mut self.list {
            let view = PickerView {
                title: match list.kind {
                    ListKind::Keys => "composer keys",
                    ListKind::History => "history",
                    ListKind::Stash => "stash",
                },
                context: match list.kind {
                    ListKind::Keys => "type filters · esc closes",
                    ListKind::History => "enter recalls · esc closes · ctrl+r older match",
                    ListKind::Stash => "enter restores · d drops · u undoes · esc closes",
                },
                max_height: area.height.min(9),
                presentation,
                ..PickerView::default()
            };
            let height = list.picker.height(area.width, view).min(area.height);
            let area = Rect {
                y: area.bottom().saturating_sub(height),
                height,
                ..area
            };
            list.picker.render(frame, area, view, true);
        } else {
            let mut lines: Vec<_> = self
                .queue()
                .into_iter()
                .take(3)
                .enumerate()
                .map(|(i, entry)| {
                    ratatui::text::Line::styled(
                        format!(
                            "{} {}  {}{}{}{}",
                            if i == 0 { "queued" } else { "      " },
                            i + 1,
                            if entry.mode == super::InputMode::Shell {
                                "$ "
                            } else {
                                ""
                            },
                            if entry.editing {
                                "editing below".into()
                            } else {
                                crate::text::clean(&entry.text, 256, false)
                            },
                            if entry.mode == super::InputMode::Shell {
                                " · shell"
                            } else {
                                ""
                            },
                            if self.paused { " · paused" } else { "" }
                        ),
                        if entry.editing {
                            presentation.selection()
                        } else {
                            presentation.muted()
                        },
                    )
                })
                .collect();
            if self.queue.len() > 3 {
                lines.push(ratatui::text::Line::styled(
                    format!("       +{} more", self.queue.len() - 3),
                    presentation.muted(),
                ));
            }
            frame.render_widget(ratatui::widgets::Paragraph::new(lines), area);
        }
    }
    /// Requested height grows upward without changing the message dock's height.
    #[must_use]
    pub fn above_height(&self) -> u16 {
        if let Some(completion) = &self.completion {
            completion.menu.height(u16::MAX)
        } else if self.list.is_some() {
            9
        } else {
            u16::try_from(self.queue.len().min(3) + usize::from(self.queue.len() > 3)).unwrap_or(4)
        }
    }
    /// Requested above-dock height adapted to the host's available width.
    #[must_use]
    pub fn above_height_for(&self, width: u16) -> u16 {
        self.completion
            .as_ref()
            .map_or_else(|| self.above_height(), |c| c.menu.height(width))
    }
    /// Completion help for the activity row above the draft.
    #[must_use]
    pub fn completion_hint_for(&self, width: u16) -> Option<&str> {
        self.completion.as_ref().map(|c| {
            self.editor
                .notice()
                .unwrap_or_else(|| c.menu.hint_for(width))
        })
    }
    /// Composer help below the dock; completion help uses the activity row.
    #[must_use]
    pub fn footer_hint_for(&self, width: u16) -> &str {
        if width >= 50 {
            return self.footer_hint();
        }
        if self.conflict.is_some() {
            return "enter chooses · esc keeps box";
        }
        if let Some(list) = &self.list {
            return match list.kind {
                ListKind::History => "enter recalls · esc closes",
                ListKind::Stash => "enter restores · esc closes",
                ListKind::Keys => "esc closes",
            };
        }
        if self.leader.is_some() {
            return "esc cancels sequence";
        }
        if self.arm.is_some() {
            return self.footer_hint();
        }
        match self.mode {
            super::BoxMode::Queued { .. } => "enter saves · esc keeps",
            super::BoxMode::History { .. } => "enter sends · esc your draft",
            super::BoxMode::Locked { .. } => "esc stops waiting",
            super::BoxMode::Draft if self.is_busy() => "enter queues · esc stops",
            super::BoxMode::Draft if self.input_mode == super::InputMode::Shell => {
                "enter runs · esc leaves"
            }
            super::BoxMode::Draft => "enter sends · esc clears",
        }
    }
    /// Contextual key status below the dock, including the active picker or box mode.
    #[must_use]
    pub fn footer_hint(&self) -> &str {
        if self.work_request.is_some() {
            return "actions on · enter queues · esc already asked";
        }
        if self.conflict.is_some() {
            return "1 editor · 2 box · enter chooses · esc keeps box";
        }
        if let Some(list) = &self.list {
            return match list.kind {
                ListKind::History => "enter recalls · esc closes",
                ListKind::Stash => "enter restores · d drops · u undoes · esc closes",
                ListKind::Keys => "type filters · esc closes",
            };
        }
        if self.leader.is_some() {
            return "actions on · esc cancels sequence";
        }
        if let Some((arm, _)) = self.arm {
            return match arm {
                super::lifecycle::Arm::Clear => "esc again clears · any other key cancels",
                super::lifecycle::Arm::Rewind => "esc again edits last message",
                super::lifecycle::Arm::Quit('d') => "ctrl+d again quits",
                super::lifecycle::Arm::Quit(_) => "ctrl+c again quits",
            };
        }
        match self.mode {
            super::BoxMode::Queued { .. } => "enter saves · esc keeps as was · ctrl+d removes",
            super::BoxMode::History { .. } => "↑↓ older/newer · enter sends again · esc your draft",
            super::BoxMode::Locked { .. } => "editing outside · ctrl+c cancels waiting",
            super::BoxMode::Draft if self.literal_command.as_ref() == Some(&self.version) => {
                "actions on · enter sends as text · F1 keys"
            }
            super::BoxMode::Draft if self.command_ready() => {
                "actions on · enter runs command · F1 keys"
            }
            super::BoxMode::Draft if self.is_busy() => "actions on · enter queues · esc stops",
            super::BoxMode::Draft if self.input_mode == super::InputMode::Shell => {
                "enter runs · backspace leaves"
            }
            super::BoxMode::Draft => "actions on · enter sends · F1 keys",
        }
    }
}
