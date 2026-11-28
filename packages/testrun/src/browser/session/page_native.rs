//! Real page control/capture acceptance; owned data documents, no external URLs.
use super::*;
use crate::browser::{
    contract::{ContextOptions, Engine, Mode},
    page::{Action, Condition, Format, Load, Locator, Navigation, Read, Screenshot},
};

const HTML: &str = r#"<!doctype html><html><style>body{margin:0;min-height:1500px}main{padding:30px}#covered{position:relative}#cover{position:absolute;inset:0;z-index:3;background:#777}.edge{position:fixed;top:0;width:12px;height:12px;background:#f00}.right{right:0;background:#00f}</style><body><div class=edge></div><div class="edge right"></div><main>
<h1>Acme checkout</h1><label>Name<input id=name></label><label>Password<input type=password value="Acme-secret-sentinel"></label>
<button id=save onclick="this.dataset.count=String(Number(this.dataset.count||0)+1);document.querySelector('#result').textContent='Saved '+document.querySelector('#name').value;let toast=document.createElement('div');toast.setAttribute('role','alert');toast.textContent='Acme transient';document.body.append(toast);setTimeout(()=>toast.remove(),500)">Save</button>
<button>Duplicate</button><button>Duplicate</button><button disabled>Disabled</button>
<div id=covered><button id=blocked onclick="this.textContent='Activated'">Covered</button><div id=cover></div></div>
<label>Accept<input id=accept type=checkbox></label><label>Plan<select id=plan><option value=a>A</option><option value=b>B</option></select></label>
<div id=result role=status>Ready</div><div id=shadow></div><iframe id=child srcdoc="<button>Frame action</button>"></iframe>
<script>document.querySelector('#shadow').attachShadow({mode:'open'}).innerHTML='<button>Shadow action</button>';</script></main></body></html>"#;
fn role(name: &str) -> Locator {
    Locator::Role {
        role: "button".into(),
        name: Some(name.into()),
        exact: true,
    }
}
fn css(selector: &str) -> Locator {
    Locator::Css {
        selector: selector.into(),
    }
}
pub(super) async fn execute(session: &Session, request: Request, timeout: u64) -> Operation {
    let id = session.submit(Uuid::new_v4(), request, timeout).unwrap();
    let result = session
        .wait(id, Instant::now() + Duration::from_secs(45))
        .await
        .unwrap();
    assert!(result.settled, "{}", session.status());
    result
}
pub(super) async fn successful(session: &Session, request: Request) -> Value {
    let requested = format!("{request:?}");
    let outcome = execute(session, request, 30000).await;
    assert_eq!(
        outcome.outcome,
        Outcome::Succeeded,
        "{requested}: {outcome:?}; {:?}",
        session.private_failures()
    );
    outcome.result.unwrap()
}
fn read(page: Reference, locator: Option<Locator>, kind: Read) -> Request {
    Request::PageRead {
        page,
        frame: None,
        locator,
        read: kind,
        max_bytes: 8192,
    }
}
fn act(page: Reference, locator: Locator, action: Action) -> Request {
    Request::PageAction {
        page,
        frame: None,
        locator,
        action,
        force: false,
        postcondition: None,
    }
}

#[tokio::test]
async fn firefox_webkit_page_control_and_capture_in_both_modes() {
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
            let scenario = tokio::spawn(exercise(session.clone())).await;
            let id = session.disconnect(30000).unwrap();
            let cleanup = session
                .wait(id, Instant::now() + Duration::from_secs(35))
                .await
                .unwrap();
            assert_eq!(cleanup.outcome, Outcome::Succeeded, "{}", session.status());
            scenario.expect("Page scenario failed");
        }
    }
}

#[tokio::test]
async fn firefox_webkit_aria_masks_secure_values_without_mutating_inputs() {
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
            let scenario = tokio::spawn(secure_aria(session.clone())).await;
            let closed = session
                .wait(
                    session.disconnect(30000).unwrap(),
                    Instant::now() + Duration::from_secs(35),
                )
                .await
                .unwrap();
            assert_eq!(closed.outcome, Outcome::Succeeded, "{}", session.status());
            scenario.expect("Secure ARIA fixture failed");
        }
    }
}

