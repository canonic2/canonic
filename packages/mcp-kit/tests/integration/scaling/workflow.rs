use crate::fixture::{Events, Host, Latch, Script, WAIT, arguments, text};
use canonic_mcp_kit::{
    Decision,
    elicitation::{Form, Request as Question},
    modern, rpc,
    stdio::{Confirm, Elicit, Outcome},
};
use serde_json::{Value, json};
use std::sync::{Arc, Weak, mpsc};

pub(super) fn script(owners: mpsc::Sender<Weak<()>>, events: Events) -> Script {
    let question = Question::form(
        "Continue Acme?",
        Form::new(json!({"type":"object","properties":{}})).expect("valid form"),
    )
    .expect("valid question");
    Script::new(move |request, context| {
        match arguments(&request)["message"]
            .as_str()
            .expect("fixture action")
        {
            "confirm" => {
                let token = Arc::new(());
                owners
                    .send(Arc::downgrade(&token))
                    .expect("ownership receiver");
                Outcome::Confirm(Confirm::new("Continue Acme?", move |_, _| {
                    drop(token);
                    text("done")
                }))
            }
            "rich" => {
                let token = Arc::new(());
                owners
                    .send(Arc::downgrade(&token))
                    .expect("ownership receiver");
                let next = question.clone();
                Outcome::Elicit(Elicit::new(question.clone(), move |_, _| {
                    Outcome::Elicit(Elicit::new(next, move |_, _| {
                        drop(token);
                        text("done")
                    }))
                }))
            }
            "modern" => {
                let protocol = context.request_context().expect("modern context");
                if protocol.request_state.is_none() {
                    Outcome::InputRequired(
                        protocol
                            .confirmation("Continue Acme?", "acme-state")
                            .expect("eligible interim"),
                    )
                } else {
                    assert_eq!(protocol.request_state.as_deref(), Some("acme-state"));
                    assert_eq!(
                        protocol.confirmation_decision().expect("valid answer"),
                        Some(Decision::Accepted)
                    );
                    text("done")
                }
            }
            "deadline" => {
                events.record("deadline worker started");
                Latch::default().wait(context);
                text("late")
            }
            "echo" => text("done"),
            other => panic!("unknown fixture action {other}"),
        }
    })
}

pub(super) fn send(host: &Host, id: &str, action: &str, modern: bool) {
    let key = if action == "echo" {
        "ordinary"
    } else {
        "shared"
    };
    let mut params = json!({"name":"acme_echo","arguments":{"message":action,"key":key}});
    if modern {
        metadata(&mut params);
    }
    host.send(&rpc::request(json!(id), "tools/call", params));
}

fn metadata(params: &mut Value) {
    params["_meta"] = json!({modern::VERSION_KEY:modern::PROTOCOL_VERSION,
        modern::CAPABILITIES_KEY:{"elicitation":{"form":{}}}});
}

pub(super) fn answer(host: &Host, form: &Value) {
    assert_eq!(form["method"], "elicitation/create");
    host.send(&rpc::success(
        form["id"].clone(),
        json!({"action":"accept","content":{}}),
    ));
}

pub(super) fn cancel(host: &Host, id: &str, form: &Value) {
    host.send(&rpc::notification(
        "notifications/cancelled",
        json!({"requestId":id}),
    ));
    let notice = host.recv();
    assert_eq!(notice["method"], "notifications/cancelled");
    assert_eq!(notice["params"]["requestId"], form["id"]);
    answer(host, form); // A late answer cannot invoke the discarded continuation.
    host.assert_quiet();
}

pub(super) fn modern_round(host: &Host, retry: bool) -> Value {
    let mut params = json!({"name":"acme_echo","arguments":{"message":"modern"}});
    metadata(&mut params);
    if retry {
        params["requestState"] = json!("acme-state");
        params["inputResponses"] = json!({"confirmation":{"action":"accept","content":{}}});
    }
    host.send(&rpc::request(json!("sample-modern"), "tools/call", params));
    host.recv()
}

pub(super) fn owner(owners: &mpsc::Receiver<Weak<()>>) -> Weak<()> {
    owners
        .recv_timeout(WAIT)
        .expect("captured continuation owner")
}
