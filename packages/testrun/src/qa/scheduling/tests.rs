//! FIFO admission and cancellation without input dispatch.
use super::*;

#[tokio::test]
async fn later_target_failures_and_run_end_preserve_closed_history() {
    let runtimes = Runtimes::new(Arc::default(), super::super::reads::Processing::default());
    let root = tempfile::tempdir().unwrap();
    let evidence = root.path().join("evidence");
    crate::artifacts::directory(&evidence).unwrap();
    runtimes.state.lock().unwrap().sessions.insert(
        "Acme-session".into(),
        SessionRecord {
            target: "android:Acme-retired".into(),
            run: "Acme-run".into(),
            goal: "acme".into(),
            read: None,
            status: json!({"state":"closed","closed":true,"schedulingError":{"code":"CLEANUP_FAILED","message":"Acme original failure"}}),
            archive: Archive::for_session(&evidence, "Acme-session").unwrap(),
        },
    );
    runtimes.failed(
        "android:Acme-retired",
        Error::new("CLEANUP_FAILED", "Acme later failure"),
    );
    runtimes.finish_run("Acme-run").await.unwrap();
    let status = runtimes
        .status(&json!({"runId":"Acme-run","goalId":"acme","sessionId":"Acme-session"}))
        .unwrap()
        .unwrap();
    assert_eq!(status["closed"], true, "{status}");
    assert_eq!(status["state"], "closed");
    assert_eq!(
        status["schedulingError"]["message"],
        "Acme original failure"
    );
}

#[tokio::test]
async fn retirement_keeps_queued_and_unsettled_owners_until_cleanup_is_verified() {
    let runtimes = Runtimes::new(Arc::default(), super::super::reads::Processing::default());
    let evidence = tempfile::tempdir().unwrap();
    runtimes.register(
        "Acme-run",
        &Run {
            goals: HashSet::from(["acme".into()]),
            evidence: evidence.path().into(),
            end_attempted: false,
        },
    );
    let client = crate::managed::Client::default();
    let args = |index| json!({"runId":"Acme-run","goalId":"acme","target":{"targetId":format!("android:Acme-{index}"),"generation":"fixture"},"mode":"control"});
    let mut queued = (0..16)
        .map(|index| {
            runtimes
                .prepare("session_open", &args(index), &client)
                .unwrap()
                .unwrap()
        })
        .collect::<Vec<_>>();
    assert_eq!(
        runtimes
            .prepare("session_open", &args(16), &client)
            .unwrap()
            .err()
            .unwrap()
            .code,
        "RESOURCE_LIMIT"
    );
    let target = queued[0].target.clone();
    runtimes.set_idle(&target, false);
    drop(queued.remove(0));
    runtimes.retire_idle();
    assert_eq!(runtimes.state.lock().unwrap().lanes.len(), 16);
    runtimes.failed(
        &target,
        Error::new("CLEANUP_FAILED", "owned fixture cleanup"),
    );
    runtimes.retire_idle();
    assert_eq!(runtimes.state.lock().unwrap().lanes.len(), 16);
    runtimes.set_idle(&target, true);
    runtimes.retire_idle();
    assert_eq!(runtimes.state.lock().unwrap().lanes.len(), 15);
    assert!(
        runtimes
            .prepare("session_open", &args(16), &client)
            .unwrap()
            .is_ok()
    );
    // Dropping requests before dispatch leaves pristine owners reclaimable.
    drop(queued);
    runtimes.retire_idle();
    assert!(runtimes.state.lock().unwrap().lanes.is_empty());
}

#[tokio::test]
async fn queue_includes_active_work_and_cancelled_waiters_cannot_reorder_neighbors() {
    let queue = Arc::new(Queue::default());
    let first = queue.reserve(3).unwrap();
    let cancelled = queue.reserve(3).unwrap();
    let neighbor = queue.reserve(3).unwrap();
    assert_eq!(queue.reserve(3).err().unwrap().code, "OVERLOADED");
    first.ready().await;
    let mut waiting = Box::pin(neighbor.ready());
    assert!(futures_util::poll!(&mut waiting).is_pending());
    drop(cancelled);
    assert!(futures_util::poll!(&mut waiting).is_pending());
    drop(first);
    waiting.await;
    let next = queue.reserve(3).unwrap();
    let blocked = next.ready();
    tokio::pin!(blocked);
    assert!(futures_util::poll!(&mut blocked).is_pending());
    drop(neighbor);
    blocked.await;
}

#[tokio::test]
async fn queued_work_rechecks_cleanup_failure_before_dispatch() {
    let runtimes = Runtimes::new(Arc::default(), super::super::reads::Processing::default());
    let queue = Arc::new(Queue::default());
    let predecessor = queue.reserve(8).unwrap();
    let waiting = queue.reserve(8).unwrap();
    assert!(runtimes.target_ready("android:one", "tap").is_ok());
    runtimes.failed("android:one", Error::new("CLEANUP_FAILED", "held fixture"));
    drop(predecessor);
    waiting.ready().await;
    assert_eq!(
        runtimes
            .target_ready("android:one", "tap")
            .unwrap_err()
            .code,
        "SESSION_CLOSING"
    );
    assert!(runtimes.target_ready("android:two", "tap").is_ok());
    assert!(
        runtimes
            .target_ready("android:one", "session_close")
            .is_ok()
    );
}
