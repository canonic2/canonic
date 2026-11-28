//! Identical read/control, callback, frame and event journeys across engines.
use super::*;

pub(super) async fn reading_and_editing(session: &Session, page: Reference) {
    successful(
        session,
        action(
            page,
            css("#drag"),
            Action::Drag {
                target: css("#drop"),
            },
        ),
    )
    .await;
    wait_text(session, page, "#drop", "Acme drag").await;
    let snapshot = successful(session, read(page, Some(css("#name")), Read::Snapshot)).await;
    let locator: Locator =
        serde_json::from_value(snapshot["result"]["value"][0]["locator"].clone()).unwrap();
    successful(
        session,
        action(
            page,
            locator.clone(),
            Action::Fill {
                value: "Acme".into(),
            },
        ),
    )
    .await;
    successful(
        session,
        action(page, locator, Action::Press { key: "End".into() }),
    )
    .await;
    let geometry = successful(session, read(page, None, Read::Geometry)).await;
    let observation = serde_json::from_value(geometry["inputObservation"].clone()).unwrap();
    successful(
        session,
        Request::PageInput {
            page,
            observation: Some(observation),
            actions: vec![
                super::super::input::Action::KeyDown {
                    key: "Shift".into(),
                },
                super::super::input::Action::KeyPress {
                    key: "KeyA".into(),
                    delay_ms: 0,
                },
                super::super::input::Action::KeyUp {
                    key: "Shift".into(),
                },
            ],
        },
    )
    .await;
    let state = successful(session, read(page, Some(css("#name")), Read::State)).await;
    assert_eq!(state["result"]["value"]["value"], "AcmeA", "{state}");
    successful(
        session,
        action(page, css("#check"), Action::Check { checked: true }),
    )
    .await;
    successful(
        session,
        action(
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
            locator: css("#save"),
            action: Action::Click {},
            force: false,
            postcondition: Some(Condition::Text {
                value: "Saved".into(),
                contains: false,
            }),
        },
    )
    .await;
    wait_text(session, page, "#result", "Saved AcmeA").await;
    let form = successful(session, read(page, None, Read::Form)).await;
    assert!(!form.to_string().contains("Acme-secret-sentinel"));
    let style = successful(session, read(page, Some(css("#name")), Read::Styles)).await;
    assert!(style["result"]["value"]["display"].is_string());
    let selected = successful(
        session,
        Request::PageStyles {
            page,
            frame: None,
            locator: css("#name"),
            properties: vec!["box-sizing".into(), "margin-left".into()],
            max_bytes: 1024,
        },
    )
    .await;
    assert!(selected["result"]["value"]["box-sizing"].is_string());
    assert_eq!(selected["result"]["value"].as_object().unwrap().len(), 2);
    let readable = successful(session, read(page, None, Read::Readable)).await;
    assert!(
        readable["result"]["value"]["text"]
            .as_str()
            .unwrap()
            .contains("Acme checkout")
    );
    let duplicate = session
        .submit(
            Uuid::new_v4(),
            read(
                page,
                Some(Locator::Role {
                    role: "button".into(),
                    name: Some("Duplicate".into()),
                    exact: true,
                }),
                Read::Text,
            ),
            30000,
        )
        .unwrap();
    assert_eq!(
        settled(session, duplicate).await.code.as_deref(),
        Some("AMBIGUOUS_LOCATOR")
    );
}

