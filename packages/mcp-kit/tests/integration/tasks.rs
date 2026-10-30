//! Tasks through real durable consumer storage, the core and the shared stdio driver.

#[path = "../../examples/tasks/store.rs"]
mod store;

use crate::fixture::{Events, Host, Latch, Script};
use canonic_mcp_kit::{
    Catalog, Inbound, ModernOptions, RequestContext, RpcError, ServerInfo, Session, SessionOptions,
    Tool, json, modern, rpc,
    stdio::{self, Lane, Options, Outcome, Request, ServeError},
    task::{self, TaskId, TaskRequest, TaskResult, Timestamp},
};
use serde_json::{Value, json as value};
use std::{
    fs,
    io::{self, BufReader, Read, Write},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    thread,
};

struct Folder(PathBuf);
impl Folder {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "acme-task-fixture-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).expect("create this fixture's directory");
        Self(path)
    }
}
impl Drop for Folder {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).expect("remove only this fixture's directory");
    }
}
fn clock() -> Timestamp {
    Timestamp::parse("2026-10-30T12:00:00Z").expect("valid fixture time")
}
fn caps() -> Value {
    value!({"extensions":{task::EXTENSION:{}},"elicitation":{"form":{}}})
}
fn frame(id: u64, method: &str, mut params: Value) -> Value {
    params["_meta"] =
        value!({modern::VERSION_KEY:modern::PROTOCOL_VERSION,modern::CAPABILITIES_KEY:caps()});
    rpc::request(value!(id), method, params)
}
fn options() -> Options {
    Options {
        session: SessionOptions {
            modern: Some(ModernOptions::new(ServerInfo::new("acme", "2"))),
            tasks: Some(store::policy()),
            ..SessionOptions::default()
        },
        lanes: vec![Lane::new("rpc", 2, 8)],
        ..Options::default()
    }
}
fn catalog() -> Arc<Catalog> {
    Arc::new(
        Catalog::new(vec![Tool::new(
            "acme_report",
            "Start the Acme report workflow.",
            value!({"type":"object","additionalProperties":false}),
        )])
        .expect("valid report catalog"),
    )
}
fn dispatch(
    store: &store::Store,
    request: Request,
    context: &RequestContext,
    now: &Timestamp,
) -> Result<TaskResult, RpcError> {
    match request {
        Request::Tool(_) => store
            .create(context, "scope-a", now)
            .map(TaskResult::Created),
        Request::Task(TaskRequest::Get { task_id }) => store
            .get(&task_id, "scope-a", now)
            .map(TaskResult::Snapshot),
        Request::Task(request @ TaskRequest::Update { .. }) => {
            if store.accept(&request, context, "scope-a", now)?.is_some() {
                store.finish_report(request.id(), "scope-a", now)?;
            }
            Ok(TaskResult::Updated)
        }
        Request::Task(TaskRequest::Cancel { task_id }) => store
            .cancel(&task_id, "scope-a", now)
            .map(|()| TaskResult::Cancelled),
        _ => Err(RpcError::new(-32601, "Method not found")),
    }
}
fn host(store: Arc<store::Store>, now: Arc<Mutex<Timestamp>>) -> Host {
    Host::start_with(
        Arc::new(Script::new(move |request, context| {
            Outcome::Task(dispatch(
                &store,
                request,
                context.request_context().expect("modern request"),
                &now.lock().expect("fixture clock"),
            ))
        })),
        catalog(),
        options(),
    )
}
fn create(host: &Host, id: u64) -> TaskId {
    host.send(&frame(id, "tools/call", value!({"name":"acme_report"})));
    let reply = host.recv();
    assert_eq!(reply["result"]["resultType"], "task", "{reply}");
    assert!(reply["result"].get("task").is_none());
    TaskId::new(reply["result"]["taskId"].as_str().expect("task ID string")).expect("valid task ID")
}

