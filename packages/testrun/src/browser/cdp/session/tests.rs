//! In-memory native lifecycle fixtures with real journals and ordered CDP events.

use super::*;
use crate::browser::contract::{Engine, Mode};
use futures_util::{SinkExt, StreamExt};
use tokio::io::DuplexStream;
use tokio_tungstenite::{WebSocketStream, tungstenite::Message};

mod browser_features;
mod client_certificates;
mod clock;
mod collectors;
mod context_proxy;
mod credentials;
mod diagnostics;
mod emulation;
mod evaluation_adoption;
mod execution_concurrency;
mod frame_element;
mod frame_relationships;
mod framework_registry;
mod header_defaults;
mod http_concurrency;
mod http_credentials;
mod http_request_inputs;
mod http_storage_state;
mod http_user_agent;
mod input_focus;
mod interactive_debugging;
mod investigation;
mod native_tracing;
mod navigation;
mod owned_execution;
mod performance_lifecycle;
#[cfg(all(feature = "acceptance-browser-public", target_os = "macos"))]
mod popup_world;
mod retained_allocation;
mod route_limits;
mod selector_evaluation;
mod service_worker_routes;

pub(crate) async fn lost_with_fixture(
    fail_first_close: bool,
) -> (
    Uuid,
    Session,
    tokio::sync::oneshot::Receiver<tokio::sync::oneshot::Sender<()>>,
    tokio::task::JoinHandle<()>,
) {
    let (mut worker, peer, root) = fixture().await;
    let (mut session, mut ordinary, _executions, _responses, _controls) =
        execution_concurrency::client(&worker);
    let (sender, mut cleanup) = mpsc::channel(1);
    Arc::get_mut(&mut session.0).unwrap().cleanup = sender;
    worker
        .fixture_register(
            serde_json::from_value(json!({
                "name":"acme.txt","mimeType":"text/plain","dataBase64":"QWNtZQ==",
                "provenance":"synthetic_generated","cleared":{"by":"Acme","method":"Generated"}
            }))
            .unwrap(),
        )
        .await
        .unwrap();
    lose_to_journal_failure(&mut worker, &session, &mut ordinary, root.path()).await;
    let connection = worker.shared.state.lock().unwrap().connection;
    let blocked = root.path().join("blocked-download-cleanup");
    if fail_first_close {
        std::fs::write(&blocked, []).unwrap();
        let context = Reference {
            connection_id: connection,
            id: Uuid::new_v4(),
            generation: Uuid::new_v4(),
        };
        worker
            .transfers
            .download_contexts
            .insert(context, blocked.clone());
    }
    let (started, admitted) = tokio::sync::oneshot::channel();
    let cleanup = tokio::spawn(async move {
        if fail_first_close {
            worker.perform(cleanup.recv().await.unwrap()).await;
            std::fs::remove_file(blocked).unwrap();
        }
        let command = cleanup.recv().await.unwrap();
        let (proceed, ready) = tokio::sync::oneshot::channel();
        let _ = started.send(proceed);
        let _ = ready.await;
        worker.perform(command).await;
        assert!(worker.connection.is_none());
        drop((peer, root));
    });
    (connection, session, admitted, cleanup)
}

async fn lose_to_journal_failure(
    worker: &mut Worker,
    session: &Session,
    ordinary: &mut mpsc::Receiver<Command>,
    root: &Path,
) {
    std::fs::rename(worker.journal.directory(), root.join("retained-journal")).unwrap();
    let read = session
        .submit(Uuid::new_v4(), Request::PageList {}, 5000)
        .unwrap();
    worker.perform(ordinary.try_recv().unwrap()).await;
    assert_eq!(
        session.operation(read).unwrap().code.as_deref(),
        Some("STATE_IO")
    );
    assert_eq!(session.status()["phase"], "lost");
}

#[test]
fn native_capture_refusals_preserve_typed_errors_without_echoing_page_messages() {
    for code in [
        "REDACTION_UNAVAILABLE",
        "CAPTURE_BOUNDS",
        "ELEMENT_NOT_VISIBLE",
    ] {
        assert_eq!(page::execution_error(&json!({"exceptionDetails":{"exception":{"description":format!("Error: canonic:{code}\nprivate page stack")}}})).code,code);
    }
    assert_eq!(
        page::execution_error(
            &json!({"exceptionDetails":{"exception":{"description":"private page message"}}})
        )
        .code,
        "PAGE_EXECUTION_FAILED"
    );
    assert_eq!(
        page::query_result(json!({"error":"CAPTURE_BUSY"}))
            .unwrap_err()
            .code,
        "CAPTURE_BUSY"
    );
}

