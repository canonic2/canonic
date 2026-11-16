//! Stable MCP sessions, exact source ownership and native subprocess cutover.
use super::fixture::{self, Events, Host, Script, result_text, text};
use canonic_mcp_kit::{
    stdio::{Confirm, Context, Options, Outcome, Request},
    worker::{self, Contract, Frontend, Implementation},
};
use canonic_process_kit::{
    self as process,
    endpoint::{Lease, Root},
    protocol::Identity,
    worker::Worker,
};
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Write},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

const WAIT: Duration = Duration::from_secs(5);
struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let mut bytes = [0; 8];
        getrandom::fill(&mut bytes).expect("owned fixture identity");
        let name = format!("{:016x}", u64::from_ne_bytes(bytes));
        Self(std::env::temp_dir().join(format!("acme-mcp-{name}")))
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn identity(id: &str, build: &str) -> Identity {
    Identity {
        manager_id: id.into(),
        build: build.into(),
        protocol: 1,
        control_version: process::protocol::CONTROL_VERSION,
        capabilities: vec![],
    }
}
type Call = dyn Fn(Request, &Context) -> Outcome + Send + Sync;
struct Fake {
    identity: Identity,
    contract: Contract,
    call: Box<Call>,
    events: Events,
}
impl Worker for Fake {
    fn identity(&self) -> &Identity {
        &self.identity
    }
    fn prepare(&self, _: &worker::Transition<'_>) -> process::Result<()> {
        self.events.record("prepare");
        Ok(())
    }
    fn fence(&self, _: &worker::Transition<'_>) -> process::Result<()> {
        self.events.record("fence");
        Ok(())
    }
    fn retire(&self, _: &worker::Transition<'_>) -> process::Result<()> {
        self.events.record("retire");
        Ok(())
    }
    fn activate(&self, _: &worker::Transition<'_>) -> process::Result<()> {
        self.events.record("activate");
        Ok(())
    }
    fn stop(&self, _: Instant) -> process::Result<()> {
        self.events.record("stop");
        Ok(())
    }
}
impl Implementation for Fake {
    fn contract(&self) -> &Contract {
        &self.contract
    }
    fn call(&self, request: Request, context: &Context) -> Result<Outcome, worker::Failure> {
        Ok((self.call)(request, context))
    }
}
fn contract(options: &Options, policy: &str) -> Contract {
    Contract::new(&fixture::catalog(), options, json!({"policy":policy}), 1024)
        .expect("fixture contract")
}
fn fake(
    id: &str,
    build: &str,
    contract: Contract,
    events: Events,
    call: impl Fn(Request, &Context) -> Outcome + Send + Sync + 'static,
) -> Arc<dyn Implementation> {
    Arc::new(Fake {
        identity: identity(id, build),
        contract,
        events,
        call: Box::new(call),
    })
}
fn setup(
    options: &Options,
    active: Arc<dyn Implementation>,
    preparation: Duration,
) -> (Directory, Arc<Frontend>) {
    let directory = Directory::new();
    let root = Root::explicit(&directory.0).expect("fixture root");
    let frontend = Frontend::new(
        Lease::acquire(&root, WAIT).expect("fixture lease"),
        identity("frontend", "1.0.0"),
        active,
        worker::SupervisorOptions {
            preparation,
            activation: WAIT,
            cleanup: WAIT,
            capacity: options.lanes.iter().map(|lane| lane.capacity).sum(),
            history: 4,
            history_bytes: 65536,
        },
        1024 * 1024,
        8,
    )
    .expect("fixture frontend");
    (directory, frontend)
}
fn update() -> worker::Request {
    worker::Request {
        frontend: "frontend".into(),
        sequence: 1,
        selection: worker::Selection {
            key: "acme-v2".into(),
            build: "2.0.0".into(),
            protocol: 1,
            requires: vec![],
        },
    }
}
fn host(frontend: Arc<Frontend>, mut options: Options, capabilities: Value) -> Host {
    options.worker = Some(frontend);
    Host::ready(
        Script::new(|_, _| panic!("resident callback executed work")),
        options,
        capabilities,
    )
}

