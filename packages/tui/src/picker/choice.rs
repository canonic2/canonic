//! Owns rich picker state and stable-ID intent semantics without host operations.

mod input;
mod view;

use super::model::{self, Match};
use super::model::{Choice, DuplicateId, Filter, SelectionMode, Status};
use crate::{editor::Editor, event::Event};
use ratatui::{Frame, layout::Rect};
use std::collections::HashSet;
pub use view::PickerView;

/// Typed user intent; hosts execute and recheck availability themselves.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Intent {
    /// No host operation.
    None,
    /// Explicit single choice.
    Selected(String),
    /// Explicit multiple-choice confirmation, in supplied order.
    Confirmed(Vec<String>),
    /// Close without choosing.
    Cancelled,
    /// Inspect this choice without selecting it.
    Inspect(String),
}

/// Rich compact picker, distinct from the compatibility index-based view.
pub struct ChoicePicker {
    choices: Vec<Choice>,
    selected: Option<String>,
    marked: HashSet<String>,
    mode: SelectionMode,
    filter: Filter,
    query: Editor,
    status: Status,
    matches: Vec<Match>,
    offset: usize,
    viewport: usize,
    opened_height: Option<u16>,
    minimum: usize,
    maximum: Option<usize>,
    inspect: bool,
    notice: Option<String>,
}

impl ChoicePicker {
    /// Validate rows and select a supplied safe default or the first safe row.
    ///
    /// # Errors
    /// Rejects duplicate identities before constructing any state.
    pub fn new(
        mut choices: Vec<Choice>,
        default: Option<&str>,
        mode: SelectionMode,
        filter: Filter,
    ) -> Result<Self, DuplicateId> {
        model::validate(&mut choices)?;
        let selected = default
            .and_then(|id| {
                choices
                    .iter()
                    .find(|c| c.id == id && c.disabled.is_none() && !c.danger)
            })
            .or_else(|| choices.iter().find(|c| c.disabled.is_none() && !c.danger))
            .map(|c| c.id.clone());
        let matches = model::matches(&choices, "");
        Ok(Self {
            choices,
            selected,
            marked: HashSet::new(),
            mode,
            filter,
            query: Editor::default(),
            status: Status::Ready,
            matches,
            offset: 0,
            viewport: 1,
            opened_height: None,
            minimum: 0,
            maximum: None,
            inspect: false,
            notice: None,
        })
    }

    /// Current highlighted stable identity; this is not a confirmation.
    #[must_use]
    pub fn selected(&self) -> Option<&str> {
        self.selected.as_deref()
    }
    /// Set a safe highlight, or clear it when the host requires an explicit choice.
    /// Returns false for a missing, disabled or dangerous identity.
    pub fn highlight(&mut self, id: Option<&str>) -> bool {
        if let Some(id) = id
            && !self
                .choices
                .iter()
                .any(|choice| choice.id == id && choice.disabled.is_none() && !choice.danger)
        {
            return false;
        }
        self.selected = id.map(str::to_owned);
        true
    }
    /// Bounded Unicode filter text.
    #[must_use]
    pub fn query(&self) -> &str {
        self.query.text()
    }
    /// Local selection-bound feedback for a host-owned activity/status row.
    #[must_use]
    pub fn notice(&self) -> Option<&str> {
        self.notice.as_deref()
    }
    /// Read host-supplied rows for detail/refresh composition.
    #[must_use]
    pub fn choices(&self) -> &[Choice] {
        &self.choices
    }
    /// Replace the filter text without confirming or changing marked identities.
    pub fn set_query(&mut self, query: &str) {
        if self.filtering() {
            self.query.set_text(&crate::text::clean(query, 4096, false));
            self.refilter();
        }
    }
    /// Whether this instance exposes a query editor.
    #[must_use]
    pub fn filtering(&self) -> bool {
        self.filter == Filter::On || (self.filter == Filter::Auto && self.choices.len() > 7)
    }
    /// Marked identities, including rows hidden by the current filter.
    #[must_use]
    pub fn marked(&self) -> Vec<&str> {
        self.choices
            .iter()
            .filter(|c| self.marked.contains(&c.id))
            .map(|c| c.id.as_str())
            .collect()
    }
    /// Set marks from host state, ignoring missing, disabled and dangerous identities.
    pub fn set_marked(&mut self, ids: &[&str]) {
        self.marked = self
            .choices
            .iter()
            .filter(|c| c.disabled.is_none() && !c.danger && ids.contains(&c.id.as_str()))
            .take(self.maximum.unwrap_or(usize::MAX))
            .map(|c| c.id.clone())
            .collect();
    }
    /// Set host-owned multi-selection bounds. Empty confirmation is allowed by
    /// default. Existing marks above a new maximum are trimmed in source order.
    /// Returns false, without changing state, when the bounds conflict.
    pub fn set_selection_limits(&mut self, minimum: usize, maximum: Option<usize>) -> bool {
        if maximum.is_some_and(|maximum| minimum > maximum) {
            return false;
        }
        self.minimum = minimum;
        self.maximum = maximum;
        let retained: HashSet<_> = self
            .marked()
            .into_iter()
            .take(maximum.unwrap_or(usize::MAX))
            .map(str::to_owned)
            .collect();
        self.marked = retained;
        true
    }
    /// Enable the F3 inspection intent only when the host supplies a detail view.
    pub fn set_inspect_available(&mut self, available: bool) {
        self.inspect = available;
    }
    /// Replace the source status. No loading, retries or processes are started.
    pub fn set_status(&mut self, status: Status) {
        self.status = status;
    }
    /// Replace choices atomically, preserving surviving selection and marks by ID.
    ///
    /// # Errors
    /// Duplicate identities leave all previous state intact.
    pub fn replace_choices(&mut self, mut choices: Vec<Choice>) -> Result<(), DuplicateId> {
        model::validate(&mut choices)?;
        self.choices = choices;
        self.marked.retain(|id| {
            self.choices
                .iter()
                .any(|c| &c.id == id && c.disabled.is_none())
        });
        if !self.filtering() {
            self.query.set_text("");
        }
        self.refilter();
        Ok(())
    }
    fn refilter(&mut self) {
        self.matches = model::matches(&self.choices, self.query());
        if !self.matches.iter().any(|m| {
            self.selected.as_ref() == Some(&self.choices[m.index].id)
                && self.choices[m.index].disabled.is_none()
        }) {
            self.selected = self
                .matches
                .iter()
                .map(|m| &self.choices[m.index])
                .find(|c| c.disabled.is_none() && !c.danger)
                .map(|c| c.id.clone());
        }
        self.offset = 0;
    }
    /// Apply one event; no event acquires terminal input or executes an operation.
    pub fn update(&mut self, event: &Event) -> Intent {
        self.input(event)
    }
    /// Render at content height, capped by the host's area and height policy.
    /// Returns the actual used rectangle so hosts can place following content.
    pub fn render(
        &mut self,
        frame: &mut Frame<'_>,
        area: Rect,
        view: PickerView<'_>,
        focused: bool,
    ) -> Rect {
        self.view(frame, area, view, focused)
    }
    /// Advance the filter editor's explicit clock; owns no timer.
    pub fn tick(&mut self, elapsed: std::time::Duration) {
        self.query.tick(elapsed);
    }
}
