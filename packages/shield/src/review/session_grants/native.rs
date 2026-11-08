//! Native session-context capture and revision-locked reusable grant handoff.
use super::{
    Context, Conversation, Error, Record, Value, activation, hash, json, registration, scope, store,
};
use crate::policy::Request;

/// Observational capture: existing contexts need neither a lock nor a write.
fn capture(context: &Context, receipt: &mut Value) -> Result<(), Error> {
    let Some(agent) = context
        .agent
        .as_deref()
        .filter(|a| crate::agents::find(a).is_some())
    else {
        return Ok(());
    };
    let Some(session) = context.session_id.as_deref().filter(|s| !s.is_empty()) else {
        return Ok(());
    };
    let now = super::super::clock::now_ms()?;
    let key = hash(&json!([agent, session]));
    let store = store(context)?;
    let state = store.read_checkpoint()?;
    state.check(context)?;
    if registration(&state, agent).is_err() {
        return Ok(());
    }
    let existing = state
        .session_grants
        .contexts
        .get(&key)
        .filter(|s| {
            super::Authority::capture(&state).context_matches(&key, s)
                && now < s.created.saturating_add(super::RETAIN)
        })
        .cloned();
    let first = existing.is_none();
    let captured = if existing.is_some() {
        existing
    } else {
        let mut captured = None;
        store.update_checkpoint_if_changed(context, &|| Ok(()), |state| {
            let Ok(reg) = registration(state, agent) else {
                return Ok(false);
            };
            // Another hook may have captured the same tuple since our read.
            if let Some(record) = state.session_grants.contexts.get(&key).filter(|s| {
                super::Authority::capture(state).context_matches(&key, s)
                    && now < s.created.saturating_add(super::RETAIN)
            }) {
                captured = Some(record.clone());
                return Ok(false);
            }
            state.session_grants.compact(now);
            if state.session_grants.contexts.len() >= 256 {
                return Ok(false);
            }
            let record = Conversation {
                reference: format!(
                    "session-{}",
                    hash(&json!([state.project_id, key, now, state.sequence]))
                ),
                agent: agent.into(),
                session_id: session.into(),
                registration: reg,
                created: now,
            };
            state
                .session_grants
                .contexts
                .insert(key.clone(), record.clone());
            captured = Some(record);
            Ok(true)
        })?;
        captured
    };
    let Some(captured) = captured else {
        return Ok(());
    };
    receipt["session_context"] = json!({"session_ref":captured.reference,"consumer":agent,"offer_hint":first || receipt["decision"]=="ask" || receipt["decision"]=="deny",
        "next_action":"For repeated privileged operations, inspect session_grant_show and propose a bounded batch or argv family before the batch. The user confirms session_grant_approve; project policy stays unchanged."});
    Ok(())
}

/// Called only by the registered native transport, after ordinary protection checks.
pub(crate) fn handoff(
    context: &Context,
    receipt: &mut Value,
    original: Option<&Value>,
) -> Result<(), Error> {
    // Context capture never grants authority and cannot veto ordinary policy.
    let _ = capture(context, receipt);
    if receipt["decision"] == "allow" {
        return Ok(());
    }
    if !eligible(receipt, original) {
        return Ok(());
    }
    let Some(agent) = context
        .agent
        .as_deref()
        .filter(|a| crate::agents::find(a).is_some())
    else {
        return Ok(());
    };
    let Some(session) = context.session_id.as_deref().filter(|s| !s.is_empty()) else {
        return Ok(());
    };
    handoff_session(context, receipt, agent, session)
}

fn eligible(receipt: &Value, original: Option<&Value>) -> bool {
    !(!["approval-required", "policy-deny", "policy-allow"]
        .iter()
        .any(|c| receipt["code"] == *c)
        || receipt["fixed_protection"] == true
        || receipt["protected_path"].is_string()
        || receipt["outside_path"].is_object()
        || !["single-literal-invocation", "structured-literal-operations"]
            .iter()
            .any(|c| receipt["_shell_analysis"]["coverage"] == *c)
        || original.and_then(Value::as_object).is_none_or(|o| {
            o.keys().any(|k| {
                ![
                    "command",
                    "description",
                    "justification",
                    "workdir",
                    "timeout",
                    "timeout_ms",
                    "yield_time_ms",
                    "max_output_tokens",
                    "max_output_chars",
                ]
                .contains(&k.as_str())
            })
        }))
}

