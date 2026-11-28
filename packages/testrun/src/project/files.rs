//! Bounded fingerprints of declared file effects; never restores project files.
use super::*;
use sha2::{Digest, Sha256};
use std::{fs, io::Read, path::PathBuf};
pub(super) fn relative(value: &str) -> Result<()> {
    if value.is_empty()
        || Path::new(value)
            .components()
            .any(|p| !matches!(p, std::path::Component::Normal(_)))
    {
        return Err(Error::invalid(
            "Task effects must be relative project paths without dot or parent components",
        ));
    }
    Ok(())
}
pub(crate) fn capture(root: &Path, effects: &[String]) -> Value {
    let mut queue: Vec<PathBuf> = effects.iter().map(|p| root.join(p)).collect();
    let mut entries = BTreeMap::new();
    let mut budget = 16 * 1024 * 1024;
    let mut complete = true;
    while let Some(path) = queue.pop() {
        if entries.len() + queue.len() > 1024 {
            complete = false;
            break;
        }
        let label = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .into_owned();
        // Refuse traversal through intermediate symlinks as well as leaf links.
        let mut ancestor = path.as_path();
        let mut symlink = false;
        while ancestor != root {
            if fs::symlink_metadata(ancestor).is_ok_and(|m| m.file_type().is_symlink()) {
                symlink = true;
                break;
            }
            let Some(parent) = ancestor.parent() else {
                symlink = true;
                break;
            };
            ancestor = parent;
        }
        if symlink {
            entries.insert(label, json!({"state":"uninspected_symlink"}));
            complete = false;
            continue;
        }
        match fs::symlink_metadata(&path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                entries.insert(label, json!({"state":"absent"}));
            }
            Ok(metadata) if metadata.is_dir() => {
                entries.insert(label, json!({"state":"directory"}));
                match fs::read_dir(&path) {
                    Ok(children) => {
                        for child in children.take(1025) {
                            if let Ok(child) = child {
                                queue.push(child.path());
                            } else {
                                complete = false;
                            }
                            if queue.len() + entries.len() > 1024 {
                                complete = false;
                                break;
                            }
                        }
                    }
                    Err(_) => complete = false,
                }
            }
            Ok(metadata) if metadata.is_file() && metadata.len() <= budget => {
                let mut bytes = Vec::new();
                match fs::File::open(&path).and_then(|f| f.take(budget + 1).read_to_end(&mut bytes))
                {
                    Ok(_) if bytes.len() as u64 <= budget => {
                        budget -= bytes.len() as u64;
                        entries.insert(
                            label,
                            json!({"state":"file","sha256":format!("{:x}",Sha256::digest(&bytes))}),
                        );
                    }
                    _ => {
                        complete = false;
                        entries.insert(label, json!({"state":"unknown"}));
                    }
                }
            }
            _ => {
                complete = false;
                entries.insert(label, json!({"state":"unknown"}));
            }
        }
    }
    json!({"complete":complete,"entries":entries})
}
pub(crate) fn changes(before: &Value, after: &Value) -> Value {
    let mut keys = std::collections::BTreeSet::new();
    for state in [before, after] {
        if let Some(entries) = state["entries"].as_object() {
            keys.extend(entries.keys());
        }
    }
    let mut changed: Vec<_> = keys
        .into_iter()
        .filter(|key| before["entries"][*key] != after["entries"][*key])
        .collect();
    let truncated = changed.len() > 128;
    changed.truncate(128);
    json!({"paths":changed,"complete":!truncated && before["complete"]==true && after["complete"]==true,"truncated":truncated,"attribution":"observed_during_task; concurrent edits may contribute","restored":false})
}
