//! Bounded, connection-owned pack form continuations; input never approves activation.
use super::{Server, response};
use crate::review::{Context as Review, mcp::PackQuestion};
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
    sync::Arc,
    time::{Duration, Instant},
};

#[derive(Default)]
pub(super) struct Questions {
    pending: BTreeMap<String, Pending>,
}

struct Pending {
    tool: String,
    arguments: Value,
    plan: PackQuestion,
    requests: Requests,
    expires: Instant,
    rounds: u8,
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
    let plan = match PackQuestion::prepare(&review, &call.name, &call.arguments) {
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
    if !plan.needs_input {
        return output(
            &review,
            plan.tool,
            plan.propose(&review, &server.connection, &json!({}), &token, &|| {
                context
                    .check()
                    .map_err(|_| crate::policy::Error::Interrupted)
            }),
            context,
        );
    }
    if let Some(request) = context.request_context() {
        let requests = match Requests::new(vec![("settings".into(), plan.question.clone())]) {
            Ok(requests) => requests,
            Err(error) => return Outcome::Method(Err(error)),
        };
        let mut questions = server
            .questions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        questions
            .pending
            .retain(|_, pending| pending.expires > Instant::now());
        if questions.pending.len() >= 8 {
            return Outcome::Method(Err(RpcError::new(
                rpc::INVALID_PARAMS,
                "Too many pending pack forms",
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
                expires: Instant::now() + Duration::from_secs(300),
                rounds: 1,
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
        plan.question.clone(),
        move |answer, context| {
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
            "Invalid, expired or consumed pack form; start a new settings review",
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
    if pending.expires <= Instant::now()
        || pending.tool != call.name
        || pending.arguments != call.arguments
        || pending.rounds >= 8
    {
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
    plan: &PackQuestion,
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
                    "Pack settings declined; no proposal was prepared. Do not retry this offer.",
                ),
                Answer::Response(_) => (
                    "input_cancelled",
                    "Pack settings dismissed; no proposal was prepared. Do not retry this offer.",
                ),
                Answer::Invalid => (
                    "invalid_input",
                    "Invalid pack settings answer; no proposal was prepared.",
                ),
                Answer::Unavailable => (
                    "input_unavailable",
                    "MCP pack form unavailable. Use shield pack add or shield pack configure in the user's terminal, or collect explicit values for ordinary policy_propose changes. Never infer missing answers.",
                ),
            };
            json!({"schema_version":1,"project_id":review.project_id,"policy_revision":null,"data":null,"missing":[],"truncated":false,"redacted":false,"error":{"code":code,"message":message,"operator_route":null}})
        }
    };
    output(review, plan.tool, value, context)
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
