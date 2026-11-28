//! Read-only MCP requests that remain independent of the active QA mutation.
use super::{Protocol, presentation};
use crate::{error::Error, managed::Client, qa::DiscoveryReader};
use futures_util::{
    FutureExt, StreamExt,
    future::{AbortHandle, Abortable, LocalBoxFuture},
    stream::FuturesUnordered,
};
use serde_json::{Value, json};
use std::time::Instant;

#[cfg(test)]
mod tests;

#[derive(Default)]
pub(super) struct Reads {
    jobs: FuturesUnordered<LocalBoxFuture<'static, ReadOutcome>>,
    requests: std::collections::HashMap<String, (u64, AbortHandle)>,
    generation: u64,
}
type ReadOutcome = (u64, Value, crate::error::Result<Option<Value>>);
impl Reads {
    pub(super) fn admit_target_status(
        &mut self,
        protocol: &Protocol,
        targets: &std::sync::Arc<crate::qa::scheduling::Runtimes>,
        request: &Value,
    ) -> Option<Value> {
        self.spawn(request, protocol.target_status(request, targets)?, 8)
    }
    pub(super) fn is_empty(&self) -> bool {
        self.jobs.is_empty()
    }
    pub(super) fn cancel(&mut self, id: &Value) -> bool {
        if let Some((_, task)) = self.requests.remove(&id.to_string()) {
            task.abort();
            true
        } else {
            false
        }
    }
    pub(super) fn admit(
        &mut self,
        protocol: &Protocol,
        client: &Client,
        request: &Value,
    ) -> Option<Value> {
        let read = protocol.responsive_request(request, client)?;
        self.spawn(request, read, 8)
    }
    pub(super) fn admit_long(
        &mut self,
        protocol: &Protocol,
        reader: &DiscoveryReader,
        client: &Client,
        request: &Value,
        admitted: Instant,
    ) -> Option<Value> {
        let read = protocol.independent_request(request, reader, client, admitted)?;
        self.spawn(request, read, 4)
    }
    /// Serves a skill file without the runtime, the manager or a target.
    pub(super) fn admit_bundled(
        &mut self,
        id: Value,
        read: canonic_mcp_kit::BundledRead,
    ) -> Option<Value> {
        let request = json!({"id":id});
        self.spawn(
            &request,
            Box::pin(async move { super::bundled(id, &read) }),
            8,
        )
    }
    fn spawn(
        &mut self,
        request: &Value,
        read: LocalBoxFuture<'static, Value>,
        capacity: usize,
    ) -> Option<Value> {
        let id = request["id"].clone();
        if self.jobs.len() >= capacity {
            return Some(super::rpc_error(id, -32000, "MCP read capacity is full"));
        }
        let key = id.to_string();
        self.generation += 1;
        let generation = self.generation;
        let (task, registration) = AbortHandle::new_pair();
        self.jobs.push(Box::pin(async move {
            (
                generation,
                id,
                std::panic::AssertUnwindSafe(Abortable::new(read, registration))
                    .catch_unwind()
                    .await
                    .map(|result| result.ok())
                    .map_err(|_| Error::new("INTERNAL", "Responsive MCP read task stopped")),
            )
        }));
        self.requests.insert(key, (generation, task));
        None
    }
    pub(super) async fn next(&mut self) -> crate::error::Result<Option<Value>> {
        match self.jobs.next().await {
            Some((generation, id, response)) => {
                if self
                    .requests
                    .get(&id.to_string())
                    .is_some_and(|(owner, _)| *owner == generation)
                {
                    self.requests.remove(&id.to_string());
                    response
                } else {
                    Ok(None)
                }
            }
            None => Ok(None),
        }
    }
}

