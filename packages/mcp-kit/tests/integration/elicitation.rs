//! Rich input through the real shared driver and the independent pure core boundary.
use crate::fixture::{Host, Script, catalog, result_text, text};
use canonic_mcp_kit::{
    Inbound, ModernOptions, ServerInfo, Session, SessionOptions,
    elicitation::{Action, Answer, Form, Request as Question, Requests},
    modern, rpc,
    stdio::{Elicit, ElicitationPolicy, Options, Outcome},
};
use serde_json::{Value, json};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

fn question() -> Question {
    Question::form(
        "Select Acme report",
        Form::new(json!({"type":"object","properties":{
        "format":{"type":"string","enum":["md","json"]}},"required":["format"]}))
        .expect("valid Acme schema"),
    )
    .expect("valid Acme question")
}

fn rich() -> Script {
    Script::new(|_, _| {
        Outcome::Elicit(Elicit::new(question(), |answer, _| match answer {
            Answer::Response(response) if response.action == Action::Accept => {
                Outcome::Elicit(Elicit::new(question(), |answer, _| {
                    text(&format!("second: {answer:?}"))
                }))
            }
            other => text(&format!("{other:?}")),
        }))
    })
}

fn answer(host: &Host, question: &Value, value: Value) {
    let mut reply = json!({"jsonrpc":"2.0","id":question["id"]});
    reply["result"] = value;
    host.send(&reply);
}

#[test]
fn legacy_elicitation_multiround_keeps_worker_free_and_invalid_distinct() {
    let host = Host::ready(
        rich(),
        Options::default(),
        json!({"elicitation":{"form":{}}}),
    );
    host.call(1, "acme_change", json!({"key":"report"}));
    let first = host.recv();
    host.assert_quiet();
    answer(
        &host,
        &first,
        json!({"action":"accept","content":{"format":"json"}}),
    );
    let second = host.recv();
    assert_ne!(second["id"], first["id"]);
    answer(
        &host,
        &first,
        json!({"action":"accept","content":{"format":"md"}}),
    );
    host.assert_quiet();
    answer(
        &host,
        &second,
        json!({"action":"accept","content":{"format":"unknown"}}),
    );
    assert_eq!(result_text(&host.recv()), "second: Invalid");
    host.call(2, "acme_change", json!({"key":"report"}));
    answer(&host, &host.recv(), json!({"action":"cancel"}));
    assert!(result_text(&host.recv()).contains("Cancel"));
    host.finish().0.unwrap();
}

#[test]
fn legacy_elicitation_caps_total_time_cancellation_eof_and_overflow() {
    let options = Options {
        elicitation: ElicitationPolicy {
            max_rounds: 1,
            timeout: Duration::from_secs(2),
        },
        ..Options::default()
    };
    let host = Host::ready(rich(), options, json!({"elicitation":{}}));
    host.call(1, "acme_change", json!({}));
    answer(
        &host,
        &host.recv(),
        json!({"action":"accept","content":{"format":"md"}}),
    );
    assert_eq!(host.recv()["error"]["code"], -32603);
    host.call(2, "acme_change", json!({}));
    let pending = host.recv();
    host.cancel(2);
    assert_eq!(host.recv()["params"]["requestId"], pending["id"]);
    answer(
        &host,
        &pending,
        json!({"action":"accept","content":{"format":"md"}}),
    );
    host.assert_quiet();
    host.call(3, "acme_change", json!({}));
    let pending = host.recv();
    let (result, rest) = host.finish();
    result.unwrap();
    assert!(
        rest.iter()
            .any(|reply| reply["params"]["requestId"] == pending["id"])
    );
    assert!(rest.iter().any(|reply| reply["id"] == 3
        && reply["error"]["code"] == -32800
        && reply["error"]["message"] == "Connection closed before elicitation"));

    let host = Host::ready(
        rich(),
        Options {
            elicitation: ElicitationPolicy {
                max_rounds: 8,
                timeout: Duration::from_millis(30),
            },
            ..Options::default()
        },
        json!({"elicitation":{}}),
    );
    host.call(1, "acme_change", json!({}));
    let first = host.recv();
    if first["method"] == "elicitation/create" {
        assert_eq!(host.recv()["method"], "notifications/cancelled");
        let expired = host.recv();
        assert_eq!(expired["error"]["code"], -32001);
        assert_eq!(expired["error"]["message"], "Elicitation deadline exceeded");
    } else {
        // The same total deadline may expire before the first question is issued.
        assert_eq!(first["error"]["code"], -32001);
    }
    host.finish().0.unwrap();

    let script = Script::new(|_, _| {
        Outcome::Elicit(Elicit::new(
            Question::form(
                &"\"".repeat(1000),
                Form::new(json!({"type":"object","properties":{}})).unwrap(),
            )
            .unwrap(),
            |answer, _| text(&format!("{answer:?}")),
        ))
    });
    let host = Host::ready(
        script,
        Options {
            max_output_bytes: 1024,
            ..Options::default()
        },
        json!({"elicitation":{}}),
    );
    host.call(1, "acme_change", json!({}));
    assert_eq!(result_text(&host.recv()), "Unavailable");
    host.finish().0.unwrap();
}

