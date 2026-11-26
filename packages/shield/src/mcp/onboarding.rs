//! A fresh project's MCP connection can initialize itself through a human form,
//! then delegate to the ordinary enrolled server without reconnecting.
use super::{ServeError, Server, confirmation::Confirmations, response};
use crate::{
    cli::ReviewContext,
    review::{self, initialization::Onboarding, setup},
};
use canonic_mcp_kit::{
    Annotations, Catalog, Client, Decision, RpcError, ServerInfo, Tool, ToolCall, ToolResult, rpc,
    stdio::{self, Handler, Outcome, Request},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
};

const INSTRUCTIONS: &str = "Use setup to connect Shield. The user reviews changes in this session. Follow the returned action.";

struct Bootstrap {
    input: Arc<ReviewContext>,
    active: Arc<OnceLock<Server>>,
    confirmations: Mutex<Confirmations>,
    plans: Mutex<BTreeMap<u64, Onboarding>>,
    sequence: AtomicU64,
    log: Arc<crate::diagnostics::Log>,
    prepare: fn(
        &std::path::Path,
        crate::policy::Profile,
        &[String],
    ) -> Result<Onboarding, review::Error>,
}

fn instructions(project: &std::path::Path) -> String {
    format!("{INSTRUCTIONS} Selected project: {}.", json!(project))
}

fn first_run_target_allowed(project: &std::path::Path, home: Option<&std::path::Path>) -> bool {
    project
        .canonicalize()
        .is_ok_and(|root| setup::first_run_target_allowed(&root, home))
}

fn tool() -> Tool {
    let integrations = review::integrations::host_definition::HOSTS
        .into_iter()
        .map(|host| host.id)
        .chain(
            crate::agents::ADAPTERS
                .iter()
                .map(|adapter| adapter.spec().id),
        )
        .collect::<Vec<_>>();
    Tool::new(
        "init",
        "Initialize Shield in this connection's project with a verified baseline and complete agent integration, after human confirmation. Existing policy is preserved. Use agents: [\"hub\"] for Hub publication, or native agent IDs without hub.",
        json!({"type":"object","additionalProperties":false,"properties":{
            "agents":{"type":"array","minItems":1,"maxItems":16,"uniqueItems":true,"items":{"type":"string","enum":integrations}},
            "profile":{"type":"string","enum":["loose","recommended","strict"],"default":"recommended"}},"required":["agents"]}),
    ).with_annotations(Annotations { read_only_hint: Some(false), destructive_hint: Some(false), idempotent_hint: Some(true), open_world_hint: Some(true), ..Annotations::default() })
}

fn result(input: &ReviewContext, data: Result<Value, review::Error>) -> ToolResult {
    let (value, failed) = match data {
        Ok(data) => (
            json!({"schema_version":1,"project_id":data["project_id"].as_str().unwrap_or("uninitialized"),"policy_revision":data["policy_revision"],"data":data,"error":null,"missing":[],"truncated":false}),
            false,
        ),
        Err(error) => (
            json!({"schema_version":1,"project_id":"uninitialized","policy_revision":null,"data":null,"error":{"code":error.code(),"message":error.to_string(),"operator_route":null},"missing":[],"truncated":false}),
            true,
        ),
    };
    let tool = if value["data"]["setup_status"].is_string() {
        setup::TOOL
    } else {
        "status"
    };
    response::tool_result(tool, input.agent.as_deref(), value, failed, &|| Ok(()))
}

fn finish(
    plan: Onboarding,
    decision: Decision,
    input: &ReviewContext,
    active: &OnceLock<Server>,
    log: &Arc<crate::diagnostics::Log>,
    arguments: Option<&Value>,
    checkpoint: review::Checkpoint<'_>,
) -> ToolResult {
    let value = match decision {
        Decision::Accepted => {
            let committed = plan.commit(checkpoint, "mcp", None);
            // If publication was interrupted after initialization, expose the
            // ordinary inspection/setup tools instead of stranding the caller
            // on a fresh-project endpoint that can no longer initialize.
            let activation = if crate::layout::state(&input.project)
                .join("state.json")
                .is_file()
            {
                crate::mcp_io::review_context(input).and_then(|mut context| {
                    if context.agent.is_none() {
                        context.confirmed_mcp_forms = true;
                    }
                    let server = Server::for_review(context)?.with_log(log.clone());
                    let _ = active.set(server);
                    Ok(())
                })
            } else {
                Err(review::Error::Missing)
            };
            committed.and_then(|receipt| activation.map(|()| receipt))
        }
        Decision::Declined | Decision::Cancelled => {
            return refusal(
                "approval_declined",
                "Shield setup was not approved. Nothing was initialized; do not retry unless the user requests it.",
            );
        }
        Decision::Unavailable => {
            if let Some(arguments) = arguments {
                return setup_unavailable(input, arguments);
            }
            return refusal(
                "approval_unavailable",
                "This host could not show Shield's setup confirmation. Ask the user to review shield setup in a terminal for the selected project, or enable in-session confirmation and retry. Nothing was initialized.",
            );
        }
    };
    result(
        input,
        value.map(|receipt| {
            if arguments.is_some() {
                setup::receipt(&input.project, receipt, "initialized")
            } else {
                receipt
            }
        }),
    )
}

