//! Installed owned-driver cleanup evidence is bounded and contains no payloads.
use super::*;
#[path = "../../../fixtures/browser_http_defaults.rs"]
mod wire;

#[tokio::test]
async fn installed_step_four_lifecycle_evidence_checkpoint() {
    let fixture = installed_fixture();
    let mut mcp = Mcp::start(&fixture).await;
    let site = wire::Site::start(None).await;
    let mut failures = Vec::new();
    for mode in ["headless", "headed"] {
        for engine in ["chrome", "firefox", "webkit"] {
            let connected = mcp
                .tool("browser_connect", json!({"engine":engine,"mode":mode}))
                .await;
            let target = connected["targetId"].as_str().unwrap();
            let ready = fixture.ready_target(target).await;
            let navigations_before = site
                .records()
                .iter()
                .filter(|record| record["path"] == "/header-page")
                .count();
            let scenario=std::panic::AssertUnwindSafe(async {
                for context in [ready["browser"]["selectedContext"].clone(),operation(&mut mcp,target,json!({"operation":"context_create"})).await["context"].clone()] {
                    let page=operation(&mut mcp,target,json!({"operation":"page_create","context":context,"select":false})).await["page"].clone();
                    operation(&mut mcp,target,json!({"operation":"page_navigate","page":page,"navigation":{"kind":"goto","url":format!("{}/header-page",site.url)}})).await;
                    operation(&mut mcp,target,json!({"operation":"evaluate","scope":page,"expression":"() => {console.error('Acme-private-lifecycle-payload');return 42}","isFunction":true})).await;
                }
                assert_eq!(site.records().iter().filter(|record|record["path"]=="/header-page").count(),navigations_before+2,"Both owned contexts must perform their loopback navigation");
            }).catch_unwind().await;
            let response = performance_lifecycle::disconnect_observed_via(
                &fixture,
                &mut mcp,
                target,
                engine,
                mode,
                &ready["browser"]["selectedContext"],
                if mode == "headed" {
                    "browser_disconnect"
                } else {
                    "disconnect"
                },
            )
            .await;
            let result = &response["result"]["structuredContent"];
            if result["ok"] != true {
                failures.push(format!("{engine}/{mode}: {result}"));
            }
            if let Err(panic) = scenario {
                std::panic::resume_unwind(panic);
            }
            if result["ok"] != true {
                continue;
            }
            let mut recorded = false;
            for namespace in std::fs::read_dir(fixture.workspace.data_root()).unwrap() {
                let path = namespace.unwrap().path().join("browser status.json");
                if path.is_file() {
                    let record: Value =
                        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
                    assert_eq!(
                        record["browsers"],
                        json!([]),
                        "A verified close must update the durable browser list: {record}"
                    );
                    recorded = true;
                }
            }
            assert!(recorded, "The live browser status record must exist");
            if engine == "chrome" {
                assert!(
                    result["data"].get("cleanup").is_none(),
                    "Native CDP must not claim SDK driver evidence: {result}"
                );
                continue;
            }
            let cleanup = &result["data"]["cleanup"];
            assert_eq!(cleanup["complete"], true, "{cleanup}");
            assert_eq!(cleanup["preCloseInspection"], true, "{cleanup}");
            assert!(cleanup["unfinishedStage"].is_null(), "{cleanup}");
            let driver = &cleanup["driverShutdown"];
            assert_eq!(driver["driverExited"], true, "{driver}");
            assert_eq!(driver["descendantsInspected"], true, "{driver}");
            assert_eq!(driver["aliveProcesses"], 0, "{driver}");
            assert!(driver["unfinishedStep"].is_null(), "{driver}");
            assert_eq!(driver["stderrClosed"], true, "{driver}");
            assert_eq!(driver["stderrReadFailed"], false, "{driver}");
            let samples = &driver["processWait"];
            assert_eq!(samples["clock"], "driver-session");
            assert_eq!(samples["observationsOnly"], true);
            assert_eq!(samples["omittedSamples"], 0);
            let samples = samples["samples"].as_array().unwrap();
            assert!((2..=32).contains(&samples.len()), "{driver}");
            for sample in samples {
                assert_eq!(sample["inspectionSucceeded"], true, "{sample}");
                assert!(sample["driverAlive"].is_boolean(), "{sample}");
                for count in [
                    "descendantsAlive",
                    "descendantsZombies",
                    "descendantsAbsent",
                    "descendantsReused",
                ] {
                    assert!(sample[count].is_u64(), "{sample}");
                }
                assert!(sample["elapsedMs"].is_u64(), "{sample}");
                assert_eq!(sample.as_object().unwrap().len(), 7, "{sample}");
            }
            let ended = samples.last().unwrap();
            assert_eq!(ended["driverAlive"], false, "{driver}");
            // The driver can exit before a witnessed descendant; final release
            // is proved independently by aliveProcesses and completion above.
            let lifecycle = &driver["lifecycle"];
            let children = &lifecycle["ownedChildren"];
            assert_eq!(children["observationsOnly"], true, "{children}");
            assert_eq!(children["invalidObservations"], 0, "{children}");
            assert_eq!(children["capacityExceeded"], false, "{children}");
            assert_eq!(children["sourceUnavailable"], false, "{children}");
            let owners = children["children"].as_array().unwrap();
            assert_eq!(
                owners.len(),
                1,
                "One owned SDK browser child is expected: {children}"
            );
            let child = &owners[0];
            let observing = child["observingElapsedMs"].as_u64().unwrap();
            let exited = child["nativeExitElapsedMs"].as_u64().unwrap();
            let closed = child["nativeCloseElapsedMs"].as_u64().unwrap();
            assert!(observing <= exited && exited <= closed, "{child}");
            for field in [
                "observingStreams",
                "nativeExitStreams",
                "nativeCloseStreams",
            ] {
                let streams = child[field].as_array().unwrap();
                assert!(!streams.is_empty() && streams.len() <= 5, "{child}");
                for (index, stream) in streams.iter().enumerate() {
                    assert_eq!(stream["index"], index, "{stream}");
                    assert!(stream["present"].is_boolean(), "{stream}");
                    for flag in ["closed", "destroyed", "readableEnded", "writableFinished"] {
                        assert!(
                            stream[flag].is_boolean() || stream[flag].is_null(),
                            "{stream}"
                        );
                    }
                    assert_eq!(stream.as_object().unwrap().len(), 6, "{stream}");
                }
            }
            assert!(!children.to_string().contains("pid"), "{children}");
            assert!(!children.to_string().contains("Acme-private"), "{children}");
            assert_eq!(lifecycle["observationsOnly"], true);
            assert_eq!(lifecycle["omittedEvents"], 0);
            assert_eq!(lifecycle["omittedLines"], 0);
            assert_eq!(lifecycle["partialLine"], false);
            let events = lifecycle["events"].as_array().unwrap();
            assert!(events.len() <= 64);
            for phase in [
                "graceful_close_started",
                "browser_process_closed",
                "temporary_cleanup_started",
                "temporary_cleanup_finished",
                "graceful_close_finished",
            ] {
                assert!(
                    events.iter().any(|event| event["phase"] == phase),
                    "Missing {phase}: {lifecycle}"
                );
            }
            for event in events {
                assert_eq!(event.as_object().unwrap().len(), 2, "{event}");
            }
            assert!(!cleanup.to_string().contains("Acme-private"), "{cleanup}");
            assert!(!cleanup.to_string().contains(&site.url), "{cleanup}");
        }
    }
    assert!(
        failures.is_empty(),
        "Original cleanup failures: {failures:?}"
    );
    mcp.stop().await;
}
