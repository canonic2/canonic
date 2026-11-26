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
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub(in crate::review) mcp_borrowed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    settings: Option<serde_json::Value>,
}

#[cfg(not(windows))]
pub(in crate::review) fn quoted(path: &Path) -> Result<String, Error> {
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

#[cfg(any(windows, test))]
fn windows_command(
    shell: &Path,
    binary: &Path,
    root: &Path,
    config: Option<&str>,
    agent: &str,
    event: &str,
) -> Result<String, Error> {
    use base64::Engine as _;
    let shell = shell
        .to_str()
        .ok_or(Error::Invalid("hook shell is not UTF-8"))?;
    // cmd, PowerShell and Git Bash can invoke this unquoted absolute system path.
    // Refuse an unusual shell location rather than fall back to PATH lookup.
    if shell.as_bytes().get(1) != Some(&b':')
        || !shell
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphabetic)
        || !matches!(shell.as_bytes().get(2), Some(b'\\' | b'/'))
        || !shell
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b":/\\._-".contains(&byte))
    {
        return Err(Error::Invalid("unsupported Windows hook shell path"));
    }
    let literal = |text: &str| -> Result<String, Error> {
        if text.contains(['\0', '\n', '\r']) {
            return Err(Error::Invalid("invalid integration path"));
        }
        Ok(format!("'{}'", text.replace('\'', "''")))
    };
    let config = config
        .map(|config| literal(config).map(|config| format!(" --config {config}")))
        .transpose()?
        .unwrap_or_default();
    let script = format!(
        "& {} __internal native-hook --project {}{config} --agent {} --event {}; if ($LASTEXITCODE -ne 0) {{ exit 2 }}",
        literal(
            binary
                .to_str()
                .ok_or(Error::Invalid("integration path is not UTF-8"))?
        )?,
        literal(
            root.to_str()
                .ok_or(Error::Invalid("integration path is not UTF-8"))?
        )?,
        literal(agent)?,
        literal(event)?,
    );
    let bytes: Vec<_> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
    // Forward separators also survive Bash's unquoted word parsing. cmd's
    // command-string limit is tighter than CreateProcess's native argv limit.
    let shell = shell.replace('\\', "/");
    let command = format!("{shell} -NoLogo -NoProfile -NonInteractive -EncodedCommand {encoded}");
    if command.len() > 8_000 {
        return Err(Error::Invalid(
            "Windows hook command exceeds the shell limit",
        ));
    }
    Ok(command)
}

/// The POSIX hook command line from already quoted binary and project words.
/// Box definitions pass its placeholders for the project, agent and event.
#[cfg(not(windows))]
pub(super) fn posix_hook_command(binary: &str, project: &str, agent: &str, event: &str) -> String {
    format!(
        "{binary} __internal native-hook --project {project} --agent {agent} --event {event} || exit 2"
    )
}

pub(in crate::review) fn hook_command(
    root: &Path,
    agent: &str,
    event: &str,
) -> Result<String, Error> {
    // An explicit configuration selection travels with the hook, relative to
    // the project, so the hook enforces the same policy files.
    let config = crate::layout::launch_args(root)?.pop();
    #[cfg(windows)]
    {
        let shell = std::path::PathBuf::from(
            winsafe::GetSystemDirectory()
                .map_err(|_| Error::Invalid("Windows system directory unavailable"))?,
        )
        .join("WindowsPowerShell/v1.0/powershell.exe");
        if !shell.is_file() {
            return Err(Error::Invalid("Windows PowerShell unavailable"));
        }
        windows_command(
            &shell,
            Path::new("shield"),
            Path::new("."),
            config.as_deref(),
            agent,
            event,
        )
    }
    #[cfg(not(windows))]
    {
        let mut project = quoted(Path::new("."))?;
        if let Some(config) = config {
            project = format!("{project} --config {}", quoted(Path::new(&config))?);
        }
        Ok(posix_hook_command(
            &quoted(Path::new("shield"))?,
            &project,
            agent,
            event,
        ))
    }
}

fn installation(
    adapter: &dyn AgentAdapter,
    binary: &Path,
    root: &Path,
    approved: &[String],
) -> Result<Installation, Error> {
    Ok(Installation {
        groups: adapter
            .spec()
            .events
            .iter()
            .map(|(event, _)| {
                if let Some(group) = adapter.direct_hook_group(binary, root, event) {
                    return Ok(((*event).into(), group));
                }
                let command = hook_command(root, adapter.spec().id, event)?;
                Ok(((*event).into(), adapter.hook_group(&command)))
            })
            .collect::<Result<_, Error>>()?,
        mcp: adapter
            .spec()
            .mcp_path
            .map(|_| adapter.mcp_entry(binary, root, approved)),
        settings: None,
        mcp_borrowed: false,
    })
}

