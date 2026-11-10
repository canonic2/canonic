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
    if current.legacy_preview.is_some() {
        let mut value = state::status_from(context, &current);
        value["checks"] = json!([{"check":"checkpoint_upgrade","detail":"The historical record shape and recorded activation proof were inspected. Policy source, matcher model, cache and history were not validated.","repair":"The next actual runtime operation validates and converts the retained checkpoint without another policy review solely for its format."}]);
        return Ok(value);
    }
    let meta = state::metadata(&current);
    let mut checks = storage_checks(context, &current);
    let mut value = state::status_from(context, &current);
    value["enabled"] = json!(current.enforcement.enabled);
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

    // Presence/compatibility probes are metadata-only. Payload integrity and
    // source preparation are established by explicit use, never by diagnosis.
    if !value["active"].is_null()
        && (value["compiled_cache"]["presence"] != "present"
            || value["compiled_cache"]["compatibility"] != "current")
    {
        let repair = crate::review::cache::repair_route(&context.project_root);
        problem(
            "policy",
            "The compiled policy is missing or incompatible; payload integrity is checked on use."
                .into(),
            &repair,
        );
    }
    value["policy_validation"] =
        json!({"source":"not-performed","compiled_integrity":"checked-on-use"});

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
    match registries::effective_from_metadata(&meta) {
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
fn storage_checks(context: &Context, current: &crate::review::model::State) -> Vec<Value> {
    let mut checks = Vec::new();
    let mut directories = vec![Path::new(&context.project_root).join(".shield/review/sources")];
    if let Some(active) = &current.active
        && let Some(partition) = active.compiled.path().parent()
    {
        directories.push(partition.to_path_buf());
    }
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
