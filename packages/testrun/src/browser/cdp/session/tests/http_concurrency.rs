//! Native HTTP owners retain ordinary permits and settle independently.
use super::*;
use crate::http_fixtures::concurrency as wire;
fn queued(worker: &Worker, id: Uuid) {
    worker
        .shared
        .state
        .lock()
        .unwrap()
        .begin(id, "HTTP concurrency fixture".into())
        .unwrap();
}
async fn until_settled(worker: &mut Worker, id: Uuid) {
    let until = Instant::now() + Duration::from_secs(2);
    loop {
        worker.http_poll().await;
        if worker.shared.state.lock().unwrap().operations[&id].settled {
            return;
        }
        assert!(Instant::now() < until, "Native HTTP owner did not settle");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}
#[tokio::test]
async fn native_http_concurrency_publishes_running_retains_permits_and_settles_out_of_order() {
    let (mut worker, _peer, _root) = fixture().await;
    let mut site = wire::Site::start().await;
    let connection = worker.shared.state.lock().unwrap().connection;
    let client: Reference = serde_json::from_value(
        worker
            .http_contexts
            .create(
                connection,
                serde_json::from_value(json!({"baseUrl":site.url})).unwrap(),
            )
            .unwrap()["httpContext"]
            .clone(),
    )
    .unwrap();
    let capacity = Arc::new(tokio::sync::Semaphore::new(2));
    let mut ids = vec![];
    for path in ["/A", "/B"] {
        let id = Uuid::new_v4();
        queued(&worker, id);
        worker
            .perform(Command {
                id,
                request: Some(
                    serde_json::from_value(json!({"operation":"http_fetch",
            "httpContext":client,"request":{"url":path,"readBody":true}}))
                    .unwrap(),
                ),
                deadline: deadline(),
                queue_slot: Some(capacity.clone().try_acquire_owned().unwrap()),
            })
            .await;
        ids.push(id);
    }
    let mut held = site.held(2).await;
    assert_eq!(capacity.available_permits(), 0);
    for id in &ids {
        let state = worker.shared.state.lock().unwrap();
        assert_eq!(state.operations[id].outcome, Outcome::Running);
        assert!(!state.operations[id].settled);
    }
    held.remove("/A").unwrap().send(()).unwrap();
    until_settled(&mut worker, ids[0]).await;
    {
        let state = worker.shared.state.lock().unwrap();
        assert_eq!(state.operations[&ids[0]].outcome, Outcome::Succeeded);
        assert_eq!(
            state.operations[&ids[0]].result.as_ref().unwrap()["status"],
            201
        );
        assert!(
            !state.operations[&ids[1]].settled,
            "An unrelated held request cannot delay A or complete B"
        );
    }
    assert_eq!(capacity.available_permits(), 1);
    worker
        .shared
        .state
        .lock()
        .unwrap()
        .operations
        .get_mut(&ids[1])
        .unwrap()
        .cancel_requested = true;
    until_settled(&mut worker, ids[1]).await;
    let request_id: Uuid = {
        let state = worker.shared.state.lock().unwrap();
        assert_eq!(state.operations[&ids[1]].outcome, Outcome::Cancelled);
        assert_eq!(state.operations[&ids[1]].code.as_deref(), Some("CANCELLED"));
        serde_json::from_value(
            state.operations[&ids[1]].result.as_ref().unwrap()["requestId"].clone(),
        )
        .unwrap()
    };
    assert_eq!(capacity.available_permits(), 2);
    let until = Instant::now() + Duration::from_secs(2);
    while worker.http_contexts.has_pending(request_id) {
        let _ = worker.http_finalize(request_id);
        assert!(
            Instant::now() < until,
            "Cancelled HTTP worker must actually exit"
        );
        tokio::task::yield_now().await;
    }
    assert!(worker.http_requests.is_empty());
    assert_eq!(site.count.load(std::sync::atomic::Ordering::SeqCst), 2);
    let _ = held.remove("/B").unwrap().send(());
    worker.http_contexts.remove(client).unwrap();
    worker.close(deadline()).await.unwrap();
}
#[tokio::test]
async fn native_http_concurrency_cancelled_or_expired_admission_never_starts_network() {
    let (mut worker, _peer, _root) = fixture().await;
    let site = wire::Site::start().await;
    let connection = worker.shared.state.lock().unwrap().connection;
    let client: Reference = serde_json::from_value(
        worker
            .http_contexts
            .create(
                connection,
                serde_json::from_value(json!({"baseUrl":site.url})).unwrap(),
            )
            .unwrap()["httpContext"]
            .clone(),
    )
    .unwrap();
    for (cancel, code) in [(true, "CANCELLED"), (false, "NOT_DISPATCHED")] {
        let id = Uuid::new_v4();
        queued(&worker, id);
        worker
            .shared
            .state
            .lock()
            .unwrap()
            .operations
            .get_mut(&id)
            .unwrap()
            .cancel_requested = cancel;
        worker
            .perform(Command {
                id,
                request: Some(
                    serde_json::from_value(json!({"operation":"http_fetch",
            "httpContext":client,"request":{"url":"/A"}}))
                    .unwrap(),
                ),
                deadline: if cancel { deadline() } else { Instant::now() },
                queue_slot: None,
            })
            .await;
        assert_eq!(
            worker.shared.state.lock().unwrap().operations[&id]
                .code
                .as_deref(),
            Some(code)
        );
    }
    assert!(worker.http_requests.is_empty());
    assert_eq!(site.count.load(std::sync::atomic::Ordering::SeqCst), 0);
    worker.http_contexts.remove(client).unwrap();
    worker.close(deadline()).await.unwrap();
}
#[tokio::test]
async fn native_http_concurrency_responsive_admission_publishes_running_without_dispatch_on_refusal()
 {
    let (mut worker, _peer, _root) = fixture().await;
    let id = Uuid::new_v4();
    queued(&worker, id);
    worker.http_running(id, deadline()).await.unwrap();
    assert_eq!(
        worker.shared.state.lock().unwrap().operations[&id].outcome,
        Outcome::Running
    );
    let denied = Uuid::new_v4();
    queued(&worker, denied);
    worker
        .shared
        .state
        .lock()
        .unwrap()
        .operations
        .get_mut(&denied)
        .unwrap()
        .cancel_requested = true;
    assert_eq!(
        worker
            .http_running(denied, deadline())
            .await
            .unwrap_err()
            .code,
        "CANCELLED"
    );
    assert_eq!(
        worker.shared.state.lock().unwrap().operations[&denied].outcome,
        Outcome::Queued
    );
    assert_eq!(worker.connection.as_ref().unwrap().status().pending, 0);
    worker.close(deadline()).await.unwrap();
}
