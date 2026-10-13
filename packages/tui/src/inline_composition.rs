//! Owns ordered write-once host records; rendering, terminal ownership and operations stay outside.

pub use crate::review_ticket::Ticket;
use std::collections::{BTreeMap, BTreeSet};

/// A supplied paragraph, settled card, review receipt or marker in host order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Part {
    pub id: String,
    pub revision: u64,
    pub label: String,
    pub text: String,
    pub settled: bool,
}
/// Exact output batch. Acknowledge only after the terminal accepts it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Batch {
    pub ticket: Ticket,
    pub lines: Vec<String>,
    parts: Vec<Part>,
}
/// Invalid supplied records preserve the accepted order and any outstanding batch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Invalid {
    Identity,
    Revision,
    Limit,
}
impl std::fmt::Display for Invalid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid inline records: {self:?}")
    }
}
impl std::error::Error for Invalid {}
/// Bounded per-turn ledger. Later information appends a named correction rather than rewriting.
#[derive(Default)]
pub struct PrintLedger {
    parts: Vec<Part>,
    printed: BTreeMap<String, Part>,
    pending: Option<Batch>,
    failed: bool,
}
impl PrintLedger {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
    /// Replace authoritative host order without changing already printed text.
    /// # Errors
    /// Duplicate/control identities, stale material revisions and bounded-data overflow are rejected.
    pub fn replace(&mut self, parts: Vec<Part>) -> Result<(), Invalid> {
        let mut ids = BTreeSet::new();
        let mut bytes = 0usize;
        for p in &parts {
            if p.id.is_empty()
                || p.id.len() > 128
                || p.id.chars().any(char::is_control)
                || !ids.insert(p.id.clone())
            {
                return Err(Invalid::Identity);
            }
            bytes = bytes.saturating_add(p.text.len() + p.label.len() + p.id.len());
            if p.text.len() > 60 * 1024 || p.label.len() > 256 || bytes > 1024 * 1024 {
                return Err(Invalid::Limit);
            }
            if let Some(old) = self
                .parts
                .iter()
                .filter(|old| old.id == p.id)
                .chain(self.printed.get(&p.id))
                .chain(
                    self.pending
                        .iter()
                        .flat_map(|b| b.parts.iter())
                        .filter(|old| old.id == p.id),
                )
                .max_by_key(|old| old.revision)
                && (p.revision < old.revision || p.revision == old.revision && p != old)
            {
                return Err(Invalid::Revision);
            }
        }
        ids.extend(self.printed.keys().cloned());
        if ids.len() > 256 || parts.len() > 256 {
            return Err(Invalid::Limit);
        }
        self.parts = parts;
        Ok(())
    }
    /// Produce settled contiguous records once, retaining exact versions across later host updates.
    #[must_use]
    pub fn ready(&mut self) -> Option<Batch> {
        if self.failed || self.pending.is_some() {
            return None;
        }
        let mut parts = vec![];
        let mut lines = vec![];
        for p in &self.parts {
            if !p.settled {
                break;
            }
            match self.printed.get(&p.id) {
                Some(old) if old == p => continue,
                Some(_) => lines.push(format!("↳ {} → {}", p.label, p.text)),
                None => lines.push(p.text.clone()),
            }
            parts.push(p.clone());
        }
        if parts.is_empty() {
            return None;
        }
        let batch = Batch {
            ticket: Ticket::new(),
            parts,
            lines,
        };
        self.pending = Some(batch.clone());
        Some(batch)
    }
    /// Freeze exactly the accepted batch; stale/foreign/altered/duplicate receipts are inert.
    pub fn acknowledge(&mut self, batch: &Batch) -> bool {
        if self.pending.as_ref() != Some(batch) {
            return false;
        }
        for p in &batch.parts {
            self.printed.insert(p.id.clone(), p.clone());
        }
        self.pending = None;
        true
    }
    /// Stop after an ambiguous partial write. No automatic retry can duplicate printed history.
    pub fn abort(&mut self, batch: &Batch) -> bool {
        if self.pending.as_ref() != Some(batch) {
            return false;
        }
        self.pending = None;
        self.failed = true;
        true
    }
    #[must_use]
    pub fn stopped(&self) -> bool {
        self.failed
    }
    /// Records still needing live presentation; caller notices/status are deliberately separate.
    #[must_use]
    pub fn live(&self) -> Vec<&Part> {
        self.parts
            .iter()
            .filter(|p| self.printed.get(&p.id) != Some(*p))
            .collect()
    }
}