impl Protocol {
    pub(super) fn target_status(
        &self,
        request: &Value,
        targets: &std::sync::Arc<crate::qa::scheduling::Runtimes>,
    ) -> Option<LocalBoxFuture<'static, Value>> {
        let id = request.get("id")?.clone();
        if !self.accepts(request)
            || request["jsonrpc"] != "2.0"
            || request["method"] != "tools/call"
            || (!id.is_number() && !id.is_string())
        {
            return None;
        }
        let name = request["params"]["name"].as_str()?;
        if name != "session_status" && name != "command_status" {
            return None;
        }
        let args = request["params"]
            .get("arguments")
            .cloned()
            .unwrap_or(json!({}));
        let snapshot = targets.status(&args)?;
        let context = if name == "session_status" {
            targets.read(&args).and_then(Result::ok)
        } else {
            None
        };
        let name = name.to_owned();
        Some(Box::pin(async move {
            let started = Instant::now();
            let definition = super::definition(&name);
            let result = match definition {
                Some(definition) => {
                    match crate::schema::validate(&definition["inputSchema"], &args) {
                        Err(error) => Err(error),
                        Ok(()) => match context {
                            Some(context) => context.status().await,
                            None => snapshot,
                        },
                    }
                }
                None => Err(Error::invalid("Unknown status tool")),
            };
            json!({"jsonrpc":"2.0","id":id,"result":presentation::tool_result(&name,result,started.elapsed())})
        }))
    }
    pub(super) fn independent_request(
        &self,
        request: &Value,
        reader: &DiscoveryReader,
        client: &Client,
        admitted: Instant,
    ) -> Option<LocalBoxFuture<'static, Value>> {
        let id = request.get("id")?.clone();
        if !self.accepts(request)
            || request["jsonrpc"] != "2.0"
            || (!id.is_string() && !id.is_number())
            || request["method"] != "tools/call"
        {
            return None;
        }
        let name = request["params"]["name"].as_str()?;
        // Session reads retain their resource owners behind the controller;
        // dropping this future abandons only its request socket.
        let managed = crate::managed::independent_read(name, &request["params"]["arguments"]);
        if !managed && !matches!(name, "target_list" | "doctor" | "host_info") {
            return None;
        }
        let name = name.to_owned();
        let arguments = request["params"]
            .get("arguments")
            .cloned()
            .unwrap_or(json!({}));
        let reader = reader.clone();
        let mut client = client.clone();
        Some(Box::pin(async move {
            client.media_deadline = super::requested_deadline(&name, &arguments);
            let definition = super::definition(&name).expect("Registered independent read");
            let result = if let Err(error) =
                crate::schema::validate(&definition["inputSchema"], &arguments)
            {
                Err(error)
            } else if managed {
                client.call(&name, arguments).await
            } else if name == "target_list" {
                match reader.call(&name, arguments).await {
                    Ok(list) => Ok(client.with_held_capabilities(list).await),
                    error => error,
                }
            } else {
                reader.call(&name, arguments).await
            };
            json!({"jsonrpc":"2.0","id":id,"result":presentation::tool_result(&name, result, admitted.elapsed())})
        }))
    }
    /// Scheduling accepts only tool calls already admitted by Session.
    pub(super) fn responsive_request(
        &self,
        request: &Value,
        client: &Client,
    ) -> Option<LocalBoxFuture<'static, Value>> {
        if !self.accepts(request) || request["method"] != "tools/call" {
            return None;
        }
        let name = request["params"]["name"].as_str()?;
        if crate::builds::config_handles(name) {
            let id = request["id"].clone();
            let name = name.to_owned();
            let arguments = request["params"]["arguments"].clone();
            return Some(Box::pin(async move {
                let started = Instant::now();
                let result = crate::builds::config(&name, &arguments);
                json!({"jsonrpc":"2.0","id":id,"result":presentation::tool_result(&name,result,started.elapsed())})
            }));
        }
        if crate::documentation::handles(name) {
            let id = request["id"].clone();
            let name = name.to_owned();
            let arguments = request["params"]
                .get("arguments")
                .cloned()
                .unwrap_or(json!({}));
            return Some(Box::pin(async move {
                let started = Instant::now();
                let result = crate::documentation::call(&name, &arguments, None).await;
                json!({"jsonrpc":"2.0","id":id,"result":presentation::tool_result(&name,result,started.elapsed())})
            }));
        }
        if !crate::diagnostics::handles(name)
            && !matches!(
                name,
                "status"
                    | "build_start"
                    | "build_status"
                    | "build_cancel"
                    | "build_log"
                    | "project_status"
                    | "project_start"
                    | "project_open"
                    | "browser_status"
                    | "browser_cancel"
                    | "browser_send"
                    | "browser_connect"
                    | "browser_disconnect"
            )
            && !crate::browser::api::selected(name, &request["params"]["arguments"])
        {
            return None;
        }
        let id = request["id"].clone();
        let name = name.to_owned();
        let arguments = request["params"]["arguments"].clone();
        let mut client = client.clone();
        client.request_id = Some(format!(
            "mcp-{}",
            crate::managed::request_key(&self.connection_id, &id)
        ));
        Some(Box::pin(async move {
            let started = Instant::now();
            let result = client.call(&name, arguments).await;
            json!({"jsonrpc":"2.0","id":id,"result":presentation::tool_result(&name, result, started.elapsed())})
        }))
    }
}

/// The reply when `name` outlives its `limit`. The request already reached the
/// owner, so a timed-out `browser_connect` may still start a browser; another
/// connect would start a second one.
pub(super) fn timed_out(name: &str, limit: u64) -> Error {
    if matches!(name, "browser_connect" | "connect") {
        Error::new(
            "OUTCOME_UNKNOWN",
            format!("browser_connect exceeded {limit} seconds and a browser may still be starting; list the connected browsers with browser_status before connecting again"),
        )
        .details(json!({"nextAction":"browser_status"}))
    } else if matches!(name, "control_acquire" | "control_release") {
        Error::new("OUTCOME_UNKNOWN", format!("{name} exceeded {limit} seconds after owner submission; inspect control_status before requesting another handoff"))
            .details(json!({"nextAction":"control_status"}))
    } else if matches!(
        name,
        "act" | "observe" | "cancel" | "browser_send" | "browser_cancel"
    ) {
        Error::new("OUTCOME_UNKNOWN", format!("{name} exceeded {limit} seconds after owner submission; inspect browser_status before submitting again"))
            .details(json!({"nextAction":"browser_status"}))
    } else {
        Error::new("TIMEOUT", format!("{name} exceeded {limit} seconds"))
    }
}
