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
    let (configured, policy_revision) = if root.join(".shield/review/state.json").exists() {
        let snapshot =
            super::review::runtime::consumer::load(&super::review::open(root)?, &|| Ok(()))?;
        (
            snapshot.mcp_connections().clone(),
            Some(snapshot.identity().policy_revision().into()),
        )
    } else {
        // Init plans precede activation. Capture only bounded project sources;
        // ordinary candidate validation still runs before the plan is applied.
        let path = root.join(".shield/config.yml");
        if path.exists() {
            let captured = super::super::project_sources::capture(&path)
                .map_err(|_| Error::Invalid("invalid project MCP configuration"))?;
            let evaluator = crate::configuration::Evaluator::new(vec![(
                crate::policy::Scope::Project,
                captured.snapshot()?,
            )])?;
            (evaluator.effective().mcp_connections().clone(), None)
        } else {
            (BTreeMap::new(), None)
        }
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
        if retained {
            if configured.values().any(|connection| {
                connection.enabled && connection.local && connection.agents.contains_key(agent)
            }) {
                super::review::integrations::private_mcp_destination(root, agent)?;
                if root.join(".shield/review/state.json").exists() {
                    crate::mcp_connections::launcher::private_state(root)?;
                }
            }
            for (id, connection) in configured.iter().filter(|(_, c)| c.enabled) {
                if let Some(alias) = connection.agents.get(agent) {
                    adapter.check_mcp_scopes(root, root, alias)?;
                    let previous_binding = old["review"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .any(|row| row["id"] == *id && row["alias"] == *alias)
                        .then(|| {
                            serde_json::from_value::<Binding>(old["launches"][id].clone()).ok()
                        })
                        .flatten();
                    let binding = if connection.local {
                        match previous_binding {
                            Some(binding) => Some(binding.refreshed(connection)?),
                            None => Some(Binding::new(connection)?),
                        }
                    } else {
                        None
                    };
                    let binary = &desired.as_ref().expect("retained registration").binary;
                    entries.insert(
                        alias.clone(),
                        match &binding {
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
        if entries.is_empty() && old.is_null() {
            continue;
        }
        let aliases = entries.keys().cloned().collect::<Vec<_>>();
        let review=configured.iter().filter(|_|retained).filter_map(|(id,c)|c.agents.get(agent).map(|alias|json!({"id":id,"alias":alias,"enabled":c.enabled,"source":if c.local {"local"}else{"project"},"settings":c.transport,"packs":c.packs,"projection":if c.local {"private Shield launcher"}else{"native project entry"}}))).collect::<Vec<_>>();
        let ownership;
        if let Some(path) = adapter.spec().mcp_path {
            let mut captured = None;
            files::edit(root, changes, path, |source| {
                let update =
                    adapter.merge_connections(source.unwrap_or(""), &old["entries"], &entries)?;
                captured = update.ownership;
                Ok(update.source)
            })?;
            ownership = captured.unwrap_or(Value::Null);
        } else {
            let update = adapter.merge_connections(
                if shared.is_empty() { "{}" } else { &shared },
                &old["entries"],
                &entries,
            )?;
            shared = update.source;
            ownership = update.ownership.unwrap_or(Value::Null);
        }
        let mut settings = None;
        files::edit(root, changes, adapter.spec().hooks_path, |source| {
            let update =
                adapter.connection_settings(source.unwrap_or("{}"), &old["settings"], &aliases)?;
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
