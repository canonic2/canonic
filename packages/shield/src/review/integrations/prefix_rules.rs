//! Shield-owned agent prefix rules files, derived from the active policy.
use super::{Error, files, registration};
use crate::{agents, policy::agent_outputs::CodexPrefix};
use serde_json::Value;
use std::path::Path;

/// Shield never replaces a file it did not write; only moving it repairs this.
const UNOWNED: &str =
    "agent prefix rules file is not Shield's; move it aside, then run shield mcp setup";

/// Each registered adapter's file with its desired contents; None removes it.
fn plan(root: &Path, meta: &Value, prefixes: &[CodexPrefix]) -> Result<Vec<files::Change>, Error> {
    let registered = registration(meta)?;
    let mut changes = Vec::new();
    for adapter in agents::ADAPTERS {
        let Some(path) = adapter.spec().prefix_rules_path else {
            continue;
        };
        let installed = registered
            .as_ref()
            .is_some_and(|r| r.agents.contains_key(adapter.spec().id));
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
pub(crate) fn sync(root: &Path, meta: &Value, prefixes: &[CodexPrefix]) -> Result<(), Error> {
    files::publish(root, &plan(root, meta, prefixes)?)
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
/// applies, from `prefixes`, for each agent the review leaves installed.
pub(crate) fn preview(root: &Path, review: &mut Value, prefixes: &[CodexPrefix]) {
    let agents = review["agents"].clone();
    let installed = |id: &str| {
        agents
            .as_array()
            .into_iter()
            .flatten()
            .any(|a| a["agent"] == id && a["change"] != "removed")
    };
    for adapter in agents::ADAPTERS {
        let Some(path) = adapter.spec().prefix_rules_path else {
            continue;
        };
        let after = if installed(adapter.spec().id) {
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
    diagnostics: &mut Vec<&'static str>,
) {
    match plan(root, meta, prefixes) {
        Ok(changes) if changes.is_empty() => {}
        Ok(_) => diagnostics
            .push("agent prefix rules differ from the active policy; run shield mcp setup"),
        Err(Error::Invalid(message)) => diagnostics.push(message),
        Err(_) => diagnostics.push("agent prefix rules file is unreadable"),
    }
}
