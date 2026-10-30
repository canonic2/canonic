//! Owns rich choice identities, selection modes and deterministic match ranges.

use crate::text::clean;
use std::{fmt, ops::Range};
use unicode_segmentation::UnicodeSegmentation;

/// Host-supplied row; the identity is returned unchanged in picker intents.
#[derive(Clone, Debug)]
pub struct Choice {
    /// Stable, unique identity.
    pub id: String,
    /// Primary text.
    pub label: String,
    /// Optional aligned description.
    pub detail: String,
    /// Optional group heading, in supplied order.
    pub group: String,
    /// Explanation that makes this row unselectable.
    pub disabled: Option<String>,
    /// Error-colored label; never chosen as an implicit default.
    pub danger: bool,
    /// Host's current value, independent of the keyboard cursor.
    pub current: bool,
}

impl Choice {
    /// Construct an enabled row with no optional metadata.
    #[must_use]
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            detail: String::new(),
            group: String::new(),
            disabled: None,
            danger: false,
            current: false,
        }
    }

    pub(super) fn sanitize(&mut self) {
        self.label = clean(&self.label, 4096, false);
        self.detail = clean(&self.detail, 4096, false);
        self.group = clean(&self.group, 128, false);
        self.disabled = self.disabled.as_ref().map(|s| clean(s, 4096, false));
    }
}

/// Explicit single or multiple selection semantics.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SelectionMode {
    /// Enter or an available digit returns one identity.
    #[default]
    Single,
    /// Space marks rows; Enter returns the entire marked set.
    Multiple,
}

/// Host's filter-row policy, resolved when choices are supplied.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Filter {
    /// Show the row above seven choices.
    #[default]
    Auto,
    /// Always show a query editor.
    On,
    /// No query editor; digits select directly.
    Off,
}

/// Host-owned source status; non-ready lists never confirm.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Status {
    /// Supplied choices are available, including an empty source.
    #[default]
    Ready,
    /// Static working text; the component has no timer or loader.
    Loading(String),
    /// Local failure text; retry remains a host action.
    Error(String),
}

/// Input validation failure; the previous picker remains intact on refresh.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DuplicateId(pub String);

impl fmt::Display for DuplicateId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "duplicate choice identity: {}", self.0)
    }
}
impl std::error::Error for DuplicateId {}

pub(super) fn validate(choices: &mut [Choice]) -> Result<(), DuplicateId> {
    let mut ids = std::collections::HashSet::new();
    for choice in choices {
        if !ids.insert(choice.id.clone()) {
            return Err(DuplicateId(choice.id.clone()));
        }
        choice.sanitize();
    }
    Ok(())
}

#[derive(Clone)]
pub(super) struct Match {
    pub index: usize,
    pub score: usize,
    pub detail_only: bool,
    pub label: Vec<Range<usize>>,
    pub detail: Vec<Range<usize>>,
}

fn subsequence(text: &str, query: &[char]) -> Option<(usize, Vec<Range<usize>>)> {
    let mut next = 0;
    let mut score = 0;
    let mut ranges = Vec::new();
    for (position, (byte, grapheme)) in text.grapheme_indices(true).enumerate() {
        for c in grapheme.chars().flat_map(char::to_lowercase) {
            if query.get(next) == Some(&c) {
                next += 1;
                score += position;
                let range = byte..byte + grapheme.len();
                if ranges.last() != Some(&range) {
                    ranges.push(range);
                }
                if next == query.len() {
                    return Some((score, ranges));
                }
            }
        }
    }
    None
}

fn ranked_match(text: &str, query: &[char]) -> Option<(usize, Vec<Range<usize>>)> {
    let (_, fallback) = subsequence(text, query)?;
    for (byte, _) in text.grapheme_indices(true) {
        if byte > 0
            && text[..byte]
                .chars()
                .next_back()
                .is_some_and(char::is_alphanumeric)
        {
            continue;
        }
        let Some((_, mut ranges)) = subsequence(&text[byte..], query) else {
            continue;
        };
        if ranges.first().is_some_and(|r| r.start == 0)
            && ranges.windows(2).all(|pair| pair[0].end == pair[1].start)
        {
            for range in &mut ranges {
                range.start += byte;
                range.end += byte;
            }
            return Some((usize::from(byte > 0), ranges));
        }
    }
    Some((2, fallback))
}

pub(super) fn matches(choices: &[Choice], query: &str) -> Vec<Match> {
    let query: Vec<_> = query
        .chars()
        .filter(|c| !c.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect();
    let mut found = Vec::new();
    for (index, choice) in choices.iter().enumerate() {
        let candidate = if query.is_empty() {
            Some((0, false, Vec::new(), Vec::new()))
        } else if let Some((score, ranges)) = ranked_match(&choice.label, &query) {
            Some((score, false, ranges, Vec::new()))
        } else {
            ranked_match(&choice.detail, &query)
                .map(|(score, ranges)| (score, true, Vec::new(), ranges))
        };
        if let Some((score, detail_only, label, detail)) = candidate {
            found.push(Match {
                index,
                score,
                detail_only,
                label,
                detail,
            });
        }
    }
    found.sort_by_key(|m| (m.detail_only, m.score, m.index));
    found
}
