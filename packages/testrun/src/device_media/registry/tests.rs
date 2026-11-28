//! Registry retention, cancelled cleanup and failed remote release fixtures.
use super::*;
use crate::device_media::{lifecycle::JoinState, source::recording_fixture};
use std::{sync::Mutex, time::Duration};
use tokio::sync::{broadcast, mpsc, oneshot, watch};
use uuid::Uuid;

#[tokio::test]
async fn startup_refusal_without_allocation_does_not_retain_media_exclusion() {
    let serial = format!("Acme-refused-{}", Uuid::new_v4());
    let target = json!({"targetId":serial,"generation":"fixture"});
    let mut registry = Sources::default();
    let error = registry
        .acquire_with(&serial, &target, |_| async {
            Err(Error::new("USB_UNAVAILABLE", "Acme USB fixture absent"))
        })
        .await
        .err()
        .unwrap();
    assert_eq!(error.code, "USB_UNAVAILABLE");
    assert!(crate::lease::ControlLease::acquire(&format!("device-media:{serial}")).is_ok());
    registry.shutdown().await.unwrap();
}

#[tokio::test]
async fn cancelled_startup_retains_ownership_then_releases_without_another_call() {
    let serial = format!("Acme-startup-{}", Uuid::new_v4());
    let lease_name = format!("device-media:{serial}");
    let target = json!({"targetId":format!("device:{serial}"),"generation":"fixture"});
    let allocated = Arc::new(tokio::sync::Notify::new());
    let allocated_job = allocated.clone();
    let ready = Arc::new(tokio::sync::Notify::new());
    let ready_job = ready.clone();
    let mut registry = Sources::default();
    {
        let startup = registry.acquire_with(&serial, &target, move |ownership| async move {
            allocated_job.notify_one();
            ready_job.notified().await;
            let source = Arc::try_unwrap(recording_fixture()).ok().unwrap();
            ownership.streaming(source.lifecycle.clone());
            Ok(source)
        });
        tokio::pin!(startup);
        tokio::select! {
            result = &mut startup => panic!("Startup completed before readiness: {}", result.is_ok()),
            () = allocated.notified() => {}
        }
    }
    // A cancelled acquisition must not free exclusion while its allocation is pending.
    assert_eq!(
        crate::lease::ControlLease::acquire(&lease_name)
            .err()
            .map(|e| e.code),
        Some("TARGET_BUSY".into())
    );
    // Cancelling a shutdown waiter must retain the same acquisition task too.
    assert!(
        tokio::time::timeout(Duration::from_millis(10), registry.shutdown())
            .await
            .is_err()
    );
    assert!(crate::lease::ControlLease::acquire(&lease_name).is_err());
    ready.notify_one();
    tokio::time::timeout(Duration::from_secs(2), async {
        while crate::lease::ControlLease::acquire(&lease_name).is_err() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("Abandoned startup must finish release without another registry call");
    registry.shutdown().await.unwrap();
    assert!(crate::lease::ControlLease::acquire(&lease_name).is_ok());
}

#[tokio::test]
async fn cancelled_allocation_retries_only_owned_release_and_preserves_failure() {
    use crate::device_media::lifecycle::Release;
    use std::sync::atomic::{AtomicUsize, Ordering};
    struct Remote(AtomicUsize);
    impl Release for Remote {
        fn verify(
            &self,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
            Box::pin(async {
                if self.0.fetch_add(1, Ordering::SeqCst) < 2 {
                    Err(Error::new(
                        "CLEANUP_FAILED",
                        "Acme allocation still releasing",
                    ))
                } else {
                    Ok(())
                }
            })
        }
    }
    let serial = format!("Acme-cancel-{}", Uuid::new_v4());
    let lease_name = format!("device-media:{serial}");
    let foreign_name = format!("device-media:Acme-foreign-{}", Uuid::new_v4());
    let foreign = crate::lease::ControlLease::acquire(&foreign_name).unwrap();
    let remote = Arc::new(Remote(AtomicUsize::new(0)));
    let witness = remote.clone();
    let allocated = Arc::new(tokio::sync::Notify::new());
    let allocated_job = allocated.clone();
    let settle = Arc::new(tokio::sync::Notify::new());
    let settle_job = settle.clone();
    let target = json!({"targetId":serial,"generation":"fixture"});
    let mut registry = Sources::default();
    {
        let startup = registry.acquire_with(&serial, &target, move |ownership| async move {
            ownership.allocated(witness);
            allocated_job.notify_one();
            settle_job.notified().await;
            Err(Error::new(
                "DEVICE_DISCONNECTED",
                "Acme allocation transport ended",
            ))
        });
        tokio::pin!(startup);
        tokio::select! {
            result = &mut startup => panic!("Unexpected completion: {}",result.is_ok()),
            () = allocated.notified() => {}
        }
    }
    settle.notify_one();
    let failed = registry.shutdown().await.unwrap_err();
    assert!(
        serde_json::to_string(&failed)
            .unwrap()
            .contains("Acme allocation still releasing")
    );
    assert!(crate::lease::ControlLease::acquire(&lease_name).is_err());
    registry.shutdown().await.unwrap();
    assert_eq!(remote.0.load(Ordering::SeqCst), 3);
    assert!(crate::lease::ControlLease::acquire(&lease_name).is_ok());
    assert!(crate::lease::ControlLease::acquire(&foreign_name).is_err());
    drop(foreign);
}

impl Sources {
    pub(crate) fn fixture_failed_stream(&mut self, serial: &str) -> Arc<Source> {
        self.fixture_source(serial, false)
    }
    pub(crate) fn fixture_permanent_source_failure(&mut self, serial: &str) -> Arc<Source> {
        self.fixture_source(serial, true)
    }
    fn fixture_source(&mut self, serial: &str, permanent: bool) -> Arc<Source> {
        let mut source = recording_fixture();
        let diagnostic = Arc::new(Mutex::new(None));
        let job_error = diagnostic.clone();
        let lease = crate::lease::ControlLease::acquire(&format!("device-media:{serial}")).unwrap();
        let job = tokio::spawn(async move {
            let _lease = lease;
            if permanent {
                return Err(Error::new(
                    "CLEANUP_FAILED",
                    "Fixture permanent source failure",
                ));
            }
            super::super::stream::finish_release(
                Err(Error::new("DEVICE_DISCONNECTED", "Fixture receive failed")),
                Ok(()),
                &job_error,
            )
        });
        Arc::get_mut(&mut source).unwrap().lifecycle = Arc::new(Lifecycle {
            stream_error: diagnostic,
            remote: None,
            observation: None,
            stop: Mutex::new(None),
            join: tokio::sync::Mutex::new(JoinState {
                job: Some(job),
                result: None,
            }),
        });
        self.sources.insert(
            serial.into(),
            Entry {
                ownership: None,
                source: Arc::downgrade(&source),
                lifecycle: source.lifecycle.clone(),
            },
        );
        source
    }
}

#[tokio::test]
async fn audio_shares_registry_video_without_a_hid_source_in_both_start_orders() {
    for video_first in [true, false] {
        let serial = format!("Acme-{}", Uuid::new_v4());
        let identity = json!({"targetId":format!("device:{serial}"),"generation":"fixture"});
        let mut source = recording_fixture();
        Arc::get_mut(&mut source).unwrap().target = identity.clone();
        let mut registry = Sources::default();
        let mut playback = crate::playback::Owners::default();
        // XCTest keeps preview/recording sources in this registry, with no HID video field.
        let lease_name = format!("device-media:{serial}");
        let lease = crate::lease::ControlLease::acquire(&lease_name).unwrap();
        if !video_first {
            assert!(registry.existing(&serial, &identity).unwrap().is_none());
            playback
                .prepare_audio("fixture-controller", &serial, None)
                .unwrap();
        }
        registry.sources.insert(
            serial.clone(),
            Entry {
                ownership: None,
                source: Arc::downgrade(&source),
                lifecycle: source.lifecycle.clone(),
            },
        );
        assert_eq!(Arc::strong_count(&source), 1);
        playback
            .prepare_audio(
                "fixture-controller",
                &serial,
                registry.existing(&serial, &identity).unwrap(),
            )
            .unwrap();
        assert_eq!(Arc::strong_count(&source), 1);
        assert_eq!(Arc::weak_count(&source), 2);
        assert_eq!(
            registry
                .existing(&serial, &json!({"generation":"foreign"}))
                .err()
                .unwrap()
                .code,
            "STALE_TARGET"
        );
        // Repeated preparation uses the same source instead of acquiring a competing lease.
        playback
            .prepare_audio(
                "fixture-controller",
                &serial,
                registry.existing(&serial, &identity).unwrap(),
            )
            .unwrap();
        assert_eq!(Arc::strong_count(&source), 1);
        assert_eq!(Arc::weak_count(&source), 2);
        playback.close("fixture-controller").await.unwrap();
        assert_eq!(Arc::strong_count(&source), 1);
        assert_eq!(Arc::weak_count(&source), 1);
        assert_eq!(
            crate::lease::ControlLease::acquire(&lease_name)
                .err()
                .unwrap()
                .code,
            "TARGET_BUSY"
        );
        drop(lease);
        drop(source);
        registry.release_unused().await.unwrap();
        assert!(registry.existing(&serial, &identity).unwrap().is_none());
    }
}

#[tokio::test]
async fn registry_does_not_keep_capture_alive_after_last_consumer() {
    let (stop, signal) = oneshot::channel();
    let (observed, done) = oneshot::channel();
    let job = tokio::spawn(async move {
        signal.await.unwrap();
        let _ = observed.send(());
        Ok(())
    });
    let lifecycle = Arc::new(Lifecycle {
        stream_error: Default::default(),
        remote: None,
        observation: None,
        stop: Mutex::new(Some(stop)),
        join: tokio::sync::Mutex::new(JoinState {
            job: Some(job),
            result: None,
        }),
    });
    let (frames, _) = broadcast::channel(64);
    let (_, latest) = watch::channel(None);
    let (commands, _) = mpsc::channel(8);
    let observation = super::super::observation::Observation::start(
        Err(Error::new("CAPABILITY_UNAVAILABLE", "Fixture")),
        commands.clone(),
    )
    .unwrap();
    let source = Arc::new(Source {
        audio: None,
        device_info: None,
        consumer: Err(Error::new(
            "MEDIA_UNSUPPORTED",
            "Fixture has no consumer channel",
        )),
        display_geometry: Err(Error::new(
            "CAPABILITY_UNAVAILABLE",
            "Fixture has no calibration",
        )),
        hid: Err(Error::new("CAPABILITY_UNAVAILABLE", "Fixture has no HID")),
        observation,
        target: json!({}),
        frames,
        latest,
        commands,
        lifecycle: lifecycle.clone(),
    });
    let mut registry = Sources::default();
    registry.sources.insert(
        "Acme".into(),
        Entry {
            ownership: None,
            source: Arc::downgrade(&source),
            lifecycle,
        },
    );
    let second = source.clone();
    drop(source);
    assert_eq!(registry.sources["Acme"].source.strong_count(), 1);
    drop(second);
    tokio::time::timeout(Duration::from_secs(1), done)
        .await
        .unwrap()
        .unwrap();
    registry.release_unused().await.unwrap();
    assert!(registry.sources.is_empty());
}
#[tokio::test]
async fn cancelled_close_keeps_cleanup_owned_until_remote_release() {
    let target = format!("device-media:Acme-{}", Uuid::new_v4());
    let lease = crate::lease::ControlLease::acquire(&target).unwrap();
    let (stop, signal) = oneshot::channel();
    let (release, released) = oneshot::channel();
    let job = tokio::spawn(async move {
        let _lease = lease;
        signal.await.unwrap();
        released.await.unwrap();
        Ok(())
    });
    let lifecycle = Arc::new(Lifecycle {
        stream_error: Default::default(),
        remote: None,
        observation: None,
        stop: Mutex::new(Some(stop)),
        join: tokio::sync::Mutex::new(JoinState {
            job: Some(job),
            result: None,
        }),
    });
    let mut source = recording_fixture();
    Arc::get_mut(&mut source).unwrap().lifecycle = lifecycle;
    let mut registry = Sources::default();
    registry.sources.insert(
        "Acme".into(),
        Entry {
            ownership: None,
            source: Arc::downgrade(&source),
            lifecycle: source.lifecycle.clone(),
        },
    );
    drop(source);
    assert!(
        tokio::time::timeout(Duration::from_millis(30), registry.release_unused())
            .await
            .is_err()
    );
    assert!(registry.sources.contains_key("Acme"));
    assert!(
        crate::lease::ControlLease::inspect(&target)
            .unwrap()
            .unwrap()
            .0
    );
    let close = registry.shutdown();
    tokio::pin!(close);
    assert!(
        tokio::time::timeout(Duration::from_millis(30), &mut close)
            .await
            .is_err()
    );
    release.send(()).unwrap();
    close.await.unwrap();
    assert!(
        !crate::lease::ControlLease::inspect(&target)
            .unwrap()
            .unwrap()
            .0
    );
}
#[tokio::test]
async fn failed_source_cleanup_releases_its_kernel_lease_and_retains_the_error() {
    let target = format!("device-media:Acme-{}", Uuid::new_v4());
    let lease = crate::lease::ControlLease::acquire(&target).unwrap();
    let (stop, signal) = oneshot::channel();
    let job = tokio::spawn(async move {
        let _lease = lease;
        let _ = signal.await;
        Err(Error::new(
            "CLEANUP_FAILED",
            "Fixture remote cleanup failed",
        ))
    });
    let lifecycle = Arc::new(Lifecycle {
        stream_error: Default::default(),
        remote: None,
        observation: None,
        stop: Mutex::new(Some(stop)),
        join: tokio::sync::Mutex::new(JoinState {
            job: Some(job),
            result: None,
        }),
    });
    let mut source = recording_fixture();
    Arc::get_mut(&mut source).unwrap().lifecycle = lifecycle.clone();
    let mut registry = Sources::default();
    registry.sources.insert(
        "Acme".into(),
        Entry {
            ownership: None,
            source: Arc::downgrade(&source),
            lifecycle: lifecycle.clone(),
        },
    );
    drop(source);
    assert!(
        registry
            .release_unused()
            .await
            .unwrap_err()
            .details
            .unwrap()
            .to_string()
            .contains("Fixture remote cleanup failed")
    );
    assert!(registry.sources.contains_key("Acme"));
    assert!(registry.release_unused().await.is_err());
    // A retained error must not become another target's cleanup outcome.
    let healthy = recording_fixture();
    registry.sources.insert(
        "Acme-healthy".into(),
        Entry {
            ownership: None,
            source: Arc::downgrade(&healthy),
            lifecycle: healthy.lifecycle.clone(),
        },
    );
    drop(healthy);
    registry.release_unused_for("Acme-healthy").await.unwrap();
    assert!(registry.sources.contains_key("Acme"));
    assert!(!registry.sources.contains_key("Acme-healthy"));
    crate::lease::ControlLease::acquire(&target).unwrap();
    assert_eq!(
        lifecycle.close().await.unwrap_err().message,
        "Fixture remote cleanup failed"
    );
}

#[tokio::test]
async fn panicked_source_retains_remote_identity_for_read_only_release_verification() {
    use crate::device_media::lifecycle::Release;
    use std::sync::atomic::{AtomicUsize, Ordering};
    struct Monitor(AtomicUsize);
    impl Release for Monitor {
        fn verify(
            &self,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
            Box::pin(async {
                if self.0.fetch_add(1, Ordering::SeqCst) == 0 {
                    return Err(Error::new("CLEANUP_FAILED", "Acme remote is still present"));
                }
                Ok(())
            })
        }
    }
    let monitor = Arc::new(Monitor(AtomicUsize::new(0)));
    let mut source = recording_fixture();
    let lifecycle = Arc::get_mut(&mut Arc::get_mut(&mut source).unwrap().lifecycle).unwrap();
    lifecycle.remote = Some(monitor.clone());
    lifecycle.join.get_mut().result = None;
    lifecycle.join.get_mut().job = Some(tokio::spawn(async { panic!("Acme source task panic") }));
    let mut registry = Sources::default();
    registry.sources.insert(
        "Acme".into(),
        Entry {
            ownership: None,
            source: Arc::downgrade(&source),
            lifecycle: source.lifecycle.clone(),
        },
    );
    drop(source);
    // The first close asks the device at once; a still-present session keeps
    // the task failure and the read-only verifier's cause.
    let failure = registry.release_unused().await.unwrap_err();
    let details = failure.details.unwrap().to_string();
    assert!(details.contains("Media source task failed"), "{details}");
    assert!(
        details.contains("Acme remote is still present"),
        "{details}"
    );
    assert_eq!(monitor.0.load(Ordering::SeqCst), 1);
    assert!(registry.sources.contains_key("Acme"));
    registry.release_unused().await.unwrap();
    assert!(registry.sources.is_empty());
    assert_eq!(monitor.0.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn transient_remote_release_is_reverified_on_first_close_without_restarting_the_source() {
    use crate::device_media::lifecycle::Release;
    use std::sync::atomic::{AtomicUsize, Ordering};
    struct Monitor(AtomicUsize);
    impl Release for Monitor {
        fn verify(
            &self,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
            Box::pin(async {
                self.0.fetch_add(1, Ordering::SeqCst);
                Ok(())
            })
        }
    }
    let monitor = Arc::new(Monitor(AtomicUsize::new(0)));
    let foreign = format!("device-media:Acme-foreign-{}", Uuid::new_v4());
    let _foreign_lease = crate::lease::ControlLease::acquire(&foreign).unwrap();
    let mut source = recording_fixture();
    let lifecycle = &mut Arc::get_mut(&mut source).unwrap().lifecycle;
    let lifecycle_mut = Arc::get_mut(lifecycle).unwrap();
    lifecycle_mut.remote = Some(monitor.clone());
    // The in-flight release failed while USB was briefly unavailable, as after
    // an unplug. The first close rechecks the owned UUID through the read-only
    // verifier rather than restarting the source or reporting the stale failure.
    lifecycle_mut.join.get_mut().result = None;
    lifecycle_mut.join.get_mut().job = Some(tokio::spawn(async {
        super::super::stream::finish_release(
            Err(Error::new("DEVICE_DISCONNECTED", "Acme transport closed")),
            Err(Error::new(
                "USB_UNAVAILABLE",
                "Selected device has no USB connection",
            )),
            &Mutex::new(None),
        )
    }));
    let lifecycle = lifecycle.clone();
    let mut registry = Sources::default();
    registry.sources.insert(
        "Acme".into(),
        Entry {
            ownership: None,
            source: Arc::downgrade(&source),
            lifecycle,
        },
    );
    drop(source);
    registry.release_unused().await.unwrap();
    assert!(registry.sources.is_empty());
    assert_eq!(monitor.0.load(Ordering::SeqCst), 1);
    // A settled release is not checked again.
    registry.release_unused().await.unwrap();
    assert_eq!(monitor.0.load(Ordering::SeqCst), 1);
    assert!(
        crate::lease::ControlLease::inspect(&foreign)
            .unwrap()
            .unwrap()
            .0
    );
}
