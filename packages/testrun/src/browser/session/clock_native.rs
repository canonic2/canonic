//! Owned virtual clocks remain separate from capture scheduling and host time.
use super::*;
use super::{page_native::successful, settings_native::js};
use crate::browser::{
    clock::Action as Clock,
    contract::{ContextOptions, Engine, Mode, Viewport},
    input::Action as Input,
    page::{Load, Navigation, Read},
};

#[tokio::test]
async fn firefox_webkit_clocks_preserve_host_deadlines_and_paused_media_in_both_modes() {
    for engine in [Engine::Firefox, Engine::Webkit] {
        for mode in [Mode::Headless, Mode::Headed] {
            let root = tempfile::tempdir().unwrap();
            let private = root.path().join("state");
            crate::artifacts::directory(&private).unwrap();
            let session = Session::launch(
                super::native::bundle().await,
                Uuid::new_v4(),
                Uuid::new_v4(),
                LaunchOptions {
                    media_fixtures: Default::default(),
                    proxy: None,
                    engine,
                    mode,
                    context: ContextOptions::default(),
                },
                &private,
            )
            .unwrap();
            let exercising = tokio::spawn(exercise(session.clone())).await;
            let disconnect = session.disconnect(30000).unwrap();
            assert_eq!(
                session
                    .wait(disconnect, Instant::now() + Duration::from_secs(35))
                    .await
                    .unwrap()
                    .outcome,
                Outcome::Succeeded
            );
            if let Err(error) = exercising {
                let evidence = root.keep();
                panic!("{engine:?}/{mode:?}: {error}; retained evidence: {evidence:?}");
            }
        }
    }
}
async fn clock(session: &Session, context: Reference, action: Clock) -> Value {
    successful(session, Request::ContextClock { context, action }).await
}
async fn page(session: &Session, context: Reference) -> Reference {
    serde_json::from_value(
        successful(
            session,
            Request::PageCreate {
                context,
                select: false,
            },
        )
        .await["page"]
            .clone(),
    )
    .unwrap()
}
#[expect(
    clippy::too_many_lines,
    reason = "One ordered clock fixture preserves its context timeline through timer, media and host-time assertions"
)]
async fn exercise(session: Session) {
    let startup = session
        .wait(session.startup(), Instant::now() + Duration::from_secs(45))
        .await
        .unwrap();
    assert_eq!(startup.outcome, Outcome::Succeeded);
    let started = startup.result.unwrap();
    let context: Reference = serde_json::from_value(started["context"].clone()).unwrap();
    let main: Reference = serde_json::from_value(started["page"].clone()).unwrap();
    successful(
        &session,
        Request::PageNavigate {
            page: main,
            navigation: Navigation::SetContent {
                html: super::preview_native::fixture_html().into(),
                wait_until: Load::Load,
            },
        },
    )
    .await;
    let sibling = page(&session, context).await;
    let foreign_context: Reference = serde_json::from_value(
        successful(
            &session,
            Request::ContextCreate {
                options: ContextOptions::default(),
            },
        )
        .await["context"]
            .clone(),
    )
    .unwrap();
    let foreign = page(&session, foreign_context).await;
    let foreign_time = js(&session, foreign, "() => Date.now()")
        .await
        .as_i64()
        .unwrap();
    const EPOCH: i64 = 1_700_000_000_000;
    clock(
        &session,
        context,
        Clock::Install {
            time_ms: Some(EPOCH),
        },
    )
    .await;
    clock(&session, context, Clock::SetFixedTime { time_ms: EPOCH }).await;
    assert_eq!(js(&session, main, "() => Date.now()").await, EPOCH);
    assert_eq!(js(&session, sibling, "() => Date.now()").await, EPOCH);
    assert!(
        js(&session, foreign, "() => Date.now()")
            .await
            .as_i64()
            .unwrap()
            >= foreign_time
    );
    clock(&session, context, Clock::PauseAt { time_ms: EPOCH }).await;
    js(&session,main,"() => {globalThis.acmeTicks=0;globalThis.acmeInterval=setInterval(()=>acmeTicks++,1000);return true;}").await;
    clock(&session, context, Clock::RunFor { duration_ms: 2000 }).await;
    assert_eq!(js(&session, main, "() => acmeTicks").await, 2);
    clock(
        &session,
        context,
        Clock::FastForward {
            duration_ms: 86_400_000,
        },
    )
    .await;
    assert_eq!(js(&session, main, "() => acmeTicks").await, 3);
    clock(&session, context, Clock::RunFor { duration_ms: 2000 }).await;
    assert_eq!(js(&session, main, "() => acmeTicks").await, 5);
    clock(
        &session,
        context,
        Clock::SetSystemTime {
            time_ms: EPOCH + 10_000,
        },
    )
    .await;
    assert_eq!(js(&session, main, "() => Date.now()").await, EPOCH + 10_000);
    assert_eq!(js(&session, main, "() => acmeTicks").await, 5);
    clock(&session, context, Clock::Resume {}).await;
    assert_eq!(
        js(
            &session,
            main,
            "() => new Promise(resolve => setTimeout(()=>resolve(true),5))"
        )
        .await,
        true
    );
    let future = page(&session, context).await;
    let future_time = js(&session, future, "() => Date.now()")
        .await
        .as_i64()
        .unwrap();
    assert!(
        (EPOCH + 10_000..EPOCH + 70_000).contains(&future_time),
        "{future_time}"
    );
    clock(&session, context, Clock::SetFixedTime { time_ms: EPOCH }).await;
    clock(&session, context, Clock::PauseAt { time_ms: EPOCH }).await;
    let observation = successful(
        &session,
        Request::PageRead {
            page: main,
            frame: None,
            locator: None,
            read: Read::Geometry,
            max_bytes: 8192,
        },
    )
    .await;
    let observation = serde_json::from_value(observation["inputObservation"].clone()).unwrap();
    clock(
        &session,
        context,
        Clock::SetSystemTime {
            time_ms: EPOCH + 86_400_000,
        },
    )
    .await;
    successful(
        &session,
        Request::PageInput {
            page: main,
            observation: Some(observation),
            actions: vec![
                Input::KeyDown {
                    key: "Shift".into(),
                },
                Input::KeyUp {
                    key: "ShiftLeft".into(),
                },
            ],
        },
    )
    .await;
    let host_before = super::page::now_ms();
    let preview: Reference = serde_json::from_value(
        successful(
            &session,
            Request::PreviewStart {
                page: main,
                options: Default::default(),
            },
        )
        .await["preview"]
            .clone(),
    )
    .unwrap();
    let before = super::preview_native::latest(&session, preview, 0).await;
    assert!(u128::from(before.metadata["arrivalUnixMs"].as_u64().unwrap()) >= host_before);
    assert_eq!(
        js(&session, main, "() => Date.now()").await,
        EPOCH + 86_400_000
    );
    let recording: Reference = serde_json::from_value(
        successful(
            &session,
            Request::RecordingStart {
                page: main,
                options: Default::default(),
            },
        )
        .await["recording"]
            .clone(),
    )
    .unwrap();
    let resized = successful(
        &session,
        Request::PageResize {
            page: main,
            viewport: Viewport {
                width: 959,
                height: 599,
            },
        },
    )
    .await;
    assert_eq!(resized["previewRecovery"]["succeeded"], true, "{resized}");
    let after = super::preview_native::latest(
        &session,
        preview,
        before.metadata["seq"].as_str().unwrap().parse().unwrap(),
    )
    .await;
    assert_ne!(
        after.metadata["sourceEpoch"],
        before.metadata["sourceEpoch"]
    );
    super::preview_native::corners(&after, session.journal_directory());
    let resized_movie = successful(&session, Request::RecordingStop { recording }).await;
    assert_eq!(resized_movie["status"], "partial", "{resized_movie}");
    assert_eq!(
        resized_movie["verification"]["decodable"], true,
        "{resized_movie}"
    );
    let recording: Reference = serde_json::from_value(
        successful(
            &session,
            Request::RecordingStart {
                page: main,
                options: crate::browser::recording::Options {
                    max_duration_ms: 500,
                },
            },
        )
        .await["recording"]
            .clone(),
    )
    .unwrap();
    let terminal = tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let changed = session.0.host.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            let result = session.status()["recordings"]
                .as_array()
                .unwrap()
                .iter()
                .find(|result| result["recording"] == json!(recording))
                .unwrap()
                .clone();
            if matches!(
                result["status"].as_str(),
                Some("complete" | "partial" | "failed")
            ) {
                return result;
            }
            changed.await;
        }
    })
    .await
    .unwrap();
    assert!(
        matches!(terminal["status"].as_str(), Some("complete" | "partial")),
        "{terminal}"
    );
    assert_eq!(terminal["verification"]["decodable"], true, "{terminal}");
    assert_eq!(terminal["statistics"]["durationMs"], 500);
    assert_eq!(
        js(&session, main, "() => Date.now()").await,
        EPOCH + 86_400_000
    );
    successful(&session, Request::PreviewStop { preview }).await;
    successful(
        &session,
        Request::ContextClose {
            context: foreign_context,
        },
    )
    .await;
}
