//! Host wait and release acknowledgements without Android processes or native decode.
use super::*;
use crate::android::streaming::Source;

struct WorkerThread {
    sender: Option<mpsc::SyncSender<Work>>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl WorkerThread {
    fn start(worker: Worker) -> Self {
        assert!(worker.producer.is_none());
        let Worker {
            serial,
            adb,
            health,
            ..
        } = worker;
        let (sender, receiver) = mpsc::sync_channel(4);
        Self {
            sender: Some(sender),
            thread: Some(std::thread::spawn(move || {
                let mut worker = Worker::new(serial, health);
                worker.adb = adb;
                worker.run(receiver);
            })),
        }
    }
    fn send(&self, work: Work) {
        assert!(
            self.sender.as_ref().unwrap().send(work).is_ok(),
            "Worker exited before its request"
        );
    }
    fn finish(mut self) {
        self.sender.take();
        self.thread.take().unwrap().join().unwrap();
    }
}
impl Drop for WorkerThread {
    fn drop(&mut self) {
        self.sender.take();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn process_fixture() -> (tempfile::TempDir, Worker) {
    use std::os::unix::fs::PermissionsExt;
    let directory = tempfile::tempdir().unwrap();
    let adb = directory.path().join("adb");
    std::fs::write(&adb, r#"#!/bin/sh
root=$(dirname "$0")
case "$5" in
  *'dd bs=1'*)
    echo launch >> "$root/calls"
    [ -f "$root/healthy" ] || exit 7
    count=$(printf '%s\n' "$5" | sed -n 's/^dd bs=1 count=\([0-9]*\).*/\1/p')
    nonce=$(printf '%s\n' "$5" | sed -n "s/^nonce='\([^']*\)'/\1/p")
    head -c "$count" > /dev/null
    cat "$root/header"
    printf '{"version":1,"nonce":"%s","width":100,"height":200,"sourceFence":"surface_acquisition"}' "$nonce"
    while IFS= read -r control; do
      case "$control" in S) touch "$root/stopped"; exit 0 ;; esac
    done
    ;;
  *) echo release >> "$root/calls"; [ ! -f "$root/fail-release" ] ;;
esac
"#).unwrap();
    std::fs::set_permissions(&adb, std::fs::Permissions::from_mode(0o700)).unwrap();
    let hello = format!(
        r#"{{"version":1,"nonce":"testrun-capture-{}","width":100,"height":200,"sourceFence":"surface_acquisition"}}"#,
        "0".repeat(32)
    );
    let mut header = b"TRV1".to_vec();
    header.extend_from_slice(&4_u32.to_be_bytes());
    header.extend_from_slice(&(hello.len() as u32).to_be_bytes());
    header.extend_from_slice(&[0; 20]);
    std::fs::write(directory.path().join("header"), header).unwrap();
    let mut worker = Worker::new("Acme-worker".into(), Arc::new(Mutex::new(json!({}))));
    worker.adb = adb;
    (directory, worker)
}

#[test]
fn worker_starts_a_healthy_transport_and_closes_it_before_acknowledging_release() {
    let (directory, worker) = process_fixture();
    std::fs::write(directory.path().join("healthy"), "").unwrap();
    let health = worker.health.clone();
    let worker = WorkerThread::start(worker);
    let (reply, result) = oneshot::channel();
    worker.send(Work::Capture((100, 200), reply));
    // A valid handshake without any decoded frame must return no retained image.
    assert!(result.blocking_recv().unwrap().unwrap().is_none());
    assert_eq!(health.lock().unwrap()["state"], "healthy");
    assert_eq!(health.lock().unwrap()["generation"], 1);
    let (reply, result) = oneshot::channel();
    worker.send(Work::Close("closed", reply));
    result.blocking_recv().unwrap().unwrap();
    assert!(directory.path().join("stopped").exists());
    worker.finish();
    assert_eq!(
        std::fs::read_to_string(directory.path().join("calls")).unwrap(),
        "launch\nrelease\n"
    );
}

#[test]
fn worker_retries_one_failed_producer_then_latches_raw_and_acknowledges_close() {
    let (directory, worker) = process_fixture();
    let health = worker.health.clone();
    let worker = WorkerThread::start(worker);
    for _ in 0..2 {
        let (reply, result) = oneshot::channel();
        worker.send(Work::Capture((100, 200), reply));
        assert!(result.blocking_recv().unwrap().unwrap().is_none());
    }
    assert_eq!(health.lock().unwrap()["state"], "raw_fallback");
    assert_eq!(health.lock().unwrap()["generation"], 2);
    let (reply, result) = oneshot::channel();
    worker.send(Work::Close("closed", reply));
    result.blocking_recv().unwrap().unwrap();
    worker.finish();
    let calls = std::fs::read_to_string(directory.path().join("calls")).unwrap();
    assert_eq!(calls.lines().filter(|line| *line == "launch").count(), 2);
    assert_eq!(calls.lines().filter(|line| *line == "release").count(), 2);
}

#[test]
fn worker_retains_failed_release_until_the_same_producer_is_cleaned() {
    let (directory, mut worker) = process_fixture();
    std::fs::write(directory.path().join("fail-release"), "").unwrap();
    assert_eq!(
        worker.capture((100, 200)).err().unwrap().code,
        "CLEANUP_FAILED"
    );
    assert_eq!(worker.health.lock().unwrap()["state"], "cleanup_pending");
    assert!(worker.producer.is_some());
    assert_eq!(
        worker.generation, 1,
        "Release failure must prevent a replacement producer"
    );
    std::fs::remove_file(directory.path().join("fail-release")).unwrap();
    worker.stop().unwrap();
    assert!(worker.producer.is_none());
    let calls = std::fs::read_to_string(directory.path().join("calls")).unwrap();
    assert_eq!(calls.lines().filter(|line| *line == "launch").count(), 1);
    assert_eq!(calls.lines().filter(|line| *line == "release").count(), 2);
}

fn fixture() -> (Source, mpsc::Receiver<Work>) {
    let (sender, receiver) = mpsc::sync_channel(4);
    let host = Host {
        sender,
        health: Arc::new(Mutex::new(
            json!({"requested":"stream","state":"recovering"}),
        )),
        admission: tokio::sync::Mutex::new(()),
        failed: AtomicBool::new(false),
        closing: Mutex::new(None),
    };
    (
        Source {
            mode: Mode::Stream,
            serial: "Acme".into(),
            admission: tokio::sync::Mutex::new(()),
            host,
        },
        receiver,
    )
}

async fn expired_capture(
    source: &Source,
    receiver: &mpsc::Receiver<Work>,
) -> oneshot::Sender<Result<()>> {
    let error = source
        .host
        .snapshot_with_budget((100, 200), Duration::ZERO)
        .await
        .err()
        .unwrap();
    assert_eq!(error.code, "CAPTURE_TIMEOUT");
    let Work::Capture(_, reply) = receiver.try_recv().unwrap() else {
        panic!("Expected capture")
    };
    assert!(
        reply.is_closed(),
        "A late image cannot reach the expired caller"
    );
    let Work::Close(reason, reply) = receiver.try_recv().unwrap() else {
        panic!("Expected owned release")
    };
    assert_eq!(reason, "host_deadline");
    reply
}

#[tokio::test]
async fn host_deadline_keeps_release_owned_then_restores_raw_fallback() {
    let (source, receiver) = fixture();
    let reply = expired_capture(&source, &receiver).await;
    let status = source.status();
    assert_eq!(status["state"], "recovering");
    assert_eq!(status["releasePending"], true);
    assert_eq!(source.safe_for_raw().unwrap_err().code, "CAPTURE_PENDING");
    assert_eq!(
        source.host.snapshot((100, 200)).await.err().unwrap().code,
        "CAPTURE_PENDING"
    );
    assert!(matches!(
        receiver.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));

    // The worker finishes its slow retry, discards its late image and verifies
    // release before acknowledging the queued close.
    *source.host.health.lock().unwrap() = json!({"requested":"stream","state":"raw_fallback"});
    reply.send(Ok(())).unwrap();
    assert!(source.safe_for_raw().is_ok());
    assert_eq!(source.status()["state"], "raw_fallback");
    assert_ne!(source.status()["releasePending"], true);
    assert!(!source.host.failed.load(Ordering::Acquire));
}

#[tokio::test]
async fn close_deadlines_retain_one_release_acknowledgement() {
    let (source, receiver) = fixture();
    for _ in 0..2 {
        let error = source
            .host
            .invalidate_with_budget("closed", Duration::ZERO)
            .await
            .unwrap_err();
        assert_eq!(error.code, "CAPTURE_TIMEOUT");
        assert_eq!(source.safe_for_raw().unwrap_err().code, "CAPTURE_PENDING");
    }
    let Work::Close(_, reply) = receiver.try_recv().unwrap() else {
        panic!("Expected close")
    };
    assert!(matches!(
        receiver.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
    *source.host.health.lock().unwrap() = json!({"requested":"stream","state":"raw_fallback"});
    reply.send(Ok(())).unwrap();
    assert!(source.safe_for_raw().is_ok());
}

#[tokio::test]
async fn a_failed_release_blocks_raw_until_a_successful_close_retry() {
    let (source, receiver) = fixture();
    let reply = expired_capture(&source, &receiver).await;
    *source.host.health.lock().unwrap() = json!({"requested":"stream","state":"cleanup_pending"});
    reply.send(Err(cleanup())).unwrap();
    assert_eq!(source.safe_for_raw().unwrap_err().code, "CLEANUP_FAILED");
    assert_eq!(source.status()["state"], "cleanup_pending");
    source.host.queue_close("closed").unwrap();
    let Work::Close(_, reply) = receiver.try_recv().unwrap() else {
        panic!("Expected retry")
    };
    *source.host.health.lock().unwrap() = json!({"requested":"stream","state":"raw_fallback"});
    reply.send(Ok(())).unwrap();
    assert!(source.safe_for_raw().is_ok());
}

#[tokio::test]
async fn a_lost_worker_cannot_turn_a_deadline_into_raw_admission() {
    let (source, receiver) = fixture();
    let reply = expired_capture(&source, &receiver).await;
    drop(reply);
    assert_eq!(source.safe_for_raw().unwrap_err().code, "CLEANUP_FAILED");
    assert_eq!(source.status()["state"], "cleanup_pending");
}

#[tokio::test]
async fn latched_fallback_skips_stream_admission_and_worker_requests() {
    let (source, receiver) = fixture();
    for state in ["raw_fallback", "blocked"] {
        *source.host.health.lock().unwrap() = json!({"requested":"stream","state":state});
        // No Android target or adb fixture: a latched source must return before
        // transport, API, geometry, protection or worker admission.
        assert!(source.capture(true).await.unwrap().0.is_none());
        assert!(matches!(
            receiver.try_recv(),
            Err(mpsc::TryRecvError::Empty)
        ));
    }
}
