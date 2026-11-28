//! Installed performance lifecycle defaults and pre-capture document reset.
use super::native_tracing::CacheSite;
use super::*;
use std::sync::atomic::Ordering;

fn driver_pid(fixture: &Fixture, context: &Value) -> (u32, u64) {
    let connection = context["connectionId"].as_str().unwrap();
    for namespace in std::fs::read_dir(fixture.workspace.data_root())
        .unwrap()
        .flatten()
    {
        let path = namespace
            .path()
            .join("browser")
            .join(format!("browser-{connection}"))
            .join("00000002.json");
        if let Ok(bytes) = std::fs::read(path) {
            let record: Value = serde_json::from_slice(&bytes).unwrap();
            if record["kind"] == "driver" {
                return (
                    u32::try_from(record["pid"].as_u64().unwrap()).unwrap(),
                    record["startWitness"].as_u64().unwrap(),
                );
            }
        }
    }
    panic!("Exact connection's private driver witness is missing");
}

pub(super) async fn disconnect_with_diagnostics(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    engine: &str,
    mode: &str,
    context: &Value,
) {
    let response = disconnect_observed(fixture, mcp, target, engine, mode, context).await;
    assert_eq!(
        response["result"]["structuredContent"]["ok"], true,
        "{response}"
    );
}

fn current_driver(pid: u32, start: u64) -> bool {
    #[cfg(target_os = "macos")]
    {
        let Ok(native_pid) = i32::try_from(pid) else {
            return false;
        };
        let mut info = std::mem::MaybeUninit::<libc::proc_bsdinfo>::zeroed();
        let size = i32::try_from(std::mem::size_of::<libc::proc_bsdinfo>()).unwrap();
        // SAFETY: the kernel writes at most this exact writable structure size.
        if unsafe {
            libc::proc_pidinfo(
                native_pid,
                libc::PROC_PIDTBSDINFO,
                0,
                info.as_mut_ptr().cast(),
                size,
            )
        } != size
        {
            return false;
        }
        // SAFETY: proc_pidinfo filled the complete structure above.
        let info = unsafe { info.assume_init() };
        info.pbi_start_tvsec * 1_000_000 + info.pbi_start_tvusec == start && info.pbi_status != 5
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (pid, start);
        false
    }
}

pub(super) async fn disconnect_observed(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    engine: &str,
    mode: &str,
    context: &Value,
) -> Value {
    disconnect_observed_via(fixture, mcp, target, engine, mode, context, "disconnect").await
}

