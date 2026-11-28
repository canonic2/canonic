//! Owned loopback documents and interruption boundaries for the installed clock.
use super::*;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

const EPOCH: i64 = 1_700_000_000_000;
const FIRST: &str = "()=>JSON.stringify({first:acmeFirst,ticks:acmeTicks,date:Date.now(),performance:performance.now()})";

struct ClockSite {
    url: String,
    started: Arc<AtomicBool>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for ClockSite {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl ClockSite {
    async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let port = listener.local_addr().unwrap().port();
        let started = Arc::new(AtomicBool::new(false));
        let signal = started.clone();
        let task = tokio::spawn(async move {
            let mut requests = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    accepted=listener.accept(),if requests.len()<32=>{
                        let (stream,_)=accepted.unwrap();
                        requests.spawn(respond(stream,port,signal.clone()));
                    },
                    Some(result)=requests.join_next(),if !requests.is_empty()=>{result.unwrap();},
                }
            }
        });
        Self {
            url: format!("http://127.0.0.1:{port}"),
            started,
            task,
        }
    }
}
async fn respond(mut stream: tokio::net::TcpStream, port: u16, started: Arc<AtomicBool>) {
    let mut head = Vec::new();
    let mut chunk = [0; 1024];
    while head.len() < 8192 && !head.windows(4).any(|part| part == b"\r\n\r\n") {
        let count = stream.read(&mut chunk).await.unwrap();
        if count == 0 {
            return;
        }
        head.extend_from_slice(&chunk[..count]);
    }
    let parent = head.starts_with(b"GET /parent ");
    if head.starts_with(b"GET /started ") {
        started.store(true, Ordering::SeqCst);
    }
    let child = if parent {
        format!("<iframe id=child src='http://localhost:{port}/child'></iframe>")
    } else {
        String::new()
    };
    let html = format!(
        "<!doctype html><script>globalThis.acmeFirst={{origin:performance.timeOrigin,date:Date.now(),performance:performance.now()}};globalThis.acmeTicks=0;setTimeout(()=>acmeTicks++,0);</script><body style='background:#27a'><button>Acme clock</button>{child}"
    );
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{html}",
        html.len()
    );
    let _ = stream.write_all(response.as_bytes()).await;
}
async fn context(mcp: &mut Mcp, target: &str) -> Value {
    operation(
        mcp,
        target,
        json!({"operation":"context_create","options":{"viewport":{"width":800,"height":600},"deviceScaleFactor":1}}),
    )
    .await["context"]
        .clone()
}
async fn page(mcp: &mut Mcp, target: &str, context: &Value) -> Value {
    operation(
        mcp,
        target,
        json!({"operation":"page_create","context":context}),
    )
    .await["page"]
        .clone()
}
async fn clock(mcp: &mut Mcp, target: &str, context: &Value, action: Value) -> Value {
    operation(
        mcp,
        target,
        json!({"operation":"context_clock","context":context,"action":action}),
    )
    .await
}
async fn evaluate(mcp: &mut Mcp, target: &str, scope: &Value, expression: &str) -> Value {
    read(mcp, target, scope, &json!(expression)).await
}
async fn navigate(mcp: &mut Mcp, target: &str, page: &Value, url: &str) {
    operation(mcp,target,json!({"operation":"page_navigate","page":page,"navigation":{"kind":"goto","url":url,"waitUntil":"load"}})).await;
}
async fn pause(mcp: &mut Mcp, target: &str, context: &Value) {
    clock(
        mcp,
        target,
        context,
        json!({"kind":"install","timeMs":EPOCH}),
    )
    .await;
    clock(
        mcp,
        target,
        context,
        json!({"kind":"set_fixed_time","timeMs":EPOCH}),
    )
    .await;
    clock(
        mcp,
        target,
        context,
        json!({"kind":"pause_at","timeMs":EPOCH}),
    )
    .await;
}

