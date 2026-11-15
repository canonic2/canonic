//! Explicit fresh preparation of an unchanged declined offer; never issues authority.
use super::{Authority, Context, Error, Record, State, Value, event, registration, store, view};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    proposal_id: String,
    candidate_digest: String,
    idempotency_key: String,
}

fn declined<'a>(
    state: &'a State,
    context: &Context,
    input: &Input,
    authority: &Authority,
    now: u64,
) -> Result<&'a Record, Error> {
    let previous = state
        .session_grants
        .records
        .get(&input.proposal_id)
        .ok_or(Error::Missing)?;
    if previous.digest != input.candidate_digest {
        return Err(Error::Invalid(
            "candidate digest differs; inspect session_grant_show",
        ));
    }
    if previous.effective_with(authority, now) != "declined"
        || !previous.current_with(authority)
        || !state
            .session_grants
            .contexts
            .get(&previous.session_key)
            .is_some_and(|session| {
                session.reference == previous.session.reference
                    && authority.context_matches(&previous.session_key, session)
            })
    {
        return Err(Error::StaleSnapshot);
    }
    if context.surface != "cli"
        && (context.surface != "mcp"
            || context.agent.as_ref() != Some(&previous.session.agent)
            || context
                .session_id
                .as_ref()
                .is_some_and(|s| s != &previous.session.session_id))
    {
        return Err(Error::Missing);
    }
    Ok(previous)
}

/// The agent invokes this only at the person's explicit request. Arguments are
/// preparation intent; the existing approval form remains the consent boundary.
pub(crate) fn reconsider(context: &Context, args: &Value) -> Result<Value, Error> {
    let input: Input = serde_json::from_value(args.clone()).map_err(|_| {
        Error::Invalid(
            "select the declined proposal_id, candidate_digest and a fresh idempotency_key",
        )
    })?;
    if input.idempotency_key.is_empty() || input.idempotency_key.len() > 128 {
        return Err(Error::Invalid("invalid reconsideration idempotency key"));
    }
    let now = super::super::clock::now_ms()?;
    let scope = store(context)?.inspect_checkpoint(context, &|| Ok(()), |state| {
        let authority = Authority::capture(state);
        Ok(declined(state, context, &input, &authority, now)?
            .scope
            .clone())
    })?;
    let scope_checks = super::scope::check_git(context, &scope)?;
    store(context)?.update_checkpoint(context, &|| Ok(()), |state| {
        state.check_operations()?;
        state.session_grants.compact(now);
        let authority = Authority::capture(state);
        let previous = declined(state, context, &input, &authority, now)?;
        let owner = context.retry_owner();
        if let Some(row) = state.session_grants.records.values().find(|row| {
            row.reconsiders.as_deref() == Some(&input.proposal_id)
                && row.owner == owner
                && row.key == input.idempotency_key
        }) {
            return Ok(view::public(row, state, now));
        }
        // Explicit retries with another key cannot duplicate a live equivalent
        // review or renew an active/suspended grant.
        if let Some(row) = state.session_grants.records.values().find(|row| {
            row.session.reference == previous.session.reference
                && row.scope == previous.scope
                && row.duration == previous.duration
                && row.overrides == previous.overrides
                && row.current_with(&authority)
                && ["pending", "active", "suspended"].contains(&row.effective_with(&authority, now))
        }) {
            return Ok(view::public(row, state, now));
        }
        if state.session_grants.records.len() >= 1024
            || state
                .session_grants
                .records
                .values()
                .filter(|r| r.effective_with(&authority, now) == "pending")
                .count()
                >= 64
        {
            return Err(Error::Invalid(
                "capacity_exceeded: inspect session_grant_show before reconsidering more",
            ));
        }
        let mut row = previous.clone();
        row.reconsiders = Some(previous.id.clone());
        row.reconsiders_digest = Some(previous.digest.clone());
        row.session.registration = registration(state, &row.session.agent)?;
        row.id = state.id("session-proposal");
        row.owner = owner;
        row.key = input.idempotency_key;
        row.created = now;
        row.provenance = context.provenance();
        row.scope_checks.clone_from(&scope_checks);
        row.status = "pending".into();
        row.grant = None;
        row.terminal = None;
        row.terminal_reason = None;
        row.digest = row.digest();
        view::message(&row, &context.project_root, now)?;
        let result = view::public(&row, state, now);
        event(state, context, "session_grant_reconsidered", &row);
        state.session_grants.records.insert(row.id.clone(), row);
        Ok(result)
    })
}
