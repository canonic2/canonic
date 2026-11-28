//! Native browser headers have exact attachment ownership and a retained uncertainty fence.
use super::investigation::owned_page;
use super::*;
use crate::http_fixtures::defaults as wire;

#[tokio::test]
async fn page_header_defaults_native_preserves_page_precedence_after_context_replacement_and_reset()
{
    let (mut worker, mut peer, _root, context, page) = owned_page().await;
    worker
        .context_layout
        .insert(context, ContextOptions::default());
    let cases = [
        (
            json!({"operation":"context_configure","context":context,"setting":{"kind":"extra_http_headers","headers":[{"name":"X-Acme","value":"context"},{"name":"X-Context","value":"one"}]}}),
            json!({"x-acme":"context","x-context":"one"}),
        ),
        (
            json!({"operation":"page_set_extra_http_headers","page":page,"headers":[{"name":"x-ACME","value":"page"}]}),
            json!({"x-acme":"page","x-context":"one"}),
        ),
        (
            json!({"operation":"context_configure","context":context,"setting":{"kind":"extra_http_headers","headers":[{"name":"X-Acme","value":"replacement"},{"name":"X-Context","value":"two"}]}}),
            json!({"x-acme":"page","x-context":"two"}),
        ),
        (
            json!({"operation":"page_set_extra_http_headers","page":page,"headers":[]}),
            json!({"x-acme":"replacement","x-context":"two"}),
        ),
    ];
    for (request, expected) in cases {
        let task = tokio::spawn(async move {
            let value = worker
                .execute_one(
                    Uuid::new_v4(),
                    serde_json::from_value(request).unwrap(),
                    deadline(),
                )
                .await;
            (worker, value)
        });
        let enable = command(&mut peer, "Network.enable").await;
        assert_eq!(enable["sessionId"], "session-acme");
        reply(&mut peer, &enable, json!({})).await;
        let setter = command(&mut peer, "Network.setExtraHTTPHeaders").await;
        assert_eq!(setter["params"]["headers"], expected);
        reply(&mut peer, &setter, json!({})).await;
        let (next, result) = task.await.unwrap();
        worker = next;
        let receipt = result.unwrap();
        assert_eq!(receipt["configuredAttachments"], 1);
    }
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.next())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn page_header_defaults_native_scope_and_failed_dispatch_do_not_replay_or_change_http_defaults()
 {
    let (mut worker, mut peer, _root, context, page) = owned_page().await;
    let initial = vec![crate::browser::http::Header {
        name: "X-Acme".into(),
        value: "context".into(),
    }];
    worker.context_layout.insert(
        context,
        ContextOptions {
            extra_http_headers: Some(initial.clone()),
            ..Default::default()
        },
    );
    let mut options = crate::browser::http::Options::default();
    worker
        .http_contexts
        .inherit_headers(&mut options, context, &initial);
    let shared = options.inherited_headers.unwrap();
    assert_eq!(
        worker
            .execute_one(
                Uuid::new_v4(),
                Request::PageSetExtraHttpHeaders {
                    page,
                    headers: vec![]
                },
                Instant::now()
            )
            .await
            .unwrap_err()
            .code,
        "NOT_DISPATCHED"
    );
    assert!(worker.page_headers.is_empty());
    assert_eq!(
        worker
            .execute_one(
                Uuid::new_v4(),
                Request::PageSetExtraHttpHeaders {
                    page: context,
                    headers: vec![]
                },
                deadline()
            )
            .await
            .unwrap_err()
            .code,
        "INVALID_SCOPE"
    );
    let task = tokio::spawn(async move {
        let result = worker
            .execute_one(
                Uuid::new_v4(),
                Request::PageSetExtraHttpHeaders {
                    page,
                    headers: vec![],
                },
                deadline(),
            )
            .await;
        (worker, result)
    });
    let enable = command(&mut peer, "Network.enable").await;
    reply(&mut peer, &enable, json!({})).await;
    let setter = command(&mut peer, "Network.setExtraHTTPHeaders").await;
    send(&mut peer,json!({"id":setter["id"],"sessionId":"session-acme","error":{"code":-32000,"message":"Acme failed setter"}})).await;
    let (mut worker, result) = task.await.unwrap();
    assert_eq!(
        result.unwrap_err().partial.unwrap()["configurationComplete"],
        false
    );
    assert_eq!(*shared.lock().unwrap(), Some(initial));
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.next())
            .await
            .is_err()
    );
    worker
        .apply(EventItem::Event(super::super::super::Event {
            method: "Target.targetDestroyed".into(),
            params: json!({"targetId":"page-acme"}),
            session_id: None,
        }))
        .unwrap();
    // A terminal target event must discard its page-specific future setting.
    assert!(worker.page_headers.is_empty());
}

