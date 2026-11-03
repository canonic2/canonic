//! Compare live ownership and publication work before and after ordinary request history.
mod observations;
mod workflow;

use crate::fixture::{Events, Host, WAIT};
use canonic_mcp_kit::{
    ModernOptions, ServerInfo, rpc,
    stdio::{self, Options, Settlement},
};
use observations::Shape;
use serde_json::json;
use std::sync::mpsc;

fn sample(samples: &mpsc::Receiver<Shape>, why: Settlement) -> Shape {
    let shape = samples.recv_timeout(WAIT).expect("released sample");
    assert_eq!(shape.settlement, Some(why));
    shape
}

fn trials(
    host: &Host,
    owners: &mpsc::Receiver<std::sync::Weak<()>>,
    samples: &mpsc::Receiver<Shape>,
    events: &Events,
) -> Vec<Shape> {
    let mut shapes = vec![];
    for (action, rounds) in [("confirm", 1), ("rich", 2)] {
        workflow::send(host, "sample-interaction", action, false);
        let owner = workflow::owner(owners);
        for _ in 0..rounds {
            workflow::answer(host, &host.recv());
        }
        assert_eq!(host.recv()["id"], "sample-interaction");
        let shape = sample(samples, Settlement::Reply);
        assert_eq!(shape.handlers, rounds + 1);
        assert_eq!(shape.interactions, rounds);
        assert_eq!(shape.frames, rounds + 1);
        assert!(owner.upgrade().is_none());
        shapes.push(shape);
    }
    workflow::send(host, "sample-cancel", "confirm", false);
    let form = host.recv();
    let owner = workflow::owner(owners);
    workflow::cancel(host, "sample-cancel", &form);
    shapes.push(sample(samples, Settlement::Cancelled));
    assert!(owner.upgrade().is_none());
    assert_eq!(
        workflow::modern_round(host, false)["result"]["resultType"],
        "input_required"
    );
    let interim = sample(samples, Settlement::Reply);
    assert_eq!(interim.interactions, 0);
    assert_eq!(interim.handlers, 1);
    assert_eq!(interim.frames, 1);
    shapes.push(interim);
    assert_eq!(
        workflow::modern_round(host, true)["result"]["resultType"],
        "complete"
    );
    shapes.push(sample(samples, Settlement::Reply));
    host.send(&rpc::request(
        json!("sample-deadline"),
        "tools/call",
        json!({"name":"acme_echo",
        "arguments":{"message":"deadline","key":"shared","timeoutMs":1000}}),
    ));
    assert_eq!(events.next(), "deadline worker started");
    assert_eq!(host.recv()["error"]["code"], rpc::DEADLINE_EXCEEDED);
    shapes.push(sample(samples, Settlement::Deadline));
    shapes
}

fn run(history: usize) -> (Vec<Shape>, observations::Summary) {
    let (sender, records) = stdio::observation_channel(4096).expect("valid observation channel");
    let (samples, collector) = observations::start(records);
    let mut options = Options {
        observations: Some(sender.clone().with_rpc_ids(true)),
        ..Options::default()
    };
    options.session.modern = Some(ModernOptions::new(ServerInfo::new("acme", "1")));
    let (captured, owners) = mpsc::channel();
    let events = Events::new();
    let host = Host::ready(
        workflow::script(captured, events.clone()),
        options,
        json!({"elicitation":{"form":{}}}),
    );
    // Age ordinary worker completions while one unrelated confirmation remains live.
    workflow::send(&host, "anchor", "confirm", false);
    let pending = host.recv();
    let owner = workflow::owner(&owners);
    for index in 0..history {
        let id = if index % 2 == 0 {
            "reused".into()
        } else {
            format!("history-{index}")
        };
        workflow::send(&host, &id, "echo", index % 2 == 0);
        assert_eq!(host.recv()["id"], id);
        assert!(
            owner.upgrade().is_some(),
            "pending continuation was discarded"
        );
    }
    workflow::cancel(&host, "anchor", &pending);
    assert!(
        owner.upgrade().is_none(),
        "cancelled anchor retained its continuation"
    );
    let mut shapes = trials(&host, &owners, &samples, &events);
    workflow::send(&host, "sample-drain", "rich", false);
    assert_eq!(host.recv()["method"], "elicitation/create");
    let owner = workflow::owner(&owners);
    let (result, remaining) = host.finish();
    result.expect("driver drained");
    assert!(
        remaining
            .iter()
            .any(|r| r["id"] == "sample-drain" && r["error"]["code"] == -32800)
    );
    shapes.push(sample(&samples, Settlement::Disconnected));
    assert!(owner.upgrade().is_none());
    assert_eq!(sender.dropped(), 0);
    drop(sender);
    let summary = collector.join().expect("observation collector joined");
    (shapes, summary)
}

#[test]
fn scaling_interactions_and_publication_keep_only_live_work_after_completed_history() {
    let (fresh, fresh_summary) = run(0);
    let (aged, aged_summary) = run(1024);
    assert_eq!(
        aged, fresh,
        "interaction/publication work grew with unrelated history"
    );
    assert_eq!(aged_summary.released - fresh_summary.released, 1024);
    assert!(aged_summary.peak <= 8 + stdio::INPUT_BUFFER + 2);
}
