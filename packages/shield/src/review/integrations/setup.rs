//! Transport-neutral integration planning, exact review and publication.
use super::{
    AskFallback, IntegrationChoice, MAX_CONFIG, Plan, Registration, apply, binary_digest,
    box_definition, confirmed_fallback, files, mcp_after, native, pending, read_file, registration,
};
use crate::{
    agents,
    review::{self, Context, Error},
};
use serde_json::{Value, json};
use std::path::Path;
mod connections;
use connections::prepare_connections;

pub(in crate::review) fn desired(root: &Path) -> Result<Registration, Error> {
    let binary = std::env::current_exe()?.canonicalize()?;
    // Claude expands environment placeholders in command and argument strings.
    for path in [&binary, root] {
        if path
            .to_str()
            .is_none_or(|s| s.contains('$') || s.contains('\0'))
        {
            return Err(Error::Invalid(
                "integration paths must be UTF-8 without environment placeholders",
            ));
        }
    }
    Ok(Registration {
        binary_digest: binary_digest(&binary)?,
        entry: agents::ADAPTERS
            .iter()
            .find(|a| a.spec().mcp_path.is_none())
            .map(|a| crate::json::parse(a.mcp_entry(&binary, root, &[]).as_bytes()))
            .transpose()
            .map_err(|_| Error::Invalid("invalid adapter MCP entry"))?
            .unwrap_or(Value::Null),
        binary,
        version: env!("CARGO_PKG_VERSION").into(),
        agents: std::collections::BTreeMap::new(),
        ask_fallback: std::collections::BTreeMap::new(),
        approved_tools: None,
        connections: std::collections::BTreeMap::new(),
        box_definition: None,
    })
}

#[cfg(test)]
pub(in crate::review) fn prepare(
    root: &Path,
    meta: &Value,
    choice: IntegrationChoice,
) -> Result<Option<Plan>, Error> {
    prepare_with(root, meta, choice, false, None, true)
}

#[cfg(test)]
pub(in crate::review) fn prepare_initial(
    root: &Path,
    choice: IntegrationChoice,
) -> Result<Option<Plan>, Error> {
    prepare_approving(root, &json!({}), choice, true, None, true)
}

/// Prepare with the operator's tool approvals; `None` keeps the saved ones,
/// or the defaults for a first setup. `handover` drops the records of direct
/// installations a Box install replaces, leaving their files in place.
pub(in crate::review) fn prepare_approving(
    root: &Path,
    meta: &Value,
    choice: IntegrationChoice,
    initial: bool,
    approved: Option<Vec<String>>,
    handover: bool,
) -> Result<Option<Plan>, Error> {
    if initial && matches!(choice, IntegrationChoice::Keep) {
        return Ok(None);
    }
    prepare_with(root, meta, choice, initial, approved, handover)
}

/// The tools approved without asking: saved, or the defaults.
pub(in crate::review) fn approved_tools(meta: &Value) -> Result<Vec<String>, Error> {
    Ok(registration(meta)?
        .and_then(|r| r.approved_tools)
        .unwrap_or_else(crate::review::mcp::default_approved))
}