async fn secure_aria(session: Session) {
    let startup = session
        .wait(session.startup(), Instant::now() + Duration::from_secs(45))
        .await
        .unwrap();
    assert_eq!(startup.outcome, Outcome::Succeeded, "{}", session.status());
    let page = serde_json::from_value(startup.result.unwrap()["page"].clone()).unwrap();
    successful(
        &session,
        Request::PageNavigate {
            page,
            navigation: Navigation::SetContent {
                html: crate::browser::reading::SECURE_FIXTURE.into(),
                wait_until: Load::Load,
            },
        },
    )
    .await;
    let aria = successful(&session, read(page, None, Read::Aria))
        .await
        .to_string();
    let form = successful(&session, read(page, None, Read::Form))
        .await
        .to_string();
    assert!(aria.contains("Visible ordinary text"));
    for secret in [
        "Acme-secret-sentinel",
        "Acme-role-secret",
        "Acme-autocomplete-secret",
    ] {
        assert!(!aria.contains(secret));
        assert!(!form.contains(secret));
    }
    let unchanged=successful(&session,Request::Evaluate {scope:page,expression:"() => document.querySelector('#secret').value === 'Acme-secret-sentinel' && document.querySelector('#ordinary').value === 'Visible ordinary text' && globalThis.inputEvents === 0".into(),is_function:Some(true),argument:crate::browser::evaluation::EvaluationValue::default(),return_handle:false}).await;
    assert_eq!(unchanged["value"]["value"], true);
    let snapshot = successful(&session, read(page, Some(css("#ordinary")), Read::Snapshot)).await;
    let locator: Locator =
        serde_json::from_value(snapshot["result"]["value"][0]["locator"].clone()).unwrap();
    successful(
        &session,
        act(
            page,
            locator.clone(),
            Action::Fill {
                value: "Acme reference edit".into(),
            },
        ),
    )
    .await;
    let value = successful(&session, read(page, Some(locator.clone()), Read::State)).await;
    assert_eq!(value["result"]["value"]["value"], "Acme reference edit");
    successful(
        &session,
        Request::PageNavigate {
            page,
            navigation: Navigation::SetContent {
                html: crate::browser::reading::SECURE_FIXTURE.into(),
                wait_until: Load::Load,
            },
        },
    )
    .await;
    let stale = execute(
        &session,
        act(
            page,
            locator,
            Action::Fill {
                value: "Must not dispatch".into(),
            },
        ),
        30000,
    )
    .await;
    assert_eq!(stale.code.as_deref(), Some("STALE_REFERENCE"));
}
async fn exercise(session: Session) {
    let startup = session
        .wait(session.startup(), Instant::now() + Duration::from_secs(45))
        .await
        .unwrap();
    assert_eq!(startup.outcome, Outcome::Succeeded, "{}", session.status());
    let page: Reference = serde_json::from_value(startup.result.unwrap()["page"].clone()).unwrap();
    evaluation_values(&session, page).await;
    native_input(&session, page).await;
    form_controls(&session, page).await;
    let frame = frame_actions(&session, page).await;
    capture_viewport_and_element(&session, page).await;
    capture_masks_and_clip(&session, page).await;
    messages_and_navigation(&session, page, frame).await;
    capture_scaled_odd_viewport(&session).await;
    compound_failure_recovery(&session).await;
}

