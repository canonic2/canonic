//! Reserved automation controls retain pause identity while ordinary calls wait.
use super::*;
use crate::browser::automation_debugger::Action as Debug;
use crate::browser::automation_debugger::Location;

fn source(line: u32) -> Location {
    Location {
        file: "Acme-private-journey.rs".into(),
        line: Some(line),
        column: Some(3),
    }
}

async fn debugger_control(
    session: &Session,
    peer: &mut Fixture,
    context: Reference,
    action: Debug,
    method: &str,
) {
    let operation = session
        .submit(
            Uuid::new_v4(),
            Request::ContextDebugger { context, action },
            5000,
        )
        .unwrap();
    let request = command(peer, method).await;
    if matches!(method, "next" | "runTo" | "resume") {
        paused(peer, None).await;
    }
    reply(peer, &request).await;
    assert_eq!(
        complete(session, operation).await.outcome,
        Outcome::Succeeded
    );
}

#[tokio::test]
async fn debugger_pre_call_pause_and_next_precede_host_actionability_without_call_collection() {
    for next in [false, true] {
        waiting_caller_case(false, next).await;
    }
}

#[tokio::test]
async fn debugger_collected_ordinary_caller_precedes_host_actionability() {
    collected_unpaused_caller(false).await;
}

#[tokio::test]
async fn debugger_collected_nonmatching_run_to_caller_precedes_host_actionability() {
    collected_unpaused_caller(true).await;
}

async fn collected_unpaused_caller(run_to: bool) {
    use crate::browser::page::Action;
    let (session, mut peer, _root, context, page) = harness().await;
    debugger_control(&session, &mut peer, context, Debug::Enable, "enable").await;
    if run_to {
        paused(&mut peer, Some("Acme initial call")).await;
        let pause = status(&session, context, false).await;
        debugger_control(
            &session,
            &mut peer,
            context,
            Debug::RunTo {
                pause_id: serde_json::from_value(pause["pauseId"].clone()).unwrap(),
                location: source(30),
            },
            "runTo",
        )
        .await;
    }
    let action = if run_to {
        Action::AtSource {
            source: source(20),
            action: Box::new(Action::Click {}),
        }
    } else {
        Action::Click {}
    };
    let operation = session
        .submit(Uuid::new_v4(), compound_request(page, action), 5000)
        .unwrap();
    let click = command(&mut peer, "click").await;
    assert_eq!(click["params"]["strict"], true);
    assert_eq!(click["params"]["force"], false);
    assert_ne!(click["metadata"]["internal"], true);
    if run_to {
        assert_eq!(click["metadata"]["location"], json!(source(20)));
    }
    reply(&mut peer, &click).await;
    assert_eq!(
        complete(&session, operation).await.outcome,
        Outcome::Succeeded
    );
    close(&session).await;
}

#[tokio::test]
async fn debugger_collected_pause_dispatches_the_waiting_caller_before_host_actionability() {
    waiting_caller_case(true, false).await;
}

#[tokio::test]
async fn debugger_collected_next_dispatches_the_waiting_caller_before_host_actionability() {
    waiting_caller_case(true, true).await;
}

async fn waiting_caller_case(collect: bool, next: bool) {
    use crate::browser::page::Action;
    let (session, mut peer, _root, context, page) = harness().await;
    if collect {
        debugger_control(&session, &mut peer, context, Debug::Enable, "enable").await;
    }
    status(&session, context, false).await;
    let action = if next {
        paused(&mut peer, Some("Acme first call")).await;
        let status = status(&session, context, false).await;
        Debug::Next {
            pause_id: serde_json::from_value(status["pauseId"].clone()).unwrap(),
        }
    } else {
        Debug::Pause
    };
    debugger_control(
        &session,
        &mut peer,
        context,
        action,
        if next { "next" } else { "requestPause" },
    )
    .await;
    let operation = session
        .submit(
            Uuid::new_v4(),
            compound_request(page, Action::Click {}),
            5000,
        )
        .unwrap();
    let click = command(&mut peer, "click").await;
    assert_eq!(click["params"]["force"], false);
    assert_eq!(click["params"]["strict"], true);
    assert_ne!(click["metadata"]["internal"], true);
    paused(&mut peer, Some("Acme caller click")).await;
    let paused = status(&session, context, false).await;
    assert_eq!(
        session.0.host.inputs.lock().unwrap().held[&page].inflight,
        1
    );
    debugger_control(
        &session,
        &mut peer,
        context,
        Debug::Resume {
            pause_id: serde_json::from_value(paused["pauseId"].clone()).unwrap(),
        },
        "resume",
    )
    .await;
    reply(&mut peer, &click).await;
    assert_eq!(
        complete(&session, operation).await.outcome,
        Outcome::Succeeded
    );
    close(&session).await;
}

#[tokio::test]
async fn debugger_pre_call_run_to_matches_source_before_actionability_with_call_collection() {
    use crate::browser::page::Action;
    for (line, column, file) in [
        (30, 3, "Acme-private-journey.rs"),
        (20, 3, "Acme-private-journey.rs"),
        (30, 4, "Acme-private-journey.rs"),
        (30, 3, "Acme-other.rs"),
    ] {
        let (session, mut peer, _root, context, page) = harness().await;
        debugger_control(&session, &mut peer, context, Debug::Enable, "enable").await;
        paused(&mut peer, Some("Acme first call")).await;
        let paused = status(&session, context, false).await;
        let location = Location {
            file: "private-journey.rs".into(),
            ..source(30)
        };
        debugger_control(
            &session,
            &mut peer,
            context,
            Debug::RunTo {
                pause_id: serde_json::from_value(paused["pauseId"].clone()).unwrap(),
                location,
            },
            "runTo",
        )
        .await;
        let source = Location {
            file: file.into(),
            line: Some(line),
            column: Some(column),
        };
        let operation = session
            .submit(
                Uuid::new_v4(),
                compound_request(
                    page,
                    Action::AtSource {
                        source: source.clone(),
                        action: Box::new(Action::Click {}),
                    },
                ),
                5000,
            )
            .unwrap();
        let click = command(&mut peer, "click").await;
        assert_eq!(click["params"]["strict"], true);
        assert_eq!(click["params"]["force"], false);
        assert_eq!(click["metadata"]["location"], json!(source));
        reply(&mut peer, &click).await;
        assert_eq!(
            complete(&session, operation).await.outcome,
            Outcome::Succeeded
        );
        close(&session).await;
    }
}