#[test]
fn tasks_driver_reconnects_repeats_pending_input_and_preserves_tool_results() {
    let root = Folder::new();
    let now = Arc::new(Mutex::new(clock()));
    let store = Arc::new(store::Store::open(&root.0, &clock()).unwrap());
    let connection = host(store.clone(), now.clone());
    let id = create(&connection, 1);
    connection.finish().0.unwrap();
    let connection = host(store.clone(), now);
    for rpc_id in [2, 3] {
        connection.send(&frame(rpc_id, "tasks/get", value!({"taskId":id})));
        let reply = connection.recv();
        assert_eq!(reply["result"]["status"], "input_required");
        assert!(
            reply["result"]["inputRequests"]
                .get("report-approval")
                .is_some()
        );
        assert_eq!(reply["result"]["resultType"], "complete");
    }
    // Malformed known input is refused without consuming the pending question.
    connection.send(&frame(
        4,
        "tasks/update",
        value!({"taskId":id,"inputResponses":{"report-approval":{"action":"wrong"}}}),
    ));
    assert_eq!(connection.recv()["error"]["code"], -32602);
    assert_eq!(
        store.get(&id, "scope-a", &clock()).unwrap().status(),
        "input_required"
    );
    connection.send(&frame(
        5,
        "tasks/update",
        value!({"taskId":id,"inputResponses":{"report-approval":{"action":"accept"}}}),
    ));
    let ack = connection.recv();
    assert_eq!(ack["result"]["resultType"], "complete");
    assert!(ack["result"].get("taskId").is_none());
    let before = store.get(&id, "scope-a", &clock()).unwrap();
    connection.send(&frame(6,"tasks/update",value!({"taskId":id,"inputResponses":{"report-approval":{"action":"decline"},"unknown":{"result":{}}}})));
    assert_eq!(connection.recv()["result"]["resultType"], "complete");
    connection.send(&frame(7, "tasks/get", value!({"taskId":id})));
    let reply = connection.recv();
    assert_eq!(reply["result"]["status"], "completed");
    assert_eq!(
        reply["result"]["result"]["content"][0]["text"],
        "Acme report generated."
    );
    assert!(reply["result"]["result"].get("_meta").is_none());
    assert_eq!(before, store.get(&id, "scope-a", &clock()).unwrap());
    connection.finish().0.unwrap();
}

#[test]
fn tasks_core_runtime_and_driver_share_creation_get_update_and_cancel() {
    let root = Folder::new();
    let store = store::Store::open(&root.0, &clock()).unwrap();
    let mut session = Session::try_new(catalog(), options().session).unwrap();
    let mut exchange = |message: Value| {
        let (id, request) = match session.receive_value(message) {
            Inbound::Tool { id, call } => (id, Request::Tool(call)),
            Inbound::Task { id, request } => (id, Request::Task(request)),
            event => panic!("unexpected event {event:?}"),
        };
        let context = session.request_context(&id).unwrap();
        let result = dispatch(&store, request, &context, &clock()).unwrap();
        assert!(session.finish(&id));
        serde_json::from_slice::<Value>(
            &context.encode_task(&id, &result, 4096, &|| Ok(())).unwrap(),
        )
        .unwrap()
    };
    let created = exchange(frame(1, "tools/call", value!({"name":"acme_report"})));
    let id = created["result"]["taskId"].clone();
    assert_eq!(
        exchange(frame(2, "tasks/get", value!({"taskId":id})))["result"]["status"],
        "input_required"
    );
    exchange(frame(
        3,
        "tasks/update",
        value!({"taskId":id,"inputResponses":{"report-approval":{"action":"decline"}}}),
    ));
    let done = exchange(frame(4, "tasks/get", value!({"taskId":id})));
    assert_eq!(done["result"]["status"], "completed");
    assert_eq!(done["result"]["result"]["isError"], true);
    exchange(frame(5, "tasks/cancel", value!({"taskId":id})));
    assert_eq!(exchange(frame(6, "tasks/get", value!({"taskId":id}))), {
        let mut reply = done;
        reply["id"] = value!(6);
        reply
    });
    let second =
        exchange(frame(7, "tools/call", value!({"name":"acme_report"})))["result"]["taskId"]
            .clone();
    exchange(frame(8, "tasks/cancel", value!({"taskId":second})));
    assert_eq!(
        exchange(frame(9, "tasks/get", value!({"taskId":second})))["result"]["status"],
        "cancelled"
    );
}

fn tool_context() -> Arc<RequestContext> {
    let mut session = Session::new(catalog(), options().session);
    assert!(matches!(
        session.receive_value(frame(1, "tools/call", value!({"name":"acme_report"}))),
        Inbound::Tool { .. }
    ));
    session
        .request_context(&value!(1))
        .expect("modern tool context")
}

