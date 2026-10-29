//! Bounded session review evidence, recurrence and non-authorizing deferral.
use super::{Context, Error};
use crate::{
    proposal_draft,
    review::{observations, storage::Store},
};
use serde_json::{Value, json};
use std::path::Path;
const MAX_REQUESTS: usize = 256;
/// Review deferrals a project retains; deferrals survive reconnects.
pub(super) const MAX_DEFERRALS: usize = 1024;

pub(super) fn list(context: &Context, session: Option<&str>) -> Value {
    let root = Path::new(&context.project_root);
    let roots = crate::agents::machine_roots(root);
    let collected = observations::asked_requests(root, session, MAX_REQUESTS);
    let mut rows: Vec<Value> = Vec::new();
    let mut capped = false;
    for request in &collected.requests {
        let guidance = &request.explanation["guidance"];
        let mut counted = Vec::new();
        for item in guidance.as_array().into_iter().flatten() {
            if let Some(exact) = proposal_draft::decisive_command(item) {
                let conditions = &exact["rule"]["conditions"];
                let observed = observed(conditions);
                let words: Vec<_> =
                    std::iter::once(conditions["executable"].as_str().unwrap_or_default())
                        .chain(
                            conditions["arguments"]["values"]
                                .as_array()
                                .into_iter()
                                .flatten()
                                .filter_map(Value::as_str),
                        )
                        .collect();
                let web = crate::http_client::fetch(words[0], &words[1..])
                    .and_then(|fetch| proposal_draft::web_fetch(fetch.url));
                let identity = json!([request.session, web.as_ref().unwrap_or(&observed)]);
                if counted.contains(&identity) {
                    continue;
                }
                counted.push(identity.clone());
                let mut row = json!({"command":words.join(" "),"reason":item["message"],
                    "request":observed,"decided_as":if web.is_some(){"web_fetch"}else{"command"},
                    "pack_requests_rule":proposal_draft::invited(guidance, &item["operation_ref"])});
                if let Some(web) = web {
                    row["suggested_change"] = web;
                }
                let portable = if row.get("suggested_change").is_some() {
                    crate::policy::portability::check(&row["suggested_change"]["rule"], &roots)
                } else {
                    crate::policy::portability::check_request(&row["request"], &roots)
                };
                if let Err(reason) = portable {
                    row["not_portable"] = json!(reason);
                }
                capped |= !push(&mut rows, request, &identity, row, context);
            }
        }
        if counted.is_empty() || request.partial {
            let identity = json!([
                request.session,
                request.operation,
                request.explanation["cwd"]
            ]);
            let source = request.operation["source"]
                .as_str()
                .unwrap_or("Unresolved recorded request");
            let row = json!({"command":source.chars().take(256).collect::<String>(),
                "reason":"Command analysis did not provide complete reviewable arguments. Inspect the retained request; do not infer a safe rule.",
                "source_truncated":source.chars().count() > 256,"decided_as":"unresolved","pack_requests_rule":false});
            capped |= !push(&mut rows, request, &identity, row, context);
        }
    }
    let state = Store::open(root).and_then(|store| store.read_checkpoint());
    annotate(&mut rows, &state, session);
    json!({"session_id":session,"requests":rows,
        "coverage":{"complete":collected.complete && !capped && state.is_ok(),"available":collected.available && state.is_ok(),
            "scope":"retained history since the latest policy activation","collection_limit":MAX_REQUESTS,
            "reason":if !collected.available || state.is_err(){"history or review state unavailable"} else if capped {"review item limit reached"} else if !collected.complete {"history scan or retained request limit reached"} else {"complete retained window"}},
        "next_action":"Review eligible groups after the task. Listing or deferral grants no permission. Incomplete evidence is not an empty successful review.",
        "authoring_examples":super::schemas::proposal_examples(),
        "approval_guidance":crate::agents::review::resolve(context.agent.as_deref()).guidance().ask})
}

