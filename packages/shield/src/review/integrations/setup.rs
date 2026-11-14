//! Transport-neutral integration planning, exact review and publication.
use super::{
    IntegrationChoice, MAX_CONFIG, Plan, Registration, binary_digest, box_definition, files,
    mcp_after, native, pending, read_file, registration,
};
use crate::configuration::settings::Values;
use crate::{
    agents,
    review::{self, Context, Error},
};
use serde_json::{Value, json};
use std::path::Path;
mod connections;
mod scope;
pub(super) use connections::prepare_configured;
use connections::prepare_connections;

/// Continue already confirmed work without asking the person to repeat setup.
/// Conflicting owned edits remain pending for a new, exact human review.
pub(in crate::review) fn resume_confirmed(context: &Context) -> Result<bool, Error> {
    resume_for(context, true)
}

pub(in crate::review) fn resume_for(context: &Context, explicit: bool) -> Result<bool, Error> {
    let meta = crate::review::project::metadata(&crate::review::project::checkpoint(context)?);
    if let Some(plan) = pending(&meta)? {
        if !explicit
            && context
                .agent
                .as_deref()
                .is_some_and(|agent| !plan.affects(agent))
        {
            return Ok(false);
        }
        resume_plan(context, &meta, &plan, None)?;
        return Ok(true);
    }
    Ok(false)
}

pub(super) fn resume_plan(
    context: &Context,
    metadata: &Value,
    plan: &Plan,
    activation: Option<&str>,
) -> Result<(), Error> {
    review::project::checkpoint(context)?;
    let publication =
        review::publication::ProjectionLease::acquire(Path::new(&context.project_root))?;
    resume_plan_guarded(context, metadata, plan, activation, &publication)
}

pub(in crate::review) fn resume_plan_guarded(
    context: &Context,
    metadata: &Value,
    plan: &Plan,
    activation: Option<&str>,
    publication: &review::publication::ProjectionLease,
) -> Result<(), Error> {
    plan.check_maintenance_binary()?;
    super::apply_owned_guarded(context, metadata, plan, &|| Ok(()), activation, publication)?;
    sync_prefix_rules_for(context, Some(plan), publication)
}

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
        interceptor: None,
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
    prepare_with(root, meta, choice, false, None, true, false)
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
    prepare_with(root, meta, choice, initial, approved, handover, false)
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
    repair_hooks: bool,
) -> Result<Option<Plan>, Error> {
    let canonical_root = root.canonicalize()?;
    let root = canonical_root.as_path();
    if let Some(plan) = pending(meta)? {
        return retained_plan(root, meta, choice, plan).map(Some);
    }
    let mut previous = registration(meta)?;
    if matches!(choice, IntegrationChoice::Keep) {
        return Ok(None);
    }
    let fresh = matches!(choice, IntegrationChoice::Agents(_))
        .then(|| desired(root))
        .transpose()?;
    let approved = approved.unwrap_or(approved_tools(meta)?);
    let requested = choice;
    let (choice, scope_reason) =
        scope::selection(choice, previous.as_ref(), fresh.as_ref(), &approved)?;
    let targets = selected_targets(choice);
    let shared = choice.contains(box_definition::ID)
        || agents::ADAPTERS
            .iter()
            .any(|a| a.spec().mcp_path.is_none() && choice.contains(a.spec().id));
    let before = if shared { read_file(root)? } else { None };
    if initial && previous.is_none() && !matches!(choice, IntegrationChoice::RemoveAgents(_)) {
        previous = capture_initial(root, choice, before.as_deref())?;
    }
    let mut desired = next_registration(previous.as_ref(), fresh, &approved, choice);
    let handed_over = handed_over(previous.as_ref(), choice, handover);
    if let Some(desired) = &mut desired {
        for agent in &handed_over {
            desired.agents.remove(agent);
        }
    }
    // Files are reconciled as if Shield never recorded the handed-over
    // installations, so their native entries stay exactly as they are.
    let planned = previous.clone().map(|mut registration| {
        for agent in &handed_over {
            registration.agents.remove(agent);
        }
        registration
    });
    let mut files = Vec::new();
    let file_choice = file_selection(choice, requested, previous.as_ref())?;
    prepare_files(
        root,
        file_choice,
        planned.as_ref(),
        desired.as_mut(),
        &approved,
        &mut files,
        repair_hooks,
    )?;
    if desired.as_ref().is_some_and(|registration| {
        registration.agents.is_empty()
            && registration.box_definition.is_none()
            && registration.interceptor.is_none()
    }) {
        desired = None;
    }
    if desired.is_none() && previous.is_none() {
        return Ok(None);
    }
    let repair_shared = repair_hooks
        && agents::ADAPTERS.iter().any(|adapter| {
            adapter.spec().mcp_path.is_none() && selected_agent(choice, adapter.spec().id)
        });
    let after = if shared {
        mcp_after(
            before.as_deref(),
            planned.as_ref(),
            desired.as_ref(),
            repair_shared,
        )?
    } else {
        String::new()
    };
    let (after, policy_revision) = prepare_connections(
        root,
        &after,
        planned.as_ref(),
        desired.as_mut(),
        &mut files,
        &targets,
    )?;
    if after.len() as u64 > MAX_CONFIG {
        return Err(Error::Invalid("MCP configuration exceeds limit"));
    }
    let plan = Plan {
        targets: Some(targets),
        scope_reason,
        policy_revision,
        before,
        after,
        previous,
        desired,
        files,
        handed_over,
    };
    super::maintenance::validate(&plan)?;
    Ok(Some(plan))
}