#[tokio::test]
async fn debugger_pre_call_backend_refusal_retains_input_uncertainty_without_retry() {
    use crate::browser::page::Action;
    let (session, mut peer, _root, context, page) = harness().await;
    debugger_control(&session, &mut peer, context, Debug::Pause, "requestPause").await;
    let operation = session
        .submit(
            Uuid::new_v4(),
            compound_request(page, Action::Click {}),
            5000,
        )
        .unwrap();
    let click = command(&mut peer, "click").await;
    assert_eq!(click["params"]["force"], false);
    assert_eq!(click["params"]["strict"], true);
    paused(&mut peer, Some("Acme caller click")).await;
    let observed = status(&session, context, false).await;
    debugger_control(
        &session,
        &mut peer,
        context,
        Debug::Resume {
            pause_id: serde_json::from_value(observed["pauseId"].clone()).unwrap(),
        },
        "resume",
    )
    .await;
    peer.send(&json!({"id":click["id"],"error":{"error":{"name":"TimeoutError","message":"Acme element remains hidden"}}})).await;
    let settled = complete(&session, operation).await;
    assert_eq!(settled.outcome, Outcome::Unknown);
    assert_eq!(settled.code.as_deref(), Some("OUTCOME_UNKNOWN"));
    assert!(session.0.host.inputs.lock().unwrap().held[&page].uncertain);
    let again = session
        .submit(
            Uuid::new_v4(),
            compound_request(page, Action::Click {}),
            5000,
        )
        .unwrap();
    assert_eq!(
        complete(&session, again).await.code.as_deref(),
        Some("INPUT_STATE_CONFLICT")
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.request())
            .await
            .is_err()
    );
    close(&session).await;
}

#[tokio::test]
async fn caller_evaluation_dispatches_while_an_ordinary_input_is_automation_paused() {
    use crate::browser::page::Action;
    for variant in [
        "page",
        "frame",
        "handle",
        "retain",
        "read",
        "property",
        "properties",
        "property_batch",
        "properties_batch",
        "selector",
        "batch",
    ] {
        let (session, mut peer, _root, context, page) = harness().await;
        let (frame, handle) = retained_paused_handle(&session, &mut peer).await;
        debugger_control(&session, &mut peer, context, Debug::Pause, "requestPause").await;
        let input = session
            .submit(
                Uuid::new_v4(),
                compound_request(page, Action::Click {}),
                5000,
            )
            .unwrap();
        let click = command(&mut peer, "click").await;
        paused(&mut peer, Some("Acme caller input")).await;
        let observed = status(&session, context, false).await;
        let request = paused_caller_request(variant, page, frame, handle);
        assert!(control::fenced(&request));
        assert!(!control::responsive(&request));
        let evaluation = session.submit(Uuid::new_v4(), request, 5000).unwrap();
        for _ in 0..if variant.ends_with("batch") { 2 } else { 1 } {
            let wire = tokio::time::timeout(Duration::from_millis(250), peer.request())
                .await
                .expect("Caller evaluation must dispatch before the held input completes");
            assert_eq!(
                wire["method"],
                match variant {
                    "selector" => "evalOnSelector",
                    "retain" => "evaluateExpressionHandle",
                    "read" => "jsonValue",
                    "property" => "getProperty",
                    "property_batch" => "getProperty",
                    _ => "evaluateExpression",
                }
            );
            assert_ne!(wire["metadata"]["internal"], true);
            assert_eq!(
                wire["guid"],
                if matches!(variant, "handle" | "read") || variant.starts_with("propert") {
                    "Acme-paused-handle"
                } else {
                    "frame"
                }
            );
            assert!(
                tokio::time::timeout(Duration::from_millis(25), peer.request())
                    .await
                    .is_err(),
                "Batch calls remain ordered"
            );
            let result = if variant == "retain" || variant.starts_with("property") {
                json!({"handle":{"guid":"Acme-paused-handle"}})
            } else if variant.starts_with("properties") {
                json!({"value":crate::browser::values::argument(&json!({"names":["Acme"],"hasMore":false,"nextOffset":1,"offsetLimitReached":false,"snapshot":false,"keys":"own-enumerable-string"})).unwrap()["value"]})
            } else {
                json!({"value":{"n":42}})
            };
            peer.send(&json!({"id":wire["id"],"result":result})).await;
        }
        assert_eq!(
            complete(&session, evaluation).await.outcome,
            Outcome::Succeeded
        );
        assert!(!session.operation(input).unwrap().settled);
        assert_eq!(
            session.0.host.inputs.lock().unwrap().held[&page].inflight,
            1
        );
        debugger_control(
            &session,
            &mut peer,
            context,
            Debug::Resume {
                pause_id: serde_json::from_value(observed["pauseId"].clone()).unwrap(),
            },
            "resume",
        )
        .await;
        reply(&mut peer, &click).await;
        assert_eq!(complete(&session, input).await.outcome, Outcome::Succeeded);
        close(&session).await;
    }
}

#[tokio::test]
async fn caller_evaluation_independence_preserves_authority_capacity_and_mixed_batch_order() {
    let (session, _peer, _root, _context, page) = harness().await;
    let evaluate: Request =
        serde_json::from_value(json!({"operation":"evaluate","scope":page,"expression":"42"}))
            .unwrap();
    let callers = [
        evaluate.clone(),
        Request::FrameRetain { frame: page },
        Request::FrameOwnerRelease { frame: page },
        Request::HandleProperty {
            handle: page,
            name: "Acme".into(),
        },
        Request::HandleProperties {
            handle: page,
            query: Default::default(),
        },
    ];
    for caller in &callers {
        assert!(!control::independent(
            &Request::Batch {
                requests: vec![caller.clone(), Request::PageList {}]
            },
            false
        ));
    }
    for request in [
        Request::Batch { requests: vec![] },
        Request::Batch {
            requests: vec![evaluate.clone(), Request::PageList {}],
        },
    ] {
        assert!(!control::independent(
            &request,
            control::responsive(&request)
        ));
    }
    let permits = session.0.host.capacity.available_permits() as u32;
    let capacity = session
        .0
        .host
        .capacity
        .clone()
        .try_acquire_many_owned(permits)
        .unwrap();
    for caller in &callers {
        assert_eq!(
            session
                .submit(Uuid::new_v4(), caller.clone(), 5000)
                .unwrap_err()
                .code,
            "OPERATION_CAPACITY"
        );
    }
    assert_eq!(session.0.host.responsive.available_permits(), 8);
    drop(capacity);
    session
        .change_control("browser_control_release", json!({"expectedVersion":1}))
        .await
        .unwrap();
    for caller in callers {
        assert_eq!(
            session
                .submit(Uuid::new_v4(), caller, 5000)
                .unwrap_err()
                .code,
            "CONTROL_NOT_OWNED"
        );
    }
    close(&session).await;
}