#[tokio::test]
async fn native_media_and_control_status_conforms_to_the_published_schema() {
    let (mut worker, _peer, _root) = fixture().await;
    worker.publish();
    let mut status = worker.shared.state.lock().unwrap().snapshot();
    status["resources"] = worker.shared.resources.lock().unwrap().clone();
    status.as_object_mut().unwrap().extend(
        worker
            .shared
            .details
            .lock()
            .unwrap()
            .as_object()
            .unwrap()
            .clone(),
    );
    crate::schema::validate(&contract::schemas()["status"], &status).unwrap();
    worker.close(deadline()).await.unwrap();
}

#[tokio::test]
async fn interrupted_native_key_write_releases_without_replaying_the_down_event() {
    let (mut worker, mut peer, _root) = fixture().await;
    let id = Uuid::new_v4();
    worker
        .shared
        .state
        .lock()
        .unwrap()
        .begin(id, "fixture".into())
        .unwrap();
    let setup = tokio::spawn(async move {
        let context = worker.context_create(id, deadline()).await.unwrap();
        let page = worker.page_create(id, context, deadline()).await.unwrap();
        (worker, page)
    });
    serve_page_allocation(&mut peer).await;
    let (mut worker, page) = setup.await.unwrap();
    let shared = worker.shared.clone();
    let write = tokio::spawn(async move {
        let result = worker.press_key(id, page, "a", deadline()).await;
        (worker, result)
    });
    let down = command(&mut peer, "Input.dispatchKeyEvent").await;
    assert_eq!(down["params"]["type"], "keyDown");
    shared
        .state
        .lock()
        .unwrap()
        .operations
        .get_mut(&id)
        .unwrap()
        .cancel_requested = true;
    shared.changed.notify_waiters();
    let up = command(&mut peer, "Input.dispatchKeyEvent").await;
    assert_eq!(up["params"]["type"], "keyUp");
    reply(&mut peer, &down, json!({})).await;
    reply(&mut peer, &up, json!({})).await;
    let (mut worker, result) = write.await.unwrap();
    assert_eq!(result.unwrap_err().code, "OUTCOME_UNKNOWN");
    assert!(
        tokio::time::timeout(Duration::from_millis(50), peer.next())
            .await
            .is_err()
    );
    worker.close(deadline()).await.unwrap();
}

type Peer = WebSocketStream<DuplexStream>;

#[tokio::test]
async fn media_fixture_stalled_reader_cannot_spawn_after_timeout_or_cancellation() {
    use std::{io::Read, sync::mpsc as blocking};

    struct StalledRead {
        started: Option<tokio::sync::oneshot::Sender<()>>,
        release: blocking::Receiver<()>,
    }
    impl Read for StalledRead {
        fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
            let _ = self.started.take().unwrap().send(());
            let _ = self.release.recv();
            Ok(0)
        }
    }

    for cause in ["deadline", "cancel", "disconnect"] {
        let (mut worker, _peer, _root) = fixture().await;
        worker.connection = None;
        worker.events = None;
        let startup = Uuid::new_v4();
        {
            let mut state = worker.shared.state.lock().unwrap();
            state.phase = Phase::Starting;
            state.begin(startup, "launch".into()).unwrap();
        }
        let profile = Process::profile_path(worker.journal.directory());
        crate::artifacts::directory(&profile).unwrap();
        let (started, reading) = tokio::sync::oneshot::channel();
        let (release, blocked) = blocking::channel();
        let copy = profile.join("microphone.wav");
        worker.fixture_stage = Some(media_fixtures::Pending::run(move |_| {
            let mut output = std::fs::File::create(copy).unwrap();
            std::io::copy(
                &mut StalledRead {
                    started: Some(started),
                    release: blocked,
                },
                &mut output,
            )
            .unwrap();
            Ok(media_fixtures::Staged::default())
        }));
        tokio::time::timeout_at(deadline(), reading)
            .await
            .unwrap()
            .unwrap();

        let shared = worker.shared.clone();
        let expires = if cause == "deadline" {
            Instant::now()
        } else {
            deadline()
        };
        let (result, ()) = tokio::join!(
            worker.launch_after_staging::<()>(&profile, startup, expires, |_| panic!(
                "Unexpected launch"
            )),
            async {
                if cause != "deadline" {
                    tokio::task::yield_now().await;
                    let mut state = shared.state.lock().unwrap();
                    if cause == "cancel" {
                        state.operations.get_mut(&startup).unwrap().cancel_requested = true;
                    } else {
                        state.phase = Phase::Closing;
                    }
                    shared.changed.notify_waiters();
                }
            }
        );
        let code = if cause == "deadline" {
            "MEDIA_FIXTURE_STAGE_TIMEOUT"
        } else {
            "CANCELLED"
        };
        assert_eq!(result.unwrap_err().code, code, "{cause}");
        assert!(!profile.exists());
        assert_eq!(
            worker.close(Instant::now()).await.unwrap_err().code,
            "CLEANUP_PENDING"
        );
        assert!(worker.fixture_stage.is_some());
        release.send(()).unwrap();
        worker.close(deadline()).await.unwrap();
        assert!(worker.fixture_stage.is_none());
        assert!(!profile.exists());
    }
}

