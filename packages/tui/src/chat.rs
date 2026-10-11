//! Owns a bounded identified transcript and Composer; hosts own all external work.

mod content;
mod interaction;
mod layout;
mod model;
mod syntax;
mod transcript;
mod view;

use crate::{
    composer::{Composer, Identity, Submission},
    editor::Editor,
};
use layout::{BlockId, Cached, Located};
pub use model::{
    Anchor, Change, Content, Conversation, End, Intent, Metrics, Mode, Part, Request, Role, Turn,
    Update, Updated, Welcome,
};
use std::{
    collections::{BTreeSet, HashMap},
    sync::Arc,
    time::Duration,
};

const MESSAGE_LIMIT: usize = 65_536;
const HISTORY_LIMIT: usize = 100;
const PART_LIMIT: usize = 128;

/// Composer actions remain compatible with plain hosts. Drain Chat intents separately.
#[derive(Debug, PartialEq, Eq)]
pub enum Action {
    /// No submission or cancellation.
    None,
    /// Fixed submission snapshot; acknowledge its opaque Composer ticket.
    Submit(Submission),
    /// Request cancellation; the host supplies the stopped reply outcome.
    Cancel,
}

struct Record {
    turn: Turn,
    delivery: Option<(Identity, Option<String>)>,
    shell: Option<(Identity, bool)>,
    fixed: bool,
    omitted: HashMap<String, (usize, usize)>,
    table_omitted: HashMap<String, usize>,
    reloaded: bool,
}
impl Record {
    fn new(mut turn: Turn) -> Self {
        turn.label = model::label(&turn.label);
        turn.timestamp = model::label(&turn.timestamp);
        turn.day = model::label(&turn.day);
        turn.duration = model::label(&turn.duration);
        match &mut turn.end {
            End::Done(s) | End::Stopped(s) | End::Failed(s) => *s = model::label(s),
            End::Live | End::Quiet => {}
        }
        let excess = turn.parts.len().saturating_sub(PART_LIMIT);
        turn.parts.truncate(PART_LIMIT);
        let fixed = turn.role == Role::User;
        let mut result = Self {
            turn,
            delivery: None,
            shell: None,
            fixed,
            omitted: HashMap::new(),
            table_omitted: HashMap::new(),
            reloaded: false,
        };
        if excess > 0 {
            result.omitted.insert(String::new(), (excess, 0));
        }
        result.bound();
        result
    }
    fn bound(&mut self) {
        let mut remaining = MESSAGE_LIMIT;
        let mut cells = 8192;
        for part in &mut self.turn.parts {
            let dropped = part.content.bound_structure(&mut cells);
            if dropped > 0 {
                *self.table_omitted.entry(part.id.clone()).or_default() += dropped;
            }
            let original = part.content.bound(remaining);
            part.content.sanitize();
            let missing = part.content.bound(remaining);
            remaining = remaining.saturating_sub(part.content.bytes());
            if missing.0 > 0 || original.0 > 0 {
                let entry = self.omitted.entry(part.id.clone()).or_default();
                entry.0 += missing.0 + original.0;
                entry.1 += missing.1 + original.1;
            }
        }
    }
}
struct Search {
    editor: Editor,
    saved: Option<Anchor>,
    saved_follow: bool,
    matches: Vec<Match>,
    current: usize,
    wrapped: bool,
    temporary: BTreeSet<(String, String)>,
    cached: HashMap<(String, String), (u64, usize, Vec<Match>)>,
    query: String,
    limited: bool,
}
#[derive(Clone, PartialEq, Eq)]
struct Match {
    block: BlockId,
    start: usize,
    end: usize,
}
struct Selection {
    first: BlockId,
    last: BlockId,
    whole: bool,
    was_following: bool,
}
struct Pending {
    request: Request,
    label: String,
    latest: bool,
}
struct Resync {
    message: String,
    versions: Vec<(String, u64)>,
    end: Option<End>,
}
enum Page {
    Loading(Request),
    Failed(String),
}