#[tokio::test]
async fn socket_control_batch_dispatches_while_ordinary_input_is_held() {
    use crate::browser::page::Action;
    let (session, mut peer, _root, context, page) = harness().await;
    debugger_control(&session, &mut peer, context, Debug::Pause, "requestPause").await;
    let input = session
        .submit(
            Uuid::new_v4(),
            compound_request(page, Action::Click {}),
            5000,
        )
        .unwrap();
    let click = command(&mut peer, "click").await;
    paused(&mut peer, Some("Acme caller input")).await;
    let observed = status(&session, context, false).await;
    let socket: Request = serde_json::from_value(json!({"operation":"web_socket_route_handlers","scope":page,"frame":page,"routeId":Uuid::new_v4(),"socketId":"a".repeat(64),"handlers":{"pageMessage":null}})).unwrap();
    let batch = Request::Batch {
        requests: vec![socket.clone(), socket.clone()],
    };
    assert!(!control::responsive(&batch));
    let operation = session.submit(Uuid::new_v4(), batch, 5000).unwrap();
    let result = tokio::time::timeout(Duration::from_millis(250), complete(&session, operation))
        .await
        .expect("Socket-only batches must dispatch before held input completes");
    assert_eq!(result.code.as_deref(), Some("UNKNOWN_WEBSOCKET_ROUTE"));
    assert!(!session.operation(input).unwrap().settled);
    assert!(!control::independent(
        &Request::Batch {
            requests: vec![socket.clone(), Request::PageList {}]
        },
        false
    ));
    let permits = session.0.host.capacity.available_permits() as u32;
    let capacity = session
        .0
        .host
        .capacity
        .clone()
        .try_acquire_many_owned(permits)
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
    assert_eq!(session.0.host.responsive.available_permits(), 8);
    drop(capacity);
    debugger_control(
        &session,
        &mut peer,
        context,
        Debug::Resume {
            pause_id: serde_json::from_value(observed["pauseId"].clone()).unwrap(),
        },
        "resume",
    )
    .await;
    reply(&mut peer, &click).await;
    assert_eq!(complete(&session, input).await.outcome, Outcome::Succeeded);
    session
        .change_control("browser_control_release", json!({"expectedVersion":1}))
        .await
        .unwrap();
    assert_eq!(
        session
            .submit(
                Uuid::new_v4(),
                Request::Batch {
                    requests: vec![socket]
                },
                5000
            )
            .unwrap_err()
            .code,
        "CONTROL_NOT_OWNED"
    );
    close(&session).await;
}

#[tokio::test]
async fn callback_and_close_only_batches_dispatch_while_ordinary_input_is_held() {
    use crate::browser::page::Action;
    let (session, mut peer, _root, context, page) = harness().await;
    debugger_control(&session, &mut peer, context, Debug::Pause, "requestPause").await;
    let input = session
        .submit(
            Uuid::new_v4(),
            compound_request(page, Action::Click {}),
            5000,
        )
        .unwrap();
    let click = command(&mut peer, "click").await;
    paused(&mut peer, Some("Acme caller input")).await;
    let observed = status(&session, context, false).await;
    let stale = Reference {
        generation: Uuid::new_v4(),
        ..context
    };
    for request in [
        Request::CallbackResolve {
            callback: stale,
            response: contract::CallbackResponse::RouteContinue {},
        },
        Request::ContextClose { context: stale },
        serde_json::from_value(json!({"operation":"network_route","scope":stale,"action":{"kind":"clear","behavior":"wait"}})).unwrap(),
    ] {
        let batch = Request::Batch {
            requests: vec![request.clone()],
        };
        assert!(!control::responsive(&batch));
        let operation = session.submit(Uuid::new_v4(), batch, 5000).unwrap();
        let result =
            tokio::time::timeout(Duration::from_millis(250), complete(&session, operation))
                .await
                .expect("Callback/close-only batches must dispatch before held input completes");
        assert_eq!(result.code.as_deref(), Some("STALE_REFERENCE"));
        assert!(!session.operation(input).unwrap().settled);
        assert!(!control::independent(
            &Request::Batch {
                requests: vec![request.clone(), Request::PageList {}]
            },
            false
        ));
        let permits = session.0.host.capacity.available_permits() as u32;
        let capacity = session
            .0
            .host
            .capacity
            .clone()
            .try_acquire_many_owned(permits)
            .unwrap();
        assert_eq!(
            session
                .submit(
                    Uuid::new_v4(),
                    Request::Batch {
                        requests: vec![request]
                    },
                    5000
                )
                .unwrap_err()
                .code,
            "OPERATION_CAPACITY"
        );
        assert_eq!(session.0.host.responsive.available_permits(), 8);
        drop(capacity);
    }
    debugger_control(
        &session,
        &mut peer,
        context,
        Debug::Resume {
            pause_id: serde_json::from_value(observed["pauseId"].clone()).unwrap(),
        },
        "resume",
    )
    .await;
    reply(&mut peer, &click).await;
    assert_eq!(complete(&session, input).await.outcome, Outcome::Succeeded);
    close(&session).await;
}

