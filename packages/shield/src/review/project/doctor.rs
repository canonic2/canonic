//! `shield doctor`: status plus additional checks and repair routes.
//! Probes inspect retained authority without publishing upgrades or cache data.
use super::{Context, Error};
use crate::review::{project as state, registries};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// The status report with a `checks` list of the problems found.
pub(in crate::review) fn report(context: &Context) -> Result<Value, Error> {
    let store = crate::review::storage::Store::inspect(Path::new(&context.project_root))?;
    let (current, upgrade_pending) = store.read_diagnostic_checkpoint()?;
    current.check_inspection(context)?;
    let meta = state::metadata(&current);
    let mut checks = storage_checks(context);
    let mut value = state::status_from(context, &current);
    value["enabled"] = json!(current.settings.enabled());
    value["checkpoint_upgrade_pending"] = json!(upgrade_pending);
    let mut problem = |check: &str, detail: String, repair: &str| {
        checks.push(json!({"check":check,"detail":detail,"repair":repair}));
    };

    if upgrade_pending {
        problem(
            "checkpoint_upgrade",
            "The historical checkpoint is validated in memory; conversion has not been saved."
                .into(),
            "The next Shield mutation or hook will save the conversion. If saving fails, check write access and free space in the project review directory.",
        );
    }

    // Requests are decided by the policy files; report whether they load.
    let cache = crate::review::files::cache_for(&current.operator).ok();
    match crate::review::files::check(Path::new(&context.project_root), cache.as_deref()) {
        Ok(digest) => value["policy_files"] = json!({"digest":digest}),
        Err(error) => {
            value["policy_files"] = json!({"error":error.to_string()});
            problem(
                "policy",
                error.to_string(),
                "Fix the policy files, then run shield doctor",
            );
        }
    }

    // Pack updates are verified against retained registry checkpoints and the
    // enrolled source each pack came from.
    if serde_json::from_value::<canonic_registry_kit::checkpoint::Checkpoints>(json!(
        current.registry_checkpoints
    ))
    .is_err()
    {
        problem(
            "sources",
            "the retained registry checkpoints are invalid".into(),
            "shield registry list",
        );
    }
    match registries::effective_from(&current.settings) {
        Ok(effective) => {
            for row in meta["installed"].as_array().into_iter().flatten() {
                let identity = row["source_identity"].as_str().unwrap_or_default();
                if row["source_alias"] != "project-files"
                    && effective.by_identity(identity).is_none()
                {
                    problem(
                        "sources",
                        format!(
                            "pack {} comes from a registry that is no longer enrolled",
                            row["id"].as_str().unwrap_or("?")
                        ),
                        "shield registry add",
                    );
                }
            }
        }
        Err(error) => problem(
            "sources",
            format!("registry registrations cannot be read: {error}"),
            "shield registry list",
        ),
    }

    // Agents run the binary setup registered; another one means hooks and
    // this command may disagree about the policy format.
    if let (Ok(binary), Some(registered)) = (std::env::current_exe(), registered_binary(&meta))
        && canonical(&registered) != canonical(&binary)
    {
        problem(
            "binary",
            format!(
                "Registered executable is {}; this CLI is {}. Content identity and compatibility were not inspected.",
                registered.display(),
                binary.display()
            ),
            &state::agent_setup_route(&value["coverage"]),
        );
    }

    value["checks"] = json!(checks);
    state::inspection_location(context, &current, &mut value);
    Ok(value)
}

/// Report preserved storage anomalies without blocking unrelated operations.
fn storage_checks(context: &Context) -> Vec<Value> {
    let mut checks = Vec::new();
    if crate::layout::split_state(Path::new(&context.project_root)) {
        checks.push(json!({"check":"state_folder","blocking":false,"path":".shield/review",
            "detail":"An earlier Shield recreated .shield/review after this folder's state moved to .shield/state; requests it records there are not used. Restart agent sessions still running the earlier version, then remove .shield/review."}));
    }
    let directories = [crate::layout::state(Path::new(&context.project_root)).join("sources")];
    for directory in directories {
        match crate::review::storage::objects::anomalies(&directory) {
            Ok(paths) => {
                for path in paths {
                    checks.push(json!({"check":"object_storage","blocking":false,"path":path,"detail":"Unsafe neighboring entry is preserved and counts toward capacity; unrelated objects remain usable."}));
                }
            }
            Err(error) => checks.push(
                json!({"check":"object_storage","path":directory,"detail":error.to_string()}),
            ),
        }
    }
    checks
}

/// The binary the project's agent integrations run, when any are registered.
fn registered_binary(meta: &Value) -> Option<PathBuf> {
    meta["integration"]["binary"].as_str().map(PathBuf::from)
}

fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}
