//! Abandoned image workers retain bounded capacity and participate in teardown.
use super::*;

fn context() -> ReadContext {
    ReadContext {
        #[cfg(unix)]
        authority: Arc::new(crate::control::Authority::new("l".into(), true)),
        #[cfg(unix)]
        viewer: None,
        validated: false,
        processing: Processing::default(),
        processing_target: Processing::target(),
        target: crate::targets::qa_fixture_target(),
        evidence: PathBuf::new(),
        control_path: "native".into(),
        android_capture: None,
        control: false,
        playback: None,
        discovery: Server::new().discovery_reader(),
        runner: Err(Error::new("RUNNER_SETUP_REQUIRED", "fixture has no runner")),
        simulator_sources: Arc::default(),
        simulator_source: Arc::default(),
        simulator_incarnation: Arc::default(),
        last_observation: Arc::default(),
        capabilities: Arc::default(),
        gate: Arc::default(),
        #[cfg(target_os = "macos")]
        video: None,
        #[cfg(target_os = "macos")]
        hid: None,
    }
}

#[tokio::test]
async fn exited_inspection_host_loses_xctest_control_but_preserves_independent_hid() {
    for control_path in ["xctest", "hid"] {
        let mut context = context();
        context.control_path = control_path.into();
        context.runner = Err(Error::new("RUNNER_EXITED", "Fixture XCTest host exited"));
        let status = context.status().await.unwrap();
        if control_path == "xctest" {
            assert_eq!(status["state"], "lost");
            assert_eq!(status["reason"], "RUNNER_EXITED");
            assert_eq!(status["nextAction"], "disconnect");
        } else {
            assert_eq!(status["state"], "ready");
            assert_eq!(status["inspectionHealth"]["state"], "unavailable");
        }
    }
}

struct HeldWorker(Option<std::sync::mpsc::Sender<()>>);
impl HeldWorker {
    async fn start(context: &ReadContext) -> Self {
        let (started, ready) = tokio::sync::oneshot::channel();
        let (release, finish) = std::sync::mpsc::channel();
        let mut worker = Box::pin(context.process(move || {
            started.send(()).unwrap();
            finish
                .recv_timeout(std::time::Duration::from_secs(5))
                .unwrap();
            Ok(())
        }));
        tokio::select! {
            result=&mut worker=>panic!("fixture worker finished before release: {result:?}"),
            result=ready=>result.unwrap(),
        }
        // Model a cancelled caller: the actual worker still owns all permits.
        drop(worker);
        Self(Some(release))
    }
}
impl Drop for HeldWorker {
    fn drop(&mut self) {
        if let Some(release) = self.0.take() {
            let _ = release.send(());
        }
    }
}

#[tokio::test]
async fn ordered_evidence_waits_for_shared_capacity_and_expired_waiters_never_dispatch() {
    let processing = Processing::default();
    let contexts = (0..5)
        .map(|_| {
            let mut context = context();
            context.validated = true;
            context.processing = processing.clone();
            context
        })
        .collect::<Vec<_>>();
    let mut workers = Vec::new();
    for context in &contexts[..4] {
        workers.push(HeldWorker::start(context).await);
    }
    let context = &contexts[4];
    let dispatched = Arc::new(AtomicBool::new(false));
    let flag = dispatched.clone();
    let expired = tokio::time::timeout(
        std::time::Duration::from_millis(20),
        context.process(move || {
            flag.store(true, Ordering::Release);
            Ok(())
        }),
    )
    .await;
    assert!(
        expired.is_err(),
        "ordered evidence refused instead of waiting: {expired:?}"
    );
    let mut evidence = Box::pin(context.process(|| Ok("evidence")));
    assert!(futures_util::poll!(&mut evidence).is_pending());
    assert_eq!(context.status().await.unwrap()["state"], "ready");
    drop(workers.remove(0));
    assert_eq!(
        tokio::time::timeout(std::time::Duration::from_secs(2), evidence)
            .await
            .unwrap()
            .unwrap(),
        "evidence"
    );
    assert!(!dispatched.load(Ordering::Acquire));
}

