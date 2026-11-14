//! Shared bounded hook orchestration; adapters alone interpret and encode vendors.
use super::{AgentAdapter, Decision, Event, Reply, Request, feedback, find};
#[cfg(test)]
use crate::runtime::Project;
use crate::{cli::NativeHook, enforcement::protocol};
use serde_json::{Value, json};
use std::{io::Read, path::Path, process::ExitCode};

mod approval;
pub(super) use approval::OFFER;
#[cfg(test)]
mod tests;

pub(crate) fn run(input: &NativeHook, log: &crate::diagnostics::Log) -> ExitCode {
    let Some(adapter) = find(&input.agent) else {
        return ExitCode::from(2);
    };
    let legacy = input.event.is_empty() && adapter.accepts_unpinned_hook_events();
    let pinned = adapter.event(&input.event);
    if pinned.is_none() && !legacy {
        return ExitCode::from(2);
    }
    log.event("hook_start", || json!({"agent":input.agent}));
    let (bytes, oversized) = read_input(&mut std::io::stdin().lock());
    let read_ok = bytes.is_ok();
    let bytes = bytes.unwrap_or_default();
    log.hook_input(&input.agent, &bytes, read_ok, oversized.is_some());
    // Old launchers did not pin an event. Infer only from a complete, strictly
    // parsed vendor envelope. Unknown, duplicate, truncated and oversized input
    // always uses the authorization denial format, never completion silence.
    let expected = pinned.unwrap_or_else(|| {
        if read_ok && oversized.is_none() {
            legacy_event(adapter, &bytes)
        } else {
            Event::Before
        }
    });
    let value = if !read_ok || oversized.is_some() {
        if read_ok && expected.post() && oversized.as_ref().is_some_and(|o| !o.names_pre) {
            json!({})
        } else {
            adapter.reply(expected, &feedback::deny("protection-input-failure"))
        }
    } else {
        translate_hook(
            &input.project,
            adapter,
            &bytes,
            expected,
            &|name| std::env::var_os(name),
            log,
        )
    };
    log.hook_output(&value);
    println!("{value}");
    ExitCode::SUCCESS
}

fn legacy_event(adapter: &dyn AgentAdapter, bytes: &[u8]) -> Event {
    crate::json::parse(bytes)
        .ok()
        .and_then(|host| adapter.input_event(&host))
        .unwrap_or(Event::Before)
}

const MAX_HOOK_INPUT: usize = 8 * 1024 * 1024;
struct Oversized {
    names_pre: bool,
}

/// Drain overflow with bounded memory, retaining authorization-name evidence
/// even when an event name crosses the retained prefix or a read boundary.
fn read_input(source: &mut impl Read) -> (std::io::Result<Vec<u8>>, Option<Oversized>) {
    let overlap = authorization_event_names()
        .map(<[u8]>::len)
        .max()
        .unwrap_or(1)
        - 1;
    let contains = names_authorization_event;
    let mut bytes = Vec::new();
    if let Err(error) = source
        .take((MAX_HOOK_INPUT + 1) as u64)
        .read_to_end(&mut bytes)
    {
        return (Err(error), None);
    }
    if bytes.len() <= MAX_HOOK_INPUT {
        return (Ok(bytes), None);
    }
    let mut names_pre = contains(&bytes);
    let mut carry = bytes[bytes.len() - overlap..].to_vec();
    let mut chunk = vec![0; 65_536];
    loop {
        match source.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                carry.extend_from_slice(&chunk[..n]);
                names_pre |= contains(&carry);
                carry.drain(..carry.len() - overlap);
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) => return (Err(error), None),
        }
    }
    bytes.truncate(MAX_HOOK_INPUT);
    (Ok(bytes), Some(Oversized { names_pre }))
}

/// Every adapter's authorization (non-completion) event name.
fn authorization_event_names() -> impl Iterator<Item = &'static [u8]> {
    super::ADAPTERS
        .iter()
        .flat_map(|a| a.spec().events.iter())
        .filter(|(_, event)| !event.post())
        .map(|(name, _)| name.as_bytes())
}

