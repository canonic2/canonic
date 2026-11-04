//! Terminal onboarding, integration selection and confirmation.
use super::{state, terminal::Terminal};
use crate::{
    agents,
    cli::public::IntegrationChoice,
    review::{
        self, Context, Error,
        integrations::{
            AskFallback, MAX_CONFIG, Plan, Registration, apply, binary_digest, box_definition,
            confirmed_fallback, files, mcp_after, native, pending, prefix_rules, read_file,
            registration,
        },
    },
};
use serde_json::{Value, json};
#[cfg(test)]
use std::fs;
use std::path::Path;
mod connections;
mod workflow;
use connections::prepare_connections;

fn desired(root: &Path) -> Result<Registration, Error> {
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
fn prepare(root: &Path, meta: &Value, choice: IntegrationChoice) -> Result<Option<Plan>, Error> {
    prepare_with(root, meta, choice, false, None, true)
}

#[cfg(test)]
fn prepare_initial(root: &Path, choice: IntegrationChoice) -> Result<Option<Plan>, Error> {
    prepare_approving(root, &json!({}), choice, true, None, true)
}

/// Prepare with the operator's tool approvals; `None` keeps the saved ones,
/// or the defaults for a first setup. `handover` drops the records of direct
/// installations a Box install replaces, leaving their files in place.
fn prepare_approving(
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
fn approved_tools(meta: &Value) -> Result<Vec<String>, Error> {
    Ok(registration(meta)?
        .and_then(|r| r.approved_tools)
        .unwrap_or_else(crate::review::mcp::default_approved))
}

fn prepare_with(
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
fn handed_over(
    previous: Option<&Registration>,
    choice: IntegrationChoice,
    handover: bool,
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
                .cloned()
                .collect()
        }
        _ => Vec::new(),
    }
}

/// Plan native and Box files, then keep only the Ask fallbacks of agents the
/// desired record still covers.
fn prepare_files(
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
fn prepare_native(
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
fn prepare_box(
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

fn capture_initial(
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

fn selected_agent(choice: IntegrationChoice, agent: &str) -> bool {
    choice.contains(agent)
}

#[derive(Clone, Copy)]
pub(super) enum Baseline {
    Skip,
    Choose,
    Selected(crate::policy::Profile),
}

pub(super) fn initialize_in_cache(
    start: &Path,
    choice: Option<IntegrationChoice>,
    json_mode: bool,
    cache_dir: Option<&Path>,
    baseline: Baseline,
) -> Result<Value, Error> {
    let root = start.canonicalize()?;
    if root.join(".shield/review/state.json").exists() {
        Terminal::required(json_mode)?;
        if choice.is_some_and(|c| !matches!(c, IntegrationChoice::Keep)) {
            return Err(Error::Invalid(
                "project already initialized; use shield mcp setup to change agents",
            ));
        }
        let mut receipt = crate::review::project::status(&review::open(&root)?)?;
        receipt["next_action"] = json!("shield mcp setup");
        return Ok(receipt);
    }
    workflow::onboard(&root, choice, json_mode, cache_dir, baseline)
}

pub(super) fn setup(
    context: &Context,
    choice: Option<IntegrationChoice>,
    json_mode: bool,
) -> Result<Value, Error> {
    workflow::setup(context, choice, json_mode)
}

pub(super) fn setup_review(
    context: &Context,
    choice: Option<IntegrationChoice>,
    json_mode: bool,
) -> Result<Value, Error> {
    workflow::review(context, choice, json_mode)
}

#[cfg(test)]
pub(crate) fn configure_fixture(root: &Path, choice: IntegrationChoice, setting: AskFallback) {
    let context = review::open(root).unwrap();
    let meta = crate::review::project::metadata(&crate::review::project::load(&context).unwrap());
    let mut plan = prepare(root, &meta, choice).unwrap().unwrap();
    plan.desired
        .as_mut()
        .unwrap()
        .ask_fallback
        .insert("codex".into(), setting);
    apply(&context, &meta, &plan).unwrap();
}

#[cfg(test)]
mod box_tests;
#[cfg(test)]
mod tests;
