//! Adapter configuration preparation; contained publication stays shared.
use super::{
    Error,
    files::{self, Change},
};
use crate::agents::{self, AgentAdapter, setup::HookGroups};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::review) struct Installation {
    pub(in crate::review) groups: HookGroups,
    mcp: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    settings: Option<serde_json::Value>,
}

fn quoted(path: &Path) -> Result<String, Error> {
    let text = path
        .to_str()
        .ok_or(Error::Invalid("integration path is not UTF-8"))?;
    if text.contains(['\0', '\n', '\r']) {
        return Err(Error::Invalid("invalid integration path"));
    }
    Ok(format!("'{}'", text.replace('\'', "'\\''")))
}

fn lookup(agent: &str) -> Result<&'static dyn AgentAdapter, Error> {
    agents::find(agent).ok_or(Error::Invalid("unknown agent adapter"))
}
fn installation(
    adapter: &dyn AgentAdapter,
    binary: &Path,
    root: &Path,
    approved: &[String],
) -> Result<Installation, Error> {
    let launch = format!(
        "{} __internal native-hook --project {} --agent {}",
        quoted(binary)?,
        quoted(root)?,
        adapter.spec().id
    );
    Ok(Installation {
        groups: adapter
            .spec()
            .events
            .iter()
            .map(|(event, _)| {
                let command = format!("{launch} --event {event} || exit 2");
                ((*event).into(), adapter.hook_group(&command))
            })
            .collect(),
        mcp: adapter
            .spec()
            .mcp_path
            .map(|_| adapter.mcp_entry(binary, root, approved)),
        settings: None,
    })
}

/// Capture only exact generated entries for initial operator review.
pub(in crate::review) fn capture_matching(
    root: &Path,
    agent: &str,
    binary: &Path,
    shared_mcp_present: bool,
) -> Result<Option<Installation>, Error> {
    let adapter = lookup(agent)?;
    let expected = installation(
        adapter,
        binary,
        root,
        &crate::review::mcp::default_approved(),
    )?;
    let before = files::read(root, adapter.spec().hooks_path)?;
    let mut captured = Installation {
        groups: adapter.capture_hooks(before.as_deref(), &expected.groups)?,
        mcp: None,
        settings: None,
    };
    if let Some(path) = adapter.spec().mcp_path {
        let before = files::read(root, path)?;
        if let Some(bytes) = before.as_deref()
            && adapter.has_mcp_entry(bytes)?
        {
            captured.mcp.clone_from(&expected.mcp);
        }
        adapter.merge_mcp(
            before.as_deref(),
            captured.mcp.as_deref(),
            expected.mcp.as_deref(),
        )?;
    }
    Ok(
        (!captured.groups.is_empty() || captured.mcp.is_some() || shared_mcp_present)
            .then_some(captured),
    )
}

pub(in crate::review) fn prepare(
    root: &Path,
    agent: &str,
    previous: Option<&Installation>,
    binary: Option<&Path>,
    approved: &[String],
    changes: &mut Vec<Change>,
) -> Result<Option<Installation>, Error> {
    prepare_adapter(root, lookup(agent)?, previous, binary, approved, changes)
}

fn prepare_adapter(
    root: &Path,
    adapter: &dyn AgentAdapter,
    previous: Option<&Installation>,
    binary: Option<&Path>,
    approved: &[String],
    changes: &mut Vec<Change>,
) -> Result<Option<Installation>, Error> {
    if previous.is_none() && binary.is_none() {
        return Ok(None);
    }
    if cfg!(windows) {
        return Err(Error::Invalid(
            "native command hook installation is not supported on Windows",
        ));
    }
    let mut desired = binary
        .map(|b| installation(adapter, b, root, approved))
        .transpose()?;
    let empty = HookGroups::new();
    files::edit(root, changes, adapter.spec().hooks_path, |source| {
        let hooks = adapter.merge_hooks(
            source,
            previous.map_or(&empty, |p| &p.groups),
            desired.as_ref().map_or(&empty, |p| &p.groups),
        )?;
        let settings = adapter.merge_settings(
            Some(&hooks),
            previous.and_then(|p| p.settings.as_ref()),
            desired.is_some().then_some(approved),
        )?;
        if let Some(desired) = &mut desired {
            desired.settings = settings.ownership;
        }
        Ok(settings.source)
    })?;
    if let Some(path) = adapter.spec().mcp_path {
        files::edit(root, changes, path, |source| {
            adapter.merge_mcp(
                source,
                previous.and_then(|p| p.mcp.as_deref()),
                desired.as_ref().and_then(|p| p.mcp.as_deref()),
            )
        })?;
    }
    Ok(desired)
}

pub(in crate::review) fn diagnose(
    root: &Path,
    agent: &str,
    installation: &Installation,
    diagnostics: &mut Vec<&'static str>,
) -> Result<(), Error> {
    let adapter = lookup(agent)?;
    let mut changes = Vec::new();
    prepare_adapter(root, adapter, Some(installation), None, &[], &mut changes)?;
    if changes.iter().any(|f| f.before.is_none()) {
        diagnostics.push("owned native hook configuration is missing");
    }
    if adapter
        .spec()
        .events
        .iter()
        .any(|(event, _)| !installation.groups.contains_key(*event))
    {
        diagnostics.push("native hook registration needs an adapter upgrade");
    }
    let hooks = files::read(root, adapter.spec().hooks_path)?;
    let extra = adapter
        .spec()
        .mcp_path
        .map(|path| files::read(root, path))
        .transpose()?
        .flatten();
    adapter.diagnose(
        hooks.as_deref(),
        &installation.groups,
        extra.as_deref(),
        diagnostics,
    )
}

#[cfg(test)]
mod tests;