fn frame(id: u64, params: Value, capabilities: Value) -> Value {
    let mut params = params;
    params["_meta"] = json!({modern::VERSION_KEY:modern::PROTOCOL_VERSION});
    params["_meta"][modern::CAPABILITIES_KEY] = capabilities;
    rpc::request(json!(id), "tools/call", params)
}

fn options() -> Options {
    let mut options = Options::default();
    options.session.modern = Some(ModernOptions::new(ServerInfo::new("acme", "1")));
    options
}

#[test]
fn modern_elicitation_state_binding_and_single_use_precede_effects() {
    let questions = Requests::new(vec![("report".into(), question())]).unwrap();
    let pending = Arc::new(Mutex::new(false));
    let issued = pending.clone();
    let script = Script::new(move |_, context| {
        let context = context.request_context().unwrap();
        if context.request_state.is_none() {
            let result = context
                .elicitation(questions.clone(), Some("retained-operation-1"))
                .unwrap();
            *issued.lock().unwrap() = true;
            return Outcome::InputRequired(result);
        }
        if context.request_state.as_deref() != Some("retained-operation-1")
            || !*issued.lock().unwrap()
        {
            return Outcome::Method(Err(RpcError::new(-32602, "Invalid or consumed state")));
        }
        match context.elicitation_answers(&questions) {
            Ok(answers) if answers.contains_key("report") => {
                *issued.lock().unwrap() = false;
                text("accepted exactly once")
            }
            Ok(_) => Outcome::InputRequired(
                context
                    .elicitation(questions.clone(), Some("retained-operation-1"))
                    .unwrap(),
            ),
            Err(error) => Outcome::Method(Err(error)),
        }
    });
    let host = Host::start(script, options());
    let capabilities = json!({"elicitation":{"form":{}}});
    let params = json!({"name":"acme_echo","arguments":{}});
    host.send(&frame(1, params.clone(), capabilities.clone()));
    let result = host.recv();
    assert_eq!(result["result"]["resultType"], "input_required");
    assert!(result["result"].get("ttlMs").is_none());
    let mut retry = params;
    retry["requestState"] = result["result"]["requestState"].clone();
    retry["inputResponses"] = json!({"report":{"action":"accept","content":{"format":"pdf"}}});
    host.send(&frame(2, retry.clone(), capabilities.clone()));
    assert_eq!(host.recv()["error"]["code"], -32602);
    retry["inputResponses"] = json!({"unknown":"opaque"});
    host.send(&frame(3, retry.clone(), capabilities.clone()));
    assert_eq!(host.recv()["result"]["resultType"], "input_required");
    retry["inputResponses"] = json!({"report":{"action":"accept","content":{"format":"md"}}});
    host.send(&frame(4, retry.clone(), json!({})));
    assert_eq!(host.recv()["error"]["code"], -32021);
    host.send(&frame(5, retry.clone(), capabilities.clone()));
    assert_eq!(result_text(&host.recv()), "accepted exactly once");
    host.send(&frame(6, retry, capabilities));
    assert_eq!(host.recv()["error"]["code"], -32602);
    host.finish().0.unwrap();
}

use canonic_mcp_kit::RpcError;

