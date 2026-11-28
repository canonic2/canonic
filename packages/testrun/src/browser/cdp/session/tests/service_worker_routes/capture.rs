//! Captured worker identity is provenance, never a creating page's authority.
use super::*;
use crate::browser::contract::{HarCaptureContent, HarCaptureMode};

fn request(worker: &mut Worker, session: &str, native: &str) {
    worker.diagnostic_event(&Event{method:"Network.requestWillBeSent".into(),session_id:Some(session.into()),params:json!({"requestId":native,"type":"Fetch","timestamp":1,"wallTime":2,"request":{"url":"https://example.com/acme","method":"GET","headers":{}}})});
}

fn publish(worker: &mut Worker, context: Reference) -> Value {
    worker.har_archives.capture_stop(context).unwrap();
    let receipt = worker
        .har_archives
        .capture_publish(worker.journal.directory(), context)
        .unwrap();
    serde_json::from_slice(
        &std::fs::read(
            receipt["reports"][0]["attachment"]["path"]
                .as_str()
                .unwrap(),
        )
        .unwrap(),
    )
    .unwrap()
}

#[tokio::test]
async fn native_service_worker_capture_preserves_source_in_full_and_minimal_after_page_close() {
    for mode in [HarCaptureMode::Full, HarCaptureMode::Minimal] {
        let (mut worker, _peer, _root, context, page) = owned_page().await;
        worker
            .har_archives
            .capture_start(
                context,
                None,
                (HarCaptureContent::Omit, mode, None),
                None,
                None,
            )
            .unwrap();
        let service = service_worker(&mut worker);
        worker.index.retire(page).unwrap();
        request(&mut worker, "Acme-service-session", "Acme-captured");
        let archive = publish(&mut worker, context);
        let entries = archive["log"]["entries"].as_array().unwrap();
        assert_eq!(entries.len(), 1, "{archive}");
        assert!(entries[0]["pageref"].is_null(), "{archive}");
        assert_eq!(
            entries[0]["_serviceWorkerRef"],
            format!("worker@{}", service.id),
            "{archive}"
        );
        assert_eq!(
            entries[0]["_testrunCaptureComplete"], false,
            "A source identity cannot invent completion"
        );
    }
}

#[tokio::test]
async fn native_service_worker_capture_page_filter_and_foreign_or_retired_sessions_grant_no_scope()
{
    for page_only in [false, true] {
        let (mut worker, _peer, _root, context, page) = owned_page().await;
        worker
            .har_archives
            .capture_start(
                context,
                page_only.then_some(page),
                (HarCaptureContent::Omit, HarCaptureMode::Full, None),
                None,
                None,
            )
            .unwrap();
        let service = service_worker(&mut worker);
        request(&mut worker, "Acme-foreign-session", "Acme-foreign");
        request(&mut worker, "Acme-service-session", "Acme-owned");
        worker.index.retire(service).unwrap();
        request(&mut worker, "Acme-service-session", "Acme-retired");
        let archive = publish(&mut worker, context);
        let entries = archive["log"]["entries"].as_array().unwrap();
        assert_eq!(entries.len(), if page_only { 0 } else { 1 }, "{archive}");
        if !page_only {
            assert_eq!(
                entries[0]["_serviceWorkerRef"],
                format!("worker@{}", service.id)
            );
        }
    }
}
