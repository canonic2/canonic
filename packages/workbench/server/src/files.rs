//! Shared filesystem observations for discovery and index freshness.
use crate::model::Stamp;
use std::{fs, path::Path, time::UNIX_EPOCH};

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

pub(crate) fn watch(root: &Path, file: &Path) -> (String, Stamp) {
    (
        file.strip_prefix(root)
            .unwrap_or(file)
            .to_string_lossy()
            .into_owned(),
        stamp(file),
    )
}
