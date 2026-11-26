//! Bounded, connection-owned input continuations; input never approves effects.
//! A form stays answerable until answered, refused or its connection ends.
use super::{Server, operation_call, requests_review, response};
use crate::review::{Context as Review, mcp::Question};
use canonic_mcp_kit::{
    RequestContext, RpcError, ToolCall,
    elicitation::{Action, Answer, Requests},
    rpc,
    stdio::{Context, Elicit, Outcome},
};
use ring::rand::{SecureRandom, SystemRandom};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

pub(super) struct Questions {
    pending: BTreeMap<String, Pending>,
    registry: BTreeMap<String, RegistryInput>,
    source_bytes: Arc<AtomicUsize>,
    pub(super) source_limit: usize,
}

impl Default for Questions {
    fn default() -> Self {
        Self {
            pending: BTreeMap::new(),
            registry: BTreeMap::new(),
            source_bytes: Arc::new(AtomicUsize::new(0)),
            source_limit: crate::policy::MAX_DOCUMENT_BYTES,
        }
    }
}

struct RegistryInput {
    owner: String,
    project: String,
    binding: String,
    handoff: Option<Value>,
}

/// Reserve capacity before showing a legacy form. An unanswered or declined
/// form drops its slot. A submitted answer keeps it until consumed or stale.
struct RegistryReservation {
    questions: Arc<Mutex<Questions>>,
    token: String,
}

impl RegistryReservation {
    fn new(
        questions: &Arc<Mutex<Questions>>,
        review: &Review,
        token: &str,
        binding: &str,
    ) -> Result<Self, crate::review::Error> {
        let mut state = questions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let outdated =
            |input: &RegistryInput| input.project == review.project_id && input.binding != binding;
        if state.registry.values().any(outdated) {
            // An older form may have prepared before a newer one reserved a slot.
            // Check the live binding under the mutex before evicting either one.
            crate::review::mcp::validate_registry_inputs(review, binding)?;
            state.registry.retain(|_, input| !outdated(input));
        }
        if state.registry.len() >= 8 {
            return Err(crate::review::Error::Overloaded);
        }
        state.registry.insert(
            token.into(),
            RegistryInput {
                owner: review.retry_owner(),
                project: review.project_id.clone(),
                binding: binding.into(),
                handoff: None,
            },
        );
        Ok(Self {
            questions: questions.clone(),
            token: token.into(),
        })
    }

    fn publish(&self, arguments: Value) -> Result<(), crate::review::Error> {
        let mut state = self
            .questions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let entry = state
            .registry
            .get_mut(&self.token)
            .ok_or(crate::review::Error::StaleSnapshot)?;
        entry.handoff = Some(arguments);
        Ok(())
    }
}

impl Drop for RegistryReservation {
    fn drop(&mut self) {
        let mut state = self
            .questions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state
            .registry
            .get(&self.token)
            .is_some_and(|input| input.handoff.is_none())
        {
            state.registry.remove(&self.token);
        }
    }
}

pub(super) fn use_registry_inputs(
    server: &Server,
    review: Arc<Review>,
    call: ToolCall,
    context: &Context,
) -> Outcome {
    let reference = call.arguments["review_ref"].as_str().unwrap_or_default();
    let input = server
        .questions
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take_registry_input(&review, reference);
    let result = input
        .ok_or(crate::review::Error::Missing)
        .and_then(|input| {
            if call.arguments["discard"] == true {
                Ok(None)
            } else {
                crate::review::mcp::validate_registry_inputs(&review, &input.binding)?;
                Ok(input.handoff)
            }
        });
    match result {
        Ok(Some(args)) => server.confirm_registry_source(review, call, args, context),
        Ok(None) => output(
            &review,
            &call.name,
            crate::review::mcp::registry_input_discarded(&review),
            context,
        ),
        Err(error) => output(
            &review,
            &call.name,
            crate::review::mcp::question_error(&review, &error),
            context,
        ),
    }
}

struct Capture {
    source_bytes: Arc<AtomicUsize>,
    bytes: usize,
}