fn annotate(
    rows: &mut [Value],
    state: &Result<crate::review::model::State, Error>,
    session: Option<&str>,
) {
    for row in rows {
        let ids = row
            .as_object_mut()
            .and_then(|row| row.remove("invocations"))
            .unwrap_or_default();
        row["intentional_invocations"] = json!(ids.as_array().map_or(0, Vec::len));
        let dismissed = state.as_ref().is_ok_and(|state| {
            state
                .dismissed_reviews
                .iter()
                .any(|id| row["review_id"] == *id)
        });
        let proposal = state.as_ref().ok().and_then(|state| {
            state.proposals.iter().find(|proposal| {
                proposal.links["review_ids"]
                    .as_array()
                    .is_some_and(|ids| ids.contains(&row["review_id"]))
            })
        });
        let recurrent = row["recurrence_known"] == true
            && row["intentional_invocations"].as_u64().unwrap_or(0) >= 2;
        row["eligible_for_offer"] = json!(
            recurrent
                && state.is_ok()
                && session.is_some()
                && !dismissed
                && proposal.is_none()
                && row["session_id"].is_string()
                && row["decided_as"] != "unresolved"
                && row.get("not_portable").is_none()
        );
        row["review_status"] = json!(if dismissed {
            "deferred"
        } else if proposal.is_some() {
            "proposed"
        } else if row["decided_as"] == "unresolved" {
            "unsupported"
        } else {
            "needs_inspection"
        });
        row["next_action"] = if let Some(proposal) = proposal {
            json!({"tool":"policy_show","arguments":{"proposal_id":proposal.id},
                "instruction":"Inspect the existing outcome if needed. Do not repeat an unchanged offer or retry declined/cancelled approval."})
        } else if dismissed {
            json!({"instruction":"Continue the task. This review is deferred for this session."})
        } else if row["decided_as"] == "unresolved" {
            json!({"tool":"history_list","arguments":{"entry_id":row["source_entry_id"]},
                "instruction":"Inspect the recorded source and effects. Partial analysis is evidence, not an executable rule draft."})
        } else {
            json!({"instruction":crate::proposal_draft::command_offer(None),
                "proposal_context":{"source_entry_id":row["source_entry_id"],"review_ids":[row["review_id"]]},
                "defer":{"tool":"review_defer","arguments":{"session_id":row["session_id"],"review_ids":[row["review_id"]]}}})
        };
    }
}

fn push(
    rows: &mut Vec<Value>,
    request: &observations::AskedRequest,
    identity: &Value,
    mut row: Value,
    context: &Context,
) -> bool {
    let id =
        crate::policy::fingerprint(json!([context.project_id, identity]).to_string().as_bytes());
    if let Some(known) = rows.iter_mut().find(|known| known["review_id"] == id) {
        known["times_asked"] =
            json!(known["times_asked"].as_u64().unwrap_or(0) + request.count as u64);
        let mut ids: std::collections::BTreeSet<String> =
            serde_json::from_value(known["invocations"].clone()).unwrap_or_default();
        ids.extend(request.invocations.iter().cloned());
        known["invocations"] = json!(ids);
        known["recurrence_known"] =
            json!(known["recurrence_known"] == true && request.recurrence_known);
        return true;
    }
    if rows.len() == MAX_REQUESTS {
        return false;
    }
    row["review_id"] = json!(id);
    row["session_id"] = json!(request.session);
    row["times_asked"] = json!(request.count);
    row["source_entry_id"] = json!(request.entry);
    row["invocations"] = json!(request.invocations);
    row["recurrence_known"] = json!(request.recurrence_known);
    rows.push(row);
    true
}

/// Advisory deferral changes no policy and survives reconnect/history clearing.
pub(super) fn dismiss(
    context: &Context,
    session: Option<&str>,
    ids: &Value,
    checkpoint: super::Checkpoint<'_>,
) -> Result<(), Error> {
    let ids = ids
        .as_array()
        .filter(|ids| !ids.is_empty() && ids.len() <= 16)
        .ok_or(Error::Invalid("dismiss requires 1-16 review IDs"))?;
    let current = list(context, session);
    if ids.iter().any(|id| {
        !current["requests"]
            .as_array()
            .is_some_and(|rows| rows.iter().any(|row| row["review_id"] == *id))
    }) {
        return Err(Error::Invalid(
            "review ID is unavailable in this session; list current reviews",
        ));
    }
    Store::open(Path::new(&context.project_root))?.update_checkpoint(
        context,
        checkpoint,
        |state| {
            for id in ids {
                let id = id.as_str().ok_or(Error::Invalid("invalid review ID"))?;
                if !state.dismissed_reviews.iter().any(|known| known == id) {
                    if state.dismissed_reviews.len() >= MAX_DEFERRALS {
                        return Err(Error::Overloaded);
                    }
                    state.dismissed_reviews.push(id.into());
                }
            }
            Ok(json!({}))
        },
    )?;
    Ok(())
}

fn observed(conditions: &Value) -> Value {
    let mut request = json!({"executable":conditions["executable"],"arguments":conditions["arguments"]["values"]});
    if let Some(cwd) = conditions.get("cwd") {
        request["cwd"] = cwd.clone();
    }
    request
}