/// Shared-launcher review does not select an omitted SDK-to-native migration.
fn file_selection(
    choice: IntegrationChoice,
    requested: IntegrationChoice,
    previous: Option<&Registration>,
) -> Result<IntegrationChoice, Error> {
    match choice {
        IntegrationChoice::Agents(selected)
            if !requested.contains("claude")
                && previous.is_some_and(|r| r.interceptor.is_some()) =>
        {
            Ok(IntegrationChoice::Agents(
                selected & !IntegrationChoice::bits(&["claude"])?,
            ))
        }
        _ => Ok(choice),
    }
}

fn selected_targets(choice: IntegrationChoice) -> Vec<String> {
    std::iter::once(box_definition::ID)
        .chain(agents::ADAPTERS.iter().map(|a| a.spec().id))
        .filter(|id| choice.contains(id))
        .map(str::to_owned)
        .collect()
}

fn retained_plan(
    root: &Path,
    meta: &Value,
    choice: IntegrationChoice,
    plan: Plan,
) -> Result<Plan, Error> {
    super::validate_pending_ownership(meta)?;
    files::validate_plan_recovery(root, &plan)?;
    if plan.touches_shared() {
        super::reconcile::reconcile(
            ".mcp.json",
            plan.before.as_deref(),
            super::shared_after(&plan),
            read_file(root)?.as_deref(),
        )?;
    }
    if !matches!(choice, IntegrationChoice::Keep) {
        return Err(Error::Invalid(
            "a confirmed setup is unfinished; review or cancel it through mcp_setup before selecting another change",
        ));
    }
    Ok(plan)
}

fn next_registration(
    previous: Option<&Registration>,
    mut fresh: Option<Registration>,
    approved: &[String],
    choice: IntegrationChoice,
) -> Option<Registration> {
    if !choice.contains("claude")
        && let Some((fresh, previous)) = fresh.as_mut().zip(previous)
    {
        fresh.entry.clone_from(&previous.entry);
    }
    let mut next = previous.cloned().or_else(|| fresh.clone());
    if let (Some(next), Some(fresh)) = (&mut next, fresh) {
        next.binary = fresh.binary;
        next.binary_digest = fresh.binary_digest;
        next.version = fresh.version;
        next.entry = fresh.entry;
        next.approved_tools = Some(approved.to_vec());
    }
    next
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
    repair_hooks: bool,
) -> Result<(), Error> {
    prepare_native(
        root,
        choice,
        previous,
        desired.as_deref_mut(),
        approved,
        files,
        repair_hooks,
    )?;
    prepare_box(
        root,
        choice,
        previous,
        desired.as_deref_mut(),
        approved,
        files,
    )?;
    let replace_legacy = selected_agent(choice, "claude")
        || (desired.as_ref().is_some_and(|r| r.native_covers("claude"))
            && previous.is_none_or(|r| !r.native_covers("claude")));
    super::legacy::prepare(
        root,
        previous,
        desired.as_deref_mut(),
        replace_legacy,
        files,
    )?;
    if let Some(registration) = desired {
        let covered: Vec<String> = registration
            .ask_fallback
            .keys()
            .filter(|agent| {
                registration.covers(agent)
                    || !(choice.contains(agent)
                        || choice.contains(box_definition::ID)
                            && agents::find(agent)
                                .is_some_and(agents::AgentAdapter::supports_box_hooks))
            })
            .cloned()
            .collect();
        registration
            .ask_fallback
            .retain(|agent, _| covered.contains(agent));
    }
    Ok(())
}

