//! Associated state is read once from the exact live context, never a host jar.
use super::*;

fn queued(session: &Session, request: Value) -> Uuid {
    session
        .submit(
            Uuid::new_v4(),
            serde_json::from_value(request).unwrap(),
            5000,
        )
        .unwrap()
}
async fn associated(session: &Session, context: Reference) -> Value {
    session
        .0
        .host
        .state
        .lock()
        .unwrap()
        .context_options
        .insert(context, ContextOptions::default());
    let created=complete(session,queued(session,json!({"operation":"http_context_create","options":{"browserContext":context,"userAgent":"Acme state agent","allowUnknownCookiePartitions":true}}))).await;
    assert_eq!(created.outcome, Outcome::Succeeded, "{created:?}");
    created.result.unwrap()["httpContext"].clone()
}
async fn capture_call(session: &Session, peer: &mut Fixture, id: Uuid) -> Value {
    tokio::select! {
        request=peer.request()=>request,
        result=complete(session,id)=>panic!("Associated export must read its live SDK context before completion: {result:?}"),
    }
}
fn state() -> Value {
    json!({"cookies":[{"name":"Acme","value":"Acme-private-cookie","domain":"example.com","path":"/","expires":-1,"httpOnly":true,"secure":true,"sameSite":"Lax"}],"origins":[{"origin":"https://example.com","localStorage":[{"name":"Acme","value":"Acme-private-local"}],"indexedDB":[{"name":"Acme-private-db","version":1,"stores":[]}],"opfs":[]}]})
}

#[tokio::test]
async fn http_storage_state_associated_sdk_reads_exact_flags_and_preserves_browser_bytes_privately()
{
    for engine in [Engine::Firefox, Engine::Webkit] {
        let (session, mut peer, _root, context, _page) = harness().await;
        session.0.host.state.lock().unwrap().engine = engine;
        let http = associated(&session, context).await;
        for (indexed_db, opfs) in [(false, false), (true, false), (false, true), (true, true)] {
            let id = queued(
                &session,
                json!({"operation":"http_context_storage_state","httpContext":http,"options":{"indexedDB":indexed_db,"opfs":opfs}}),
            );
            if engine == Engine::Webkit && opfs {
                assert_eq!(
                    complete(&session, id).await.code.as_deref(),
                    Some("OPFS_UNSUPPORTED")
                );
                continue;
            }
            let call = capture_call(&session, &mut peer, id).await;
            assert_eq!(call["guid"], "context");
            assert_eq!(call["method"], "storageState");
            assert_eq!(
                call["params"],
                json!({"indexedDB":indexed_db,"opfs":opfs,"credentials":false})
            );
            let mut source = state();
            if !indexed_db {
                source["origins"][0]["indexedDB"] = json!([]);
            }
            if !opfs {
                source["origins"][0].as_object_mut().unwrap().remove("opfs");
            }
            let mut reply = source.clone();
            reply["credentials"] = json!([]);
            peer.send(&json!({"id":call["id"],"result":reply})).await;
            let result = complete(&session, id).await;
            assert_eq!(result.outcome, Outcome::Succeeded, "{result:?}");
            let receipt = result.result.unwrap();
            assert!(!receipt.to_string().contains("Acme-private"));
            let evidence =
                serde_json::from_value(receipt["reports"][0]["evidenceId"].clone()).unwrap();
            let artifact = session
                .0
                .host
                .http_contexts
                .lock()
                .unwrap()
                .evidence_get(evidence)
                .unwrap();
            assert_eq!(
                serde_json::from_slice::<Value>(&artifact.read().unwrap()).unwrap(),
                source
            );
            assert_eq!(artifact.summary["browserAssociated"], true);
        }
        assert!(
            tokio::time::timeout(Duration::from_millis(25), peer.request())
                .await
                .is_err()
        );
        close(&session).await;
    }
}

#[tokio::test]
async fn http_storage_state_associated_sdk_late_reply_cannot_publish_after_client_or_context_close()
{
    for remove_client in [true, false] {
        let (session, mut peer, _root, context, _page) = harness().await;
        let http = associated(&session, context).await;
        let id = queued(
            &session,
            json!({"operation":"http_context_storage_state","httpContext":http}),
        );
        let call = capture_call(&session, &mut peer, id).await;
        if remove_client {
            session
                .0
                .host
                .http_contexts
                .lock()
                .unwrap()
                .remove(serde_json::from_value(http).unwrap())
                .unwrap();
        } else {
            peer.send(&json!({"guid":"context","method":"close","params":{}}))
                .await;
        }
        peer.send(&json!({"id":call["id"],"result":state()})).await;
        let result = complete(&session, id).await;
        assert_eq!(
            result.code.as_deref(),
            Some("STALE_REFERENCE"),
            "{result:?}"
        );
        let evidence = session.0.host.http_contexts.lock().unwrap().evidence_list();
        assert_eq!(evidence["evidence"], json!([]));
        assert_eq!(evidence["pendingArtifacts"], json!([]));
        assert!(
            !session
                .0
                .host
                .journal
                .directory()
                .join("http-state")
                .exists()
        );
        close(&session).await;
    }
}

