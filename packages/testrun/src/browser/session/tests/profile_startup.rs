//! Inspect all Playwright pages in a real saved-profile session at readiness.
use super::*;
use crate::browser::profiles::{Identity, Selection};

#[tokio::test]
async fn firefox_and_webkit_profile_startup_selects_one_initialized_page() {
    std::env::var_os("TESTRUN_BROWSER_ROOT").expect("Select a private browser cache");
    for engine in [Engine::Firefox, Engine::Webkit] {
        for mode in [Mode::Headed, Mode::Headless] {
            let bundle = crate::browser::preparation::prepare(engine, |_| {})
                .await
                .unwrap();
            let directory = tempfile::tempdir().unwrap();
            let private = directory.path().join("state");
            let path = directory.path().join("profile");
            crate::artifacts::directory(&private).unwrap();
            crate::artifacts::directory(&path).unwrap();
            let identity = Identity {
                engine,
                channel: None,
                version: engine.pinned_version().unwrap(),
            };
            crate::artifacts::write(
                &path.join(".testrun-profile-v1.json"),
                &serde_json::to_vec(&identity).unwrap(),
            )
            .unwrap();
            let profile = Selection::Borrowed { path }.acquire(&identity).unwrap();
            let session = Session::launch_selected(
                BackendStart::Launch(bundle),
                Uuid::new_v4(),
                Uuid::new_v4(),
                Uuid::new_v4(),
                LaunchOptions {
                    media_fixtures: Default::default(),
                    proxy: None,
                    engine,
                    mode,
                    context: ContextOptions::default(),
                },
                Some(profile),
                &private,
            )
            .unwrap();
            let check = tokio::spawn(check_startup(session.clone())).await;
            let disconnect = session.disconnect(30000).unwrap();
            let cleanup = session
                .wait(disconnect, Instant::now() + Duration::from_secs(35))
                .await
                .unwrap();
            assert_eq!(cleanup.outcome, Outcome::Succeeded, "{cleanup:?}");
            check.unwrap_or_else(|error| panic!("{engine:?}/{mode:?}: {error}"));
        }
    }
}

async fn check_startup(session: Session) {
    let deadline = Instant::now() + Duration::from_secs(60);
    let startup = session.wait(session.startup(), deadline).await.unwrap();
    assert_eq!(startup.outcome, Outcome::Succeeded, "{startup:?}");
    let result = startup.result.unwrap();
    let context = serde_json::from_value(result["context"].clone()).unwrap();
    let page = serde_json::from_value(result["page"].clone()).unwrap();
    let host = &session.0.host;
    let pages = host
        .connection()
        .unwrap()
        .children(&host.wire(context, Kind::Context).unwrap(), "Page")
        .await
        .unwrap();
    assert_eq!(pages, [host.wire(page, Kind::Page).unwrap()]);
    assert_eq!(session.status()["selectedPage"], result["page"]);

    let frame = host.page_scope(page, None, deadline).await.unwrap();
    let initialized = host
        .evaluate(
            &frame.wire,
            "() => location.href === 'about:blank' && !!globalThis.__testrunMessages && !!globalThis[Symbol.for('canonic.testrun.native-timing.v1')]",
            Value::Null,
            deadline,
        )
        .await
        .unwrap();
    assert_eq!(initialized, true);
}
