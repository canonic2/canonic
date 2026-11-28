//! Deferred public HTTP calls keep ordinary admission and independent settlement.
use super::*;

pub(in crate::browser::cdp::session) struct PendingFetch {
    command: Command,
    context: Reference,
    request_id: Uuid,
    disposed: Arc<std::sync::atomic::AtomicBool>,
}
impl Worker {
    pub(in crate::browser::cdp::session) async fn http_running(
        &mut self,
        id: Uuid,
        deadline: Instant,
    ) -> Result<(), Error> {
        self.observation_ready(id, deadline)?;
        self.journal
            .record(json!({"operationId":id,"outcome":"running","settled":false}))
            .await?;
        self.observation_ready(id, deadline)?;
        {
            let mut state = self.shared.state.lock().expect("Chrome lifecycle");
            state
                .operations
                .get_mut(&id)
                .ok_or("UNKNOWN_OPERATION")?
                .outcome = Outcome::Running;
        }
        self.shared.changed.notify_waiters();
        Ok(())
    }
    pub(in crate::browser::cdp::session) async fn http_begin(&mut self, mut command: Command) {
        let id = command.id;
        let result = async {
            self.http_running(id, command.deadline)
                .await
                .map_err(|mut error| {
                    if error.code == "CONDITION_TIMEOUT" {
                        error.code = "NOT_DISPATCHED";
                    }
                    error
                })?;
            let (http_context, request) = match command.request.take() {
                Some(Request::HttpFetch {
                    http_context,
                    request,
                }) => (http_context, request),
                Some(Request::HttpFetchRequest {
                    http_context,
                    source_request_id,
                    options,
                }) => (
                    http_context,
                    self.http_request_input(source_request_id, options)?,
                ),
                _ => return Err(Error::from("INVALID_SCOPE")),
            };
            if let Some(context) = self.http_contexts.associated_context(http_context)? {
                self.index
                    .native(context, Kind::Context)
                    .map_err(resource_error)?;
            }
            let request = self.prepare_http(id, request, command.deadline).await?;
            if let Some(context) = self.http_contexts.associated_context(http_context)? {
                self.index
                    .native(context, Kind::Context)
                    .map_err(resource_error)?;
            }
            let request_id = self
                .http_contexts
                .start(http_context, request, command.deadline)?;
            Ok((http_context, request_id))
        }
        .await;
        match result {
            Ok((context, request_id)) => {
                let disposed = self
                    .http_contexts
                    .disposal_watch(request_id)
                    .expect("Owned HTTP disposal watch");
                self.http_requests.insert(
                    id,
                    PendingFetch {
                        command,
                        context,
                        request_id,
                        disposed,
                    },
                );
                self.http_poll().await;
            }
            Err(error) => self.finish(id, Err(error)).await,
        }
    }
    pub(in crate::browser::cdp::session) async fn http_poll(&mut self) {
        let owners = self.http_requests.keys().copied().collect::<Vec<_>>();
        for id in owners {
            let pending = &self.http_requests[&id];
            let context = pending.context;
            let request_id = pending.request_id;
            if pending.disposed.load(std::sync::atomic::Ordering::Acquire) {
                let mut error = crate::browser::http::pending(context, request_id);
                error.code = "HTTP_CONTEXT_DISPOSED";
                let _pending = self.http_requests.remove(&id).expect("Owned HTTP request");
                self.finish(id, Err(error)).await;
                continue;
            }
            let result = match self.http_finalize(request_id) {
                Err(error) if error.code == "HTTP_RESPONSE_PENDING" => {
                    let cancelled = self
                        .shared
                        .state
                        .lock()
                        .expect("Chrome lifecycle")
                        .operations
                        .get(&id)
                        .is_some_and(|operation| operation.cancel_requested);
                    if !cancelled
                        && self
                            .http_contexts
                            .deadline(request_id)
                            .is_none_or(|deadline| Instant::now() < deadline)
                    {
                        continue;
                    }
                    if cancelled {
                        self.http_contexts.cancel_request(request_id);
                    }
                    let mut error = crate::browser::http::pending(context, request_id);
                    if cancelled {
                        error.code = "CANCELLED";
                    }
                    Err(error)
                }
                result => result,
            };
            // Hold the original ordinary-capacity permit through settlement.
            let _pending = self.http_requests.remove(&id).expect("Owned HTTP request");
            self.finish(id, result).await;
        }
    }
    pub(in crate::browser::cdp::session) async fn http_cancel_owned(&mut self) {
        let owned = std::mem::take(&mut self.http_requests);
        for (id, pending) in owned {
            self.http_contexts.cancel_request(pending.request_id);
            let mut error = crate::browser::http::pending(pending.context, pending.request_id);
            error.code = "CANCELLED";
            self.finish(id, Err(error)).await;
            drop(pending.command);
        }
    }
}
