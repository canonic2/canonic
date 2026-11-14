//! Durable captures contain no live proposal or approval authority.
use super::{
    Change, Input, Prepared, compare_prefixes, historical, prepare_all, prepared_binding,
    validate_input,
};
use crate::{
    configuration,
    review::{Checkpoint, Context, Error, snapshots, storage::Store},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::review) struct Capture {
    pub(in crate::review) engine: String,
    pub(in crate::review) revision: String,
    pub(in crate::review) input: Value,
    pub(in crate::review) global_sources: Option<Value>,
    pub(in crate::review) prepared: Prepared,
}

pub(in crate::review) fn eligible(arguments: &Value) -> Result<bool, Error> {
    if arguments.get("preparation_task_id").is_some() {
        return Ok(false);
    }
    let input: Input = serde_json::from_value(arguments.clone())
        .map_err(|_| Error::Invalid("invalid preparation arguments"))?;
    validate_input(&input)?;
    input.changes()?;
    Ok(true)
}

pub(in crate::review) fn capture(
    context: &Context,
    arguments: &Value,
    checkpoint: Checkpoint<'_>,
) -> Result<Capture, Error> {
    if !eligible(arguments)? {
        return Err(Error::Invalid("this change requires synchronous review"));
    }
    let input: Input = serde_json::from_value(arguments.clone())
        .map_err(|_| Error::Invalid("invalid preparation arguments"))?;
    checkpoint()?;
    let store = Store::open(Path::new(&context.project_root))?;
    let before = store.read_following_files()?;
    before.check(context)?;
    before.check_operations()?;
    let (mut before, base, snapshots_before) =
        crate::review::storage::Store::read_for_comparison(before)?;
    let global_sources = if input.changes()?.iter().any(|change| {
        matches!(
            change,
            Change::PackAdd { .. }
                | Change::PackUpdate { .. }
                | Change::PackRemove { .. }
                | Change::PackConfigure { .. }
        )
    }) {
        Some(json!(super::super::registries::global()?))
    } else {
        None
    };
    let binding = prepared_binding(&before);
    context
        .progress
        .stage(super::super::progress::Stage::PreparePolicy);
    let (candidate, metadata, _) = prepare_all(
        context,
        &mut before,
        &input.changes()?,
        checkpoint,
        snapshots_before.first().map(|(_, snapshot)| snapshot),
    )?;
    historical::validate(&before.candidates["project"], &candidate)?;
    let next = snapshots(&BTreeMap::from([("project".into(), candidate.clone())]))?;
    context
        .progress
        .stage(super::super::progress::Stage::ComparePolicy);
    let mut comparison = json!(configuration::compare_policy(&snapshots_before, &next)?);
    let next = configuration::Evaluator::new(next)?;
    compare_prefixes(&base, &next, &mut comparison);
    if let Some(lines) = super::super::formatting_changes(&before.candidates["project"], &candidate)
    {
        comparison["formatting"] = lines;
    }
    checkpoint()?;
    if let Some(expected) = &global_sources
        && expected != &json!(super::super::registries::global()?)
    {
        return Err(Error::StaleSnapshot);
    }
    Ok(Capture {
        engine: env!("CARGO_PKG_VERSION").into(),
        revision: before.policy_revision,
        input: arguments.clone(),
        global_sources,
        prepared: Prepared {
            binding,
            candidate,
            metadata,
            comparison: Some(comparison),
        },
    })
}
