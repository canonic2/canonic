//! Acme report preview with structured choices and consumer-owned modern retry state.
use canonic_mcp_kit::{
    Catalog, Client, Content, ModernOptions, RpcError, ServerInfo, Tool, ToolResult,
    elicitation::{Action, Answer, Form, Request as Question, Requests},
    stdio::{self, Context, Elicit, Handler, Options, Outcome, Request},
};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

struct Pending {
    token: String,
    arguments: Value,
    tool: String,
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
        // Connection-owned state binds exact arguments and operation, never authority.
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
            }) else {
                return Outcome::Method(Err(RpcError::new(
                    -32602,
                    "Invalid or consumed Acme state",
                )));
            };
            let answers = match context.elicitation_answers(&requests) {
                Ok(answers) => answers,
                Err(error) => return Outcome::Method(Err(error)),
            };
            let Some(answer) = answers.get("details") else {
                if state.rounds >= 8 {
                    *pending = None;
                    return Outcome::Method(Err(RpcError::new(
                        -32603,
                        "Acme input round limit exceeded",
                    )));
                }
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
        if pending.is_some() {
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

#[cfg(test)]
#[allow(dead_code)]
#[path = "../tests/integration/fixture.rs"]
mod fixture;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_input_lifetime_example_last_round_accepts_or_releases_exhausted_state() {
        for accepted in [true, false] {
            let report = Question::form(
                "Choose?",
                Form::new(json!({"type":"object","properties":{}})).unwrap(),
            )
            .unwrap();
            let server = Acme {
                pending: Mutex::new(Some(Pending {
                    token: "issued-state".into(),
                    arguments: json!({}),
                    tool: "acme_report".into(),
                    rounds: 8,
                })),
                connect: report.clone(),
                report,
            };
            let catalog = Arc::new(
                Catalog::new(vec![Tool::new(
                    "acme_report",
                    "Preview report",
                    json!({"type":"object"}),
                )])
                .unwrap(),
            );
            let mut options = Options::default();
            options.session.modern = Some(ModernOptions::new(ServerInfo::new("acme", "1")));
            let host = fixture::Host::start_with(Arc::new(server), catalog, options);
            let call = |id, retry| {
                let mut params = json!({"name":"acme_report","arguments":{},"_meta":{
                    "io.modelcontextprotocol/protocolVersion":"2026-07-28",
                    "io.modelcontextprotocol/clientCapabilities":{"elicitation":{"form":{}}}}});
                if retry {
                    params["requestState"] = json!("issued-state");
                    params["inputResponses"] = if accepted {
                        json!({"details":{"action":"accept","content":{}}})
                    } else {
                        json!({})
                    };
                }
                json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":params})
            };
            host.send(&call(1, true));
            let reply = host.recv();
            if accepted {
                assert!(fixture::result_text(&reply).starts_with("Acme report preview:"));
            } else {
                assert_eq!(reply["error"]["code"], -32603);
            }
            // Neither a consumed answer nor round exhaustion leaves the slot occupied.
            host.send(&call(2, false));
            assert_eq!(host.recv()["result"]["resultType"], "input_required");
            host.finish().0.unwrap();
        }
    }
}