/// Same-binary maintenance preserves the installed event profile and settings.
/// Box publications are excluded by the caller; their publisher owns those files.
pub(super) fn migrate_owned(
    root: &Path,
    agent: &str,
    previous: &Installation,
    binary: &Path,
    roots: &[&Path],
    changes: &mut Vec<Change>,
) -> Result<Installation, Error> {
    let adapter = lookup(agent)?;
    let current_events: Vec<_> = adapter
        .spec()
        .events
        .iter()
        .map(|(event, _)| *event)
        .collect();
    let matches = |events: &[&str]| {
        events.len() == previous.groups.len()
            && events
                .iter()
                .all(|event| previous.groups.contains_key(*event))
    };
    if !matches(&current_events)
        && !adapter
            .historical_hook_profiles()
            .iter()
            .any(|events| matches(events))
    {
        return Err(Error::Invalid(
            "unknown installed hook profile is preserved",
        ));
    }
    let mut desired = installation(adapter, binary, root, &[])?;
    desired
        .groups
        .retain(|event, _| previous.groups.contains_key(event));
    desired.settings.clone_from(&previous.settings);
    desired.mcp_borrowed = previous.mcp_borrowed;
    let permitted = known_groups(adapter, binary, roots, &desired.groups)?;
    if previous.groups.iter().any(|(event, group)| {
        !permitted
            .get(event)
            .is_some_and(|groups| groups.contains(group))
    }) {
        return Err(Error::Invalid(
            "unknown recorded hook encoding is preserved",
        ));
    }
    files::edit_optional(root, changes, adapter.spec().hooks_path, |source| {
        let source = source.ok_or(Error::Invalid("owned hook file missing"))?;
        adapter
            .migrate_hooks(source, &permitted, &desired.groups)?
            .map(Some)
            .ok_or(Error::Invalid("automatic hook migration unsupported"))
    })?;
    desired.mcp = previous
        .mcp
        .as_ref()
        .map(|entry| {
            adapter
                .portable_mcp_entry(entry, binary, roots)?
                .ok_or(Error::Invalid("automatic MCP migration unsupported"))
        })
        .transpose()?;
    if let Some(path) = adapter.spec().mcp_path {
        files::edit_optional(root, changes, path, |source| {
            if !source.is_some_and(|source| adapter.has_mcp_entry(source).unwrap_or(false)) {
                return Err(Error::Invalid("owned MCP entry missing"));
            }
            Ok(Some(adapter.merge_mcp(
                source,
                previous.mcp.as_deref(),
                desired.mcp.as_deref(),
            )?))
        })?;
    }
    Ok(desired)
}

fn known_groups(
    adapter: &dyn AgentAdapter,
    binary: &Path,
    roots: &[&Path],
    portable: &HookGroups,
) -> Result<agents::setup::PortableHookGroups, Error> {
    let mut result: agents::setup::PortableHookGroups = portable
        .iter()
        .map(|(event, group)| (event.clone(), vec![group.clone()]))
        .collect();
    #[cfg(not(windows))]
    for root in roots.iter().take(3) {
        let binary = quoted(binary)?;
        let root = quoted(root)?;
        for event in portable.keys() {
            let commands = [
                posix_hook_command(&binary, &root, adapter.spec().id, event),
                format!(
                    "{binary} __internal native-hook --project {root} --agent {} || exit 2",
                    adapter.spec().id
                ),
            ];
            for command in commands {
                let group = adapter.hook_group(&command);
                let entries = result.get_mut(event).expect("portable event");
                if !entries.contains(&group) {
                    entries.push(group);
                }
            }
        }
    }
    #[cfg(windows)]
    let _ = (adapter, binary, roots);
    Ok(result)
}

/// Inspect Box's published callbacks without claiming ownership of its files.
pub(super) fn published_hooks(
    root: &Path,
    agent: &str,
    binary: &Path,
    previous_roots: &[&Path],
) -> Result<Option<bool>, Error> {
    let adapter = lookup(agent)?;
    let expected = installation(adapter, binary, root, &[])?;
    let source = files::read(root, adapter.spec().hooks_path)?;
    let groups = published_groups(
        adapter,
        binary,
        root,
        previous_roots,
        source.as_deref(),
        expected.groups,
    )?;
    let captured = adapter.capture_hooks(source.as_deref(), &groups)?;
    if captured.is_empty() {
        return Ok(None);
    }
    let extra = adapter
        .spec()
        .mcp_path
        .map(|path| files::read(root, path))
        .transpose()?
        .flatten();
    let mut diagnostics = Vec::new();
    adapter.diagnose_published_hooks(
        source.as_deref(),
        &groups,
        extra.as_deref(),
        &mut diagnostics,
    )?;
    Ok(Some(captured == groups && diagnostics.is_empty()))
}

