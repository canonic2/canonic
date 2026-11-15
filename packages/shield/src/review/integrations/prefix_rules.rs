//! Shield-owned agent prefix rules files, derived from the active policy.
use super::{Error, Plan, Registration, files, registration};
use crate::{agents, policy::rules::agent_outputs::CodexPrefix};
use serde_json::Value;
use std::path::Path;

/// Shield never replaces a file it did not write; only moving it repairs this.
const UNOWNED: &str =
    "agent prefix rules file is not Shield's; move it aside, then run shield mcp setup";

/// Whether `registration` keeps `agent`'s prefix rules file. Box publishes
/// Codex's hooks but not this file, so Shield keeps writing it for an agent it
/// covers through Box. Preview and publication both decide here.
fn installed(registration: Option<&Registration>, agent: &str) -> bool {
    registration.is_some_and(|r| r.covers(agent))
}

/// Each registered adapter's file with its desired contents; None removes it.
pub(in crate::review) fn plan(
    root: &Path,
    meta: &Value,
    prefixes: &[CodexPrefix],
) -> Result<Vec<files::Change>, Error> {
    plan_for(root, meta, prefixes, None)
}

/// Limit setup maintenance to the agents covered by its confirmed plan.
pub(in crate::review) fn plan_for(
    root: &Path,
    meta: &Value,
    prefixes: &[CodexPrefix],
    scope: Option<&Plan>,
) -> Result<Vec<files::Change>, Error> {
    let registered = registration(meta)?;
    let mut changes = Vec::new();
    for adapter in agents::ADAPTERS {
        if scope.is_some_and(|plan| !plan.affects(adapter.spec().id)) {
            continue;
        }
        let Some(path) = adapter.spec().prefix_rules_path else {
            continue;
        };
        let installed = installed(registered.as_ref(), adapter.spec().id);
        let before = files::read(root, path)?;
        if before
            .as_deref()
            .is_some_and(|s| !adapter.owns_prefix_rules(s))
        {
            if installed {
                return Err(Error::Invalid(UNOWNED));
            }
            continue;
        }
        let after = installed.then(|| adapter.prefix_rules(prefixes)).flatten();
        if before != after {
            changes.push(files::Change {
                path: path.into(),
                before,
                after,
            });
        }
    }
    Ok(changes)
}

/// Write, update or remove each adapter's prefix rules for the given policy.
/// The caller holds the projection-publication lease and supplies the checked mode.
pub(crate) fn sync(
    root: &Path,
    meta: &Value,
    prefixes: &[CodexPrefix],
    enabled: bool,
) -> Result<(), Error> {
    sync_for(root, meta, prefixes, enabled, None)
}

/// Setup uses its reviewed scope; policy and mode changes use all agents.
pub(in crate::review) fn sync_for(
    root: &Path,
    meta: &Value,
    prefixes: &[CodexPrefix],
    enabled: bool,
    scope: Option<&Plan>,
) -> Result<(), Error> {
    let prefixes = if enabled { prefixes } else { &[] };
    files::publish(root, &plan_for(root, meta, prefixes, scope)?)
}

/// The prefixes of the current activation, from its prepared snapshot.
pub(in crate::review) fn active(root: &Path) -> Result<Vec<CodexPrefix>, Error> {
    let snapshot = crate::runtime::Project::open(root)
        .and_then(|project| project.snapshot())
        .map_err(|_| Error::Invalid("active policy is unavailable"))?;
    Ok(snapshot.codex_prefixes().to_vec())
}

/// Add the prefix rules files an integration review leads to, and the prefixes
/// they hold, to its preview. They are written after the confirmed plan
/// applies, from `prefixes`, for each agent its `desired` registration covers.
/// The caller validates the mode; first enrollment explicitly starts enabled.
pub(in crate::review) fn preview(
    root: &Path,
    review: &mut Value,
    desired: Option<&Registration>,
    prefixes: &[CodexPrefix],
    enabled: bool,
) {
    preview_for(root, review, desired, prefixes, enabled, None);
}

/// Preview only prefix files included in the setup plan's maintenance scope.
pub(in crate::review) fn preview_for(
    root: &Path,
    review: &mut Value,
    desired: Option<&Registration>,
    prefixes: &[CodexPrefix],
    enabled: bool,
    scope: Option<&Plan>,
) {
    let prefixes = if enabled { prefixes } else { &[] };
    for adapter in agents::ADAPTERS {
        if scope.is_some_and(|plan| !plan.affects(adapter.spec().id)) {
            continue;
        }
        let Some(path) = adapter.spec().prefix_rules_path else {
            continue;
        };
        let after = if installed(desired, adapter.spec().id) {
            adapter.prefix_rules(prefixes)
        } else {
            None
        };
        // An unreadable or unowned file refuses when the plan finishes.
        let before = files::read(root, path).ok().flatten();
        if before.is_none() && after.is_none() {
            continue;
        }
        if let Some(files) = review["files"].as_array_mut() {
            files.push(serde_json::json!({"path":path,"before":before,"after":after}));
        }
        if after.is_some() {
            review["codex_prefixes"] = serde_json::json!(prefixes);
        }
    }
}

