//! Prepare declarative connections in the same reviewed, resumable native plan.
use super::{Error, Path, Registration, Value, agents, files, json};
use crate::mcp_connections::launcher::Binding;
use std::collections::BTreeMap;

pub(super) fn prepare_connections(
    root: &Path,
    shared: &str,
    previous: Option<&Registration>,
    desired: Option<&mut Registration>,
    changes: &mut Vec<files::Change>,
    targets: &[String],
) -> Result<(String, Option<String>), Error> {
    // The connections are the policy files' connections. An enrolled
    // project's plan records their revision so applying it can refuse files
    // changed since review; init writes the files itself after planning.
    let (configured, policy_revision) = if root.join(".shield/config.yml").exists() {
        let policy = crate::review::files::load(root)?;
        let enrolled = root.join(".shield/review/state.json").exists();
        (
            policy.evaluator.effective().mcp_connections().clone(),
            enrolled.then(|| policy.evaluator.revision().to_owned()),
        )
    } else {
        (BTreeMap::new(), None)
    };
    let shared = prepare_configured(
        root,
        shared,
        previous,
        desired,
        changes,
        targets,
        &configured,
    )?;
    Ok((shared, policy_revision))
}

pub(in crate::review::integrations) fn prepare_configured(
    root: &Path,
    shared: &str,
    previous: Option<&Registration>,
    mut desired: Option<&mut Registration>,
    changes: &mut Vec<files::Change>,
    targets: &[String],
    configured: &crate::mcp_connections::Connections,
) -> Result<String, Error> {
    let mut shared = shared.to_owned();
    for adapter in agents::ADAPTERS {
        let agent = adapter.spec().id;
        if !selected(targets, *adapter) {
            continue;
        }
        let old = previous
            .and_then(|r| r.connections.get(agent))
            .unwrap_or(&Value::Null);
        let retained = desired.as_ref().is_some_and(|r| r.covers(agent));
        let mut entries = BTreeMap::new();
        let mut launches = BTreeMap::new();
        let mut native = std::collections::BTreeSet::new();
        let mut old_entries = old["entries"].clone();
        if retained {
            for (id, connection) in configured.iter().filter(|(_, c)| c.enabled) {
                if let Some(alias) = connection.agents.get(agent) {
                    adapter.check_mcp_scopes(root, root, alias)?;
                    if connection.local
                        && super::super::connections::native_matches(root, agent, connection)
                    {
                        native.insert(alias.clone());
                        // Matching native entries belong to their existing owner.
                        // Relinquish any prior projection without deleting it.
                        if let Some(entries) = old_entries.as_object_mut() {
                            entries.remove(alias);
                        }
                        continue;
                    }
                    if connection.local && root.join(".shield/review/state.json").exists() {
                        crate::mcp_connections::launcher::private_state(root)?;
                    }
                    let binding = launch_binding(old, id, alias, connection)?;
                    let binary = &desired.as_ref().expect("retained registration").binary;
                    entries.insert(
                        alias.clone(),
                        match &binding {
                            Some(binding) if adapter.portable_mcp_launcher() => {
                                binding.portable_projected(connection, binary, root)?
                            }
                            Some(binding) => binding.projected(connection, binary, root)?,
                            None => connection.projected(binary, root, None)?,
                        },
                    );
                    if let Some(binding) = binding {
                        launches.insert(id.clone(), binding);
                    }
                }
            }
        }
        if entries.is_empty() && old.is_null() && native.is_empty() {
            continue;
        }
        let aliases = entries
            .keys()
            .chain(native.iter())
            .cloned()
            .collect::<Vec<_>>();
        let review=configured.iter().filter(|_|retained).filter_map(|(id,c)|c.agents.get(agent).map(|alias|json!({"id":id,"alias":alias,"enabled":c.enabled,"source":if c.local {"local"}else{"project"},"settings":c.transport,"packs":c.packs,"projection":if native.contains(alias) {"existing native entry"} else if c.local {"private Shield launcher"}else{"native project entry"}}))).collect::<Vec<_>>();
        let ownership;
        if entries.is_empty()
            && old_entries
                .as_object()
                .is_none_or(serde_json::Map::is_empty)
        {
            ownership = Value::Null;
        } else if let Some(path) = adapter.spec().mcp_path {
            let mut captured = None;
            files::edit(root, changes, path, |source| {
                let update =
                    adapter.merge_connections(source.unwrap_or(""), &old_entries, &entries)?;
                captured = update.ownership;
                Ok(update.source)
            })?;
            ownership = captured.unwrap_or(Value::Null);
        } else {
            let update = adapter.merge_connections(
                if shared.is_empty() { "{}" } else { &shared },
                &old_entries,
                &entries,
            )?;
            shared = update.source;
            ownership = update.ownership.unwrap_or(Value::Null);
        }
        let mut settings = None;
        // Native entry ownership and Shield-added enablement are separate.
        let old_settings = old["settings"].clone();
        files::edit(root, changes, adapter.spec().hooks_path, |source| {
            let update =
                adapter.connection_settings(source.unwrap_or("{}"), &old_settings, &aliases)?;
            settings = update.ownership;
            Ok(update.source)
        })?;
        if let Some(registration) = desired.as_mut() {
            registration.connections.insert(
                agent.into(),
                json!({"entries":ownership,"settings":settings,"review":review,"launches":launches}),
            );
        }
    }
    Ok(shared)
}

fn selected(targets: &[String], adapter: &dyn crate::agents::AgentAdapter) -> bool {
    targets.iter().any(|target| {
        target == adapter.spec().id
            || target == super::box_definition::ID && adapter.supports_box_hooks()
    })
}

/// Stable locators belong to one connection, agent registration and alias.
fn launch_binding(
    old: &Value,
    id: &str,
    alias: &str,
    connection: &crate::mcp_connections::Connection,
) -> Result<Option<Binding>, Error> {
    if !connection.local {
        return Ok(None);
    }
    let previous = old["review"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|row| row["id"] == id && row["alias"] == alias)
        .then(|| serde_json::from_value::<Binding>(old["launches"][id].clone()).ok())
        .flatten();
    match previous {
        Some(binding) => binding.refreshed(connection).map(Some),
        None => Binding::new(connection).map(Some),
    }
}
