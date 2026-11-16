//! One human review across consumer kinds, retaining each consumer's authority checks.
use crate::review::{
    Context, Error, clock, hook_approval,
    model::State,
    project, read_approval, run_approval,
    runtime::consumer::{self, Snapshot},
    storage::Store,
};
use serde_json::{Value, json};
use std::{fmt::Write as _, path::Path};

#[derive(Clone, Copy)]
enum Kind {
    Read,
    Execution,
    Hook,
}

fn kind(state: &State, id: &str) -> Option<(Kind, Option<String>)> {
    if let Some(row) = state.read_requests.iter().find(|row| row.id() == id) {
        Some((Kind::Read, row.session_id().map(Into::into)))
    } else if let Some(row) = state.run_requests.iter().find(|row| row.id() == id) {
        Some((Kind::Execution, row.session_id().map(Into::into)))
    } else {
        state
            .hook_requests
            .iter()
            .find(|row| row.id() == id)
            .map(|row| (Kind::Hook, Some(row.session_id().into())))
    }
}

fn only_hooks(state: &State, ids: &[&str]) -> bool {
    ids.iter()
        .all(|id| matches!(kind(state, id), None | Some((Kind::Hook, _))))
}

struct Row {
    text: String,
    deny: bool,
    settings: bool,
    policy: bool,
}

fn compose(rows: &[Row]) -> String {
    if let [single] = rows {
        return single.text.clone();
    }
    let mut text = format!(
        "Allow these {} requests, each once? {} override a deny; {} change protected agent settings; {} may replace Shield's policy files. Review every request. No standing permission is created.\n",
        rows.len(),
        rows.iter().filter(|row| row.deny).count(),
        rows.iter().filter(|row| row.settings).count(),
        rows.iter().filter(|row| row.policy).count()
    );
    for (index, row) in rows.iter().enumerate() {
        let _ = write!(
            text,
            "\nRequest {} of {}:\n{}\n",
            index + 1,
            rows.len(),
            row.text
        );
    }
    text
}

fn question(
    state: &State,
    context: &Context,
    ids: &[&str],
    snapshot: &Snapshot,
) -> Result<String, Error> {
    let time = clock::now_ms()?;
    let mut session = None;
    let mut rows = Vec::with_capacity(ids.len());
    for (index, id) in ids.iter().enumerate() {
        let (kind, origin) = kind(state, id).ok_or_else(|| Error::Grouped {
            request: format!("request_ids[{index}]"),
            reason: "no such request is pending",
            recovery: "Call request_approve again without it",
        })?;
        if index > 0 && (origin.is_none() || session != origin) {
            return Err(Error::Grouped {
                request: format!("request_ids[{index}]"),
                reason: "it belongs to another or unidentified session",
                recovery: "Approve each request separately with request_id, or group only requests from one identified session",
            });
        }
        session = origin;
        let row = match kind {
            Kind::Hook => {
                let (text, deny, settings, policy) = hook_approval::group_row(state, context, id)
                    .map_err(|error| match error {
                    Error::AgentNotConnected if ids.len() > 1 => {
                        hook_approval::not_connected_in_group(id)
                    }
                    error => error,
                })?;
                Row {
                    text,
                    deny,
                    settings,
                    policy,
                }
            }
            Kind::Read | Kind::Execution => Row {
                text: if matches!(kind, Kind::Read) {
                    read_approval::review_in(state, context, id, snapshot, time)?
                } else {
                    run_approval::review_in(state, context, id, snapshot, time)?
                },
                deny: false,
                settings: false,
                policy: false,
            },
        };
        if row.text.len() > hook_approval::MAX_FORM {
            return Err(Error::Grouped {
                request: format!("request_ids[{index}]"),
                reason: "its complete review exceeds the human form limit",
                recovery: "Continue without this unsupported request; review the remaining requests separately",
            });
        }
        rows.push(row);
    }
    let text = compose(&rows);
    // One row passed the form limit above, so only a group can exceed it.
    if text.len() <= hook_approval::MAX_FORM {
        return Ok(text);
    }
    let mut split = rows.len() - 1;
    while split > 1 && compose(&rows[..split]).len() > hook_approval::MAX_FORM {
        split -= 1;
    }
    Err(Error::ApprovalFormTooLong {
        retry: ids[..split].iter().map(|s| (*s).into()).collect(),
        remaining: ids[split..].iter().map(|s| (*s).into()).collect(),
    })
}

pub(crate) fn confirmation(context: &Context, args: &Value) -> Result<String, Error> {
    let (ids, list) = hook_approval::requested(args)?;
    let state = project::authorization(context)?;
    if terminal(&state, context, &ids, list)?.is_some() {
        return Err(Error::Invalid("request is terminal"));
    }
    if only_hooks(&state, &ids) {
        return hook_approval::confirmation_in(&state, context, args);
    }
    let snapshot = consumer::load(context, &|| Ok(()))?;
    question(&state, context, &ids, &snapshot)
}

pub(crate) fn approve(context: &Context, args: &Value, confirmed: bool) -> Result<Value, Error> {
    let (ids, list) = hook_approval::requested(args)?;
    let before = project::authorization(context)?;
    if let Some(value) = terminal(&before, context, &ids, list)? {
        return Ok(value);
    }
    if only_hooks(&before, &ids) {
        return hook_approval::approve(context, args, confirmed);
    }
    if !confirmed {
        confirmation(context, args)?;
        return Err(Error::ApprovalRequired);
    }
    let snapshot = consumer::load(context, &|| Ok(()))?;
    Store::open(Path::new(&context.project_root))?.update_checkpoint(context, &|| Ok(()), |state| {
        if let Some(value)=terminal(state,context,&ids,list)? {return Ok(value);}
        question(state, context, &ids, &snapshot)?;
        let time = clock::now_ms()?;
        let mut grants = Vec::with_capacity(ids.len());
        for id in &ids {
            let value = match kind(state, id).ok_or(Error::Missing)?.0 {
                Kind::Read => read_approval::approve_in(state, context, id, &snapshot, time)?,
                Kind::Execution => run_approval::approve_in(state, context, id, &snapshot, time)?,
                Kind::Hook => hook_approval::approve_in(state, context, &json!({"request_id":id}))?,
            };
            grants.push(value);
        }
        let next = "Follow each grant's next_action and retry once through its original consumer. Checked commands require --approval-request with that grant's request_id. Approval does not execute requests.";
        if list {
            Ok(json!({"status":"approved","grants":grants,"next_action":next}))
        } else {
            let mut value = grants.remove(0);
            if value.get("next_action").is_none() {
                value["next_action"] = json!("Retry this identical read through the same factory and session, with the same window. Approval does not execute it.");
            }
            Ok(value)
        }
    })
}

fn terminal(
    state: &State,
    context: &Context,
    ids: &[&str],
    list: bool,
) -> Result<Option<Value>, Error> {
    state.check_operations()?;
    Ok(hook_approval::terminal_registry(state, clock::now_ms()?)
        .replay_approval_group(context, ids, clock::now_ms()?)?
        .map(|mut outcomes| {
            if list {
                json!({"status":"terminal","grants":outcomes,"replayed":true,"allowed":false})
            } else {
                outcomes.remove(0)
            }
        }))
}
