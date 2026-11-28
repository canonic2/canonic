//! Cancellation after preparation cannot allocate an SDK context or host client.
use super::*;

#[tokio::test]
async fn client_certificate_paths_sdk_cancelled_preparation_does_not_dispatch_or_allocate() {
    for browser_context in [true, false] {
        let (session, mut peer, root, _context, _page) = harness().await;
        session.0.host.state.lock().unwrap().engine = Engine::Firefox;
        let cert = root.path().join("Acme certificate.pem");
        let key = root.path().join("Acme key.pem");
        std::fs::write(&cert, include_bytes!("../../http/fixtures/server.pem")).unwrap();
        std::fs::write(&key, include_bytes!("../../http/fixtures/server-key.pem")).unwrap();
        let selected = json!([{"origin":"https://example.com","certPath":cert,"keyPath":key}]);
        let request = if browser_context {
            json!({"operation":"context_create","options":{"viewport":{"width":640,"height":480},"deviceScaleFactor":1,"clientCertificates":selected}})
        } else {
            json!({"operation":"http_context_create","options":{"clientCertificates":selected}})
        };
        let readers = crate::browser::http::client_certificates::hold_file_readers().await;
        let operation = session
            .submit(
                Uuid::new_v4(),
                serde_json::from_value(request).unwrap(),
                5000,
            )
            .unwrap();
        tokio::time::timeout(Duration::from_secs(2), async {
            while session.operation(operation).unwrap().outcome != Outcome::Running {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        session.cancel(operation).await.unwrap();
        drop(readers);
        let result = complete(&session, operation).await;
        assert_eq!(result.outcome, Outcome::Cancelled, "{result:?}");
        assert_eq!(result.code.as_deref(), Some("CANCELLED"));
        assert!(result.cancel_requested);
        assert!(
            tokio::time::timeout(Duration::from_millis(25), peer.request())
                .await
                .is_err(),
            "Cancelled preparation must not dispatch SDK allocation"
        );
        assert!(
            session.0.host.http_contexts.lock().unwrap().list()["contexts"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert!(
            !session
                .0
                .host
                .state
                .lock()
                .unwrap()
                .context_options
                .values()
                .any(|options| options.client_certificates.is_some())
        );
        close(&session).await;
    }
}