pub(in crate::review) fn prepare_with(
    root: &Path,
    meta: &Value,
    choice: IntegrationChoice,
    initial: bool,
    approved: Option<Vec<String>>,
    handover: bool,
) -> Result<Option<Plan>, Error> {
    let before = read_file(root)?;
    if let Some(plan) = pending(meta)? {
        files::validate_recovery(root, &plan.files)?;
        if before != plan.before && before.as_deref() != Some(&plan.after) {
            return Err(Error::Invalid(
                "MCP file changed after partial setup; restore the reviewed file before repair",
            ));
        }
        if !matches!(choice, IntegrationChoice::Keep) {
            return Err(Error::Invalid(
                "finish the confirmed setup first: run shield mcp setup without --agent or --remove",
            ));
        }
        return Ok(Some(plan));
    }
    let mut previous = registration(meta)?;
    if matches!(choice, IntegrationChoice::Keep) {
        return Ok(None);
    }
    if initial && previous.is_none() && !matches!(choice, IntegrationChoice::RemoveAgents(_)) {
        previous = capture_initial(root, choice, before.as_deref())?;
    }
    let mut desired = match choice {
        IntegrationChoice::Agents(_) => Some(desired(root)?),
        IntegrationChoice::RemoveAgents(_) => previous.clone().map(|mut registration| {
            registration.agents.clear();
            registration.box_definition = None;
            registration
        }),
        IntegrationChoice::Keep => None,
    };
    let approved = match approved {
        Some(approved) => approved,
        None => approved_tools(meta)?,
    };
    if let Some(desired) = &mut desired {
        desired.ask_fallback = previous
            .as_ref()
            .map(|r| r.ask_fallback.clone())
            .unwrap_or_default();
        desired.approved_tools = Some(approved.clone());
    }
    let handed_over = handed_over(previous.as_ref(), choice, handover);
    // Files are reconciled as if Shield never recorded the handed-over
    // installations, so their native entries stay exactly as they are.
    let planned = previous.clone().map(|mut registration| {
        for agent in &handed_over {
            registration.agents.remove(agent);
            registration.connections.remove(agent);
        }
        registration
    });
    let mut files = Vec::new();
    prepare_files(
        root,
        choice,
        planned.as_ref(),
        desired.as_mut(),
        &approved,
        &mut files,
    )?;
    if desired.as_ref().is_some_and(|registration| {
        registration.agents.is_empty() && registration.box_definition.is_none()
    }) {
        desired = None;
    }
    if desired.is_none() && previous.is_none() {
        return Ok(None);
    }
    let after = mcp_after(before.as_deref(), planned.as_ref(), desired.as_ref())?;
    let (after, policy_revision) =
        prepare_connections(root, &after, planned.as_ref(), desired.as_mut(), &mut files)?;
    if after.len() as u64 > MAX_CONFIG {
        return Err(Error::Invalid("MCP configuration exceeds limit"));
    }
    Ok(Some(Plan {
        policy_revision,
        before,
        after,
        previous,
        desired,
        files,
        handed_over,
    }))
}

/// The direct installations an install of Box hands over: those recorded and
/// not selected again, when the operator keeps the default hand-over.
pub(in crate::review) fn handed_over(
    previous: Option<&Registration>,
    choice: IntegrationChoice,
    handover: bool,
) -> Vec<String> {
    handed_over_with(previous, choice, handover, |agent| {
        agents::find(agent).is_some_and(agents::AgentAdapter::supports_box_hooks)
    })
}

fn handed_over_with(
    previous: Option<&Registration>,
    choice: IntegrationChoice,
    handover: bool,
    supports_box_hooks: impl Fn(&str) -> bool,
) -> Vec<String> {
    match previous {
        Some(registration)
            if handover
                && matches!(choice, IntegrationChoice::Agents(_))
                && choice.contains(box_definition::ID) =>
        {
            registration
                .agents
                .keys()
                .filter(|agent| !choice.contains(agent))
                .filter(|agent| supports_box_hooks(agent))
                .cloned()
                .collect()
        }
        _ => Vec::new(),
    }
}

/// Plan native and Box files, then keep only the Ask fallbacks of agents the
/// desired record still covers.
pub(in crate::review) fn prepare_files(
    root: &Path,
    choice: IntegrationChoice,
    previous: Option<&Registration>,
    mut desired: Option<&mut Registration>,
    approved: &[String],
    files: &mut Vec<files::Change>,
) -> Result<(), Error> {
    prepare_native(
        root,
        choice,
        previous,
        desired.as_deref_mut(),
        approved,
        files,
    )?;
    prepare_box(
        root,
        choice,
        previous,
        desired.as_deref_mut(),
        approved,
        files,
    )?;
    if let Some(registration) = desired {
        let covered: Vec<String> = registration
            .ask_fallback
            .keys()
            .filter(|agent| registration.covers(agent))
            .cloned()
            .collect();
        registration
            .ask_fallback
            .retain(|agent, _| covered.contains(agent));
    }
    Ok(())
}

