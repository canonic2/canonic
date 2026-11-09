//! A connection can obtain human enrollment consent before project authority opens.
use super::{ServeError, Server, bundled_options, catalog, prompts};
use crate::{cli::ReviewContext, review::bootstrap::Plan};
use canonic_mcp_kit::{
    Annotations, Catalog, Client, Decision, RpcError, ServerInfo, Tool, ToolCall, ToolResult, rpc,
    stdio::{self, Confirm, Handler, Outcome, Request},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fmt::Write as _,
    path::PathBuf,
    sync::{Arc, Mutex, RwLock},
    time::{Duration, Instant},
};

#[cfg(test)]
mod tests;

const INSTRUCTIONS: &str = "This project needs enrollment review. Call status to inspect the location and policy, then project_enroll to ask the person to confirm. Existing approvals and registration records grant no authority here. After acceptance, ordinary Shield tools work on this same connection. Stop on decline.";

struct Pending {
    plan: Arc<Plan>,
    expires: Instant,
}

#[derive(Clone)]
struct Accepted {
    plan: Arc<Plan>,
    expires: Instant,
    inspect_only: bool,
}

#[derive(Default)]
struct Reviews {
    confirmations: super::confirmation::Confirmations,
    plans: BTreeMap<u64, Pending>,
    sequence: u64,
}

struct Bootstrap {
    project: PathBuf,
    attribution: Arc<ReviewContext>,
    current: Arc<RwLock<Option<Arc<Server>>>>,
    accepted: Arc<Mutex<Option<Accepted>>>,
    continuation: Arc<Mutex<()>>,
    reviews: Arc<Mutex<Reviews>>,
    client: Mutex<Option<Client>>,
    #[cfg(test)]
    fail_open_once: Arc<std::sync::atomic::AtomicBool>,
    #[cfg(test)]
    cancel_commit_once: Arc<std::sync::atomic::AtomicBool>,
}