#[tokio::test]
async fn debugger_only_batches_reach_held_call_with_order_capacity_and_authority() {
    use crate::browser::page::Action;
    let (session, mut peer, _root, context, page) = harness().await;
    debugger_control(&session, &mut peer, context, Debug::Pause, "requestPause").await;
    let input = session
        .submit(
            Uuid::new_v4(),
            compound_request(page, Action::Click {}),
            5000,
        )
        .unwrap();
    let click = command(&mut peer, "click").await;
    paused(&mut peer, Some("Acme caller input")).await;
    let observed = status(&session, context, false).await;
    let pause_id = serde_json::from_value(observed["pauseId"].clone()).unwrap();
    let read = Request::ContextDebugger {
        context,
        action: Debug::Status {
            include_details: false,
        },
    };
    let step = Request::ContextDebugger {
        context,
        action: Debug::Next { pause_id },
    };
    let before = session.0.host.capacity.available_permits();
    let batch = session
        .submit(
            Uuid::new_v4(),
            Request::Batch {
                requests: vec![read.clone(), step.clone()],
            },
            5000,
        )
        .unwrap();
    assert_eq!(session.0.host.capacity.available_permits(), before - 1);
    assert_eq!(session.0.host.responsive.available_permits(), 8);
    let next = tokio::time::timeout(Duration::from_millis(250), command(&mut peer, "next"))
        .await
        .expect("Debugger-only shared batches must reach their paused caller");
    paused(&mut peer, None).await;
    reply(&mut peer, &next).await;
    let result = complete(&session, batch).await;
    assert_eq!(result.outcome, Outcome::Succeeded);
    let result = result.result.unwrap();
    assert_eq!(result["results"][0]["pauseId"], observed["pauseId"]);
    assert_eq!(result["results"][1]["pauseRequested"], true);
    assert!(!session.operation(input).unwrap().settled);
    let mixed = session
        .submit(
            Uuid::new_v4(),
            Request::Batch {
                requests: vec![read.clone(), Request::PageList {}],
            },
            5000,
        )
        .unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(25), complete(&session, mixed))
            .await
            .is_err(),
        "Mixed batches must retain ordinary ordering"
    );
    assert_shared_batch_capacity(&session, read);
    reply(&mut peer, &click).await;
    assert_eq!(complete(&session, input).await.outcome, Outcome::Succeeded);
    assert_eq!(complete(&session, mixed).await.outcome, Outcome::Succeeded);
    session
        .change_control("browser_control_release", json!({"expectedVersion":1}))
        .await
        .unwrap();
    assert_eq!(
        session
            .submit(
                Uuid::new_v4(),
                Request::Batch {
                    requests: vec![step]
                },
                5000
            )
            .unwrap_err()
            .code,
        "CONTROL_NOT_OWNED"
    );
    close(&session).await;
}

async fn command(peer: &mut Fixture, method: &str) -> Value {
    let request = peer.request().await;
    assert_eq!(
        request["method"], method,
        "id={} guid={} metadata={}",
        request["id"], request["guid"], request["metadata"]
    );
    request
}
async fn reply(peer: &mut Fixture, request: &Value) {
    peer.send(&json!({"id":request["id"],"result":{}})).await;
}
async fn paused(peer: &mut Fixture, title: Option<&str>) {
    peer.send(&json!({"guid":"debugger","method":"pausedStateChanged","params":title.map_or(json!({}),|title|json!({"pausedDetails":{"title":title,"location":{"file":"Acme-private-source.js","line":7}}}))})).await;
}
async fn status(session: &Session, context: Reference, private: bool) -> Value {
    session
        .0
        .host
        .context_debugger(
            context,
            Debug::Status {
                include_details: private,
            },
            deadline(),
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn host_probes_are_internal_but_caller_click_and_evaluation_remain_visible() {
    use crate::browser::page::Action;
    let (session, mut peer, _root, _context, page) = harness().await;
    let click = session
        .submit(
            Uuid::new_v4(),
            compound_request(page, Action::Click {}),
            5000,
        )
        .unwrap();
    let preflight = command(&mut peer, "evaluateExpression").await;
    assert_eq!(preflight["metadata"]["internal"], true);
    peer.send(&json!({"id":preflight["id"],"result":{"value":crate::browser::values::argument(&json!({"satisfied":true})).unwrap()["value"]}})).await;
    let input = command(&mut peer, "click").await;
    assert_ne!(input["metadata"]["internal"], true);
    // Observations use their existing independent lane while the caller call waits.
    for (read, method, value) in [
        ("count", "queryCount", json!({"value":1})),
        (
            "text",
            "evalOnSelector",
            json!({"value":crate::browser::values::argument(&json!("{\"value\":\"Acme\"}")).unwrap()["value"]}),
        ),
        (
            "geometry",
            "evaluateExpression",
            json!({"value":crate::browser::values::argument(&json!({"width":800,"height":600,"deviceScaleFactor":1})).unwrap()["value"]}),
        ),
    ] {
        let request: Request = serde_json::from_value(
            json!({"operation":"page_read","page":page,"read":read,"maxBytes":8192}),
        )
        .unwrap();
        let request = if read == "count" {
            Request::Batch {
                requests: vec![request],
            }
        } else {
            request
        };
        let observing = session.submit(Uuid::new_v4(), request, 5000).unwrap();
        let wire = command(&mut peer, method).await;
        assert_eq!(wire["metadata"]["internal"], true, "{wire}");
        peer.send(&json!({"id":wire["id"],"result":value})).await;
        assert_eq!(
            complete(&session, observing).await.outcome,
            Outcome::Succeeded
        );
        assert!(!session.operation(click).unwrap().settled);
    }
    let geometry = serde_json::from_value(json!({"operation":"batch","requests":[
        {"operation":"page_read","page":page,"read":"geometry"}
    ]}))
    .unwrap();
    let observing = session.submit(Uuid::new_v4(), geometry, 5000).unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.request())
            .await
            .is_err(),
        "Batched geometry must wait for the held mutation"
    );
    reply(&mut peer, &input).await;
    assert_eq!(complete(&session, click).await.outcome, Outcome::Succeeded);
    let wire = command(&mut peer, "evaluateExpression").await;
    assert_eq!(wire["metadata"]["internal"], true);
    let value = crate::browser::values::argument(&json!({"width":800,"height":600})).unwrap();
    peer.send(&json!({"id":wire["id"],"result":{"value":value["value"]}}))
        .await;
    let observed = complete(&session, observing).await;
    assert_eq!(observed.outcome, Outcome::Succeeded);
    assert!(observed.result.unwrap()["results"][0]["inputObservation"].is_object());
    let evaluate: Request = serde_json::from_value(
        json!({"operation":"evaluate","scope":page,"expression":"42","isFunction":false}),
    )
    .unwrap();
    let evaluating = session.submit(Uuid::new_v4(), evaluate, 5000).unwrap();
    let wire = command(&mut peer, "evaluateExpression").await;
    assert_ne!(wire["metadata"]["internal"], true);
    peer.send(&json!({"id":wire["id"],"result":{"value":{"n":42}}}))
        .await;
    assert_eq!(
        complete(&session, evaluating).await.outcome,
        Outcome::Succeeded
    );
    close(&session).await;
}