#[tokio::test]
async fn media_fixture_pre_spawn_errors_remove_the_private_profile_before_reporting() {
    for invalid_camera in [true, false] {
        let (mut worker, _peer, root) = fixture().await;
        let profile = Process::profile_path(worker.journal.directory());
        crate::artifacts::directory(&profile).unwrap();
        let wav = root.path().join("fixture.wav");
        let camera = root.path().join("fixture.y4m");
        std::fs::write(&wav, b"RIFF\0\0\0\0WAVE").unwrap();
        std::fs::write(&camera, b"Not a Y4M frame").unwrap();
        worker.fixture_stage = media_fixtures::Pending::start(
            &profile,
            contract::MediaFixtures {
                microphone_path: Some(wav.to_str().unwrap().to_owned()),
                camera_path: invalid_camera.then(|| camera.to_str().unwrap().to_owned()),
            },
        );
        let result = worker
            .launch_after_staging::<()>(&profile, Uuid::new_v4(), deadline(), |_| {
                assert!(!invalid_camera);
                assert!(profile.join("microphone.wav").exists());
                Err("BROWSER_LAUNCH_FAILED")
            })
            .await;
        assert_eq!(
            result.unwrap_err().code,
            if invalid_camera {
                "MEDIA_FIXTURE_FORMAT"
            } else {
                "BROWSER_LAUNCH_FAILED"
            }
        );
        assert!(worker.fixture_stage.is_none());
        assert!(!profile.exists());
        assert!(wav.exists());
    }
}

fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(5)
}

