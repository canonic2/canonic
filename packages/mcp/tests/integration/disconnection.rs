//! End-of-input rules for queued, running and waiting work.

use crate::fixture::{Events, Host, Latch, Script, arguments, text, tool_name};
use canonic_mcp::stdio::{Confirm, Lane, Options, Outcome};
use serde_json::json;
use std::time::Duration;

fn blocking(events: &Events, latch: &Latch) -> Script {
    let events = events.clone();
    let latch = latch.clone();
    Script::new(move |request, context| {
        let message = arguments(&request)["message"]
            .as_str()
            .unwrap_or("")
            .to_owned();
        events.record(format!("start {message}"));
        let finished = tool_name(&request) == "acme_echo" || latch.wait(context);
        events.record(format!(
            "{} {message}",
            if finished { "end" } else { "stopped" }
        ));
        text(&message)
    })
}

#[test]
fn closing_input_refuses_queued_mutations_but_runs_queued_reads() {
    let events = Events::new();
    let latch = Latch::default();
    let options = Options {
        lanes: vec![Lane::new("tools", 1, 8)],
        disconnect_grace: Duration::from_secs(5),
        ..Options::default()
    };
    let mut host = Host::ready(blocking(&events, &latch), options, json!({}));
    host.call(1, "acme_probe", json!({"message":"read"}));
    assert_eq!(events.next(), "start read");
    host.call(2, "acme_change", json!({"message":"write"}));
    host.call(3, "acme_echo", json!({"message":"echo"}));
    host.close();
    let refused = host.recv();
    assert_eq!(refused["id"], 2);
    assert_eq!(refused["error"]["code"], -32800);
    latch.release();
    let (result, replies) = host.finish();
    result.unwrap();
    let ids: Vec<_> = replies.iter().map(|reply| reply["id"].clone()).collect();
    assert_eq!(ids, [json!(1), json!(3)]);
    assert_eq!(events.next(), "end read");
    assert_eq!(events.next(), "start echo");
}

#[test]
fn a_running_mutation_is_cancelled_after_the_grace_period() {
    let events = Events::new();
    let latch = Latch::default();
    let options = Options {
        disconnect_grace: Duration::from_millis(20),
        ..Options::default()
    };
    let host = Host::ready(blocking(&events, &latch), options, json!({}));
    host.call(1, "acme_change", json!({"message":"write"}));
    assert_eq!(events.next(), "start write");
    let (result, replies) = host.finish();
    result.unwrap();
    assert_eq!(events.next(), "stopped write");
    assert_eq!(replies.len(), 1);
    assert_eq!(replies[0]["id"], 1);
}

#[test]
fn a_running_read_continues_after_the_input_closes() {
    let events = Events::new();
    let latch = Latch::default();
    let options = Options {
        disconnect_grace: Duration::from_millis(1),
        ..Options::default()
    };
    let mut host = Host::ready(blocking(&events, &latch), options, json!({}));
    host.call(1, "acme_probe", json!({"message":"read"}));
    assert_eq!(events.next(), "start read");
    host.close();
    assert_eq!(events.none_within(Duration::from_millis(50)), None);
    latch.release();
    let (result, replies) = host.finish();
    result.unwrap();
    assert_eq!(events.next(), "end read");
    assert_eq!(replies[0]["id"], 1);
}

#[test]
fn a_confirmation_asked_after_the_input_closes_is_refused() {
    let events = Events::new();
    let recorded = events.clone();
    let latch = Latch::default();
    let script = Script::new(move |request, context| {
        if tool_name(&request) == "acme_probe" {
            recorded.record("probe");
            return text("probe");
        }
        recorded.record("start");
        // Never released: the grace period cancels the wait after the input closes.
        latch.wait(context);
        let recorded = recorded.clone();
        Outcome::Confirm(Confirm::new("Delete the Acme data?", move |_, _| {
            recorded.record("resumed");
            text("resumed")
        }))
    });
    let options = Options {
        disconnect_grace: Duration::from_millis(20),
        ..Options::default()
    };
    let host = Host::ready(script, options, json!({"elicitation":{}}));
    host.call(1, "acme_change", json!({"key":"acme"}));
    assert_eq!(events.next(), "start");
    host.call(2, "acme_probe", json!({"key":"acme"}));
    let (result, replies) = host.finish();
    result.unwrap();
    assert_eq!(replies[0]["id"], 1);
    assert_eq!(replies[0]["error"]["code"], -32800);
    assert_eq!(
        replies[0]["error"]["message"],
        "Connection closed before confirmation"
    );
    // The released key lets the queued read with the same key run.
    assert_eq!(replies[1]["id"], 2);
    assert_eq!(events.next(), "probe");
    assert_eq!(events.none_within(Duration::from_millis(20)), None);
}

#[test]
fn confirmations_requested_after_input_closes_never_resume() {
    for name in ["acme_change", "acme_probe"] {
        let events = Events::new();
        let recorded = events.clone();
        let latch = Latch::default();
        let blocked = latch.clone();
        let script = Script::new(move |request, context| {
            if tool_name(&request) == "acme_echo" {
                return text("after");
            }
            recorded.record("started");
            blocked.wait(context);
            let recorded = recorded.clone();
            Outcome::Confirm(Confirm::new("Delete the Acme data?", move |decision, _| {
                recorded.record(format!("{decision:?}"));
                text("resumed")
            }))
        });
        let options = Options {
            disconnect_grace: Duration::from_secs(5),
            ..Options::default()
        };
        let mut host = Host::ready(script, options, json!({"elicitation":{}}));
        host.call(1, name, json!({"key":"acme"}));
        assert_eq!(events.next(), "started");
        host.call(2, "acme_change", json!({"key":"acme"}));
        host.call(3, "acme_echo", json!({"key":"acme"}));
        host.close();
        // The queued mutation's refusal proves the dispatcher processed EOF.
        let refused = host.recv();
        assert_eq!(refused["id"], 2);
        assert_eq!(refused["error"]["code"], -32800);
        latch.release();
        let (result, replies) = host.finish();
        result.unwrap();
        assert_eq!(replies.len(), 2);
        assert_eq!(replies[0]["id"], 1);
        assert_eq!(replies[0]["error"]["code"], -32800);
        assert_eq!(
            replies[0]["error"]["message"],
            "Connection closed before confirmation"
        );
        // The retained read must run once the confirmation releases its key.
        assert_eq!(replies[1]["id"], 3);
        assert_eq!(replies[1]["result"]["content"][0]["text"], "after");
        assert_eq!(events.none_within(Duration::ZERO), None);
    }
}

#[test]
fn closing_input_withdraws_a_pending_confirmation() {
    let events = Events::new();
    let recorded = events.clone();
    let script = Script::new(move |_, _| {
        let recorded = recorded.clone();
        Outcome::Confirm(Confirm::new("Delete the Acme data?", move |decision, _| {
            recorded.record(format!("{decision:?}"));
            text("resumed")
        }))
    });
    let host = Host::ready(script, Options::default(), json!({"elicitation":{}}));
    host.call(1, "acme_change", json!({}));
    let request = host.recv();
    assert_eq!(request["method"], "elicitation/create");
    let (result, replies) = host.finish();
    result.unwrap();
    assert_eq!(replies[0]["method"], "notifications/cancelled");
    assert_eq!(replies[0]["params"]["requestId"], request["id"]);
    assert_eq!(replies[1]["id"], 1);
    assert_eq!(replies[1]["error"]["code"], -32800);
    assert_eq!(events.none_within(Duration::from_millis(20)), None);
}
