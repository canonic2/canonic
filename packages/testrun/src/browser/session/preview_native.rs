//! Owned Firefox/WebKit preview, resize and document-freshness acceptance.

use super::page_native::successful;
use super::*;
use crate::browser::{
    contract::{ContextOptions, Engine, Mode, Viewport},
    page::{Load, Navigation},
    preview::Options,
};

const HTML: &str = r#"<!doctype html><style>body{margin:0}i{position:fixed;width:40px;height:40px;background:#f00;top:0;left:0}.tr{background:#00f;left:auto;right:0}.bl{background:#0f0;top:auto;bottom:0}.br{background:#ff0;left:auto;right:0;top:auto;bottom:0}output{position:fixed;top:60px}</style><i></i><i class=tr></i><i class=bl></i><i class=br></i><output>0</output><script>let frame=0;function paint(){document.querySelector('output').textContent=++frame;requestAnimationFrame(paint)}paint()</script>"#;

pub(super) fn fixture_html() -> &'static str {
    HTML
}

#[tokio::test]
async fn firefox_webkit_preview_survives_navigation_and_viewport_growth_in_both_modes() {
    for engine in [Engine::Firefox, Engine::Webkit] {
        for mode in [Mode::Headless, Mode::Headed] {
            for scale in [1.0, 2.0] {
                let directory = tempfile::tempdir().unwrap();
                let private = directory.path().join("state");
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
                        context: ContextOptions {
                            device_scale_factor: scale,
                            ..ContextOptions::default()
                        },
                    },
                    &private,
                )
                .unwrap();
                let scenario = tokio::spawn(exercise(session.clone(), scale)).await;
                let id = session.disconnect(30000).unwrap();
                let closed = session
                    .wait(id, Instant::now() + Duration::from_secs(35))
                    .await
                    .unwrap();
                assert_eq!(closed.outcome, Outcome::Succeeded, "{}", session.status());
                if let Err(error) = scenario {
                    let evidence = directory.keep();
                    panic!(
                        "{engine:?}/{mode:?}/DPR{scale}: {error}; retained fixture evidence: {evidence:?}"
                    );
                }
            }
        }
    }
}

async fn exercise(session: Session, scale: f64) {
    let startup = session
        .wait(session.startup(), Instant::now() + Duration::from_secs(45))
        .await
        .unwrap();
    assert_eq!(startup.outcome, Outcome::Succeeded, "{}", session.status());
    let page: Reference = serde_json::from_value(startup.result.unwrap()["page"].clone()).unwrap();
    successful(
        &session,
        Request::PageNavigate {
            page,
            navigation: Navigation::SetContent {
                html: HTML.into(),
                wait_until: Load::Load,
            },
        },
    )
    .await;
    let started = successful(
        &session,
        Request::PreviewStart {
            page,
            options: Options::default(),
        },
    )
    .await;
    let preview: Reference = serde_json::from_value(started["preview"].clone()).unwrap();
    assert_eq!(started["viewer"]["access"], "read_only");
    let before = latest(&session, preview, 0).await;
    assert_eq!(
        before.metadata["dpr"].as_f64(),
        Some(scale),
        "{}",
        before.metadata
    );
    corners(&before, session.journal_directory());
    let recording = successful(
        &session,
        Request::RecordingStart {
            page,
            options: crate::browser::recording::Options::default(),
        },
    )
    .await;
    let recording: Reference = serde_json::from_value(recording["recording"].clone()).unwrap();
    resize_preview(
        &session,
        page,
        preview,
        before.metadata["seq"].as_str().unwrap().parse().unwrap(),
    )
    .await;
    navigate_preview(&session, page, preview).await;
    finalize_recording(&session, preview, recording).await;
    recording_duration_limit(&session, page).await;
}

async fn resize_preview(session: &Session, page: Reference, preview: Reference, after: u64) {
    for viewport in [
        Viewport {
            width: 959,
            height: 599,
        },
        Viewport {
            width: 1600,
            height: 900,
        },
    ] {
        let resized = successful(session, Request::PageResize { page, viewport }).await;
        assert_eq!(resized["previewRecovery"]["succeeded"], true, "{resized}");
        let frame = latest(session, preview, after).await;
        assert_eq!(frame.metadata["logicalViewport"], json!(viewport));
        assert!(
            (i64::from(viewport.width) - frame.metadata["width"].as_i64().unwrap()).abs() <= 1,
            "Viewport {viewport:?}; frame {}; identity {}",
            frame.metadata,
            session.status()["identity"]
        );
        assert!(
            (i64::from(viewport.height) - frame.metadata["height"].as_i64().unwrap()).abs() <= 1,
            "Viewport {viewport:?}; frame {}; identity {}",
            frame.metadata,
            session.status()["identity"]
        );
        corners(&frame, session.journal_directory());
    }
}

async fn navigate_preview(session: &Session, page: Reference, preview: Reference) {
    let detached = successful(session, Request::PreviewStop { preview }).await;
    assert_eq!(detached["previewActive"], false);
    assert_eq!(detached["state"], "live");
    assert_eq!(detached["recordingConsumers"], 1);
    let attached = successful(
        session,
        Request::PreviewStart {
            page,
            options: Options::default(),
        },
    )
    .await;
    assert_eq!(attached["preview"], json!(preview));
    let old = latest(session, preview, 0).await;
    let navigation = successful(
        session,
        Request::PageNavigate {
            page,
            navigation: Navigation::SetContent {
                html: HTML.into(),
                wait_until: Load::Load,
            },
        },
    )
    .await;
    assert_eq!(navigation["previewRecovery"]["succeeded"], true);
    let fresh = latest(
        session,
        preview,
        old.metadata["seq"].as_str().unwrap().parse().unwrap(),
    )
    .await;
    assert_ne!(fresh.metadata["sourceEpoch"], old.metadata["sourceEpoch"]);
    assert_ne!(fresh.metadata["frame"], old.metadata["frame"]);
    corners(&fresh, session.journal_directory());
}

