//! A blocked native decoder must not block frame admission, health or owned cancellation.
use super::*;
use serde_json::json;

fn frame() -> Arc<VideoFrame> {
    let nals: Vec<Vec<u8>> = include_str!("../../../tests/fixtures/ios/hevc-keyframe.hex")
        .lines()
        .map(|line| {
            line.as_bytes()
                .as_chunks::<2>()
                .0
                .iter()
                .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
                .collect()
        })
        .collect();
    Arc::new(VideoFrame {
        media: crate::media::Frame {
            packet: vec![],
            metadata: json!({}),
            configuration: None,
            received: std::time::Instant::now(),
        },
        payload: vec![],
        key: true,
        pts: 1,
        format: Arc::new(
            apple::Format::new(&[nals[0].clone(), nals[1].clone(), nals[2].clone()]).unwrap(),
        ),
        configuration_sequence: 1,
    })
}
struct Blocked {
    entered: Option<oneshot::Sender<()>>,
    release: mpsc::Receiver<()>,
}
impl Decode for Blocked {
    fn append(&mut self, _: &VideoFrame) -> Result<()> {
        if let Some(entered) = self.entered.take() {
            let _ = entered.send(());
            self.release.recv_timeout(Duration::from_secs(5)).unwrap();
        }
        Ok(())
    }
    fn latest_pts(&self) -> Option<u64> {
        None
    }
    fn snapshot(&self) -> Result<Option<apple::Snapshot>> {
        Ok(None)
    }
    fn reset(&mut self) {}
}
#[tokio::test]
async fn blocked_decode_has_bounded_admission_and_cancellation_retains_its_worker() {
    let (entered, observed) = oneshot::channel();
    let (release, wait) = mpsc::channel();
    let (requests, _) = tokio::sync::mpsc::channel(8);
    let observation = Observation::spawn(
        Blocked {
            entered: Some(entered),
            release: wait,
        },
        requests,
    )
    .unwrap();
    observation.enable();
    let frame = frame();
    observation.submit(frame.clone());
    tokio::time::timeout(Duration::from_secs(1), observed)
        .await
        .unwrap()
        .unwrap();
    // This executes while the decoder is blocked. The receiver can still
    // publish these same frames independently to preview/recording consumers.
    for _ in 0..100 {
        observation.submit(frame.clone());
    }
    let (pts, error) = observation.health();
    assert!(pts.is_none());
    assert_eq!(error.unwrap().code, "CAPTURE_STALLED");
    assert!(
        tokio::time::timeout(Duration::from_millis(30), observation.close())
            .await
            .is_err()
    );
    assert!(observation.worker.lock().unwrap().job.is_some());
    release.send(()).unwrap();
    observation.close().await.unwrap();
    assert!(observation.worker.lock().unwrap().job.is_none());
}

#[tokio::test]
async fn failed_worker_cleanup_cannot_become_a_successful_repeated_close() {
    let (entered, observed) = oneshot::channel();
    let (release, wait) = mpsc::channel();
    let (requests, _) = tokio::sync::mpsc::channel(8);
    let observation = Observation::spawn(
        Blocked {
            entered: Some(entered),
            release: wait,
        },
        requests,
    )
    .unwrap();
    observation.enable();
    observation.submit(frame());
    tokio::time::timeout(Duration::from_secs(1), observed)
        .await
        .unwrap()
        .unwrap();
    // A disconnected fixture releases the blocking wait through a driver panic.
    drop(release);
    assert_eq!(
        observation.close().await.unwrap_err().code,
        "CLEANUP_FAILED"
    );
    assert_eq!(
        observation.close().await.unwrap_err().code,
        "CLEANUP_FAILED"
    );
}

struct Starting {
    gate: Blocked,
    appended: Arc<AtomicUsize>,
    resets: Arc<AtomicUsize>,
    pts: Option<u64>,
}
impl Decode for Starting {
    fn append(&mut self, frame: &VideoFrame) -> Result<()> {
        self.gate.append(frame)?;
        self.pts = Some(frame.pts);
        self.appended.fetch_add(1, Ordering::AcqRel);
        Ok(())
    }
    fn latest_pts(&self) -> Option<u64> {
        self.pts
    }
    fn snapshot(&self) -> Result<Option<apple::Snapshot>> {
        Ok(None)
    }
    fn reset(&mut self) {
        self.pts = None;
        self.resets.fetch_add(1, Ordering::AcqRel);
    }
}

#[tokio::test]
async fn decoder_startup_preserves_a_predictive_burst_without_resetting() {
    let (entered, observed) = oneshot::channel();
    let (release, wait) = mpsc::channel();
    let (requests, _) = tokio::sync::mpsc::channel(8);
    let appended = Arc::new(AtomicUsize::new(0));
    let resets = Arc::new(AtomicUsize::new(0));
    let observation = Observation::spawn(
        Starting {
            gate: Blocked {
                entered: Some(entered),
                release: wait,
            },
            appended: appended.clone(),
            resets: resets.clone(),
            pts: None,
        },
        requests,
    )
    .unwrap();
    observation.enable();
    observation.submit(frame());
    tokio::time::timeout(Duration::from_secs(1), observed)
        .await
        .unwrap()
        .unwrap();
    // Model half a second of 60 FPS reception while native creation is blocked.
    // Deltas depend on the initial keyframe and must survive startup together.
    for pts in 2..=31 {
        let mut next = frame();
        let next_frame = Arc::get_mut(&mut next).unwrap();
        next_frame.key = false;
        next_frame.pts = pts;
        observation.submit(next);
    }
    assert!(observation.health().1.is_none());
    release.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(1), async {
        while observation.health().0 != Some(31) {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(appended.load(Ordering::Acquire), 31);
    assert_eq!(resets.load(Ordering::Acquire), 0);
    observation.close().await.unwrap();
    assert_eq!(observation.queued_bytes.load(Ordering::Acquire), 0);
}

#[tokio::test]
async fn observation_byte_budget_includes_the_in_flight_frame_and_releases_on_stop() {
    let (entered, observed) = oneshot::channel();
    let (release, wait) = mpsc::channel();
    let (requests, _) = tokio::sync::mpsc::channel(8);
    let observation = Observation::spawn(
        Blocked {
            entered: Some(entered),
            release: wait,
        },
        requests,
    )
    .unwrap();
    observation.enable();
    let mut large = frame();
    let large_frame = Arc::get_mut(&mut large).unwrap();
    large_frame.payload.resize(1024 * 1024, 0);
    large_frame.media.packet.resize(1024 * 1024, 0);
    observation.submit(large.clone());
    tokio::time::timeout(Duration::from_secs(1), observed)
        .await
        .unwrap()
        .unwrap();
    for _ in 0..3 {
        observation.submit(large.clone());
    }
    assert!(observation.health().1.is_none());
    assert_eq!(
        observation.queued_bytes.load(Ordering::Acquire),
        QUEUED_BYTES
    );
    observation.submit(large.clone());
    assert_eq!(observation.health().1.unwrap().code, "CAPTURE_STALLED");
    assert_eq!(
        observation.queued_bytes.load(Ordering::Acquire),
        QUEUED_BYTES
    );
    release.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(1), async {
        while observation.queued_bytes.load(Ordering::Acquire) != 0 {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .unwrap();
    // Recovery releases every discarded dependency; subsequent admission and
    // shutdown must not inherit a leaked byte reservation.
    observation.submit(large);
    observation.close().await.unwrap();
    assert_eq!(observation.queued_bytes.load(Ordering::Acquire), 0);
}
