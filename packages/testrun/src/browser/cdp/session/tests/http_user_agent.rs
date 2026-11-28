//! Native request identity borrows verified bootstrap metadata without a probe.
use super::investigation::owned_page;
use super::*;
use crate::http_fixtures::defaults as wire;

#[tokio::test]
async fn http_user_agent_native_defaults_and_overrides_use_bootstrap_identity_without_page_calls() {
    let site = wire::Site::start(None).await;
    let (mut worker, mut peer, _root, context, _page) = owned_page().await;
    worker
        .context_layout
        .insert(context, ContextOptions::default());
    worker.shared.state.lock().unwrap().identity =
        Some(json!({"browser":{"userAgent":"Acme engine agent"}}));
    for (agent, expected) in [
        (None, "Acme engine agent"),
        (Some(""), "Acme engine agent"),
        (Some("Acme HTTP agent"), "Acme HTTP agent"),
    ] {
        let created=worker.http_operation(Uuid::new_v4(),serde_json::from_value(json!({"operation":"http_context_create","options":{"browserContext":context,"baseUrl":site.url,"userAgent":agent}})).unwrap(),deadline()).await.unwrap();
        assert!(
            tokio::time::timeout(Duration::from_millis(25), peer.next())
                .await
                .is_err(),
            "Native bootstrap identity needs no additional CDP call"
        );
        let request=serde_json::from_value(json!({"operation":"http_fetch","httpContext":created["httpContext"],"request":{"url":"/echo"}})).unwrap();
        let task = tokio::spawn(async move {
            let result = worker
                .http_operation(Uuid::new_v4(), request, deadline())
                .await;
            (worker, result)
        });
        let cookies = command(&mut peer, "Storage.getCookies").await;
        reply(&mut peer, &cookies, json!({"cookies":[]})).await;
        let (returned, result) = task.await.unwrap();
        worker = returned;
        let result = result.unwrap();
        assert_eq!(
            site.records().last().unwrap()["headers"]["user-agent"],
            expected
        );
        worker
            .http_operation(
                Uuid::new_v4(),
                serde_json::from_value(
                    json!({"operation":"http_response_remove","requestId":result["requestId"]}),
                )
                .unwrap(),
                deadline(),
            )
            .await
            .unwrap();
        worker
            .http_operation(
                Uuid::new_v4(),
                serde_json::from_value(
                    json!({"operation":"http_context_remove","httpContext":created["httpContext"]}),
                )
                .unwrap(),
                deadline(),
            )
            .await
            .unwrap();
    }
}