fn handoff_session(
    context: &Context,
    receipt: &mut Value,
    agent: &str,
    session: &str,
) -> Result<(), Error> {
    let now = super::super::clock::now_ms()?;
    let key = hash(&json!([agent, session]));
    let store = store(context)?;
    let operations = receipt["_shell_analysis"]["operations"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    if operations.is_empty()
        || operations.len() > 256
        || operations.iter().any(|o| o["coverage"] != "literal-argv")
    {
        return Ok(());
    }
    let top = &receipt["_observation"]["evaluation"];
    let explanation = top.get("explanation").unwrap_or(&top["evaluation"]);
    // A source-level refusal cannot be covered by argv-only selectors.
    if explanation["matches"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|m| {
            m["applied"] != false && m["decision"] == "deny" && m.get("operation_ref").is_none()
        })
    {
        return Ok(());
    }
    let identity = receipt["identity"].clone();
    let covered=store.inspect_checkpoint(context,&||Ok(()),|state| {
        if activation(state)?!=json!([identity["activation_id"],identity["policy_revision"]]) {return Err(Error::StaleSnapshot);}
        let authority=super::Authority::capture(state);
        let ids=state.session_grants.sessions.get(&key).map_or(&[][..],Vec::as_slice);
        let mut candidates:Vec<&Record>=ids.iter().filter_map(|id|state.session_grants.records.get(id))
            .filter(|r|r.effective_with(&authority,now)=="active").collect();
        candidates.sort_by_key(|r|(!r.overrides.as_array().is_none_or(Vec::is_empty),r.grant.as_ref().map(|g|g.expires),&r.id));
        let prepared=candidates.iter().map(|r|scope::prepare(&r.scope)).collect::<Result<Vec<_>,_>>()?;
        let mut selected=std::collections::BTreeMap::<String,Value>::new();
        for operation in &operations {
            if operation["decision"]=="allow" {continue;}
            if operation["evaluation"]["evaluation"]["requirement_checks"].as_array().is_some_and(|checks|
                checks.iter().any(|c|c["applied"]!=false && c["status"]!="met")) {return Ok(None);}
            let request:Request=serde_json::from_value(operation["request"].clone()).map_err(|_|Error::Invalid("unsupported_scope: operation context unavailable"))?;
            // Remapped argv/file consumers need their own selector binding;
            // the written argv alone cannot describe their effective context.
            if !operation["policy_request"].is_null() {return Ok(None);}
            if !std::path::Path::new(&request.cwd).starts_with(&context.project_root) {return Ok(None);}
            // Never share an outer project's authorization with a nested enrollment.
            if std::path::Path::new(&request.cwd).ancestors().take_while(|p|*p!=std::path::Path::new(&context.project_root))
                .any(|p|p.join(".shield/review").exists()){return Ok(None);}
            let mut found=None;
            for (candidate,prepared) in candidates.iter().zip(&prepared) {
                let Some(grant)=&candidate.grant else{continue;};
                if !grant.clock.valid(candidate.duration*1000)? {continue;}
                if operation["decision"]=="deny" && !scope::covered_denies(&candidate.override_evidence,&operation["evaluation"]["evaluation"]) {continue;}
                if !scope::matches(prepared,&request)? {continue;}
                let (Some(unit),Some(start),Some(end))=(operation["source_unit"].as_u64(),operation["span"]["start"].as_u64(),operation["span"]["end"].as_u64()) else{return Ok(None);};
                found=Some(json!({"grant_id":grant.id,"approval_id":grant.approval,"operation_refs":[format!("shell:{unit}:{start}-{end}")],"override_denies":if operation["decision"]=="deny"{candidate.overrides.clone()}else{json!([])}}));break;
            }
            let Some(found)=found else{return Ok(None);};
            let id=found["grant_id"].as_str().ok_or(Error::Invalid("grant identity unavailable"))?.to_owned();
            if let Some(previous)=selected.get_mut(&id) {
                previous["operation_refs"].as_array_mut().ok_or(Error::Invalid("grant references unavailable"))?.push(found["operation_refs"][0].clone());
                if !found["override_denies"].as_array().is_none_or(Vec::is_empty) {previous["override_denies"]=found["override_denies"].clone();}
            }else{selected.insert(id,found);}
        }
        Ok((!selected.is_empty()).then(||json!({"kind":"session_grant","grants":selected.into_values().collect::<Vec<_>>()})))
    })?;
    if let Some(authorization) = covered {
        receipt["authorization"] = authorization;
        receipt["disposition"] = json!("allowed");
        receipt["ask_fallback"] = Value::Null;
    }
    Ok(())
}
