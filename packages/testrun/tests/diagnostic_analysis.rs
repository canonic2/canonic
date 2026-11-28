//! Real executable parser boundary, independent of target/device acceptance.
use base64::Engine;
use serde_json::{Value, json};
use std::{
    io::Write,
    process::{Command, Stdio},
};

fn worker(format: &str, raw: &[u8]) -> Value {
    // Concurrent package builds can replace Cargo's public executable while a
    // test launches several workers. Thread ownership also removes the pin
    // when that test thread exits, including panic paths.
    thread_local! {static PINNED:tempfile::TempDir={
        let directory=tempfile::tempdir().unwrap();
        let destination=directory.path().join("testrun");
        match std::fs::hard_link(env!("CARGO_BIN_EXE_testrun"),&destination){
            Ok(())=>{},
            Err(error) if error.kind()==std::io::ErrorKind::CrossesDevices=>{std::fs::copy(env!("CARGO_BIN_EXE_testrun"),&destination).unwrap();},
            Err(error)=>panic!("Cannot pin parser test executable: {error}"),
        }
        directory
    };}
    PINNED.with(|directory| worker_at(&directory.path().join("testrun"), format, raw))
}
#[tokio::test]
async fn diagnostic_observability_public_trace_correction_survives_export_deletion() {
    let f = InvestigationFixture(tempfile::tempdir().unwrap());
    std::fs::hard_link(env!("CARGO_BIN_EXE_testrun"), f.0.path().join("testrun")).unwrap();
    let caps = f.call(&["diagnostics", "capabilities"]).await;
    assert_eq!(caps["observability"]["queryView"], "observability");
    assert!(!f.0.path().join("state/owner.log").exists());
    let opened = f
        .call(&[
            "diagnostics",
            "open",
            "--submission-id",
            "incident",
            "--question",
            "Verify Acme deployed trace correction",
        ])
        .await;
    let id = opened["investigationId"].as_str().unwrap();
    for (n, code) in [2, 1].into_iter().enumerate() {
        let packet = json!({"resourceSpans":[{"resource":{"attributes":[{"key":"service.name","value":{"stringValue":"api"}},{"key":"service.version","value":{"stringValue":format!("release-{n}")}}]},"scopeSpans":[{"spans":[{"traceId":"11111111111111111111111111111111","spanId":"2222222222222222","name":"checkout","startTimeUnixNano":"1000000000","endTimeUnixNano":"1000000100","status":{"code":code}}]}]}],"resourceLogs":[{"scopeLogs":[{"logRecords":[{"traceId":"11111111111111111111111111111111","body":{"stringValue":"observed"}}]}]}]});
        let path = f.0.path().join(format!("incident-{n}.json"));
        std::fs::write(&path, packet.to_string()).unwrap();
        let source = json!({"kind":"import","path":path,"format":"otlp-json"}).to_string();
        let captured = f
            .call(&[
                "diagnostics",
                "start",
                id,
                "--submission-id",
                &format!("incident-{n}"),
                "--source",
                &source,
            ])
            .await;
        let cid = captured["collectors"].as_array().unwrap().last().unwrap()["collectorId"]
            .as_str()
            .unwrap();
        f.settled(id, None).await;
        std::fs::remove_file(path).unwrap();
        let result=state_query(&f,id,cid,r#"{"view":"assertions","assertions":{"checks":[{"id":"status","domain":"api","name":"checkout","predicate":{"type":"equals","pointer":"/status/code","expected":1}}]}}"#).await;
        assert_eq!(
            result["outcome"],
            if n == 0 { "assertion_failed" } else { "passed" }
        );
        assert_eq!(result["checks"][0]["witnessOrdinals"], json!([0]));
        let analysis = result["analysisId"].as_str().unwrap();
        let joins = f
            .call(&[
                "diagnostics",
                "query",
                id,
                analysis,
                "--filters",
                r#"{"view":"observability"}"#,
            ])
            .await;
        assert_eq!(joins["traces"][0]["spanObservations"], 1);
        assert_eq!(
            joins["traces"][0]["traceId"],
            "11111111111111111111111111111111"
        );
        assert_eq!(joins["rawSha256"].as_str().unwrap().len(), 64);
        assert_eq!(joins["causalityEstablished"], false);
    }
    f.call(&["diagnostics", "close", id]).await;
    f.call(&["diagnostics", "remove", id]).await;
    f.call(&["manager", "stop"]).await;
}
#[tokio::test]
async fn diagnostic_services_public_runtime_failure_correction_survives_source_deletion() {
    let f = InvestigationFixture(tempfile::tempdir().unwrap());
    std::fs::hard_link(env!("CARGO_BIN_EXE_testrun"), f.0.path().join("testrun")).unwrap();
    let caps = f.call(&["diagnostics", "capabilities"]).await;
    assert_eq!(caps["services"]["registration"], "implemented_bounded");
    assert!(!f.0.path().join("state/owner.log").exists());
    let opened = f
        .call(&[
            "diagnostics",
            "open",
            "--submission-id",
            "service",
            "--question",
            "Verify Acme sampled path correction",
        ])
        .await;
    let id = opened["investigationId"].as_str().unwrap();
    for (n, name) in ["Acme.slow", "Acme.fast"].into_iter().enumerate() {
        let payload = json!({"nodes":[{"id":1,"callFrame":{"functionName":"root"},"children":[2]},{"id":2,"callFrame":{"functionName":name,"url":"acme.js","lineNumber":12}}],"startTime":0,"endTime":100,"samples":[2],"timeDeltas":[100]});
        let envelope = json!({"schemaVersion":1,"selection":{"targetId":"service:Acme:api","serviceId":"api","hostId":"Acme","os":"linux","transport":"kubernetes_export","pid":42,"processGeneration":format!("namespace:42:birth-{n}"),"runtime":"node","runtimeVersion":"fixture-1","buildId":format!("build-{n}"),"architecture":"x86_64","scope":{"kind":"kubernetes","cluster":"acme","namespace":"qa","podUid":format!("pod-{n}"),"containerId":format!("container-{n}"),"restartCount":n,"imageId":format!("sha256:image-{n}")},"owner":{"kind":"external","invocationRef":format!("capture-{n}")}},"format":"js-cpu-profile","encoding":"json","payload":payload});
        let path = f.0.path().join(format!("service-{n}.json"));
        std::fs::write(&path, envelope.to_string()).unwrap();
        let source =
            json!({"kind":"import","path":path,"format":"service-evidence-v1"}).to_string();
        let captured = f
            .call(&[
                "diagnostics",
                "start",
                id,
                "--submission-id",
                &format!("service-{n}"),
                "--source",
                &source,
            ])
            .await;
        let cid = captured["collectors"].as_array().unwrap().last().unwrap()["collectorId"]
            .as_str()
            .unwrap();
        f.settled(id, None).await;
        std::fs::remove_file(path).unwrap();
        let filters = r#"{"view":"assertions","assertions":{"checks":[{"id":"path","domain":"performance","name":"JavaScript CPU sample","predicate":{"type":"equals","pointer":"/frames/1/functionName","expected":"Acme.fast"}}]}}"#;
        let query = state_query(&f, id, cid, filters).await;
        assert_eq!(
            query["outcome"],
            if n == 0 { "assertion_failed" } else { "passed" }
        );
        assert_eq!(query["checks"][0]["witnessOrdinals"], json!([1]));
        assert_eq!(query["rawSha256"].as_str().unwrap().len(), 64);
        let analysis = query["analysisId"].as_str().unwrap();
        let tree = f
            .call(&[
                "diagnostics",
                "query",
                id,
                analysis,
                "--filters",
                r#"{"view":"flamegraph"}"#,
            ])
            .await;
        assert_eq!(tree["rows"][0]["value"], 1.0);
    }
    f.call(&["diagnostics", "close", id]).await;
    f.call(&["diagnostics", "remove", id]).await;
    f.call(&["manager", "stop"]).await;
}
#[cfg(unix)]
#[tokio::test]
async fn diagnostic_observability_public_configured_prometheus_and_sentry_gets_are_retained() {
    use std::os::unix::fs::PermissionsExt;
    let f = InvestigationFixture(tempfile::tempdir().unwrap());
    std::fs::hard_link(env!("CARGO_BIN_EXE_testrun"), f.0.path().join("testrun")).unwrap();
    let opened = f
        .call(&[
            "diagnostics",
            "open",
            "--submission-id",
            "remote",
            "--question",
            "Retain Acme provider evidence",
        ])
        .await;
    let id = opened["investigationId"].as_str().unwrap();
    for provider in ["prometheus", "sentry_event"] {
        let resource = if provider == "sentry_event" {
            "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"
        } else {
            "api"
        };
        let scope = json!({"account":"acme","project":"api","environment":"staging","region":"eu","resource":resource,"release":"v1"});
        let mut metric = scope.clone();
        metric["__name__"] = json!("requests_total");
        let payload=if provider=="prometheus"{json!({"status":"success","data":{"resultType":"matrix","result":[{"metric":metric,"values":[[1767225600,"3"]]}]}})}else{json!({"eventID":resource,"dateCreated":"2026-01-01T00:00:00Z","release":{"version":"v1","customer":"omit"},"level":"error","tags":[{"key":"environment","value":"staging"}],"email":"omit","contexts":{"trace":{"trace_id":"11111111111111111111111111111111","span_id":"2222222222222222","customer":"omit"}}})}.to_string();
        let app = axum::Router::new().fallback(axum::routing::get(
            move |request: axum::extract::Request| {
                let payload = payload.clone();
                async move {
                    assert!(request.headers().get("authorization").is_none());
                    assert_eq!(
                        request.uri().path(),
                        if provider == "prometheus" {
                            "/api/v1/query_range"
                        } else {
                            "/api/0/projects/acme/api/events/eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee/"
                        }
                    );
                    payload
                }
            },
        ));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let labels = json!({"account":"account","project":"project","environment":"environment","region":"region","resource":"resource","release":"release"});
        let read = if provider == "prometheus" {
            json!({"kind":provider,"metric":"requests_total","unit":"1","definition":"recorded cumulative count","stepSeconds":60,"maxSeries":1,"scopeLabels":labels})
        } else {
            json!({"kind":provider})
        };
        let path = f.0.path().join(format!("{provider}.json"));
        let config = json!({"schemaVersion":1,"sourceId":provider,"scope":scope,"endpoint":format!("http://{addr}"),"authorizationOwner":"owned-fixture-proxy","authorization":{"kind":"none"},"read":read,"timeoutMs":3000,"maxWindowSeconds":60,"maxRecords":10,"maxBytes":4096});
        std::fs::write(&path, config.to_string()).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let source=json!({"kind":"observability_http","configFile":path,"sourceId":provider,"scope":scope,"window":{"from":"2026-01-01T00:00:00Z","to":"2026-01-01T00:01:00Z"}}).to_string();
        let captured = f
            .call(&[
                "diagnostics",
                "start",
                id,
                "--submission-id",
                provider,
                "--source",
                &source,
            ])
            .await;
        let cid = captured["collectors"].as_array().unwrap().last().unwrap()["collectorId"]
            .as_str()
            .unwrap();
        f.settled(id, None).await;
        std::fs::remove_file(path).unwrap();
        server.abort();
        let _ = server.await;
        let rows = state_query(&f, id, cid, "{}").await;
        assert_eq!(rows["rows"].as_array().unwrap().len(), 2, "{rows}");
        assert_eq!(
            rows["rows"][1]["record"]["data"]["observability"]["provider"],
            if provider == "prometheus" {
                "prometheus"
            } else {
                "sentry"
            }
        );
        assert!(!rows.to_string().contains("customer"));
    }
    f.call(&["diagnostics", "close", id]).await;
    f.call(&["diagnostics", "remove", id]).await;
    f.call(&["manager", "stop"]).await;
}
#[cfg(any(target_os = "macos", target_os = "linux"))]
#[tokio::test]
async fn diagnostic_services_public_host_snapshot_borrows_only_selected_child_and_replays_after_exit()
 {
    let f = InvestigationFixture(tempfile::tempdir().unwrap());
    std::fs::hard_link(env!("CARGO_BIN_EXE_testrun"), f.0.path().join("testrun")).unwrap();
    let mut child = tokio::process::Command::new("/bin/sleep")
        .arg("20")
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let pid = child.id().unwrap();
    let target = format!("process:Acme:{pid}");
    let binding = json!({"targetId":target,"appId":"api"}).to_string();
    let opened = f
        .call(&[
            "diagnostics",
            "open",
            "--submission-id",
            "host",
            "--question",
            "Inspect selected Acme child",
            "--binding",
            &binding,
        ])
        .await;
    let id = opened["investigationId"].as_str().unwrap();
    let source=json!({"kind":"host_process","targetId":target,"serviceId":"api","hostId":"Acme","processId":pid}).to_string();
    let captured = f
        .call(&[
            "diagnostics",
            "start",
            id,
            "--submission-id",
            "snapshot",
            "--source",
            &source,
        ])
        .await;
    let cid = captured["collectors"][0]["collectorId"].as_str().unwrap();
    f.settled(id, None).await;
    assert!(child.try_wait().unwrap().is_none());
    child.kill().await.unwrap();
    child.wait().await.unwrap();
    let query = state_query(&f, id, cid, r#"{"kind":"metric"}"#).await;
    let rows = query["rows"].as_array().unwrap();
    assert!(rows.len() >= 2);
    assert_eq!(rows[0]["record"]["data"]["pid"], pid);
    assert_eq!(rows[0]["record"]["data"]["ownership"], "borrowed");
    assert!(rows.iter().any(|r| r["record"]["unit"] == "bytes"));
    let replay = f
        .call(&[
            "diagnostics",
            "start",
            id,
            "--submission-id",
            "snapshot",
            "--source",
            &source,
        ])
        .await;
    assert_eq!(replay["collectors"].as_array().unwrap().len(), 1);
    assert_eq!(replay["collectors"][0]["collectorId"], cid);
    f.call(&["diagnostics", "close", id]).await;
    f.call(&["diagnostics", "remove", id]).await;
    f.call(&["manager", "stop"]).await;
}
#[tokio::test]
async fn diagnostic_frameworks_public_composition_retains_failed_corrected_and_native_witnesses() {
    let f = InvestigationFixture(tempfile::tempdir().unwrap());
    std::fs::hard_link(env!("CARGO_BIN_EXE_testrun"), f.0.path().join("testrun")).unwrap();
    let caps = f.call(&["diagnostics", "capabilities"]).await;
    assert_eq!(caps["frameworks"]["registration"], "implemented_bounded");
    assert!(!f.0.path().join("state/owner.log").exists());
    let opened = f
        .call(&[
            "diagnostics",
            "open",
            "--submission-id",
            "framework",
            "--question",
            "Investigate Acme bridge correction",
        ])
        .await;
    let id = opened["investigationId"].as_str().unwrap();
    for (attempt, state) in ["failed", "ready"].into_iter().enumerate() {
        let packet = json!({"schemaVersion":1,"framework":"kotlin_multiplatform","frameworkVersion":"fixture-1","target":{"id":"android:Acme","appId":"com.example.acme","class":"automotive","os":"android","transport":"import"},"build":{"id":"build-a","architecture":"arm64","mode":"debug","aot":false,"trimmed":false},"components":[{"id":"shared","role":"runtime","processGeneration":"boot:42:birth","runtimeGeneration":format!("runtime-{attempt}"),"buildId":"build-a","architecture":"arm64","format":"runtime-samples-v1","payload":{"schemaVersion":1,"runtime":"kotlin","buildId":"build-a","symbolsBuildId":"build-a","architecture":"arm64","records":[{"name":"Acme bridge result","category":"bridge","identity":"bridge-1","state":state,"frames":["shared.Acme.load(Acme.kt:12)"],"actionId":"load-1"}]}},{"id":"native","role":"native","processGeneration":"boot:42:birth","buildId":"build-a","architecture":"arm64","format":"android-diagnostics-v1","payload":{"schemaVersion":1,"appId":"com.example.acme","kind":"state","data":{"status":"ok","fields":{},"metrics":[]}}}],"links":[{"id":"load-1","kind":"bridge","from":{"componentId":"shared","ordinal":0},"to":{"componentId":"native","ordinal":0}}]});
        let path = f.0.path().join(format!("framework-{attempt}.json"));
        std::fs::write(&path, packet.to_string()).unwrap();
        let source =
            json!({"kind":"import","path":path,"format":"framework-evidence-v1"}).to_string();
        let capture = f
            .call(&[
                "diagnostics",
                "start",
                id,
                "--submission-id",
                &format!("attempt-{attempt}"),
                "--source",
                &source,
            ])
            .await;
        let cid = capture["collectors"].as_array().unwrap().last().unwrap()["collectorId"]
            .as_str()
            .unwrap();
        f.settled(id, None).await;
        std::fs::remove_file(path).unwrap();
        let filters = r#"{"view":"assertions","assertions":{"checks":[{"id":"bridge-ready","domain":"api","name":"Acme bridge result","predicate":{"type":"equals","pointer":"/state","expected":"ready"}}]}}"#;
        let result = state_query(&f, id, cid, filters).await;
        assert_eq!(
            result["outcome"],
            if attempt == 0 {
                "assertion_failed"
            } else {
                "passed"
            }
        );
        assert_eq!(result["checks"][0]["witnessOrdinals"], json!([1]));
        assert_eq!(result["rawSha256"].as_str().unwrap().len(), 64);
        let analysis = result["analysisId"].as_str().unwrap();
        let links = f
            .call(&[
                "diagnostics",
                "query",
                id,
                analysis,
                "--filters",
                r#"{"correlationId":"load-1"}"#,
            ])
            .await;
        assert_eq!(
            links["rows"][0]["record"]["data"]["witnessOrdinals"],
            json!([1, 3])
        );
    }
    f.call(&["diagnostics", "close", id]).await;
    f.call(&["diagnostics", "remove", id]).await;
    f.call(&["manager", "stop"]).await;
}
#[tokio::test]
async fn diagnostic_testing_public_failed_corrected_and_runner_outcomes_keep_evidence_witnesses() {
    let f = InvestigationFixture(tempfile::tempdir().unwrap());
    std::fs::hard_link(env!("CARGO_BIN_EXE_testrun"), f.0.path().join("testrun")).unwrap();
    let caps = f.call(&["diagnostics", "capabilities"]).await;
    assert_eq!(caps["testing"]["registration"], "implemented_bounded");
    assert!(!f.0.path().join("state/owner.log").exists());
    let opened = f
        .call(&[
            "diagnostics",
            "open",
            "--submission-id",
            "testing",
            "--question",
            "Verify Acme checkout correction",
        ])
        .await;
    let id = opened["investigationId"].as_str().unwrap();
    for (n, outcome) in [
        "assertion_failed",
        "passed",
        "automation_failed",
        "fixture_failed",
        "unavailable",
    ]
    .into_iter()
    .enumerate()
    {
        let packet = json!({"schemaVersion":1,"runId":"run-1","runner":"web","invocationRef":"check-1","control":"testrun_released","buildId":"build-a","targetId":"browser:Acme","coverage":"complete","cases":[{"id":"checkout","attempt":1,"name":"checkout renders","outcome":outcome,"message":"fixture observation","durationMs":12,"actionIds":["tap-1"],"attachments":[{"kind":"screenshot","name":"failure","reference":"/outside/never-opened.png"}],"capture":{"strategy":"none","beforeMs":0,"afterMs":0,"limitBytes":0,"lost":false},"checks":[]}]});
        let path = f.0.path().join(format!("report-{n}.json"));
        std::fs::write(&path, packet.to_string()).unwrap();
        let source = json!({"kind":"import","path":path,"format":"test-results-v1"}).to_string();
        let capture = f
            .call(&[
                "diagnostics",
                "start",
                id,
                "--submission-id",
                &format!("case-{n}"),
                "--source",
                &source,
            ])
            .await;
        let cid = capture["collectors"].as_array().unwrap().last().unwrap()["collectorId"]
            .as_str()
            .unwrap();
        f.settled(id, None).await;
        std::fs::remove_file(path).unwrap();
        let q = r#"{"view":"assertions","assertions":{"checks":[{"id":"rendered","domain":"ui","name":"checkout renders","predicate":{"type":"equals","pointer":"/test/outcome","expected":"passed"}}]}}"#;
        let result = state_query(&f, id, cid, q).await;
        assert_eq!(result["outcome"], outcome, "{result}");
        assert_eq!(result["checks"][0]["witnessOrdinals"], json!([0]));
        assert!(result["rawSha256"].as_str().is_some_and(|s| s.len() == 64));
        assert_eq!(result["executionPerformed"], false);
    }
    let path = f.0.path().join("junit.xml");
    std::fs::write(
        &path,
        "<testsuite><testcase name='corrected' time='0.01'/></testsuite>",
    )
    .unwrap();
    let source = json!({"kind":"import","path":path,"format":"junit-xml"}).to_string();
    let capture = f
        .call(&[
            "diagnostics",
            "start",
            id,
            "--submission-id",
            "junit",
            "--source",
            &source,
        ])
        .await;
    let cid = capture["collectors"].as_array().unwrap().last().unwrap()["collectorId"]
        .as_str()
        .unwrap();
    f.settled(id, None).await;
    let result=state_query(&f,id,cid,r#"{"view":"assertions","assertions":{"checks":[{"id":"runner-pass","domain":"api","name":"corrected","predicate":{"type":"equals","pointer":"/test/outcome","expected":"passed"}}]}}"#).await;
    assert_eq!(result["outcome"], "passed");
    assert_eq!(result["observationComplete"], false);
    f.call(&["diagnostics", "close", id]).await;
    f.call(&["diagnostics", "remove", id]).await;
    f.call(&["manager", "stop"]).await;
}
#[tokio::test]
async fn diagnostic_reproduction_public_plan_marker_and_controlled_build_comparison() {
    let f = InvestigationFixture(tempfile::tempdir().unwrap());
    std::fs::hard_link(env!("CARGO_BIN_EXE_testrun"), f.0.path().join("testrun")).unwrap();
    let began = std::time::Instant::now();
    let plan=f.call(&["diagnostics","plan","soak","--platform","android","--options",r#"{"targetId":"android:Acme","appId":"com.example.acme","userId":0,"durationMs":10000}"#]).await;
    assert_eq!(plan["automaticExecution"], false);
    assert_eq!(plan["capture"]["limits"]["durationMs"], 10000);
    assert!(!f.0.path().join("state/owner.log").exists());
    eprintln!(
        "reproduction plan fresh CLI wall {:?}; debug host fixture, no target or warm-process latency claim",
        began.elapsed()
    );
    let opened = f
        .call(&[
            "diagnostics",
            "open",
            "--submission-id",
            "reproduction",
            "--question",
            "Compare Acme checkout",
            "--binding",
            r#"{"actionIds":["checkout"]}"#,
        ])
        .await;
    let id = opened["investigationId"].as_str().unwrap();
    verify_reproduction_marker(&f, id).await;
    let mut analyses = vec![];
    for (n, (build, value, outcome)) in [
        ("build-a", 100, "passed"),
        ("build-b", 70, "passed"),
        ("build-c", 60, "failed"),
    ]
    .into_iter()
    .enumerate()
    {
        let raw = json!({"schemaVersion":1,"runId":build,"scenario":"checkout","scenarioVersion":"v1","buildId":build,"targetId":"android:Acme","appId":"com.example.acme","runtime":"ART","osVersion":"API36","hardware":"emulator","mode":"debug","warmState":"warm","warmupAttempts":2,"sampling":"elapsed per attempt","instrumentation":["markers"],"contention":"isolated fixture","fixtureId":"data-v1","metric":{"name":"checkout","unit":"milliseconds","definition":"request to rendered result","direction":"lower"},"budget":{"maxAttempts":2,"maxDurationMs":10000,"elapsedMs":2000},"termination":"completed","cleanup":"verified","coverage":"complete","attempts":[
            {"id":"a1","actionId":"checkout","atMs":100,"outcome":outcome,"reason":"fixture observation","value":value,"evidenceRefs":["capture-1"]},
            {"id":"a2","actionId":"checkout","atMs":1000,"outcome":"passed","reason":"fixture observation","value":value+20,"evidenceRefs":["capture-2"]}
        ]});
        let mut raw = raw;
        raw["metric"]["uncertainty"] = json!(1.0);
        let path = f.0.path().join(format!("run-{n}.json"));
        std::fs::write(&path, raw.to_string()).unwrap();
        let source = json!({"kind":"import","path":path,"format":"reproduction-v1"}).to_string();
        let captured = f
            .call(&[
                "diagnostics",
                "start",
                id,
                "--submission-id",
                &format!("run-{n}"),
                "--source",
                &source,
            ])
            .await;
        let cid = captured["collectors"].as_array().unwrap().last().unwrap()["collectorId"]
            .as_str()
            .unwrap();
        f.settled(id, None).await;
        std::fs::remove_file(&path).unwrap();
        let parsed = f
            .call(&[
                "diagnostics",
                "analyze",
                id,
                cid,
                "--submission-id",
                &format!("parse-{n}"),
                "--metadata",
                r#"{"observationComplete":true}"#,
            ])
            .await;
        let aid = parsed["analysisId"].as_str().unwrap();
        assert_eq!(f.settled(id, Some(aid)).await["phase"], "complete");
        analyses.push(aid.to_owned());
    }
    let good = f
        .call(&["diagnostics", "compare", id, &analyses[0], id, &analyses[1]])
        .await;
    assert_eq!(good["compatible"], true, "{good}");
    assert_eq!(good["candidateMinusBaselineMedian"], -30.0);
    assert_eq!(good["medianDifferenceUncertainty"], 2.0);
    let bad = f
        .call(&["diagnostics", "compare", id, &analyses[0], id, &analyses[2]])
        .await;
    assert_eq!(bad["compatible"], false);
    assert!(bad["candidateMinusBaselineMedian"].is_null());
    f.call(&["diagnostics", "close", id]).await;
    f.call(&["diagnostics", "remove", id]).await;
    f.call(&["manager", "stop"]).await;
}

async fn verify_reproduction_marker(f: &InvestigationFixture, id: &str) {
    let marker = r#"{"kind":"marker","name":"before checkout","actionId":"checkout","phase":"before_action"}"#;
    let marked = f
        .call(&[
            "diagnostics",
            "start",
            id,
            "--submission-id",
            "marker",
            "--source",
            marker,
        ])
        .await;
    let cid = marked["collectors"][0]["collectorId"].as_str().unwrap();
    f.settled(id, None).await;
    let rows = state_query(f, id, cid, r#"{"kind":"marker"}"#).await;
    assert_eq!(
        rows["rows"][0]["record"]["data"]["actionOutcomeVerified"],
        false
    );
    let replay = f
        .call(&[
            "diagnostics",
            "start",
            id,
            "--submission-id",
            "marker",
            "--source",
            marker,
        ])
        .await;
    assert_eq!(
        replay["collectors"][0]["sha256"],
        f.settled(id, None).await["collectors"][0]["sha256"]
    );
}
#[tokio::test]
async fn diagnostic_runtime_checks_public_import_analysis_and_private_failure_query() {
    let f = InvestigationFixture(tempfile::tempdir().unwrap());
    std::fs::hard_link(env!("CARGO_BIN_EXE_testrun"), f.0.path().join("testrun")).unwrap();
    let caps = f.call(&["diagnostics", "capabilities"]).await;
    assert_eq!(
        caps["interactiveDebugging"]["registration"],
        "implemented_bounded"
    );
    assert!(!f.0.path().join("state/owner.log").exists());
    let normalized=json!({"schemaVersion":1,"targetId":"android:Acme","appId":"com.example.acme",
        "processGeneration":"boot:42:start","buildId":"build-a","timing":"instrumented",
        "environment":["ASan enabled"],"coverage":"fixture process",
        "reports":[{"id":"fault-1","tool":"strict_mode","category":"policy_violation","message":"private fixture read on main thread",
            "stacks":[],"rawReference":"artifact:log"}]}).to_string();
    for (n, (format, raw)) in [
        ("runtime-checks-v1", normalized.as_str()),
        (
            "sanitizer-report-text",
            "==42==ERROR: AddressSanitizer: heap-use-after-free\n #0 /src/acme.cc:3\n",
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let opened = f
            .call(&[
                "diagnostics",
                "open",
                "--submission-id",
                &format!("checks-{n}"),
                "--question",
                "Inspect Acme runtime checks",
            ])
            .await;
        let id = opened["investigationId"].as_str().unwrap();
        let input = f.0.path().join(format!("checks-{n}"));
        std::fs::write(&input, raw).unwrap();
        let source = json!({"kind":"import","path":input,"format":format}).to_string();
        let capture = f
            .call(&[
                "diagnostics",
                "start",
                id,
                "--submission-id",
                "checks",
                "--source",
                &source,
            ])
            .await;
        let cid = capture["collectors"][0]["collectorId"].as_str().unwrap();
        f.settled(id, None).await;
        let result = state_query(&f, id, cid, r#"{"kind":"failure","limit":20}"#).await;
        assert_eq!(result["rows"].as_array().unwrap().len(), 1, "{result}");
        assert_eq!(
            result["rows"][0]["record"]["data"]["absenceProvesCleanRun"],
            false
        );
        assert!(result["rows"][0]["record"]["timestamp"].is_null());
        f.call(&["diagnostics", "close", id]).await;
        f.call(&["diagnostics", "remove", id]).await;
    }
    f.call(&["manager", "stop"]).await;
}
#[tokio::test]
async fn diagnostic_scenario_context_public_media_energy_restore_and_replay() {
    let f = InvestigationFixture(tempfile::tempdir().unwrap());
    std::fs::hard_link(env!("CARGO_BIN_EXE_testrun"), f.0.path().join("testrun")).unwrap();
    let caps = f.call(&["diagnostics", "capabilities"]).await;
    assert_eq!(caps["scenarioContext"]["registration"], "implemented");
    assert!(!f.0.path().join("state/owner.log").exists());
    let raw = json!({"schemaVersion":1,"targetId":"android:Acme","appId":"com.example.acme","processGeneration":"boot:42:birth","hardware":"physical","coverage":"fixture markers","clocks":[{"id":"app","unit":"milliseconds"}],"records":[
        {"id":"media","clock":"app","at":0,"scope":"app","actionId":"play1","observation":{"type":"media","sessionId":"session1","itemId":"track1","state":"buffering","positionSource":"service_extrapolated","positionMs":100,"bufferState":"stalled","route":"speaker reported"}},
        {"id":"power","clock":"app","at":1,"scope":"device","observation":{"type":"energy","kind":"battery_level","value":70,"unit":"percent","attribution":"device","definition":"device battery level"}},
        {"id":"restore","clock":"app","at":2,"scope":"device","observation":{"type":"setting","name":"connectivity","before":"online","applied":"offline","current":"offline","phase":"restore_failed","owner":"owned_scenario","receiptRef":"network1","restoreError":"readback unavailable"}}
    ]});
    let parsed = worker_at(
        &f.0.path().join("testrun"),
        "scenario-context-v1",
        raw.to_string().as_bytes(),
    );
    assert!(parsed["index"].is_object(), "{parsed}");
    let opened = f
        .call(&[
            "diagnostics",
            "open",
            "--submission-id",
            "scenario",
            "--question",
            "Acme media/background evidence",
        ])
        .await;
    let id = opened["investigationId"].as_str().unwrap();
    let input = f.0.path().join("scenario.json");
    std::fs::write(&input, raw.to_string()).unwrap();
    let source = json!({"kind":"import","path":input,"format":"scenario-context-v1"}).to_string();
    let started = f
        .call(&[
            "diagnostics",
            "start",
            id,
            "--submission-id",
            "scenario",
            "--source",
            &source,
        ])
        .await;
    let cid = started["collectors"][0]["collectorId"].as_str().unwrap();
    f.settled(id, None).await;
    let result = state_query(
        &f,
        id,
        cid,
        r#"{"view":"scenario","scenario":{"operation":"media","actionId":"play1"}}"#,
    )
    .await;
    assert_eq!(result["matched"], 1);
    assert_eq!(
        result["rows"][0]["data"]["scenario"]["observation"]["bufferState"],
        "stalled"
    );
    let aid = result["analysisId"].as_str().unwrap();
    std::fs::remove_file(&input).unwrap();
    let restore = f
        .call(&[
            "diagnostics",
            "query",
            id,
            aid,
            "--filters",
            r#"{"view":"scenario","scenario":{"operation":"restore"}}"#,
        ])
        .await;
    assert_eq!(
        restore["rows"][0]["data"]["restoration"]["status"],
        "reported_restore_failure"
    );
    assert_eq!(
        restore["rows"][0]["data"]["restoration"]["performed"],
        false
    );
    assert_eq!(restore["collectorId"], cid);
    let energy = f
        .call(&[
            "diagnostics",
            "query",
            id,
            aid,
            "--filters",
            r#"{"view":"scenario","scenario":{"operation":"energy"}}"#,
        ])
        .await;
    assert_eq!(energy["rows"][0]["data"]["appId"], Value::Null);
    assert_eq!(energy["rawSha256"].as_str().unwrap().len(), 64);
}
#[tokio::test]
async fn diagnostic_network_data_public_queries_indexes_and_source_deletion_replay() {
    let f = InvestigationFixture(tempfile::tempdir().unwrap());
    std::fs::hard_link(env!("CARGO_BIN_EXE_testrun"), f.0.path().join("testrun")).unwrap();
    let caps = f.call(&["diagnostics", "capabilities"]).await;
    assert_eq!(caps["networkData"]["registration"], "implemented");
    assert!(!f.0.path().join("state/owner.log").exists());
    let opened = f
        .call(&[
            "diagnostics",
            "open",
            "--submission-id",
            "network-data",
            "--question",
            "Acme request and data evidence",
        ])
        .await;
    let id = opened["investigationId"].as_str().unwrap();
    let network = json!({"schemaVersion":1,"format":"network-evidence-v1","source":"Acme request probe","coverage":"selected request and service log","records":[
        {"type":"request","id":"request","identity":{"scope":"client","requestId":"request1","traceId":"11111111111111111111111111111111"},"protocol":"http3","method":"GET","address":"https://example.com/orders","status":503,"phases":{"dns":null,"connect":0,"total":12000}},
        {"type":"log","id":"log","identity":{"scope":"service","requestId":"request1","traceId":"11111111111111111111111111111111"},"message":"upstream unavailable"}
    ]});
    let snapshot = json!({"schemaVersion":1,"format":"data-snapshot-v1","backend":"room","dataset":"orders","scope":"Acme fixture","account":"local","project":"Acme","role":"cache","snapshotId":"one","consistency":"best_effort","sourceOwner":"app export","coverage":"selected fixture keys","collections":[{"name":"orders","indexes":["state"],"records":[{"key":"a","fields":{"state":null}},{"key":"b","fields":{}}]}]});
    for (format, raw, filters) in [
        (
            "network-evidence-v1",
            network,
            r#"{"view":"network_data","networkData":{"operation":"correlate","traceId":"11111111111111111111111111111111"}}"#,
        ),
        (
            "data-snapshot-v1",
            snapshot,
            r#"{"view":"network_data","networkData":{"operation":"data","collection":"orders","equals":{"field":"state","value":null}}}"#,
        ),
    ] {
        // The CLI, manager and standalone parser share one pinned executable;
        // another Cargo build must not substitute a different schema mid-flow.
        let parsed = worker_at(
            &f.0.path().join("testrun"),
            format,
            raw.to_string().as_bytes(),
        );
        assert!(parsed["index"].is_object(), "{format}: {parsed}");
        let path = f.0.path().join(format);
        std::fs::write(&path, raw.to_string()).unwrap();
        let source = json!({"kind":"import","path":path,"format":format}).to_string();
        let started = f
            .call(&[
                "diagnostics",
                "start",
                id,
                "--submission-id",
                format,
                "--source",
                &source,
            ])
            .await;
        let cid = started["collectors"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["format"] == format)
            .unwrap()["collectorId"]
            .as_str()
            .unwrap();
        f.settled(id, None).await;
        let result = state_query(&f, id, cid, filters).await;
        assert_eq!(
            result["matched"],
            if format == "network-evidence-v1" {
                2
            } else {
                1
            }
        );
        if format == "data-snapshot-v1" {
            assert_eq!(result["rows"][0]["record"]["key"], "a");
            assert_eq!(result["selection"]["remoteExecution"], false);
        }
        let aid = result["analysisId"].as_str().unwrap();
        std::fs::remove_file(&path).unwrap();
        let replay = f
            .call(&["diagnostics", "query", id, aid, "--filters", filters])
            .await;
        assert_eq!(result["rows"], replay["rows"]);
        assert_eq!(replay["collectorId"], cid);
        assert_eq!(replay["rawSha256"].as_str().unwrap().len(), 64);
    }
}
#[tokio::test]
async fn diagnostic_performance_public_milestones_frames_and_replay() {
    let f = InvestigationFixture(tempfile::tempdir().unwrap());
    std::fs::hard_link(env!("CARGO_BIN_EXE_testrun"), f.0.path().join("testrun")).unwrap();
    let caps = f.call(&["diagnostics", "capabilities"]).await;
    assert_eq!(caps["performance"]["registration"], "implemented");
    assert!(!f.0.path().join("state/owner.log").exists());
    let raw = json!({"schemaVersion":1,"runtime":"android","buildMode":"profile","instrumentation":"Acme app markers","clocks":[{"id":"app","unit":"milliseconds"}],"records":[
        {"type":"startup","runId":"cold1","state":"cold","stateEvidence":"producer witnessed new process","milestones":[{"name":"launch_requested","clock":"app","at":10,"uncertainty":1,"definition":"launch intent observed"},{"name":"first_display","clock":"app","at":210,"uncertainty":1,"definition":"producer initial display report"}]},
        {"type":"frame","id":"1","clock":"app","start":210,"end":230,"deadline":226,"thread":"UI","definition":"producer completion duration"}
    ]});
    let parsed = worker("ui-performance-v1", raw.to_string().as_bytes());
    assert_eq!(parsed["index"]["rows"][2]["value"], 200000.0);
    let opened = f
        .call(&[
            "diagnostics",
            "open",
            "--submission-id",
            "performance",
            "--question",
            "Acme startup and frame costs",
        ])
        .await;
    let id = opened["investigationId"].as_str().unwrap();
    let input = f.0.path().join("ui-performance.json");
    std::fs::write(&input, raw.to_string()).unwrap();
    let source = json!({"kind":"import","path":input,"format":"ui-performance-v1"}).to_string();
    let started = f
        .call(&[
            "diagnostics",
            "start",
            id,
            "--submission-id",
            "performance",
            "--source",
            &source,
        ])
        .await;
    let cid = started["collectors"][0]["collectorId"].as_str().unwrap();
    f.settled(id, None).await;
    let query = state_query(
        &f,
        id,
        cid,
        r#"{"view":"performance","performance":{"operation":"startup"}}"#,
    )
    .await;
    assert_eq!(query["rows"][2]["value"], 200000.0);
    assert_eq!(
        query["rows"][2]["data"]["performance"]["intervalRangeMicros"],
        json!({"lower":198000.0,"upper":202000.0})
    );
    assert_eq!(query["collectorId"], cid);
    assert_eq!(query["rawSha256"].as_str().unwrap().len(), 64);
    let aid = query["analysisId"].as_str().unwrap();
    std::fs::remove_file(&input).unwrap();
    let frames = f
        .call(&[
            "diagnostics",
            "query",
            id,
            aid,
            "--filters",
            r#"{"view":"performance","performance":{"operation":"frames"}}"#,
        ])
        .await;
    assert_eq!(
        frames["rows"][0]["data"]["performance"]["missedDeadline"],
        true
    );
    assert_eq!(
        frames["rows"][0]["data"]["performance"]["presented"],
        Value::Null
    );
    let refused=tokio::process::Command::from(f.command()).args(["diagnostics","query",id,aid,"--filters",r#"{"view":"performance","performance":{"operation":"frames","evaluate":"must-not-run"}}"#]).output().await.unwrap();
    assert!(!refused.status.success());
}
#[tokio::test]
async fn diagnostic_memory_public_hprof_queries_series_probe_validation_and_replay() {
    let f = InvestigationFixture(tempfile::tempdir().unwrap());
    std::fs::hard_link(env!("CARGO_BIN_EXE_testrun"), f.0.path().join("testrun")).unwrap();
    let caps = f.call(&["diagnostics", "capabilities"]).await;
    assert_eq!(caps["memory"]["registration"], "implemented");
    assert!(!f.0.path().join("state/owner.log").exists());
    let raw = minimal_hprof();
    let parsed = worker("java-hprof", &raw);
    assert_eq!(
        parsed["index"]["rows"][0]["data"]["memorySummary"]["runtime"],
        "jvm_hprof"
    );
    assert_eq!(
        worker("java-hprof", &raw[..raw.len() - 1])["error"],
        "INVALID_MEMORY_ARTIFACT"
    );
    let opened = f
        .call(&[
            "diagnostics",
            "open",
            "--submission-id",
            "memory",
            "--question",
            "Acme retained memory",
        ])
        .await;
    let id = opened["investigationId"].as_str().unwrap();
    let input = f.0.path().join("acme.hprof");
    std::fs::write(&input, raw).unwrap();
    let source = json!({"kind":"import","path":input,"format":"java-hprof"}).to_string();
    let started = f
        .call(&[
            "diagnostics",
            "start",
            id,
            "--submission-id",
            "heap",
            "--source",
            &source,
        ])
        .await;
    let cid = started["collectors"][0]["collectorId"].as_str().unwrap();
    let status = f.settled(id, None).await;
    let sha = status["collectors"][0]["sha256"].as_str().unwrap();
    let object = state_query(
        &f,
        id,
        cid,
        r#"{"view":"memory","memory":{"operation":"object","objectId":"0xc8"}}"#,
    )
    .await;
    let aid = object["analysisId"].as_str().unwrap();
    assert_eq!(object["rows"][0]["modeledRetainedBytes"], 16);
    assert_eq!(object["leakEstablished"], false);
    std::fs::remove_file(&input).unwrap();
    let path = f
        .call(&[
            "diagnostics",
            "query",
            id,
            aid,
            "--filters",
            r#"{"view":"memory","memory":{"operation":"paths","objectId":"0xc8"}}"#,
        ])
        .await;
    assert_eq!(path["rows"][0]["nodes"][0]["className"], "Acme");
    verify_memory_series_probes(&f, id, cid, sha).await;
}

fn minimal_hprof() -> Vec<u8> {
    // Independent minimal JVM HPROF producer: one class, one rooted instance.
    fn rec(tag: u8, body: &[u8], raw: &mut Vec<u8>) {
        raw.push(tag);
        raw.extend(0u32.to_be_bytes());
        raw.extend((body.len() as u32).to_be_bytes());
        raw.extend(body);
    }
    let mut raw = b"JAVA PROFILE 1.0.2\0".to_vec();
    raw.extend(4u32.to_be_bytes());
    raw.extend(1u64.to_be_bytes());
    let mut string = 1u32.to_be_bytes().to_vec();
    string.extend(b"Acme");
    rec(1, &string, &mut raw);
    let mut load = vec![];
    for n in [1u32, 100, 0, 1] {
        load.extend(n.to_be_bytes());
    }
    rec(2, &load, &mut raw);
    let mut heap = vec![0x20];
    for n in [100u32, 0, 0, 0, 0, 0, 0, 0, 16] {
        heap.extend(n.to_be_bytes());
    }
    heap.extend([0; 6]);
    heap.push(0x21);
    for n in [200u32, 0, 100, 0] {
        heap.extend(n.to_be_bytes());
    }
    heap.push(0xff);
    heap.extend(200u32.to_be_bytes());
    rec(0x0c, &heap, &mut raw);
    raw
}

async fn verify_memory_series_probes(f: &InvestigationFixture, id: &str, cid: &str, sha: &str) {
    let body = json!({"schemaVersion":1,"runtime":"native","processGeneration":"birth-a","buildId":"build-a","probeRefs":[{"collectorId":cid,"sha256":sha}],"samples":[{"phase":"baseline","iteration":0,"elapsedMicros":0,"counters":[{"resource":"file_handles","value":3,"unit":"count","source":"app-owned descriptor inventory"}]},{"phase":"after_action","iteration":1,"elapsedMicros":1,"actionId":"action:one","counters":[{"resource":"file_handles","value":8,"unit":"count","source":"app-owned descriptor inventory"}]},{"phase":"release","iteration":1,"elapsedMicros":2,"actionId":"action:one","probeIds":[cid],"counters":[{"resource":"file_handles","value":4,"unit":"count","source":"app-owned descriptor inventory"}]},{"phase":"cleanup","iteration":1,"elapsedMicros":3,"probeIds":[cid],"counters":[{"resource":"file_handles","value":3,"unit":"count","source":"app-owned descriptor inventory"}]}]});
    for valid in [true, false] {
        let mut body = body.clone();
        if !valid {
            body["probeRefs"][0]["sha256"] = json!("0".repeat(64));
        }
        let file = f.0.path().join(if valid {
            "series.json"
        } else {
            "wrong-probe.json"
        });
        std::fs::write(&file, body.to_string()).unwrap();
        let source = json!({"kind":"import","path":file,"format":"memory-series-v1"}).to_string();
        let submission = if valid { "series" } else { "wrong-probe" };
        let started = f
            .call(&[
                "diagnostics",
                "start",
                id,
                "--submission-id",
                submission,
                "--source",
                &source,
            ])
            .await;
        let series_id = started["collectors"].as_array().unwrap().last().unwrap()["collectorId"]
            .as_str()
            .unwrap();
        f.settled(id, None).await;
        if valid {
            let series = state_query(
                f,
                id,
                series_id,
                r#"{"view":"memory","memory":{"operation":"series"}}"#,
            )
            .await;
            assert_eq!(series["rows"][0]["baselineDelta"], 1.0);
            assert_eq!(series["rows"][1]["baselineDelta"], 0.0);
            assert_eq!(series["summary"]["probeRefs"][0]["collectorId"], cid);
            assert_eq!(series["leakEstablished"], false);
        } else {
            let output = tokio::process::Command::from(f.command())
                .args([
                    "diagnostics",
                    "analyze",
                    id,
                    series_id,
                    "--submission-id",
                    "refused",
                ])
                .output()
                .await
                .unwrap();
            assert!(!output.status.success());
            assert!(
                String::from_utf8_lossy(&output.stderr).contains("MEMORY_PROBE_MISMATCH")
                    || String::from_utf8_lossy(&output.stdout).contains("MEMORY_PROBE_MISMATCH")
            );
        }
    }
}
#[tokio::test]
async fn diagnostic_native_trace_public_queries_and_original_export_survive_input_deletion() {
    use sha2::{Digest, Sha256};
    let f = InvestigationFixture(tempfile::tempdir().unwrap());
    std::fs::hard_link(env!("CARGO_BIN_EXE_testrun"), f.0.path().join("testrun")).unwrap();
    let original = b"owned private native artifact";
    let report = "sample:\n  event_type: cpu-clock:u\n  time: 200\n  event_count: 99\n  thread_id: 42\n  thread_name: Acme\n  vaddr_in_file: 20\n  file: /libacme.so\n  symbol: leaf\n  callchain:\n    vaddr_in_file: 10\n    file: /libacme.so\n    symbol: main\nsample:\n  event_type: cpu-clock:u\n  time: 100\n  event_count: 88\n  thread_id: 42\n  thread_name: Acme\n  vaddr_in_file: 20\n  file: /libacme.so\n  symbol: leaf\n  callchain:\n    vaddr_in_file: 10\n    file: /libacme.so\n    symbol: main\nlost_situation:\n  lost_count: 0\n";
    let envelope = json!({"schemaVersion":1,"backend":"simpleperf","targetId":"android:ACME","declaredAppId":"com.example.acme","actionIds":["action:native"],"report":{"identity":{"bootId":"owned-boot"},"gaps":[]},"artifact":{"encoding":"identity","sha256":format!("{:x}",Sha256::digest(original)),"base64":base64::engine::general_purpose::STANDARD.encode(original)},"sections":[{"name":"samples","format":"simpleperf-text","text":report}]});
    let decoded = worker("native-trace-v1", envelope.to_string().as_bytes());
    assert_eq!(decoded["index"]["rows"][0]["unit"], "samples");
    let input = f.0.path().join("native.json");
    std::fs::write(&input, envelope.to_string()).unwrap();
    let opened = f
        .call(&[
            "diagnostics",
            "open",
            "--submission-id",
            "native",
            "--question",
            "Acme native path",
        ])
        .await;
    let id = opened["investigationId"].as_str().unwrap();
    let source = json!({"kind":"import","path":input,"format":"native-trace-v1"}).to_string();
    let started = f
        .call(&[
            "diagnostics",
            "start",
            id,
            "--submission-id",
            "native",
            "--source",
            &source,
        ])
        .await;
    let cid = started["collectors"][0]["collectorId"].as_str().unwrap();
    f.settled(id, None).await;
    let tree = state_query(&f, id, cid, r#"{"view":"call_tree","kind":"stack"}"#).await;
    let aid = tree["analysisId"].as_str().unwrap();
    assert_eq!(tree["matched"], 2);
    assert!(
        tree["rows"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["value"] == 2.0 && r["unit"] == "samples")
    );
    assert_eq!(tree["offCpuInference"], false);
    std::fs::remove_file(&input).unwrap();
    let flame = f
        .call(&[
            "diagnostics",
            "query",
            id,
            aid,
            "--filters",
            r#"{"view":"flamegraph"}"#,
        ])
        .await;
    assert_eq!(flame["matched"], 1);
    assert_eq!(flame["rows"][0]["witnessOrdinals"], json!([0, 1]));
    let timeline = f
        .call(&[
            "diagnostics",
            "query",
            id,
            aid,
            "--filters",
            r#"{"view":"timeline","kind":"stack","clockDomain":"native:simpleperf"}"#,
        ])
        .await;
    assert_eq!(timeline["rows"][0]["record"]["timestamp"], 100.0);
    assert_eq!(timeline["rows"][1]["record"]["timestamp"], 200.0);
    let destination = verify_native_exports(&f, id, aid, original).await;
    recover_unindexed_native(&f, id, &envelope, original).await;
    f.call(&["diagnostics", "close", id]).await;
    f.settled(id, None).await;
    f.call(&["diagnostics", "remove", id]).await;
    assert_eq!(
        std::fs::read(destination).unwrap(),
        b"different caller content"
    );
}

async fn verify_native_exports(
    f: &InvestigationFixture,
    id: &str,
    aid: &str,
    original: &[u8],
) -> std::path::PathBuf {
    let destination = f.0.path().join("original.perf.data");
    let args = [
        "diagnostics",
        "export",
        id,
        aid,
        "--path",
        destination.to_str().unwrap(),
        "--filters",
        r#"{"format":"native_artifact","scope":"all"}"#,
    ];
    let exported = f.call(&args).await;
    assert_eq!(exported["privacy"], "unredacted_native_artifact");
    assert_eq!(std::fs::read(&destination).unwrap(), original);
    assert_eq!(f.call(&args).await["alreadyPresent"], true);
    let refused = f.0.path().join("must-not-export");
    let output = tokio::process::Command::from(f.command())
        .args([
            "diagnostics",
            "export",
            id,
            aid,
            "--path",
            refused.to_str().unwrap(),
            "--filters",
            r#"{"format":"native_artifact"}"#,
        ])
        .output()
        .await
        .unwrap();
    assert!(!output.status.success());
    assert!(!refused.exists());
    std::fs::write(&destination, b"different caller content").unwrap();
    let output = tokio::process::Command::from(f.command())
        .args(args)
        .output()
        .await
        .unwrap();
    assert!(!output.status.success());
    assert_eq!(
        std::fs::read(&destination).unwrap(),
        b"different caller content"
    );
    let default = f.0.path().join("projection.json");
    f.call(&[
        "diagnostics",
        "export",
        id,
        aid,
        "--path",
        default.to_str().unwrap(),
        "--filters",
        r#"{"scope":"all"}"#,
    ])
    .await;
    assert!(
        !std::fs::read_to_string(default)
            .unwrap()
            .contains("libacme")
    );
    destination
}

async fn recover_unindexed_native(
    f: &InvestigationFixture,
    id: &str,
    envelope: &Value,
    original: &[u8],
) {
    // Even an unavailable structured section must not prevent recovery of the
    // independently retained original artifact. Exercise the gzip boundary too.
    let mut unavailable = envelope.clone();
    unavailable["sections"][0]["format"] = json!("unknown-projection");
    let mut compressed = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    compressed
        .write_all(unavailable.to_string().as_bytes())
        .unwrap();
    let compressed = compressed.finish().unwrap();
    let path = f.0.path().join("unindexed.json.gz");
    std::fs::write(&path, compressed).unwrap();
    let source = json!({"kind":"import","path":path,"format":"native-trace-v1"}).to_string();
    let started = f
        .call(&[
            "diagnostics",
            "start",
            id,
            "--submission-id",
            "unindexed",
            "--source",
            &source,
        ])
        .await;
    let cid = started["collectors"].as_array().unwrap().last().unwrap()["collectorId"]
        .as_str()
        .unwrap();
    f.settled(id, None).await;
    let parsed = f
        .call(&[
            "diagnostics",
            "analyze",
            id,
            cid,
            "--submission-id",
            "unindexed",
        ])
        .await;
    let failed = parsed["analysisId"].as_str().unwrap();
    assert_eq!(f.settled(id, Some(failed)).await["phase"], "failed");
    let recovered = f.0.path().join("recovered.native");
    f.call(&[
        "diagnostics",
        "export",
        id,
        failed,
        "--path",
        recovered.to_str().unwrap(),
        "--filters",
        r#"{"format":"native_artifact","scope":"all"}"#,
    ])
    .await;
    assert_eq!(std::fs::read(recovered).unwrap(), original);
}
fn dart_snapshot(position: &str, clock: u64, bytes: u64) -> Value {
    json!({"schemaVersion":1,"kind":"snapshot","targetId":"android:acme","declaredAppId":"com.example.acme","buildId":"build-a","vmStartTime":1,"vmPid":42,"isolateId":"isolates/1","isolateStartTime":2,"rootLibrary":{"id":"libraries/1","uri":"package:acme/main.dart"},"runtimeGeneration":"owned-window-a","actionIds":["action:dart"],"data":{"position":position,"vmMicros":clock,"memory":{"heapUsage":bytes,"heapCapacity":4096,"externalUsage":512},"allocations":{"members":[{"class":{"id":"classes/1","name":"Retained","library":{"uri":"package:acme/main.dart"}},"bytesCurrent":bytes,"instancesCurrent":1,"accumulatedSize":bytes,"instancesAccumulated":1}]}}})
}
#[test]
fn diagnostic_dart_large_class_snapshots_preserve_all_statistics_without_row_amplification() {
    let mut before = dart_snapshot("before", 1000, 1024);
    before["data"]["allocations"]["members"]=json!((0..5000).map(|i|json!({"class":{"id":format!("classes/{i}"),"name":format!("Acme{i}")},"bytesCurrent":1024,"instancesCurrent":2,"accumulatedSize":4096,"instancesAccumulated":8})).collect::<Vec<_>>());
    let mut after = before.clone();
    after["data"]["position"] = json!("after");
    after["data"]["vmMicros"] = json!(2000);
    let parsed = worker(
        "dart-diagnostics-v1",
        format!("{before}\n{after}\n").as_bytes(),
    );
    let rows = parsed["index"]["rows"]
        .as_array()
        .unwrap_or_else(|| panic!("{parsed}"));
    assert_eq!(rows.len(), 10006);
    assert_eq!(
        rows.iter().filter(|r| r["kind"] == "allocation").count(),
        10000
    );
    let class = &rows[3]["data"]["observation"];
    assert_eq!(class["statistics"]["instancesCurrent"], 2);
    assert_eq!(class["statistics"]["accumulatedSize"], 4096);
    assert_eq!(class["statistics"]["instancesAccumulated"], 8);
    assert_eq!(class["units"]["instancesCurrent"], "objects");
}
#[test]
fn diagnostic_dart_real_worker_keeps_cpu_frames_retention_network_and_size_semantics_distinct() {
    let mut packet = dart_snapshot("after", 2000, 2048);
    packet["data"]["observations"] = json!([{"method":"getCpuSamples","result":{"samplePeriod":1000,"timeOriginMicros":1000,"timeExtentMicros":1000,"functions":[{"kind":"Dart","function":{"name":"Acme"}}],"samples":[{"timestamp":1100,"stack":[0]},{"timestamp":1200,"stack":[0]}]}},{"method":"getVMTimeline","result":{"traceEvents":[{"name":"Layout","ph":"X","ts":1100,"dur":1500}]}}]);
    packet["data"]["pendingEvents"] = json!([{"params":{"event":{"kind":"Extension","isolate":{"id":"isolates/1"},"extensionKind":"Flutter.Frame","extensionData":{"build":11000,"raster":7000,"elapsed":20000}}}},{"params":{"event":{"kind":"Logging","isolate":{"id":"isolates/1"},"logRecord":{"level":1000,"stackTrace":{"valueAsString":"async stack"}}}}}]);
    packet["data"]["pendingEvents"].as_array_mut().unwrap().push(json!({"params":{"event":{"kind":"PauseException","isolate":{"id":"isolates/2"},"exception":{"valueAsString":"neighbor failure"}}}}));
    let raw = format!("{packet}\n");
    let parsed = worker("dart-diagnostics-v1", raw.as_bytes());
    let rows = parsed["index"]["rows"]
        .as_array()
        .unwrap_or_else(|| panic!("{parsed}"));
    assert_eq!(rows.iter().filter(|r| r["kind"] == "failure").count(), 1);
    assert!(rows.iter().any(|r| r["name"] == "PauseException"
        && r["kind"] == "event"
        && r["data"]["eventScope"] == "neighbor_isolate_context"));
    assert!(rows.iter().any(|r| r["kind"] == "failure"
        && r["data"]["observation"]["logRecord"]["stackTrace"]["valueAsString"] == "async stack"));
    assert!(
        rows.iter()
            .any(|r| r["unit"] == "samples" && r["value"] == 2.0)
    );
    assert!(rows.iter().any(|r| r["name"] == "flutter_raster_duration"
        && r["unit"] == "microseconds"
        && r["value"] == 7000.0));
    assert!(
        rows.iter()
            .all(|r| r["data"]["appId"].is_null() && r["timestamp"].is_null())
    );
    let clipped = format!("{raw}{{\"schemaVersion\":");
    assert!(
        worker("dart-diagnostics-v1", clipped.as_bytes())["index"]["unsupported"]
            .to_string()
            .contains("clipped final")
    );
    packet["data"]["observations"][0]["result"]["samples"][0]["stack"] = json!([9999]);
    assert_eq!(
        worker("dart-diagnostics-v1", format!("{packet}\n").as_bytes())["error"],
        "INVALID_DIAGNOSTIC_FORMAT"
    );
    let mut object = dart_snapshot("after", 2000, 2048);
    object["kind"] = json!("objects");
    object["data"] = json!({"observations":[{"method":"object_retention","objectId":"objects/1","retainingPath":{"type":"RetainingPath","length":2,"elements":[{"value":{"id":"objects/1"}},{"value":{"id":"objects/2"},"parentField":"cache"}]}},{"method":"getInstances","result":{"availability":"unavailable","reason":"DART_OBJECT_UNAVAILABLE"}}]});
    let parsed = worker("dart-diagnostics-v1", format!("{object}\n").as_bytes());
    assert_eq!(parsed["index"]["rows"][0]["kind"], "retention");
    assert_eq!(
        parsed["index"]["rows"][0]["data"]["observation"]["query"]["retainingPath"]["elements"][1]
            ["parentField"],
        "cache"
    );
    let size = json!({"schemaVersion":1,"buildId":"aot-a","architecture":"arm64","sizeAnalysis":{"n":"Acme.apk","value":4096,"type":"apk","children":[{"n":"assets","value":1024},{"n":"libapp.so","value":3072,"children":[{"n":"package:acme","value":1024}]}]}});
    let parsed = worker("flutter-app-size-v1", size.to_string().as_bytes());
    assert_eq!(parsed["index"]["rows"].as_array().unwrap().len(), 4);
    assert_eq!(
        parsed["index"]["rows"][3]["source"],
        "/sizeAnalysis/children/1/children/0/value"
    );
    assert_eq!(parsed["index"]["rows"][3]["data"]["buildId"], "aot-a");
    assert_eq!(parsed["index"]["rows"][3]["data"]["resolved"], false);
    let mut invalid = size;
    invalid["sizeAnalysis"]["children"][0]["value"] = json!(-1);
    assert_eq!(
        worker("flutter-app-size-v1", invalid.to_string().as_bytes())["error"],
        "INVALID_DIAGNOSTIC_FORMAT"
    );
}
#[tokio::test]
async fn diagnostic_dart_public_import_query_snapshot_comparison_and_templates_are_retained() {
    let f = InvestigationFixture(tempfile::tempdir().unwrap());
    std::fs::hard_link(env!("CARGO_BIN_EXE_testrun"), f.0.path().join("testrun")).unwrap();
    let caps = f.call(&["diagnostics", "capabilities"]).await;
    assert_eq!(
        caps["dartProbe"]["source"],
        include_str!("../probes/dart.dart")
    );
    assert!(!f.0.path().join("state/owner.log").exists());
    let opened = f
        .call(&[
            "diagnostics",
            "open",
            "--submission-id",
            "dart",
            "--question",
            "Acme snapshot changes",
            "--binding",
            r#"{"targetId":"android:acme","appId":"com.example.acme","buildId":"build-a"}"#,
        ])
        .await;
    let id = opened["investigationId"].as_str().unwrap();
    let file = f.0.path().join("dart.jsonl");
    let raw = format!(
        "{}\n{}\n",
        dart_snapshot("before", 1000, 1024),
        dart_snapshot("after", 2000, 2048)
    );
    std::fs::write(&file, raw).unwrap();
    let source = json!({"kind":"import","path":file,"format":"dart-diagnostics-v1"}).to_string();
    let started = f
        .call(&[
            "diagnostics",
            "start",
            id,
            "--submission-id",
            "dart-copy",
            "--source",
            &source,
        ])
        .await;
    let cid = started["collectors"][0]["collectorId"].as_str().unwrap();
    f.settled(id, None).await;
    let query = state_query(&f, id, cid, r#"{"kind":"metric"}"#).await;
    assert!(!query["rows"].as_array().unwrap().is_empty());
    let aid = query["analysisId"].as_str().unwrap();
    let compared = f.call(&["diagnostics", "compare", id, aid, id, aid]).await;
    assert!(
        compared["observedDartSnapshots"]["groups"]
            .as_array()
            .unwrap()
            .iter()
            .any(|g| g["name"] == "bytesCurrent"
                && g["delta"] == 1024.0
                && g["leakEstablished"] == false)
    );
    let before = query.clone();
    std::fs::remove_file(&file).unwrap();
    let replay = f
        .call(&[
            "diagnostics",
            "query",
            id,
            aid,
            "--filters",
            r#"{"kind":"metric"}"#,
        ])
        .await;
    assert_eq!(before, replay);
    verify_dart_symbol_references(&f, id).await;
    f.call(&["diagnostics", "close", id]).await;
}

async fn verify_dart_symbol_references(f: &InvestigationFixture, id: &str) {
    let symbols = f.0.path().join("acme.symbols");
    std::fs::write(
        &symbols,
        b"owned symbol-reference fixture; not an AOT decoder",
    )
    .unwrap();
    let source = json!({"kind":"import","path":symbols,"format":"dart-debug-info"}).to_string();
    let imported = f
        .call(&[
            "diagnostics",
            "start",
            id,
            "--submission-id",
            "symbols",
            "--source",
            &source,
        ])
        .await;
    let symbol_id = imported["collectors"].as_array().unwrap().last().unwrap()["collectorId"]
        .as_str()
        .unwrap()
        .to_owned();
    let status = f.settled(id, None).await;
    let symbol = status["collectors"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["collectorId"] == symbol_id)
        .unwrap();
    let digest = symbol["sha256"].as_str().unwrap();
    let size = f.0.path().join("size.json");
    std::fs::write(&size,json!({"schemaVersion":1,"buildId":"build-a","architecture":"arm64","sizeAnalysis":{"n":"Acme.apk","value":1024,"type":"apk"}}).to_string()).unwrap();
    let source = json!({"kind":"import","path":size,"format":"flutter-app-size-v1"}).to_string();
    let imported = f
        .call(&[
            "diagnostics",
            "start",
            id,
            "--submission-id",
            "size",
            "--source",
            &source,
        ])
        .await;
    let size_id = imported["collectors"].as_array().unwrap().last().unwrap()["collectorId"]
        .as_str()
        .unwrap();
    f.settled(id, None).await;
    for (submission, build, state) in [
        ("matched", "build-a", "reference_matched"),
        ("mismatch", "other-build", "unmatched"),
    ] {
        let metadata=json!({"symbols":[{"kind":"dart","evidenceId":symbol_id,"sha256":digest,"buildId":build,"architecture":"arm64"}]}).to_string();
        let parsed = f
            .call(&[
                "diagnostics",
                "analyze",
                id,
                size_id,
                "--submission-id",
                submission,
                "--metadata",
                &metadata,
            ])
            .await;
        let aid = parsed["analysisId"].as_str().unwrap();
        f.settled(id, Some(aid)).await;
        let rows = f.call(&["diagnostics", "query", id, aid]).await;
        assert_eq!(rows["rows"][0]["symbols"]["state"], state);
        assert_eq!(rows["rows"][0]["symbols"]["resolved"], false);
    }
}
#[test]
fn diagnostic_react_native_worker_keeps_hermes_react_and_native_boundary_evidence_distinct() {
    let base = json!({"schemaVersion":1,"targetId":"android:acme","proxyAppId":"com.example.acme","pageId":"device-1","logicalDeviceId":"device","runtimeGeneration":"socket-a","actionIds":["action:acme"]});
    let mut packets = vec![];
    for (kind, data) in [
        (
            "events",
            json!({"events":[{"method":"Runtime.exceptionThrown","params":{"exceptionDetails":{"stackTrace":{"parent":{"description":"async","callFrames":[]}}}}},{"method":"Debugger.scriptParsed","params":{"scriptId":"1","hash":"bundle-a","sourceMapURL":"acme.map"}},{"method":"Network.requestWillBeSent","params":{"request":{"url":"https://example.com"}}}]}),
        ),
        (
            "profile",
            json!({"mode":"cpu","profile":{"nodes":[{"id":1,"callFrame":{"functionName":"root"},"children":[2]},{"id":2,"callFrame":{"functionName":"Acme"}}],"samples":[2,2,1],"timeDeltas":[10,10,10],"startTime":1,"endTime":31}}),
        ),
        (
            "profile",
            json!({"mode":"heap_sampling","samplingIntervalBytes":32768,"profile":{"head":{"id":1,"selfSize":32768,"callFrame":{"functionName":"Acme"},"children":[]}}}),
        ),
        (
            "probe",
            json!({"hermes":true,"probe":{"schemaVersion":1,"metadata":{"runtimeGeneration":"probe-a","buildMode":"development","clientMode":"expo-go","updateId":"update-a","bundleHash":"bundle-a"},"dropped":3,"events":[{"sequence":1,"kind":"react_commit","name":"Acme","fields":{"actualDuration":2}},{"sequence":2,"kind":"marker","name":"action","actionId":"action:probe","fields":{}}]}}),
        ),
    ] {
        let mut packet = base.clone();
        packet["kind"] = json!(kind);
        packet["data"] = data;
        packets.push(packet);
    }
    packets.push(packets.last().unwrap().clone()); // Duplicate ring snapshot must not double count commits.
    let raw = packets.iter().map(|p| format!("{p}\n")).collect::<String>();
    let result = worker("react-native-diagnostics-v1", raw.as_bytes());
    let rows = result["index"]["rows"]
        .as_array()
        .unwrap_or_else(|| panic!("{result}"));
    assert!(rows.iter().any(|r| r["kind"] == "failure"
        && r["data"]["observation"]["event"]["params"]["exceptionDetails"]["stackTrace"]["parent"]
            ["description"]
            == "async"));
    assert_eq!(rows.iter().filter(|r| r["name"] == "Acme").count(), 1);
    assert!(rows.iter().any(|r| r["value"] == 2.0
        && r["unit"] == "samples"
        && r["source"] == "/lines/1/data/profile/nodes/1"));
    assert!(
        rows.iter()
            .any(|r| r["unit"] == "bytes" && r["source"] == "/lines/2/data/profile/head")
    );
    assert!(
        rows.iter()
            .any(|r| r["unit"] == "samples" && r["value"] == 2.0)
    );
    assert!(rows.iter().any(|r| r["unit"] == "bytes"
        && r["value"] == 32768.0
        && r["definition"].as_str().unwrap().contains("not live heap")));
    assert!(
        rows.iter()
            .all(|r| r["data"]["appId"].is_null() && r["timestamp"].is_null())
    );
    assert!(rows.iter().any(|r| {
        r["correlations"]
            .as_array()
            .unwrap()
            .contains(&json!("action:probe"))
    }));
    let mut clipped = raw.clone();
    clipped.push_str("{\"schemaVersion\":");
    assert!(
        worker("react-native-diagnostics-v1", clipped.as_bytes())["index"]["unsupported"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v.as_str().unwrap().contains("truncated final"))
    );
    let mut malformed = packets[1].clone();
    malformed["data"]["profile"]["nodes"][1]["children"] = json!([1]);
    assert_eq!(
        worker(
            "react-native-diagnostics-v1",
            format!("{malformed}\n").as_bytes()
        )["error"],
        "INVALID_DIAGNOSTIC_FORMAT"
    );
    let mut malformed = packets[2].clone();
    malformed["data"]["profile"]["head"]["selfSize"] = json!(-1);
    assert_eq!(
        worker(
            "react-native-diagnostics-v1",
            format!("{malformed}\n").as_bytes()
        )["error"],
        "INVALID_DIAGNOSTIC_FORMAT"
    );
}
#[tokio::test]
async fn diagnostic_react_native_capabilities_ship_the_optional_probe_without_installation_or_owner_start()
 {
    let f = InvestigationFixture(tempfile::tempdir().unwrap());
    let caps = f.call(&["diagnostics", "capabilities"]).await;
    assert_eq!(
        caps["reactNativeProbe"]["source"],
        include_str!("../probes/react-native.js")
    );
    assert_eq!(caps["reactNativeProbe"]["format"], "commonjs");
    assert!(
        caps["sources"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["kind"] == "react_native" && s["availability"] == "unknown")
    );
    assert_eq!(caps["disabled"]["collectorProcessesStarted"], 0);
    for operation in caps["operations"].as_array().unwrap() {
        assert!(operation["access"]["attach"].is_boolean());
        assert!(operation["access"]["injection"].is_boolean());
        if operation["operation"] == "diagnostics_start" {
            assert_eq!(operation["access"]["attach"], true);
            assert_eq!(operation["access"]["injection"], true);
        }
    }
    assert!(!f.0.path().join("owner.log").exists());
    assert!(!f.0.path().join("commands.jsonl").exists());
}
fn worker_at(executable: &std::path::Path, format: &str, raw: &[u8]) -> Value {
    let mut child = Command::new(executable)
        .arg("diagnostic-parser-worker")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let request = serde_json::to_vec(
        &json!({"format":format,"rawBase64":base64::engine::general_purpose::STANDARD.encode(raw)}),
    )
    .unwrap();
    child.stdin.take().unwrap().write_all(&request).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "parser status: {:?}",
        output.status
    );
    assert!(output.stderr.is_empty());
    serde_json::from_slice(&output.stdout).unwrap()
}
#[test]
fn diagnostic_analysis_real_worker_emits_bounded_index_and_typed_refusals() {
    let result = worker(
        "chrome-trace",
        br#"{"traceEvents":[{"name":"Acme","ph":"X","ts":10,"dur":5}]}"#,
    );
    assert_eq!(result["index"]["rows"][0]["source"], "/traceEvents/0");
    assert_eq!(result["index"]["rows"][0]["value"], 5.0);
    assert_eq!(result["index"]["clocks"][0]["unit"], "microseconds");
    assert_eq!(
        worker("jfr", b"FLR\0")["error"],
        "UNSUPPORTED_DIAGNOSTIC_FORMAT"
    );
    assert_eq!(
        worker("har", b"{\"secret\":\"private-fixture\"}")["error"],
        "INVALID_DIAGNOSTIC_FORMAT"
    );
    assert!(
        !worker("har", b"{\"secret\":\"private-fixture\"}")
            .to_string()
            .contains("private-fixture")
    );
}
#[test]
fn diagnostic_analysis_real_worker_is_killable_while_waiting_for_input() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_testrun"))
        .arg("diagnostic-parser-worker")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    // Keep its stdin open: the worker cannot finish, so termination is real.
    assert!(child.try_wait().unwrap().is_none());
    child.kill().unwrap();
    assert!(!child.wait().unwrap().success());
    assert!(child.try_wait().unwrap().is_some());
}

#[tokio::test]
async fn diagnostic_browser_retained_packet_keeps_native_results_loss_and_private_scope() {
    let f = InvestigationFixture(tempfile::tempdir().unwrap());
    let connection = uuid::Uuid::new_v4();
    let operation = uuid::Uuid::new_v4();
    let context = json!({"connectionId":connection,"id":uuid::Uuid::new_v4(),"generation":uuid::Uuid::new_v4()});
    let packet = json!({"schemaVersion":1,"targetId":format!("browser:{connection}"),"engine":"firefox","backend":"playwright","sessionGeneration":uuid::Uuid::new_v4(),
        "request":{"operation":"diagnostics_read","context":context},"scopes":[{"field":"context","reference":context,"currentlyLive":false}],"actionIds":["action:browser"],
        "operation":{"id":operation,"settled":true,"outcome":"succeeded","result":{"dropped":7,"nextCursor":12,"events":[{"kind":"page_error","sequence":10,"context":context,"data":{"text":"private-browser-error","timestampText":"producer clock"}},{"kind":"console","sequence":11,"data":{"worker":{"connectionId":connection,"id":uuid::Uuid::new_v4(),"generation":uuid::Uuid::new_v4()},"level":"log","text":"private-worker-log"}}]}}});
    let decoded = worker("browser-operation-v1", packet.to_string().as_bytes());
    let rows = decoded["index"]["rows"].as_array().unwrap();
    assert!(rows.iter().any(|r| r["kind"] == "failure"));
    assert!(
        rows.iter()
            .all(|r| r["timestamp"].is_null() && r["data"]["appId"].is_null())
    );
    assert!(rows.iter().any(|r| r["data"]["nativeResult"] == 7));
    let path = f.0.path().join("browser.json");
    std::fs::write(&path, packet.to_string()).unwrap();
    let opened = f
        .call(&[
            "diagnostics",
            "open",
            "--submission-id",
            "browser",
            "--question",
            "Inspect browser",
        ])
        .await;
    let id = opened["investigationId"].as_str().unwrap();
    let started = f
        .call(&[
            "diagnostics",
            "start",
            id,
            "--submission-id",
            "copy",
            "--source",
            &json!({"kind":"import","path":path,"format":"browser-operation-v1"}).to_string(),
        ])
        .await;
    let collector = started["collectors"][0]["collectorId"].as_str().unwrap();
    f.settled(id, None).await;
    let parsed = f
        .call(&[
            "diagnostics",
            "analyze",
            id,
            collector,
            "--submission-id",
            "parse",
        ])
        .await;
    let aid = parsed["analysisId"].as_str().unwrap();
    let status = f.settled(id, Some(aid)).await;
    assert_eq!(status["phase"], "complete", "{status}");
    assert!(!status.to_string().contains("private-browser-error"));
    let result = f
        .call(&[
            "diagnostics",
            "query",
            id,
            aid,
            "--filters",
            r#"{"kind":"failure","correlationId":"action:browser"}"#,
        ])
        .await;
    assert_eq!(result["observationComplete"], false);
    assert_eq!(result["rows"].as_array().unwrap().len(), 1);
    assert!(result.to_string().contains("private-browser-error"));
    assert!(path.exists());
    let mut bad = packet.clone();
    bad["operation"]["settled"] = json!(false);
    assert_eq!(
        worker("browser-operation-v1", bad.to_string().as_bytes())["error"],
        "INVALID_DIAGNOSTIC_FORMAT"
    );
    let mut oversized = packet;
    oversized["operation"]["result"] = json!({"report":"x".repeat(40000)});
    let limited = worker("browser-operation-v1", oversized.to_string().as_bytes());
    assert!(
        limited["index"]["unsupported"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v.as_str().unwrap().starts_with("oversized_native_field:"))
    );
    assert!(limited["index"]["rows"][1]["data"]["nativeResult"].is_null());
}

#[test]
fn diagnostic_android_text_imports_keep_unknown_scope_unresolved_frames_and_actual_signatures() {
    let tombstone = b"*** *** ***\nABI: 'arm64'\npid: 42, tid: 42 >>> com.example.acme <<<\nuid: 10042\nsignal 11 (SIGSEGV)\nbacktrace:\n #00 pc 123 libacme.so";
    let parsed = worker("android-tombstone", tombstone);
    assert_eq!(parsed["index"]["rows"][0]["kind"], "failure", "{parsed}");
    assert_eq!(
        parsed["index"]["rows"][0]["data"]["appId"],
        "com.example.acme"
    );
    assert_eq!(parsed["index"]["rows"][1]["data"]["resolved"], false);
    let unknown = worker("android-tombstone", b"not a tombstone");
    assert_eq!(unknown["index"]["rows"][0]["kind"], "event", "{unknown}");
    let anr=b"----- pid 42 at time -----\nCmd line: com.example.acme\nDALVIK THREADS\n\"main\" prio=5 tid=1 Waiting\n----- pid 43 at time -----\nCmd line: com.example.neighbor\n\"main\" prio=5 tid=1 Waiting";
    let parsed = worker("android-anr", anr);
    assert!(parsed["index"]["rows"][0]["data"]["appId"].is_null());
    assert_eq!(
        parsed["index"]["rows"][1]["data"]["appId"],
        "com.example.acme"
    );
    assert_eq!(
        parsed["index"]["rows"][2]["data"]["appId"],
        "com.example.neighbor"
    );
    let broad = worker("android-bugreport-text", anr);
    assert!(
        broad["index"]["rows"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["data"]["appId"].is_null())
    );
}

#[test]
fn diagnostic_android_classifier_keeps_competing_context_as_candidates() {
    let cases = [
        (
            "AndroidRuntime",
            "FATAL EXCEPTION: main",
            "java_crash_candidate",
        ),
        (
            "libc",
            "Fatal signal 11 (SIGSEGV)",
            "native_signal_candidate",
        ),
        (
            "ActivityManager",
            "ANR in com.example.acme user=0",
            "anr_candidate",
        ),
        (
            "am_kill",
            "Killing com.example.acme low memory",
            "low_memory_kill_candidate",
        ),
        (
            "ActivityManager",
            "Killing com.example.acme user request",
            "system_kill_candidate",
        ),
        (
            "Watchdog",
            "Watchdog observed com.example.acme",
            "watchdog_context",
        ),
        (
            "ActivityManager",
            "Unable to start service com.example.acme",
            "service_start_failure_candidate",
        ),
        (
            "ActivityManager",
            "Background execution not allowed com.example.acme",
            "background_policy_candidate",
        ),
        (
            "PackageManager",
            "package replaced com.example.acme",
            "package_replacement_context",
        ),
    ];
    let entries:Vec<_>=cases.iter().map(|(tag,message,_)|json!({"entry":{"time":"1700000000.123456","uid":10042,"pid":42,"tid":42,"priority":"E","tag":tag,"message":message},"processGeneration":null,"systemContext":false})).collect();
    let packet = json!({"schemaVersion":1,"kind":"logs","appId":"com.example.acme","userId":0,"uid":10042,"data":{"status":"available","complete":false,"entries":entries}});
    let parsed = worker("android-diagnostics-v1", format!("{packet}\n").as_bytes());
    let rows = parsed["index"]["rows"].as_array().unwrap();
    for (index, (_, _, class)) in cases.iter().enumerate() {
        assert_eq!(rows[index]["data"]["classification"], *class, "{parsed}");
        assert_eq!(rows[index]["data"]["causeEstablished"], false);
        assert!(rows[index]["data"]["processGeneration"].is_null());
        assert!(
            rows[index]["source"]
                .as_str()
                .unwrap()
                .starts_with("line:0/data/entries/")
        );
    }
    assert!(
        !parsed["index"]["unsupported"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

fn sha256(bytes: &[u8]) -> String {
    use sha2::Digest;
    format!("{:x}", sha2::Sha256::digest(bytes))
}
fn investigation_dir(f: &InvestigationFixture, id: &str) -> std::path::PathBuf {
    std::fs::read_dir(f.0.path().join("state/diagnostics"))
        .unwrap()
        .map(|entry| entry.unwrap().path().join(id))
        .find(|dir| dir.exists())
        .unwrap()
}
/// Replaces a derived index with `index` and records its digest, as a
/// different parser's publication would.
fn replace_index(dir: &std::path::Path, aid: &str, index: &Value) -> Vec<u8> {
    let bytes = serde_json::to_vec(index).unwrap();
    std::fs::write(dir.join(format!("{aid}.index.json")), &bytes).unwrap();
    let path = dir.join(format!("{aid}.json"));
    let mut receipt: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    receipt["indexSha256"] = json!(sha256(&bytes));
    std::fs::write(path, receipt.to_string()).unwrap();
    bytes
}
async fn refused(f: &InvestigationFixture, args: &[&str]) -> Value {
    let mut command = tokio::process::Command::from(f.command());
    let output = command.args(args).output().await.unwrap();
    assert!(!output.status.success(), "{args:?}");
    serde_json::from_slice::<Value>(&output.stderr).unwrap()["error"].clone()
}
#[tokio::test]
async fn diagnostic_state_earlier_parser_indexes_rebuild_and_future_formats_refuse_unchanged() {
    let f = InvestigationFixture(tempfile::tempdir().unwrap());
    std::fs::hard_link(env!("CARGO_BIN_EXE_testrun"), f.0.path().join("testrun")).unwrap();
    let opened = f
        .call(&[
            "diagnostics",
            "open",
            "--submission-id",
            "upgrade",
            "--question",
            "Read Acme evidence after an upgrade",
        ])
        .await;
    let id = opened["investigationId"].as_str().unwrap();
    let path = f.0.path().join("events.json");
    let metric = |value: f64| json!({"kind":"metric","name":"rss","timestamp":110.0,"clockDomain":"app","value":value,"unit":"bytes","definition":"resident memory","source":"caller-pointer","data":{}});
    let events = json!({"schemaVersion":1,"clocks":[{"id":"app","unit":"milliseconds"}],"records":[metric(64.0),metric(96.0)]});
    std::fs::write(&path, events.to_string()).unwrap();
    let source = json!({"kind":"import","path":path,"format":"testrun-events-v1"}).to_string();
    let captured = f
        .call(&[
            "diagnostics",
            "start",
            id,
            "--submission-id",
            "events",
            "--source",
            &source,
        ])
        .await;
    let cid = captured["collectors"][0]["collectorId"].as_str().unwrap();
    f.settled(id, None).await;
    let baseline = state_query(&f, id, cid, "{}").await;
    let aid = baseline["analysisId"].as_str().unwrap();
    let dir = investigation_dir(&f, id);
    let index_file = dir.join(format!("{aid}.index.json"));
    let current: Value = serde_json::from_slice(&std::fs::read(&index_file).unwrap()).unwrap();
    let query = ["diagnostics", "query", id, aid, "--filters", "{}"];

    verify_projection_pages(&f, id, aid).await;

    // An earlier parser's index is rebuilt from the raw evidence and published.
    let mut earlier = current.clone();
    earlier["parser"] = json!("testrun-diagnostics/0");
    earlier["rows"] = json!([]);
    replace_index(&dir, aid, &earlier);
    let rebuilt = f.call(&query).await;
    assert_eq!(rebuilt["rows"], baseline["rows"]);
    assert_eq!(rebuilt["parser"], "testrun-diagnostics/1");
    let published = std::fs::read(&index_file).unwrap();
    assert_eq!(rebuilt["indexSha256"], sha256(&published));
    assert!(!dir.join(format!("{aid}.next.index.json")).exists());
    assert_eq!(f.call(&query).await["rows"], baseline["rows"]);

    // Concurrent clients each get the rebuilt answer and one index is published.
    replace_index(&dir, aid, &earlier);
    let answers = futures_util::future::join_all((0..4).map(|_| f.call(&query))).await;
    let published = std::fs::read(&index_file).unwrap();
    for answer in &answers {
        assert_eq!(answer["rows"], baseline["rows"]);
        assert_eq!(answer["indexSha256"], sha256(&published), "{answer}");
    }
    assert!(!dir.join(format!("{aid}.next.index.json")).exists());

    // A rebuild that fails leaves both the index and its receipt unchanged.
    let receipt_file = dir.join(format!("{aid}.json"));
    let stale = replace_index(&dir, aid, &earlier);
    let mut receipt: Value =
        serde_json::from_slice(&std::fs::read(&receipt_file).unwrap()).unwrap();
    let format = receipt["format"].clone();
    receipt["format"] = json!("acme-unregistered-format");
    let unparsable = serde_json::to_vec(&receipt).unwrap();
    std::fs::write(&receipt_file, &unparsable).unwrap();
    let error = refused(&f, &query).await;
    assert_eq!(error["code"], "STATE_FORMAT", "{error}");
    assert!(error["details"]["cause"].is_string(), "{error}");
    assert_eq!(std::fs::read(&index_file).unwrap(), stale);
    assert_eq!(std::fs::read(&receipt_file).unwrap(), unparsable);
    receipt["format"] = format;
    std::fs::write(&receipt_file, receipt.to_string()).unwrap();
    assert_eq!(f.call(&query).await["rows"], baseline["rows"]);
    let published = std::fs::read(&index_file).unwrap();

    // A rebuild interrupted after its receipt names the staged index resumes.
    std::fs::write(dir.join(format!("{aid}.next.index.json")), &published).unwrap();
    std::fs::write(&index_file, serde_json::to_vec(&earlier).unwrap()).unwrap();
    assert_eq!(f.call(&query).await["rows"], baseline["rows"]);
    assert_eq!(std::fs::read(&index_file).unwrap(), published);
    assert!(!dir.join(format!("{aid}.next.index.json")).exists());

    // A newer parser's index is refused with its format and left unchanged.
    let mut newer = current.clone();
    newer["parser"] = json!("testrun-diagnostics/2");
    let newer = replace_index(&dir, aid, &newer);
    let error = refused(&f, &query).await;
    assert_eq!(error["code"], "STATE_FORMAT", "{error}");
    assert_eq!(error["details"]["observedParser"], "testrun-diagnostics/2");
    assert_eq!(error["details"]["supportedParser"], "testrun-diagnostics/1");
    assert_eq!(std::fs::read(&index_file).unwrap(), newer);
    f.call(&["manager", "stop"]).await;

    // Without a manager the rebuild answers from memory and writes nothing.
    let earlier = replace_index(&dir, aid, &earlier);
    let receipt = std::fs::read(dir.join(format!("{aid}.json"))).unwrap();
    assert_eq!(f.call(&query).await["rows"], baseline["rows"]);
    assert_eq!(std::fs::read(&index_file).unwrap(), earlier);
    assert_eq!(
        std::fs::read(dir.join(format!("{aid}.json"))).unwrap(),
        receipt
    );

    verify_future_record_formats(&f, id, aid, &dir).await;
    f.call(&["diagnostics", "close", id]).await;
    f.call(&["diagnostics", "remove", id]).await;
    f.call(&["manager", "stop"]).await;
}

async fn verify_projection_pages(f: &InvestigationFixture, id: &str, aid: &str) {
    // Projection pages through the CLI flags and follows nextCursor to its end.
    let page = [
        "diagnostics",
        "project",
        id,
        aid,
        "--filters",
        r#"{"scope":"all"}"#,
        "--limit",
        "1",
    ];
    let first = f.call(&page).await;
    assert_eq!(first["rows"][0]["ordinal"], 0, "{first}");
    let cursor = first["nextCursor"].as_str().unwrap();
    // An agent continues with exactly the arguments the result names.
    let next = &first["nextOperation"];
    assert_eq!(next["tool"], "diagnostics_project", "{first}");
    assert_eq!(
        next["arguments"],
        json!({"investigationId":id,"analysisId":aid,"scope":"all","cursor":cursor,"limit":1})
    );
    let second = f.call(&[&page[..], &["--cursor", cursor]].concat()).await;
    assert_eq!(second["rows"][0]["ordinal"], 1, "{second}");
    assert!(second["nextCursor"].is_null(), "{second}");
    assert!(second.get("nextOperation").is_none(), "{second}");
    let error = refused(
        f,
        &[
            "diagnostics",
            "project",
            id,
            aid,
            "--cursor",
            "not-a-cursor",
        ],
    )
    .await;
    assert_eq!(error["code"], "invalid_cursor", "{error}");
}

async fn verify_future_record_formats(
    f: &InvestigationFixture,
    id: &str,
    aid: &str,
    dir: &std::path::Path,
) {
    // Newer session and receipt formats are refused by name and left unchanged.
    for (file, args) in [
        ("session.json", vec!["diagnostics", "status", id]),
        (
            &format!("{aid}.json") as &str,
            vec!["diagnostics", "analysis-status", id, aid],
        ),
    ] {
        let file = dir.join(file);
        let original = std::fs::read(&file).unwrap();
        let mut record: Value = serde_json::from_slice(&original).unwrap();
        record["version"] = json!(2);
        record["addedByNewerTestrun"] = json!(true);
        let newer = serde_json::to_vec(&record).unwrap();
        std::fs::write(&file, &newer).unwrap();
        let error = refused(f, &args).await;
        assert_eq!(error["code"], "STATE_FORMAT", "{error}");
        assert_eq!(error["details"]["observedVersion"], 2);
        assert_eq!(error["details"]["supportedVersion"], 1);
        assert_eq!(std::fs::read(&file).unwrap(), newer);
        std::fs::write(&file, original).unwrap();
    }
}

#[cfg(unix)]
async fn state_capture(
    f: &InvestigationFixture,
    groups: &[&str],
    follow: bool,
) -> (String, String) {
    let opened = f
        .call(&[
            "diagnostics",
            "open",
            "--submission-id",
            "state-case",
            "--question",
            "Inspect resource observations",
            "--binding",
            r#"{"targetId":"android:ACME","appId":"com.example.acme","actionIds":["action:step"]}"#,
        ])
        .await;
    let id = opened["investigationId"].as_str().unwrap().to_string();
    let plan: Value =
        serde_json::from_slice(&std::fs::read(f.0.path().join("plan.json")).unwrap()).unwrap();
    let source=json!({"kind":"android_state","targetId":"android:ACME","appId":"com.example.acme","userId":0,"groups":groups,"mode":if follow{"follow"}else{"snapshot"},"intervalMs":plan["intervalMs"].as_u64().unwrap_or(500),"maxProcesses":1,"label":"after-step"}).to_string();
    let captured = f
        .call(&[
            "diagnostics",
            "start",
            &id,
            "--submission-id",
            "resources",
            "--source",
            &source,
            "--limits",
            r#"{"durationMs":60000,"maxBytes":8388608}"#,
        ])
        .await;
    (
        id,
        captured["collectors"][0]["collectorId"]
            .as_str()
            .unwrap()
            .to_string(),
    )
}
async fn state_query(f: &InvestigationFixture, id: &str, collector: &str, filters: &str) -> Value {
    let submission = format!("parse-{collector}");
    let parsed = f
        .call(&[
            "diagnostics",
            "analyze",
            id,
            collector,
            "--submission-id",
            &submission,
        ])
        .await;
    let aid = parsed["analysisId"].as_str().unwrap();
    let status = f.settled(id, Some(aid)).await;
    assert_eq!(status["phase"], "complete", "{status}");
    f.call(&["diagnostics", "query", id, aid, "--filters", filters])
        .await
}
#[cfg(unix)]
#[tokio::test]
async fn diagnostic_android_state_snapshot_public_cli_preserves_units_scope_actions_and_read_only_network()
 {
    let f = android_fixture(json!({"stableState":true}));
    let (id, collector) = state_capture(
        &f,
        &[
            "process",
            "memory",
            "scheduling",
            "power",
            "frames",
            "connectivity",
        ],
        false,
    )
    .await;
    let status = f.settled(&id, None).await;
    assert_eq!(status["collectors"][0]["phase"], "partial", "{status}");
    assert_eq!(
        status["collectors"][0]["sourceReport"]["adapter"],
        "android-state-adb/1"
    );
    let metrics = state_query(
        &f,
        &id,
        &collector,
        r#"{"kind":"metric","correlationId":"action:step","limit":100}"#,
    )
    .await;
    assert_android_state_metrics(&metrics);
    assert_eq!(metrics["observationComplete"], false);
    let raw = f
        .call(&["diagnostics", "read", &id, &collector, "--limit", "65536"])
        .await;
    let text = String::from_utf8(
        base64::engine::general_purpose::STANDARD
            .decode(raw["dataBase64"].as_str().unwrap())
            .unwrap(),
    )
    .unwrap();
    assert!(!text.contains("PrivateJob"));
    assert!(!text.contains("private-network"));
    assert!(text.contains("network::android::READBACK"));
    assert!(text.contains("\"connectivity\":\"online\""));
    let transcript = std::fs::read(f.0.path().join("commands.jsonl")).unwrap();
    state_capture(
        &f,
        &[
            "process",
            "memory",
            "scheduling",
            "power",
            "frames",
            "connectivity",
        ],
        false,
    )
    .await;
    assert_eq!(
        std::fs::read(f.0.path().join("commands.jsonl")).unwrap(),
        transcript
    );
    let source=json!({"kind":"android_state","targetId":"android:ACME","appId":"com.example.acme","userId":0,"groups":["process","memory"],"maxProcesses":1,"label":"after-again"}).to_string();
    let after = f
        .call(&[
            "diagnostics",
            "start",
            &id,
            "--submission-id",
            "after",
            "--source",
            &source,
        ])
        .await;
    let collector_after = after["collectors"][1]["collectorId"].as_str().unwrap();
    f.settled(&id, None).await;
    let after = state_query(&f, &id, collector_after, r#"{"kind":"metric"}"#).await;
    let compared = f
        .call(&[
            "diagnostics",
            "compare",
            &id,
            metrics["analysisId"].as_str().unwrap(),
            &id,
            after["analysisId"].as_str().unwrap(),
        ])
        .await;
    let groups = compared["observedAndroidState"]["groups"]
        .as_array()
        .unwrap();
    let rss = groups
        .iter()
        .find(|g| g["name"] == "rss")
        .unwrap_or_else(|| panic!("{compared}"));
    assert_eq!(rss["delta"], 102400.0);
    assert_eq!(rss["leakEstablished"], false);
    assert_eq!(compared["compatible"], false);
}

#[cfg(unix)]
fn assert_android_state_metrics(metrics: &Value) {
    let rows = metrics["rows"].as_array().unwrap();
    for (name, unit, value) in [
        ("rss", "bytes", 1150976.0),
        ("pss", "bytes", 532480.0),
        ("file_descriptors", "count", 4.0),
        ("io_read_bytes", "bytes", 4096.0),
        ("battery_temperature", "celsius", 32.7),
        ("frame_duration", "milliseconds", 16.0),
    ] {
        let row = &rows
            .iter()
            .find(|r| r["record"]["name"] == name)
            .unwrap_or_else(|| panic!("missing {name}: {metrics}"))["record"];
        assert_eq!(row["unit"], unit);
        assert_eq!(row["value"], value);
        assert!(!row["definition"].as_str().unwrap().is_empty());
    }
}
#[cfg(unix)]
#[tokio::test]
async fn diagnostic_android_state_follow_reports_cpu_windows_and_deduplicates_frame_history() {
    let f = android_fixture(json!({"stableState":true}));
    let (id, collector) = state_capture(&f, &["process", "memory", "frames"], true).await;
    tokio::time::timeout(std::time::Duration::from_secs(30), async {
        loop {
            let status = f.call(&["diagnostics", "status", &id]).await;
            if status["collectors"][0]["sourceReport"]["cycles"]
                .as_u64()
                .unwrap_or(0)
                >= 2
            {
                break;
            }
            assert_ne!(status["settled"], true, "{status}");
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap();
    f.call(&["diagnostics", "stop", &id, "--collector-id", &collector])
        .await;
    f.settled(&id, None).await;
    let metrics = state_query(&f, &id, &collector, r#"{"kind":"metric","limit":100}"#).await;
    let rows = metrics["rows"].as_array().unwrap();
    let cpu = &rows
        .iter()
        .find(|r| r["record"]["name"] == "cpu_utilization")
        .unwrap()["record"];
    assert_eq!(cpu["value"], 6.0);
    assert_eq!(cpu["data"]["measurement"]["intervalSeconds"], 10.0);
    assert_eq!(
        rows.iter()
            .filter(|r| r["record"]["name"] == "frame_duration" && r["record"]["value"] == 16.0)
            .count(),
        1
    );
}
#[cfg(unix)]
#[tokio::test]
async fn diagnostic_android_state_denied_reads_and_mid_sample_pid_reuse_never_become_zero_measurements()
 {
    for plan in [json!({"stateDenied":true}), json!({"midReadReuse":true})] {
        let f = android_fixture(plan.clone());
        let (id, collector) = state_capture(&f, &["memory"], false).await;
        let status = f.settled(&id, None).await;
        assert_eq!(status["collectors"][0]["phase"], "partial", "{status}");
        let metrics = state_query(&f, &id, &collector, r#"{"kind":"metric"}"#).await;
        assert!(metrics["rows"].as_array().unwrap().is_empty(), "{metrics}");
        assert_eq!(metrics["observationComplete"], false);
        let raw = f
            .call(&["diagnostics", "read", &id, &collector, "--limit", "65536"])
            .await;
        let text = String::from_utf8(
            base64::engine::general_purpose::STANDARD
                .decode(raw["dataBase64"].as_str().unwrap())
                .unwrap(),
        )
        .unwrap();
        assert!(text.contains(if plan["stateDenied"] == true {
            "permission_denied"
        } else {
            "ANDROID_PROCESS_CHANGED"
        }));
    }
}

#[cfg(unix)]
#[tokio::test]
async fn diagnostic_android_state_stop_reaps_hung_reads_and_interrupts_long_sampling_waits() {
    for plan in [json!({"stateHang":true}), json!({"intervalMs":60000})] {
        let f = android_fixture(plan.clone());
        let (id, collector) = state_capture(&f, &["memory"], plan["stateHang"] != true).await;
        tokio::time::timeout(std::time::Duration::from_secs(15), async {
            loop {
                let ready = if plan["stateHang"] == true {
                    f.0.path().join("hanging-pid").exists()
                } else {
                    let text = std::fs::read_to_string(f.0.path().join("commands.jsonl"))
                        .unwrap_or_default();
                    text.lines()
                        .filter(|l| l.contains("dumpsys") && l.contains("package"))
                        .count()
                        >= 2
                };
                if ready {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(8), async {
            f.call(&["diagnostics", "stop", &id, "--collector-id", &collector])
                .await;
            let status = f.settled(&id, None).await;
            assert_eq!(status["collectors"][0]["reason"], "stopped", "{status}");
        })
        .await
        .unwrap();
        if plan["stateHang"] == true {
            let pid: i32 = std::fs::read_to_string(f.0.path().join("hanging-pid"))
                .unwrap()
                .parse()
                .unwrap();
            assert_eq!(unsafe { libc::kill(pid, 0) }, -1);
        }
    }
}

#[test]
fn diagnostic_android_state_correlation_expansion_is_bounded_before_further_packets() {
    let metric =
        json!({"name":"rss","value":1,"unit":"bytes","definition":"observed resident bytes"});
    let actions = vec!["a".repeat(256); 32];
    let packet = json!({"schemaVersion":1,"kind":"state","appId":"com.example.acme","actionIds":actions,"data":{"status":"sampled","group":"process","metrics":vec![metric;512]}});
    let input = format!("{packet}\n").repeat(4);
    assert_eq!(
        worker("android-diagnostics-v1", input.as_bytes())["error"],
        "ANALYSIS_BOUNDS"
    );
}

#[cfg(unix)]
fn apple_fixture(plan: Value) -> InvestigationFixture {
    use std::os::unix::fs::PermissionsExt;
    let f = InvestigationFixture(tempfile::tempdir().unwrap());
    // Cargo replaces the shared output when another checkout/build finishes.
    // Keep one inode for this fixture's CLI, manager and nested parser lifetime.
    let binary = f.0.path().join("testrun");
    std::fs::hard_link(env!("CARGO_BIN_EXE_testrun"), &binary)
        .or_else(|_| std::fs::copy(env!("CARGO_BIN_EXE_testrun"), &binary).map(|_| ()))
        .unwrap();
    std::fs::write(f.0.path().join("apple-plan.json"), plan.to_string()).unwrap();
    for name in ["xcrun", "codesign", "apple-ps"] {
        let p = f.0.path().join(name);
        std::fs::write(&p, include_bytes!("fixtures/diagnostic-apple.mjs")).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    // The one-second sample should measure collection rather than Node startup.
    let syslog = f.0.path().join("syslog");
    std::fs::write(
        &syslog,
        include_bytes!("fixtures/diagnostic-apple-syslog.sh"),
    )
    .unwrap();
    std::fs::set_permissions(syslog, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(
        f.0.path().join("apple-syslog-plan"),
        format!(
            "{} {} {}\n",
            plan["network"] == true,
            plan["noSupport"] == true,
            plan["serviceDenied"] == true
        ),
    )
    .unwrap();
    let app = f.0.path().join("Acme.app");
    std::fs::create_dir(&app).unwrap();
    std::fs::write(app.join("Info.plist"),br#"<?xml version="1.0"?><plist version="1.0"><dict><key>CFBundleIdentifier</key><string>com.example.acme</string><key>CFBundleExecutable</key><string>Acme</string><key>CFBundleVersion</key><string>42</string><key>CFBundleShortVersionString</key><string>1.0</string><key>UIBackgroundModes</key><array><string>audio</string></array></dict></plist>"#).unwrap();
    std::fs::write(app.join("Acme"), b"owned fixture").unwrap();
    f
}
fn apple_ips() -> Vec<u8> {
    format!("{}\n{}",json!({"bug_type":"309","bundleID":"com.example.acme","timestamp":"2026-11-07 10:00:00 +0000"}),json!({"pid":42,"captureTime":"2026-11-07 10:00:00 +0000","bundleInfo":{"CFBundleIdentifier":"com.example.acme","CFBundleVersion":"42"},"exception":{"type":"EXC_BAD_ACCESS"},"termination":{"namespace":"FRONTBOARD","code":"0x8badf00d"},"usedImages":[{"uuid":"AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE","arch":"arm64","base":4096,"size":4096,"name":"Acme"}],"threads":[{"id":1,"triggered":true,"frames":[{"imageIndex":0,"imageOffset":16}]}]})).into_bytes()
}
#[test]
fn diagnostic_apple_report_worker_preserves_historical_scope_clocks_and_image_addresses() {
    let parsed = worker("apple-ips", &apple_ips());
    let rows = parsed["index"]["rows"].as_array().unwrap();
    assert_eq!(
        rows[0]["data"]["classification"], "watchdog_candidate",
        "{parsed}"
    );
    assert!(rows[0]["timestamp"].is_null());
    assert_eq!(rows[2]["data"]["address"], "0x1010");
    assert_eq!(rows[2]["data"]["resolved"], false);
    assert!(rows[2]["data"]["processGeneration"].is_null());
    let jetsam=worker("apple-jetsam",br#"{"date":"2026-11-07","pageSize":16384,"processes":[{"name":"Acme","pid":42,"reason":"per-process-limit","rpages":20},{"name":"Other","pid":99,"rpages":3}]}"#);
    assert_eq!(jetsam["index"]["rows"][0]["kind"], "failure");
    assert_eq!(jetsam["index"]["rows"][1]["kind"], "process");
    assert!(jetsam["index"]["rows"][0]["data"]["appId"].is_null());
    let metrics=worker("apple-metrickit",br#"{"timeStampBegin":"2026-11-06T00:00:00Z","timeStampEnd":"2026-11-07T00:00:00Z","cpuMetrics":{"cumulativeCPUTime":"12 sec"},"hangDiagnostics":[{"hangDuration":"2 sec"}]}"#);
    assert_eq!(
        metrics["index"]["rows"][0]["data"]["window"]["begin"],
        "2026-11-06T00:00:00Z"
    );
    assert!(metrics["index"]["rows"][0]["value"].is_null());
    let text=worker("apple-crash-text",b"Identifier: com.example.acme\nException Type: EXC_BAD_ACCESS\n0 Acme 0x1010 0x1000 + 16\nBinary Images:\n0x1000 - 0x1fff Acme arm64 <AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE> /Acme.app/Acme");
    assert_eq!(text["index"]["rows"][1]["data"]["loadAddress"], "0x1000");
    assert_eq!(
        worker("apple-ips", b"{\"unrelated\":true}")["error"],
        "INVALID_DIAGNOSTIC_FORMAT"
    );
}
#[cfg(unix)]
async fn apple_capture(
    f: &InvestigationFixture,
    sim: bool,
    network: bool,
    reports: bool,
) -> (String, String) {
    let target = if sim { "simulator:ACME" } else { "device:ACME" };
    let binding =
        json!({"targetId":target,"appId":"com.example.acme","actionIds":["action:apple"]})
            .to_string();
    let opened = f
        .call(&[
            "diagnostics",
            "open",
            "--submission-id",
            "apple",
            "--question",
            "Inspect Apple failure",
            "--binding",
            &binding,
        ])
        .await;
    let id = opened["investigationId"].as_str().unwrap().to_string();
    let mut source = json!({"kind":"ios","targetId":target,"appId":"com.example.acme","transport":if network{"network"}else{"usb"},"logSeconds":1});
    if reports {
        let dir = f.0.path().join("reports");
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("Acme.ips"), apple_ips()).unwrap();
        source["reportDirectory"] = json!(dir);
    }
    let started = f
        .call(&[
            "diagnostics",
            "start",
            &id,
            "--submission-id",
            "native",
            "--source",
            &source.to_string(),
        ])
        .await;
    (
        id,
        started["collectors"][0]["collectorId"]
            .as_str()
            .unwrap()
            .to_string(),
    )
}
#[cfg(unix)]
#[tokio::test]
async fn diagnostic_apple_simulator_and_physical_routes_keep_logs_reports_and_private_scope_separate()
 {
    for (sim, network) in [(true, false), (false, false), (false, true)] {
        let f = apple_fixture(json!({"network":network}));
        let (id, collector) = apple_capture(&f, sim, network, true).await;
        let status = f.settled(&id, None).await;
        assert_eq!(status["collectors"][0]["phase"], "partial", "{status}");
        assert!(!status.to_string().contains("private-token"));
        let raw = f
            .call(&["diagnostics", "read", &id, &collector, "--limit", "65536"])
            .await;
        let text = String::from_utf8(
            base64::engine::general_purpose::STANDARD
                .decode(raw["dataBase64"].as_str().unwrap())
                .unwrap(),
        )
        .unwrap();
        assert!(text.contains("private-token"), "{text}");
        assert!(!text.contains("private-neighbor"));
        if sim {
            assert!(text.contains("backgroundModes"));
            assert!(text.contains("get-task-allow"));
        } else {
            assert!(text.contains("host counters are not device observations"));
        }
        let decoded = worker_at(
            &f.0.path().join("testrun"),
            "apple-diagnostics-v1",
            text.as_bytes(),
        );
        assert!(
            decoded["index"]["rows"].is_array(),
            "{sim}/{network}: {decoded}"
        );
        let parsed = f
            .call(&[
                "diagnostics",
                "analyze",
                &id,
                &collector,
                "--submission-id",
                "parse",
            ])
            .await;
        let aid = parsed["analysisId"].as_str().unwrap();
        let result = f.settled(&id, Some(aid)).await;
        assert_eq!(result["phase"], "complete", "{sim}/{network}: {result}");
        let failures = f
            .call(&[
                "diagnostics",
                "query",
                &id,
                aid,
                "--filters",
                r#"{"kind":"failure","correlationId":"action:apple"}"#,
            ])
            .await;
        assert!(!failures["rows"].as_array().unwrap().is_empty());
        assert_eq!(failures["observationComplete"], false);
        assert!(f.0.path().join("reports/Acme.ips").exists());
    }
}
#[cfg(unix)]
#[tokio::test]
async fn diagnostic_apple_locked_untrusted_missing_support_and_wrong_transport_are_explicit() {
    for (plan, reason) in [
        (json!({"locked":true}), "APPLE_DEVICE_LOCKED"),
        (json!({"untrusted":true}), "APPLE_DEVICE_UNTRUSTED"),
        (json!({"network":true}), "APPLE_TRANSPORT_UNAVAILABLE"),
        (json!({"noSupport":true}), "developer_support_unavailable"),
    ] {
        let f = apple_fixture(plan);
        let (id, _) = apple_capture(&f, false, false, false).await;
        let status = f.settled(&id, None).await;
        assert!(status.to_string().contains(reason), "{status}");
        assert_ne!(status["collectors"][0]["phase"], "complete");
    }
}
/// Makes one fake host tool unstartable, as an uninstalled tool would be.
#[cfg(unix)]
fn disable_tool(f: &InvestigationFixture, name: &str) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(
        f.0.path().join(name),
        std::fs::Permissions::from_mode(0o644),
    )
    .unwrap();
}
#[cfg(unix)]
#[tokio::test]
async fn diagnostic_apple_missing_optional_tools_leave_other_sections_and_analysis_usable() {
    for (sim, tool, gap) in [
        (false, "syslog", "device_log_dependency"),
        (true, "codesign", "entitlements"),
    ] {
        let f = apple_fixture(json!({}));
        disable_tool(&f, tool);
        let (id, collector) = apple_capture(&f, sim, false, true).await;
        let status = f.settled(&id, None).await;
        assert_eq!(status["collectors"][0]["phase"], "partial", "{status}");
        assert!(status.to_string().contains(gap), "{tool}: {status}");
        let raw = f
            .call(&["diagnostics", "read", &id, &collector, "--limit", "65536"])
            .await;
        let text = String::from_utf8(
            base64::engine::general_purpose::STANDARD
                .decode(raw["dataBase64"].as_str().unwrap())
                .unwrap(),
        )
        .unwrap();
        if sim {
            assert!(text.contains("backgroundModes"), "{text}");
            assert!(!text.contains("get-task-allow"));
        } else {
            assert!(text.contains("host counters are not device observations"));
        }
        let parsed = f
            .call(&[
                "diagnostics",
                "analyze",
                &id,
                &collector,
                "--submission-id",
                "parse",
            ])
            .await;
        let aid = parsed["analysisId"].as_str().unwrap();
        let result = f.settled(&id, Some(aid)).await;
        assert_eq!(result["phase"], "complete", "{tool}: {result}");
    }
}
#[cfg(unix)]
#[tokio::test]
async fn diagnostic_apple_retained_dsym_uuid_and_architecture_gate_symbol_tool_dispatch() {
    for (mismatch, architecture, missing_tool) in [
        (false, "arm64", false),
        (true, "arm64", false),
        (false, "arm64e", false),
        (false, "arm64", true),
    ] {
        let f = apple_fixture(json!({}));
        let path = f.0.path().join("crash.ips");
        std::fs::write(&path, apple_ips()).unwrap();
        let symbol = f.0.path().join("Acme.dwarf");
        std::fs::write(
            &symbol,
            if mismatch {
                b"mismatch".as_slice()
            } else {
                b"matching".as_slice()
            },
        )
        .unwrap();
        let opened = f
            .call(&[
                "diagnostics",
                "open",
                "--submission-id",
                "symbols",
                "--question",
                "Symbolicate exact image",
            ])
            .await;
        let id = opened["investigationId"].as_str().unwrap();
        let mut ids = Vec::new();
        for (submission, path, format) in
            [("crash", &path, "apple-ips"), ("dwarf", &symbol, "mach-o")]
        {
            let source = json!({"kind":"import","path":path,"format":format}).to_string();
            let capture = f
                .call(&[
                    "diagnostics",
                    "start",
                    id,
                    "--submission-id",
                    submission,
                    "--source",
                    &source,
                ])
                .await;
            ids.push(
                capture["collectors"].as_array().unwrap().last().unwrap()["collectorId"]
                    .as_str()
                    .unwrap()
                    .to_string(),
            );
            f.settled(id, None).await;
        }
        let symbol_status = f.call(&["diagnostics", "status", id]).await;
        let sha = symbol_status["collectors"][1]["sha256"].as_str().unwrap();
        let metadata=json!({"symbolicate":true,"symbols":[{"kind":"dsym","evidenceId":ids[1],"sha256":sha,"buildId":"AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE","architecture":architecture}]}).to_string();
        if missing_tool {
            disable_tool(&f, "xcrun");
        }
        let parsed = f
            .call(&[
                "diagnostics",
                "analyze",
                id,
                &ids[0],
                "--submission-id",
                "symbolicate",
                "--metadata",
                &metadata,
            ])
            .await;
        let aid = parsed["analysisId"].as_str().unwrap();
        let state = f.settled(id, Some(aid)).await;
        assert_eq!(state["phase"], "complete", "{state}");
        let query = f
            .call(&[
                "diagnostics",
                "query",
                id,
                aid,
                "--filters",
                r#"{"kind":"stack"}"#,
            ])
            .await;
        assert_symbol_tool_dispatch(&f, &query, &state, mismatch, architecture, missing_tool);
        assert!(symbol.exists());
    }
}

#[cfg(unix)]
fn assert_symbol_tool_dispatch(
    f: &InvestigationFixture,
    query: &Value,
    state: &Value,
    mismatch: bool,
    architecture: &str,
    missing_tool: bool,
) {
    let dispatched = !mismatch && architecture == "arm64";
    assert_eq!(
        query["rows"][0]["symbols"]["resolved"],
        dispatched && !missing_tool,
        "{query}"
    );
    // The imports wrote no commands; a missing tool records none either.
    let commands =
        std::fs::read_to_string(f.0.path().join("apple-commands.jsonl")).unwrap_or_default();
    assert_eq!(commands.contains("atos"), dispatched && !missing_tool);
    if missing_tool {
        assert!(
            state
                .to_string()
                .contains("apple_symbol_uuid_tool_unavailable_or_failed"),
            "{state}"
        );
    }
}

#[cfg(unix)]
#[tokio::test]
async fn diagnostic_apple_stopping_stream_reaps_only_the_owned_client() {
    let f = apple_fixture(json!({}));
    let (id, collector) = apple_capture(&f, false, false, false).await;
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        while !f.0.path().join("apple-hanging-pid").exists() {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let pid: i32 = std::fs::read_to_string(f.0.path().join("apple-hanging-pid"))
        .unwrap()
        .parse()
        .unwrap();
    f.call(&["diagnostics", "stop", &id, "--collector-id", &collector])
        .await;
    let status = f.settled(&id, None).await;
    assert_eq!(status["collectors"][0]["reason"], "stopped", "{status}");
    assert_eq!(unsafe { libc::kill(pid, 0) }, -1);
}

struct InvestigationFixture(tempfile::TempDir);
#[tokio::test]
#[ignore = "Run with --ignored and an explicitly selected installed TESTRUN_DIAGNOSTIC_NODE"]
async fn diagnostic_verification_generated_node_corpus() {
    let node = std::env::var_os("TESTRUN_DIAGNOSTIC_NODE")
        .expect("Set TESTRUN_DIAGNOSTIC_NODE to an absolute Node executable");
    assert!(std::path::Path::new(&node).is_absolute());
    let f = InvestigationFixture(tempfile::tempdir().unwrap());
    std::fs::hard_link(env!("CARGO_BIN_EXE_testrun"), f.0.path().join("testrun")).unwrap();
    let corpus = f.0.path().join("generated-corpus");
    generate_node_corpus(node, &corpus).await;
    let opened = f
        .call(&[
            "diagnostics",
            "open",
            "--submission-id",
            "corpus",
            "--question",
            "Verify actual Acme retained-buffer correction",
        ])
        .await;
    let id = opened["investigationId"].as_str().unwrap();
    let mut owned_analyses = Vec::new();
    use futures_util::FutureExt;
    let outcome=std::panic::AssertUnwindSafe(async {
    for (name, format) in [
        ("profile.cpuprofile", "js-cpu-profile"),
        ("defective.json", "testrun-events-v1"),
        ("corrected.json", "testrun-events-v1"),
        ("insufficient.json", "testrun-events-v1"),
    ] {
        let path = corpus.join(name);
        let source = json!({"kind":"import","path":path,"format":format}).to_string();
        let capture = f
            .call(&[
                "diagnostics",
                "start",
                id,
                "--submission-id",
                name,
                "--source",
                &source,
            ])
            .await;
        let cid = capture["collectors"].as_array().unwrap().last().unwrap()["collectorId"]
            .as_str()
            .unwrap();
        f.settled(id, None).await;
        std::fs::remove_file(path).unwrap();
        let metadata=json!({"observationComplete":format=="testrun-events-v1"&&name!="insufficient.json"}).to_string();
        let parsed=f.call(&["diagnostics","analyze",id,cid,"--submission-id",&format!("parse-{name}"),"--metadata",&metadata]).await;
        let aid=parsed["analysisId"].as_str().unwrap();owned_analyses.push(aid.to_owned());let settled=f.settled(id,Some(aid)).await;assert_eq!(settled["phase"],"complete","{settled}");
        if format == "js-cpu-profile" {
            let rows = f.call(&["diagnostics","query",id,aid,"--filters",r#"{"kind":"stack","limit":20}"#]).await;
            assert!(!rows["rows"].as_array().unwrap().is_empty());
            assert_eq!(rows["rows"][0]["record"]["unit"], "samples");
        } else {
            let result=f.call(&["diagnostics","query",id,aid,"--filters",r#"{"view":"assertions","assertions":{"checks":[{"id":"released","domain":"performance","name":"Acme retained buffers","predicate":{"type":"metric","unit":"bytes","definition":"Acme fixture-owned Uint8Array byte lengths; not a heap census","max":0,"aggregation":"max"}}]}}"#]).await;
            assert_eq!(
                result["outcome"],
                match name {
                    "defective.json" => "assertion_failed",
                    "corrected.json" => "passed",
                    _ => "unavailable",
                },
                "{result}"
            );
            let analysis = result["analysisId"].as_str().unwrap();
            let replay = f
                .report_mcp(
                    "diagnostics_query",
                    json!({"investigationId":id,"analysisId":analysis,"view":"report","report":{}}),
                )
                .await;
            assert_eq!(replay["privacy"], "minimal");
            assert_eq!(replay["sources"][0]["rawSha256"], result["rawSha256"]);
        }
    }
    }).catch_unwind().await;
    for aid in &owned_analyses {
        f.call(&["diagnostics", "analysis-cancel", id, aid]).await;
        f.settled(id, Some(aid)).await;
    }
    f.call(&["diagnostics", "close", id]).await;
    f.call(&["diagnostics", "remove", id]).await;
    f.call(&["manager", "stop"]).await;
    assert_eq!(retained_bytes(&f.0.path().join("state/diagnostics")), 0);
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
}

async fn generate_node_corpus(node: std::ffi::OsString, corpus: &std::path::Path) {
    let mut command = tokio::process::Command::new(node);
    command
        .arg(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/diagnostics/generate.mjs"),
        )
        .arg(corpus)
        .kill_on_drop(true);
    let output = tokio::time::timeout(std::time::Duration::from_secs(15), command.output())
        .await
        .unwrap()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let producer: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(producer["runtime"], "node");
    assert!(producer["version"].as_str().unwrap().starts_with('v'));
}
#[tokio::test]
async fn diagnostic_reports_public_cli_mcp_compose_witnesses_and_keep_exports_after_removal() {
    let f = InvestigationFixture(tempfile::tempdir().unwrap());
    std::fs::hard_link(env!("CARGO_BIN_EXE_testrun"), f.0.path().join("testrun")).unwrap();
    let caps = f.call(&["diagnostics", "capabilities"]).await;
    assert_eq!(caps["workflows"]["limits"]["selectedAnalyses"], 4);
    assert!(!f.0.path().join("state/owner.log").exists());
    let opened = f
        .call(&[
            "diagnostics",
            "open",
            "--submission-id",
            "report",
            "--question",
            "Acme private-question checkout correction",
        ])
        .await;
    let id = opened["investigationId"].as_str().unwrap();
    let analyses = report_analyses(&f, id).await;
    let filters = json!({"view":"investigation","investigation":{"analysisIds":[analyses[1]],"operation":"timeline"},"limit":1});
    let page = f
        .call(&[
            "diagnostics",
            "query",
            id,
            &analyses[0],
            "--filters",
            &filters.to_string(),
        ])
        .await;
    assert!(page["nextCursor"].is_string());
    assert_eq!(page["rows"][0]["evidence"]["analysisId"], analyses[0]);
    assert_eq!(page["rows"][0]["record"]["timestamp"], 100.0);
    let next = f
        .report_mcp(
            "diagnostics_query",
            page["nextOperation"]["arguments"].clone(),
        )
        .await;
    assert_eq!(next["rows"][0]["evidence"]["analysisId"], analyses[1]);
    assert_eq!(next["rows"][0]["record"]["timestamp"], 1.0);
    let mut notes = json!({"analysisIds":[analyses[1]],"exactBuild":{"id":"private-build","architecture":"arm64","mode":"release"},"reproduction":["private-reproduction"],"facts":[{"statement":"private-fact","witnesses":[{"analysisId":analyses[0],"ordinal":0}]}],"hypotheses":[{"statement":"private-hypothesis"}],"correction":"private-correction","comparison":{"baselineAnalysisId":analyses[0],"correctedAnalysisId":analyses[1]},"unresolved":["private-unresolved"]});
    let args =
        json!({"investigationId":id,"analysisId":analyses[0],"view":"report","report":notes});
    let cli = f
        .call(&[
            "diagnostics",
            "query",
            id,
            &analyses[0],
            "--filters",
            &json!({"view":"report","report":notes}).to_string(),
        ])
        .await;
    assert!(!cli.to_string().contains("private-"));
    let mcp = f.report_mcp("diagnostics_query", args).await;
    assert_eq!(mcp, cli);
    notes["privacy"] = json!("private");
    let private = f
        .call(&[
            "diagnostics",
            "query",
            id,
            &analyses[0],
            "--filters",
            &json!({"view":"report","report":notes}).to_string(),
        ])
        .await;
    assert!(
        private["question"]
            .as_str()
            .unwrap()
            .contains("private-question")
    );
    assert_eq!(private["facts"][0]["witnesses"][0]["ordinal"], 0);
    assert_eq!(private["comparison"]["compatible"], false);
    assert!(!private.to_string().contains("private-body"));
    let (path, markdown) = durable_report_exports(&f, id, &analyses).await;
    let mut bad = notes.clone();
    bad["facts"][0]["witnesses"][0]["ordinal"] = json!(999);
    let error = f
        .report_mcp_error(
            "diagnostics_query",
            json!({"investigationId":id,"analysisId":analyses[0],"view":"report","report":bad}),
        )
        .await;
    assert_eq!(error["isError"], true);
    let conflict = f.0.path().join("conflict.json");
    std::fs::write(&conflict, b"preserve").unwrap();
    let error=f.report_mcp_error("diagnostics_export",json!({"investigationId":id,"analysisId":analyses[0],"scope":"all","format":"investigation-json","report":{},"path":conflict})).await;
    assert_eq!(error["isError"], true);
    assert_eq!(std::fs::read(&conflict).unwrap(), b"preserve");
    f.call(&["diagnostics", "close", id]).await;
    f.call(&["diagnostics", "remove", id]).await;
    f.call(&["manager", "stop"]).await;
    assert!(path.exists());
    assert!(markdown.exists());
    let stored: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(stored["projection"], "investigation-report-v1");
    assert!(!stored.to_string().contains("private-"));
}

async fn report_analyses(f: &InvestigationFixture, id: &str) -> Vec<String> {
    let mut analyses = vec![];
    for (n, state) in ["failed", "ready"].into_iter().enumerate() {
        let packet = json!({"schemaVersion":1,"clocks":[{"id":"clock","unit":"milliseconds"}],"records":[{"kind":if n==0{"failure"}else{"event"},"name":"checkout","timestamp":if n==0{100}else{1},"clockDomain":"clock","data":{"state":state,"customer":"private-body"}}]});
        let path = f.0.path().join(format!("report-source-{n}.json"));
        std::fs::write(&path, packet.to_string()).unwrap();
        let source = json!({"kind":"import","path":path,"format":"testrun-events-v1"}).to_string();
        let capture = f
            .call(&[
                "diagnostics",
                "start",
                id,
                "--submission-id",
                &format!("report-{n}"),
                "--source",
                &source,
            ])
            .await;
        let cid = capture["collectors"].as_array().unwrap().last().unwrap()["collectorId"]
            .as_str()
            .unwrap();
        let status = f.settled(id, None).await;
        assert_eq!(
            status["collectors"].as_array().unwrap().last().unwrap()["nextOperation"]["tool"],
            "diagnostics_analyze"
        );
        std::fs::remove_file(path).unwrap();
        let result=state_query(f,id,cid,r#"{"view":"assertions","assertions":{"checks":[{"id":"ready","domain":"api","name":"checkout","predicate":{"type":"equals","pointer":"/state","expected":"ready"}}]}}"#).await;
        assert_eq!(
            result["outcome"],
            if n == 0 { "assertion_failed" } else { "passed" }
        );
        analyses.push(result["analysisId"].as_str().unwrap().to_owned());
    }
    analyses
}

async fn durable_report_exports(
    f: &InvestigationFixture,
    id: &str,
    analyses: &[String],
) -> (std::path::PathBuf, std::path::PathBuf) {
    let path = f.0.path().join("durable-report.json");
    let export =
        json!({"scope":"all","format":"investigation-json","report":{"analysisIds":[analyses[1]]}})
            .to_string();
    let first = f
        .call(&[
            "diagnostics",
            "export",
            id,
            &analyses[0],
            "--path",
            path.to_str().unwrap(),
            "--filters",
            &export,
        ])
        .await;
    assert_eq!(first["alreadyPresent"], false);
    let replay = f
        .call(&[
            "diagnostics",
            "export",
            id,
            &analyses[0],
            "--path",
            path.to_str().unwrap(),
            "--filters",
            &export,
        ])
        .await;
    assert_eq!(replay["alreadyPresent"], true);
    assert_eq!(first["sha256"], replay["sha256"]);
    let markdown = f.0.path().join("durable-report.md");
    let receipt=f.report_mcp("diagnostics_export",json!({"investigationId":id,"analysisId":analyses[0],"scope":"all","format":"investigation-markdown","report":{"analysisIds":[analyses[1]]},"path":markdown})).await;
    assert_eq!(receipt["exported"], true);
    assert!(
        std::fs::read_to_string(&markdown)
            .unwrap()
            .starts_with("# Investigation report")
    );
    (path, markdown)
}
impl InvestigationFixture {
    async fn report_mcp_error(&self, name: &str, args: Value) -> Value {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
        let mut command = tokio::process::Command::from(self.command());
        command
            .args(["mcp", "serve"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        let mut child = command.spawn().unwrap();
        let mut input = child.stdin.take().unwrap();
        let mut output = tokio::io::BufReader::new(child.stdout.take().unwrap()).lines();
        let mut result = Value::Null;
        for message in [
            json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"Acme investigation reports","version":"1"}}}),
            json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
            json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":name,"arguments":args}}),
        ] {
            input
                .write_all(format!("{message}\n").as_bytes())
                .await
                .unwrap();
            if message.get("id").is_none() {
                continue;
            }
            loop {
                let line =
                    tokio::time::timeout(std::time::Duration::from_secs(15), output.next_line())
                        .await
                        .unwrap()
                        .unwrap()
                        .unwrap();
                let response: Value = serde_json::from_str(&line).unwrap();
                if response["id"] == message["id"] {
                    assert!(response.get("error").is_none(), "{response}");
                    result = response["result"].clone();
                    break;
                }
            }
        }
        drop(input);
        assert!(
            tokio::time::timeout(std::time::Duration::from_secs(15), child.wait())
                .await
                .unwrap()
                .unwrap()
                .success()
        );
        result
    }
    async fn report_mcp(&self, name: &str, args: Value) -> Value {
        let result = self.report_mcp_error(name, args).await;
        assert_eq!(result["isError"], false, "{result}");
        result["structuredContent"]["data"].clone()
    }
    fn command(&self) -> Command {
        let pinned = self.0.path().join("testrun");
        let mut command = Command::new(if pinned.exists() {
            pinned.as_os_str()
        } else {
            std::ffi::OsStr::new(env!("CARGO_BIN_EXE_testrun"))
        });
        command
            .current_dir(self.0.path())
            .env("TESTRUN_DATA_ROOT", self.0.path().join("state"))
            .env_remove("TESTRUN_TASK_ID")
            .env_remove("CODEX_THREAD_ID")
            .arg("--json");
        let adb = self.0.path().join("adb");
        if adb.exists() {
            command.env("TESTRUN_ADB", adb);
        }
        if self.0.path().join("xcrun").exists() {
            command
                .env("TESTRUN_XCRUN", self.0.path().join("xcrun"))
                .env("TESTRUN_IDEVICESYSLOG", self.0.path().join("syslog"))
                .env("TESTRUN_CODESIGN", self.0.path().join("codesign"))
                .env("TESTRUN_APPLE_PS", self.0.path().join("apple-ps"));
        }
        command
    }
    async fn call(&self, args: &[&str]) -> Value {
        let mut command = tokio::process::Command::from(self.command());
        command.args(args).kill_on_drop(true);
        let output = tokio::time::timeout(std::time::Duration::from_secs(15), command.output())
            .await
            .unwrap()
            .unwrap();
        assert!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }
    async fn settled(&self, investigation: &str, analysis: Option<&str>) -> Value {
        let mut last = Value::Null;
        let waited = tokio::time::timeout(std::time::Duration::from_secs(20), async {
            loop {
                last = if let Some(analysis) = analysis {
                    self.call(&["diagnostics", "analysis-status", investigation, analysis])
                        .await
                } else {
                    self.call(&["diagnostics", "status", investigation]).await
                };
                if last["settled"] == true {
                    return last.clone();
                }
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        })
        .await;
        waited.unwrap_or_else(|_| {
            panic!(
                "Diagnostic settlement expired: {last}; fixture commands: {}",
                std::fs::read_to_string(self.0.path().join("commands.jsonl")).unwrap_or_default()
            )
        })
    }
}
impl Drop for InvestigationFixture {
    fn drop(&mut self) {
        let _ = self.command().args(["manager", "drain"]).output();
    }
}

#[cfg(unix)]
fn android_fixture(plan: Value) -> InvestigationFixture {
    use std::os::unix::fs::PermissionsExt;
    let fixture = InvestigationFixture(tempfile::tempdir().unwrap());
    let adb = fixture.0.path().join("adb");
    std::fs::write(&adb, include_bytes!("fixtures/diagnostic-adb.sh")).unwrap();
    std::fs::set_permissions(&adb, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(fixture.0.path().join("plan.json"), plan.to_string()).unwrap();
    fixture
}

#[cfg(unix)]
#[test]
fn diagnostic_android_fixture_refuses_foreign_transport_unbounded_logs_and_command_execution() {
    let f = android_fixture(json!({}));
    let marker = f.0.path().join("must-not-run");
    let command = format!("'touch' '{}'", marker.display());
    for args in [
        vec!["-s", "OTHER", "shell", "'date' '+%s.%N'"],
        vec!["-t", "11", "shell", "'logcat' '-c'"],
        vec![
            "-t",
            "11",
            "shell",
            "'logcat' '-d' '-v' 'epoch,uid,usec' '-b' 'main' '-b' 'crash' '--uid=10042' '-t' '999999' '*:V'",
        ],
        vec!["-t", "11", "shell", &command],
    ] {
        let output = Command::new(f.0.path().join("adb"))
            .args(&args)
            .output()
            .unwrap();
        assert!(
            !output.status.success(),
            "accepted unexpected fake-tool command {args:?}"
        );
        assert!(!output.stderr.is_empty());
    }
    assert!(!marker.exists());
}

#[cfg(unix)]
async fn android_capture(f: &InvestigationFixture, follow: bool) -> (String, String) {
    let opened = f
        .call(&[
            "diagnostics",
            "open",
            "--submission-id",
            "android",
            "--question",
            "Inspect disappearance",
            "--binding",
            r#"{"targetId":"android:ACME","appId":"com.example.acme"}"#,
        ])
        .await;
    let id = opened["investigationId"].as_str().unwrap().to_string();
    let source = json!({"kind":"android","targetId":"android:ACME","appId":"com.example.acme","userId":0,"mode":if follow{"follow"}else{"snapshot"},"includeReports":true}).to_string();
    let captured = f
        .call(&[
            "diagnostics",
            "start",
            &id,
            "--submission-id",
            "native",
            "--source",
            &source,
            "--limits",
            r#"{"durationMs":20000,"maxBytes":1048576}"#,
        ])
        .await;
    (
        id,
        captured["collectors"][0]["collectorId"]
            .as_str()
            .unwrap()
            .to_string(),
    )
}

#[cfg(unix)]
#[tokio::test]
async fn diagnostic_android_public_cli_scopes_release_crash_reports_and_offline_analysis() {
    let fixture = android_fixture(json!({}));
    let (id, collector) = android_capture(&fixture, false).await;
    let status = fixture.settled(&id, None).await;
    assert_eq!(status["collectors"][0]["phase"], "partial", "{status}");
    assert_eq!(status["collectors"][0]["sourceReport"]["uid"], 10042);
    assert!(!status.to_string().contains("private-marker"));
    let raw = fixture
        .call(&["diagnostics", "read", &id, &collector, "--limit", "65536"])
        .await;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(raw["dataBase64"].as_str().unwrap())
        .unwrap();
    let text = String::from_utf8(bytes).unwrap();
    assert!(text.contains("private-marker"));
    assert!(!text.contains("private-neighbor"));
    let packets: Vec<Value> = text
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    let installed = packets.iter().find(|p| p["kind"] == "installed").unwrap();
    assert_eq!(installed["data"]["identity"]["debuggable"], false);
    assert_eq!(installed["data"]["identity"]["profileableByShell"], true);
    assert!(
        packets
            .iter()
            .any(|p| p["kind"] == "report" && p["data"]["status"] == "permission_denied")
    );
    assert_eq!(
        packets.iter().find(|p| p["kind"] == "exits").unwrap()["data"]["entries"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let parsed = fixture
        .call(&[
            "diagnostics",
            "analyze",
            &id,
            &collector,
            "--submission-id",
            "parse",
        ])
        .await;
    let aid = parsed["analysisId"].as_str().unwrap();
    let analysis = fixture.settled(&id, Some(aid)).await;
    assert_eq!(analysis["phase"], "complete", "{analysis}");
    let transcript = std::fs::read(fixture.0.path().join("commands.jsonl")).unwrap();
    fixture.call(&["manager", "stop"]).await;
    let failures = fixture
        .call(&[
            "diagnostics",
            "query",
            &id,
            aid,
            "--filters",
            r#"{"kind":"failure"}"#,
        ])
        .await;
    let content = failures.to_string();
    assert!(content.contains("java_crash_candidate"), "{failures}");
    assert!(content.contains("native_signal_candidate"));
    assert!(content.contains("anr_candidate"));
    assert_eq!(failures["observationComplete"], false);
    let stacks = fixture
        .call(&[
            "diagnostics",
            "query",
            &id,
            aid,
            "--filters",
            r#"{"kind":"stack"}"#,
        ])
        .await;
    assert_eq!(stacks["rows"].as_array().unwrap().len(), 1, "{stacks}");
    assert!(stacks["nextCursor"].is_null());
    assert_eq!(
        std::fs::read(fixture.0.path().join("commands.jsonl")).unwrap(),
        transcript
    );
}

#[cfg(unix)]
#[tokio::test]
async fn diagnostic_android_follow_pid_reuse_boot_change_and_denial_remain_partial() {
    let fixture = android_fixture(json!({"reboot":true,"denied":true}));
    let (id, collector) = android_capture(&fixture, true).await;
    let status = fixture.settled(&id, None).await;
    assert_ne!(status["collectors"][0]["phase"], "complete", "{status}");
    assert!(
        status.to_string().contains("ANDROID_BOOT_CHANGED"),
        "{status}"
    );
    let raw = fixture
        .call(&["diagnostics", "read", &id, &collector, "--limit", "65536"])
        .await;
    let text = String::from_utf8(
        base64::engine::general_purpose::STANDARD
            .decode(raw["dataBase64"].as_str().unwrap())
            .unwrap(),
    )
    .unwrap();
    assert!(text.contains("permission_denied"));
    assert!(!text.contains("bbbbbbbb"));
    assert!(!text.contains("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa:42:200"));
}

#[cfg(unix)]
#[tokio::test]
async fn diagnostic_android_follow_observes_generation_change_and_stops_owned_work() {
    let fixture = android_fixture(json!({}));
    let (id, collector) = android_capture(&fixture, true).await;
    let mut last = Value::Null;
    let waited = tokio::time::timeout(std::time::Duration::from_secs(15), async {
        loop {
            last = fixture.call(&["diagnostics", "status", &id]).await;
            if last["collectors"][0]["sourceReport"]["cycles"]
                .as_u64()
                .unwrap_or(0)
                >= 2
            {
                break;
            }
            assert_ne!(
                last["settled"], true,
                "follow stopped before second completed cycle: {last}"
            );
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    })
    .await;
    waited.unwrap_or_else(|_| {
        panic!(
            "Follow observation expired: {last}; fixture commands: {}",
            std::fs::read_to_string(fixture.0.path().join("commands.jsonl")).unwrap_or_default()
        )
    });
    fixture
        .call(&["diagnostics", "stop", &id, "--collector-id", &collector])
        .await;
    let status = fixture.settled(&id, None).await;
    assert_eq!(status["collectors"][0]["phase"], "partial", "{status}");
    assert_eq!(status["collectors"][0]["reason"], "stopped", "{status}");
    let raw = fixture
        .call(&["diagnostics", "read", &id, &collector, "--limit", "65536"])
        .await;
    let text = String::from_utf8(
        base64::engine::general_purpose::STANDARD
            .decode(raw["dataBase64"].as_str().unwrap())
            .unwrap(),
    )
    .unwrap();
    assert!(text.contains("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa:42:100"));
    assert!(text.contains("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa:42:200"));
    assert!(text.contains("noLongerObserved"));
}

#[cfg(unix)]
#[tokio::test]
async fn diagnostic_android_output_overflow_is_explicit_and_hung_client_is_reaped() {
    for plan in [json!({"overflow":true}), json!({"hang":true})] {
        let fixture = android_fixture(plan.clone());
        let (id, collector) = android_capture(&fixture, false).await;
        if plan["hang"] == true {
            tokio::time::timeout(std::time::Duration::from_secs(10), async {
                while !fixture.0.path().join("hanging-pid").exists() {
                    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                }
            })
            .await
            .unwrap();
            fixture
                .call(&["diagnostics", "stop", &id, "--collector-id", &collector])
                .await;
        }
        let status = fixture.settled(&id, None).await;
        assert_ne!(status["collectors"][0]["phase"], "complete", "{status}");
        if plan["hang"] == true {
            let pid: i32 = std::fs::read_to_string(fixture.0.path().join("hanging-pid"))
                .unwrap()
                .parse()
                .unwrap();
            assert_eq!(
                unsafe { libc::kill(pid, 0) },
                -1,
                "owned adb client remains alive"
            );
        } else {
            assert!(status.to_string().contains("output_limit"), "{status}");
        }
    }
}
#[tokio::test]
async fn diagnostic_analysis_public_cli_runs_isolated_parser_and_reads_index_offline() {
    let fixture = InvestigationFixture(tempfile::tempdir().unwrap());
    let path = fixture.0.path().join("trace.json");
    std::fs::write(
        &path,
        br#"{"traceEvents":[{"name":"Acme work","ph":"X","ts":10,"dur":5}]}"#,
    )
    .unwrap();
    let opened = fixture
        .call(&[
            "diagnostics",
            "open",
            "--submission-id",
            "case",
            "--question",
            "Inspect Acme work",
        ])
        .await;
    let investigation = opened["investigationId"].as_str().unwrap();
    let source = json!({"kind":"import","path":path,"format":"chrome-trace"}).to_string();
    let capture = fixture
        .call(&[
            "diagnostics",
            "start",
            investigation,
            "--submission-id",
            "trace",
            "--source",
            &source,
        ])
        .await;
    let collector = capture["collectors"][0]["collectorId"].as_str().unwrap();
    for _ in 0..100 {
        let status = fixture
            .call(&["diagnostics", "status", investigation])
            .await;
        if status["settled"] == true {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    let analysis = fixture
        .call(&[
            "diagnostics",
            "analyze",
            investigation,
            collector,
            "--submission-id",
            "parse",
        ])
        .await;
    let aid = analysis["analysisId"].as_str().unwrap();
    let status = tokio::time::timeout(std::time::Duration::from_secs(20), async {
        loop {
            let status = fixture
                .call(&["diagnostics", "analysis-status", investigation, aid])
                .await;
            if status["settled"] == true {
                break status;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(status["phase"], "complete", "{status}");
    std::fs::remove_file(&path).unwrap();
    assert_eq!(
        fixture
            .call(&[
                "diagnostics",
                "analyze",
                investigation,
                collector,
                "--submission-id",
                "parse"
            ])
            .await["phase"],
        "complete"
    );
    fixture.call(&["manager", "stop"]).await;
    let query = fixture
        .call(&[
            "diagnostics",
            "query",
            investigation,
            aid,
            "--filters",
            "{\"kind\":\"span\",\"limit\":1}",
        ])
        .await;
    assert_eq!(query["rows"][0]["record"]["value"], 5.0);
    assert_eq!(query["rows"][0]["evidence"]["source"], "/traceEvents/0");
    assert_eq!(query["observationComplete"], false);
    fixture.call(&["diagnostics", "close", investigation]).await;
    fixture
        .call(&["diagnostics", "remove", investigation])
        .await;
    fixture.call(&["manager", "stop"]).await;
}

#[tokio::test]
async fn diagnostic_access_discovery_preparation_and_exports_preserve_boundaries() {
    let fixture = InvestigationFixture(tempfile::tempdir().unwrap());
    let caps = fixture.call(&["diagnostics", "capabilities"]).await;
    assert_eq!(caps["dependencies"][0]["availability"], "bundled_unprobed");
    assert!(!fixture.0.path().join("state").exists());
    let prepared = fixture
        .call(&["diagnostics", "prepare", "builtin-parser"])
        .await;
    assert_eq!(prepared["availability"], "ready");
    assert_eq!(prepared["downloads"], false);
    assert!(!fixture.0.path().join("state").exists());
    let path = fixture.0.path().join("events.json");
    std::fs::write(&path,serde_json::to_vec(&json!({"schemaVersion":1,"records":[{"kind":"event","name":"private-fixture","source":"/private-pointer","data":{"appId":"com.example.acme","processGeneration":"one","url":"wss://user:secret@example.com/private-token"}}]})).unwrap()).unwrap();
    let opened = fixture
        .call(&[
            "diagnostics",
            "open",
            "--submission-id",
            "case",
            "--question",
            "Investigate wss://example.com/private-token token=private-fixture",
            "--binding",
            "{\"appId\":\"com.example.acme\",\"processId\":42,\"processGeneration\":\"one\"}",
        ])
        .await;
    assert!(!opened.to_string().contains("private-token"));
    assert!(!opened.to_string().contains("private-fixture"));
    let id = opened["investigationId"].as_str().unwrap();
    let source = json!({"kind":"import","path":path,"format":"testrun-events-v1"}).to_string();
    let capture = fixture
        .call(&[
            "diagnostics",
            "start",
            id,
            "--submission-id",
            "capture",
            "--source",
            &source,
        ])
        .await;
    let collector = capture["collectors"][0]["collectorId"].as_str().unwrap();
    let status = fixture.settled(id, None).await;
    assert!(
        status["collectors"][0]["cost"]["runWallMs"]
            .as_f64()
            .unwrap()
            >= 0.0
    );
    let analysis = fixture
        .call(&[
            "diagnostics",
            "analyze",
            id,
            collector,
            "--submission-id",
            "parse",
        ])
        .await;
    let aid = analysis["analysisId"].as_str().unwrap();
    let status = fixture.settled(id, Some(aid)).await;
    assert_eq!(status["phase"], "complete");
    assert!(status["cost"]["runWallMs"].is_number());
    let report = fixture.call(&["diagnostics", "project", id, aid]).await;
    assert_eq!(report["matched"], 1);
    assert!(!report.to_string().contains("private-fixture"));
    let destination = fixture.0.path().join("report.json");
    let destination_arg = destination.to_str().unwrap();
    let export = fixture
        .call(&["diagnostics", "export", id, aid, "--path", destination_arg])
        .await;
    assert_eq!(export["alreadyPresent"], false);
    assert_eq!(
        fixture
            .call(&["diagnostics", "export", id, aid, "--path", destination_arg])
            .await["alreadyPresent"],
        true
    );
    let report_bytes = std::fs::read(&destination).unwrap();
    assert!(!String::from_utf8_lossy(&report_bytes).contains("private-token"));
    let raw = fixture.call(&["diagnostics", "read", id, collector]).await;
    let original = base64::engine::general_purpose::STANDARD
        .decode(raw["dataBase64"].as_str().unwrap())
        .unwrap();
    assert!(String::from_utf8_lossy(&original).contains("private-token"));
    fixture.call(&["diagnostics", "close", id]).await;
    fixture.call(&["diagnostics", "remove", id]).await;
    assert_eq!(std::fs::read(&destination).unwrap(), report_bytes);
    assert!(path.exists());
    fixture.call(&["manager", "stop"]).await;
}

fn retained_bytes(path: &std::path::Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(path) else {
        return 0;
    };
    entries
        .filter_map(Result::ok)
        .map(|entry| {
            let metadata = std::fs::symlink_metadata(entry.path()).unwrap();
            if metadata.is_dir() {
                retained_bytes(&entry.path())
            } else if metadata.is_file() {
                metadata.len()
            } else {
                0
            }
        })
        .sum()
}
#[tokio::test]
#[ignore = "Run with --ignored and an explicit new TESTRUN_DIAGNOSTIC_REPORT path"]
async fn diagnostic_access_overhead_measurement() {
    use sha2::{Digest, Sha256};
    let os_version = if cfg!(target_os = "macos") {
        std::process::Command::new("/usr/bin/sw_vers")
            .arg("-productVersion")
            .output()
            .ok()
            .filter(|o| o.status.success())
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_owned())
    } else {
        None
    };
    let report_path = std::env::var_os("TESTRUN_DIAGNOSTIC_REPORT")
        .expect("Select an explicit new report path through TESTRUN_DIAGNOSTIC_REPORT");
    assert!(std::path::Path::new(&report_path).is_absolute());
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut output = options
        .open(&report_path)
        .expect("Select a new private measurement report path before starting the fixture");
    output
        .write_all(b"{\"schemaVersion\":2,\"state\":\"incomplete\"}")
        .unwrap();
    let fixture = InvestigationFixture(tempfile::tempdir().unwrap());
    std::fs::hard_link(
        env!("CARGO_BIN_EXE_testrun"),
        fixture.0.path().join("testrun"),
    )
    .unwrap();
    let path = fixture.0.path().join("trace.json");
    let events: Vec<_> = (0..256)
        .map(|n| json!({"ph":"X","name":"work","ts":n,"dur":1,"args":{"payload":"x".repeat(1024)}}))
        .collect();
    let raw = serde_json::to_vec(&json!({"traceEvents":events})).unwrap();
    std::fs::write(&path, &raw).unwrap();
    let mut samples = Vec::new();
    for n in 0..3 {
        samples.push(measure_diagnostic_access(&fixture, &path, &raw, n).await);
    }
    fixture.call(&["manager", "stop"]).await;
    let report = json!({"schemaVersion":2,"sourceBuildId":env!("TESTRUN_BUILD_ID"),"harnessSha256":format!("{:x}",Sha256::digest(include_bytes!("diagnostic_analysis.rs"))),"host":{"os":std::env::consts::OS,"version":os_version,"architecture":std::env::consts::ARCH,"contention":"shared development host; no load isolation or host-wide process control"},"profile":if cfg!(debug_assertions){"debug"}else{"release"},"workload":{"format":"chrome-trace","sha256":format!("{:x}",Sha256::digest(&raw)),"bytes":raw.len(),"events":256,"repeats":3,"target":"owned import fixture; no app instrumentation","order":"discovery, admission, collection, ordinary status, explicit read, analysis, query, separate memory arm, cleanup"},"samples":samples,"acceptance":{"correctness":"exact row/read/settlement assertions and zero owned investigation bytes after removal","resourceLimits":"existing collector bytes/events, parser raw/index/row/time budgets and query page limit","latencyThreshold":null},"gaps":["target app/native capture overhead and event loss not measured","manager/parser peak RSS and CPU attribution not measured","small fixed-order host sample; no individual-action tail or cross-host/build guarantee"],"meaning":"CLI arms include startup/IPC/JSON decoding and settlement polling where applicable; filesystem walks and assertions after each call are outside the outer clocks; runWallMs is separate internal host work; query RSS uses a separate instrumented child and is not subtracted from timing arms"});
    use std::io::Seek;
    output.set_len(0).unwrap();
    output.rewind().unwrap();
    output
        .write_all(&serde_json::to_vec_pretty(&report).unwrap())
        .unwrap();
}

async fn measure_diagnostic_access(
    fixture: &InvestigationFixture,
    path: &std::path::Path,
    raw: &[u8],
    n: usize,
) -> Value {
    let began = std::time::Instant::now();
    fixture.call(&["diagnostics", "capabilities"]).await;
    let disabled = began.elapsed().as_secs_f64() * 1000.0;
    let began = std::time::Instant::now();
    let opened = fixture
        .call(&[
            "diagnostics",
            "open",
            "--submission-id",
            &format!("case-{n}"),
            "--question",
            "Measure owned fixture",
        ])
        .await;
    let open_admission = began.elapsed().as_secs_f64() * 1000.0;
    let id = opened["investigationId"].as_str().unwrap();
    let before = retained_bytes(&fixture.0.path().join("state"));
    let source = json!({"kind":"import","path":path,"format":"chrome-trace"}).to_string();
    let began = std::time::Instant::now();
    let capture = fixture
        .call(&[
            "diagnostics",
            "start",
            id,
            "--submission-id",
            "capture",
            "--source",
            &source,
        ])
        .await;
    let collector = capture["collectors"][0]["collectorId"].as_str().unwrap();
    let collection_admission = began.elapsed().as_secs_f64() * 1000.0;
    let status = fixture.settled(id, None).await;
    let light = began.elapsed().as_secs_f64() * 1000.0;
    let after_light = retained_bytes(&fixture.0.path().join("state"));
    assert_eq!(status["collectors"][0]["phase"], "complete");
    let began_read = std::time::Instant::now();
    let ordinary = fixture.call(&["diagnostics", "status", id]).await;
    let ordinary_ms = began_read.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(ordinary["settled"], true);
    let began_read = std::time::Instant::now();
    let read = fixture
        .call(&["diagnostics", "read", id, collector, "--limit", "16384"])
        .await;
    let read_ms = began_read.elapsed().as_secs_f64() * 1000.0;
    let read_bytes = base64::engine::general_purpose::STANDARD
        .decode(read["dataBase64"].as_str().unwrap())
        .unwrap();
    assert_eq!(read_bytes.len(), 16384);
    assert_eq!(read_bytes, &raw[..read_bytes.len()]);
    let began = std::time::Instant::now();
    let analysis = fixture
        .call(&[
            "diagnostics",
            "analyze",
            id,
            collector,
            "--submission-id",
            "parse",
        ])
        .await;
    let aid = analysis["analysisId"].as_str().unwrap();
    let analysis_admission = began.elapsed().as_secs_f64() * 1000.0;
    let parsed = fixture.settled(id, Some(aid)).await;
    let deep = began.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(parsed["phase"], "complete", "{parsed}");
    let after_deep = retained_bytes(&fixture.0.path().join("state"));
    let began_query = std::time::Instant::now();
    let query = fixture
        .call(&[
            "diagnostics",
            "query",
            id,
            aid,
            "--filters",
            r#"{"view":"timeline","clockDomain":"trace","limit":20}"#,
        ])
        .await;
    let query_ms = began_query.elapsed().as_secs_f64() * 1000.0;
    assert!(query["nextCursor"].is_string());
    assert_eq!(query["rows"].as_array().unwrap().len(), 20);
    let memory = diagnostic_query_memory(fixture, id, aid).await;
    let began_cleanup = std::time::Instant::now();
    fixture.call(&["diagnostics", "close", id]).await;
    fixture.call(&["diagnostics", "remove", id]).await;
    let cleanup_ms = began_cleanup.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(
        retained_bytes(&fixture.0.path().join("state/diagnostics")),
        0
    );
    assert!(path.exists());
    json!({"managerWarmBeforeOpen":n>0,"disabledDiscoveryWallMs":disabled,"openAdmissionCliWallMs":open_admission,"collectionAdmissionCliWallMs":collection_admission,"lightweightCollectionCliWallMs":light,"ordinaryStatusCliWallMs":ordinary_ms,"explicitReadCliWallMs":read_ms,"readBytes":read_bytes.len(),"analysisAdmissionCliWallMs":analysis_admission,"deepAnalysisCliWallMs":deep,"queryCliWallMs":query_ms,"queryRows":20,"cleanupCliWallMs":cleanup_ms,"queryMemory":memory,"collection":status["collectors"][0]["cost"],"analysis":parsed["cost"],"lightweightRetainedGrowthBytes":after_light.saturating_sub(before),"deepRetainedGrowthBytes":after_deep.saturating_sub(after_light),"ownedInvestigationBytesAfterRemoval":0})
}

async fn diagnostic_query_memory(
    fixture: &InvestigationFixture,
    inv: &str,
    analysis: &str,
) -> Value {
    if !cfg!(target_os = "macos") {
        return json!({"state":"unavailable","reason":"Darwin native query-process peak RSS probe not selected on this host"});
    }
    let original = fixture.command();
    let mut command = tokio::process::Command::new("/usr/bin/time");
    command
        .arg("-l")
        .arg(original.get_program())
        .args(original.get_args())
        .args([
            "diagnostics",
            "query",
            inv,
            analysis,
            "--filters",
            r#"{"limit":20}"#,
        ])
        .current_dir(fixture.0.path())
        .kill_on_drop(true);
    for (key, value) in original.get_envs() {
        if let Some(value) = value {
            command.env(key, value);
        } else {
            command.env_remove(key);
        }
    }
    let output = tokio::time::timeout(std::time::Duration::from_secs(15), command.output())
        .await
        .unwrap()
        .unwrap();
    assert!(output.status.success(), "query RSS probe failed");
    let query: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(query["rows"].as_array().unwrap().len(), 20);
    let native = String::from_utf8(output.stderr).unwrap();
    let rss = native
        .lines()
        .find(|line| line.trim().ends_with("maximum resident set size"))
        .and_then(|line| line.split_whitespace().next())
        .and_then(|n| n.parse::<u64>().ok())
        .filter(|n| *n > 0)
        .expect("Darwin time -l peak RSS field");
    json!({"state":"observed","scope":"one owned CLI query child; excludes manager/parser and is not heap ownership","peakResidentBytes":rss,"source":"Darwin time -l maximum resident set size; separate instrumented arm"})
}
