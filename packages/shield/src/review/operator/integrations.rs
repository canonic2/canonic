//! Terminal onboarding, integration selection and confirmation.
use super::{state, terminal::Terminal};
use crate::{
    agents,
    cli::public::IntegrationChoice,
    review::{
        self, Context, Error,
        integrations::{
            AskFallback, MAX_CONFIG, Plan, Registration, apply, binary_digest, confirmed_fallback,
            files, mcp_after, native, pending, prefix_rules, read_file, registration,
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
    })
}

#[cfg(test)]
fn prepare(root: &Path, meta: &Value, choice: IntegrationChoice) -> Result<Option<Plan>, Error> {
    prepare_with(root, meta, choice, false, None)
}

#[cfg(test)]
fn prepare_initial(root: &Path, choice: IntegrationChoice) -> Result<Option<Plan>, Error> {
    prepare_approving(root, &json!({}), choice, true, None)
}

/// Prepare with the operator's tool approvals; `None` keeps the saved ones,
/// or the defaults for a first setup.
fn prepare_approving(
    root: &Path,
    meta: &Value,
    choice: IntegrationChoice,
    initial: bool,
    approved: Option<Vec<String>>,
) -> Result<Option<Plan>, Error> {
    if initial && matches!(choice, IntegrationChoice::Keep) {
        return Ok(None);
    }
    prepare_with(root, meta, choice, initial, approved)
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
    let mut files = Vec::new();
    for adapter in agents::ADAPTERS {
        let agent = adapter.spec().id;
        let old = previous.as_ref().and_then(|r| r.agents.get(agent));
        let selected = selected_agent(choice, agent);
        if matches!(choice, IntegrationChoice::RemoveAgents(_)) {
            if !selected {
                if let (Some(registration), Some(old)) = (&mut desired, old) {
                    registration.agents.insert(agent.into(), old.clone());
                }
                continue;
            }
            if let Some(registration) = &mut desired {
                registration.ask_fallback.remove(agent);
            }
        }
        let retain = if matches!(choice, IntegrationChoice::RemoveAgents(_)) {
            old.is_some() && !selected
        } else {
            selected || old.is_some()
        };
        let binary = desired
            .as_ref()
            .filter(|_| retain)
            .map(|r| r.binary.as_path());
        let installation = native::prepare(root, agent, old, binary, &approved, &mut files)?;
        if let (Some(r), Some(installation)) = (&mut desired, &installation) {
            r.agents.insert(agent.into(), installation.clone());
        }
    }
    if desired
        .as_ref()
        .is_some_and(|registration| registration.agents.is_empty())
    {
        desired = None;
    }
    if desired.is_none() && previous.is_none() {
        return Ok(None);
    }
    let after = mcp_after(before.as_deref(), previous.as_ref(), desired.as_ref())?;
    let (after, policy_revision) = prepare_connections(
        root,
        &after,
        previous.as_ref(),
        desired.as_mut(),
        &mut files,
    )?;
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
    }))
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
mod tests;
