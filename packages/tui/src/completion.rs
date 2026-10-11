//! Focus-preserving completion with pure source updates and an injected clock.
//!
//! ```
//! use canonic_tui::completion::{Batch, Candidate, Menu};
//! let mut menu = Menu::new("files", "set", &["repository"], vec![])?;
//! let request = menu.request();
//! assert!(menu.complete(&request, "repository", Batch {
//!     candidates: vec![Candidate::new("settings", "settings.ts", "settings.ts")],
//!     complete: true,
//!     ..Batch::default()
//! }));
//! assert_eq!(menu.selected().map(|item| item.id.as_str()), Some("settings"));
//! # Ok::<(), &'static str>(())
//! ```

mod matching;
mod view;

use crate::{
    event::{Event, KeyCode, KeyEventKind, KeyModifiers},
    text::clean,
};
use std::{sync::Arc, time::Duration};

/// One suggestion. IDs are stable and unique across sources in an opening.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    pub id: String,
    pub name: String,
    /// Searchable spellings ranked after name prefix and word-start matches.
    pub aliases: Vec<String>,
    pub insertion: String,
    pub description: String,
    pub tag: String,
    /// Optional command argument help displayed by an editor host after insertion.
    pub argument_hint: String,
    /// Optional visual placeholder; never part of the inserted text.
    pub argument_placeholder: String,
    pub disabled: Option<String>,
    /// Keep a host descriptor out of visible results without dropping its identity.
    pub hidden: bool,
    /// Higher values win equal-quality matches.
    pub recency: u64,
}
impl Candidate {
    /// Construct a plain suggestion; hosts may then add metadata.
    #[must_use]
    pub fn new(id: &str, name: &str, insertion: &str) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            aliases: Vec::new(),
            insertion: insertion.into(),
            description: String::new(),
            tag: String::new(),
            argument_hint: String::new(),
            argument_placeholder: String::new(),
            disabled: None,
            hidden: false,
            recency: 0,
        }
    }
}

/// Unforgeable identity of one query, including retry and opening identity.
#[derive(Clone, Debug)]
pub struct Request {
    identity: Arc<()>,
    pub query: String,
}
impl PartialEq for Request {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.identity, &other.identity) && self.query == other.query
    }
}
impl Eq for Request {}

/// A source batch replaces that source's accumulated rows for this query.
#[derive(Clone, Debug, Default)]
pub struct Batch {
    pub candidates: Vec<Candidate>,
    pub scanned: usize,
    pub total: Option<usize>,
    pub complete: bool,
    pub error: Option<String>,
}
#[derive(Clone, Debug)]
struct Source {
    id: String,
    batch: Batch,
    timed_out: bool,
}

/// Input disposition. The host performs insertion or retries; no I/O occurs here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    None,
    /// Editing/cursor input belongs to the box.
    Unhandled,
    Insert(Candidate),
    Close,
    /// No selectable suggestion: the host may handle this Enter as submission.
    PassEnter,
    Retry(Request),
}

/// Bounded menu state. Rendering never takes keyboard focus from its host editor.
pub struct Menu {
    title: String,
    request: Request,
    sources: Vec<Source>,
    rows: Vec<Candidate>,
    visible: Vec<usize>,
    order: Vec<String>,
    selected: Option<String>,
    guess: Option<Candidate>,
    elapsed: Duration,
    dispatched: bool,
    open: bool,
    body_rows: u16,
    page_rows: usize,
    offset: usize,
}