#[expect(
    clippy::too_many_lines,
    reason = "The shared CDP fixture initializes one complete worker and an in-memory protocol peer"
)]
pub(super) async fn fixture() -> (Worker, Peer, tempfile::TempDir) {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("private");
    crate::artifacts::directory(&directory).unwrap();
    let connection = Uuid::new_v4();
    let options = LaunchOptions {
        media_fixtures: Default::default(),
        proxy: None,
        engine: Engine::Chrome,
        mode: Mode::Headless,
        context: ContextOptions::default(),
    };
    let mut state = State::connection(connection, Uuid::new_v4(), Uuid::new_v4(), &options);
    state.phase = Phase::Ready;
    let shared = Arc::new(Shared {
        state: Mutex::new(state),
        resources: Mutex::new(json!([])),
        details: Mutex::new(json!({})),
        control: Default::default(),
        changed: Notify::new(),
        #[cfg(all(test, feature = "acceptance-browser-lifecycle"))]
        native_pid: Default::default(),
    });
    let (client, peer) = tokio::io::duplex(64 * 1024);
    let (connection_result, peer_result) = tokio::join!(
        Connection::open(
            client,
            "ws://127.0.0.1:12345/devtools/browser/acme",
            deadline()
        ),
        tokio_tungstenite::accept_async(peer)
    );
    let (transport, events) = connection_result.unwrap();
    (
        Worker {
            shared,
            process: None,
            fixture_stage: None,
            profile: None,
            persistent_context: None,
            adoption: None,
            attached: false,
            connection: Some(transport),
            events: Some(events),
            index: Index::new(connection),
            journal: Journal::create(&directory, connection).unwrap(),
            lease: None,
            configured: BTreeSet::new(),
            startup: Default::default(),
            closed: BTreeMap::new(),
            navigation: Default::default(),
            diagnostics: Default::default(),
            argument_groups: Default::default(),
            evaluation: Default::default(),
            network_evidence: Default::default(),
            har_archives: Default::default(),
            routing_rules: Default::default(),
            fetch: Default::default(),
            har_servicing: false,
            http_contexts: Default::default(),
            http_requests: Default::default(),
            http_state: Default::default(),
            snapshots: Default::default(),
            authenticators: Default::default(),
            request_inputs: Default::default(),
            developer_tools: Default::default(),
            webmcp_debug: Default::default(),
            managed_apps: false,
            extension_profile: false,
            app_tab_probe: None,
            extensions: Default::default(),
            pwas: Default::default(),
            frameworks: Default::default(),
            heap: Default::default(),
            performance: Default::default(),
            websockets: Default::default(),
            developers: Default::default(),
            profiling: Default::default(),
            trace: None,
            trace_last: None,
            trace_tabs: Default::default(),
            debuggers: Default::default(),
            interactions: Default::default(),
            transfers: Default::default(),
            responses: None,
            executions: None,
            deferred_executions: VecDeque::new(),
            waits: BTreeMap::new(),
            condition_probes: BTreeMap::new(),
            inputs: Default::default(),
            pointer_positions: BTreeMap::new(),
            touch_contexts: BTreeSet::new(),
            context_layout: BTreeMap::new(),
            certificate_tunnels: Default::default(),
            page_headers: BTreeMap::new(),
            emulation: Default::default(),
            capture_store: Default::default(),
            media: Default::default(),
            control_commands: None,
        },
        peer_result.unwrap(),
        root,
    )
}

async fn command(peer: &mut Peer, method: &str) -> Value {
    let message = tokio::time::timeout_at(deadline(), peer.next())
        .await
        .expect(method)
        .unwrap_or_else(|| panic!("CDP peer closed while waiting for {method}"))
        .unwrap_or_else(|error| panic!("CDP transport failed while waiting for {method}: {error}"));
    let Message::Text(message) = message else {
        panic!("Expected CDP command");
    };
    let value: Value = serde_json::from_str(&message).unwrap();
    assert_eq!(value["method"], method);
    value
}
async fn send(peer: &mut Peer, value: Value) {
    peer.send(Message::Text(value.to_string().into()))
        .await
        .unwrap();
}
async fn reply(peer: &mut Peer, command: &Value, value: Value) {
    let mut result = json!({"id":command["id"],"result":value});
    if let Some(session) = command.get("sessionId") {
        result["sessionId"] = session.clone();
    }
    send(peer, result).await;
}

#[tokio::test]
async fn allocation_events_are_indexed_before_references_and_foreign_scopes_never_dispatch() {
    let (mut worker, mut peer, _root) = fixture().await;
    let id = Uuid::new_v4();
    worker
        .shared
        .state
        .lock()
        .unwrap()
        .begin(id, "fixture".into())
        .unwrap();
    let workflow = tokio::spawn(async move {
        let context = worker.context_create(id, deadline()).await.unwrap();
        let page = worker.page_create(id, context, deadline()).await.unwrap();
        (worker, context, page)
    });
    serve_page_allocation(&mut peer).await;
    let (mut worker, context, page) = workflow.await.unwrap();
    assert_eq!(worker.index.list(Kind::World).len(), 1);
    assert_eq!(
        worker
            .index
            .route(page, Some(Kind::Page))
            .unwrap()
            .session_id,
        "session-acme"
    );
    let mut foreign = context;
    foreign.connection_id = Uuid::new_v4();
    assert_eq!(
        worker
            .execute_one(id, Request::ContextClose { context: foreign }, deadline())
            .await
            .unwrap_err()
            .code,
        "FOREIGN_REFERENCE"
    );
    let status = worker.shared.resources.lock().unwrap().to_string();
    assert!(!status.contains("session-acme"));
    assert!(!status.contains("page-acme"));
    let mut status = worker.shared.state.lock().unwrap().snapshot();
    status["resources"] = worker.shared.resources.lock().unwrap().clone();
    crate::schema::validate(&crate::browser::contract::schemas()["status"], &status).unwrap();
    {
        let mut state = worker.shared.state.lock().unwrap();
        state.selected_context = Some(context);
        state.selected_page = Some(page);
    }
    let workflow = tokio::spawn(async move {
        worker
            .execute_one(id, Request::ContextClose { context }, deadline())
            .await
            .unwrap();
        worker.publish();
        // A completed native close is an idempotent witness. A second close
        // must not dispatch another mutation or pick a surviving page.
        worker
            .execute_one(id, Request::ContextClose { context }, deadline())
            .await
            .unwrap();
        worker
            .execute_one(id, Request::PageClose { page }, deadline())
            .await
            .unwrap();
        worker
    });
    let close = command(&mut peer, "Target.disposeBrowserContext").await;
    send(
        &mut peer,
        json!({"method":"Target.targetDestroyed","params":{"targetId":"page-acme"}}),
    )
    .await;
    reply(&mut peer, &close, json!({})).await;
    let mut worker = workflow.await.unwrap();
    assert!(worker.shared.state.lock().unwrap().selected_page.is_none());
    assert!(worker.index.list(Kind::Context).is_empty());
    worker.close(deadline()).await.unwrap();
}

