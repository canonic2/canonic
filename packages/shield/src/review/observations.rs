//! Optional host history uses the shared store without holding the enforcement lock.
use super::{
    Context, Error, EventInput,
    model::State,
    storage::{Store, history},
};
use serde_json::{Value, json};
use std::{ops::ControlFlow, path::Path};

#[cfg(test)]
mod tests;

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Host {
    pub(crate) agent: String,
    pub(crate) session_id: Option<String>,
    pub(crate) tool_use_id: Option<String>,
    pub(crate) event: String,
    pub(crate) consumer_id: Option<String>,
}
impl Host {
    pub(crate) fn valid(&self) -> bool {
        [&self.agent, &self.event]
            .into_iter()
            .all(|s| !s.is_empty() && s.len() <= 128 && !s.contains('\0'))
            && self
                .tool_use_id
                .as_ref()
                .is_none_or(|s| !s.is_empty() && s.len() <= 128 && !s.contains('\0'))
            && self
                .session_id
                .as_ref()
                .is_none_or(|s| !s.is_empty() && s.len() <= 4096 && !s.contains('\0'))
            && self
                .consumer_id
                .as_ref()
                .is_none_or(|s| !s.is_empty() && s.len() <= 128 && !s.contains('\0'))
    }
}

pub(crate) fn context(
    root: &Path,
    project_id: &str,
    cwd: &str,
    host: &Host,
) -> Result<Context, Error> {
    let mut context = super::open(root)?;
    if context.project_id != project_id {
        return Err(Error::Invalid("observation project changed"));
    }
    context.surface = "hook".into();
    context.source = "host".into();
    context.agent = Some(host.agent.clone());
    context.cwd = Some(cwd.into());
    context.session_id.clone_from(&host.session_id);
    context.tool_use_id.clone_from(&host.tool_use_id);
    context.host_event_name = Some(host.event.clone());
    Ok(context)
}

fn receipt(event: &Value, request: &Value) -> Value {
    json!({"status":"recorded","request_id":request,"event_id":event["event_id"]})
}

pub(crate) fn decision(context: &Context, mut details: Value) -> Result<Value, Error> {
    // Large selected-pack metadata is diagnostic evidence, not request history.
    // Keep the decision, request, matched rules and source identity when omitting it.
    if details.to_string().len() > 4096 {
        if let Some(evaluation) = details["evaluation"].as_object_mut() {
            evaluation.remove("effective");
            if let Some(explanation) = evaluation
                .get_mut("evaluation")
                .and_then(Value::as_object_mut)
            {
                explanation.remove("packs");
            }
        }
        details["missing"] = json!(["selected_pack_metadata"]);
    }
    Store::open(Path::new(&context.project_root))?.observe(context, |state| {
        if !state.history_enabled {
            return Ok(json!({"status":"disabled","request_id":null,"event_id":null}));
        }
        let request = details["approval"]["request_id"]
            .as_str()
            .map(str::to_owned)
            .map_or_else(|| history::identifier("request", &state.project_id), Ok)?;
        let mut event = state.event(
            "decision",
            context,
            json!({"request_id":request,"approval_id":details["approval"]["approval_id"]}),
            json!({"policy_reason":details["evaluation"]["evaluation"]["reason"]}),
            details,
        );
        event["event_id"] = json!(history::identifier("event", &state.project_id)?);
        let result = receipt(&event, &json!(request));
        state.record(event);
        Ok(result)
    })
}

pub(crate) fn completion(
    context: &Context,
    input: &EventInput,
    operation: &Value,
) -> Result<Value, Error> {
    input.validate()?;
    let signature = crate::policy::fingerprint(
        &serde_json::to_vec(&json!([input, operation]))
            .map_err(|_| Error::Invalid("completion encoding"))?,
    );
    // Raw publication only: history readers derive links, duplicates and
    // conflicts, so the receipt claims no correlation.
    Store::open(Path::new(&context.project_root))?.observe(context, |state| {
        let mut event = state.event(
            "completion",
            context,
            json!({}),
            json!({}),
            json!({"delivery_id":input.delivery_id,"input_digest":signature,"status":input.status,
                "operation":operation,"exit_code":null,"command":null,"evidence":"host-reported",
                "reported_request_id":input.request_id}),
        );
        event["event_id"] = json!(history::identifier("event", &state.project_id)?);
        let result = receipt(&event, &Value::Null);
        state.record(event);
        Ok(result)
    })
}