#[test]
fn worker_replacement_retains_initialized_session_and_rejects_incompatible_policy() {
    let options = Options::default();
    let declared = contract(&options, "v1");
    let events = Events::new();
    let (_directory, frontend) = setup(
        &options,
        fake("old", "1.0.0", declared.clone(), events.clone(), |_, _| {
            text("old")
        }),
        WAIT,
    );
    let host = host(frontend.clone(), options.clone(), json!({}));
    host.call(1, "acme_echo", json!({}));
    assert_eq!(result_text(&host.recv()), "old");
    let bad = fake(
        "bad",
        "2.0.0",
        contract(&options, "bad"),
        events.clone(),
        |_, _| text("bad"),
    );
    assert!(
        frontend
            .replace(&update(), |_| Ok(bad), |_| panic!("incompatible commit"))
            .is_err()
    );
    assert_eq!(frontend.status().unwrap().phase, worker::Phase::Deferred);
    host.call(1, "acme_echo", json!({}));
    assert_eq!(result_text(&host.recv()), "old");
    let next = fake("new", "2.0.0", declared, events, |_, _| text("new"));
    let mut request = update();
    request.sequence = 2;
    frontend
        .replace(&request, |_| Ok(next), |_| Ok(()))
        .unwrap();
    host.call(1, "acme_echo", json!({}));
    assert_eq!(result_text(&host.recv()), "new");
    assert!(host.finish().0.is_ok());
    assert_eq!(frontend.status().unwrap().phase, worker::Phase::Stopped);
}

#[test]
fn worker_replacement_preserves_cancelled_execution_until_it_actually_returns() {
    let options = Options::default();
    let declared = contract(&options, "v1");
    let events = Events::new();
    let (release, waiting) = std::sync::mpsc::sync_channel(1);
    let waiting = Mutex::new(waiting);
    let started = events.clone();
    let (_directory, frontend) = setup(
        &options,
        fake(
            "old",
            "1.0.0",
            declared.clone(),
            events.clone(),
            move |_, _| {
                started.record("running");
                // Intentionally ignore cancellation: the adapter cannot infer settlement.
                waiting.lock().unwrap().recv_timeout(WAIT).unwrap();
                text("late")
            },
        ),
        Duration::from_millis(30),
    );
    let host = host(frontend.clone(), options, json!({}));
    host.call(1, "acme_echo", json!({}));
    assert_eq!(events.next(), "running");
    host.cancel(1);
    let candidate = fake("new", "2.0.0", declared, events.clone(), |_, _| text("new"));
    assert!(
        frontend
            .replace(
                &update(),
                |_| Ok(candidate),
                |_| panic!("cancelled work remains live")
            )
            .is_err()
    );
    assert_eq!(frontend.status().unwrap().work, 1);
    release.send(()).unwrap();
    host.send(&json!({"jsonrpc":"2.0","id":2,"method":"ping"}));
    assert_eq!(host.recv()["id"], 2);
    assert!(host.finish().0.is_ok());
}

#[test]
fn worker_replacement_defers_for_legacy_continuation_and_resource_hold() {
    let options = Options::default();
    let declared = contract(&options, "v1");
    let events = Events::new();
    let held = Arc::new(Mutex::new(None));
    let save = held.clone();
    let (_directory, frontend) = setup(
        &options,
        fake(
            "old",
            "1.0.0",
            declared.clone(),
            events.clone(),
            move |_, _| {
                let save = save.clone();
                Outcome::Confirm(Confirm::new("Continue?", move |_, context| {
                    *save.lock().unwrap() = context.worker_hold().unwrap();
                    text("accepted")
                }))
            },
        ),
        Duration::from_millis(30),
    );
    let host = host(
        frontend.clone(),
        options,
        json!({"elicitation":{"form":{}}}),
    );
    host.call(1, "acme_echo", json!({}));
    let question = host.recv();
    assert_eq!(question["method"], "elicitation/create");
    let candidate = fake("new", "2.0.0", declared.clone(), events.clone(), |_, _| {
        text("new")
    });
    assert!(
        frontend
            .replace(
                &update(),
                |_| Ok(candidate),
                |_| panic!("pending continuation")
            )
            .is_err()
    );
    host.send(
        &json!({"jsonrpc":"2.0","id":question["id"],"result":{"action":"accept","content":{}}}),
    );
    assert_eq!(result_text(&host.recv()), "accepted");
    assert_eq!(frontend.status().unwrap().held, 1);
    let mut request = update();
    request.sequence = 2;
    let candidate = fake("new2", "2.0.0", declared.clone(), events.clone(), |_, _| {
        text("new")
    });
    assert!(
        frontend
            .replace(
                &request,
                |_| Ok(candidate),
                |_| panic!("resource remains owned")
            )
            .is_err()
    );
    held.lock().unwrap().take();
    request.sequence = 3;
    let candidate = fake("new3", "2.0.0", declared, events, |_, _| text("new"));
    frontend
        .replace(&request, |_| Ok(candidate), |_| Ok(()))
        .unwrap();
    assert!(host.finish().0.is_ok());
}

