//! Real target topology and renderer faults inside the fixture's private Chrome.
use super::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[tokio::test]
async fn native_locator_actions_use_snapshot_identity_and_trusted_input() {
    use crate::browser::page::{Action, Load, Locator, Navigation, Read};
    std::env::var_os("TESTRUN_BROWSER_ROOT").expect("Select a private browser cache");
    for mode in [Mode::Headless, Mode::Headed] {
        let (mut worker, _root, id) = fixture(mode).await;
        let exercise=std::panic::AssertUnwindSafe(async {
            let page=worker.shared.state.lock().unwrap().selected_page.unwrap();
            let html="<label>Name<input id=name></label><label><input id=check type=checkbox>Agree</label><select id=select><option value=a>A</option><option value=b>B</option></select><button id=save onclick='document.querySelector(\"#result\").textContent=event.isTrusted?\"Saved\":\"Untrusted\"'>Save</button><p id=result></p><script>globalThis.keys=[];document.addEventListener('keydown',e=>keys.push(e.key));</script>";
            worker.execute_one(id,Request::PageNavigate{page,navigation:Navigation::SetContent{html:html.into(),wait_until:Load::Load}},Instant::now()+Duration::from_secs(20)).await.unwrap();
            let snapshot=worker.execute_one(id,Request::PageRead{page,frame:None,locator:Some(Locator::Css{selector:"#save".into()}),read:Read::Snapshot,max_bytes:8192},Instant::now()+Duration::from_secs(10)).await.unwrap();
            let button:Locator=serde_json::from_value(snapshot["result"]["value"][0]["locator"].clone()).unwrap();
            let action=|selector:&str,action|Request::PageAction{page,frame:None,locator:Locator::Css{selector:selector.into()},action,force:false,postcondition:None};
            for request in [action("#name",Action::Fill{value:"Acme".into()}),action("#name",Action::Press{key:"End".into()}),action("#name",Action::Type{text:" QA🙂".into()}),action("#check",Action::Check{checked:true}),action("#select",Action::Select{values:vec!["b".into()]}),Request::PageAction{page,frame:None,locator:button.clone(),action:Action::Click{},force:false,postcondition:None}] {
                let requested=format!("{request:?}");
                worker.execute_one(id,request,Instant::now()+Duration::from_secs(10)).await.unwrap_or_else(|error|panic!("{requested}: {error:?}"));
            }
            let form=worker.execute_one(id,Request::PageRead{page,frame:None,locator:None,read:Read::Form,max_bytes:8192},Instant::now()+Duration::from_secs(10)).await.unwrap();
            assert_eq!(form["result"]["value"][0]["value"],"Acme QA🙂");
            assert_eq!(form["result"]["value"][1]["checked"],true);
            assert_eq!(form["result"]["value"][2]["value"],"b");
            let saved=worker.execute_one(id,Request::PageRead{page,frame:None,locator:Some(Locator::Css{selector:"#result".into()}),read:Read::Text,max_bytes:8192},Instant::now()+Duration::from_secs(10)).await.unwrap();
            assert_eq!(saved["result"]["value"],"Saved");
            worker.execute_one(id,Request::PageNavigate{page,navigation:Navigation::SetContent{html:html.into(),wait_until:Load::Load}},Instant::now()+Duration::from_secs(20)).await.unwrap();
            let stale=worker.execute_one(id,Request::PageAction{page,frame:None,locator:button,action:Action::Click{},force:false,postcondition:None},Instant::now()+Duration::from_secs(10)).await.unwrap_err();
            assert_eq!(stale.code,"STALE_REFERENCE");
        }).catch_unwind().await;
        worker
            .close(Instant::now() + Duration::from_secs(30))
            .await
            .unwrap();
        if let Err(panic) = exercise {
            std::panic::resume_unwind(panic);
        }
    }
}

pub(super) struct Site {
    pub(super) port: u16,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Site {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Site {
    pub(super) async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let port = listener.local_addr().unwrap().port();
        let task = tokio::spawn(async move {
            let mut requests = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    accepted = listener.accept() => {
                        let (stream, _) = accepted.unwrap();
                        requests.spawn(respond(stream, port));
                    }
                    _ = requests.join_next(), if !requests.is_empty() => {}
                }
            }
        });
        Self { port, task }
    }
}

