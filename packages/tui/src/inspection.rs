//! Owns bounded read-only code/patch state and host requests; never loads or copies itself.

mod input;
mod model;
mod view;
use crate::{
    editor::Editor,
    event::Event,
    theme::{Presentation, Symbols},
};
pub use model::{
    BYTE_LIMIT, Content, Intent, Invalid, Item, LINE_LIMIT, Operation, Origin, Outcome, Request,
    State, Ticket,
};
use model::{Kind, Loaded, Row};
use ratatui::{Frame, layout::Rect};
use std::{collections::BTreeSet, ops::Range};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Field {
    Find,
    Line,
}
/// One inspector for one retained item. Hosts retain separate instances per item if needed.
#[expect(
    clippy::struct_excessive_bools,
    reason = "Activity, focus, file projection and wrapping are independent view controls."
)]
pub struct Inspector {
    item: Item,
    state: State,
    loaded: Loaded,
    active: bool,
    focused: bool,
    whole: bool,
    cursor: usize,
    top: usize,
    top_part: usize,
    horizontal: usize,
    wrap: bool,
    partial: bool,
    expanded: BTreeSet<usize>,
    anchor: Option<usize>,
    copy_selection: Option<Range<usize>>,
    field: Option<Field>,
    editor: Editor,
    query: String,
    matches: Vec<(usize, Range<usize>)>,
    current_match: Option<usize>,
    pending: Option<Request>,
    intents: Vec<Intent>,
    notice: String,
    page_rows: usize,
    visible: Vec<(usize, usize)>,
    width: u16,
    symbols: Symbols,
}
impl Inspector {
    /// Validate all supplied text before constructing a view. No implicit request is issued.
    ///
    /// # Errors
    /// Returns invalid identity, size or unified-patch errors before constructing state.
    pub fn new(item: Item, state: State) -> Result<Self, Invalid> {
        model::item(&item)?;
        let loaded = Self::validate(&state)?;
        let mut editor = Editor::new(4096);
        editor.set_reject_overflow(true);
        let line = item.line;
        let partial = matches!(&state, State::Ready(Content::CodeAt { .. }));
        let mut result = Self {
            item,
            state,
            loaded,
            active: true,
            focused: true,
            whole: false,
            cursor: 0,
            top: 0,
            top_part: 0,
            horizontal: 0,
            wrap: false,
            partial,
            expanded: BTreeSet::new(),
            anchor: None,
            copy_selection: None,
            field: None,
            editor,
            query: String::new(),
            matches: vec![],
            current_match: None,
            pending: None,
            intents: vec![],
            notice: String::new(),
            page_rows: 20,
            visible: vec![],
            width: 80,
            symbols: Symbols::Unicode,
        };
        if let Some(n) = line {
            result.goto(n);
        }
        Ok(result)
    }
    fn validate(state: &State) -> Result<Loaded, Invalid> {
        match state {
            State::Ready(c) => model::parse(c),
            State::Unavailable { reason, preview } => {
                if reason.len() > 65536 {
                    return Err(Invalid::Limit);
                }
                if let Some(c) = preview {
                    model::parse(c)?;
                }
                Ok(Loaded::default())
            }
            State::Loading(s) | State::Failed(s) if s.len() > 65536 => Err(Invalid::Limit),
            _ => Ok(Loaded::default()),
        }
    }
    /// Replace with a strictly newer authoritative revision, invalidating outstanding effects.
    ///
    /// # Errors
    /// Rejects another identity, non-increasing revisions or invalid bounded content atomically.
    pub fn replace(&mut self, item: Item, state: State) -> Result<(), Invalid> {
        model::item(&item)?;
        let loaded = Self::validate(&state)?;
        if item.id != self.item.id || item.revision <= self.item.revision {
            return Err(Invalid::Revision);
        }
        let line = self.line();
        self.item = item;
        self.state = state;
        self.partial = matches!(&self.state, State::Ready(Content::CodeAt { .. }));
        self.loaded = loaded;
        self.pending = None;
        self.intents.clear();
        self.anchor = None;
        self.expanded.clear();
        self.field = None;
        self.notice.clear();
        self.whole = self.whole && self.loaded.file.is_some();
        self.cursor = 0;
        self.top = 0;
        self.top_part = 0;
        if let Some(n) = line {
            self.goto(n);
        }
        self.search();
        Ok(())
    }
    /// Reopen this retained item at its saved logical position; old replies cannot revive it.
    pub fn reopen(&mut self, origin: Origin) {
        if !self.active {
            self.intents.clear();
        }
        self.item.origin = origin;
        self.active = true;
        self.focused = true;
    }
    pub fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }
    #[must_use]
    pub fn active(&self) -> bool {
        self.active
    }
    #[must_use]
    pub fn item(&self) -> &Item {
        &self.item
    }
    #[must_use]
    pub fn state(&self) -> &State {
        &self.state
    }
    #[must_use]
    pub fn line(&self) -> Option<usize> {
        let rows = self.rows();
        rows.get(self.cursor).and_then(|r| {
            r.new.or(r.old).or_else(|| {
                rows[self.cursor..]
                    .iter()
                    .find_map(|line| line.new.or(line.old))
            })
        })
    }
    #[must_use]
    pub fn whole_file(&self) -> bool {
        self.whole
    }
    #[must_use]
    pub fn selection(&self) -> Option<Range<usize>> {
        self.anchor.map(|a| {
            let high = a.max(self.cursor);
            let end = self.fold(high).map_or(high + 1, |(_, range)| range.end);
            a.min(self.cursor)..end
        })
    }
    #[must_use]
    pub fn match_count(&self) -> usize {
        self.matches.len()
    }
    #[must_use]
    pub fn notice(&self) -> &str {
        &self.notice
    }
    #[must_use]
    pub fn pending(&self) -> Option<&Request> {
        self.pending.as_ref()
    }
    pub fn take_intents(&mut self) -> Vec<Intent> {
        std::mem::take(&mut self.intents)
    }
    fn rows(&self) -> &[Row] {
        if self.whole {
            self.loaded.file.as_deref().unwrap_or(&[])
        } else {
            &self.loaded.diff
        }
    }
    fn projection(&self) -> Vec<usize> {
        (0..self.rows().len())
            .filter(|i| {
                self.whole
                    || !self
                        .loaded
                        .folds
                        .iter()
                        .enumerate()
                        .any(|(n, r)| !self.expanded.contains(&n) && *i > r.start && *i < r.end)
            })
            .collect()
    }
    fn fold(&self, index: usize) -> Option<(usize, Range<usize>)> {
        if self.whole {
            return None;
        }
        self.loaded
            .folds
            .iter()
            .enumerate()
            .find(|(n, r)| !self.expanded.contains(n) && r.contains(&index))
            .map(|(n, r)| (n, r.clone()))
    }
    fn reveal(&mut self, index: usize) {
        if let Some((n, _)) = self.fold(index) {
            self.expanded.insert(n);
        }
    }
    fn request(&mut self, operation: Operation) {
        if self.pending.is_some() {
            return;
        }
        if matches!(operation, Operation::Load) {
            self.state = State::Loading("retrying · waiting for the host".into());
        }
        self.copy_selection = if matches!(operation, Operation::Copy { .. }) {
            self.selection()
        } else {
            None
        };
        let r = Request {
            ticket: Ticket::new(),
            item: self.item.id.clone(),
            revision: self.item.revision,
            operation,
        };
        self.pending = Some(r.clone());
        self.intents.push(Intent::Request(r));
        self.notice.clear();
    }
    /// Resolve one exact request. Invalid content and wrong-kind replies leave it pending.
    ///
    /// # Errors
    /// Returns validation errors for malformed or oversized host results without consuming them.
    pub fn complete(&mut self, request: &Request, outcome: Outcome) -> Result<bool, Invalid> {
        if !self.active || self.pending.as_ref() != Some(request) {
            return Ok(false);
        }
        let loaded = match (&request.operation, &outcome) {
            (Operation::Load, Outcome::Loaded(s)) => Some(Self::validate(s)?),
            (Operation::Copy { .. }, Outcome::Copied)
            | (Operation::Editor { .. }, Outcome::Opened)
            | (_, Outcome::Failed(_)) => None,
            _ => return Ok(false),
        };
        if let Outcome::Failed(s) = &outcome
            && s.len() > 65536
        {
            return Err(Invalid::Limit);
        }
        self.pending = None;
        self.intents
            .retain(|intent| !matches!(intent,Intent::Request(r) if r==request));
        match outcome {
            Outcome::Loaded(s) => {
                self.state = s;
                self.partial = matches!(&self.state, State::Ready(Content::CodeAt { .. }));
                self.loaded = loaded.unwrap_or_default();
                self.whole = false;
                self.cursor = 0;
                self.top = 0;
                self.top_part = 0;
                self.expanded.clear();
                self.search();
                if let Some(n) = self.item.line {
                    self.goto(n);
                }
            }
            Outcome::Copied => {
                let Operation::Copy { lines, patch, .. } = request.operation else {
                    return Ok(false);
                };
                self.notice = format!(
                    "copied {lines} lines of {} · via the host",
                    if patch { "patch" } else { "code" }
                );
                if self.selection() == self.copy_selection {
                    self.anchor = None;
                }
            }
            Outcome::Opened => self.notice = "opened in editor · via the host".into(),
            Outcome::Failed(reason) => {
                self.notice = format!(
                    "couldn't {} · {reason}",
                    match request.operation {
                        Operation::Load => "load",
                        Operation::Copy { .. } => "copy",
                        Operation::Editor { .. } => "open",
                    }
                );
                if matches!(request.operation, Operation::Load) {
                    self.state = State::Failed(reason);
                }
            }
        }
        Ok(true)
    }
    pub(crate) fn close(&mut self) {
        self.active = false;
        self.pending = None;
        self.intents.clear();
        self.field = None;
        self.anchor = None;
        self.intents.push(Intent::Returned);
    }
    /// Consume input only when active and focused. Text fields isolate paste and command keys.
    pub fn update(&mut self, event: &Event) -> bool {
        input::update(self, event)
    }
    /// Draw in a caller-owned frame and rectangle; no terminal modes or external effects.
    pub fn render(&mut self, frame: &mut Frame<'_>, area: Rect, p: Presentation) {
        view::render(self, frame, area, p);
    }
}
