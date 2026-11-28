//! Native credentials preserve callback authority, exact request lifetime and HTTP ownership.
use super::investigation::owned_page;
use super::*;
use std::sync::atomic::Ordering;

fn credentials() -> crate::browser::http::Credentials {
    serde_json::from_value(json!([
        {"origin":"http://example.invalid","username":"wrong","password":"wrong"},
        {"origin":"HTTPS://EXAMPLE.COM","username":"Acme","password":"Acme-private","send":"always"}
    ]))
    .unwrap()
}
fn event(method: &str, request: &str, url: &str) -> EventItem {
    let mut params = json!({"requestId":request,"request":{"url":url,"method":"GET","headers":{},"initialPriority":"High","referrerPolicy":"no-referrer"},"frameId":"frame-acme","resourceType":"Fetch"});
    if method == "Fetch.requestPaused" {
        params["networkId"] = json!("network-acme");
    } else {
        params["authChallenge"] = json!({"origin":"https://foreign.invalid","scheme":"Digest","realm":"Acme","source":"Proxy"});
    }
    crate::browser::cdp::schema::event(method, &params).unwrap();
    EventItem::Event(Event {
        method: method.into(),
        session_id: Some("session-acme".into()),
        params,
    })
}
fn install(worker: &mut Worker, context: Reference) {
    worker.context_layout.insert(
        context,
        ContextOptions {
            http_credentials: Some(credentials()),
            ..Default::default()
        },
    );
    worker
        .http_contexts
        .context_credentials_initial(context, Some(&credentials()));
}

async fn automatic_context() -> (Worker, Peer, tempfile::TempDir, Reference, Reference) {
    let (mut worker, mut peer, root, context, page) = owned_page().await;
    install(&mut worker, context);
    let task = tokio::spawn(async move {
        worker
            .configure_fetch(Uuid::new_v4(), "session-acme", deadline())
            .await
            .unwrap();
        worker
    });
    let network = command(&mut peer, "Network.enable").await;
    reply(&mut peer, &network, json!({})).await;
    let fetch = command(&mut peer, "Fetch.enable").await;
    reply(&mut peer, &fetch, json!({})).await;
    (task.await.unwrap(), peer, root, context, page)
}

#[tokio::test]
async fn native_credentials_setter_settles_queued_private_replies_under_their_original_defaults() {
    for authentication in [false, true] {
        for reset in [false, true] {
            let (mut worker, mut peer, _root, context, _page) = automatic_context().await;
            let mut inherited = crate::browser::http::Options::default();
            worker
                .http_contexts
                .inherit_credentials(&mut inherited, context, Some(&credentials()));
            worker
                .apply(event(
                    if authentication {
                        "Fetch.authRequired"
                    } else {
                        "Fetch.requestPaused"
                    },
                    "queued-auto",
                    "https://example.com/automatic",
                ))
                .unwrap();
            let mut replacement = credentials();
            if let crate::browser::http::Credentials::List(values) = &mut replacement {
                values[1].password = "Acme-replacement".into();
            }
            let selected = (!reset).then_some(replacement.clone());
            let mut task = tokio::spawn(async move {
                let result = worker
                    .configure_context_credentials(Uuid::new_v4(), context, selected, deadline())
                    .await;
                (worker, result)
            });
            let method = if authentication {
                "Fetch.continueWithAuth"
            } else {
                "Fetch.continueRequest"
            };
            let call = tokio::select! {
                result=&mut task=>panic!("Credential setter refused serviceable private work: {:?}",result.map(|(_,result)|result)),
                call=command(&mut peer,method)=>call,
            };
            assert_eq!(call["params"]["requestId"], "queued-auto");
            if authentication {
                assert_eq!(
                    call["params"]["authChallengeResponse"]["password"],
                    "Acme-private"
                );
            }
            assert_eq!(
                inherited.effective_credentials().unwrap(),
                Some(credentials())
            );
            reply(&mut peer, &call, json!({})).await;
            if reset {
                let disable = command(&mut peer, "Fetch.disable").await;
                reply(&mut peer, &disable, json!({})).await;
            }
            let (worker, result) = task.await.unwrap();
            assert_eq!(
                result.unwrap()["requested"]["credentialCount"],
                if reset { 0 } else { 2 }
            );
            assert!(worker.interactions.routes.is_empty());
            if reset {
                assert_eq!(inherited.effective_credentials().unwrap().unwrap().len(), 0);
            } else {
                assert_eq!(
                    inherited.effective_credentials().unwrap(),
                    Some(replacement)
                );
            }
            assert!(
                tokio::time::timeout(Duration::from_millis(25), peer.next())
                    .await
                    .is_err()
            );
        }
    }
}

