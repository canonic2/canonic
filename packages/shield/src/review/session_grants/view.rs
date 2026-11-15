//! Complete bounded human reviews and read-only session-grant inspection.
use super::{Context, Error, MAX_FORM, PENDING, Record, State, Value, json, store};

pub(super) fn apply_fields(args: &Value) -> Result<(&str, &str), Error> {
    if args.as_object().is_none_or(|o| o.len() != 2) {
        return Err(Error::Invalid(
            "copy session grant apply_arguments unchanged",
        ));
    }
    Ok((
        args["proposal_id"]
            .as_str()
            .ok_or(Error::Invalid("proposal_id required"))?,
        args["candidate_digest"]
            .as_str()
            .ok_or(Error::Invalid("candidate_digest required"))?,
    ))
}

fn timestamp(ms: u64) -> String {
    crate::evidence::format_utc(ms / 1000)
}

pub(super) fn public(row: &Record, state: &State, now: u64) -> Value {
    let status = row.effective(state, now);
    let grant = row.grant.as_ref();
    json!({"proposal_id":row.id,"reconsiders_proposal_id":row.reconsiders,"reconsiders_candidate_digest":row.reconsiders_digest,"grant_id":grant.map(|g|&g.id),"session_ref":row.session.reference,
        "target":{"project":state.project_root,"agent":row.session.agent,"session_id":row.session.session_id},
        "candidate_digest":row.digest,"scope":row.scope,"override_denies":row.overrides,"duration_seconds":row.duration,
        "provenance":row.provenance,"created_at_ms":row.created,"proposal_expires_at_ms":row.created.saturating_add(PENDING),
        "proposal_status":if grant.is_some(){"issued"}else{status},"grant_status":grant.map(|_|status),
        "issued_at_ms":grant.map(|g|g.issued),"expires_at_ms":grant.map(|g|g.expires),
        "created_at":timestamp(row.created),"proposal_expires_at":timestamp(row.created.saturating_add(PENDING)),
        "issued_at":grant.map(|g|timestamp(g.issued)),"expires_at":grant.map(|g|timestamp(g.expires)),
        "proposal_terminal_at":grant.map(|g|g.issued).or(row.terminal).map(timestamp),
        "proposal_terminal_reason":if grant.is_some(){Some("issued")}else{row.terminal_reason.as_deref()},
        "grant_terminal_at":grant.and(row.terminal).map(timestamp),"grant_terminal_reason":grant.and(row.terminal_reason.as_deref()),
        "terminal_at_ms":row.terminal,"receipt":grant.map(|g|json!({"approval_id":g.approval,"mechanism":g.mechanism,"candidate_digest":row.digest})),
        "session_binding":"host-reported","mcp_session_association":if row.provenance["session_id"].is_null(){"unavailable"}else{"host-reported"},
        "review":message(row,&state.project_root,now).ok(),
        "apply_arguments":{"proposal_id":row.id,"candidate_digest":row.digest},
        "next_action":if status=="pending"{"Call session_grant_approve with apply_arguments unchanged; the user confirms Shield's form."}else if status=="active"{"Use covered native commands; project policy is unchanged."}else if status=="declined"{"Do not offer this scope again automatically. Only if the person explicitly asks to reconsider, call session_grant_reconsider with this proposal_id, candidate_digest and a fresh idempotency_key; then obtain fresh human confirmation."}else{"This authorization is unavailable. Inspect current policy and obtain fresh confirmation only if still needed."}})
}