/// Whether raw input bytes name any authorization event, independent of parsing.
fn names_authorization_event(bytes: &[u8]) -> bool {
    authorization_event_names().any(|name| bytes.windows(name.len()).any(|w| w == name))
}

#[cfg(test)]
fn translate(root: &Path, agent: &str, bytes: &[u8]) -> Value {
    translate_logged(root, agent, bytes, &crate::diagnostics::Log::disabled())
}
#[cfg(test)]
fn translate_logged(
    root: &Path,
    agent: &str,
    bytes: &[u8],
    log: &crate::diagnostics::Log,
) -> Value {
    translate_event(
        root,
        find(agent).expect("test adapter"),
        bytes,
        Event::Before,
        log,
    )
}

#[cfg(test)]
pub(crate) fn translate_event(
    root: &Path,
    adapter: &dyn AgentAdapter,
    bytes: &[u8],
    event: Event,
    log: &crate::diagnostics::Log,
) -> Value {
    translate_hook(root, adapter, bytes, event, &|_| None, log)
}

/// `env` reads the hook's environment, which locates the host's session folder.
fn translate_hook(
    root: &Path,
    adapter: &dyn AgentAdapter,
    bytes: &[u8],
    event: Event,
    env: &dyn Fn(&str) -> Option<std::ffi::OsString>,
    log: &crate::diagnostics::Log,
) -> Value {
    let host = crate::json::parse(bytes);
    let deny = |code| adapter.reply(event, &feedback::deny(code));
    // A completion has no denial format; invalid completion input stays quiet
    // unless it names an authorization event.
    let Ok(host) = host else {
        return if event.post() && !names_authorization_event(bytes) {
            json!({})
        } else {
            deny("invalid-host-input")
        };
    };
    let Ok(launch) = std::path::absolute(root) else {
        return if event.post() {
            json!({})
        } else {
            deny("runtime-unavailable")
        };
    };
    // Like Git, the folder the host started the session in finds its project,
    // so a moved checkout or a worktree that inherited the hook's absolute
    // `--project` keeps working. The agent cannot change that folder.
    // Cwd supplies context, not policy authority.
    let launch = adapter
        .session_dir(&host, env)
        .and_then(|session| crate::mcp_io::session_project(&session))
        .unwrap_or(launch);
    let root = launch.as_path();
    let actual = adapter.input_event_for(&host, event);
    if actual != Some(event) {
        return match actual {
            // The host awaits a reply in the supplied authorization event's format.
            Some(supplied) if !supplied.post() => {
                adapter.reply(supplied, &feedback::deny("invalid-host-event"))
            }
            // Like unparsable input, a completion stays quiet only when its
            // bytes name no authorization event the host could be awaiting.
            _ if event.post() && !names_authorization_event(bytes) => json!({}),
            _ => deny("invalid-host-event"),
        };
    }
    if event == Event::Ended {
        if let Some(session) = host["session_id"]
            .as_str()
            .filter(|s| !s.is_empty() && s.len() <= 4096)
        {
            let _ =
                crate::review::proposal_lifecycle::end_session(root, adapter.spec().id, session);
        }
        return json!({});
    }
    let Some(mut request) = adapter.request_for_project(&host, event, root) else {
        return if event.post() {
            json!({})
        } else {
            let mut reply = feedback::deny("invalid-host-input");
            if let Some(guidance) = adapter.invalid_input_guidance() {
                reply.agent_context = Some(guidance.into());
            }
            adapter.reply(event, &reply)
        };
    };
    if !request.event.post() {
        request.working_files = Some(adapter.working_files(&host, root));
    }
    handle(root,adapter,&request,log).unwrap_or_else(|| {
        if event.post() { json!({"systemMessage":"Shield completion history unavailable; outcome remains unknown."}) }
        else { deny("protection-failure") }
    })
}

/// An Allow reply with no added context for the agent.
fn allow(adapter: &dyn AgentAdapter, event: Event, reason: String) -> Value {
    adapter.reply(
        event,
        &Reply {
            decision: Decision::Allow,
            agent_context: None,
            reason,
        },
    )
}

