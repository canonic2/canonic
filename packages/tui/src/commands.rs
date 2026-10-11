//! Owns command descriptors, ranked search and palette/keys state without execution.

pub mod bindings;
pub mod remaps;
mod view;

use crate::{
    editor::Editor,
    event::{Event, KeyCode, KeyEventKind, KeyModifiers},
    picker::{Intent, PickerView},
    text::clean,
};
use bindings::Binding;
use ratatui::{Frame, layout::Rect};
use std::collections::HashMap;

/// Host-supplied command. Execution and availability rechecking belong to hosts.
#[derive(Clone, Debug)]
pub struct Command {
    /// Stable invocation identity.
    pub id: String,
    /// Display label.
    pub label: String,
    /// Palette group.
    pub category: String,
    /// Searchable description.
    pub description: String,
    /// Search aliases, including optional slash spelling.
    pub aliases: Vec<String>,
    /// Explanation for an unavailable command.
    pub disabled: Option<String>,
    /// Effective shortcuts in the caller's scope; empty means unbound.
    pub bindings: Vec<Binding>,
}

/// Palette and keys screen over one validated, context-specific command map.
pub struct Commands {
    entries: Vec<Command>,
    query: Editor,
    matches: Vec<usize>,
    selected: Option<usize>,
    recent: Vec<String>,
    scopes: HashMap<String, String>,
    scope: Option<String>,
    scope_names: Vec<String>,
    complete_key_map: bool,
    keys: bool,
    offset: usize,
    viewport: usize,
    opened_height: Option<u16>,
    metadata: HashMap<String, remaps::KeyInfo>,
}
impl Commands {
    /// Validate IDs and shortcuts before constructing a searchable view.
    ///
    /// # Errors
    /// Rejects duplicate identities and conflicting or reserved shortcut sequences.
    pub fn new(mut commands: Vec<Command>) -> Result<Self, Error> {
        bindings::Dispatcher::new(&commands, std::time::Duration::from_secs(3))?;
        for c in &mut commands {
            c.label = clean(&c.label, 4096, false);
            c.category = clean(&c.category, 128, false);
            c.description = clean(&c.description, 4096, false);
            c.aliases = c.aliases.iter().map(|a| clean(a, 128, false)).collect();
            c.disabled = c.disabled.as_ref().map(|s| clean(s, 4096, false));
        }
        let mut view = Self {
            entries: commands,
            query: Editor::default(),
            matches: vec![],
            selected: None,
            recent: vec![],
            scopes: HashMap::new(),
            scope: None,
            scope_names: vec![],
            complete_key_map: false,
            keys: false,
            offset: 0,
            viewport: 1,
            opened_height: None,
            metadata: HashMap::new(),
        };
        view.refilter();
        Ok(view)
    }
    /// Apply ordered host-loaded remaps; conflicts remain visible in keyboard help.
    ///
    /// # Errors
    /// Rejects an invalid base map or an unknown remapped command ID.
    pub fn remapped(commands: Vec<Command>, changes: &[remaps::Remap]) -> Result<Self, Error> {
        let map = remaps::resolve(commands, changes)?;
        let mut view = Self::new(map.commands)?;
        view.metadata = map.metadata;
        Ok(view)
    }
    /// Immutable effective descriptors shared with the host dispatcher.
    #[must_use]
    pub fn descriptors(&self) -> &[Command] {
        &self.entries
    }
    /// Custom key and conflict evidence; no source file is read or opened.
    #[must_use]
    pub fn key_info(&self, id: &str) -> Option<&remaps::KeyInfo> {
        self.metadata.get(id)
    }
    /// Supply display scopes for help, independently of palette groups.
    pub fn set_scopes(&mut self, scopes: &[(&str, &str)]) {
        self.scopes = scopes
            .iter()
            .map(|(id, scope)| ((*id).to_owned(), clean(scope, 128, false)))
            .collect();
        self.scope = None;
        self.refilter();
    }
    /// Supply additional keyboard-help scopes, including scopes with no commands.
    pub fn set_scope_names(&mut self, scopes: &[&str]) {
        self.scope_names = scopes.iter().map(|s| clean(s, 128, false)).collect();
        self.scope = None;
        self.refilter();
    }
    /// Assert the supplied map covers every host scope before displaying free-key evidence.
    /// Defaults to false; a local map cannot establish global shortcut availability.
    pub fn set_complete_key_map(&mut self, complete: bool) {
        self.complete_key_map = complete;
    }
    /// Supply recent IDs first; at most three appear in the recent group.
    pub fn set_recent(&mut self, ids: &[&str]) {
        self.recent = ids.iter().take(3).map(|id| (*id).to_owned()).collect();
        self.refilter();
    }
    /// Switch between palette and help, preserving the query and starting at its first match.
    pub fn set_keys(&mut self, keys: bool) {
        if self.keys != keys {
            self.selected = None;
        }
        self.keys = keys;
        self.scope = None;
        self.offset = 0;
        self.refilter();
    }
    /// Whether the view uses the keyboard-help projection.
    #[must_use]
    pub fn is_keys(&self) -> bool {
        self.keys
    }
    /// Filter text; Escape clears it before returning cancellation.
    #[must_use]
    pub fn query(&self) -> &str {
        self.query.text()
    }
    /// Set a bounded filter without invocation.
    pub fn set_query(&mut self, query: &str) {
        self.capture_height();
        self.query.set_text(&clean(query, 4096, false));
        self.selected = None;
        self.refilter();
    }
    /// Number of matches, excluding group headings.
    #[must_use]
    pub fn match_count(&self) -> usize {
        self.matches.len()
    }
    /// Highlighted command; unavailable entries are never highlighted.
    #[must_use]
    pub fn selected(&self) -> Option<&str> {
        self.selected.map(|i| self.entries[i].id.as_str())
    }
    fn group(&self, index: usize) -> &str {
        let c = &self.entries[index];
        if self.keys {
            self.scopes.get(&c.id).map_or("everywhere", String::as_str)
        } else if self.recent.contains(&c.id) {
            "recent"
        } else {
            &c.category
        }
    }
    fn search_rank(&self, c: &Command, q: &str) -> Option<u8> {
        if q.is_empty() || c.bindings.iter().any(|b| b.label().to_lowercase() == q) {
            Some(0)
        } else if c.label.to_lowercase().starts_with(q)
            || c.bindings
                .iter()
                .any(|b| b.label().to_lowercase().starts_with(q))
        {
            Some(1)
        } else if c
            .label
            .to_lowercase()
            .split(|c: char| !c.is_alphanumeric())
            .any(|word| word.starts_with(q))
        {
            Some(2)
        } else if c.aliases.iter().any(|a| view::matches(a, q)) {
            Some(3)
        } else if view::matches(&c.label, q)
            || view::matches(&c.description, q)
            || c.disabled.as_ref().is_some_and(|d| view::matches(d, q))
        {
            Some(4)
        } else if self.keys
            && self.metadata.get(&c.id).is_some_and(|m| {
                m.ignored
                    .iter()
                    .any(|b| b.label().to_lowercase().contains(q))
            })
        {
            Some(5)
        } else {
            None
        }
    }
    fn refilter(&mut self) {
        let q = self.query().to_lowercase();
        let mut categories = HashMap::new();
        for (i, c) in self.entries.iter().enumerate() {
            categories.entry(c.category.as_str()).or_insert(i);
        }
        let mut found: Vec<_> = self
            .entries
            .iter()
            .enumerate()
            .filter_map(|(i, c)| {
                if self.keys
                    && self
                        .scope
                        .as_ref()
                        .is_some_and(|scope| self.group(i) != scope)
                {
                    return None;
                }
                let keys = c
                    .bindings
                    .iter()
                    .chain(
                        self.metadata
                            .get(&c.id)
                            .into_iter()
                            .filter(|_| self.keys)
                            .flat_map(|m| m.ignored.iter()),
                    )
                    .map(Binding::label)
                    .collect::<Vec<_>>()
                    .join(" ")
                    .to_lowercase();
                let rank = self.search_rank(c, &q)?;
                let recent = self
                    .recent
                    .iter()
                    .position(|id| id == &c.id)
                    .unwrap_or(usize::MAX);
                Some((i, rank, recent, keys))
            })
            .collect();
        found.sort_by(|a, b| {
            if self.keys && q.is_empty() {
                let left = self.group(a.0);
                let right = self.group(b.0);
                return (left != "everywhere", left, a.0).cmp(&(right != "everywhere", right, b.0));
            }
            if q.is_empty() {
                return (
                    a.2 == usize::MAX,
                    a.2,
                    categories[self.entries[a.0].category.as_str()],
                    a.0,
                )
                    .cmp(&(
                        b.2 == usize::MAX,
                        b.2,
                        categories[self.entries[b.0].category.as_str()],
                        b.0,
                    ));
            }
            a.1.cmp(&b.1)
                .then_with(|| {
                    if a.1 == 1 && a.3.starts_with(&q) && b.3.starts_with(&q) {
                        a.3.cmp(&b.3)
                    } else {
                        a.2.cmp(&b.2)
                    }
                })
                .then(a.0.cmp(&b.0))
        });
        self.matches = found.into_iter().map(|(i, _, _, _)| i).collect();
        if !self
            .selected
            .is_some_and(|i| self.matches.contains(&i) && self.entries[i].disabled.is_none())
        {
            self.selected = self
                .matches
                .iter()
                .copied()
                .find(|i| self.entries[*i].disabled.is_none());
        }
        self.offset = 0;
    }
    fn navigate(&mut self, code: KeyCode) {
        let enabled: Vec<_> = self
            .matches
            .iter()
            .copied()
            .filter(|i| self.entries[*i].disabled.is_none())
            .collect();
        if enabled.is_empty() {
            self.selected = None;
            return;
        }
        let at = enabled
            .iter()
            .position(|i| Some(*i) == self.selected)
            .unwrap_or(0);
        let next = match code {
            KeyCode::Home => 0,
            KeyCode::End => enabled.len() - 1,
            KeyCode::Up => at.saturating_sub(1),
            KeyCode::PageUp => at.saturating_sub(self.viewport.max(1)),
            KeyCode::PageDown => (at + self.viewport.max(1)).min(enabled.len() - 1),
            _ => (at + 1).min(enabled.len() - 1),
        };
        self.selected = Some(enabled[next]);
    }
    /// Apply one event. Keys-screen `o` returns Inspect for the binding source.
    pub fn update(&mut self, event: &Event) -> Intent {
        self.capture_height();
        if let Event::Key(k) = event {
            if k.kind == KeyEventKind::Release {
                return Intent::None;
            }
            if k.code == KeyCode::Esc {
                if !self.query().is_empty() {
                    self.set_query("");
                    return Intent::None;
                }
                return Intent::Cancelled;
            }
            if matches!(
                k.code,
                KeyCode::Up
                    | KeyCode::Down
                    | KeyCode::Home
                    | KeyCode::End
                    | KeyCode::PageUp
                    | KeyCode::PageDown
            ) {
                self.navigate(k.code);
                return Intent::None;
            }
            if k.kind == KeyEventKind::Press && k.modifiers.is_empty() {
                match k.code {
                    KeyCode::Enter if !self.keys => {
                        return self
                            .selected()
                            .map_or(Intent::None, |id| Intent::Selected(id.into()));
                    }
                    KeyCode::Char('o')
                        if self.keys
                            && self
                                .selected()
                                .is_some_and(|id| self.metadata.contains_key(id)) =>
                    {
                        return Intent::Inspect(self.selected().unwrap_or_default().into());
                    }
                    KeyCode::Tab if self.keys => {
                        self.next_scope();
                        return Intent::None;
                    }
                    _ => {}
                }
            }
            if (k.code == KeyCode::Char('j') && k.modifiers.contains(KeyModifiers::CONTROL))
                || matches!(k.code, KeyCode::Enter | KeyCode::Tab | KeyCode::BackTab)
            {
                return Intent::None;
            }
        }
        let before = self.query().to_owned();
        match event {
            Event::Paste(s) => {
                self.query.insert(&clean(s, 4096, false));
            }
            _ => {
                self.query.update(event);
            }
        }
        if before != self.query() {
            self.selected = None;
            self.refilter();
        }
        Intent::None
    }
    fn next_scope(&mut self) {
        let mut scopes: Vec<_> = (0..self.entries.len())
            .map(|i| self.group(i).to_owned())
            .collect();
        scopes.extend(self.scope_names.iter().cloned());
        scopes.sort_by(|a, b| (a != "everywhere", a).cmp(&(b != "everywhere", b)));
        scopes.dedup();
        let next = self
            .scope
            .as_ref()
            .and_then(|s| scopes.iter().position(|v| v == s))
            .map_or(0, |i| i + 1);
        self.scope = scopes.get(next).cloned();
        self.refilter();
    }
    /// Render command-specific columns using shared rules and semantic styles.
    pub fn render(&mut self, frame: &mut Frame<'_>, area: Rect, view: PickerView<'_>) -> Rect {
        self.draw(frame, area, view)
    }
    /// Measured opening height stays stable while filtering.
    #[must_use]
    pub fn height(&self, _width: u16, view: PickerView<'_>) -> u16 {
        if self.keys {
            return view.max_height;
        }
        self.opened_height
            .unwrap_or_else(|| u16::try_from(self.rows().len().max(5) + 3).unwrap_or(u16::MAX))
            .min(view.max_height)
    }
    fn capture_height(&mut self) {
        if !self.keys && self.opened_height.is_none() {
            self.opened_height =
                Some(u16::try_from(self.rows().len().max(5) + 3).unwrap_or(u16::MAX));
        }
    }
}
/// Invalid effective map; construction leaves existing views untouched.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// Duplicate command identity.
    Duplicate(String),
    /// Invalid, reserved or ambiguous binding.
    Binding(String),
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid command map: {self:?}")
    }
}
impl std::error::Error for Error {}
