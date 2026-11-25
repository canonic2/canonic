//! Terminal initialization and candidate confirmation for operator workflows.
use super::terminal::Terminal;
use crate::{
    configuration, policy,
    review::{self, Context, Error, model::State, project::status, publication},
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path};

pub(super) fn initialize(
    project: &Path,
    json_mode: bool,
    setup: &Value,
    desired: Option<&review::integrations::Registration>,
    cache_dir: Option<&Path>,
    baseline: Option<policy::Profile>,
) -> Result<Value, Error> {
    let mut tty = Terminal::required(json_mode)?;
    let project = project.canonicalize()?;
    // An enrolled folder's files are already its policy; there is nothing to
    // initialize or restore.
    if crate::layout::state(&project).join("state.json").exists() {
        let context = review::open(&project)?;
        return status(&context);
    }
    let config = crate::layout::init_destination(&project);
    let candidate = if config.exists() {
        super::project_files::capture(&config).map_err(|_| {
            Error::Invalid("invalid project configuration; review config.yml and local artifacts")
        })?
    } else {
        configuration::Bundle::capture(r#"{"schema_version":1,"packs":[]}"#, &[], None)?
    };
    let candidate = json!(candidate);
    review::changes::historical::validate(&Value::Null, &candidate)?;
    let config = configuration::layers::merged(&candidate)?;
    let installed = review::integrations::project_sources::installed(&config, &candidate)?;
    let mut meta = json!({"sources":{},"default":null,"installed":installed,"projection":null});
    let disabled = review::integrations::project_sources::disabled(&config, &candidate)?;
    if !disabled.is_empty() {
        meta["disabled"] = json!(disabled);
    }
    if let Some(cache_dir) = cache_dir {
        meta["compiled_cache_root"] = json!(cache_dir);
    }
    let mut bytes: Vec<String> = serde_json::from_value(candidate["packs"].clone())
        .map_err(|_| Error::Invalid("pack bytes missing"))?;
    let checkpoints = if let Some(profile) = baseline {
        tty.progress("Fetching the baseline from the main registry…")?;
        let initial = crate::review::packs::bootstrap::acquire(
            &project,
            profile,
            &crate::review::registries::fetcher(),
        )?;
        meta["installed"]
            .as_array_mut()
            .ok_or(Error::Invalid("installed missing"))?
            .extend(initial.rows);
        bytes.extend(initial.bytes);
        initial.checkpoints
    } else {
        BTreeMap::default()
    };
    let candidate = crate::review::packs::rebuild(
        &candidate,
        meta["installed"]
            .as_array_mut()
            .ok_or(Error::Invalid("installed missing"))?,
        &bytes,
        None,
    )?;
    let (candidate, meta) = super::packs::settle_first(&mut tty, &project, candidate, meta)?;
    let verified_main = checkpoints.contains_key("main");
    let prepared =
        review::initialization::Prepared::new(&project, candidate.clone(), meta, checkpoints)?;
    let mut setup = setup.clone();
    prepared.preview_prefixes(&mut setup, desired);
    tty.confirm_admin(&json!({"operation":"initialize project","project":project,
        "policy":candidate,"registry_verification":if verified_main {"signed by the main registry and verified against Shield's pinned key"} else {"project files captured for review; the main registry stays available for discovery"},
        "integration_setup":setup}), &project.to_string_lossy())?;
    prepared.commit(&|| Ok(())).map(|(_, receipt)| receipt)
}

/// Show the change once and, when the person accepts, write it to the policy
/// files. The files are checked again under the writer: an edit made while
/// the review was open is refused, so the person reviews against it.
pub(super) fn write_reviewed(
    context: &Context,
    before: &State,
    candidate: Value,
    meta: Value,
    tty: &mut Terminal,
    base: &configuration::Evaluator,
) -> Result<Value, Error> {
    let (mut view, plan) =
        publication::prepare_review(context, before, &candidate, &meta, Some(base))?;
    terminal_mcp_details(
        &mut view["changes"],
        &before.candidates["project"],
        &candidate,
    )?;
    let files = crate::review::files::write::base(context, before, &candidate, &meta)?;
    tty.confirm(&view)?;
    crate::review::files::write::write(
        context,
        crate::review::files::write::Edit {
            before,
            candidate,
            meta,
            base: files,
            evaluator: Some(&plan),
            reviewed: None,
        },
        None,
        "operator-cli",
    )
    .map_err(|error| match error {
        Error::StaleSnapshot => {
            Error::Invalid("the policy files changed during the review; run the command again")
        }
        error => error,
    })
}

/// Enrich only the ephemeral operator review, never proposals or receipts.
pub(super) fn terminal_mcp_details(
    changes: &mut Value,
    before: &Value,
    after: &Value,
) -> Result<(), Error> {
    let local = |change: &Value| change["kind"] == "mcp" && change["layer"] == "local";
    if !changes
        .as_array()
        .is_some_and(|rows| rows.iter().any(local))
    {
        return Ok(());
    }
    let before = configuration::layers::merged(before)?;
    let after = configuration::layers::merged(after)?;
    for change in changes
        .as_array_mut()
        .into_iter()
        .flatten()
        .filter(|row| local(row))
    {
        let id = change["id"]
            .as_str()
            .ok_or(Error::Invalid("MCP change identity missing"))?
            .to_owned();
        change["before"] = before["local_mcp"][&id].clone();
        change["after"] = after["local_mcp"][&id].clone();
    }
    Ok(())
}
