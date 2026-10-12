//! Exercises every Chat board with a deterministic host and acknowledged local effects.

mod alignment;

use crate::{
    chat::{
        Action, Change, Chat, Content, End, Intent, Mode, Part, Role, Turn, Update, Updated,
        Welcome,
    },
    composer::Submission,
    event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    theme::Presentation,
};
use ratatui::{Frame, layout::Rect};
use std::{collections::VecDeque, time::Duration};

const ANSWER: &str = "## What's left\n\nThe server already stores the timezone, but the app never sends it, so both digests go out at 09:00 UTC. Profile sync already sends `X-Timezone`; the digest calls should too.\n\n- send the header on both digest calls, as in [the profile client](src/profile/api.ts:168)\n- drop the old offset table: zone names are enough\n- add a test for each call";
const CODE: &str = "headers: {\n  'X-Timezone': user.timezone, // the digest calls skip this\n},";
const PATCH: &str = "--- a/src/digest/settings.ts\n+++ b/src/digest/settings.ts\n@@ -18,1 +18,1 @@\n-headers: { json },\n+headers: { json, 'X-Timezone': tz },\n@@ -30,1 +30,1 @@\n-oldOffsetTable();\n+useZoneName();\n";

pub(super) struct ChatDemo {
    chat: Chat,
    scenario: usize,
    stream: VecDeque<String>,
    live: String,
    serial: usize,
    inspection: Option<crate::inspection::Inspector>,
    elapsed: Duration,
    pending_delivery: Option<crate::composer::Identity>,
    held_effects: VecDeque<Intent>,
}
impl ChatDemo {
    pub(super) fn new(scenario: usize) -> Self {
        let mut demo = Self {
            chat: Chat::new(),
            scenario,
            stream: VecDeque::new(),
            live: "reply".into(),
            serial: 0,
            inspection: None,
            elapsed: Duration::ZERO,
            pending_delivery: None,
            held_effects: VecDeque::new(),
        };
        demo.chat.set_status("ready");
        demo.chat.set_footer("orbit-api#276 ~chat");
        demo.chat.set_retry_available(true);
        demo.chat.set_actions_available(scenario != 6);
        demo.chat.composer_mut().set_completions(vec![
            crate::composer::CompletionItem {
                id: "review".into(),
                value: "review".into(),
                detail: "Review the local simulated changes".into(),
                kind: crate::composer::CompletionKind::Command,
                disabled: None,
                touched: false,
            },
            crate::composer::CompletionItem {
                id: "digest-file".into(),
                value: "src/digest/settings.ts".into(),
                detail: "Local fixture file".into(),
                kind: crate::composer::CompletionKind::File,
                disabled: None,
                touched: true,
            },
        ]);
        demo.chat
            .composer_mut()
            .set_repository_label("orbit-api#276 ~chat");
        demo.chat.set_welcome(Welcome {
            title: "canonic".into(),
            context: "acme · orbit-api · actions on".into(),
            recent: vec![
                "Recent · digest headers · 20:03".into(),
                "Recent · webhook retries · yesterday".into(),
            ],
            suggestions: vec![
                "send the timezone on both digest calls".into(),
                "check the retry queue".into(),
            ],
        });
        if !matches!(scenario, 1 | 9) {
            demo.seed();
        }
        demo.configure();
        if scenario >= 51 {
            demo.configure_alignment();
        }
        demo.chat.set_actions_available(scenario != 6);
        demo
    }
    pub(super) fn input(&self) -> &str {
        self.chat.input()
    }
    fn seed(&mut self) {
        let mut user = Turn::new(
            "request",
            Role::User,
            "you",
            "so what's left on #276 after the server side?",
        );
        user.timestamp = "00:40".into();
        user.end = End::Quiet;
        self.chat.push_record(user);
        let mut reply = Turn::new("reply", Role::Assistant, "acme", ANSWER);
        reply.timestamp = "00:40".into();
        reply.end = End::Done("38s".into());
        reply.parts = vec![
            tool("read", "read", "src/digest/settings.ts", "", ""),
            tool(
                "search",
                "search",
                "\"X-Timezone\"",
                "4 matches in 3 files",
                "",
            ),
            Part::new("body", Content::Markdown(ANSWER.into())),
            code(),
            table(3),
            Part::new(
                "tail",
                Content::Markdown("Add the header to both digest calls.".into()),
            ),
        ];
        self.chat.push_record(reply);
    }
    fn replace(&mut self, parts: Vec<Part>, end: End) {
        let turn = Turn {
            end,
            parts,
            ..self
                .chat
                .turn("reply")
                .cloned()
                .unwrap_or_else(|| Turn::new("reply", Role::Assistant, "acme", ""))
        };
        let user = self.chat.turn("request").cloned();
        self.chat.reset_transcript();
        self.chat.set_retry_available(true);
        self.chat.set_status("ready");
        if let Some(user) = user {
            self.chat.push_record(user);
        }
        self.chat.push_record(turn);
        self.chat.set_footer("orbit-api#276 ~chat");
    }
    fn configure(&mut self) {
        match self.scenario {
            2 | 12 | 31 | 33 | 49 => self.streaming(),
            3 => { self.replace(vec![markdown("Both digest calls now send X-Timezone. An unknown zone")], End::Failed("connection lost after 41 words".into())); self.chat.set_status("reply failed · partial reply kept"); }
            4 => self.chat.set_status("Disconnected fixture · offline"),
            5 | 32 | 34 | 35 | 36 => self.long_history(),
            6 => self.actions_off(),
            7 => self.pane(),
            10 => self.replace(vec![markdown("Both digest calls now send X-Timezone, the same header profile sync uses. An unknown zone falls back to UTC, as before.")], End::Done("34s".into())),
            11 => self.delivery(),
            13 => { self.replace(vec![markdown("Checking how profile sync sends it first."), tool("read", "read", "src/profile/api.ts", "", ""), tool("run", "run", "npm test -- digest", "running · 4s", "")], End::Live); self.chat.composer_mut().set_working(true); self.chat.set_status("running npm test · 22s · esc stops"); }
            14 => self.replace(vec![markdown("Both digest calls now send X-Timezone. An unknown zone")], End::Stopped("12s · you pressed esc".into())),
            15 => self.replace(vec![markdown("Both digest calls now send X-Timezone. An unknown zone falls back to UTC.")], End::Done("34s".into())),
            16 => self.days(),
            17 => self.chat.set_earlier_count(340),
            18 | 19 => self.replace(vec![markdown("The server already stores the **timezone**, but the app *never sends it*. Profile sync sends `X-Timezone`; see [the profile client](src/profile/api.ts:168).")], End::Done("38s".into())),
            20 => self.replace(vec![markdown("# What's left\n\n- send the header on both calls\n- drop the old offset table\n  - zone names are enough now\n- add a test for each call\n\n### Order\n\n1. server\n2. app\n3. cleanup")], End::Done("38s".into())),
            21 => self.replace(vec![markdown("From the review:\n\n> Clear and submit can land in the same second and the submit gets dropped.\n\n---\n\nThat's fixed by keeping microseconds.")], End::Done("22s".into())),
            22 => self.replace(vec![markdown("The digest client should match this:"), code()], End::Done("38s".into())),
            23 => self.replace(vec![Part::new("code", Content::Code { text: "curl -s https://api.example.com/v2/digests?user=sam&zone=Europe/Lisbon&send_at=09:00 | jq .schedule\necho done".into(), language: "sh".into(), file: String::new(), line: None })], End::Done("38s".into())),
            24 => self.replace(vec![table(3)], End::Done("38s".into())),
            25 => self.replace(vec![table(6)], End::Done("38s".into())),
            26 => self.replace(vec![Part::new("diff", Content::Diff { file: "src/digest/settings.ts".into(), patch: PATCH.into(), summary: "+6 −2 · applied".into() })], End::Done("38s".into())),
            27 => self.replace(vec![markdown("Wrote the comparison to a file:"), Part::new("pdf", Content::Attachment { name: "digest-render.pdf".into(), size: "2.1 MB".into(), target: Some("digest-render.pdf".into()), unavailable: String::new() }), Part::new("trace", Content::Attachment { name: "trace.json".into(), size: String::new(), target: None, unavailable: "no longer available".into() })], End::Done("19s".into())),
            28 | 40 => self.replace(vec![Part::new("reasoning", Content::Reasoning { summary: "thought for 12s".into(), text: "The timezone header is set per call, so the shared client doesn't help. Both call sites need it.".into() }), markdown("Both digest calls build their own headers, so each needs the line.")], End::Done("38s".into())),
            29 => self.replace(vec![Part::new("chart", Content::Unknown { kind: "chart".into(), description: "can't be shown here · 3 series, 30 points".into() }), markdown("<details><summary>Raw</summary>\n</details>\n\nDelivery rate held at 99.4% all week.\n\nEscape shown: \x1b[31m")], End::Done("38s".into())),
            30 => self.replace(vec![Part::new("log", Content::Code { text: "[09:00:01] digest: sending 1,988 emails\n".repeat(3000), language: "log".into(), file: String::new(), line: None })], End::Done("38s".into())),
            47 => self.replace(vec![markdown("Profile sync already sends it; see [the profile client](src/profile/api.ts:168).")], End::Done("38s".into())),
            _ => {}
        }
        self.configure_navigation();
        if self.scenario == 15 {
            self.control(KeyCode::F(8));
        }
    }
    fn configure_navigation(&mut self) {
        if matches!(self.scenario, 32 | 33 | 35 | 36 | 50) {
            self.chat.update(&alt(KeyCode::Home));
        }
        if self.scenario == 34 {
            self.chat.update(&alt(KeyCode::Home));
            self.chat.update(&alt(KeyCode::End));
        }
        if (37..=42).contains(&self.scenario) {
            self.chat.set_input("and bump the app version");
            self.chat.open_search();
            if self.scenario != 37 {
                self.chat.update(&Event::Paste(
                    if self.scenario == 41 {
                        "moon cheese"
                    } else {
                        "timezone"
                    }
                    .into(),
                ));
            }
            if self.scenario == 39 {
                self.chat.update(&key(KeyCode::Down));
            }
            if self.scenario == 42 {
                self.chat.close_search(false);
            }
        }
        if (43..=47).contains(&self.scenario) {
            self.chat.set_input("and bump the app version");
            self.chat.open_selection();
            if matches!(self.scenario, 44 | 45) {
                self.chat.update(&key(KeyCode::Up));
                self.chat.update(&key(KeyCode::Up));
            }
            if self.scenario == 45 {
                self.chat.update(&key(KeyCode::Char('c')));
                self.effects();
            }
            if self.scenario == 46 {
                self.chat.update(&key(KeyCode::Char('m')));
            }
        }
        if self.scenario == 28 {
            self.chat.open_selection();
            self.chat.update(&key(KeyCode::Up));
            self.chat.update(&key(KeyCode::Enter));
            self.chat.close_selection();
        }
        if self.scenario == 35 {
            self.chat.set_earlier_count(50);
            self.chat.update(&alt(KeyCode::Home));
            self.effects();
        }
    }
    fn streaming(&mut self) {
        let text = if self.scenario == 12 {
            "Here's the change:\n\n```ts\nheaders: {\n  'X-Timezone': user.timezone,"
        } else {
            "Both digest calls now send X-Timezone, the same header profile sync uses. An unknown zone"
        };
        self.replace(vec![markdown(text)], End::Live);
        self.stream = if self.scenario == 12 {
            "\n},\n```\n\nBoth calls now send it."
        } else {
            " falls back to UTC, as before. The digest tests pass."
        }
        .split_inclusive(' ')
        .map(str::to_owned)
        .collect();
        self.chat.composer_mut().set_working(true);
        self.chat.set_status("writing · 6s · esc stops");
    }
    fn long_history(&mut self) {
        self.chat.reset_transcript();
        self.chat.set_status("ready");
        self.chat.set_retry_available(true);
        self.chat
            .composer_mut()
            .set_repository_label("orbit-api#276 ~chat");
        for n in 0..40 {
            let mut turn = Turn::new(
                &format!("history-{n}"),
                if n % 2 == 0 {
                    Role::User
                } else {
                    Role::Assistant
                },
                if n % 2 == 0 { "you" } else { "acme" },
                &format!(
                    "Message {n}: Resize, scroll, and inspect the timezone wrapping.\n\n{}",
                    "The digest header preserves Europe/Lisbon. ".repeat(6)
                ),
            );
            turn.timestamp = format!("20:{n:02}");
            turn.end = End::Quiet;
            self.chat.push_record(turn);
        }
        self.chat.set_input("keep my draft and caret");
        self.chat.set_earlier_count(340);
    }
    fn delivery(&mut self) {
        self.chat.set_input("then update the changelog for #276");
        if let Action::Submit(submission) = self.chat.update(&key(KeyCode::Enter)) {
            self.chat.push_submission(&submission, "20:11");
            self.pending_delivery = Some(submission.ticket);
        }
        self.chat.set_input("and bump the app version");
    }
    fn days(&mut self) {
        self.chat.reset_transcript();
        for (id, day, role, label, text) in [
            (
                "y-you",
                "yesterday",
                Role::User,
                "you",
                "why do digests arrive at 09:00 UTC?",
            ),
            (
                "y-acme",
                "yesterday",
                Role::Assistant,
                "acme",
                "The app never sends a timezone.",
            ),
            (
                "t-you",
                "today",
                Role::User,
                "you",
                "send the user's timezone on both calls",
            ),
        ] {
            let mut turn = Turn::new(id, role, label, text);
            turn.day = day.into();
            turn.timestamp = "20:03".into();
            turn.end = End::Quiet;
            self.chat.push_record(turn);
        }
    }
    fn actions_off(&mut self) {
        self.replace(vec![markdown("Keeping microseconds restores the order. The same-second test from the review comes next."),
            Part::new("diff", Content::Diff { file: "apps/backend/WebhookQueue.php".into(), patch: "@@ -43,1 +43,2 @@\n-received_at: startOfSecond()\n+// Keep microseconds\n+received_at: immutableNow()".into(), summary: "+2 −1 · logged".into() }),
            Part::new("attention", Content::Status("actions off ────────────────\nThis edit was logged, not applied.\n\na apply this edit (simulation)\nA turn actions on (simulation)\ne inspect first\nn skip with your note".into()))], End::Quiet);
        self.chat.set_status("! needs you · a A e n · esc later");
        self.chat.set_input("type a note to send with your choice");
    }
    fn pane(&mut self) {
        self.replace(vec![markdown("Checking the retry queue and recent errors."), tool("run", "run", "canonic queue stats retries --verbose", "", ""),
            tool("grep", "run", "grep 409 logs/worker.log", "no matches", ""),
            markdown("Delivery is at 99.4% (1,988 of 2,000 today), median 210 ms.\n\n✓ 4 workers active\n✓ signatures all valid\n! retry queue at 12\n✕ 2 webhooks fail with 410; cause is a deleted endpoint\n\nBoth point at the same customer URL. Next: disable it and notify them.")], End::Done("57s".into()));
        self.chat
            .composer_mut()
            .set_repository_label("worker#retries");
        self.chat.set_status("! api#reindex needs you · ctrl+g");
        self.chat.set_footer("worker#retries");
    }
    pub(super) fn update(&mut self, event: &Event) -> bool {
        if let Some(viewer) = &mut self.inspection {
            if matches!(event, Event::Key(k) if k.code==KeyCode::F(10) || k.modifiers.contains(KeyModifiers::CONTROL)&&matches!(k.code,KeyCode::Char('c'|'t')))
            {
                return false;
            }
            viewer.update(event);
            for intent in viewer.take_intents() {
                if let crate::inspection::Intent::Request(r) = intent {
                    let result = match r.operation {
                        crate::inspection::Operation::Copy { .. } => {
                            crate::inspection::Outcome::Copied
                        }
                        crate::inspection::Operation::Editor { .. } => {
                            crate::inspection::Outcome::Opened
                        }
                        crate::inspection::Operation::Load => crate::inspection::Outcome::Loaded(
                            crate::inspection::State::Ready(crate::inspection::Content::Diff {
                                patch: PATCH.into(),
                                new_file: None,
                            }),
                        ),
                    };
                    let _ = viewer.complete(&r, result);
                }
            }
            if !viewer.active() {
                self.inspection = None;
            }
            return true;
        }
        if let Event::Key(key) = event {
            if key.kind == KeyEventKind::Release {
                return true;
            }
            if self.scenario == 6
                && key.modifiers.is_empty()
                && matches!(key.code, KeyCode::Char('a' | 'A' | 'e' | 'n'))
            {
                if key.code == KeyCode::Char('e') {
                    self.inspection = crate::inspection::Inspector::new(
                        crate::inspection::Item::new("patch", "src/digest/settings.ts"),
                        crate::inspection::State::Ready(crate::inspection::Content::Diff {
                            patch: PATCH.into(),
                            new_file: None,
                        }),
                    )
                    .ok();
                } else {
                    self.chat.set_status("choice recorded in local simulation");
                }
                return true;
            }
            if matches!(key.code, KeyCode::F(5..=9)) {
                self.control(key.code);
                return true;
            }
            if key.code == KeyCode::Esc
                && self.chat.mode() == Mode::Compose
                && !self.chat.composer().is_busy()
                && !self.chat.composer().overlay_open()
                && matches!(self.chat.composer().mode(), crate::composer::BoxMode::Draft)
            {
                return self.chat.dismiss_selection();
            }
            if matches!(key.code, KeyCode::F(_) | KeyCode::Char('c' | 't') if key.modifiers.contains(KeyModifiers::CONTROL))
            {
                return false;
            }
            if matches!(key.code, KeyCode::F(2 | 4 | 10)) {
                return false;
            }
        }
        self.handle_action(event);
        self.effects();
        true
    }
    fn handle_action(&mut self, event: &Event) {
        match self.chat.update(event) {
            Action::Submit(submission) => self.submitted(&submission),
            Action::Cancel => {
                self.stream.clear();
                self.chat
                    .end_turn(&self.live, End::Stopped("you pressed esc".into()));
                self.chat.composer_mut().turn_ended(false);
                self.chat.set_status("ready · queue paused");
            }
            Action::None => {}
        }
    }
    fn submitted(&mut self, submission: &Submission) {
        if self
            .chat
            .turn(&self.live)
            .is_some_and(|turn| matches!(turn.end, End::Live))
        {
            self.chat
                .end_turn(&self.live, End::Stopped("next queued turn".into()));
            self.stream.clear();
        }
        self.chat.push_submission(submission, "20:10");
        if self.scenario == 4 {
            self.chat.acknowledge_submission(
                &submission.ticket,
                Err("offline · back in the message box"),
            );
            return;
        }
        self.chat.acknowledge_submission(&submission.ticket, Ok(()));
        self.serial += 1;
        self.live = format!("response-{}", self.serial);
        self.chat
            .push_record(Turn::new(&self.live, Role::Assistant, "acme", ""));
        self.stream = "Simulated response: both digest calls now send the timezone. The draft and conversation work independently of an agent backend.".split_inclusive(' ').map(str::to_owned).collect();
        self.chat.composer_mut().set_working(true);
        self.chat.set_status("writing · esc stops");
    }
    pub(super) fn tick(&mut self, elapsed: Duration) {
        self.chat.tick(elapsed);
        self.elapsed += elapsed;
        self.chat.composer_mut().set_working_elapsed(self.elapsed);
        if let Some(fragment) = self.stream.pop_front() {
            self.append(&fragment);
            if self.stream.is_empty() {
                self.finish(true);
            }
        }
        self.effects();
    }
    fn append(&mut self, text: &str) {
        if let Some(part) = self
            .chat
            .turn(&self.live)
            .and_then(|t| t.parts.iter().find(|p| p.id == "body"))
        {
            self.chat.apply(Update {
                conversation: self.chat.conversation(),
                message: self.live.clone(),
                part: "body".into(),
                expected: part.revision,
                revision: part.revision + 1,
                change: Change::Append(text.into()),
            });
        }
    }
    fn finish(&mut self, success: bool) {
        self.chat.end_turn(
            &self.live,
            if success {
                End::Done("34s".into())
            } else {
                End::Failed("connection lost".into())
            },
        );
        self.chat.set_status(if success {
            "ready"
        } else {
            "reply failed · queue paused"
        });
        if let Some(submission) = self.chat.composer_mut().turn_ended(success) {
            self.submitted(&submission);
        }
    }
    fn effects(&mut self) {
        self.composer_effects();
        for intent in self.chat.take_intents() {
            if self.scenario >= 51
                && matches!(
                    intent,
                    Intent::Copy { .. }
                        | Intent::Open { .. }
                        | Intent::Inspect { .. }
                        | Intent::Earlier { .. }
                        | Intent::Latest { .. }
                        | Intent::Retry { .. }
                )
            {
                self.held_effects.push_back(intent);
            } else {
                self.resolve_effect(intent);
            }
        }
    }
    fn resolve_effect(&mut self, intent: Intent) {
        match intent {
            Intent::Run(_) | Intent::Sessions(_) | Intent::Files(_) => {}
            Intent::Tool {
                message,
                part,
                intent,
            } => {
                self.chat.acknowledge_tool(
                    &message,
                    &part,
                    &intent.call,
                    &intent.ticket,
                    Err("tool effects use the Tool Call Card gallery host"),
                );
            }
            Intent::Copy { request, .. } | Intent::Open { request, .. } => {
                self.chat.acknowledge(&request, Ok(""));
            }
            Intent::Inspect {
                request, content, ..
            } => match super::inspection::from_chat("chat-content", content) {
                Ok(viewer) => {
                    self.inspection = Some(viewer);
                    self.chat.acknowledge(&request, Ok(""));
                }
                Err(error) => {
                    self.chat.acknowledge(&request, Err(&error.to_string()));
                }
            },
            Intent::Earlier { request, .. } => {
                let mut turn = Turn::new(
                    &format!("earlier-{}", self.serial),
                    Role::User,
                    "you",
                    "Earlier session: keep the timezone and test both digest calls.",
                );
                self.serial += 1;
                turn.end = End::Quiet;
                turn.day = "yesterday".into();
                let remaining = self.chat.history_counts().0.saturating_sub(1);
                self.chat
                    .complete_earlier(&request, Ok(vec![turn]), remaining);
            }
            Intent::Latest { request } => {
                let mut turn = Turn::new(
                    "newest",
                    Role::Assistant,
                    "acme",
                    "Newest loaded window restored.",
                );
                turn.end = End::Done("34s".into());
                self.chat.complete_latest(&request, vec![turn], 340);
            }
            Intent::Retry { request, .. } => {
                self.chat
                    .acknowledge(&request, Ok("retry started · failed reply kept"));
                self.serial += 1;
                self.live = format!("retry-{}", self.serial);
                self.chat
                    .push_record(Turn::new(&self.live, Role::Assistant, "acme", ""));
                self.chat.composer_mut().set_working(true);
                self.stream = "Both calls now send the timezone. Retrying starts a new reply."
                    .split_inclusive(' ')
                    .map(str::to_owned)
                    .collect();
            }
        }
    }
    fn composer_effects(&mut self) {
        use crate::composer::{EditorResult, Intent as ComposerIntent};
        for intent in self.chat.composer_mut().take_intents() {
            match intent {
                ComposerIntent::Save { ticket, .. } => {
                    self.chat.composer_mut().acknowledge_save(&ticket, Ok(()));
                }
                ComposerIntent::Editor(request) => {
                    self.chat
                        .composer_mut()
                        .finish_editor(&request.ticket, EditorResult::Text(request.text));
                }
                ComposerIntent::Command(id) | ComposerIntent::CommandWithArguments { id, .. } => {
                    self.chat
                        .push("host", &format!("/{id} invoked · local simulation"));
                }
                ComposerIntent::Work(request) => {
                    let (_, submission) = self
                        .chat
                        .composer_mut()
                        .acknowledge_work(&request.ticket, Ok(()));
                    if let Some(submission) = submission {
                        self.chat.push_submission(&submission, "20:10");
                        self.pending_delivery = Some(submission.ticket);
                    }
                    self.chat
                        .set_status("work control confirmed · local simulation");
                }
                ComposerIntent::Rewind => {
                    self.chat.set_status("rewind requested · local simulation");
                }
                ComposerIntent::ClipboardImage => self
                    .chat
                    .set_status("clipboard image unavailable in simulation"),
                ComposerIntent::RetryCompletion(_) => {
                    self.chat.set_status("cached fixture completion");
                }
                ComposerIntent::EditorCancelled(_) | ComposerIntent::Quit => {}
            }
        }
    }
    fn control(&mut self, code: KeyCode) {
        if matches!(code, KeyCode::F(5 | 6))
            && let Some(intent) = self.held_effects.pop_front()
        {
            if code == KeyCode::F(5) {
                self.resolve_effect(intent);
            } else {
                self.fail_effect(intent);
            }
            return;
        }
        match code {
            KeyCode::F(5) => {
                if let Some(ticket) = self.pending_delivery.take() {
                    self.chat.acknowledge_submission(&ticket, Ok(()));
                } else {
                    if !self
                        .chat
                        .turn(&self.live)
                        .is_some_and(|t| matches!(t.end, End::Live))
                    {
                        self.serial += 1;
                        self.live = format!("response-{}", self.serial);
                        self.chat
                            .push_record(Turn::new(&self.live, Role::Assistant, "acme", ""));
                        self.chat.composer_mut().set_working(true);
                    }
                    self.append(" New timezone output.");
                }
            }
            KeyCode::F(6) => {
                self.stream.clear();
                self.finish(false);
            }
            KeyCode::F(7) => {
                self.handle_action(&key(KeyCode::Esc));
            }
            KeyCode::F(8) => {
                if let Some(turn) = self.chat.turn(&self.live).cloned() {
                    let bad = Update {
                        conversation: self.chat.conversation(),
                        message: self.live.clone(),
                        part: "body".into(),
                        expected: u64::MAX,
                        revision: 0,
                        change: Change::Append("bad".into()),
                    };
                    if let Updated::Resync(request) = self.chat.apply(bad) {
                        self.chat.resynchronize(&request, turn);
                    }
                }
            }
            KeyCode::F(9) => {
                self.chat.set_earlier_count(340);
                self.chat.update(&alt(KeyCode::Home));
                self.chat.update(&alt(KeyCode::Home));
                for intent in self.chat.take_intents() {
                    if let Intent::Earlier { request, .. } = intent {
                        self.chat
                            .complete_earlier(&request, Err("offline (simulation)"), 340);
                    }
                }
            }
            _ => {}
        }
    }
    pub(super) fn render(&mut self, frame: &mut Frame<'_>, area: Rect, theme: Presentation) {
        if let Some(viewer) = &mut self.inspection {
            viewer.render(frame, area, theme);
        } else {
            self.chat.render_docked_with(frame, area, theme);
        }
    }
}
fn markdown(text: &str) -> Part {
    Part::new("body", Content::Markdown(text.into()))
}
fn code() -> Part {
    Part::new(
        "code",
        Content::Code {
            text: CODE.into(),
            language: "ts".into(),
            file: "src/profile/api.ts".into(),
            line: Some(168),
        },
    )
}
fn tool(id: &str, verb: &str, target: &str, summary: &str, output: &str) -> Part {
    Part::new(
        id,
        Content::Tool {
            verb: verb.into(),
            target: target.into(),
            summary: summary.into(),
            output: output.into(),
            open: if verb == "read" {
                Some(target.into())
            } else {
                None
            },
        },
    )
}
fn table(columns: usize) -> Part {
    let mut headers = vec!["call", "timezone", "tests", "owner", "last run", "p95"];
    headers.truncate(columns);
    let mut rows: Vec<Vec<String>> = vec![
        vec!["profile sync", "yes", "4", "sam", "20:01", "120 ms"],
        vec!["daily digest", "no", "–", "sam", "09:00", "340 ms"],
        vec!["weekly digest", "no", "–", "sam", "09:00", "310 ms"],
    ]
    .into_iter()
    .map(|row| row.into_iter().take(columns).map(str::to_owned).collect())
    .collect();
    rows.truncate(3);
    Part::new(
        "table",
        Content::Table {
            headers: headers.into_iter().map(str::to_owned).collect(),
            rows,
        },
    )
}
fn key(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
}
fn alt(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::ALT))
}
