//! Shield's MCP server for enrolled projects: the tool catalog, connection
//! identity, review dispatch and host confirmation of policy changes, served by
//! canonic-mcp-kit's stdio driver.

mod confirmation;
mod elicitation;
mod frame_log;
pub(crate) mod lifecycle;
mod messages;
pub(crate) mod onboarding;
mod progress;
mod prompts;
mod resources;
mod response;
mod skills;
mod tasks;
#[cfg(test)]
pub(crate) mod tests;

use canonic_mcp_kit::{
    Annotations, Catalog, CatalogError, Client, Decision, RpcError, ServerInfo, SessionOptions,
    Tool, ToolCall, ToolResult, rpc,
    schema::ArgumentError,
    stdio::{self, Confirm, Direction, Outcome, Request},
};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, RwLock},
};

pub(crate) const MAX_MESSAGE: usize = 524_288;
pub(crate) const MAX_OUTPUT: usize = 4_194_304;
pub(crate) const MAX_MESSAGES: u64 = 1024;
/// Tool calls one connection may have queued or running. Calls run one at a
/// time in arrival order; a host sending a batch of parallel calls waits for
/// its turn instead of being refused. Each waiting call holds at most one
/// [`MAX_MESSAGE`] frame.
pub(crate) const MAX_QUEUED_CALLS: usize = 64;

/// One enrolled project connection. A pristine diagnostic connection can adopt
/// its first runtime epoch; retained authority and later epoch changes cannot.
pub(crate) struct Server {
    review: RwLock<Arc<crate::review::Context>>,
    connection: Arc<crate::review::mcp::Connection>,
    confirmations: Mutex<confirmation::Confirmations>,
    policy_waits: PolicyWaits,
    questions: Arc<Mutex<elicitation::Questions>>,
    log: Option<Arc<crate::diagnostics::Log>>,
    preparations: Arc<crate::review::preparation_tasks::Runtime>,
    task_schema: std::sync::OnceLock<canonic_mcp_kit::ResultSchema>,
}

impl Server {
    fn non_tool(&self, request: &Request, context: &stdio::Context) -> Outcome {
        match request {
            Request::Task(request) => tasks::manage(self, request, context),
            Request::Prompt(call) => prompts::render(call),
            Request::Method { method, params } if method == "resources/read" => {
                resources::read(&self.review_for_request(context), params, context)
            }
            _ => Outcome::Method(Err(RpcError::new(
                rpc::METHOD_NOT_FOUND,
                "Method not found",
            ))),
        }
    }
    pub(crate) fn for_review(
        mut context: crate::review::Context,
    ) -> Result<Self, crate::review::Error> {
        crate::review::pack_updates::enabled()?;
        let tick = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        context.surface = "mcp".into();
        context.progress = crate::review::progress::Progress::default();
        context.connection_id = Some(format!(
            "connection-{}",
            crate::policy::fingerprint(
                format!("{}:{}:{tick}", context.project_id, std::process::id()).as_bytes()
            )
        ));
        let connection = Arc::new(crate::review::mcp::Connection::enrolled(&context)?);
        let preparations = Arc::new(crate::review::preparation_tasks::Runtime::new(&context));
        Ok(Self {
            review: RwLock::new(Arc::new(context)),
            connection,
            confirmations: Mutex::new(confirmation::Confirmations::default()),
            policy_waits: PolicyWaits::default(),
            questions: Arc::new(Mutex::new(elicitation::Questions::default())),
            log: None,
            preparations,
            task_schema: std::sync::OnceLock::new(),
        })
    }

