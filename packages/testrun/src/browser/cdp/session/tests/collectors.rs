//! Precise coverage controls, bounded deltas and finalization without counter replay.
use super::investigation::owned_page;
use super::*;

async fn started() -> (Worker, Peer, tempfile::TempDir, Reference) {
    started_with(crate::browser::profiling::Options {
        cpu: false,
        javascript_coverage: true,
        css_coverage: true,
        javascript_call_count: false,
        javascript_detailed: false,
        report_anonymous_scripts: Some(false),
        ..Default::default()
    })
    .await
}
async fn started_with(
    options: crate::browser::profiling::Options,
) -> (Worker, Peer, tempfile::TempDir, Reference) {
    let (mut worker, mut peer, root, _, page) = owned_page().await;
    let debugger = options.debugger();
    let call_count = options.javascript_call_count;
    let detailed = options.javascript_detailed;
    let task = tokio::spawn(async move {
        worker
            .profile_start(Uuid::new_v4(), page, options, deadline())
            .await
            .unwrap();
        worker
    });
    let mut methods = vec![
        "Profiler.enable",
        "Profiler.startPreciseCoverage",
        "DOM.enable",
        "CSS.enable",
        "CSS.startRuleUsageTracking",
    ];
    if debugger {
        methods.splice(2..2, ["Debugger.enable", "Debugger.setSkipAllPauses"]);
    }
    for method in methods {
        let request = command(&mut peer, method).await;
        if method == "Profiler.startPreciseCoverage" {
            assert_eq!(
                request["params"],
                json!({"callCount":call_count,"detailed":detailed,"allowTriggeredUpdates":false})
            );
        }
        reply(
            &mut peer,
            &request,
            if method == "Profiler.startPreciseCoverage" {
                json!({"timestamp":1})
            } else if method == "Debugger.enable" {
                json!({"debuggerId":"debugger-acme"})
            } else {
                json!({})
            },
        )
        .await;
    }
    (task.await.unwrap(), peer, root, page)
}
fn javascript() -> Value {
    json!({"timestamp":1,"result":[{"scriptId":"anonymous","url":"","functions":[]},{"scriptId":"Acme","url":"https://example.com/acme.js","functions":[]}]})
}
async fn stopped(
    mut worker: Worker,
    peer: &mut Peer,
    page: Reference,
    uncertain: bool,
) -> (Worker, Value) {
    let options = &worker.profile_status(page).unwrap()["options"];
    let debugger = options["javascriptCoverage"] == true
        && (options["coverageFormat"] == "playwright"
            || options["resetOnNavigation"] == true
            || options["includeSourceTexts"] == true);
    let task = tokio::spawn(async move {
        let value = worker.profile_stop(page, deadline()).await.unwrap();
        (worker, value)
    });
    let mut methods = vec![
        "Profiler.stopPreciseCoverage",
        "CSS.stopRuleUsageTracking",
        "Profiler.disable",
        "CSS.disable",
        "DOM.disable",
    ];
    if !uncertain {
        methods.insert(0, "Profiler.takePreciseCoverage");
    }
    if debugger {
        methods.push("Debugger.disable");
    }
    for method in methods {
        let request = command(peer, method).await;
        let value = match method {
            "Profiler.takePreciseCoverage" => javascript(),
            "CSS.stopRuleUsageTracking" => json!({"ruleUsage":[]}),
            _ => json!({}),
        };
        reply(peer, &request, value).await;
    }
    let (worker, value) = task.await.unwrap();
    assert_eq!(worker.profile_status(page).unwrap()["state"], "idle");
    (worker, value)
}

