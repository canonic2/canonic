//! Caller pauses precede actionability where the pinned debugger requires it.
use super::*;
use std::sync::{
    Arc,
    atomic::{AtomicU8, Ordering},
};

pub(super) struct PageControl {
    pub(super) url: String,
    phase: Arc<AtomicU8>,
    changed: Arc<tokio::sync::Notify>,
    task: tokio::task::JoinHandle<()>,
}
impl PageControl {
    pub(super) async fn start(html: Option<&'static str>) -> Self {
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let phase = Arc::new(AtomicU8::new(0));
        let changed = Arc::new(tokio::sync::Notify::new());
        let state = phase.clone();
        let notify = changed.clone();
        let task = tokio::spawn(async move {
            let mut requests = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    accepted=listener.accept(), if requests.len()<64=>{
                        let (stream,_)=accepted.unwrap();
                        requests.spawn(page_response(stream,state.clone(),notify.clone(),html));
                    },
                    Some(result)=requests.join_next(),if !requests.is_empty()=>{result.unwrap();},
                }
            }
        });
        Self {
            url,
            phase,
            changed,
            task,
        }
    }
    pub(super) fn advance(&self, phase: u8) {
        self.phase.store(phase, Ordering::Release);
        self.changed.notify_waiters();
    }
}
impl Drop for PageControl {
    fn drop(&mut self) {
        self.task.abort();
    }
}
async fn page_response(
    mut stream: tokio::net::TcpStream,
    phase: Arc<AtomicU8>,
    changed: Arc<tokio::sync::Notify>,
    html: Option<&'static str>,
) {
    let mut bytes = Vec::new();
    let mut buffer = [0; 1024];
    while bytes.len() < 8192 && !bytes.windows(4).any(|part| part == b"\r\n\r\n") {
        let count = stream.read(&mut buffer).await.unwrap();
        if count == 0 {
            return;
        }
        bytes.extend_from_slice(&buffer[..count]);
    }
    let text = String::from_utf8_lossy(&bytes);
    let path = text.split_whitespace().nth(1).unwrap_or("");
    let wanted = match path {
        "/attach" => 1,
        "/visible" => 2,
        _ => 0,
    };
    while wanted > 0 {
        let notified = changed.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        if phase.load(Ordering::Acquire) >= wanted {
            break;
        }
        notified.await;
    }
    let html = html.unwrap_or("<!doctype html><button id=first>Acme</button><input id=count value=0 readonly><script>(async()=>{await fetch('/attach');const button=document.createElement('button');button.id='missing';button.hidden=true;button.onclick=()=>document.querySelector('#count').value=String(Number(document.querySelector('#count').value)+1);document.body.append(button);await fetch('/visible');button.hidden=false;})()</script>");
    let body = if wanted == 0 { html } else { "Acme ready" };
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes()).await;
}