#[tokio::test]
async fn native_credentials_pending_manual_callback_keeps_private_replies_and_defaults_untouched() {
    let (mut worker, mut peer, _root, context, page) = automatic_context().await;
    worker
        .scoped_route_set(
            Uuid::new_v4(),
            page,
            Kind::Page,
            Some("https://example.com/manual".into()),
            false,
            deadline(),
        )
        .await
        .unwrap();
    worker
        .apply(event(
            "Fetch.requestPaused",
            "queued-private",
            "https://example.com/automatic",
        ))
        .unwrap();
    worker
        .apply(event(
            "Fetch.requestPaused",
            "queued-manual",
            "https://example.com/manual",
        ))
        .unwrap();
    let result = worker
        .configure_context_credentials(Uuid::new_v4(), context, None, deadline())
        .await;
    assert_eq!(result.unwrap_err().code, "ROUTE_CALLBACKS_PENDING");
    assert_eq!(worker.interactions.routes.len(), 2);
    assert_eq!(
        worker.context_layout[&context].http_credentials,
        Some(credentials())
    );
    assert!(
        worker
            .http_contexts
            .require_context_credentials(context)
            .is_ok()
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.next())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn native_credentials_refusal_attributes_private_continuation_state_without_authority_or_secrets()
 {
    for state in ["responding", "retry_deferred", "expired"] {
        let (mut worker, mut peer, _root, context, _page) = automatic_context().await;
        worker
            .apply(event(
                "Fetch.authRequired",
                "Acme-private-native",
                "https://example.com/Acme-private-path?secret=example",
            ))
            .unwrap();
        let route = worker.interactions.routes.values_mut().next().unwrap();
        route.responding = state == "responding";
        if state == "retry_deferred" {
            route.retry_at = Instant::now() + Duration::from_secs(5);
        }
        if state == "expired" {
            route.expires = Instant::now() - Duration::from_millis(1);
        }
        route.last_failure = Some(if state == "responding" {
            "OUTCOME_UNKNOWN"
        } else {
            "BROWSER_METHOD_FAILED"
        });
        let error = worker
            .configure_context_credentials(Uuid::new_v4(), context, None, deadline())
            .await
            .unwrap_err();
        assert_eq!(error.code, "ROUTE_CALLBACKS_PENDING");
        let partial = error.partial.unwrap();
        assert_eq!(partial["context"], json!(context));
        assert_eq!(partial["manualCallbacks"], 0);
        assert_eq!(partial["automaticContinuations"], 1);
        assert_eq!(
            partial["automaticResponding"],
            usize::from(state == "responding")
        );
        assert_eq!(
            partial["automaticRetryDeferred"],
            usize::from(state == "retry_deferred")
        );
        assert_eq!(partial["automaticExpired"], usize::from(state == "expired"));
        assert_eq!(
            partial["automaticFailureCodes"][if state == "responding" {
                "OUTCOME_UNKNOWN"
            } else {
                "BROWSER_METHOD_FAILED"
            }],
            1
        );
        assert_eq!(partial["defaultsChanged"], false);
        assert_eq!(partial["replayed"], false);
        let text = partial.to_string();
        for secret in [
            "Acme-private",
            "example.com",
            "password",
            "username",
            "callbackId",
            "native",
        ] {
            assert!(!text.contains(secret), "{text}");
        }
        assert_eq!(
            worker.context_layout[&context].http_credentials,
            Some(credentials())
        );
        assert!(
            worker
                .http_contexts
                .require_context_credentials(context)
                .is_ok()
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(25), peer.next())
                .await
                .is_err()
        );
    }
}