    pub(crate) fn review(&self) -> Arc<crate::review::Context> {
        self.review
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
    /// Check the host can review proposals before opening runtime state, then
    /// verify that the connection can use the project's current review state.
    fn check_runtime(
        &self,
        name: &str,
        args: &serde_json::Value,
    ) -> Result<(), crate::review::Error> {
        if !crate::review::mcp::needs_runtime(name, args) {
            return Ok(());
        }
        let before = self.review();
        if (crate::review::mcp::prepares_review(name) || name == "policy_review")
            && crate::agents::review::for_context(&before).confirmation()
                == crate::agents::review::Confirmation::Terminal
        {
            return Err(crate::review::Error::ApprovalRequired);
        }
        let current = crate::review::mcp::runtime_context(&before)?;
        if current.project_id != before.project_id {
            *self
                .review
                .write()
                .map_err(|_| crate::review::Error::Locked)? = Arc::new(current);
        }
        Ok(())
    }

    fn call_tool(&self, call: ToolCall, context: &stdio::Context) -> Outcome {
        let call = canonic_mcp_kit::tool::preview::normalize(crate::review::mcp::PREVIEWS, call);
        if let Err(error) = self.check_runtime(&call.name, &call.arguments) {
            return self.runtime_refusal(&call, context, &error);
        }
        let review = self.review_for_request(context);
        let input_form = elicitation::owns(self, context)
            || (context.request_context().is_none_or(|request| {
                request.request_state.is_none() && request.input_responses.is_none()
            }) && needs_input_form(&call));
        if !input_form && let Some(outcome) = self.resume_confirmation(&review, &call, context) {
            return outcome;
        }
        // NULs are refused before dispatch or confirmation, so no tool stores one.
        if crate::json::contains_nul(&call.arguments) {
            return nul_arguments(&review, &call);
        }
        if let Some(outcome) = self.maintain_setup(&review, &call, context) {
            return outcome;
        }
        if input_form {
            return elicitation::call(self, review, call, context);
        }
        if call.name == "policy_review" {
            let args = crate::review::mcp::review_arguments(
                &review,
                call.arguments["proposal_id"].as_str().unwrap_or_default(),
            );
            return match args {
                Ok(args) => self.policy_apply(
                    review,
                    ReviewedCall::with_action(call, "policy_apply", args),
                    context,
                ),
                Err(error) => Self::policy_apply_failure(&review, &call, &error),
            };
        }
        if call.name == "registry_add" && call.arguments["review_ref"].is_string() {
            return elicitation::use_registry_inputs(self, review, call, context);
        }
        if requests_review(&call) && crate::review::mcp::prepares_review(&call.name) {
            let operation = operation_call(&call);
            let operation = operation.as_ref().unwrap_or(&call);
            let value = crate::review::mcp::call_with_bound_confirmation(
                &review,
                &self.connection,
                &operation.name,
                &operation.arguments,
                &|| {
                    context
                        .check()
                        .map_err(|_| crate::policy::Error::Interrupted)
                },
                false,
                None,
            );
            return self.review_guided_result(review, call, value, context);
        }
        if let Some(outcome) = tasks::create(self, &review, &call, context) {
            return outcome;
        }
        self.dispatch_tool(review, call, context)
    }

    fn dispatch_tool(
        &self,
        review: Arc<crate::review::Context>,
        call: ToolCall,
        context: &stdio::Context,
    ) -> Outcome {
        let operation = operation_call(&call);
        match crate::review::mcp::admin_confirmation(
            &review,
            &self.connection,
            &call.name,
            &operation.as_ref().unwrap_or(&call).arguments,
            &|| {
                context
                    .check()
                    .map_err(|_| crate::policy::Error::Interrupted)
            },
        ) {
            Ok(Some((message, binding))) => {
                return self.confirm_operation(
                    review,
                    ReviewedCall {
                        request: call,
                        action: operation,
                    },
                    context,
                    message,
                    Some(binding),
                    None,
                );
            }
            Err(error) => {
                return Outcome::Tool(response::tool_result(
                    &call.name,
                    review.agent.as_deref(),
                    crate::review::mcp::admin_form_error(
                        &review,
                        &call.name,
                        &call.arguments,
                        &error,
                    ),
                    true,
                    &|| Ok(()),
                ));
            }
            Ok(None) => {}
        }
        if call.name == "request_approve" {
            return self.confirm_request(review, call, context);
        }
        if call.name == "request_deny"
            && let Ok(message) = crate::review::mcp::deny_confirmation(&review, &call.arguments)
        {
            return self.confirm_apply(review, call, context, message);
        }
        if matches!(call.name.as_str(), "session_grant_approve")
            && let Ok(Some(message)) =
                crate::review::session_grants::confirmation(&review, &call.arguments)
        {
            return self.confirm_apply(review, call, context, message);
        }
        // Connection edits always show their private settings in a form;
        // ordinary policy retains the adapter's existing approval mechanism.
        if call.name == "policy_apply" {
            return self.policy_apply(review, call.into(), context);
        }
        Outcome::Tool(reply(
            &review,
            &self.connection,
            operation.as_ref().unwrap_or(&call),
            context,
            Answer::None,
        ))
    }

    pub(crate) fn with_log(mut self, log: Arc<crate::diagnostics::Log>) -> Self {
        self.log = Some(log);
        self
    }

    fn review_for_request(&self, context: &stdio::Context) -> Arc<crate::review::Context> {
        let review = self.review();
        let progress = progress::for_request(context);
        if context.request_context().is_none() && progress.is_none() {
            return review;
        }
        let mut review = (*review).clone();
        review.progress = progress.unwrap_or_default();
        let Some(request) = context.request_context() else {
            return Arc::new(review);
        };
        if let Some(session) = review
            .agent
            .as_deref()
            .and_then(crate::agents::find)
            .and_then(|adapter| adapter.mcp_session(request.metadata().as_map()))
        {
            review.session_id = Some(session.into());
        }
        review.client = request
            .client_info
            .as_ref()
            .and_then(|info| info["name"].as_str())
            .map(str::to_owned);
        review.host_version = request
            .client_info
            .as_ref()
            .and_then(|info| info["version"].as_str())
            .map(str::to_owned);
        Arc::new(review)
    }

    fn confirm_apply(
        &self,
        review: Arc<crate::review::Context>,
        call: ToolCall,
        context: &stdio::Context,
        message: String,
    ) -> Outcome {
        self.confirm_operation(review, call.into(), context, message, None, None)
    }

    fn confirm_operation(
        &self,
        review: Arc<crate::review::Context>,
        call: ReviewedCall,
        context: &stdio::Context,
        message: String,
        review_binding: Option<u64>,
        mut guard: Option<PolicyWait>,
    ) -> Outcome {
        if crate::agents::review::for_context(&review).confirmation()
            == crate::agents::review::Confirmation::Terminal
            || review
                .agent
                .as_deref()
                .and_then(crate::agents::find)
                .is_some_and(|adapter| {
                    adapter.native_forms_unavailable(review.host_version.as_deref())
                })
        {
            if let Some(guard) = &mut guard {
                guard.disarm();
            }
            return Outcome::Tool(reply(
                &review,
                &self.connection,
                call.operation(),
                context,
                review_binding.map_or(Answer::Failed("approval_unavailable"), |id| {
                    Answer::AdminFailed(id, "approval_unavailable")
                }),
            ));
        }
        if let Some(request) = context.request_context() {
            let interim = self
                .confirmations
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .begin(
                    request,
                    &call.request,
                    &message,
                    review_binding,
                    guard,
                    call.action.clone(),
                );
            return match interim {
                Ok(result) => Outcome::InputRequired(result),
                Err(confirmation::Refusal::Full(error)) => {
                    if let Some(binding) = review_binding {
                        crate::review::mcp::discard_admin(&self.connection, binding);
                    }
                    Outcome::Method(Err(error))
                }
                Err(confirmation::Refusal::Unavailable) => Outcome::Tool(reply(
                    &review,
                    &self.connection,
                    call.operation(),
                    context,
                    review_binding.map_or(Answer::Failed("approval_unavailable"), |id| {
                        Answer::AdminFailed(id, "approval_unavailable")
                    }),
                )),
            };
        }
        let mut admin = AdminWait {
            connection: self.connection.clone(),
            binding: review_binding,
        };
        traced_confirm(message, move |decision, context| {
            let review = progress::resume(review, context);
            if let Some(guard) = &mut guard {
                guard.disarm();
            }
            let review_binding = admin.take();
            let cancelled = if review_binding.is_none() && call.operation().name == "policy_apply" {
                "approval_cancelled"
            } else {
                "approval_declined"
            };
            Outcome::Tool(reply(
                &review,
                &admin.connection,
                call.operation(),
                context,
                confirmation_answer(decision, review_binding, cancelled),
            ))
        })
    }

    /// In-session approval always asks the person with Shield's form, whatever
    /// the host's tool approval does with this call. A request that cannot be
    /// approved reports why without asking.
    fn confirm_request(
        &self,
        review: Arc<crate::review::Context>,
        call: ToolCall,
        context: &stdio::Context,
    ) -> Outcome {
        match crate::review::approvals::confirmation(&review, &call.arguments, &|| {
            context
                .check()
                .map_err(|_| crate::policy::Error::Interrupted)
        }) {
            Ok(message) => self.confirm_apply(review, call, context, message),
            Err(_) => Outcome::Tool(reply(
                &review,
                &self.connection,
                &call,
                context,
                Answer::None,
            )),
        }
    }

    fn policy_apply(
        &self,
        review: Arc<crate::review::Context>,
        call: ReviewedCall,
        context: &stdio::Context,
    ) -> Outcome {
        let operation = call.operation();
        // Shield's own review routes always confirm in its form.
        match crate::review::mcp::prepare_policy_confirmation(
            &review,
            &self.connection,
            &operation.arguments,
            call.action.is_some(),
        ) {
            Ok(Some(question)) => {
                let mut guard = match PolicyWait::begin(
                    &self.policy_waits,
                    &review,
                    &operation.arguments,
                    &question.proposal_id,
                    context.request_id(),
                ) {
                    Ok(guard) => guard,
                    Err(error) => return Outcome::Method(Err(error)),
                };
                if let Err(error) = question.claim_web_offer() {
                    // A competing connection may own the durable offer.
                    guard.disarm();
                    return Self::policy_apply_failure(&review, operation, &error);
                }
                self.confirm_operation(review, call, context, question.message, None, Some(guard))
            }
            Err(error) => Self::policy_apply_failure(&review, operation, &error),
            Ok(None) => Outcome::Tool(reply(
                &review,
                &self.connection,
                operation,
                context,
                Answer::None,
            )),
        }
    }

    fn policy_apply_failure(
        review: &crate::review::Context,
        call: &ToolCall,
        error: &crate::review::Error,
    ) -> Outcome {
        let outcome = crate::review::mcp::settle_apply_failure(review, &call.arguments, error);
        let committed = outcome.is_some();
        Outcome::Tool(response::tool_result(
            &call.name,
            review.agent.as_deref(),
            outcome.unwrap_or_else(|| crate::review::mcp::question_error(review, error)),
            !committed,
            &|| Ok(()),
        ))
    }

    fn review_guided_result(
        &self,
        review: Arc<crate::review::Context>,
        call: ToolCall,
        value: serde_json::Value,
        context: &stdio::Context,
    ) -> Outcome {
        if value["error"].is_null() && requests_review(&call) {
            if call.name == "registry_add" && value["data"]["status"] == "input_collected" {
                return self.confirm_registry_source(
                    review,
                    call,
                    value["data"]["retry_arguments"].clone(),
                    context,
                );
            }
            if value["data"]["apply_arguments"].is_object() {
                return self.policy_apply(
                    review,
                    ReviewedCall::with_action(
                        call,
                        "policy_apply",
                        value["data"]["apply_arguments"].clone(),
                    ),
                    context,
                );
            }
        }
        elicitation::output(&review, &call.name, value, context)
    }

    fn confirm_registry_source(
        &self,
        review: Arc<crate::review::Context>,
        request: ToolCall,
        args: serde_json::Value,
        context: &stdio::Context,
    ) -> Outcome {
        let call = ReviewedCall::with_action(request, "registry_add", args);
        let operation = call.operation();
        match crate::review::mcp::admin_confirmation(
            &review,
            &self.connection,
            &operation.name,
            &operation.arguments,
            &|| {
                context
                    .check()
                    .map_err(|_| crate::policy::Error::Interrupted)
            },
        ) {
            Ok(Some((text, binding))) => {
                self.confirm_operation(review, call, context, text, Some(binding), None)
            }
            Ok(None) => Outcome::Tool(reply(
                &review,
                &self.connection,
                operation,
                context,
                Answer::None,
            )),
            Err(error) => elicitation::output(
                &review,
                &operation.name,
                crate::review::mcp::admin_form_error(
                    &review,
                    &operation.name,
                    &operation.arguments,
                    &error,
                ),
                context,
            ),
        }
    }

    fn runtime_refusal(
        &self,
        call: &ToolCall,
        context: &stdio::Context,
        error: &crate::review::Error,
    ) -> Outcome {
        if let Some(request) = context.request_context() {
            let binding = self
                .confirmations
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .discard(request);
            if let Some(binding) = binding {
                crate::review::mcp::discard_admin(&self.connection, binding);
            }
        }
        let review = self.review_for_request(context);
        Outcome::Tool(response::tool_result(
            &call.name,
            review.agent.as_deref(),
            crate::review::mcp::admin_error(&review, &call.name, error),
            true,
            &|| Ok(()),
        ))
    }

    fn resume_confirmation(
        &self,
        review: &crate::review::Context,
        call: &ToolCall,
        context: &stdio::Context,
    ) -> Option<Outcome> {
        let request = context.request_context()?;
        if request.request_state.is_none() && request.input_responses.is_none() {
            return None;
        }
        let (decision, cleanup) = {
            let mut confirmations = self
                .confirmations
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let cleanup = confirmations.review_binding(request);
            (confirmations.resume(request, call), cleanup)
        };
        if decision.is_err()
            && let Some(binding) = cleanup
        {
            crate::review::mcp::discard_admin(&self.connection, binding);
        }
        Some(match decision {
            Ok((decision, binding, action)) => {
                let call = action.as_ref().unwrap_or(call);
                let cancelled = if call.name == "policy_apply" {
                    "approval_cancelled"
                } else {
                    "approval_declined"
                };
                Outcome::Tool(reply(
                    review,
                    &self.connection,
                    call,
                    context,
                    confirmation_answer(decision, binding, cancelled),
                ))
            }
            Err(error) => Outcome::Method(Err(error)),
        })
    }
}

impl Server {
    fn maintain_setup(
        &self,
        review: &crate::review::Context,
        call: &ToolCall,
        context: &stdio::Context,
    ) -> Option<Outcome> {
        if context.check().is_err() {
            return Some(Outcome::Tool(reply(
                review,
                &self.connection,
                call,
                context,
                Answer::None,
            )));
        }
        let resumed = crate::review::mcp::resume_setup(review, &call.name, &call.arguments);
        if !crate::review::mcp::setup_resume_only(&call.name, &call.arguments)
            || !(resumed.as_ref().is_ok_and(|resumed| *resumed) || resumed.is_err())
        {
            return None;
        }
        let value = crate::review::mcp::setup_resume_status(
            review,
            &self.connection,
            &call.name,
            resumed.as_ref().err(),
            &|| {
                context
                    .check()
                    .map_err(|_| crate::policy::Error::Interrupted)
            },
        );
        let failed = !value["error"].is_null();
        Some(Outcome::Tool(response::tool_result(
            &call.name,
            review.agent.as_deref(),
            value,
            failed,
            &|| Ok(()),
        )))
    }
}

impl stdio::Handler for Server {
    fn admit(&self, request: &Request) -> stdio::Admission {
        stdio::Admission {
            lane: if matches!(request, Request::Task(_)) {
                2
            } else {
                usize::from(
                    matches!(request, Request::Resource(_))
                        || matches!(request, Request::Method { method, .. } if method == "resources/read"),
                )
            },
            ..stdio::Admission::default()
        }
    }