/// Full conversation composition. No terminal lease, transport, clock or clipboard I/O.
pub struct Chat {
    messages: Vec<Record>,
    input: Composer,
    status: String,
    footer: String,
    conversation: Conversation,
    serial: u64,
    next_message: u64,
    cache: HashMap<(String, String), Cached>,
    rows: Vec<Located>,
    metrics: Metrics,
    scroll: usize,
    height: usize,
    width: u16,
    follow: bool,
    anchor: Option<Anchor>,
    unread: BTreeSet<String>,
    earlier: usize,
    newer: usize,
    page: Option<Page>,
    loaded_boundary: Option<String>,
    home: Option<Anchor>,
    search: Option<Search>,
    selection: Option<Selection>,
    expanded: BTreeSet<(String, String)>,
    now: Duration,
    leader: Option<Duration>,
    notice: Option<(String, Duration)>,
    intents: Vec<Intent>,
    pending: Option<Pending>,
    resync: HashMap<u64, Resync>,
    timestamps: bool,
    retry: bool,
    actions: &'static str,
    welcome: Welcome,
}
impl Default for Chat {
    fn default() -> Self {
        Self {
            messages: vec![],
            input: Composer::new(MESSAGE_LIMIT),
            status: String::new(),
            footer: String::new(),
            conversation: Conversation(Arc::new(())),
            serial: 0,
            next_message: 0,
            cache: HashMap::new(),
            rows: vec![],
            metrics: Metrics::default(),
            scroll: 0,
            height: 1,
            width: 80,
            follow: true,
            anchor: None,
            unread: BTreeSet::new(),
            earlier: 0,
            newer: 0,
            page: None,
            loaded_boundary: None,
            home: None,
            search: None,
            selection: None,
            expanded: BTreeSet::new(),
            now: Duration::ZERO,
            leader: None,
            notice: None,
            intents: vec![],
            pending: None,
            resync: HashMap::new(),
            timestamps: true,
            retry: false,
            actions: "on",
            welcome: Welcome::default(),
        }
    }
}
impl Chat {
    /// Empty conversation with automatic following and a 64 KiB Composer.
    ///
    /// ```
    /// use canonic_tui::chat::{Chat, Change, Role, Turn, Update, Updated};
    /// let mut chat = Chat::new();
    /// chat.push_record(Turn::new("reply", Role::Assistant, "acme", "Hello"));
    /// assert_eq!(chat.apply(Update {
    ///     conversation: chat.conversation(), message: "reply".into(), part: "body".into(),
    ///     expected: 0, revision: 1, change: Change::Append(" world".into()),
    /// }), Updated::Applied);
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
    /// Host-supplied display label only; this never changes execution authority.
    pub fn set_actions_available(&mut self, available: bool) {
        self.actions = if available { "on" } else { "off" };
    }
    /// Identity used to validate addressed part updates.
    #[must_use]
    pub fn conversation(&self) -> Conversation {
        self.conversation.clone()
    }
    /// Reset transcript, requests and view state for another conversation, keeping the draft.
    pub fn reset_transcript(&mut self) {
        let input = std::mem::replace(&mut self.input, Composer::new(MESSAGE_LIMIT));
        *self = Self::default();
        self.input = input;
    }
    /// Inspect draft contents.
    #[must_use]
    pub fn input(&self) -> &str {
        self.input.editor().text()
    }
    /// Explicitly replace only the Composer draft.
    pub fn set_input(&mut self, text: &str) {
        self.input.set_text(text);
    }
    /// Draft lifecycle owner; Composer intents are drained independently.
    pub fn composer_mut(&mut self) -> &mut Composer {
        &mut self.input
    }
    /// Inspect draft lifecycle.
    #[must_use]
    pub fn composer(&self) -> &Composer {
        &self.input
    }
    /// Number of loaded messages, at most 100.
    #[must_use]
    pub fn message_count(&self) -> usize {
        self.messages.len()
    }
    /// Counts outside the bounded loaded window (earlier, newer).
    #[must_use]
    pub fn history_counts(&self) -> (usize, usize) {
        (self.earlier, self.newer)
    }
    /// Inspect a loaded host turn by stable ID.
    #[must_use]
    pub fn turn(&self, id: &str) -> Option<&Turn> {
        self.messages
            .iter()
            .find(|m| m.turn.id == id)
            .map(|m| &m.turn)
    }
    /// Current focus owner.
    #[must_use]
    pub fn mode(&self) -> Mode {
        if self.search.is_some() {
            Mode::Search
        } else if self.selection.is_some() {
            Mode::Select
        } else {
            Mode::Compose
        }
    }
    /// Whether the transcript automatically follows live output.
    #[must_use]
    pub fn following(&self) -> bool {
        self.follow
    }
    /// New/changed messages since manual inspection began.
    #[must_use]
    pub fn unread_count(&self) -> usize {
        self.unread.len()
    }
    /// Logical first visible line, updated by render and navigation.
    #[must_use]
    pub fn viewport_anchor(&self) -> Option<&Anchor> {
        self.anchor.as_ref()
    }
    /// Instance scoped part parsing and layout counters.
    #[must_use]
    pub fn metrics(&self) -> Metrics {
        self.metrics
    }
    /// Search query, empty outside search.
    #[must_use]
    pub fn query(&self) -> &str {
        self.search.as_ref().map_or("", |s| s.editor.text())
    }
    /// (one-based current match, total); zero current means no match.
    #[must_use]
    pub fn search_position(&self) -> (usize, usize) {
        self.search.as_ref().map_or((0, 0), |s| {
            (
                if s.matches.is_empty() {
                    0
                } else {
                    s.current + 1
                },
                s.matches.len(),
            )
        })
    }
    /// Host activity label, bounded and sanitized.
    pub fn set_status(&mut self, status: &str) {
        self.status = model::label(status);
    }
    /// Host session/address metadata on the lower Composer rule.
    pub fn set_footer(&mut self, footer: &str) {
        self.footer = model::label(footer);
    }
    /// Empty session context; no filesystem/session enumeration occurs here.
    pub fn set_welcome(&mut self, mut welcome: Welcome) {
        welcome.title = model::label(&welcome.title);
        welcome.context = model::label(&welcome.context);
        welcome.recent.truncate(10);
        welcome.suggestions.truncate(10);
        for s in welcome.recent.iter_mut().chain(&mut welcome.suggestions) {
            *s = model::label(s);
        }
        self.welcome = welcome;
    }
    /// Enable retry intent only when the host can perform it.
    pub fn set_retry_available(&mut self, available: bool) {
        self.retry = available;
    }
    /// Supply the count of authoritative history preceding the loaded window.
    pub fn set_earlier_count(&mut self, count: usize) {
        self.earlier = count;
    }
    /// Explicit timestamps option. Day separators remain visible.
    pub fn set_timestamps(&mut self, visible: bool) {
        self.timestamps = visible;
    }
    /// Add an identified initial turn. Duplicate/invalid identities are rejected atomically.
    pub fn push_record(&mut self, turn: Turn) -> bool {
        if !valid_turn(&turn) || self.messages.iter().any(|m| m.turn.id == turn.id) {
            return false;
        }
        if !self.follow {
            self.unread.insert(turn.id.clone());
        }
        self.messages.push(Record::new(turn));
        if self.messages.len() > HISTORY_LIMIT {
            self.messages.remove(0);
            self.earlier = self.earlier.saturating_add(1);
        }
        self.prune();
        self.refresh();
        true
    }
    /// Plain compatibility projection with generated stable IDs; add rich host parts through `push_record`.
    pub fn push(&mut self, role: &str, text: &str) {
        self.next_message = self.next_message.saturating_add(1);
        while self
            .messages
            .iter()
            .any(|m| m.turn.id == format!("local-{}", self.next_message))
        {
            self.next_message = self.next_message.saturating_add(1);
        }
        let id = format!("local-{}", self.next_message);
        let speaker = if role == "you" {
            Role::User
        } else if role == "Activity" {
            Role::Note
        } else {
            Role::Assistant
        };
        let mut turn = Turn::new(&id, speaker, role, text);
        // Compatibility plain turns do not claim a host completion outcome.
        turn.end = End::Quiet;
        self.push_record(turn);
    }
    /// Plain turn with a supplied display timestamp.
    pub fn push_turn(&mut self, role: &str, text: &str, timestamp: &str) {
        self.push(role, text);
        if let Some(message) = self.messages.last_mut() {
            message.turn.timestamp = model::label(timestamp);
        }
    }
    /// Project an immutable submission; a later editor change cannot change this turn.
    pub fn push_submission(&mut self, submission: &Submission, timestamp: &str) {
        let shell = submission.mode == crate::composer::InputMode::Shell;
        self.push(if shell { "shell" } else { "you" }, &submission.text);
        if let Some(message) = self.messages.last_mut() {
            if shell {
                message.turn.label = model::label(&format!("$ {}", submission.text));
                message.turn.parts[0].content = Content::Code {
                    text: String::new(),
                    language: "sh".into(),
                    file: String::new(),
                    line: None,
                };
                message.shell = Some((submission.ticket.clone(), false));
            }
            message.turn.timestamp = model::label(timestamp);
            message.fixed = true;
            message.delivery = Some((submission.ticket.clone(), None));
            self.cache.remove(&(message.turn.id.clone(), "body".into()));
        }
    }
    /// Update delivery only for an accepted Composer ticket. Rejection preserves the fresh draft.
    pub fn acknowledge_submission(&mut self, ticket: &Identity, result: Result<(), &str>) -> bool {
        if !self.input.acknowledge(ticket, result) {
            return false;
        }
        if let Some(message) = self
            .messages
            .iter_mut()
            .find(|m| m.delivery.as_ref().is_some_and(|(id, _)| id == ticket))
        {
            message.delivery = result
                .err()
                .map(|reason| (ticket.clone(), Some(model::label(reason))));
        }
        true
    }
    /// Replace only the addressed shell output, retaining its 20-line folded preview.
    pub fn set_shell_output(&mut self, ticket: &Identity, output: &str) -> bool {
        let Some(message) = self
            .messages
            .iter_mut()
            .find(|m| m.shell.as_ref().is_some_and(|(id, _)| id == ticket))
        else {
            return false;
        };
        if let Some(text) = message.turn.parts[0].content.text_mut() {
            *text = output.into();
        }
        message.turn.parts[0].revision = message.turn.parts[0].revision.saturating_add(1);
        message.omitted.remove("body");
        message.bound();
        self.refresh();
        true
    }
    /// Toggle a shell preview without executing it.
    pub fn toggle_shell_output(&mut self, ticket: &Identity) -> bool {
        let Some(message) = self
            .messages
            .iter_mut()
            .find(|m| m.shell.as_ref().is_some_and(|(id, _)| id == ticket))
        else {
            return false;
        };
        let Some((_, expanded)) = &mut message.shell else {
            return false;
        };
        *expanded = !*expanded;
        self.cache.remove(&(message.turn.id.clone(), "body".into()));
        self.refresh();
        true
    }
    /// Compatibility append skips all fixed submitted turns, even after acceptance.
    pub fn append(&mut self, text: &str) {
        let Some(message) = self
            .messages
            .iter()
            .rev()
            .find(|m| !m.fixed && m.turn.timestamp.is_empty())
        else {
            return;
        };
        let Some(part) = message.turn.parts.last() else {
            return;
        };
        let update = Update {
            conversation: self.conversation(),
            message: message.turn.id.clone(),
            part: part.id.clone(),
            expected: part.revision,
            revision: part.revision.saturating_add(1),
            change: Change::Append(text.into()),
        };
        self.apply(update);
    }
    /// Apply an exact part revision. Foreign/stale data cannot alter another turn.
    pub fn apply(&mut self, update: Update) -> Updated {
        if update.conversation != self.conversation {
            return Updated::Foreign;
        }
        let target = self.messages.iter_mut().find(|m| {
            m.turn.id == update.message && !m.fixed && matches!(m.turn.end, End::Live | End::Quiet)
        });
        if let Some(message) = target
            && let Some(part) = message.turn.parts.iter_mut().find(|p| {
                p.id == update.part
                    && p.revision == update.expected
                    && update.expected.checked_add(1) == Some(update.revision)
            })
        {
            let accepted = match update.change {
                Change::Replace(content) => {
                    part.content = content;
                    message.omitted.remove(&part.id);
                    true
                }
                Change::Append(text) => part.content.text_mut().is_some_and(|content| {
                    content.push_str(&text);
                    true
                }),
            };
            if accepted {
                part.revision = update.revision;
                message.bound();
                if !self.follow {
                    self.unread.insert(update.message);
                }
                self.refresh();
                return Updated::Applied;
            }
        }
        let request = self.request();
        if self.resync.len() >= PART_LIMIT {
            self.resync.clear();
        }
        let record = self.turn(&update.message);
        let versions = record.map_or_else(Vec::new, |t| {
            t.parts.iter().map(|p| (p.id.clone(), p.revision)).collect()
        });
        let end = record.map(|t| t.end.clone());
        self.resync.insert(
            request.number,
            Resync {
                message: update.message,
                versions,
                end,
            },
        );
        Updated::Resync(request)
    }
    /// Complete the exact resync request with a whole addressed reply snapshot.
    pub fn resynchronize(&mut self, request: &Request, turn: Turn) -> bool {
        if request.conversation != self.conversation || !valid_turn(&turn) {
            return false;
        }
        let Some(pending) = self.resync.get(&request.number) else {
            return false;
        };
        if turn.id != pending.message {
            return false;
        }
        let index = self
            .messages
            .iter()
            .position(|m| m.turn.id == turn.id && !m.fixed);
        if let Some(index) = index {
            let current = &self.messages[index].turn;
            if Some(&current.end) != pending.end.as_ref()
                || (!matches!(current.end, End::Live | End::Quiet) && turn.end != current.end)
                || current
                    .parts
                    .iter()
                    .map(|p| (p.id.clone(), p.revision))
                    .collect::<Vec<_>>()
                    != pending.versions
                || current.parts.iter().any(|p| {
                    turn.parts
                        .iter()
                        .find(|new| new.id == p.id)
                        .is_none_or(|new| new.revision < p.revision)
                })
            {
                return false;
            }
        } else if pending.end.is_some() || self.messages.iter().any(|m| m.turn.id == turn.id) {
            return false;
        }
        self.resync.retain(|_, pending| pending.message != turn.id);
        self.cache.retain(|(message, _), _| message != &turn.id);
        if !self.follow {
            self.unread.insert(turn.id.clone());
        }
        let mut record = Record::new(turn);
        record.reloaded = true;
        if let Some(index) = index {
            self.messages[index] = record;
        } else {
            self.messages.push(record);
            if self.messages.len() > HISTORY_LIMIT {
                self.messages.remove(0);
                self.earlier += 1;
            }
        }
        self.prune();
        self.refresh();
        true
    }
    /// Host outcome for an existing reply; partial content remains intact.
    pub fn end_turn(&mut self, message: &str, mut end: End) -> bool {
        let Some(record) = self
            .messages
            .iter_mut()
            .find(|m| m.turn.id == message && !m.fixed)
        else {
            return false;
        };
        match &mut end {
            End::Done(s) | End::Stopped(s) | End::Failed(s) => *s = model::label(s),
            End::Live | End::Quiet => {}
        }
        if !matches!(record.turn.end, End::Live | End::Quiet) {
            return record.turn.end == end;
        }
        record.turn.end = end;
        true
    }
    /// Finish a host reply with an explicit elapsed label, retaining all partial content.
    pub fn finish_turn(&mut self, message: &str, end: End, duration: &str) -> bool {
        if !self.end_turn(message, end) {
            return false;
        }
        if let Some(record) = self.messages.iter_mut().find(|m| m.turn.id == message) {
            record.turn.duration = model::label(duration);
        }
        true
    }
    /// Drain host work requested by navigation/selection; submission uses `Action`.
    pub fn take_intents(&mut self) -> Vec<Intent> {
        std::mem::take(&mut self.intents)
    }
    /// Acknowledge a copy/open/inspection/retry. Stale completions do nothing.
    pub fn acknowledge(&mut self, request: &Request, result: Result<&str, &str>) -> bool {
        if self.pending.as_ref().is_none_or(|p| &p.request != request) {
            return false;
        }
        if self.pending.as_ref().is_some_and(|p| p.latest) && result.is_ok() {
            return false;
        }
        let Some(pending) = self.pending.take() else {
            return false;
        };
        self.say(&match result {
            Ok("") => pending.label,
            Ok(label) => model::label(label),
            Err(reason) => format!("failed · {}", model::label(reason)),
        });
        true
    }
    /// Resolve older-history loading. Duplicate IDs or a stale request leave rows untouched.
    pub fn complete_earlier(
        &mut self,
        request: &Request,
        result: Result<Vec<Turn>, &str>,
        remaining: usize,
    ) -> bool {
        if !matches!(&self.page, Some(Page::Loading(id)) if id == request) {
            return false;
        }
        let turns = match result {
            Ok(turns) => turns,
            Err(reason) => {
                self.page = Some(Page::Failed(model::label(reason)));
                return true;
            }
        };
        let mut ids: BTreeSet<_> = self.messages.iter().map(|m| m.turn.id.clone()).collect();
        if turns.len() > HISTORY_LIMIT
            || turns
                .iter()
                .any(|t| !valid_turn(t) || !ids.insert(t.id.clone()))
        {
            self.page = Some(Page::Failed("invalid history snapshot".into()));
            return false;
        }
        let boundary = self.messages.first().map(|m| m.turn.id.clone());
        let mut earlier: Vec<_> = turns.into_iter().map(Record::new).collect();
        earlier.append(&mut self.messages);
        let removed = earlier.len().saturating_sub(HISTORY_LIMIT);
        earlier.truncate(HISTORY_LIMIT);
        self.messages = earlier;
        self.newer = self.newer.saturating_add(removed);
        self.earlier = remaining;
        self.loaded_boundary = boundary;
        self.page = None;
        self.prune();
        self.refresh();
        true
    }
    /// Resolve the newest-window request after browsing a bounded earlier page.
    pub fn complete_latest(&mut self, request: &Request, turns: Vec<Turn>, earlier: usize) -> bool {
        if self
            .pending
            .as_ref()
            .is_none_or(|p| !p.latest || &p.request != request)
            || turns.len() > HISTORY_LIMIT
        {
            return false;
        }
        let mut ids = BTreeSet::new();
        if turns
            .iter()
            .any(|t| !valid_turn(t) || !ids.insert(t.id.clone()))
        {
            return false;
        }
        self.pending = None;
        self.messages = turns.into_iter().map(Record::new).collect();
        self.cache.clear();
        self.newer = 0;
        self.earlier = earlier;
        self.loaded_boundary = None;
        self.resume();
        self.refresh();
        true
    }
    /// Jump to a supplied loaded position. Missing/unloaded identities leave the view intact.
    pub fn jump(&mut self, anchor: Anchor) -> bool {
        if !self.messages.iter().any(|m| {
            m.turn.id == anchor.message
                && (anchor.part.is_empty() || m.turn.parts.iter().any(|p| p.id == anchor.part))
        }) {
            return false;
        }
        self.follow = false;
        self.anchor = Some(anchor);
        self.refresh();
        true
    }
    /// Advance notices, leader expiry and Composer using the host's elapsed clock.
    pub fn tick(&mut self, elapsed: Duration) {
        self.now = self.now.saturating_add(elapsed);
        self.input.tick(elapsed);
        if self.leader.is_some_and(|due| self.now >= due) {
            self.leader = None;
        }
        if self
            .notice
            .as_ref()
            .is_some_and(|(_, due)| self.now >= *due)
        {
            self.notice = None;
        }
    }
    fn request(&mut self) -> Request {
        self.serial = self.serial.saturating_add(1);
        Request {
            conversation: self.conversation(),
            number: self.serial,
        }
    }
    fn say(&mut self, text: &str) {
        self.notice = Some((model::label(text), self.now + Duration::from_secs(3)));
    }
    fn resume(&mut self) {
        self.follow = true;
        self.unread.clear();
        self.anchor = None;
    }
    fn prune(&mut self) {
        self.cache.retain(|(message, part), _| {
            self.messages
                .iter()
                .any(|m| &m.turn.id == message && m.turn.parts.iter().any(|p| &p.id == part))
        });
        self.expanded.retain(|(message, part)| {
            self.messages
                .iter()
                .any(|m| &m.turn.id == message && m.turn.parts.iter().any(|p| &p.id == part))
        });
        self.unread
            .retain(|id| self.messages.iter().any(|m| &m.turn.id == id));
    }
}
fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 128 && !id.chars().any(char::is_control)
}
fn valid_turn(turn: &Turn) -> bool {
    let mut ids = BTreeSet::new();
    valid_id(&turn.id)
        && turn
            .parts
            .iter()
            .take(PART_LIMIT)
            .all(|p| valid_id(&p.id) && ids.insert(&p.id))
}