pub(super) async fn dialog_and_waits(session: &Session, page: Reference) {
    let waiting = session
        .submit(
            Uuid::new_v4(),
            Request::PageWait {
                page,
                frame: None,
                locator: Some(css("#result")),
                condition: Condition::Text {
                    value: "Acme answer".into(),
                    contains: false,
                },
            },
            30000,
        )
        .unwrap();
    let click = session
        .submit(
            Uuid::new_v4(),
            action(page, css("#prompt"), Action::Click {}),
            30000,
        )
        .unwrap();
    let callback = tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            let status = session.status();
            let reference = status["callbacks"]
                .as_array()
                .and_then(|items| items.first())
                .map(|value| value["reference"].clone())
                .or_else(|| {
                    status["resources"]
                        .as_array()
                        .and_then(|items| items.iter().find(|value| value["kind"] == "dialog"))
                        .map(|value| value["reference"].clone())
                });
            if let Some(reference) = reference {
                break serde_json::from_value::<Reference>(reference).unwrap();
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap_or_else(|error| {
        panic!(
            "Dialog: {error:?}; click={:?}; {}",
            session.operation(click),
            session.status()
        )
    });
    successful(
        session,
        Request::CallbackResolve {
            callback,
            response: super::super::contract::CallbackResponse::Dialog {
                accept: true,
                prompt_text: Some("Acme answer".into()),
            },
        },
    )
    .await;
    assert_eq!(settled(session, click).await.outcome, Outcome::Succeeded);
    assert_eq!(settled(session, waiting).await.outcome, Outcome::Succeeded);
    let pending = session
        .submit(
            Uuid::new_v4(),
            Request::PageWait {
                page,
                frame: None,
                locator: Some(css("#result")),
                condition: Condition::Text {
                    value: "Never".into(),
                    contains: false,
                },
            },
            30000,
        )
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while session.operation(pending).unwrap().outcome != Outcome::Running {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    session.cancel(pending).await.unwrap();
    assert_eq!(settled(session, pending).await.outcome, Outcome::Cancelled);
    assert_eq!(session.status()["phase"], "ready");
}

pub(super) async fn frames_and_shadow(session: &Session, page: Reference) {
    successful(
        session,
        action(
            page,
            Locator::Role {
                role: "button".into(),
                name: Some("Shadow button".into()),
                exact: true,
            },
            Action::Click {},
        ),
    )
    .await;
    let shadow = successful(
        session,
        read(
            page,
            Some(Locator::Role {
                role: "button".into(),
                name: Some("Shadow pressed".into()),
                exact: true,
            }),
            Read::Text,
        ),
    )
    .await;
    assert_eq!(shadow["result"]["value"], "Shadow pressed");
    for selector in ["#same", "#cross"] {
        let entered = successful(
            session,
            Request::FrameEnter {
                page,
                frame: None,
                locator: css(selector),
            },
        )
        .await;
        let frame = serde_json::from_value(entered["frame"].clone()).unwrap();
        successful(
            session,
            Request::PageAction {
                page,
                frame: Some(frame),
                locator: css("button"),
                action: Action::Click {},
                force: false,
                postcondition: Some(Condition::Text {
                    value: "Pressed".into(),
                    contains: false,
                }),
            },
        )
        .await;
        if selector == "#same" {
            successful(session, action(page, css("#hideframe"), Action::Click {})).await;
            successful(
                session,
                Request::PageWait {
                    page,
                    frame: Some(frame),
                    locator: Some(css("button")),
                    condition: Condition::Hidden {},
                },
            )
            .await;
        }
    }
}

pub(super) async fn popup_and_events(session: &Session, page: Reference) {
    let context: Reference =
        serde_json::from_value(session.status()["selectedContext"].clone()).unwrap();
    let before = successful(
        session,
        Request::EventsRead {
            context,
            cursor: None,
            limit: 100,
        },
    )
    .await;
    let after = before["latestSequence"].as_u64().unwrap();
    let waiting = session
        .submit(
            Uuid::new_v4(),
            Request::EventsWait {
                context,
                after,
                conditions: vec![super::super::activity::Condition {
                    event: super::super::activity::Event::PageCreated,
                    page: None,
                }],
            },
            30000,
        )
        .unwrap();
    successful(session, action(page, css("#popup"), Action::Click {})).await;
    let event = settled(session, waiting).await;
    assert_eq!(event.outcome, Outcome::Succeeded, "{event:?}");
    let popup: Reference =
        serde_json::from_value(event.result.unwrap()["event"]["resource"].clone()).unwrap();
    successful(
        session,
        Request::PageWait {
            page: popup,
            frame: None,
            locator: Some(css("h1")),
            condition: Condition::Text {
                value: "Acme popup".into(),
                contains: false,
            },
        },
    )
    .await;
    successful(session, Request::PageSelect { page: popup }).await;
    successful(session, Request::PageClose { page: popup }).await;
    successful(session, Request::PageSelect { page }).await;
}