#[tokio::test]
async fn nested_frame_condition_probes_and_cleanup_are_internal() {
    let (session, mut peer, _root, _context, page) = harness().await;
    peer.send(&created(
        "child",
        "Frame",
        json!({"url":"about:blank","name":"","parentFrame":{"guid":"frame"},"loadStates":[]}),
    ))
    .await;
    peer.send(&json!({"guid":"page","method":"frameAttached","params":{"frame":{"guid":"child"}}}))
        .await;
    let connection = session.0.host.connection().unwrap();
    let until = deadline();
    let wire = loop {
        if let Ok(wire) = connection.resolve("child", "Frame").await {
            break wire;
        }
        assert!(Instant::now() < until);
        tokio::task::yield_now().await;
    };
    let frame = loop {
        if let Some(frame) = session
            .0
            .host
            .state
            .lock()
            .unwrap()
            .by_wire(&wire)
            .map(|r| r.reference)
        {
            break frame;
        }
        assert!(Instant::now() < until);
        tokio::task::yield_now().await;
    };
    let request: Request = serde_json::from_value(
        json!({"operation":"page_wait","page":page,"frame":frame,"condition":{"kind":"visible"}}),
    )
    .unwrap();
    let waiting = session.submit(Uuid::new_v4(), request, 5000).unwrap();
    let element = command(&mut peer, "frameElement").await;
    assert_eq!(element["metadata"]["internal"], true);
    peer.send(&created(
        "frame-element",
        "ElementHandle",
        json!({"preview":"iframe"}),
    ))
    .await;
    peer.send(&json!({"id":element["id"],"result":{"element":{"guid":"frame-element"}}}))
        .await;
    let visible = command(&mut peer, "isVisible").await;
    assert_eq!(visible["metadata"]["internal"], true);
    peer.send(&json!({"id":visible["id"],"result":{"value":true}}))
        .await;
    let disposal = command(&mut peer, "dispose").await;
    assert_eq!(disposal["metadata"]["internal"], true);
    reply(&mut peer, &disposal).await;
    let query = command(&mut peer, "evaluateExpression").await;
    assert_eq!(query["metadata"]["internal"], true);
    peer.send(&json!({"id":query["id"],"result":{"value":crate::browser::values::argument(&json!({"satisfied":true})).unwrap()["value"]}})).await;
    assert_eq!(
        complete(&session, waiting).await.outcome,
        Outcome::Succeeded
    );
    close(&session).await;
}

#[tokio::test]
async fn invalid_source_annotations_refuse_the_whole_batch_before_allocation() {
    use crate::browser::page::Action;
    let (session, mut peer, _root, _context, page) = harness().await;
    let wrap = |action| Action::AtSource {
        source: source(10),
        action: Box::new(action),
    };
    let mut invalid = source(10);
    invalid.file = "Acme\0source".into();
    for (action, code) in [
        (
            Action::AtSource {
                source: invalid,
                action: Box::new(Action::Click {}),
            },
            "CALL_SOURCE_BOUNDS",
        ),
        (wrap(wrap(Action::Click {})), "CALL_SOURCE_NESTING"),
        (
            wrap(Action::Press {
                key: "Shift+NotAKey".into(),
            }),
            "INVALID_BROWSER_KEY",
        ),
    ] {
        let error = session
            .submit(
                Uuid::new_v4(),
                Request::Batch {
                    requests: vec![
                        Request::ContextCreate {
                            options: Default::default(),
                        },
                        compound_request(page, action),
                    ],
                },
                5000,
            )
            .unwrap_err();
        assert_eq!(error.code, code);
    }
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.request())
            .await
            .is_err()
    );
    close(&session).await;
}

#[tokio::test]
async fn source_annotated_input_and_run_to_keep_distinct_wire_and_pause_owners() {
    use crate::browser::page::Action;
    let (session, mut peer, _root, context, page) = harness().await;
    status(&session, context, false).await;
    let input = session
        .submit(
            Uuid::new_v4(),
            compound_request(
                page,
                Action::AtSource {
                    source: source(10),
                    action: Box::new(Action::Press { key: "a".into() }),
                },
            ),
            5000,
        )
        .unwrap();
    let press = command(&mut peer, "press").await;
    assert_eq!(press["metadata"]["location"], json!(source(10)));
    assert!(press["params"].get("source").is_none());
    assert!(press["params"].get("location").is_none());
    peer.send(&json!({"guid":"debugger","method":"pausedStateChanged","params":{"pausedDetails":{"title":"Acme source input","location":source(10)}}})).await;
    let first = status(&session, context, false).await;
    assert!(!first.to_string().contains("Acme-private"));
    let first_pause = serde_json::from_value(first["pauseId"].clone()).unwrap();
    let run = session
        .submit(
            Uuid::new_v4(),
            Request::ContextDebugger {
                context,
                action: Debug::RunTo {
                    pause_id: first_pause,
                    location: source(30),
                },
            },
            5000,
        )
        .unwrap();
    let request = command(&mut peer, "runTo").await;
    assert_eq!(request["params"]["location"], json!(source(30)));
    assert!(request["metadata"].get("location").is_none());
    paused(&mut peer, None).await;
    reply(&mut peer, &request).await;
    let result = complete(&session, run).await;
    assert_eq!(result.outcome, Outcome::Succeeded);
    assert_eq!(result.result.unwrap()["lastControl"]["action"], "run_to");
    assert!(!session.operation(input).unwrap().settled);
    let stale = session
        .submit(
            Uuid::new_v4(),
            Request::ContextDebugger {
                context,
                action: Debug::RunTo {
                    pause_id: first_pause,
                    location: source(30),
                },
            },
            5000,
        )
        .unwrap();
    assert_eq!(
        complete(&session, stale).await.code.as_deref(),
        Some("STALE_AUTOMATION_PAUSE")
    );
    reply(&mut peer, &press).await;
    let result = complete(&session, input).await;
    assert_eq!(result.outcome, Outcome::Succeeded);
    assert!(!result.result.unwrap().to_string().contains("Acme-private"));
    close(&session).await;
}