async fn form_controls(session: &Session, page: Reference) {
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
    let form = successful(session, read(page, None, Read::Form)).await;
    assert!(!form.to_string().contains("Acme-secret-sentinel"));
    successful(
        session,
        act(
            page,
            Locator::Label {
                text: "Name".into(),
                exact: true,
            },
            Action::Fill {
                value: "Zoë 東京".into(),
            },
        ),
    )
    .await;
    successful(
        session,
        act(page, css("#accept"), Action::Check { checked: true }),
    )
    .await;
    successful(
        session,
        act(
            page,
            css("#plan"),
            Action::Select {
                values: vec!["b".into()],
            },
        ),
    )
    .await;
    successful(
        session,
        Request::PageAction {
            page,
            frame: None,
            locator: role("Save"),
            action: Action::Click {},
            force: false,
            postcondition: Some(Condition::Text {
                value: "Save".into(),
                contains: false,
            }),
        },
    )
    .await;
    let saved = successful(session, read(page, Some(css("#result")), Read::Text)).await;
    assert_eq!(saved["result"]["value"], "Saved Zoë 東京");
    successful(session, act(page, role("Shadow action"), Action::Click {})).await;
}

async fn frame_actions(session: &Session, page: Reference) -> Reference {
    let frame = successful(
        session,
        Request::FrameEnter {
            page,
            frame: None,
            locator: css("#child"),
        },
    )
    .await;
    let frame: Reference = serde_json::from_value(frame["frame"].clone()).unwrap();
    successful(
        session,
        Request::PageAction {
            page,
            frame: Some(frame),
            locator: role("Frame action"),
            action: Action::Click {},
            force: false,
            postcondition: None,
        },
    )
    .await;
    let forced = successful(
        session,
        Request::PageAction {
            page,
            frame: None,
            locator: role("Covered"),
            action: Action::Click {},
            force: true,
            postcondition: None,
        },
    )
    .await;
    assert_eq!(forced["mode"], "forced");
    assert_eq!(
        successful(session, read(page, Some(css("#blocked")), Read::Text)).await["result"]["value"],
        "Covered"
    );
    successful(
        session,
        Request::PageAction {
            page,
            frame: None,
            locator: css("#blocked"),
            action: Action::Dispatch {
                event: "click".into(),
            },
            force: false,
            postcondition: None,
        },
    )
    .await;
    frame
}

async fn capture_viewport_and_element(session: &Session, page: Reference) {
    let screenshot = successful(
        session,
        Request::PageScreenshot {
            page,
            capture: Screenshot::default(),
        },
    )
    .await;
    assert_eq!(screenshot["width"], 1280);
    assert_eq!(screenshot["height"], 720);
    assert!(screenshot["geometryStable"].as_bool().unwrap());
    assert_eq!(screenshot["coordinateInputAvailable"], true);
    assert!(screenshot["inputObservation"].is_object());
    let image = image::open(screenshot["attachment"]["path"].as_str().unwrap())
        .unwrap()
        .to_rgb8();
    assert_eq!(image.get_pixel(2, 2).0, [255, 0, 0]);
    assert_eq!(image.get_pixel(1277, 2).0, [0, 0, 255]);
    let full = successful(
        session,
        Request::PageScreenshot {
            page,
            capture: Screenshot {
                full_page: true,
                ..Screenshot::default()
            },
        },
    )
    .await;
    assert!(full["height"].as_u64().unwrap() >= 1500);
    assert_eq!(full["coordinateInputAvailable"], false);
    let element = successful(
        session,
        Request::PageScreenshot {
            page,
            capture: Screenshot {
                element: Some(css("#save")),
                ..Screenshot::default()
            },
        },
    )
    .await;
    assert!(element["width"].as_u64().unwrap() < 500);
}

async fn capture_masks_and_clip(session: &Session, page: Reference) {
    let masked = successful(
        session,
        Request::PageScreenshot {
            page,
            capture: Screenshot {
                masks: vec![css("#name")],
                format: Format::Jpeg,
                quality: Some(80),
                ..Screenshot::default()
            },
        },
    )
    .await;
    assert_eq!(masked["attachment"]["mimeType"], "image/jpeg");
    let state = successful(session, read(page, Some(css("#name")), Read::State)).await;
    let rect = &state["result"]["value"]["box"];
    let x = (rect["x"].as_f64().unwrap() + rect["width"].as_f64().unwrap() / 2.0) as u32;
    let y = (rect["y"].as_f64().unwrap() + rect["height"].as_f64().unwrap() / 2.0) as u32;
    let image = image::open(masked["attachment"]["path"].as_str().unwrap())
        .unwrap()
        .to_rgb8();
    let pixel = image.get_pixel(x, y).0;
    assert!(
        pixel[0] > 230 && pixel[1] < 25 && pixel[2] > 230,
        "Mask pixel {pixel:?}"
    );
    let clipped = successful(
        session,
        Request::PageScreenshot {
            page,
            capture: Screenshot {
                clip: Some(crate::browser::page::Clip {
                    x: 0.0,
                    y: 0.0,
                    width: 200.0,
                    height: 100.0,
                }),
                ..Screenshot::default()
            },
        },
    )
    .await;
    assert_eq!(clipped["width"], 200);
    assert_eq!(clipped["height"], 100);
}

