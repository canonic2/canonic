//! Simulates card effects and details without agents, clipboard access or grants.

mod extended;
mod fixtures;
mod inspection;
mod run_demo;

use crate::{
    chat::{Action, Change, Chat, Content, End, Intent, Mode, Role, Turn, Update},
    event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    theme::Presentation,
    tool_call_card::{Call, Effect, State},
};
use ratatui::{Frame, layout::Rect};
use std::time::Duration;

pub(super) struct ToolDemo {
    chat: Chat,
    scenario: usize,
    inspection: Option<inspection::Inspection>,
    elapsed: Duration,
    serial: usize,
    pending: Vec<(String, String, crate::tool_call_card::Intent)>,
    run_host: Option<run_demo::Host>,
}
impl ToolDemo {
    pub(super) fn new(scenario: usize) -> Self {
        let mut chat = Chat::new();
        chat.set_footer("orbit-api#276 ~chat");
        chat.set_input("and bump the app version");
        chat.set_status("local tool simulation · F5 step · F6 fail · F7 acknowledge");
        let mut turn = Turn::new("tools", Role::Assistant, "acme", "");
        turn.timestamp = "20:03".into();
        turn.end = End::Quiet;
        turn.parts = if scenario >= 44 {
            extended::parts(scenario)
        } else {
            fixtures::parts(scenario)
        };
        let working = turn.parts.iter().any(|p| match &p.content {
            Content::ToolCard(call) => !call.state.terminal(),
            Content::ToolGroup(group) => group.calls.iter().any(|c| !c.state.terminal()),
            _ => false,
        });
        chat.push_record(turn);
        chat.composer_mut().set_working(working);
        let mut demo = Self {
            chat,
            scenario,
            inspection: None,
            elapsed: Duration::ZERO,
            serial: 0,
            pending: vec![],
            run_host: None,
        };
        if matches!(scenario, 8..=10 | 14..=19 | 26 | 29 | 33..=40) {
            demo.chat.open_selection();
        }
        if matches!(scenario, 8..=10) {
            demo.chat.update(&key(KeyCode::Enter));
            demo.chat.close_selection();
        }
        if matches!(scenario, 15 | 26 | 29 | 33) {
            demo.chat.update(&key(KeyCode::Enter));
        }
        if scenario == 17 {
            demo.chat.update(&key(KeyCode::Char('r')));
        }
        if scenario == 18 {
            demo.chat.update(&key(KeyCode::Char('o')));
        }
        if scenario == 19 {
            demo.chat.update(&key(KeyCode::Char('c')));
        }
        if matches!(scenario, 38..=40) {
            demo.chat.update(&key(KeyCode::Char('c')));
        }
        if matches!(scenario, 48 | 49 | 52 | 53 | 54 | 55 | 58) {
            demo.chat.open_selection();
            if scenario == 53 {
                demo.chat.update(&key(KeyCode::Up));
            }
            if matches!(scenario, 48 | 49 | 53 | 54 | 55) {
                demo.chat.update(&key(KeyCode::Enter));
            }
        }
        demo.effects();
        if scenario >= 59 {
            demo.run_host = Some(run_demo::Host::new(&mut demo.chat, scenario - 59));
        }
        demo
    }
    pub(super) fn input(&self) -> &str {
        self.chat.input()
    }
    pub(super) fn update(&mut self, event: &Event) -> bool {
        if let Some(host) = &mut self.run_host
            && host.input(&mut self.chat, event)
        {
            self.effects();
            return true;
        }
        if let Some(inspection) = &mut self.inspection {
            if matches!(event, Event::Key(k) if k.code == KeyCode::Esc && k.kind == KeyEventKind::Press)
                && !inspection.searching()
            {
                self.inspection = None;
            } else {
                inspection.update(event);
                if inspection.finished() {
                    self.inspection = None;
                }
            }
            return !matches!(event, Event::Key(k) if matches!(k.code, KeyCode::F(2 | 4 | 10)) || (matches!(k.code, KeyCode::Char('c' | 't')) && k.modifiers.contains(KeyModifiers::CONTROL)));
        }
        if let Event::Key(k) = event {
            if k.kind == KeyEventKind::Release {
                return true;
            }
            if matches!(k.code, KeyCode::F(5..=8)) {
                if k.kind == KeyEventKind::Press {
                    self.control(k.code);
                }
                return true;
            }
            if matches!(k.code, KeyCode::F(2 | 4 | 10))
                || (k.code == KeyCode::Char('t') && k.modifiers.contains(KeyModifiers::CONTROL))
                || (k.code == KeyCode::Char('c')
                    && k.modifiers.contains(KeyModifiers::CONTROL)
                    && self.chat.mode() == Mode::Compose
                    && !self.chat.composer().is_busy())
            {
                return false;
            }
            if k.code == KeyCode::Esc
                && self.chat.mode() == Mode::Compose
                && !self.chat.composer().is_busy()
                && !self.chat.composer().overlay_open()
            {
                return false;
            }
        }
        match self.chat.update(event) {
            Action::Cancel => {
                self.mutate(|call| {
                    if !call.state.terminal() {
                        call.state = State::Cancelled("you pressed esc".into());
                    }
                });
                self.chat.composer_mut().turn_ended(false);
            }
            Action::Submit(submission) => {
                self.chat.push_submission(&submission, "20:10");
                self.chat.acknowledge_submission(&submission.ticket, Ok(()));
                self.chat.composer_mut().turn_ended(true);
            }
            Action::None => {}
        }
        self.effects();
        true
    }
    fn effects(&mut self) {
        for intent in self.chat.take_intents() {
            if matches!(intent, Intent::Run(_)) {
                if let Some(host) = &mut self.run_host {
                    host.effect(&mut self.chat, intent);
                }
                continue;
            }
            if let Intent::Tool {
                message,
                part,
                intent,
            } = intent
            {
                if matches!(self.scenario, 38..=40) {
                    self.pending.push((message, part, intent));
                } else {
                    self.effect(&message, &part, &intent);
                }
            }
        }
        for intent in self.chat.composer_mut().take_intents() {
            if let crate::composer::Intent::Save { ticket, .. } = intent {
                self.chat.composer_mut().acknowledge_save(&ticket, Ok(()));
            }
        }
    }
    fn effect(&mut self, message: &str, part: &str, intent: &crate::tool_call_card::Intent) {
        let refused = matches!(
            (self.scenario, &intent.effect),
            (35, Effect::Copy { .. }) | (36, Effect::Details) | (37, Effect::Retry)
        );
        if refused {
            self.chat.acknowledge_tool(
                message,
                part,
                &intent.call,
                &intent.ticket,
                Err("simulated host refusal · try again"),
            );
            return;
        }
        if intent.effect == Effect::Retry {
            self.serial += 1;
            let old = self.call(message, part, &intent.call);
            if let Some(old) = old {
                let mut call = Call::new(
                    &format!("retry-{}", self.serial),
                    &old.verb,
                    &format!("{} · retry {}", old.target, self.serial),
                );
                call.state = State::Running;
                call.capabilities = old.capabilities;
                if self
                    .chat
                    .complete_tool_retry(message, part, &intent.call, &intent.ticket, &call)
                {
                    self.chat.composer_mut().set_working(true);
                }
            }
        } else {
            if intent.effect == Effect::Details
                && let Some(call) = self.call(message, part, &intent.call)
            {
                self.inspection = Some(inspection::Inspection::new(&call));
            }
            if let Effect::OpenTask { .. } = &intent.effect
                && let Some(mut call) = self.call(message, part, &intent.call)
                && let Some(child) = &call.child_task
            {
                call.output = crate::tool_call_card::Output::Text(child.latest_calls.join("\n"));
                self.inspection = Some(inspection::Inspection::new(&call));
            }
            if let Effect::OpenArtifact { artifact, .. } = &intent.effect
                && let Some(mut call) = self.call(message, part, &intent.call)
            {
                call.output = crate::tool_call_card::Output::Text(format!(
                    "Simulated artifact: {artifact}\nNo external file or browser was opened."
                ));
                self.inspection = Some(inspection::Inspection::new(&call));
            }
            self.chat
                .acknowledge_tool(message, part, &intent.call, &intent.ticket, Ok(""));
        }
    }
    fn call(&self, message: &str, part: &str, id: &str) -> Option<Call> {
        match &self
            .chat
            .turn(message)?
            .parts
            .iter()
            .find(|p| p.id == part)?
            .content
        {
            Content::ToolCard(call) if call.id == id => Some((**call).clone()),
            Content::ToolGroup(group) => group.calls.iter().find(|c| c.id == id).cloned(),
            _ => None,
        }
    }
    fn mutate(&mut self, mut change: impl FnMut(&mut Call)) {
        let Some(turn) = self.chat.turn("tools").cloned() else {
            return;
        };
        for mut part in turn.parts {
            let original = part.content.clone();
            match &mut part.content {
                Content::ToolCard(call) => {
                    let before = call.clone();
                    change(call);
                    if *call != before {
                        call.revision += 1;
                    }
                }
                Content::ToolGroup(group) => {
                    let before = group.calls.clone();
                    for call in &mut group.calls {
                        let old = call.clone();
                        change(call);
                        if *call != old {
                            call.revision += 1;
                        }
                    }
                    if group.calls != before {
                        group.revision += 1;
                    }
                }
                _ => continue,
            }
            if part.content == original {
                continue;
            }
            self.chat.apply(Update {
                conversation: self.chat.conversation(),
                message: "tools".into(),
                part: part.id,
                expected: part.revision,
                revision: part.revision + 1,
                change: Change::Replace(part.content),
            });
        }
    }
    fn control(&mut self, code: KeyCode) {
        if let Some(host) = &mut self.run_host {
            host.control(&mut self.chat, code);
            return;
        }
        match code {
            KeyCode::F(5) => self.mutate(|call| {
                if call.state == State::Queued || matches!(call.state, State::Waiting(_)) {
                    call.state = State::Running;
                } else if call.state == State::Running {
                    call.state = State::Completed;
                    call.summary = "completed · host fixture result".into();
                }
            }),
            KeyCode::F(6) => self.mutate(|call| {
                if !call.state.terminal() {
                    call.state = State::Failed;
                    call.summary = "exit 1 · simulated failure".into();
                    call.output = crate::tool_call_card::Output::Text(
                        "✕ digest schedule uses the timezone\nexpected Europe/Lisbon, got UTC"
                            .into(),
                    );
                }
            }),
            KeyCode::F(7) => {
                if self.scenario == 39 {
                    self.mutate(|call| call.summary = "updated while request pending".into());
                }
                for (message, part, intent) in std::mem::take(&mut self.pending) {
                    self.effect(&message, &part, &intent);
                }
            }
            KeyCode::F(8) => self.grow(),
            _ => {}
        }
        if matches!(code, KeyCode::F(5 | 6)) {
            self.finish_if_done();
        }
    }
    fn finish_if_done(&mut self) {
        let Some(turn) = self.chat.turn("tools") else {
            return;
        };
        let calls: Vec<_> = turn
            .parts
            .iter()
            .flat_map(|part| match &part.content {
                Content::ToolCard(call) => vec![call.as_ref()],
                Content::ToolGroup(group) => group.calls.iter().collect(),
                _ => Vec::new(),
            })
            .collect();
        let done = !calls.is_empty() && calls.iter().all(|call| call.state.terminal());
        let success = calls
            .iter()
            .all(|call| call.retried.is_some() || call.state == State::Completed);
        if done && let Some(submission) = self.chat.composer_mut().turn_ended(success) {
            self.chat.push_submission(&submission, "20:10");
            self.chat.acknowledge_submission(&submission.ticket, Ok(()));
            self.chat.composer_mut().turn_ended(true);
        }
    }
    fn grow(&mut self) {
        self.mutate(|call| {
            if call.state == State::Running {
                call.elapsed += Duration::from_millis(100);
                if let crate::tool_call_card::Output::Text(text) = &mut call.output {
                    let count = text.lines().count();
                    for i in count..count + 8 {
                        use std::fmt::Write;
                        let _ = writeln!(text, "transforming ({i}) src/digest/render.tsx");
                    }
                }
            }
        });
    }
    pub(super) fn tick(&mut self, elapsed: Duration) {
        if let Some(host) = &mut self.run_host {
            host.tick(&mut self.chat, elapsed);
        }
        self.chat.tick(elapsed);
        let previous = self.elapsed.as_millis() / 100;
        self.elapsed += elapsed;
        if matches!(self.scenario, 3 | 25) && self.elapsed.as_millis() / 100 > previous {
            self.grow();
        }
        self.effects();
    }
    pub(super) fn render(&mut self, frame: &mut Frame<'_>, area: Rect, theme: Presentation) {
        if let Some(inspection) = &mut self.inspection {
            inspection.render(frame, area, theme);
        } else {
            self.chat.render_docked_with(frame, area, theme);
            if let Some(host) = &mut self.run_host {
                host.render(&self.chat, frame, area, theme);
            }
        }
    }
}
fn key(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
}