#[test]
fn independent_runtime_elicitation_preserves_modes_limits_and_interruption() {
    let mut session = Session::new(catalog(), options().session);
    let question = Question::url("Connect Acme", "https://example.com/connect", None).unwrap();
    let requests = Requests::new(vec![("login".into(), question)]).unwrap();
    let request = frame(
        1,
        json!({"name":"acme_echo"}),
        json!({"elicitation":{"url":{}}}),
    );
    assert!(matches!(
        session.receive_value(request),
        Inbound::Tool { .. }
    ));
    let context = session.request_context(&json!(1)).unwrap();
    let interim = context
        .elicitation(requests, Some("private-state"))
        .unwrap();
    let bytes = context
        .encode_interim(&json!(1), &interim, 4096, &|| Ok(()))
        .unwrap();
    assert!(session.finish(&json!(1)));
    let value: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        value["result"]["inputRequests"]["login"]["params"]["mode"],
        "url"
    );
    assert!(
        context
            .encode_interim(&json!(1), &interim, bytes.len() - 1, &|| Ok(()))
            .is_err()
    );
    assert!(
        context
            .encode_interim(&json!(1), &interim, 4096, &|| Err(
                canonic_mcp_kit::control::Interrupted
            ))
            .is_err()
    );
    let mut legacy = Session::new(catalog(), SessionOptions::default());
    legacy.receive_value(rpc::request(json!(1),"initialize",json!({"protocolVersion":"2025-06-18","clientInfo":{"name":"acme","version":"1"},"capabilities":{"elicitation":{}}})));
    legacy.accept_initialize(json!(1), &ServerInfo::new("acme", "1"));
    legacy.receive_value(rpc::notification("notifications/initialized", json!({})));
    let (id, request) = legacy.request_elicitation(question_for_legacy()).unwrap();
    assert!(request["params"].get("mode").is_none());
    assert!(matches!(
        legacy.receive_value(json!({"jsonrpc":"2.0","id":id,"result":{"action":"cancel"}})),
        Inbound::Elicitation {
            answer: Answer::Response(_),
            ..
        }
    ));
}

fn question_for_legacy() -> Question {
    question()
}

#[test]
fn rich_wait_and_confirmation_share_one_legacy_pending_slot() {
    use canonic_mcp_kit::stdio::Confirm;
    let script = Script::new(|request, _| {
        if crate::fixture::tool_name(&request) == "acme_echo" {
            Outcome::Confirm(Confirm::new("Continue?", |decision, _| {
                text(&format!("{decision:?}"))
            }))
        } else {
            Outcome::Elicit(Elicit::new(question(), |answer, _| {
                text(&format!("{answer:?}"))
            }))
        }
    });
    let host = Host::ready(script, Options::default(), json!({"elicitation":{}}));
    host.call(1, "acme_echo", json!({}));
    let pending = host.recv();
    host.call(2, "acme_change", json!({}));
    assert_eq!(result_text(&host.recv()), "Unavailable");
    answer(&host, &pending, json!({"action":"decline"}));
    assert_eq!(result_text(&host.recv()), "Declined");
    host.call(3, "acme_change", json!({}));
    let pending = host.recv();
    host.call(4, "acme_echo", json!({}));
    assert_eq!(result_text(&host.recv()), "Unavailable");
    answer(&host, &pending, json!({"action":"cancel"}));
    assert!(result_text(&host.recv()).contains("Cancel"));
    host.finish().0.unwrap();
}

#[test]
fn rich_elicitation_reports_the_unsupported_legacy_confirmation_transition() {
    use canonic_mcp_kit::stdio::Confirm;
    let script = Script::new(|_, _| {
        Outcome::Elicit(Elicit::new(question(), |_, _| {
            Outcome::Confirm(Confirm::new("Apply?", |_, _| {
                panic!("unsupported continuation must not run")
            }))
        }))
    });
    let host = Host::ready(script, Options::default(), json!({"elicitation":{}}));
    host.call(1, "acme_change", json!({}));
    answer(&host, &host.recv(), json!({"action":"decline"}));
    let reply = host.recv();
    assert_eq!(reply["error"]["code"], -32603);
    assert_eq!(
        reply["error"]["message"],
        "Rich elicitation cannot resume into legacy confirmation"
    );
    host.assert_quiet();
    host.finish().0.unwrap();
}

#[test]
fn invalid_rich_lifetime_policy_refuses_before_serving() {
    for policy in [
        ElicitationPolicy {
            max_rounds: 0,
            timeout: Duration::from_secs(1),
        },
        ElicitationPolicy {
            max_rounds: 65,
            timeout: Duration::from_secs(1),
        },
        ElicitationPolicy {
            max_rounds: 8,
            timeout: Duration::ZERO,
        },
        ElicitationPolicy {
            max_rounds: 8,
            timeout: Duration::from_secs(86401),
        },
    ] {
        let options = Options {
            elicitation: policy,
            ..Options::default()
        };
        assert!(
            canonic_mcp_kit::stdio::serve(
                Arc::new(rich()),
                catalog(),
                std::io::Cursor::new(Vec::<u8>::new()),
                Vec::<u8>::new(),
                &options
            )
            .is_err()
        );
    }
}

