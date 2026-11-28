//! Decoded capture pixels, private viewer grants and independent recording across owned engines.
use super::*;
use crate::browser::{
    contract::Viewport,
    evidence::Marker,
    page::{Clip, Scale, Screenshot},
};
use serde_json::json;
mod viewer;

macro_rules! cell {
    ($name:ident,$engine:ident,$mode:ident) => {
        #[tokio::test]
        async fn $name() {
            run_item(Engine::$engine, Mode::$mode, true).await;
        }
    };
}
cell!(item_three_capture_chrome_headless, Chrome, Headless);
cell!(item_three_capture_chrome_headed, Chrome, Headed);
cell!(item_three_capture_firefox_headless, Firefox, Headless);
cell!(item_three_capture_firefox_headed, Firefox, Headed);
cell!(item_three_capture_webkit_headless, Webkit, Headless);
cell!(item_three_capture_webkit_headed, Webkit, Headed);

const HTML: &str = r#"<!doctype html><style>body{margin:0;background:white;height:1400px}i{position:fixed;width:40px;height:40px;background:red;top:0;left:0}.tr{background:blue;left:auto;right:0}.bl{background:lime;top:auto;bottom:0}.br{background:yellow;left:auto;right:0;top:auto;bottom:0}button{position:absolute;left:80px;top:80px;width:120px;height:40px}#secret{position:absolute;left:80px;top:160px;width:120px;height:40px;background:black}</style><i></i><i class=tr></i><i class=bl></i><i class=br></i><h1 style='position:absolute;top:240px'>Acme capture</h1><button onclick="this.textContent='Pressed'">Press</button><div id=secret>Private text</div><output style='position:fixed;left:240px;top:80px'>0</output><script>let n=0;function paint(){document.querySelector('output').textContent=++n;requestAnimationFrame(paint)}paint();document.addEventListener('keydown',e=>document.body.dataset.key=e.key);</script>"#;

