//! Settlement of what a lost manager left: records of this record layout
//! whose device actor is gone, such as a simulator or emulator a crashed or
//! killed manager booted. Running actors and leases held elsewhere are left
//! alone. What cannot be settled stays a reported blocker: records and lock
//! anchors are never deleted on the assumption that they are free.
use super::{Caller, Kind, service::Service, storage};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

/// Reconcile this layout's records whose device actor is gone and report
/// what remains, replacing the earlier report.
pub(super) async fn settle(service: Arc<Service>) {
    let mut blockers = Vec::new();
    for directory in unfinished(&service.root) {
        let caller = Caller::new(Some(service.clone()), service.root.clone(), Kind::Cli);
        if let Err(error) = caller.settle_stopped(&directory).await {
            blockers.push(json!({
                "kind":"lost_manager","directory":directory,"cause":error,
                "recoveryAction":"testrun manager retry","expiry":{"unknown":true},
            }));
        }
    }
    service.set_lost(blockers);
}

/// Owner records of this layout under `root`, unparsed. Owner records sit
/// in the target directories of each records directory.
fn records(root: &Path) -> Vec<Value> {
    let mut found = Vec::new();
    let Ok(entries) = std::fs::read_dir(root) else {
        return found;
    };
    let prefix = format!("v{}-", super::WIRE);
    for entry in entries.take(4096).flatten() {
        if !entry.file_name().to_string_lossy().starts_with(&prefix) {
            continue;
        }
        for parent in
            std::iter::once(entry.path()).chain(super::selection::target_directories(&entry.path()))
        {
            let Ok(owners) = std::fs::read_dir(&parent) else {
                continue;
            };
            for owner in owners.take(4096).flatten() {
                if owner.file_name().to_string_lossy().starts_with("o-")
                    && let Ok(Some(record)) =
                        storage::read::<Value>(&owner.path().join("owner.json"))
                {
                    found.push(record);
                }
            }
        }
    }
    found
}

/// The records directory an owner record's directory belongs to.
fn records_of(owner: &Path) -> Option<&Path> {
    let parent = owner.parent()?;
    if parent.file_name().is_some_and(|name| {
        name.to_string_lossy()
            .starts_with(super::selection::TARGET_PREFIX)
    }) {
        parent.parent()
    } else {
        Some(parent)
    }
}

fn incomplete(record: &Value) -> bool {
    record["directory"].as_str().is_some_and(|directory| {
        storage::read::<Value>(&Path::new(directory).join("cleanup.json"))
            .ok()
            .flatten()
            .is_none_or(|cleanup| cleanup["complete"] != true)
    })
}

/// Records directories with unfinished cleanup, as recovery after a manager
/// loss finds them.
pub(super) fn unfinished(root: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = Vec::new();
    for record in records(root) {
        if !incomplete(&record) {
            continue;
        }
        let Some(directory) = record["directory"]
            .as_str()
            .and_then(|directory| records_of(Path::new(directory)))
        else {
            continue;
        };
        if !found.iter().any(|existing| existing == directory) {
            found.push(directory.to_owned());
        }
    }
    found
}

/// Whether durable records show cleanup that recovery may settle: the rule
/// for starting an absent manager for a recovery request.
pub(crate) fn recovery_needed(root: &Path) -> bool {
    records(root).iter().any(incomplete)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn owner(root: &Path, protocol: u32, cleanup: Option<bool>) -> PathBuf {
        let directory = root
            .join(format!("v{protocol}-acme"))
            .join(format!("o-{}", uuid::Uuid::new_v4().simple()));
        storage::directory(directory.parent().unwrap()).unwrap();
        storage::directory(&directory).unwrap();
        storage::write(
            &directory.join("owner.json"),
            &json!({"version":protocol,"task":"acme-task","target":format!("android:Acme-{}", uuid::Uuid::new_v4()),"epoch":"acme-epoch","directory":directory,"setup":false,"created":1}),
        )
        .unwrap();
        if let Some(complete) = cleanup {
            storage::write(
                &directory.join("cleanup.json"),
                &json!({"complete":complete}),
            )
            .unwrap();
        }
        directory
    }

    #[test]
    fn recovery_is_needed_only_for_unfinished_cleanup_of_this_layout() {
        let root = super::super::tests::root();
        let state = root.path().join("state");
        storage::directory(&state).unwrap();
        assert!(!recovery_needed(&state));
        owner(&state, super::super::WIRE, Some(true));
        assert!(!recovery_needed(&state));
        // Records of retired layouts are history, not recovery work.
        owner(&state, 3, None);
        assert!(!recovery_needed(&state));
        owner(&state, super::super::WIRE, None);
        assert!(recovery_needed(&state));
    }
}
