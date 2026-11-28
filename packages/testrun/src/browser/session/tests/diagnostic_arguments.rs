//! Real driver pipes keep private argument handles owned through racing reads and disposal.
use super::*;

async fn success(session: &Session, request: Request) -> Value {
    let id = session.submit(Uuid::new_v4(), request, 5000).unwrap();
    let result = complete(session, id).await;
    assert_eq!(result.outcome, Outcome::Succeeded, "{result:?}");
    result.result.unwrap()
}
async fn console(peer: &mut Fixture, guid: &str) {
    let mut handle = created(guid, "JSHandle", json!({"preview":"Acme private object"}));
    handle["guid"] = json!("page");
    peer.send(&handle).await;
    peer.send(&json!({"guid":"context","method":"console","params":{"type":"log","text":"Acme private console","args":[{"guid":guid}],"location":{"url":"https://example.com/acme","lineNumber":0,"columnNumber":0},"timestamp":0,"page":{"guid":"page"}}})).await;
}
fn group(value: &Value) -> Uuid {
    serde_json::from_value(
        value["events"].as_array().unwrap().last().unwrap()["data"]["arguments"]["groupId"].clone(),
    )
    .unwrap()
}

#[tokio::test]
#[expect(
    clippy::too_many_lines,
    reason = "One retained driver group verifies an in-flight getter, removal and stale-result refusal without replay"
)]
async fn diagnostic_arguments_driver_removal_discards_an_inflight_getter_result_without_replay() {
    let (session, mut peer, _directory, context, _page) = harness().await;
    diagnostic_subscription(&session, &mut peer, context, true).await;
    success(
        &session,
        Request::DiagnosticsArgumentsSet {
            context,
            enabled: true,
        },
    )
    .await;
    console(&mut peer, "Acme-private-argument-wire").await;
    let metadata = success(
        &session,
        Request::DiagnosticsRead {
            context,
            query: Default::default(),
        },
    )
    .await;
    assert!(!metadata.to_string().contains("Acme-private-argument-wire"));
    assert!(!metadata.to_string().contains("Acme private console"));
    let first = group(&metadata);
    let query = crate::browser::diagnostic_arguments::Read {
        path: vec!["nested".into()],
        ..Default::default()
    };
    let reading = session
        .submit(
            Uuid::new_v4(),
            Request::DiagnosticsArgumentsRead {
                context,
                group_id: first,
                query,
            },
            5000,
        )
        .unwrap();
    let call = peer.request().await;
    assert_eq!(call["method"], "evaluateExpression");
    assert_eq!(call["guid"], "Acme-private-argument-wire");
    let removing = session
        .submit(
            Uuid::new_v4(),
            Request::DiagnosticsArgumentsRemove {
                context,
                group_id: first,
            },
            5000,
        )
        .unwrap();
    let dispose = peer.request().await;
    assert_eq!(dispose["method"], "dispose");
    assert_eq!(dispose["guid"], "Acme-private-argument-wire");
    peer.send(&json!({"id":dispose["id"],"result":{}})).await;
    assert_eq!(
        complete(&session, removing).await.outcome,
        Outcome::Succeeded
    );
    peer.send(&json!({"id":call["id"],"result":{"value":{"o":[{"k":"n","v":{"n":17}}],"id":1}}}))
        .await;
    let result = complete(&session, reading).await;
    assert_eq!(
        result.code.as_deref(),
        Some("DIAGNOSTIC_ARGUMENTS_EXPIRED"),
        "{result:?}"
    );
    let partial = result.result.unwrap();
    assert_eq!(partial["dispatchCompleted"], true);
    assert_eq!(partial["potentiallyMutating"], true);
    assert!(partial.get("value").is_none());
    console(&mut peer, "second-private-wire").await;
    let metadata = success(
        &session,
        Request::DiagnosticsRead {
            context,
            query: Default::default(),
        },
    )
    .await;
    let second = group(&metadata);
    assert_ne!(first, second);
    let reading = session
        .submit(
            Uuid::new_v4(),
            Request::DiagnosticsArgumentsRead {
                context,
                group_id: second,
                query: Default::default(),
            },
            5000,
        )
        .unwrap();
    let call = peer.request().await;
    assert_eq!(call["guid"], "second-private-wire");
    peer.send(&json!({"id":call["id"],"result":{"value":{"o":[{"k":"n","v":{"n":17}}],"id":1}}}))
        .await;
    assert_eq!(
        complete(&session, reading).await.result.unwrap()["value"]["value"],
        17
    );
    let disabling = session
        .submit(
            Uuid::new_v4(),
            Request::DiagnosticsArgumentsSet {
                context,
                enabled: false,
            },
            5000,
        )
        .unwrap();
    let dispose = peer.request().await;
    assert_eq!(dispose["method"], "dispose");
    assert_eq!(dispose["guid"], "second-private-wire");
    peer.send(&json!({"id":dispose["id"],"result":{}})).await;
    let disabled = complete(&session, disabling).await.result.unwrap();
    assert_eq!(disabled["enabled"], false);
    assert!(disabled["groups"].as_array().unwrap().is_empty());
    close(&session).await;
}
