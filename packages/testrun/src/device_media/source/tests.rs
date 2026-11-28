//! Media observation, consumer ownership and cancellation-safe remote cleanup fixtures.
use super::*;
use std::time::Instant;
#[tokio::test]
async fn absent_lock_service_does_not_claim_an_unlocked_phone() {
    let source = recording_fixture();
    let status = source.lock_status().await;
    assert_eq!(status["state"], "unknown");
    assert_eq!(status["code"], "CAPABILITY_UNAVAILABLE");
    assert!(status.get("nextAction").is_none());
    source.close().await.unwrap();
}

#[tokio::test]
async fn snapshot_waits_through_a_gap_and_observes_late_decode_completion() {
    let nals: Vec<Vec<u8>> = include_str!("../../../tests/fixtures/ios/hevc-keyframe.hex")
        .lines()
        .map(|line| {
            line.as_bytes()
                .as_chunks::<2>()
                .0
                .iter()
                .map(|b| u8::from_str_radix(std::str::from_utf8(b).unwrap(), 16).unwrap())
                .collect()
        })
        .collect();
    let format =
        Arc::new(apple::Format::new(&[nals[0].clone(), nals[1].clone(), nals[2].clone()]).unwrap());
    let mut payload = (nals[3].len() as u32).to_be_bytes().to_vec();
    payload.extend(&nals[3]);
    let frame = |pts| {
        Arc::new(VideoFrame {
            media: crate::media::Frame {
                packet: vec![],
                metadata: json!({}),
                configuration: None,
                received: Instant::now(),
            },
            payload: payload.clone(),
            key: true,
            pts,
            format: format.clone(),
            configuration_sequence: 1,
        })
    };
    // Warm decoding/JPEG construction before the wait regression, so the
    // deadline measures missing notification rather than cold native setup.
    let (requests, _) = mpsc::channel(8);
    // Framework/device initialization is setup, not callback latency. Construct
    // the real decoder before the unchanged decoded-completion deadline starts.
    let decoder = apple::Decoder::new(&format, (300, 550)).unwrap();
    let decode = Observation::with_decoder(decoder, (300, 550), requests).unwrap();
    decode.enable();
    decode.submit(frame(0));
    let warm_deadline = Instant::now() + Duration::from_secs(2);
    while decode.health().0.is_none() {
        assert!(
            Instant::now() < warm_deadline,
            "Decoder warm-up health: {:?}",
            decode.health()
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    decode.snapshot().await.unwrap().unwrap();
    let (sender, latest) = watch::channel(Some(frame(0)));
    let mut stale = frame(50_000_000);
    Arc::get_mut(&mut stale).unwrap().media.received = Instant::now() - Duration::from_secs(1);
    let next = frame(123_000_000);
    let mut source = recording_fixture();
    let owned = Arc::get_mut(&mut source).unwrap();
    owned.display_geometry = Ok((300, 550));
    owned.latest = latest;
    owned.observation = decode.clone();
    let submission_decode = decode.clone();
    let submission = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(10)).await;
        sender.send_replace(Some(stale));
        tokio::time::sleep(Duration::from_millis(10)).await;
        sender.send_replace(Some(next.clone()));
        tokio::time::sleep(Duration::from_millis(10)).await;
        submission_decode.submit(next);
        tokio::time::sleep(Duration::from_millis(300)).await;
    });
    let observation = tokio::time::timeout(Duration::from_millis(250), source.snapshot()).await;
    submission.await.unwrap();
    // The source ended after that successful observation. A following action
    // wait must not reuse the already captured picture as fresh output.
    let ended = source.settle(None).await.err().unwrap();
    assert_eq!(ended.code, "CAPTURE_STALLED");
    decode.close().await.unwrap();
    assert_eq!(observation.unwrap().unwrap().pts, 123_000_000);
}
