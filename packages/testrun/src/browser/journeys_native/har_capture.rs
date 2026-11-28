//! Capture private bodies once, replay immutable bytes, and preserve independent subscriptions.
use super::*;
use crate::browser::contract::{HarCaptureContent, HarCaptureMode, HarNotFound};
use serde_json::json;
use std::sync::atomic::Ordering;
macro_rules! cell {
    ($name:ident,$engine:ident,$mode:ident) => {
        #[tokio::test]
        async fn $name() {
            run(Engine::$engine, Mode::$mode).await;
        }
    };
}
cell!(item_four_har_capture_chrome_headless, Chrome, Headless);
cell!(item_four_har_capture_chrome_headed, Chrome, Headed);
cell!(item_four_har_capture_firefox_headless, Firefox, Headless);
cell!(item_four_har_capture_firefox_headed, Firefox, Headed);
cell!(item_four_har_capture_webkit_headless, Webkit, Headless);
cell!(item_four_har_capture_webkit_headed, Webkit, Headed);

async fn run(engine: Engine, mode: Mode) {
    std::env::var_os("TESTRUN_BROWSER_ROOT").expect("Select a private browser cache");
    let site = super::investigation::Site::start().await;
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
    let exercise = std::panic::AssertUnwindSafe(async {
        let ready = settled(&session, session.startup()).await;
        assert_eq!(ready.outcome, Outcome::Succeeded, "{ready:?}");
        let ready = ready.result.unwrap();
        let page: Reference = serde_json::from_value(ready["page"].clone()).unwrap();
        let context: Reference = serde_json::from_value(ready["context"].clone()).unwrap();
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
        successful(&session, Request::DiagnosticsStart { context }).await;
        let started = successful(
            &session,
            Request::HarCaptureStart {
                path: None,
                resources_dir: None,
                context,
                page: None,
                content: Some(HarCaptureContent::Embed),
                mode: HarCaptureMode::Minimal,
                replaces_archive_id: None,
                url_filter: None,
            },
        )
        .await;
        assert_eq!(started["content"], "embed");
        successful(&session, Request::DiagnosticsStop { context }).await;
        let other = successful(
            &session,
            Request::ContextCreate {
                options: Default::default(),
            },
        )
        .await;
        let other: Reference = serde_json::from_value(other["context"].clone()).unwrap();
        let sibling = successful(
            &session,
            Request::PageCreate {
                context: other,
                select: false,
            },
        )
        .await;
        let sibling = serde_json::from_value(sibling["page"].clone()).unwrap();
        successful(
            &session,
            Request::PageNavigate {
                page: sibling,
                navigation: Navigation::Goto {
                    url: format!("{}/audit", site.url),
                    wait_until: Load::Load,
                },
            },
        )
        .await;
        successful(&session, Request::ContextClose { context: other }).await;
        let response = fetch(&session, page, &site.url).await;
        assert_eq!(
            response,
            json!({"status":409,"text":"Acme response secret","bytes":[0,255,128]})
        );
        wait_complete(&session, context).await;
        successful(&session, Request::DiagnosticsStart { context }).await;
        let stopped = successful(&session, Request::HarCaptureStop { context }).await;
        assert!(!stopped.to_string().contains("Acme response secret"));
        let archive_id = serde_json::from_value(stopped["archiveId"].clone()).unwrap();
        let path = stopped["reports"][0]["attachment"]["path"]
            .as_str()
            .unwrap();
        let original_bytes = std::fs::read(path).unwrap();
        let archive: Value = serde_json::from_slice(&original_bytes).unwrap();
        let entries = archive["log"]["entries"].as_array().unwrap();
        assert!(
            !entries
                .iter()
                .any(|entry| entry["request"]["url"] == format!("{}/audit", site.url)),
            "Sibling context leaked: {archive}"
        );
        let post = entries
            .iter()
            .find(|entry| entry["request"]["method"] == "POST")
            .unwrap();
        assert_eq!(
            post["request"]["postData"]["text"],
            "QWNtZSByZXF1ZXN0IHNlY3JldA=="
        );
        assert_eq!(
            post["response"]["content"]["text"],
            "QWNtZSByZXNwb25zZSBzZWNyZXQ="
        );
        let binary = entries
            .iter()
            .find(|entry| entry["request"]["url"] == format!("{}/capture-binary", site.url))
            .unwrap();
        assert_eq!(binary["response"]["content"]["text"], "AP+A");
        assert_eq!(
            archive["log"]["_testrunCapture"]["droppedEntries"], 0,
            "{archive}"
        );
        assert_eq!(site.posts.load(Ordering::SeqCst), 2);
        let diagnostics = successful(
            &session,
            Request::DiagnosticsRead {
                context,
                query: Default::default(),
            },
        )
        .await;
        assert_eq!(diagnostics["active"], true);
        successful(
            &session,
            Request::PageHarSet {
                page,
                archive_id: Some(archive_id),
                not_found: HarNotFound::Abort,
                url_filter: None,
            },
        )
        .await;
        assert_eq!(fetch(&session, page, &site.url).await, response);
        assert_eq!(
            site.posts.load(Ordering::SeqCst),
            2,
            "Capture/replay must not repeat tracked network requests"
        );
        let refused = session
            .submit(
                Uuid::new_v4(),
                Request::HarCaptureStart {
                    path: None,
                    resources_dir: None,
                    context,
                    page: None,
                    content: Some(HarCaptureContent::Embed),
                    mode: HarCaptureMode::Minimal,
                    replaces_archive_id: Some(archive_id),
                    url_filter: None,
                },
                30000,
            )
            .unwrap();
        assert_eq!(
            settled(&session, refused).await.code.as_deref(),
            Some("HAR_ARCHIVE_IN_USE")
        );
        successful(
            &session,
            Request::PageHarSet {
                page,
                archive_id: None,
                not_found: HarNotFound::Abort,
                url_filter: None,
            },
        )
        .await;
        successful(
            &session,
            Request::HarCaptureStart {
                path: None,
                resources_dir: None,
                context,
                page: None,
                content: Some(HarCaptureContent::Embed),
                mode: HarCaptureMode::Minimal,
                replaces_archive_id: Some(archive_id),
                url_filter: None,
            },
        )
        .await;
        let removing = session
            .submit(Uuid::new_v4(), Request::HarRemove { archive_id }, 30000)
            .unwrap();
        assert_eq!(
            settled(&session, removing).await.code.as_deref(),
            Some("HAR_ARCHIVE_IN_USE")
        );
        let changed = fetch(&session, page, &site.url).await;
        assert_eq!(
            changed,
            json!({"status":409,"text":"Acme response secret","bytes":[1,2,3]})
        );
        wait_complete(&session, context).await;
        let updated = successful(&session, Request::HarCaptureStop { context }).await;
        let replacement = serde_json::from_value(updated["archiveId"].clone()).unwrap();
        assert_ne!(archive_id, replacement);
        assert_eq!(updated["replacesArchiveId"], json!(archive_id));
        assert_eq!(std::fs::read(path).unwrap(), original_bytes);
        successful(
            &session,
            Request::PageHarSet {
                page,
                archive_id: Some(replacement),
                not_found: HarNotFound::Abort,
                url_filter: None,
            },
        )
        .await;
        assert_eq!(fetch(&session, page, &site.url).await, changed);
        assert_eq!(site.posts.load(Ordering::SeqCst), 4);
        successful(
            &session,
            Request::PageHarSet {
                page,
                archive_id: None,
                not_found: HarNotFound::Abort,
                url_filter: None,
            },
        )
        .await;
        successful(
            &session,
            Request::HarRemove {
                archive_id: replacement,
            },
        )
        .await;
        successful(&session, Request::HarRemove { archive_id }).await;
        assert!(!std::path::Path::new(path).exists());
        successful(&session, Request::DiagnosticsStop { context }).await;
        successful(
            &session,
            Request::HarCaptureStart {
                path: None,
                resources_dir: None,
                context,
                page: None,
                content: Some(HarCaptureContent::Omit),
                mode: HarCaptureMode::Minimal,
                replaces_archive_id: None,
                url_filter: None,
            },
        )
        .await;
        fetch(&session, page, &site.url).await;
        wait_complete(&session, context).await;
        let omitted = successful(&session, Request::HarCaptureStop { context }).await;
        let bytes = std::fs::read(
            omitted["reports"][0]["attachment"]["path"]
                .as_str()
                .unwrap(),
        )
        .unwrap();
        let archive: Value = serde_json::from_slice(&bytes).unwrap();
        let post = archive["log"]["entries"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["request"]["method"] == "POST")
            .unwrap();
        assert_eq!(post["request"]["postData"]["_testrunBodyUnavailable"], true);
        assert_eq!(post["response"]["content"]["_testrunBodyUnavailable"], true);
        let text = String::from_utf8(bytes).unwrap();
        assert!(!text.contains("QWNtZSByZXF1ZXN0IHNlY3JldA=="));
        assert!(!text.contains("QWNtZSByZXNwb25zZSBzZWNyZXQ="));
        successful(
            &session,
            Request::HarRemove {
                archive_id: serde_json::from_value(omitted["archiveId"].clone()).unwrap(),
            },
        )
        .await;
        assert_eq!(site.posts.load(Ordering::SeqCst), 6);
    })
    .catch_unwind()
    .await;
    let closed = settled(&session, session.disconnect(30000).unwrap()).await;
    assert_eq!(closed.outcome, Outcome::Succeeded, "{closed:?}");
    if let Err(panic) = exercise {
        std::panic::resume_unwind(panic);
    }
}
async fn fetch(session: &Session, page: Reference, url: &str) -> Value {
    let expression = format!(
        "async()=>{{const response=await fetch({},{{method:'POST',headers:{{Authorization:'Bearer Acme header secret'}},body:'Acme request secret'}});const text=await response.text();const bytes=Array.from(new Uint8Array(await(await fetch({})).arrayBuffer()));return JSON.stringify({{status:response.status,text,bytes}})}}",
        json!(format!("{url}/api?token=Acme-secret")),
        json!(format!("{url}/capture-binary"))
    );
    let value = successful(
        session,
        Request::Evaluate {
            scope: page,
            expression,
            is_function: Some(true),
            argument: Default::default(),
            return_handle: false,
        },
    )
    .await;
    serde_json::from_str(value["value"]["value"].as_str().unwrap()).unwrap()
}
async fn wait_complete(session: &Session, context: Reference) {
    let until = Instant::now() + Duration::from_secs(10);
    loop {
        let status = successful(session, Request::HarCaptureStatus { context }).await;
        if status["entries"].as_u64().unwrap() >= 2
            && status["entries"] == status["completedEntries"]
        {
            break;
        }
        assert!(Instant::now() < until, "Capture did not settle: {status}");
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}