#[test]
fn tasks_durable_reopen_survives_worker_loss_and_never_replays_consumed_input() {
    let root = Folder::new();
    let store = store::Store::open(&root.0, &clock()).unwrap();
    assert!(
        store::Store::open(&root.0, &clock()).is_err(),
        "one kernel-held consumer owner"
    );
    let snapshot = store.create(&tool_context(), "scope-a", &clock()).unwrap();
    let id = snapshot.metadata().id().clone();
    drop(store);
    let store = store::Store::open(&root.0, &clock()).unwrap();
    assert_eq!(snapshot, store.get(&id, "scope-a", &clock()).unwrap());
    let mut session = Session::new(catalog(), options().session);
    let Inbound::Task {
        id: rpc_id,
        request,
    } = session.receive_value(frame(
        2,
        "tasks/update",
        value!({"taskId":id,"inputResponses":{"report-approval":{"action":"accept"}}}),
    ))
    else {
        panic!("update")
    };
    store
        .accept(
            &request,
            &session.request_context(&rpc_id).unwrap(),
            "scope-a",
            &clock(),
        )
        .unwrap();
    // Crash boundary: input intent is committed, executor completion is absent.
    drop(store);
    let store = store::Store::open(&root.0, &clock()).unwrap();
    let recovered = store.get(&id, "scope-a", &clock()).unwrap();
    assert_eq!(recovered.status(), "failed");
    let encoded = serde_json::to_value(&recovered).unwrap();
    assert!(
        encoded["error"]["message"]
            .as_str()
            .unwrap()
            .contains("effects may have occurred")
    );
    assert!(
        store
            .accept(
                &request,
                &session.request_context(&rpc_id).unwrap(),
                "scope-a",
                &clock()
            )
            .unwrap()
            .is_none()
    );
    store.finish_report(&id, "scope-a", &clock()).unwrap();
    assert_eq!(store.get(&id, "scope-a", &clock()).unwrap(), recovered);
}

#[test]
fn tasks_rpc_cancellation_after_commit_leaves_the_job_and_releases_original_id() {
    let root = Folder::new();
    let store = Arc::new(store::Store::open(&root.0, &clock()).unwrap());
    let events = Events::new();
    let latch = Latch::default();
    let saved = Arc::new(Mutex::new(None));
    let (owned, notice, release, id) =
        (store.clone(), events.clone(), latch.clone(), saved.clone());
    let connection = Host::start_with(
        Arc::new(Script::new(move |request, context| {
            if matches!(request, Request::Tool(_)) {
                let result = owned
                    .create(context.request_context().unwrap(), "scope-a", &clock())
                    .unwrap();
                *id.lock().unwrap() = Some(result.metadata().id().clone());
                notice.record("committed");
                release.wait(context);
                Outcome::Task(Ok(TaskResult::Created(result)))
            } else {
                Outcome::Task(dispatch(
                    &owned,
                    request,
                    context.request_context().unwrap(),
                    &clock(),
                ))
            }
        })),
        catalog(),
        options(),
    );
    connection.send(&frame(1, "tools/call", value!({"name":"acme_report"})));
    assert_eq!(events.next(), "committed");
    let id = saved.lock().unwrap().clone().unwrap();
    // A separate poll is usable before the creation reply: durable visibility.
    connection.send(&frame(2, "tasks/get", value!({"taskId":id})));
    assert_eq!(connection.recv()["result"]["status"], "input_required");
    connection.cancel(1);
    // Ping is a dispatcher barrier after cancel. Releasing first could let
    // the creation reply legitimately beat cancellation on the input reader.
    connection.assert_quiet();
    latch.release();
    connection.send(&frame(1, "tasks/get", value!({"taskId":id})));
    assert_eq!(connection.recv()["result"]["status"], "input_required");
    connection.finish().0.unwrap();
}

