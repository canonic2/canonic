//! Durable web save-offer deduplication. These records never grant permission.
use super::{Context, Error, model::State, storage::Store};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;

const MAX_OFFERS: usize = 4096;
/// Offers are per session; a record outlives any ordinary session, then frees its slot.
const LIFETIME_MS: u64 = 30 * 24 * 60 * 60 * 1000;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Entry {
    /// Project, agent, native session and canonical site, without raw URLs.
    id: String,
    agent: String,
    proposal: Option<String>,
    prompted: bool,
    /// Records from builds without this field expire on the next refresh.
    #[serde(default)]
    created_at_ms: u64,
}

/// Drop records past their lifetime, as session drafts are reclaimed.
pub(in crate::review) fn reclaim(entries: &mut Vec<Entry>, now: u64) {
    entries.retain(|entry| now < entry.created_at_ms.saturating_add(LIFETIME_MS));
}

pub(crate) enum Offer {
    Pending {
        id: String,
        proposal: Option<String>,
    },
    Suppressed,
}

fn identity(project: &str, agent: &str, session: &str, change: &Value) -> Option<String> {
    let site = change["rule"]["conditions"]["url"]["hosts"][0].as_str()?;
    (!session.is_empty()).then(|| {
        crate::policy::fingerprint(
            serde_json::json!([project, agent, session, site])
                .to_string()
                .as_bytes(),
        )
    })
}

fn current(state: &State, id: &str) -> Option<Offer> {
    let entry = state.web_reviews.iter().find(|entry| entry.id == id)?;
    Some(if entry.prompted {
        Offer::Suppressed
    } else {
        Offer::Pending {
            id: id.into(),
            proposal: entry
                .proposal
                .as_ref()
                .filter(|id| {
                    state
                        .proposals
                        .iter()
                        .any(|proposal| proposal.id == **id && proposal.status == "pending")
                })
                .cloned(),
        }
    })
}

/// Register one stable review per site/session. Repeated hooks do not rewrite
/// state. Exhausted capacity suppresses additional offers, never permissions.
pub(crate) fn offer(
    root: &Path,
    agent: &str,
    session: &str,
    change: &Value,
) -> Result<Offer, Error> {
    let store = Store::open(root)?;
    let state = store.read_checkpoint()?;
    let Some(id) = identity(&state.project_id, agent, session, change) else {
        return Ok(Offer::Suppressed);
    };
    if let Some(offer) = current(&state, &id) {
        return Ok(offer);
    }
    let context = Context::new(state.project_id, state.project_root, "hook");
    let mut result = Offer::Suppressed;
    store.update_checkpoint_if_changed(&context, &|| Ok(()), |state| {
        if let Some(offer) = current(state, &id) {
            result = offer;
            return Ok(false);
        }
        if state.web_reviews.len() == MAX_OFFERS {
            return Ok(false);
        }
        state.web_reviews.push(Entry {
            id: id.clone(),
            agent: agent.into(),
            proposal: None,
            prompted: false,
            created_at_ms: super::clock::best_effort_ms(),
        });
        result = Offer::Pending { id, proposal: None };
        Ok(true)
    })?;
    Ok(result)
}

pub(crate) fn suppressed(root: &Path, agent: &str, session: &str, change: &Value) -> bool {
    Store::open(root)
        .and_then(|store| store.read_checkpoint())
        .is_ok_and(|state| {
            identity(&state.project_id, agent, session, change)
                .is_some_and(|id| matches!(current(&state, &id), Some(Offer::Suppressed)))
        })
}

/// Presented offers indexed once per pass: each lookup hashes one identity per
/// agent instead of one per stored record.
pub(in crate::review) struct Offered<'a> {
    project: &'a str,
    by_agent: std::collections::BTreeMap<&'a str, std::collections::HashSet<&'a str>>,
}

