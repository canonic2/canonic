//! Native caller execution must not queue behind the input it can unblock.
use super::*;

pub(super) fn client(
    worker: &Worker,
) -> (
    Session,
    mpsc::Receiver<Command>,
    mpsc::Receiver<Command>,
    mpsc::Receiver<Command>,
    mpsc::Receiver<control::ControlCommand>,
) {
    let (commands, ordinary) = mpsc::channel(32);
    let (executions, execution_queue) = mpsc::channel(32);
    let (cleanup, _cleanup) = mpsc::channel(8);
    let (responses, response_queue) = mpsc::channel(8);
    let (controls, control_queue) = mpsc::channel(4);
    let session = Session(Arc::new(Client {
        shared: worker.shared.clone(),
        commands,
        cleanup,
        responses,
        controls,
        executions,
        capacity: Arc::new(tokio::sync::Semaphore::new(32)),
        journal: worker.journal.clone(),
        startup: Uuid::new_v4(),
    }));
    (
        session,
        ordinary,
        execution_queue,
        response_queue,
        control_queue,
    )
}

fn evaluate(scope: Reference) -> Request {
    serde_json::from_value(json!({"operation":"evaluate","scope":scope,"expression":"42"})).unwrap()
}

async fn pending_condition_helper(
    peer: &mut Peer,
    numeric: i64,
    unique: &str,
    fresh: bool,
) -> Value {
    let world = command(peer, "Page.createIsolatedWorld").await;
    if fresh {
        send(peer,json!({"method":"Runtime.executionContextCreated","sessionId":"session-acme","params":{"context":{"id":numeric,"uniqueId":unique,"origin":"https://example.com","name":page::WORLD,"auxData":{"frameId":"frame-acme","isDefault":false}}}})).await;
    }
    reply(peer, &world, json!({"executionContextId":numeric})).await;
    let initialize_world = command(peer, "Runtime.evaluate").await;
    assert_eq!(initialize_world["params"]["uniqueContextId"], unique);
    reply(
        peer,
        &initialize_world,
        json!({"result":{"type":"boolean","value":true}}),
    )
    .await;
    let install_helper = command(peer, "Runtime.evaluate").await;
    assert_eq!(install_helper["params"]["uniqueContextId"], unique);
    install_helper
}

async fn condition_world(peer: &mut Peer, numeric: i64, unique: &str, fresh: bool) {
    let install_helper = pending_condition_helper(peer, numeric, unique, fresh).await;
    reply(
        peer,
        &install_helper,
        json!({"result":{"type":"boolean","value":true}}),
    )
    .await;
}

async fn registered_large_fixture(worker: &mut Worker) -> (String, Value, Reference) {
    use crate::browser::transfer::FIXTURE_BYTES;
    use base64::{Engine as _, engine::general_purpose::STANDARD};

    let data = STANDARD.encode(vec![42; FIXTURE_BYTES]);
    let registered = worker
        .fixture_register(
            serde_json::from_value(json!({
                "name":"acme.bin","mimeType":"application/octet-stream","dataBase64":data,
                "provenance":"synthetic_generated","cleared":{"by":"Acme","method":"Generated"}
            }))
            .unwrap(),
        )
        .await
        .unwrap();
    let summary = registered["fixture"].clone();
    let file = serde_json::from_value(summary["reference"].clone()).unwrap();
    (data, summary, file)
}

#[tokio::test]
async fn large_fixture_native_upload_dispatches_verified_bytes() {
    use crate::browser::page::Locator;

    let (mut worker, mut peer, _root, _context, page) = investigation::owned_page().await;
    let (data, summary, file) = registered_large_fixture(&mut worker).await;
    let upload = tokio::spawn(async move {
        worker
            .page_upload(
                Uuid::new_v4(),
                page,
                None,
                Locator::Css {
                    selector: "input".into(),
                },
                vec![file],
                deadline(),
            )
            .await
    });
    condition_world(&mut peer, 1, "world-acme", false).await;
    let attach = command(&mut peer, "Runtime.evaluate").await;
    assert!(
        attach["params"]["expression"]
            .as_str()
            .unwrap()
            .contains(&data)
    );
    reply(
        &mut peer,
        &attach,
        json!({"result":{"type":"object","value":{"attached":true}}}),
    )
    .await;
    let receipt = upload.await.unwrap().unwrap();
    assert_eq!(receipt["fixtures"][0], summary);
    assert_eq!(receipt["attached"], true);
    assert_eq!(receipt["serverUploadVerified"], false);
}

#[tokio::test]
async fn large_fixture_native_chooser_dispatches_verified_bytes_and_releases_the_group() {
    let (mut worker, mut peer, _root, _context, _page) = investigation::owned_page().await;
    let (data, summary, file) = registered_large_fixture(&mut worker).await;
    worker
        .chooser_event(&Event {
            method: "Page.fileChooserOpened".into(),
            session_id: Some("session-acme".into()),
            params: json!({"frameId":"frame-acme","backendNodeId":42,"mode":"selectSingle"}),
        })
        .unwrap();
    let chooser = *worker.transfers.choosers.keys().next().unwrap();
    let upload = tokio::spawn(async move {
        let receipt = worker
            .resolve_chooser(Uuid::new_v4(), chooser, vec![file], deadline())
            .await
            .unwrap();
        assert_eq!(
            worker
                .resolve_chooser(Uuid::new_v4(), chooser, vec![file], deadline())
                .await
                .unwrap_err()
                .code,
            "STALE_REFERENCE",
        );
        receipt
    });
    let world = command(&mut peer, "Page.createIsolatedWorld").await;
    reply(&mut peer, &world, json!({"executionContextId":1})).await;
    let initialize = command(&mut peer, "Runtime.evaluate").await;
    reply(
        &mut peer,
        &initialize,
        json!({"result":{"type":"boolean","value":true}}),
    )
    .await;
    let node = command(&mut peer, "DOM.resolveNode").await;
    assert_eq!(node["params"]["backendNodeId"], 42);
    reply(
        &mut peer,
        &node,
        json!({"object":{"type":"object","objectId":"acme-input"}}),
    )
    .await;
    let attach = command(&mut peer, "Runtime.callFunctionOn").await;
    assert_eq!(attach["params"]["objectId"], "acme-input");
    let payload = &attach["params"]["arguments"][0]["value"][0];
    assert_eq!(payload["name"], summary["name"]);
    assert_eq!(payload["mimeType"], summary["mimeType"]);
    assert!(payload["buffer"].as_str() == Some(data.as_str()));
    reply(
        &mut peer,
        &attach,
        json!({"result":{"type":"object","value":{"attached":true}}}),
    )
    .await;
    let release = command(&mut peer, "Runtime.releaseObjectGroup").await;
    assert_eq!(
        release["params"]["objectGroup"],
        node["params"]["objectGroup"]
    );
    reply(&mut peer, &release, json!({})).await;
    let receipt = upload.await.unwrap();
    assert_eq!(receipt["chooser"], json!(chooser));
    assert_eq!(receipt["fixtures"], json!([summary]));
    assert_eq!(receipt["attached"], true);
    assert_eq!(receipt["serverUploadVerified"], false);
}