impl Menu {
    /// Open a query with named sources and optional cached suggestions.
    ///
    /// # Errors
    /// Rejects empty/duplicate source names or invalid/oversized candidates.
    pub fn new(
        title: &str,
        query: &str,
        sources: &[&str],
        cached: Vec<Candidate>,
    ) -> Result<Self, &'static str> {
        if sources.is_empty()
            || sources.len() > 64
            || sources.iter().enumerate().any(|(i, id)| {
                id.is_empty()
                    || id.len() > 256
                    || id.chars().any(char::is_control)
                    || sources[..i].contains(id)
            })
        {
            return Err("Invalid sources");
        }
        let rows = validate(cached)?;
        let mut menu = Self {
            title: clean(title, 256, false),
            request: Request {
                identity: Arc::new(()),
                query: clean(query, 4096, false),
            },
            sources: sources
                .iter()
                .map(|id| Source {
                    id: clean(id, 256, false),
                    batch: Batch::default(),
                    timed_out: false,
                })
                .collect(),
            body_rows: u16::try_from(rows.len().clamp(1, 8)).unwrap_or(8),
            rows,
            visible: Vec::new(),
            order: Vec::new(),
            selected: None,
            guess: None,
            elapsed: Duration::ZERO,
            dispatched: false,
            open: true,
            page_rows: 8,
            offset: 0,
        };
        if menu.rows.is_empty() {
            menu.body_rows = 8;
        }
        menu.rank(false);
        Ok(menu)
    }
    /// Current identity; suitable for immediate cached responses.
    #[must_use]
    pub fn request(&self) -> Request {
        self.request.clone()
    }
    /// Emit a source query once, after 60 ms of unchanged input.
    pub fn take_request(&mut self) -> Option<Request> {
        if !self.open || self.dispatched || self.elapsed < Duration::from_millis(60) {
            return None;
        }
        self.dispatched = true;
        Some(self.request())
    }
    /// Change the active token query and invalidate previous source responses.
    pub fn set_query(&mut self, query: &str) {
        let query = clean(query, 4096, false);
        if self.request.query == query {
            return;
        }
        self.request = Request {
            identity: Arc::new(()),
            query,
        };
        self.elapsed = Duration::ZERO;
        self.dispatched = false;
        self.guess = None;
        for source in &mut self.sources {
            source.batch = Batch::default();
            source.timed_out = false;
        }
        self.rank(false);
    }
    /// Apply a current source snapshot atomically; completed sources reject repeats.
    pub fn complete(&mut self, request: &Request, source: &str, mut batch: Batch) -> bool {
        if !self.open || request != &self.request {
            return false;
        }
        let Some(index) = self
            .sources
            .iter()
            .position(|s| s.id == source && !s.batch.complete)
        else {
            return false;
        };
        let Ok(candidates) = validate(batch.candidates) else {
            return false;
        };
        batch.candidates = candidates;
        batch.error = batch.error.map(|e| clean(&e, 256, false));
        if batch.error.is_some() {
            batch.complete = true;
        }
        let mut rows = Vec::new();
        for (i, s) in self.sources.iter().enumerate() {
            rows.extend(
                if i == index {
                    &batch.candidates
                } else {
                    &s.batch.candidates
                }
                .iter()
                .cloned(),
            );
        }
        let Ok(rows) = validate(rows) else {
            return false;
        };
        self.sources[index].batch = batch;
        self.rows = rows;
        self.rank(true);
        true
    }
    /// Explicit host guess, shown only when no match exists; never auto-inserted.
    pub fn set_guess(&mut self, request: &Request, guess: Candidate) -> bool {
        if !self.open || request != &self.request {
            return false;
        }
        let Ok(mut candidates) = validate(vec![guess]) else {
            return false;
        };
        self.guess = candidates.pop().filter(|c| c.disabled.is_none());
        true
    }
    /// Advance debounce, loading and timeout using the host clock.
    pub fn tick(&mut self, elapsed: Duration) {
        self.elapsed = self.elapsed.saturating_add(elapsed);
        if self.open && self.visible.is_empty() && self.elapsed >= Duration::from_secs(5) {
            for source in &mut self.sources {
                if !source.batch.complete {
                    source.batch.complete = true;
                    source.timed_out = true;
                }
            }
        }
    }
    /// Close and invalidate this opening's results.
    pub fn close(&mut self) {
        self.open = false;
    }
    /// Start an asynchronous refresh while preserving visible cached suggestions.
    pub fn begin_search(&mut self) {
        self.request.identity = Arc::new(());
        for source in &mut self.sources {
            source.batch = Batch::default();
            source.timed_out = false;
        }
        self.elapsed = Duration::ZERO;
        self.dispatched = false;
    }
    #[must_use]
    pub fn is_open(&self) -> bool {
        self.open
    }
    #[must_use]
    pub fn selected(&self) -> Option<&Candidate> {
        self.selected
            .as_ref()
            .and_then(|id| self.rows.iter().find(|r| &r.id == id))
    }
    #[must_use]
    pub fn matches(&self) -> Vec<&Candidate> {
        self.visible.iter().map(|i| &self.rows[*i]).collect()
    }
    #[must_use]
    pub fn loading(&self) -> bool {
        self.open
            && self.elapsed >= Duration::from_millis(150)
            && self.sources.iter().any(|s| !s.batch.complete)
    }
    /// Sources still awaiting a final response, including retry targets.
    #[must_use]
    pub fn pending_sources(&self) -> Vec<&str> {
        self.sources
            .iter()
            .filter(|s| !s.batch.complete)
            .map(|s| s.id.as_str())
            .collect()
    }
    /// Source failures for host diagnostics or detailed retry UI.
    #[must_use]
    pub fn failures(&self) -> Vec<(&str, &str)> {
        self.sources
            .iter()
            .filter_map(|s| s.batch.error.as_deref().map(|error| (s.id.as_str(), error)))
            .collect()
    }
    /// Sources with no reply after five seconds; timeout is distinct from failure.
    #[must_use]
    pub fn timed_out_sources(&self) -> Vec<&str> {
        self.sources
            .iter()
            .filter(|s| s.timed_out)
            .map(|s| s.id.as_str())
            .collect()
    }
    /// Explicit host retry for a partial-source warning; only failed sources reset.
    pub fn retry_failed(&mut self) -> Option<Request> {
        if !self.open || !self.failed() {
            return None;
        }
        let Action::Retry(request) = self.retry() else {
            return None;
        };
        Some(request)
    }
    #[must_use]
    pub fn hint(&self) -> &'static str {
        if self.selected().is_some() {
            if self.failed() {
                "tab / enter inserts · alt+r retries · esc closes"
            } else {
                "↑↓ choose · tab / enter inserts · esc closes"
            }
        } else if self.failed() {
            "tab retries · enter sends as text · esc closes"
        } else if self.guess.is_some() {
            "tab inserts closest · enter sends as text · esc closes"
        } else {
            "enter sends as text · esc closes"
        }
    }
    /// Compact help; the full keyboard contract remains unchanged.
    #[must_use]
    pub fn hint_for(&self, width: u16) -> &'static str {
        if self.selected().is_none() && width < 60 {
            return if self.failed() {
                if width < 24 {
                    "tab retries · esc"
                } else {
                    "tab retries · esc closes"
                }
            } else if self.guess.is_some() {
                if width < 24 {
                    "tab closest · esc"
                } else {
                    "tab closest · esc closes"
                }
            } else if width < 24 {
                "enter sends · esc"
            } else {
                "enter sends · esc closes"
            };
        }
        if self.selected().is_some() && self.failed() && width >= 50 {
            return "tab inserts · alt+r retries · esc closes";
        }
        if self.selected().is_some() && width < 60 {
            if width < 24 {
                "tab inserts · esc"
            } else if width < 34 {
                "tab inserts · esc closes"
            } else {
                "tab / enter inserts · esc closes"
            }
        } else {
            self.hint()
        }
    }
    /// All source identities, including finished sources.
    #[must_use]
    pub fn sources(&self) -> Vec<&str> {
        self.sources.iter().map(|s| s.id.as_str()).collect()
    }
    pub(super) fn all_failed(&self) -> bool {
        self.sources.iter().all(|s| s.batch.error.is_some())
    }
    fn failed(&self) -> bool {
        self.sources
            .iter()
            .any(|s| s.batch.error.is_some() || s.timed_out)
    }
    /// Consume menu keys. Ordinary editing remains the host's responsibility.
    pub fn update(&mut self, event: &Event) -> Action {
        if !self.open {
            return Action::Unhandled;
        }
        let Event::Key(key) = event else {
            return Action::Unhandled;
        };
        if key.kind == KeyEventKind::Release {
            return Action::None;
        }
        let plain = key.modifiers.is_empty();
        let control = key.modifiers == KeyModifiers::CONTROL;
        if key.modifiers == KeyModifiers::ALT && key.code == KeyCode::Char('r') {
            return if key.kind == KeyEventKind::Press && self.failed() {
                self.retry()
            } else {
                Action::None
            };
        }
        if control && key.code == KeyCode::Char('c') {
            return Action::Close;
        }
        let delta = match key.code {
            KeyCode::Up if plain => Some(-1),
            KeyCode::Down if plain => Some(1),
            KeyCode::Char('p') if control => Some(-1),
            KeyCode::Char('n') if control => Some(1),
            KeyCode::PageUp if plain => Some(-isize::try_from(self.page_rows).unwrap_or(8)),
            KeyCode::PageDown if plain => Some(isize::try_from(self.page_rows).unwrap_or(8)),
            _ => None,
        };
        if let Some(delta) = delta {
            self.navigate(delta);
            return Action::None;
        }
        if !plain {
            return Action::Unhandled;
        }
        match key.code {
            KeyCode::Esc => Action::Close,
            KeyCode::Enter | KeyCode::Tab if key.kind != KeyEventKind::Press => Action::None,
            KeyCode::Enter | KeyCode::Tab => {
                if let Some(candidate) = self.selected() {
                    return Action::Insert(candidate.clone());
                }
                if key.code == KeyCode::Enter {
                    return Action::PassEnter;
                }
                if self.failed() {
                    return self.retry();
                }
                self.guess.clone().map_or(Action::None, Action::Insert)
            }
            _ => Action::Unhandled,
        }
    }
    fn retry(&mut self) -> Action {
        self.request.identity = Arc::new(());
        for source in &mut self.sources {
            if source.batch.error.take().is_some() || source.timed_out {
                source.batch.complete = false;
                source.timed_out = false;
            }
        }
        self.elapsed = Duration::ZERO;
        self.dispatched = true;
        Action::Retry(self.request())
    }
    fn navigate(&mut self, delta: isize) {
        let enabled: Vec<_> = self
            .visible
            .iter()
            .filter(|i| self.rows[**i].disabled.is_none())
            .map(|i| self.rows[*i].id.clone())
            .collect();
        if enabled.is_empty() {
            self.selected = None;
            return;
        }
        let at = enabled
            .iter()
            .position(|id| Some(id) == self.selected.as_ref())
            .unwrap_or(0);
        self.selected =
            Some(enabled[at.saturating_add_signed(delta).min(enabled.len() - 1)].clone());
    }
}