#[tokio::test]
async fn coverage_sdk_sources_fence_late_reads_at_navigation_and_preserve_private_ranges() {
    use crate::browser::profiling::{CoverageFormat, Options};
    let (mut worker, mut peer, _root, page) = started_with(Options {
        cpu: false,
        javascript_coverage: true,
        css_coverage: true,
        coverage_format: CoverageFormat::Playwright,
        ..Default::default()
    })
    .await;
    assert_eq!(
        worker.profile_status(page).unwrap()["navigation"],
        "reset-on-navigation"
    );
    assert_eq!(
        worker
            .page_debugger(
                Uuid::new_v4(),
                page,
                crate::browser::debugging::Action::Enable {
                    exceptions: crate::browser::debugging::Exceptions::None
                },
                deadline()
            )
            .await
            .unwrap_err()
            .code,
        "COVERAGE_DEBUGGER_CONFLICT"
    );
    worker.profile_event(&Event {
        method: "Debugger.scriptParsed".into(),
        session_id: Some("session-acme".into()),
        params: json!({"scriptId":"new","url":"https://example.com/old.js"}),
    });
    let old = command(&mut peer, "Debugger.getScriptSource").await;
    worker.profile_event(&Event {
        method: "Runtime.executionContextsCleared".into(),
        session_id: Some("session-acme".into()),
        params: json!({}),
    });
    worker.profile_event(&Event {
        method: "Debugger.scriptParsed".into(),
        session_id: Some("session-acme".into()),
        params: json!({"scriptId":"new","url":"https://example.com/new.js?token=Acme"}),
    });
    let source = command(&mut peer, "Debugger.getScriptSource").await;
    assert_eq!(source["params"]["scriptId"], "new");
    reply(
        &mut peer,
        &source,
        json!({"scriptSource":"/* 😀 */ function Acme() { return 42; }"}),
    )
    .await;
    // Reuse an ID and settle the old read last: a key lookup alone cannot fence it.
    reply(&mut peer, &old, json!({"scriptSource":"Acme stale source"})).await;
    worker.profile_event(&Event{method:"CSS.styleSheetAdded".into(),session_id:Some("session-acme".into()),params:json!({"header":{"styleSheetId":"new-css","sourceURL":"https://example.com/acme.css?token=Acme","startLine":0,"startColumn":0,"isInline":false,"length":24}})});
    let source = command(&mut peer, "CSS.getStyleSheetText").await;
    reply(&mut peer, &source, json!({"text":".acme { color: blue; }"})).await;
    let taking = tokio::spawn(async move {
        let result = worker
            .profile_take_coverage(Uuid::new_v4(), page, deadline())
            .await
            .unwrap();
        (worker, result)
    });
    let take = command(&mut peer, "Profiler.takePreciseCoverage").await;
    reply(&mut peer,&take,json!({"timestamp":1,"result":[{"scriptId":"old","url":"https://example.com/old.js","functions":[]},{"scriptId":"anonymous","url":"","functions":[]},{"scriptId":"new","url":"https://example.com/new.js?token=Acme","functions":[{"functionName":"Acme","isBlockCoverage":true,"ranges":[{"startOffset":9,"endOffset":38,"count":2}]}]}]})).await;
    let take = command(&mut peer, "CSS.takeCoverageDelta").await;
    reply(&mut peer,&take,json!({"timestamp":2,"coverage":[{"styleSheetId":"old-css","startOffset":0,"endOffset":40,"used":true},{"styleSheetId":"new-css","startOffset":0,"endOffset":20,"used":true},{"styleSheetId":"new-css","startOffset":4,"endOffset":9,"used":false}]})).await;
    let (worker, result) = taking.await.unwrap();
    assert_eq!(result["complete"], true, "{result}");
    let reports = result["reports"].as_array().unwrap();
    assert_eq!(reports[0]["format"], "playwright-javascript-coverage-delta");
    assert_eq!(reports[0]["sourceTextsIncluded"], true);
    let js: Value = serde_json::from_slice(
        &std::fs::read(reports[0]["attachment"]["path"].as_str().unwrap()).unwrap(),
    )
    .unwrap();
    assert_eq!(js["entries"].as_array().unwrap().len(), 1);
    assert_eq!(
        js["entries"][0]["source"],
        "/* 😀 */ function Acme() { return 42; }"
    );
    assert_eq!(
        js["entries"][0]["functions"][0]["ranges"][0]["startOffset"],
        9
    );
    let css: Value = serde_json::from_slice(
        &std::fs::read(reports[1]["attachment"]["path"].as_str().unwrap()).unwrap(),
    )
    .unwrap();
    assert_eq!(css["entries"].as_array().unwrap().len(), 1);
    assert_eq!(
        css["entries"][0]["url"],
        "https://example.com/acme.css?token=Acme"
    );
    assert_eq!(
        css["entries"][0]["ranges"],
        json!([{"start":0,"end":4},{"start":9,"end":20}])
    );
    stopped(worker, &mut peer, page, false).await;
}
struct BlockedDirectory {
    path: std::path::PathBuf,
    held: std::path::PathBuf,
}