fn handle(
    root: &Path,
    adapter: &dyn AgentAdapter,
    request: &Request<'_>,
    log: &crate::diagnostics::Log,
) -> Option<Value> {
    log.event("hook_stage", || json!({"stage":"project_open_start"}));
    let (context, paused) = match crate::review::open_native(root) {
        Ok(opened) => opened,
        // A recognized name keeps inspection/recovery reachable, but the alias
        // alone cannot justify granting permission without project authority.
        Err(_) if request.shield_tool.is_some() => return Some(json!({})),
        Err(_) => return None,
    };
    log.event("hook_stage", || json!({"stage":"project_open_ready"}));
    if let Some(paused) = paused {
        return Some(paused_reply(adapter, request, &paused));
    }
    if let Some(name) = request.shield_tool {
        return Some(shield_tool_reply(adapter, request, name, log));
    }
    log.event("hook_stage", || json!({"stage":"registration_ready"}));
    if request.operation.is_none() && request.harmless && request.event == Event::Permission {
        return Some(allow(adapter, request.event, String::new()));
    }
    // Internal tools outside Shield's mappings belong to the host's permission
    // system. Apply this uniformly to every native adapter, including future ones.
    // Encoded MCP requests retain connection classification and never use this
    // fallback, even if their adapter cannot map them.
    let operation = request.operation.as_ref().filter(|operation| {
        operation["kind"] != "unsupported" || request.tool.starts_with("mcp__")
    });
    let Some(operation) = operation else {
        log.event(
            "hook_internal_bypass",
            || json!({"tool_name":request.tool,"post":request.event.post()}),
        );
        return Some(json!({}));
    };
    let prepared = if operation["kind"] == "mcp" {
        match prepare_mcp(root, adapter, request, operation["alias"].as_str()?, log) {
            Ok(prepared) => Some(prepared),
            Err(response) => return Some(response),
        }
    } else {
        None
    };
    let operations = if operation["kind"] == "batch" {
        operation["operations"].as_array()?.clone()
    } else {
        vec![operation.clone()]
    };
    decide_operations(
        adapter,
        request,
        &context,
        operations,
        prepared,
        log,
        operation["unmodeled"] == true,
    )
}

/// One native batch retains a single exact source authority through every
/// override and the final vendor callback. Individual observations are optional.
fn decide_operations(
    adapter: &dyn AgentAdapter,
    request: &Request<'_>,
    context: &crate::review::Context,
    operations: Vec<Value>,
    mut prepared: Option<protocol::Prepared>,
    log: &crate::diagnostics::Log,
    mut unmodeled: bool,
) -> Option<Value> {
    let root = Path::new(&context.project_root);
    let local_id = if request.event.post() {
        request.tool_id.map_or_else(invocation_id, str::to_owned)
    } else {
        invocation_id()
    };
    let mut strictest: Option<Reply> = None;
    let mut refusals = Vec::new();
    let mut authorities: Vec<crate::review::runtime::consumer::NativeAuthority> = Vec::new();
    for (index, operation) in operations.into_iter().enumerate() {
        if request.event.post()
            && !["read-text", "shell", "file", "web", "mcp"]
                .contains(&operation["kind"].as_str().unwrap_or_default())
        {
            unmodeled = true;
            continue;
        }
        let id = if index == 0 {
            local_id.clone()
        } else {
            format!("{local_id}:{index}")
        };
        let NativeDecision {
            reply,
            refusal,
            authority,
        } = decide(
            adapter,
            request,
            context,
            &id,
            operation,
            prepared.take(),
            log,
        )?;
        if let Some(authority) = authority {
            if let Some(current) = authorities.first() {
                if !current.same_source(&authority) {
                    return Some(finalize_native(authorities, || {
                        adapter.reply(request.event, &feedback::deny("runtime-unavailable"))
                    }));
                }
                // One exact source guard covers the whole batch. Do not
                // multiply retained head/file ownership per operation.
            } else {
                authorities.push(authority);
            }
        }
        if reply.decision != Decision::Allow {
            refusals.push(refusal);
        }
        if strictest
            .as_ref()
            .is_none_or(|r| reply.decision > r.decision)
        {
            strictest = Some(reply);
        }
    }
    finalize_native(authorities, || {
        if request.event.post() {
            Some(completion_reply(root, adapter, request, unmodeled))
        } else {
            let mut reply = strictest?;
            if blocks(adapter, request.event, &reply)
                && let Some(allowed) =
                    approval::in_session(root, adapter, request, &refusals, &mut reply)
            {
                reply = allowed;
            }
            if unmodeled && reply.decision == Decision::Allow {
                // A reply covers the whole call; leave unmodeled actions under
                // host permissions even when every modeled operation may proceed.
                return Some(json!({}));
            }
            Some(adapter.reply(request.event, &reply))
        }
    })
}