/// Add each native adapter's hooks, MCP entry and settings to the plan:
/// installed, refreshed or removed only when selected. Omitted records stay intact.
pub(in crate::review) fn prepare_native(
    root: &Path,
    choice: IntegrationChoice,
    previous: Option<&Registration>,
    mut desired: Option<&mut Registration>,
    approved: &[String],
    files: &mut Vec<files::Change>,
    repair_hooks: bool,
) -> Result<(), Error> {
    let removing = matches!(choice, IntegrationChoice::RemoveAgents(_));
    for adapter in agents::ADAPTERS {
        let agent = adapter.spec().id;
        let old = previous.and_then(|r| r.agents.get(agent));
        let selected = selected_agent(choice, agent);
        if !selected {
            continue;
        }
        // Removal reaches here only for selected agents, which are removed.
        let retain = !removing;
        let binary = desired
            .as_deref()
            .filter(|_| retain)
            .map(|r| r.binary.clone());
        let installation = native::prepare(
            root,
            agent,
            old,
            binary.as_deref(),
            approved,
            files,
            repair_hooks && selected,
        )?;
        if let (Some(r), Some(installation)) = (desired.as_deref_mut(), installation) {
            r.agents.insert(agent.into(), installation);
        } else if removing && let Some(r) = desired.as_deref_mut() {
            r.agents.remove(agent);
        }
    }
    // A later installation can invalidate an omitted adapter's supported
    // runtime contract. Validate the complete desired set against the final
    // projected files, including edits staged by another adapter above.
    if let Some(registration) = desired.as_deref() {
        for adapter in agents::ADAPTERS {
            if registration.agents.contains_key(adapter.spec().id) {
                adapter.check_native_setup(root, &|name| {
                    files
                        .iter()
                        .find(|change| change.path == name)
                        .map(|change| change.after.clone())
                })?;
            }
        }
    }
    Ok(())
}