impl Bootstrap {
    fn new(project: &std::path::Path, attribution: ReviewContext) -> Self {
        Self {
            project: project.to_path_buf(),
            attribution: Arc::new(attribution),
            current: Arc::new(RwLock::new(None)),
            accepted: Arc::new(Mutex::new(None)),
            continuation: Arc::new(Mutex::new(())),
            reviews: Arc::new(Mutex::default()),
            client: Mutex::new(None),
            #[cfg(test)]
            fail_open_once: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            #[cfg(test)]
            cancel_commit_once: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    fn active(&self) -> Option<Arc<Server>> {
        self.current
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    fn agent(&self) -> Option<&'static str> {
        let name = match self.attribution.agent.as_deref()? {
            "claude-code" => "claude",
            name => name,
        };
        crate::agents::find(name).map(|adapter| adapter.spec().id)
    }

    fn message(&self, plan: &Plan) -> String {
        let mut message = plan.message();
        if let Some(agent) = self.agent() {
            let _ = write!(
                message,
                "\n\nAlso connect this MCP session as {agent} for Shield's normal review controls. This confirms only this connection; it does not import the old agent registration or install hooks."
            );
        }
        message
    }

    fn finish(&self, plan: Arc<Plan>, decision: Decision, context: &stdio::Context) -> Outcome {
        if context.check().is_err() {
            return failure(
                "approval_cancelled",
                "Enrollment was cancelled. Nothing was approved.",
            );
        }
        if decision == Decision::Unavailable {
            return failure(
                "approval_unavailable",
                "This host did not provide enrollment confirmation. No project was enrolled.",
            );
        }
        if decision != Decision::Accepted {
            return failure(
                "approval_declined",
                "Enrollment was declined or cancelled. No project authority was imported. Continue without it; retry only when the person requests it.",
            );
        }
        let _continuation = self
            .continuation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(server) = self.active() {
            return enrolled(&server);
        }
        *self
            .accepted
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Accepted {
            plan,
            expires: Instant::now() + Duration::from_mins(5),
            inspect_only: false,
        });
        self.recover_locked(context).unwrap_or_else(|| failure("enrollment_stale", "The accepted enrollment no longer matches this project. Inspect status and request a fresh review."))
    }

    fn recover(&self, context: &stdio::Context) -> Option<Outcome> {
        let _continuation = self
            .continuation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.recover_locked(context)
    }

    /// Another confirmed writer may finish enrollment while this bootstrap
    /// connection is open. Only its validated receipt can open ordinary tools.
    fn adopt(&self) -> Option<Outcome> {
        let _continuation = self
            .continuation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(server) = self.active() {
            return Some(enrolled(&server));
        }
        let own = self
            .accepted
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        if own.is_some_and(|accepted| matches!(accepted.plan.committed(), Ok(Some(_)))) {
            return None;
        }
        if !crate::review::bootstrap::forms_reviewed_at(&self.project) {
            return None;
        }
        let input = ReviewContext {
            project: self.project.clone(),
            agent: self.attribution.agent.clone(),
            session_id: self.attribution.session_id.clone(),
            tool_use_id: self.attribution.tool_use_id.clone(),
        };
        match crate::mcp_io::review_context(&input) {
            Ok(review) => Some(self.install(review, false)),
            Err(error) => Some(failure("enrollment_unavailable", &error.to_string())),
        }
    }

    /// A fresh cache choice withdraws an uncommitted accepted attempt before
    /// asking a new question. A committed receipt always resumes its identity.
    fn supersede_cache_review(&self, request: &Request, context: &stdio::Context) -> bool {
        let Request::Tool(call) = request else {
            return false;
        };
        if call.name != "project_enroll"
            || enrollment_cache(call).ok().flatten().is_none()
            || context.request_context().is_some_and(|request| {
                request.request_state.is_some() || request.input_responses.is_some()
            })
        {
            return false;
        }
        let _continuation = self
            .continuation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let accepted = self
            .accepted
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        if accepted.is_some_and(|accepted| matches!(accepted.plan.committed(), Ok(None))) {
            self.accepted
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take();
            true
        } else {
            false
        }
    }

    fn recover_locked(&self, context: &stdio::Context) -> Option<Outcome> {
        if let Some(server) = self.active() {
            return Some(enrolled(&server));
        }
        let accepted = self
            .accepted
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()?;
        if context.check().is_err() {
            self.cancel_uncommitted();
            return Some(failure(
                "approval_cancelled",
                "Enrollment continuation was cancelled.",
            ));
        }
        let review = match accepted.plan.committed() {
            Ok(Some(review)) => Ok(review),
            Ok(None) if accepted.inspect_only || accepted.expires <= Instant::now() => {
                self.accepted
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .take();
                return None;
            }
            Ok(None) => accepted.plan.commit_checked(&|| {
                #[cfg(test)]
                if self
                    .cancel_commit_once
                    .load(std::sync::atomic::Ordering::Acquire)
                {
                    context.control().cancel();
                    self.cancel_commit_once
                        .store(false, std::sync::atomic::Ordering::Release);
                }
                context
                    .check()
                    .map_err(|_| crate::policy::Error::Interrupted)
            }),
            Err(error) => Err(error),
        };
        match review {
            Ok(review) => Some(self.install(review, true)),
            Err(crate::review::Error::StaleSnapshot) => {
                self.accepted
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .take();
                None
            }
            Err(crate::review::Error::Policy(crate::policy::Error::Interrupted)) => {
                self.cancel_uncommitted();
                Some(failure(
                    "approval_cancelled",
                    "Enrollment continuation was cancelled before publication. A fresh review is required.",
                ))
            }
            Err(error) => Some(failure("enrollment_failed", &error.to_string())),
        }
    }

    fn cancel_uncommitted(&self) {
        if let Some(accepted) = self
            .accepted
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_mut()
        {
            accepted.inspect_only = true;
        }
    }

    fn install(&self, mut review: crate::review::Context, reviewed_agent: bool) -> Outcome {
        let mut current = self
            .current
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(server) = current.as_ref() {
            return enrolled(server);
        }
        review.confirmed_mcp_forms = true;
        if reviewed_agent {
            review.agent = self.agent().map(str::to_owned);
            review.declared_agent.clone_from(&self.attribution.agent);
        }
        review.session_id.clone_from(&self.attribution.session_id);
        review.tool_use_id.clone_from(&self.attribution.tool_use_id);
        if let Some(client) = self
            .client
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
        {
            review.client = Some(client.name.clone());
            review.host_version = Some(client.version.clone());
        }
        #[cfg(test)]
        if self
            .fail_open_once
            .swap(false, std::sync::atomic::Ordering::AcqRel)
        {
            return failure(
                "enrollment_failed",
                "fixture connection interruption after enrollment",
            );
        }
        match Server::for_review(review) {
            Ok(server) => {
                let result = enrolled(&server);
                *current = Some(Arc::new(server));
                self.accepted
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .take();
                *self
                    .reviews
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = Reviews::default();
                result
            }
            Err(error) => failure("enrollment_failed", &error.to_string()),
        }
    }

    fn modern(
        &self,
        plan: Arc<Plan>,
        call: &ToolCall,
        request: &canonic_mcp_kit::RequestContext,
    ) -> Outcome {
        let mut reviews = self
            .reviews
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        reviews
            .plans
            .retain(|_, pending| pending.expires > Instant::now());
        reviews.sequence = match reviews.sequence.checked_add(1) {
            Some(sequence) => sequence,
            None => {
                return failure(
                    "approval_unavailable",
                    "Enrollment review capacity is unavailable.",
                );
            }
        };
        let binding = reviews.sequence;
        match reviews
            .confirmations
            .begin(request, call, &self.message(&plan), Some(binding), None)
        {
            Ok(interim) => {
                reviews.plans.insert(
                    binding,
                    Pending {
                        plan,
                        expires: Instant::now() + Duration::from_mins(5),
                    },
                );
                Outcome::InputRequired(interim)
            }
            Err(_) => failure(
                "approval_unavailable",
                "This host did not provide enrollment confirmation. No project was enrolled.",
            ),
        }
    }

    fn resume(&self, call: &ToolCall, context: &stdio::Context) -> Option<Outcome> {
        let request = context.request_context()?;
        if request.request_state.is_none() && request.input_responses.is_none() {
            return None;
        }
        let (answer, pending) = {
            let mut reviews = self
                .reviews
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let binding = reviews.confirmations.review_binding(request);
            let answer = reviews.confirmations.resume(request, call);
            let pending = binding.and_then(|binding| reviews.plans.remove(&binding));
            (answer, pending)
        };
        Some(match (answer, pending) {
            (Ok((decision, _)), Some(pending)) if pending.expires > Instant::now() => {
                self.finish(pending.plan, decision, context)
            }
            _ => failure(
                "invalid_confirmation",
                "Enrollment confirmation is invalid, expired or consumed. Inspect status before requesting a fresh review.",
            ),
        })
    }
}

impl Handler for Bootstrap {
    fn admit(&self, request: &Request) -> stdio::Admission {
        stdio::Admission {
            lane: match request {
                Request::Task(_) => 2,
                Request::Resource(_) => 1,
                Request::Method { method, .. } if method == "resources/read" => 1,
                _ => 0,
            },
            ..stdio::Admission::default()
        }
    }
    fn initialize(&self, client: &Client) -> Result<ServerInfo, RpcError> {
        *self
            .client
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(client.clone());
        Ok(ServerInfo::new("shield", env!("CARGO_PKG_VERSION")).with_instructions(INSTRUCTIONS))
    }