/// Exact source trust stays stable through override consumption and the final
/// vendor-disposition callback, never through external host execution.
pub(crate) fn finalize_native<T>(
    authorities: Vec<crate::review::runtime::consumer::NativeAuthority>,
    disposition: impl FnOnce() -> T,
) -> T {
    let response = disposition();
    drop(authorities);
    response
}

fn completion_reply(
    root: &Path,
    adapter: &dyn AgentAdapter,
    request: &Request<'_>,
    unmodeled: bool,
) -> Value {
    let note = if unmodeled {
        None
    } else {
        request
            .tool_id
            .and_then(|id| feedback::approval_note(root, adapter, request.session, id))
    };
    adapter.completion(request.event_name, note.as_deref())
}

fn paused_reply(
    adapter: &dyn AgentAdapter,
    request: &Request<'_>,
    paused: &crate::review::mode::Paused,
) -> Value {
    if request.event.post()
        || request.shield_tool.is_some()
        || (request.operation.is_none() && request.harmless)
    {
        return json!({});
    }
    let result = request
        .operation
        .as_ref()
        .ok_or("unsupported-operation")
        .and_then(|operation| {
            protocol::paused_protection(
                Path::new(request.cwd.as_ref()),
                operation,
                paused,
                request.working_files.as_ref(),
            )
        });
    match result {
        Ok(()) => json!({}),
        Err(code) => adapter.reply(request.event, &feedback::deny(code)),
    }
}

/// Whether the host stops the request rather than asking the user itself.
fn blocks(adapter: &dyn AgentAdapter, event: Event, reply: &Reply) -> bool {
    match reply.decision {
        Decision::Allow => false,
        Decision::Deny => true,
        Decision::Ask => adapter.approval(event) == protocol::NativeApproval::Fallback,
    }
}

fn shield_tool_reply(
    adapter: &dyn AgentAdapter,
    request: &Request<'_>,
    name: &str,
    log: &crate::diagnostics::Log,
) -> Value {
    log.event(
        "hook_mcp_bypass",
        || json!({"tool_name":request.tool,"post":request.event.post()}),
    );
    if request.event.post() {
        return adapter.completion(request.event_name, None);
    }
    if name == "policy_apply" {
        // A tool-permission review needs the host's own prompt at either hook
        // event. Form review remains owned by the MCP server.
        return if adapter.review().confirmation() == super::review::Confirmation::ToolPermission {
            json!({})
        } else {
            allow(adapter, request.event, String::new())
        };
    }
    allow(
        adapter,
        request.event,
        adapter.allow_reason(super::AllowNotice::ShieldTool).into(),
    )
}

