//! Media authentication, revocation, framing and decoder contracts.
use super::*;
use super::{grants::consume_grant, packet::Decoder};
use crate::grants::issue_grant;
#[cfg(unix)]
#[tokio::test]
async fn startup_checks_real_listener_and_private_broker_and_removes_them_on_close() {
    use std::os::unix::fs::PermissionsExt;
    let mut media = Media::start().await.unwrap();
    let address = media
        .address
        .strip_prefix("http://")
        .unwrap()
        .parse()
        .unwrap();
    startup::check_address(address).unwrap();
    startup::check_socket(&media.broker).unwrap();
    let directory = media._files.0.clone();
    let socket = media.broker.clone();
    std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o666)).unwrap();
    assert!(startup::check_socket(&socket).is_err());
    media.close().unwrap();
    assert!(!socket.exists());
    assert!(!directory.exists());
}

#[tokio::test]
async fn closing_preview_revokes_pending_grants_and_cached_video() {
    let grants = Grants::default();
    let token = issue_grant(&grants).unwrap();
    let (frames, receiver) = watch::channel(Some(Arc::new(Frame {
        packet: encode(&json!({"seq":"1"}), b"frame").unwrap(),
        metadata: json!({"seq":"1"}),
        configuration: None,
        received: std::time::Instant::now(),
    })));
    let (requests, _) = tokio::sync::mpsc::channel(1);
    let preview = Preview {
        browser: false,
        _simulator: None,
        grants: grants.clone(),
        frames,
        slots: Arc::new(Semaphore::new(8)),
        worker: tokio::spawn(std::future::pending()),
        requests,
        health: Arc::new(Mutex::new(Health {
            last_seen: std::time::Instant::now(),
            encoded: 1,
            activity: "0".into(),
            alive: true,
        })),
        delivery: None,
        #[cfg(target_os = "macos")]
        _source: None,
    };
    drop(preview);
    assert!(!consume_grant(&grants, &token));
    assert!(receiver.borrow().is_none());
}
#[test]
fn viewer_credentials_expire_and_cannot_be_reused() {
    let grants = Grants::default();
    let token = issue_grant(&grants).unwrap();
    assert!(!consume_grant(&grants, "wrong"));
    assert!(consume_grant(&grants, &token));
    assert!(!consume_grant(&grants, &token));
    grants.lock().unwrap().insert(
        "expired".into(),
        std::time::Instant::now() - Duration::from_secs(1),
    );
    assert!(!consume_grant(&grants, "expired"));
}
#[test]
fn validates_frame_bounds_before_allocation() {
    let packet = encode(&json!({"seq":"1"}), b"jpeg").unwrap();
    let header = packet[..24].try_into().unwrap();
    assert_eq!(lengths(&header).unwrap(), (11, 4));
    let mut invalid = header;
    invalid[16..20].copy_from_slice(&u32::MAX.to_be_bytes());
    assert_eq!(lengths(&invalid).unwrap_err().code, "OUTPUT_LIMIT");
}

fn preview_grant_with_stalled_peer(broker: &std::path::Path) -> String {
    use canonic_process_kit::{endpoint::connect_local, frame};
    let timeout = Duration::from_secs(2);
    let mut stalled = connect_local(broker, timeout).unwrap();
    std::io::Write::write_all(&mut stalled, &[0, 0]).unwrap();
    let mut denied = connect_local(broker, timeout).unwrap();
    frame::write_json(&mut denied, &json!({"streamId":"Acme-absent"}), 4096).unwrap();
    let refusal: Value = frame::read_json(&mut denied, 4096).unwrap();
    assert_eq!(refusal["ok"], false);
    assert_eq!(refusal["error"]["code"], "NOT_FOUND");
    let mut client = connect_local(broker, timeout).unwrap();
    frame::write_json(&mut client, &json!({"streamId":"Acme-browser"}), 4096).unwrap();
    let reply: Value = frame::read_json(&mut client, 4096).unwrap();
    assert_eq!(reply["access"], "read_only");
    assert_eq!(reply["singleUse"], true);
    assert_eq!(reply["expiresInMs"], 30000);
    reply["token"].as_str().unwrap().to_owned()
}