#[tokio::test]
async fn coverage_sdk_source_failure_is_incomplete_without_repeating_a_counter_drain() {
    use crate::browser::profiling::{CoverageFormat, Options};
    for oversized in [false, true] {
        let (mut worker, mut peer, _root, page) = started_with(Options {
            cpu: false,
            javascript_coverage: true,
            css_coverage: true,
            coverage_format: CoverageFormat::Playwright,
            ..Default::default()
        })
        .await;
        worker.profile_event(&Event {
            method: "Debugger.scriptParsed".into(),
            session_id: Some("session-acme".into()),
            params: json!({"scriptId":"unreadable","url":"https://example.com/acme.js"}),
        });
        let source = command(&mut peer, "Debugger.getScriptSource").await;
        if oversized {
            reply(
                &mut peer,
                &source,
                json!({"scriptSource":"😀".repeat(1024*1024+1)}),
            )
            .await;
        } else {
            send(&mut peer,json!({"id":source["id"],"sessionId":source["sessionId"],"error":{"code":-32000,"message":"Acme source unavailable"}})).await;
        }
        let taking = tokio::spawn(async move {
            let result = worker
                .profile_take_coverage(Uuid::new_v4(), page, deadline())
                .await
                .unwrap();
            (worker, result)
        });
        let take = command(&mut peer, "Profiler.takePreciseCoverage").await;
        reply(&mut peer,&take,json!({"timestamp":1,"result":[{"scriptId":"unreadable","url":"https://example.com/acme.js","functions":[]}]})).await;
        let take = command(&mut peer, "CSS.takeCoverageDelta").await;
        reply(&mut peer, &take, json!({"timestamp":1,"coverage":[]})).await;
        let (worker, result) = taking.await.unwrap();
        assert_eq!(result["complete"], false, "{result}");
        assert_eq!(result["reports"][0]["complete"], false);
        let report: Value = serde_json::from_slice(
            &std::fs::read(result["reports"][0]["attachment"]["path"].as_str().unwrap()).unwrap(),
        )
        .unwrap();
        assert_eq!(report["entries"][0]["scriptId"], "unreadable");
        assert!(report["entries"][0].get("source").is_none());
        assert!(
            tokio::time::timeout(Duration::from_millis(25), peer.next())
                .await
                .is_err()
        );
        stopped(worker, &mut peer, page, false).await;
    }
}

#[tokio::test]
async fn coverage_sdk_refuses_to_take_an_existing_application_debugger() {
    let (mut worker, mut peer, _root, _context, page) = owned_page().await;
    let enabling = tokio::spawn(async move {
        worker
            .page_debugger(
                Uuid::new_v4(),
                page,
                crate::browser::debugging::Action::Enable {
                    exceptions: crate::browser::debugging::Exceptions::None,
                },
                deadline(),
            )
            .await
            .unwrap();
        worker
    });
    let command = command(&mut peer, "Debugger.enable").await;
    reply(&mut peer, &command, json!({"debuggerId":"debugger-acme"})).await;
    let request = super::command(&mut peer, "Debugger.setPauseOnExceptions").await;
    reply(&mut peer, &request, json!({})).await;
    let mut worker = enabling.await.unwrap();
    let error = worker
        .profile_start(
            Uuid::new_v4(),
            page,
            crate::browser::profiling::Options {
                javascript_coverage: true,
                coverage_format: crate::browser::profiling::CoverageFormat::Playwright,
                ..Default::default()
            },
            deadline(),
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, "COVERAGE_DEBUGGER_CONFLICT");
    assert_eq!(
        worker.debugger_status(page, false).unwrap()["enabled"],
        true
    );
    assert_eq!(worker.profile_status(page).unwrap()["state"], "idle");
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.next())
            .await
            .is_err()
    );
    let disabling = tokio::spawn(async move {
        worker
            .page_debugger(
                Uuid::new_v4(),
                page,
                crate::browser::debugging::Action::Disable,
                deadline(),
            )
            .await
            .unwrap();
    });
    let request = super::command(&mut peer, "Debugger.disable").await;
    reply(&mut peer, &request, json!({})).await;
    disabling.await.unwrap();
}