    fn call(&self, request: Request, context: &stdio::Context) -> Outcome {
        if let Some(server) = self.active() {
            return delegate(&server, request, context);
        }
        if matches!(request, Request::Task(_)) {
            return Outcome::Task(Err(canonic_mcp_kit::task::not_found()));
        }
        if let Request::Method { method, params } = &request
            && method == "resources/read"
        {
            return Outcome::ResourceNotFound(params["uri"].as_str().unwrap_or_default().into());
        }
        if let Some(outcome) = self.adopt() {
            if let Some(server) = self.active() {
                return delegate(&server, request, context);
            }
            return outcome;
        }
        if !self.supersede_cache_review(&request, context)
            && let Some(outcome) = self.recover(context)
        {
            if let Some(server) = self.active() {
                return delegate(&server, request, context);
            }
            return outcome;
        }
        if let Request::Prompt(call) = &request {
            return prompts::render(call);
        }
        let Request::Tool(call) = request else {
            return Outcome::Method(Err(RpcError::new(
                rpc::METHOD_NOT_FOUND,
                "Method not found",
            )));
        };
        if let Some(outcome) = self.resume(&call, context) {
            return outcome;
        }
        let cache = if call.name == "project_enroll" {
            match enrollment_cache(&call) {
                Ok(cache) => cache,
                Err(message) => return failure("invalid_input", message),
            }
        } else {
            None
        };
        if !matches!(call.name.as_str(), "status" | "project_enroll") {
            return failure(
                "enrollment_required",
                "This location has no usable enrollment. Call status, then project_enroll for in-session human review.",
            );
        }
        if call.name == "status" {
            return match crate::review::bootstrap::inspect(&self.project) {
                Ok(probe) => success(probe.report()),
                Err(error) => failure("enrollment_unavailable", &error.to_string()),
            };
        }
        let plan = match crate::review::bootstrap::prepare_options(
            &self.project,
            Some(&self.attribution.project),
            cache,
        ) {
            Ok(plan) => Arc::new(plan),
            Err(error) => return failure("enrollment_unavailable", &error.to_string()),
        };
        if let Some(request) = context.request_context() {
            return self.modern(plan, &call, request);
        }
        let message = self.message(&plan);
        let pending = self.current.clone();
        let accepted = self.accepted.clone();
        let continuation = self.continuation.clone();
        let reviews = self.reviews.clone();
        let attribution = self.attribution.clone();
        let project = self.project.clone();
        #[cfg(test)]
        let fail_open_once = self.fail_open_once.clone();
        #[cfg(test)]
        let cancel_commit_once = self.cancel_commit_once.clone();
        let client = self
            .client
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        Outcome::Confirm(Confirm::new(message, move |decision, context| {
            let bootstrap = Self {
                project,
                attribution,
                current: pending,
                accepted,
                continuation,
                reviews,
                client: Mutex::new(client),
                #[cfg(test)]
                fail_open_once,
                #[cfg(test)]
                cancel_commit_once,
            };
            bootstrap.finish(plan, decision, context)
        }))
    }
}

fn envelope(data: Value, error: Value) -> Value {
    let mut value = json!({"schema_version":1,"project_id":null,"policy_revision":null,"data":null,"missing":[],"truncated":false,"redacted":false,"error":null});
    value["data"] = data;
    value["error"] = error;
    value
}

fn enrollment_cache(call: &ToolCall) -> Result<Option<&std::path::Path>, &'static str> {
    const INVALID: &str = "project_enroll accepts only an optional absolute cache_dir; the reviewed project and policy come from this connection.";
    let arguments = call.arguments.as_object().ok_or(INVALID)?;
    if arguments.keys().any(|key| key != "cache_dir") {
        return Err(INVALID);
    }
    let Some(value) = arguments.get("cache_dir") else {
        return Ok(None);
    };
    let path = value
        .as_str()
        .filter(|path| !path.is_empty() && path.len() <= 4096 && !path.contains('\0'))
        .ok_or(INVALID)?;
    let path = std::path::Path::new(path);
    if !path.is_absolute() {
        return Err(INVALID);
    }
    Ok(Some(path))
}