async fn serve_page_allocation(peer: &mut Peer) {
    let create = command(peer, "Target.createBrowserContext").await;
    reply(peer, &create, json!({"browserContextId":"context-acme"})).await;
    let downloads = command(peer, "Browser.setDownloadBehavior").await;
    assert_eq!(
        downloads["params"],
        json!({"behavior":"deny","browserContextId":"context-acme"})
    );
    reply(peer, &downloads, json!({})).await;
    let create = command(peer, "Target.createTarget").await;
    let info = json!({"targetId":"page-acme","type":"page","title":"Acme","url":"about:blank","attached":false,"canAccessOpener":false,"browserContextId":"context-acme"});
    send(
        peer,
        json!({"method":"Target.targetCreated","params":{"targetInfo":info}}),
    )
    .await;
    reply(peer, &create, json!({"targetId":"page-acme"})).await;
    let get = command(peer, "Target.getTargetInfo").await;
    reply(peer, &get, json!({"targetInfo":info})).await;
    serve_page_setup(
        peer,
        &info,
        "session-acme",
        "frame-acme",
        "world-acme",
        false,
    )
    .await;
}

async fn serve_page_setup(
    peer: &mut Peer,
    info: &Value,
    session: &str,
    frame: &str,
    world: &str,
    context_emulation: bool,
) {
    let attach = command(peer, "Target.attachToTarget").await;
    assert_eq!(attach["params"]["targetId"], info["targetId"]);
    reply(peer, &attach, json!({"sessionId":session})).await;
    let enable = command(peer, "Page.enable").await;
    assert_eq!(enable["sessionId"], session);
    send(peer, json!({"method":"Target.attachedToTarget","params":{"sessionId":session,"targetInfo":info,"waitingForDebugger":false}})).await;
    reply(peer, &enable, json!({})).await;
    let tree = command(peer, "Page.getFrameTree").await;
    reply(peer, &tree, json!({"frameTree":{"frame":{"id":frame,"loaderId":"loader-acme","url":info["url"],"domainAndRegistry":"","securityOrigin":"://","mimeType":"text/html","secureContextType":"Secure","crossOriginIsolatedContextType":"NotIsolated","gatedAPIFeatures":[]}}})).await;
    let enable = command(peer, "Runtime.enable").await;
    send(peer, json!({"method":"Runtime.executionContextCreated","sessionId":session,"params":{"context":{"id":1,"origin":"://","name":"","uniqueId":world,"auxData":{"frameId":frame,"isDefault":true}}}})).await;
    reply(peer, &enable, json!({})).await;
    let geometry = command(peer, "Emulation.setDeviceMetricsOverride").await;
    assert_eq!(geometry["params"]["width"], 1280);
    reply(peer, &geometry, json!({})).await;
    if context_emulation {
        let focus = command(peer, "Emulation.setFocusEmulationEnabled").await;
        reply(peer, &focus, json!({})).await;
        let media = command(peer, "Emulation.setEmulatedMedia").await;
        reply(peer, &media, json!({})).await;
    }
    let attachment = command(peer, "Target.setAutoAttach").await;
    assert_eq!(attachment["params"]["waitForDebuggerOnStart"], false);
    reply(peer, &attachment, json!({})).await;
    let initialization = command(peer, "Page.addScriptToEvaluateOnNewDocument").await;
    assert_eq!(initialization["params"]["worldName"], page::WORLD);
    assert_eq!(initialization["params"]["runImmediately"], true);
    reply(peer, &initialization, json!({"identifier":"acme-script"})).await;
    let chooser = command(peer, "Page.setInterceptFileChooserDialog").await;
    assert_eq!(chooser["params"]["enabled"], true);
    reply(peer, &chooser, json!({})).await;
}