#[tokio::test]
async fn native_credentials_http_preparation_keeps_an_automatic_reply_until_acknowledged() {
    let (mut worker, mut peer, _root, context, _page) = automatic_context().await;
    worker
        .apply(event(
            "Fetch.authRequired",
            "prepare-overlap",
            "https://example.com/automatic",
        ))
        .unwrap();
    let (ready, prepared) = tokio::sync::oneshot::channel::<()>();
    let mut task = tokio::spawn(async move {
        let result = worker
            .await_http_preparation(
                Uuid::new_v4(),
                async {
                    prepared.await.unwrap();
                    Ok::<_, Error>("Acme prepared")
                },
                deadline(),
            )
            .await;
        (worker, result)
    });
    let continued = command(&mut peer, "Fetch.continueWithAuth").await;
    ready.send(()).unwrap();
    tokio::select! {
        result=&mut task=>{
            let (worker,result)=result.unwrap();
            panic!("Preparation dropped an active private reply: result={result:?}, serviceActive={}, callbacks={}",worker.har_servicing,worker.interactions.routes.len());
        },
        ()=tokio::time::sleep(Duration::from_millis(25))=>{},
    }
    reply(&mut peer, &continued, json!({})).await;
    let (worker, result) = task.await.unwrap();
    assert_eq!(result.unwrap(), "Acme prepared");
    assert!(!worker.har_servicing);
    assert!(worker.interactions.routes.is_empty());
    assert_eq!(
        worker.context_layout[&context].http_credentials,
        Some(credentials())
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.next())
            .await
            .is_err(),
        "The original private reply must not be replayed"
    );
}

#[tokio::test]
async fn native_credentials_http_preparation_cancellation_settles_the_started_private_reply() {
    let (mut worker, mut peer, _root, context, _page) = automatic_context().await;
    let id = Uuid::new_v4();
    worker
        .shared
        .state
        .lock()
        .unwrap()
        .begin(id, "http-preparation".into())
        .unwrap();
    let shared = worker.shared.clone();
    worker
        .apply(event(
            "Fetch.authRequired",
            "cancel-prepare",
            "https://example.com/automatic",
        ))
        .unwrap();
    let (ready, prepared) = tokio::sync::oneshot::channel::<()>();
    let mut task = tokio::spawn(async move {
        let result = worker
            .await_http_preparation(
                id,
                async {
                    prepared.await.unwrap();
                    Ok::<_, Error>(())
                },
                deadline(),
            )
            .await;
        (worker, result)
    });
    let continued = command(&mut peer, "Fetch.continueWithAuth").await;
    shared
        .state
        .lock()
        .unwrap()
        .operations
        .get_mut(&id)
        .unwrap()
        .cancel_requested = true;
    ready.send(()).unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(25), &mut task)
            .await
            .is_err()
    );
    reply(&mut peer, &continued, json!({})).await;
    let (worker, result) = task.await.unwrap();
    assert_eq!(result.unwrap_err().code, "CANCELLED");
    assert!(!worker.har_servicing);
    assert!(worker.interactions.routes.is_empty());
    assert_eq!(
        worker.context_layout[&context].http_credentials,
        Some(credentials())
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.next())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn native_credentials_http_preparation_ready_or_already_cancelled_does_not_start_housekeeping()
 {
    for cancelled in [false, true] {
        let (mut worker, mut peer, _root, _context, _page) = automatic_context().await;
        let id = Uuid::new_v4();
        {
            let mut state = worker.shared.state.lock().unwrap();
            state.begin(id, "http-preparation".into()).unwrap();
            state.operations.get_mut(&id).unwrap().cancel_requested = cancelled;
        }
        worker
            .apply(event(
                "Fetch.authRequired",
                "ready-prepare",
                "https://example.com/automatic",
            ))
            .unwrap();
        let polled = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let observed = polled.clone();
        let result = worker
            .await_http_preparation(
                id,
                async move {
                    observed.store(true, Ordering::SeqCst);
                    Ok::<_, Error>("Acme ready")
                },
                deadline(),
            )
            .await;
        if cancelled {
            assert_eq!(result.unwrap_err().code, "CANCELLED");
        } else {
            assert_eq!(result.unwrap(), "Acme ready");
        }
        assert_eq!(polled.load(Ordering::SeqCst), !cancelled);
        assert!(!worker.har_servicing);
        assert_eq!(worker.interactions.routes.len(), 1);
        assert!(
            tokio::time::timeout(Duration::from_millis(25), peer.next())
                .await
                .is_err()
        );
    }
}