#[tokio::test]
async fn http_storage_state_associated_sdk_publication_retry_survives_source_and_client_removal() {
    let (session, mut peer, _root, context, _page) = harness().await;
    let http = associated(&session, context).await;
    let blocked = session.0.host.journal.directory().join("http-state");
    std::fs::write(&blocked, b"Acme publication blocker").unwrap();
    let id = queued(
        &session,
        json!({"operation":"http_context_storage_state","httpContext":http,"options":{"indexedDB":true,"opfs":true}}),
    );
    let call = capture_call(&session, &mut peer, id).await;
    peer.send(&json!({"id":call["id"],"result":state()})).await;
    let failed = complete(&session, id).await;
    assert_eq!(failed.code.as_deref(), Some("EVIDENCE_IO"), "{failed:?}");
    let partial = failed.result.unwrap();
    assert_eq!(partial["stateRetained"], true);
    assert!(!partial.to_string().contains("Acme-private"));
    session
        .0
        .host
        .http_contexts
        .lock()
        .unwrap()
        .remove(serde_json::from_value(http).unwrap())
        .unwrap();
    peer.send(&json!({"guid":"context","method":"close","params":{}}))
        .await;
    std::fs::remove_file(blocked).unwrap();
    let finalized = complete(
        &session,
        queued(
            &session,
            json!({"operation":"http_evidence_finalize","captureId":partial["captureId"]}),
        ),
    )
    .await;
    assert_eq!(finalized.outcome, Outcome::Succeeded, "{finalized:?}");
    let receipt = finalized.result.unwrap();
    let evidence = serde_json::from_value(receipt["reports"][0]["evidenceId"].clone()).unwrap();
    let artifact = session
        .0
        .host
        .http_contexts
        .lock()
        .unwrap()
        .evidence_get(evidence)
        .unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&artifact.read().unwrap()).unwrap(),
        state()
    );
    assert_eq!(
        session.0.host.http_contexts.lock().unwrap().evidence_list()["pendingArtifacts"],
        json!([])
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.request())
            .await
            .is_err(),
        "Publication retry must not read browser state again"
    );
    close(&session).await;
}

#[tokio::test]
async fn http_storage_state_associated_sdk_refuses_unrequested_credentials_and_invalid_or_oversized_results()
 {
    for (source, outcome, code) in [
        (
            json!({"cookies":[],"origins":[],"credentials":[{"id":"Acme","rpId":"example.com","userHandle":"Acme","privateKey":"Acme-private-authenticator","publicKey":"Acme-public-fixture","signCount":0}]}),
            Outcome::Failed,
            "UNREQUESTED_CREDENTIAL_STATE",
        ),
        (
            json!({"cookies":[],"origins":"invalid"}),
            Outcome::Unknown,
            "OUTCOME_UNKNOWN",
        ),
        (
            json!({"cookies":[],"origins":[{"origin":"https://example.com","localStorage":[{"name":"Acme","value":"x".repeat(crate::browser::storage::MAX_BYTES)}]}]}),
            Outcome::Failed,
            "STORAGE_BOUNDS",
        ),
    ] {
        let (session, mut peer, _root, context, _page) = harness().await;
        let http = associated(&session, context).await;
        let id = queued(
            &session,
            json!({"operation":"http_context_storage_state","httpContext":http}),
        );
        let call = capture_call(&session, &mut peer, id).await;
        peer.send(&json!({"id":call["id"],"result":source})).await;
        let result = complete(&session, id).await;
        assert_eq!(result.outcome, outcome, "{result:?}");
        assert_eq!(result.code.as_deref(), Some(code), "{result:?}");
        assert!(!format!("{result:?}").contains("Acme-private"));
        let evidence = session.0.host.http_contexts.lock().unwrap().evidence_list();
        assert_eq!(evidence["evidence"], json!([]));
        assert_eq!(evidence["pendingArtifacts"], json!([]));
        close(&session).await;
    }
}