#[tokio::test]
async fn ordered_evidence_waits_for_an_abandoned_worker_on_the_same_target() {
    let mut context = context();
    context.validated = true;
    let worker = HeldWorker::start(&context).await;
    let mut next_session = context.clone();
    next_session.gate = Arc::default();
    let mut evidence = Box::pin(next_session.process(|| Ok("next evidence")));
    assert!(futures_util::poll!(&mut evidence).is_pending());
    assert_eq!(next_session.status().await.unwrap()["state"], "ready");
    drop(worker);
    assert_eq!(
        tokio::time::timeout(std::time::Duration::from_secs(2), evidence)
            .await
            .unwrap()
            .unwrap(),
        "next evidence"
    );
}

#[tokio::test]
async fn abandoned_processor_keeps_its_slot_and_cleanup_waits_without_blocking_status() {
    let context = context();
    let (started, ready) = std::sync::mpsc::channel();
    let (release, finish) = std::sync::mpsc::channel();
    let mut worker = Box::pin(context.process(move || {
        started.send(()).unwrap();
        finish
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        Ok(())
    }));
    assert!(futures_util::poll!(&mut worker).is_pending());
    ready
        .recv_timeout(std::time::Duration::from_secs(2))
        .unwrap();
    drop(worker);
    // A public read waits PROCESSING_WAIT for the held slot. Advance only that
    // wait on a paused clock, just past it because timers round up to the next
    // millisecond; the worker's hold stays on real time.
    tokio::time::pause();
    let mut overloaded = Box::pin(context.process(|| Ok(())));
    assert!(futures_util::poll!(&mut overloaded).is_pending());
    tokio::time::advance(PROCESSING_WAIT + std::time::Duration::from_millis(1)).await;
    assert_eq!(overloaded.await.unwrap_err().code, "OVERLOADED");
    tokio::time::resume();
    let status = tokio::time::timeout(std::time::Duration::from_secs(1), context.status())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(status["state"], "ready");
    let closing = context.gate.close();
    tokio::pin!(closing);
    assert!(futures_util::poll!(&mut closing).is_pending());
    release.send(()).unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(2), closing)
        .await
        .unwrap();
}

#[cfg(target_os = "macos")]
#[tokio::test]
async fn status_reports_a_lost_device_transport_instead_of_ready() {
    let serial = format!("Acme-unplugged-{}", uuid::Uuid::new_v4());
    let mut sources = crate::device_media::Sources::default();
    let source = sources.fixture_failed_stream(&serial);
    let mut context = context();
    context.control_path = "hid".into();
    context.video = Some(Arc::downgrade(&source));
    // The fixture's receive loop records its transport error asynchronously.
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
    while source.transport_error().is_none() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "fixture stream did not fail"
        );
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    let status = context.status().await.unwrap();
    assert_eq!(status["state"], "lost", "{status}");
    assert_eq!(status["reason"], "DEVICE_DISCONNECTED");
    assert_eq!(status["nextAction"], "connect");
    // A dropped source is lost too, never reported ready.
    drop(source);
    drop(sources);
    assert_eq!(context.status().await.unwrap()["state"], "lost");
}

#[test]
fn usb_inventory_marks_absent_and_replugged_devices_lost() {
    let mut target = crate::targets::qa_fixture_target();
    target.platform_id = "Acme-phone".into();
    target.epoch = "7".into();
    let device = |id, serial: &str| crate::usbmux::Device {
        id,
        serial: serial.into(),
    };
    assert!(usb_presence(&target, &[device(7, "Acme-phone")]).is_none());
    assert_eq!(
        usb_presence(&target, &[device(3, "Acme-other")])
            .unwrap()
            .code,
        "USB_UNAVAILABLE"
    );
    assert_eq!(usb_presence(&target, &[]).unwrap().code, "USB_UNAVAILABLE");
    assert_eq!(
        usb_presence(&target, &[device(8, "Acme-phone")])
            .unwrap()
            .code,
        "STALE_TARGET"
    );
}
