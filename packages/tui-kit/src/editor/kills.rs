//! Owns instance-local kill history and atomic yank replacement, including chips.

use super::{Document, Editor, Outcome, history::EditKind};
use std::{collections::BTreeMap, ops::Range};
use unicode_segmentation::UnicodeSegmentation;

#[derive(Clone, Copy)]
pub(super) struct Group {
    cursor: usize,
    forward: bool,
    at: std::time::Duration,
}

impl Editor {
    pub(super) fn kill_line(&mut self, forward: bool) -> Outcome {
        let cursor = self.cursor();
        let mut edge = if forward {
            self.text()[cursor..]
                .find('\n')
                .map_or(self.text().len(), |i| cursor + i)
        } else {
            self.text()[..cursor].rfind('\n').map_or(0, |i| i + 1)
        };
        if edge == cursor {
            edge = if forward {
                self.next()
            } else {
                self.previous()
            };
        }
        self.kill(
            self.selection()
                .unwrap_or(cursor.min(edge)..cursor.max(edge)),
        )
    }
    pub(super) fn kill(&mut self, range: Range<usize>) -> Outcome {
        if range.is_empty() {
            return Outcome::Handled;
        }
        let text = self.text()[range.clone()].to_owned();
        let killed = Document {
            cursor: text.len(),
            atoms: self
                .draft
                .atoms
                .iter()
                .filter(|(c, _)| text.contains(**c))
                .map(|(c, a)| (*c, a.clone()))
                .collect(),
            text,
        };
        let forward = range.start == self.cursor();
        let grouped = self.selection().is_none()
            && self.kill_group.is_some_and(|group| {
                group.cursor == self.cursor()
                    && group.forward == forward
                    && self.now.saturating_sub(group.at) < std::time::Duration::from_secs(1)
            });
        if grouped && let Some(previous) = self.kills.front_mut() {
            if forward {
                previous.text.push_str(&killed.text);
            } else {
                previous.text.insert_str(0, &killed.text);
            }
            previous.atoms.extend(killed.atoms);
            previous.cursor = previous.text.len();
        } else {
            self.kills.push_front(killed);
        }
        self.kills.truncate(10);
        let selected = self.selection().is_some();
        let result = self.delete_range(range, EditKind::Delete);
        self.kill_group = (!selected).then_some(Group {
            cursor: self.cursor(),
            forward,
            at: self.now,
        });
        result
    }
    pub(super) fn space_boundary(&self) -> usize {
        let mut start = self.cursor();
        let mut found = false;
        for (i, g) in self.text()[..self.cursor()].grapheme_indices(true).rev() {
            let whitespace = g.chars().all(char::is_whitespace);
            if whitespace && found {
                break;
            }
            found |= !whitespace;
            start = i;
        }
        start
    }
    pub(super) fn word_part(&self) -> usize {
        let parts: Vec<_> = self.text()[..self.cursor()]
            .grapheme_indices(true)
            .collect();
        let mut end = parts.len();
        while end > 0 && !parts[end - 1].1.chars().any(char::is_alphanumeric) {
            end -= 1;
        }
        if end == 0 {
            return 0;
        }
        let mut start = end - 1;
        while start > 0 {
            let left = parts[start - 1].1.chars().next().unwrap_or(' ');
            let right = parts[start].1.chars().next().unwrap_or(' ');
            let next = parts.get(start + 1).and_then(|(_, g)| g.chars().next());
            if !left.is_alphanumeric()
                || (left.is_lowercase() && right.is_uppercase())
                || left.is_numeric() != right.is_numeric()
                || (left.is_uppercase()
                    && right.is_uppercase()
                    && next.is_some_and(char::is_lowercase))
            {
                break;
            }
            start -= 1;
        }
        parts[start].0
    }
    pub(super) fn yank(&mut self, cycle: bool) -> Outcome {
        let (range, index) = if cycle {
            let Some((range, index)) = &self.yanked else {
                return Outcome::Handled;
            };
            (range.clone(), (index + 1) % self.kills.len().max(1))
        } else {
            (self.selection().unwrap_or(self.cursor()..self.cursor()), 0)
        };
        let Some(killed) = self.kills.get(index).cloned() else {
            return Outcome::Handled;
        };
        let mut base = self.document();
        base.text.replace_range(range.clone(), "");
        base.atoms.retain(|c, _| base.text.contains(*c));
        let (killed, dropped) = fit_yank(killed, &base, self.limit);
        if self.reject_overflow && dropped > 0 {
            self.rejected = true;
            self.message("yank rejected · insertion exceeds byte limit".into());
            return Outcome::Rejected;
        }
        if killed.text.is_empty() {
            self.message(format!(
                "yank refused · byte or chip limit · {dropped} bytes left out"
            ));
            return Outcome::Rejected;
        }
        let mut remap = BTreeMap::new();
        for (old, atom) in &killed.atoms {
            let Some(marker) = ('\u{e000}'..='\u{f8ff}').find(|c| {
                !base.text.contains(*c) && !base.atoms.contains_key(c) && !killed.text.contains(*c)
            }) else {
                return Outcome::Rejected;
            };
            remap.insert(*old, marker);
            base.atoms.insert(marker, atom.clone());
        }
        let inserted: String = killed
            .text
            .chars()
            .map(|c| remap.get(&c).copied().unwrap_or(c))
            .collect();
        let end = range.start + inserted.len();
        base.text.insert_str(range.start, &inserted);
        base.cursor = base
            .text
            .grapheme_indices(true)
            .map(|(i, _)| i)
            .find(|i| *i >= end)
            .unwrap_or(base.text.len());
        let outcome = self.replace_document(&base);
        if outcome == Outcome::Rejected {
            if cycle {
                self.yanked = Some((range, index));
            }
            self.message("yank refused · byte or chip limit".into());
        } else {
            let boundary = |offset| {
                offset == base.text.len()
                    || base.text.grapheme_indices(true).any(|(i, _)| i == offset)
            };
            if boundary(range.start) && boundary(end) {
                self.yanked = Some((range.start..end, index));
            }
            self.message(if dropped > 0 {
                format!("yank cut at limit · {dropped} bytes left out · alt+y cycles")
            } else {
                format!(
                    "yanked {} of {} · alt+y cycles",
                    index + 1,
                    self.kills.len()
                )
            });
        }
        outcome
    }
}

fn expanded_bytes(document: &Document) -> usize {
    document
        .text
        .chars()
        .map(|c| {
            document
                .atoms
                .get(&c)
                .map_or(c.len_utf8(), |atom| atom.content.len())
        })
        .sum()
}

fn fit_yank(mut killed: Document, base: &Document, limit: usize) -> (Document, usize) {
    let original = expanded_bytes(&killed);
    let mut available = limit.saturating_sub(expanded_bytes(base));
    let mut blocks = 256_usize.saturating_sub(base.atoms.len());
    let mut end = 0;
    for (byte, grapheme) in killed.text.grapheme_indices(true) {
        let cost: usize = grapheme
            .chars()
            .map(|c| {
                killed
                    .atoms
                    .get(&c)
                    .map_or(c.len_utf8(), |a| a.content.len())
            })
            .sum();
        let atoms = grapheme
            .chars()
            .filter(|c| killed.atoms.contains_key(c))
            .count();
        if cost > available || atoms > blocks {
            break;
        }
        available -= cost;
        blocks -= atoms;
        end = byte + grapheme.len();
    }
    killed.text.truncate(end);
    killed.atoms.retain(|c, _| killed.text.contains(*c));
    let dropped = original.saturating_sub(expanded_bytes(&killed));
    (killed, dropped)
}