/// Records an owned native child's result, separate from SDK-supplied observations.
#[cfg(target_os = "macos")]
pub(crate) fn execution_completion(
    context: &Context,
    request: Option<&str>,
    operation: &Value,
    status: &str,
    exit_code: Option<i32>,
    signal: Option<i32>,
) -> Result<Value, Error> {
    Store::open(Path::new(&context.project_root))?.observe(context, |state| {
        let mut event = state.event("completion",context,json!({}),json!({}),json!({"operation":operation,"status":status,"exit_code":exit_code,"signal":signal,"evidence":"owned-child","reported_request_id":request}));
        event["event_id"] = json!(history::identifier("event", &state.project_id)?);
        let value = receipt(&event,&Value::Null);
        state.record(event);
        Ok(value)
    })
}

/// Stored state for advisory reply text on the hook path. Storage identity and
/// bindings are checked; policy is not re-prepared, since these readers grant
/// nothing and the decision already came from the authenticated plan.
fn advisory(root: &Path) -> Option<State> {
    Store::open(root).ok()?.read_unprepared().ok()
}

/// Newest-first retained events for hook reply text, from the checkpoint and the
/// bounded history tail; the work does not grow with retained history.
fn recent(root: &Path, visit: impl FnMut(&Value) -> ControlFlow<()>) -> Option<()> {
    let store = Store::open(root).ok()?;
    let state = store.read_checkpoint().ok()?;
    history::recent(&store, &state, visit).ok().map(|_| ())
}