#[tokio::test]
async fn native_credentials_http_preparation_expiry_settles_the_reply_before_refusing_admission() {
    let (mut worker, mut peer, _root, context, _page) = automatic_context().await;
    worker
        .apply(event(
            "Fetch.authRequired",
            "expiry-prepare",
            "https://example.com/automatic",
        ))
        .unwrap();
    let until = Instant::now() + Duration::from_millis(100);
    let (ready, prepared) = tokio::sync::oneshot::channel::<()>();
    let mut task = tokio::spawn(async move {
        let result = worker
            .await_http_preparation(
                Uuid::new_v4(),
                async {
                    prepared.await.unwrap();
                    Ok::<_, Error>(())
                },
                until,
            )
            .await;
        (worker, result)
    });
    let continued = command(&mut peer, "Fetch.continueWithAuth").await;
    ready.send(()).unwrap();
    tokio::time::sleep_until(until + Duration::from_millis(10)).await;
    assert!(
        !task.is_finished(),
        "The private reply still has an owner after HTTP admission expires"
    );
    reply(&mut peer, &continued, json!({})).await;
    let (worker, result) = (&mut task).await.unwrap();
    assert_eq!(result.unwrap_err().code, "HTTP_DEADLINE");
    assert!(!worker.har_servicing);
    assert!(worker.interactions.routes.is_empty());
    assert_eq!(
        worker.context_layout[&context].http_credentials,
        Some(credentials())
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.next())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn native_credentials_setter_settles_an_auth_phase_arriving_during_request_ack() {
    let (mut worker, mut peer, _root, context, _page) = automatic_context().await;
    let mut inherited = crate::browser::http::Options::default();
    worker
        .http_contexts
        .inherit_credentials(&mut inherited, context, Some(&credentials()));
    worker
        .apply(event(
            "Fetch.requestPaused",
            "overlap",
            "https://example.com/automatic",
        ))
        .unwrap();
    let mut replacement = credentials();
    if let crate::browser::http::Credentials::List(values) = &mut replacement {
        values[1].password = "Acme-replacement".into();
    }
    let selected = replacement.clone();
    let task = tokio::spawn(async move {
        let result = worker
            .configure_context_credentials(Uuid::new_v4(), context, Some(selected), deadline())
            .await;
        (worker, result)
    });
    let continued = command(&mut peer, "Fetch.continueRequest").await;
    let EventItem::Event(auth) = event(
        "Fetch.authRequired",
        "overlap",
        "https://example.com/automatic",
    ) else {
        unreachable!()
    };
    send(
        &mut peer,
        json!({"method":auth.method,"sessionId":auth.session_id,"params":auth.params}),
    )
    .await;
    assert_eq!(
        inherited.effective_credentials().unwrap(),
        Some(credentials())
    );
    reply(&mut peer, &continued, json!({})).await;
    let challenged = command(&mut peer, "Fetch.continueWithAuth").await;
    assert_eq!(challenged["params"]["requestId"], "overlap");
    assert_eq!(
        challenged["params"]["authChallengeResponse"]["password"],
        "Acme-private"
    );
    assert_eq!(
        inherited.effective_credentials().unwrap(),
        Some(credentials())
    );
    reply(&mut peer, &challenged, json!({})).await;
    let (worker, result) = task.await.unwrap();
    assert_eq!(result.unwrap()["requested"]["credentialCount"], 2);
    assert_eq!(
        inherited.effective_credentials().unwrap(),
        Some(replacement)
    );
    assert!(worker.interactions.routes.is_empty());
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.next())
            .await
            .is_err()
    );
}

