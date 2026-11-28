//! Ordered protocol fixtures exercise owned pauses and probes without a live browser.
use super::*;
use crate::browser::debugging::{Action, Exceptions};
async fn enabled() -> (Worker, Peer, tempfile::TempDir, Reference) {
    let (mut worker, mut peer, root, _, page) = super::investigation::owned_page().await;
    let task = tokio::spawn(async move {
        worker
            .page_debugger(
                Uuid::new_v4(),
                page,
                Action::Enable {
                    exceptions: Exceptions::All,
                },
                deadline(),
            )
            .await
            .unwrap();
        worker
    });
    let c = command(&mut peer, "Debugger.enable").await;
    reply(&mut peer, &c, json!({"debuggerId":"Acme"})).await;
    let c = command(&mut peer, "Debugger.setPauseOnExceptions").await;
    assert_eq!(c["params"]["state"], "all");
    reply(&mut peer, &c, json!({})).await;
    (task.await.unwrap(), peer, root, page)
}
fn paused(worker: &mut Worker, page: Reference) -> Uuid {
    worker.debugger_event(&Event{method:"Debugger.paused".into(),session_id:Some("session-acme".into()),params:json!({"reason":"exception","callFrames":[{"callFrameId":"frame-native","functionName":"Acme","url":"https://example.com/a.js","location":{"lineNumber":2,"columnNumber":0},"scopeChain":[{"type":"local","object":{"objectId":"scope-native"}}]}]})});
    serde_json::from_value(worker.debugger_status(page, false).unwrap()["pauseId"].clone()).unwrap()
}
#[tokio::test]
async fn interactive_debugging_reads_only_owned_scope_descriptors_and_pages_private_values() {
    let (mut worker, mut peer, _root, page) = enabled().await;
    let pause = paused(&mut worker, page);
    let status = worker.debugger_status(page, true).unwrap();
    assert!(status["sessionId"].is_string());
    assert_eq!(status["frames"][0]["scopes"][0]["type"], "local");
    let task = tokio::spawn(async move {
        let v = worker
            .page_debugger(
                Uuid::new_v4(),
                page,
                Action::Variables {
                    pause_id: pause,
                    frame: 0,
                    scope: 0,
                    offset: 0,
                    limit: 2,
                },
                deadline(),
            )
            .await
            .unwrap();
        (worker, v)
    });
    let c = command(&mut peer, "Runtime.getProperties").await;
    assert_eq!(
        c["params"],
        json!({"objectId":"scope-native","ownProperties":true,"generatePreview":false})
    );
    reply(&mut peer,&c,json!({"result":[{"name":"secret","configurable":true,"enumerable":true,"value":{"type":"string","value":"private-value"}},{"name":"getter","configurable":true,"enumerable":true,"get":{"type":"function","objectId":"never-invoke"}},{"name":"nested","configurable":true,"enumerable":true,"value":{"type":"object","objectId":"never-expose"}}]})).await;
    let (mut worker, v) = task.await.unwrap();
    assert_eq!(v["variables"][0]["value"], "private-value");
    assert_eq!(v["variables"][1]["accessorSkipped"], true);
    assert_eq!(v["nextOffset"], 2);
    assert_eq!(v["potentiallyMutating"], false);
    assert!(!v.to_string().contains("never-invoke"));
    paused(&mut worker, page);
    assert_eq!(
        worker
            .page_debugger(
                Uuid::new_v4(),
                page,
                Action::Variables {
                    pause_id: pause,
                    frame: 0,
                    scope: 0,
                    offset: 0,
                    limit: 2
                },
                deadline()
            )
            .await
            .unwrap_err()
            .code,
        "STALE_DEBUGGER_PAUSE"
    );
}
#[tokio::test]
async fn interactive_debugging_discards_variables_when_resume_arrives_before_read_reply() {
    let (mut worker, mut peer, _root, page) = enabled().await;
    let pause = paused(&mut worker, page);
    let task = tokio::spawn(async move {
        worker
            .page_debugger(
                Uuid::new_v4(),
                page,
                Action::Variables {
                    pause_id: pause,
                    frame: 0,
                    scope: 0,
                    offset: 0,
                    limit: 10,
                },
                deadline(),
            )
            .await
    });
    let c = command(&mut peer, "Runtime.getProperties").await;
    send(
        &mut peer,
        json!({"method":"Debugger.resumed","sessionId":"session-acme","params":{}}),
    )
    .await;
    reply(
        &mut peer,
        &c,
        json!({"result":[{"name":"stale","configurable":true,"enumerable":true,"value":{"type":"number","value":42}}]}),
    )
    .await;
    assert_eq!(
        task.await.unwrap().unwrap_err().code,
        "STALE_DEBUGGER_PAUSE"
    );
}
#[tokio::test]
async fn interactive_debugging_logpoints_are_literal_owned_and_removed_without_erasing_other_diagnostics()
 {
    let (mut worker, mut peer, _root, page) = enabled().await;
    let marker = "Acme\"),evil(),(\"";
    let task = tokio::spawn(async move {
        let result = worker
            .page_debugger(
                Uuid::new_v4(),
                page,
                Action::LogpointSet {
                    url: "https://example.com/a.js".into(),
                    line: 2,
                    column: 0,
                    marker: marker.into(),
                },
                deadline(),
            )
            .await
            .unwrap();
        (worker, result)
    });
    let c = command(&mut peer, "Debugger.setBreakpointByUrl").await;
    assert_eq!(
        c["params"]["condition"],
        format!(
            "(console.debug({}),false)",
            json!(json!({"testrunMarker":marker}).to_string())
        )
    );
    reply(
        &mut peer,
        &c,
        json!({"breakpointId":"native-probe","locations":[]}),
    )
    .await;
    let (mut worker, result) = task.await.unwrap();
    let id = serde_json::from_value(result["breakpointId"].clone()).unwrap();
    assert_eq!(
        worker.debugger_status(page, false).unwrap()["temporaryLogpoints"][0]["marker"],
        marker
    );
    let task = tokio::spawn(async move {
        worker
            .page_debugger(
                Uuid::new_v4(),
                page,
                Action::BreakpointRemove { breakpoint_id: id },
                deadline(),
            )
            .await
            .unwrap();
        worker
    });
    let c = command(&mut peer, "Debugger.removeBreakpoint").await;
    assert_eq!(c["params"]["breakpointId"], "native-probe");
    reply(&mut peer, &c, json!({})).await;
    let worker = task.await.unwrap();
    assert_eq!(
        worker.debugger_status(page, false).unwrap()["temporaryLogpoints"],
        json!([])
    );
}
#[tokio::test]
async fn interactive_debugging_excludes_cpu_profiles_without_dispatch() {
    let (mut worker, mut peer, _root, page) = enabled().await;
    assert_eq!(
        worker
            .profile_start(
                Uuid::new_v4(),
                page,
                crate::browser::profiling::Options::default(),
                deadline()
            )
            .await
            .unwrap_err()
            .code,
        "DEBUGGER_PROFILE_CONFLICT"
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.next())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn interactive_debugging_refuses_to_take_an_owned_cpu_profiler() {
    let (mut worker, mut peer, _root, _, page) = super::investigation::owned_page().await;
    let task = tokio::spawn(async move {
        worker
            .profile_start(
                Uuid::new_v4(),
                page,
                crate::browser::profiling::Options::default(),
                deadline(),
            )
            .await
            .unwrap();
        worker
    });
    for method in [
        "Profiler.enable",
        "Profiler.setSamplingInterval",
        "Profiler.start",
    ] {
        let c = command(&mut peer, method).await;
        reply(&mut peer, &c, json!({})).await;
    }
    let mut worker = task.await.unwrap();
    assert_eq!(
        worker
            .page_debugger(
                Uuid::new_v4(),
                page,
                Action::Enable {
                    exceptions: Exceptions::None
                },
                deadline()
            )
            .await
            .unwrap_err()
            .code,
        "DEBUGGER_PROFILE_CONFLICT"
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.next())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn interactive_debugging_failed_enable_retains_exclusion_and_explicit_owned_release() {
    let (mut worker, mut peer, _root, _, page) = super::investigation::owned_page().await;
    let task = tokio::spawn(async move {
        assert!(
            worker
                .page_debugger(
                    Uuid::new_v4(),
                    page,
                    Action::Enable {
                        exceptions: Exceptions::None
                    },
                    deadline()
                )
                .await
                .is_err()
        );
        worker
    });
    let c = command(&mut peer, "Debugger.enable").await;
    send(&mut peer,json!({"id":c["id"],"sessionId":c["sessionId"],"error":{"code":-32000,"message":"Acme enable unavailable"}})).await;
    let mut worker = task.await.unwrap();
    assert!(worker.debugger_owned(page));
    assert_eq!(
        worker
            .profile_start(
                Uuid::new_v4(),
                page,
                crate::browser::profiling::Options::default(),
                deadline()
            )
            .await
            .unwrap_err()
            .code,
        "DEBUGGER_PROFILE_CONFLICT"
    );
    let task = tokio::spawn(async move {
        worker
            .page_debugger(Uuid::new_v4(), page, Action::Disable, deadline())
            .await
            .unwrap();
        worker
    });
    let c = command(&mut peer, "Debugger.disable").await;
    reply(&mut peer, &c, json!({})).await;
    let worker = task.await.unwrap();
    assert!(!worker.debugger_owned(page));
}