/// Whether an agent's prefix rules file holds anything other than what this
/// policy would write. Codex may then skip its sandbox for commands the policy
/// does not know about, so the caller cannot rely on policy prefixes alone.
/// A missing file grants nothing; an unreadable one counts as drift.
pub(crate) fn drifted(root: &Path, prefixes: &[CodexPrefix]) -> bool {
    agents::ADAPTERS.iter().any(|adapter| {
        adapter
            .spec()
            .prefix_rules_path
            .is_some_and(|path| match files::read(root, path) {
                Ok(None) => false,
                Ok(Some(text)) => Some(text) != adapter.prefix_rules(prefixes),
                Err(_) => true,
            })
    })
}

/// Report a missing, stale, edited or unowned prefix rules file.
pub(in crate::review) fn diagnose(
    root: &Path,
    meta: &Value,
    prefixes: &[CodexPrefix],
    enabled: bool,
    diagnostics: &mut Vec<&'static str>,
) {
    let prefixes = if enabled { prefixes } else { &[] };
    match plan(root, meta, prefixes) {
        Ok(changes) if changes.is_empty() => {}
        Ok(_) => diagnostics
            .push("agent prefix rules differ from the active policy; run shield mcp setup"),
        Err(Error::Invalid(message)) => diagnostics.push(message),
        Err(_) => diagnostics.push("agent prefix rules file is unreadable"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser as _;

    #[test]
    fn targeted_prefix_maintenance_skips_unowned_and_unreadable_omitted_files() {
        for unreadable in [false, true] {
            let root = assert_fs::TempDir::new().unwrap();
            let mut setup = super::super::setup::prepare_initial(
                root.path(),
                super::super::IntegrationChoice::agent("codex"),
            )
            .unwrap()
            .unwrap();
            setup.targets = Some(vec!["claude".into()]);
            let meta = serde_json::json!({"integration":setup.desired});
            let path = root.path().join(".codex/rules/shield.rules");
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            if unreadable {
                std::fs::create_dir(&path).unwrap();
            } else {
                std::fs::write(&path, "foreign rules remain untouched").unwrap();
            }
            let mut review = serde_json::json!({"files":[]});
            preview_for(
                root.path(),
                &mut review,
                setup.desired.as_ref(),
                &[],
                true,
                Some(&setup),
            );
            assert_eq!(review, serde_json::json!({"files":[]}));
            assert!(
                plan_for(root.path(), &meta, &[], Some(&setup))
                    .unwrap()
                    .is_empty()
            );
            sync_for(root.path(), &meta, &[], true, Some(&setup)).unwrap();
            assert!(plan(root.path(), &meta, &[]).is_err());
            setup.targets = Some(vec!["codex".into()]);
            assert!(plan_for(root.path(), &meta, &[], Some(&setup)).is_err());
            if unreadable {
                assert!(path.is_dir());
            } else {
                assert_eq!(
                    std::fs::read_to_string(path).unwrap(),
                    "foreign rules remain untouched"
                );
            }
        }
    }

    #[test]
    fn shield_mode_initial_prefix_preview_requires_no_enrollment() {
        let root = assert_fs::TempDir::new().unwrap();
        let plan = super::super::setup::prepare_initial(
            root.path(),
            super::super::IntegrationChoice::agent("codex"),
        )
        .unwrap()
        .unwrap();
        let prefixes = vec![CodexPrefix {
            owner: "custom".into(),
            rule: "logs".into(),
            pattern: vec![vec!["acme".into()], vec!["logs".into()]],
        }];
        for enabled in [true, false] {
            let mut review = super::super::setup::preview(root.path(), Some(&plan));
            preview(
                root.path(),
                &mut review,
                plan.desired.as_ref(),
                &prefixes,
                enabled,
            );
            let file = review["files"]
                .as_array()
                .unwrap()
                .iter()
                .find(|file| file["path"] == ".codex/rules/shield.rules");
            if enabled {
                assert!(file.unwrap()["after"].as_str().unwrap().contains("acme"));
            } else {
                assert!(file.is_none());
            }
        }
        assert!(!root.path().join(".shield/state").exists());
        assert!(!root.path().join(".codex/rules/shield.rules").exists());
    }

    #[test]
    fn shield_mode_prefix_sync_reuses_the_locked_checkpoint() {
        let root = assert_fs::TempDir::new().unwrap();
        let bundle =
            crate::configuration::Bundle::capture("schema_version: 1\npacks: []\n", &[], None)
                .unwrap();
        let context = crate::review::mcp::initialize_fixture(root.path(), &bundle);
        let logs = assert_fs::TempDir::new().unwrap();
        let cli = crate::cli::Cli::parse_from([
            "shield",
            "--debug-log-dir",
            logs.path().to_str().unwrap(),
            "status",
        ]);
        let log = std::sync::Arc::new(crate::diagnostics::Log::open(&cli));
        for enabled in [true, false] {
            crate::review::mode::Prepared::new(&context, enabled)
                .unwrap()
                .commit(&context, &|| Ok(()))
                .unwrap();
            let request = log.request("command", "mcp_setup", None, None).unwrap();
            {
                let _scope = request.enter();
                crate::review::storage::Store::open(root.path())
                    .unwrap()
                    .inspect_checkpoint(&context, &|| Ok(()), |state| {
                        sync(root.path(), &state.operator, &[], state.settings.enabled())
                    })
                    .unwrap();
                assert_eq!(
                    request.recorded_count("state.checkpoint_reads"),
                    1,
                    "enabled={enabled}"
                );
            }
            request.finish("success");
        }
    }
}