#[test]
fn worker_replacement_private_codec_refuses_foreign_results_and_unbounded_frames() {
    use worker::wire::{self, Assignment, Reply};
    let assignment = Assignment {
        frontend: "front".into(),
        worker: "old".into(),
        sequence: 1,
    };
    let mut reply = Reply {
        version: wire::VERSION,
        assignment: assignment.clone(),
        kind: "method".into(),
        value: json!({}),
    };
    assert!(reply.check(&assignment).is_ok());
    reply.assignment.worker = "new".into();
    assert!(reply.check(&assignment).is_err());
    assert!(wire::encode(&reply, 1).is_err());
    assert!(wire::decode::<Reply>(br#"{"version":1,"version":1}"#, 1024).is_err());
}

#[test]
fn worker_replacement_drains_reserved_queue_while_controls_and_busy_refusals_continue() {
    let options = Options::default();
    let declared = contract(&options, "v1");
    let starts = Events::new();
    let started = starts.clone();
    let (release, waiting) = std::sync::mpsc::sync_channel(1);
    let waiting = Mutex::new(waiting);
    let (_directory, frontend) = setup(
        &options,
        fake(
            "old",
            "1.0.0",
            declared.clone(),
            Events::new(),
            move |request, _| {
                let message = fixture::arguments(&request)["message"]
                    .as_str()
                    .unwrap()
                    .to_owned();
                started.record(&message);
                if message == "first" {
                    waiting.lock().unwrap().recv_timeout(WAIT).unwrap();
                }
                text(&message)
            },
        ),
        WAIT,
    );
    let host = host(frontend.clone(), options, json!({}));
    host.call(1, "acme_echo", json!({"message":"first"}));
    assert_eq!(starts.next(), "first");
    host.call(2, "acme_echo", json!({"message":"queued"}));
    host.send(&json!({"jsonrpc":"2.0","id":99,"method":"ping"}));
    assert_eq!(host.recv()["id"], 99);
    assert_eq!(frontend.status().unwrap().work, 2);
    let events = Events::new();
    let candidate = fake("new", "2.0.0", declared, events.clone(), |_, _| text("new"));
    let controller = frontend.clone();
    let updating =
        std::thread::spawn(move || controller.replace(&update(), |_| Ok(candidate), |_| Ok(())));
    assert_eq!(events.next(), "prepare");
    host.call(3, "acme_echo", json!({}));
    let refused = host.recv();
    assert_eq!(refused["error"]["code"], -32000);
    assert!(
        refused["error"]["message"]
            .as_str()
            .unwrap()
            .contains("paused or closed")
    );
    host.send(&json!({"jsonrpc":"2.0","id":4,"method":"ping"}));
    assert_eq!(host.recv()["id"], 4);
    release.send(()).unwrap();
    assert_eq!(result_text(&host.recv()), "first");
    assert_eq!(starts.next(), "queued");
    assert_eq!(result_text(&host.recv()), "queued");
    updating.join().unwrap().unwrap();
    host.call(5, "acme_echo", json!({}));
    assert_eq!(result_text(&host.recv()), "new");
    assert!(host.finish().0.is_ok());
}

#[test]
fn worker_replacement_preserves_commit_uncertainty_and_never_reopens_fenced_source() {
    let options = Options::default();
    let declared = contract(&options, "v1");
    let (_directory, frontend) = setup(
        &options,
        fake("old", "1.0.0", declared.clone(), Events::new(), |_, _| {
            text("old")
        }),
        WAIT,
    );
    let host = host(frontend.clone(), options, json!({}));
    let candidate = fake("new", "2.0.0", declared, Events::new(), |_, _| text("new"));
    assert!(
        frontend
            .replace(
                &update(),
                |_| Ok(candidate),
                |_| Err(process::Error::Worker)
            )
            .is_err()
    );
    let status = frontend.status().unwrap();
    assert_eq!(status.phase, worker::Phase::Reconcile);
    assert_eq!(status.attempt.unwrap().commit, worker::Commit::Unknown);
    host.call(1, "acme_echo", json!({}));
    assert_eq!(host.recv()["error"]["code"], -32000);
    host.send(&json!({"jsonrpc":"2.0","id":2,"method":"ping"}));
    assert_eq!(host.recv()["id"], 2);
    assert!(host.finish().0.is_ok());
    assert_eq!(
        frontend.outcome(&update()).unwrap().commit,
        worker::Commit::Unknown
    );
}

#[test]
fn worker_replacement_close_preserves_admitted_read_only_work_and_blocks_publication() {
    let options = Options::default();
    let declared = contract(&options, "v1");
    let events = Events::new();
    let starts = events.clone();
    let (release, waiting) = std::sync::mpsc::sync_channel(1);
    let waiting = Mutex::new(waiting);
    let (_directory, frontend) = setup(
        &options,
        fake(
            "old",
            "1.0.0",
            declared.clone(),
            Events::new(),
            move |_, _| {
                starts.record("running");
                waiting.lock().unwrap().recv_timeout(WAIT).unwrap();
                text("finished")
            },
        ),
        WAIT,
    );
    let mut host = host(frontend.clone(), options, json!({}));
    host.call(1, "acme_echo", json!({}));
    assert_eq!(events.next(), "running");
    let candidate_events = Events::new();
    let candidate = fake(
        "new",
        "2.0.0",
        declared,
        candidate_events.clone(),
        |_, _| text("new"),
    );
    let controller = frontend.clone();
    let updating = std::thread::spawn(move || {
        controller.replace(
            &update(),
            |_| Ok(candidate),
            |_| panic!("closed frontend cannot commit"),
        )
    });
    assert_eq!(candidate_events.next(), "prepare");
    frontend.close();
    host.close();
    release.send(()).unwrap();
    assert_eq!(result_text(&host.recv()), "finished");
    assert!(updating.join().unwrap().is_err());
    assert!(host.finish().0.is_ok());
    assert_eq!(frontend.status().unwrap().phase, worker::Phase::Stopped);
}

struct OwnedChild(Child);

struct ContinuationDrop {
    frontend: std::sync::Weak<Frontend>,
    observed: std::sync::mpsc::SyncSender<usize>,
}
impl Drop for ContinuationDrop {
    fn drop(&mut self) {
        let work = self
            .frontend
            .upgrade()
            .expect("live frontend")
            .status()
            .expect("status")
            .work;
        self.observed
            .send(work)
            .expect("continuation drop observation");
    }
}

#[test]
fn worker_replacement_keeps_source_reserved_through_cancelled_proxy_drop() {
    let options = Options::default();
    let declared = contract(&options, "v1");
    let weak = Arc::new(Mutex::new(std::sync::Weak::new()));
    let owner = weak.clone();
    let (observed, waiting) = std::sync::mpsc::sync_channel(1);
    let (_directory, frontend) = setup(
        &options,
        fake("old", "1.0.0", declared, Events::new(), move |_, _| {
            let probe = ContinuationDrop {
                frontend: owner.lock().unwrap().clone(),
                observed: observed.clone(),
            };
            Outcome::Confirm(Confirm::new("Continue?", move |_, _| {
                let _probe = &probe;
                text("accepted")
            }))
        }),
        WAIT,
    );
    *weak.lock().unwrap() = Arc::downgrade(&frontend);
    let host = host(frontend, options, json!({"elicitation":{"form":{}}}));
    host.call(1, "acme_echo", json!({}));
    assert_eq!(host.recv()["method"], "elicitation/create");
    host.cancel(1);
    assert!(
        waiting.recv_timeout(WAIT).unwrap() > 0,
        "proxy must drop while source is still reserved"
    );
    assert!(host.finish().0.is_ok());
}

#[test]
fn worker_replacement_explicit_shutdown_can_cleanup_resources_pinned_by_holds() {
    let options = Options::default();
    let declared = contract(&options, "v1");
    let holds = Arc::new(Mutex::new(None));
    let saved = holds.clone();
    let (_directory, frontend) = setup(
        &options,
        fake(
            "old",
            "1.0.0",
            declared,
            Events::new(),
            move |_, context| {
                *saved.lock().unwrap() = context.worker_hold().unwrap();
                text("resource owned")
            },
        ),
        WAIT,
    );
    let host = host(frontend.clone(), options, json!({}));
    host.call(1, "acme_echo", json!({}));
    assert_eq!(result_text(&host.recv()), "resource owned");
    assert_eq!(frontend.status().unwrap().held, 1);
    assert!(host.finish().0.is_ok());
    assert_eq!(frontend.status().unwrap().phase, worker::Phase::Stopped);
    holds.lock().unwrap().take();
}

#[test]
fn worker_replacement_preserves_original_result_schema_after_private_decode() {
    use canonic_mcp_kit::{Catalog, ToolResult};
    let mut tools = fixture::catalog().tools().to_vec();
    tools[0]=tools[0].clone().with_output_schema(json!({"type":"object","properties":{"status":{"type":"string"}},"required":["status"],"additionalProperties":false}));
    let catalog = Arc::new(Catalog::new(tools).unwrap());
    let mut options = Options::default();
    let declared = Contract::new(&catalog, &options, json!({"policy":"v1"}), 1024).unwrap();
    let (_directory, frontend) = setup(
        &options,
        fake(
            "old",
            "1.0.0",
            declared.clone(),
            Events::new(),
            |_, context| {
                let assignment = worker::wire::Assignment {
                    frontend: "frontend".into(),
                    worker: "old".into(),
                    sequence: 1,
                };
                let reply = worker::wire::Reply {
                    version: worker::wire::VERSION,
                    assignment: assignment.clone(),
                    kind: "tool".into(),
                    value: serde_json::to_value(ToolResult::structured(json!({"status":7}), false))
                        .unwrap(),
                };
                reply.outcome(&assignment, context).unwrap()
            },
        ),
        WAIT,
    );
    options.worker = Some(frontend.clone());
    let mut host = Host::start_with(
        Arc::new(Script::new(|_, _| panic!("resident call"))),
        catalog,
        options,
    );
    host.initialize(json!({}));
    host.call(1, "acme_echo", json!({}));
    assert_eq!(host.recv()["error"]["code"], -32603);
    let candidate = fake("new", "2.0.0", declared, Events::new(), |_, _| {
        Outcome::Tool(ToolResult::structured(json!({"status":"ready"}), false))
    });
    frontend
        .replace(&update(), |_| Ok(candidate), |_| Ok(()))
        .unwrap();
    host.call(2, "acme_echo", json!({}));
    assert_eq!(
        host.recv()["result"]["structuredContent"]["status"],
        "ready"
    );
    assert!(host.finish().0.is_ok());
}

#[test]
fn worker_replacement_input_budget_refuses_before_effects() {
    let options = Options::default();
    let declared = contract(&options, "v1");
    let directory = Directory::new();
    let root = Root::explicit(&directory.0).unwrap();
    let frontend = Frontend::new(
        Lease::acquire(&root, WAIT).unwrap(),
        identity("frontend", "1.0.0"),
        fake("old", "1.0.0", declared, Events::new(), |_, _| {
            panic!("over-budget input cannot dispatch")
        }),
        worker::SupervisorOptions {
            preparation: WAIT,
            activation: WAIT,
            cleanup: WAIT,
            capacity: 8,
            history: 4,
            history_bytes: 65536,
        },
        1,
        1,
    )
    .unwrap();
    let host = host(frontend.clone(), options, json!({}));
    for id in 1..=2 {
        host.call(id, "acme_echo", json!({}));
        let refused = host.recv();
        assert_eq!(refused["error"]["code"], -32000);
        assert!(
            refused["error"]["message"]
                .as_str()
                .unwrap()
                .contains("capacity reached")
        );
    }
    assert_eq!(frontend.status().unwrap().work, 0);
    assert!(host.finish().0.is_ok());
}

#[test]
fn worker_replacement_exchange_panic_disables_generation_without_replay() {
    let options = Options::default();
    let declared = contract(&options, "v1");
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let called = calls.clone();
    let (_directory, frontend) = setup(
        &options,
        fake(
            "old",
            "1.0.0",
            declared.clone(),
            Events::new(),
            move |_, _| {
                called.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
                panic!("private exchange failed")
            },
        ),
        Duration::from_millis(30),
    );
    let host = host(frontend.clone(), options, json!({}));
    host.call(1, "acme_echo", json!({}));
    assert_eq!(host.recv()["error"]["code"], -32603);
    host.call(2, "acme_echo", json!({}));
    let refused = host.recv();
    assert_eq!(refused["error"]["code"], -32000);
    assert!(
        refused["error"]["message"]
            .as_str()
            .unwrap()
            .contains("reconcile cleanup")
    );
    assert_eq!(calls.load(std::sync::atomic::Ordering::Acquire), 1);
    let candidate = fake("new", "2.0.0", declared, Events::new(), |_, _| text("new"));
    assert_eq!(frontend.status().unwrap().held, 1);
    assert!(
        frontend
            .replace(
                &update(),
                |_| Ok(candidate),
                |_| panic!("unknown execution cannot transfer")
            )
            .is_err()
    );
    host.call(3, "acme_echo", json!({}));
    assert_eq!(host.recv()["error"]["code"], -32000);
    assert!(host.finish().0.is_ok());
}

fn continuation_panic(elicit: bool) {
    let options = Options::default();
    let declared = contract(&options, "v1");
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let called = calls.clone();
    let events = Events::new();
    let (_directory, frontend) = setup(
        &options,
        fake(
            "old",
            "1.0.0",
            declared.clone(),
            events.clone(),
            move |_, _| {
                called.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
                if elicit {
                    let question = canonic_mcp_kit::elicitation::Request::form(
                        "Continue?",
                        canonic_mcp_kit::elicitation::Form::new(json!({
                            "type":"object", "properties":{}
                        }))
                        .expect("valid continuation form"),
                    )
                    .expect("valid continuation question");
                    Outcome::Elicit(canonic_mcp_kit::stdio::Elicit::new(question, |_, _| {
                        panic!("remote continuation execution is uncertain")
                    }))
                } else {
                    Outcome::Confirm(Confirm::new("Continue?", |_, _| {
                        panic!("remote continuation execution is uncertain")
                    }))
                }
            },
        ),
        Duration::from_millis(30),
    );
    let host = host(
        frontend.clone(),
        options,
        json!({"elicitation":{"form":{}}}),
    );
    host.call(1, "acme_echo", json!({}));
    let question = host.recv();
    assert_eq!(question["method"], "elicitation/create");
    host.send(&json!({"jsonrpc":"2.0", "id":question["id"],
        "result":{"action":"accept", "content":{}}}));
    assert_eq!(host.recv()["error"]["code"], -32603);
    assert_eq!(frontend.status().expect("worker status").held, 1);
    host.call(2, "acme_echo", json!({}));
    assert_eq!(host.recv()["error"]["code"], -32000);
    assert_eq!(calls.load(std::sync::atomic::Ordering::Acquire), 1);
    let candidate = fake("new", "2.0.0", declared, Events::new(), |_, _| text("new"));
    assert!(
        frontend
            .replace(
                &update(),
                |_| Ok(candidate),
                |_| { panic!("uncertain continuation cannot transfer") }
            )
            .is_err()
    );
    assert_eq!(frontend.status().expect("worker status").held, 1);
    assert!(host.finish().0.is_ok());
    assert_eq!(events.next(), "stop");
    assert_eq!(frontend.status().expect("worker status").held, 0);
}

#[test]
fn worker_replacement_confirmation_panic_retains_uncertain_execution() {
    continuation_panic(false);
}

#[test]
fn worker_replacement_elicitation_panic_retains_uncertain_execution() {
    continuation_panic(true);
}

struct PublishingResident(Arc<Mutex<Option<canonic_mcp_kit::stdio::ChangeSender>>>);
impl canonic_mcp_kit::stdio::Handler for PublishingResident {
    fn initialize(
        &self,
        _: &canonic_mcp_kit::Client,
    ) -> Result<canonic_mcp_kit::ServerInfo, canonic_mcp_kit::RpcError> {
        Ok(canonic_mcp_kit::ServerInfo::new("acme", "1.0.0"))
    }
    fn connected(&self, sender: canonic_mcp_kit::stdio::ChangeSender) {
        *self.0.lock().expect("publication sender") = Some(sender);
    }
    fn call(&self, _: Request, _: &Context) -> Outcome {
        panic!("resident must not execute implementation work")
    }
}

#[test]
fn worker_replacement_queued_source_changes_pin_generation_until_publication() {
    let mut options = Options::default();
    options.session.modern = Some(canonic_mcp_kit::ModernOptions::new(
        canonic_mcp_kit::ServerInfo::new("acme", "1.0.0"),
    ));
    options.session.subscriptions = Some(canonic_mcp_kit::SubscriptionOptions::default());
    let declared = contract(&options, "v1");
    let sender = Arc::new(Mutex::new(None));
    let changes = sender.clone();
    let (release, waiting) = std::sync::mpsc::sync_channel(1);
    let waiting = Mutex::new(waiting);
    let events = Events::new();
    let started = events.clone();
    let (_directory, frontend) = setup(
        &options,
        fake(
            "old",
            "1.0.0",
            declared.clone(),
            Events::new(),
            move |_, context| {
                started.record("running");
                waiting.lock().unwrap().recv_timeout(WAIT).unwrap();
                let publisher = context
                    .worker_publisher(changes.lock().unwrap().take().unwrap())
                    .unwrap()
                    .unwrap();
                publisher
                    .send(canonic_mcp_kit::Change::Tools(fixture::catalog()))
                    .unwrap();
                text("queued publication")
            },
        ),
        Duration::from_millis(30),
    );
    options.worker = Some(frontend.clone());
    let mut host = Host::start_with(
        Arc::new(PublishingResident(sender)),
        fixture::catalog(),
        options,
    );
    host.initialize(json!({}));
    host.send(
        &json!({"jsonrpc":"2.0", "id":8, "method":"subscriptions/listen", "params":{
            "notifications":{"toolsListChanged":true}, "_meta":{
                "io.modelcontextprotocol/protocolVersion":"2026-07-28",
                "io.modelcontextprotocol/clientCapabilities":{}
            }
        }}),
    );
    assert_eq!(
        host.recv()["method"],
        "notifications/subscriptions/acknowledged"
    );
    host.call(1, "acme_echo", json!({}));
    assert_eq!(events.next(), "running");
    let candidate_events = Events::new();
    let candidate = fake(
        "new",
        "2.0.0",
        declared.clone(),
        candidate_events.clone(),
        |_, _| text("new"),
    );
    let controller = frontend.clone();
    let updating = std::thread::spawn(move || {
        controller.replace(
            &update(),
            |_| Ok(candidate),
            |_| panic!("queued source change still owns old process"),
        )
    });
    assert_eq!(candidate_events.next(), "prepare");
    release.send(()).unwrap();
    assert_eq!(result_text(&host.recv()), "queued publication");
    assert!(updating.join().unwrap().is_err());
    // No host input: releasing replacement must wake publication by itself.
    assert_eq!(host.recv()["method"], "notifications/tools/list_changed");
    assert_eq!(frontend.status().unwrap().held, 0);
    let mut request = update();
    request.sequence = 2;
    let candidate = fake("next", "2.0.0", declared, Events::new(), |_, _| text("new"));
    frontend
        .replace(&request, |_| Ok(candidate), |_| Ok(()))
        .unwrap();
    assert!(host.finish().0.is_ok());
}

#[cfg(feature = "http-server")]
#[test]
fn worker_replacement_http_retains_listener_ids_and_request_local_context() {
    use canonic_mcp_kit::{ModernOptions, ServerInfo, http};
    use std::io::Read;
    let mut options = Options::default();
    options.session.legacy_protocols = false;
    options.session.modern = Some(ModernOptions::new(ServerInfo::new("acme", "1.0.0")));
    let declared = contract(&options, "v1");
    let (_directory, frontend) = setup(
        &options,
        fake(
            "old",
            "1.0.0",
            declared.clone(),
            Events::new(),
            |_, context| {
                text(&format!(
                    "old:{}",
                    context.request_context().unwrap().capabilities["experimental"]["acme"]["tag"]
                        .as_str()
                        .unwrap()
                ))
            },
        ),
        WAIT,
    );
    options.worker = Some(frontend.clone());
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let shutdown = http::Shutdown::new();
    let closing = shutdown.clone();
    let server = std::thread::spawn(move || {
        http::serve(
            Arc::new(Script::new(|_, _| panic!("resident call"))),
            fixture::catalog(),
            listener,
            &http::HttpOptions::default(),
            &options,
            &closing,
        )
    });
    let post = |tag: &str| {
        let mut stream = std::net::TcpStream::connect(address).unwrap();
        stream.set_read_timeout(Some(WAIT)).unwrap();
        stream.set_write_timeout(Some(WAIT)).unwrap();
        let body=json!({"jsonrpc":"2.0","id":"same-id","method":"tools/call","params":{"name":"acme_echo","arguments":{},"_meta":{
            "io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{"experimental":{"acme":{"tag":tag}}}
        }}}).to_string();
        write!(stream,"POST /mcp HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nAccept: application/json, text/event-stream\r\nMCP-Protocol-Version: 2026-07-28\r\nMcp-Method: tools/call\r\nMcp-Name: acme_echo\r\nContent-Length: {}\r\n\r\n{body}",body.len()).unwrap();
        stream.flush().unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
        serde_json::from_str::<Value>(response.split_once("\r\n\r\n").unwrap().1).unwrap()
    };
    let old = post("first");
    assert_eq!(old["id"], "same-id");
    assert_eq!(result_text(&old), "old:first");
    let candidate = fake("new", "2.0.0", declared, Events::new(), |_, context| {
        text(&format!(
            "new:{}",
            context.request_context().unwrap().capabilities["experimental"]["acme"]["tag"]
                .as_str()
                .unwrap()
        ))
    });
    frontend
        .replace(&update(), |_| Ok(candidate), |_| Ok(()))
        .unwrap();
    let new = post("second");
    assert_eq!(new["id"], "same-id");
    assert_eq!(result_text(&new), "new:second");
    shutdown.shutdown();
    assert!(server.join().unwrap().is_ok());
    assert_eq!(frontend.status().unwrap().phase, worker::Phase::Stopped);
}
impl Drop for OwnedChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn worker_replacement_native_stdio_keeps_frontend_and_partial_frame_across_processes() {
    let directory = Directory::new();
    let mut frontend = OwnedChild(
        Command::new(env!("CARGO_BIN_EXE_acme-worker-fixture"))
            .arg(&directory.0)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap(),
    );
    let pid = frontend.0.id();
    let mut input = frontend.0.stdin.take().unwrap();
    let output = frontend.0.stdout.take().unwrap();
    let (sent, received) = std::sync::mpsc::channel();
    let reader = std::thread::spawn(move || {
        for line in BufReader::new(output).lines() {
            if sent
                .send(serde_json::from_str::<Value>(&line.unwrap()).unwrap())
                .is_err()
            {
                break;
            }
        }
    });
    writeln!(input,"{}",json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"acme","version":"1"}}})).unwrap();
    input.flush().unwrap();
    assert_eq!(received.recv_timeout(WAIT).unwrap()["id"], 1);
    writeln!(
        input,
        "{}",
        json!({"jsonrpc":"2.0","method":"notifications/initialized"})
    )
    .unwrap();
    let call=json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"acme_echo","arguments":{}}}).to_string();
    writeln!(input, "{call}").unwrap();
    input.flush().unwrap();
    let old = received.recv_timeout(WAIT).unwrap();
    assert!(result_text(&old).starts_with("1.0.0:"));
    let half = call.len() / 2;
    input.write_all(&call.as_bytes()[..half]).unwrap();
    input.flush().unwrap();
    let root = Root::explicit(directory.0.join("control")).unwrap();
    let stream = process::endpoint::connect(&root, false, WAIT).unwrap();
    let mut io = process::frame::Deadline::new(&stream, Instant::now() + WAIT);
    process::frame::write(&mut io, b"update", 16).unwrap();
    assert_eq!(process::frame::read(&mut io, 16).unwrap(), b"replaced");
    input.write_all(&call.as_bytes()[half..]).unwrap();
    input.write_all(b"\n").unwrap();
    input.flush().unwrap();
    let new = received.recv_timeout(WAIT).unwrap();
    assert!(result_text(&new).starts_with("2.0.0:"));
    assert_ne!(
        result_text(&old).split(':').nth(1),
        result_text(&new).split(':').nth(1)
    );
    assert_eq!(frontend.0.id(), pid);
    let modern = json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{
        "name":"acme_echo","arguments":{},"requestState":"acme-retained-state","_meta":{
            canonic_mcp_kit::modern::VERSION_KEY:canonic_mcp_kit::modern::PROTOCOL_VERSION,
            canonic_mcp_kit::modern::CAPABILITIES_KEY:{},
            canonic_mcp_kit::modern::CLIENT_KEY:{"name":"acme-native-host","version":"1"},
            "acme.example/opaque":{"value":7},
        }
    }});
    writeln!(input, "{modern}").unwrap();
    input.flush().unwrap();
    let response = received.recv_timeout(WAIT).unwrap();
    assert_eq!(response["id"], 2);
    let details = &response["result"]["structuredContent"];
    assert_eq!(details["id"], 2);
    assert_eq!(details["client"]["name"], "acme-native-host");
    assert_eq!(details["state"], "acme-retained-state");
    assert_eq!(details["metadata"]["acme.example/opaque"]["value"], 7);
    assert_eq!(frontend.0.id(), pid);
    drop(input);
    assert!(frontend.0.wait().unwrap().success());
    reader.join().unwrap();
}
