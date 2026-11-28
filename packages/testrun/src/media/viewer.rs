//! Authenticated read-only WebSocket viewers and per-consumer decoder recovery.
use super::{Frame, Health, Shared};
use super::{
    grants::{Grants, consume_grant},
    packet::Decoder,
};
use axum::{
    extract::{
        Path, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::{Semaphore, watch};

pub(super) async fn upgrade(
    State(state): State<Shared>,
    Path(id): Path<String>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    // Browser viewers connect through an authorized native host integration. Raw
    // browser origins cannot obtain or exercise the private grant credentials.
    if headers.contains_key("origin") {
        return StatusCode::FORBIDDEN.into_response();
    }
    let selected = {
        let Ok(previews) = state.previews.lock() else {
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        };
        let Some(preview) = previews.get(&id) else {
            return StatusCode::NOT_FOUND.into_response();
        };
        (
            preview.grants.clone(),
            preview.frames.subscribe(),
            preview.slots.clone(),
            preview.requests.clone(),
            preview.health.clone(),
            preview.delivery.as_ref().map(|s| s.subscribe()),
            preview.browser,
        )
    };
    ws.max_message_size(4096)
        .max_frame_size(4096)
        .on_upgrade(move |socket| viewer(socket, selected))
}
type ViewerState = (
    Grants,
    watch::Receiver<Option<Arc<Frame>>>,
    Arc<Semaphore>,
    tokio::sync::mpsc::Sender<()>,
    Arc<Mutex<Health>>,
    Option<tokio::sync::broadcast::Receiver<Arc<Frame>>>,
    bool,
);
#[expect(
    clippy::too_many_lines,
    reason = "Authentication, GOP recovery and socket teardown share one consumer lifetime"
)]
async fn viewer(
    mut socket: WebSocket,
    (grants, mut frames, slots, requests, health, mut delivery, browser): ViewerState,
) {
    let Ok(_slot) = slots.try_acquire_owned() else {
        let _ = socket.close().await;
        return;
    };
    let auth = tokio::time::timeout(Duration::from_secs(3), socket.next()).await;
    let authorized = matches!(auth,Ok(Some(Ok(Message::Text(ref text)))) if serde_json::from_str::<Value>(text).ok().and_then(|v|v["token"].as_str().map(str::to_owned)).is_some_and(|token|consume_grant(&grants,&token)));
    if !authorized {
        let _ = socket.close().await;
        return;
    }
    let initial = frames.borrow().clone();
    let Some(initial) = initial else {
        let _ = socket.close().await;
        return;
    };
    if matches!(initial.metadata["codec"].as_str(), Some("h264" | "hevc")) {
        let _ = requests.try_send(());
    }
    if !matches!(tokio::time::timeout(Duration::from_secs(2),socket.send(Message::Text(json!({"event":"ready","eventSeq":"1","protocolVersion":1,"metadata":initial.metadata,"frameAckRequired":browser}).to_string().into()))).await,Ok(Ok(()))){return;}
    let (mut writer, mut reader) = socket.split();
    let mut decoder = Decoder::default();
    let mut recovery_started: Option<std::time::Instant> = None;
    let mut last_request = std::time::Instant::now() - Duration::from_secs(3);
    let mut events = 1u64;
    let mut stalled = false;
    let mut delivered = Some(initial);
    let mut pending_frame: Option<String> = None;
    let mut last_sent_frame = 0u64;
    loop {
        if frames.borrow().is_none() {
            if browser && health.lock().is_ok_and(|health| health.alive) {
                pending_frame = None;
                // Browser navigation clears the old document's cache. Keep
                // the authenticated viewer alive across this explicit gap.
                if !matches!(tokio::time::timeout(Duration::from_secs(2), writer.send(Message::Text(json!({"event":"discontinuity","reason":"document_or_viewport_changed"}).to_string().into()))).await, Ok(Ok(()))) { break; }
                if tokio::time::timeout(Duration::from_secs(3), frames.changed())
                    .await
                    .is_err()
                {
                    break;
                }
                continue;
            }
            let _ = tokio::time::timeout(
                Duration::from_secs(2),
                writer.send(Message::Text(json!({"event":"ended"}).to_string().into())),
            )
            .await;
            break;
        }
        let current = if delivery.is_some() {
            delivered.take()
        } else {
            frames.borrow_and_update().clone()
        };
        let Some(frame) = current else {
            let _ = writer
                .send(Message::Text(json!({"event":"ended"}).to_string().into()))
                .await;
            break;
        };
        if (!browser || pending_frame.is_none()) && decoder.accept(&frame) {
            recovery_started = None;
            if frame.packet[7] & 1 != 0
                && let Some(configuration) = &frame.configuration
                && !matches!(
                    tokio::time::timeout(
                        Duration::from_secs(2),
                        writer.send(Message::Binary(configuration.clone().into()))
                    )
                    .await,
                    Ok(Ok(()))
                )
            {
                break;
            }
            if !matches!(
                tokio::time::timeout(
                    Duration::from_secs(2),
                    writer.send(Message::Binary(frame.packet.clone().into()))
                )
                .await,
                Ok(Ok(()))
            ) {
                break;
            }
            if browser {
                pending_frame = frame.metadata["seq"].as_str().map(str::to_owned);
                last_sent_frame = pending_frame
                    .as_deref()
                    .and_then(|sequence| sequence.parse().ok())
                    .unwrap_or(last_sent_frame);
            }
        }
        if decoder.waiting_for_keyframe() {
            let started = recovery_started.get_or_insert_with(std::time::Instant::now);
            if started.elapsed() > Duration::from_secs(10) {
                let _ = tokio::time::timeout(
                    Duration::from_secs(2),
                    writer.send(Message::Text(
                        json!({"event":"error","code":"KEYFRAME_TIMEOUT"})
                            .to_string()
                            .into(),
                    )),
                )
                .await;
                break;
            }
            if last_request.elapsed() > Duration::from_secs(3) {
                let _ = requests.try_send(());
                last_request = std::time::Instant::now();
            }
        }
        if delivery.is_some() {
            delivered = Some(frame.clone());
        }
        tokio::select! {
            received=async{match &mut delivery{
                Some(receiver)=>match receiver.recv().await{Ok(frame)=>Ok(Some(frame)),Err(tokio::sync::broadcast::error::RecvError::Lagged(_))=>Ok(None),Err(_)=>Err(())},
                None=>frames.changed().await.map(|_|None).map_err(|_|()),
            }}=>{match received{Ok(Some(frame))=>delivered=Some(frame),Ok(None)=>{if delivery.is_some(){decoder.require_keyframe();let _=requests.try_send(());delivered=Some(frame.clone());}},Err(_)=>break}},
            _=tokio::time::sleep(Duration::from_secs(1))=>{events+=1;let age=frame.received.elapsed().as_millis();let source_stalled=health.lock().map_or(true,|health|!health.alive||health.last_seen.elapsed()>Duration::from_secs(2));let event=if source_stalled{"stalled"}else if stalled{"resumed"}else{"statistics"};stalled=source_stalled;if !matches!(tokio::time::timeout(Duration::from_secs(2),writer.send(Message::Text(json!({"event":event,"eventSeq":events.to_string(),"lastFrameAgeMs":age,"idle":!source_stalled&&age>2000,"droppedFrames":decoder.dropped_frames()}).to_string().into()))).await,Ok(Ok(()))){break;}},
            message=reader.next()=>{match message{Some(Ok(Message::Ping(data)))=>{if !matches!(tokio::time::timeout(Duration::from_secs(2),writer.send(Message::Pong(data))).await,Ok(Ok(()))){break;}},Some(Ok(Message::Text(text)))=>{
                let value=serde_json::from_str::<Value>(&text).ok();
                match value.as_ref().and_then(|v|v["type"].as_str()) {
                    Some("frame_ack") if browser=>{
                        let sequence=value.as_ref().and_then(|v|v["sequence"].as_str());
                        if sequence==pending_frame.as_deref() && pending_frame.is_some() { pending_frame=None; }
                        else if sequence.and_then(|sequence|sequence.parse::<u64>().ok()).is_none_or(|sequence| sequence > last_sent_frame) { break; }
                    },
                    Some("keyframe")=>{let _=requests.try_send(());},Some("keepalive")=>{},_=>break
                }
            },_=>break}}
        }
    }
    let _ = tokio::time::timeout(Duration::from_secs(2), writer.close()).await;
}
