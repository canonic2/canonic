//! Acme report preview with structured choices and consumer-owned modern retry state.
use canonic_mcp_kit::{
    Catalog, Client, Content, ModernOptions, RpcError, ServerInfo, Tool, ToolResult,
    elicitation::{Action, Answer, Form, Request as Question, Requests},
    stdio::{self, Context, Elicit, Handler, Options, Outcome, Request},
};
use serde_json::{Value, json};
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

struct Pending {
    token: String,
    arguments: Value,
    tool: String,
    expires: Instant,
    rounds: u8,
}

struct Acme {
    pending: Mutex<Option<Pending>>,
    report: Question,
    connect: Question,
}

fn result(message: impl Into<String>) -> Outcome {
    Outcome::Tool(ToolResult::success(vec![Content::text(message)]))
}

fn answered(answer: Answer) -> Outcome {
    match answer {
        Answer::Response(response) if response.action == Action::Accept => {
            response.content.map_or_else(
                || result("External interaction consented to; completion is still unverified. No connection was established."),
                |content| result(format!("Acme report preview: {content}")),
            )
        }
        other => result(format!("No report or external action was performed: {other:?}")),
    }
}

impl Handler for Acme {
    fn initialize(&self, _: &Client) -> Result<ServerInfo, RpcError> {
        Ok(ServerInfo::new("acme", "1"))
    }

    fn call(&self, request: Request, context: &Context) -> Outcome {
        let Request::Tool(call) = request else {
            return Outcome::Method(Err(RpcError::new(-32601, "Method not found")));
        };
        let question = if call.name == "acme_report" {
            &self.report
        } else {
            &self.connect
        };
        let Some(context) = context.request_context() else {
            let legacy = if call.name == "acme_connect" {
                token().and_then(|id| {
                    Question::url(
                        "Open the illustrative Acme connection page",
                        "https://example.com/connect",
                        Some(&id),
                    )
                })
            } else {
                Ok(question.clone())
            };
            return legacy.map_or_else(
                |error| Outcome::Method(Err(error)),
                |question| Outcome::Elicit(Elicit::new(question, |answer, _| answered(answer))),
            );
        };
        let requests = match Requests::new(vec![("details".into(), question.clone())]) {
            Ok(requests) => requests,
            Err(error) => return Outcome::Method(Err(error)),
        };
        // A private retained token binds exact arguments, operation and expiry. It grants no authority.
        let mut pending = self.pending.lock().expect("Acme retry state");
        let make_interim = |state: &str| {
            context
                .elicitation(requests.clone(), Some(state))
                .map_or_else(|error| Outcome::Method(Err(error)), Outcome::InputRequired)
        };
        if let Some(token) = &context.request_state {
            let Some(state) = pending.as_mut().filter(|state| {
                state.token == *token
                    && state.tool == call.name
                    && state.arguments == call.arguments
                    && state.expires > Instant::now()
                    && state.rounds < 8
            }) else {
                return Outcome::Method(Err(RpcError::new(
                    -32602,
                    "Invalid, expired or consumed Acme state",
                )));
            };
            let answers = match context.elicitation_answers(&requests) {
                Ok(answers) => answers,
                Err(error) => return Outcome::Method(Err(error)),
            };
            let Some(answer) = answers.get("details") else {
                state.rounds += 1;
                return make_interim(&state.token);
            };
            let answer = answer.clone();
            *pending = None; // Consume under the same lock before the continuation can run.
            return answered(Answer::Response(answer));
        }
        if context.input_responses.is_some() {
            return Outcome::Method(Err(RpcError::new(
                -32602,
                "Acme input requires its issued state",
            )));
        }
        if pending
            .as_ref()
            .is_some_and(|state| state.expires > Instant::now())
        {
            return Outcome::Method(Err(RpcError::new(
                -32000,
                "One Acme interaction is already pending",
            )));
        }
        let token = match token() {
            Ok(token) => token,
            Err(error) => return Outcome::Method(Err(error)),
        };
        let result = make_interim(&token);
        if matches!(result, Outcome::InputRequired(_)) {
            *pending = Some(Pending {
                token,
                arguments: call.arguments,
                tool: call.name,
                expires: Instant::now() + Duration::from_secs(300),
                rounds: 1,
            });
        }
        result
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let report = Question::form(
        "Choose the Acme report format and sections",
        Form::new(json!({
            "type":"object","properties":{
                "format":{"type":"string","oneOf":[{"const":"md","title":"Markdown"},{"const":"json","title":"JSON"}]},
                "sections":{"type":"array","items":{"type":"string","enum":["summary","details"]},"minItems":1,"maxItems":2}
            },"required":["format","sections"]
        })).map_err(startup_error)?,
    ).map_err(startup_error)?;
    let connect = Question::url(
        "Open the illustrative Acme connection page",
        "https://example.com/connect",
        Some("acme-connect"),
    )
    .map_err(startup_error)?;
    let schema = json!({"type":"object","additionalProperties":false});
    let catalog = Catalog::new(vec![
        Tool::new(
            "acme_report",
            "Preview an Acme report using the user's selections.",
            schema.clone(),
        ),
        Tool::new(
            "acme_connect",
            "Demonstrate external consent without establishing a connection.",
            schema,
        ),
    ])?;
    let server = Acme {
        pending: Mutex::new(None),
        report,
        connect,
    };
    let mut options = Options::default();
    options.session.modern = Some(ModernOptions::new(ServerInfo::new("acme", "1")));
    stdio::serve(
        Arc::new(server),
        Arc::new(catalog),
        std::io::BufReader::new(std::io::stdin()),
        std::io::stdout(),
        &options,
    )?;
    Ok(())
}

fn startup_error(error: RpcError) -> std::io::Error {
    std::io::Error::other(error.message)
}

fn token() -> Result<String, RpcError> {
    let mut entropy = [0_u8; 32];
    getrandom::fill(&mut entropy)
        .map_err(|_| RpcError::new(-32603, "Cannot create private Acme interaction state"))?;
    let hex = b"0123456789abcdef";
    Ok(entropy
        .iter()
        .flat_map(|byte| {
            [
                char::from(hex[usize::from(byte >> 4)]),
                char::from(hex[usize::from(byte & 15)]),
            ]
        })
        .collect())
}
