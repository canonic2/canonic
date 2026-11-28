//! Durability distinguishes an actual SDK reply from conservative transport uncertainty.
use super::*;

fn remote(name: &str) -> connection::Error {
    connection::Error::Remote(Box::new(crate::browser::dispatcher::RemoteError {
        error: json!({"error":{"name":name,"message":"Acme private reply","stack":"Acme private stack"}}),
        details: Some(json!({"url":"https://example.com/?Acme-private"})),
        log: Some(json!(["Acme private call log"])),
    }))
}

#[test]
fn backend_failure_witness_distinguishes_reply_deadline_and_dispatch_without_payloads() {
    for (cause, code, expected) in [
        (
            remote("TimeoutError"),
            "OUTCOME_UNKNOWN",
            json!({"source":"sdk-reply","kind":"timeout","dispatch":"replied"}),
        ),
        (
            remote("TargetClosedError"),
            "STALE_REFERENCE",
            json!({"source":"sdk-reply","kind":"target-closed","dispatch":"replied"}),
        ),
        (
            remote("Acme private class"),
            "BROWSER_METHOD_FAILED",
            json!({"source":"sdk-reply","kind":"method-error","dispatch":"replied"}),
        ),
        (
            connection::Error::Timeout {
                dispatch: connection::Dispatch::NotDispatched,
            },
            "NOT_DISPATCHED",
            json!({"source":"driver-transport","kind":"deadline","dispatch":"not-dispatched"}),
        ),
        (
            connection::Error::Timeout {
                dispatch: connection::Dispatch::PossiblyDispatched,
            },
            "OUTCOME_UNKNOWN",
            json!({"source":"driver-transport","kind":"deadline","dispatch":"possibly-dispatched"}),
        ),
        (
            connection::Error::Lost {
                dispatch: connection::Dispatch::NotDispatched,
            },
            "BACKEND_LOST",
            json!({"source":"driver-transport","kind":"lost","dispatch":"not-dispatched"}),
        ),
        (
            connection::Error::Lost {
                dispatch: connection::Dispatch::PossiblyDispatched,
            },
            "OUTCOME_UNKNOWN",
            json!({"source":"driver-transport","kind":"lost","dispatch":"possibly-dispatched"}),
        ),
    ] {
        let error = Error::from(cause);
        assert_eq!(error.code, code);
        let evidence = error.failure_evidence().unwrap();
        assert_eq!(evidence, expected);
        assert!(!evidence.to_string().contains("Acme"));
        let wrapped = Error::caused(
            "CALLBACK_OUTCOME_UNKNOWN",
            Error::caused("OUTCOME_UNKNOWN", error),
        );
        assert_eq!(wrapped.failure_evidence(), Some(expected));
    }
    assert!(
        Error::from("OUTCOME_UNKNOWN").failure_evidence().is_none(),
        "An outcome code alone cannot invent backend evidence"
    );
    assert!(
        Error::from(connection::Error::Busy)
            .failure_evidence()
            .is_none()
    );
}

#[tokio::test]
async fn backend_failure_witness_is_durable_and_correlated_without_public_payloads() {
    let (session, mut peer, _directory, context, _) = harness().await;
    let id = session
        .submit(
            Uuid::new_v4(),
            Request::PageCreate {
                context,
                select: false,
            },
            5000,
        )
        .unwrap();
    let call = peer.request().await;
    assert_eq!(call["method"], "newPage");
    peer.send(&json!({"id":call["id"],"error":{"error":{"name":"TimeoutError","message":"Acme private reply","stack":"Acme private stack"}},"log":["Acme private call log"]})).await;
    let operation = complete(&session, id).await;
    assert_eq!(operation.outcome, Outcome::Unknown);
    assert_eq!(operation.code.as_deref(), Some("OUTCOME_UNKNOWN"));
    assert!(operation.result.is_none());
    let mut witnesses = vec![];
    for entry in std::fs::read_dir(session.journal_directory()).unwrap() {
        let path = entry.unwrap().path();
        if path
            .extension()
            .is_some_and(|extension| extension == "json")
        {
            let bytes = std::fs::read(path).unwrap();
            assert!(!String::from_utf8_lossy(&bytes).contains("Acme private"));
            let value: Value = serde_json::from_slice(&bytes).unwrap();
            if value["operationId"] == json!(id) && value.get("backendFailure").is_some() {
                witnesses.push(value);
            }
        }
    }
    assert_eq!(witnesses.len(), 1);
    assert_eq!(
        witnesses[0]["backendFailure"],
        json!({"source":"sdk-reply","kind":"timeout","dispatch":"replied"})
    );
    assert!(witnesses[0].get("result").is_none());
    close(&session).await;
}