/// Older Box publications pinned the same executable and a checkout root.
/// Recognize exact historical callbacks without adopting duplicate or edited
/// groups, and without asking Box to rewrite working host files.
fn published_groups(
    adapter: &dyn AgentAdapter,
    binary: &Path,
    root: &Path,
    previous_roots: &[&Path],
    source: Option<&str>,
    portable: HookGroups,
) -> Result<HookGroups, Error> {
    match adapter.capture_hooks(source, &portable) {
        Ok(_) => Ok(portable),
        Err(error) => {
            #[cfg(not(windows))]
            {
                let generated = |binary: &str, project: &str| -> HookGroups {
                    adapter
                        .spec()
                        .events
                        .iter()
                        .map(|(event, _)| {
                            let command =
                                posix_hook_command(binary, project, adapter.spec().id, event);
                            ((*event).into(), adapter.hook_group(&command))
                        })
                        .collect()
                };
                // Box expands the definition's `{command} ... --project .` with
                // the quoted pinned binary, or the bare command without a pin.
                let pinned = quoted(binary)?;
                let historical_roots = std::iter::once(root)
                    .chain(previous_roots.iter().copied().take(2))
                    .map(quoted)
                    .collect::<Result<Vec<_>, _>>()?;
                let candidates = [
                    (pinned.clone(), ".".to_owned()),
                    ("shield".into(), ".".into()),
                ]
                .into_iter()
                .chain(
                    historical_roots
                        .into_iter()
                        .map(|project| (pinned.clone(), project)),
                );
                for (binary, project) in candidates {
                    let groups = generated(&binary, &project);
                    if adapter.capture_hooks(source, &groups).is_ok() {
                        return Ok(groups);
                    }
                }
            }
            #[cfg(windows)]
            let _ = (binary, root, previous_roots);
            Err(error)
        }
    }
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
        mcp_borrowed: false,
    };
    if let Some(path) = adapter.spec().mcp_path {
        let before = files::read(root, path)?;
        if let Some(bytes) = before.as_deref()
            && adapter.has_mcp_entry(bytes)?
        {
            let borrowed = adapter.borrow_mcp_entry(bytes, binary, root)?;
            captured.mcp_borrowed = borrowed.is_some();
            captured.mcp = borrowed.or_else(|| expected.mcp.clone());
        }
        adapter.merge_mcp(
            before.as_deref(),
            captured.mcp.as_deref(),
            if captured.mcp_borrowed {
                captured.mcp.as_deref()
            } else {
                expected.mcp.as_deref()
            },
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
    repair_hooks: bool,
) -> Result<Option<Installation>, Error> {
    prepare_adapter(
        root,
        lookup(agent)?,
        previous,
        binary,
        approved,
        changes,
        repair_hooks,
    )
}

fn prepare_adapter(
    root: &Path,
    adapter: &dyn AgentAdapter,
    previous: Option<&Installation>,
    binary: Option<&Path>,
    approved: &[String],
    changes: &mut Vec<Change>,
    repair_hooks: bool,
) -> Result<Option<Installation>, Error> {
    if previous.is_none() && binary.is_none() {
        return Ok(None);
    }
    if binary.is_some() {
        adapter.check_native_setup(root, &|name| {
            changes
                .iter()
                .find(|change| change.path == name)
                .map(|change| change.after.clone())
        })?;
    }
    let mut desired = binary
        .map(|b| installation(adapter, b, root, approved))
        .transpose()?;
    let empty = HookGroups::new();
    files::edit_optional(root, changes, adapter.spec().hooks_path, |source| {
        let groups = desired.as_ref().map_or(&empty, |p| &p.groups);
        let hooks = if repair_hooks {
            adapter.repair_hooks(source, groups)?
        } else {
            adapter.merge_hooks(source, previous.map_or(&empty, |p| &p.groups), groups)?
        };
        let settings = adapter.merge_settings(
            Some(&hooks),
            previous.and_then(|p| p.settings.as_ref()),
            desired.is_some().then_some(approved),
        )?;
        if let Some(desired) = &mut desired {
            desired.settings = settings.ownership;
        }
        Ok((desired.is_some() || !settings.source.trim().is_empty()).then_some(settings.source))
    })?;
    // A borrowed entry belongs to the external publisher. Removing Shield's
    // hooks has no dependency on the publisher retaining its entry or file.
    if desired.is_none() && previous.is_some_and(|p| p.mcp_borrowed) {
        return Ok(None);
    }
    if let Some(path) = adapter.spec().mcp_path {
        files::edit_optional(root, changes, path, |source| {
            let borrowed = if let Some(previous) = previous.filter(|p| p.mcp_borrowed) {
                previous.mcp.clone()
            } else if previous.is_none() {
                match source.zip(binary) {
                    Some((source, binary)) => adapter.borrow_mcp_entry(source, binary, root)?,
                    None => None,
                }
            } else {
                None
            };
            if let Some(entry) = borrowed {
                // Validate the reserved entry while retaining original file bytes.
                // The no-op Change binds this dependency to confirmation/recovery.
                validate_borrowed_mcp(adapter, source, &entry)?;
                if let Some(desired) = &mut desired {
                    desired.mcp = Some(entry);
                    desired.mcp_borrowed = true;
                }
                return Ok(source.map(str::to_owned));
            }
            let new = desired.as_ref().and_then(|p| p.mcp.as_deref());
            let merged = if repair_hooks {
                adapter.repair_mcp(source, new)?
            } else {
                adapter.merge_mcp(source, previous.and_then(|p| p.mcp.as_deref()), new)?
            };
            let empty = crate::json::parse(merged.as_bytes())
                .is_ok_and(|value| value.as_object().is_some_and(serde_json::Map::is_empty));
            Ok((desired.is_some() || !empty).then_some(merged))
        })?;
    }
    Ok(desired)
}

fn validate_borrowed_mcp(
    adapter: &dyn AgentAdapter,
    source: Option<&str>,
    entry: &str,
) -> Result<(), Error> {
    if !source.is_some_and(|source| adapter.has_mcp_entry(source).unwrap_or(false)) {
        return Err(Error::Invalid("borrowed Shield MCP entry is missing"));
    }
    adapter
        .merge_mcp(source, Some(entry), Some(entry))
        .map(|_| ())
}

pub(in crate::review) fn diagnose(
    root: &Path,
    agent: &str,
    binary: &Path,
    installation: &Installation,
    diagnostics: &mut Vec<&'static str>,
) -> Result<bool, Error> {
    let adapter = lookup(agent)?;
    let mut changes = Vec::new();
    let mut ownership_conflict = false;
    if let Err(error) = prepare_adapter(
        root,
        adapter,
        Some(installation),
        None,
        &[],
        &mut changes,
        false,
    ) {
        match error {
            Error::HookOwnership(reason) => {
                diagnostics.push(reason);
                ownership_conflict = true;
            }
            Error::Invalid(reason) => diagnostics.push(reason),
            other => return Err(other),
        }
    }
    if changes.iter().any(|f| f.before.is_none()) {
        diagnostics.push("owned native hook configuration is missing");
    }
    if !compatible_historical_hooks(root, adapter, binary, installation)
        && adapter
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
    if installation.mcp_borrowed
        && let Some(entry) = installation.mcp.as_deref()
        && let Err(error) = validate_borrowed_mcp(adapter, extra.as_deref(), entry)
    {
        match error {
            Error::Invalid(reason) => diagnostics.push(reason),
            other => return Err(other),
        }
    }
    adapter.diagnose(
        hooks.as_deref(),
        &installation.groups,
        extra.as_deref(),
        diagnostics,
    )?;
    Ok(ownership_conflict)
}

/// Missing later advisory events do not invalidate an exact installed profile
/// whose entry points remain supported by the CLI compatibility boundary.
fn compatible_historical_hooks(
    root: &Path,
    adapter: &dyn AgentAdapter,
    binary: &Path,
    installed: &Installation,
) -> bool {
    let profile = adapter.historical_hook_profiles().iter().find(|profile| {
        installed.groups.len() == profile.len()
            && profile
                .iter()
                .all(|event| installed.groups.contains_key(*event))
    });
    let Some(profile) = profile else { return false };
    if profile.iter().all(|event| {
        hook_command(root, adapter.spec().id, event).is_ok_and(|command| {
            installed.groups.get(*event) == Some(&adapter.hook_group(&command))
        })
    }) {
        return true;
    }
    #[cfg(not(windows))]
    if let (Ok(binary), Ok(project)) = (quoted(binary), quoted(root)) {
        let command = format!(
            "{binary} __internal native-hook --project {project} --agent {} || exit 2",
            adapter.spec().id
        );
        let group = adapter.hook_group(&command);
        return profile
            .iter()
            .all(|event| installed.groups.get(*event) == Some(&group));
    }
    false
}

#[cfg(test)]
mod tests;