async fn messages_and_navigation(session: &Session, page: Reference, frame: Reference) {
    let messages = successful(session, read(page, None, Read::Messages)).await;
    assert!(
        messages.to_string().contains("Acme transient"),
        "{messages}"
    );
    assert!(
        !successful(session, read(page, Some(css("body")), Read::Text))
            .await
            .to_string()
            .contains("Acme transient"),
        "Toast should be retained after its DOM node disappears"
    );
    let outcome = execute(
        session,
        Request::PageAction {
            page,
            frame: None,
            locator: role("Save"),
            action: Action::Click {},
            force: false,
            postcondition: Some(Condition::Text {
                value: "Never present".into(),
                contains: false,
            }),
        },
        500,
    )
    .await;
    assert_ne!(outcome.outcome, Outcome::Succeeded);
    assert_eq!(outcome.result.unwrap()["dispatchCompleted"], true);
    let old_frame = frame;
    successful(
        session,
        Request::PageNavigate {
            page,
            navigation: Navigation::SetContent {
                html: "<h1>Replacement</h1>".into(),
                wait_until: Load::Load,
            },
        },
    )
    .await;
    let stale = execute(
        session,
        Request::PageRead {
            page,
            frame: Some(old_frame),
            locator: None,
            read: Read::Text,
            max_bytes: 8192,
        },
        1000,
    )
    .await;
    assert_eq!(stale.code.as_deref(), Some("STALE_REFERENCE"));
}

async fn compound_failure_recovery(session: &Session) {
    let created = successful(
        session,
        Request::ContextCreate {
            options: ContextOptions::default(),
        },
    )
    .await;
    let context = serde_json::from_value(created["context"].clone()).unwrap();
    for (locator, cause, timeout) in [
        (role("Duplicate"), "AMBIGUOUS_LOCATOR", 1000),
        (role("Covered"), "OUTCOME_UNKNOWN", 600),
    ] {
        let created = successful(
            session,
            Request::PageCreate {
                context,
                select: false,
            },
        )
        .await;
        let page = serde_json::from_value(created["page"].clone()).unwrap();
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
        let outcome = execute(session, act(page, locator, Action::Click {}), timeout).await;
        assert_eq!(outcome.outcome, Outcome::Unknown, "{outcome:?}");
        let evidence = outcome.result.unwrap();
        assert_eq!(evidence["cause"], cause);
        assert_eq!(evidence["cleanup"]["requiresOwnedPageClose"], true);
        let conflict = execute(session, act(page, role("Save"), Action::Click {}), 1000).await;
        assert_eq!(conflict.code.as_deref(), Some("INPUT_STATE_CONFLICT"));
        let released = successful(session, Request::InputRelease {}).await;
        assert!(
            released["pages"]
                .as_array()
                .unwrap()
                .iter()
                .any(|entry| entry["page"] == json!(page)
                    && entry["cleanup"]["requiresOwnedPageClose"] == true)
        );
        successful(session, Request::PageClose { page }).await;
    }
    successful(session, Request::ContextClose { context }).await;
}