impl<'a> Offered<'a> {
    pub(in crate::review) fn new(state: &'a State, agent: Option<&str>) -> Self {
        let mut by_agent = std::collections::BTreeMap::<_, std::collections::HashSet<_>>::new();
        for entry in state
            .web_reviews
            .iter()
            .filter(|entry| entry.prompted && agent.is_none_or(|agent| agent == entry.agent))
        {
            by_agent
                .entry(entry.agent.as_str())
                .or_default()
                .insert(entry.id.as_str());
        }
        Self {
            project: &state.project_id,
            by_agent,
        }
    }

    pub(in crate::review) fn contains(&self, session: &str, change: &Value) -> bool {
        self.by_agent.iter().any(|(agent, ids)| {
            identity(self.project, agent, session, change)
                .is_some_and(|id| ids.contains(id.as_str()))
        })
    }
}

/// A second automatic proposal cannot replace a pending review or reopen a
/// presented one. Proposals without these IDs pass here, but at native
/// `policy_apply` [`link_native`] links any web allowance for an offered site,
/// so the agent cannot reopen it in that session; `shield rule add` can.
pub(in crate::review) fn check_proposal(
    state: &State,
    context: &Context,
    ids: &[String],
) -> Result<(), Error> {
    for entry in state
        .web_reviews
        .iter()
        .filter(|entry| ids.contains(&entry.id))
    {
        if context.agent.as_deref() != Some(&entry.agent) {
            return Err(Error::Invalid("web review belongs to another agent"));
        }
        if entry.prompted {
            return Err(Error::WebReviewOffered);
        }
        if let Some(id) = &entry.proposal
            && state
                .proposals
                .iter()
                .any(|p| p.id == *id && p.status == "pending")
        {
            return Err(Error::Invalid(
                "this site already has a pending proposal; inspect its outcome with policy_show",
            ));
        }
    }
    Ok(())
}

pub(in crate::review) fn attach(
    state: &mut State,
    context: &Context,
    ids: &[String],
    proposal: &str,
) -> Result<(), Error> {
    check_proposal(state, context, ids)?;
    for entry in state
        .web_reviews
        .iter_mut()
        .filter(|entry| ids.contains(&entry.id))
    {
        entry.proposal = Some(proposal.into());
    }
    Ok(())
}

/// Added or edited web allow rules among a proposal's public changes.
fn web_allowances(changes: &Value) -> impl Iterator<Item = &Value> {
    changes
        .as_array()
        .into_iter()
        .flatten()
        .filter(|change| {
            change["kind"] == "rule"
                && change["change"] != "removed"
                && change["rule"]["decision"] == "allow"
                && change["rule"]["conditions"]["domain"] == "web"
        })
        .map(|change| &change["rule"])
}

/// Whether a retained proposal can carry a save offer at all.
pub(in crate::review) fn concerns(proposal: &super::model::Proposal) -> bool {
    proposal.links["review_ids"]
        .as_array()
        .is_some_and(|ids| !ids.is_empty())
        || web_allowances(&proposal.apply_arguments()["changes"])
            .next()
            .is_some()
}

