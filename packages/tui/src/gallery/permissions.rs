//! Deterministic permission acknowledgements and mixed reviews, without external effects.

use crate::{
    chat::{Anchor, Chat, Content, Part, Role, Turn},
    event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    permission_review::{self, Choice, Kind, PermissionReview, Request, Resource, Result as Reply},
    review_queue::{Intent, Review},
    theme::Presentation,
    tool_call_card::{Call, Output, State as ToolState},
};
use ratatui::{Frame, layout::Rect};
use std::{fmt::Write, time::Duration};

pub(super) struct PermissionDemo {
    chat: Chat,
    pending: Option<permission_review::Response>,
    stale: Option<permission_review::Ticket>,
    fixture: usize,
    elapsed: Duration,
    delayed: bool,
    stream: bool,
    feedback: String,
}
fn key(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
}
fn press(review: &mut PermissionReview, code: KeyCode) {
    review.update(&key(code));
}
pub(super) fn request() -> Request {
    let mut once = Choice::new("once", "Allow once", Kind::Allow);
    once.detail = "this edit".into();
    once.scope = "edit · these 2 files · once".into();
    let mut session = Choice::new("session", "Allow for this session", Kind::Allow);
    session.detail = "edits in src/digest/".into();
    session.scope = "edit · src/digest/** · until you quit".into();
    session.wider = true;
    let mut deny = Choice::new("deny", "Deny", Kind::Deny);
    deny.detail = "acme is told no".into();
    let mut skip = Choice::new("skip", "Skip", Kind::Skip);
    skip.detail = "acme goes on without it".into();
    Request{id:"digest-edit".into(),revision:2,run:"run #276".into(),title:"edit 2 files".into(),summary:"Edit 2 files in src/digest/".into(),reason:"To send each user's timezone with the digest.".into(),resources:vec![Resource{name:"src/digest/settings.ts".into(),detail:"+6 -2".into()},Resource{name:"src/digest/schedule.ts".into(),detail:"+3 -1".into()}],details_label:"diff · 2 files · +9 -3".into(),details:Output::Diff("--- a/src/digest/settings.ts\n+++ b/src/digest/settings.ts\n@@ -17,3 +17,4 @@\n headers: {\n-  json,\n+  json, 'X-Timezone':\n+    user.timezone,\n },\n--- a/src/digest/schedule.ts\n+++ b/src/digest/schedule.ts\n@@ -1,2 +1,3 @@\n schedule({\n+  timezone: user.timezone,\n });".into()),choices:vec![once,session,deny,skip],default:None}
}
fn descriptor(name: &str) -> Request {
    let mut r = request();
    match name {
        "safe-default"=>r.default=Some("deny".into()),
        "many-resources"=>{r.title="edit 12 files".into();r.summary="Edit 12 files in src/digest/".into();r.details_label="diff · 12 files".into();r.resources=(0..12).map(|i|Resource{name:format!("src/digest/file-{i}.ts"),detail:"+6 -2".into()}).collect();}
        "command"=>{r.title="run a command".into();r.summary="Run npm run migrate:digest".into();r.resources=vec![Resource{name:"in acme-api/".into(),detail:"env DATABASE_URL=[masked]".into()}];r.details_label="command · 1 line · no diff".into();r.details=Output::Code{text:"DATABASE_URL=[masked] npm run migrate:digest".into(),first_line:1};r.choices[1].scope="run · npm run migrate:* in acme-api/ · until you quit".into();}
        "network"=>{r.title="fetch a URL".into();r.summary="Fetch status.example.com/api/incidents".into();r.reason="Read-only GET, no body, no credentials.".into();r.resources.clear();r.details_label="request · GET · 0 B".into();r.details=Output::Text("GET https://status.example.com/api/incidents\nNo body or credentials".into());r.choices[1].scope="GET · status.example.com · until you quit".into();}
        "destructive"|"armed"|"disarmed"|"arm-expired"=>{r.title="delete 3 files".into();r.summary="Delete 3 files in dist/legacy/".into();r.choices[0].label="Delete these 3 files".into();r.choices[0].detail="can't be undone".into();r.choices[0].destructive=true;}
        "scope-destructive"=>r.choices[1].destructive=true,
        "long-scope"=>r.choices[1].scope="edit · src/digest/** with every supplied resource included · until you quit; this exact scope is retained word for word".repeat(12),
        "long-details"=>{
            let mut text=String::new();
            for i in 0..250 {let _=writeln!(text,"loaded inspection line {i}: timezone 東京 é 👨‍👩‍👧‍👦");}
            r.details=Output::Text(text);
        }
        "long-choices"=>r.choices=(0..32).map(|i|Choice::new(&format!("deny-{i}"),&format!("Deny response {i}"),Kind::Deny)).collect(),
        "disabled"=>r.choices[1].disabled=Some("session grants are off for this project".into()),
        _=>{}
    }
    r
}
fn prepare(name: &str) -> PermissionReview {
    let mut review = PermissionReview::new(descriptor(name)).expect("valid permission fixture");
    match name {
        "details" | "narrow-details" | "short" | "long-details" => press(&mut review, KeyCode::Tab),
        "chosen" | "narrow" => press(&mut review, KeyCode::Char('1')),
        "disabled" => press(&mut review, KeyCode::Char('2')),
        "paste-ignored" => review.update(&Event::Paste("1\nAllow\r".into())),
        "search" => {
            review.update(&Event::Key(KeyEvent::new(
                KeyCode::Char('f'),
                KeyModifiers::CONTROL,
            )));
            for c in "zone".chars() {
                press(&mut review, KeyCode::Char(c));
            }
            review.update(&Event::Paste("1\r".into()));
        }
        "scope-check" | "narrow-scope" | "long-scope" | "scope-back" | "scope-sending"
        | "allowed-session" | "changed-scope" | "scope-destructive" => {
            press(&mut review, KeyCode::Char('2'));
            press(&mut review, KeyCode::Enter);
            if name == "scope-back" {
                press(&mut review, KeyCode::Esc);
            }
            if matches!(name, "scope-sending" | "allowed-session") {
                press(&mut review, KeyCode::Char('2'));
                press(&mut review, KeyCode::Enter);
            }
        }
        "armed" | "disarmed" | "arm-expired" => {
            press(&mut review, KeyCode::Char('1'));
            press(&mut review, KeyCode::Enter);
            if name == "disarmed" {
                press(&mut review, KeyCode::Down);
            }
            if name == "arm-expired" {
                review.tick(Duration::from_secs(3));
            }
        }
        "sending" | "allowed" | "failed" | "retry" | "no-ack" | "delayed-ack" | "foreign-ack"
        | "stale-ack" | "expired-sending" | "draft-restored" | "card-resumed" => {
            press(&mut review, KeyCode::Char('1'));
            press(&mut review, KeyCode::Enter);
        }
        "denied" | "skipped" => {
            press(
                &mut review,
                KeyCode::Char(if name == "denied" { '3' } else { '4' }),
            );
            press(&mut review, KeyCode::Enter);
        }
        "refused" => {
            press(&mut review, KeyCode::Char('2'));
            press(&mut review, KeyCode::Enter);
            press(&mut review, KeyCode::Down);
            press(&mut review, KeyCode::Enter);
        }
        "dismissed" => press(&mut review, KeyCode::Esc),
        _ => {}
    }
    if matches!(name, "expired" | "expired-sending") {
        review.expire("expired before host applied it · nothing was allowed");
    }
    if name == "withdrawn" {
        review.withdraw("acme no longer needs this edit");
    }
    if matches!(name, "changed" | "changed-scope") {
        let mut r = request();
        r.revision += 1;
        r.choices[1].scope = "edit · src/** · until you quit".into();
        r.choices[1].detail = "edits in src/".into();
        review
            .replace(r, "acme widened the session scope")
            .expect("higher fixture revision");
    }
    review
}
impl PermissionDemo {
    pub(super) fn new(fixture: usize) -> Self {
        let name = super::scenarios::entries(super::Component::PermissionReview)[fixture].id;
        let mut chat = Chat::new();
        chat.set_input("and bump the app version");
        chat.set_footer("acme-api#276 ~chat");
        for _ in 0..7 {
            chat.update(&key(KeyCode::Left));
        }
        chat.push_record(Turn::new(
            "permission-turn",
            Role::Assistant,
            "acme",
            "I'll make the digest follow each user's timezone.",
        ));
        if matches!(name, "linked-card" | "card-resumed" | "covered-later") {
            add_card(&mut chat, name);
        }
        if name == "anchor-restored" {
            chat.jump(Anchor {
                message: "permission-turn".into(),
                part: "body".into(),
                offset: 0,
            });
        }
        if name == "waiting-open" {
            chat.update(&key(KeyCode::Char('d')));
        }
        if name != "covered-later" {
            chat.push_review(Review::Permission(Box::new(prepare(name))))
                .expect("fixture review identity");
        }
        if matches!(
            name,
            "mixed-queue" | "next-request" | "queue-reordered" | "queue-withdrawn" | "withdrawn"
        ) {
            enqueue(&mut chat, name);
        }
        let mut demo = Self {
            chat,
            pending: None,
            stale: None,
            fixture,
            elapsed: Duration::ZERO,
            delayed: name == "delayed-ack",
            stream: name == "streaming",
            feedback: String::new(),
        };
        demo.drain();
        demo.prepare_reply(name);
        demo.chat.tick(Duration::ZERO);
        demo
    }
    fn prepare_reply(&mut self, name: &str) {
        match name {
            "allowed" | "allowed-session" | "denied" | "skipped" | "draft-restored"
            | "card-resumed" => self.reply(Reply::Applied),
            "failed" | "retry" => {
                self.reply(Reply::Failed("the file is locked by a build".into()));
                if name == "retry" {
                    self.chat.update(&key(KeyCode::Char('r')));
                    self.drain();
                }
            }
            "refused" => self.reply(Reply::Refused(
                "session grants are off for this project".into(),
            )),
            "no-ack" => self.chat.tick(Duration::from_secs(14)),
            "foreign-ack" => {
                let mut f = prepare("sending");
                if let Some(permission_review::Intent::Respond(r)) = f.take_intents().pop()
                    && let Some(Review::Permission(panel)) = self.chat.reviews_mut().active_mut()
                {
                    self.feedback = format!(
                        "foreign acknowledgement rejected: {}",
                        !panel.acknowledge(&r.ticket, Reply::Applied)
                    );
                }
            }
            "stale-ack" => {
                if let Some(r) = self.pending.take() {
                    self.stale = Some(r.ticket);
                }
                self.replace();
                self.stale_reply();
            }
            "next-request" => {
                self.chat.update(&key(KeyCode::Char('1')));
                self.chat.update(&key(KeyCode::Enter));
                self.drain();
                self.reply(Reply::Applied);
            }
            _ => {}
        }
    }
    fn drain(&mut self) {
        for intent in self.chat.reviews_mut().take_intents() {
            if let Intent::Permission(permission_review::Intent::Respond(response)) = intent {
                self.pending = Some(response);
                self.elapsed = Duration::ZERO;
            } else if let Intent::Question(crate::question_form::Intent::Submit(response)) = intent
                && let Some(Review::Questions(form)) = self.chat.reviews_mut().get_mut(&response.id)
            {
                form.acknowledge(&response.ticket, crate::question_form::Result::Accepted);
            }
        }
    }
    fn reply(&mut self, result: Reply) {
        if let Some(response) = self.pending.take()
            && let Some(Review::Permission(panel)) = self.chat.reviews_mut().get_mut(&response.id)
        {
            panel.acknowledge(&response.ticket, result);
            self.stale = Some(response.ticket);
        }
        self.chat.tick(Duration::ZERO);
    }
    fn replace(&mut self) {
        if let Some(Review::Permission(panel)) = self.chat.reviews_mut().active_mut() {
            let mut r = panel.request().clone();
            r.revision += 1;
            r.choices[1].scope = "edit · src/** · until you quit".into();
            panel
                .replace(r, "acme changed the operation")
                .expect("new material revision");
        }
    }
    fn stale_reply(&mut self) {
        if let Some(ticket) = &self.stale
            && let Some(Review::Permission(panel)) = self.chat.reviews_mut().active_mut()
        {
            self.feedback = format!(
                "stale acknowledgement rejected: {}",
                !panel.acknowledge(ticket, Reply::Applied)
            );
        }
    }
    pub(super) fn update(&mut self, event: &Event) -> bool {
        if let Event::Key(k) = event
            && k.kind == KeyEventKind::Press
        {
            match k.code {
                KeyCode::F(2) => {
                    *self = Self::new(
                        (self.fixture + 1)
                            % super::scenarios::entries(super::Component::PermissionReview).len(),
                    );
                    return true;
                }
                KeyCode::F(5) => {
                    self.reply(Reply::Applied);
                    return true;
                }
                KeyCode::F(6) => {
                    self.reply(Reply::Failed("simulated host application failure".into()));
                    return true;
                }
                KeyCode::F(7) => {
                    self.reply(Reply::Refused("host refused this choice".into()));
                    return true;
                }
                KeyCode::F(8) => {
                    if let Some(Review::Permission(panel)) = self.chat.reviews_mut().active_mut() {
                        panel.expire("host stopped waiting");
                    }
                    return true;
                }
                KeyCode::F(9) => {
                    self.replace();
                    return true;
                }
                KeyCode::F(12) => {
                    self.stale_reply();
                    return true;
                }
                KeyCode::F(4) => return false,
                KeyCode::Char('c' | 't') if k.modifiers == KeyModifiers::CONTROL => return false,
                KeyCode::Esc
                    if self.chat.reviews().active().is_none()
                        && self.chat.reviews().waiting_count() == 0 =>
                {
                    return false;
                }
                _ => {}
            }
        }
        self.chat.update(event);
        self.drain();
        true
    }
    pub(super) fn tick(&mut self, elapsed: Duration) {
        self.elapsed = self.elapsed.saturating_add(elapsed);
        self.chat.tick(elapsed);
        if self.delayed && self.elapsed >= Duration::from_secs(2) && self.pending.is_some() {
            self.reply(Reply::Applied);
            self.delayed = false;
        }
        if self.stream && self.elapsed >= Duration::from_millis(800) {
            self.chat
                .append(" More output arrived while review owns focus.");
            self.elapsed = Duration::ZERO;
        }
        self.drain();
    }
    pub(super) fn render(&mut self, frame: &mut Frame<'_>, area: Rect, p: Presentation) {
        let name = super::scenarios::entries(super::Component::PermissionReview)[self.fixture].id;
        let area = Rect::new(
            area.x,
            area.y,
            area.width,
            area.height.min(match name {
                "short" => 20,
                "tiny" => 12,
                _ => area.height,
            }),
        );
        self.chat.render_docked_with(frame, area, p);
        if !self.feedback.is_empty() && area.height > 8 {
            frame.render_widget(
                ratatui::widgets::Paragraph::new(self.feedback.clone()).style(p.muted()),
                Rect::new(area.x, area.y, area.width, 1),
            );
        }
    }
}
fn enqueue(chat: &mut Chat, name: &str) {
    let mut r = request();
    r.id = "command-review".into();
    r.title = "run a command".into();
    r.summary = "Run npm test -- digest".into();
    r.details = Output::Code {
        text: "npm test -- digest".into(),
        first_line: 1,
    };
    chat.push_review(Review::Permission(Box::new(
        PermissionReview::new(r).expect("valid later request"),
    )))
    .expect("unique later review");
    chat.push_review(Review::Questions(Box::new(
        crate::question_form::QuestionForm::new(super::reviews::question_request())
            .expect("valid questions"),
    )))
    .expect("unique mixed request");
    if name == "queue-reordered" {
        chat.reviews_mut()
            .reorder(&["digest-edit", "digest-rollout", "command-review"])
            .expect("complete host reorder");
    }
    if name == "queue-withdrawn" {
        chat.reviews_mut().withdraw("command-review");
    }
}
fn add_card(chat: &mut Chat, name: &str) {
    let mut call = Call::new("edit-call", "edit", "src/digest/settings.ts");
    call.state = if name == "linked-card" {
        ToolState::Waiting("waiting for your approval · answer below".into())
    } else {
        ToolState::Completed
    };
    call.summary = if name == "covered-later" {
        "applied · allowed this session".into()
    } else {
        "applied · +6 -2".into()
    };
    let mut turn = Turn::new("permission-card", Role::Assistant, "acme", "");
    turn.parts = vec![Part::new("tool", Content::ToolCard(Box::new(call)))];
    chat.push_record(turn);
}
