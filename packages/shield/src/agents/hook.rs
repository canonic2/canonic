//! Shared bounded hook orchestration; adapters alone interpret and encode vendors.
use super::{AgentAdapter, Decision, Event, Reply, Request, feedback, find};
#[cfg(test)]
use crate::runtime::Project;
use crate::{cli::NativeHook, enforcement::protocol};
use serde_json::{Value, json};
use std::{io::Read, path::Path, process::ExitCode};

#[cfg(test)]
mod tests;

pub(crate) fn run(input: &NativeHook, log: &crate::diagnostics::Log) -> ExitCode {
    let Some(adapter) = find(&input.agent) else {
        return ExitCode::from(2);
    };
    let Some(expected) = adapter.event(&input.event) else {
        return ExitCode::from(2);
    };
    log.event(
        "hook_start",
        json!({"project":input.project,"agent":input.agent}),
    );
    let (bytes, oversized) = read_input(&mut std::io::stdin().lock());
    let read_ok = bytes.is_ok();
    let bytes = bytes.unwrap_or_default();
    log.hook_input(&input.agent, &bytes, read_ok, oversized.is_some());
    let value = if !read_ok || oversized.is_some() {
        if read_ok && expected.post() && oversized.as_ref().is_some_and(|o| !o.names_pre) {
            json!({})
        } else {
            adapter.reply(expected, &feedback::deny("protection-input-failure"))
        }
    } else {
        translate_event(&input.project, adapter, &bytes, expected, log)
    };
    log.hook_output(&value);
    println!("{value}");
    ExitCode::SUCCESS
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

fn translate_event(
    root: &Path,
    adapter: &dyn AgentAdapter,
    bytes: &[u8],
    event: Event,
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
    let actual = adapter.input_event(&host);
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
    let Some(mut request) = adapter.request(&host) else {
        return if event.post() {
            json!({})
        } else {
            deny("invalid-host-input")
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
    log.event("hook_stage", json!({"stage":"project_open_start"}));
    let context = crate::review::open(root).ok()?;
    log.event("hook_stage", json!({"stage":"project_open_ready"}));
    log.event("hook_stage", json!({"stage":"registration_ready"}));
    if let Some(name) = request.tool.strip_prefix("mcp__shield__")
        && crate::review::mcp::tools_for(Some(adapter.spec().id))["tools"]
            .as_array()?
            .iter()
            .any(|t| t["name"] == name)
    {
        return Some(shield_tool_reply(adapter, request, name, log));
    }
    if request.operation.is_none() && request.harmless && request.event == Event::Permission {
        return Some(allow(adapter, request.event, String::new()));
    }
    let Some(operation) = &request.operation else {
        log.event(
            "hook_internal_bypass",
            json!({"tool_name":request.tool,"post":request.event.post()}),
        );
        return Some(json!({}));
    };
    let mut prepared = if operation["kind"] == "mcp" {
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
    let local_id = if request.event.post() {
        request.tool_id.map_or_else(invocation_id, str::to_owned)
    } else {
        invocation_id()
    };
    let mut strictest: Option<Reply> = None;
    let mut unmodeled = false;
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
        let reply = decide(
            adapter,
            request,
            &context,
            &id,
            operation,
            prepared.take(),
            log,
        )?;
        if strictest
            .as_ref()
            .is_none_or(|r| reply.decision > r.decision)
        {
            strictest = Some(reply);
        }
    }
    if request.event.post() {
        let note = if unmodeled {
            None
        } else {
            request
                .tool_id
                .and_then(|id| feedback::approval_note(root, adapter, request.session, id))
        };
        Some(adapter.completion(request.event_name, note.as_deref()))
    } else {
        strictest.map(|r| adapter.reply(request.event, &r))
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
        json!({"tool_name":request.tool,"post":request.event.post()}),
    );
    if request.event.post() {
        return adapter.completion(request.event_name, None);
    }
    if name == "policy_apply" {
        // Neither native Codex event may preapprove policy activation; its
        // configured tool prompt decides. Claude's MCP form confirms the candidate.
        return if adapter.spec().id == "codex" {
            json!({})
        } else {
            allow(adapter, request.event, String::new())
        };
    }
    allow(
        adapter,
        request.event,
        if adapter.spec().id == "claude" {
            String::new()
        } else {
            "Shield inspection and proposal tools do not activate policy; policy_apply requires user approval.".into()
        },
    )
}

fn inactive_mcp(adapter: &dyn AgentAdapter, request: &Request<'_>) -> Value {
    if request.event.post() {
        json!({})
    } else {
        adapter.reply(request.event, &feedback::deny("mcp-binding-inactive"))
    }
}

enum McpBinding {
    Active,
    Inactive,
    External,
}

fn mcp_binding(snapshot: &crate::runtime::Snapshot, agent: &str, alias: &str) -> McpBinding {
    if snapshot.mcp_connection(agent, alias).is_some() {
        McpBinding::Active
    } else if snapshot
        .mcp_connections()
        .values()
        .any(|c| c.agents.get(agent).is_some_and(|a| a == alias))
    {
        McpBinding::Inactive
    } else {
        McpBinding::External
    }
}

fn prepare_mcp(
    root: &Path,
    adapter: &dyn AgentAdapter,
    request: &Request<'_>,
    alias: &str,
    log: &crate::diagnostics::Log,
) -> Result<protocol::Prepared, Value> {
    let prepared = protocol::Prepared::load(root, &|stage| log.event("hook_stage",json!({"stage":stage})))
        .map_err(|_| {
            if request.event.post() {
                return json!({"systemMessage":"Shield completion history unavailable; MCP bindings could not be checked."});
            }
            let mut reply = feedback::deny("mcp-bindings-unavailable");
            reply.reason = "Shield: blocked. MCP bindings could not be checked. Run `shield doctor` in your terminal before retrying.".into();
            reply.agent_context = Some("Shield could not determine whether this MCP server is bound to project policy. Authorization fails closed until the project snapshot is available; ask the user to run `shield doctor`.".into());
            adapter.reply(request.event, &reply)
        })?;
    match mcp_binding(prepared.snapshot(), adapter.spec().id, alias) {
        McpBinding::Active => Ok(prepared),
        McpBinding::External => Err(json!({})),
        McpBinding::Inactive => Err(inactive_mcp(adapter, request)),
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

fn decide(
    adapter: &dyn AgentAdapter,
    request: &Request<'_>,
    context: &crate::review::Context,
    id: &str,
    mut operation: Value,
    prepared: Option<protocol::Prepared>,
    log: &crate::diagnostics::Log,
) -> Option<Reply> {
    let root = Path::new(&context.project_root);
    let decided = operation.clone();
    if request.event.post() {
        operation = json!({"kind":"completion","delivery_id":format!("{id}-{}",request.event_name),"request_id":null,
            "status":if request.event==Event::Failed {"failed"} else if operation["kind"]=="shell" {"unknown"} else {"completed"},"operation":operation});
    }
    let normalized = json!({"protocol_version":1,"request_id":id,"expected_project_id":context.project_id,"cwd":request.cwd,"operation":operation,
        "host":{"agent":adapter.spec().history_agent,"session_id":request.session,"tool_use_id":request.tool_id,"event":request.event_name,"consumer_id":null},"approval_request_id":null});
    let native = protocol::NativeContext {
        integration: adapter.spec().id,
        approval: adapter.approval(request.event),
        context_complete: request.context_complete,
        working_files: request.working_files.as_ref(),
    };
    let (receipt, exit) = protocol::evaluate_native(
        root,
        &serde_json::to_vec(&normalized).ok()?,
        request.input,
        native,
        prepared,
        &|stage| log.event("hook_stage", json!({"stage":stage})),
    );
    log.event("hook_evaluation",json!({"exit":exit,"decision":receipt["decision"],"disposition":receipt["disposition"],"code":receipt["code"],"ask_fallback":receipt["ask_fallback"],"policy_revision":receipt["identity"]["policy_revision"],"history_status":receipt["history"]["status"]}));
    if request.event.post() {
        return (exit == 0 && receipt["history"]["status"] != "failed").then(|| Reply {
            decision: Decision::Allow,
            reason: String::new(),
            agent_context: None,
        });
    }
    if exit != 0 {
        let code = receipt["code"].as_str().unwrap_or("protection-failure");
        let mut reply = feedback::deny(code);
        if code == "compiled-cache-unavailable"
            && let Some(route) = crate::agent_message::cache_repair(&context.project_root)
        {
            reply.reason = format!(
                "Shield: blocked. The policy cache is unavailable. Run `{route}` in your terminal before retrying."
            );
            reply.agent_context = Some(format!(
                "Shield: compiled policy is unavailable. Ask the user to run `{route}` in their terminal, then retry."
            ));
        }
        return Some(reply);
    }
    feedback::respond(
        root,
        adapter,
        request.event,
        &receipt,
        &decided,
        request.session,
    )
}
