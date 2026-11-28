//! Owned real React component, Suspense and profiling acceptance.
use super::investigation::Site;
use super::*;
use serde_json::json;
#[path = "../../../tests/fixtures/browser_react.rs"]
mod fixture;
pub(super) use fixture::{HTML, asset};
macro_rules! cell {
    ($name:ident,$engine:ident,$mode:ident) => {
        #[tokio::test]
        async fn $name() {
            run(Engine::$engine, Mode::$mode).await;
        }
    };
}
cell!(item_four_framework_chrome_headless, Chrome, Headless);
cell!(item_four_framework_chrome_headed, Chrome, Headed);
cell!(item_four_framework_firefox_headless, Firefox, Headless);
cell!(item_four_framework_firefox_headed, Firefox, Headed);
cell!(item_four_framework_webkit_headless, Webkit, Headless);
cell!(item_four_framework_webkit_headed, Webkit, Headed);
async fn query(session: &Session, registry: Reference, query: Value) -> Value {
    successful(
        session,
        Request::FrameworkQuery {
            registry,
            query: serde_json::from_value(query).unwrap(),
        },
    )
    .await["report"]
        .clone()
}
async fn evaluate(session: &Session, page: Reference, expression: &str) -> Value {
    successful(
        session,
        Request::Evaluate {
            scope: page,
            expression: expression.into(),
            is_function: Some(false),
            argument: Default::default(),
            return_handle: false,
        },
    )
    .await["value"]["value"]
        .clone()
}
async fn run(engine: Engine, mode: Mode) {
    asset(false);
    asset(true);
    std::env::var_os("TESTRUN_BROWSER_ROOT").expect("Select a private browser cache");
    let site = Site::start().await;
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("state");
    crate::artifacts::directory(&directory).unwrap();
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
        &directory,
    )
    .unwrap();
    let exercise = std::panic::AssertUnwindSafe(async {
        let ready = settled(&session, session.startup()).await;
        assert_eq!(ready.outcome, Outcome::Succeeded, "{ready:?}");
        let page = serde_json::from_value(ready.result.unwrap()["page"].clone()).unwrap();
        let prepared = successful(&session, Request::FrameworkPrepare { page }).await;
        let script_id = serde_json::from_value(prepared["scriptId"].clone()).unwrap();
        let navigate = || Request::PageNavigate {
            page,
            navigation: Navigation::Goto {
                url: format!("{}/framework", site.url),
                wait_until: Load::Load,
            },
        };
        successful(&session, navigate()).await;
        let until = Instant::now() + Duration::from_secs(10);
        while evaluate(
            &session,
            page,
            "document.querySelector('#count')?.textContent",
        )
        .await
            != "Count 0"
        {
            assert!(Instant::now() < until, "React application did not mount");
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        let opened = successful(&session, Request::FrameworkOpen { scope: page }).await;
        let registry = serde_json::from_value(opened["registry"].clone()).unwrap();
        let dispose = settled(
            &session,
            session
                .submit(
                    Uuid::new_v4(),
                    Request::HandleDispose { handle: registry },
                    30000,
                )
                .unwrap(),
        )
        .await;
        assert_eq!(
            dispose.code.as_deref(),
            Some("FRAMEWORK_RELEASE_REQUIRED"),
            "{dispose:?}"
        );
        let tree = query(&session, registry, json!({"kind":"tree","limit":100})).await;
        let counter = tree["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["name"] == "AcmeCounter")
            .expect("Counter missing from native React tree");
        let inspected = query(
            &session,
            registry,
            json!({"kind":"inspect","id":counter["id"]}),
        )
        .await;
        assert_eq!(inspected["name"], "AcmeCounter", "{inspected}");
        let suspense = query(&session, registry, json!({"kind":"suspense"})).await;
        assert!(
            suspense["items"]
                .as_array()
                .unwrap()
                .iter()
                .any(|row| row["suspended"] == true && row["dynamic"] == true),
            "{suspense}"
        );
        query(
            &session,
            registry,
            json!({"kind":"renders_start","durationMs":30000}),
        )
        .await;
        evaluate(
            &session,
            page,
            "document.querySelector('#count').click();true",
        )
        .await;
        let until = Instant::now() + Duration::from_secs(10);
        while evaluate(
            &session,
            page,
            "document.querySelector('#count').textContent",
        )
        .await
            != "Count 1"
        {
            assert!(Instant::now() < until, "React update did not commit");
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        let profile = query(&session, registry, json!({"kind":"renders_stop"})).await;
        assert!(
            profile["data"]["dataForRoots"]
                .as_array()
                .unwrap()
                .iter()
                .any(|root| root["commitData"]
                    .as_array()
                    .is_some_and(|commits| !commits.is_empty())),
            "No React commit profile: {profile}"
        );
        assert_eq!(
            query(&session, registry, json!({"kind":"renders_stop"})).await,
            profile
        );
        query(
            &session,
            registry,
            json!({"kind":"renders_start","durationMs":30000}),
        )
        .await;
        successful(&session, Request::FrameworkRelease { registry }).await;
        successful(&session, Request::FrameworkRelease { registry }).await;
        let reopened = successful(&session, Request::FrameworkOpen { scope: page }).await;
        let registry = serde_json::from_value(reopened["registry"].clone()).unwrap();
        query(
            &session,
            registry,
            json!({"kind":"renders_start","durationMs":30000}),
        )
        .await;
        query(&session, registry, json!({"kind":"renders_stop"})).await;
        successful(&session, navigate()).await;
        let stale = settled(
            &session,
            session
                .submit(
                    Uuid::new_v4(),
                    Request::FrameworkQuery {
                        registry,
                        query: serde_json::from_value(json!({"kind":"tree"})).unwrap(),
                    },
                    30000,
                )
                .unwrap(),
        )
        .await;
        assert_eq!(stale.code.as_deref(), Some("STALE_REFERENCE"), "{stale:?}");
        successful(&session, Request::FrameworkRelease { registry }).await;
        successful(&session, Request::FrameworkRelease { registry }).await;
        successful(&session, Request::PageInitScriptRemove { page, script_id }).await;
    })
    .catch_unwind()
    .await;
    let closed = settled(&session, session.disconnect(30000).unwrap()).await;
    assert_eq!(closed.outcome, Outcome::Succeeded, "{closed:?}");
    if let Err(panic) = exercise {
        std::panic::resume_unwind(panic);
    }
}