pub(super) async fn disconnect_observed_via(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    engine: &str,
    mode: &str,
    context: &Value,
    tool: &str,
) -> Value {
    assert!(matches!(tool, "disconnect" | "browser_disconnect"));
    let pid = (engine != "chrome").then(|| driver_pid(fixture, context));
    let response = mcp
        .request(
            "tools/call",
            json!({"name":tool,"arguments":{"targetId":target}}),
        )
        .await;
    let prefix = format!("preset-disconnect-{engine}-{mode}");
    std::fs::write(
        fixture.workspace.path().join(format!("{prefix}.json")),
        serde_json::to_vec_pretty(&response).unwrap(),
    )
    .unwrap();
    if response["result"]["structuredContent"]["ok"] != true {
        if let Some((pid, start)) = pid
            && current_driver(pid, start)
        {
            // Read only the journaled driver PID. No command arguments,
            // environment, signals, caller input or process-wide discovery.
            let metadata = Command::new("/bin/ps")
                .args([
                    "-p",
                    &pid.to_string(),
                    "-o",
                    "pid=,ppid=,stat=,etime=,comm=",
                ])
                .output()
                .await
                .unwrap();
            std::fs::write(
                fixture
                    .workspace
                    .path()
                    .join(format!("{prefix}-driver-process.txt")),
                metadata.stdout,
            )
            .unwrap();
            let sampled = tokio::time::timeout(
                Duration::from_secs(3),
                Command::new("/usr/bin/sample")
                    .arg(pid.to_string())
                    .arg("1")
                    .arg("-file")
                    .arg(
                        fixture
                            .workspace
                            .path()
                            .join(format!("{prefix}-driver-sample.txt")),
                    )
                    .kill_on_drop(true)
                    .output(),
            )
            .await;
            let sample_result = match sampled {
                Ok(Ok(output)) => json!({"completed":true,"exitCode":output.status.code()}),
                Ok(Err(_)) => json!({"completed":false,"reason":"sample-start-failed"}),
                Err(_) => json!({"completed":false,"reason":"sample-timeout-3s"}),
            };
            std::fs::write(
                fixture
                    .workspace
                    .path()
                    .join(format!("{prefix}-driver-sample-result.json")),
                serde_json::to_vec_pretty(&sample_result).unwrap(),
            )
            .unwrap();
            let pipes = tokio::time::timeout(
                Duration::from_secs(2),
                Command::new("/usr/sbin/lsof")
                    .args(["-a", "-p", &pid.to_string(), "-d", "0,1,2"])
                    .kill_on_drop(true)
                    .output(),
            )
            .await;
            let pipes = match pipes {
                Ok(Ok(output)) => output.stdout,
                Ok(Err(_)) => b"lsof start failed\n".to_vec(),
                Err(_) => b"lsof timed out after 2s\n".to_vec(),
            };
            std::fs::write(
                fixture
                    .workspace
                    .path()
                    .join(format!("{prefix}-driver-pipes.txt")),
                pipes,
            )
            .unwrap();
        }
        let retry = mcp
            .request(
                "tools/call",
                json!({"name":tool,"arguments":{"targetId":target}}),
            )
            .await;
        std::fs::write(
            fixture
                .workspace
                .path()
                .join(format!("{prefix}-cleanup-retry.json")),
            serde_json::to_vec_pretty(&retry).unwrap(),
        )
        .unwrap();
    }
    response
}

#[tokio::test]
async fn installed_step_four_firefox_disconnect_diagnostic() {
    let fixture = installed_fixture();
    let mut mcp = Mcp::start(&fixture).await;
    let connected = fixture
        .cli(&[
            "browser", "connect", "--engine", "firefox", "--mode", "headless",
        ])
        .await;
    let target = connected["targetId"].as_str().unwrap();
    let ready = fixture.ready_target(target).await;
    let context = &ready["browser"]["selectedContext"];
    collectors::refused(
        &mut mcp,
        target,
        json!({"operation":"trace_start","context":context,"options":{"performance":{}}}),
        "TRACE_NATIVE_OPTIONS_UNSUPPORTED",
        false,
    )
    .await;
    disconnect_with_diagnostics(&fixture, &mut mcp, target, "firefox", "headless", context).await;
    mcp.stop().await;
}