pub(super) fn message(row: &Record, project_root: &str, now: u64) -> Result<String, Error> {
    let mut lines=vec!["Allow repeated operations for one conversation?".into(),
        format!("Project: {}",quote(project_root)),
        format!("Agent: {}; recorded conversation: {}",quote(&row.session.agent),quote(&row.session.session_id)),
        "MCP conversation association may be unavailable. This approval targets the recorded conversation shown above.".into(),
        format!("Duration: {} seconds from acceptance; proposed expiry: {}.",row.duration,timestamp(now.saturating_add(row.duration*1000))),
        "Unlimited matching uses until expiry, revoke or a policy/protection/integration change. Resume of this same conversation can reuse it.".into(),
        "Project policy stays unchanged. Other conversations receive no permission. This does not prove review, passing checks or execution.".into()];
    if let Some(previous) = &row.reconsiders {
        // Replace boilerplate rather than adding a paragraph: a scope that fit
        // the original form must still fit when explicitly reconsidered.
        lines[0] = format!("Fresh review of declined proposal {}?", quote(previous));
        lines[3] = "MCP association may be unavailable; this review targets the recorded conversation above.".into();
        lines[6] = "Policy unchanged. Other conversations get no permission. Permission proves no review, checks or execution.".into();
    }
    if row.overrides.as_array().is_none_or(Vec::is_empty) {
        lines.push("Covers Ask only; Deny and protected-state checks remain effective.".into());
    } else {
        lines.push("OVERRIDES DENY: this temporarily overrides the following current policy Deny rules within the scope below.".into());
        for rule in row.override_evidence.as_array().into_iter().flatten() {
            lines.push(format!(
                "Deny rule {} from {}. Reason: {}. Body digest: {}.",
                quote(&rule["rule_ref"].to_string()),
                quote(rule["source"].as_str().unwrap_or_default()),
                quote(&rule["reason"].to_string()),
                quote(rule["rule_digest"].as_str().unwrap_or_default())
            ));
        }
    }
    for (i, selector) in row.scope.as_array().into_iter().flatten().enumerate() {
        lines.push(format!("Alternative {} (every constraint applies):", i + 1));
        describe("", selector, &mut lines);
    }
    if let Some(reason) = &row.reason {
        lines.push(format!("Agent's reason (untrusted): {}", quote(reason)));
    }
    lines.push("Revoke with session_grant_revoke or shield session-grant revoke ID. Rejecting issues nothing.".into());
    let message = lines.join("\n\n");
    if message.len() > MAX_FORM {
        return Err(Error::Invalid(
            "unsupported_scope: review exceeds 12000 bytes; split the scope without broadening it",
        ));
    }
    Ok(message)
}

fn quote(text: &str) -> String {
    let text=text.chars().map(|c|if matches!(c,'\u{061c}'|'\u{200e}'|'\u{200f}'|'\u{202a}'..='\u{202e}'|'\u{2066}'..='\u{2069}') {c.escape_unicode().to_string()}else{c.to_string()}).collect::<String>();
    serde_json::to_string(&text).unwrap_or_default()
}

// Render every predicate leaf without abbreviation. The path preserves all
// conjunctions and array positions while labels give operators readable text.
fn describe(path: &str, value: &Value, lines: &mut Vec<String>) {
    match value {
        Value::Object(fields) if !fields.is_empty() => {
            for (key, value) in fields {
                let label = key.replace('_', " ");
                describe(
                    &if path.is_empty() {
                        label
                    } else {
                        format!("{path} / {label}")
                    },
                    value,
                    lines,
                );
            }
        }
        Value::Array(items) if !items.is_empty() => {
            for (i, value) in items.iter().enumerate() {
                describe(&format!("{path} [{}]", i + 1), value, lines);
            }
        }
        Value::String(text) => lines.push(format!("{path}: {}", quote(text))),
        _ => lines.push(format!("{path}: {value}")),
    }
}

pub(crate) fn confirmation(context: &Context, args: &Value) -> Result<Option<String>, Error> {
    let (id, digest) = apply_fields(args)?;
    let now = super::super::clock::now_ms()?;
    store(context)?.inspect_checkpoint(context, &|| Ok(()), |state| {
        state.check_operations()?;
        let row = state.session_grants.records.get(id).ok_or(Error::Missing)?;
        if row.digest != digest {
            return Err(Error::Invalid("candidate digest differs"));
        }
        if row.status == "issued" {
            return Ok(None);
        }
        if row.effective(state, now) != "pending" {
            return Err(Error::StaleSnapshot);
        }
        message(row, &context.project_root, now).map(Some)
    })
}