/// Add the Box MCP definition to the plan: written when Box is selected,
/// or removed on request. Omitted definitions are preserved without inspection.
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
    if !selected {
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

/// Put the Ask fallbacks into the plan: the one the policy files name for
/// agents already set up, kept for omitted ones; for selected agents the one the policy files
/// name, or else the adapter's first-setup default shown in the final review.
pub(in crate::review) fn with_fallbacks(
    settings: &Values,
    selected: IntegrationChoice,
    plan: Option<&mut Plan>,
) {
    let Some(plan) = plan else {
        return;
    };
    let needed = crate::agents::ADAPTERS
        .iter()
        .filter(|adapter| adapter.spec().needs_ask_fallback);
    if let Some(previous) = plan.previous.as_mut() {
        for adapter in needed.clone() {
            let id = adapter.spec().id;
            if let Some(named) = settings
                .ask_fallback
                .get(id)
                .filter(|_| previous.covers(id))
            {
                previous.ask_fallback.insert(id.into(), *named);
            }
        }
    }
    let Some(desired) = plan.desired.as_mut() else {
        return;
    };
    let removing = matches!(selected, IntegrationChoice::RemoveAgents(_));
    for adapter in needed {
        let id = adapter.spec().id;
        let chosen = !removing
            && (selected.contains(id)
                || (selected.contains(super::box_definition::ID) && adapter.supports_box_hooks()));
        let kept = plan.previous.as_ref().and_then(|r| r.ask_fallback.get(id));
        if chosen {
            desired.ask_fallback.insert(
                id.into(),
                settings
                    .ask_fallback
                    .get(id)
                    .copied()
                    .unwrap_or_else(|| adapter.default_ask_fallback()),
            );
        } else if let Some(kept) = kept.filter(|_| desired.covers(id)) {
            desired.ask_fallback.insert(id.into(), *kept);
        }
    }
}

/// The review of a setup plan, including the agents' prefix rules.
pub(in crate::review) fn setup_preview(root: &Path, plan: Option<&Plan>) -> Result<Value, Error> {
    let enabled = crate::review::mode::current(root)?;
    let mut review = preview(root, plan);
    if plan.is_none() {
        return Ok(review);
    }
    let prefixes = super::prefix_rules::active(root).unwrap_or_default();
    let desired = plan.and_then(|plan| plan.desired.as_ref());
    super::prefix_rules::preview_for(root, &mut review, desired, &prefixes, enabled, plan);
    Ok(review)
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
    .filter(|_| p.affects(box_definition::ID))
    .map(|change| json!({"agent":super::box_definition::ID,"change":change,"mcp_server":"shield"}));
    let direct = p.desired.as_ref().is_some_and(|r| !r.agents.is_empty());
    let native = crate::agents::ADAPTERS
        .iter()
        .map(|a| a.spec().id)
        .filter(|agent| p.affects(agent))
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
        .filter(|a| a.spec().needs_ask_fallback && p.affects(a.spec().id))
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
        "maintenance_scope":{"targets":p.targets,"reason":p.scope_reason},
        "operation":if p.desired.is_some() {"set up agent integrations"} else {"remove Shield integrations"},
        "project":root,
        "host_path_warning":super::HOST_PATH_WARNING,
        "binary":p.desired.as_ref().or(p.previous.as_ref()).map(|r| &r.binary),
        "agents":agents,
        "box":box_next,
        "connections":p.desired.as_ref().map(|r|r.connections.iter().filter(|(agent,_)|p.affects(agent)).flat_map(|(agent,value)|value["review"].as_array().into_iter().flatten().map(move |row| {let mut row=row.clone();row["agent"]=json!(agent);row})).collect::<Vec<_>>()).unwrap_or_default(),
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
    finish_checked(context, meta, plan, receipt, &|| Ok(()))
}

pub(in crate::review) fn finish_checked(
    context: &Context,
    meta: &Value,
    plan: Option<&Plan>,
    receipt: Option<Value>,
    checkpoint: crate::review::Checkpoint<'_>,
) -> Result<Value, Error> {
    review::project::checkpoint(context)?;
    let publication =
        review::publication::ProjectionLease::acquire(Path::new(&context.project_root))?;
    finish_guarded(context, meta, plan, receipt, checkpoint, &publication)
}
pub(in crate::review) fn finish_guarded(
    context: &Context,
    meta: &Value,
    plan: Option<&Plan>,
    receipt: Option<Value>,
    checkpoint: crate::review::Checkpoint<'_>,
    publication: &review::publication::ProjectionLease,
) -> Result<Value, Error> {
    publication.check(context)?;
    if let Some(plan) = plan
        && let Err(error) =
            super::apply_owned_guarded(context, meta, plan, checkpoint, None, publication)
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
        receipt["repair_route"] = json!("mcp_setup");
        receipt["repair_arguments"] = json!({"pending":"review"});
        receipt["pending_setup"] = json!({"review":{"tool":"mcp_setup","arguments":{"pending":"review"}},"cancel":{"tool":"mcp_setup","arguments":{"pending":"cancel"}}});
        return Ok(receipt);
    }
    let synced = if plan.is_some() {
        sync_prefix_rules_for(context, plan, publication)
    } else {
        Ok(())
    };
    let mut receipt = match receipt {
        Some(receipt) if plan.is_none() => receipt,
        _ => crate::review::project::status(context)?,
    };
    if let Err(error) = synced {
        receipt["setup_error"] = json!(error.to_string());
        receipt["repair_route"] = json!("mcp_setup");
        receipt["next_action"] = json!(
            "Review agent setup in this session. The integration publication is settled; this separate prefix-rule failure is not a pending setup conflict."
        );
    }
    if let Some(plan) = plan {
        receipt["maintenance_scope"] = json!({"targets":plan.targets,"reason":plan.scope_reason});
    }
    Ok(receipt)
}