async fn respond(mut stream: tokio::net::TcpStream, port: u16) {
    let mut request = Vec::new();
    let mut chunk = [0; 1024];
    while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
        let count = stream.read(&mut chunk).await.unwrap();
        if count == 0 || request.len() + count > 8192 {
            break;
        }
        request.extend_from_slice(&chunk[..count]);
    }
    let worker = request.starts_with(b"GET /worker.js ");
    let body = if worker {
        "self.onmessage = () => {};".to_owned()
    } else if request.starts_with(b"GET /main ") {
        format!(
            "<!doctype html><title>Acme</title><iframe id='child' src='http://localhost:{port}/frame'></iframe><script>globalThis.ownedWorker = new Worker('/worker.js');</script>"
        )
    } else {
        "<!doctype html><title>Acme child</title>Owned fixture".into()
    };
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        if worker {
            "application/javascript"
        } else {
            "text/html"
        },
        body.len()
    );
    let _ = stream.write_all(response.as_bytes()).await;
}

async fn fixture(mode: Mode) -> (Worker, tempfile::TempDir, Uuid) {
    let bundle = crate::browser::preparation::prepare_chrome(|_| {})
        .await
        .unwrap();
    let root = tempfile::tempdir().unwrap();
    let private = root.path().join("state");
    crate::artifacts::directory(&private).unwrap();
    let connection = Uuid::new_v4();
    let owner = Uuid::new_v4();
    let generation = Uuid::new_v4();
    let options = LaunchOptions {
        media_fixtures: Default::default(),
        proxy: None,
        engine: Engine::Chrome,
        mode,
        context: ContextOptions::default(),
    };
    let mut state = State::connection(connection, owner, generation, &options);
    let id = Uuid::new_v4();
    state.begin(id, "fixture-launch".into()).unwrap();
    let mut worker = Worker {
        shared: Arc::new(Shared {
            state: Mutex::new(state),
            resources: Mutex::new(json!([])),
            details: Mutex::new(json!({})),
            control: Default::default(),
            changed: Notify::new(),
            native_pid: Default::default(),
        }),
        process: None,
        fixture_stage: None,
        connection: None,
        events: None,
        index: Index::new(connection),
        journal: Journal::create(&private, connection).unwrap(),
        lease: Some(
            crate::lease::ControlLease::acquire(&format!(
                "browser-session:{owner}:{generation}:{connection}"
            ))
            .unwrap(),
        ),
        profile: None,
        persistent_context: None,
        adoption: None,
        attached: false,
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
        waits: BTreeMap::new(),
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
    };
    let result = worker
        .start(
            bundle,
            options,
            id,
            Instant::now() + Duration::from_secs(30),
        )
        .await;
    if let Err(error) = result {
        worker
            .close(Instant::now() + Duration::from_secs(30))
            .await
            .unwrap();
        panic!("Chrome fixture startup: {error:?}");
    }
    (worker, root, id)
}

async fn until(worker: &mut Worker, id: Uuid, predicate: impl Fn(&Worker) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        worker.drain().unwrap();
        worker
            .reconcile(id, deadline)
            .await
            .unwrap_or_else(|error| {
                panic!(
                    "Target reconciliation: {error:?}; {}",
                    worker.index.snapshot()
                )
            });
        if predicate(worker) {
            return;
        }
        let event = tokio::time::timeout_at(deadline, next_event(&mut worker.events))
            .await
            .unwrap_or_else(|_| panic!("Target condition missing: {}", worker.index.snapshot()))
            .expect("Owned browser transport lost");
        worker.apply(event).unwrap();
    }
}

async fn evaluate(worker: &mut Worker, id: Uuid, page: Reference, expression: String) {
    let session = worker
        .index
        .route(page, Some(Kind::Page))
        .unwrap()
        .session_id;
    let result = worker
        .call(
            id,
            Some(&session),
            "Runtime.evaluate",
            json!({"expression":expression,"userGesture":true,"returnByValue":true}),
            Instant::now() + Duration::from_secs(10),
        )
        .await
        .unwrap();
    assert!(result.get("exceptionDetails").is_none(), "{result}");
}

struct Topology {
    main: Reference,
    context: Reference,
    session: String,
    frame: Reference,
    worlds: Vec<Reference>,
}

