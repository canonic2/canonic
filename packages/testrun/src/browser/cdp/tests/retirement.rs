//! Closed scope ownership never weakens wire identity or live command budgets.
use super::*;

async fn held(
    connection: &Connection,
    peer: &mut Peer,
    session: &str,
) -> (tokio::task::JoinHandle<Result<Value, Error>>, Value) {
    let owner = connection.clone();
    let session = session.to_owned();
    let call = tokio::spawn(async move {
        owner.call(Some(&session), "Runtime.callFunctionOn", json!({"functionDeclaration":"function(){return new Promise(()=>{})}","uniqueContextId":"Acme-world","awaitPromise":true,"returnByValue":true}), Lane::Ordinary, deadline()).await
    });
    let request = command(peer).await;
    assert_eq!(request["method"], "Runtime.callFunctionOn");
    (call, request)
}

async fn retired(connection: &Connection, peer: &mut Peer) -> Value {
    let (call, request) = held(connection, peer, "acme-closing").await;
    connection
        .reserve_retirement(["acme-closing".into()])
        .unwrap()
        .commit_after_close();
    assert!(matches!(
        call.await.unwrap(),
        Err(Error::ScopeClosed {
            dispatch: Dispatch::PossiblyDispatched
        })
    ));
    assert_eq!(connection.status().pending, 0);
    assert_eq!(connection.status().retired_calls, 1);
    request
}