pub(crate) fn show(context: &Context, args: &Value) -> Result<Value, Error> {
    let args = args
        .as_object()
        .ok_or(Error::Invalid("invalid session grant selection"))?;
    if args.keys().any(|k| {
        !["session_ref", "proposal_id", "grant_id", "cursor", "limit"].contains(&k.as_str())
    }) || ["session_ref", "proposal_id", "grant_id"]
        .iter()
        .filter(|k| args.contains_key(**k))
        .count()
        > 1
    {
        return Err(Error::Invalid(
            "select one session_ref, proposal_id or grant_id",
        ));
    }
    let now = super::super::clock::now_ms()?;
    store(context)?.inspect_checkpoint(context,&||Ok(()),|state| {
        if let Some(id)=args.get("proposal_id").or_else(||args.get("grant_id")) {
            if args.len()!=1 || !id.is_string() {return Err(Error::Invalid("ID selection cannot include pagination"));}
            let row=state.session_grants.records.values().find(|r|json!(r.id)==*id || r.grant.as_ref().is_some_and(|g|json!(g.id)==*id)).ok_or(Error::Missing)?;
            return Ok(public(row,state,now));
        }
        let selection=args.get("session_ref").and_then(Value::as_str);
        if args.get("session_ref").is_some() && selection.is_none() {return Err(Error::Invalid("session_ref must be a string"));}
        let mut query=Value::Object(args.clone());
        query["project_id"]=json!(state.project_id);
        let rows:Vec<_>=state.session_grants.records.values().filter(|r|selection.is_none_or(|s|r.session.reference==s)).collect();
        let total=rows.len();
        let request=canonic_mcp_kit::page::Request::from_mcp(&query,canonic_mcp_kit::page::Limits::STANDARD).map_err(crate::review::page::error)?;
        let scope=crate::review::page::scope("session_grant_list",&query);
        let after=crate::review::page::after(&scope,&request)?;
        let page=crate::review::page::collect("session_grant_list",&query,
            rows.into_iter().filter(|r|after.as_ref().is_none_or(|last|r.id.as_bytes()>last.as_slice())).map(|r|(r.id.as_bytes().to_vec(),public(r,state,now))),
            "session_grant_show")?;
        let supported=context.agent.as_deref().is_some_and(|a|super::registration(state,a).is_ok());
        Ok(json!({"supported":supported,"consumer":context.agent,"session_binding":"native host context required",
            "confirmation_available":Value::Null,"reason":if supported{Value::Null}else{json!("A registered native Claude Code or Codex integration is required.")},
            "records":page.items,"total":total,
            "next_cursor":page.next_cursor}))
    })
}

#[cfg(test)]
mod paging_tests {
    use super::*;

    #[test]
    fn session_grant_pages_are_stateless_and_refuse_changed_filters() {
        let (_root, context, reference) = crate::review::session_grants::tests::fixture();
        let offer = |index| {
            crate::review::session_grants::propose(&context,&json!({
            "session_ref":reference,"idempotency_key":format!("page-{index}"),
            "scope":[{"conditions":{"domain":"argv","executable":format!("acme-{index}"),
                "arguments":{"kind":"exact","values":["read"]},"cwd":{"within":{"root":"project"}}}}]
        })).unwrap()
        };
        for index in 0..3 {
            offer(index);
        }
        let first = show(&context, &json!({"session_ref":reference,"limit":1})).unwrap();
        let first_id = first["records"][0]["proposal_id"].as_str().unwrap();
        assert!(first["next_cursor"].is_string());
        offer(3);
        let mut cursor = first["next_cursor"].clone();
        let mut seen = std::collections::BTreeSet::from([first_id.to_owned()]);
        while cursor.is_string() {
            let page = show(
                &context,
                &json!({"session_ref":reference,"limit":2,"cursor":cursor}),
            )
            .unwrap();
            for row in page["records"].as_array().unwrap() {
                let id = row["proposal_id"].as_str().unwrap();
                assert!(id > first_id);
                assert!(seen.insert(id.to_owned()));
            }
            cursor = page["next_cursor"].clone();
        }
        assert!(seen.len() >= 3);
        assert!(matches!(
            show(
                &context,
                &json!({"session_ref":"foreign","cursor":first["next_cursor"]})
            ),
            Err(Error::InvalidCursor)
        ));
        assert!(matches!(
            show(&context, &json!({"cursor":"malformed"})),
            Err(Error::InvalidCursor)
        ));
        for limit in [0, 101] {
            assert!(show(&context, &json!({"limit":limit})).is_err());
        }
    }
}