/// Add each native adapter's hooks, MCP entry and settings to the plan:
/// installed or refreshed when selected or already owned, kept or removed on
/// removal.
pub(in crate::review) fn prepare_native(
    root: &Path,
    choice: IntegrationChoice,
    previous: Option<&Registration>,
    mut desired: Option<&mut Registration>,
    approved: &[String],
    files: &mut Vec<files::Change>,
) -> Result<(), Error> {
    let removing = matches!(choice, IntegrationChoice::RemoveAgents(_));
    for adapter in agents::ADAPTERS {
        let agent = adapter.spec().id;
        let old = previous.and_then(|r| r.agents.get(agent));
        let selected = selected_agent(choice, agent);
        if removing && !selected {
            if let (Some(registration), Some(old)) = (desired.as_deref_mut(), old) {
                registration.agents.insert(agent.into(), old.clone());
            }
            continue;
        }
        // Removal reaches here only for selected agents, which are removed.
        let retain = !removing && (selected || old.is_some());
        let binary = desired
            .as_deref()
            .filter(|_| retain)
            .map(|r| r.binary.clone());
        let installation = native::prepare(root, agent, old, binary.as_deref(), approved, files)?;
        if let (Some(r), Some(installation)) = (desired.as_deref_mut(), installation) {
            r.agents.insert(agent.into(), installation);
        }
    }
    Ok(())
}

/// Add the Box MCP definition to the plan: written when Box is selected,
/// refreshed with the other agents while unchanged, or removed on request.
pub(in crate::review) fn prepare_box(
    root: &Path,
    choice: IntegrationChoice,
    previous: Option<&Registration>,
    desired: Option<&mut Registration>,
    approved: &[String],
    files: &mut Vec<files::Change>,
) -> Result<(), Error> {
    let old = previous.and_then(|r| r.box_definition.as_ref());
    let selected = choice.contains(box_definition::ID);
    let removing = matches!(choice, IntegrationChoice::RemoveAgents(_));
    let Some(desired) = desired else {
        return Ok(());
    };
    if removing && !selected {
        desired.box_definition = old.cloned();
        return Ok(());
    }
    let binary = (!removing && (selected || old.is_some())).then_some(desired.binary.as_path());
    desired.box_definition = box_definition::prepare(root, old, binary, selected, approved, files)?;
    Ok(())
}

pub(in crate::review) fn capture_initial(
    root: &Path,
    choice: IntegrationChoice,
    before: Option<&str>,
) -> Result<Option<Registration>, Error> {
    let mut captured = desired(root)?;
    for adapter in agents::ADAPTERS {
        let agent = adapter.spec().id;
        if selected_agent(choice, agent) {
            let mcp_present = adapter.spec().mcp_path.is_none()
                && before.is_some_and(|bytes| {
                    crate::json::parse(bytes.as_bytes())
                        .is_ok_and(|v| v["mcpServers"]["shield"] == captured.entry)
                });
            if let Some(existing) =
                native::capture_matching(root, agent, &captured.binary, mcp_present)?
            {
                captured.agents.insert(agent.into(), existing);
            }
        }
    }
    Ok((!captured.agents.is_empty()).then_some(captured))
}

pub(in crate::review) fn selected_agent(choice: IntegrationChoice, agent: &str) -> bool {
    choice.contains(agent)
}

