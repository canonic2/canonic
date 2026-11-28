//! Bounded target jobs whose cancellation settles only their owning runtime.
use super::{Protocol, presentation, rpc_error};
use crate::{
    error::{Error, Result},
    managed::Client,
    qa::{ReadContext, scheduling::Runtimes},
};
use futures_util::{FutureExt, StreamExt, future::LocalBoxFuture, stream::FuturesUnordered};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

/// A job either answers its request or, for a settled run barrier, returns the
/// request for ordered metadata finalization on the root server.
enum Reply {
    Response(Option<Value>),
    Finalize(Value),
}
type Completion = (u64, Value, Result<Reply>, Arc<Runtimes>);
#[derive(Default)]
pub(super) struct Jobs {
    jobs: FuturesUnordered<LocalBoxFuture<'static, Completion>>,
    requests: HashMap<String, (u64, tokio::sync::watch::Sender<bool>)>,
    generation: u64,
    finalize: Vec<Value>,
}
impl Jobs {
    pub(super) fn is_empty(&self) -> bool {
        self.jobs.is_empty()
    }
    pub(super) fn cancel(&mut self, id: &Value) -> bool {
        if let Some((_, stop)) = self.requests.remove(&id.to_string()) {
            stop.send_replace(true);
            true
        } else {
            false
        }
    }
    #[expect(
        clippy::too_many_lines,
        reason = "One admission transaction binds target ownership, cancellation and the response identity"
    )]
    pub(super) fn admit(
        &mut self,
        protocol: &Protocol,
        client: &Client,
        targets: &Arc<Runtimes>,
        request: &Value,
        admitted: Instant,
    ) -> Option<Option<Value>> {
        if !protocol.accepts(request)
            || request["jsonrpc"] != "2.0"
            || request["method"] != "tools/call"
            || !request
                .get("id")
                .is_some_and(|id| id.is_number() || id.is_string())
        {
            return None;
        }
        let id = request["id"].clone();
        let name = request["params"]["name"].as_str()?;
        // Session-level operations are internal; MCP reaches only published tools.
        super::tools().get(name)?;
        let args = request["params"]
            .get("arguments")
            .cloned()
            .unwrap_or(json!({}));
        if name == "run.end" {
            return self.admit_barrier(targets, request, &args, admitted);
        }
        if !targets.handles(name, &args) {
            return None;
        }
        let definition = super::definition(name)?;
        if super::validate_arguments(name, &definition["inputSchema"], &args).is_err() {
            return None;
        }
        if self.jobs.len() >= 64 {
            return Some(Some(rpc_error(
                id,
                -32000,
                "MCP target work capacity is full",
            )));
        }
        let read = ReadContext::supports(name)
            .then(|| targets.read(&args))
            .flatten();
        let prepared = if read.is_none() {
            Some(targets.prepare(name, &args, client)?)
        } else {
            None
        };
        self.generation += 1;
        let generation = self.generation;
        let (stop, mut cancelled) = tokio::sync::watch::channel(false);
        let mut protocol = protocol.clone();
        let request = request.clone();
        let name = name.to_owned();
        let targets = targets.clone();
        let completion_targets = targets.clone();
        let response_id = id.clone();
        let future = async move {
            let id = response_id;
            let response = if let Some(read) = read {
                let result = match read {
                    Err(error) => Err(error),
                    Ok(context) => {
                        let definition = super::definition(&name)
                            .ok_or_else(|| Error::invalid("Unknown read tool"))?;
                        super::validate_arguments(&name, &definition["inputSchema"], &args)?;
                        tokio::select! {
                            biased;
                            _=cancelled.changed()=>return Ok(None),
                            result=context.call(&name,args)=>result,
                        }
                    }
                };
                Some(
                    json!({"jsonrpc":"2.0","id":id,"result":presentation::tool_result(&name,result,admitted.elapsed())}),
                )
            } else {
                match prepared.expect("Mutation has admission") {
                    Err(error) => Some(
                        json!({"jsonrpc":"2.0","id":id,"result":presentation::tool_result(&name,Err(error),admitted.elapsed())}),
                    ),
                    Ok(mut prepared) => {
                        tokio::select! {biased; _=cancelled.changed()=>return Ok(None), ()=prepared.ticket.ready()=>{}}
                        // A predecessor may have failed cancellation cleanup after
                        // this request was admitted. Recheck before any dispatch.
                        if let Err(error) = targets.target_ready(&prepared.target, &name) {
                            return Ok(Some(
                                json!({"jsonrpc":"2.0","id":id,"result":presentation::tool_result(&name,Err(error),admitted.elapsed())}),
                            ));
                        }
                        let mut server = prepared.server.lock().await;
                        targets.set_idle(&prepared.target, false);
                        targets.populate(&mut server);
                        let existing = server.session_ids();
                        let resources = prepared
                            .resource_slot
                            .as_ref()
                            .map(|(kind, _)| server.resource_ids(*kind))
                            .unwrap_or_default();
                        targets.fence_close(&name, &args);
                        let mut response = {
                            let operation =
                                protocol.admitted_request(&mut server, request, admitted);
                            tokio::pin!(operation);
                            tokio::select! {biased; _=cancelled.changed()=>None, response=&mut operation=>response}
                        };
                        server.retain_session_slot(&existing, prepared.session_slot.take());
                        targets.retain_resource(
                            &prepared.target,
                            &server,
                            &resources,
                            prepared.resource_slot.take(),
                        );
                        let mut failures = Vec::new();
                        if *cancelled.borrow()
                            && let Err(error) = server.cancel_dispatched().await
                        {
                            failures.push(error);
                        }
                        if name == "session_open"
                            && (*cancelled.borrow()
                                || response.as_ref().is_some_and(|response| {
                                    response["result"]["structuredContent"]["ok"] == false
                                }))
                            && let Err(error) = server.cancel_startup().await
                        {
                            failures.push(error);
                        }
                        let idle = match server.retire_runtime().await {
                            Ok(idle) => idle,
                            Err(error) => {
                                failures.push(error);
                                false
                            }
                        };
                        targets.publish(&prepared.target, &server);
                        if !failures.is_empty() {
                            let operation = response
                                .as_ref()
                                .map(|reply| reply["result"]["structuredContent"].clone());
                            let error = Error::new(
                                "CLEANUP_FAILED",
                                "Target runtime cleanup is unfinished",
                            )
                            .details(json!({"failures":failures,"operation":operation}));
                            targets.failed(&prepared.target, error.clone());
                            if operation.is_some_and(|result| result["ok"] == true)
                                && let Some(reply) = response.as_mut()
                            {
                                reply["result"] = presentation::tool_result(
                                    &name,
                                    Err(error),
                                    admitted.elapsed(),
                                );
                            }
                        }
                        targets.set_idle(&prepared.target, idle && failures.is_empty());
                        response
                    }
                }
            };
            Ok(response)
        };
        self.push(
            generation,
            id,
            stop,
            completion_targets,
            future.map(|result| result.map(Reply::Response)),
        );
        Some(None)
    }
    /// Runs a `run.end` cleanup barrier beside other target work. Unknown or
    /// already settled runs stay on the ordered root path.
    fn admit_barrier(
        &mut self,
        targets: &Arc<Runtimes>,
        request: &Value,
        args: &Value,
        admitted: Instant,
    ) -> Option<Option<Value>> {
        let run = args["runId"].as_str()?.to_owned();
        let definition = super::definition("run.end")?;
        if super::validate_arguments("run.end", &definition["inputSchema"], args).is_err()
            || !targets.barrier_pending(&run)
        {
            return None;
        }
        let id = request["id"].clone();
        if self.jobs.len() >= 64 {
            return Some(Some(rpc_error(
                id,
                -32000,
                "MCP target work capacity is full",
            )));
        }
        self.generation += 1;
        let generation = self.generation;
        let (stop, mut cancelled) = tokio::sync::watch::channel(false);
        let barrier = targets.clone();
        let request = request.clone();
        let response_id = id.clone();
        let future = async move {
            let result = tokio::select! {
                biased;
                _ = cancelled.changed() => return Ok(Reply::Response(None)),
                result = barrier.finish_run(&run) => result,
            };
            let Err(error) = result else {
                return Ok(Reply::Finalize(request));
            };
            Ok(Reply::Response(Some(
                json!({"jsonrpc":"2.0","id":response_id,"result":presentation::tool_result("run.end",Err(error),admitted.elapsed())}),
            )))
        };
        self.push(generation, id, stop, targets.clone(), future);
        Some(None)
    }
    fn push(
        &mut self,
        generation: u64,
        id: Value,
        stop: tokio::sync::watch::Sender<bool>,
        targets: Arc<Runtimes>,
        future: impl std::future::Future<Output = Result<Reply>> + 'static,
    ) {
        let key = id.to_string();
        self.jobs.push(Box::pin(async move {
            (
                generation,
                id,
                std::panic::AssertUnwindSafe(future)
                    .catch_unwind()
                    .await
                    .unwrap_or_else(|_| {
                        Err(Error::new("INTERNAL", "Target runtime request stopped"))
                    }),
                targets,
            )
        }));
        self.requests.insert(key, (generation, stop));
    }
    /// Requests whose run barrier settled and now need root finalization.
    pub(super) fn take_finalize(&mut self) -> Vec<Value> {
        std::mem::take(&mut self.finalize)
    }
    pub(super) async fn next(&mut self) -> Result<Option<Value>> {
        if let Some((generation, id, result, targets)) = self.jobs.next().await {
            targets.retire_idle();
            let current = self
                .requests
                .get(&id.to_string())
                .is_some_and(|(current, _)| *current == generation);
            if current {
                self.requests.remove(&id.to_string());
            }
            match result? {
                Reply::Response(response) if current => return Ok(response),
                Reply::Finalize(request) if current => self.finalize.push(request),
                Reply::Response(_) | Reply::Finalize(_) => {}
            }
        }
        Ok(None)
    }
    pub(super) async fn finish(&mut self) -> Result<()> {
        let grace = tokio::time::sleep(Duration::from_millis(250));
        tokio::pin!(grace);
        while !self.is_empty() {
            tokio::select! {result=self.next()=>{result?;},()=&mut grace=>break,}
        }
        for (_, stop) in self.requests.values() {
            stop.send_replace(true);
        }
        while !self.is_empty() {
            self.next().await?;
        }
        Ok(())
    }
}
