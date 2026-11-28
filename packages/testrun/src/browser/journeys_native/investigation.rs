//! Real bounded HTTP bodies and WebSocket traffic in the six owned host cells.
use super::*;
use crate::browser::{
    diagnostics::{DiagnosticKind, EventCondition, Query},
    network::{BodySide, Read as NetworkRead},
};
use futures_util::{SinkExt, StreamExt};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

macro_rules! cell {
    ($name:ident,$engine:ident,$mode:ident) => {
        #[tokio::test]
        async fn $name() {
            run(Engine::$engine, Mode::$mode).await;
        }
    };
}
cell!(item_four_network_chrome_headless, Chrome, Headless);
cell!(item_four_network_chrome_headed, Chrome, Headed);
cell!(item_four_network_firefox_headless, Firefox, Headless);
cell!(item_four_network_firefox_headed, Firefox, Headed);
cell!(item_four_network_webkit_headless, Webkit, Headless);
cell!(item_four_network_webkit_headed, Webkit, Headed);

macro_rules! script_cell {
    ($name:ident,$engine:ident,$mode:ident) => {
        #[tokio::test]
        async fn $name() {
            run_scripts(Engine::$engine, Mode::$mode).await;
        }
    };
}
script_cell!(item_four_init_scripts_chrome_headless, Chrome, Headless);
script_cell!(item_four_init_scripts_chrome_headed, Chrome, Headed);
script_cell!(item_four_init_scripts_firefox_headless, Firefox, Headless);
script_cell!(item_four_init_scripts_firefox_headed, Firefox, Headed);
script_cell!(item_four_init_scripts_webkit_headless, Webkit, Headless);
script_cell!(item_four_init_scripts_webkit_headed, Webkit, Headed);

async fn run_scripts(engine: Engine, mode: Mode) {
    std::env::var_os("TESTRUN_BROWSER_ROOT").expect("Select a private browser cache");
    let site = Site::start().await;
    let root = tempfile::tempdir().unwrap();
    let private = root.path().join("state");
    crate::artifacts::directory(&private).unwrap();
    let bundle = if engine == Engine::Chrome {
        Bundle::Chrome(
            super::super::preparation::prepare_chrome(|_| {})
                .await
                .unwrap(),
        )
    } else {
        Bundle::Playwright(
            super::super::preparation::prepare(engine, |_| {})
                .await
                .unwrap(),
        )
    };
    let session = Session::launch_connection(
        bundle,
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        LaunchOptions {
            media_fixtures: Default::default(),
            proxy: None,
            engine,
            mode,
            context: Default::default(),
        },
        &private,
    )
    .unwrap();
    let result = std::panic::AssertUnwindSafe(async {
        let ready = settled(&session, session.startup()).await;
        assert_eq!(ready.outcome, Outcome::Succeeded, "{ready:?}");
        let ready = ready.result.unwrap();
        let page: Reference = serde_json::from_value(ready["page"].clone()).unwrap();
        let context: Reference = serde_json::from_value(ready["context"].clone()).unwrap();
        let added = successful(
            &session,
            Request::PageInitScriptAdd {
                page,
                source: "globalThis.acmeInit=42".into(),
            },
        )
        .await;
        let script_id = serde_json::from_value(added["scriptId"].clone()).unwrap();
        let evaluate = || Request::Evaluate {
            scope: page,
            expression: "typeof acmeInit==='number'?acmeInit:null".into(),
            is_function: Some(false),
            argument: Default::default(),
            return_handle: false,
        };
        assert_eq!(
            successful(&session, evaluate()).await["value"],
            serde_json::json!({"kind":"json","value":null})
        );
        for _ in 0..2 {
            successful(
                &session,
                Request::PageNavigate {
                    page,
                    navigation: Navigation::Goto {
                        url: format!("{}/audit", site.url),
                        wait_until: Load::Load,
                    },
                },
            )
            .await;
            assert_eq!(successful(&session, evaluate()).await["value"]["value"], 42);
        }
        let list = successful(&session, Request::PageInitScriptList { page }).await;
        assert_eq!(list["scripts"].as_array().unwrap().len(), 1);
        assert!(!list.to_string().contains("acmeInit"));
        successful(&session, Request::PageInitScriptRemove { page, script_id }).await;
        assert_eq!(
            successful(&session, evaluate()).await["value"]["value"],
            42,
            "Removal must not undo existing script effects"
        );
        successful(
            &session,
            Request::PageNavigate {
                page,
                navigation: Navigation::Goto {
                    url: format!("{}/audit", site.url),
                    wait_until: Load::Load,
                },
            },
        )
        .await;
        assert_eq!(
            successful(&session, evaluate()).await["value"],
            serde_json::json!({"kind":"json","value":null})
        );
        exercise_routes(&session, page, format!("{}/audit", site.url)).await;
        exercise_handlers(&session, page, context, &site.url).await;
        super::route_limits::exercise(&session, page, context, &site.url).await;
        exercise_overrides(&session, page, &site).await;
        successful(
            &session,
            Request::PageNavigate {
                page,
                navigation: Navigation::Goto {
                    url: format!("{}/audit", site.url),
                    wait_until: Load::Load,
                },
            },
        )
        .await;
        exercise_bindings(
            &session,
            page,
            engine == Engine::Chrome && mode == Mode::Headless,
        )
        .await;
        if engine == Engine::Chrome {
            exercise_authentication(&session, page, &site.url).await;
        }
        exercise_worker_console(&session, page, context).await;
    })
    .catch_unwind()
    .await;
    let closed = settled(&session, session.disconnect(30000).unwrap()).await;
    assert_eq!(closed.outcome, Outcome::Succeeded, "{closed:?}");
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}