#[tokio::test]
async fn run_to_lost_reply_preserves_the_original_control_until_teardown() {
    let (session, mut peer, _root, context, _page) = harness().await;
    status(&session, context, false).await;
    paused(&mut peer, Some("Acme paused input")).await;
    let first = status(&session, context, false).await;
    let run = session
        .submit(
            Uuid::new_v4(),
            Request::ContextDebugger {
                context,
                action: Debug::RunTo {
                    pause_id: serde_json::from_value(first["pauseId"].clone()).unwrap(),
                    location: source(30),
                },
            },
            5000,
        )
        .unwrap();
    command(&mut peer, "runTo").await;
    drop(peer);
    let result = complete(&session, run).await;
    assert_eq!(result.outcome, Outcome::Unknown);
    assert_eq!(result.code.as_deref(), Some("OUTCOME_UNKNOWN"));
    let until = deadline();
    loop {
        let state = session
            .0
            .host
            .automation_debuggers
            .lock()
            .unwrap()
            .summary();
        if state[0]["sourceLost"] == true {
            assert!(state[0]["pauseId"].is_null());
            assert!(state[0]["pendingControlId"].is_string());
            assert_eq!(state[0]["lastControl"]["action"], "run_to");
            assert!(!state.to_string().contains("Acme-private"));
            break;
        }
        assert!(Instant::now() < until);
        tokio::task::yield_now().await;
    }
    close(&session).await;
}

#[tokio::test]
async fn automation_call_collection_keeps_enable_replay_private_and_fences_source_loss() {
    let (session, mut peer, _root, context, _page) = harness().await;
    let calls = |private| Debug::Calls {
        offset: 0,
        limit: 16,
        include_details: private,
    };
    let host = &session.0.host;
    assert_eq!(
        host.context_debugger(context, calls(false), deadline())
            .await
            .unwrap_err()
            .code,
        "AUTOMATION_CALL_COLLECTION_DISABLED"
    );
    let enable = session
        .submit(
            Uuid::new_v4(),
            Request::ContextDebugger {
                context,
                action: Debug::Enable,
            },
            5000,
        )
        .unwrap();
    let request = tokio::select! {
        request=command(&mut peer,"enable")=>request,
        result=complete(&session,enable)=>panic!("Enable settled before dispatch: {result:?}"),
    };
    peer.send(&json!({"guid":"debugger","method":"apiCallsUpdated","params":{"apiCalls":[{"id":"Acme-private-id","title":"Acme-private-click","location":{"file":"Acme-private-file","line":7},"status":"running","newLogEntries":["Acme-private-log"]}]}})).await;
    reply(&mut peer, &request).await;
    assert_eq!(complete(&session, enable).await.outcome, Outcome::Succeeded);
    let public = host
        .context_debugger(context, calls(false), deadline())
        .await
        .unwrap();
    assert_eq!(public["calls"].as_array().unwrap().len(), 1);
    assert!(!public.to_string().contains("Acme-private"));
    let private = host
        .context_debugger(context, calls(true), deadline())
        .await
        .unwrap();
    assert_eq!(
        private["calls"][0]["details"]["title"],
        "Acme-private-click"
    );
    assert_eq!(private["calls"][0]["logs"], json!(["Acme-private-log"]));
    assert_eq!(
        host.context_debugger(context, Debug::Enable, deadline())
            .await
            .unwrap()["callCollectionEnabled"],
        true
    );
    host.automation_debuggers.lock().unwrap().lose();
    assert_eq!(
        host.context_debugger(context, calls(true), deadline())
            .await
            .unwrap_err()
            .code,
        "AUTOMATION_DEBUGGER_LOST"
    );
    assert!(
        !status(&session, context, true)
            .await
            .to_string()
            .contains("Acme-private")
    );
    close(&session).await;
}

#[tokio::test]
async fn automation_call_enable_lost_reply_stays_owned_and_is_not_replayed() {
    let (session, mut peer, _root, context, _page) = harness().await;
    let enable = session
        .submit(
            Uuid::new_v4(),
            Request::ContextDebugger {
                context,
                action: Debug::Enable,
            },
            5000,
        )
        .unwrap();
    command(&mut peer, "enable").await;
    drop(peer);
    let operation = complete(&session, enable).await;
    assert_eq!(operation.outcome, Outcome::Unknown);
    assert_eq!(operation.code.as_deref(), Some("OUTCOME_UNKNOWN"));
    let until = deadline();
    loop {
        let state = session
            .0
            .host
            .automation_debuggers
            .lock()
            .unwrap()
            .summary();
        if state[0]["sourceLost"] == true {
            assert!(!state[0]["pendingControlId"].is_null());
            assert_eq!(state[0]["callCollectionEnabled"], false);
            break;
        }
        assert!(
            Instant::now() < until,
            "Lost enable was not fenced: {state}"
        );
        tokio::task::yield_now().await;
    }
    assert_eq!(
        session
            .0
            .host
            .context_debugger(context, Debug::Enable, deadline())
            .await
            .unwrap_err()
            .code,
        "BACKEND_LOST"
    );
    close(&session).await;
}

