//! Deterministic output pressure, queue exclusion and bounded subscription fanout.
use crate::fixture::{self, Events, Latch, Script, WAIT, arguments, text};
use canonic_mcp_kit::{
    Change, Client, ModernOptions, RpcError, ServerInfo, SubscriptionOptions, modern, rpc,
    stdio::{
        self, Admission, ChangeSender, Context, Handler, Lane, Observation,
        ObservationKind as Kind, Options, Outcome, Request, SendChangeError, Stage,
    },
};
use serde_json::{Value, json};
use std::{
    io::{self, Write},
    sync::{Arc, mpsc},
    thread,
    time::Instant,
};

struct Producer {
    sender: mpsc::Sender<ChangeSender>,
    script: Script,
}
impl Handler for Producer {
    fn connected(&self, sender: ChangeSender) {
        self.sender
            .send(sender)
            .expect("connection producer receiver");
    }
    fn initialize(&self, client: &Client) -> Result<ServerInfo, RpcError> {
        self.script.initialize(client)
    }
    fn admit(&self, request: &Request) -> Admission {
        self.script.admit(request)
    }
    fn call(&self, request: Request, context: &Context) -> Outcome {
        self.script.call(request, context)
    }
}

struct PausedWriter {
    entered: mpsc::SyncSender<()>,
    resume: mpsc::Receiver<()>,
    bytes: Vec<u8>,
}
impl Write for PausedWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if serde_json::from_slice::<Value>(bytes).is_ok_and(|v| v["id"] == "gate") {
            self.entered.send(()).expect("writer boundary receiver");
            self.resume
                .recv_timeout(WAIT)
                .expect("release controlled writer");
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn modern_request(id: Value, method: &str, mut params: Value) -> Value {
    params["_meta"] = json!({modern::VERSION_KEY:modern::PROTOCOL_VERSION,
        modern::CAPABILITIES_KEY:{}});
    rpc::request(id, method, params)
}

fn input(streams: usize) -> Vec<u8> {
    let mut requests = vec![
        rpc::request(
            json!("init"),
            "initialize",
            json!({
        "protocolVersion":"2025-11-25","capabilities":{},
        "clientInfo":{"name":"acme-host","version":"1"}}),
        ),
        rpc::notification("notifications/initialized", json!({})),
    ];
    requests.extend((0..streams).map(|i| {
        modern_request(
            json!(format!("stream-{i}")),
            "subscriptions/listen",
            json!({"notifications":{"toolsListChanged":true}}),
        )
    }));
    for (id, message) in [(100, "holder"), (101, "follower"), (102, "refused")] {
        requests.push(modern_request(
            json!(id),
            "tools/call",
            json!({"name":"acme_probe",
            "arguments":{"key":"shared","message":message},
            "_meta":{}}),
        ));
        if id == 100 {
            requests.last_mut().expect("holder request")["params"]["_meta"]["progressToken"] =
                json!("p");
        }
    }
    requests.extend([
        rpc::request(json!("gate"), "ping", json!({})),
        rpc::notification("notifications/cancelled", json!({"requestId":100})),
        rpc::request(json!("after"), "ping", json!({})),
    ]);
    let mut bytes = Vec::new();
    for request in requests {
        serde_json::to_writer(&mut bytes, &request).expect("encodable request");
        bytes.push(b'\n');
    }
    bytes
}

fn interval(records: &[Observation], id: &Value, stage: Stage) -> u128 {
    let request = records
        .iter()
        .find(|r| r.rpc_id.as_deref() == Some(id))
        .expect("observed protocol ID")
        .request;
    let at = |kind| {
        records
            .iter()
            .find(|r| r.request == request && r.kind == kind)
            .expect("observed interval boundary")
            .at
    };
    at(Kind::End(stage))
        .duration_since(at(Kind::Start(stage)))
        .as_nanos()
}

fn check(bytes: &[u8], streams: usize) -> usize {
    let frames: Vec<Value> = bytes
        .split(|b| *b == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_slice(line).expect("valid frame"))
        .collect();
    assert!(!frames.iter().any(|f| f["id"] == 100));
    assert_eq!(
        frames
            .iter()
            .find(|f| f["id"] == 102)
            .expect("refused reply")["error"]["code"],
        -32000
    );
    assert_eq!(
        frames
            .iter()
            .find(|f| f["id"] == 101)
            .expect("follower reply")["result"]["isError"],
        false
    );
    assert_eq!(
        frames
            .iter()
            .find(|f| f["id"] == "after")
            .expect("ping reply")["result"],
        json!({})
    );
    let ping = frames
        .iter()
        .position(|f| f["id"] == "after")
        .expect("ping position");
    assert!(
        !frames[ping + 1..]
            .iter()
            .any(|f| f["method"] == "notifications/progress")
    );
    let updates = frames
        .iter()
        .filter(|f| f["method"] == "notifications/tools/list_changed")
        .count();
    assert_eq!(updates, streams * stdio::CHANGE_BUFFER);
    for i in 0..streams {
        let id = json!(format!("stream-{i}"));
        let updates: Vec<_> = frames
            .iter()
            .enumerate()
            .filter(|(_, f)| {
                f["params"]["_meta"][canonic_mcp_kit::subscription::SUBSCRIPTION_KEY] == id
            })
            .collect();
        assert_eq!(updates.len(), 1 + stdio::CHANGE_BUFFER);
        assert_eq!(
            updates[0].1["method"],
            "notifications/subscriptions/acknowledged"
        );
    }
    updates
}

fn script(start: Latch, events: Events, reported: mpsc::Sender<(usize, bool)>) -> Script {
    Script::new(move |request, context| {
        if arguments(&request)["message"] == "holder" {
            start.wait(context);
            let accepted = (0..=stdio::PROGRESS_BUFFER)
                .filter(|i| {
                    context.progress(u32::try_from(*i).expect("bounded count").into(), None, None)
                })
                .count();
            reported
                .send((accepted, context.check().is_ok()))
                .expect("progress probe receiver");
            Latch::default().wait(context);
            events.record("holder stopped");
        } else {
            events.record("follower started");
        }
        text("done")
    })
}

/// Returns stage diagnostics; assertions establish ownership/order rather than latency thresholds.
pub(crate) fn run(streams: usize) -> Value {
    let (observation, observed) =
        stdio::observation_channel(4096).expect("valid observation capacity");
    let collector = thread::spawn(move || observed.iter().take(32768).collect::<Vec<_>>());
    let mut options = Options {
        lanes: vec![Lane::new("tools", 2, 2)],
        observations: Some(observation.clone().with_rpc_ids(true)),
        ..Options::default()
    };
    options.session.modern = Some(ModernOptions::new(ServerInfo::new("acme", "1")));
    options.session.subscriptions = Some(SubscriptionOptions {
        max_streams: streams,
        ..SubscriptionOptions::default()
    });
    let start_progress = Latch::default();
    let events = Events::new();
    let (reported, report) = mpsc::channel();
    let script = script(start_progress.clone(), events.clone(), reported);
    let (connected, changes) = mpsc::channel();
    let (entered, blocked) = mpsc::sync_channel(1);
    let (resume, resumed) = mpsc::sync_channel(1);
    let bytes = input(streams);
    let catalog = fixture::catalog();
    let replacement = catalog.clone();
    let server = thread::spawn(move || {
        let mut writer = PausedWriter {
            entered,
            resume: resumed,
            bytes: vec![],
        };
        let result = stdio::serve(
            Arc::new(Producer {
                sender: connected,
                script,
            }),
            catalog,
            io::Cursor::new(bytes),
            &mut writer,
            &options,
        );
        (result, writer.bytes)
    });
    let paused = blocked.recv_timeout(WAIT);
    let start = Instant::now();
    start_progress.release();
    let progress = report.recv_timeout(WAIT);
    let sender = changes.recv_timeout(WAIT).expect("connection producer");
    let queued: Vec<_> = (0..stdio::CHANGE_BUFFER)
        .map(|_| sender.send(Change::Tools(replacement.clone())))
        .collect();
    let overflow = sender.send(Change::Tools(replacement));
    let held_ns = start.elapsed().as_nanos();
    let _ = resume.send(());
    let (result, bytes) = server.join().expect("fixture server joined");
    let dropped = observation.dropped();
    drop(observation);
    let records = collector.join().expect("collector joined");
    paused.expect("writer reached controlled boundary");
    assert_eq!(
        progress.expect("progress probe"),
        (stdio::PROGRESS_BUFFER, true)
    );
    assert!(queued.iter().all(Result::is_ok));
    assert!(matches!(overflow, Err(SendChangeError::Full(_))));
    result.expect("fixture server succeeded");
    assert_eq!(dropped, 0);
    assert_eq!(events.next(), "holder stopped");
    assert_eq!(events.next(), "follower started");
    let updates = check(&bytes, streams);
    json!({"streams":streams,"updates":updates,"progress_accepted":stdio::PROGRESS_BUFFER,
        "controlled_hold_ns":held_ns,"gate_publication_ns":interval(&records,&json!("gate"),Stage::Publication),
        "follower_queue_ns":interval(&records,&json!(101),Stage::Queue),
        "follower_handler_ns":interval(&records,&json!(101),Stage::Handler),
        "ping_input_queue_ns":interval(&records,&json!("after"),Stage::InputQueue),
        "dropped":dropped,"frames_bytes":bytes.len()})
}