/// The stored explanation of one native call's Ask decision, when that decision
/// is still in the bounded history tail. The scan stops at the call's decision.
/// A completion through the host is not operator approval, so none is required.
pub(crate) fn asked_explanation(root: &Path, session: &str, tool_use_id: &str) -> Option<Value> {
    let mut decision = None;
    recent(root, |event| {
        if event["kind"] == "decision"
            && event["details"]["host_decision"] == "ask"
            && event["provenance"]["session_id"] == session
            && event["provenance"]["tool_use_id"] == tool_use_id
        {
            decision = Some(event.clone());
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    })?;
    let decision = decision?;
    let evaluation = &decision["details"]["evaluation"];
    Some(
        evaluation
            .get("explanation")
            .unwrap_or(&evaluation["evaluation"])
            .clone(),
    )
}

/// The request a history entry recorded: its operation, the agent and when, so
/// a proposal linked to it can show what the agent was trying to do.
pub(crate) fn requested(root: &Path, entry: &str) -> Option<Value> {
    let state = advisory(root)?;
    let decision = state
        .events
        .iter()
        .find(|event| event["kind"] == "decision" && event["links"]["request_id"] == entry)?;
    Some(
        serde_json::json!({"operation":decision["details"]["operation"],
        "agent":decision["provenance"]["agent"],"recorded_at":decision["recorded_at"]}),
    )
}

/// One distinct request policy asked about: the newest decision's explanation
/// and history entry, and how many times it was asked.
pub(crate) struct AskedRequest {
    pub(crate) explanation: Value,
    pub(crate) entry: Option<String>,
    pub(crate) count: usize,
    pub(crate) operation: Value,
    pub(crate) session: Option<String>,
    pub(crate) invocations: std::collections::BTreeSet<String>,
    pub(crate) recurrence_known: bool,
    pub(crate) partial: bool,
    /// A newer escalated call not yet known to retry an earlier attempt.
    escalated: Option<(Option<String>, Option<i64>)>,
}

/// An escalated call this soon after an unescalated attempt of the same request
/// is that attempt's retry. Agents retry a sandbox refusal within seconds.
const RETRY_WINDOW_SECONDS: i64 = 60;

impl AskedRequest {
    /// Counts one pre-execution attempt, visited newest first. Every deliberate
    /// invocation counts once, escalated or not; an escalated retry of the
    /// attempt just before it adds nothing.
    fn attempt(&mut self, invocation: Option<String>, escalation: bool, at: Option<i64>) {
        if let Some((held, held_at)) = self.escalated.take() {
            let retry = !escalation
                && matches!((at, held_at), (Some(at), Some(held_at))
                    if (0..=RETRY_WINDOW_SECONDS).contains(&(held_at - at)));
            if !retry {
                self.invocations.extend(held);
            }
        }
        if escalation {
            self.escalated = Some((invocation, at));
        } else {
            self.invocations.extend(invocation);
        }
    }

    fn finish(&mut self) {
        if let Some((held, _)) = self.escalated.take() {
            self.invocations.extend(held);
        }
    }
}

pub(crate) struct AskedRequests {
    pub(crate) requests: Vec<AskedRequest>,
    pub(crate) complete: bool,
    pub(crate) available: bool,
}

/// Distinct requests policy asked about since the last policy activation,
/// newest first and at most `cap`, counted once per pre-execution decision.
/// A session limits them to that agent session. An activation ends the scan,
/// so applying changes starts a fresh list. Declines do not activate policy.
pub(crate) fn asked_requests(root: &Path, session: Option<&str>, cap: usize) -> AskedRequests {
    let mut found: Vec<(Value, AskedRequest)> = Vec::new();
    let store = Store::open(root).and_then(|store| Ok((store.read_checkpoint()?, store)));
    let Ok((state, store)) = store else {
        return AskedRequests {
            requests: vec![],
            complete: false,
            available: false,
        };
    };
    if !state.history_enabled {
        return AskedRequests {
            requests: vec![],
            complete: false,
            available: false,
        };
    }
    let mut capped = false;
    // Escalation requests not yet matched to the call they escalate.
    let mut escalations: Vec<Value> = Vec::new();
    let scanned = history::recent(&store, &state, |event| {
        if event["kind"] == "runtime_activated" {
            return ControlFlow::Break(());
        }
        let provenance = &event["provenance"];
        if event["kind"] != "decision"
            || session.is_some_and(|session| provenance["session_id"] != session)
        {
            return ControlFlow::Continue(());
        }
        let operation = &event["details"]["operation"];
        let identity = json!([operation, provenance["session_id"]]);
        // Codex's hook input omits sandbox_permissions; an escalated call is
        // followed by a PermissionRequest for the same command. Scanning newest
        // first meets that request before the call it escalates.
        if provenance["host_event_name"] == "PermissionRequest" {
            escalations.push(identity);
            return ControlFlow::Continue(());
        }
        if event["details"]["decision"] != "ask" || provenance["host_event_name"] != "PreToolUse" {
            return ControlFlow::Continue(());
        }
        let invocation = provenance["tool_use_id"]
            .as_str()
            .map(|id| format!("{}:{id}", provenance["session_id"]));
        let requested = escalations.iter().position(|seen| seen == &identity);
        if let Some(index) = requested {
            escalations.swap_remove(index);
        }
        let escalation = requested.is_some()
            || event["details"]["original_input"]["sandbox_permissions"] == "require_escalated";
        let at = event["recorded_at"]
            .as_str()
            .and_then(canonic_registry::timestamp::parse)
            .map(canonic_registry::timestamp::unix_seconds);
        if let Some((_, known)) = found.iter_mut().find(|(seen, _)| seen == &identity) {
            known.count += 1;
            known.recurrence_known &= invocation.is_some();
            known.attempt(invocation, escalation, at);
        } else if found.len() < cap {
            let evaluation = &event["details"]["evaluation"];
            let mut request = AskedRequest {
                explanation: evaluation
                    .get("explanation")
                    .or_else(|| evaluation.get("evaluation"))
                    .unwrap_or(evaluation)
                    .clone(),
                entry: event["links"]["request_id"].as_str().map(str::to_owned),
                count: 1,
                operation: operation.clone(),
                session: provenance["session_id"].as_str().map(str::to_owned),
                invocations: std::collections::BTreeSet::new(),
                recurrence_known: invocation.is_some(),
                partial: evaluation["coverage"] == "partial",
                escalated: None,
            };
            request.attempt(invocation, escalation, at);
            found.push((identity, request));
        } else {
            capped = true;
        }
        ControlFlow::Continue(())
    });
    for (_, request) in &mut found {
        request.finish();
    }
    AskedRequests {
        requests: found.into_iter().map(|(_, request)| request).collect(),
        complete: scanned.as_ref().is_ok_and(|complete| *complete) && !capped,
        available: scanned.is_ok(),
    }
}

/// Relevant uninstalled packs in deterministic reference order. Only the latest
/// admitted catalog's prepared index is consulted, through currently enrolled
/// aliases; historical releases and removed/shadowed sources are not searched.
pub(crate) fn uninstalled_packs(root: &Path, executable: &str) -> Vec<String> {
    uninstalled_candidates(root, executable).unwrap_or_default()
}

fn uninstalled_candidates(root: &Path, executable: &str) -> Option<Vec<String>> {
    // Catalogs and installed packs are checkpoint fields; skip history and sources.
    let state = Store::open(root).ok()?.read_checkpoint().ok()?;
    let leaf = Path::new(executable).file_name()?.to_str()?;
    let sources = crate::review::registries::advisory_sources(&state.operator).ok()?;
    let installed = |identity: &str, id: &str| {
        state.operator["installed"].as_array().is_some_and(|rows| {
            rows.iter()
                .any(|row| row["id"] == id && row["source_identity"] == identity)
        })
    };
    let mut found = std::collections::BTreeMap::new();
    for (alias, source, _) in sources.iter() {
        let Some(checkpoint) = state.registry_checkpoints.get(source.identity()) else {
            continue;
        };
        for id in checkpoint["commands"][leaf]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            if !installed(source.identity(), id)
                && id.parse::<canonic_registry::names::PackId>().is_ok()
            {
                found
                    .entry((source.identity().to_owned(), id.to_owned()))
                    .or_insert_with(|| format!("{alias}/{id}"));
            }
        }
    }
    let mut references: Vec<_> = found.into_values().collect();
    references.sort();
    Some(references)
}