async fn input_observation(session: &Session, page: Reference) -> Reference {
    for _ in 0..10 {
        let before = successful(session, read(page, None, Read::Geometry)).await;
        tokio::time::sleep(Duration::from_millis(20)).await;
        let after = successful(session, read(page, None, Read::Geometry)).await;
        if before["result"] == after["result"] {
            return serde_json::from_value(after["inputObservation"].clone()).unwrap();
        }
    }
    panic!("Owned fixture viewport did not settle");
}
async fn native_input(session: &Session, page: Reference) {
    keyboard_input(session, page).await;
    let observation = pointer_input(session, page).await;
    resized_input(session, page, observation).await;
    touch_input(session).await;
    successful(
        session,
        Request::PageResize {
            page,
            viewport: contract::Viewport {
                width: 1280,
                height: 720,
            },
        },
    )
    .await;
}

async fn keyboard_input(session: &Session, page: Reference) {
    use crate::browser::input::Action as Input;
    const INPUT_HTML: &str = r#"<html><body style="margin:0;min-height:2000px"><input id=raw style="position:fixed;left:20px;top:50px;width:300px;height:40px"><button style="position:fixed;left:20px;top:120px;width:200px;height:40px" onclick="document.querySelector('#out').textContent='native '+event.isTrusted">Native</button><output id=out style="position:fixed;top:200px">Ready</output></body></html>"#;
    successful(
        session,
        Request::PageNavigate {
            page,
            navigation: Navigation::SetContent {
                html: INPUT_HTML.into(),
                wait_until: Load::Load,
            },
        },
    )
    .await;
    successful(session, act(page, css("#raw"), Action::Focus {})).await;
    let observation = input_observation(session, page).await;
    let result = successful(
        session,
        Request::PageInput {
            page,
            observation: Some(observation),
            actions: vec![Input::Type {
                text: "aAé🙂".into(),
                delay_ms: 1,
            }],
        },
    )
    .await;
    assert_eq!(result["held"]["possiblyHeldKeys"], 0);
    let value = successful(session, read(page, Some(css("#raw")), Read::State)).await;
    assert_eq!(value["result"]["value"]["value"], "aAé🙂");
    successful(
        session,
        Request::PageInput {
            page,
            observation: Some(observation),
            actions: vec![Input::KeyDown {
                key: "Shift".into(),
            }],
        },
    )
    .await;
    let conflict = execute(session, act(page, css("#raw"), Action::Focus {}), 5000).await;
    assert_eq!(conflict.code.as_deref(), Some("INPUT_STATE_CONFLICT"));
    successful(
        session,
        Request::PageInput {
            page,
            observation: None,
            actions: vec![Input::KeyUp {
                key: "ShiftLeft".into(),
            }],
        },
    )
    .await;
}

async fn pointer_input(session: &Session, page: Reference) -> Reference {
    use crate::browser::input::{Action as Input, Button};
    let observation = input_observation(session, page).await;
    successful(
        session,
        Request::PageInput {
            page,
            observation: Some(observation),
            actions: vec![Input::MouseClick {
                x: 60.0,
                y: 140.0,
                button: Button::Left,
                click_count: 1,
                delay_ms: 0,
            }],
        },
    )
    .await;
    assert_eq!(
        successful(session, read(page, Some(css("#out")), Read::Text)).await["result"]["value"],
        "native true"
    );
    let touch = execute(
        session,
        Request::PageInput {
            page,
            observation: Some(observation),
            actions: vec![Input::TouchTap { x: 60.0, y: 140.0 }],
        },
        5000,
    )
    .await;
    assert_eq!(touch.code.as_deref(), Some("TOUCH_CONTEXT_REQUIRED"));
    successful(
        session,
        Request::PageInput {
            page,
            observation: Some(observation),
            actions: vec![
                Input::MouseDown {
                    x: 60.0,
                    y: 140.0,
                    button: Button::Left,
                },
                Input::MouseMove {
                    x: 80.0,
                    y: 140.0,
                    steps: 2,
                },
                Input::MouseUp {
                    button: Button::Left,
                },
            ],
        },
    )
    .await;
    observation
}

