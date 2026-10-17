//! Index cache validity and publication decisions; persistence stays in saved.
use super::timings::Timings;
use crate::{
    files::stamp,
    model::{Directory, Record, Request, Stamp},
    saved::Saved,
};
use anyhow::Result;
use serde_json::Value;
use std::{
    collections::HashMap,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

pub(super) struct Settings {
    changed: bool,
    pub(super) needs_evaluation: bool,
}
impl Settings {
    pub(super) fn check(root: &Path, options: &Value, previous: &Saved) -> Self {
        let config = root.join(
            options
                .get("config")
                .and_then(Value::as_str)
                .unwrap_or("workbench.config.ts"),
        );
        let changed = previous
            .settings
            .iter()
            .any(|(file, saved)| stamp(&root.join(file)) != *saved);
        let needs_evaluation = config.exists() && (previous.settings.is_empty() || changed);
        Self {
            changed,
            needs_evaluation,
        }
    }
    pub(super) fn changed(
        &self,
        previous: &Saved,
        directories_read: usize,
        count: usize,
        reused: usize,
    ) -> bool {
        directories_read > 0
            || reused != count
            || previous.records.len() != count
            || self.needs_evaluation
            || self.changed
    }
    pub(super) fn reuse_answer(
        &self,
        previous: &Saved,
        directories_read: usize,
        count: usize,
        reused: usize,
        generation: &str,
    ) -> bool {
        !self.changed(previous, directories_read, count, reused)
            && !previous.generation.is_empty()
            && generation == previous.generation
    }
}

pub(super) fn generation() -> Result<String> {
    Ok(format!(
        "{}-{}",
        std::process::id(),
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
    ))
}

pub(super) fn store(
    request: &Request,
    records: HashMap<String, Record>,
    directories: HashMap<String, Directory>,
    settings: Vec<(String, Stamp)>,
    ignores: Vec<(String, Stamp)>,
    generation: String,
    timings: &mut Timings,
) {
    let mut saved = Saved::new(records, directories, settings, ignores, generation);
    if let Err(error) = saved.save(&request.saved, &request.identity) {
        timings.save_error(&error);
    }
}
