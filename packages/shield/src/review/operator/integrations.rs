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
        integrations::{Plan, box_definition, pending, registration},
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