#[tokio::test]
async fn automation_debugger_steps_and_resumes_with_private_details_while_allocation_waits() {
    let (session, mut peer, _root, context, _page) = harness().await;
    status(&session, context, false).await;
    let allocation = session
        .submit(
            Uuid::new_v4(),
            Request::PageCreate {
                context,
                select: false,
            },
            5000,
        )
        .unwrap();
    let creating = command(&mut peer, "newPage").await;
    let arm = session
        .submit(
            Uuid::new_v4(),
            Request::ContextDebugger {
                context,
                action: Debug::Pause,
            },
            5000,
        )
        .unwrap();
    let pause = command(&mut peer, "requestPause").await;
    reply(&mut peer, &pause).await;
    assert_eq!(complete(&session, arm).await.outcome, Outcome::Succeeded);
    paused(&mut peer, Some("Acme-private-action")).await;
    let first = status(&session, context, false).await;
    assert_eq!(first["paused"], true);
    assert_eq!(first["javascriptDebugger"], false);
    assert!(!first.to_string().contains("Acme-private"));
    let details = status(&session, context, true).await;
    assert_eq!(details["details"]["title"], "Acme-private-action");
    let pause_id = serde_json::from_value(first["pauseId"].clone()).unwrap();
    let next = session
        .submit(
            Uuid::new_v4(),
            Request::ContextDebugger {
                context,
                action: Debug::Next { pause_id },
            },
            5000,
        )
        .unwrap();
    let request = command(&mut peer, "next").await;
    paused(&mut peer, None).await;
    reply(&mut peer, &request).await;
    assert_eq!(complete(&session, next).await.outcome, Outcome::Succeeded);
    assert_eq!(
        status(&session, context, false).await["pauseRequested"],
        true
    );
    let stale = session
        .submit(
            Uuid::new_v4(),
            Request::ContextDebugger {
                context,
                action: Debug::Resume { pause_id },
            },
            5000,
        )
        .unwrap();
    assert_eq!(
        complete(&session, stale).await.code.as_deref(),
        Some("STALE_AUTOMATION_PAUSE")
    );
    paused(&mut peer, Some("Acme second action")).await;
    let second = status(&session, context, false).await;
    assert_ne!(first["pauseId"], second["pauseId"]);
    let resume = session
        .submit(
            Uuid::new_v4(),
            Request::ContextDebugger {
                context,
                action: Debug::Resume {
                    pause_id: serde_json::from_value(second["pauseId"].clone()).unwrap(),
                },
            },
            5000,
        )
        .unwrap();
    let request = command(&mut peer, "resume").await;
    paused(&mut peer, None).await;
    reply(&mut peer, &request).await;
    assert_eq!(complete(&session, resume).await.outcome, Outcome::Succeeded);
    assert_eq!(status(&session, context, false).await["paused"], false);
    peer.send(&method_error(creating["id"].clone())).await;
    assert_eq!(
        complete(&session, allocation).await.code.as_deref(),
        Some("BROWSER_METHOD_FAILED")
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.request())
            .await
            .is_err()
    );
    close(&session).await;
}

