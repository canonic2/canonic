//! Deterministic run host: F5 completion, F6 failure, F7 cancel acknowledgement.

use crate::{
    chat::{Change, Chat, Content, Intent, Update},
    dialog::{ClosePolicy, Dialog, DialogView, Reason, Size, Stack},
    event::{Event, KeyCode, KeyModifiers},
    picker::{Choice, ChoicePicker, Filter, Intent as ChoiceIntent, SelectionMode},
    run_activity::{Connection, Effect, Snapshot, State, Ticket},
    theme::Presentation,
    tool_call_card::State as CallState,
};
use ratatui::{Frame, layout::Rect};
use std::time::Duration;

pub(super) struct Host {
    cancel: Option<Ticket>,
    dialogs: Stack<ChoicePicker, u8>,
    viewport: Rect,
}
impl Host {
    pub(super) fn new(chat: &mut Chat, scenario: usize) -> Self {
        let mut host = Self {
            cancel: None,
            dialogs: Stack::new_timed(vec![0]).expect("distinct fixture target"),
            viewport: Rect::default(),
        };
        let mut snapshot = Snapshot::new("fixture-run");
        snapshot.elapsed = Duration::from_secs(14);
        snapshot.cancel_available = true;
        snapshot.options_available = true;
        snapshot.state = match scenario {
            0 | 10 => State::Idle,
            1 => State::Preparing,
            3 => State::Waiting {
                questions: 1,
                subject: "acme is waiting".into(),
            },
            19 => State::Waiting {
                questions: 0,
                subject: "permission review".into(),
            },
            4 | 9 => State::Completed,
            5 => State::Failed("model limit reached".into()),
            6 => State::Interrupted("acme restarted".into()),
            13 => State::Cancelled,
            _ => State::Running,
        };
        chat.begin_run(snapshot);
        chat.set_run_reduced_motion(scenario == 18);
        if matches!(scenario, 8 | 10) {
            chat.set_run_connection(Connection::Offline {
                retry_in: Some(Duration::from_secs(4)),
            });
            if scenario == 8 {
                Self::unknown(chat, true);
            }
        }
        if matches!(scenario, 11 | 12 | 14 | 15 | 16) {
            chat.update(&Event::Key(crate::event::KeyEvent::new(
                KeyCode::Esc,
                KeyModifiers::NONE,
            )));
            for intent in chat.take_intents() {
                host.effect(chat, intent);
            }
            if scenario == 12 {
                chat.update(&Event::Key(crate::event::KeyEvent::new(
                    KeyCode::Esc,
                    KeyModifiers::NONE,
                )));
            }
            if matches!(scenario, 15 | 16) {
                host.fail_cancel(chat);
            }
            if scenario == 14 {
                host.finish(chat, &State::Completed);
            }
            if scenario == 16 {
                host.open_options();
            }
        }
        match scenario {
            0 | 4 | 9 | 10 => Self::calls(chat, &CallState::Completed),
            5 | 7 => Self::calls(chat, &CallState::Failed),
            6 => Self::calls(chat, &CallState::Interrupted("run interrupted".into())),
            13 => Self::calls(chat, &CallState::Cancelled("you pressed esc".into())),
            _ => {}
        }
        let end = match scenario {
            0 | 4 | 9 | 10 => Some(crate::chat::End::Done("38s".into())),
            5 => Some(crate::chat::End::Failed("model limit reached".into())),
            6 => Some(crate::chat::End::Interrupted("acme restarted".into())),
            13 => Some(crate::chat::End::Cancelled("you pressed esc".into())),
            _ => None,
        };
        if let Some(end) = end {
            chat.finish_turn("tools", end, "14s");
        }
        chat.set_retry_available(matches!(scenario, 5 | 6));
        host
    }
    pub(super) fn effect(&mut self, chat: &mut Chat, intent: Intent) {
        if let Intent::Run(intent) = intent {
            match intent.effect {
                Effect::Cancel => {
                    self.cancel = Some(intent.ticket);
                    Self::calls(chat, &CallState::Cancelling);
                }
                Effect::Options => {
                    self.open_options();
                    chat.acknowledge_run_options(&intent.ticket);
                }
            }
        }
    }
    fn fail_cancel(&mut self, chat: &mut Chat) {
        if let Some(ticket) = self.cancel.take() {
            chat.fail_run_cancel(&ticket, "acme didn't confirm in 10s");
            Self::calls(chat, &CallState::Running);
        }
    }
    fn finish(&mut self, chat: &mut Chat, state: &State) {
        let Some(run) = chat.run_activity() else {
            return;
        };
        let mut snapshot = run.snapshot().clone();
        snapshot.revision += 1;
        snapshot.state = state.clone();
        if chat.update_run(snapshot) {
            self.cancel = None;
            Self::calls(
                chat,
                &if *state == State::Completed {
                    CallState::Completed
                } else {
                    CallState::Cancelled("you pressed esc".into())
                },
            );
            chat.finish_turn(
                "tools",
                if *state == State::Completed {
                    crate::chat::End::Done("15s".into())
                } else {
                    crate::chat::End::Cancelled("you pressed esc".into())
                },
                "15s",
            );
        }
    }
    fn calls(chat: &mut Chat, state: &CallState) {
        let Some(turn) = chat.turn("tools").cloned() else {
            return;
        };
        for mut part in turn.parts {
            if let Content::ToolCard(call) = &mut part.content {
                if call.state.terminal() {
                    continue;
                }
                call.revision += 1;
                call.state = state.clone();
                if *state == CallState::Completed {
                    call.summary = "6 passed".into();
                }
                chat.apply(Update {
                    conversation: chat.conversation(),
                    message: "tools".into(),
                    part: part.id,
                    expected: part.revision,
                    revision: part.revision + 1,
                    change: Change::Replace(part.content),
                });
            }
        }
    }
    fn unknown(chat: &mut Chat, unknown: bool) {
        let Some(turn) = chat.turn("tools").cloned() else {
            return;
        };
        for mut part in turn.parts {
            if let Content::ToolCard(call) = &mut part.content {
                if call.state.terminal() {
                    continue;
                }
                call.result_unknown = unknown;
                call.revision += 1;
                chat.apply(Update {
                    conversation: chat.conversation(),
                    message: "tools".into(),
                    part: part.id,
                    expected: part.revision,
                    revision: part.revision + 1,
                    change: Change::Replace(part.content),
                });
            }
        }
    }
    fn open_options(&mut self) {
        if !self.dialogs.is_empty() {
            return;
        }
        let picker = ChoicePicker::new(
            vec![
                Choice::new("retry", "Try cancelling again"),
                Choice::new("leave", "Leave it running"),
            ],
            None,
            SelectionMode::Single,
            Filter::Off,
        )
        .expect("unique fixture choices");
        self.dialogs
            .push(Dialog {
                id: "cancel-options".into(),
                title: "cancel didn't go through".into(),
                targets: vec![1],
                body: picker,
                size: Size::default(),
                close: ClosePolicy::default(),
                view: DialogView {
                    message: "Only local host simulation actions are available.".into(),
                    footer: "1–2 · enter · esc close".into(),
                    compact_footer: "enter · esc".into(),
                    ..DialogView::default()
                },
            })
            .expect("valid fixture dialog");
    }
    pub(super) fn input(&mut self, chat: &mut Chat, event: &Event) -> bool {
        if self.dialogs.is_empty() {
            return false;
        }
        let Some(ticket) = self.dialogs.ticket().cloned() else {
            return false;
        };
        let mut result = ChoiceIntent::None;
        self.dialogs
            .dispatch(event, self.viewport, |picker, _, event| {
                result = picker.update(event);
                result != ChoiceIntent::Cancelled
            });
        if let ChoiceIntent::Selected(id) = result {
            self.dialogs.close(&ticket, Reason::Submitted);
            if id == "retry" {
                chat.update(&Event::Key(crate::event::KeyEvent::new(
                    KeyCode::Esc,
                    KeyModifiers::NONE,
                )));
            }
        }
        true
    }
    pub(super) fn tick(&mut self, chat: &mut Chat, elapsed: Duration) {
        self.dialogs.tick(elapsed);
        if let Some(run) = chat.run_activity()
            && matches!(
                run.snapshot().state,
                State::Preparing | State::Running | State::Waiting { .. }
            )
        {
            let mut snapshot = run.snapshot().clone();
            snapshot.revision = snapshot.revision.saturating_add(1);
            snapshot.elapsed = snapshot.elapsed.saturating_add(elapsed);
            chat.update_run(snapshot);
        }
    }
    pub(super) fn control(&mut self, chat: &mut Chat, key: KeyCode) {
        match key {
            KeyCode::F(5) => self.finish(chat, &State::Completed),
            KeyCode::F(6) if self.cancel.is_some() => self.fail_cancel(chat),
            KeyCode::F(7) if self.cancel.is_some() => self.finish(chat, &State::Cancelled),
            KeyCode::F(8) => {
                if let Some(run) = chat.run_activity() {
                    let next = if run.connection() == Connection::Online {
                        Connection::Offline { retry_in: None }
                    } else {
                        Connection::Online
                    };
                    chat.set_run_connection(next);
                    Self::unknown(chat, next != Connection::Online);
                }
            }
            _ => {}
        }
    }
    pub(super) fn render(
        &mut self,
        chat: &Chat,
        frame: &mut Frame<'_>,
        area: Rect,
        theme: Presentation,
    ) {
        self.viewport = area;
        self.dialogs.render_with(
            frame,
            area,
            theme,
            |_, _, _| 2,
            |p, f, area, _, _| {
                p.render_list(f, area, theme, true);
                None
            },
        );
        if area.height >= 8
            && let Some(panel) = self.dialogs.panel_area(area)
        {
            let activity = Rect::new(area.x, area.bottom() - 7, area.width, 1);
            if activity.y < panel.y || activity.y >= panel.bottom() {
                frame.buffer_mut().set_style(
                    activity,
                    theme.base().remove_modifier(ratatui::style::Modifier::DIM),
                );
                chat.render_run_activity(frame, activity, theme);
            }
        }
    }
}