#[tokio::test]
async fn native_condition_polling_retains_one_dispatch_witness_and_other_call_witnesses() {
    let (mut worker, mut peer, _root, _context, page) = investigation::owned_page().await;
    let (session, mut ordinary, _executions, _responses, _controls) = client(&worker);
    let condition = Uuid::new_v4();
    let other = Uuid::new_v4();
    session
        .submit(
            condition,
            Request::PageWait {
                page,
                frame: None,
                locator: None,
                condition: crate::browser::page::Condition::Visible {},
            },
            5000,
        )
        .unwrap();
    let queued = ordinary.try_recv().unwrap();
    let polling = tokio::spawn(async move {
        worker.perform(queued).await;
        worker
            .call_read(other, None, "Runtime.getHeapUsage", json!({}), deadline())
            .await
            .unwrap();
        for _ in 0..32 {
            worker.poll_waits().await;
        }
        worker
    });
    for index in 0..33 {
        if index == 0 {
            condition_world(&mut peer, 1, "world-acme", false).await;
        }
        if index == 1 {
            let usage = command(&mut peer, "Runtime.getHeapUsage").await;
            reply(
                &mut peer,
                &usage,
                json!({"usedSize":1,"totalSize":2,"embedderHeapUsedSize":0,"backingStorageSize":0}),
            )
            .await;
        }
        let query = command(&mut peer, "Runtime.evaluate").await;
        assert_eq!(query["params"]["uniqueContextId"], "world-acme");
        assert!(query["params"]["expression"].as_str().unwrap().len() < 4096);
        reply(
            &mut peer,
            &query,
            json!({"result":{"type":"object","value":{"satisfied":index==32}}}),
        )
        .await;
    }
    let worker = polling.await.unwrap();
    assert_eq!(
        session.operation(condition).unwrap().outcome,
        Outcome::Succeeded
    );
    assert!(worker.waits.is_empty());
    assert!(worker.condition_probes.is_empty());
    let mut dispatches = BTreeMap::new();
    for entry in std::fs::read_dir(worker.journal.directory()).unwrap() {
        let entry = entry.unwrap();
        if entry
            .path()
            .extension()
            .is_some_and(|extension| extension == "json")
        {
            let value: Value =
                serde_json::from_slice(&std::fs::read(entry.path()).unwrap()).unwrap();
            if value["stage"] == "dispatch_admitted" {
                *dispatches
                    .entry(value["operationId"].as_str().unwrap().to_owned())
                    .or_insert(0) += 1;
            }
        }
    }
    assert_eq!(dispatches.get(&condition.to_string()), Some(&1));
    assert_eq!(dispatches.get(&other.to_string()), Some(&1));
}

async fn assert_aria_read_progresses_during_wait(
    session: &Session,
    peer: &mut Peer,
    page: Reference,
    id: Uuid,
) {
    let pending = session.wait(id, Instant::now()).await.unwrap();
    assert_eq!(pending.outcome, Outcome::Running);
    assert_eq!(pending.code.as_deref(), Some("WAIT_EXPIRED"));
    assert!(!pending.settled);
    let read = serde_json::from_value(json!({"operation":"batch","requests":[{"operation":"page_read","page":page,"read":"aria"}]})).unwrap();
    let read = session.submit(Uuid::new_v4(), read, 5000).unwrap();
    let world = command(peer, "Page.createIsolatedWorld").await;
    reply(peer, &world, json!({"executionContextId":1})).await;
    let initialize = command(peer, "Runtime.evaluate").await;
    reply(
        peer,
        &initialize,
        json!({"result":{"type":"boolean","value":true}}),
    )
    .await;
    let query = command(peer, "Runtime.evaluate").await;
    reply(peer, &query, json!({"result":{"type":"object","value":{"truncated":true,"narrowingRequired":true,"value":null}}})).await;
    let read = settled(&session.0.shared, read).await;
    assert_eq!(read.outcome, Outcome::Succeeded);
    let read = read.result.unwrap();
    assert!(
        read["results"][0]["result"]["hint"]
            .as_str()
            .unwrap()
            .contains("locator")
    );
    assert!(!session.operation(id).unwrap().settled);
}