/// Write only the prefix rules covered by this setup's review.
fn sync_prefix_rules_for(
    context: &Context,
    plan: Option<&Plan>,
    publication: &review::publication::ProjectionLease,
) -> Result<(), Error> {
    publication.check(context)?;
    let root = Path::new(&context.project_root);
    let Ok((state, evaluator)) =
        super::Store::open(root).and_then(|store| store.read_current_evaluated())
    else {
        return Ok(());
    };
    // The evaluator is the policy files' own plan, which requests follow.
    state.check(context)?;
    super::prefix_rules::sync_for(
        root,
        &crate::review::project::metadata(&state),
        evaluator.codex_prefixes(),
        state.settings.enabled(),
        plan,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_registry_selection_refuses_incompatible_copilot_and_claude() {
        let root = assert_fs::TempDir::new().unwrap();
        let ids: Vec<_> = crate::agents::ADAPTERS
            .iter()
            .map(|adapter| adapter.spec().id)
            .collect();
        let selection = IntegrationChoice::Agents(IntegrationChoice::bits(&ids).unwrap());
        let mut next = desired(root.path()).unwrap();
        let mut changes = vec![];
        let error = prepare_native(
            root.path(),
            selection,
            None,
            Some(&mut next),
            &[],
            &mut changes,
            false,
        )
        .err()
        .unwrap();
        assert!(error.to_string().contains("separate trusted Copilot"));
        assert!(!root.path().join(".claude/settings.local.json").exists());
        assert!(!root.path().join(".github/hooks/shield.json").exists());
    }

    #[test]
    fn copilot_existing_registration_refuses_claude_later_and_removal_stays_available() {
        let root = assert_fs::TempDir::new().unwrap();
        let mut previous = desired(root.path()).unwrap();
        previous.agents.insert(
            "copilot".into(),
            serde_json::from_value(json!({"groups":{}})).unwrap(),
        );
        let mut next = previous.clone();
        let mut changes = vec![];
        let error = prepare_native(
            root.path(),
            IntegrationChoice::agent("claude"),
            Some(&previous),
            Some(&mut next),
            &[],
            &mut changes,
            false,
        )
        .err()
        .unwrap();
        assert!(error.to_string().contains("separate trusted Copilot"));
        assert!(!root.path().join(".claude/settings.local.json").exists());
        let mut next = previous.clone();
        let mut changes = vec![];
        prepare_native(
            root.path(),
            IntegrationChoice::RemoveAgents(IntegrationChoice::bits(&["copilot"]).unwrap()),
            Some(&previous),
            Some(&mut next),
            &[],
            &mut changes,
            false,
        )
        .unwrap();
        assert!(!next.agents.contains_key("copilot"));
    }

    #[test]
    fn shield_mode_setup_preview_refuses_unreadable_or_invalid_enrolled_state() {
        let root = assert_fs::TempDir::new().unwrap();
        let rules = json!({"schema_version":1,"kind":"custom",
            "capabilities":["agent-outputs-v1","argument-rules-v1","structured-rules-v1"],
            "rules":[{"id":"logs","decision":"allow","agents":{"codex":{"prefixes":[["acme","logs"]]}},
                "conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"prefix","values":["logs"]}}}]}).to_string();
        let bundle = crate::configuration::Bundle::capture(
            "schema_version: 1\npacks: []\nrules: rules.yml\n",
            &[],
            Some(&rules),
        )
        .unwrap();
        let context = crate::review::mcp::initialize_fixture(root.path(), &bundle);
        let metadata = crate::review::project::metadata(
            &crate::review::project::checkpoint(&context).unwrap(),
        );
        let plan = prepare(root.path(), &metadata, IntegrationChoice::agent("codex")).unwrap();
        let shown = setup_preview(root.path(), plan.as_ref()).unwrap();
        assert!(!shown["codex_prefixes"].as_array().unwrap().is_empty());
        let path = root.path().join(".shield/review/state.json");
        let mut state: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        state["enforcement"] = json!({"enabled":"unreadable-mode","generation":0});
        let damaged = state.to_string();
        std::fs::write(&path, &damaged).unwrap();
        assert!(matches!(
            setup_preview(root.path(), plan.as_ref()),
            Err(Error::StateCorrupt { .. })
        ));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), damaged);
        assert!(!root.path().join(".codex/rules/shield.rules").exists());
    }

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
