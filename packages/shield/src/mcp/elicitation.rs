//! Bounded, connection-owned input continuations; input never approves effects.
//! A form stays answerable until answered, refused or its connection ends.
use super::{Server, response};
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
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

pub(super) struct Questions {
    pending: BTreeMap<String, Pending>,
    source_bytes: Arc<AtomicUsize>,
    pub(super) source_limit: usize,
}

impl Default for Questions {
    fn default() -> Self {
        Self {
            pending: BTreeMap::new(),
            source_bytes: Arc::new(AtomicUsize::new(0)),
            source_limit: crate::policy::MAX_DOCUMENT_BYTES,
        }
    }
}

struct Capture {
    source_bytes: Arc<AtomicUsize>,
    bytes: usize,
}

impl Questions {
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
    arguments: Value,
    plan: Question,
    requests: Requests,
    rounds: u8,
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
        return modern_resume(server, &review, &call, context, request);
    }
    let plan = match Question::prepare(&review, &call.name, &call.arguments) {
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
        return output(
            &review,
            plan.tool(),
            plan.propose(&review, &server.connection, &json!({}), &token, &|| {
                context
                    .check()
                    .map_err(|_| crate::policy::Error::Interrupted)
            }),
            context,
        );
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
                arguments: call.arguments,
                plan,
                requests,
                rounds: 1,
                _capture: capture,
            },
        );
        return Outcome::InputRequired(result);
    }
    let connection = server.connection.clone();
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
            let outcome = finish(&review, &connection, &plan, &token, answer, context);
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
    review: &Review,
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
    if pending.tool != call.name || pending.arguments != call.arguments || pending.rounds >= 8 {
        questions.pending.remove(token);
        return invalid();
    }
    let answers = match request.elicitation_answers(&pending.requests) {
        Ok(answers) => answers,
        Err(error) => return Outcome::Method(Err(error)),
    };
    let Some(answer) = answers.get("settings") else {
        pending.rounds += 1;
        return match request.elicitation(pending.requests.clone(), Some(token)) {
            Ok(result) => Outcome::InputRequired(result),
            Err(error) => Outcome::Method(Err(error)),
        };
    };
    let answer = answer.clone();
    let pending = questions.pending.remove(token).expect("retained pack form");
    drop(questions);
    finish(
        review,
        &server.connection,
        &pending.plan,
        token,
        Answer::Response(answer),
        context,
    )
}

fn finish(
    review: &Review,
    connection: &crate::review::mcp::Connection,
    plan: &Question,
    token: &str,
    answer: Answer,
    context: &Context,
) -> Outcome {
    let value = match answer {
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
    };
    output(review, plan.tool(), value, context)
}

fn output(review: &Review, name: &str, value: Value, context: &Context) -> Outcome {
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