#[tokio::test]
async fn attachment_adoption_indexes_only_the_selected_context_and_preserves_it() {
    let (mut worker, mut peer, _root) = fixture().await;
    let id = Uuid::new_v4();
    let operation = tokio::spawn(async move {
        let (context, page) = worker
            .adopt_isolated_context(id, "isolated-acme", deadline())
            .await
            .unwrap();
        assert!(page.is_none());
        assert_eq!(worker.index.list(Kind::Context).len(), 1);
        assert!(worker.index.list(Kind::Page).is_empty());
        let error = worker
            .execute_one(id, Request::ContextClose { context }, deadline())
            .await
            .unwrap_err();
        assert_eq!(error.code, "ATTACHMENT_BORROWED_CONTEXT");
        let error = worker
            .page_create(id, context, deadline())
            .await
            .unwrap_err();
        assert_eq!(
            error.code,
            "ATTACHMENT_ADOPTED_CONTEXT_READ_ONLY_ALLOCATION"
        );
        worker.close(deadline()).await.unwrap();
    });
    let contexts = command(&mut peer, "Target.getBrowserContexts").await;
    reply(
        &mut peer,
        &contexts,
        json!({"browserContextIds":["isolated-acme","neighbor"]}),
    )
    .await;
    let targets = command(&mut peer, "Target.getTargets").await;
    reply(&mut peer, &targets, json!({"targetInfos":[
        {"targetId":"foreign","type":"page","title":"Foreign","url":"about:blank","attached":false,"canAccessOpener":false,"browserContextId":"neighbor"}
    ]})).await;
    let message = tokio::time::timeout_at(deadline(), peer.next())
        .await
        .unwrap();
    assert!(
        !matches!(message, Some(Ok(Message::Text(_)))),
        "{message:?}"
    );
    operation.await.unwrap();
}