#[test]
fn legacy_url_consent_is_not_completion_and_missing_mode_is_unavailable() {
    let script = || {
        Script::new(|_, _| {
            Outcome::Elicit(Elicit::new(
                Question::url(
                    "Connect Acme",
                    "https://example.com/connect",
                    Some("acme-1"),
                )
                .unwrap(),
                |answer, _| match answer {
                    Answer::Response(response) if response.action == Action::Accept => {
                        assert!(response.content.is_none());
                        text("consented; external completion unverified")
                    }
                    other => text(&format!("{other:?}")),
                },
            ))
        })
    };
    let host = Host::ready(
        script(),
        Options::default(),
        json!({"elicitation":{"url":{}}}),
    );
    host.call(1, "acme_change", json!({}));
    let pending = host.recv();
    assert_eq!(pending["params"]["mode"], "url");
    assert_eq!(pending["params"]["elicitationId"], "acme-1");
    answer(&host, &pending, json!({"action":"accept"}));
    assert_eq!(
        result_text(&host.recv()),
        "consented; external completion unverified"
    );
    host.finish().0.unwrap();
    let host = Host::ready(script(), Options::default(), json!({"elicitation":{}}));
    host.call(1, "acme_change", json!({}));
    assert_eq!(result_text(&host.recv()), "Unavailable");
    host.finish().0.unwrap();
}

#[cfg(unix)]
mod records;

#[cfg(unix)]
#[test]
fn rich_tasks_reopen_consumer_records_and_do_not_reapply_consumed_input() {
    use canonic_mcp_kit::task::{Metadata, Snapshot, State, TaskId, TaskOptions, Timestamp};
    use records::{open, save};
    use std::{fs, path::PathBuf};
    struct Folder(PathBuf);
    impl Drop for Folder {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let mut entropy = [0_u8; 16];
    getrandom::fill(&mut entropy).unwrap();
    let hex = b"0123456789abcdef";
    let name = entropy
        .iter()
        .flat_map(|byte| {
            [
                char::from(hex[usize::from(byte >> 4)]),
                char::from(hex[usize::from(byte & 15)]),
            ]
        })
        .collect::<String>();
    let folder = Folder(std::env::temp_dir().join(format!("acme-rich-task-{name}")));
    fs::create_dir(&folder.0).unwrap();
    let path = folder.0.join("record.json");
    let now = Timestamp::parse("2026-10-31T12:00:00Z").unwrap();
    let snapshot = Snapshot::new(
        Metadata::new(TaskId::new("report-1").unwrap(), now.clone(), 60_000).unwrap(),
        State::RichInputRequired(Requests::new(vec![("report".into(), question())]).unwrap()),
    )
    .unwrap();
    save(&path, &snapshot);
    let mut options = options();
    options.session.tasks = Some(TaskOptions {
        max_records: 1,
        max_bytes: 4 * 1024 * 1024,
        max_running: 1,
        max_input_rounds: 8,
        execution_timeout: Duration::from_secs(60),
        input_timeout: Duration::from_secs(60),
    });
    let start = |path: PathBuf| records::start(path, options.clone());
    let send = |host: &Host, id, method, params| {
        let mut frame = frame(
            id,
            params,
            json!({"extensions":{"io.modelcontextprotocol/tasks":{}},"elicitation":{"form":{}}}),
        );
        frame["method"] = json!(method);
        host.send(&frame);
        host.recv()
    };
    let host = start(path.clone());
    assert_eq!(
        send(&host, 1, "tasks/get", json!({"taskId":"report-1"}))["result"]["status"],
        "input_required"
    );
    assert_eq!(
        send(
            &host,
            2,
            "tasks/update",
            json!({"taskId":"report-1","inputResponses":{"report":{"action":"accept","content":{"format":"bad"}}}})
        )["error"]["code"],
        -32602
    );
    assert_eq!(open(&path), snapshot);
    let responses = json!({"report":{"action":"accept","content":{"format":"json"}}});
    assert_eq!(
        send(
            &host,
            3,
            "tasks/update",
            json!({"taskId":"report-1","inputResponses":responses})
        )["result"]["resultType"],
        "complete"
    );
    host.finish().0.unwrap();
    let committed = fs::read(&path).unwrap();
    let host = start(path.clone());
    assert_eq!(
        send(&host, 4, "tasks/get", json!({"taskId":"report-1"}))["result"]["result"]["structuredContent"],
        json!({"format":"json"})
    );
    send(
        &host,
        5,
        "tasks/update",
        json!({"taskId":"report-1","inputResponses":responses}),
    );
    assert_eq!(fs::read(&path).unwrap(), committed);
    host.finish().0.unwrap();
}
