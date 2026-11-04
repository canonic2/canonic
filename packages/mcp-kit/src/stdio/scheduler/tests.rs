//! Refill pressure must not turn a bounded progress queue into an unbounded batch.
//!
//! The batch bound and the total count pass deterministically. Detecting a
//! drain-until-empty regression depends on the producer refilling the channel
//! during a drain; a blocking sender of many items makes that likely, not certain.
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

#[test]
fn progress_refill_keeps_each_dispatcher_batch_bounded() {
    const COUNT: usize = 65536;
    let mut scheduler =
        Scheduler::start(&Arc::new(NoCalls), &Options::default(), thread::current()).unwrap();
    // Supply one live assignment without a consumer handler. This unit boundary
    // exercises the real channel, live-owner filter and outgoing batch ownership.
    scheduler.lanes[0].idle.clear();
    scheduler.lanes[0].running = 1;
    scheduler.running.insert(
        0,
        Running {
            execution: Trace::default().span(Stage::Worker),
            meta: Meta {
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
                timeout: Duration::from_secs(60),
                read_only: true,
                progress_token: None,
            },
            worker: 0,
            control: Arc::new(Control::new(Duration::from_secs(60))),
            answered: false,
            grace: None,
        },
    );
    let mut session = Session::new(
        Arc::new(Catalog::new(vec![]).unwrap()),
        SessionOptions::default(),
    );
    let sender = scheduler.progress_sender.clone();
    let producer = thread::spawn(move || {
        for _ in 0..COUNT {
            // A timed-out drain drops the receiver; stop so the assertions report it.
            if sender.send((0, Value::Null)).is_err() {
                break;
            }
        }
    });
    let mut received = 0;
    let mut maximum = 0;
    let deadline = Instant::now() + Duration::from_secs(10);
    while received < COUNT && Instant::now() < deadline {
        let batch = scheduler.settle(&mut session);
        received += batch.len();
        maximum = maximum.max(batch.len());
        thread::yield_now();
    }
    drop(scheduler);
    producer.join().unwrap();
    eprintln!("progress frames={received}, maximum batch={maximum}");
    assert_eq!(received, COUNT);
    assert!(maximum <= PROGRESS_BUFFER, "batch grew to {maximum}");
}
