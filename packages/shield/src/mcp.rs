//! Shield's MCP server for enrolled projects: the tool catalog, connection
//! identity, review dispatch and host confirmation of policy changes, served by
//! canonic-mcp-kit's stdio driver.

mod confirmation;
mod elicitation;
mod frame_log;
mod messages;
mod prompts;
mod response;
mod skills;
#[cfg(test)]
pub(crate) mod tests;

use canonic_mcp_kit::{
    Annotations, Catalog, CatalogError, Client, Decision, RpcError, ServerInfo, SessionOptions,
    Tool, ToolCall, ToolResult, rpc,
    schema::ArgumentError,
    stdio::{self, Confirm, Direction, Outcome, Request},
};
use std::{
    sync::{Arc, Mutex, RwLock},
    time::Duration,
};

pub(crate) const MAX_MESSAGE: usize = 524_288;
pub(crate) const MAX_OUTPUT: usize = 4_194_304;
pub(crate) const MAX_MESSAGES: u64 = 1024;

/// One enrolled project connection. The review context is fixed at startup
/// except for the client name and version recorded during initialization.
pub(crate) struct Server {
    review: RwLock<Arc<crate::review::Context>>,
    connection: Arc<crate::review::mcp::Connection>,
    confirmations: Mutex<confirmation::Confirmations>,
    questions: Mutex<elicitation::Questions>,
    log: Option<Arc<crate::diagnostics::Log>>,
}

impl Server {
    pub(crate) fn for_review(
        mut context: crate::review::Context,
    ) -> Result<Self, crate::review::Error> {
        let tick = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        context.surface = "mcp".into();
        context.connection_id = Some(format!(
            "connection-{}",
            crate::policy::fingerprint(
                format!("{}:{}:{tick}", context.project_id, std::process::id()).as_bytes()
            )
        ));
        let connection = Arc::new(crate::review::mcp::Connection::enrolled(&context)?);
        Ok(Self {
            review: RwLock::new(Arc::new(context)),
            connection,
            confirmations: Mutex::new(confirmation::Confirmations::default()),
            questions: Mutex::new(elicitation::Questions::default()),
            log: None,
        })
    }

    pub(crate) fn review(&self) -> Arc<crate::review::Context> {
        self.review
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    pub(crate) fn with_log(mut self, log: Arc<crate::diagnostics::Log>) -> Self {
        self.log = Some(log);
        self
    }

    fn review_for_request(&self, context: &stdio::Context) -> Arc<crate::review::Context> {
        let review = self.review();
        let Some(request) = context.request_context() else {
            return review;
        };
        let mut review = (*review).clone();
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
        if let Some(request) = context.request_context() {
            let interim = self
                .confirmations
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .begin(request, &call, &message);
            return match interim {
                Ok(result) => Outcome::InputRequired(result),
                Err(_) => Outcome::Tool(reply(
                    &review,
                    &self.connection,
                    &call,
                    context,
                    Answer::Failed("approval_unavailable"),
                )),
            };
        }
        let connection = self.connection.clone();
        let trace = crate::diagnostics::trace::current();
        let waiting = trace
            .as_ref()
            .map(|trace| trace.wait("approval.wait", "approval.wait_us"));
        Outcome::Confirm(Confirm::new(message, move |decision, context| {
            let _scope = trace
                .as_ref()
                .map(crate::diagnostics::trace::Request::enter);
            drop(waiting);
            let answer = match decision {
                Decision::Accepted => Answer::Accepted,
                Decision::Declined => Answer::Failed("approval_declined"),
                Decision::Unavailable => Answer::Failed("approval_unavailable"),
            };
            let result = reply(&review, &connection, &call, context, answer);
            if let Some(trace) = &trace {
                trace.finish(if result.is_error {
                    "refused"
                } else {
                    "success"
                });
            }
            Outcome::Tool(result)
        }))
    }

    fn request_trace(
        &self,
        request: &Request,
        context: &stdio::Context,
    ) -> Option<crate::diagnostics::trace::Request> {
        let log = self.log.as_ref()?;
        log.path()?;
        let operation = match request {
            Request::Tool(call) => crate::diagnostics::trace::operation(&call.name),
            _ => "protocol",
        };
        log.request(
            "mcp",
            operation,
            context.request_id(),
            self.review().connection_id.as_deref(),
        )
    }
}

impl stdio::Handler for Server {
    fn admit(&self, request: &Request) -> stdio::Admission {
        stdio::Admission {
            lane: usize::from(matches!(request, Request::Resource(_))),
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
            .with_instructions(messages::instructions(context.agent.as_deref())))
    }