async fn exercise_worker_console(session: &Session, page: Reference, context: Reference) {
    successful(session, Request::DiagnosticsStart { context }).await;
    successful(session,Request::Evaluate {scope:page,expression:"void(globalThis.acmeConsoleWorker=new Worker(URL.createObjectURL(new Blob(['self.onmessage=()=>{}'],{type:'application/javascript'}))))".into(),is_function:Some(false),argument:Default::default(),return_handle:false}).await;
    let until = Instant::now() + Duration::from_secs(10);
    let worker = loop {
        let workers = successful(session, Request::WorkerList { page }).await;
        if let Some(worker) = workers["workers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|worker| worker["executionAvailable"] != false)
        {
            break serde_json::from_value(worker["worker"].clone()).unwrap();
        }
        assert!(Instant::now() < until, "Worker not discovered: {workers}");
        tokio::time::sleep(Duration::from_millis(25)).await;
    };
    successful(
        session,
        Request::Evaluate {
            scope: worker,
            expression: "console.log('Acme worker private console')".into(),
            is_function: Some(false),
            argument: Default::default(),
            return_handle: false,
        },
    )
    .await;
    let until = Instant::now() + Duration::from_secs(10);
    loop {
        let history = successful(
            session,
            Request::DiagnosticsRead {
                context,
                query: Query {
                    kind: Some(DiagnosticKind::Console),
                    include_text: true,
                    ..Default::default()
                },
            },
        )
        .await;
        if history["events"].as_array().unwrap().iter().any(|event| {
            event["data"]["worker"] == serde_json::json!(worker)
                && event["data"]["text"] == "Acme worker private console"
        }) {
            break;
        }
        assert!(
            Instant::now() < until,
            "Worker console not correlated: {history}"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    let private = successful(
        session,
        Request::DiagnosticsRead {
            context,
            query: Query::default(),
        },
    )
    .await;
    assert!(!private.to_string().contains("Acme worker private console"));
    successful(
        session,
        Request::Evaluate {
            scope: page,
            expression: "acmeConsoleWorker.terminate()".into(),
            is_function: Some(false),
            argument: Default::default(),
            return_handle: false,
        },
    )
    .await;
    successful(session, Request::DiagnosticsStop { context }).await;
}

async fn exercise_authentication(session: &Session, page: Reference, base: &str) {
    use crate::browser::contract::CallbackResponse;
    for (path, allow) in [("auth", true), ("auth-cancel", false)] {
        let url = format!("{base}/{path}");
        successful(
            session,
            Request::PageRouteSet {
                page,
                url: Some(url.clone()),
                authentication_challenges: true,
            },
        )
        .await;
        let navigation = session
            .submit(
                Uuid::new_v4(),
                Request::PageNavigate {
                    page,
                    navigation: Navigation::Goto {
                        url,
                        wait_until: Load::Load,
                    },
                },
                30000,
            )
            .unwrap();
        for phase in ["request", "authentication"] {
            let until = Instant::now() + Duration::from_secs(10);
            let callback = loop {
                let status = session.status();
                let callbacks = status["callbacks"].as_array().unwrap();
                if let Some(item) = callbacks
                    .iter()
                    .find(|item| item["kind"] == "route" && item["details"]["phase"] == phase)
                {
                    break serde_json::from_value(item["reference"].clone()).unwrap();
                }
                assert!(Instant::now() < until, "Missing {phase} callback: {status}");
                tokio::time::sleep(Duration::from_millis(25)).await;
            };
            let response = if phase == "request" {
                CallbackResponse::RouteContinue {}
            } else if allow {
                CallbackResponse::AuthCredentials {
                    username: "Acme".into(),
                    password: "Acme-password".into(),
                }
            } else {
                CallbackResponse::AuthCancel {}
            };
            successful(session, Request::CallbackResolve { callback, response }).await;
        }
        let result = settled(session, navigation).await;
        assert_eq!(result.outcome, Outcome::Succeeded, "{result:?}");
        let title = successful(
            session,
            Request::Evaluate {
                scope: page,
                expression: "document.title".into(),
                is_function: Some(false),
                argument: Default::default(),
                return_handle: false,
            },
        )
        .await;
        assert_eq!(
            title["value"]["value"],
            if allow {
                "Acme authenticated"
            } else {
                "Acme denied"
            }
        );
    }
    successful(
        session,
        Request::PageRouteSet {
            page,
            url: None,
            authentication_challenges: false,
        },
    )
    .await;
}

async fn exercise_bindings(session: &Session, page: Reference, check_idle_expiry: bool) {
    use crate::browser::contract::CallbackResponse;
    let added = successful(
        session,
        Request::BindingAdd {
            scope: page,
            name: "acmeBridge".into(),
        },
    )
    .await;
    let binding_id = serde_json::from_value(added["bindingId"].clone()).unwrap();
    let evaluation = session
        .submit(
            Uuid::new_v4(),
            Request::Evaluate {
                scope: page,
                expression: "acmeBridge('Acme private binding',41)".into(),
                is_function: Some(false),
                argument: Default::default(),
                return_handle: false,
            },
            30000,
        )
        .unwrap();
    let until = Instant::now() + Duration::from_secs(10);
    let callback = loop {
        let status = session.status();
        assert!(!status.to_string().contains("Acme private binding"));
        let items = status["callbacks"]
            .as_array()
            .or_else(|| status["resources"].as_array())
            .unwrap();
        if let Some(item) = items.iter().find(|item| item["kind"] == "binding") {
            break serde_json::from_value(item["reference"].clone()).unwrap();
        }
        assert!(
            Instant::now() < until,
            "Binding callback unavailable: {status}"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    };
    let arguments = successful(session, Request::BindingRead { callback }).await;
    assert!(
        arguments.to_string().contains("Acme private binding"),
        "{arguments}"
    );
    let remove = settled(
        session,
        session
            .submit(
                Uuid::new_v4(),
                Request::BindingRemove {
                    scope: page,
                    binding_id,
                },
                30000,
            )
            .unwrap(),
    )
    .await;
    assert_eq!(remove.code.as_deref(), Some("BINDING_CALLS_PENDING"));
    successful(
        session,
        Request::CallbackResolve {
            callback,
            response: CallbackResponse::BindingResolve {
                value: serde_json::json!(42),
            },
        },
    )
    .await;
    let result = settled(session, evaluation).await;
    assert_eq!(result.outcome, Outcome::Succeeded, "{result:?}");
    assert_eq!(result.result.unwrap()["value"]["value"], 42);
    let duplicate = settled(
        session,
        session
            .submit(
                Uuid::new_v4(),
                Request::CallbackResolve {
                    callback,
                    response: CallbackResponse::BindingReject {},
                },
                30000,
            )
            .unwrap(),
    )
    .await;
    assert_ne!(duplicate.outcome, Outcome::Succeeded);
    if check_idle_expiry {
        successful(
            session,
            Request::Evaluate {
                scope: page,
                expression: "void acmeBridge().catch(()=>{globalThis.acmeExpired=true})".into(),
                is_function: Some(false),
                argument: Default::default(),
                return_handle: false,
            },
        )
        .await;
        let until = Instant::now() + Duration::from_secs(35);
        assert!(
            session.status()["callbacks"]
                .as_array()
                .unwrap()
                .iter()
                .any(|item| item["kind"] == "binding")
        );
        loop {
            // Status reads the published snapshot without sending work to the actor.
            if !session.status()["callbacks"]
                .as_array()
                .unwrap()
                .iter()
                .any(|item| item["kind"] == "binding")
            {
                break;
            }
            assert!(Instant::now() < until, "Idle callback did not expire");
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        assert_eq!(
            successful(
                session,
                Request::Evaluate {
                    scope: page,
                    expression: "acmeExpired".into(),
                    is_function: Some(false),
                    argument: Default::default(),
                    return_handle: false
                }
            )
            .await["value"]["value"],
            true
        );
    }
    successful(
        session,
        Request::BindingRemove {
            scope: page,
            binding_id,
        },
    )
    .await;
    let removed=successful(session,Request::Evaluate {scope:page,expression:"typeof acmeBridge==='undefined' && !Object.getOwnPropertyNames(globalThis).some(name=>name.startsWith('__testrun_binding_'))".into(),is_function:Some(false),argument:Default::default(),return_handle:false}).await;
    assert_eq!(removed["value"]["value"], true, "{removed}");
}

async fn exercise_routes(session: &Session, page: Reference, url: String) {
    use crate::browser::contract::CallbackResponse;
    successful(
        session,
        Request::PageRouteSet {
            page,
            url: Some(url.clone()),
            authentication_challenges: false,
        },
    )
    .await;
    for response in [
        CallbackResponse::RouteFulfill {
            status: Some(200),
            body: Some("<!doctype html><title>Acme fulfilled</title>".into()),
            json: None,
            content_type: Some("text/html".into()),
            headers: vec![],
            path: None,
            file: None,
        },
        CallbackResponse::RouteContinue {},
        CallbackResponse::RouteAbort { error_code: None },
    ] {
        let fulfilled = matches!(response, CallbackResponse::RouteFulfill { .. });
        let aborted = matches!(response, CallbackResponse::RouteAbort { .. });
        let navigation = session
            .submit(
                Uuid::new_v4(),
                Request::PageNavigate {
                    page,
                    navigation: Navigation::Goto {
                        url: url.clone(),
                        wait_until: Load::Load,
                    },
                },
                30000,
            )
            .unwrap();
        let until = Instant::now() + Duration::from_secs(10);
        let callback = loop {
            let status = session.status();
            let items = status["callbacks"]
                .as_array()
                .or_else(|| status["resources"].as_array())
                .unwrap();
            if let Some(route) = items.iter().find(|item| item["kind"] == "route") {
                break serde_json::from_value(route["reference"].clone()).unwrap();
            }
            assert!(
                Instant::now() < until,
                "Interception callback not observed: {status}"
            );
            tokio::time::sleep(Duration::from_millis(25)).await;
        };
        successful(session, Request::CallbackResolve { callback, response }).await;
        let outcome = settled(session, navigation).await;
        if aborted {
            assert_ne!(outcome.outcome, Outcome::Succeeded, "{outcome:?}");
        } else {
            assert_eq!(outcome.outcome, Outcome::Succeeded, "{outcome:?}");
            let title = successful(
                session,
                Request::Evaluate {
                    scope: page,
                    expression: "document.title".into(),
                    is_function: Some(false),
                    argument: Default::default(),
                    return_handle: false,
                },
            )
            .await;
            assert_eq!(
                title["value"]["value"],
                if fulfilled {
                    "Acme fulfilled"
                } else {
                    "Acme audit 東京🙂"
                }
            );
            if fulfilled {
                exercise_binary_route(session, page, &url).await;
            }
        }
        let stale = settled(
            session,
            session
                .submit(
                    Uuid::new_v4(),
                    Request::CallbackResolve {
                        callback,
                        response: CallbackResponse::RouteContinue {},
                    },
                    30000,
                )
                .unwrap(),
        )
        .await;
        assert_eq!(stale.code.as_deref(), Some("STALE_REFERENCE"));
    }
    successful(
        session,
        Request::PageRouteSet {
            page,
            url: None,
            authentication_challenges: false,
        },
    )
    .await;
    assert_eq!(session.status()["phase"], "ready");
}
async fn exercise_evidence_route(
    session: &Session,
    page: Reference,
    url: String,
    evidence_id: Uuid,
    expected: &[u8],
) {
    use crate::browser::contract::CallbackResponse;
    successful(
        session,
        Request::PageRouteSet {
            page,
            url: Some(url.clone()),
            authentication_challenges: false,
        },
    )
    .await;
    let expression = format!(
        "async()=>{{const response=await fetch({});return JSON.stringify(Array.from(new Uint8Array(await response.arrayBuffer())));}}",
        serde_json::json!(url)
    );
    let pending = session
        .submit(
            Uuid::new_v4(),
            Request::Evaluate {
                scope: page,
                expression,
                is_function: Some(true),
                argument: Default::default(),
                return_handle: false,
            },
            30000,
        )
        .unwrap();
    let until = Instant::now() + Duration::from_secs(10);
    let callback = loop {
        let status = session.status();
        let items = status["callbacks"]
            .as_array()
            .or_else(|| status["resources"].as_array())
            .unwrap();
        if let Some(route) = items.iter().find(|item| item["kind"] == "route") {
            break serde_json::from_value(route["reference"].clone()).unwrap();
        }
        assert!(
            Instant::now() < until,
            "Evidence route unavailable: {status}"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    };
    let invalid = settled(
        session,
        session
            .submit(
                Uuid::new_v4(),
                Request::CallbackResolve {
                    callback,
                    response: CallbackResponse::RouteFulfillEvidence {
                        status: 201,
                        network_evidence_id: Uuid::new_v4(),
                        headers: vec![],
                    },
                },
                30000,
            )
            .unwrap(),
    )
    .await;
    assert_eq!(
        invalid.code.as_deref(),
        Some("UNKNOWN_NETWORK_EVIDENCE"),
        "{invalid:?}"
    );
    successful(
        session,
        Request::CallbackResolve {
            callback,
            response: CallbackResponse::RouteFulfillEvidence {
                status: 201,
                network_evidence_id: evidence_id,
                headers: vec![],
            },
        },
    )
    .await;
    let result = settled(session, pending).await;
    assert_eq!(result.outcome, Outcome::Succeeded, "{result:?}");
    let bytes: Vec<u8> =
        serde_json::from_str(result.result.unwrap()["value"]["value"].as_str().unwrap()).unwrap();
    assert_eq!(bytes, expected);
    successful(
        session,
        Request::PageRouteSet {
            page,
            url: None,
            authentication_challenges: false,
        },
    )
    .await;
}

async fn exercise_binary_route(session: &Session, page: Reference, url: &str) {
    use crate::browser::contract::{CallbackResponse, RouteHeader};
    for (method, status_code, body, length, bytes, reported_length) in [
        ("GET", 201, "AP+A", None, vec![0, 255, 128], Some("3")),
        ("HEAD", 200, "", Some("123"), vec![], Some("123")),
        ("GET", 304, "", Some("123"), vec![], Some("123")),
        ("GET", 204, "", None, vec![], None),
    ] {
        let expression = format!(
            "async()=>{{const response=await fetch({},{{method:{},cache:'no-store'}});return JSON.stringify({{status:response.status,bytes:Array.from(new Uint8Array(await response.arrayBuffer())),header:response.headers.get('x-acme'),length:response.headers.get('content-length'),cookieOne:document.cookie.includes('AcmeBinaryOne=one'),cookieTwo:document.cookie.includes('AcmeBinaryTwo=two')}});}}",
            serde_json::json!(url),
            serde_json::json!(method)
        );
        let pending = session
            .submit(
                Uuid::new_v4(),
                Request::Evaluate {
                    scope: page,
                    expression,
                    is_function: Some(true),
                    argument: Default::default(),
                    return_handle: false,
                },
                30000,
            )
            .unwrap();
        let until = Instant::now() + Duration::from_secs(10);
        let callback = loop {
            let status = session.status();
            let items = status["callbacks"]
                .as_array()
                .or_else(|| status["resources"].as_array())
                .unwrap();
            if let Some(route) = items.iter().find(|item| item["kind"] == "route") {
                break serde_json::from_value(route["reference"].clone()).unwrap();
            }
            assert!(
                Instant::now() < until,
                "Binary interception unavailable: {status}"
            );
            tokio::time::sleep(Duration::from_millis(25)).await;
        };
        let invalid = session
            .submit(
                Uuid::new_v4(),
                Request::CallbackResolve {
                    callback,
                    response: CallbackResponse::RouteFulfillBinary {
                        status: 201,
                        body_base64: "!".into(),
                        headers: vec![],
                    },
                },
                30000,
            )
            .unwrap_err();
        assert_eq!(invalid.code, "ROUTE_BODY_ENCODING");
        let reply = Request::CallbackResolve {
            callback,
            response: CallbackResponse::RouteFulfillBinary {
                status: status_code,
                body_base64: body.into(),
                headers: {
                    let mut headers = vec![
                        RouteHeader {
                            name: "content-type".into(),
                            value: "application/octet-stream".into(),
                        },
                        RouteHeader {
                            name: "x-acme".into(),
                            value: "first".into(),
                        },
                        RouteHeader {
                            name: "x-acme".into(),
                            value: "second".into(),
                        },
                        RouteHeader {
                            name: "set-cookie".into(),
                            value:
                                "AcmeBinaryOne=one; Path=/; Expires=Wed, 21 Oct 2037 07:28:00 GMT"
                                    .into(),
                        },
                        RouteHeader {
                            name: "set-cookie".into(),
                            value: "AcmeBinaryTwo=two; Path=/".into(),
                        },
                    ];
                    if let Some(length) = length {
                        headers.push(RouteHeader {
                            name: "content-length".into(),
                            value: length.into(),
                        });
                    }
                    headers
                },
            },
        };
        if status_code == 304 && session.status()["engine"] == "webkit" {
            let refused = session.submit(Uuid::new_v4(), reply, 30000).unwrap_err();
            assert_eq!(refused.code, "ROUTE_STATUS_UNSUPPORTED");
            successful(
                session,
                Request::CallbackResolve {
                    callback,
                    response: CallbackResponse::RouteAbort { error_code: None },
                },
            )
            .await;
            assert_ne!(settled(session, pending).await.outcome, Outcome::Succeeded);
            continue;
        }
        successful(session, reply).await;
        let result = settled(session, pending).await;
        assert_eq!(result.outcome, Outcome::Succeeded, "{result:?}");
        let result: serde_json::Value =
            serde_json::from_str(result.result.unwrap()["value"]["value"].as_str().unwrap())
                .unwrap();
        assert_eq!(
            result,
            serde_json::json!({"status":status_code,"bytes":bytes,"header":"first, second","length":reported_length,"cookieOne":true,"cookieTwo":true})
        );
    }
}

#[tokio::test]
async fn item_four_lighthouse_chrome_headless() {
    run_audit(Mode::Headless).await;
}
#[tokio::test]
async fn item_four_lighthouse_chrome_headed() {
    run_audit(Mode::Headed).await;
}
#[tokio::test]
async fn item_four_context_workers_chrome_headless() {
    run_context_workers(Mode::Headless).await;
}
#[tokio::test]
async fn item_four_context_workers_chrome_headed() {
    run_context_workers(Mode::Headed).await;
}
pub(super) struct Site {
    pub(super) url: String,
    task: tokio::task::JoinHandle<()>,
    pub(super) posts: Arc<AtomicUsize>,
    pub(super) release: Arc<tokio::sync::Notify>,
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
        let url = format!("http://{}", listener.local_addr().unwrap());
        let posts = Arc::new(AtomicUsize::new(0));
        let count = posts.clone();
        let release = Arc::new(tokio::sync::Notify::new());
        let gate = release.clone();
        let task = tokio::spawn(async move {
            let mut tasks = tokio::task::JoinSet::new();
            loop {
                tokio::select! {accepted=listener.accept()=>{let(stream,_)=accepted.unwrap();tasks.spawn(respond(stream,count.clone(),gate.clone()));},_=tasks.join_next(),if !tasks.is_empty()=>{}}
            }
        });
        Self {
            url,
            task,
            posts,
            release,
        }
    }
}
async fn exercise_handlers(session: &Session, page: Reference, context: Reference, base: &str) {
    use crate::browser::{
        contract::CallbackResponse,
        routing::{HandlerAction, Matcher, Overrides},
    };
    let url = format!("{base}/Acme%20folder/handler-chain?private=Acme-route");
    let mut order = Vec::new();
    for scope in [page, context] {
        let mut handlers = Vec::new();
        for index in 0..3 {
            let added = successful(
                session,
                Request::NetworkRoute {
                    scope,
                    action: if index == 1 {
                        HandlerAction::AddMatcher {
                            times: None,
                            matcher: Matcher::Glob {
                                pattern: format!(
                                    "{base}/Acme folder/**/handler-{{chain,other}}?private=*"
                                ),
                            },
                        }
                    } else {
                        HandlerAction::Add {
                            url: url.clone(),
                            times: None,
                        }
                    },
                },
            )
            .await;
            handlers.push(added["handlerId"].clone());
        }
        order.extend(handlers.into_iter().rev());
    }
    for path in [
        "Acme%20folder/handler-chain/deeper?private=Acme-route",
        "Acme%20folder/handler-chainXprivate=Acme-route",
    ] {
        let passed = successful(
            session,
            Request::Evaluate {
                scope: page,
                expression: format!(
                    "async()=>(await fetch({})).status",
                    serde_json::json!(format!("{base}/{path}"))
                ),
                is_function: Some(true),
                argument: Default::default(),
                return_handle: false,
            },
        )
        .await;
        assert_eq!(passed["value"]["value"], 200);
    }
    let request = || Request::Evaluate {
        scope: page,
        expression: format!(
            "async()=>await (await fetch({})).text()",
            serde_json::json!(url)
        ),
        is_function: Some(true),
        argument: Default::default(),
        return_handle: false,
    };
    let pending = session.submit(Uuid::new_v4(), request(), 30000).unwrap();
    for (index, expected) in order.iter().enumerate() {
        let until = Instant::now() + Duration::from_secs(10);
        let callback = loop {
            let status = session.status();
            let items = status["callbacks"]
                .as_array()
                .or_else(|| status["resources"].as_array())
                .unwrap();
            if let Some(callback) = items
                .iter()
                .find(|item| item["kind"] == "route" && item["handlerId"] == *expected)
            {
                assert!(!callback.to_string().contains("Acme-route"));
                break serde_json::from_value(callback["reference"].clone()).unwrap();
            }
            assert!(
                Instant::now() < until,
                "Handler callback unavailable: {status}"
            );
            tokio::time::sleep(Duration::from_millis(25)).await;
        };
        let response = if index + 1 == order.len() {
            CallbackResponse::RouteFulfill {
                status: Some(200),
                body: Some("Acme handler chain".into()),
                json: None,
                content_type: Some("text/plain".into()),
                headers: vec![],
                path: None,
                file: None,
            }
        } else if index == 0 {
            CallbackResponse::RouteFallbackWith {
                overrides: Overrides {
                    url: Some(format!("{base}/rewritten-chain")),
                    ..Default::default()
                },
            }
        } else {
            CallbackResponse::RouteFallback {}
        };
        successful(session, Request::CallbackResolve { callback, response }).await;
    }
    let result = settled(session, pending).await;
    assert_eq!(result.outcome, Outcome::Succeeded, "{result:?}");
    assert_eq!(
        result.result.unwrap()["value"]["value"],
        "Acme handler chain"
    );
    for scope in [page, context] {
        let listed = successful(
            session,
            Request::NetworkRoute {
                scope,
                action: HandlerAction::List {},
            },
        )
        .await;
        let id = serde_json::from_value(listed["handlers"][0]["handlerId"].clone()).unwrap();
        let removed = successful(
            session,
            Request::NetworkRoute {
                scope,
                action: HandlerAction::Remove { handler_id: id },
            },
        )
        .await;
        assert_eq!(removed["handlers"].as_array().unwrap().len(), 2);
        assert_eq!(removed["handlers"][0]["matcherKind"], "glob");
        let glob = Matcher::Glob {
            pattern: format!("{base}/Acme folder/**/handler-{{chain,other}}?private=*"),
        };
        successful(
            session,
            Request::NetworkRoute {
                scope,
                action: HandlerAction::AddMatcher {
                    times: None,
                    matcher: glob.clone(),
                },
            },
        )
        .await;
        let narrowed = successful(
            session,
            Request::NetworkRoute {
                scope,
                action: HandlerAction::RemoveMatcher {
                    matcher: glob.clone(),
                },
            },
        )
        .await;
        assert_eq!(narrowed["handlers"].as_array().unwrap().len(), 1);
        let unchanged = successful(
            session,
            Request::NetworkRoute {
                scope,
                action: HandlerAction::RemoveMatcher { matcher: glob },
            },
        )
        .await;
        assert_eq!(unchanged["handlers"], narrowed["handlers"]);
        successful(
            session,
            Request::NetworkRoute {
                scope,
                action: HandlerAction::Clear { behavior: None },
            },
        )
        .await;
    }
}
async fn exercise_overrides(session: &Session, page: Reference, site: &Site) {
    use crate::browser::contract::{CallbackResponse, RouteHeader, RouteOverrides};
    let source = format!(
        "{}/rewrite-source?item=Acme&token=Acme-private-route",
        site.url
    );
    successful(
        session,
        Request::PageRouteSet {
            page,
            url: Some(source.clone()),
            authentication_challenges: false,
        },
    )
    .await;
    let near_miss = source.replacen('?', "X", 1);
    let probe = successful(
        session,
        Request::Evaluate {
            scope: page,
            expression: format!(
                "async()=>fetch({}).then(()=>true,()=>true)",
                serde_json::json!(near_miss)
            ),
            is_function: Some(true),
            argument: Default::default(),
            return_handle: false,
        },
    )
    .await;
    assert_eq!(probe["value"]["value"], true);
    let expression = format!(
        "async()=>{{const response=await fetch({},{{headers:{{'x-acme':'old','x-remove':'old'}}}});return JSON.stringify(await response.json())}}",
        serde_json::json!(source)
    );
    let pending = session
        .submit(
            Uuid::new_v4(),
            Request::Evaluate {
                scope: page,
                expression,
                is_function: Some(true),
                argument: Default::default(),
                return_handle: false,
            },
            30000,
        )
        .unwrap();
    let until = Instant::now() + Duration::from_secs(10);
    let callback = loop {
        let status = session.status();
        let resources = status["callbacks"]
            .as_array()
            .or_else(|| status["resources"].as_array())
            .unwrap();
        if let Some(route) = resources.iter().find(|r| r["kind"] == "route") {
            break serde_json::from_value(route["reference"].clone()).unwrap();
        }
        assert!(
            Instant::now() < until,
            "Override callback unavailable: {status}"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    };
    let bad = CallbackResponse::RouteContinueWith {
        overrides: RouteOverrides {
            url: Some(source.replacen("http:", "https:", 1)),
            ..Default::default()
        },
    };
    let id = session
        .submit(
            Uuid::new_v4(),
            Request::CallbackResolve {
                callback,
                response: bad,
            },
            30000,
        )
        .unwrap();
    assert_eq!(
        settled(session, id).await.code.as_deref(),
        Some("ROUTE_URL_PROTOCOL_MISMATCH")
    );
    let before = site.posts.load(Ordering::SeqCst);
    successful(
        session,
        Request::CallbackResolve {
            callback,
            response: CallbackResponse::RouteContinueWith {
                overrides: RouteOverrides {
                    url: Some(format!("{}/rewrite-target?private=Acme", site.url)),
                    method: Some("POST".into()),
                    headers: Some(vec![RouteHeader {
                        name: "x-acme".into(),
                        value: "new".into(),
                    }]),
                    body_base64: Some("AP+A".into()),
                },
            },
        },
    )
    .await;
    let result = settled(session, pending).await;
    assert_eq!(result.outcome, Outcome::Succeeded, "{result:?}");
    let value: serde_json::Value =
        serde_json::from_str(result.result.unwrap()["value"]["value"].as_str().unwrap()).unwrap();
    assert_eq!(
        value,
        serde_json::json!({"target":true,"header":"new","removed":true,"bytes":[0,255,128]})
    );
    assert_eq!(
        site.posts.load(Ordering::SeqCst),
        before + 1,
        "A request override must dispatch once"
    );
    successful(
        session,
        Request::PageRouteSet {
            page,
            url: None,
            authentication_challenges: false,
        },
    )
    .await;
}

async fn respond(
    mut stream: tokio::net::TcpStream,
    posts: Arc<AtomicUsize>,
    release: Arc<tokio::sync::Notify>,
) {
    let mut bytes = Vec::new();
    let mut buffer = [0; 1024];
    while bytes.len() < 8192 && !bytes.windows(4).any(|part| part == b"\r\n\r\n") {
        let n = stream.read(&mut buffer).await.unwrap();
        if n == 0 {
            return;
        }
        bytes.extend_from_slice(&buffer[..n]);
    }
    let header = String::from_utf8_lossy(&bytes).into_owned();
    if header.starts_with("POST /http-upload ") {
        let end = bytes
            .windows(4)
            .position(|part| part == b"\r\n\r\n")
            .unwrap()
            + 4;
        let length = header
            .lines()
            .find_map(|line| {
                line.split_once(':')
                    .filter(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                    .and_then(|(_, value)| value.trim().parse::<usize>().ok())
            })
            .unwrap();
        assert!(length <= 65536);
        while bytes.len() < end + length {
            let count = stream.read(&mut buffer).await.unwrap();
            assert!(count > 0, "Upload ended early");
            bytes.extend_from_slice(&buffer[..count]);
        }
        posts.fetch_add(1, Ordering::SeqCst);
        stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Length: {length}\r\nConnection: close\r\n\r\n").as_bytes()).await.unwrap();
        stream.write_all(&bytes[end..end + length]).await.unwrap();
        return;
    }
    if header.starts_with("GET /capture-binary ") {
        let previous = posts.fetch_add(1, Ordering::SeqCst);
        let body = if previous == 1 {
            [0, 255, 128]
        } else {
            [1, 2, 3]
        };
        stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Length: 3\r\nConnection: close\r\n\r\n").await.unwrap();
        stream.write_all(&body).await.unwrap();
        return;
    }
    if header.starts_with("POST /rewrite-target?private=Acme ") {
        let end = bytes
            .windows(4)
            .position(|part| part == b"\r\n\r\n")
            .unwrap()
            + 4;
        let length = header
            .lines()
            .find_map(|line| {
                line.split_once(':')
                    .filter(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                    .and_then(|(_, value)| value.trim().parse::<usize>().ok())
            })
            .unwrap_or(0);
        assert!(length <= 65536);
        while bytes.len() < end + length {
            let n = stream.read(&mut buffer).await.unwrap();
            assert!(n > 0, "Rewritten body ended early");
            bytes.extend_from_slice(&buffer[..n]);
        }
        let field = |name: &str| {
            header.lines().find_map(|line| {
                line.split_once(':')
                    .filter(|(key, _)| key.eq_ignore_ascii_case(name))
                    .map(|(_, value)| value.trim())
            })
        };
        let body=serde_json::to_vec(&serde_json::json!({"target":true,"header":field("x-acme"),"removed":field("x-remove").is_none(),"bytes":&bytes[end..end+length]})).unwrap();
        posts.fetch_add(1, Ordering::SeqCst);
        stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",body.len()).as_bytes()).await.unwrap();
        stream.write_all(&body).await.unwrap();
        return;
    }
    if header.starts_with("POST /har-write ") {
        posts.fetch_add(1, Ordering::SeqCst);
        stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 18\r\nConnection: close\r\n\r\nAcme network write").await.unwrap();
        return;
    }
    if header.starts_with("POST /http-drop ") {
        let start = bytes
            .windows(4)
            .position(|part| part == b"\r\n\r\n")
            .unwrap()
            + 4;
        let length = header
            .lines()
            .find_map(|line| {
                line.split_once(':')
                    .filter(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                    .and_then(|(_, value)| value.trim().parse::<usize>().ok())
            })
            .unwrap();
        while bytes.len() < start + length {
            let n = stream.read(&mut buffer).await.unwrap();
            if n == 0 {
                break;
            }
            bytes.extend_from_slice(&buffer[..n]);
        }
        assert_eq!(&bytes[start..start + length], b"Acme request secret");
        posts.fetch_add(1, Ordering::SeqCst);
        return;
    }
    if header.starts_with("GET /http-cookie ")
        || header.starts_with("GET /http-hold ")
        || header.starts_with("GET /http-associated ")
    {
        let body = if header.starts_with("GET /http-hold ") {
            release.notified().await;
            "Acme held HTTP"
        } else {
            header
                .lines()
                .find_map(|line| {
                    line.split_once(':')
                        .filter(|(name, _)| name.eq_ignore_ascii_case("cookie"))
                        .map(|(_, value)| value.trim())
                })
                .unwrap_or("")
        };
        let cookie = if header.starts_with("GET /http-associated ") {
            "FromHttp=Acme-response; Path=/; HttpOnly; SameSite=Lax"
        } else {
            "Acme=Acme-secret; Path=/"
        };
        let _=stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nSet-Cookie: {cookie}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await;
        return;
    }
    if header.starts_with("GET /auth") {
        let authorized = !header.starts_with("GET /auth-cancel ")
            && header.lines().any(|line| {
                line.split_once(':').is_some_and(|(name, value)| {
                    name.eq_ignore_ascii_case("authorization")
                        && value.trim() == "Basic QWNtZTpBY21lLXBhc3N3b3Jk"
                })
            });
        let body = if authorized {
            "<!doctype html><title>Acme authenticated</title>"
        } else {
            "<!doctype html><title>Acme denied</title>"
        };
        let status = if authorized {
            "200 OK"
        } else {
            "401 Unauthorized"
        };
        stream.write_all(format!("HTTP/1.1 {status}\r\nContent-Type: text/html\r\nContent-Length: {}\r\nWWW-Authenticate: Basic realm=\"Acme\"\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
        return;
    }
    if header.starts_with("GET /socket?") || header.starts_with("GET /socket ") {
        let key = header
            .lines()
            .find_map(|line| {
                line.split_once(':')
                    .filter(|(name, _)| name.eq_ignore_ascii_case("sec-websocket-key"))
                    .map(|(_, value)| value.trim())
            })
            .unwrap();
        let accept = tokio_tungstenite::tungstenite::handshake::derive_accept_key(key.as_bytes());
        stream.write_all(format!("HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n\r\n").as_bytes()).await.unwrap();
        let mut socket = tokio_tungstenite::WebSocketStream::from_raw_socket(
            stream,
            tokio_tungstenite::tungstenite::protocol::Role::Server,
            None,
        )
        .await;
        socket
            .send(tokio_tungstenite::tungstenite::Message::Text(
                "Acme socket secret".into(),
            ))
            .await
            .unwrap();
        let incoming = tokio::time::timeout(Duration::from_secs(5), socket.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let automatic = incoming.to_text().unwrap() == "Acme automatic close";
        assert!(automatic || incoming.to_text().unwrap() == "Acme client secret");
        socket
            .send(tokio_tungstenite::tungstenite::Message::Binary(
                vec![0, 1, 2, 255].into(),
            ))
            .await
            .unwrap();
        if automatic {
            use tokio_tungstenite::tungstenite::protocol::{CloseFrame, frame::coding::CloseCode};
            socket
                .close(Some(CloseFrame {
                    code: CloseCode::Away,
                    reason: "Acme relay closed".into(),
                }))
                .await
                .unwrap();
            let closed = tokio::time::timeout(Duration::from_secs(5), socket.next())
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            assert!(closed.is_close());
        } else {
            let _ = socket.close(None).await;
        }
        return;
    }
    if header.starts_with("GET /framework ") {
        let body = super::framework::HTML;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = stream.write_all(response.as_bytes()).await;
        return;
    }
    if header.starts_with("GET /react-fixture.js ")
        || header.starts_with("GET /react-dom-fixture.js ")
    {
        let body = super::framework::asset(header.starts_with("GET /react-dom-fixture.js "));
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/javascript\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let _ = stream.write_all(response.as_bytes()).await;
        let _ = stream.write_all(&body).await;
        return;
    }
    if header.starts_with("GET /tools ") {
        let body = super::developer_tools::HTML;
        let _=stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Security-Policy: script-src 'unsafe-inline'; object-src 'none'\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await;
        return;
    }
    let (status, body, mime) = if header.starts_with("GET /route-frame ") {
        (
            "200 OK",
            "<!doctype html><script>globalThis.AcmeFrameRoute=fetch('/frame-probe').then(response=>response.text()).then(text=>{if(parent!==self)parent.postMessage({kind:'Acme routed frame',text},'*');else if(opener)opener.postMessage({kind:'Acme routed frame',text},'*');return text})</script>",
            "text/html",
        )
    } else if header.starts_with("GET /frame-probe ") {
        ("200 OK", "Acme network", "text/plain")
    } else if header.starts_with("GET /context-binding-app ") {
        ("200 OK", super::context_bindings::HTML, "text/html")
    } else if header.starts_with("GET /init-script-app ") {
        ("200 OK", super::init_scripts::HTML, "text/html")
    } else if header.starts_with("GET /startup-popup ") {
        (
            "200 OK",
            "<!doctype html><title>Acme popup</title><script>console.log('Acme startup popup');opener.postMessage('Acme popup','*')</script>",
            "text/html",
        )
    } else if header.starts_with("GET /startup-frame ") {
        (
            "200 OK",
            "<!doctype html><title>Acme frame</title><script>console.log('Acme startup frame');parent.postMessage('Acme frame','*')</script>",
            "text/html",
        )
    } else if header.starts_with("GET /startup-worker ") {
        (
            "200 OK",
            "console.log('Acme startup worker');self.postMessage('Acme worker')",
            "text/javascript",
        )
    } else if header.starts_with("GET /performance ") {
        (
            "200 OK",
            "<!doctype html><title>Acme performance</title><h1>Acme performance</h1><button id=work onclick=\"const end=performance.now()+25;while(performance.now()<end){};document.body.dataset.clicked='yes'\">Work</button>",
            "text/html",
        )
    } else if header.starts_with("GET /workers ") {
        (
            "200 OK",
            "<!doctype html><title>Acme workers</title><script>globalThis.acmeShared=new SharedWorker('/shared-worker.js');acmeShared.port.start();navigator.serviceWorker.register('/service-worker.js').then(()=>navigator.serviceWorker.ready).then(()=>globalThis.acmeServiceReady=true)</script>",
            "text/html",
        )
    } else if header.starts_with("GET /shared-worker.js ") {
        (
            "200 OK",
            "globalThis.acmeWorkerValue=84;console.log('Acme shared');onconnect=e=>{const p=e.ports[0];p.start();p.onmessage=()=>p.postMessage('Acme')}",
            "text/javascript",
        )
    } else if header.starts_with("GET /service-worker.js ") {
        (
            "200 OK",
            "globalThis.acmeWorkerValue=42;console.log('Acme service');self.addEventListener('install',e=>e.waitUntil(self.skipWaiting()));self.addEventListener('activate',e=>e.waitUntil(self.clients.claim()));self.addEventListener('fetch',()=>{})",
            "text/javascript",
        )
    } else if header.starts_with("GET /audit ") {
        (
            "200 OK",
            "<!doctype html><html lang=en><head><title>Acme audit 東京🙂</title></head><body><h1>Acme audit</h1><img width=24 height=24 src='data:image/gif;base64,R0lGODlhAQABAIAAAAAAAP///ywAAAAAAQABAAACAUwAOw=='><button></button></body></html>",
            "text/html",
        )
    } else if header.starts_with("POST /api?") {
        let start = bytes
            .windows(4)
            .position(|part| part == b"\r\n\r\n")
            .unwrap()
            + 4;
        let length = header
            .lines()
            .find_map(|line| {
                line.split_once(':')
                    .filter(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                    .and_then(|(_, value)| value.trim().parse::<usize>().ok())
            })
            .unwrap();
        assert!(length < 1024);
        while bytes.len() < start + length {
            let n = stream.read(&mut buffer).await.unwrap();
            if n == 0 {
                break;
            }
            bytes.extend_from_slice(&buffer[..n]);
        }
        assert_eq!(&bytes[start..start + length], b"Acme request secret");
        posts.fetch_add(1, Ordering::SeqCst);
        ("409 Conflict", "Acme response secret", "text/plain")
    } else {
        (
            "200 OK",
            "<!doctype html><script>fetch('/api?token=Acme-secret',{method:'POST',headers:{Authorization:'Bearer Acme-secret','X-Acme-Private':'Acme-secret'},body:'Acme request secret'});const s=new WebSocket(location.origin.replace('http','ws')+'/socket?token=Acme-secret');s.onopen=()=>s.send('Acme client secret');</script>",
            "text/html",
        )
    };
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {mime}; charset=utf-8\r\nContent-Length: {}\r\nSet-Cookie: Acme=Acme-secret; SameSite=Lax\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes()).await;
}
async fn run_context_workers(mode: Mode) {
    std::env::var_os("TESTRUN_BROWSER_ROOT").expect("Select a private browser cache");
    let site = Site::start().await;
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("state");
    crate::artifacts::directory(&directory).unwrap();
    let bundle = Bundle::Chrome(
        super::super::preparation::prepare_chrome(|_| {})
            .await
            .unwrap(),
    );
    let session = Session::launch_connection(
        bundle,
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        LaunchOptions {
            media_fixtures: Default::default(),
            proxy: None,
            engine: Engine::Chrome,
            mode,
            context: Default::default(),
        },
        &directory,
    )
    .unwrap();
    let result=std::panic::AssertUnwindSafe(async{
        let ready=settled(&session,session.startup()).await;assert_eq!(ready.outcome,Outcome::Succeeded,"{ready:?}");let ready=ready.result.unwrap();
        let page:Reference=serde_json::from_value(ready["page"].clone()).unwrap();let context:Reference=serde_json::from_value(ready["context"].clone()).unwrap();
        let second=successful(&session,Request::PageCreate{context,select:false}).await;let second:Reference=serde_json::from_value(second["page"].clone()).unwrap();
        successful(&session,Request::DiagnosticsStart{context}).await;
        for page in [page,second]{successful(&session,Request::PageNavigate{page,navigation:Navigation::Goto{url:format!("{}/workers",site.url),wait_until:Load::Load}}).await;}
        let deadline=Instant::now()+Duration::from_secs(10);
        let inventory=loop{
            let value=successful(&session,Request::ContextWorkerList{context}).await;
            let workers=value["workers"].as_array().unwrap();
            if [("shared","shared-worker.js"),("service","service-worker.js")].iter().all(|(kind,path)|workers.iter().any(|w|w["kind"]==*kind&&w["url"]==format!("{}/{path}",site.url)&&w["executionAvailable"]==true)){break value;}
            assert!(Instant::now()<deadline,"Context workers not ready: {value}; {}",session.status());tokio::time::sleep(Duration::from_millis(25)).await;
        };
        let worker=|kind:&str|serde_json::from_value::<Reference>(inventory["workers"].as_array().unwrap().iter().find(|w|w["kind"]==kind&&w["url"].as_str().is_some_and(|url|url.starts_with(&site.url))).unwrap()["worker"].clone()).unwrap();
        let evaluate=|scope,expression:&str,return_handle|Request::Evaluate{scope,expression:expression.into(),is_function:Some(false),argument:Default::default(),return_handle};
        let shared=worker("shared");let service=worker("service");
        for (worker,expected) in [(shared,84),(service,42)]{
            let until=Instant::now()+Duration::from_secs(10);
            loop{
                let probe=successful(&session,evaluate(worker,"({url:location.href,type:self.constructor.name,value:typeof acmeWorkerValue==='number'?acmeWorkerValue:null})",false)).await;
                let value=probe["value"]["entries"].as_array().unwrap().iter().find(|entry|entry["key"]=="value").unwrap()["value"]["value"].clone();
                if value==expected{break;}
                assert!(Instant::now()<until,"Worker application startup did not settle: {probe}; inventory: {inventory}");
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
        assert!(successful(&session,Request::WorkerList{page}).await["workers"].as_array().unwrap().is_empty());
        let history=successful(&session,Request::DiagnosticsRead{context,query:Query{kind:Some(DiagnosticKind::Console),include_text:true,..Default::default()}}).await;
        for (worker,text) in [(shared,"Acme shared"),(service,"Acme service")]{
            assert!(history["events"].as_array().unwrap().iter().any(|event|event["page"].is_null()&&event["data"]["worker"]==serde_json::json!(worker)&&event["data"]["text"]==text),"Missing context-worker console event: {history}");
            let selected=successful(&session,Request::DiagnosticsRead{context,query:Query{worker:Some(worker),levels:vec!["LOG".into()],..Default::default()}}).await;
            let events=selected["events"].as_array().unwrap();
            assert!(!events.is_empty(),"{selected}");
            assert!(events.iter().all(|event|event["data"]["worker"]==serde_json::json!(worker)&&event["data"].get("text").is_none()),"{selected}");
        }
        let value=successful(&session,evaluate(shared,"({name:'Acme shared'})",true)).await;let handle=serde_json::from_value(value["handle"].clone()).unwrap();
        successful(&session,Request::PageClose{page}).await;
        assert_eq!(successful(&session,Request::HandleRead{handle}).await["value"]["entries"][0]["value"]["value"],"Acme shared");
        assert_eq!(successful(&session,evaluate(service,"acmeWorkerValue",false)).await["value"]["value"],42);
        let other=successful(&session,Request::ContextCreate{options:Default::default()}).await;let other=serde_json::from_value(other["context"].clone()).unwrap();
        assert!(successful(&session,Request::ContextWorkerList{context:other}).await["workers"].as_array().unwrap().is_empty());
        let foreign=settled(&session,session.submit(Uuid::new_v4(),Request::DiagnosticsRead {context:other,query:Query {worker:Some(shared),..Default::default()}},30000).unwrap()).await;
        assert_eq!(foreign.code.as_deref(),Some("FOREIGN_WORKER"));
        successful(&session,Request::ContextClose{context:other}).await;
        assert_eq!(successful(&session,evaluate(shared,"acmeWorkerValue",false)).await["value"]["value"],84);
    }).catch_unwind().await;
    let closed = settled(&session, session.disconnect(30000).unwrap()).await;
    assert_eq!(closed.outcome, Outcome::Succeeded, "{closed:?}");
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}
async fn run_audit(mode: Mode) {
    use crate::browser::audits;
    std::env::var_os("TESTRUN_BROWSER_ROOT").expect("Select a private browser cache");
    let site = Site::start().await;
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("state");
    crate::artifacts::directory(&directory).unwrap();
    let bundle = Bundle::Chrome(
        super::super::preparation::prepare_chrome(|_| {})
            .await
            .unwrap(),
    );
    let session = Session::launch_connection(
        bundle,
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        LaunchOptions {
            media_fixtures: Default::default(),
            proxy: None,
            engine: Engine::Chrome,
            mode,
            context: Default::default(),
        },
        &directory,
    )
    .unwrap();
    let result = std::panic::AssertUnwindSafe(async {
        let ready = settled(&session, session.startup()).await;
        assert_eq!(ready.outcome, Outcome::Succeeded, "{ready:?}");
        let page: Reference =
            serde_json::from_value(ready.result.unwrap()["page"].clone()).unwrap();
        successful(
            &session,
            Request::PageNavigate {
                page,
                navigation: Navigation::Goto {
                    url: format!("{}/audit", site.url),
                    wait_until: Load::Load,
                },
            },
        )
        .await;
        for (mode, device) in [
            (audits::Mode::Navigation, audits::Device::Desktop),
            (audits::Mode::Snapshot, audits::Device::Mobile),
        ] {
            let id = session
                .submit(
                    Uuid::new_v4(),
                    Request::PageAudit {
                        page,
                        options: audits::Options {
                            mode,
                            device,
                            include_nodes: true,
                            ..Default::default()
                        },
                    },
                    120000,
                )
                .unwrap();
            let outcome = session
                .wait(id, Instant::now() + Duration::from_secs(125))
                .await
                .unwrap();
            assert_eq!(outcome.outcome, Outcome::Succeeded, "{outcome:?}");
            let report = outcome.result.unwrap();
            assert!(
                report["scores"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|score| score["id"] == "accessibility"),
                "{report}"
            );
            assert!(
                report["scores"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|score| score["id"] == "agentic-browsing"),
                "{report}"
            );
            assert!(
                report["audits"]["failed"].as_u64().unwrap() > 0,
                "Fixture must produce real failing audits"
            );
            let json = &report["reports"][0];
            let lhr: Value = serde_json::from_slice(
                &std::fs::read(json["attachment"]["path"].as_str().unwrap()).unwrap(),
            )
            .unwrap();
            assert!(lhr["lighthouseVersion"].is_string());
            assert!(lhr["audits"]["image-alt"]["score"].as_f64().unwrap() < 1.0);
            let html = std::fs::read_to_string(
                report["reports"][1]["attachment"]["path"].as_str().unwrap(),
            )
            .unwrap();
            assert!(html.contains("LIGHTHOUSE_JSON") || html.contains("lighthouseVersion"));
            assert_eq!(session.status()["phase"], "ready");
            assert!(
                session.status()["developers"]
                    .as_array()
                    .unwrap()
                    .is_empty()
            );
            for artifact in report["reports"].as_array().unwrap() {
                successful(
                    &session,
                    Request::DeveloperEvidenceRemove {
                        evidence_id: serde_json::from_value(artifact["evidenceId"].clone())
                            .unwrap(),
                    },
                )
                .await;
            }
        }
    })
    .catch_unwind()
    .await;
    let closed = settled(&session, session.disconnect(30000).unwrap()).await;
    assert_eq!(closed.outcome, Outcome::Succeeded, "{closed:?}");
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}
async fn run(engine: Engine, mode: Mode) {
    std::env::var_os("TESTRUN_BROWSER_ROOT").expect("Select a private browser cache");
    let site = Site::start().await;
    let root = tempfile::tempdir().unwrap();
    let private = root.path().join("state");
    crate::artifacts::directory(&private).unwrap();
    let bundle = if engine == Engine::Chrome {
        Bundle::Chrome(
            super::super::preparation::prepare_chrome(|_| {})
                .await
                .unwrap(),
        )
    } else {
        Bundle::Playwright(
            super::super::preparation::prepare(engine, |_| {})
                .await
                .unwrap(),
        )
    };
    let session = Session::launch_connection(
        bundle,
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        LaunchOptions {
            media_fixtures: Default::default(),
            proxy: None,
            engine,
            mode,
            context: Default::default(),
        },
        &private,
    )
    .unwrap();
    let result = std::panic::AssertUnwindSafe(async {
        let ready = settled(&session, session.startup()).await;
        assert_eq!(ready.outcome, Outcome::Succeeded, "{ready:?}");
        let ready = ready.result.unwrap();
        let page: Reference = serde_json::from_value(ready["page"].clone()).unwrap();
        let context: Reference = serde_json::from_value(ready["context"].clone()).unwrap();
        let start = successful(&session, Request::DiagnosticsStart { context }).await;
        let after = start["latestSequence"].as_u64().unwrap();
        successful(
            &session,
            Request::PageNavigate {
                page,
                navigation: Navigation::Goto {
                    url: site.url.clone(),
                    wait_until: Load::Load,
                },
            },
        )
        .await;
        let matched = successful(
            &session,
            Request::DiagnosticsWait {
                context,
                after,
                conditions: vec![EventCondition {
                    event: DiagnosticKind::Response,
                    page: Some(page),
                    request_id: None,
                    web_socket_id: None,
                    method: Some("POST".into()),
                    status: Some(409),
                    url: Some(format!("{}/api", site.url)),
                }],
            },
        )
        .await;
        let request_id: Uuid =
            serde_json::from_value(matched["event"]["data"]["requestId"].clone()).unwrap();
        successful(
            &session,
            Request::DiagnosticsWait {
                context,
                after,
                conditions: vec![EventCondition {
                    event: DiagnosticKind::RequestFinished,
                    page: Some(page),
                    request_id: Some(request_id),
                    web_socket_id: None,
                    method: None,
                    status: None,
                    url: None,
                }],
            },
        )
        .await;
        let headers = successful(
            &session,
            Request::NetworkRead {
                context,
                request_id,
                options: NetworkRead {
                    headers: true,
                    ..Default::default()
                },
            },
        )
        .await;
        assert!(!headers.to_string().contains("Acme-secret"), "{headers}");
        for (side, expected) in [
            (BodySide::Request, b"Acme request secret".as_slice()),
            (BodySide::Response, b"Acme response secret".as_slice()),
        ] {
            let body = successful(
                &session,
                Request::NetworkRead {
                    context,
                    request_id,
                    options: NetworkRead {
                        body: Some(side),
                        ..Default::default()
                    },
                },
            )
            .await;
            let path = body["body"]["attachment"]["path"].as_str().unwrap();
            assert_eq!(std::fs::read(path).unwrap(), expected);
            let evidence_id = serde_json::from_value(body["body"]["evidenceId"].clone()).unwrap();
            if matches!(side, BodySide::Response) {
                exercise_evidence_route(
                    &session,
                    page,
                    format!("{}/evidence-replay", site.url),
                    evidence_id,
                    expected,
                )
                .await;
            }
            successful(&session, Request::NetworkEvidenceRemove { evidence_id }).await;
            assert!(!Path::new(path).exists());
        }
        let small = settled(
            &session,
            session
                .submit(
                    Uuid::new_v4(),
                    Request::NetworkRead {
                        context,
                        request_id,
                        options: NetworkRead {
                            body: Some(BodySide::Response),
                            max_bytes: 1,
                            ..Default::default()
                        },
                    },
                    30000,
                )
                .unwrap(),
        )
        .await;
        assert_eq!(small.code.as_deref(), Some("BODY_BOUNDS"));
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let sockets = successful(
                &session,
                Request::DiagnosticsRead {
                    context,
                    query: Query {
                        after: Some(after),
                        kind: Some(DiagnosticKind::WebSocket),
                        include_text: true,
                        limit: 100,
                        ..Default::default()
                    },
                },
            )
            .await;
            let events = sockets["events"].as_array().unwrap();
            if events
                .iter()
                .any(|event| event["data"]["phase"] == "closed")
            {
                for expected in ["Acme socket secret", "Acme client secret", "AAEC/w=="] {
                    assert!(
                        events.iter().any(|event| event["data"]["text"] == expected),
                        "{sockets}"
                    );
                }
                let metadata = successful(
                    &session,
                    Request::DiagnosticsRead {
                        context,
                        query: Query {
                            after: Some(after),
                            kind: Some(DiagnosticKind::WebSocket),
                            limit: 100,
                            ..Default::default()
                        },
                    },
                )
                .await;
                assert!(!metadata.to_string().contains("Acme-secret"));
                assert!(!metadata.to_string().contains("Acme socket secret"));
                break;
            }
            assert!(
                Instant::now() < deadline,
                "Socket traffic unavailable: {sockets}"
            );
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        assert_eq!(
            site.posts.load(Ordering::SeqCst),
            1,
            "Inspection must not replay the POST"
        );
    })
    .catch_unwind()
    .await;
    let closed = settled(&session, session.disconnect(30000).unwrap()).await;
    assert_eq!(closed.outcome, Outcome::Succeeded, "{closed:?}");
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}