async fn shot(session: &Session, page: Reference, capture: Screenshot) -> Value {
    successful(session, Request::PageScreenshot { page, capture }).await
}
fn image(record: &Value) -> image::RgbaImage {
    image::open(record["attachment"]["path"].as_str().unwrap())
        .unwrap()
        .to_rgba8()
}
fn pixel(image: &image::RgbaImage, x: u32, y: u32, color: [u8; 3]) {
    assert_eq!(&image.get_pixel(x, y).0[..3], &color, "pixel {x},{y}");
}
async fn compare(session: &Session, a: &Value, b: &Value, limit: f64) -> Value {
    successful(
        session,
        Request::CaptureCompare {
            baseline: serde_json::from_value(a["captureId"].clone()).unwrap(),
            actual: serde_json::from_value(b["captureId"].clone()).unwrap(),
            threshold: 0,
            max_changed_ratio: limit,
        },
    )
    .await
}
pub(super) async fn exercise(session: &Session, engine: Engine, mode: Mode) {
    let startup = settled(session, session.startup()).await;
    assert_eq!(startup.outcome, Outcome::Succeeded, "{startup:?}");
    let page: Reference = serde_json::from_value(startup.result.unwrap()["page"].clone()).unwrap();
    successful(
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
    successful(
        session,
        Request::PageResize {
            page,
            viewport: Viewport {
                width: 959,
                height: 599,
            },
        },
    )
    .await;
    let viewport = shot(session, page, Screenshot::default()).await;
    let pixels = image(&viewport);
    assert_eq!(pixels.dimensions(), (959, 599));
    for (x, y, color) in [
        (10, 10, [255, 0, 0]),
        (949, 10, [0, 0, 255]),
        (10, 589, [0, 255, 0]),
        (949, 589, [255, 255, 0]),
    ] {
        pixel(&pixels, x, y, color);
    }
    let device = shot(
        session,
        page,
        Screenshot {
            scale: Scale::Device,
            ..Screenshot::default()
        },
    )
    .await;
    assert_eq!(image(&device).dimensions(), (1918, 1198));
    let clip = shot(
        session,
        page,
        Screenshot {
            clip: Some(Clip {
                x: 80.,
                y: 160.,
                width: 120.,
                height: 40.,
            }),
            ..Screenshot::default()
        },
    )
    .await;
    pixel(&image(&clip), 110, 30, [0, 0, 0]);
    let masked = shot(
        session,
        page,
        Screenshot {
            clip: Some(Clip {
                x: 80.,
                y: 160.,
                width: 120.,
                height: 40.,
            }),
            masks: vec![css("#secret")],
            ..Screenshot::default()
        },
    )
    .await;
    pixel(&image(&masked), 110, 30, [255, 0, 255]);
    let diff = compare(session, &clip, &masked, 0.).await;
    assert_eq!(diff["matches"], false);
    assert_eq!(diff["changedPixels"], 4800);
    assert_eq!(
        compare(session, &masked, &masked, 0.).await["matches"],
        true
    );
    let element = shot(
        session,
        page,
        Screenshot {
            element: Some(css("#secret")),
            ..Screenshot::default()
        },
    )
    .await;
    assert_eq!(image(&element).dimensions(), (120, 40));
    let full = shot(
        session,
        page,
        Screenshot {
            full_page: true,
            ..Screenshot::default()
        },
    )
    .await;
    assert!(image(&full).height() >= 1400);
    let before = successful(
        session,
        Request::PageSnapshot {
            page,
            frame: None,
            locator: Some(css("button")),
        },
    )
    .await;
    successful(session, action(page, css("button"), Action::Click {})).await;
    let after = successful(
        session,
        Request::PageSnapshot {
            page,
            frame: None,
            locator: Some(css("button")),
        },
    )
    .await;
    assert_eq!(
        compare(session, &before, &after, 0.).await["matches"],
        false
    );
    let marked = successful(
        session,
        Request::PageAnnotate {
            page,
            markers: vec![Marker {
                locator: css("#secret"),
                label: "Acme mark".into(),
            }],
        },
    )
    .await;
    assert_eq!(marked["result"]["count"], 1);
    successful(
        session,
        Request::PageAnnotate {
            page,
            markers: vec![],
        },
    )
    .await;
    if engine == Engine::Chrome && mode == Mode::Headless {
        let pdf = successful(
            session,
            Request::PagePdf {
                page,
                options: Default::default(),
            },
        )
        .await;
        assert!(
            std::fs::read(pdf["attachment"]["path"].as_str().unwrap())
                .unwrap()
                .starts_with(b"%PDF-")
        );
    }
    viewer::exercise(session, page).await;
    dpr_one(session).await;
}

async fn dpr_one(session: &Session) {
    let context = successful(
        session,
        Request::ContextCreate {
            options: ContextOptions {
                viewport: Viewport {
                    width: 640,
                    height: 480,
                },
                device_scale_factor: 1.,
                ..ContextOptions::default()
            },
        },
    )
    .await;
    let context: Reference = serde_json::from_value(context["context"].clone()).unwrap();
    let page = successful(
        session,
        Request::PageCreate {
            context,
            select: false,
        },
    )
    .await;
    let page: Reference = serde_json::from_value(page["page"].clone()).unwrap();
    successful(
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
    let capture = shot(
        session,
        page,
        Screenshot {
            scale: Scale::Device,
            ..Screenshot::default()
        },
    )
    .await;
    assert_eq!(image(&capture).dimensions(), (640, 480));
    assert_eq!(capture["geometryAfter"]["dpr"], 1);
    let preview = successful(
        session,
        Request::PreviewStart {
            page,
            options: Default::default(),
        },
    )
    .await;
    assert_eq!(preview["metadata"]["dpr"], 1);
    assert_eq!(preview["metadata"]["width"], 640);
    let preview: Reference = serde_json::from_value(preview["preview"].clone()).unwrap();
    successful(session, Request::PreviewStop { preview }).await;
    successful(session, Request::ContextClose { context }).await;
}
