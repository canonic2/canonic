//! Bounded display-only approval presentation captured while exact source is
//! already prepared. Actual application always regenerates from complete source.
use super::{Context, Error, Proposal, State, approval};
use crate::review::storage::{
    Store,
    objects::Pins,
    sources::{self, Namespace},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::Path;

const LIMIT: usize = 256 * 1024;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Presentation {
    schema_version: u8,
    engine: String,
    before: String,
    after: String,
    before_metadata: String,
    after_metadata: String,
    revision: String,
    changes: Value,
    review: Value,
    summary: String,
}

pub(in crate::review) fn capture(
    store: &Store,
    base: &State,
    candidate: &Value,
    metadata: &Value,
    comparison: &Value,
    reviewed_files: bool,
) -> Result<Option<(Value, Pins)>, Error> {
    let _span = crate::diagnostics::trace::span("proposal.capture_presentation");
    let before = &base.candidates["project"];
    let changes =
        crate::review::changes::diff::between(&base.operator, before, metadata, candidate);
    let review = approval::captured(before, candidate, comparison);
    let mut args = json!({"changes":changes,"review":review});
    super::redact(&mut args);
    let presentation = Presentation {
        schema_version: 1,
        engine: env!("CARGO_PKG_VERSION").into(),
        before: sources::candidate_identity(before),
        after: sources::candidate_identity(candidate),
        before_metadata: crate::json::fingerprint(&base.operator)
            .map_err(|_| Error::Invalid("presentation metadata digest"))?,
        after_metadata: crate::json::fingerprint(metadata)
            .map_err(|_| Error::Invalid("presentation metadata digest"))?,
        revision: base.policy_revision.clone(),
        summary: crate::review::mcp::captured_summary(candidate, reviewed_files, &args),
        changes: args["changes"].take(),
        review: args["review"].take(),
    };
    if !crate::json::fits(&presentation, LIMIT) {
        return Ok(None);
    }
    Namespace::project(store)
        .retain_json(&json!(presentation))
        .map(Some)
}

/// Missing/corrupt/future/unbound metadata supplies no apply arguments. This
/// never prepares source, activates policy or substitutes for actual validation.
pub(in crate::review) fn read(
    context: &Context,
    state: &State,
    row: &Proposal,
) -> Option<(Value, String)> {
    let reference = &row.presentation;
    if !crate::json::fits(reference, 1024)
        || reference["kind"] != "retained-json-v2"
        || reference["length"]
            .as_u64()
            .is_none_or(|length| length > LIMIT as u64)
    {
        return None;
    }
    let before = sources::candidate_identity(&row.base_candidate);
    let after = sources::candidate_identity(&row.candidate);
    if !state.authorization_events.iter().any(|event| {
        event["kind"] == "proposal_created"
            && event["links"]["proposal_id"] == row.id
            && event["details"]["presentation"] == *reference
            && event["details"]["before_candidate"] == before
            && event["details"]["after_candidate"] == after
            && event["details"]["policy_revision"] == row.policy_revision
    }) {
        return None;
    }
    let store = Store::inspect(Path::new(&context.project_root)).ok()?;
    let _pin = sources::pin_inspection(&store, reference).ok()?;
    let value = Namespace::project(&store).read_json(reference).ok()?;
    let presentation: Presentation = serde_json::from_value(value).ok()?;
    if presentation.schema_version != 1
        || presentation.engine != env!("CARGO_PKG_VERSION")
        || presentation.before != before
        || presentation.after != after
        || presentation.revision != row.policy_revision
        || presentation.before_metadata != crate::json::fingerprint(&row.base_metadata).ok()?
        || presentation.after_metadata != crate::json::fingerprint(&row.candidate_metadata).ok()?
    {
        return None;
    }
    let mut args = json!({"proposal_id":row.id,"candidate_digest":after,"changes":presentation.changes,"review":presentation.review});
    super::redact(&mut args);
    Some((args, presentation.summary))
}
