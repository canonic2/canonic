//! Closed native outcomes never replace the separate live follow-up capability.
use super::{Error, HookRequest, State};
use crate::review::authorization_receipts::{
    Capture, Claim, Decision, Granted, Kind, RETENTION_MS, Registry, Status,
};
use std::borrow::Cow;

fn capture(
    registry: &mut Registry,
    project: &str,
    row: &HookRequest,
    at: u64,
    denied: bool,
) -> Result<(), Error> {
    registry.capture(Capture {
        claim: Claim {
            kind: Kind::Hook,
            id: &row.request_id,
            project,
            agent: Some(&row.agent),
            session: Some(&row.session_id),
            consumer: "native-hook",
            binding: &row.binding,
        },
        decision: if denied {
            Decision::Deny
        } else {
            Decision::Approve
        },
        status: if denied {
            Status::Revoked
        } else {
            Status::Consumed
        },
        grant: row.grant.as_ref().map(|grant| Granted {
            id: &grant.approval_id,
            issued: grant.issued_at_ms,
            expires: grant.expires_at_ms,
        }),
        reason: None,
        at,
    })
}

fn consumed_at(row: &HookRequest, time: u64) -> Option<u64> {
    let at = (row.status == "consumed").then_some(row.grant.as_ref()?.consumed_at_ms?)?;
    (at <= time && time < at.saturating_add(RETENTION_MS)).then_some(at)
}

/// A safe closed view can coexist with a still-live, one-use follow-up. This
/// overlay never mutates that capability or persists a premature receipt.
pub(in crate::review) fn registry(state: &State, time: u64) -> Cow<'_, Registry> {
    let mut registry = Cow::Borrowed(&state.terminal_receipts);
    for row in &state.hook_requests {
        if let Some(at) = consumed_at(row, time) {
            // Unsupported historical metadata is unavailable, not an admission failure.
            let _ = capture(registry.to_mut(), &state.project_id, row, at, false);
        }
    }
    registry
}

pub(in crate::review) fn compact(state: &mut State, time: u64) {
    for row in &state.hook_requests {
        if row.live_id(time).is_none()
            && let Some(at) = consumed_at(row, time)
        {
            let _ = capture(
                &mut state.terminal_receipts,
                &state.project_id,
                row,
                at,
                false,
            );
        }
    }
    state.terminal_receipts.compact(&state.project_id, time);
}

pub(super) fn revoke(state: &mut State, index: usize, time: u64) -> Result<(), Error> {
    capture(
        &mut state.terminal_receipts,
        &state.project_id,
        &state.hook_requests[index],
        time,
        true,
    )
}