async fn topology(worker: &mut Worker, id: Uuid, site: &Site) -> Topology {
    let main = worker.shared.state.lock().unwrap().selected_page.unwrap();
    let context = worker
        .shared
        .state
        .lock()
        .unwrap()
        .selected_context
        .unwrap();
    assert_eq!(
        worker.index.list(Kind::Page).len(),
        1,
        "Default foreign page must stay outside the owned index"
    );
    let session = worker
        .index
        .route(main, Some(Kind::Page))
        .unwrap()
        .session_id;
    worker
        .call(
            id,
            Some(&session),
            "Page.navigate",
            json!({"url":format!("http://127.0.0.1:{}/main",site.port)}),
            Instant::now() + Duration::from_secs(10),
        )
        .await
        .unwrap();
    until(worker, id, |w| {
        w.index
            .targets()
            .iter()
            .any(|(_, k, _, s)| *k == Kind::Frame && s.is_some())
            && w.index
                .list(Kind::Worker)
                .iter()
                .filter(|(_, parent)| *parent == Some(main))
                .count()
                == 1
    })
    .await;
    let frame = worker
        .index
        .targets()
        .into_iter()
        .find(|(_, k, _, s)| *k == Kind::Frame && s.is_some())
        .unwrap()
        .0;
    let frame_worlds = worker
        .index
        .list(Kind::World)
        .into_iter()
        .filter(|(_, parent)| *parent == Some(frame))
        .map(|(r, _)| r)
        .collect::<Vec<_>>();
    assert!(!frame_worlds.is_empty());
    Topology {
        main,
        context,
        session,
        frame,
        worlds: frame_worlds,
    }
}

async fn popups_and_frames(worker: &mut Worker, id: Uuid, site: &Site, topology: &Topology) {
    let main = topology.main;
    let context = topology.context;
    let frame_worlds = &topology.worlds;
    evaluate(
        worker,
        id,
        main,
        "window.open('/popup', 'AcmePopup'); undefined".into(),
    )
    .await;
    until(worker, id, |w| w.index.list(Kind::Page).len() == 2).await;
    let popup = worker
        .index
        .list(Kind::Page)
        .into_iter()
        .find(|(r, _)| *r != main)
        .unwrap()
        .0;
    assert_eq!(
        worker
            .index
            .list(Kind::Page)
            .into_iter()
            .find(|(r, _)| *r == popup)
            .unwrap()
            .1,
        Some(context)
    );
    evaluate(
        worker,
        id,
        main,
        format!(
            "document.querySelector('#child').src = 'http://127.0.0.1:{}/frame'; undefined",
            site.port
        ),
    )
    .await;
    until(worker, id, |w| {
        frame_worlds
            .iter()
            .all(|r| w.index.route(*r, Some(Kind::World)).is_err())
    })
    .await;
    worker
        .execute_one(
            id,
            Request::PageClose { page: popup },
            Instant::now() + Duration::from_secs(10),
        )
        .await
        .unwrap();
    assert!(worker.index.route(popup, Some(Kind::Page)).is_err());
    until(worker, id, |w| {
        w.index.list(Kind::Frame).iter().any(|(r, _)| {
            r.id == topology.frame.id
                && w.index
                    .route(*r, Some(Kind::Frame))
                    .is_ok_and(|route| route.session_id == topology.session)
        })
    })
    .await;
    evaluate(
        worker,
        id,
        main,
        format!(
            "document.querySelector('#child').src = 'http://localhost:{}/frame'; undefined",
            site.port
        ),
    )
    .await;
    until(worker, id, |w| {
        w.index.targets().iter().any(|(r, k, _, s)| {
            r.id == topology.frame.id
                && *k == Kind::Frame
                && s.as_ref()
                    .is_some_and(|session| session != &topology.session)
        })
    })
    .await;
}

