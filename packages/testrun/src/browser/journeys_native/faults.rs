//! Explicit partial results and guarded writes in the common application journey.
use super::*;

pub(super) async fn exercise(session: &Session, page: Reference) {
    let covered = session
        .submit(
            Uuid::new_v4(),
            action(page, css("#blocked"), Action::Click {}),
            1500,
        )
        .unwrap();
    let blocked = settled(session, covered).await;
    assert_ne!(blocked.outcome, Outcome::Succeeded, "{blocked:?}");
    let before = successful(session, read(page, Some(css("#blocked")), Read::Text)).await;
    assert_eq!(before["result"]["value"], "Covered");
    let forced = successful(
        session,
        Request::PageAction {
            page,
            frame: None,
            locator: css("#blocked"),
            action: Action::Click {},
            force: true,
            postcondition: None,
        },
    )
    .await;
    assert_eq!(forced["mode"], "forced");
    let scripted = successful(
        session,
        action(
            page,
            css("#blocked"),
            Action::Dispatch {
                event: "click".into(),
            },
        ),
    )
    .await;
    assert_eq!(scripted["mode"], "scripted");
    wait_text(session, page, "#blocked", "Activated").await;
    successful(session, action(page, css("#uncover"), Action::Click {})).await;
    successful(session, action(page, css("#blocked"), Action::Click {})).await;
    let failed = session
        .submit(
            Uuid::new_v4(),
            Request::PageAction {
                page,
                frame: None,
                locator: css("#save"),
                action: Action::Click {},
                force: false,
                postcondition: Some(Condition::Text {
                    value: "Impossible postcondition".into(),
                    contains: false,
                }),
            },
            1500,
        )
        .unwrap();
    let failed = settled(session, failed).await;
    assert_ne!(failed.outcome, Outcome::Succeeded, "{failed:?}");
    assert_eq!(
        failed.result.as_ref().unwrap()["dispatchCompleted"],
        true,
        "{failed:?}"
    );
    let batch = session
        .submit(
            Uuid::new_v4(),
            Request::Batch {
                requests: vec![
                    action(page, css("#save"), Action::Click {}),
                    Request::PageWait {
                        page,
                        frame: None,
                        locator: Some(css("#result")),
                        condition: Condition::Text {
                            value: "Never".into(),
                            contains: false,
                        },
                    },
                    action(page, css("#prompt"), Action::Click {}),
                ],
            },
            1500,
        )
        .unwrap();
    let batch = settled(session, batch).await;
    assert_ne!(batch.outcome, Outcome::Succeeded, "{batch:?}");
    assert_eq!(
        batch.result.as_ref().unwrap()["completedActions"],
        1,
        "{batch:?}"
    );
    assert_eq!(
        batch.result.as_ref().unwrap()["replayed"],
        false,
        "{batch:?}"
    );
    assert_eq!(
        session.status()["phase"],
        "ready",
        "{}",
        serde_json::json!({"phase":session.status()["phase"],"failure":session.status()["failure"]})
    );
    let clipboard = session
        .submit(
            Uuid::new_v4(),
            Request::PageClipboard {
                page,
                origin: "https://example.com".into(),
                action: super::super::clipboard::Action::Write {
                    text: "Acme".into(),
                },
            },
            30000,
        )
        .unwrap();
    assert_eq!(
        settled(session, clipboard).await.code,
        Some("CLIPBOARD_ORIGIN_MISMATCH".into())
    );
}
