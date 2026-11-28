//! One application-control journey shared by all owned engines and launch modes.
use super::{
    contract::{ContextOptions, Engine, LaunchOptions, Mode, Reference, Request},
    lifecycle::{Operation, Outcome},
    owned::{Bundle, Session},
    page::{Action, Condition, Load, Locator, Navigation, Read},
};
use futures_util::FutureExt;
use serde_json::Value;
use std::{path::Path, time::Duration};
use tokio::time::Instant;
use uuid::Uuid;

mod automation_debugger;
mod captures;
mod context_bindings;
mod context_routes;
mod controls;
mod developer_tools;
mod diagnostic_arguments;
mod faults;
mod files;
mod frame_routes;
mod framework;
mod har;
mod har_capture;
mod http;
mod http_partitions;
mod init_scripts;
mod investigation;
mod performance;
mod route_limits;
mod site;
mod socket_routes;
mod startup;
mod tracing;
mod webmcp_debug;

#[tokio::test]
async fn item_two_shared_application_chrome_headless() {
    run(Engine::Chrome, Mode::Headless).await;
}
#[tokio::test]
async fn item_two_shared_application_chrome_headed() {
    run(Engine::Chrome, Mode::Headed).await;
}
#[tokio::test]
async fn item_two_shared_application_firefox_headless() {
    run(Engine::Firefox, Mode::Headless).await;
}
#[tokio::test]
async fn item_two_shared_application_firefox_headed() {
    run(Engine::Firefox, Mode::Headed).await;
}
#[tokio::test]
async fn item_two_shared_application_webkit_headless() {
    run(Engine::Webkit, Mode::Headless).await;
}
#[tokio::test]
async fn item_two_shared_application_webkit_headed() {
    run(Engine::Webkit, Mode::Headed).await;
}

async fn run(engine: Engine, mode: Mode) {
    run_item(engine, mode, false).await;
}
async fn run_item(engine: Engine, mode: Mode, media: bool) {
    let cache = std::env::var_os("TESTRUN_BROWSER_ROOT").expect("Select a private browser cache");
    assert!(Path::new(&cache).is_absolute());
    let site = site::Site::start().await;
    let root = tempfile::tempdir().unwrap();
    let private = root.path().join("state");
    crate::artifacts::directory(&private).unwrap();
    let bundle = if engine == Engine::Chrome {
        Bundle::Chrome(super::preparation::prepare_chrome(|_| {}).await.unwrap())
    } else {
        Bundle::Playwright(super::preparation::prepare(engine, |_| {}).await.unwrap())
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
            context: ContextOptions {
                accept_downloads: true,
                has_touch: true,
                device_scale_factor: if media { 2.0 } else { 1.0 },
                ..ContextOptions::default()
            },
        },
        &private,
    )
    .unwrap();
    let result = std::panic::AssertUnwindSafe(async {
        if media {
            captures::exercise(&session, engine, mode).await;
        } else {
            exercise(&session, &site).await;
        }
    })
    .catch_unwind()
    .await;
    if result.is_err() {
        if let Session::Playwright(session) = &session {
            eprintln!("Fixture backend causes: {:?}", session.private_failures());
        }
        let directory = private.join(format!(
            "browser-{}",
            session.status()["connectionId"].as_str().unwrap()
        ));
        let mut entries = std::fs::read_dir(directory)
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
            .collect::<Vec<_>>();
        entries.sort();
        for path in entries.iter().rev().take(8).rev() {
            let value: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
            eprintln!("Fixture journal: {value}");
        }
    }
    let closed = settled(&session, session.disconnect(30000).unwrap()).await;
    assert_eq!(
        closed.outcome,
        Outcome::Succeeded,
        "{engine:?}/{mode:?}: {closed:?}"
    );
    site.stop().await;
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}
async fn settled(session: &Session, id: Uuid) -> Operation {
    let operation = session
        .wait(id, Instant::now() + Duration::from_secs(35))
        .await
        .unwrap();
    assert!(operation.settled, "{operation:?}; {}", session.status());
    operation
}
async fn successful(session: &Session, request: Request) -> Value {
    let requested = format!("{request:?}");
    let id = session
        .submit(Uuid::new_v4(), request, 30000)
        .unwrap_or_else(|error| panic!("{requested}: {error:?}; {}", session.status()));
    let operation = settled(session, id).await;
    assert_eq!(
        operation.outcome,
        Outcome::Succeeded,
        "{requested}: {operation:?}; {}",
        serde_json::json!({"phase":session.status()["phase"],"engine":session.status()["engine"],"failure":session.status()["failure"],"input":session.status()["input"]})
    );
    operation.result.unwrap()
}
fn css(selector: &str) -> Locator {
    Locator::Css {
        selector: selector.into(),
    }
}
fn action(page: Reference, selector: Locator, action: Action) -> Request {
    Request::PageAction {
        page,
        frame: None,
        locator: selector,
        action,
        force: false,
        postcondition: None,
    }
}
fn read(page: Reference, selector: Option<Locator>, read: Read) -> Request {
    Request::PageRead {
        page,
        frame: None,
        locator: selector,
        read,
        max_bytes: 8192,
    }
}
async fn wait_text(session: &Session, page: Reference, selector: &str, text: &str) {
    successful(
        session,
        Request::PageWait {
            page,
            frame: None,
            locator: Some(css(selector)),
            condition: Condition::Text {
                value: text.into(),
                contains: false,
            },
        },
    )
    .await;
}
async fn exercise(session: &Session, site: &site::Site) {
    let startup = settled(session, session.startup()).await;
    assert_eq!(startup.outcome, Outcome::Succeeded, "{startup:?}");
    let page: Reference = serde_json::from_value(startup.result.unwrap()["page"].clone()).unwrap();
    successful(
        session,
        Request::PageNavigate {
            page,
            navigation: Navigation::Goto {
                url: site.url("/redirect"),
                wait_until: Load::Load,
            },
        },
    )
    .await;
    controls::reading_and_editing(session, page).await;
    controls::dialog_and_waits(session, page).await;
    controls::frames_and_shadow(session, page).await;
    controls::popup_and_events(session, page).await;
    faults::exercise(session, page).await;
    files::fixtures_and_download(session, page).await;
    let direct = successful(
        session,
        Request::PageReadUrl {
            page,
            url: site.url("/article"),
            max_bytes: 8192,
        },
    )
    .await;
    assert_eq!(direct["status"], 200);
    assert_eq!(direct["scriptsExecuted"], false);
    assert_eq!(direct["result"]["value"], "Acme article");
    let heading = successful(session, read(page, Some(css("h1")), Read::Text)).await;
    assert_eq!(heading["result"]["value"], "Acme checkout");
}