#[test]
fn tasks_expiry_scope_and_capacity_are_consumer_owned() {
    let root = Folder::new();
    let store = store::Store::open(&root.0, &clock()).unwrap();
    let context = tool_context();
    let mut ids = Vec::new();
    for _ in 0..store::policy().max_records {
        ids.push(
            store
                .create(&context, "scope-a", &clock())
                .unwrap()
                .metadata()
                .id()
                .clone(),
        );
    }
    assert_eq!(
        store
            .create(&context, "scope-a", &clock())
            .unwrap_err()
            .code,
        -32000
    );
    assert_eq!(
        store.get(&ids[0], "scope-b", &clock()).unwrap_err(),
        task::not_found()
    );
    let expired = Timestamp::parse("2026-10-30T12:10:00Z").unwrap();
    assert_eq!(
        store.get(&ids[0], "scope-a", &expired).unwrap_err(),
        task::not_found()
    );
    assert_eq!(
        store.cancel(&ids[0], "scope-a", &expired).unwrap_err(),
        task::not_found()
    );
    // A capacity refusal preserved all unexpired records; expiry reclaims slots.
    assert_eq!(
        store.get(&ids[0], "scope-a", &clock()).unwrap().status(),
        "input_required"
    );
    assert!(store.create(&context, "scope-a", &expired).is_ok());
    assert_eq!(
        store.get(&ids[0], "scope-a", &expired).unwrap_err(),
        task::not_found()
    );
}

#[test]
fn tasks_consumer_execution_deadline_is_distinct_from_retention() {
    let root = Folder::new();
    let store = store::Store::open(&root.0, &clock()).unwrap();
    let task = store.create(&tool_context(), "scope-a", &clock()).unwrap();
    let id = task.metadata().id().clone();
    let mut session = Session::new(catalog(), options().session);
    let Inbound::Task {
        id: rpc_id,
        request,
    } = session.receive_value(frame(
        1,
        "tasks/update",
        value!({"taskId":id,"inputResponses":{"report-approval":{"action":"accept"}}}),
    ))
    else {
        panic!("update")
    };
    store
        .accept(
            &request,
            &session.request_context(&rpc_id).unwrap(),
            "scope-a",
            &clock(),
        )
        .unwrap();
    let deadline = Timestamp::parse("2026-10-30T12:01:00Z").unwrap();
    // Still retained and readable, but the consumer's execution deadline has elapsed.
    assert_eq!(
        store.get(&id, "scope-a", &deadline).unwrap().status(),
        "working"
    );
    store.finish_report(&id, "scope-a", &deadline).unwrap();
    let failed = serde_json::to_value(store.get(&id, "scope-a", &deadline).unwrap()).unwrap();
    assert_eq!(failed["status"], "failed");
    assert_eq!(failed["error"]["code"], -32001);
}

#[test]
fn tasks_storage_outage_cannot_publish_a_handle_or_look_like_missing() {
    let root = Folder::new();
    let store = store::Store::open(&root.0, &clock()).unwrap();
    let unavailable = root.0.with_extension("offline");
    fs::rename(&root.0, &unavailable).unwrap();
    let result = store.create(&tool_context(), "scope-a", &clock());
    // Restore before asserting, so this fixture owns and cleans every file.
    fs::rename(&unavailable, &root.0).unwrap();
    assert_eq!(result.unwrap_err().code, -32603);
    assert_eq!(
        store
            .get(&TaskId::new("unknown").unwrap(), "scope-a", &clock())
            .unwrap_err()
            .code,
        -32603
    );
}

#[test]
fn tasks_output_loss_after_commit_retains_a_recoverable_handle() {
    struct Input {
        bytes: std::io::Cursor<Vec<u8>>,
        committed: mpsc::Receiver<()>,
    }
    impl Read for Input {
        fn read(&mut self, target: &mut [u8]) -> io::Result<usize> {
            let read = self.bytes.read(target)?;
            if read == 0 {
                self.committed
                    .recv_timeout(crate::fixture::WAIT)
                    .map_err(io::Error::other)?;
            }
            Ok(read)
        }
    }
    struct FailedOutput;
    impl Write for FailedOutput {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "fixture output lost",
            ))
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let root = Folder::new();
    let store = Arc::new(store::Store::open(&root.0, &clock()).unwrap());
    let saved = Arc::new(Mutex::new(None));
    let id = saved.clone();
    let owned = store.clone();
    let (committed, received) = mpsc::channel();
    let handler = Arc::new(Script::new(move |request, context| {
        let result = dispatch(
            &owned,
            request,
            context.request_context().unwrap(),
            &clock(),
        )
        .unwrap();
        let TaskResult::Created(snapshot) = &result else {
            panic!("created")
        };
        *id.lock().unwrap() = Some(snapshot.metadata().id().clone());
        committed.send(()).unwrap();
        Outcome::Task(Ok(result))
    }));
    let mut bytes = json::encode(
        &frame(1, "tools/call", value!({"name":"acme_report"})),
        4096,
    )
    .unwrap();
    bytes.push(b'\n');
    let input = Input {
        bytes: std::io::Cursor::new(bytes),
        committed: received,
    };
    let result = stdio::serve(
        handler,
        catalog(),
        BufReader::new(input),
        FailedOutput,
        &options(),
    );
    assert!(matches!(result, Err(ServeError::Output(_))));
    let task_id = saved.lock().unwrap().clone().unwrap();
    assert_eq!(
        store.get(&task_id, "scope-a", &clock()).unwrap().status(),
        "input_required"
    );
    drop(store);
    let reopened = store::Store::open(&root.0, &clock()).unwrap();
    assert_eq!(
        reopened
            .get(&task_id, "scope-a", &clock())
            .unwrap()
            .status(),
        "input_required"
    );
}

