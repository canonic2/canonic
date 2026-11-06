//! Explicitly deferred intent; drafts contain no candidate or approval binding.
use super::{Context, Error, Store, close};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::Path;

const LIFETIME_MS: u64 = 30 * 24 * 60 * 60 * 1000;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::review) struct Draft {
    id: String,
    created_at: String,
    intent: String,
    constraints: Option<String>,
    provenance: Value,
    source_proposal_id: Option<String>,
    retry_key: String,
}

impl Draft {
    fn owns(&self, context: &Context) -> bool {
        let current = context.provenance();
        self.provenance["agent"] == current["agent"]
            && self.provenance["surface"] == current["surface"]
            && self
                .provenance
                .get("session_id")
                .filter(|v| !v.is_null())
                .or_else(|| self.provenance.get("connection_id"))
                == current
                    .get("session_id")
                    .filter(|v| !v.is_null())
                    .or_else(|| current.get("connection_id"))
    }
}

pub(super) fn reclaim(drafts: &mut Vec<Draft>, now: u64) {
    use canonic_registry_kit::timestamp::{parse, unix_seconds};
    drafts.retain(|draft| {
        parse(&draft.created_at)
            .and_then(|parts| u64::try_from(unix_seconds(parts)).ok())
            .and_then(|seconds| seconds.checked_mul(1000))
            .is_some_and(|created| now < created.saturating_add(LIFETIME_MS))
    });
}

pub(in crate::review) fn save(context: &Context, args: &Value) -> Result<Value, Error> {
    let intent = args["intent"]
        .as_str()
        .filter(|s| !s.trim().is_empty() && s.len() <= 4096)
        .ok_or(Error::Invalid(
            "supply the explicitly deferred intent, within 4096 bytes",
        ))?;
    let constraints = args
        .get("constraints")
        .map(|v| {
            v.as_str().filter(|s| s.len() <= 4096).ok_or(Error::Invalid(
                "draft constraints must be text within 4096 bytes",
            ))
        })
        .transpose()?;
    let key = args["idempotency_key"]
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= 128 && s.is_ascii() && !s.contains('\0'))
        .ok_or(Error::Invalid("supply a draft idempotency_key"))?;
    let source = args
        .get("proposal_id")
        .map(|v| {
            v.as_str()
                .filter(|s| !s.is_empty() && s.len() <= 128)
                .ok_or(Error::Invalid("invalid proposal_id"))
        })
        .transpose()?;
    Store::open(Path::new(&context.project_root))?.update_checkpoint(context, &|| Ok(()), |state| {
        if let Some(draft) = state
            .policy_drafts
            .iter()
            .find(|d| d.retry_key == key && d.owns(context))
        {
            if draft.intent != intent
                || draft.constraints.as_deref() != constraints
                || draft.source_proposal_id.as_deref() != source
            {
                return Err(Error::Invalid("draft idempotency key reused with changed intent"));
            }
            return Ok(json!({"draft":draft,"next_action":"Intent only. To resume, inspect current policy and prepare a fresh change for immediate approval."}));
        }
        if state.policy_drafts.len() >= 128 {
            return Err(Error::Invalid(
                "draft capacity reached; remove one of this session's drafts with policy_draft_remove or wait for expiry",
            ));
        }
        if let Some(id) = source {
            let proposal = state.proposals.iter_mut().find(|p| p.id == id).ok_or(Error::Missing)?;
            if !super::owns(proposal, context) {
                return Err(Error::Invalid("cannot defer another interaction's candidate"));
            }
            close(proposal, "withdrawn", "explicitly_deferred");
        }
        let draft = Draft {
            id: state.id("draft"),
            created_at: crate::review::model::now(),
            intent: intent.into(),
            constraints: constraints.map(str::to_owned),
            provenance: context.provenance(),
            source_proposal_id: source.map(str::to_owned),
            retry_key: key.into(),
        };
        let result = json!({"draft":draft,"next_action":"Intent only. To resume, inspect current policy and prepare a fresh change for immediate approval."});
        state.policy_drafts.push(draft);
        Ok(result)
    })
}

pub(in crate::review) fn list(context: &Context, id: Option<&str>) -> Result<Value, Error> {
    let state = Store::open(Path::new(&context.project_root))?.read_checkpoint()?;
    state.check(context)?;
    if let Some(id) = id {
        let draft = state
            .policy_drafts
            .iter()
            .find(|d| d.id == id)
            .ok_or(Error::Missing)?;
        return Ok(
            json!({"draft":draft,"next_action":"Inspect current policy and prepare fresh from this intent. This draft cannot be applied."}),
        );
    }
    Ok(json!({"drafts":state.policy_drafts}))
}

pub(in crate::review) fn remove(context: &Context, id: &str) -> Result<Value, Error> {
    Store::open(Path::new(&context.project_root))?.update_checkpoint(context, &|| Ok(()), |state| {
        if let Some(draft) = state.policy_drafts.iter().find(|d| d.id == id)
            && !draft.owns(context)
        {
            return Err(Error::Invalid("cannot remove another session's draft"));
        }
        state.policy_drafts.retain(|d| d.id != id);
        Ok(json!({"draft_id":id,"removed":true}))
    })
}
