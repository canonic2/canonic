//! Versioned bounded persistence and filesystem stamps. Invalid saves are misses.
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fs, path::Path, time::UNIX_EPOCH};

const VERSION: u32 = 2;
pub(crate) const MAX_BYTES: u64 = 32 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Stamp(Option<(u128, u64, bool)>);

pub(crate) fn stamp(file: &Path) -> Stamp {
    Stamp(fs::metadata(file).ok().and_then(|meta| {
        Some((
            meta.modified()
                .ok()?
                .duration_since(UNIX_EPOCH)
                .ok()?
                .as_nanos(),
            meta.len(),
            meta.is_dir(),
        ))
    }))
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub(crate) struct Directory {
    pub(crate) stamp: Option<Stamp>,
    pub(crate) children: Vec<(String, bool)>,
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Record {
    pub(crate) file: String,
    pub(crate) watched: Vec<(String, Stamp)>,
    /// JSON entry, validated and projected, or the compiler's error message.
    pub(crate) entry: Result<String, String>,
    pub(crate) docs: bool,
}

#[derive(Default, Serialize, Deserialize)]
pub(crate) struct Saved {
    version: u32,
    identity: String,
    pub(crate) directories: HashMap<String, Directory>,
    pub(crate) records: HashMap<String, Record>,
    pub(crate) settings: Vec<(String, Stamp)>,
    pub(crate) generation: String,
}

impl Saved {
    pub(crate) fn load(file: &Path, identity: &str) -> Self {
        let load = || {
            if fs::metadata(file).ok()?.len() > MAX_BYTES {
                return None;
            }
            let saved: Self = postcard::from_bytes(&fs::read(file).ok()?).ok()?;
            (saved.version == VERSION && saved.identity == identity).then_some(saved)
        };
        load().unwrap_or_default()
    }

    pub(crate) fn save(&mut self, file: &Path, identity: &str) -> std::io::Result<()> {
        self.version = VERSION;
        self.identity = identity.into();
        let bytes = postcard::to_allocvec(self).map_err(std::io::Error::other)?;
        if bytes.len() as u64 > MAX_BYTES {
            return Ok(());
        }
        let Some(parent) = file.parent() else {
            return Ok(());
        };
        fs::create_dir_all(parent)?;
        let temporary = file.with_extension(format!("{}.tmp", std::process::id()));
        let write = || {
            fs::write(&temporary, bytes)?;
            fs::rename(&temporary, file)
        };
        let result = write();
        if result.is_err() {
            let _ = fs::remove_file(temporary);
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn corrupt_or_other_version_is_a_miss() {
        let file = std::env::temp_dir().join(format!("workbench-save-{}", std::process::id()));
        fs::write(&file, b"partial").unwrap();
        assert!(Saved::load(&file, "a").records.is_empty());
        let mut saved = Saved::default();
        saved.records.insert(
            "page".into(),
            Record {
                file: "page".into(),
                watched: vec![],
                entry: Ok("{}".into()),
                docs: false,
            },
        );
        saved.save(&file, "a").unwrap();
        assert_eq!(Saved::load(&file, "a").records.len(), 1);
        assert!(Saved::load(&file, "b").records.is_empty());
        fs::remove_file(file).unwrap();
    }
}