/// Native hook identity supplies the session even when the model omitted
/// `review_ids`. Only validated retained rule changes supply the sites.
pub(in crate::review) fn link_native(
    context: &Context,
    session: &str,
    proposal: &str,
    changes: &Value,
) -> Result<(), Error> {
    let Some(agent) = context.agent.as_deref() else {
        return Ok(());
    };
    let mut ids = std::collections::BTreeSet::new();
    for rule in web_allowances(changes) {
        let url = rule["conditions"]
            .get("url")
            .or_else(|| rule["requirements"].get("url"));
        for host in url
            .and_then(|url| url["hosts"].as_array())
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            if let Some(change) = crate::proposal_draft::web_fetch(&format!("https://{host}/"))
                && let Some(id) = identity(&context.project_id, agent, session, &change)
            {
                ids.insert(id);
            }
        }
    }
    if ids.is_empty() {
        return Ok(());
    }
    Store::open(Path::new(&context.project_root))?.update_checkpoint_if_changed(
        context,
        &|| Ok(()),
        |state| {
            let entries: Vec<_> = state
                .web_reviews
                .iter()
                .filter(|entry| ids.contains(&entry.id))
                .collect();
            if entries.iter().any(|entry| entry.prompted) {
                return Err(Error::WebReviewOffered);
            }
            let ids: Vec<String> = entries.iter().map(|entry| entry.id.clone()).collect();
            if ids.len() > 16 {
                return Err(Error::Invalid(
                    "split a web review covering more than 16 tracked sites",
                ));
            }
            let row = state
                .proposals
                .iter_mut()
                .find(|row| row.id == proposal)
                .ok_or(Error::Missing)?;
            let mut linked = row.links["review_ids"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            let mut updated = false;
            for id in &ids {
                if !linked.contains(&Value::String(id.clone())) {
                    linked.push(Value::String(id.clone()));
                    updated = true;
                }
            }
            row.links["review_ids"] = Value::Array(linked);
            for entry in state
                .web_reviews
                .iter_mut()
                .filter(|entry| ids.contains(&entry.id))
            {
                if entry.proposal.as_deref() != Some(proposal) {
                    entry.proposal = Some(proposal.into());
                    updated = true;
                }
            }
            Ok(updated)
        },
    )
}

/// Claim before returning a confirmation to the host, not after its answer.
/// Concurrent retries and lost answers cannot produce another save prompt.
pub(in crate::review) fn claim(context: &Context, proposal: &str) -> Result<(), Error> {
    let store = Store::open(Path::new(&context.project_root))?;
    let state = store.read_checkpoint()?;
    let row = state
        .proposals
        .iter()
        .find(|p| p.id == proposal)
        .ok_or(Error::Missing)?;
    if row.status == "applied" || !state.web_reviews.iter().any(|entry| linked(entry, row)) {
        return Ok(());
    }
    store.update_checkpoint_if_changed(context, &|| Ok(()), |state| {
        let row = state
            .proposals
            .iter()
            .find(|p| p.id == proposal)
            .ok_or(Error::Missing)?;
        if row.status == "applied" {
            return Ok(false);
        }
        if row.status != "pending" || !super::proposal_lifecycle::owns(row, context) {
            return Err(Error::Invalid(
                "this web save review is closed or belongs to another interaction",
            ));
        }
        let ids = row.links["review_ids"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let matches = |entry: &&Entry| ids.iter().any(|id| id == &entry.id);
        if state
            .web_reviews
            .iter()
            .filter(matches)
            .any(|entry| entry.prompted)
        {
            return Err(Error::WebReviewOffered);
        }
        let mut changed = false;
        for entry in state
            .web_reviews
            .iter_mut()
            .filter(|entry| ids.iter().any(|id| id == &entry.id))
        {
            entry.prompted = true;
            changed = true;
        }
        Ok(changed)
    })
}

pub(in crate::review) fn linked(entry: &Entry, proposal: &super::model::Proposal) -> bool {
    proposal.links["review_ids"]
        .as_array()
        .is_some_and(|ids| ids.iter().any(|id| id == &entry.id))
}

pub(in crate::review) fn validate(state: &State) -> Result<(), Error> {
    let mut ids = std::collections::BTreeSet::new();
    if state.web_reviews.len() > MAX_OFFERS
        || state.web_reviews.iter().any(|entry| {
            entry.id.len() != 64
                || !entry.id.bytes().all(|byte| byte.is_ascii_hexdigit())
                || !ids.insert(&entry.id)
                || crate::agents::find(&entry.agent).is_none()
                || entry
                    .proposal
                    .as_ref()
                    .is_some_and(|id| id.is_empty() || id.len() > 128)
        })
    {
        return Err(Error::Invalid("invalid web review records"));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
