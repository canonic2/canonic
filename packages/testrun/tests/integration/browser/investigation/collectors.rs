//! Structured trace options and native incremental coverage through installed clients.
use super::*;

pub(super) async fn refused(mcp: &mut Mcp, target: &str, request: Value, code: &str, queued: bool) {
    let response = mcp
        .request(
            "tools/call",
            json!({"name":"browser_send","arguments":{"targetId":target,"request":request}}),
        )
        .await;
    let result = &response["result"]["structuredContent"];
    assert_eq!(result["ok"], queued, "{response}");
    if queued {
        let status = mcp.tool("browser_status", json!({"targetId":target,"operationId":result["data"]["operation"]["id"],"waitMs":30000})).await;
        assert_eq!(status["operation"]["settled"], true, "{status}");
        assert_eq!(status["operation"]["outcome"], "failed", "{status}");
        assert_eq!(status["operation"]["code"], code, "{status}");
    } else {
        assert_eq!(result["code"], code, "{response}");
    }
}
fn trace(report: &Value, title: &str) {
    use std::io::Read;
    let bytes = std::fs::read(report["attachment"]["path"].as_str().unwrap()).unwrap();
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut text = String::new();
    zip.by_name("trace.trace")
        .unwrap()
        .read_to_string(&mut text)
        .unwrap();
    let events = text
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert!(
        events
            .iter()
            .any(|event| event["type"] == "context-options" && event["title"] == title)
    );
    for kind in ["frame-snapshot", "aria-snapshot", "screenshot"] {
        assert!(
            events.iter().any(|event| event["type"] == kind),
            "Missing {kind}: {text}"
        );
    }
    assert!(
        zip.file_names()
            .map(|name| name.expect("Valid owned ZIP member name"))
            .any(|name| name.starts_with("aria/") && name.ends_with(".json"))
    );
    assert!(
        zip.file_names()
            .map(|name| name.expect("Valid owned ZIP member name"))
            .any(|name| name.starts_with("screenshots/") && name.ends_with(".png"))
    );
}
async fn traces(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    context: &Value,
    page: &Value,
) -> Value {
    let site = Site::start().await;
    let source = fixture
        .workspace
        .path()
        .join("Acme-private-selected.spec.ts");
    std::fs::write(&source, b"// Acme immutable selected source\n").unwrap();
    let request=json!({"operation":"trace_start","context":context,"options":{"snapshots":{"dom":true,"aria":true,"screen":true},"name":"Acme_first","title":"Acme trace first","maxDurationMs":60000,"live":true,"sources":true,"sourceFiles":[source]}}).to_string();
    let started = std::time::Instant::now();
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &request])
        .await;
    let started_capture = completed(mcp, target, submitted).await;
    assert_eq!(started_capture["liveRecording"], true);
    assert_eq!(started_capture["sourceTextsIncluded"], true);
    assert!(
        !started_capture
            .to_string()
            .contains("Acme-private-selected")
    );
    std::fs::remove_file(&source).unwrap();
    let fresh_ms = started.elapsed().as_secs_f64() * 1000.0;
    operation(mcp,target,json!({"operation":"page_action","page":page,"locator":{"kind":"css","selector":"#acme"},"action":{"kind":"click"}})).await;
    trace_group_and_script(mcp, target, page, &source, &site.url, "first").await;
    let first = operation(mcp, target, json!({"operation":"trace_chunk_stop"})).await;
    trace(&first["reports"][0], "Acme trace first");
    trace_sources(&first["reports"][0], &source, "first", true);
    let started = std::time::Instant::now();
    operation(
        mcp,
        target,
        json!({"operation":"trace_chunk_start","name":"Acme_second","title":"Acme trace second"}),
    )
    .await;
    let retained_ms = started.elapsed().as_secs_f64() * 1000.0;
    operation(mcp,target,json!({"operation":"page_action","page":page,"locator":{"kind":"css","selector":"#acme"},"action":{"kind":"click"}})).await;
    trace_group_and_script(mcp, target, page, &source, &site.url, "second").await;
    let started = std::time::Instant::now();
    let stopped = operation(mcp, target, json!({"operation":"trace_stop"})).await;
    let stop_ms = started.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(stopped["reports"].as_array().unwrap().len(), 2);
    trace(&stopped["reports"][1], "Acme trace second");
    trace_sources(&stopped["reports"][1], &source, "second", true);
    for report in stopped["reports"].as_array().unwrap() {
        operation(
            mcp,
            target,
            json!({"operation":"developer_evidence_remove","evidenceId":report["evidenceId"]}),
        )
        .await;
    }
    operation(mcp,target,json!({"operation":"trace_start","context":context,"options":{"snapshots":true,"name":"Acme_control","live":false,"sources":false}})).await;
    trace_group_and_script(mcp, target, page, &source, &site.url, "control").await;
    let control = operation(mcp, target, json!({"operation":"trace_stop"})).await;
    trace_sources(&control["reports"][0], &source, "control", false);
    operation(mcp,target,json!({"operation":"developer_evidence_remove","evidenceId":control["reports"][0]["evidenceId"]})).await;
    json!({"freshCliStartMs":fresh_ms,"retainedChunkStartMs":retained_ms,"stopMs":stop_ms})
}
async fn trace_group_and_script(
    mcp: &mut Mcp,
    target: &str,
    page: &Value,
    source: &std::path::Path,
    site: &str,
    label: &str,
) {
    operation(mcp,target,json!({"operation":"trace_group_start","name":format!("Acme {label} source group"),"location":{"file":source,"line":42,"column":3}})).await;
    let url = format!("{site}/trace-live-source.js?{label}");
    let loaded=operation(mcp,target,json!({"operation":"evaluate","scope":page,"isFunction":true,"expression":format!("async()=>{{const script=document.createElement('script');script.src={};await new Promise((resolve,reject)=>{{script.onload=resolve;script.onerror=reject;document.head.append(script);}});return AcmeTraceScript;}}",json!(url))})).await;
    assert_eq!(
        loaded["value"]["value"],
        match label {
            "first" => 1,
            "second" => 2,
            "control" => 3,
            _ => unreachable!(),
        }
    );
    operation(mcp, target, json!({"operation":"trace_group_end"})).await;
}
fn trace_sources(report: &Value, source: &std::path::Path, label: &str, live: bool) {
    use sha1::{Digest, Sha1};
    use std::io::Read;
    assert_eq!(report["sourceTextsIncluded"], live);
    assert_eq!(report["liveRecording"], live);
    assert!(!report.to_string().contains("Acme-private-selected"));
    let bytes = std::fs::read(report["attachment"]["path"].as_str().unwrap()).unwrap();
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let name = format!(
        "src/{:x}.ts",
        Sha1::digest(source.to_str().unwrap().as_bytes())
    );
    if live {
        let mut text = String::new();
        zip.by_name(&name)
            .unwrap()
            .read_to_string(&mut text)
            .unwrap();
        assert_eq!(text, "// Acme immutable selected source\n");
    } else {
        assert!(zip.by_name(&name).is_err());
    }
    let mut text = String::new();
    zip.by_name("trace.trace")
        .unwrap()
        .read_to_string(&mut text)
        .unwrap();
    let events = text
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert!(
        events.iter().any(
            |event| event["title"] == format!("Acme {label} source group")
                && event["stack"][0] == json!({"file":source,"line":42,"column":3})
        ),
        "{events:?}"
    );
    text.clear();
    zip.by_name("trace.network")
        .unwrap()
        .read_to_string(&mut text)
        .unwrap();
    let events = text
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    let entry = events
        .iter()
        .find(|event| {
            event["snapshot"]["request"]["url"]
                .as_str()
                .is_some_and(|url| url.ends_with(&format!("/trace-live-source.js?{label}")))
        })
        .expect("Owned script network entry");
    let content = &entry["snapshot"]["response"]["content"];
    if live {
        let resource = content["_file"].as_str().expect("Live script body");
        let mut text = String::new();
        zip.by_name(resource)
            .unwrap()
            .read_to_string(&mut text)
            .unwrap();
        assert!(text.contains("Acme live private script"));
    } else {
        assert!(content.get("_file").is_none());
        assert!(content.get("text").is_none());
    }
}
async fn coverage(fixture: &Fixture, mcp: &mut Mcp, target: &str, page: &Value) -> Value {
    let request=json!({"operation":"profile_start","page":page,"options":{"cpu":true,"javascriptCoverage":true,"cssCoverage":true,"javascriptCallCount":false,"javascriptDetailed":false,"reportAnonymousScripts":false}}).to_string();
    let started = std::time::Instant::now();
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &request])
        .await;
    completed(mcp, target, submitted).await;
    let fresh_ms = started.elapsed().as_secs_f64() * 1000.0;
    let status = operation(
        mcp,
        target,
        json!({"operation":"profile_status","page":page}),
    )
    .await;
    assert_eq!(status["state"], "recording", "{status}");
    assert_eq!(status["options"]["cpu"], true, "{status}");
    let mut reports = Vec::new();
    let mut timings = Vec::new();
    let mut previous = Value::Null;
    for second in [false, true] {
        operation(mcp,target,json!({"operation":"evaluate","scope":page,"isFunction":true,"expression":if second{"()=>{acmeCoverageFn();globalThis.acmeSecondCoverageFn=new Function('return 43;\\n//# sourceURL=https://example.com/acme-second-coverage.js');acmeSecondCoverageFn();acmeSecondCoverageFn();document.querySelector('#acme').className='acmeB';return getComputedStyle(document.querySelector('#acme')).color;}"}else{"()=>{globalThis.acmeCoverageFn=new Function('return 42;\\n//# sourceURL=https://example.com/acme-coverage.js');acmeCoverageFn();acmeCoverageFn();const style=document.createElement('style');style.textContent='.acmeA{color:red}.acmeB{color:blue}';document.head.appendChild(style);document.querySelector('#acme').className='acmeA';return getComputedStyle(document.querySelector('#acme')).color;}"}})).await;
        let started = std::time::Instant::now();
        let captured = operation(
            mcp,
            target,
            json!({"operation":"profile_take_coverage","page":page}),
        )
        .await;
        timings.push(started.elapsed().as_secs_f64() * 1000.0);
        assert_eq!(captured["recording"], true);
        assert_eq!(captured["complete"], true);
        assert_ne!(captured["captureId"], previous);
        previous = captured["captureId"].clone();
        assert_eq!(captured["reports"].as_array().unwrap().len(), 2);
        let javascript: Value = serde_json::from_slice(
            &std::fs::read(
                captured["reports"][0]["attachment"]["path"]
                    .as_str()
                    .unwrap(),
            )
            .unwrap(),
        )
        .unwrap();
        let entries = javascript["result"].as_array().unwrap();
        assert!(
            entries
                .iter()
                .all(|entry| entry["url"].as_str().is_some_and(|url| !url.is_empty()))
        );
        let expected_url = if second {
            "https://example.com/acme-second-coverage.js"
        } else {
            "https://example.com/acme-coverage.js"
        };
        let named = entries
            .iter()
            .find(|entry| entry["url"] == expected_url)
            .unwrap_or_else(|| {
                panic!("Named coverage source; second window: {second}; coverage: {javascript}")
            });
        assert!(
            named["functions"].as_array().unwrap().iter().all(
                |function| function["isBlockCoverage"] == false
                    && function["ranges"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .all(|range| range["count"].as_u64().unwrap() <= 1)
            )
        );
        let css: Value = serde_json::from_slice(
            &std::fs::read(
                captured["reports"][1]["attachment"]["path"]
                    .as_str()
                    .unwrap(),
            )
            .unwrap(),
        )
        .unwrap();
        assert!(!css["coverage"].as_array().unwrap().is_empty());
        reports.extend(captured["reports"].as_array().unwrap().iter().cloned());
    }
    operation(mcp,target,json!({"operation":"evaluate","scope":page,"isFunction":true,"expression":"()=>{function acmeCpuBusy(){const end=performance.now()+80;let n=0;while(performance.now()<end)n++;return n;}return acmeCpuBusy()>0;}"})).await;
    let started = std::time::Instant::now();
    let stopped = operation(mcp, target, json!({"operation":"profile_stop","page":page})).await;
    let stop_ms = started.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(stopped["reports"].as_array().unwrap().len(), 3);
    reports.extend(stopped["reports"].as_array().unwrap().iter().cloned());
    cpu_summary(fixture, mcp, target, &reports).await;
    for report in reports {
        operation(
            mcp,
            target,
            json!({"operation":"developer_evidence_remove","evidenceId":report["evidenceId"]}),
        )
        .await;
    }
    json!({"freshCliStartMs":fresh_ms,"retainedCoverageTakeMs":timings,"stopMs":stop_ms})
}
/// `profile_query` over the saved CPU profile through the CLI and MCP: the busy
/// function is found by self time, pages are ordered and bounded, and a
/// coverage report is refused as the wrong format.
async fn cpu_summary(fixture: &Fixture, mcp: &mut Mcp, target: &str, reports: &[Value]) {
    let cpu = reports
        .iter()
        .find(|report| report["format"] == "v8-cpu-profile")
        .expect("CPU profile report");
    let request = json!({"operation":"profile_query","evidenceId":cpu["evidenceId"],"query":{"kind":"functions","limit":100}}).to_string();
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &request])
        .await;
    let queried = completed(mcp, target, submitted).await;
    let page = &queried["result"];
    assert_eq!(queried["evidenceId"], cpu["evidenceId"], "{queried}");
    assert_eq!(page["kind"], "functions", "{queried}");
    assert_eq!(page["timing"], "sample-timeline", "{page}");
    assert!(page["sampleCount"].as_u64().unwrap() > 0, "{page}");
    let items = page["items"].as_array().unwrap();
    let busy = items
        .iter()
        .find(|item| item["functionName"] == "acmeCpuBusy")
        .unwrap_or_else(|| panic!("Busy function in CPU summary: {page}"));
    assert!(busy["selfMs"].as_f64().unwrap() > 20.0, "{busy}");
    let selves: Vec<f64> = items
        .iter()
        .map(|item| item["selfMs"].as_f64().unwrap())
        .collect();
    assert!(selves.windows(2).all(|pair| pair[0] >= pair[1]), "{page}");
    let first = operation(mcp,target,json!({"operation":"profile_query","evidenceId":cpu["evidenceId"],"query":{"kind":"functions","sortBy":"total","limit":1}})).await;
    assert_eq!(
        first["result"]["items"].as_array().unwrap().len(),
        1,
        "{first}"
    );
    assert_eq!(first["result"]["nextOffset"], 1, "{first}");
    let coverage = reports
        .iter()
        .find(|report| report["format"] != "v8-cpu-profile")
        .expect("Coverage report");
    refused(mcp,target,json!({"operation":"profile_query","evidenceId":coverage["evidenceId"],"query":{"kind":"functions"}}),"PROFILE_EVIDENCE_FORMAT",true).await;
    refused(mcp,target,json!({"operation":"profile_query","evidenceId":cpu["evidenceId"],"query":{"kind":"functions","limit":101}}),"INVALID_ARGUMENT",false).await;
}
#[tokio::test]
async fn installed_step_four_collector_options_checkpoint() {
    let fixture = installed_fixture();
    let mut mcp = Mcp::start(&fixture).await;
    for mode in ["headless", "headed"] {
        for engine in ["chrome", "firefox", "webkit"] {
            let connected = fixture
                .cli(&["browser", "connect", "--engine", engine, "--mode", mode])
                .await;
            let target = connected["targetId"].as_str().unwrap();
            let ready = fixture.ready_target(target).await;
            let context = &ready["browser"]["selectedContext"];
            let result=std::panic::AssertUnwindSafe(async {
            let page=operation(&mut mcp,target,json!({"operation":"page_create","context":context,"select":false})).await["page"].clone();
            operation(&mut mcp,target,json!({"operation":"page_navigate","page":page,"navigation":{"kind":"set_content","html":"<button id=acme onclick='this.textContent=Number(this.textContent)+1'>0</button>"}})).await;
            let latency=if engine=="chrome" {
                for options in [json!({"live":true}),json!({"sources":true,"sourceFiles":[fixture.workspace.path().join("Acme-native-unread.ts")]})] {
                    refused(&mut mcp,target,json!({"operation":"trace_start","context":context,"options":options}),"TRACE_ARCHIVE_OPTIONS_UNSUPPORTED",false).await;
                }
                refused(&mut mcp,target,json!({"operation":"trace_start","context":context,"options":{"snapshots":{"aria":true}}}),"TRACE_ARCHIVE_OPTIONS_UNSUPPORTED",false).await;
                coverage(&fixture,&mut mcp,target,&page).await
            }else{
                refused(&mut mcp,target,json!({"operation":"trace_start","context":context,"options":{"reload":{}}}),"TRACE_RELOAD_UNSUPPORTED",false).await;
                refused(&mut mcp,target,json!({"operation":"profile_take_coverage","page":page}),"NATIVE_DEVELOPER_UNSUPPORTED",true).await;
                refused(&mut mcp,target,json!({"operation":"profile_query","evidenceId":"00000000-0000-4000-8000-000000000001","query":{"kind":"functions"}}),"NATIVE_DEVELOPER_UNSUPPORTED",true).await;
                traces(&fixture,&mut mcp,target,context,&page).await
            };
            operation(&mut mcp,target,json!({"operation":"page_close","page":page})).await;
            eprintln!("Collector option latency {engine} {mode}: {latency}");
        }).catch_unwind().await;
            mcp.tool("disconnect", json!({"targetId":target})).await;
            if let Err(panic) = result {
                fixture.cli(&["disconnect"]).await;
                mcp.tool("disconnect", json!({})).await;
                mcp.stop().await;
                std::panic::resume_unwind(panic);
            }
        }
    }
    fixture.cli(&["disconnect"]).await;
    mcp.tool("disconnect", json!({})).await;
    mcp.stop().await;
}