fn success(data: Value) -> Outcome {
    Outcome::Tool(ToolResult::structured(envelope(data, Value::Null), false))
}

fn failure(code: &str, message: &str) -> Outcome {
    Outcome::Tool(ToolResult::structured(
        envelope(
            Value::Null,
            json!({"code":code,"message":message,"operator_route":null}),
        ),
        true,
    ))
}

fn enrolled(server: &Server) -> Outcome {
    let review = server.review();
    success(
        json!({"status":"enrolled","project_id":review.project_id,"project_root":review.project_root,"next_action":"Ordinary Shield tools are available on this connection. Use mcp_setup for a separate review of agent registrations."}),
    )
}

fn delegate(server: &Server, request: Request, context: &stdio::Context) -> Outcome {
    if matches!(&request, Request::Tool(call) if call.name == "project_enroll") {
        enrolled(server)
    } else {
        server.call(request, context)
    }
}

pub(crate) fn serve(
    project: &std::path::Path,
    attribution: ReviewContext,
    input: impl std::io::BufRead + Send + 'static,
    output: impl std::io::Write,
    options: &stdio::Options,
) -> Result<(), ServeError> {
    serve_handler(Bootstrap::new(project, attribution), input, output, options)
}

#[cfg(test)]
pub(super) fn serve_recovery_fixture(
    project: &std::path::Path,
    attribution: ReviewContext,
    input: impl std::io::BufRead + Send + 'static,
    output: impl std::io::Write,
    options: &stdio::Options,
) -> Result<(), ServeError> {
    let bootstrap = Bootstrap::new(project, attribution);
    bootstrap
        .fail_open_once
        .store(true, std::sync::atomic::Ordering::Release);
    serve_handler(bootstrap, input, output, options)
}