async fn resized_input(session: &Session, page: Reference, observation: Reference) {
    use crate::browser::input::Action as Input;
    successful(
        session,
        Request::PageResize {
            page,
            viewport: contract::Viewport {
                width: 801,
                height: 601,
            },
        },
    )
    .await;
    let stale = execute(
        session,
        Request::PageInput {
            page,
            observation: Some(observation),
            actions: vec![Input::InsertText {
                text: "unexpected".into(),
            }],
        },
        5000,
    )
    .await;
    assert_eq!(stale.code.as_deref(), Some("INPUT_OBSERVATION_STALE"));
    let observation = input_observation(session, page).await;
    successful(
        session,
        Request::PageInput {
            page,
            observation: Some(observation),
            actions: vec![Input::MouseWheel {
                x: 400.0,
                y: 400.0,
                delta_x: 0.0,
                delta_y: 100.0,
            }],
        },
    )
    .await;
    successful(session, Request::InputRelease {}).await;
}

async fn touch_input(session: &Session) {
    use crate::browser::input::Action as Input;
    let context: Reference = serde_json::from_value(
        successful(
            session,
            Request::ContextCreate {
                options: ContextOptions {
                    has_touch: true,
                    ..ContextOptions::default()
                },
            },
        )
        .await["context"]
            .clone(),
    )
    .unwrap();
    let touch_page: Reference = serde_json::from_value(
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
    .unwrap();
    successful(session,Request::PageNavigate{page:touch_page,navigation:Navigation::SetContent{html:"<button style='width:200px;height:100px' ontouchstart=\"this.textContent='touch '+event.isTrusted\">Ready</button>".into(),wait_until:Load::Load}}).await;
    let observation = input_observation(session, touch_page).await;
    successful(
        session,
        Request::PageInput {
            page: touch_page,
            observation: Some(observation),
            actions: vec![Input::TouchTap { x: 60.0, y: 50.0 }],
        },
    )
    .await;
    assert_eq!(
        successful(session, read(touch_page, Some(css("button")), Read::Text)).await["result"]["value"],
        "touch true"
    );
    successful(session, Request::ContextClose { context }).await;
}

async fn evaluation_values(session: &Session, page: Reference) {
    use crate::browser::evaluation::EvaluationValue;
    let evaluated = successful(session, Request::Evaluate {
        scope:page,
        expression:"() => { const value = {missing:undefined, nan:NaN, infinity:Infinity, negative:-Infinity, zero:-0, big:90071992547409931234n, date:new Date('2026-10-25T00:00:00.000Z'), url:new URL('https://example.com/'), pattern:/Acme/gi, bytes:new Uint16Array([1,2])}; value.self = value; return value; }".into(),
        is_function:Some(true), argument:EvaluationValue::default(), return_handle:false,
    }).await;
    let argument: EvaluationValue = serde_json::from_value(evaluated["value"].clone()).unwrap();
    let verified = successful(session, Request::Evaluate {
        scope:page,
        expression:"value => value.self === value && value.missing === undefined && Number.isNaN(value.nan) && value.infinity === Infinity && value.negative === -Infinity && Object.is(value.zero,-0) && value.big === 90071992547409931234n && value.date.toISOString() === '2026-10-25T00:00:00.000Z' && value.url.href === 'https://example.com/' && value.pattern.source === 'Acme' && value.pattern.flags === 'gi' && value.bytes instanceof Uint16Array && value.bytes[0] === 1 && value.bytes[1] === 2".into(),
        is_function:Some(true), argument, return_handle:false,
    }).await;
    assert_eq!(verified["value"], json!({"kind":"json","value":true}));
    let created = successful(
        session,
        Request::Evaluate {
            scope: page,
            expression: "() => ({name:'Acme',count:1})".into(),
            is_function: Some(true),
            argument: EvaluationValue::default(),
            return_handle: true,
        },
    )
    .await;
    let handle: Reference = serde_json::from_value(created["handle"].clone()).unwrap();
    let verified = successful(
        session,
        Request::Evaluate {
            scope: page,
            expression: "value => value.name".into(),
            is_function: Some(true),
            argument: EvaluationValue::Handle { handle },
            return_handle: false,
        },
    )
    .await;
    assert_eq!(verified["value"]["value"], "Acme");
    let properties = successful(
        session,
        Request::HandleProperties {
            handle,
            query: crate::browser::evaluation::PropertyQuery {
                offset: 0,
                limit: 1,
            },
        },
    )
    .await;
    assert_eq!(properties["properties"]["names"], json!(["name"]));
    assert_eq!(properties["properties"]["hasMore"], true);
    assert_eq!(properties["properties"]["nextOffset"], 1);
    let more = successful(
        session,
        Request::HandleProperties {
            handle,
            query: crate::browser::evaluation::PropertyQuery {
                offset: 1,
                limit: 1,
            },
        },
    )
    .await;
    assert_eq!(more["properties"]["names"], json!(["count"]));
    assert_eq!(more["properties"]["hasMore"], false);
    let property = successful(
        session,
        Request::HandleProperty {
            handle,
            name: "name".into(),
        },
    )
    .await;
    let property: Reference = serde_json::from_value(property["handle"].clone()).unwrap();
    successful(session, Request::HandleDispose { handle }).await;
    let read = successful(session, Request::HandleRead { handle: property }).await;
    assert_eq!(read["value"]["value"], "Acme");
    successful(session, Request::HandleDispose { handle: property }).await;
    worker_execution(session, page).await;
    diagnostic_messages(session, page).await;
}

async fn worker_execution(session: &Session, page: Reference) {
    use crate::browser::evaluation::EvaluationValue;
    successful(session,Request::Evaluate {scope:page,expression:r#"() => new Promise(resolve => {
        const url=URL.createObjectURL(new Blob(["self.identity={name:'Acme worker'};self.postMessage('ready')"],{type:'text/javascript'}));
        const worker=new Worker(url);globalThis.acmeWorker=worker;
        worker.onmessage=()=>{URL.revokeObjectURL(url);resolve(true);};
    })"#.into(),is_function:Some(true),argument:Default::default(),return_handle:false}).await;
    let listed = successful(session, Request::WorkerList { page }).await;
    assert_eq!(listed["workers"].as_array().unwrap().len(), 1, "{listed}");
    let worker: Reference = serde_json::from_value(listed["workers"][0]["worker"].clone()).unwrap();
    let retained = successful(
        session,
        Request::Evaluate {
            scope: worker,
            expression: "() => self.identity".into(),
            is_function: Some(true),
            argument: Default::default(),
            return_handle: true,
        },
    )
    .await;
    assert_eq!(retained["worker"], json!(worker));
    assert!(retained["frame"].is_null());
    let handle: Reference = serde_json::from_value(retained["handle"].clone()).unwrap();
    let read = successful(
        session,
        Request::Evaluate {
            scope: worker,
            expression: "value => value.name".into(),
            is_function: Some(true),
            argument: EvaluationValue::Handle { handle },
            return_handle: false,
        },
    )
    .await;
    assert_eq!(read["value"]["value"], "Acme worker");
    let foreign = execute(
        session,
        Request::Evaluate {
            scope: page,
            expression: "value => !!value".into(),
            is_function: Some(true),
            argument: EvaluationValue::Handle { handle },
            return_handle: false,
        },
        30000,
    )
    .await;
    assert_eq!(foreign.code.as_deref(), Some("FOREIGN_HANDLE"));
    successful(session,Request::Evaluate {scope:page,expression:"() => new Promise(resolve => { const frame=document.createElement('iframe'); frame.onload=()=>resolve(true); frame.srcdoc='<p>Acme frame</p>'; document.body.append(frame); })".into(),is_function:Some(true),argument:Default::default(),return_handle:false}).await;
    let retained = successful(session, Request::HandleRead { handle }).await;
    assert_eq!(retained["worker"], json!(worker));
    assert_eq!(
        retained["value"]["entries"][0]["value"]["value"],
        "Acme worker"
    );
    let listed = successful(session, Request::WorkerList { page }).await;
    let current: Reference =
        serde_json::from_value(listed["workers"][0]["worker"].clone()).unwrap();
    assert_eq!(current, worker);
    let read = successful(
        session,
        Request::Evaluate {
            scope: current,
            expression: "() => self.identity.name".into(),
            is_function: Some(true),
            argument: Default::default(),
            return_handle: false,
        },
    )
    .await;
    assert_eq!(read["value"]["value"], "Acme worker");
    successful(
        session,
        Request::Evaluate {
            scope: page,
            expression: "() => globalThis.acmeWorker.terminate()".into(),
            is_function: Some(true),
            argument: Default::default(),
            return_handle: false,
        },
    )
    .await;
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let changed = session.0.host.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            let workers = successful(session, Request::WorkerList { page }).await;
            if workers["workers"].as_array().unwrap().is_empty() {
                break;
            }
            changed.await;
        }
    })
    .await
    .unwrap();
}