async fn wait_retired(connection: &Connection, count: usize) {
    timeout(Duration::from_secs(2), async {
        while connection.status().retired_calls != count {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}

fn late(request: &Value) -> Value {
    json!({"id":request["id"],"sessionId":request["sessionId"],"result":{"result":{"type":"undefined"}}})
}

#[tokio::test]
async fn native_closed_call_retirement_returns_active_capacity_without_reply_or_replay() {
    let (connection, _events, mut peer) = fixture().await;
    let mut calls = Vec::new();
    let mut requests = Vec::new();
    for _ in 0..MAX_PENDING - CONTROL_RESERVE {
        let (call, request) = held(&connection, &mut peer, "acme-closing").await;
        calls.push(call);
        requests.push(request);
    }
    assert!(matches!(
        connection
            .call(
                None,
                "Runtime.getHeapUsage",
                json!({}),
                Lane::Ordinary,
                deadline()
            )
            .await,
        Err(Error::Busy)
    ));
    connection
        .reserve_retirement(["acme-closing".into()])
        .unwrap()
        .commit_after_close();
    for call in calls {
        assert!(matches!(
            call.await.unwrap(),
            Err(Error::ScopeClosed {
                dispatch: Dispatch::PossiblyDispatched
            })
        ));
    }
    assert_eq!(connection.status().pending, 0);
    assert_eq!(
        connection.status().retired_calls,
        MAX_PENDING - CONTROL_RESERVE
    );
    let copy = connection.clone();
    let probe = tokio::spawn(async move {
        copy.call(
            None,
            "Runtime.getHeapUsage",
            json!({}),
            Lane::Ordinary,
            deadline(),
        )
        .await
    });
    let request = command(&mut peer).await;
    assert_eq!(request["method"], "Runtime.getHeapUsage");
    send(&mut peer,json!({"id":request["id"],"result":{"usedSize":1,"totalSize":2,"embedderHeapUsedSize":0,"backingStorageSize":0}})).await;
    assert!(probe.await.unwrap().is_ok());
    for request in requests {
        send(&mut peer, late(&request)).await;
    }
    wait_retired(&connection, 0).await;
    assert_eq!(connection.status().retained_replies, 0);
    assert!(connection.status().open);
    assert!(
        timeout(Duration::from_millis(25), peer.next())
            .await
            .is_err(),
        "Closed calls cannot replay after their late replies"
    );
    connection.close(deadline()).await.unwrap();
}

#[tokio::test]
async fn native_closed_call_unacknowledged_retirement_preserves_live_ownership() {
    let (connection, _events, mut peer) = fixture().await;
    let (call, request) = held(&connection, &mut peer, "acme-closing").await;
    let reservation = connection
        .reserve_retirement(["acme-closing".into()])
        .unwrap();
    assert!(matches!(
        connection
            .call(
                Some("acme-closing"),
                "Runtime.getHeapUsage",
                json!({}),
                Lane::Ordinary,
                deadline()
            )
            .await,
        Err(Error::ScopeClosing)
    ));
    assert!(!call.is_finished());
    assert_eq!(connection.status().pending, 1);
    drop(reservation);
    assert_eq!(connection.status().retired_calls, 0);
    send(&mut peer, late(&request)).await;
    assert!(call.await.unwrap().is_ok());
    assert!(
        timeout(Duration::from_millis(25), peer.next())
            .await
            .is_err(),
        "An unacknowledged close sends no replacement command"
    );
    connection.close(deadline()).await.unwrap();
}

#[tokio::test]
async fn native_closed_call_queued_retirement_never_dispatches_the_old_command() {
    let (connection, _events, mut peer) = fixture().await;
    let call = connection.call(
        Some("acme-closing"),
        "Runtime.getHeapUsage",
        json!({}),
        Lane::Ordinary,
        deadline(),
    );
    tokio::pin!(call);
    assert!(futures_util::poll!(call.as_mut()).is_pending());
    connection
        .reserve_retirement(["acme-closing".into()])
        .unwrap()
        .commit_after_close();
    assert!(matches!(
        call.await,
        Err(Error::ScopeClosed {
            dispatch: Dispatch::NotDispatched
        })
    ));
    assert_eq!(connection.status().retired_calls, 0);
    assert!(
        timeout(Duration::from_millis(25), peer.next())
            .await
            .is_err(),
        "The unsent closed-scope command cannot reach the wire"
    );
    connection.close(deadline()).await.unwrap();
}

#[tokio::test]
async fn native_closed_call_already_replied_retirement_cannot_deliver_closed_allocations() {
    let (connection, _events, mut peer) = fixture().await;
    let call=connection.call(Some("acme-closing"),"Runtime.callFunctionOn",json!({"functionDeclaration":"function(){return this}","uniqueContextId":"Acme-world","returnByValue":false}),Lane::Ordinary,deadline());
    tokio::pin!(call);
    assert!(futures_util::poll!(call.as_mut()).is_pending());
    let request = command(&mut peer).await;
    send(&mut peer,json!({"id":request["id"],"sessionId":"acme-closing","result":{"result":{"type":"object","objectId":"Acme-closed-allocation"}}})).await;
    timeout(Duration::from_secs(2), async {
        while connection.status().retained_replies != 1 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    connection
        .reserve_retirement(["acme-closing".into()])
        .unwrap()
        .commit_after_close();
    assert_eq!(connection.status().retained_replies, 0);
    assert!(matches!(
        call.await,
        Err(Error::ScopeClosed {
            dispatch: Dispatch::PossiblyDispatched
        })
    ));
    assert_eq!(connection.status().retired_calls, 0);
    assert!(connection.take_late_reply().is_none());
    connection.close(deadline()).await.unwrap();
}

#[tokio::test]
async fn native_closed_call_late_replies_keep_exact_session_schema_and_single_delivery() {
    for scenario in [
        "wrong-session",
        "sessionless-success",
        "invalid-result",
        "unknown-id",
        "duplicate",
    ] {
        let (connection, _events, mut peer) = fixture().await;
        let request = retired(&connection, &mut peer).await;
        let mut reply = late(&request);
        match scenario {
            "wrong-session" => reply["sessionId"] = json!("acme-sibling"),
            "sessionless-success" => {
                reply.as_object_mut().unwrap().remove("sessionId");
            }
            "invalid-result" => reply["result"] = json!({}),
            "unknown-id" => reply["id"] = json!(request["id"].as_u64().unwrap() + 1),
            "duplicate" => {
                send(&mut peer, reply.clone()).await;
                wait_retired(&connection, 0).await;
            }
            _ => unreachable!(),
        }
        send(&mut peer, reply).await;
        timeout(Duration::from_secs(2), async {
            while connection.status().loss.is_none() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert_eq!(connection.status().loss, Some(Loss::Protocol), "{scenario}");
        assert_eq!(connection.status().retired_calls, 0);
        connection.close(deadline()).await.unwrap();
    }
    let (connection, _events, mut peer) = fixture().await;
    let request = retired(&connection, &mut peer).await;
    send(
        &mut peer,
        json!({"id":request["id"],"error":{"code":-32000,"message":"Acme target closed"}}),
    )
    .await;
    wait_retired(&connection, 0).await;
    assert!(connection.status().open);
    connection.close(deadline()).await.unwrap();
}

#[tokio::test]
async fn native_closed_call_retirement_budget_preserves_live_and_cleanup_capacity() {
    let (connection, _events, mut peer) = fixture().await;
    for _ in 0..crate::browser::cdp::MAX_RETIRED_CALLS {
        let (call, _request) = held(&connection, &mut peer, "acme-closing").await;
        connection
            .reserve_retirement(["acme-closing".into()])
            .unwrap()
            .commit_after_close();
        assert!(matches!(
            call.await.unwrap(),
            Err(Error::ScopeClosed { .. })
        ));
    }
    let (call, request) = held(&connection, &mut peer, "acme-live").await;
    assert!(matches!(
        connection.reserve_retirement(["acme-live".into()]),
        Err(Error::RetirementCapacity)
    ));
    assert!(!call.is_finished());
    assert_eq!(connection.status().pending, 1);
    let copy = connection.clone();
    let close = tokio::spawn(async move {
        copy.call(None, "Browser.close", json!({}), Lane::Control, deadline())
            .await
    });
    let closing = command(&mut peer).await;
    assert_eq!(closing["method"], "Browser.close");
    send(&mut peer, json!({"id":closing["id"],"result":{}})).await;
    close.await.unwrap().unwrap();
    send(&mut peer, late(&request)).await;
    call.await.unwrap().unwrap();
    connection.close(deadline()).await.unwrap();
    assert_eq!(connection.status().retired_calls, 0);
}