async fn terminal(mcp: &mut Mcp, target: &str) -> Value {
    let until = tokio::time::Instant::now() + Duration::from_secs(15);
    loop {
        let status = operation(mcp, target, json!({"operation":"trace_status"})).await;
        if status["active"] == false {
            assert_eq!(status["state"], "complete", "{status}");
            return status;
        }
        assert!(tokio::time::Instant::now() < until, "{status}");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}
async fn evaluate(
    mcp: &mut Mcp,
    target: &str,
    scope: &Value,
    expression: &str,
    retain: bool,
) -> Value {
    operation(mcp,target,json!({"operation":"evaluate","scope":scope,"expression":expression,"isFunction":true,"returnHandle":retain})).await
}
fn artifact(result: &Value, reload: bool) {
    assert_eq!(result["report"]["complete"], true, "{result}");
    let bytes = std::fs::read(result["report"]["attachment"]["path"].as_str().unwrap()).unwrap();
    let trace: Value = serde_json::from_slice(&bytes).unwrap();
    let events = trace["traceEvents"].as_array().unwrap();
    assert!(!events.is_empty());
    if reload {
        for name in ["Screenshot", "ProfileChunk"] {
            assert!(
                events.iter().any(|event| event["name"] == name),
                "Missing preset {name}"
            );
        }
    }
}
#[expect(
    clippy::too_many_lines,
    reason = "One correlated four-case lifecycle workflow measures document ownership, cache effects, timers and receipt replay before cleanup"
)]
async fn native_cell(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    context: &Value,
    site: &CacheSite,
) -> Value {
    // Keep logical selection on a sibling while the newly created metadata
    // page owns the active render surface needed for compositor screenshots.
    let other = operation(
        mcp,
        target,
        json!({"operation":"page_create","context":context,"select":true}),
    )
    .await["page"]
        .clone();
    let page = operation(
        mcp,
        target,
        json!({"operation":"page_create","context":context,"select":false}),
    )
    .await["page"]
        .clone();
    let url = format!("{}/?Acme=selected#original", site.url);
    operation(mcp,target,json!({"operation":"page_navigate","page":page,"navigation":{"kind":"goto","url":url,"waitUntil":"load"}})).await;
    let mut fresh_ms = 0.0;
    let mut retained = None;
    for (index, (reload, auto_stop)) in [(true, true), (false, false), (true, false), (false, true)]
        .into_iter()
        .enumerate()
    {
        let old = evaluate(
            mcp,
            target,
            &page,
            "()=>({label:'Acme original document'})",
            true,
        )
        .await["handle"]
            .clone();
        let before = site.pages.load(Ordering::SeqCst);
        let assets_before = site.assets.load(Ordering::SeqCst);
        let ignore_cache = index > 0;
        let performance = if index == 0 {
            json!({})
        } else {
            json!({"reload":reload,"autoStop":auto_stop,"ignoreCache":ignore_cache})
        };
        let request = json!({"operation":"trace_start","context":context,"options":{"page":page,"performance":performance}});
        let start = if index == 0 {
            let began = std::time::Instant::now();
            let submitted = fixture
                .cli(&["browser", "send", "--target", target, &request.to_string()])
                .await;
            let value = completed(mcp, target, submitted).await;
            fresh_ms = began.elapsed().as_secs_f64() * 1000.0;
            value
        } else {
            let submitted=mcp.tool("act",json!({"targetId":target,"actions":[{"action":"trace_start","context":context,"options":{"page":page,"performance":performance}}]})).await;
            completed(mcp, target, submitted).await["results"][0].clone()
        };
        assert_eq!(start["state"], "recording", "{start}");
        assert_eq!(start["metadataPage"], page);
        assert_eq!(
            start["options"]["performance"],
            json!({"reload":reload,"autoStop":auto_stop,"ignoreCache":ignore_cache})
        );
        assert_eq!(
            site.pages.load(Ordering::SeqCst),
            before + usize::from(reload)
        );
        assert_eq!(
            site.assets.load(Ordering::SeqCst),
            assets_before + usize::from(reload && ignore_cache),
            "Only a selected reload bypasses the cache"
        );
        assert_eq!(start["cacheRestorePending"], false);
        assert_eq!(
            mcp.tool("browser_status", json!({"targetId":target})).await["browser"]["selectedPage"],
            other
        );
        if reload {
            assert_eq!(start["reset"]["url"], "about:blank");
            assert_eq!(start["reload"]["url"], url);
            assert_eq!(
                evaluate(
                    mcp,
                    target,
                    &page,
                    "()=>performance.getEntriesByType('navigation')[0].type",
                    false
                )
                .await["value"]["value"],
                "navigate"
            );
            collectors::refused(
                mcp,
                target,
                json!({"operation":"handle_read","handle":old}),
                "STALE_REFERENCE",
                true,
            )
            .await;
        } else {
            assert!(start["reset"].is_null() && start["reload"].is_null());
            assert_eq!(
                evaluate(mcp, target, &old, "value=>value.label", false).await["value"]["value"],
                "Acme original document"
            );
            operation(
                mcp,
                target,
                json!({"operation":"handle_dispose","handle":old}),
            )
            .await;
        }
        if auto_stop {
            terminal(mcp, target).await;
        } else {
            assert!(start["durationRemainingMs"].is_null());
            if index == 1 {
                evaluate(
                    mcp,
                    target,
                    &page,
                    "()=>new Promise(resolve=>setTimeout(()=>resolve(true),6000))",
                    false,
                )
                .await;
                assert_eq!(
                    operation(mcp, target, json!({"operation":"trace_status"})).await["state"],
                    "recording"
                );
            }
        }
        let stopped = operation(mcp, target, json!({"operation":"trace_stop"})).await;
        artifact(&stopped, reload);
        assert_eq!(
            stopped["report"]["stopReason"],
            if auto_stop { "auto-stop" } else { "requested" }
        );
        assert_eq!(
            operation(mcp, target, json!({"operation":"trace_stop"})).await,
            stopped
        );
        if index == 0 {
            retained = Some(std::time::Instant::now());
        }
    }
    let cached = site.assets.load(Ordering::SeqCst);
    operation(mcp,target,json!({"operation":"page_navigate","page":page,"navigation":{"kind":"reload","waitUntil":"load"}})).await;
    assert_eq!(
        site.assets.load(Ordering::SeqCst),
        cached,
        "Preset cache bypass must be restored for subsequent navigation"
    );
    collectors::refused(mcp,target,json!({"operation":"trace_start","context":context,"options":{"page":page,"performance":{},"reload":{}}}),"TRACE_PERFORMANCE_OPTIONS_CONFLICT",false).await;
    assert_eq!(
        operation(mcp, target, json!({"operation":"trace_status"})).await["active"],
        false
    );
    operation(mcp, target, json!({"operation":"page_close","page":page})).await;
    operation(mcp, target, json!({"operation":"page_close","page":other})).await;
    json!({"freshCliPresetStartWithResetMs":fresh_ms,"retainedThreeCaseWorkflowWithWindowsMs":retained.unwrap().elapsed().as_secs_f64()*1000.0})
}
#[tokio::test]
async fn installed_step_four_performance_lifecycle_checkpoint() {
    let fixture = installed_fixture();
    let site = CacheSite::start().await;
    let mut mcp = Mcp::start(&fixture).await;
    for mode in ["headless", "headed"] {
        for engine in ["chrome", "firefox", "webkit"] {
            let began = std::time::Instant::now();
            let connected = fixture
                .cli(&["browser", "connect", "--engine", engine, "--mode", mode])
                .await;
            let target = connected["targetId"].as_str().unwrap();
            let ready = fixture.ready_target(target).await;
            let connect_ms = began.elapsed().as_secs_f64() * 1000.0;
            let context = &ready["browser"]["selectedContext"];
            let result=std::panic::AssertUnwindSafe(async {
                if engine=="chrome" {native_cell(&fixture,&mut mcp,target,context,&site).await}
                else {
                    collectors::refused(&mut mcp,target,json!({"operation":"trace_start","context":context,"options":{"performance":{}}}),"TRACE_NATIVE_OPTIONS_UNSUPPORTED",false).await;
                    assert_eq!(operation(&mut mcp,target,json!({"operation":"trace_status"})).await["active"],false);
                    json!({"nativePresetRefusedBeforeCapture":true})
                }
            }).catch_unwind().await;
            let began = std::time::Instant::now();
            disconnect_with_diagnostics(&fixture, &mut mcp, target, engine, mode, context).await;
            let cleanup_ms = began.elapsed().as_secs_f64() * 1000.0;
            match result {
                Ok(metrics) => eprintln!(
                    "Performance lifecycle {engine} {mode}: {metrics}; connectMs={connect_ms}; cleanupMs={cleanup_ms}"
                ),
                Err(panic) => {
                    fixture.cli(&["disconnect"]).await;
                    mcp.tool("disconnect", json!({})).await;
                    mcp.stop().await;
                    std::panic::resume_unwind(panic);
                }
            }
        }
    }
    fixture.cli(&["disconnect"]).await;
    mcp.tool("disconnect", json!({})).await;
    mcp.stop().await;
}