impl Questions {
    fn take_registry_input(&mut self, review: &Review, reference: &str) -> Option<RegistryInput> {
        let input = self.registry.get(reference)?;
        if input.owner != review.retry_owner()
            || input.project != review.project_id
            || input.handoff.is_none()
        {
            return None;
        }
        self.registry.remove(reference)
    }

    fn capture(&self, bytes: usize) -> Result<Capture, RpcError> {
        self.source_bytes.try_update(Ordering::Relaxed, Ordering::Relaxed, |used| {
            used.checked_add(bytes).filter(|total| *total <= self.source_limit)
        }).map_err(|_| RpcError::new(rpc::INVALID_PARAMS,
            "Pending input forms reached the connection's captured policy source limit; answer or cancel an open form, then retry"))?;
        Ok(Capture {
            source_bytes: self.source_bytes.clone(),
            bytes,
        })
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        self.source_bytes.fetch_sub(self.bytes, Ordering::Relaxed);
    }
}
struct Pending {
    tool: String,
    owner: String,
    arguments: Value,
    plan: Question,
    requests: Requests,
    rounds: u8,
    trace: Option<crate::diagnostics::trace::Link>,
    _capture: Capture,
}

pub(super) fn owns(server: &Server, context: &Context) -> bool {
    context
        .request_context()
        .and_then(|request| request.request_state.as_ref())
        .is_some_and(|token| {
            server
                .questions
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .pending
                .contains_key(token)
        })
}

fn token() -> Result<String, RpcError> {
    let mut bytes = [0_u8; 32];
    SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| RpcError::new(rpc::INTERNAL_ERROR, "Form entropy unavailable"))?;
    Ok(crate::policy::fingerprint(&bytes))
}

pub(super) fn call(
    server: &Server,
    review: Arc<Review>,
    call: ToolCall,
    context: &Context,
) -> Outcome {
    if let Some(request) = context.request_context()
        && (request.request_state.is_some() || request.input_responses.is_some())
    {
        return modern_resume(server, review, &call, context, request);
    }
    let operation = operation_call(&call);
    let operation = operation.as_ref().unwrap_or(&call);
    let plan = match Question::prepare(&review, &operation.name, &operation.arguments) {
        Ok(plan) => plan,
        Err(error) => {
            return output(
                &review,
                &call.name,
                crate::review::mcp::question_error(&review, &error),
                context,
            );
        }
    };
    let token = match token() {
        Ok(token) => token,
        Err(error) => return Outcome::Method(Err(error)),
    };
    if !plan.needs_input() {
        let value = plan.propose(&review, &server.connection, &json!({}), &token, &|| {
            context
                .check()
                .map_err(|_| crate::policy::Error::Interrupted)
        });
        return server.review_guided_result(review, call, value, context);
    }
    let capture = match server
        .questions
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .capture(plan.source_bytes())
    {
        Ok(capture) => capture,
        Err(error) => return Outcome::Method(Err(error)),
    };
    if let Some(request) = context.request_context() {
        let requests = match Requests::new(vec![("settings".into(), plan.question().clone())]) {
            Ok(requests) => requests,
            Err(error) => return Outcome::Method(Err(error)),
        };
        let mut questions = server
            .questions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if questions.pending.len() >= 8 {
            return Outcome::Method(Err(RpcError::new(
                rpc::INVALID_PARAMS,
                "Too many pending input forms; answer or cancel an open one, then retry",
            )));
        }
        let result = match request.elicitation(requests.clone(), Some(&token)) {
            Ok(result) => result,
            Err(error) => return Outcome::Method(Err(error)),
        };
        questions.pending.insert(
            token,
            Pending {
                tool: call.name,
                owner: review.retry_owner(),
                arguments: call.arguments,
                plan,
                requests,
                rounds: 1,
                trace: crate::diagnostics::trace::current_link(),
                _capture: capture,
            },
        );
        return Outcome::InputRequired(result);
    }
    legacy_form(server, review, &call, context, plan, token, capture)
}