async fn bootstrap_reference(fixture: &Fixture, engine: &str, mode: &str) -> Value {
    let output = tokio::process::Command::new(
        fixture
            .cache
            .join("node-24.12.0/node-v24.12.0-darwin-x64/bin/node"),
    )
    .arg(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/browser-clock-bootstrap-baseline.cjs"
    ))
    .arg(fixture.cache.join("playwright-core-1.64.1/package"))
    .args([engine, mode])
    .arg(fixture.cache.join("browsers"))
    .env_clear()
    .env("PATH", "/usr/bin:/bin")
    .stdin(std::process::Stdio::null())
    .kill_on_drop(true)
    .output();
    let output = tokio::time::timeout(Duration::from_secs(120), output)
        .await
        .unwrap()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

pub(super) async fn exercise(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    engine: &str,
    mode: &str,
) {
    let reference = bootstrap_reference(fixture, engine, mode).await;
    let site = ClockSite::start().await;
    let owned = context(mcp, target).await;
    pause(mcp, target, &owned).await;
    let main = page(mcp, target, &owned).await;
    navigate(mcp, target, &main, &format!("{}/parent", site.url)).await;
    let child = operation(
        mcp,
        target,
        json!({"operation":"frame_enter","page":main,"locator":{"kind":"css","selector":"#child"}}),
    )
    .await["frame"]
        .clone();
    let first = evaluate(mcp, target, &main, FIRST).await;
    let child_first = evaluate(mcp, target, &child, FIRST).await;
    assert_eq!(first["first"]["date"], EPOCH);
    assert_eq!(child_first["first"]["date"], EPOCH);
    assert!(first["first"]["origin"].as_f64().unwrap() > 0.0);
    assert!(child_first["first"]["origin"].as_f64().unwrap() > 0.0);
    tokio::time::sleep(Duration::from_millis(200)).await;
    let paused_main = evaluate(mcp, target, &main, FIRST).await;
    let paused_child = evaluate(mcp, target, &child, FIRST).await;
    assert_eq!(paused_main["ticks"], 0);
    assert_eq!(paused_child["ticks"], 0);
    clock(
        mcp,
        target,
        &owned,
        json!({"kind":"run_for","durationMs":10}),
    )
    .await;
    assert_eq!(evaluate(mcp, target, &main, FIRST).await["ticks"], 1);
    assert_eq!(evaluate(mcp, target, &child, FIRST).await["ticks"], 1);
    let media = paused_media(mcp, target, &owned, &main).await;
    let mut report = json!({"first":first,"crossSiteFrame":child_first,"bootstrapReference":reference,"pausedMain":paused_main,"pausedChild":paused_child,"media":media});
    if engine == "chrome" {
        report["native"] = native_documents(mcp, target, &owned, &main, &site).await;
    }
    operation(
        mcp,
        target,
        json!({"operation":"context_close","context":owned}),
    )
    .await;
    if engine == "chrome" {
        report["failure"] = failed_context(mcp, target, &site).await;
        report["cancellation"] = cancelled_clock(mcp, target, &site).await;
    }
    std::fs::write(
        fixture
            .workspace
            .path()
            .join(format!("clock-boundaries-{engine}-{mode}.json")),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
}

async fn paused_media(mcp: &mut Mcp, target: &str, context: &Value, page: &Value) -> Value {
    let observed = operation(
        mcp,
        target,
        json!({"operation":"page_read","page":page,"read":"geometry"}),
    )
    .await;
    clock(
        mcp,
        target,
        context,
        json!({"kind":"set_system_time","timeMs":EPOCH+86_400_000}),
    )
    .await;
    operation(mcp,target,json!({"operation":"page_input","page":page,"observation":observed["inputObservation"],"actions":[{"kind":"key_down","key":"Shift"},{"kind":"key_up","key":"ShiftLeft"}]})).await;
    let preview = operation(
        mcp,
        target,
        json!({"operation":"preview_start","page":page}),
    )
    .await["preview"]
        .clone();
    let recording = operation(
        mcp,
        target,
        json!({"operation":"recording_start","page":page,"options":{"maxDurationMs":500}}),
    )
    .await["recording"]
        .clone();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    let terminal = loop {
        let value = operation(
            mcp,
            target,
            json!({"operation":"recording_status","recording":recording}),
        )
        .await;
        if matches!(
            value["status"].as_str(),
            Some("complete" | "partial" | "failed")
        ) {
            break value;
        }
        assert!(tokio::time::Instant::now() < deadline, "{value}");
        tokio::time::sleep(Duration::from_millis(25)).await;
    };
    // Dropped frames produce a declared partial movie, not a clock failure.
    // The paused-clock contract requires a decoded, full-duration recording
    // without source loss or encoder failure, and truthful drop reporting.
    let stats = &terminal["statistics"];
    assert_eq!(stats["reason"], Value::Null, "{terminal}");
    assert_eq!(stats["failure"], Value::Null, "{terminal}");
    assert_eq!(stats["sourceInterruptions"], 0, "{terminal}");
    assert_eq!(stats["discontinuities"], 0, "{terminal}");
    assert_eq!(
        terminal["status"],
        if stats["droppedFrames"].as_u64().unwrap() > 0 {
            "partial"
        } else {
            "complete"
        },
        "{terminal}"
    );
    assert_eq!(terminal["verification"]["decodable"], true, "{terminal}");
    assert_eq!(terminal["statistics"]["durationMs"], 500);
    let live = operation(
        mcp,
        target,
        json!({"operation":"preview_status","preview":preview}),
    )
    .await;
    assert!(live["sequence"].as_u64().unwrap() > 0, "{live}");
    assert_eq!(
        evaluate(mcp, target, page, "()=>JSON.stringify(Date.now())").await,
        EPOCH + 86_400_000
    );
    operation(
        mcp,
        target,
        json!({"operation":"preview_stop","preview":preview}),
    )
    .await;
    json!({"recording":terminal,"preview":live})
}

async fn native_documents(
    mcp: &mut Mcp,
    target: &str,
    context: &Value,
    main: &Value,
    site: &ClockSite,
) -> Value {
    clock(
        mcp,
        target,
        context,
        json!({"kind":"set_fixed_time","timeMs":EPOCH}),
    )
    .await;
    evaluate(mcp,target,main,"()=>{globalThis.acmeZero=0;globalThis.acmeInterval=setInterval(()=>acmeZero++,0);return JSON.stringify(true)}").await;
    clock(
        mcp,
        target,
        context,
        json!({"kind":"run_for","durationMs":5}),
    )
    .await;
    assert_eq!(
        evaluate(mcp, target, main, "()=>JSON.stringify(acmeZero)").await,
        5
    );
    clock(
        mcp,
        target,
        context,
        json!({"kind":"fast_forward","durationMs":5}),
    )
    .await;
    assert_eq!(
        evaluate(
            mcp,
            target,
            main,
            "()=>{clearInterval(acmeInterval);return JSON.stringify(acmeZero)}"
        )
        .await,
        6
    );
    clock(
        mcp,
        target,
        context,
        json!({"kind":"pause_at","timeMs":EPOCH+100}),
    )
    .await;
    clock(
        mcp,
        target,
        context,
        json!({"kind":"run_for","durationMs":10}),
    )
    .await;
    let before = evaluate(
        mcp,
        target,
        main,
        "()=>JSON.stringify({date:Date.now(),ticks:performance.now()})",
    )
    .await;
    let future = page(mcp, target, context).await;
    let after = evaluate(
        mcp,
        target,
        &future,
        "()=>JSON.stringify({date:Date.now(),ticks:performance.now()})",
    )
    .await;
    assert_eq!(
        before, after,
        "Fixed Date and elapsed ticks must reconstruct together"
    );
    let popup_expression = format!(
        "()=>{{setTimeout(()=>{{globalThis.acmePopup=window.open({})}},1);return JSON.stringify(true)}}",
        json!(format!("{}/popup", site.url))
    );
    evaluate(mcp, target, main, &popup_expression).await;
    clock(
        mcp,
        target,
        context,
        json!({"kind":"run_for","durationMs":1}),
    )
    .await;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    let popup = loop {
        let value = evaluate(
            mcp,
            target,
            main,
            "()=>JSON.stringify(acmePopup.acmeFirst||null)",
        )
        .await;
        if !value.is_null() {
            break value;
        }
        assert!(tokio::time::Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(25)).await;
    };
    assert_eq!(popup["date"], EPOCH);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(
        evaluate(mcp, target, main, "()=>JSON.stringify(acmePopup.acmeTicks)").await,
        0
    );
    json!({"existing":before,"future":after,"callbackPopup":popup})
}

async fn settlement(mcp: &mut Mcp, target: &str, submitted: &Value) -> Value {
    let status = mcp
        .tool(
            "browser_status",
            json!({"targetId":target,"operationId":submitted["operation"]["id"],"waitMs":30000}),
        )
        .await;
    assert_eq!(status["operation"]["settled"], true, "{status}");
    status["operation"].clone()
}
async fn failed_context(mcp: &mut Mcp, target: &str, site: &ClockSite) -> Value {
    let failed = context(mcp, target).await;
    pause(mcp, target, &failed).await;
    let main = page(mcp, target, &failed).await;
    navigate(mcp, target, &main, &format!("{}/failed", site.url)).await;
    evaluate(
        mcp,
        target,
        &main,
        "()=>{setTimeout(()=>{throw Error('Acme clock callback')},1);return JSON.stringify(true)}",
    )
    .await;
    let submitted=mcp.tool("browser_send",json!({"targetId":target,"request":{"operation":"context_clock","context":failed,"action":{"kind":"run_for","durationMs":1}}})).await;
    let error = settlement(mcp, target, &submitted).await;
    assert_eq!(error["code"], "CLOCK_EXECUTION_FAILED", "{error}");
    collectors::refused(
        mcp,
        target,
        json!({"operation":"context_clock","context":failed,"action":{"kind":"resume"}}),
        "CLOCK_PENDING",
        true,
    )
    .await;
    let attached = page(mcp, target, &failed).await;
    let native = evaluate(mcp, target, &attached, "()=>JSON.stringify(Date.now())").await;
    assert!(native.as_i64().unwrap() > EPOCH + 86_400_000);
    let foreign = context(mcp, target).await;
    let foreign_page = page(mcp, target, &foreign).await;
    assert_eq!(
        evaluate(mcp, target, &foreign_page, "()=>JSON.stringify(6*7)").await,
        42
    );
    operation(
        mcp,
        target,
        json!({"operation":"context_close","context":failed}),
    )
    .await;
    assert_eq!(
        evaluate(mcp, target, &foreign_page, "()=>JSON.stringify(6*7)").await,
        42
    );
    operation(
        mcp,
        target,
        json!({"operation":"context_close","context":foreign}),
    )
    .await;
    error
}
async fn cancelled_clock(mcp: &mut Mcp, target: &str, site: &ClockSite) -> Value {
    let context = context(mcp, target).await;
    pause(mcp, target, &context).await;
    let page = page(mcp, target, &context).await;
    navigate(mcp, target, &page, &format!("{}/cancel", site.url)).await;
    let expression = "()=>{globalThis.acmeCalls=0;setInterval(()=>{if(++acmeCalls===1)fetch('/started')},1);return JSON.stringify(true)}";
    evaluate(mcp, target, &page, expression).await;
    let submitted=mcp.tool("browser_send",json!({"targetId":target,"request":{"operation":"context_clock","context":context,"action":{"kind":"run_for","durationMs":60000}}})).await;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while !site.started.load(Ordering::SeqCst) {
        assert!(
            tokio::time::Instant::now() < deadline,
            "Clock callback never reached the owned loopback witness"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let cancelled = mcp
        .tool(
            "browser_cancel",
            json!({"targetId":target,"operationId":submitted["operation"]["id"]}),
        )
        .await;
    let terminal = settlement(mcp, target, &submitted).await;
    assert_ne!(terminal["outcome"], "succeeded", "{terminal}");
    assert_eq!(terminal["outcome"], "unknown", "{terminal}");
    assert_eq!(terminal["result"]["replayed"], false, "{terminal}");
    json!({"cancelled":cancelled,"terminal":terminal,"callbackWitness":true})
}