#[tokio::test]
async fn coverage_sdk_identity_budget_bounds_retention_without_source_reads() {
    use crate::browser::profiling::{CoverageFormat, Options};
    let (mut worker, mut peer, _root, page) = started_with(Options {
        cpu: false,
        javascript_coverage: true,
        css_coverage: true,
        coverage_format: CoverageFormat::Playwright,
        include_source_texts: Some(false),
        ..Default::default()
    })
    .await;
    for id in 0..513 {
        worker.profile_event(&Event {
            method: "Debugger.scriptParsed".into(),
            session_id: Some("session-acme".into()),
            params: json!({"scriptId":id.to_string(),"url":format!("https://example.com/{id}.js")}),
        });
    }
    let status = worker.profile_status(page).unwrap();
    assert_eq!(status["coverageSourceCount"], 512);
    assert_eq!(status["coverageSourceComplete"], false);
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.next())
            .await
            .is_err()
    );
    let (_, result) = stopped(worker, &mut peer, page, false).await;
    assert_eq!(result["reports"][0]["complete"], false);
}

#[tokio::test]
async fn coverage_sdk_cumulative_sources_preserve_old_entries_when_native_counters_return_them() {
    use crate::browser::profiling::{CoverageFormat, Options};
    let (mut worker, mut peer, _root, page) = started_with(Options {
        cpu: false,
        javascript_coverage: true,
        css_coverage: true,
        coverage_format: CoverageFormat::Playwright,
        reset_on_navigation: Some(false),
        ..Default::default()
    })
    .await;
    for id in ["before", "after"] {
        if id == "after" {
            worker.profile_event(&Event {
                method: "Runtime.executionContextsCleared".into(),
                session_id: Some("session-acme".into()),
                params: json!({}),
            });
        }
        worker.profile_event(&Event {
            method: "Debugger.scriptParsed".into(),
            session_id: Some("session-acme".into()),
            params: json!({"scriptId":id,"url":format!("https://example.com/{id}.js")}),
        });
        let source = command(&mut peer, "Debugger.getScriptSource").await;
        reply(
            &mut peer,
            &source,
            json!({"scriptSource":format!("Acme {id} source")}),
        )
        .await;
    }
    let taking = tokio::spawn(async move {
        let result = worker
            .profile_take_coverage(Uuid::new_v4(), page, deadline())
            .await
            .unwrap();
        (worker, result)
    });
    let take = command(&mut peer, "Profiler.takePreciseCoverage").await;
    reply(&mut peer,&take,json!({"timestamp":1,"result":[{"scriptId":"before","url":"https://example.com/before.js","functions":[]},{"scriptId":"after","url":"https://example.com/after.js","functions":[]}]})).await;
    let take = command(&mut peer, "CSS.takeCoverageDelta").await;
    reply(&mut peer, &take, json!({"timestamp":1,"coverage":[]})).await;
    let (worker, result) = taking.await.unwrap();
    assert_eq!(result["complete"], true, "{result}");
    let js: Value = serde_json::from_slice(
        &std::fs::read(result["reports"][0]["attachment"]["path"].as_str().unwrap()).unwrap(),
    )
    .unwrap();
    assert_eq!(js["entries"].as_array().unwrap().len(), 2);
    assert_eq!(js["entries"][0]["source"], "Acme before source");
    assert_eq!(js["entries"][1]["source"], "Acme after source");
    stopped(worker, &mut peer, page, false).await;
}
impl BlockedDirectory {
    fn new(path: &Path) -> Self {
        let held = path.with_file_name(format!("held-{}", Uuid::new_v4()));
        std::fs::rename(path, &held).unwrap();
        std::fs::write(path, b"Acme blocked evidence directory").unwrap();
        Self {
            path: path.into(),
            held,
        }
    }
}
impl Drop for BlockedDirectory {
    fn drop(&mut self) {
        std::fs::remove_file(&self.path).unwrap();
        std::fs::rename(&self.held, &self.path).unwrap();
    }
}