fn setup_unavailable(input: &ReviewContext, arguments: &Value) -> ToolResult {
    setup_refusal(
        input,
        arguments,
        "approval_unavailable",
        "The host could not present the Shield setup review. Ask the user to run the returned terminal command to review it. Nothing was initialized.",
    )
}

fn setup_refusal(
    input: &ReviewContext,
    arguments: &Value,
    code: &str,
    message: &str,
) -> ToolResult {
    response::tool_result(
        setup::TOOL,
        input.agent.as_deref(),
        json!({
            "schema_version":1,"project_id":"uninitialized","policy_revision":null,"data":null,
            "error":{"code":code,"message":message,"operator_route":"shield setup","operator_action":setup::operator_action(&input.project, arguments)},
            "missing":[],"truncated":false
        }),
        true,
        &|| Ok(()),
    )
}

fn refusal(code: &str, message: &str) -> ToolResult {
    response::tool_result(
        "init",
        None,
        json!({"schema_version":1,"project_id":"uninitialized","policy_revision":null,"data":null,"error":{"code":code,"message":message,"operator_route":null},"missing":[],"truncated":false}),
        true,
        &|| Ok(()),
    )
}

impl Bootstrap {
    fn initialize_project(&self, call: &ToolCall, context: &stdio::Context) -> Outcome {
        if !first_run_target_allowed(&self.input.project, std::env::home_dir().as_deref()) {
            return Outcome::Tool(refusal(
                "invalid_project",
                &format!(
                    "Cannot initialize Shield in selected project {}: choose a project folder below or outside your home directory, not HOME or one of its ancestors.",
                    json!(self.input.project)
                ),
            ));
        }
        if let Some(request) = context
            .request_context()
            .filter(|request| request.request_state.is_some())
        {
            return self.resume(request, call, context);
        }
        let (plan, agents) = match self.prepare_call(call) {
            Ok(prepared) => prepared,
            Err(error) => return Outcome::Tool(result(&self.input, Err(error))),
        };
        let ordinary_setup = call.name == setup::TOOL;
        let mut operator_arguments = call.arguments.clone();
        operator_arguments["agents"] = json!(agents);
        let message = match plan.message() {
            Ok(message) => message,
            Err(error)
                if let Some((code, message)) =
                    review::operator::administrative_form_refusal(&error) =>
            {
                return Outcome::Tool(setup_refusal(
                    &self.input,
                    &operator_arguments,
                    code,
                    message,
                ));
            }
            Err(error) => return Outcome::Tool(result(&self.input, Err(error))),
        };
        let operator_arguments = ordinary_setup.then_some(operator_arguments);
        if let Some(request) = context.request_context() {
            let binding = self.sequence.fetch_add(1, Ordering::Relaxed);
            let confirmation = self
                .confirmations
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .begin(request, call, &message, Some(binding), None, None);
            return match confirmation {
                Ok(question) => {
                    self.plans
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .insert(binding, plan);
                    Outcome::InputRequired(question)
                }
                Err(super::confirmation::Refusal::Unavailable)
                    if let Some(arguments) = &operator_arguments =>
                {
                    Outcome::Tool(setup_unavailable(&self.input, arguments))
                }
                Err(_) => Outcome::Tool(refusal(
                    "approval_unavailable",
                    "The host could not open a setup confirmation, or eight reviews are already pending. Answer pending reviews before retrying; nothing was initialized.",
                )),
            };
        }
        let input = self.input.clone();
        let active = self.active.clone();
        let log = self.log.clone();
        super::traced_confirm(message, move |decision, context| {
            Outcome::Tool(finish(
                plan,
                decision,
                &input,
                &active,
                &log,
                operator_arguments.as_ref(),
                &|| {
                    context
                        .check()
                        .map_err(|_| crate::policy::Error::Interrupted)
                },
            ))
        })
    }

