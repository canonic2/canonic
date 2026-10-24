//! Host confirmation through form elicitation.

use crate::fixture::{Events, Host, Script, arguments, result_text, text};
use canonic_mcp::{
    Decision,
    stdio::{Confirm, Direction, Observer, Options, Outcome},
};
use serde_json::{Value, json};
use std::{sync::Arc, thread, time::Duration};

/// Client capabilities that accept form confirmation.
fn form() -> Value {
    json!({"elicitation":{"form":{}}})
}

/// Asks to confirm, then reports the decision, recording when the continuation runs.
fn confirming(events: &Events) -> Script {
    let events = events.clone();
    Script::new(move |_, _| {
        let events = events.clone();
        Outcome::Confirm(Confirm::new("Delete the Acme data?", move |decision, _| {
            events.record(format!("{decision:?}"));
            text(&format!("{decision:?}"))
        }))
    })
}

fn answer(host: &Host, request: &Value, result: Value) {
    let mut reply = json!({"jsonrpc":"2.0","id":request["id"]});
    reply["result"] = result;
    host.send(&reply);
}

#[test]
fn accepted_and_declined_forms_resume_with_the_decision() {
    let events = Events::new();
    let host = Host::ready(confirming(&events), Options::default(), form());
    host.call(1, "acme_change", json!({}));
    let request = host.recv();
    assert_eq!(request["method"], "elicitation/create");
    assert_eq!(request["params"]["message"], "Delete the Acme data?");
    assert_eq!(request["params"]["mode"], "form");
    answer(&host, &request, json!({"action":"accept","content":{}}));
    assert_eq!(result_text(&host.recv()), "Accepted");

    host.call(2, "acme_change", json!({}));
    let request = host.recv();
    answer(&host, &request, json!({"action":"decline"}));
    assert_eq!(result_text(&host.recv()), "Declined");
    assert_eq!(events.next(), "Accepted");
    assert_eq!(events.next(), "Declined");
}

#[test]
fn oversized_confirmation_requests_resume_as_unavailable_and_release_the_slot() {
    let events = Events::new();
    let recorded = events.clone();
    let script = Script::new(move |request, _| {
        let message = if arguments(&request)["message"] == "large" {
            // Escaping makes this exceed the limit even though its raw length fits.
            "\"".repeat(600)
        } else {
            "Delete the Acme data?".into()
        };
        let recorded = recorded.clone();
        Outcome::Confirm(Confirm::new(message, move |decision, _| {
            recorded.record(format!("{decision:?}"));
            text(&format!("{decision:?}"))
        }))
    });
    let options = Options {
        max_output_bytes: 1024,
        ..Options::default()
    };
    let host = Host::ready(script, options, form());
    host.call(1, "acme_change", json!({"message":"large","key":"acme"}));
    let reply = host.recv();
    assert_eq!(reply["id"], 1);
    assert_eq!(result_text(&reply), "Unavailable");
    assert_eq!(events.next(), "Unavailable");
    host.call(2, "acme_change", json!({"key":"acme"}));
    let request = host.recv();
    assert_eq!(request["method"], "elicitation/create");
    answer(&host, &request, json!({"action":"accept"}));
    let reply = host.recv();
    assert_eq!(reply["id"], 2);
    assert_eq!(result_text(&reply), "Accepted");
    assert_eq!(events.next(), "Accepted");
    host.assert_quiet();
}

#[test]
fn hosts_without_forms_resume_as_unavailable() {
    let events = Events::new();
    let host = Host::ready(confirming(&events), Options::default(), json!({}));
    host.call(1, "acme_change", json!({}));
    assert_eq!(result_text(&host.recv()), "Unavailable");
}

#[test]
fn a_second_confirmation_waits_for_none_and_resumes_as_unavailable() {
    let events = Events::new();
    let options = Options {
        lanes: vec![canonic_mcp::stdio::Lane::new("tools", 2, 8)],
        ..Options::default()
    };
    let host = Host::ready(confirming(&events), options, form());
    host.call(1, "acme_change", json!({}));
    let request = host.recv();
    assert_eq!(request["method"], "elicitation/create");
    host.call(2, "acme_change", json!({}));
    let second = host.recv();
    assert_eq!(second["id"], 2);
    assert_eq!(result_text(&second), "Unavailable");
    answer(&host, &request, json!({"action":"accept"}));
    assert_eq!(result_text(&host.recv()), "Accepted");
}

#[test]
fn cancelling_a_waiting_request_withdraws_its_form() {
    let events = Events::new();
    let host = Host::ready(confirming(&events), Options::default(), form());
    host.call(1, "acme_change", json!({}));
    let request = host.recv();
    host.cancel(1);
    let notice = host.recv();
    assert_eq!(notice["method"], "notifications/cancelled");
    assert_eq!(notice["params"]["requestId"], request["id"]);
    answer(&host, &request, json!({"action":"accept"}));
    host.assert_quiet();
    assert_eq!(events.none_within(Duration::from_millis(50)), None);
}

#[test]
fn an_unanswered_form_expires_without_resuming() {
    let events = Events::new();
    let options = Options {
        confirmation_timeout: Duration::from_millis(30),
        ..Options::default()
    };
    let host = Host::ready(confirming(&events), options, form());
    host.call(1, "acme_change", json!({}));
    let request = host.recv();
    let notice = host.recv();
    assert_eq!(notice["method"], "notifications/cancelled");
    assert_eq!(notice["params"]["requestId"], request["id"]);
    let expired = host.recv();
    assert_eq!(expired["id"], 1);
    assert_eq!(
        expired["error"]["message"],
        "Confirmation deadline exceeded"
    );
    assert_eq!(events.none_within(Duration::from_millis(50)), None);
}

#[test]
fn an_answer_read_after_the_deadline_does_not_resume() {
    let events = Events::new();
    let timeout = Duration::from_millis(100);
    // Holding the dispatcher on the answer frame lets the deadline pass
    // after the last sweep but before the answer is processed.
    let observer: Observer = Arc::new(move |direction, frame: &[u8]| {
        if direction == Direction::Input && frame.windows(8).any(|window| window == b"\"action\"") {
            thread::sleep(timeout + Duration::from_millis(50));
        }
    });
    let options = Options {
        confirmation_timeout: timeout,
        observer: Some(observer),
        ..Options::default()
    };
    let host = Host::ready(confirming(&events), options, form());
    host.call(1, "acme_change", json!({}));
    let request = host.recv();
    answer(&host, &request, json!({"action":"accept"}));
    let expired = host.recv();
    assert_eq!(expired["id"], 1);
    assert_eq!(expired["error"]["code"], -32001);
    assert_eq!(
        expired["error"]["message"],
        "Confirmation deadline exceeded"
    );
    host.assert_quiet();
    assert_eq!(events.none_within(Duration::from_millis(50)), None);
}

#[test]
fn a_continuation_that_asks_again_is_an_internal_error() {
    let script = Script::new(|_, _| {
        Outcome::Confirm(Confirm::new("First?", |_: Decision, _| {
            Outcome::Confirm(Confirm::new("Second?", |_, _| text("never")))
        }))
    });
    let host = Host::ready(script, Options::default(), json!({}));
    host.call(1, "acme_change", json!({}));
    assert_eq!(host.recv()["error"]["code"], -32603);
}