fn validate(mut rows: Vec<Candidate>) -> Result<Vec<Candidate>, &'static str> {
    let mut ids = std::collections::HashSet::new();
    if rows.len() > 5000 {
        return Err("Too many candidates");
    }
    for row in &mut rows {
        if row.id.is_empty()
            || row.id.len() > 256
            || !ids.insert(row.id.clone())
            || row.name.is_empty()
            || row.name.len() > 4096
            || row.aliases.len() > 16
            || row
                .aliases
                .iter()
                .any(|alias| alias.is_empty() || alias.len() > 128)
            || row.insertion.is_empty()
            || row.insertion.len() > 16384
            || row.insertion.chars().any(char::is_control)
        {
            return Err("Invalid candidate");
        }
        row.name = clean(&row.name, 4096, false);
        for alias in &mut row.aliases {
            *alias = clean(alias, 128, false);
            if alias.is_empty() {
                return Err("Invalid alias");
            }
        }
        if row.name.is_empty() {
            return Err("Invalid name");
        }
        row.description = clean(&row.description, 4096, true);
        row.tag = clean(&row.tag, 256, false);
        row.argument_hint = clean(&row.argument_hint, 256, false);
        row.argument_placeholder = clean(&row.argument_placeholder, 256, false);
        row.disabled = row.disabled.take().map(|s| clean(&s, 256, false));
    }
    Ok(rows)
}