fn prepare_mcp(
    root: &Path,
    adapter: &dyn AgentAdapter,
    request: &Request<'_>,
    alias: &str,
    log: &crate::diagnostics::Log,
) -> Result<protocol::Prepared, Value> {
    let mut prepared = protocol::Prepared::load(root, &|stage| log.event("hook_stage", || json!({"stage":stage})))
        .map_err(|_| {
            if request.event.post() {
                return json!({"systemMessage":"Shield completion history unavailable; MCP bindings could not be checked."});
            }
            let mut reply = feedback::deny("mcp-bindings-unavailable");
            reply.reason = "Shield: blocked. MCP bindings could not be checked. The agent can diagnose and request repair in this session.".into();
            reply.agent_context = Some("Shield could not determine whether this MCP server is bound to project policy. Call doctor or status, then use its in-session repair route and obtain any required user confirmation before retrying.".into());
            adapter.reply(request.event, &reply)
        })?;
    match prepared.prepare_mcp(
        root,
        Path::new(request.cwd.as_ref()),
        adapter.spec().id,
        alias,
    ) {
        Ok(true) => Ok(prepared),
        Ok(false) => {
            log.event(
                "hook_mcp_external",
                || json!({"alias":alias,"reason":"no matching configured MCP policy"}),
            );
            Err(json!({}))
        }
        Err(error) => {
            if request.event.post() {
                return Err(json!({"systemMessage":"Shield MCP completion mapping unavailable."}));
            }
            let mut reply = feedback::deny("mcp-binding-changed");
            reply.reason = format!(
                "Shield: blocked. {error}. Inspect the MCP configuration with the doctor tool before retrying."
            );
            Err(adapter.reply(request.event, &reply))
        }
    }
}

fn invocation_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!(
        "hook-{}-{time}-{}",
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    )
}

struct NativeDecision {
    reply: Reply,
    refusal: Value,
    authority: Option<crate::review::runtime::consumer::NativeAuthority>,
}

fn decide(
    adapter: &dyn AgentAdapter,
    request: &Request<'_>,
    context: &crate::review::Context,
    id: &str,
    mut operation: Value,
    prepared: Option<protocol::Prepared>,
    log: &crate::diagnostics::Log,
) -> Option<NativeDecision> {
    let root = Path::new(&context.project_root);
    let decided = operation.clone();
    if request.event.post() {
        operation = json!({"kind":"completion","delivery_id":format!("{id}-{}",request.event_name),"request_id":null,
            "status":if request.event==Event::Failed {"failed"} else if operation["kind"]=="shell" {"unknown"} else {"completed"},"operation":operation});
    }
    let normalized = json!({"protocol_version":1,"request_id":id,"expected_project_id":context.project_id,"cwd":request.cwd,"operation":operation,
        "host":{"agent":adapter.spec().id,"session_id":request.session,"tool_use_id":request.tool_id,"event":request.event_name,"consumer_id":null},"approval_request_id":null});
    let native = protocol::NativeContext {
        integration: adapter.spec().id,
        approval: adapter.approval(request.event),
        context_complete: request.context_complete,
        working_files: request.working_files.as_ref(),
    };
    let protocol::NativeOutcome {
        receipt,
        code: exit,
        authority,
    } = protocol::evaluate_native_guarded(
        root,
        &serde_json::to_vec(&normalized).ok()?,
        request.input,
        native,
        prepared,
        &|stage| log.event("hook_stage", || json!({"stage":stage})),
    );
    log.event("hook_evaluation", || json!({"exit":exit,"decision":receipt["decision"],"disposition":receipt["disposition"],"code":receipt["code"],"ask_fallback":receipt["ask_fallback"],"policy_revision":receipt["identity"]["policy_revision"],"history_status":receipt["history"]["status"]}));
    if request.event.post() {
        return (exit == 0 && receipt["history"]["status"] != "failed").then(|| {
            let reply = Reply {
                decision: Decision::Allow,
                reason: String::new(),
                agent_context: None,
            };
            NativeDecision {
                reply,
                refusal: Value::Null,
                authority,
            }
        });
    }
    // What in-session approval needs from a refusal; failures carry none of it.
    let refusal = if exit == 0 {
        approval::refusal(&receipt)
    } else {
        Value::Null
    };
    if exit != 0 {
        let code = receipt["code"].as_str().unwrap_or("protection-failure");
        let reply = if code == "policy-files-invalid" {
            feedback::invalid_policy_files(
                adapter,
                receipt["diagnostics"]["message"]
                    .as_str()
                    .unwrap_or_default(),
            )
        } else {
            feedback::deny(code)
        };
        return Some(NativeDecision {
            reply,
            refusal,
            authority,
        });
    }
    feedback::respond(
        root,
        adapter,
        request.event,
        &receipt,
        &decided,
        request.session,
    )
    .map(|reply| NativeDecision {
        reply,
        refusal,
        authority,
    })
}