fn legacy_form(
    server: &Server,
    review: Arc<Review>,
    call: &ToolCall,
    context: &Context,
    plan: Question,
    token: String,
    capture: Capture,
) -> Outcome {
    let connection = server.connection.clone();
    let review_requested = requests_review(call);
    let reservation = if review_requested && let Some(binding) = plan.registry_binding() {
        match RegistryReservation::new(&server.questions, &review, &token, binding) {
            Ok(reservation) => Some(reservation),
            Err(error) => {
                return output(
                    &review,
                    &call.name,
                    crate::review::mcp::registry_input_error(&review, &error),
                    context,
                );
            }
        }
    } else {
        None
    };
    let trace = crate::diagnostics::trace::current();
    let waiting = trace
        .as_ref()
        .map(|trace| trace.wait("input.wait", "input.wait_us"));
    Outcome::Elicit(Elicit::new(
        plan.question().clone(),
        move |answer, context| {
            let _capture = capture;
            let review = super::progress::resume(review, context);
            drop(waiting);
            let _scope = trace
                .as_ref()
                .map(crate::diagnostics::trace::Request::enter);
            let mut value = finish(&review, &connection, &plan, &token, answer, context);
            if review_requested && value["error"].is_null() {
                legacy_review(&review, reservation.as_ref(), &mut value);
            }
            let outcome = output(&review, plan.tool(), value, context);
            if let Some(trace) = &trace {
                trace.finish(match &outcome {
                    Outcome::Tool(result) if result.is_error => "refused",
                    Outcome::Tool(_) => "success",
                    _ => "input_completed",
                });
            }
            outcome
        },
    ))
}

fn modern_resume(
    server: &Server,
    review: Arc<Review>,
    call: &ToolCall,
    context: &Context,
    request: &RequestContext,
) -> Outcome {
    let invalid = || {
        Outcome::Method(Err(RpcError::new(
            rpc::INVALID_PARAMS,
            "Invalid or consumed input form; start a new review",
        )))
    };
    let Some(token) = request.request_state.as_ref() else {
        return invalid();
    };
    let mut questions = server
        .questions
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(pending) = questions.pending.get_mut(token) else {
        return invalid();
    };
    if pending.owner != review.retry_owner() {
        return invalid();
    }
    if pending.tool != call.name || pending.arguments != call.arguments || pending.rounds >= 8 {
        questions.pending.remove(token);
        return invalid();
    }
    let answers = match request.elicitation_answers(&pending.requests) {
        Ok(answers) => answers,
        Err(error) => return Outcome::Method(Err(error)),
    };
    crate::diagnostics::trace::link_retry(pending.trace.as_ref());
    let Some(answer) = answers.get("settings") else {
        pending.rounds += 1;
        return match request.elicitation(pending.requests.clone(), Some(token)) {
            Ok(result) => {
                pending.trace = crate::diagnostics::trace::current_link();
                Outcome::InputRequired(result)
            }
            Err(error) => Outcome::Method(Err(error)),
        };
    };
    let answer = answer.clone();
    let pending = questions.pending.remove(token).expect("retained pack form");
    drop(questions);
    let value = finish(
        &review,
        &server.connection,
        &pending.plan,
        token,
        Answer::Response(answer),
        context,
    );
    server.review_guided_result(review, call.clone(), value, context)
}

