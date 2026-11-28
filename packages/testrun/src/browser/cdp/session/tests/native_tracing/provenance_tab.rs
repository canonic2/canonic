//! Retired metadata attachments preserve loss ownership without inventing native flags.
use super::*;

#[tokio::test]
async fn native_trace_provenance_metadata_tab_loss_is_distinct_from_completion_loss() {
    for detached in [false, true] {
        let (mut worker, mut peer, _root, context, page) = owned_page().await;
        let native = worker.index.native(page, Kind::Page).unwrap();
        worker.apply(EventItem::Event(Event{method:"Target.targetInfoChanged".into(),session_id:None,params:json!({"targetInfo":{"targetId":native,"parentId":"acme-tab","browserContextId":"context-acme","type":"page","title":"Acme","url":"https://example.com/","attached":true,"canAccessOpener":false}})})).unwrap();
        let starting = tokio::spawn(async move {
            worker
                .trace_start(Uuid::new_v4(), context, Options::default(), deadline())
                .await
                .unwrap();
            worker
        });
        let lookup = command(&mut peer, "Target.getTargetInfo").await;
        let info = json!({"targetId":"acme-tab","browserContextId":"context-acme","type":"tab","title":"Acme","url":"https://example.com/","attached":false,"canAccessOpener":false});
        reply(&mut peer, &lookup, json!({"targetInfo":info})).await;
        let attach = command(&mut peer, "Target.attachToTarget").await;
        reply(&mut peer, &attach, json!({"sessionId":"tab-session-acme"})).await;
        let start = command(&mut peer, "Tracing.start").await;
        assert_eq!(start["sessionId"], "tab-session-acme");
        reply(&mut peer, &start, json!({})).await;
        let mut worker = starting.await.unwrap();
        worker.trace_tab_event(&Event {
            method: "Target.targetDestroyed".into(),
            session_id: None,
            params: json!({"targetId":"Acme-unrelated-tab"}),
        });
        assert_eq!(worker.trace_status()["sourceLossReasons"], json!([]));
        worker.trace_tab_event(&Event {
            method: if detached {
                "Target.detachedFromTarget"
            } else {
                "Target.targetDestroyed"
            }
            .into(),
            session_id: None,
            params: if detached {
                json!({"sessionId":"tab-session-acme"})
            } else {
                json!({"targetId":"acme-tab"})
            },
        });
        assert_eq!(
            worker.trace_status()["sourceLossReasons"],
            json!(["metadata-session-lost"])
        );
        assert_eq!(
            worker.trace_status()["nativeCompletion"],
            json!({"received":false,"dataLossOccurred":null})
        );
        worker.seal_trace_after_exit().unwrap();
        let report = worker.trace_status()["report"].clone();
        assert_eq!(report["complete"], false);
        assert_eq!(
            report["sourceLossReasons"],
            json!(["metadata-session-lost", "owned-browser-exited"])
        );
        assert_eq!(
            report["nativeCompletion"],
            json!({"received":false,"dataLossOccurred":null})
        );
    }
}