async fn reference(fixture: &Fixture, engine: &str, mode: &str) -> Value {
    let result = Command::new(
        fixture
            .cache
            .join("node-24.12.0/node-v24.12.0-darwin-x64/bin/node"),
    )
    .arg(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/browser-debugger-order-baseline.cjs"),
    )
    .arg(fixture.cache.join("playwright-core-1.64.1/package"))
    .arg(engine)
    .arg(mode)
    .current_dir(fixture.workspace.path())
    .env_clear()
    .env("PATH", "/usr/bin:/bin")
    .env("PW_LANG_NAME", "javascript")
    .env("PLAYWRIGHT_BROWSERS_PATH", fixture.cache.join("browsers"))
    .output()
    .await
    .unwrap();
    std::fs::write(
        fixture
            .workspace
            .path()
            .join(format!("debugger-order-sdk-{engine}-{mode}.json")),
        &result.stdout,
    )
    .unwrap();
    std::fs::write(
        fixture
            .workspace
            .path()
            .join(format!("debugger-order-sdk-{engine}-{mode}.stderr")),
        &result.stderr,
    )
    .unwrap();
    assert!(
        result.status.success(),
        "Unchanged SDK debugger reference: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    let report: Value = serde_json::from_slice(&result.stdout).unwrap();
    for kind in ["pause", "next", "runTo"] {
        assert_eq!(
            report[kind],
            json!({"pausedBeforeAttachment":true,"hiddenClicks":0,"clicks":1})
        );
    }
    report
}
async fn pause(mcp: &mut Mcp, target: &str, context: &Value) -> Value {
    let until = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        let status = operation(
            mcp,
            target,
            json!({"operation":"context_debugger","context":context,"action":{"kind":"status"}}),
        )
        .await;
        if status["paused"] == true {
            return status["pauseId"].clone();
        }
        assert!(
            tokio::time::Instant::now() < until,
            "Pre-actionability caller pause unavailable: {status}"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}
async fn clicks(mcp: &mut Mcp, target: &str, page: &Value) -> Value {
    let read=operation(mcp,target,json!({"operation":"page_read","page":page,"locator":{"kind":"css","selector":"#count"},"read":"state","maxBytes":4096})).await;
    json!(
        read["result"]["value"]["value"]
            .as_str()
            .unwrap()
            .parse::<u64>()
            .unwrap()
    )
}
struct Ledger {
    file: std::fs::File,
    started: std::time::Instant,
}

async fn held_input_running(fixture: &Fixture, target: &str, operation: &Value) {
    let connection = target.strip_prefix("browser:").unwrap();
    let until = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        for namespace in std::fs::read_dir(fixture.workspace.data_root())
            .unwrap()
            .flatten()
        {
            let directory = namespace
                .path()
                .join("browser")
                .join(format!("browser-{connection}"));
            for entry in std::fs::read_dir(directory).into_iter().flatten().flatten() {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if name.len() != 13
                    || !name.ends_with(".json")
                    || !name.as_bytes()[..8].iter().all(u8::is_ascii_digit)
                {
                    continue;
                }
                if let Ok(bytes) = std::fs::read(entry.path())
                    && let Ok(record) = serde_json::from_slice::<Value>(&bytes)
                    && record["operationId"] == *operation
                    && record["outcome"] == "running"
                {
                    return;
                }
            }
        }
        assert!(
            tokio::time::Instant::now() < until,
            "Original input never entered its running phase"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

async fn paused_evaluation(
    mcp: &mut Mcp,
    target: &str,
    context: &Value,
    ordinary: Option<&Fixture>,
) -> Value {
    let page = operation(
        mcp,
        target,
        json!({"operation":"page_create","context":context,"select":false}),
    )
    .await["page"]
        .clone();
    let html = r#"<button id=click>Acme</button><script>globalThis.acmeClicks=0;document.querySelector('button').onclick=()=>acmeClicks++;</script><iframe id=child srcdoc="<button>Acme child</button>"></iframe>"#;
    operation(mcp,target,json!({"operation":"page_navigate","page":page,"navigation":{"kind":"set_content","html":html,"waitUntil":"load"}})).await;
    if ordinary.is_some() {
        operation(mcp,target,json!({"operation":"evaluate","scope":page,"expression":"()=>{document.querySelector('#click').hidden=true;}","isFunction":true})).await;
    }
    let frame = operation(
        mcp,
        target,
        json!({"operation":"frame_enter","page":page,"locator":{"kind":"css","selector":"#child"}}),
    )
    .await["frame"]
        .clone();
    let source=operation(mcp,target,json!({"operation":"evaluate","scope":page,"expression":"()=>({label:'Acme owner'})","isFunction":true,"returnHandle":true})).await["handle"].clone();
    if ordinary.is_none() {
        operation(
            mcp,
            target,
            json!({"operation":"context_debugger","context":context,"action":{"kind":"pause"}}),
        )
        .await;
    }
    let input=mcp.tool("browser_send",json!({"targetId":target,"request":{"operation":"page_action","page":page,"locator":{"kind":"css","selector":"#click"},"action":{"kind":"click"}}})).await;
    let pause_id = if let Some(fixture) = ordinary {
        held_input_running(fixture, target, &input["operation"]["id"]).await;
        Value::Null
    } else {
        pause(mcp, target, context).await
    };
    let page_value=operation(mcp,target,json!({"operation":"evaluate","scope":page,"expression":"()=>{globalThis.acmeSequence=['page'];return acmeClicks;}","isFunction":true})).await["value"]["value"].clone();
    let frame_value=operation(mcp,target,json!({"operation":"evaluate","scope":frame,"expression":"()=>document.querySelector('button').textContent","isFunction":true})).await["value"]["value"].clone();
    let retained=operation(mcp,target,json!({"operation":"evaluate","scope":source,"expression":"owner=>({label:owner.label+' retained'})","isFunction":true,"returnHandle":true})).await["handle"].clone();
    let handle_value=operation(mcp,target,json!({"operation":"evaluate","scope":retained,"expression":"owner=>owner.label","isFunction":true})).await["value"]["value"].clone();
    let read_value = operation(
        mcp,
        target,
        json!({"operation":"handle_read","handle":retained}),
    )
    .await;
    let read_value = read_value["value"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["key"] == "label")
        .unwrap()["value"]["value"]
        .clone();
    let selector_value=operation(mcp,target,json!({"operation":"selector_evaluate","scope":page,"options":{"locator":{"kind":"css","selector":"#click"},"expression":"button=>button.textContent","isFunction":true}})).await["value"]["value"].clone();
    let batch=mcp.tool("act",json!({"targetId":target,"actions":[
        {"action":"evaluate","scope":page,"expression":"()=>{acmeSequence.push('first');return acmeSequence.length;}","isFunction":true},
        {"action":"evaluate","scope":page,"expression":"()=>{acmeSequence.push('second');return acmeSequence.join('|');}","isFunction":true}
    ]})).await;
    let batch = completed(mcp, target, batch).await;
    if ordinary.is_none() {
        let status = operation(
            mcp,
            target,
            json!({"operation":"context_debugger","context":context,"action":{"kind":"status"}}),
        )
        .await;
        assert_eq!(status["paused"], true);
        assert_eq!(status["pauseId"], pause_id);
    }
    let pending = mcp
        .tool(
            "browser_status",
            json!({"targetId":target,"operationId":input["operation"]["id"]}),
        )
        .await;
    assert_eq!(pending["operation"]["settled"], false, "{pending}");
    for handle in [retained, source] {
        operation(
            mcp,
            target,
            json!({"operation":"handle_dispose","handle":handle}),
        )
        .await;
    }
    if ordinary.is_some() {
        operation(mcp,target,json!({"operation":"evaluate","scope":page,"expression":"()=>{document.querySelector('#click').hidden=false;}","isFunction":true})).await;
    } else {
        operation(mcp,target,json!({"operation":"context_debugger","context":context,"action":{"kind":"resume","pauseId":pause_id}})).await;
    }
    completed(mcp, target, input).await;
    let clicks=operation(mcp,target,json!({"operation":"evaluate","scope":page,"expression":"()=>acmeClicks","isFunction":true})).await["value"]["value"].clone();
    operation(
        mcp,
        target,
        json!({"operation":"context_close","context":context}),
    )
    .await;
    json!({"pageValue":page_value,"frameValue":frame_value,"handleValue":handle_value,"readValue":read_value,"selectorValue":selector_value,
        "first":batch["results"][0]["value"]["value"],"second":batch["results"][1]["value"]["value"],"clicks":clicks,"held":true})
}

#[tokio::test]
async fn installed_step_four_paused_evaluation_checkpoint() {
    caller_evaluation_checkpoint(false).await;
}

#[tokio::test]
async fn installed_step_four_ordinary_input_evaluation_checkpoint() {
    caller_evaluation_checkpoint(true).await;
}

async fn caller_evaluation_checkpoint(ordinary: bool) {
    let fixture = installed_fixture();
    let mut mcp = Mcp::start(&fixture).await;
    let mut failures = vec![];
    for mode in ["headless", "headed"] {
        for engine in if ordinary {
            &["chrome", "firefox", "webkit"][..]
        } else {
            &["firefox", "webkit"][..]
        } {
            let reference = Command::new(
                fixture
                    .cache
                    .join("node-24.12.0/node-v24.12.0-darwin-x64/bin/node"),
            )
            .arg(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("tests/fixtures/browser-paused-evaluation-baseline.cjs"),
            )
            .arg(fixture.cache.join("playwright-core-1.64.1/package"))
            .arg(engine)
            .arg(mode)
            .arg(if ordinary { "ordinary" } else { "paused" })
            .arg(fixture.cache.join("browsers"))
            .current_dir(fixture.workspace.path())
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("PW_LANG_NAME", "javascript")
            .env("PLAYWRIGHT_BROWSERS_PATH", fixture.cache.join("browsers"))
            .output()
            .await
            .unwrap();
            let prefix = if ordinary {
                "ordinary-input-evaluation"
            } else {
                "paused-evaluation"
            };
            std::fs::write(
                fixture
                    .workspace
                    .path()
                    .join(format!("{prefix}-{engine}-{mode}.json")),
                &reference.stdout,
            )
            .unwrap();
            std::fs::write(
                fixture
                    .workspace
                    .path()
                    .join(format!("{prefix}-{engine}-{mode}.stderr")),
                &reference.stderr,
            )
            .unwrap();
            assert!(
                reference.status.success(),
                "{}",
                String::from_utf8_lossy(&reference.stderr)
            );
            let expected: Value = serde_json::from_slice(&reference.stdout).unwrap();
            assert_eq!(
                expected,
                json!({"pageValue":0,"frameValue":"Acme child","handleValue":"Acme owner retained","readValue":"Acme owner retained","selectorValue":"Acme","first":2,"second":"page|first|second","clicks":1,"held":true})
            );
            let connected = mcp
                .tool("browser_connect", json!({"engine":engine,"mode":mode}))
                .await;
            let target = connected["targetId"].as_str().unwrap();
            let ready = fixture.ready_target(target).await;
            let context = &ready["browser"]["selectedContext"];
            let result = std::panic::AssertUnwindSafe(paused_evaluation(
                &mut mcp,
                target,
                context,
                ordinary.then_some(&fixture),
            ))
            .catch_unwind()
            .await;
            let closed = performance_lifecycle::disconnect_observed(
                &fixture, &mut mcp, target, engine, mode, context,
            )
            .await;
            if closed["result"]["structuredContent"]["ok"] != true {
                failures.push(format!("{engine}/{mode}: {closed}"));
            }
            match result {
                Ok(actual) => assert_eq!(actual, expected, "{engine}/{mode}"),
                Err(panic) => {
                    mcp.stop().await;
                    std::panic::resume_unwind(panic);
                }
            }
        }
    }
    mcp.stop().await;
    assert!(
        failures.is_empty(),
        "Original cleanup failures: {failures:?}"
    );
}
impl Ledger {
    fn new(fixture: &Fixture, engine: &str, mode: &str) -> Self {
        Self {
            file: std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(
                    fixture
                        .workspace
                        .path()
                        .join(format!("debugger-order-{engine}-{mode}-phases.jsonl")),
                )
                .unwrap(),
            started: std::time::Instant::now(),
        }
    }
    fn record(&mut self, kind: &str, stage: &str, value: Value) {
        use std::io::Write as _;
        writeln!(self.file,"{}",json!({"kind":kind,"stage":stage,"elapsedMs":self.started.elapsed().as_millis(),"value":value})).unwrap();
    }
}
async fn run(
    mcp: &mut Mcp,
    target: &str,
    report: &Value,
    control: &PageControl,
    ledger: &mut Ledger,
) {
    for kind in ["pause", "next", "runTo"] {
        ledger.record(kind, "begin", json!({"targetId":target}));
        control.advance(0);
        let context =
            operation(mcp, target, json!({"operation":"context_create"})).await["context"].clone();
        let page = operation(
            mcp,
            target,
            json!({"operation":"page_create","context":context,"select":false}),
        )
        .await["page"]
            .clone();
        operation(mcp,target,json!({"operation":"page_navigate","page":page,"navigation":{"kind":"goto","url":control.url,"waitUntil":"load"}})).await;
        ledger.record(kind, "page-ready", json!({"context":context,"page":page}));
        if kind == "runTo" {
            operation(mcp,target,json!({"operation":"context_debugger","context":context,"action":{"kind":"enable"}})).await;
        }
        operation(
            mcp,
            target,
            json!({"operation":"context_debugger","context":context,"action":{"kind":"pause"}}),
        )
        .await;
        if kind != "pause" {
            let first=mcp.tool("browser_send",json!({"targetId":target,"request":{"operation":"page_action","page":page,"locator":{"kind":"css","selector":"#first"},"action":{"kind":"click"}}})).await;
            let first_pause = pause(mcp, target, &context).await;
            ledger.record(
                kind,
                "bootstrap-paused",
                json!({"operationId":first["operation"]["id"],"pauseId":first_pause}),
            );
            let action = if kind == "next" {
                json!({"kind":"next","pauseId":first_pause})
            } else {
                json!({"kind":"run_to","pauseId":first_pause,"location":{"file":"Acme-debugger-order.rs","line":30,"column":3}})
            };
            operation(
                mcp,
                target,
                json!({"operation":"context_debugger","context":context,"action":action}),
            )
            .await;
            completed(mcp, target, first).await;
            ledger.record(kind, "bootstrap-complete", Value::Null);
        }
        let pending = if kind == "runTo" {
            mcp.tool("act",json!({"targetId":target,"actions":[{"action":"page_action","page":page,"locator":{"kind":"css","selector":"#missing"},"input":{"action":"at_source","source":{"file":"src/Acme-debugger-order.rs","line":30,"column":3},"input":{"action":"click"}}}]})).await
        } else {
            mcp.tool("browser_send",json!({"targetId":target,"request":{"operation":"page_action","page":page,"locator":{"kind":"css","selector":"#missing"},"action":{"kind":"click"}}})).await
        };
        ledger.record(
            kind,
            "missing-submitted",
            json!({"operationId":pending["operation"]["id"]}),
        );
        let id = pause(mcp, target, &context).await;
        ledger.record(kind, "missing-paused", json!({"pauseId":id}));
        assert_eq!(clicks(mcp, target, &page).await, 0);
        control.advance(1);
        let until = tokio::time::Instant::now() + Duration::from_secs(5);
        loop {
            let read=operation(mcp,target,json!({"operation":"page_read","page":page,"locator":{"kind":"css","selector":"#missing"},"read":"count","maxBytes":4096})).await;
            if read["result"]["value"] == 1 {
                ledger.record(kind, "attached", read);
                break;
            }
            assert!(
                tokio::time::Instant::now() < until,
                "Controlled element attachment unavailable: {read}"
            );
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        let resumed=operation(mcp,target,json!({"operation":"context_debugger","context":context,"action":{"kind":"resume","pauseId":id}})).await;
        ledger.record(kind, "resume-acknowledged", resumed);
        tokio::time::sleep(Duration::from_millis(50)).await;
        let hidden = clicks(mcp, target, &page).await;
        ledger.record(kind, "hidden-counter", hidden.clone());
        control.advance(2);
        ledger.record(kind, "visibility-phase-released", Value::Null);
        let done = completed(mcp, target, pending).await;
        ledger.record(kind, "missing-complete", done.clone());
        if kind == "runTo" {
            assert_eq!(done["completedActions"], 1);
        } else {
            assert_eq!(done["dispatchCompleted"], true);
        }
        assert_eq!(
            json!({"pausedBeforeAttachment":true,"hiddenClicks":hidden,"clicks":clicks(mcp,target,&page).await}),
            report[kind]
        );
        operation(
            mcp,
            target,
            json!({"operation":"context_close","context":context}),
        )
        .await;
    }
}
#[tokio::test]
async fn installed_step_four_debugger_order_checkpoint() {
    let fixture = installed_fixture();
    let mut mcp = Mcp::start(&fixture).await;
    let control = PageControl::start(None).await;
    let mut failures = vec![];
    for mode in ["headless", "headed"] {
        for engine in ["firefox", "webkit"] {
            let report = reference(&fixture, engine, mode).await;
            let connected = mcp
                .tool("browser_connect", json!({"engine":engine,"mode":mode}))
                .await;
            let target = connected["targetId"].as_str().unwrap();
            let ready = fixture.ready_target(target).await;
            let mut ledger = Ledger::new(&fixture, engine, mode);
            let result =
                std::panic::AssertUnwindSafe(run(&mut mcp, target, &report, &control, &mut ledger))
                    .catch_unwind()
                    .await;
            let closed = performance_lifecycle::disconnect_observed(
                &fixture,
                &mut mcp,
                target,
                engine,
                mode,
                &ready["browser"]["selectedContext"],
            )
            .await;
            if closed["result"]["structuredContent"]["ok"] != true {
                failures.push(format!("{engine}/{mode}: {closed}"));
            }
            if let Err(panic) = result {
                mcp.stop().await;
                std::panic::resume_unwind(panic);
            }
        }
    }
    mcp.stop().await;
    assert!(
        failures.is_empty(),
        "Original cleanup failures: {failures:?}"
    );
}
