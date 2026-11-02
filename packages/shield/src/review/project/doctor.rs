//! `shield doctor`: status plus additional checks and repair routes.
//! Probes may recover derived cache data; policy and host-setting repairs stay
//! with their owning commands.
use super::{Context, Error};
use crate::review::{project as state, registries};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// The status report with a `checks` list of the problems found.
pub(in crate::review) fn report(context: &Context) -> Result<Value, Error> {
    let store = crate::review::storage::Store::open(Path::new(&context.project_root))?;
    // History inspection already validates the authority checkpoint. If it
    // fails, retain the compact-only recovery report when authority is readable.
    let (current, history_error) = match store.read_history() {
        Ok(current) => (current, None),
        Err(error) => (store.read_checkpoint()?, Some(error)),
    };
    current.check(context)?;
    let meta = state::metadata(&current);
    let mut checks = Vec::new();
    let complete = match history_error {
        Some(error) => Err(error),
        None => state::status_from(context, &current),
    };
    let mut value = match complete {
        Ok(value) => value,
        Err(error) => {
            checks.push(json!({"check":"status","detail":format!("the complete status report is unavailable: {error}"),"repair":"shield policy review"}));
            let mut partial = crate::review::runtime::status_from(&current);
            partial["policy_revision"] = partial["review"]["policy_revision"].clone();
            partial["missing"] = json!(["complete_status"]);
            partial["repair_needed"] = json!(true);
            partial["repair_route"] = json!("shield policy review");
            partial
        }
    };
    let mut problem = |check: &str, detail: String, repair: &str| {
        checks.push(json!({"check":check,"detail":detail,"repair":repair}));
    };

    if let Err(error) = crate::review::storage::Store::open(Path::new(&context.project_root))
        .and_then(|store| store.prepare_current())
    {
        problem(
            "policy_source",
            format!("the retained policy source cannot be prepared: {error}"),
            "shield policy review",
        );
    }

    // The active policy must load as hooks load it, compiled plan included.
    if let Err(error) = crate::review::runtime::consumer::load(context, &|| Ok(()))
        .map_err(crate::runtime::Error::from)
    {
        // Without an activation there is nothing for the cache to rebuild from,
        // and preparing the cache repairs only an unavailable compiled plan.
        let repair = policy_route(context, &value["active"], &error);
        problem(
            "policy",
            format!("the active policy cannot be loaded: {error}"),
            &repair,
        );
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
                "agents run {}, not this Shield at {}",
                registered.display(),
                binary.display()
            ),
            &state::agent_setup_route(&value["coverage"]),
        );
    }

    value["checks"] = json!(checks);
    Ok(value)
}

/// Cache preparation repairs an unavailable plan; other policy failures need review.
fn policy_route(context: &Context, active: &Value, error: &crate::runtime::Error) -> String {
    if active.is_null() {
        "shield init".into()
    } else if matches!(error, crate::runtime::Error::CacheUnavailable) {
        crate::review::cache::repair_route(&context.project_root)
    } else {
        "shield policy review".into()
    }
}

/// The binary the project's agent integrations run, when any are registered.
fn registered_binary(meta: &Value) -> Option<PathBuf> {
    meta["integration"]["binary"].as_str().map(PathBuf::from)
}

fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}