#[tokio::test]
async fn cancelled_allocation_is_never_replayed_and_late_reply_remains_owned() {
    let (mut worker, mut peer, _root) = fixture().await;
    let id = Uuid::new_v4();
    let shared = worker.shared.clone();
    shared
        .state
        .lock()
        .unwrap()
        .begin(id, "fixture".into())
        .unwrap();
    let workflow = tokio::spawn(async move {
        worker
            .perform(Command {
                queue_slot: None,
                id,
                request: Some(Request::ContextCreate {
                    options: ContextOptions::default(),
                }),
                deadline: deadline(),
            })
            .await;
        worker
    });
    let create = command(&mut peer, "Target.createBrowserContext").await;
    shared
        .state
        .lock()
        .unwrap()
        .operations
        .get_mut(&id)
        .unwrap()
        .cancel_requested = true;
    shared.changed.notify_waiters();
    let mut worker = workflow.await.unwrap();
    {
        let state = shared.state.lock().unwrap();
        assert_eq!(state.phase, Phase::Lost);
        assert_eq!(state.operations[&id].outcome, Outcome::Unknown);
    }
    reply(
        &mut peer,
        &create,
        json!({"browserContextId":"late-context"}),
    )
    .await;
    let transport = worker.connection.as_ref().unwrap();
    let late = tokio::time::timeout_at(deadline(), async {
        loop {
            if let Some(reply) = transport.take_late_reply() {
                break reply;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(late.method, "Target.createBrowserContext");
    assert_eq!(late.result.unwrap()["browserContextId"], "late-context");
    worker.close(deadline()).await.unwrap();
}

#[tokio::test]
async fn queued_cancellation_and_expired_deadlines_do_not_reach_the_wire() {
    let (mut worker, _peer, _root) = fixture().await;
    for (cancel, code) in [(true, "CANCELLED"), (false, "NOT_DISPATCHED")] {
        let id = Uuid::new_v4();
        {
            let mut state = worker.shared.state.lock().unwrap();
            state.begin(id, "fixture".into()).unwrap();
            state.operations.get_mut(&id).unwrap().cancel_requested = cancel;
        }
        worker
            .perform(Command {
                queue_slot: None,
                id,
                request: Some(Request::ContextCreate {
                    options: ContextOptions::default(),
                }),
                deadline: if cancel { deadline() } else { Instant::now() },
            })
            .await;
        assert_eq!(
            worker.shared.state.lock().unwrap().operations[&id]
                .code
                .as_deref(),
            Some(code)
        );
        assert_eq!(worker.connection.as_ref().unwrap().status().pending, 0);
    }
    worker.close(deadline()).await.unwrap();
}

#[tokio::test]
async fn batch_cancellation_stops_local_selection_before_the_next_step() {
    let (mut worker, _peer, _root) = fixture().await;
    let context = worker.index.register_context("owned-context").unwrap();
    let id = Uuid::new_v4();
    {
        let mut state = worker.shared.state.lock().unwrap();
        state.begin(id, "fixture".into()).unwrap();
        state.operations.get_mut(&id).unwrap().cancel_requested = true;
    }
    let error = worker
        .execute(
            id,
            Request::Batch {
                requests: vec![Request::ContextSelect { context }],
            },
            deadline(),
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, "CANCELLED");
    assert_eq!(error.partial.unwrap()["completedActions"], 0);
    assert!(
        worker
            .shared
            .state
            .lock()
            .unwrap()
            .selected_context
            .is_none()
    );
    assert_eq!(worker.connection.as_ref().unwrap().status().pending, 0);
    worker.close(deadline()).await.unwrap();
}

#[test]
fn unsupported_context_options_and_mixed_batches_refuse_before_admission() {
    let bundle = ChromeBundle {
        executable: std::path::PathBuf::from("/nonexistent/acme-chrome"),
        installed: None,
    };
    let options = LaunchOptions {
        media_fixtures: Default::default(),
        proxy: None,
        engine: Engine::Firefox,
        mode: Mode::Headless,
        context: ContextOptions::default(),
    };
    let refusal = Session::launch_connection(
        bundle,
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        options,
        Path::new("/nonexistent/acme-private-state"),
    )
    .err()
    .expect("Backend mismatch must refuse before host or filesystem access");
    assert_eq!(refusal.code, "BACKEND_UNAVAILABLE");
    let options = ContextOptions {
        locale: Some("en-US".into()),
        permissions: Some(vec![crate::browser::settings::Permission::Push]),
        ..ContextOptions::default()
    };
    assert_eq!(
        chrome_options(&options).unwrap_err().code,
        "PERMISSION_UNSUPPORTED"
    );
    assert!(
        supported(&Request::Batch {
            requests: vec![Request::ContextList {}, Request::PageList {}]
        })
        .is_ok()
    );
    assert!(
        supported(&Request::Batch {
            requests: vec![Request::ContextList {}, Request::InputRelease {}]
        })
        .is_err()
    );
    let reference = Reference {
        connection_id: Uuid::new_v4(),
        id: Uuid::new_v4(),
        generation: Uuid::new_v4(),
    };
    let scoped = Request::PageWait {
        page: reference,
        frame: Some(reference),
        locator: None,
        condition: crate::browser::page::Condition::Visible {},
    };
    assert!(supported(&scoped).is_ok());
    assert_eq!(
        supported(&Request::ContextDebugger {
            context: reference,
            action: crate::browser::automation_debugger::Action::Status {
                include_details: false
            }
        })
        .unwrap_err()
        .code,
        "AUTOMATION_DEBUGGER_UNSUPPORTED"
    );
    assert_eq!(
        supported(&Request::PageAction {
            page: reference,
            frame: None,
            locator: crate::browser::page::Locator::Css {
                selector: "#Acme".into()
            },
            action: crate::browser::page::Action::AtSource {
                source: crate::browser::automation_debugger::Location {
                    file: "Acme-journey.rs".into(),
                    line: Some(30),
                    column: None
                },
                action: Box::new(crate::browser::page::Action::Click {}),
            },
            force: false,
            postcondition: None,
        })
        .unwrap_err()
        .code,
        "CALL_SOURCE_UNSUPPORTED"
    );
}