async fn diagnostic_messages(session: &Session, page: Reference) {
    use crate::browser::{
        diagnostics::{DiagnosticKind, Query},
        evaluation::EvaluationValue,
    };
    let context: Reference =
        serde_json::from_value(session.status()["selectedContext"].clone()).unwrap();
    successful(session, Request::DiagnosticsStart { context }).await;
    successful(
        session,
        Request::Evaluate {
            scope: page,
            expression:
                "() => { console.error('Acme diagnostic sentinel', {name:'Acme'}); return true; }"
                    .into(),
            is_function: Some(true),
            argument: EvaluationValue::default(),
            return_handle: false,
        },
    )
    .await;
    let metadata = successful(
        session,
        Request::DiagnosticsRead {
            context,
            query: Query::default(),
        },
    )
    .await;
    assert!(!metadata.to_string().contains("Acme diagnostic sentinel"));
    let messages = successful(
        session,
        Request::DiagnosticsRead {
            context,
            query: Query {
                page: Some(page),
                kind: Some(DiagnosticKind::Console),
                include_text: true,
                ..Default::default()
            },
        },
    )
    .await;
    assert!(
        messages.to_string().contains("Acme diagnostic sentinel"),
        "{messages}"
    );
    let stopped = successful(session, Request::DiagnosticsStop { context }).await;
    assert_eq!(stopped["active"], false);
}