#[test]
fn native_credentials_creation_preserves_user_agent_and_tls_refusals() {
    let options = ContextOptions {
        http_credentials: Some(credentials()),
        ..Default::default()
    };
    assert!(chrome_options(&options).is_ok());
    for value in [
        json!({"userAgent":"Acme"}),
        json!({"ignoreHTTPSErrors":false}),
        json!({"ignoreHTTPSErrors":true}),
    ] {
        let mut wire = serde_json::to_value(&options).unwrap();
        wire.as_object_mut()
            .unwrap()
            .extend(value.as_object().unwrap().clone());
        let options: ContextOptions = serde_json::from_value(wire).unwrap();
        chrome_options(&options).unwrap();
    }
    let mut wire = serde_json::to_value(&options).unwrap();
    wire["permissions"] = json!(["persistent-storage"]);
    let options: ContextOptions = serde_json::from_value(wire).unwrap();
    assert_eq!(
        chrome_options(&options).unwrap_err().code,
        "PERMISSION_UNSUPPORTED"
    );
}

#[tokio::test]
async fn native_credentials_challenges_are_private_single_use_and_retired_by_exact_network_owner() {
    let (mut worker, mut peer, _root, context, _page) = owned_page().await;
    install(&mut worker, context);
    let setup = tokio::spawn(async move {
        worker
            .configure_fetch(Uuid::new_v4(), "session-acme", deadline())
            .await
            .unwrap();
        worker
    });
    let network = command(&mut peer, "Network.enable").await;
    reply(&mut peer, &network, json!({})).await;
    let enable = command(&mut peer, "Fetch.enable").await;
    assert_eq!(
        enable["params"],
        json!({"patterns":[{"urlPattern":"*","requestStage":"Request"}],"handleAuthRequests":true})
    );
    reply(&mut peer, &enable, json!({})).await;
    worker = setup.await.unwrap();
    worker
        .apply(event(
            "Fetch.requestPaused",
            "fetch-acme",
            "https://example.com/private",
        ))
        .unwrap();
    let passing = tokio::spawn(async move {
        worker.poll_har_routes().await;
        worker
    });
    let pass = command(&mut peer, "Fetch.continueRequest").await;
    assert_eq!(pass["params"], json!({"requestId":"fetch-acme"}));
    reply(&mut peer, &pass, json!({})).await;
    worker = passing.await.unwrap();
    for response in ["ProvideCredentials", "CancelAuth"] {
        worker
            .apply(event(
                "Fetch.authRequired",
                "fetch-acme",
                "https://example.com/private",
            ))
            .unwrap();
        assert!(
            worker.interactions.snapshot()["callbacks"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert!(
            !worker
                .interactions
                .snapshot()
                .to_string()
                .contains("Acme-private")
        );
        let answering = tokio::spawn(async move {
            worker.poll_har_routes().await;
            worker
        });
        let call = command(&mut peer, "Fetch.continueWithAuth").await;
        assert_eq!(
            call["params"]["authChallengeResponse"]["response"],
            response
        );
        if response == "ProvideCredentials" {
            assert_eq!(call["params"]["authChallengeResponse"]["username"], "Acme");
            assert_eq!(
                call["params"]["authChallengeResponse"]["password"],
                "Acme-private"
            );
        } else {
            assert!(
                call["params"]["authChallengeResponse"]
                    .get("password")
                    .is_none()
            );
        }
        reply(&mut peer, &call, json!({})).await;
        worker = answering.await.unwrap();
    }
    verify_auth_retirement(&mut worker, context);
}

#[tokio::test]
async fn native_credentials_auth_event_during_request_ack_keeps_independent_phase_ownership() {
    let (mut worker, _peer, _root, context, _page) = owned_page().await;
    install(&mut worker, context);
    worker
        .apply(event(
            "Fetch.requestPaused",
            "same-native",
            "https://example.com",
        ))
        .unwrap();
    worker
        .interactions
        .routes
        .values_mut()
        .next()
        .unwrap()
        .responding = true;
    worker
        .apply(event(
            "Fetch.authRequired",
            "same-native",
            "https://example.com",
        ))
        .unwrap();
    worker
        .apply(event(
            "Fetch.authRequired",
            "same-native",
            "https://example.com",
        ))
        .unwrap();
    assert_eq!(
        worker.interactions.routes.len(),
        2,
        "Only same-phase duplicate events coalesce"
    );
    assert_eq!(
        worker
            .interactions
            .routes
            .values()
            .filter(|route| route.responding)
            .count(),
        1
    );
}

#[tokio::test]
async fn native_credentials_manual_auth_owns_challenges_and_pending_routes_fence_configuration() {
    let (mut worker, mut peer, _root, context, page) = owned_page().await;
    install(&mut worker, context);
    let setup = tokio::spawn(async move {
        worker
            .scoped_route_set(
                Uuid::new_v4(),
                page,
                Kind::Page,
                Some("https://example.com/private".into()),
                true,
                deadline(),
            )
            .await
            .unwrap();
        worker
    });
    let network = command(&mut peer, "Network.enable").await;
    reply(&mut peer, &network, json!({})).await;
    let enable = command(&mut peer, "Fetch.enable").await;
    reply(&mut peer, &enable, json!({})).await;
    worker = setup.await.unwrap();
    worker
        .apply(event(
            "Fetch.authRequired",
            "manual",
            "https://example.com/private",
        ))
        .unwrap();
    let callbacks = worker.interactions.snapshot();
    assert_eq!(callbacks["callbacks"].as_array().unwrap().len(), 1);
    assert_eq!(callbacks["callbacks"][0]["policyScope"], json!(page));
    let error = worker
        .configure_context_credentials(Uuid::new_v4(), context, None, deadline())
        .await
        .unwrap_err();
    assert_eq!(error.code, "ROUTE_CALLBACKS_PENDING");
    assert!(
        worker
            .http_contexts
            .require_context_credentials(context)
            .is_ok()
    );
    assert!(worker.context_layout[&context].http_credentials.is_some());
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.next())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn native_credentials_failed_fetch_configuration_fences_inherited_defaults_without_replay() {
    let (mut worker, mut peer, _root, context, _page) = owned_page().await;
    worker
        .context_layout
        .insert(context, ContextOptions::default());
    let mut inherited = crate::browser::http::Options::default();
    worker
        .http_contexts
        .inherit_credentials(&mut inherited, context, None);
    let configure = tokio::spawn(async move {
        let result = worker
            .configure_context_credentials(Uuid::new_v4(), context, Some(credentials()), deadline())
            .await;
        (worker, result)
    });
    let network = command(&mut peer, "Network.enable").await;
    reply(&mut peer, &network, json!({})).await;
    let enable = command(&mut peer, "Fetch.enable").await;
    send(&mut peer,json!({"id":enable["id"],"sessionId":"session-acme","error":{"code":-32000,"message":"Acme refused"}})).await;
    let (mut worker, result) = configure.await.unwrap();
    assert_eq!(
        result.unwrap_err().partial.unwrap()["configurationComplete"],
        false
    );
    assert_eq!(
        inherited.effective_credentials().unwrap_err().code,
        "HTTP_CONTEXT_CREDENTIALS_UNCERTAIN"
    );
    assert_eq!(
        worker
            .authentication_response(context, "session-acme", "uncertain", "https://example.com")
            .unwrap(),
        json!({"response":"CancelAuth"})
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.next())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn native_credentials_source_gap_cancels_retained_secret_reply_and_requires_connection_replacement()
 {
    let (mut worker, mut peer, _root, context, _page) = owned_page().await;
    install(&mut worker, context);
    worker
        .apply(event(
            "Fetch.authRequired",
            "before-gap",
            "https://example.com",
        ))
        .unwrap();
    worker.apply(EventItem::Gap { dropped: 1 }).unwrap();
    assert_eq!(
        worker
            .configure_context_credentials(Uuid::new_v4(), context, None, deadline())
            .await
            .unwrap_err()
            .code,
        "AUTHENTICATION_OBSERVATION_LOST"
    );
    let answering = tokio::spawn(async move {
        worker.poll_har_routes().await;
        worker
    });
    let call = command(&mut peer, "Fetch.continueWithAuth").await;
    assert_eq!(
        call["params"]["authChallengeResponse"],
        json!({"response":"CancelAuth"})
    );
    reply(&mut peer, &call, json!({})).await;
    let _worker = answering.await.unwrap();
}

#[tokio::test]
async fn native_credentials_reconfiguration_preserves_manual_filters_and_shared_http_defaults() {
    let (mut worker, mut peer, _root, context, page) = owned_page().await;
    worker
        .context_layout
        .insert(context, ContextOptions::default());
    let mut inherited = crate::browser::http::Options::default();
    worker
        .http_contexts
        .inherit_credentials(&mut inherited, context, None);
    let setup = tokio::spawn(async move {
        worker
            .scoped_route_set(
                Uuid::new_v4(),
                context,
                Kind::Context,
                Some("https://example.com/private".into()),
                false,
                deadline(),
            )
            .await
            .unwrap();
        worker
    });
    let manual = command(&mut peer, "Fetch.enable").await;
    assert_eq!(manual["params"]["handleAuthRequests"], false);
    reply(&mut peer, &manual, json!({})).await;
    worker = setup.await.unwrap();
    let setup = tokio::spawn(async move {
        let result = worker
            .configure_context_credentials(Uuid::new_v4(), context, Some(credentials()), deadline())
            .await;
        (worker, result)
    });
    let network = command(&mut peer, "Network.enable").await;
    reply(&mut peer, &network, json!({})).await;
    let enable = command(&mut peer, "Fetch.enable").await;
    assert_eq!(enable["params"]["handleAuthRequests"], true);
    reply(&mut peer, &enable, json!({})).await;
    let (next, result) = setup.await.unwrap();
    worker = next;
    assert_eq!(result.unwrap()["requested"]["credentialCount"], 2);
    assert_eq!(
        inherited.effective_credentials().unwrap(),
        Some(credentials())
    );
    assert_eq!(
        worker.fetch_owner(context, Some(page), "https://example.com/private", false),
        Some(context)
    );
    assert_eq!(
        worker.fetch_owner(context, Some(page), "https://example.com/other", false),
        None
    );
    let mut replacement = credentials();
    if let crate::browser::http::Credentials::List(values) = &mut replacement {
        values[1].password = "replacement-private".into();
    }
    worker
        .configure_context_credentials(
            Uuid::new_v4(),
            context,
            Some(replacement.clone()),
            deadline(),
        )
        .await
        .unwrap();
    assert_eq!(
        inherited.effective_credentials().unwrap(),
        Some(replacement)
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.next())
            .await
            .is_err(),
        "Credential values change privately without replacing an unchanged native interception policy"
    );
    let reset = tokio::spawn(async move {
        let result = worker
            .configure_context_credentials(Uuid::new_v4(), context, None, deadline())
            .await;
        (worker, result)
    });
    let restore = command(&mut peer, "Fetch.enable").await;
    assert_eq!(restore["params"], manual["params"]);
    reply(&mut peer, &restore, json!({})).await;
    let (worker, result) = reset.await.unwrap();
    assert_eq!(result.unwrap()["requested"]["credentialCount"], 0);
    assert_eq!(inherited.effective_credentials().unwrap().unwrap().len(), 0);
    assert_eq!(
        worker.fetch_owner(context, Some(page), "https://example.com/private", false),
        Some(context)
    );
}

#[tokio::test]
async fn native_credentials_unknown_automatic_reply_is_retained_without_secret_replay() {
    let (mut worker, mut peer, _root, context, _page) = owned_page().await;
    install(&mut worker, context);
    worker
        .apply(event(
            "Fetch.authRequired",
            "unknown",
            "https://example.com",
        ))
        .unwrap();
    worker
        .interactions
        .routes
        .values_mut()
        .next()
        .unwrap()
        .expires = Instant::now() + Duration::from_millis(100);
    let answering = tokio::spawn(async move {
        worker.poll_har_routes().await;
        worker
    });
    let call = command(&mut peer, "Fetch.continueWithAuth").await;
    assert_eq!(
        call["params"]["authChallengeResponse"]["response"],
        "ProvideCredentials"
    );
    let mut worker = answering.await.unwrap();
    assert!(
        worker
            .interactions
            .routes
            .values()
            .next()
            .unwrap()
            .responding
    );
    reply(&mut peer, &call, json!({})).await;
    worker.poll_har_routes().await;
    let refused = worker
        .configure_context_credentials(Uuid::new_v4(), context, None, deadline())
        .await
        .unwrap_err();
    assert_eq!(refused.code, "ROUTE_CALLBACKS_PENDING");
    let partial = refused.partial.unwrap();
    assert_eq!(partial["automaticContinuations"], 1);
    assert_eq!(partial["automaticResponding"], 1);
    assert_eq!(partial["automaticFailureCodes"]["OUTCOME_UNKNOWN"], 1);
    assert_eq!(
        worker.context_layout[&context].http_credentials,
        Some(credentials())
    );
    assert!(
        worker
            .http_contexts
            .require_context_credentials(context)
            .is_ok()
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.next())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn native_credentials_tracking_capacity_never_evicts_a_live_attempt_to_allow_replay() {
    let (mut worker, _peer, _root, context, _page) = owned_page().await;
    install(&mut worker, context);
    for index in 0..512 {
        assert_eq!(
            worker
                .authentication_response(
                    context,
                    "session-acme",
                    &format!("request-{index}"),
                    "https://example.com"
                )
                .unwrap()["response"],
            "ProvideCredentials"
        );
    }
    assert_eq!(
        worker
            .authentication_response(context, "session-acme", "overflow", "https://example.com")
            .unwrap_err()
            .code,
        "AUTHENTICATION_REQUEST_CAPACITY"
    );
    assert_eq!(
        worker
            .authentication_response(context, "session-acme", "request-0", "https://example.com")
            .unwrap()["response"],
        "CancelAuth"
    );
}

fn verify_auth_retirement(worker: &mut Worker, context: Reference) {
    let finished = |session: &str| Event {
        method: "Network.loadingFinished".into(),
        session_id: Some(session.into()),
        params: json!({"requestId":"network-acme"}),
    };
    worker.authentication_finished(&finished("foreign-session"));
    assert_eq!(worker.fetch.authentication.len(), 1);
    worker.authentication_finished(&finished("session-acme"));
    assert!(worker.fetch.authentication.is_empty());
    assert_eq!(
        worker
            .authentication_response(context, "session-acme", "fetch-acme", "https://example.com")
            .unwrap()["response"],
        "ProvideCredentials"
    );
    assert_eq!(
        worker
            .authentication_response(
                context,
                "session-acme",
                "foreign-origin",
                "https://foreign.invalid"
            )
            .unwrap(),
        json!({"response":"Default"})
    );
}