#[tokio::test]
async fn browser_viewers_require_frame_acks_and_skip_replaced_frames() {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message;
    let mut media = Media::start().await.unwrap();
    let make = |sequence: u64| {
        Arc::new(Frame {
            metadata: json!({"seq":sequence.to_string(),"codec":"jpeg"}),
            packet: encode(
                &json!({"seq":sequence.to_string(),"codec":"jpeg"}),
                b"Acme-frame",
            )
            .unwrap(),
            configuration: None,
            received: std::time::Instant::now(),
        })
    };
    let (source, frames) = watch::channel(Some(make(1)));
    let alive = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let presented = media
        .browser_preview("Acme-browser", frames, alive)
        .unwrap();
    let mut observed = {
        let previews = media.shared.previews.lock().unwrap();
        let preview = previews.get("Acme-browser").unwrap();
        preview.frames.subscribe()
    };
    let broker = PathBuf::from(presented["grantBrokerPath"].as_str().unwrap());
    let grant = tokio::task::spawn_blocking(move || preview_grant_with_stalled_peer(&broker))
        .await
        .unwrap();
    let url = presented["mediaUrl"]
        .as_str()
        .unwrap()
        .replacen("http://", "ws://", 1);
    let (mut socket, _) = tokio_tungstenite::connect_async(url).await.unwrap();
    socket
        .send(Message::Text(json!({"token":grant}).to_string().into()))
        .await
        .unwrap();
    let ready = socket.next().await.unwrap().unwrap();
    let ready: Value = serde_json::from_str(ready.to_text().unwrap()).unwrap();
    assert_eq!(ready["frameAckRequired"], true);
    assert!(socket.next().await.unwrap().unwrap().is_binary());
    source.send_replace(Some(make(2)));
    source.send_replace(Some(make(3)));
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if observed
                .borrow_and_update()
                .as_ref()
                .is_some_and(|frame| frame.metadata["seq"] == "3")
            {
                break;
            }
            observed.changed().await.unwrap();
        }
    })
    .await
    .unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(50), socket.next())
            .await
            .is_err(),
        "A second browser frame was sent before ACK"
    );
    socket
        .send(Message::Text(
            json!({"type":"frame_ack","sequence":"1"})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
    let next = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if let Message::Binary(packet) = socket.next().await.unwrap().unwrap() {
                break packet;
            }
        }
    })
    .await
    .unwrap();
    let (metadata, _) = lengths(&next[..24].try_into().unwrap()).unwrap();
    let metadata: Value = serde_json::from_slice(&next[24..24 + metadata]).unwrap();
    assert_eq!(metadata["seq"], "3");
    socket.close(None).await.unwrap();
    media.stop("Acme-browser").unwrap();
    media.close().unwrap();
    assert!(!media.broker.exists());
    assert!(!media.broker.parent().unwrap().exists());
}
#[test]
fn h264_consumers_resume_only_at_decodable_keyframes() {
    let make = |seq: &str, keyframe| Frame {
        packet: encode_packet(&json!({}), b"frame", 1, keyframe).unwrap(),
        metadata: json!({"seq":seq,"codec":"h264"}),
        configuration: None,
        received: std::time::Instant::now(),
    };
    let mut decoder = Decoder::default();
    assert!(!decoder.accept(&make("1", false)));
    assert!(decoder.accept(&make("2", true)));
    assert!(decoder.accept(&make("3", false)));
    assert!(!decoder.accept(&make("5", false)));
    assert!(!decoder.accept(&make("6", false)));
    assert!(decoder.accept(&make("7", true)));
    assert_eq!(decoder.dropped_frames(), 1);
    assert!(
        h264_configuration(&[
            0, 0, 0, 1, 0x67, 1, 0, 0, 0, 1, 0x68, 2, 0, 0, 0, 1, 0x65, 3
        ])
        .is_ok()
    );
    assert!(h264_configuration(&[0, 0, 0, 1, 0x65, 3]).is_err());
}
