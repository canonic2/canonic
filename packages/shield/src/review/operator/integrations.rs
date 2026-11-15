//! Terminal onboarding, integration selection and confirmation.
use super::{state, terminal::Terminal};
#[cfg(test)]
use crate::{
    agents,
    review::integrations::{
        AskFallback, MAX_CONFIG, apply, files, native, prefix_rules, read_file,
    },
};
use crate::{
    cli::public::IntegrationChoice,
    review::{
        self, Context, Error,
        integrations::{Plan, host_definition, pending, registration},
    },
};
use serde_json::{Value, json};
#[cfg(test)]
use std::fs;
use std::path::Path;
mod workflow;

use crate::review::integrations::setup::{approved_tools, handed_over, prepare_approving};
#[cfg(test)]
use crate::review::integrations::setup::{prepare, prepare_initial, prepare_with};

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
    // The policy files are the policy: a folder that has them, such as a clone,
    // worktree or copy, is already initialized and needs no review.
    if crate::layout::init_destination(&root).exists() {
        let existing = crate::layout::state(&root).join("state.json").exists();
        let context = review::open(&root)?;
        if existing && choice.is_some_and(|c| !matches!(c, IntegrationChoice::Keep)) {
            return Err(Error::Invalid(
                "project already initialized; use shield mcp setup to change agents",
            ));
        }
        write_missing_locks(&context)?;
        // A folder initialized here, such as a fresh clone, takes the chosen
        // plan location; an enrolled one changes it with mcp setup.
        if let Some(directory) = cache_dir.filter(|_| !existing) {
            crate::review::files::relocate_cache(&context, &std::path::absolute(directory)?)?;
        }
        let mut receipt = crate::review::project::status(&context)?;
        receipt["next_action"] = json!("shield mcp setup");
        if !existing
            && choice.is_some_and(|choice| {
                !matches!(
                    choice,
                    IntegrationChoice::Keep | IntegrationChoice::Agents(0)
                )
            })
        {
            receipt["agent_setup"] = setup(&context, choice, json_mode, false)?;
        }
        return Ok(receipt);
    }
    workflow::onboard(&root, choice, json_mode, cache_dir, baseline)
}

/// Write the portable lock files of policy files that arrived without them,
/// so the packs they select stay reproducible. Existing locks are kept.
fn write_missing_locks(context: &Context) -> Result<(), Error> {
    use crate::review::publication::projections::{locks, materialize};
    let root = Path::new(&context.project_root);
    let state = crate::review::project::load(context)?;
    let meta = crate::review::project::metadata(&state);
    let missing: std::collections::BTreeMap<_, _> = locks(&state.candidates["project"], &meta)?
        .into_iter()
        .filter(|(name, _)| !crate::layout::bundle_file(root, name).exists())
        .collect();
    if !missing.is_empty() {
        materialize(context, &missing)?;
    }
    Ok(())
}

pub(super) fn setup(
    context: &Context,
    choice: Option<IntegrationChoice>,
    json_mode: bool,
    reset_tool_permissions: bool,
) -> Result<Value, Error> {
    workflow::setup(context, choice, json_mode, reset_tool_permissions)
}

pub(super) fn recover(
    context: &Context,
    action: review::integrations::recovery::Action,
    json_mode: bool,
    dry_run: bool,
) -> Result<Value, Error> {
    if json_mode && !dry_run {
        return Err(Error::ApprovalRequired);
    }
    let prepared = review::admin::prepare(
        context,
        "mcp_setup",
        &json!({"pending":action,"dry_run":dry_run}),
        &|| Ok(()),
    )?;
    if dry_run {
        return Ok(prepared.public_review());
    }
    let mut terminal = Terminal::required(json_mode)?;
    terminal.confirm_mcp(&prepared.review())?;
    prepared.commit(context, &|| Ok(()))
}

pub(super) fn setup_review(
    context: &Context,
    choice: Option<IntegrationChoice>,
    json_mode: bool,
    reset_tool_permissions: bool,
) -> Result<Value, Error> {
    workflow::review(context, choice, json_mode, reset_tool_permissions)
}

#[cfg(test)]
pub(crate) fn configure_fixture(root: &Path, choice: IntegrationChoice, setting: AskFallback) {
    let context = review::open(root).unwrap();
    let meta = crate::review::project::metadata(&crate::review::project::load(&context).unwrap());
    let mut plan = prepare(root, &meta, choice).unwrap().unwrap();
    if choice.contains("codex") {
        plan.desired
            .as_mut()
            .unwrap()
            .ask_fallback
            .insert("codex".into(), setting);
    }
    apply(&context, &meta, &plan).unwrap();
}

#[cfg(test)]
mod box_tests;
#[cfg(test)]
mod tests;