#[tokio::test]
async fn native_batch_text_waits_bound_dispatch_journal_per_step() {
    let (mut worker, mut peer, _root, _context, page) = investigation::owned_page().await;
    let (session, mut ordinary, executions, _responses, _controls) = client(&worker);
    worker.executions = Some(executions);
    let id = session
        .submit(
            Uuid::new_v4(),
            Request::Batch {
                requests: ["Add form", "Form saved"]
                    .into_iter()
                    .map(|value| Request::PageWait {
                        page,
                        frame: None,
                        locator: None,
                        condition: crate::browser::page::Condition::Text {
                            value: value.into(),
                            contains: true,
                        },
                    })
                    .collect(),
            },
            5000,
        )
        .unwrap();
    let queued = ordinary.try_recv().unwrap();
    let polling = tokio::spawn(async move {
        worker.perform(queued).await;
        worker
    });
    for value in ["Add form", "Form saved"] {
        for sample in 0..4 {
            loop {
                let message = tokio::time::timeout_at(deadline(), peer.next())
                    .await
                    .unwrap()
                    .unwrap()
                    .unwrap();
                let Message::Text(message) = message else {
                    panic!("Expected CDP command");
                };
                let request: Value = serde_json::from_str(&message).unwrap();
                let result = match request["method"].as_str().unwrap() {
                    "Page.createIsolatedWorld" => json!({"executionContextId":1}),
                    "Runtime.evaluate"
                        if request["params"]["expression"]
                            .as_str()
                            .unwrap()
                            .contains(value) =>
                    {
                        if value == "Add form" && sample == 0 {
                            assert_aria_read_progresses_during_wait(&session, &mut peer, page, id)
                                .await;
                        }
                        reply(&mut peer, &request, json!({"result":{"type":"object","value":{"satisfied":sample==3,"text":if sample==3 { value } else { "" }}}})).await;
                        break;
                    }
                    "Runtime.evaluate" => json!({"result":{"type":"boolean","value":true}}),
                    method => panic!("Unexpected condition command {method}"),
                };
                reply(&mut peer, &request, result).await;
            }
        }
    }
    let worker = polling.await.unwrap();
    let outcome = session.operation(id).unwrap();
    assert_eq!(outcome.outcome, Outcome::Succeeded);
    assert_eq!(outcome.result.unwrap()["completedActions"], 2);
    assert!(worker.condition_probes.is_empty());
    assert_eq!(session.status()["phase"], "ready");
    let dispatches = std::fs::read_dir(worker.journal.directory())
        .unwrap()
        .map(|entry| {
            serde_json::from_slice::<Value>(&std::fs::read(entry.unwrap().path()).unwrap()).unwrap()
        })
        .filter(|entry| entry["stage"] == "dispatch_admitted" && entry["operationId"] == json!(id))
        .count();
    assert_eq!(dispatches, 2, "One dispatch witness per batch wait");
}

#[tokio::test]
async fn native_role_wait_reports_an_unmatched_locator_on_timeout() {
    let (mut worker, mut peer, _root, _context, page) = investigation::owned_page().await;
    let (session, mut ordinary, _executions, _responses, _controls) = client(&worker);
    let request = serde_json::from_value(json!({"operation":"page_wait","page":page,
        "locator":{"kind":"role","role":"button","name":"Save"},"condition":{"kind":"attached"}}))
    .unwrap();
    let id = session.submit(Uuid::new_v4(), request, 5000).unwrap();
    let queued = ordinary.try_recv().unwrap();
    let polling = tokio::spawn(async move {
        worker.perform(queued).await;
        worker
    });
    let world = command(&mut peer, "Page.createIsolatedWorld").await;
    reply(&mut peer, &world, json!({"executionContextId":1})).await;
    for _ in 0..2 {
        let install = command(&mut peer, "Runtime.evaluate").await;
        reply(
            &mut peer,
            &install,
            json!({"result":{"type":"boolean","value":true}}),
        )
        .await;
    }
    let probe = command(&mut peer, "Runtime.evaluate").await;
    reply(
        &mut peer,
        &probe,
        json!({"result":{"type":"object","value":{"satisfied":false,"matchCount":0}}}),
    )
    .await;
    let mut worker = polling.await.unwrap();
    worker.waits.get_mut(&id).unwrap().deadline = Instant::now();
    worker.poll_waits().await;
    let operation = session.operation(id).unwrap();
    assert_eq!(operation.outcome, Outcome::Failed);
    assert_eq!(operation.code.as_deref(), Some("CONDITION_TIMEOUT"));
    let result = operation.result.unwrap();
    assert_eq!(result["locatorMatched"], false);
    assert!(
        result["hint"]
            .as_str()
            .unwrap()
            .contains("No element matched")
    );
    assert_eq!(session.status()["phase"], "ready");
}

#[tokio::test]
async fn native_disconnect_releases_lost_session_when_journal_writes_fail() {
    let (mut worker, _peer, root, context, page) = investigation::owned_page().await;
    worker.diagnostics.history.enable(context, true);
    let (mut session, mut ordinary, _executions, _responses, _controls) = client(&worker);
    let (sender, mut cleanup) = mpsc::channel(1);
    Arc::get_mut(&mut session.0).unwrap().cleanup = sender;
    lose_to_journal_failure(&mut worker, &session, &mut ordinary, root.path()).await;
    assert_eq!(session.status()["diagnostics"]["activeContexts"], json!([]));
    assert!(worker.index.route(page, Some(Kind::Page)).is_ok());

    let disconnect = session.disconnect(5000).unwrap();
    worker.perform(cleanup.try_recv().unwrap()).await;
    let operation = session.wait(disconnect, deadline()).await.unwrap();
    assert_eq!(operation.outcome, Outcome::Succeeded);
    let receipt = operation.result.unwrap();
    assert_eq!(receipt["closed"], true);
    assert_eq!(receipt["cleanup"]["journalFailure"], "STATE_IO");
    assert_eq!(session.status()["phase"], "closed");
    assert!(worker.connection.is_none());
}

#[tokio::test]
async fn unfinished_native_disconnect_preserves_journal_failure() {
    let (mut worker, _peer, root, context, _page) = investigation::owned_page().await;
    let (mut session, _ordinary, _executions, _responses, _controls) = client(&worker);
    let (sender, mut cleanup) = mpsc::channel(1);
    Arc::get_mut(&mut session.0).unwrap().cleanup = sender;
    std::fs::rename(
        worker.journal.directory(),
        root.path().join("retained-journal"),
    )
    .unwrap();
    let blocked = root.path().join("download-path-is-a-file");
    std::fs::write(&blocked, []).unwrap();
    worker
        .transfers
        .download_contexts
        .insert(context, blocked.clone());

    let id = session.disconnect(5000).unwrap();
    worker.perform(cleanup.try_recv().unwrap()).await;
    let outcome = session.wait(id, deadline()).await.unwrap();
    assert!(outcome.settled);
    assert_eq!(outcome.outcome, Outcome::Unknown);
    assert_eq!(outcome.code.as_deref(), Some("STATE_IO"));
    assert_eq!(session.status()["phase"], "cleanup_pending");
    assert_eq!(session.status()["failure"], "STATE_IO");

    std::fs::remove_file(blocked).unwrap();
    let retry = session.disconnect(5000).unwrap();
    worker.perform(cleanup.try_recv().unwrap()).await;
    assert_eq!(
        session.wait(retry, deadline()).await.unwrap().outcome,
        Outcome::Succeeded
    );
    assert_eq!(session.status()["phase"], "closed");
}