#[tokio::test]
async fn http_header_defaults_native_configuration_normalizes_names_and_updates_associated_clients()
{
    let site = wire::Site::start(None).await;
    let (mut worker, mut peer, _root, context, _page) = owned_page().await;
    worker.context_layout.insert(
        context,
        ContextOptions {
            base_url: Some(site.url.clone()),
            ..Default::default()
        },
    );
    let create: Request = serde_json::from_value(json!({"operation":"http_context_create","options":{"browserContext":context,"userAgent":"Acme"}})).unwrap();
    let client = worker
        .http_operation(Uuid::new_v4(), create, deadline())
        .await
        .unwrap()["httpContext"]
        .clone();
    let configure: Request = serde_json::from_value(json!({"operation":"context_configure","context":context,"setting":{"kind":"extra_http_headers","headers":[{"name":"X-Acme","value":"first"},{"name":"x-aCmE","value":"last"}]}})).unwrap();
    let task = tokio::spawn(async move {
        let response = worker
            .execute_one(Uuid::new_v4(), configure, deadline())
            .await;
        (worker, response)
    });
    let enable = command(&mut peer, "Network.enable").await;
    reply(&mut peer, &enable, json!({})).await;
    let call = command(&mut peer, "Network.setExtraHTTPHeaders").await;
    assert_eq!(call["sessionId"], "session-acme");
    assert_eq!(call["params"]["headers"], json!({"x-acme":"last"}));
    reply(&mut peer, &call, json!({})).await;
    let (mut worker, response) = task.await.unwrap();
    assert_eq!(
        response.unwrap()["configuredAttachments"],
        1,
        "The main frame must not cause a second native call"
    );
    let request: Request = serde_json::from_value(
        json!({"operation":"http_fetch","httpContext":client,"request":{"url":"/echo"}}),
    )
    .unwrap();
    let task = tokio::spawn(async move {
        let response = worker
            .http_operation(Uuid::new_v4(), request, deadline())
            .await;
        (worker, response)
    });
    let cookies = command(&mut peer, "Storage.getCookies").await;
    reply(&mut peer, &cookies, json!({"cookies":[]})).await;
    let (mut worker, response) = task.await.unwrap();
    assert_eq!(site.records().last().unwrap()["headers"]["x-acme"], "last");
    worker.http_operation(Uuid::new_v4(), serde_json::from_value(json!({"operation":"http_response_remove","requestId":response.unwrap()["requestId"]})).unwrap(), deadline()).await.unwrap();
    let configure: Request = serde_json::from_value(json!({"operation":"context_configure","context":context,"setting":{"kind":"extra_http_headers","headers":[]}})).unwrap();
    let task = tokio::spawn(async move {
        let response = worker
            .execute_one(Uuid::new_v4(), configure, deadline())
            .await;
        (worker, response)
    });
    let enable = command(&mut peer, "Network.enable").await;
    reply(&mut peer, &enable, json!({})).await;
    let call = command(&mut peer, "Network.setExtraHTTPHeaders").await;
    assert_eq!(call["params"]["headers"], json!({}));
    send(&mut peer, json!({"id":call["id"],"sessionId":"session-acme","error":{"code":-32000,"message":"Acme uncertain native update"}})).await;
    let restore = command(&mut peer, "Network.setExtraHTTPHeaders").await;
    assert_eq!(
        restore["params"]["headers"],
        json!({"x-acme":"last"}),
        "The previous list is reapplied like the pinned SDK rollback"
    );
    send(&mut peer, json!({"id":restore["id"],"sessionId":"session-acme","error":{"code":-32000,"message":"Acme failed restoration"}})).await;
    let (mut worker, response) = task.await.unwrap();
    let partial = response.unwrap_err().partial.unwrap();
    assert_eq!(partial["configurationComplete"], false);
    assert_eq!(partial["restoredAttachments"], 0);
    assert_eq!(partial["stateMayHaveChanged"], true);
    let request: Request = serde_json::from_value(
        json!({"operation":"http_fetch","httpContext":client,"request":{"url":"/echo"}}),
    )
    .unwrap();
    assert_eq!(
        worker
            .http_operation(Uuid::new_v4(), request, deadline())
            .await
            .unwrap_err()
            .code,
        "HTTP_CONTEXT_HEADERS_UNCERTAIN"
    );
    assert_eq!(site.records().len(), 1);
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.next())
            .await
            .is_err()
    );
    verify_headers_recover(worker, peer, context, client, &site).await;
}

async fn verify_headers_recover(
    mut worker: Worker,
    mut peer: Peer,
    context: Reference,
    client: Value,
    site: &wire::Site,
) {
    let configure: Request = serde_json::from_value(json!({"operation":"context_configure","context":context,"setting":{"kind":"extra_http_headers","headers":[{"name":"X-Acme","value":"rejected"}]}})).unwrap();
    let task = tokio::spawn(async move {
        let response = worker
            .execute_one(Uuid::new_v4(), configure, deadline())
            .await;
        (worker, response)
    });
    let enable = command(&mut peer, "Network.enable").await;
    reply(&mut peer, &enable, json!({})).await;
    let call = command(&mut peer, "Network.setExtraHTTPHeaders").await;
    send(&mut peer, json!({"id":call["id"],"sessionId":"session-acme","error":{"code":-32000,"message":"Acme rejected native update"}})).await;
    let restore = command(&mut peer, "Network.setExtraHTTPHeaders").await;
    assert_eq!(restore["params"]["headers"], json!({"x-acme":"last"}));
    reply(&mut peer, &restore, json!({})).await;
    let (mut worker, response) = task.await.unwrap();
    let partial = response.unwrap_err().partial.unwrap();
    assert_eq!(partial["restoredAttachments"], 1);
    assert_eq!(partial["stateMayHaveChanged"], false);
    assert_eq!(partial["httpDefaults"], "previous-headers");
    let request: Request = serde_json::from_value(
        json!({"operation":"http_fetch","httpContext":client,"request":{"url":"/echo"}}),
    )
    .unwrap();
    let task = tokio::spawn(async move {
        let response = worker
            .http_operation(Uuid::new_v4(), request, deadline())
            .await;
        (worker, response)
    });
    let cookies = command(&mut peer, "Storage.getCookies").await;
    reply(&mut peer, &cookies, json!({"cookies":[]})).await;
    let (mut worker, response) = task.await.unwrap();
    assert_eq!(site.records().last().unwrap()["headers"]["x-acme"], "last");
    worker.http_operation(Uuid::new_v4(), serde_json::from_value(json!({"operation":"http_response_remove","requestId":response.unwrap()["requestId"]})).unwrap(), deadline()).await.unwrap();
    worker.http_contexts.close().unwrap();
}
