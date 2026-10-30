//! Deterministic Session Browser host with retained Chat instances and no external effects.

use crate::{
    chat::{Anchor, Chat, Intent as ChatIntent, Role, Turn},
    commands::{Command, Commands},
    event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    picker::{Choice, Intent as Pick, PickerView},
    session_browser::{
        Discovery, Intent, Kind, Offered, Outcome, Request, Session, SessionBrowser, Snapshot,
        Status, Tone,
    },
    theme::Presentation,
};
use ratatui::{Frame, layout::Rect};
use std::{collections::HashMap, fmt::Write, time::Duration};

pub(super) const NAMES: &[&str] = &[
    "main",
    "current",
    "loading",
    "empty",
    "load-failed",
    "search",
    "search-all",
    "all-results",
    "no-matches",
    "refreshed",
    "removed",
    "long-list",
    "actions",
    "unavailable",
    "rename",
    "rename-pending",
    "rename-refused",
    "forked",
    "move",
    "pinned",
    "action-failed",
    "delete-confirm",
    "delete-armed",
    "deleting",
    "deleted",
    "delete-failed",
    "last-deleted",
    "narrow",
    "narrow-search",
    "narrow-delete",
    "medium",
    "ascii",
    "ansi16",
    "no-color",
    "light",
    "no-color-pending",
    "before-chat",
    "returning",
    "switched",
    "switch-back",
    "open-failed",
    "paging",
    "page-failed",
    "page-retry",
    "search-failed",
    "search-retry",
    "stale-search",
    "foreign-search",
    "stale-action",
    "foreign-action",
    "duplicate-result",
    "material-changed",
    "action-removed",
    "concurrent",
    "pending-return",
    "delayed",
    "no-ack",
    "long-title",
    "long-preview",
    "long-delete",
    "long-destinations",
    "rename-limit",
    "paste-isolated",
    "reduced-motion",
    "tiny",
    "short",
    "streaming",
    "rename-replaced",
    "delete-expired",
    "delete-disarmed",
    "palette",
    "queued-drafts",
];
fn key(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
}
pub(super) fn snapshot() -> Snapshot {
    let facts = [
        ("digest", "Digest timezone", "pinned", "done", "20:01"),
        ("reindex", "Search reindex", "recent", "needs you", "now"),
        ("retries", "Stuck retries", "recent", "working", "3m"),
        ("backoff", "try backoff", "recent", "", "1h"),
        ("webhook", "Webhook docs", "recent", "", "2h"),
        ("cache", "Cache spike", "recent", "failed", "5h"),
        ("invoice", "Invoice export", "older", "", "Tue"),
        ("onboarding", "Onboarding copy", "older", "", "Mon"),
    ];
    let rows=facts.into_iter().enumerate().map(|(i,(id,title,group,state,age))|{
        let mut r=Session::new(id,title);r.revision=1;r.group=group.into();r.project=if i<2{"acme-api"}else{"acme-worker"}.into();
        r.reference=if i<2{"#281"}else{"#88"}.into();r.state=state.into();r.age=age.into();
        r.tone=match state{"done"=>Tone::Done,"working"=>Tone::Working,"needs you"=>Tone::Attention,"failed"=>Tone::Failed,_=>Tone::Normal};
        r.pinned=i==0;r.parent=(id=="backoff").then(||"retries".into());
        r.activity=if i==2{"run grep 409 logs/worker.log"}else{"3 files changed"}.into();
        r.messages=if id=="backoff"{"retry storm"}else if i==2{"digest job retry storm"}else{"conversation messages"}.into();
        r.preview="acme · 14:02\nThe 409s come from two workers claiming the same job.\nI'm adding a lease check before retrying.\nrun grep 409 logs/worker.log · running".into();
        r.actions=vec![Offered::new(Kind::Open,"open"),Offered::new(Kind::Rename,"rename"),Offered::new(Kind::Fork,"fork"),Offered::new(Kind::Move,"move to"),Offered::new(Kind::Pin,if r.pinned{"unpin"}else{"pin"}),Offered::new(Kind::Delete,"delete")];
        r.actions[5].confirmation=format!("Delete '{title}' in {}?\n38 messages and 1 fork. This can't be undone.",r.project);
        r.destinations=vec![Choice::new("api","acme-api"),Choice::new("web","acme-web"),Choice::new("worker","acme-worker")];r.destinations[2].disabled=Some("already here".into());r
    }).collect();
    let mut s = Snapshot::new(1, "all projects", rows);
    s.search_all = true;
    s
}
pub(super) struct SessionDemo {
    browser: SessionBrowser,
    chats: HashMap<String, Chat>,
    current: String,
    pending: Vec<Request>,
    discoveries: Vec<Discovery>,
    stale: Option<Request>,
    stale_search: Option<Discovery>,
    fixture: usize,
    elapsed: Duration,
    delayed: bool,
    stream: bool,
    feedback: String,
    palette: Option<Commands>,
}
impl SessionDemo {
    pub(super) fn new(fixture: usize) -> Self {
        let name = NAMES[fixture % NAMES.len()];
        let mut s = snapshot();
        match name {
            "loading" => {
                s.rows.clear();
                s.status = Status::Loading;
            }
            "empty" => {
                s.rows.clear();
                s.search_all = false;
            }
            "load-failed" => s.status = Status::Failed("index locked by another window".into()),
            "long-list" | "paging" | "page-failed" | "page-retry" => {
                for i in 0..140 {
                    let mut r = s.rows[4].clone();
                    r.id = format!("session-{i}");
                    r.title = format!("Conversation {i}");
                    s.rows.push(r);
                }
                s.more = Some("page-2".into());
            }
            "unavailable" => {
                s.rows[1].actions[3].disabled = Some("only one project".into());
                s.rows[1].actions[5].disabled = Some("not while open in Chat".into());
            }
            "last-deleted" => {
                s.rows = vec![s.rows[4].clone()];
            }
            "long-title" => s.rows[1].title = "東京 long session title e\u{301} 🧑‍💻 ".repeat(20),
            "long-preview" => {
                s.rows[1].preview.clear();
                for i in 0..80 {
                    let _ = writeln!(s.rows[1].preview, "preview line {i}");
                }
            }
            "long-delete" => {
                s.rows[1].actions[5].confirmation =
                    "Delete this conversation and every supplied message? ".repeat(50);
            }
            "long-destinations" => {
                s.rows[1].destinations = (0..80)
                    .map(|i| Choice::new(format!("project-{i}"), format!("Acme project {i}")))
                    .collect();
            }
            "rename-limit" => s.rows[1].rename_limit = 8,
            _ => {}
        }
        let chats = Self::chats(&s, name);
        let mut demo = Self {
            browser: SessionBrowser::new(s, "reindex").expect("valid session fixture"),
            chats,
            current: "reindex".into(),
            pending: vec![],
            discoveries: vec![],
            stale: None,
            stale_search: None,
            fixture,
            elapsed: Duration::ZERO,
            delayed: false,
            stream: name == "streaming",
            feedback: String::new(),
            palette: None,
        };
        demo.prepare(name);
        demo
    }
    fn chats(s: &Snapshot, name: &str) -> HashMap<String, Chat> {
        let mut chats = HashMap::new();
        for row in s
            .rows
            .iter()
            .filter(|row| row.id == "reindex" || name == "queued-drafts" && row.id == "retries")
        {
            let mut chat = Chat::new();
            chat.set_sessions_available(true);
            chat.set_footer(&format!("{} {} ~chat", row.project, row.reference));
            if name == "queued-drafts" && matches!(row.id.as_str(), "reindex" | "retries") {
                chat.composer_mut().set_working(true);
                chat.set_input(&format!("Queued follow-up for {}", row.title));
                chat.composer_mut().update(&key(KeyCode::Enter));
                chat.composer_mut().set_working(false);
                chat.set_input(&format!("Draft for {}", row.title));
            }
            for i in 0..18 {
                chat.push_record(Turn::new(
                    &format!("turn-{i}"),
                    Role::Assistant,
                    "acme",
                    &format!(
                        "{} conversation line {i}. Keep the reading position.",
                        row.title
                    ),
                ));
            }
            if row.id == "reindex" {
                chat.set_input("and then open the PR");
                for _ in 0..6 {
                    chat.composer_mut().update(&key(KeyCode::Left));
                }
                chat.jump(Anchor {
                    message: "turn-2".into(),
                    part: "body".into(),
                    offset: 0,
                });
            }
            chats.insert(row.id.clone(), chat);
        }
        if !chats.contains_key("reindex") {
            let mut chat = Chat::new();
            chat.set_input("and then open the PR");
            chat.set_sessions_available(true);
            chats.insert("reindex".into(), chat);
        }
        chats
    }
    fn press(&mut self, code: KeyCode) {
        self.browser.update(&key(code));
        self.collect();
    }
    fn select(&mut self, id: &str) {
        self.press(KeyCode::Home);
        for _ in 0..4096 {
            if self.browser.selected() == Some(id) {
                break;
            }
            let before = self.browser.selected().map(str::to_owned);
            self.press(KeyCode::Down);
            if before.as_deref() == self.browser.selected() {
                break;
            }
        }
    }
    fn query(&mut self, query: &str) {
        self.press(KeyCode::Char('/'));
        self.browser.update(&Event::Paste(query.into()));
        self.collect();
    }
    fn remote_request(&mut self) {
        self.query("dig");
        self.press(KeyCode::End);
        self.press(KeyCode::Enter);
    }
    #[expect(
        clippy::too_many_lines,
        reason = "Named gallery states are prepared through public input and actual simulated acknowledgements"
    )]
    fn prepare(&mut self, name: &str) {
        match name {
            "search" | "narrow-search" => self.query("ret"),
            "no-matches" => self.query("zzz"),
            "search-all" | "all-results" | "search-failed" | "search-retry" | "stale-search"
            | "foreign-search" => {
                self.remote_request();
                match name {
                    "all-results" => self.reply(true),
                    "search-failed" | "search-retry" => {
                        self.reply(false);
                        if name == "search-retry" {
                            self.press(KeyCode::Enter);
                        }
                    }
                    "stale-search" => {
                        self.stale_search = self.discoveries.first().cloned();
                        self.browser.update(&Event::Paste("new".into()));
                        self.collect();
                        self.reject_stale();
                    }
                    "foreign-search" => self.reject_foreign(),
                    _ => {}
                }
            }
            "refreshed" => {
                self.select("retries");
                self.refresh(false);
            }
            "removed" => {
                self.select("webhook");
                self.refresh(true);
            }
            "actions" | "unavailable" => self.press(KeyCode::Char('a')),
            "move" | "long-destinations" => self.press(KeyCode::Char('m')),
            "rename" | "rename-limit" | "paste-isolated" => {
                self.press(KeyCode::Char('r'));
                if name == "rename-limit" {
                    self.browser
                        .update(&Event::Paste("over the limit title".into()));
                }
            }
            "rename-pending" | "rename-refused" | "rename-replaced" => {
                self.press(KeyCode::Char('r'));
                self.browser
                    .update(&Event::Paste("Worker 409 retries".into()));
                if name != "rename-replaced" {
                    self.press(KeyCode::Enter);
                }
                if name == "rename-refused" {
                    self.reply(false);
                }
                if name == "rename-replaced" {
                    self.refresh(false);
                }
            }
            "forked" | "action-failed" | "pinned" | "delayed" | "no-ack" | "reduced-motion"
            | "concurrent" | "material-changed" | "action-removed" | "pending-return"
            | "foreign-action" | "stale-action" | "duplicate-result" => {
                self.select("retries");
                self.press(KeyCode::Char(if name == "pinned" { 'p' } else { 'f' }));
                match name {
                    "forked" | "pinned" => self.reply(true),
                    "action-failed" => self.reply(false),
                    "delayed" => self.delayed = true,
                    "no-ack" | "reduced-motion" => self.browser.tick(Duration::from_secs(14)),
                    "concurrent" => {
                        self.select("webhook");
                        self.press(KeyCode::Char('p'));
                    }
                    "material-changed" | "stale-action" => {
                        self.stale = self.pending.first().cloned();
                        self.refresh(false);
                        self.reject_stale();
                    }
                    "action-removed" => self.refresh(true),
                    "pending-return" => self.press(KeyCode::Esc),
                    "foreign-action" => self.reject_foreign(),
                    "duplicate-result" => {
                        self.reply(true);
                        self.reject_stale();
                    }
                    _ => {}
                }
            }
            "delete-confirm" | "delete-armed" | "deleting" | "deleted" | "delete-failed"
            | "last-deleted" | "narrow-delete" | "no-color-pending" | "long-delete"
            | "delete-expired" | "delete-disarmed" => {
                if name != "long-delete" {
                    self.select("webhook");
                }
                self.press(KeyCode::Char('d'));
                if matches!(
                    name,
                    "delete-armed"
                        | "deleting"
                        | "deleted"
                        | "delete-failed"
                        | "last-deleted"
                        | "no-color-pending"
                        | "delete-expired"
                        | "delete-disarmed"
                ) {
                    self.press(KeyCode::Char('2'));
                    self.press(KeyCode::Enter);
                    match name {
                        "delete-armed" => {}
                        "delete-expired" => self.browser.tick(Duration::from_secs(4)),
                        "delete-disarmed" => self.press(KeyCode::Up),
                        _ => {
                            self.press(KeyCode::Enter);
                            if matches!(name, "deleted" | "last-deleted") {
                                self.reply(true);
                            }
                            if name == "delete-failed" {
                                self.reply(false);
                            }
                        }
                    }
                }
            }
            "paging" | "page-failed" | "page-retry" => {
                self.press(KeyCode::End);
                if name != "paging" {
                    self.reply(false);
                    if name == "page-retry" {
                        self.press(KeyCode::Char('r'));
                    }
                }
            }
            "before-chat" | "returning" => self.press(KeyCode::Esc),
            "switched" | "switch-back" | "open-failed" => {
                self.select("retries");
                self.press(KeyCode::Enter);
                self.reply(name != "open-failed");
                if name == "switch-back" {
                    self.browser
                        .reopen("retries")
                        .expect("valid deterministic session fixture");
                    self.select("reindex");
                    self.press(KeyCode::Enter);
                    self.reply(true);
                }
            }
            "palette" => {
                self.press(KeyCode::Esc);
                self.palette = Some(Self::palette());
            }
            _ => {}
        }
        self.collect();
    }
    fn collect(&mut self) {
        for i in self.browser.take_intents() {
            match i {
                Intent::Action(r) => self.pending.push(r),
                Intent::Discover(r) => self.discoveries.push(r),
                Intent::Returned(id) | Intent::Opened(id) => {
                    self.current = id;
                }
            }
        }
    }
    fn current_snapshot(&self) -> Snapshot {
        let mut s = self.browser.snapshot().clone();
        s.revision += 1;
        s
    }
    #[expect(
        clippy::too_many_lines,
        reason = "The deterministic host handles typed row and discovery outcomes without external effects"
    )]
    fn reply(&mut self, success: bool) {
        if !self.pending.is_empty() {
            let r = self.pending.remove(0);
            let outcome = if !success {
                Outcome::Failed("the session is still running · try when idle".into())
            } else if r.kind == Kind::Open {
                Outcome::Opened
            } else {
                let mut s = self.current_snapshot();
                let at = s.rows.iter().position(|row| row.id == r.id);
                if let Some(at) = at {
                    match r.kind {
                        Kind::Delete => {
                            s.rows.remove(at);
                        }
                        Kind::Rename => {
                            s.rows[at].title.clone_from(&r.value);
                            s.rows[at].revision += 1;
                        }
                        Kind::Move => {
                            s.rows[at].project.clone_from(&r.value);
                            s.rows[at].revision += 1;
                        }
                        Kind::Pin => {
                            let mut row = s.rows.remove(at);
                            row.pinned = r.value == "true";
                            row.group = if row.pinned { "pinned" } else { "recent" }.into();
                            row.revision += 1;
                            row.actions
                                .iter_mut()
                                .filter(|a| a.kind == Kind::Pin)
                                .for_each(|a| {
                                    a.label = if row.pinned { "unpin" } else { "pin" }.into();
                                });
                            s.rows.insert(0, row);
                        }
                        Kind::Fork => {
                            let mut fork = s.rows[at].clone();
                            fork.id = format!("{}-fork-{}", r.id, s.revision);
                            fork.parent = Some(r.id.clone());
                            fork.title = format!("{} · fork", fork.title);
                            fork.age = "now".into();
                            s.rows.insert(at + 1, fork);
                        }
                        Kind::Open => {}
                    }
                }
                if r.kind == Kind::Fork {
                    let id = format!("{}-fork-{}", r.id, s.revision);
                    Outcome::Forked { snapshot: s, id }
                } else {
                    Outcome::Applied(s)
                }
            };
            let _ = self.browser.complete(&r, outcome);
            self.stale = Some(r);
            self.collect();
            if self.browser.closed()
                && !self.chats.contains_key(&self.current)
                && let Some(row) = self
                    .browser
                    .snapshot()
                    .rows
                    .iter()
                    .find(|r| r.id == self.current)
            {
                let mut chat = Chat::new();
                chat.set_sessions_available(true);
                chat.set_footer(&format!("{} {} ~chat", row.project, row.reference));
                for i in 0..18 {
                    chat.push_record(Turn::new(
                        &format!("turn-{i}"),
                        Role::Assistant,
                        "acme",
                        &format!(
                            "{} conversation line {i}. Keep the reading position.",
                            row.title
                        ),
                    ));
                }
                self.chats.insert(row.id.clone(), chat);
            }
        } else if !self.discoveries.is_empty() {
            let r = self.discoveries.remove(0);
            let result = if success {
                let mut s = self.current_snapshot();
                s.status = Status::Ready;
                if r.search && !r.cursor.is_empty() {
                    let mut row = snapshot().rows[0].clone();
                    row.id = format!("archive-page-{}", s.revision);
                    row.title = "Digest archived continuation".into();
                    row.group = "archived".into();
                    s.rows.push(row);
                    s.more = None;
                } else if r.search {
                    s.more = Some("archive-page-2".into());
                    s.rows.retain(|row| {
                        row.title.to_lowercase().contains(&r.query.to_lowercase())
                            || row
                                .messages
                                .to_lowercase()
                                .contains(&r.query.to_lowercase())
                    });
                    for (id, title) in [
                        ("archive-1", "Digest retries draft"),
                        ("archive-2", "Redigest weekly job"),
                    ] {
                        let mut row = snapshot().rows[0].clone();
                        row.id = id.into();
                        row.title = title.into();
                        row.group = "archived".into();
                        row.age = "Sep 2".into();
                        s.rows.push(row);
                    }
                } else {
                    let mut row = snapshot().rows[0].clone();
                    row.id = format!("loaded-{}", s.revision);
                    row.title = "More conversations loaded".into();
                    s.rows.push(row);
                    s.more = None;
                }
                Ok(s)
            } else {
                Err("index locked · explicit retry".into())
            };
            let _ = self.browser.complete_discovery(&r, result);
            self.stale_search = Some(r);
            self.collect();
        }
    }
    fn refresh(&mut self, remove: bool) {
        let mut s = self.current_snapshot();
        if let Some(id) = self.browser.selected() {
            if remove {
                s.rows.retain(|r| r.id != id);
            } else if let Some(row) = s.rows.iter_mut().find(|r| r.id == id) {
                row.revision += 1;
                row.activity = "updated by host".into();
            }
        }
        if !remove {
            let mut row = snapshot().rows[0].clone();
            row.id = format!("new-{}", s.revision);
            row.title = "Release notes".into();
            row.group = "recent".into();
            s.rows.insert(0, row);
        }
        let _ = self.browser.replace(s);
        self.collect();
    }
    fn reject_stale(&mut self) {
        let accepted = if let Some(r) = &self.stale {
            self.browser
                .complete(r, Outcome::Failed("late result".into()))
                .unwrap_or(false)
        } else if let Some(r) = &self.stale_search {
            self.browser
                .complete_discovery(r, Ok(self.current_snapshot()))
                .unwrap_or(false)
        } else {
            false
        };
        self.feedback = format!("stale result rejected: {}", !accepted);
        self.browser.set_notice(&self.feedback);
    }
    fn reject_foreign(&mut self) {
        if let Some(request) = self.discoveries.first() {
            let mut foreign = SessionBrowser::new(snapshot(), "digest").expect("valid fixture");
            foreign.update(&key(KeyCode::Char('/')));
            foreign.update(&Event::Paste(request.query.clone()));
            foreign.update(&key(KeyCode::End));
            foreign.update(&key(KeyCode::Enter));
            let remote = foreign.take_intents().into_iter().find_map(|i| {
                if let Intent::Discover(r) = i {
                    Some(r)
                } else {
                    None
                }
            });
            let accepted = remote.is_some_and(|r| {
                self.browser
                    .complete_discovery(&r, Err("foreign result".into()))
                    .unwrap_or(false)
            });
            self.feedback = format!("foreign result rejected: {}", !accepted);
            self.browser.set_notice(&self.feedback);
            return;
        }
        let mut foreign =
            SessionBrowser::new(snapshot(), "digest").expect("valid deterministic session fixture");
        foreign.update(&key(KeyCode::Down));
        foreign.update(&key(KeyCode::Char('f')));
        let r = foreign.take_intents().into_iter().find_map(|i| {
            if let Intent::Action(r) = i {
                Some(r)
            } else {
                None
            }
        });
        let accepted = r.is_some_and(|r| {
            self.browser
                .complete(&r, Outcome::Failed("foreign result".into()))
                .unwrap_or(false)
        });
        self.feedback = format!("foreign result rejected: {}", !accepted);
        self.browser.set_notice(&self.feedback);
    }
    fn palette() -> Commands {
        Commands::new(vec![Command {
            id: "sessions".into(),
            label: "Sessions".into(),
            category: "Navigation".into(),
            description: "Open the session browser".into(),
            aliases: vec!["sessions".into()],
            disabled: None,
            bindings: vec![],
        }])
        .expect("valid deterministic session fixture")
    }
    pub(super) fn update(&mut self, event: &Event) -> bool {
        if let Event::Key(k) = event
            && k.kind == KeyEventKind::Press
        {
            if k.code == KeyCode::F(2) {
                *self = Self::new((self.fixture + 1) % NAMES.len());
                return true;
            }
            if k.code == KeyCode::F(5) {
                self.reply(true);
                return true;
            }
            if k.code == KeyCode::F(6) || k.code == KeyCode::F(7) {
                self.reply(false);
                return true;
            }
            if k.code == KeyCode::F(8) {
                self.refresh(false);
                return true;
            }
            if k.code == KeyCode::F(9) {
                self.refresh(true);
                return true;
            }
            if k.code == KeyCode::F(12) {
                self.reject_stale();
                return true;
            }
            if k.code == KeyCode::F(4)
                || k.code == KeyCode::F(10)
                || k.modifiers.contains(KeyModifiers::CONTROL)
                    && matches!(k.code, KeyCode::Char('c' | 't'))
            {
                return false;
            }
            if k.code == KeyCode::F(1) && self.browser.closed() {
                self.palette = Some(Self::palette());
                return true;
            }
        }
        if let Some(palette) = &mut self.palette {
            match palette.update(event) {
                Pick::Selected(id) if id == "sessions" => {
                    self.palette = None;
                    if let Some(chat) = self.chats.get_mut(&self.current) {
                        chat.request_sessions();
                    }
                }
                Pick::Cancelled => self.palette = None,
                _ => {}
            }
        } else if self.browser.closed() {
            if matches!(event,Event::Key(k) if k.code==KeyCode::Esc) {
                return false;
            }
            if let Some(chat) = self.chats.get_mut(&self.current) {
                chat.update(event);
            }
        } else {
            self.browser.update(event);
        }
        if let Some(chat) = self.chats.get_mut(&self.current) {
            for intent in chat.take_intents() {
                if matches!(intent, ChatIntent::Sessions(_)) {
                    self.browser
                        .reopen(&self.current)
                        .expect("valid deterministic session fixture");
                }
            }
        }
        self.collect();
        true
    }
    pub(super) fn tick(&mut self, elapsed: Duration) {
        self.elapsed += elapsed;
        self.browser.tick(elapsed);
        if self.delayed && self.elapsed >= Duration::from_secs(2) {
            self.delayed = false;
            self.reply(true);
        }
        if self.stream && self.elapsed >= Duration::from_millis(800) {
            self.elapsed = Duration::ZERO;
            if let Some(chat) = self.chats.get_mut("reindex") {
                let id = format!("stream-{}", chat.message_count());
                chat.push_record(Turn::new(
                    &id,
                    Role::Assistant,
                    "acme",
                    "New output while the browser is open.",
                ));
            }
        }
    }
    pub(super) fn render(&mut self, frame: &mut Frame<'_>, area: Rect, p: Presentation) {
        if self.browser.closed() {
            if let Some(chat) = self.chats.get_mut(&self.current) {
                chat.render_docked_with(frame, area, p);
            }
        } else {
            self.browser.render_with(frame, area, p);
        }
        if let Some(palette) = &mut self.palette {
            palette.render(
                frame,
                area,
                PickerView {
                    title: "commands",
                    presentation: p,
                    ..Default::default()
                },
            );
        }
    }
}