#[cfg(test)]
pub(super) fn serve_cancellation_fixture(
    project: &std::path::Path,
    attribution: ReviewContext,
    cancel_commit_once: Arc<std::sync::atomic::AtomicBool>,
    input: impl std::io::BufRead + Send + 'static,
    output: impl std::io::Write,
    options: &stdio::Options,
) -> Result<(), ServeError> {
    let mut bootstrap = Bootstrap::new(project, attribution);
    bootstrap.cancel_commit_once = cancel_commit_once;
    serve_handler(bootstrap, input, output, options)
}

fn serve_handler(
    bootstrap: Bootstrap,
    input: impl std::io::BufRead + Send + 'static,
    output: impl std::io::Write,
    options: &stdio::Options,
) -> Result<(), ServeError> {
    let mut tools = catalog(None).map_err(ServeError::Catalog)?.tools().to_vec();
    // This restricted connection has no trusted project identity before consent.
    for tool in &mut tools {
        if let Some(output) = tool.output_schema.as_mut()
            && let Some(project) = output["properties"].get("project_id").cloned()
        {
            output["properties"]["project_id"] = json!({"oneOf":[{"type":"null"},project]});
        }
    }
    tools.push(Tool::new("project_enroll", "Review this location's policy and confirm a fresh enrollment; optional absolute cache_dir selects its derived cache", json!({"type":"object","properties":{"cache_dir":{"type":"string","minLength":1,"maxLength":4096}},"additionalProperties":false})).with_annotations(Annotations {
        read_only_hint: Some(false), destructive_hint: Some(true), idempotent_hint: Some(true), open_world_hint: Some(false), ..Annotations::default()
    }));
    let catalog = Catalog::new(tools).map_err(ServeError::Catalog)?;
    let mut options = bundled_options(options)?;
    options.session.tasks = Some(super::tasks::policy());
    options
        .lanes
        .push(stdio::Lane::new("preparation-management", 2, 8));
    options.session.resources = Some(Arc::new(
        super::resources::catalog().map_err(ServeError::Resources)?,
    ));
    options.session.modern = Some(canonic_mcp_kit::ModernOptions::new(
        ServerInfo::new("shield", env!("CARGO_PKG_VERSION")).with_instructions(INSTRUCTIONS),
    ));
    stdio::serve(
        Arc::new(bootstrap),
        Arc::new(catalog),
        input,
        output,
        &options,
    )
    .map_err(ServeError::Transport)
}
