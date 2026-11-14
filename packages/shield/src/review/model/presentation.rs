//! Bounded display-only approval presentation captured while the proposal is
//! already prepared, kept inline in the proposal. Application always checks
//! the proposal's own files; a missing or mismatched presentation is rebuilt.
use super::{Context, Error, Proposal, State, approval};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// The largest presentation kept inline; a larger one is rebuilt on demand.
const LIMIT: usize = 32 * 1024;
const KIND: &str = "inline-presentation-v1";

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

/// The inline presentation to store with a proposal, or `None` when it is too
/// large to keep. Its digest goes in the proposal's creation event.
pub(in crate::review) fn capture(
    base: &State,
    before: &Value,
    candidate: &Value,
    metadata: &Value,
    comparison: &Value,
    reviewed_files: bool,
) -> Result<Option<Value>, Error> {
    let _span = crate::diagnostics::trace::span("proposal.capture_presentation");
    let changes =
        crate::review::changes::diff::between(&base.operator, before, metadata, candidate);
    let review = approval::captured(before, candidate, comparison);
    let mut args = json!({"changes":changes,"review":review});
    super::redact(&mut args);
    let presentation = Presentation {
        schema_version: 1,
        engine: env!("CARGO_PKG_VERSION").into(),
        before: crate::review::candidate_identity(before),
        after: crate::review::candidate_identity(candidate),
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
    let value = json!(presentation);
    let digest =
        crate::json::fingerprint(&value).map_err(|_| Error::Invalid("presentation digest"))?;
    Ok(Some(json!({"kind":KIND,"digest":digest,"value":value})))
}

/// The digest a proposal's creation event binds its presentation by.
pub(in crate::review) fn digest(presentation: &Value) -> Value {
    presentation["digest"].clone()
}

/// Missing/corrupt/future/unbound presentations supply no apply arguments.
/// This never prepares policy or substitutes for actual validation.
pub(in crate::review) fn read(
    _context: &Context,
    state: &State,
    row: &Proposal,
) -> Option<(Value, String)> {
    let stored = &row.presentation;
    if stored["kind"] != KIND || !crate::json::fits(&stored["value"], LIMIT) {
        return None;
    }
    let digest = crate::json::fingerprint(&stored["value"]).ok()?;
    if stored["digest"] != digest.as_str() {
        return None;
    }
    let before = &row.base_digest;
    let after = &row.candidate_digest;
    if !state.authorization_events.iter().any(|event| {
        event["kind"] == "proposal_created"
            && event["links"]["proposal_id"] == row.id
            && event["details"]["presentation"] == digest.as_str()
            && event["details"]["before_candidate"] == before.as_str()
            && event["details"]["after_candidate"] == after.as_str()
            && event["details"]["policy_revision"] == row.policy_revision
    }) {
        return None;
    }
    let presentation: Presentation = serde_json::from_value(stored["value"].clone()).ok()?;
    if presentation.schema_version != 1
        || presentation.engine != env!("CARGO_PKG_VERSION")
        || presentation.before != *before
        || presentation.after != *after
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