    fn call(&self, request: Request, context: &stdio::Context) -> Outcome {
        let trace = self.request_trace(&request, context);
        let _trace_scope = trace
            .as_ref()
            .map(crate::diagnostics::trace::Request::enter);
        let outcome = (|| {
            if let Request::Prompt(call) = &request {
                return prompts::render(call);
            }
            if let Request::Method { method, params } = &request
                && method == "resources/read"
            {
                return Outcome::ResourceNotFound(
                    params["uri"].as_str().unwrap_or_default().into(),
                );
            }
            let Request::Tool(call) = request else {
                return Outcome::Method(Err(RpcError::new(
                    rpc::METHOD_NOT_FOUND,
                    "Method not found",
                )));
            };
            let review = self.review_for_request(context);
            if !["pack_configure", "pack_add"].contains(&call.name.as_str())
                && let Some(request) = context.request_context()
            {
                let retry = request.request_state.is_some() || request.input_responses.is_some();
                if retry {
                    let decision = self
                        .confirmations
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .resume(request, &call);
                    return match decision {
                        Ok(decision) => Outcome::Tool(reply(
                            &review,
                            &self.connection,
                            &call,
                            context,
                            if decision == Decision::Accepted {
                                Answer::Accepted
                            } else {
                                Answer::Failed("approval_declined")
                            },
                        )),
                        Err(error) => Outcome::Method(Err(error)),
                    };
                }
            }
            // NULs are refused before dispatch or confirmation, so no tool stores one.
            if crate::json::contains_nul(&call.arguments) {
                let value = crate::review::mcp::invalid_arguments(
                    &review,
                    &call.name,
                    "",
                    "argument names and strings must not contain NUL characters",
                );
                return Outcome::Tool(response::tool_result(
                    &call.name,
                    review.agent.as_deref(),
                    value,
                    true,
                    &|| Ok(()),
                ));
            }
            if ["pack_configure", "pack_add"].contains(&call.name.as_str()) {
                return elicitation::call(self, review, call, context);
            }
            // The resolved adapter declares the host confirmation mechanism.
            if call.name == "policy_apply"
                && crate::agents::review::resolve(review.agent.as_deref()).confirmation()
                    == crate::agents::review::Confirmation::Form
                && let Ok(Some(message)) =
                    crate::review::mcp::apply_confirmation(&review, &call.arguments)
            {
                return self.confirm_apply(review, call, context, message);
            }
            Outcome::Tool(reply(
                &review,
                &self.connection,
                &call,
                context,
                Answer::None,
            ))
        })();
        if let Some(trace) = &trace {
            match &outcome {
                Outcome::Confirm(_) | Outcome::Elicit(_) => {}
                Outcome::Tool(result) => trace.finish(if result.is_error {
                    "refused"
                } else {
                    "success"
                }),
                Outcome::InputRequired(_) => trace.finish("input_required"),
                _ => trace.finish("protocol_result"),
            }
        }
        outcome
    }

    fn invalid_arguments(&self, call: &ToolCall, error: &ArgumentError) -> ToolResult {
        let review = self.review();
        let value =
            crate::review::mcp::invalid_arguments(&review, &call.name, &error.path, &error.message);
        response::tool_result(&call.name, review.agent.as_deref(), value, true, &|| Ok(()))
    }
}

/// What the host's user answered before the call runs.
#[derive(Clone, Copy)]
enum Answer {
    None,
    Accepted,
    Failed(&'static str),
}

fn reply(
    review: &crate::review::Context,
    connection: &crate::review::mcp::Connection,
    call: &ToolCall,
    context: &stdio::Context,
    answer: Answer,
) -> ToolResult {
    let checkpoint = || {
        context
            .check()
            .map_err(|_| crate::policy::Error::Interrupted)
    };
    let mut value = crate::review::mcp::call_with_confirmation(
        review,
        connection,
        &call.name,
        &call.arguments,
        &checkpoint,
        matches!(answer, Answer::Accepted),
    );
    if let Answer::Failed(code) = answer
        && value["error"]["code"] == "operator_review_required"
    {
        value["error"] = serde_json::json!({"code":code,"message":match code {
            "approval_declined" => messages::APPROVAL_DECLINED,
            _ => messages::approval_unavailable(review.agent.as_deref()),
        },"operator_route":null});
    }
    let failed = !value["error"].is_null();
    response::tool_result(&call.name, review.agent.as_deref(), value, failed, &|| {
        context.check()
    })
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

/// Shield's limits: one evaluator worker with eight outstanding requests,
/// bounded frames and output, and a per-connection message count.
pub(crate) fn options(
    timeout: Duration,
    log: Option<Arc<crate::diagnostics::Log>>,
) -> stdio::Options {
    stdio::Options {
        session: SessionOptions {
            max_message_bytes: MAX_MESSAGE,
            max_messages: Some(MAX_MESSAGES),
            ..SessionOptions::default()
        },
        timeout,
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
    let catalog = catalog(server.review().agent.as_deref()).map_err(ServeError::Catalog)?;
    let mut options = bundled_options(options)?;
    options.session.modern = Some(canonic_mcp_kit::ModernOptions::new(
        ServerInfo::new("shield", env!("CARGO_PKG_VERSION"))
            .with_instructions(messages::instructions(server.review().agent.as_deref())),
    ));
    stdio::serve(Arc::new(server), Arc::new(catalog), input, output, &options)
        .map_err(ServeError::Transport)
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
    #[error("Cannot start Shield MCP server: invalid prompt catalog")]
    Prompts(#[source] canonic_mcp_kit::PromptError),
    #[error("Cannot start Shield MCP server: invalid skill catalog")]
    Skills(#[source] canonic_mcp_kit::SkillError),
    #[error("Cannot start Shield MCP server: invalid tool catalog")]
    Catalog(#[source] CatalogError),
    #[error("Shield review server transport failed")]
    Transport(#[source] stdio::ServeError),
}