/// Retains one registry release in the project's catalogs, as `shield registry add` would.
#[cfg(test)]
pub(crate) fn retain_release(context: &Context, registry: &str, release: &str) {
    Store::open(Path::new(&context.project_root))
        .unwrap()
        .update(context, &|| Ok(()), |state| {
            let sources = crate::review::registries::advisory_sources(&state.operator)?;
            let source = sources
                .select(Some(registry))
                .map_err(|_| Error::Invalid("test registry"))?;
            let id = release.split_once('@').unwrap().0;
            let command = id.rsplit('/').next().unwrap();
            state.registry_checkpoints.insert(
                source.identity().into(),
                json!({"digest":"0".repeat(64),"releases":{release:"1".repeat(64)},"sequence":1,"commands":{command:[id]}}),
            );
            Ok(Value::Null)
        })
        .unwrap();
}

/// Changes advisory metadata in an isolated hook fixture without exposing storage.
#[cfg(test)]
pub(crate) fn update_advisory_fixture(
    context: &Context,
    mut update: impl FnMut(&mut Value, &mut std::collections::BTreeMap<String, Value>),
) -> Value {
    Store::open(Path::new(&context.project_root))
        .unwrap()
        .update(context, &|| Ok(()), |state| {
            update(&mut state.operator, &mut state.registry_checkpoints);
            Ok(state.operator.clone())
        })
        .unwrap()
}