#[test]
fn tasks_completion_and_cancel_settle_one_terminal_snapshot() {
    let root = Folder::new();
    let store = Arc::new(store::Store::open(&root.0, &clock()).unwrap());
    let snapshot = store.create(&tool_context(), "scope-a", &clock()).unwrap();
    let id = snapshot.metadata().id().clone();
    let mut session = Session::new(catalog(), options().session);
    let Inbound::Task {
        id: rpc_id,
        request,
    } = session.receive_value(frame(
        1,
        "tasks/update",
        value!({"taskId":id,"inputResponses":{"report-approval":{"action":"accept"}}}),
    ))
    else {
        panic!("update")
    };
    store
        .accept(
            &request,
            &session.request_context(&rpc_id).unwrap(),
            "scope-a",
            &clock(),
        )
        .unwrap();
    let gate = Arc::new(std::sync::Barrier::new(3));
    let done = {
        let store = store.clone();
        let id = id.clone();
        let gate = gate.clone();
        thread::spawn(move || {
            gate.wait();
            store.finish_report(&id, "scope-a", &clock())
        })
    };
    let cancel = {
        let store = store.clone();
        let id = id.clone();
        let gate = gate.clone();
        thread::spawn(move || {
            gate.wait();
            store.cancel(&id, "scope-a", &clock())
        })
    };
    gate.wait();
    let results = [done.join().unwrap(), cancel.join().unwrap()];
    assert!(results.iter().any(Result::is_ok));
    for error in results.into_iter().filter_map(Result::err) {
        assert_eq!(error.code, -32000);
    }
    let settled = store.get(&id, "scope-a", &clock()).unwrap();
    assert!(settled.terminal());
    store.finish_report(&id, "scope-a", &clock()).unwrap();
    store.cancel(&id, "scope-a", &clock()).unwrap();
    assert_eq!(settled, store.get(&id, "scope-a", &clock()).unwrap());
}

#[test]
fn tasks_eof_preserves_get_but_refuses_queued_updates() {
    let root = Folder::new();
    let store = Arc::new(store::Store::open(&root.0, &clock()).unwrap());
    let id = store
        .create(&tool_context(), "scope-a", &clock())
        .unwrap()
        .metadata()
        .id()
        .clone();
    let events = Events::new();
    let latch = Latch::default();
    let owned = store.clone();
    let event = events.clone();
    let release = latch.clone();
    let mut opts = options();
    opts.lanes = vec![Lane::new("rpc", 1, 8)];
    let mut connection = Host::start_with(
        Arc::new(Script::new(move |request, context| {
            if matches!(request, Request::Task(TaskRequest::Get { .. })) {
                event.record("get started");
                assert!(release.wait(context));
            }
            Outcome::Task(dispatch(
                &owned,
                request,
                context.request_context().unwrap(),
                &clock(),
            ))
        })),
        catalog(),
        opts,
    );
    connection.send(&frame(1, "tasks/get", value!({"taskId":id})));
    assert_eq!(events.next(), "get started");
    connection.send(&frame(
        2,
        "tasks/update",
        value!({"taskId":id,"inputResponses":{"report-approval":{"action":"accept"}}}),
    ));
    connection.close();
    let refused = connection.recv();
    assert_eq!(refused["id"], 2);
    assert_eq!(refused["error"]["code"], -32800);
    latch.release();
    assert_eq!(connection.recv()["result"]["status"], "input_required");
    connection.finish().0.unwrap();
    assert_eq!(
        store.get(&id, "scope-a", &clock()).unwrap().status(),
        "input_required"
    );
}