async fn exercise(worker: &mut Worker, id: Uuid, site: &Site) {
    let topology = topology(worker, id, site).await;
    popups_and_frames(worker, id, site, &topology).await;
    let Topology {
        main,
        context,
        session,
        ..
    } = topology;
    let sibling = worker
        .context_create(id, Instant::now() + Duration::from_secs(10))
        .await
        .unwrap();
    let survivor = worker
        .page_create(id, sibling, Instant::now() + Duration::from_secs(10))
        .await
        .unwrap();
    // Page.crash deliberately may never reply. Its pending request remains owned
    // by this exclusive browser until fixture disconnect; it is never replayed.
    let _ = worker
        .connection
        .as_ref()
        .unwrap()
        .call(
            Some(&session),
            "Page.crash",
            json!({}),
            Lane::Ordinary,
            Instant::now() + Duration::from_millis(250),
        )
        .await;
    until(worker, id, |w| {
        w.index.route(main, Some(Kind::Page)).is_err()
    })
    .await;
    assert!(worker.index.route(survivor, Some(Kind::Page)).is_ok());
    assert!(worker.index.native(context, Kind::Context).is_ok());
    assert!(worker.shared.state.lock().unwrap().selected_page.is_none());
    evaluate(
        worker,
        id,
        survivor,
        "document.title = 'Acme survivor'; 42".into(),
    )
    .await;
    worker
        .execute_one(
            id,
            Request::PageClose { page: survivor },
            Instant::now() + Duration::from_secs(10),
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn owned_popup_worker_oopif_swap_and_renderer_loss_preserve_siblings() {
    assert!(crate::browser::processes::initial_host());
    std::env::var_os("TESTRUN_BROWSER_ROOT").expect("Select an explicit private browser cache");
    let site = Site::start().await;
    for mode in [Mode::Headless, Mode::Headed] {
        let (mut worker, _root, id) = fixture(mode).await;
        let mut witnesses = crate::browser::processes::Tracker::default();
        witnesses
            .observe(worker.process.as_ref().unwrap().pid().unwrap())
            .unwrap();
        let result = std::panic::AssertUnwindSafe(exercise(&mut worker, id, &site))
            .catch_unwind()
            .await;
        worker
            .close(Instant::now() + Duration::from_secs(30))
            .await
            .unwrap();
        assert!(witnesses.released().unwrap());
        if let Err(panic) = result {
            std::panic::resume_unwind(panic);
        }
    }
}

#[tokio::test]
async fn native_page_reads_preserve_semantic_locators_and_document_fences() {
    use crate::browser::page::{Load, Locator, Navigation, Read};
    assert!(crate::browser::processes::initial_host());
    std::env::var_os("TESTRUN_BROWSER_ROOT").expect("Select an explicit private browser cache");
    for mode in [Mode::Headless, Mode::Headed] {
        let (mut worker, _root, id) = fixture(mode).await;
        let result = std::panic::AssertUnwindSafe(async {
            let page = worker.shared.state.lock().unwrap().selected_page.unwrap();
            worker.execute_one(id, Request::PageNavigate { page, navigation: Navigation::SetContent {
                html:"<h1>Acme checkout</h1><label>Name<input></label><input type=password value=Acme-secret-sentinel><button>Duplicate</button><button>Duplicate</button><iframe id=child srcdoc='<h2>Owned frame</h2>'></iframe><div id=shadow></div><script>document.querySelector('#shadow').attachShadow({mode:'open'}).innerHTML='<button>Shadow action</button>';let toast=document.createElement('div');toast.id='toast';toast.setAttribute('role','alert');toast.textContent='Acme transient';document.body.append(toast);setTimeout(()=>toast.remove(),500);</script>".into(), wait_until:Load::Load,
            }},Instant::now()+Duration::from_secs(20)).await.unwrap();
            let read = |locator, kind| Request::PageRead {page,frame:None,locator,read:kind,max_bytes:8192};
            let heading = worker.execute_one(id,read(Some(Locator::Role {role:"heading".into(),name:Some("Acme checkout".into()),exact:true}),Read::Text),Instant::now()+Duration::from_secs(10)).await.unwrap();
            assert_eq!(heading["result"]["value"],"Acme checkout");
            let old_frame: Reference = serde_json::from_value(heading["frame"].clone()).unwrap();
            let form = worker.execute_one(id,read(None,Read::Form),Instant::now()+Duration::from_secs(10)).await.unwrap();
            assert!(!form.to_string().contains("Acme-secret-sentinel"));
            let shadow = worker.execute_one(id,read(Some(Locator::Role {role:"button".into(),name:Some("Shadow action".into()),exact:true}),Read::Text),Instant::now()+Duration::from_secs(10)).await.unwrap();
            assert_eq!(shadow["result"]["value"],"Shadow action");
            let duplicate = worker.execute_one(id,read(Some(Locator::Role {role:"button".into(),name:Some("Duplicate".into()),exact:true}),Read::Text),Instant::now()+Duration::from_secs(10)).await.unwrap_err();
            assert_eq!(duplicate.code,"AMBIGUOUS_LOCATOR");
            let count = worker.execute_one(id,read(Some(Locator::Css {selector:"button".into()}),Read::Count),Instant::now()+Duration::from_secs(10)).await.unwrap();
            assert_eq!(count["result"]["value"],3);
            read_frames_and_waits(&mut worker,id,page).await;
            worker.execute_one(id,Request::PageNavigate {page,navigation:Navigation::SetContent {html:"<h1>Replacement</h1>".into(),wait_until:Load::Domcontentloaded}},Instant::now()+Duration::from_secs(20)).await.unwrap();
            let stale = worker.execute_one(id,Request::PageRead {page,frame:Some(old_frame),locator:None,read:Read::Text,max_bytes:8192},Instant::now()+Duration::from_secs(10)).await.unwrap_err();
            assert_eq!(stale.code,"STALE_REFERENCE");
            secure_aria(&mut worker,id,page).await;
            navigation_journey(&mut worker,id,page).await;
        }).catch_unwind().await;
        worker
            .close(Instant::now() + Duration::from_secs(30))
            .await
            .unwrap();
        if let Err(panic) = result {
            std::panic::resume_unwind(panic);
        }
    }
}

async fn read_frames_and_waits(worker: &mut Worker, id: Uuid, page: Reference) {
    use crate::browser::page::{Locator, Read};
    let deadline = || Instant::now() + Duration::from_secs(10);
    let child = worker
        .execute_one(
            id,
            Request::FrameEnter {
                page,
                frame: None,
                locator: Locator::Css {
                    selector: "#child".into(),
                },
            },
            deadline(),
        )
        .await
        .unwrap();
    let child = serde_json::from_value(child["frame"].clone()).unwrap();
    let text = worker
        .execute_one(
            id,
            Request::PageRead {
                page,
                frame: Some(child),
                locator: None,
                read: Read::Text,
                max_bytes: 8192,
            },
            deadline(),
        )
        .await
        .unwrap();
    assert_eq!(text["result"]["value"], "Owned frame");
    let ambiguous = worker
        .execute_one(
            id,
            Request::FrameEnter {
                page,
                frame: None,
                locator: Locator::Css {
                    selector: "button".into(),
                },
            },
            deadline(),
        )
        .await
        .unwrap_err();
    assert_eq!(ambiguous.code, "AMBIGUOUS_LOCATOR");
    let aria = worker
        .execute_one(
            id,
            Request::PageRead {
                page,
                frame: None,
                locator: None,
                read: Read::Aria,
                max_bytes: 8192,
            },
            deadline(),
        )
        .await
        .unwrap();
    assert!(
        aria["result"]["value"]
            .as_str()
            .unwrap()
            .contains("Acme checkout")
    );
    assert!(!aria.to_string().contains("Acme-secret-sentinel"));
    read_conditions(worker, id, page).await;
}

async fn secure_aria(worker: &mut Worker, id: Uuid, page: Reference) {
    use crate::browser::page::{Load, Navigation, Read};
    let deadline = || Instant::now() + Duration::from_secs(15);
    worker
        .execute_one(
            id,
            Request::PageNavigate {
                page,
                navigation: Navigation::SetContent {
                    html: crate::browser::reading::SECURE_FIXTURE.into(),
                    wait_until: Load::Load,
                },
            },
            deadline(),
        )
        .await
        .unwrap();
    let aria = worker
        .execute_one(
            id,
            Request::PageRead {
                page,
                frame: None,
                locator: None,
                read: Read::Aria,
                max_bytes: 8192,
            },
            deadline(),
        )
        .await
        .unwrap();
    let text = aria.to_string();
    assert!(text.contains("Visible ordinary text"));
    for secret in [
        "Acme-secret-sentinel",
        "Acme-role-secret",
        "Acme-autocomplete-secret",
    ] {
        assert!(!text.contains(secret));
    }
    evaluate(worker,id,page,"if(document.querySelector('#secret').value !== 'Acme-secret-sentinel' || document.querySelector('#ordinary').value !== 'Visible ordinary text' || globalThis.inputEvents !== 0) throw new Error('ARIA read mutated form');".into()).await;
}

async fn read_conditions(worker: &mut Worker, id: Uuid, page: Reference) {
    use crate::browser::page::{Condition, Locator, Read};
    let deadline = || Instant::now() + Duration::from_secs(10);
    let matched = worker
        .execute_one(
            id,
            Request::PageWait {
                page,
                frame: None,
                locator: Some(Locator::Css {
                    selector: "button".into(),
                }),
                condition: Condition::Count { value: 3 },
            },
            deadline(),
        )
        .await
        .unwrap();
    assert_eq!(matched["satisfied"], true);
    let expired = worker
        .execute_one(
            id,
            Request::PageWait {
                page,
                frame: None,
                locator: None,
                condition: Condition::Text {
                    value: "Never present".into(),
                    contains: true,
                },
            },
            Instant::now() + Duration::from_millis(300),
        )
        .await
        .unwrap_err();
    assert_eq!(expired.code, "CONDITION_TIMEOUT");
    assert_eq!(worker.shared.state.lock().unwrap().phase, Phase::Ready);
    worker
        .execute_one(
            id,
            Request::PageWait {
                page,
                frame: None,
                locator: Some(Locator::Css {
                    selector: "#toast".into(),
                }),
                condition: Condition::Detached {},
            },
            deadline(),
        )
        .await
        .unwrap();
    let messages = worker
        .execute_one(
            id,
            Request::PageRead {
                page,
                frame: None,
                locator: None,
                read: Read::Messages,
                max_bytes: 8192,
            },
            deadline(),
        )
        .await
        .unwrap();
    assert!(messages.to_string().contains("Acme transient"));
}

async fn navigation_journey(worker: &mut Worker, id: Uuid, page: Reference) {
    use crate::browser::page::{Load, Navigation};
    let site = Site::start().await;
    let url = format!("http://127.0.0.1:{}/frame", site.port);
    let deadline = || Instant::now() + Duration::from_secs(20);
    let first = worker
        .execute_one(
            id,
            Request::PageNavigate {
                page,
                navigation: Navigation::Goto {
                    url: url.clone(),
                    wait_until: Load::Networkidle,
                },
            },
            deadline(),
        )
        .await
        .unwrap();
    assert_eq!(first["url"], url);
    assert_eq!(first["response"]["status"], 200);
    assert_eq!(first["responseMetadataComplete"], true);
    let fragment = format!("{url}#next");
    let same = worker
        .execute_one(
            id,
            Request::PageNavigate {
                page,
                navigation: Navigation::Goto {
                    url: fragment.clone(),
                    wait_until: Load::Load,
                },
            },
            deadline(),
        )
        .await
        .unwrap();
    assert_eq!(same["url"], fragment);
    assert_eq!(same["documentReplaced"], false);
    assert!(same["response"].is_null());
    let reload = worker
        .execute_one(
            id,
            Request::PageNavigate {
                page,
                navigation: Navigation::Reload {
                    wait_until: Load::Domcontentloaded,
                },
            },
            deadline(),
        )
        .await
        .unwrap();
    assert_eq!(reload["response"]["status"], 200);
    let back = worker
        .execute_one(
            id,
            Request::PageNavigate {
                page,
                navigation: Navigation::Back {
                    wait_until: Load::Load,
                },
            },
            deadline(),
        )
        .await
        .unwrap();
    assert_eq!(back["url"], url);
    let forward = worker
        .execute_one(
            id,
            Request::PageNavigate {
                page,
                navigation: Navigation::Forward {
                    wait_until: Load::Load,
                },
            },
            deadline(),
        )
        .await
        .unwrap();
    assert_eq!(forward["url"], fragment);
    cross_site_read(worker, id, page, &site).await;
}

async fn cross_site_read(worker: &mut Worker, id: Uuid, page: Reference, site: &Site) {
    use crate::browser::page::{Load, Locator, Navigation, Read};
    let deadline = || Instant::now() + Duration::from_secs(20);
    worker
        .execute_one(
            id,
            Request::PageNavigate {
                page,
                navigation: Navigation::Goto {
                    url: format!("http://127.0.0.1:{}/main", site.port),
                    wait_until: Load::Load,
                },
            },
            deadline(),
        )
        .await
        .unwrap();
    let entered = worker
        .execute_one(
            id,
            Request::FrameEnter {
                page,
                frame: None,
                locator: Locator::Css {
                    selector: "#child".into(),
                },
            },
            deadline(),
        )
        .await
        .unwrap();
    let child = serde_json::from_value(entered["frame"].clone()).unwrap();
    let text = worker
        .execute_one(
            id,
            Request::PageRead {
                page,
                frame: Some(child),
                locator: None,
                read: Read::Text,
                max_bytes: 8192,
            },
            deadline(),
        )
        .await
        .unwrap();
    assert_eq!(text["result"]["value"], "Owned fixture");
}
