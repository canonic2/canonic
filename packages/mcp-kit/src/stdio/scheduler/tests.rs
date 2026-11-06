//! A deterministic backlog exercises the real dispatcher's progress drain.
use super::*;
use crate::{Catalog, Client, ServerInfo, SessionOptions};

struct NoCalls;
impl Handler for NoCalls {
    fn initialize(&self, _: &Client) -> Result<ServerInfo, RpcError> {
        panic!("no protocol input")
    }
    fn call(&self, _: Request, _: &Context) -> Outcome {
        panic!("progress drain only")
    }
}

fn scheduler_with_progress(frames: Vec<(u64, Value)>) -> (Scheduler, Session) {
    let mut scheduler =
        Scheduler::start(&Arc::new(NoCalls), &Options::default(), thread::current()).unwrap();
    // Supply one live assignment without a consumer handler.
    scheduler.lanes[0].idle.clear();
    scheduler.lanes[0].running = 1;
    scheduler.running.insert(
        0,
        Running {
            execution: Trace::default().span(Stage::Worker),
            meta: Meta {
                apps: false,
                trace: Trace::default(),
                reply_kind: ReplyKind::Ordinary,
                result_schema: None,
                admitted: Instant::now(),
                rich_rounds: 0,
                rich_deadline: None,
                request_context: None,
                grant: None,
                id: None,
                lane: 0,
                key: None,
                timeout: Duration::from_mins(1),
                read_only: true,
                progress_token: None,
            },
            worker: 0,
            control: Arc::new(Control::new(Duration::from_mins(1))),
            answered: false,
            grace: None,
        },
    );
    let session = Session::new(
        Arc::new(Catalog::new(vec![]).unwrap()),
        SessionOptions::default(),
    );
    // Only this test channel has extra capacity. Preloading more than a turn's
    // budget models a producer refilling the production channel during a drain,
    // without depending on thread scheduling. settle uses the real Receiver and
    // drain path; removing its budget deterministically consumes too many items.
    let (sender, receiver) = mpsc::sync_channel(frames.len());
    scheduler.progress = receiver;
    scheduler.progress_sender = sender;
    for frame in frames {
        scheduler.progress_sender.try_send(frame).unwrap();
    }
    (scheduler, session)
}

#[test]
fn progress_drain_stops_at_its_budget_and_preserves_remaining_frames() {
    let expected: Vec<Value> = (0..=2 * PROGRESS_BUFFER).map(Value::from).collect();
    let (mut scheduler, mut session) =
        scheduler_with_progress(expected.iter().cloned().map(|value| (0, value)).collect());
    let mut received = Vec::new();
    for count in [PROGRESS_BUFFER, PROGRESS_BUFFER, 1, 0] {
        let batch = scheduler.settle(&mut session);
        assert_eq!(batch.len(), count);
        for frame in batch {
            let Outgoing::Value(value) = frame else {
                panic!("progress value expected")
            };
            received.push(value);
        }
    }
    assert_eq!(received, expected);
}

#[test]
fn stale_progress_consumes_the_turn_budget() {
    // Unknown assignment 1 is stale; live assignment 0 must wait for turn two.
    let mut frames = vec![(1, Value::Null); PROGRESS_BUFFER];
    frames.push((0, Value::from("live")));
    let (mut scheduler, mut session) = scheduler_with_progress(frames);
    assert!(scheduler.settle(&mut session).is_empty());
    let mut next = scheduler.settle(&mut session);
    assert_eq!(next.len(), 1);
    assert!(matches!(next.pop(), Some(Outgoing::Value(value)) if value == "live"));
    assert!(scheduler.settle(&mut session).is_empty());
}
