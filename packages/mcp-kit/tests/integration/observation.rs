//! Lifecycle accounting through the real driver, with bounded diagnostic sinks.

use crate::fixture::{self, Events, Host, Latch, Script, WAIT, arguments, text};
use canonic_mcp_kit::{
    ModernOptions, ServerInfo, modern, rpc,
    stdio::{
        self, Confirm, FrameKind, Lane, Observation, ObservationKind as Kind, Options, Outcome,
        Settlement, Stage, observation_channel,
    },
};
use serde_json::{Value, json};
use std::{
    collections::HashSet,
    io::{self, Write},
    sync::{Arc, mpsc},
};

fn observed() -> (Options, mpsc::Receiver<Observation>) {
    let (sender, receiver) = observation_channel(4096).expect("valid observation capacity");
    (
        Options {
            observations: Some(sender.with_rpc_ids(true)),
            ..Options::default()
        },
        receiver,
    )
}

fn for_id<'a>(records: &'a [Observation], id: &Value) -> Vec<&'a Observation> {
    let inputs: HashSet<_> = records
        .iter()
        .filter(|record| record.rpc_id.as_deref() == Some(id))
        .map(|record| record.request)
        .collect();
    records
        .iter()
        .filter(|record| inputs.contains(&record.request))
        .collect()
}

fn position(records: &[&Observation], kind: &Kind) -> usize {
    records
        .iter()
        .position(|record| &record.kind == kind)
        .unwrap_or_else(|| panic!("missing {kind:?}"))
}