#[tokio::test]
async fn automation_debugger_lost_resume_fences_pause_and_keeps_unknown_control_until_cleanup() {
    let (session, mut peer, _root, context, _page) = harness().await;
    status(&session, context, false).await;
    paused(&mut peer, Some("Acme pause")).await;
    let observed = status(&session, context, false).await;
    let resume = session
        .submit(
            Uuid::new_v4(),
            Request::ContextDebugger {
                context,
                action: Debug::Resume {
                    pause_id: serde_json::from_value(observed["pauseId"].clone()).unwrap(),
                },
            },
            5000,
        )
        .unwrap();
    command(&mut peer, "resume").await;
    drop(peer);
    let result = complete(&session, resume).await;
    assert_eq!(result.outcome, Outcome::Unknown);
    assert_eq!(result.code.as_deref(), Some("OUTCOME_UNKNOWN"));
    let until = deadline();
    loop {
        let summary = session
            .0
            .host
            .automation_debuggers
            .lock()
            .unwrap()
            .summary();
        if summary[0]["sourceLost"] == true {
            assert_eq!(summary[0]["stateKnown"], false);
            assert!(summary[0]["pauseId"].is_null());
            assert!(summary[0]["pendingControlId"].is_string());
            break;
        }
        assert!(Instant::now() < until);
        tokio::task::yield_now().await;
    }
    close(&session).await;
    assert!(
        session.status()["automationDebuggers"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn automation_pause_release_waits_for_context_close_and_repeated_close_does_not_replay() {
    let (session, mut peer, _root, context, _page) = harness().await;
    status(&session, context, false).await;
    paused(&mut peer, Some("Acme paused call")).await;
    status(&session, context, false).await;
    let closing = session
        .submit(Uuid::new_v4(), Request::ContextClose { context }, 5000)
        .unwrap();
    let close_context = command(&mut peer, "close").await;
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.request())
            .await
            .is_err()
    );
    reply(&mut peer, &close_context).await;
    let release = command(&mut peer, "resume").await;
    assert_eq!(release["guid"], "debugger");
    reply(&mut peer, &release).await;
    assert_eq!(
        complete(&session, closing).await.outcome,
        Outcome::Succeeded
    );
    let repeated = session
        .submit(Uuid::new_v4(), Request::ContextClose { context }, 5000)
        .unwrap();
    assert_eq!(
        complete(&session, repeated).await.outcome,
        Outcome::Succeeded
    );
    assert!(
        session.status()["automationDebuggerCleanup"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.request())
            .await
            .is_err()
    );
    close(&session).await;
}

#[tokio::test]
async fn automation_pause_lost_closed_scope_release_retains_cleanup_without_repeating_resume() {
    let (session, mut peer, _root, context, _page) = harness().await;
    status(&session, context, false).await;
    paused(&mut peer, Some("Acme paused call")).await;
    status(&session, context, false).await;
    let closing = session
        .submit(Uuid::new_v4(), Request::ContextClose { context }, 5000)
        .unwrap();
    let request = command(&mut peer, "close").await;
    reply(&mut peer, &request).await;
    command(&mut peer, "resume").await;
    drop(peer);
    let result = complete(&session, closing).await;
    assert_eq!(result.outcome, Outcome::Unknown);
    assert_eq!(result.result.unwrap()["closed"], true);
    assert_eq!(
        session.status()["automationDebuggerCleanup"][0]["commandDispatchedOrUnknown"],
        true
    );
    let repeated = session
        .0
        .host
        .close_resource(context, Kind::Context, "close", deadline())
        .await
        .unwrap_err();
    assert_eq!(repeated.code, "AUTOMATION_DEBUGGER_CLEANUP_PENDING");
    close(&session).await;
    assert!(
        session.status()["automationDebuggerCleanup"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn compound_context_close_releases_wait_and_keeps_the_original_late_reply_owned() {
    use crate::browser::page::Action;
    let (session, mut peer, _root, context, page) = harness().await;
    let action = session
        .submit(
            Uuid::new_v4(),
            compound_request(
                page,
                Action::Press {
                    key: "Shift+a".into(),
                },
            ),
            5000,
        )
        .unwrap();
    let input = command(&mut peer, "press").await;
    let closing = session
        .submit(Uuid::new_v4(), Request::ContextClose { context }, 5000)
        .unwrap();
    let request = command(&mut peer, "close").await;
    reply(&mut peer, &request).await;
    assert_eq!(
        complete(&session, closing).await.outcome,
        Outcome::Succeeded
    );
    let result = complete(&session, action).await;
    assert_eq!(result.outcome, Outcome::Unknown);
    let partial = result.result.unwrap();
    assert_eq!(partial["scopeClosed"], true);
    assert_eq!(partial["cleanup"]["complete"], true);
    assert!(
        !session
            .0
            .host
            .inputs
            .lock()
            .unwrap()
            .held
            .contains_key(&page)
    );
    reply(&mut peer, &input).await;
    session
        .0
        .host
        .connection()
        .unwrap()
        .event_barrier(deadline())
        .await
        .unwrap();
    assert_eq!(session.operation(action).unwrap().outcome, Outcome::Unknown);
    assert_eq!(session.0.host.state.lock().unwrap().phase, Phase::Ready);
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.request())
            .await
            .is_err()
    );
    close(&session).await;
}

#[tokio::test]
async fn closed_context_terminal_input_error_preserves_uncertainty_and_completed_cleanup() {
    use crate::browser::page::Action;
    let (session, mut peer, _root, context, page) = harness().await;
    let host = session.0.host.clone();
    let connection = host.connection().unwrap();
    let frame = connection.resolve("frame", "Frame").await.unwrap();
    let action = Action::Press {
        key: "Shift+a".into(),
    };
    let call = host.compound_input(
        page,
        &frame,
        &action,
        "press",
        json!({"selector":"body","strict":true,"key":"Shift+a"}),
        deadline(),
    );
    tokio::pin!(call);
    assert!(futures_util::poll!(&mut call).is_pending());
    let input = command(&mut peer, "press").await;
    let closing = session
        .submit(Uuid::new_v4(), Request::ContextClose { context }, 5000)
        .unwrap();
    let request = command(&mut peer, "close").await;
    reply(&mut peer, &request).await;
    assert_eq!(
        complete(&session, closing).await.outcome,
        Outcome::Succeeded
    );
    peer.send(&method_error(input["id"].clone())).await;
    let until = deadline();
    while connection.status().pending != 0 {
        assert!(Instant::now() < until);
        tokio::task::yield_now().await;
    }
    let error = call.await.unwrap_err();
    assert_eq!(error.code, "OUTCOME_UNKNOWN");
    let partial = error.partial.unwrap();
    assert_eq!(partial["replyReceived"], true);
    assert_eq!(partial["cleanup"]["complete"], true);
    assert_eq!(partial["scopeClosed"], true);
    assert!(!host.inputs.lock().unwrap().held.contains_key(&page));
    close(&session).await;
}

#[tokio::test]
async fn closed_context_cleanup_accepts_only_the_known_no_pause_refusal() {
    for known_no_pause in [true, false] {
        let (session, mut peer, _root, context, _page) = harness().await;
        status(&session, context, false).await;
        let closing = session
            .submit(Uuid::new_v4(), Request::ContextClose { context }, 5000)
            .unwrap();
        let request = command(&mut peer, "close").await;
        reply(&mut peer, &request).await;
        let release = command(&mut peer, "resume").await;
        peer.send(&json!({"id":release["id"],"error":{"error":{"name":"Error","message":if known_no_pause{"Debugger is not paused"}else{"Acme cleanup refusal"}}},"log":[]})).await;
        let result = complete(&session, closing).await;
        if known_no_pause {
            assert_eq!(result.outcome, Outcome::Succeeded);
            assert!(
                session.status()["automationDebuggerCleanup"]
                    .as_array()
                    .unwrap()
                    .is_empty()
            );
        } else {
            assert_ne!(result.outcome, Outcome::Succeeded);
            assert_eq!(
                session.status()["automationDebuggerCleanup"]
                    .as_array()
                    .unwrap()
                    .len(),
                1
            );
        }
        close(&session).await;
    }
}

async fn retained_paused_handle(session: &Session, peer: &mut Fixture) -> (Reference, Reference) {
    let frame = session
        .0
        .host
        .state
        .lock()
        .unwrap()
        .resources
        .values()
        .find(|resource| resource.kind == Kind::Frame)
        .unwrap()
        .reference;
    let mut retained = created("Acme-paused-handle", "JSHandle", json!({"preview":"Acme"}));
    retained["guid"] = json!("frame");
    peer.send(&retained).await;
    let connection = session.0.host.connection().unwrap();
    connection.event_barrier(deadline()).await.unwrap();
    let wire = connection
        .resolve("Acme-paused-handle", "JSHandle")
        .await
        .unwrap();
    let handle = session
        .0
        .host
        .state
        .lock()
        .unwrap()
        .issue(Kind::Handle, wire, Some(frame))
        .unwrap();
    (frame, handle)
}

fn paused_caller_request(
    variant: &str,
    page: Reference,
    frame: Reference,
    handle: Reference,
) -> Request {
    let scope = match variant {
        "frame" => frame,
        "handle" => handle,
        _ => page,
    };
    let value = if variant == "read" {
        json!({"operation":"handle_read","handle":handle})
    } else if variant.starts_with("property") {
        json!({"operation":"handle_property","handle":handle,"name":"Acme"})
    } else if variant.starts_with("properties") {
        json!({"operation":"handle_properties","handle":handle})
    } else if variant == "selector" {
        json!({"operation":"selector_evaluate","scope":scope,"options":{"locator":{"kind":"css","selector":"button"},"expression":"element=>element.id","isFunction":true}})
    } else {
        json!({"operation":"evaluate","scope":scope,"expression":"42","isFunction":false,"returnHandle":variant == "retain"})
    };
    let request: Request = serde_json::from_value(value).unwrap();
    if variant.ends_with("batch") {
        Request::Batch {
            requests: vec![request.clone(), request],
        }
    } else {
        request
    }
}

fn assert_shared_batch_capacity(session: &Session, read: Request) {
    let permits = session.0.host.capacity.available_permits() as u32;
    let capacity = session
        .0
        .host
        .capacity
        .clone()
        .try_acquire_many_owned(permits)
        .unwrap();
    assert_eq!(
        session
            .submit(
                Uuid::new_v4(),
                Request::Batch {
                    requests: vec![read]
                },
                5000
            )
            .unwrap_err()
            .code,
        "OPERATION_CAPACITY"
    );
    assert_eq!(session.0.host.responsive.available_permits(), 8);
    drop(capacity);
}
