//! Exact registration maintenance: direct installations, the Hub or Box pin,
//! and a folder's committed Hub or Box definition. Never publisher or
//! permission adoption.
use super::{Error, Plan, State, files, host_definition, native, pending, registration};
use std::path::{Path, PathBuf};

pub(super) const REASON: &str = "equivalent native registration portability";
/// A folder with no registration whose committed Hub or Box definition is
/// exactly the one this executable renders, as in a fresh worktree or clone.
pub(super) const ADOPT_REASON: &str = "committed Hub or Box definition";

/// The configured host whose committed definition a folder carries: Hub
/// before Box, as in setup's default.
fn committed_host(root: &Path) -> Option<&'static host_definition::Host> {
    host_definition::HOSTS.into_iter().find(|host| {
        host_definition::configured(host, root)
            && files::read(root, host.path).ok().flatten().is_some()
    })
}

/// The identity of one adoption attempt: the committed definition's bytes,
/// the private pin's bytes and the executable that would adopt them. `None`
/// without a Hub or Box project or a committed definition, where there is
/// nothing to adopt. Small reads and a hash, so an unchanged attempt that
/// already failed costs no parsing, rendering or binary hashing.
pub(in crate::review) fn adoption_key(root: &Path, executable: &Path) -> Option<String> {
    let host = committed_host(root)?;
    let shared = files::read(root, host.path).ok()??;
    let local = files::read(root, host.local_path).ok()?;
    // This is only a negative-cache invalidator, never executable authority.
    // A replacement at the same path (including a same-version development
    // build) must retry a refusal made by an older implementation.
    let metadata = std::fs::metadata(executable).ok()?;
    let modified = metadata
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_nanos()
        .to_string();
    let key = serde_json::json!([
        host.id,
        shared,
        local,
        executable.to_str(),
        env!("CARGO_PKG_VERSION"),
        metadata.len(),
        modified
    ])
    .to_string();
    Some(crate::policy::fingerprint(key.as_bytes()))
}

/// The registration `shield mcp setup --agent <host>` would record for the
/// committed definition, writing only a missing private pin. The definition
/// is policy-equivalent to that setup: the same text grants the same tools.
pub(in crate::review) fn adopt_host(
    root: &Path,
    state: &State,
    executable: &Path,
) -> Result<Option<Plan>, Error> {
    if registration(&state.operator)?.is_some() || pending(&state.operator)?.is_some() {
        return Ok(None);
    }
    let Some(host) = committed_host(root) else {
        return Ok(None);
    };
    let Some((definition, approved)) = host_definition::committed(host, root, executable)? else {
        return Ok(None);
    };
    let mut desired = super::setup::desired(root)?;
    if desired.binary != executable {
        return Ok(None);
    }
    let mut changes = Vec::new();
    if files::read(root, host.local_path)?.is_none() {
        changes.push(files::Change {
            path: host.local_path.into(),
            before: None,
            after: Some(definition.local.clone()),
        });
    }
    desired.approved_tools = Some(approved);
    desired.definition = Some(definition);
    let before = super::read_file(root)?;
    let after = before.clone().unwrap_or_default();
    let plan = Plan {
        targets: Some(vec![host.id.into()]),
        scope_reason: Some(ADOPT_REASON.into()),
        policy_revision: Some(state.policy_revision.clone()),
        before,
        after,
        previous: None,
        desired: Some(desired),
        files: changes,
        handed_over: Vec::new(),
    };
    super::maintenance::validate(&plan)?;
    super::privacy::validate(root, &plan)?;
    files::validate_plan_before(root, &plan)?;
    Ok(Some(plan))
}
#[cfg(all(test, not(windows)))]
mod tests;

/// Only deliberate ownership/codec refusals suppress repeat preparation in the
/// same epoch. Missing files, parse failures, I/O and contention remain retryable.
pub(super) fn manual(error: &Error) -> bool {
    matches!(
        error,
        Error::Invalid(
            "unknown installed hook profile is preserved"
                | "unknown recorded hook encoding is preserved"
                | "unknown Shield callback is preserved; review explicit hook repair"
                | "automatic hook migration unsupported"
                | "automatic MCP migration unsupported"
                | "unknown MCP launcher is preserved"
                | "owned Shield MCP entry changed"
        )
    )
}

pub(super) fn needed(state: &State) -> bool {
    state.operator["portable_registration_epoch"] != state.project_id
        && registration(&state.operator)
            .ok()
            .flatten()
            .is_some_and(|owned| {
                (owned.definition.is_none() && !owned.agents.is_empty())
                    || owned.definition.as_ref().is_some_and(|definition| {
                        // A pin already naming this executable needs no re-pin.
                        // Avoid rendering and decoding every host/adapter field
                        // on each ordinary native project open.
                        if std::env::current_exe().is_ok_and(|binary| definition.pinned_to(&binary))
                        {
                            return false;
                        }
                        let defaults = crate::review::mcp::default_approved();
                        definition
                            .portable_codec(owned.approved_tools.as_deref().unwrap_or(&defaults))
                            && std::env::current_exe()
                                .is_ok_and(|binary| !definition.pinned_to(&binary))
                    })
            })
}