    fn initialize(&self, client: &Client) -> Result<ServerInfo, RpcError> {
        let mut review = self
            .review
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let context = Arc::make_mut(&mut review);
        // Only the enrolled agent selects the review adapter. The client's
        // name is a declaration, kept for attribution alone.
        context.client = Some(client.name.clone());
        context.host_version = Some(client.version.clone());
        Ok(ServerInfo::new("shield", env!("CARGO_PKG_VERSION"))
            .with_instructions(messages::instructions_for(context)))
    }

    fn call(&self, request: Request, context: &stdio::Context) -> Outcome {
        traced_call(
            self.log.as_ref(),
            request,
            context,
            || Some(self.review()),
            |request| match request {
                Request::Tool(call) => self.call_tool(call, context),
                _ => self.non_tool(&request, context),
            },
        )
    }

    fn invalid_arguments(&self, call: &ToolCall, error: &ArgumentError) -> ToolResult {
        let review = self.review();
        let value =
            crate::review::mcp::invalid_arguments(&review, &call.name, &error.path, &error.message);
        response::tool_result(&call.name, review.agent.as_deref(), value, true, &|| Ok(()))
    }
}

fn traced_call(
    log: Option<&Arc<crate::diagnostics::Log>>,
    request: Request,
    context: &stdio::Context,
    review: impl FnOnce() -> Option<Arc<crate::review::Context>>,
    dispatch: impl FnOnce(Request) -> Outcome,
) -> Outcome {
    let trace = log.filter(|log| log.path().is_some()).and_then(|log| {
        let operation = match &request {
            Request::Tool(call) => crate::diagnostics::trace::operation(&call.name),
            _ => "protocol",
        };
        let review = review();
        let connection = review
            .as_ref()
            .and_then(|review| review.connection_id.as_deref());
        let trace = log.request("mcp", operation, context.request_id(), connection)?;
        trace.transport(context.observation_ids());
        Some(trace)
    });
    let _scope = trace
        .as_ref()
        .map(crate::diagnostics::trace::Request::enter);
    let outcome = dispatch(request);
    finish_trace(trace.as_ref(), &outcome);
    outcome
}

/// A host confirmation whose wait and resumed call belong to the current trace.
fn traced_confirm(
    message: String,
    resume: impl FnOnce(Decision, &stdio::Context) -> Outcome + Send + 'static,
) -> Outcome {
    let trace = crate::diagnostics::trace::current();
    let waiting = trace
        .as_ref()
        .map(|trace| trace.wait("approval.wait", "approval.wait_us"));
    Outcome::Confirm(Confirm::new(message, move |decision, context| {
        let _scope = trace
            .as_ref()
            .map(crate::diagnostics::trace::Request::enter);
        drop(waiting);
        let outcome = resume(decision, context);
        finish_trace(trace.as_ref(), &outcome);
        outcome
    }))
}

fn finish_trace(trace: Option<&crate::diagnostics::trace::Request>, outcome: &Outcome) {
    let Some(trace) = trace else { return };
    let result = match outcome {
        Outcome::Confirm(_) | Outcome::Elicit(_) => return,
        Outcome::Tool(result) if result.is_error => "refused",
        Outcome::Tool(_) => "success",
        Outcome::InputRequired(_) => "input_required",
        Outcome::Task(Err(_)) | Outcome::Method(Err(_)) => "refused",
        _ => "protocol_result",
    };
    trace.finish(result);
}

fn needs_input_form(call: &ToolCall) -> bool {
    matches!(call.name.as_str(), "pack_add" | "pack_update")
        || (call.name == "registry_add"
            && call.arguments.get("review_ref").is_none()
            && ["name", "location"]
                .iter()
                .any(|key| call.arguments.get(key).is_none()))
        || (call.name == "pack_remove" && call.arguments.get("reference").is_none())
        || (call.name == "pack_configure"
            && (call.arguments.get("variables").is_some()
                || !["set", "unset", "override", "clear_override"]
                    .iter()
                    .any(|key| call.arguments.get(key).is_some())))
}

fn requests_review(call: &ToolCall) -> bool {
    crate::review::mcp::supports_guided_review(&call.name) && call.arguments["review"] == true
}

fn operation_call(call: &ToolCall) -> Option<ToolCall> {
    if call.arguments.get("review").is_none()
        || !crate::review::mcp::supports_guided_review(&call.name)
    {
        return None;
    }
    let mut operation = call.clone();
    operation
        .arguments
        .as_object_mut()
        .expect("review field in object")
        .remove("review");
    Some(operation)
}

/// The host answers the original call. Only Shield supplies the routed operation.
struct ReviewedCall {
    request: ToolCall,
    action: Option<ToolCall>,
}

impl From<ToolCall> for ReviewedCall {
    fn from(request: ToolCall) -> Self {
        Self {
            request,
            action: None,
        }
    }
}

impl ReviewedCall {
    fn operation(&self) -> &ToolCall {
        self.action.as_ref().unwrap_or(&self.request)
    }