#[tokio::test]
async fn native_condition_probes_rebuild_worlds_and_fence_explicit_documents() {
    for (explicit, during_install) in [(false, false), (true, false), (false, true), (true, true)] {
        let (mut worker, mut peer, _root, _context, page) = investigation::owned_page().await;
        let original = worker.index.page_frame(page, None).unwrap();
        let (session, mut ordinary, _executions, _responses, _controls) = client(&worker);
        let id = session
            .submit(
                Uuid::new_v4(),
                Request::PageWait {
                    page,
                    frame: explicit.then_some(original),
                    locator: None,
                    condition: crate::browser::page::Condition::Visible {},
                },
                5000,
            )
            .unwrap();
        let queued = ordinary.try_recv().unwrap();
        let navigation = json!({"type":"Navigation","frame":{"id":"frame-acme","loaderId":"loader-next","url":"https://example.com/next","domainAndRegistry":"example.com","securityOrigin":"https://example.com","mimeType":"text/html","secureContextType":"Secure","crossOriginIsolatedContextType":"NotIsolated","gatedAPIFeatures":[]}});
        let event = navigation.clone();
        let polling = tokio::spawn(async move {
            worker.perform(queued).await;
            if !during_install {
                worker
                    .apply(EventItem::Event(Event {
                        method: "Runtime.executionContextsCleared".into(),
                        session_id: Some("session-acme".into()),
                        params: json!({}),
                    }))
                    .unwrap();
                worker
                    .apply(EventItem::Event(Event {
                        method: "Page.frameNavigated".into(),
                        session_id: Some("session-acme".into()),
                        params: event,
                    }))
                    .unwrap();
            }
            worker.poll_waits().await;
            worker
        });
        if during_install {
            let install = pending_condition_helper(&mut peer, 1, "world-acme", false).await;
            send(&mut peer, json!({"method":"Page.frameNavigated","sessionId":"session-acme","params":navigation})).await;
            reply(
                &mut peer,
                &install,
                json!({"result":{"type":"boolean","value":true}}),
            )
            .await;
        } else {
            condition_world(&mut peer, 1, "world-acme", false).await;
            let query = command(&mut peer, "Runtime.evaluate").await;
            reply(
                &mut peer,
                &query,
                json!({"result":{"type":"object","value":{"satisfied":false}}}),
            )
            .await;
        }
        if !explicit {
            condition_world(&mut peer, 2, "world-next", true).await;
            let query = command(&mut peer, "Runtime.evaluate").await;
            assert_eq!(query["params"]["uniqueContextId"], "world-next");
            assert!(query["params"]["expression"].as_str().unwrap().len() < 4096);
            reply(
                &mut peer,
                &query,
                json!({"result":{"type":"object","value":{"satisfied":true}}}),
            )
            .await;
        }
        let worker = tokio::time::timeout_at(deadline(), polling)
            .await
            .unwrap()
            .unwrap();
        let outcome = session.operation(id).unwrap();
        if explicit {
            assert_eq!(outcome.code.as_deref(), Some("STALE_REFERENCE"));
            assert_eq!(outcome.outcome, Outcome::Failed);
        } else {
            assert_eq!(outcome.outcome, Outcome::Succeeded);
            assert_eq!(
                outcome.result.unwrap()["frame"],
                json!(worker.index.page_frame(page, None).unwrap())
            );
        }
        assert!(worker.index.route(original, Some(Kind::Frame)).is_err());
        assert!(worker.waits.is_empty());
        assert!(worker.condition_probes.is_empty());
        assert_eq!(session.0.capacity.available_permits(), 32);
    }
}

#[tokio::test]
async fn native_wait_snapshot_survives_nested_metadata_settlement() {
    use crate::browser::activity::{Condition, Event as PageEvent};
    let (mut worker, mut peer, _root, context, page) = investigation::owned_page().await;
    let (session, mut ordinary, mut executions, _responses, _controls) = client(&worker);
    let dom = Uuid::from_u128(1);
    let metadata = Uuid::from_u128(2);
    session
        .submit(
            dom,
            Request::PageWait {
                page,
                frame: None,
                locator: None,
                condition: crate::browser::page::Condition::Visible {},
            },
            5000,
        )
        .unwrap();
    session
        .submit(
            metadata,
            Request::EventsWait {
                context,
                after: 0,
                conditions: vec![Condition {
                    event: PageEvent::Dialog,
                    page: Some(page),
                }],
            },
            5000,
        )
        .unwrap();
    worker.waits.insert(dom, ordinary.try_recv().unwrap());
    let mut expiring = executions.try_recv().unwrap();
    expiring.deadline = Instant::now();
    worker.waits.insert(metadata, expiring);
    let polling = tokio::spawn(async move {
        worker.poll_waits().await;
        worker
    });
    let world = command(&mut peer, "Page.createIsolatedWorld").await;
    assert_eq!(
        session.operation(metadata).unwrap().code.as_deref(),
        Some("WAIT_EXPIRED")
    );
    reply(&mut peer, &world, json!({"executionContextId":1})).await;
    let initialize = command(&mut peer, "Runtime.evaluate").await;
    reply(
        &mut peer,
        &initialize,
        json!({"result":{"type":"boolean","value":true}}),
    )
    .await;
    let bootstrap = command(&mut peer, "Runtime.evaluate").await;
    reply(
        &mut peer,
        &bootstrap,
        json!({"result":{"type":"boolean","value":true}}),
    )
    .await;
    let query = command(&mut peer, "Runtime.evaluate").await;
    reply(
        &mut peer,
        &query,
        json!({"result":{"type":"object","value":{"satisfied":true}}}),
    )
    .await;
    let worker = polling.await.unwrap();
    assert_eq!(session.operation(dom).unwrap().outcome, Outcome::Succeeded);
    assert_eq!(
        session.operation(metadata).unwrap().outcome,
        Outcome::Failed
    );
    assert!(worker.waits.is_empty());
    assert_eq!(session.0.capacity.available_permits(), 32);
    assert_eq!(session.0.shared.state.lock().unwrap().phase, Phase::Ready);
}