fn finish(
    review: &Review,
    connection: &crate::review::mcp::Connection,
    plan: &Question,
    token: &str,
    answer: Answer,
    context: &Context,
) -> Value {
    match answer {
        Answer::Response(answer) if answer.action == Action::Accept => {
            let checkpoint = || {
                context
                    .check()
                    .map_err(|_| crate::policy::Error::Interrupted)
            };
            plan.propose(
                review,
                connection,
                &answer.content.unwrap_or(Value::Null),
                token,
                &checkpoint,
            )
        }
        other => {
            let (code, message) = match other {
                Answer::Response(answer) if answer.action == Action::Decline => (
                    "input_declined",
                    "Input declined; no change was prepared or approved. Do not retry this offer.",
                ),
                Answer::Response(_) => (
                    "input_cancelled",
                    "Input dismissed; no change was prepared or approved. Do not retry this offer.",
                ),
                Answer::Invalid => (
                    "invalid_input",
                    "Invalid input answer; no change was prepared or approved.",
                ),
                Answer::Unavailable => (
                    "input_unavailable",
                    "MCP input form unavailable. Collect explicit values from the person and use the tool's direct arguments; never infer missing answers or approval.",
                ),
            };
            json!({"schema_version":1,"project_id":review.project_id,"policy_revision":null,"data":null,"missing":[],"truncated":false,"redacted":false,"error":{"code":code,"message":message,"operator_route":null}})
        }
    }
}

fn legacy_review(review: &Review, reservation: Option<&RegistryReservation>, value: &mut Value) {
    let data = &mut value["data"];
    if data["status"] == "input_collected"
        && let Some(reservation) = reservation
    {
        if let Err(error) = reservation.publish(data["retry_arguments"].take()) {
            *value = crate::review::mcp::question_error(review, &error);
            return;
        }
        data.as_object_mut()
            .expect("registry input result")
            .remove("retry_arguments");
        data["review_action"] =
            json!({"tool":"registry_add","arguments":{"review_ref":reservation.token}});
        data["next_action"] = json!(
            "Call registry_add with the returned review_ref in this connection to review the captured registry source and trust scope. Pass discard: true with that reference to release it instead."
        );
    } else if data["apply_arguments"].is_object() && data["proposal_id"].is_string() {
        data.as_object_mut()
            .expect("prepared policy review")
            .remove("apply_arguments");
        data["review_action"] =
            json!({"tool":"policy_review","arguments":{"proposal_id":data["proposal_id"]}});
        data["next_action"] =
            json!("Call policy_review with the proposal_id for human confirmation.");
    }
}

pub(super) fn output(review: &Review, name: &str, value: Value, context: &Context) -> Outcome {
    let failed = !value["error"].is_null();
    Outcome::Tool(response::tool_result(
        name,
        review.agent.as_deref(),
        value,
        failed,
        &|| context.check(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_handoffs_keep_capacity_until_consumed_or_discarded() {
        let questions = Arc::new(Mutex::new(Questions::default()));
        let review = Review::new("project".into(), "/acme".into(), "mcp");
        let pending = RegistryReservation::new(&questions, &review, "pending", "binding").unwrap();
        for token in ["first", "one", "two", "three", "four", "five", "six"] {
            RegistryReservation::new(&questions, &review, token, "binding")
                .unwrap()
                .publish(json!({"name":"acme"}))
                .unwrap();
        }
        assert!(matches!(
            RegistryReservation::new(&questions, &review, "new", "binding"),
            Err(crate::review::Error::Overloaded)
        ));
        let mut state = questions.lock().unwrap();
        assert!(state.registry.contains_key("pending"));
        let first = state.take_registry_input(&review, "first").unwrap();
        assert_eq!(first.handoff, Some(json!({"name":"acme"})));
        assert!(state.take_registry_input(&review, "first").is_none());
        drop(state);
        let replacement =
            RegistryReservation::new(&questions, &review, "replacement", "binding").unwrap();
        drop((pending, replacement));
    }

    #[test]
    fn capture_budget_releases_only_when_its_owner_drops() {
        let questions = Questions {
            source_limit: 10,
            ..Questions::default()
        };
        let first = questions.capture(6).unwrap();
        let second = questions.capture(4).unwrap();
        assert!(questions.capture(1).is_err());
        assert!(questions.capture(usize::MAX).is_err());
        drop(first);
        let replacement = questions.capture(6).unwrap();
        assert!(questions.capture(1).is_err());
        drop(second);
        drop(replacement);
        assert_eq!(questions.source_bytes.load(Ordering::Relaxed), 0);
        assert!(questions.capture(10).is_ok());
    }
}
