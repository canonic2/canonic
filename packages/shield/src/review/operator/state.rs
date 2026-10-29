//! Terminal initialization and candidate confirmation for operator workflows.
use super::terminal::Terminal;
use crate::{
    configuration, policy,
    review::{
        self, Context, Error,
        model::State,
        project::{load, status},
        publication::{
            self,
            projections::{digest, file_hashes, materialize, projection_changed, projections},
        },
    },
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path};

pub(super) fn initialize(
    project: &Path,
    json_mode: bool,
    setup: &Value,
    cache_dir: Option<&Path>,
    baseline: Option<policy::Profile>,
) -> Result<Value, Error> {
    let mut tty = Terminal::required(json_mode)?;
    let project = project.canonicalize()?;
    if project.join(".shield/review/state.json").exists() {
        let context = review::open(&project)?;
        let state = load(&context)?;
        tty.confirm(
            &json!({"operation":"reconcile project","project":context.project_root,
            "policy_revision":state.policy_revision,"integration_setup":setup}),
        )?;
        return publication::reconcile(&context, &state);
    }
    let config = project.join(".shield/config.yml");
    let candidate = if config.exists() {
        super::project_files::capture(&config).map_err(|_| {
            Error::Invalid("invalid project configuration; review config.yml and local artifacts")
        })?
    } else {
        configuration::Bundle::capture(r#"{"schema_version":1,"packs":[]}"#, &[], None)?
    };
    let candidate = json!(candidate);
    let config = configuration::layers::merged(&candidate)?;
    let installed = project_file_rows(&config, &candidate)?;
    let mut meta = json!({"sources":{},"default":null,"installed":installed,"projection":null});
    let existing = json!({"installed":meta["installed"].clone()});
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
            .as_array()
            .ok_or(Error::Invalid("installed missing"))?,
        &bytes,
        None,
    )?;
    let (candidate, mut meta) = super::packs::settle_first(
        &mut tty,
        &project,
        &existing,
        candidate,
        meta,
        &super::packs::setup::Edits::default(),
    )?;
    let files = projections(&candidate, &meta)?;
    meta["projection"] = json!(digest(&files));
    meta["projection_file_hashes"] = json!(file_hashes(&files));
    let candidates = BTreeMap::from([("project".into(), candidate.clone())]);
    let evaluator = configuration::Evaluator::new(review::snapshots(&candidates)?)?;
    let mut setup = setup.clone();
    crate::review::integrations::preview_prefix_rules(
        &project,
        &mut setup,
        evaluator.codex_prefixes(),
    );
    tty.confirm(&json!({"operation":"initialize project","project":project,
        "policy":candidate,"registry_verification":if checkpoints.contains_key("main") {"signed by the main registry and verified against Shield's pinned key"} else {"project files captured for review; the main registry stays available for discovery"},
        "integration_setup":setup}))?;
    publication::initialize(
        &project,
        &candidates,
        evaluator.revision(),
        &meta,
        checkpoints,
    )?;
    let context = review::open(&project)?;
    let repair = materialize(&context, &files).is_err();
    let mut receipt = status(&context)?;
    receipt["committed"] = json!(true);
    receipt["repair_needed"] = json!(repair || receipt["repair_needed"] == true);
    Ok(receipt)
}

/// Installed rows for packs captured from existing project files. They are
/// written to the portable lock, so their source identity and location name
/// the project's own `.shield` folder, never this machine's path to it.
fn project_file_rows(config: &Value, candidate: &Value) -> Result<Vec<Value>, Error> {
    let source_identity = policy::fingerprint(b"shield-project-files");
    let rows = config["packs"]
        .as_array()
        .ok_or(Error::Invalid("packs missing"))?;
    Ok(rows.iter().enumerate().map(|(index, row)| {
        let digest = policy::fingerprint(candidate["packs"][index].as_str().unwrap_or_default().as_bytes());
        let seed = policy::fingerprint(format!("{source_identity}:{digest}:{index}").as_bytes());
        json!({"selector":format!("pack:{}-{}-{}-{}-{}",&seed[..8],&seed[8..12],&seed[12..16],&seed[16..20],&seed[20..32]),"id":row["id"],"version":row["version"],"profile":row["profile"],"digest":digest,"source_identity":source_identity,"source_alias":"project-files","source":{"identity":source_identity,"location":".shield","key":null},"pinned":true,"direct":true})
    }).collect())
}

pub(super) fn review_candidate(
    context: &Context,
    before: &State,
    candidate: &Value,
    meta: &Value,
    tty: &mut Terminal,
) -> Result<(), Error> {
    if projection_changed(context, before)? {
        return Err(Error::Invalid(
            "manual configuration changes are pending; review them through shield policy review",
        ));
    }
    let mut view = publication::change_review(context, before, candidate, meta)?;
    terminal_mcp_details(
        &mut view["changes"],
        &before.candidates["project"],
        candidate,
    )?;
    tty.confirm(&view)
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