/// Preserve confirmed Ask fallbacks of the selected agents; default new ones
/// to Allow so the final review includes their scope.
pub(in crate::review) fn with_fallbacks(
    meta: &Value,
    selected: IntegrationChoice,
    plan: Option<&mut Plan>,
) -> Result<(), Error> {
    let Some(desired) = plan.and_then(|plan| plan.desired.as_mut()) else {
        return Ok(());
    };
    if matches!(selected, IntegrationChoice::RemoveAgents(_)) {
        return Ok(());
    }
    for adapter in crate::agents::ADAPTERS {
        let spec = adapter.spec();
        let covered = selected.contains(spec.id)
            || (selected.contains(super::box_definition::ID) && adapter.supports_box_hooks());
        if covered && spec.needs_ask_fallback {
            desired.ask_fallback.insert(
                spec.id.into(),
                confirmed_fallback(meta, spec.id)?.unwrap_or(AskFallback::Allow),
            );
        }
    }
    Ok(())
}

/// The review of a setup plan, including the agents' prefix rules.
pub(in crate::review) fn setup_preview(root: &Path, plan: Option<&Plan>) -> Value {
    let mut review = preview(root, plan);
    let prefixes = super::prefix_rules::active(root).unwrap_or_default();
    let desired = plan.and_then(|plan| plan.desired.as_ref());
    super::prefix_rules::preview(root, &mut review, desired, &prefixes);
    review
}

/// The operator review of an integration plan: each agent's hooks and MCP
/// server, the Codex Ask fallback and every owned file's exact before/after.
pub(in crate::review) fn preview(root: &Path, plan: Option<&Plan>) -> Value {
    let Some(p) = plan else {
        return json!({"operation":"keep integrations"});
    };
    let box_row = match (
        p.previous.as_ref().and_then(|r| r.box_definition.as_ref()),
        p.desired.as_ref().and_then(|r| r.box_definition.as_ref()),
    ) {
        (None, None) => None,
        (None, Some(_)) => Some("added"),
        (Some(_), None) => Some("removed"),
        (Some(b), Some(a)) if b == a => Some("unchanged"),
        _ => Some("updated"),
    }
    .map(|change| json!({"agent":super::box_definition::ID,"change":change,"mcp_server":"shield"}));
    let direct = p.desired.as_ref().is_some_and(|r| !r.agents.is_empty());
    let native = crate::agents::ADAPTERS
        .iter()
        .map(|a| a.spec().id)
        .filter_map(|agent| {
            let before = p
                .previous
                .as_ref()
                .and_then(|r| r.agents.get(agent))
                .map(|i| json!(i));
            let after = p
                .desired
                .as_ref()
                .and_then(|r| r.agents.get(agent))
                .map(|i| json!(i));
            let change = match (&before, &after) {
                (None, None) => return None,
                (None, Some(_)) => "added",
                (Some(_), None) if p.handed_over.iter().any(|id| id == agent) => "handed over",
                (Some(_), None) => "removed",
                (Some(b), Some(a)) if b == a => "unchanged",
                _ => "updated",
            };
            let shown = after.as_ref().or(before.as_ref())?;
            let hooks: Vec<_> = shown["groups"]
                .as_object()
                .into_iter()
                .flatten()
                .map(|(event, _)| event.clone())
                .collect();
            Some(json!({"agent":agent,"change":change,"hooks":hooks,"mcp_server":"shield"}))
        });
    let agents: Vec<_> = box_row.iter().cloned().chain(native).collect();
    let box_next = box_row
        .as_ref()
        .filter(|row| row["change"] != "removed")
        .map(|_| {
            json!({"publish":"box setup (first time) or box config sync",
            "direct_installations":direct,"handed_over":p.handed_over})
        });
    let fallbacks: Value = crate::agents::ADAPTERS
        .iter()
        .filter(|a| a.spec().needs_ask_fallback)
        .map(|a| {
            let id = a.spec().id;
            let before = p.previous.as_ref().and_then(|r| r.ask_fallback.get(id));
            let after = p.desired.as_ref().and_then(|r| r.ask_fallback.get(id));
            (id.to_owned(), json!({"before":before,"after":after}))
        })
        .collect::<serde_json::Map<String, Value>>()
        .into();
    // Publication leaves an absent shared MCP file absent when nothing goes in it.
    let mut files: Vec<_> = (p.before.is_some() || !p.after.is_empty())
        .then(|| json!({"path":".mcp.json","before":p.before,"after":p.after}))
        .into_iter()
        .collect();
    files.extend(p.files.iter().map(|change| json!(change)));
    let tool_approvals = p
        .desired
        .as_ref()
        .and_then(|r| r.approved_tools.as_ref())
        .map(|approved| {
            let ask: Vec<_> = crate::review::mcp::tool_summaries()
                .into_iter()
                .map(|(name, _)| name)
                .filter(|name| !approved.contains(name))
                .collect();
            json!({"approved":approved,"ask":ask})
        });
    json!({
        "tool_approvals":tool_approvals,
        "operation":if p.desired.is_some() {"set up agent integrations"} else {"remove Shield integrations"},
        "project":root,
        "binary":p.desired.as_ref().or(p.previous.as_ref()).map(|r| &r.binary),
        "agents":agents,
        "box":box_next,
        "connections":p.desired.as_ref().map(|r|r.connections.iter().flat_map(|(agent,value)|value["review"].as_array().into_iter().flatten().map(move |row| {let mut row=row.clone();row["agent"]=json!(agent);row})).collect::<Vec<_>>()).unwrap_or_default(),
        "ask_fallbacks":fallbacks,
        "adopt_existing_entries":p.previous.is_some() && !root.join(".shield/review/state.json").exists(),
        "files":files,
    })
}