/// Shield owns only its two definition files. Host projections and their ledger
/// remain with Hub or Box; equivalent pin maintenance does not republish them.
pub(in crate::review) fn prepare_pin(
    root: &Path,
    state: &State,
    executable: &Path,
) -> Result<Option<Plan>, Error> {
    let Some(previous) = registration(&state.operator)? else {
        return Ok(None);
    };
    let Some(owned) = &previous.definition else {
        return Ok(None);
    };
    let host = owned.host();
    let defaults = crate::review::mcp::default_approved();
    let approved = previous.approved_tools.as_deref().unwrap_or(&defaults);
    // Unknown/edited definitions are preserved for their explicit review owner.
    let status = host_definition::diagnose(root, owned, &mut Vec::new());
    if status == "missing" || !host_definition::configured(host, root) {
        return Err(Error::Invalid(
            "owned Hub or Box definition temporarily unavailable",
        ));
    }
    if !owned.portable_codec(approved) || status != "installed" {
        return Ok(None);
    }
    let mut changes = Vec::new();
    let replacement = host_definition::prepare(
        host,
        root,
        Some(owned),
        Some(executable),
        false,
        approved,
        &mut changes,
    )?;
    changes.retain(|change| change.before != change.after);
    if changes.is_empty() {
        return Ok(None);
    }
    let mut desired = previous.clone();
    desired.definition = replacement;
    if desired.agents.is_empty() {
        desired.binary = executable.to_path_buf();
    }
    let before = super::read_file(root)?;
    let after = before.clone().unwrap_or_default();
    let plan = Plan {
        targets: Some(vec![host.id.into()]),
        scope_reason: Some(REASON.into()),
        policy_revision: Some(state.policy_revision.clone()),
        before,
        after,
        previous: Some(previous),
        desired: Some(desired),
        files: changes,
        handed_over: Vec::new(),
    };
    super::maintenance::validate(&plan)?;
    super::privacy::validate(root, &plan)?;
    files::validate_plan_before(root, &plan)?;
    Ok(Some(plan))
}

pub(super) fn prepare(root: &Path, state: &State, agent: &str) -> Result<Option<Plan>, Error> {
    let Some(previous) = registration(&state.operator)? else {
        return Ok(None);
    };
    if previous.definition.is_some() {
        return Ok(None);
    }
    let Some(installed) = previous.agents.get(agent) else {
        return Ok(None);
    };
    let adapter = super::agents::find(agent).ok_or(Error::Invalid("unknown native adapter"))?;
    let mut roots = vec![root.to_path_buf()];
    for value in state.operator["portable_launch_roots"]
        .as_array()
        .into_iter()
        .flatten()
        .take(2)
    {
        let path = PathBuf::from(
            value
                .as_str()
                .ok_or(Error::Invalid("invalid retained launch root"))?,
        );
        if !path.is_absolute() {
            return Err(Error::Invalid("invalid retained launch root"));
        }
        if !roots.contains(&path) {
            roots.push(path);
        }
    }
    let roots: Vec<_> = roots.iter().map(PathBuf::as_path).collect();
    let mut changes = Vec::new();
    let replacement = native::migrate_owned(
        root,
        agent,
        installed,
        &previous.binary,
        &roots,
        &mut changes,
    )?;
    let mut desired = previous.clone();
    desired.agents.insert(agent.into(), replacement);
    let (before, after) = if adapter.spec().mcp_path.is_none() {
        let source =
            super::read_file(root)?.ok_or(Error::Invalid("owned Shield MCP file missing"))?;
        let config = crate::json::parse(source.as_bytes())
            .map_err(|_| Error::Invalid("invalid MCP JSON"))?;
        if config["mcpServers"]["shield"].is_null() {
            return Err(Error::Invalid("owned Shield MCP entry missing"));
        }
        if config["mcpServers"]["shield"] != previous.entry {
            return Err(Error::Invalid("owned Shield MCP entry changed"));
        }
        let entry = adapter
            .portable_mcp_entry(&previous.entry.to_string(), &previous.binary, &roots)?
            .ok_or(Error::Invalid("automatic MCP migration unsupported"))?;
        desired.entry = crate::json::parse(entry.as_bytes())
            .map_err(|_| Error::Invalid("invalid migrated MCP entry"))?;
        let after = super::mcp_after(Some(&source), Some(&previous), Some(&desired), false)?;
        (Some(source), after)
    } else {
        (None, String::new())
    };
    changes.retain(|change| change.before != change.after);
    if desired == previous
        && changes.is_empty()
        && before.as_deref().is_none_or(|before| before == after)
    {
        return Ok(None);
    }
    let plan = Plan {
        targets: Some(vec![agent.into()]),
        scope_reason: Some(REASON.into()),
        policy_revision: Some(state.policy_revision.clone()),
        before,
        after,
        previous: Some(previous),
        desired: Some(desired),
        files: changes,
        handed_over: Vec::new(),
    };
    super::maintenance::validate(&plan)?;
    super::privacy::validate(root, &plan)?;
    files::validate_plan_before(root, &plan)?;
    Ok(Some(plan))
}