    fn prepare_call(&self, call: &ToolCall) -> Result<(Onboarding, Vec<String>), review::Error> {
        let agents = call.arguments["agents"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let agents = if call.name == setup::TOOL {
            setup::agents(
                &self.input.project,
                &Value::Null,
                &agents,
                self.input.agent.as_deref(),
            )?
        } else {
            agents
        };
        let profile = match call.arguments["profile"].as_str().unwrap_or("recommended") {
            "loose" => crate::policy::Profile::Loose,
            "strict" => crate::policy::Profile::Strict,
            _ => crate::policy::Profile::Recommended,
        };
        let plan = (self.prepare)(&self.input.project, profile, &agents)?;
        Ok((plan, agents))
    }

    fn resume(
        &self,
        request: &canonic_mcp_kit::RequestContext,
        call: &ToolCall,
        context: &stdio::Context,
    ) -> Outcome {
        let (binding, answer) = {
            let mut confirmations = self
                .confirmations
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            (
                confirmations.review_binding(request),
                confirmations.resume(request, call),
            )
        };
        let plan = binding.and_then(|binding| {
            self.plans
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .remove(&binding)
        });
        match (answer, plan) {
            (Ok((decision, _, _)), Some(plan)) => Outcome::Tool(finish(
                plan,
                decision,
                &self.input,
                &self.active,
                &self.log,
                (call.name == setup::TOOL).then_some(&call.arguments),
                &|| {
                    context
                        .check()
                        .map_err(|_| crate::policy::Error::Interrupted)
                },
            )),
            _ => Outcome::Tool(refusal(
                "invalid_confirmation",
                "Invalid or consumed setup confirmation. Inspect status before preparing a new setup.",
            )),
        }
    }
}

impl Handler for Bootstrap {
    fn admit(&self, request: &Request) -> stdio::Admission {
        self.active
            .get()
            .map_or_else(stdio::Admission::default, |server| server.admit(request))
    }

    fn initialize(&self, client: &Client) -> Result<ServerInfo, RpcError> {
        if let Some(server) = self.active.get() {
            return server.initialize(client);
        }
        Ok(ServerInfo::new("shield", env!("CARGO_PKG_VERSION"))
            .with_instructions(instructions(&self.input.project)))
    }

    fn call(&self, request: Request, context: &stdio::Context) -> Outcome {
        // Continuations belong to this exact bootstrap review, even if another
        // init finished while this host still had a confirmation open.
        let continuation = context.request_context().is_some_and(|request| {
            self.confirmations
                .lock()
                .ok()
                .and_then(|confirmations| confirmations.review_binding(request))
                .is_some()
        });
        if let Some(server) = self.active.get() {
            let init = matches!(&request, Request::Tool(call) if call.name == "init");
            if init
                && context
                    .request_context()
                    .is_none_or(|request| request.request_state.is_none())
            {
                return server.call(
                    Request::Tool(ToolCall {
                        name: "status".into(),
                        arguments: json!({}),
                        progress_token: None,
                    }),
                    context,
                );
            }
            if !init && !continuation {
                return server.call(request, context);
            }
        }
        super::traced_call(
            Some(&self.log),
            request,
            context,
            || None,
            |request| match request {
                Request::Tool(call) if matches!(call.name.as_str(), "init" | setup::TOOL) => {
                    self.initialize_project(&call, context)
                }
                Request::Prompt(call) => super::prompts::render(&call),
                Request::Tool(_) => Outcome::Tool(refusal(
                    "not_initialized",
                    "Shield is not initialized in this project. Call setup to review setup in this session.",
                )),
                Request::Method { method, params } if method == "resources/read" => {
                    Outcome::ResourceNotFound(params["uri"].as_str().unwrap_or_default().into())
                }
                _ => Outcome::Method(Err(RpcError::new(
                    rpc::METHOD_NOT_FOUND,
                    "Set up this project with setup before using project resources",
                ))),
            },
        )
    }
}

pub(crate) fn serve(
    input: ReviewContext,
    log: Arc<crate::diagnostics::Log>,
    reader: impl std::io::BufRead + Send + 'static,
    writer: impl std::io::Write,
    options: &stdio::Options,
) -> Result<(), ServeError> {
    let server = Bootstrap {
        input: Arc::new(input),
        active: Arc::new(OnceLock::new()),
        confirmations: Mutex::new(Confirmations::default()),
        plans: Mutex::new(BTreeMap::new()),
        sequence: AtomicU64::new(1),
        log,
        prepare: Onboarding::prepare,
    };
    serve_server(server, reader, writer, options)
}

fn serve_server(
    server: Bootstrap,
    reader: impl std::io::BufRead + Send + 'static,
    writer: impl std::io::Write,
    options: &stdio::Options,
) -> Result<(), ServeError> {
    let mut tools = super::catalog(None)
        .map_err(ServeError::Catalog)?
        .tools()
        .to_vec();
    tools.insert(0, tool());
    let catalog = Catalog::new(tools).map_err(ServeError::Catalog)?;
    let options = super::project_options(options, instructions(&server.input.project))?;
    stdio::serve(
        Arc::new(server),
        Arc::new(catalog),
        reader,
        writer,
        &options,
    )
    .map_err(ServeError::Transport)
}

#[cfg(all(test, unix))]
mod tests;