/// Apply a confirmed plan, then bring prefix rules in line with the active
/// policy. `receipt` replaces the status receipt only when nothing was applied.
pub(in crate::review) fn finish(
    context: &Context,
    meta: &Value,
    plan: Option<&Plan>,
    receipt: Option<Value>,
) -> Result<Value, Error> {
    if let Some(plan) = plan
        && let Err(error) = apply(context, meta, plan)
    {
        if pending(&crate::review::project::metadata(
            &crate::review::project::checkpoint(context)?,
        ))?
        .is_none()
        {
            return Err(error);
        }
        let mut receipt = crate::review::project::status(context)?;
        receipt["setup_error"] = json!(error.to_string());
        receipt["repair_route"] = json!("shield mcp setup");
        return Ok(receipt);
    }
    let synced = sync_prefix_rules(context);
    let mut receipt = match receipt {
        Some(receipt) if plan.is_none() => receipt,
        _ => crate::review::project::status(context)?,
    };
    if let Err(error) = synced {
        receipt["setup_error"] = json!(error.to_string());
        receipt["repair_route"] = json!("shield mcp setup");
    }
    Ok(receipt)
}

/// Write the active policy's prefix rules for installed agents, or remove them
/// with the agent. Without a readable activation they wait for the next one.
pub(in crate::review) fn sync_prefix_rules(context: &Context) -> Result<(), Error> {
    let root = Path::new(&context.project_root);
    let Ok(prefixes) = super::prefix_rules::active(root) else {
        return Ok(());
    };
    super::prefix_rules::sync(
        root,
        &crate::review::project::metadata(&crate::review::project::checkpoint(context)?),
        &prefixes,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn box_handover_preserves_adapters_without_box_hooks() {
        let root = assert_fs::TempDir::new().unwrap();
        let mut previous = desired(root.path()).unwrap();
        for id in ["claude", "codex"] {
            previous.agents.insert(
                id.into(),
                serde_json::from_value(json!({"groups":{}})).unwrap(),
            );
        }
        let choice = IntegrationChoice::Agents(IntegrationChoice::bits(&["box"]).unwrap());
        // Model an unsupported adapter without changing the global registry.
        let handed = handed_over_with(Some(&previous), choice, true, |id| id == "claude");
        assert_eq!(handed, ["claude"]);
        assert!(handed_over_with(Some(&previous), choice, true, |_| false).is_empty());
        assert!(handed_over_with(Some(&previous), choice, false, |_| true).is_empty());
    }
}