    fn with_action(request: ToolCall, name: &str, arguments: serde_json::Value) -> Self {
        let operation = ToolCall {
            name: name.into(),
            arguments,
            progress_token: request.progress_token.clone(),
        };
        Self {
            request,
            action: Some(operation),
        }
    }
}

fn nul_arguments(review: &crate::review::Context, call: &ToolCall) -> Outcome {
    let value = crate::review::mcp::invalid_arguments(
        review,
        &call.name,
        "",
        "argument names and strings must not contain NUL characters",
    );
    Outcome::Tool(response::tool_result(
        &call.name,
        review.agent.as_deref(),
        value,
        true,
        &|| Ok(()),
    ))
}

/// What the host's user answered before the call runs.
#[derive(Clone, Copy)]
enum Answer {
    None,
    Accepted,
    AdminAccepted(u64),
    AdminFailed(u64, &'static str),
    Failed(&'static str),
}

fn confirmation_answer(
    decision: Decision,
    binding: Option<u64>,
    cancelled: &'static str,
) -> Answer {
    let code = match decision {
        Decision::Accepted => return binding.map_or(Answer::Accepted, Answer::AdminAccepted),
        Decision::Declined => "approval_declined",
        Decision::Cancelled => cancelled,
        Decision::Unavailable => "approval_unavailable",
    };
    binding.map_or(Answer::Failed(code), |id| Answer::AdminFailed(id, code))
}

type PolicyWaits = Arc<Mutex<BTreeMap<String, serde_json::Value>>>;

/// Own the wait for one validated, exact policy review across both transports.
/// Dropping the wait before an answer closes the candidate.
struct PolicyWait {
    waits: PolicyWaits,
    proposal_id: String,
    context: crate::review::Context,
    arguments: serde_json::Value,
    armed: bool,
}
impl PolicyWait {
    fn disarm(&mut self) {
        self.armed = false;
    }
    fn begin(
        waits: &PolicyWaits,
        context: &crate::review::Context,
        arguments: &serde_json::Value,
        proposal_id: &str,
        request_id: Option<&serde_json::Value>,
    ) -> Result<Self, RpcError> {
        let mut open = waits
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(id) = open.get(proposal_id) {
            return Err(RpcError::new(
                -32000,
                "Confirmation for this policy review is already pending. Wait for the existing form to be answered or cancelled.",
            )
            .with_data(serde_json::json!({"code":"human_input_busy","requestId":id})));
        }
        open.insert(
            proposal_id.to_owned(),
            request_id.cloned().unwrap_or_default(),
        );
        Ok(Self {
            waits: waits.clone(),
            proposal_id: proposal_id.to_owned(),
            context: context.clone(),
            arguments: arguments.clone(),
            armed: true,
        })
    }
}
impl Drop for PolicyWait {
    fn drop(&mut self) {
        if self.armed {
            let _ = crate::review::proposal_lifecycle::settle(
                &self.context,
                &self.arguments,
                "withdrawn",
                "cancelled",
            );
        }
        self.waits
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&self.proposal_id);
    }
}

/// Release a captured administrative review even when legacy stdio drops the
/// continuation without delivering an answer.
struct AdminWait {
    connection: Arc<crate::review::mcp::Connection>,
    binding: Option<u64>,
}

impl AdminWait {
    fn take(&mut self) -> Option<u64> {
        self.binding.take()
    }
}

impl Drop for AdminWait {
    fn drop(&mut self) {
        if let Some(binding) = self.binding {
            crate::review::mcp::discard_admin(&self.connection, binding);
        }
    }
}

fn reply(
    review: &crate::review::Context,
    connection: &crate::review::mcp::Connection,
    call: &ToolCall,
    context: &stdio::Context,
    answer: Answer,
) -> ToolResult {
    let mut setup_binding = None;
    let answer = if let Answer::AdminFailed(binding, code) = answer {
        if call.name == crate::review::setup::TOOL && code == "approval_unavailable" {
            setup_binding = Some(binding);
        } else {
            crate::review::mcp::discard_admin(connection, binding);
        }
        Answer::Failed(code)
    } else {
        answer
    };
    let declined = matches!(call.name.as_str(), "session_grant_approve")
        && matches!(answer, Answer::Failed("approval_declined"));
    if declined {
        let _ = crate::review::session_grants::decline(review, &call.arguments);
    }
    let checkpoint = || {
        context
            .check()
            .map_err(|_| crate::policy::Error::Interrupted)
    };
    if call.name == "policy_apply"
        && let Answer::Failed(code) = answer
    {
        let value = crate::review::mcp::policy_outcome(review, &call.arguments, code);
        let failed = !value["error"].is_null();
        return response::tool_result(&call.name, review.agent.as_deref(), value, failed, &|| {
            Ok(())
        });
    }
    let mut value = crate::review::mcp::call_with_bound_confirmation(
        review,
        connection,
        &call.name,
        &call.arguments,
        &checkpoint,
        matches!(answer, Answer::Accepted | Answer::AdminAccepted(_)),
        if let Answer::AdminAccepted(binding) = answer {
            Some(binding)
        } else {
            setup_binding
        },
    );
    if declined && value["data"]["proposal_status"] != "issued" {
        value["data"] = serde_json::Value::Null;
        value["error"] = serde_json::json!({"code":"approval_declined","message":"The session-grant form was declined or cancelled. No permission was issued. Continue under ordinary policy and do not repeat the offer.","operator_route":null});
    }
    if let Answer::Failed(code) = answer
        && (value["error"]["code"] == "operator_review_required"
            || (code == "approval_declined"
                && call.name == crate::review::setup::TOOL
                && value["error"]["code"] == "approval_unavailable"))
    {
        let request = ["request_approve", "request_deny", "session_grant_approve"]
            .contains(&call.name.as_str());
        let message = if crate::review::mcp::admin_route(&call.name).is_some() {
            if code == "approval_declined" {
                "The administrative change was declined. Continue without it; do not repeat the offer unless the user requests it.".to_owned()
            } else if matches!(review.agent.as_deref(), Some("gemini" | "agy")) {
                "This host did not provide administrative confirmation. Report the missing in-session capability; no change was approved.".to_owned()
            } else {
                messages::request_unavailable(review.agent.as_deref())
                    .replace("request_approve", &call.name)
            }
        } else {
            match (code, request) {
            (_, _) if call.name == "session_grant_approve" && matches!(review.agent.as_deref(), Some("gemini" | "agy")) => "This host did not provide session-grant confirmation. Report the missing in-session capability; no permission was issued.",
            (_, _) if matches!(call.name.as_str(), "session_grant_approve") => "Session-grant confirmation is unavailable. Enable host confirmation forms in this session or report the missing capability; no permission was issued.",
            ("approval_declined", true) => messages::REQUEST_DECLINED,
            ("approval_declined", false) => messages::APPROVAL_DECLINED,
            (_, true) => messages::request_unavailable(review.agent.as_deref()),
            (_, false) => messages::approval_unavailable(review.agent.as_deref()),
        }.replace("request_approve", &call.name)
        };
        value["error"] = serde_json::json!({"code":code,"message":message,"operator_route":null});
    }
    let failed = !value["error"].is_null();
    // Cursor data is an immutable historical view, so it cannot supply a fresh
    // unsolicited update notice after the installed selection changes.
    if call.arguments.get("cursor").is_some() {
        return response::tool_result(&call.name, review.agent.as_deref(), value, failed, &|| {
            context.check()
        });
    }
    response::with_pack_notice(review, &call.name, value, failed, &|| context.check())
}

/// The published tools, in inventory order.
///
/// # Errors
///
/// Returns the catalog's refusal of a tool name or schema.
pub(crate) fn catalog(agent: Option<&str>) -> Result<Catalog, CatalogError> {
    let listing = crate::review::mcp::tools_for(agent);
    let tools = listing["tools"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|tool| {
            let hint = |key: &str| tool["annotations"][key].as_bool();
            Tool::new(
                tool["name"].as_str().unwrap_or_default(),
                tool["description"].as_str().unwrap_or_default(),
                tool["inputSchema"].clone(),
            )
            .with_output_schema(tool["outputSchema"].clone())
            .with_annotations(Annotations {
                read_only_hint: hint("readOnlyHint"),
                destructive_hint: hint("destructiveHint"),
                idempotent_hint: hint("idempotentHint"),
                open_world_hint: hint("openWorldHint"),
                ..Annotations::default()
            })
        })
        .collect();
    Catalog::new(tools)
}

/// One evaluator handles up to 64 outstanding calls in arrival order.
/// Requests are connection-owned, with bounded frames and a message count.
pub(crate) fn options(log: Option<Arc<crate::diagnostics::Log>>) -> stdio::Options {
    stdio::Options {
        session: SessionOptions {
            max_message_bytes: MAX_MESSAGE,
            max_messages: Some(MAX_MESSAGES),
            ..SessionOptions::default()
        },
        lanes: vec![stdio::Lane::new("tools", 1, MAX_QUEUED_CALLS)],
        budget_start: stdio::BudgetStart::Never,
        max_output_bytes: MAX_OUTPUT,
        observer: log.and_then(frame_log::observer),
        ..stdio::Options::default()
    }
}

/// Serves one connection until its input ends and admitted work settles.
///
/// # Errors
///
/// Returns the driver's input, output or protocol failure.
pub(crate) fn serve(
    server: Server,
    input: impl std::io::BufRead + Send + 'static,
    output: impl std::io::Write,
    options: &stdio::Options,
) -> Result<(), ServeError> {
    let review = server.review();
    let agent = if review.confirmed_mcp_forms {
        None
    } else {
        review.agent.as_deref()
    };
    let catalog = catalog(agent).map_err(ServeError::Catalog)?;
    if let Some(schema) = catalog.result_schema("policy_propose") {
        let _ = server.task_schema.set(schema);
    }
    let options = project_options(options, messages::instructions_for(&review))?;
    stdio::serve(Arc::new(server), Arc::new(catalog), input, output, &options)
        .map_err(ServeError::Transport)
}

fn project_options(
    options: &stdio::Options,
    instructions: String,
) -> Result<stdio::Options, ServeError> {
    let mut options = bundled_options(options)?;
    options.session.tasks = Some(tasks::policy());
    options
        .lanes
        .push(stdio::Lane::new("preparation-management", 2, 8));
    options.session.resources = Some(Arc::new(
        resources::catalog().map_err(ServeError::Resources)?,
    ));
    options.session.modern = Some(canonic_mcp_kit::ModernOptions::new(
        ServerInfo::new("shield", env!("CARGO_PKG_VERSION")).with_instructions(instructions),
    ));
    Ok(options)
}

fn bundled_options(options: &stdio::Options) -> Result<stdio::Options, ServeError> {
    let mut options = options.clone();
    options.lanes.push(stdio::Lane::new("bundled-skills", 1, 8));
    options.session.skills = Some(Arc::new(skills::catalog().map_err(ServeError::Skills)?));
    options.session.prompts = Some(Arc::new(prompts::catalog().map_err(ServeError::Prompts)?));
    options.session.modern = Some(canonic_mcp_kit::ModernOptions::new(ServerInfo::new(
        "shield",
        env!("CARGO_PKG_VERSION"),
    )));
    Ok(options)
}

/// Static discovery requires no project context or policy loading.
pub(crate) fn serve_static(
    input: impl std::io::BufRead + Send + 'static,
    output: impl std::io::Write,
    options: &stdio::Options,
) -> Result<(), ServeError> {
    struct Static;
    impl stdio::Handler for Static {
        fn initialize(&self, _: &Client) -> Result<ServerInfo, RpcError> {
            Ok(ServerInfo::new("shield", env!("CARGO_PKG_VERSION")))
        }
        fn call(&self, request: Request, _: &stdio::Context) -> Outcome {
            if let Request::Prompt(call) = &request {
                return prompts::render(call);
            }
            if let Request::Method { method, params } = request
                && method == "resources/read"
            {
                return Outcome::ResourceNotFound(
                    params["uri"].as_str().unwrap_or_default().into(),
                );
            }
            Outcome::Method(Err(RpcError::new(
                rpc::METHOD_NOT_FOUND,
                "Enrolled project required for tools",
            )))
        }
    }
    let options = bundled_options(options)?;
    let catalog = Catalog::new(vec![]).map_err(ServeError::Catalog)?;
    stdio::serve(Arc::new(Static), Arc::new(catalog), input, output, &options)
        .map_err(ServeError::Transport)
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum ServeError {
    #[error("Cannot start Shield MCP server: invalid review resource catalog")]
    Resources(#[source] canonic_mcp_kit::ResourceError),
    #[error("Cannot start Shield MCP server: invalid prompt catalog")]
    Prompts(#[source] canonic_mcp_kit::PromptError),
    #[error("Cannot start Shield MCP server: invalid skill catalog")]
    Skills(#[source] canonic_mcp_kit::SkillError),
    #[error("Cannot start Shield MCP server: invalid tool catalog")]
    Catalog(#[source] CatalogError),
    #[error("Shield review server transport failed")]
    Transport(#[source] stdio::ServeError),
}
