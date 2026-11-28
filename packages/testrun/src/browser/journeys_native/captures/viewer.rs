//! Real authenticated viewer socket deliberately withholds frame ACK while input and recording run.
use super::*;
use crate::interactive::local::{read as ipc_read, write as ipc_write};
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;

pub(super) async fn exercise(session: &Session, page: Reference) {
    let started = successful(
        session,
        Request::PreviewStart {
            page,
            options: Default::default(),
        },
    )
    .await;
    let preview: Reference = serde_json::from_value(started["preview"].clone()).unwrap();
    let viewer = &started["viewer"];
    let mut broker = tokio::net::UnixStream::connect(viewer["grantBrokerPath"].as_str().unwrap())
        .await
        .unwrap();
    ipc_write(&mut broker, &json!({"streamId":viewer["streamId"]}))
        .await
        .unwrap();
    let grant = ipc_read(&mut broker).await.unwrap();
    let url = viewer["mediaUrl"]
        .as_str()
        .unwrap()
        .replacen("http://", "ws://", 1);
    let (mut socket, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
    socket
        .send(Message::Text(
            json!({"token":grant["token"]}).to_string().into(),
        ))
        .await
        .unwrap();
    let ready = socket.next().await.unwrap().unwrap();
    assert!(ready.is_text(), "{ready:?}");
    let packet = tokio::time::timeout(Duration::from_secs(5), socket.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap()
        .into_data();
    let (length, _) = crate::media::lengths(packet[..24].try_into().unwrap()).unwrap();
    let metadata: Value = serde_json::from_slice(&packet[24..24 + length]).unwrap();
    assert_eq!(metadata["codec"], "jpeg");
    assert_eq!(metadata["coordinateInputAvailable"], true, "{metadata}");
    let decoded = image::load_from_memory(&packet[24 + length..]).unwrap();
    assert!(
        decoded.width() >= 959 && decoded.width() <= 960,
        "{metadata}"
    );
    corners(&decoded.to_rgb8(), 959, 599);
    let (mut reused, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
    reused
        .send(Message::Text(
            json!({"token":grant["token"]}).to_string().into(),
        ))
        .await
        .unwrap();
    let refused = tokio::time::timeout(Duration::from_secs(2), reused.next())
        .await
        .unwrap();
    assert!(
        !matches!(refused, Some(Ok(Message::Binary(_)))),
        "Single-use grant replay delivered pixels"
    );
    let recording = successful(
        session,
        Request::RecordingStart {
            page,
            options: crate::browser::recording::Options {
                max_duration_ms: 10000,
            },
        },
    )
    .await;
    let recording: Reference = serde_json::from_value(recording["recording"].clone()).unwrap();
    let live = successful(session, Request::PreviewStatus { preview }).await;
    successful(
        session,
        Request::PageInput {
            page,
            observation: Some(
                serde_json::from_value(live["metadata"]["inputObservation"].clone()).unwrap(),
            ),
            actions: vec![crate::browser::input::Action::MouseClick {
                x: 140.,
                y: 100.,
                button: Default::default(),
                click_count: 1,
                delay_ms: 0,
            }],
        },
    )
    .await;
    successful(session, action(page, css("button"), Action::Click {})).await;
    let observed = successful(session, read(page, None, Read::Geometry)).await;
    successful(
        session,
        Request::PageInput {
            page,
            observation: Some(
                serde_json::from_value(observed["inputObservation"].clone()).unwrap(),
            ),
            actions: vec![crate::browser::input::Action::KeyDown {
                key: "Shift".into(),
            }],
        },
    )
    .await;
    let status = session.control_status();
    let handed=session.change_control("browser_control_acquire",json!({"owner":"viewer","viewerId":"Acme-viewer","page":page,"expectedVersion":status["controlVersion"]})).await.unwrap();
    assert_eq!(handed["owner"], "viewer", "{handed}");
    let input = viewer_input(session, page, &handed).await;
    assert!(
        session
            .submit(
                Uuid::new_v4(),
                action(page, css("button"), Action::Click {}),
                30000
            )
            .is_err()
    );
    let restored = session
        .change_control(
            "browser_control_acquire",
            json!({"owner":"agent","expectedVersion":handed["controlVersion"]}),
        )
        .await
        .unwrap();
    assert_eq!(restored["owner"], "agent");
    drop(input);
    successful(session, Request::PreviewStop { preview }).await;
    successful(session, action(page, css("button"), Action::Click {})).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let finished = successful(session, Request::RecordingStop { recording }).await;
    assert!(
        matches!(finished["status"].as_str(), Some("complete" | "partial")),
        "{finished}"
    );
    assert_eq!(finished["verification"]["decodable"], true, "{finished}");
    assert_eq!(
        finished["verification"]["decodedFrames"], finished["statistics"]["encodedFrames"],
        "{finished}"
    );
    successful(session, Request::RecordingStop { recording }).await;
    successful(session, Request::PreviewStop { preview }).await;
    let _ = socket.close(None).await;
    recovery(session, page).await;
    if matches!(session, Session::Chrome(_)) {
        release_delayed_input(session, page).await;
    }
}

async fn release_delayed_input(session: &Session, page: Reference) {
    let geometry = successful(session, read(page, None, Read::Geometry)).await;
    let id = session
        .submit(
            Uuid::new_v4(),
            Request::PageInput {
                page,
                observation: Some(
                    serde_json::from_value(geometry["inputObservation"].clone()).unwrap(),
                ),
                actions: vec![
                    crate::browser::input::Action::Delay { duration_ms: 30000 },
                    crate::browser::input::Action::KeyDown {
                        key: "Shift".into(),
                    },
                ],
            },
            30000,
        )
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if session
            .operation(id)
            .is_some_and(|operation| operation.outcome == Outcome::Running)
        {
            break;
        }
        assert!(Instant::now() < deadline);
        tokio::task::yield_now().await;
    }
    tokio::time::timeout(
        Duration::from_secs(5),
        successful(session, Request::InputRelease {}),
    )
    .await
    .expect("Input release waited for the 30-second delay");
    let old = settled(session, id).await;
    assert_eq!(
        old.code.as_deref(),
        Some("INPUT_RELEASE_REQUESTED"),
        "{old:?}"
    );
    assert_eq!(session.status()["phase"], "ready");
    assert_eq!(session.status()["input"]["pages"], json!([]));
}

async fn recovery(session: &Session, page: Reference) {
    let preview = successful(
        session,
        Request::PreviewStart {
            page,
            options: Default::default(),
        },
    )
    .await;
    let reference: Reference = serde_json::from_value(preview["preview"].clone()).unwrap();
    let epoch = preview["sourceEpoch"].clone();
    successful(
        session,
        Request::PageResize {
            page,
            viewport: Viewport {
                width: 1600,
                height: 900,
            },
        },
    )
    .await;
    let grown = next_frame(session, reference, &epoch).await;
    assert_eq!(
        grown["metadata"]["logicalViewport"],
        json!({"width":1600,"height":900})
    );
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
    let navigated = next_frame(session, reference, &grown["sourceEpoch"]).await;
    assert_ne!(navigated["frame"], grown["frame"]);
    successful(session, Request::PreviewStop { preview: reference }).await;
}
async fn next_frame(session: &Session, preview: Reference, epoch: &Value) -> Value {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let current = successful(session, Request::PreviewStatus { preview }).await;
        if current["sourceEpoch"] != *epoch
            && current["state"] == "live"
            && current["metadata"].is_object()
        {
            return current;
        }
        assert!(
            Instant::now() < deadline,
            "Source did not recover: {current}"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

fn corners(image: &image::RgbImage, width: u32, height: u32) {
    for (x, y, color) in [
        (0, 0, 0),
        (width - 40, 0, 1),
        (0, height - 40, 2),
        (width - 40, height - 40, 3),
    ] {
        let (mut count, mut min_x, mut min_y, mut max_x, mut max_y) = (0, 40u32, 40u32, 0u32, 0u32);
        for dy in 0..40 {
            for dx in 0..40 {
                let p = image.get_pixel(x + dx, y + dy).0;
                let matches = match color {
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
            "Missing/cropped corner {color}: {count} pixels at {min_x},{min_y}..{max_x},{max_y}"
        );
    }
}
async fn viewer_input(
    session: &Session,
    page: Reference,
    handed: &Value,
) -> tokio::net::UnixStream {
    let channel = &handed["channel"];
    let path = channel["controlSocketPath"].as_str().unwrap();
    let mut grant = tokio::net::UnixStream::connect(path).await.unwrap();
    ipc_write(
        &mut grant,
        &json!({"kind":"control_grant","channelId":channel["channelId"]}),
    )
    .await
    .unwrap();
    let token = ipc_read(&mut grant).await.unwrap();
    let mut viewer = tokio::net::UnixStream::connect(path).await.unwrap();
    ipc_write(&mut viewer, &json!({"token":token["token"]}))
        .await
        .unwrap();
    assert_eq!(ipc_read(&mut viewer).await.unwrap()["kind"], "ready");
    let observed = successful(session, read(page, None, Read::Geometry)).await;
    ipc_write(&mut viewer,&json!({"kind":"browser_input","commandId":Uuid::new_v4(),"page":page,"observation":observed["inputObservation"],"actions":[{"action":"mouse_click","x":140,"y":100}]})).await.unwrap();
    let result = tokio::time::timeout(Duration::from_secs(4), ipc_read(&mut viewer))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(result["result"]["ok"], true, "{result}");
    assert_eq!(
        result["result"]["data"]["operation"]["outcome"], "succeeded",
        "{result}"
    );
    viewer
}