async fn finalize_recording(session: &Session, preview: Reference, recording: Reference) {
    let movie = successful(session, Request::RecordingStop { recording }).await;
    assert_eq!(movie["status"], "partial", "{movie}");
    assert_eq!(movie["verification"]["decodable"], true, "{movie}");
    assert!(
        movie["statistics"]["discontinuities"].as_u64().unwrap() > 0,
        "{movie}"
    );
    assert_eq!(
        movie["verification"]["decodedFrames"],
        movie["statistics"]["encodedFrames"]
    );
    assert_eq!(movie["artifact"]["mimeType"], "video/mp4");
    assert_eq!(
        successful(session, Request::RecordingStop { recording }).await,
        movie
    );
    successful(session, Request::PreviewStop { preview }).await;
    let stopped = successful(session, Request::PreviewStop { preview }).await;
    assert_eq!(stopped["state"], "stopped");
    assert!(
        session
            .0
            .host
            .previews
            .lock()
            .unwrap()
            .get(preview)
            .unwrap()
            .frames
            .borrow()
            .is_none()
    );
}

async fn recording_duration_limit(session: &Session, page: Reference) {
    let recording = successful(
        session,
        Request::RecordingStart {
            page,
            options: crate::browser::recording::Options {
                max_duration_ms: 300,
            },
        },
    )
    .await;
    let recording: Reference = serde_json::from_value(recording["recording"].clone()).unwrap();
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let changed = session.0.host.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            let result = session.status()["recordings"]
                .as_array()
                .unwrap()
                .iter()
                .find(|value| value["recording"] == json!(recording))
                .unwrap()
                .clone();
            if matches!(
                result["status"].as_str(),
                Some("complete" | "partial" | "failed")
            ) {
                assert_ne!(result["status"], "failed", "{result}");
                break;
            }
            changed.await;
        }
    })
    .await
    .unwrap();
    assert!(
        session
            .0
            .host
            .previews
            .lock()
            .unwrap()
            .entries
            .values()
            .all(|source| source.state == "stopped")
    );
}

pub(super) async fn latest(
    session: &Session,
    preview: Reference,
    after: u64,
) -> Arc<crate::media::Frame> {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let changed = session.0.host.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            let frame = {
                let sources = session.0.host.previews.lock().unwrap();
                let source = sources.get(preview).unwrap();
                assert!(source.failure.is_none(), "{}", source.snapshot());
                source.frames.borrow().clone()
            };
            if let Some(frame) = frame
                && frame.metadata["seq"]
                    .as_str()
                    .unwrap()
                    .parse::<u64>()
                    .unwrap()
                    > after
            {
                return frame;
            }
            changed.await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("Preview did not advance: {}", session.status()))
}

pub(super) fn corners(frame: &crate::media::Frame, evidence: &Path) {
    let header = frame.packet[..24].try_into().unwrap();
    let (metadata, _) = crate::media::lengths(&header).unwrap();
    let path = evidence.join(format!(
        "corner-{}-{}.jpg",
        frame.metadata["sourceEpoch"].as_str().unwrap(),
        frame.metadata["seq"].as_str().unwrap()
    ));
    crate::artifacts::write(&path, &frame.packet[24 + metadata..]).unwrap();
    let image = image::load_from_memory(&frame.packet[24 + metadata..])
        .unwrap()
        .to_rgb8();
    // Headed macOS window capture masks extreme bottom corner pixels. Require
    // each 40px fiducial's color and full bounding extent instead of treating
    // one OS-masked pixel as an absent viewport edge. A cropped viewport still
    // loses the corresponding fiducial and fails this independent image probe.
    for (name, x, y, kind) in [
        ("red", 0, 0, 0),
        ("blue", image.width() - 40, 0, 1),
        ("green", 0, image.height() - 40, 2),
        ("yellow", image.width() - 40, image.height() - 40, 3),
    ] {
        let mut count = 0;
        let (mut min_x, mut min_y, mut max_x, mut max_y) = (40, 40, 0, 0);
        for dy in 0..40 {
            for dx in 0..40 {
                let p = image.get_pixel(x + dx, y + dy).0;
                let matches = match kind {
                    0 => p[0] > 180 && p[1] < 80 && p[2] < 80,
                    1 => p[0] < 80 && p[1] < 80 && p[2] > 180,
                    2 => p[0] < 120 && p[1] > 150 && p[2] < 120,
                    _ => p[0] > 150 && p[1] > 150 && p[2] < 120,
                };
                if matches {
                    count += 1;
                    min_x = min_x.min(dx);
                    min_y = min_y.min(dy);
                    max_x = max_x.max(dx);
                    max_y = max_y.max(dy);
                }
            }
        }
        assert!(
            count >= 1000 && max_x.saturating_sub(min_x) >= 36 && max_y.saturating_sub(min_y) >= 36,
            "Missing or cropped {name} fiducial: pixels={count}, bounds={min_x},{min_y}..{max_x},{max_y}; frame {}; image {path:?}",
            frame.metadata
        );
    }
}