fn settled_once(records: &[&Observation], why: Settlement) {
    let endings: Vec<_> = records
        .iter()
        .filter_map(|record| {
            if let Kind::Settled(why) = record.kind {
                Some(why)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(endings, [why]);
    assert_eq!(
        records
            .iter()
            .filter(|record| record.kind == Kind::Released)
            .count(),
        1
    );
}

#[test]
fn observation_covers_immediate_decode_callbacks_and_worker_output_without_payloads() {
    let (options, receiver) = observed();
    let stats = options.observations.clone().unwrap();
    let host = Host::ready(
        Script::new(|_, _| text("private Acme result")),
        options,
        json!({}),
    );
    host.assert_quiet();
    host.send_bytes(b"{\"jsonrpc\":\"2.0\",\"id\":9,\"id\":9,\"method\":\"ping\"}\n".to_vec());
    assert_eq!(host.recv()["error"]["code"], rpc::PARSE_ERROR);
    host.call(1, "acme_echo", json!({"message":"private Acme argument"}));
    assert_eq!(host.recv()["id"], 1);
    host.call(2, "acme_echo", json!({"message":false}));
    assert_eq!(host.recv()["id"], 2);
    host.finish().0.unwrap();
    let records: Vec<_> = receiver.try_iter().collect();
    assert_eq!(stats.dropped(), 0);
    let call = for_id(&records, &json!(1));
    settled_once(&call, Settlement::Reply);
    for stage in [
        Stage::Decode,
        Stage::Callback,
        Stage::Queue,
        Stage::Worker,
        Stage::Handler,
        Stage::Validation,
        Stage::Encode,
        Stage::Publication,
    ] {
        assert!(position(&call, &Kind::Start(stage)) < position(&call, &Kind::End(stage)));
    }
    assert!(
        position(&call, &Kind::End(Stage::Handler))
            < position(&call, &Kind::Start(Stage::Validation))
    );
    assert!(
        position(&call, &Kind::Settled(Settlement::Reply))
            < position(&call, &Kind::Start(Stage::Publication))
    );
    for id in [json!("quiet"), json!("init"), json!(2)] {
        let immediate = for_id(&records, &id);
        settled_once(&immediate, Settlement::Reply);
        assert!(
            !immediate
                .iter()
                .any(|record| record.kind == Kind::Start(Stage::Worker))
        );
    }
    assert!(
        for_id(&records, &json!(9)).is_empty(),
        "duplicate-key frame was never parsed for diagnostics"
    );
    let debug = format!("{records:?}");
    assert!(!debug.contains("private Acme"));
    let inputs: HashSet<_> = records
        .iter()
        .filter(|record| record.request != 0)
        .map(|record| record.request)
        .collect();
    for input in inputs {
        let endings = records
            .iter()
            .filter(|record| record.request == input && matches!(record.kind, Kind::Settled(_)))
            .count();
        assert_eq!(endings, 1);
    }
}

#[test]
fn observation_keeps_cancelled_and_reused_ids_distinct_until_worker_release() {
    let (mut options, receiver) = observed();
    options.lanes = vec![Lane::new("tools", 2, 4)];
    let events = Events::new();
    let worker_events = events.clone();
    let (release, released) = mpsc::sync_channel(1);
    let released = std::sync::Mutex::new(released);
    let host = Host::ready(
        Script::new(move |request, _| {
            if arguments(&request)["message"] == "old" {
                worker_events.record("old started");
                released.lock().unwrap().recv_timeout(WAIT).unwrap();
            }
            text("done")
        }),
        options,
        json!({}),
    );
    host.call(7, "acme_probe", json!({"message":"old"}));
    assert_eq!(events.next(), "old started");
    host.cancel(7);
    host.assert_quiet();
    host.call(7, "acme_echo", json!({}));
    assert_eq!(host.recv()["id"], 7);
    release.send(()).unwrap();
    host.finish().0.unwrap();
    let records: Vec<_> = receiver.try_iter().collect();
    let calls = for_id(&records, &json!(7));
    let ids: Vec<_> = calls
        .iter()
        .filter(|record| record.kind == Kind::ProtocolId)
        .map(|record| record.request)
        .collect();
    assert_eq!(ids.len(), 2);
    assert_ne!(ids[0], ids[1]);
    let old: Vec<_> = calls
        .iter()
        .copied()
        .filter(|record| record.request == ids[0])
        .collect();
    let new: Vec<_> = calls
        .iter()
        .copied()
        .filter(|record| record.request == ids[1])
        .collect();
    settled_once(&old, Settlement::Cancelled);
    settled_once(&new, Settlement::Reply);
    assert!(
        position(&old, &Kind::Settled(Settlement::Cancelled))
            < position(&old, &Kind::End(Stage::Handler))
    );
    assert!(position(&old, &Kind::End(Stage::Handler)) < position(&old, &Kind::Released));
    assert!(old.iter().any(|record| record.kind == Kind::Discarded));
    assert!(!old.iter().any(|record| matches!(
        record.kind,
        Kind::Frame {
            kind: FrameKind::Reply,
            ..
        }
    )));
    assert!(
        records
            .iter()
            .any(|record| record.kind == Kind::Related(ids[0]) && record.request != ids[0])
    );
}

#[test]
fn observation_tracks_confirmation_rounds_and_progress_on_the_original_input() {
    let (options, receiver) = observed();
    let host = Host::ready(
        Script::new(|_, context| {
            assert!(context.progress(1.0, None, None));
            Outcome::Confirm(Confirm::new("Continue Acme?", |_, _| text("done")))
        }),
        options,
        json!({"elicitation":{}}),
    );
    host.send(&rpc::request(
        json!(1),
        "tools/call",
        json!({"name":"acme_echo", "_meta":{"progressToken":"p"}}),
    ));
    assert_eq!(host.recv()["method"], "notifications/progress");
    let question = host.recv();
    host.send(&rpc::success(
        question["id"].clone(),
        json!({"action":"accept"}),
    ));
    assert_eq!(host.recv()["id"], 1);
    host.finish().0.unwrap();
    let records: Vec<_> = receiver.try_iter().collect();
    let call = for_id(&records, &json!(1));
    settled_once(&call, Settlement::Reply);
    let rounds: HashSet<_> = call
        .iter()
        .filter(|record| record.kind == Kind::Start(Stage::Handler))
        .map(|record| record.assignment.unwrap())
        .collect();
    assert_eq!(rounds.len(), 2);
    assert!(
        position(&call, &Kind::Start(Stage::Interaction))
            < position(&call, &Kind::End(Stage::Interaction))
    );
    for kind in [
        FrameKind::Progress,
        FrameKind::Interaction,
        FrameKind::Reply,
    ] {
        assert!(call.iter().any(
            |record| matches!(record.kind, Kind::Frame { kind: actual, .. } if actual == kind)
        ));
    }
    let origin = call[0].request;
    assert!(
        records
            .iter()
            .any(|record| record.kind == Kind::Related(origin) && record.request != origin)
    );
}

#[test]
fn observation_full_or_closed_sink_preserves_serving_and_counts_loss() {
    for closed in [false, true] {
        let (sender, receiver) = observation_channel(1).unwrap();
        let receiver = (!closed).then_some(receiver);
        let events = Events::new();
        let started = events.clone();
        let host = Host::ready(
            Script::new(move |request, context| {
                if arguments(&request)["message"] == "blocked" {
                    started.record("started");
                    Latch::default().wait(context);
                }
                text("done")
            }),
            Options {
                observations: Some(sender.clone()),
                ..Options::default()
            },
            json!({}),
        );
        for id in 0..16 {
            host.call(id, "acme_echo", json!({}));
            assert_eq!(host.recv()["id"], id);
        }
        host.call(100, "acme_probe", json!({"message":"blocked"}));
        assert_eq!(events.next(), "started");
        host.cancel(100);
        host.assert_quiet();
        host.finish().0.unwrap();
        assert!(sender.dropped() > 0);
        if let Some(receiver) = receiver {
            assert_eq!(receiver.try_iter().count(), 1);
        }
    }
    assert!(observation_channel(0).is_err());
    assert!(observation_channel(stdio::MAX_OBSERVATION_BUFFER + 1).is_err());
}

#[test]
fn observation_queue_expiry_and_capacity_refusal_do_not_invent_handler_work() {
    let (mut options, receiver) = observed();
    options.lanes = vec![Lane::new("tools", 1, 2)];
    let latch = Latch::default();
    let blocked = latch.clone();
    let events = Events::new();
    let started = events.clone();
    let host = Host::ready(
        Script::new(move |_, context| {
            started.record("started");
            blocked.wait(context);
            text("done")
        }),
        options,
        json!({}),
    );
    host.call(1, "acme_probe", json!({}));
    assert_eq!(events.next(), "started");
    host.call(2, "acme_echo", json!({}));
    host.call(3, "acme_echo", json!({}));
    let refused = host.recv();
    assert_eq!(refused["id"], 3);
    assert_eq!(refused["error"]["code"], rpc::SERVER_ERROR);
    host.cancel(2);
    host.assert_quiet();
    host.call(4, "acme_echo", json!({"timeoutMs":20}));
    let expired = host.recv();
    assert_eq!(expired["id"], 4);
    assert_eq!(expired["error"]["code"], rpc::DEADLINE_EXCEEDED);
    latch.release();
    assert_eq!(host.recv()["id"], 1);
    host.finish().0.unwrap();
    let records: Vec<_> = receiver.try_iter().collect();
    for (id, why) in [(4, Settlement::Deadline), (3, Settlement::Refused)] {
        let call = for_id(&records, &json!(id));
        settled_once(&call, why);
        assert!(
            !call
                .iter()
                .any(|record| record.kind == Kind::Start(Stage::Handler))
        );
    }
    let queued = for_id(&records, &json!(4));
    assert!(
        position(&queued, &Kind::Start(Stage::Queue)) < position(&queued, &Kind::End(Stage::Queue))
    );
}

#[test]
fn observation_modern_reused_ids_are_independent_admissions_and_ids_are_opt_in() {
    let (mut options, receiver) = observed();
    options.session.modern = Some(ModernOptions::new(ServerInfo::new("acme", "1")));
    let host = Host::start(Script::new(|_, _| text("done")), options);
    for _ in 0..2 {
        host.send(&rpc::request(
            json!(1),
            "tools/call",
            json!({"name":"acme_echo","_meta":{
            modern::VERSION_KEY:modern::PROTOCOL_VERSION,modern::CAPABILITIES_KEY:{}}}),
        ));
        assert_eq!(host.recv()["id"], 1);
    }
    host.finish().0.unwrap();
    let records: Vec<_> = receiver.try_iter().collect();
    let ids: HashSet<_> = for_id(&records, &json!(1))
        .iter()
        .map(|record| record.request)
        .collect();
    assert_eq!(ids.len(), 2);
    let (sender, receiver) = observation_channel(256).unwrap();
    for _ in 0..2 {
        let host = Host::start(
            Script::new(|_, _| text("done")),
            Options {
                observations: Some(sender.clone()),
                ..Options::default()
            },
        );
        host.send(&rpc::request(json!("private-id"), "ping", json!({})));
        assert_eq!(host.recv()["id"], "private-id");
        host.finish().0.unwrap();
    }
    let records: Vec<_> = receiver.try_iter().collect();
    assert!(records.iter().all(|record| record.rpc_id.is_none()));
    let connections: HashSet<_> = records.iter().map(|record| record.connection).collect();
    assert_eq!(connections.len(), 2);
}

struct BrokenOutput;

struct PausedOutput {
    bytes: Vec<u8>,
    entered: mpsc::SyncSender<()>,
    resume: mpsc::Receiver<()>,
    at_flush: bool,
    paused: bool,
}

impl PausedOutput {
    fn pause(&mut self) {
        if !self.paused {
            self.paused = true;
            self.entered.send(()).expect("writer observer is waiting");
            self.resume.recv_timeout(WAIT).expect("writer released");
        }
    }
}

impl Write for PausedOutput {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if !self.at_flush {
            self.pause();
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        if self.at_flush {
            self.pause();
        }
        Ok(())
    }
}

fn immediate_request(case: &str) -> Value {
    match case {
        "initialize" | "refused" => rpc::request(
            json!(1),
            "initialize",
            json!({"protocolVersion":"2025-11-25","capabilities":{},
                "clientInfo":{"name":if case == "refused" {"refused-host"} else {"acme"},"version":"1"}}),
        ),
        "arguments" | "discovery" => rpc::request(
            json!(1),
            if case == "arguments" {
                "tools/call"
            } else {
                "server/discover"
            },
            if case == "arguments" {
                json!({"name":"acme_echo","arguments":{"message":false},"_meta":{
                    modern::VERSION_KEY:modern::PROTOCOL_VERSION,modern::CAPABILITIES_KEY:{}}})
            } else {
                json!({"_meta":{modern::VERSION_KEY:modern::PROTOCOL_VERSION,modern::CAPABILITIES_KEY:{}}})
            },
        ),
        "ping" | "fatal" => rpc::request(json!(1), "ping", json!({})),
        _ => panic!("unknown immediate response fixture"),
    }
}

#[test]
fn observation_immediate_replies_settle_while_publication_is_blocked() {
    for at_flush in [false, true] {
        for case in [
            "ping",
            "discovery",
            "initialize",
            "refused",
            "arguments",
            "fatal",
        ] {
            check_immediate_reply_publication(case, at_flush);
        }
    }
}

fn check_immediate_reply_publication(case: &str, at_flush: bool) {
    let (mut options, receiver) = observed();
    options.session.modern = Some(ModernOptions::new(ServerInfo::new("acme", "1")));
    if case == "fatal" {
        options.session.max_messages = Some(0);
    }
    let mut input =
        serde_json::to_vec(&immediate_request(case)).expect("encodable fixture request");
    input.push(b'\n');
    let (entered, paused) = mpsc::sync_channel(1);
    let (resume, resumed) = mpsc::sync_channel(1);
    let server = std::thread::spawn(move || {
        let mut writer = PausedOutput {
            bytes: Vec::new(),
            entered,
            resume: resumed,
            at_flush,
            paused: false,
        };
        let result = stdio::serve(
            Arc::new(Script::new(|_, _| {
                panic!("immediate reply never reaches worker")
            })),
            fixture::catalog(),
            io::Cursor::new(input),
            &mut writer,
            &options,
        );
        (result, writer.bytes)
    });
    let blocked = paused.recv_timeout(WAIT);
    let mut records: Vec<_> = receiver.try_iter().collect();
    let before_release = records.len();
    let _ = resume.send(());
    let (result, bytes) = server.join().expect("fixture server did not panic");
    blocked.expect("writer reached controlled publication boundary");
    if case == "fatal" {
        assert!(matches!(result, Err(stdio::ServeError::Protocol(_))));
    } else {
        result.expect("fixture server completed");
    }
    let reply: Value = serde_json::from_slice(&bytes).expect("valid fixture response");
    assert!(reply.get("result").is_some() || reply.get("error").is_some());
    let publication = records
        .iter()
        .find(|event| event.kind == Kind::Start(Stage::Publication))
        .expect("publication started before writer blocks");
    let request = publication.request;
    let call: Vec<_> = records
        .iter()
        .filter(|event| event.request == request)
        .collect();
    assert!(
        position(&call, &Kind::Settled(Settlement::Reply))
            < position(&call, &Kind::Start(Stage::Publication)),
        "{case}, flush={at_flush}"
    );
    if matches!(case, "initialize" | "refused" | "arguments") {
        assert!(
            position(&call, &Kind::End(Stage::Callback))
                < position(&call, &Kind::Settled(Settlement::Reply))
        );
    }
    assert!(
        !call
            .iter()
            .any(|event| event.kind == Kind::End(Stage::Publication))
    );
    records.extend(receiver.try_iter());
    let call: Vec<_> = records
        .iter()
        .filter(|event| event.request == request)
        .collect();
    settled_once(&call, Settlement::Reply);
    assert!(
        records[before_release..]
            .iter()
            .any(|event| event.request == request && event.kind == Kind::End(Stage::Publication))
    );
}

impl Write for BrokenOutput {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        Ok(bytes.len().min(1))
    }
    fn flush(&mut self) -> io::Result<()> {
        Err(io::Error::other("Acme flush failed"))
    }
}

#[test]
fn observation_reports_partial_publication_and_cleanup_without_claiming_delivery() {
    let (options, receiver) = observed();
    let result = stdio::serve(
        Arc::new(Script::new(|_, _| text("done"))),
        fixture::catalog(),
        io::Cursor::new(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}\n".to_vec()),
        BrokenOutput,
        &options,
    );
    assert!(matches!(result, Err(stdio::ServeError::Output(_))));
    let records: Vec<_> = receiver.try_iter().collect();
    let call = for_id(&records, &json!(1));
    settled_once(&call, Settlement::Reply);
    assert!(
        position(&call, &Kind::Settled(Settlement::Reply))
            < position(&call, &Kind::Start(Stage::Publication))
    );
    assert!(
        position(&call, &Kind::Start(Stage::Publication))
            < position(&call, &Kind::PublicationFailed)
    );
    assert!(
        position(&call, &Kind::PublicationFailed) < position(&call, &Kind::End(Stage::Publication))
    );
    assert!(
        records
            .iter()
            .any(|record| record.kind == Kind::End(Stage::Drain))
    );
}

#[test]
fn observation_records_panics_and_encoding_fallback_before_publication() {
    let (mut options, receiver) = observed();
    options.max_output_bytes = 1024;
    let host = Host::ready(
        Script::new(|request, _| {
            assert!(
                arguments(&request)["message"] != "panic",
                "Acme handler panic"
            );
            text(&"x".repeat(2048))
        }),
        options,
        json!({}),
    );
    for (id, message) in [(1, "panic"), (2, "large")] {
        host.call(id, "acme_echo", json!({"message":message}));
        assert_eq!(host.recv()["error"]["code"], rpc::INTERNAL_ERROR);
    }
    host.finish().0.unwrap();
    let records: Vec<_> = receiver.try_iter().collect();
    assert!(
        for_id(&records, &json!(1))
            .iter()
            .any(|record| record.kind == Kind::Panicked)
    );
    let large = for_id(&records, &json!(2));
    settled_once(&large, Settlement::Reply);
    assert!(
        position(&large, &Kind::EncodeFailed) < position(&large, &Kind::Start(Stage::Publication))
    );
}

fn assert_encoding_fallback(records: &[Observation], id: u64) {
    let call = for_id(records, &json!(id));
    settled_once(&call, Settlement::Reply);
    assert_eq!(
        call.iter()
            .filter(|record| record.kind == Kind::EncodeFailed)
            .count(),
        1
    );
    let failed = position(&call, &Kind::EncodeFailed);
    assert!(position(&call, &Kind::Start(Stage::Encode)) < failed);
    assert!(failed < position(&call, &Kind::End(Stage::Encode)));
    assert!(failed < position(&call, &Kind::Start(Stage::Publication)));
}

#[test]
fn observation_oversized_legacy_prompt_records_encoding_failure() {
    use canonic_mcp_kit::{Content, GetPromptResult, Prompt, PromptCatalog, PromptMessage, Role};

    let (mut options, receiver) = observed();
    options.max_output_bytes = 1024;
    options.session.prompts = Some(Arc::new(
        PromptCatalog::new(vec![Prompt::new("acme_review")]).unwrap(),
    ));
    let host = Host::ready(
        Script::new(|_, _| {
            Outcome::Prompt(
                GetPromptResult::new(
                    None,
                    vec![PromptMessage {
                        role: Role::User,
                        content: Content::text("x".repeat(2048)),
                    }],
                )
                .unwrap(),
            )
        }),
        options,
        json!({}),
    );
    host.send(&rpc::request(
        json!(1),
        "prompts/get",
        json!({"name":"acme_review"}),
    ));
    assert_eq!(host.recv()["error"]["code"], rpc::INTERNAL_ERROR);
    host.finish().0.unwrap();
    assert_encoding_fallback(&receiver.try_iter().collect::<Vec<_>>(), 1);
}

#[test]
fn observation_oversized_modern_errors_record_encoding_failure() {
    use canonic_mcp_kit::RpcError;

    let (mut options, receiver) = observed();
    options.max_output_bytes = 1024;
    options.session.modern = Some(ModernOptions::new(ServerInfo::new("acme", "1")));
    let host = Host::start(
        Script::new(|request, _| {
            let error = RpcError::new(rpc::INTERNAL_ERROR, "x".repeat(2048));
            if arguments(&request)["message"] == "task" {
                Outcome::Task(Err(error))
            } else {
                Outcome::Method(Err(error))
            }
        }),
        options,
    );
    for (id, message) in [(1, "method"), (2, "task")] {
        host.send(&rpc::request(
            json!(id),
            "tools/call",
            json!({"name":"acme_echo","arguments":{"message":message},"_meta":{
                modern::VERSION_KEY:modern::PROTOCOL_VERSION,modern::CAPABILITIES_KEY:{}}}),
        ));
        assert_eq!(host.recv()["error"]["code"], rpc::INTERNAL_ERROR);
    }
    host.finish().0.unwrap();
    let records: Vec<_> = receiver.try_iter().collect();
    for id in [1, 2] {
        assert_encoding_fallback(&records, id);
    }
}

#[test]
fn observation_oversized_task_payload_records_encoding_failure_but_validation_does_not() {
    use canonic_mcp_kit::task::{
        self, Metadata, Snapshot, State, TaskId, TaskOptions, TaskResult, Timestamp,
    };
    use std::time::Duration;

    let (mut options, receiver) = observed();
    options.max_output_bytes = 1024;
    options.session.modern = Some(ModernOptions::new(ServerInfo::new("acme", "1")));
    options.session.tasks = Some(TaskOptions {
        max_records: 1,
        max_bytes: 65536,
        max_running: 1,
        max_input_rounds: 1,
        execution_timeout: Duration::from_mins(1),
        input_timeout: Duration::from_mins(1),
    });
    let snapshot = Snapshot::new(
        Metadata::new(
            TaskId::new("acme-task").unwrap(),
            Timestamp::parse("2026-11-02T12:00:00Z").unwrap(),
            60000,
        )
        .unwrap()
        .with_message("x".repeat(2048))
        .unwrap(),
        State::Working,
    )
    .unwrap();
    let host = Host::start(
        Script::new(move |_, _| Outcome::Task(Ok(TaskResult::Snapshot(snapshot.clone())))),
        options,
    );
    for (id, task_id) in [(1, "acme-task"), (2, "different-task")] {
        host.send(&rpc::request(
            json!(id),
            "tasks/get",
            json!({"taskId":task_id,"_meta":{modern::VERSION_KEY:modern::PROTOCOL_VERSION,
                modern::CAPABILITIES_KEY:{"extensions":{task::EXTENSION:{}}}}}),
        ));
        assert_eq!(host.recv()["error"]["code"], rpc::INTERNAL_ERROR);
    }
    host.finish().0.unwrap();
    let records: Vec<_> = receiver.try_iter().collect();
    assert_encoding_fallback(&records, 1);
    let invalid = for_id(&records, &json!(2));
    settled_once(&invalid, Settlement::Reply);
    assert!(
        invalid
            .iter()
            .all(|record| record.kind != Kind::EncodeFailed)
    );
}

#[test]
fn observation_multiround_elicitation_and_disconnect_close_intervals() {
    use canonic_mcp_kit::{
        elicitation::{Form, Request as Question},
        stdio::Elicit,
    };
    fn question() -> Question {
        Question::form(
            "Continue Acme?",
            Form::new(json!({"type":"object","properties":{}})).unwrap(),
        )
        .unwrap()
    }
    let (options, receiver) = observed();
    let host = Host::ready(
        Script::new(|_, _| {
            Outcome::Elicit(Elicit::new(question(), |_, _| {
                Outcome::Elicit(Elicit::new(question(), |_, _| text("done")))
            }))
        }),
        options,
        json!({"elicitation":{}}),
    );
    host.call(1, "acme_echo", json!({}));
    for _ in 0..2 {
        let question = host.recv();
        host.send(&rpc::success(
            question["id"].clone(),
            json!({"action":"decline"}),
        ));
    }
    assert_eq!(host.recv()["id"], 1);
    host.call(2, "acme_echo", json!({}));
    assert_eq!(host.recv()["method"], "elicitation/create");
    host.finish().0.unwrap();
    let records: Vec<_> = receiver.try_iter().collect();
    let call = for_id(&records, &json!(1));
    settled_once(&call, Settlement::Reply);
    let assignments: HashSet<_> = call
        .iter()
        .filter(|record| record.kind == Kind::Start(Stage::Handler))
        .map(|record| record.assignment)
        .collect();
    assert_eq!(assignments.len(), 3);
    assert_eq!(
        call.iter()
            .filter(|record| record.kind == Kind::Start(Stage::Interaction))
            .count(),
        2
    );
    assert_eq!(
        call.iter()
            .filter(|record| record.kind == Kind::End(Stage::Interaction))
            .count(),
        2
    );
    let closed = for_id(&records, &json!(2));
    settled_once(&closed, Settlement::Disconnected);
    assert!(
        closed
            .iter()
            .any(|record| record.kind == Kind::End(Stage::Interaction))
    );
}

#[test]
fn observation_running_deadline_settles_before_late_worker_release() {
    let (options, receiver) = observed();
    let (release, released) = mpsc::sync_channel(1);
    let released = std::sync::Mutex::new(released);
    let host = Host::ready(
        Script::new(move |_, _| {
            released.lock().unwrap().recv_timeout(WAIT).unwrap();
            text("late")
        }),
        options,
        json!({}),
    );
    host.call(1, "acme_probe", json!({"timeoutMs":1000}));
    let mut records = Vec::new();
    loop {
        let record = receiver.recv_timeout(WAIT).unwrap();
        let started = record.rpc_id.as_deref() == Some(&json!(1))
            && record.kind == Kind::Start(Stage::Handler);
        records.push(record);
        if started {
            break;
        }
    }
    assert_eq!(host.recv()["error"]["code"], rpc::DEADLINE_EXCEEDED);
    release.send(()).unwrap();
    host.finish().0.unwrap();
    records.extend(receiver.try_iter());
    let call = for_id(&records, &json!(1));
    settled_once(&call, Settlement::Deadline);
    assert!(
        position(&call, &Kind::Settled(Settlement::Deadline))
            < position(&call, &Kind::End(Stage::Handler))
    );
    assert!(call.iter().any(|record| record.kind == Kind::Discarded));
}

#[test]
fn observation_unavailable_and_expired_confirmation_have_distinct_waits() {
    for forms in [false, true] {
        let (mut options, receiver) = observed();
        options.confirmation_timeout = std::time::Duration::from_millis(20);
        let host = Host::ready(
            Script::new(|_, _| {
                Outcome::Confirm(Confirm::new("Continue Acme?", |_, _| text("done")))
            }),
            options,
            if forms {
                json!({"elicitation":{}})
            } else {
                json!({})
            },
        );
        host.call(1, "acme_echo", json!({}));
        if forms {
            assert_eq!(host.recv()["method"], "elicitation/create");
            assert_eq!(host.recv()["method"], "notifications/cancelled");
            assert_eq!(host.recv()["error"]["code"], rpc::DEADLINE_EXCEEDED);
        } else {
            assert_eq!(host.recv()["id"], 1);
        }
        host.finish().0.unwrap();
        let records: Vec<_> = receiver.try_iter().collect();
        let call = for_id(&records, &json!(1));
        settled_once(
            &call,
            if forms {
                Settlement::Deadline
            } else {
                Settlement::Reply
            },
        );
        assert_eq!(
            call.iter()
                .filter(|record| record.kind == Kind::Start(Stage::Interaction))
                .count(),
            usize::from(forms)
        );
        assert_eq!(
            call.iter()
                .filter(|record| record.kind == Kind::End(Stage::Interaction))
                .count(),
            usize::from(forms)
        );
    }
}