async fn capture_scaled_odd_viewport(session: &Session) {
    let created = successful(
        session,
        Request::ContextCreate {
            options: ContextOptions {
                viewport: crate::browser::contract::Viewport {
                    width: 513,
                    height: 385,
                },
                device_scale_factor: 2.0,
                accept_downloads: false,
                has_touch: false,
                ..ContextOptions::default()
            },
        },
    )
    .await;
    let context: Reference = serde_json::from_value(created["context"].clone()).unwrap();
    let created = successful(
        session,
        Request::PageCreate {
            context,
            select: false,
        },
    )
    .await;
    let page: Reference = serde_json::from_value(created["page"].clone()).unwrap();
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
    let css = successful(
        session,
        Request::PageScreenshot {
            page,
            capture: Screenshot::default(),
        },
    )
    .await;
    assert_eq!(css["width"], 513);
    assert_eq!(css["height"], 385);
    let image = image::open(css["attachment"]["path"].as_str().unwrap())
        .unwrap()
        .to_rgb8();
    assert_eq!(image.get_pixel(2, 2).0, [255, 0, 0]);
    assert_eq!(image.get_pixel(510, 2).0, [0, 0, 255]);
    let device = successful(
        session,
        Request::PageScreenshot {
            page,
            capture: Screenshot {
                scale: crate::browser::page::Scale::Device,
                ..Screenshot::default()
            },
        },
    )
    .await;
    assert_eq!(device["width"], 1026);
    assert_eq!(device["height"], 770);
    let image = image::open(device["attachment"]["path"].as_str().unwrap())
        .unwrap()
        .to_rgb8();
    assert_eq!(image.get_pixel(2, 2).0, [255, 0, 0]);
    assert_eq!(image.get_pixel(1023, 2).0, [0, 0, 255]);
    successful(session, Request::ContextClose { context }).await;
}
