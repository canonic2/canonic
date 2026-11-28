//! Private immutable lifecycle journal entries; requests and secrets are omitted.

use serde_json::Value;
use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

const MAX_ENTRIES: usize = 4096;
const MAX_BYTES: usize = 8 * 1024 * 1024;
const MAX_ENTRY_BYTES: usize = 256 * 1024;

#[derive(Default)]
struct Sequence {
    next: u64,
    bytes: usize,
    entries: VecDeque<usize>,
}

#[derive(Clone)]
pub(crate) struct Journal {
    directory: PathBuf,
    sequence: Arc<Mutex<Sequence>>,
}

impl Journal {
    pub fn create(parent: &Path, id: uuid::Uuid) -> Result<Self, &'static str> {
        crate::artifacts::check_private_directory(parent).map_err(|_| "PRIVATE_STATE_REQUIRED")?;
        let directory = parent.join(format!("browser-{id}"));
        crate::artifacts::directory(&directory).map_err(|_| "STATE_IO")?;
        crate::artifacts::sync_directory(parent).map_err(|_| "STATE_IO")?;
        Ok(Self {
            directory,
            sequence: Arc::new(Mutex::new(Sequence::default())),
        })
    }
    pub async fn record(&self, value: Value) -> Result<(), &'static str> {
        let this = self.clone();
        tokio::task::spawn_blocking(move || {
            let mut sequence = this.sequence.lock().map_err(|_| "STATE_IO")?;
            let bytes = serde_json::to_vec(&value).map_err(|_| "STATE_IO")?;
            if bytes.len() > MAX_ENTRY_BYTES {
                return Err("STATE_CAPACITY");
            }
            let next = sequence.next.checked_add(1).ok_or("STATE_CAPACITY")?;
            while sequence.entries.len() >= MAX_ENTRIES || sequence.bytes + bytes.len() > MAX_BYTES
            {
                let oldest = sequence.next - sequence.entries.len() as u64;
                std::fs::remove_file(this.entry(oldest)).map_err(|_| "STATE_IO")?;
                sequence.bytes -= sequence
                    .entries
                    .pop_front()
                    .expect("Retained journal entry");
            }
            crate::artifacts::write(&this.entry(sequence.next), &bytes).map_err(|_| "STATE_IO")?;
            // Advance before the directory flush. If that flush fails, a later
            // cleanup can still append without colliding with the published file.
            sequence.next = next;
            sequence.bytes += bytes.len();
            sequence.entries.push_back(bytes.len());
            crate::artifacts::sync_directory(&this.directory).map_err(|_| "STATE_IO")?;
            Ok(())
        })
        .await
        .map_err(|_| "STATE_IO")?
    }
    fn entry(&self, sequence: u64) -> PathBuf {
        self.directory.join(format!("{sequence:08}.json"))
    }
    pub fn directory(&self) -> &Path {
        &self.directory
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn browser_journal_rolls_past_two_old_lifetime_limits() {
        let root = tempfile::tempdir().unwrap();
        let private = root.path().join("state");
        crate::artifacts::directory(&private).unwrap();
        let journal = Journal::create(&private, uuid::Uuid::new_v4()).unwrap();
        for sequence in 0..=2 * MAX_ENTRIES {
            journal.record(json!({"sequence":sequence})).await.unwrap();
        }
        assert_eq!(
            std::fs::read_dir(journal.directory()).unwrap().count(),
            MAX_ENTRIES
        );
        assert!(!journal.entry(MAX_ENTRIES as u64).exists());
        let latest: Value = serde_json::from_slice(
            &std::fs::read(journal.entry((2 * MAX_ENTRIES) as u64)).unwrap(),
        )
        .unwrap();
        assert_eq!(latest["sequence"], 2 * MAX_ENTRIES);
        assert!(journal.entry((MAX_ENTRIES + 1) as u64).exists());
    }
}