#[tokio::test]
async fn coverage_deltas_keep_collectors_running_and_retry_only_materialization() {
    let (mut worker, mut peer, _root, page) = started().await;
    let mut previous = Value::Null;
    for blocked in [false, true] {
        let path = worker.journal.directory().to_owned();
        let task = tokio::spawn(async move {
            let result = worker
                .profile_take_coverage(Uuid::new_v4(), page, deadline())
                .await;
            (worker, result)
        });
        let request = command(&mut peer, "Profiler.takePreciseCoverage").await;
        reply(&mut peer, &request, javascript()).await;
        let request = command(&mut peer, "CSS.takeCoverageDelta").await;
        let block = blocked.then(|| BlockedDirectory::new(&path));
        reply(&mut peer, &request, json!({"coverage":[],"timestamp":1})).await;
        let (next, result) = task.await.unwrap();
        worker = next;
        drop(block);
        let value = if blocked {
            let error = result.unwrap_err();
            assert_eq!(error.code, "EVIDENCE_IO");
            let capture =
                serde_json::from_value(error.partial.unwrap()["captureId"].clone()).unwrap();
            worker.coverage_finalize_capture(capture).unwrap().unwrap()
        } else {
            result.unwrap()
        };
        assert_eq!(value["reports"].as_array().unwrap().len(), 2);
        assert_eq!(value["complete"], true);
        assert_ne!(value["captureId"], previous);
        previous = value["captureId"].clone();
        let bytes =
            std::fs::read(value["reports"][0]["attachment"]["path"].as_str().unwrap()).unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&bytes).unwrap()["result"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(worker.profile_status(page).unwrap()["state"], "recording");
        assert!(
            tokio::time::timeout(Duration::from_millis(25), peer.next())
                .await
                .is_err(),
            "Counter read was replayed"
        );
    }
    assert_eq!(
        stopped(worker, &mut peer, page, false).await.1["reports"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

#[tokio::test]
async fn uncertain_coverage_drain_refuses_replay_and_stops_without_a_second_take() {
    let (mut worker, mut peer, _root, page) = started().await;
    let task = tokio::spawn(async move {
        let result = worker
            .profile_take_coverage(
                Uuid::new_v4(),
                page,
                Instant::now() + Duration::from_millis(25),
            )
            .await;
        (worker, result)
    });
    let request = command(&mut peer, "Profiler.takePreciseCoverage").await;
    let (next, result) = task.await.unwrap();
    worker = next;
    assert_eq!(result.unwrap_err().code, "OUTCOME_UNKNOWN");
    assert_eq!(
        worker
            .profile_take_coverage(Uuid::new_v4(), page, deadline())
            .await
            .unwrap_err()
            .code,
        "PROFILE_COVERAGE_UNCERTAIN"
    );
    reply(&mut peer, &request, javascript()).await;
    let (_worker, value) = stopped(worker, &mut peer, page, true).await;
    assert_eq!(value["coverageCapture"]["complete"], false);
    assert_eq!(
        value["coverageCapture"]["reports"][0]["format"],
        "native-coverage-capture-status-json"
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.next())
            .await
            .is_err(),
        "Uncertain counter read was replayed"
    );
}