#[tokio::test]
async fn native_event_observation_settles_through_pending_input_without_borrowing_control_capacity()
{
    assert_pending_input_metadata(Lane::Ordinary).await;
    assert_pending_input_metadata(Lane::Control).await;
}

#[expect(
    clippy::too_many_lines,
    reason = "One held-input fixture verifies event deadlines, read settlement and ordinary versus reserved admission for both dispatch lanes"
)]
async fn assert_pending_input_metadata(lane: Lane) {
    use crate::browser::activity::{Condition, Event as PageEvent};
    let (mut worker, mut peer, _root, context, page) = investigation::owned_page().await;
    let (session, mut ordinary, executions, responses, _controls) = client(&worker);
    worker.executions = Some(executions);
    worker.responses = Some(responses);
    let shared = worker.shared.clone();
    let running = tokio::spawn(async move {
        let params = json!({"type":"mouseReleased","x":10,"y":10,"button":"left"});
        let result = if matches!(lane, Lane::Control) {
            worker
                .cleanup_call(
                    Some("session-acme"),
                    "Input.dispatchMouseEvent",
                    params,
                    deadline(),
                )
                .await
        } else {
            worker
                .call(
                    Uuid::new_v4(),
                    Some("session-acme"),
                    "Input.dispatchMouseEvent",
                    params,
                    deadline(),
                )
                .await
        };
        (worker, result)
    });
    let held = command(&mut peer, "Input.dispatchMouseEvent").await;
    let waiting = session
        .submit(
            Uuid::new_v4(),
            Request::EventsWait {
                context,
                after: 0,
                conditions: vec![Condition {
                    event: PageEvent::Dialog,
                    page: Some(page),
                }],
            },
            5000,
        )
        .unwrap();
    tokio::time::timeout_at(deadline(), async {
        while shared.state.lock().unwrap().operations[&waiting].outcome != Outcome::Running {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let expiring = session
        .submit(
            Uuid::new_v4(),
            Request::EventsWait {
                context,
                after: 0,
                conditions: vec![Condition {
                    event: PageEvent::Dialog,
                    page: Some(page),
                }],
            },
            50,
        )
        .unwrap();
    let expired = settled(&shared, expiring).await;
    assert_eq!(expired.outcome, Outcome::Failed, "{expired:?}");
    assert_eq!(expired.code.as_deref(), Some("WAIT_EXPIRED"));
    assert!(!running.is_finished());
    assert_eq!(session.0.responses.capacity(), 8);
    let params = json!({"url":"https://example.com/acme","frameId":"frame-acme","type":"prompt","message":"Acme prompt","hasBrowserHandler":true});
    crate::browser::cdp::schema::event("Page.javascriptDialogOpening", &params).unwrap();
    send(
        &mut peer,
        json!({"method":"Page.javascriptDialogOpening","sessionId":"session-acme","params":params}),
    )
    .await;
    let observed = settled(&shared, waiting).await;
    assert_eq!(observed.outcome, Outcome::Succeeded, "{observed:?}");
    assert_eq!(
        observed.result.as_ref().unwrap()["event"]["event"],
        "dialog"
    );
    assert!(!running.is_finished());
    for request in [
        Request::EventsRead {
            context,
            cursor: None,
            limit: 50,
        },
        Request::DiagnosticsRead {
            context,
            query: Default::default(),
        },
    ] {
        let id = session.submit(Uuid::new_v4(), request, 5000).unwrap();
        assert_eq!(settled(&shared, id).await.outcome, Outcome::Succeeded);
        assert!(!running.is_finished());
    }
    assert!(ordinary.try_recv().is_err());
    assert_eq!(session.0.responses.capacity(), 8);
    let capacity = tokio::time::timeout_at(
        deadline(),
        session.0.capacity.clone().acquire_many_owned(32),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(
        session
            .submit(
                Uuid::new_v4(),
                Request::EventsRead {
                    context,
                    cursor: None,
                    limit: 50
                },
                5000
            )
            .unwrap_err()
            .code,
        "OPERATION_CAPACITY"
    );
    assert_eq!(session.0.responses.capacity(), 8);
    drop(capacity);
    let deferred = if matches!(lane, Lane::Control) {
        let id = session
            .submit(Uuid::new_v4(), evaluate(page), 5000)
            .unwrap();
        let observed = session
            .submit(
                Uuid::new_v4(),
                Request::EventsRead {
                    context,
                    cursor: None,
                    limit: 50,
                },
                5000,
            )
            .unwrap();
        assert_eq!(settled(&shared, observed).await.outcome, Outcome::Succeeded);
        assert_eq!(session.operation(id).unwrap().outcome, Outcome::Queued);
        Some(id)
    } else {
        None
    };
    reply(&mut peer, &held, json!({})).await;
    let (mut worker, result) = running.await.unwrap();
    assert!(result.is_ok(), "{result:?}");
    if let Some(id) = deferred {
        let command = tokio::time::timeout_at(
            deadline(),
            next_execution(&mut worker.deferred_executions, &mut worker.executions),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(command.id, id);
    }
}

#[tokio::test]
async fn native_callback_and_context_close_batches_dispatch_through_held_calls() {
    let (mut worker, mut peer, _root, context, _page) = investigation::owned_page().await;
    let (session, mut ordinary, executions, responses, _control) = client(&worker);
    worker.executions = Some(executions);
    worker.responses = Some(responses);
    let shared = worker.shared.clone();
    let running = tokio::spawn(async move {
        let result = worker
            .call(
                Uuid::new_v4(),
                None,
                "Runtime.getHeapUsage",
                json!({}),
                deadline(),
            )
            .await;
        (worker, result)
    });
    let held = command(&mut peer, "Runtime.getHeapUsage").await;
    let stale = Reference {
        generation: Uuid::new_v4(),
        ..context
    };
    for (request, batch) in [
        (Request::CallbackResolve {
            callback: stale,
            response: contract::CallbackResponse::RouteContinue {},
        }, true),
        (Request::ContextClose { context: stale }, true),
        (Request::HandleProperty { handle: stale, name: "Acme".into() }, false),
        (Request::HandleProperties { handle: stale, query: Default::default() }, true),
        (serde_json::from_value(json!({"operation":"network_route","scope":stale,"action":{"kind":"clear","behavior":"wait"}})).unwrap(), false),
        (serde_json::from_value(json!({"operation":"network_route","scope":stale,"action":{"kind":"clear","behavior":"wait"}})).unwrap(), true),
    ] {
        let id = session
            .submit(
                Uuid::new_v4(),
                if batch { Request::Batch { requests: vec![request.clone()] } } else { request.clone() },
                5000,
            )
            .unwrap();
        let result = tokio::time::timeout(Duration::from_millis(250), settled(&shared, id)).await.expect("Callback/close-only shared actions must dispatch through the call they can release");
        assert_eq!(result.code.as_deref(), Some("STALE_REFERENCE"));
        assert!(!running.is_finished());
        let mixed = session
            .submit(
                Uuid::new_v4(),
                Request::Batch {
                    requests: vec![request, Request::PageList {}],
                },
                5000,
            )
            .unwrap();
        assert_eq!(ordinary.try_recv().unwrap().id, mixed);
    }
    assert_eq!(session.0.responses.capacity(), 8);
    let capacity = session
        .0
        .capacity
        .clone()
        .try_acquire_many_owned(32)
        .unwrap();
    assert_eq!(
        session
            .submit(
                Uuid::new_v4(),
                Request::Batch {
                    requests: vec![Request::CallbackResolve {
                        callback: stale,
                        response: contract::CallbackResponse::RouteContinue {}
                    }]
                },
                5000
            )
            .unwrap_err()
            .code,
        "OPERATION_CAPACITY"
    );
    assert_eq!(session.0.responses.capacity(), 8);
    drop(capacity);
    reply(
        &mut peer,
        &held,
        json!({"usedSize":1,"totalSize":2,"embedderHeapUsedSize":0,"backingStorageSize":0}),
    )
    .await;
    let (_worker, result) = running.await.unwrap();
    assert!(result.is_ok(), "{result:?}");
}

#[tokio::test]
async fn native_socket_control_batch_dispatches_through_held_calls_with_normal_capacity() {
    let (mut worker, mut peer, _root, _context, page) = investigation::owned_page().await;
    let (session, mut ordinary, executions, responses, _control) = client(&worker);
    worker.executions = Some(executions);
    worker.responses = Some(responses);
    let socket: Request = serde_json::from_value(json!({"operation":"web_socket_route_handlers","scope":page,"frame":page,"routeId":Uuid::new_v4(),"socketId":"a".repeat(64),"handlers":{"pageMessage":null}})).unwrap();
    let shared = worker.shared.clone();
    let running = tokio::spawn(async move {
        let result = worker
            .call(
                Uuid::new_v4(),
                None,
                "Runtime.getHeapUsage",
                json!({}),
                deadline(),
            )
            .await;
        (worker, result)
    });
    let held = command(&mut peer, "Runtime.getHeapUsage").await;
    let id = session
        .submit(
            Uuid::new_v4(),
            Request::Batch {
                requests: vec![socket.clone(), socket.clone()],
            },
            5000,
        )
        .unwrap();
    let result = tokio::time::timeout(Duration::from_millis(250), settled(&shared, id))
        .await
        .expect("Socket-only batch must dispatch through held native calls");
    assert_eq!(result.code.as_deref(), Some("UNKNOWN_WEBSOCKET_ROUTE"));
    assert!(!running.is_finished());
    let mixed = session
        .submit(
            Uuid::new_v4(),
            Request::Batch {
                requests: vec![socket.clone(), Request::PageList {}],
            },
            5000,
        )
        .unwrap();
    assert_eq!(ordinary.try_recv().unwrap().id, mixed);
    let capacity = session
        .0
        .capacity
        .clone()
        .try_acquire_many_owned(32)
        .unwrap();
    assert_eq!(
        session
            .submit(
                Uuid::new_v4(),
                Request::Batch {
                    requests: vec![socket.clone()]
                },
                5000
            )
            .unwrap_err()
            .code,
        "OPERATION_CAPACITY"
    );
    assert_eq!(session.0.responses.capacity(), 8);
    drop(capacity);
    reply(
        &mut peer,
        &held,
        json!({"usedSize":1,"totalSize":2,"embedderHeapUsedSize":0,"backingStorageSize":0}),
    )
    .await;
    let (_worker, result) = running.await.unwrap();
    assert!(result.is_ok(), "{result:?}");
    assert!(!execution::independent(&Request::Batch {
        requests: vec![]
    }));
}

#[tokio::test]
async fn native_acknowledged_context_close_retires_held_execution_without_losing_siblings() {
    acknowledged_scope_close_releases_held_calls(false).await;
}

#[tokio::test]
async fn native_acknowledged_page_close_retires_held_execution_without_losing_siblings() {
    acknowledged_scope_close_releases_held_calls(true).await;
}

async fn acknowledged_scope_close_releases_held_calls(page_only: bool) {
    for grouped in [false, true] {
        let (mut worker, mut peer, _root, context, page) = investigation::owned_page().await;
        let (session, _ordinary, mut executions, responses, _control) = client(&worker);
        let id = session
            .submit(Uuid::new_v4(), evaluate(page), 5000)
            .unwrap();
        let original = executions.try_recv().unwrap();
        worker.executions = Some(executions);
        worker.responses = Some(responses);
        let key = grouped.then(|| {
            worker
                .evaluation
                .groups
                .begin(
                    context,
                    "session-acme".into(),
                    super::super::execution_groups::Purpose::ArgumentAdoption,
                )
                .unwrap()
        });
        let running = tokio::spawn(async move {
            let params = json!({"functionDeclaration":"function(){return new Promise(()=>{})}","uniqueContextId":"world-acme","awaitPromise":true,"returnByValue":true});
            let result = if let Some(key) = key {
                worker
                    .execution_group_call(id, &key, "Runtime.callFunctionOn", params, deadline())
                    .await
            } else {
                worker
                    .call(
                        id,
                        Some("session-acme"),
                        "Runtime.callFunctionOn",
                        params,
                        deadline(),
                    )
                    .await
            };
            worker.finish(id, result).await;
            drop(original);
            worker
        });
        let held = command(&mut peer, "Runtime.callFunctionOn").await;
        let close = session
            .submit(
                Uuid::new_v4(),
                Request::Batch {
                    requests: vec![if page_only {
                        Request::PageClose { page }
                    } else {
                        Request::ContextClose { context }
                    }],
                },
                5000,
            )
            .unwrap();
        let dispose = command(
            &mut peer,
            if page_only {
                "Target.closeTarget"
            } else {
                "Target.disposeBrowserContext"
            },
        )
        .await;
        assert!(
            !session.operation(id).unwrap().settled,
            "Unacknowledged close cannot settle the caller"
        );
        reply(
            &mut peer,
            &dispose,
            if page_only {
                json!({"success":true})
            } else {
                json!({})
            },
        )
        .await;
        let worker = tokio::time::timeout(Duration::from_millis(250), running).await.expect("Acknowledged context closure must settle its held caller without waiting for the original deadline").unwrap();
        assert_eq!(
            session.operation(close).unwrap().outcome,
            Outcome::Succeeded
        );
        let partial = assert_closed_scope_unknown(session.operation(id).unwrap());
        if page_only {
            assert_eq!(partial["page"], json!(page));
            assert!(worker.index.native(context, Kind::Context).is_ok());
        }
        assert_eq!(session.0.shared.state.lock().unwrap().phase, Phase::Ready);
        verify_closed_call_cleanup(worker, peer, held).await;
    }
}

async fn settled(shared: &Arc<Shared>, id: Uuid) -> Operation {
    tokio::time::timeout_at(deadline(), async {
        loop {
            let changed = shared.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            let operation = shared.state.lock().unwrap().operations[&id].clone();
            if operation.settled {
                return operation;
            }
            changed.await;
        }
    })
    .await
    .unwrap_or_else(|error| {
        panic!(
            "Operation did not settle: {error}; operation={:?}",
            shared.state.lock().unwrap().operations[&id]
        )
    })
}

#[tokio::test]
async fn native_caller_execution_does_not_enter_the_ordinary_command_lane() {
    let (worker, _peer, _root) = fixture().await;
    let (session, mut ordinary, mut execution_queue, mut responses, _control) = client(&worker);
    let scope = Reference {
        connection_id: Uuid::new_v4(),
        id: Uuid::new_v4(),
        generation: Uuid::new_v4(),
    };
    let selector=serde_json::from_value(json!({"operation":"selector_evaluate","scope":scope,"options":{"locator":{"kind":"css","selector":"button"},"expression":"element=>element.id"}})).unwrap();
    for request in [
        evaluate(scope),
        Request::HandleRead { handle: scope },
        Request::FrameRetain { frame: scope },
        Request::FrameOwnerRelease { frame: scope },
        Request::HandleProperty {
            handle: scope,
            name: "Acme".into(),
        },
        Request::HandleProperties {
            handle: scope,
            query: Default::default(),
        },
        selector,
        Request::Batch {
            requests: vec![evaluate(scope), evaluate(scope)],
        },
        Request::Batch {
            requests: vec![
                Request::HandleProperty {
                    handle: scope,
                    name: "Acme".into(),
                },
                Request::HandleProperties {
                    handle: scope,
                    query: Default::default(),
                },
            ],
        },
    ] {
        session.submit(Uuid::new_v4(), request, 5000).unwrap();
        assert!(execution_queue.try_recv().is_ok());
    }
    assert!(
        ordinary.try_recv().is_err(),
        "Caller evaluation must remain available while ordinary input owns the lane"
    );
    session
        .submit(
            Uuid::new_v4(),
            Request::HandleDispose { handle: scope },
            5000,
        )
        .unwrap();
    assert!(matches!(
        responses.try_recv().unwrap().request,
        Some(Request::HandleDispose { .. })
    ));
    assert_eq!(session.0.capacity.available_permits(), 32);
}

#[tokio::test]
async fn native_caller_execution_capacity_is_shared_and_preserves_all_response_slots() {
    let (worker, _peer, _root, _context, page) = investigation::owned_page().await;
    let (session, mut ordinary, mut executions, mut responses, _control) = client(&worker);
    let mut ids = vec![];
    for index in 0..32 {
        let id = Uuid::new_v4();
        let request = match index % 4 {
            0 => match (index / 4) % 3 {
                0 => evaluate(page),
                1 => Request::FrameRetain { frame: page },
                _ => Request::FrameOwnerRelease { frame: page },
            },
            1 => Request::HandleProperty {
                handle: page,
                name: "Acme".into(),
            },
            2 => Request::HandleProperties {
                handle: page,
                query: Default::default(),
            },
            _ => Request::Batch {
                requests: vec![evaluate(page), Request::PageList {}],
            },
        };
        session.submit(id, request, 5000).unwrap();
        ids.push(id);
    }
    assert_eq!(session.0.capacity.available_permits(), 0);
    assert_eq!(
        session
            .submit(Uuid::new_v4(), evaluate(page), 5000)
            .unwrap_err()
            .code,
        "OPERATION_CAPACITY"
    );
    assert_eq!(
        session
            .submit(Uuid::new_v4(), Request::PageList {}, 5000)
            .unwrap_err()
            .code,
        "OPERATION_CAPACITY"
    );
    assert_eq!(
        session.submit(ids[0], evaluate(page), 5000).unwrap(),
        ids[0],
        "Idempotent receipts need no new capacity"
    );
    for _ in 0..8 {
        session
            .submit(Uuid::new_v4(), Request::HttpContextList {}, 5000)
            .unwrap();
    }
    assert_eq!(
        session
            .submit(Uuid::new_v4(), Request::HttpContextList {}, 5000)
            .unwrap_err()
            .code,
        "OPERATION_CAPACITY"
    );
    assert!(matches!(
        ordinary.try_recv().unwrap().request,
        Some(Request::Batch { .. })
    ));
    drop(executions.try_recv().unwrap());
    assert_eq!(session.0.capacity.available_permits(), 2);
    assert!(matches!(
        responses.try_recv().unwrap().request,
        Some(Request::HttpContextList {})
    ));
}

#[tokio::test]
async fn native_caller_execution_runs_through_a_held_call_and_keeps_control_responsive() {
    let (mut worker, mut peer, _root, _context, page) = investigation::owned_page().await;
    let (session, _ordinary, executions, responses, _control) = client(&worker);
    worker.executions = Some(executions);
    worker.responses = Some(responses);
    let shared = worker.shared.clone();
    let running = tokio::spawn(async move {
        let result = worker
            .call(
                Uuid::new_v4(),
                None,
                "Runtime.getHeapUsage",
                json!({}),
                deadline(),
            )
            .await;
        (worker, result)
    });
    let held = command(&mut peer, "Runtime.getHeapUsage").await;
    let first = session
        .submit(Uuid::new_v4(), evaluate(page), 5000)
        .unwrap();
    let first_call = command(&mut peer, "Runtime.callFunctionOn").await;
    assert_eq!(
        session.0.capacity.available_permits(),
        31,
        "Active independent execution retains admission"
    );
    let read = session
        .submit(Uuid::new_v4(), Request::HttpContextList {}, 5000)
        .unwrap();
    assert_eq!(settled(&shared, read).await.outcome, Outcome::Succeeded);
    let second = session
        .submit(Uuid::new_v4(), evaluate(page), 5000)
        .unwrap();
    let second_call = command(&mut peer, "Runtime.callFunctionOn").await;
    reply(
        &mut peer,
        &second_call,
        json!({"result":{"type":"object","value":{"n":42}}}),
    )
    .await;
    assert_eq!(settled(&shared, second).await.outcome, Outcome::Succeeded);
    assert!(!shared.state.lock().unwrap().operations[&first].settled);
    reply(
        &mut peer,
        &first_call,
        json!({"result":{"type":"object","value":{"n":42}}}),
    )
    .await;
    assert_eq!(settled(&shared, first).await.outcome, Outcome::Succeeded);
    assert!(!running.is_finished());
    assert_eq!(session.0.capacity.available_permits(), 32);
    reply(
        &mut peer,
        &held,
        json!({"usedSize":1,"totalSize":2,"embedderHeapUsedSize":0,"backingStorageSize":0}),
    )
    .await;
    let (_worker, result) = running.await.unwrap();
    assert!(result.is_ok(), "{result:?}");
}

#[tokio::test]
async fn native_caller_execution_cancellation_and_deadline_refuse_before_dispatch() {
    for canceled in [false, true] {
        let (mut worker, mut peer, _root, _context, page) = investigation::owned_page().await;
        let (session, _ordinary, mut executions, _responses, _control) = client(&worker);
        let id = session
            .submit(Uuid::new_v4(), evaluate(page), 5000)
            .unwrap();
        let mut command = executions.recv().await.unwrap();
        if canceled {
            session.cancel(id).await.unwrap();
        } else {
            command.deadline = Instant::now();
        }
        worker.perform_independent(command).await;
        let operation = settled(&worker.shared, id).await;
        assert_eq!(
            operation.code.as_deref(),
            Some(if canceled {
                "CANCELLED"
            } else {
                "NOT_DISPATCHED"
            })
        );
        assert_eq!(session.0.capacity.available_permits(), 32);
        assert!(
            tokio::time::timeout(Duration::from_millis(25), peer.next())
                .await
                .is_err()
        );
    }
}

#[tokio::test]
async fn native_caller_execution_revalidates_queued_authority_after_control_release() {
    let (mut worker, mut peer, _root, _context, page) = investigation::owned_page().await;
    let (session, _ordinary, mut executions, _responses, mut control_queue) = client(&worker);
    let id = session
        .submit(Uuid::new_v4(), evaluate(page), 5000)
        .unwrap();
    let command = executions.recv().await.unwrap();
    let client = session.clone();
    let releasing = tokio::spawn(async move {
        client
            .change_control("browser_control_release", json!({"expectedVersion":1}))
            .await
    });
    worker
        .apply_native_control(control_queue.recv().await.unwrap())
        .await;
    assert!(releasing.await.unwrap().is_ok());
    worker.perform_independent(command).await;
    let operation = settled(&worker.shared, id).await;
    assert_eq!(operation.code.as_deref(), Some("STALE_CONTROL_LEASE"));
    assert_eq!(
        session
            .submit(Uuid::new_v4(), evaluate(page), 5000)
            .unwrap_err()
            .code,
        "CONTROL_NOT_OWNED"
    );
    assert_eq!(session.0.capacity.available_permits(), 32);
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.next())
            .await
            .is_err()
    );
}

async fn verify_closed_call_cleanup(mut worker: Worker, mut peer: Peer, held: Value) {
    let connection = worker.connection.clone().unwrap();
    assert_eq!(
        connection.status().pending,
        0,
        "Acknowledged context closure must release active native capacity while retaining exact late-reply ownership separately"
    );
    assert_eq!(connection.status().retired_calls, 1);
    reply(&mut peer, &held, json!({"result":{"type":"undefined"}})).await;
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if connection.status().retired_calls == 0 {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(connection.status().pending, 0);
    assert!(
        connection.take_late_reply().is_none(),
        "A validated closed-target reply cannot retain caller payload or active capacity"
    );
    assert!(connection.status().loss.is_none());
    let reading = tokio::spawn(async move {
        let result = worker
            .call(
                Uuid::new_v4(),
                None,
                "Runtime.getHeapUsage",
                json!({}),
                deadline(),
            )
            .await;
        (worker, result)
    });
    let probe = command(&mut peer, "Runtime.getHeapUsage").await;
    reply(
        &mut peer,
        &probe,
        json!({"usedSize":1,"totalSize":2,"embedderHeapUsedSize":0,"backingStorageSize":0}),
    )
    .await;
    assert!(reading.await.unwrap().1.is_ok());
}

fn assert_closed_scope_unknown(result: Operation) -> Value {
    assert_eq!(result.code.as_deref(), Some("STALE_REFERENCE"));
    assert_eq!(
        result.outcome,
        Outcome::Unknown,
        "Caller effects remain unknown after a dispatched call loses its closed scope"
    );
    let partial = result.result.unwrap();
    assert_eq!(partial["scopeClosed"], true);
    assert_eq!(partial["potentiallyMutating"], true);
    assert_eq!(partial["callerEffectsUnknown"], true);
    partial
}
