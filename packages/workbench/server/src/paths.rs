//! Shared lexical path resolution and project-relative display paths; no filesystem I/O.
use std::path::{Component, Path, PathBuf};

pub(crate) fn resolve(base: &Path, entry: &str) -> PathBuf {
    let mut result = PathBuf::new();
    for part in base.join(entry).components() {
        match part {
            Component::ParentDir => {
                result.pop();
            }
            Component::CurDir => {}
            other => result.push(other),
        }
    }
    result
}
pub(crate) fn relative(root: &Path, file: &Path) -> String {
    file.strip_prefix(root)
        .unwrap_or(file)
        .to_string_lossy()
        .replace('\\', "/")
}
