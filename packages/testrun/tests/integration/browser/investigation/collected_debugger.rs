//! Collected Pause/Next retain their waiting caller before SDK readiness.
use super::*;

async fn reference(fixture: &Fixture, engine: &str, mode: &str) -> Value {
    let result = Command::new(
        fixture
            .cache
            .join("node-24.12.0/node-v24.12.0-darwin-x64/bin/node"),
    )
    .arg(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/browser-collected-debugger-baseline.cjs"),
    )
    .arg(fixture.cache.join("playwright-core-1.64.1/package"))
    .arg(engine)
    .arg(mode)
    .current_dir(fixture.workspace.path())
    .env_clear()
    .env("PATH", "/usr/bin:/bin")
    .env("PLAYWRIGHT_BROWSERS_PATH", fixture.cache.join("browsers"))
    .output()
    .await
    .unwrap();
    for (extension, bytes) in [("json", &result.stdout), ("stderr", &result.stderr)] {
        std::fs::write(
            fixture.workspace.path().join(format!(
                "collected-debugger-reference-{engine}-{mode}.{extension}"
            )),
            bytes,
        )
        .unwrap();
    }
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let report: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(report["ownedProcesses"], 0, "{report}");
    assert_eq!(report["cases"].as_array().unwrap().len(), 2);
    for row in report["cases"].as_array().unwrap() {
        assert_eq!(row["waiting"]["status"], "running", "{row}");
        assert_eq!(
            row["beforeReady"],
            json!({"paused":false,"settled":false}),
            "{row}"
        );
        assert_eq!(row["beforeResume"], 0, "{row}");
        assert_eq!(row["afterResume"], 1, "{row}");
        assert_eq!(row["terminal"], "success", "{row}");
    }
    report
}

#[tokio::test]
async fn pinned_collected_debugger_reference_checkpoint() {
    let fixture = installed_fixture();
    for mode in ["headless", "headed"] {
        for engine in ["firefox", "webkit"] {
            reference(&fixture, engine, mode).await;
        }
    }
}

pub(super) const PAGE: &str = concat!(
    r#"<!doctype html><link rel=icon href="data:,"><button id=first>Acme first</button><button id=waiting hidden disabled>Acme waiting</button><input id=count value=0 readonly><script>document.querySelector('#waiting').onclick=()=>document.querySelector('#count').value=String(Number(document.querySelector('#count').value)+1);(async()=>{await fetch('/visible');const button=document.querySelector('#waiting');button.hidden=false;button.disabled=false;})()</script>"#,
    "<script>(",
    include_str!("../../../fixtures/browser-readiness-observation.js"),
    ")('#waiting');</script>"
);

fn click(page: &Value, selector: &str) -> Value {
    json!({"operation":"page_action","page":page,"locator":{"kind":"css","selector":selector},"action":{"kind":"click"}})
}
pub(super) async fn debug(mcp: &mut Mcp, target: &str, context: &Value, action: Value) -> Value {
    operation(
        mcp,
        target,
        json!({"operation":"context_debugger","context":context,"action":action}),
    )
    .await
}
pub(super) async fn pause(mcp: &mut Mcp, target: &str, context: &Value) -> Value {
    let until = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        let status = debug(mcp, target, context, json!({"kind":"status"})).await;
        if status["paused"] == true {
            return status["pauseId"].clone();
        }
        assert!(
            tokio::time::Instant::now() < until,
            "Collected caller pause unavailable: {status}"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}
pub(super) async fn count(mcp: &mut Mcp, target: &str, page: &Value) -> Value {
    let read=operation(mcp,target,json!({"operation":"page_read","page":page,"locator":{"kind":"css","selector":"#count"},"read":"state","maxBytes":4096})).await;
    json!(
        read["result"]["value"]["value"]
            .as_str()
            .unwrap()
            .parse::<u64>()
            .unwrap()
    )
}
pub(super) async fn calls(mcp: &mut Mcp, target: &str, context: &Value, private: bool) -> Value {
    debug(
        mcp,
        target,
        context,
        json!({"kind":"calls","limit":32,"includeDetails":private}),
    )
    .await
}

async fn cell(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    engine: &str,
    mode: &str,
    expected: &Value,
) {
    for (index, next) in [false, true].into_iter().enumerate() {
        let control = debugger_order::PageControl::start(Some(PAGE)).await;
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
        debug(mcp, target, &context, json!({"kind":"enable"})).await;
        let requested=fixture.cli(&["browser","send","--target",target,&json!({"operation":"context_debugger","context":context,"action":{"kind":"pause"}}).to_string()]).await;
        completed(mcp, target, requested).await;
        let mut first_pause = Value::Null;
        if next {
            let first = mcp
                .tool(
                    "browser_send",
                    json!({"targetId":target,"request":click(&page,"#first")}),
                )
                .await;
            first_pause = pause(mcp, target, &context).await;
            let step=mcp.tool("act",json!({"targetId":target,"actions":[{"action":"context_debugger","context":context,"input":{"action":"next","pauseId":first_pause}}]})).await;
            let stepped = completed(mcp, target, step).await;
            assert_eq!(stepped["results"].as_array().unwrap().len(), 1);
            completed(mcp, target, first).await;
        }
        let before = calls(mcp, target, &context, false).await;
        let previous = before["calls"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row["callId"].clone())
            .collect::<Vec<_>>();
        let pending = mcp
            .tool(
                "browser_send",
                json!({"targetId":target,"request":click(&page,"#waiting")}),
            )
            .await;
        let until = tokio::time::Instant::now() + Duration::from_secs(5);
        let waiting = loop {
            let observed = calls(mcp, target, &context, true).await;
            if let Some(row) = observed["calls"].as_array().unwrap().iter().find(|row| {
                row["status"] == "running"
                    && !previous.contains(&row["callId"])
                    && row["details"]["title"]
                        .as_str()
                        .is_some_and(|title| title.to_ascii_lowercase().contains("click"))
            }) {
                break row.clone();
            }
            assert!(
                tokio::time::Instant::now() < until,
                "Waiting caller absent from collection: {observed}"
            );
            tokio::time::sleep(Duration::from_millis(25)).await;
        };
        let status = debug(mcp, target, &context, json!({"kind":"status"})).await;
        assert_eq!(status["callCollectionEnabled"], true);
        assert_eq!(status["stateKnown"], next);
        assert_eq!(
            status["paused"],
            if next { json!(false) } else { Value::Null }
        );
        assert_eq!(status["pauseRequested"], true);
        assert!(status["pauseId"].is_null());
        let running = mcp
            .tool(
                "browser_status",
                json!({"targetId":target,"operationId":pending["operation"]["id"]}),
            )
            .await;
        assert_eq!(running["operation"]["settled"], false, "{running}");
        assert_eq!(count(mcp, target, &page).await, 0);
        let metadata = calls(mcp, target, &context, false).await;
        assert!(
            metadata["calls"]
                .as_array()
                .unwrap()
                .iter()
                .all(|row| row.get("details").is_none())
        );
        control.advance(2);
        let paused = pause(mcp, target, &context).await;
        if next {
            assert_ne!(paused, first_pause);
        }
        assert_eq!(
            count(mcp, target, &page).await,
            expected["cases"][index]["beforeResume"]
        );
        assert_eq!(
            debug(mcp, target, &context, json!({"kind":"status"})).await["pauseId"],
            paused
        );
        debug(
            mcp,
            target,
            &context,
            json!({"kind":"resume","pauseId":paused}),
        )
        .await;
        completed(mcp, target, pending).await;
        assert_eq!(
            count(mcp, target, &page).await,
            expected["cases"][index]["afterResume"]
        );
        let until = tokio::time::Instant::now() + Duration::from_secs(5);
        let terminal = loop {
            let observed = calls(mcp, target, &context, false).await;
            if let Some(row) = observed["calls"].as_array().unwrap().iter().find(|row| {
                row["callId"] == waiting["callId"]
                    && row["status"] == expected["cases"][index]["terminal"]
            }) {
                break row.clone();
            }
            assert!(
                tokio::time::Instant::now() < until,
                "Original caller terminal update unavailable: {observed}"
            );
            tokio::time::sleep(Duration::from_millis(25)).await;
        };
        std::fs::write(fixture.workspace.path().join(format!("collected-debugger-installed-{engine}-{mode}-{index}.json")),serde_json::to_vec_pretty(&json!({"waiting":waiting,"beforeReady":status,"running":running["operation"],"pauseId":paused,"terminal":terminal,"reference":expected["cases"][index]})).unwrap()).unwrap();
        operation(
            mcp,
            target,
            json!({"operation":"context_close","context":context}),
        )
        .await;
    }
}

#[tokio::test]
async fn installed_step_four_collected_debugger_checkpoint() {
    let fixture = installed_fixture();
    let mut mcp = Mcp::start(&fixture).await;
    let mut failures = vec![];
    for mode in ["headless", "headed"] {
        for engine in ["firefox", "webkit"] {
            let expected = reference(&fixture, engine, mode).await;
            let connected = fixture
                .cli(&["browser", "connect", "--engine", engine, "--mode", mode])
                .await;
            let target = connected["targetId"].as_str().unwrap();
            let ready = fixture.ready_target(target).await;
            let result = std::panic::AssertUnwindSafe(cell(
                &fixture, &mut mcp, target, engine, mode, &expected,
            ))
            .catch_unwind()
            .await;
            if result.is_err() {
                let status = mcp.tool("browser_status", json!({"targetId":target})).await;
                std::fs::write(
                    fixture
                        .workspace
                        .path()
                        .join(format!("collected-debugger-failure-{engine}-{mode}.json")),
                    serde_json::to_vec_pretty(&status).unwrap(),
                )
                .unwrap();
            }
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
                failures.push(closed);
            }
            if let Err(panic) = result {
                fixture.cli(&["disconnect"]).await;
                mcp.tool("disconnect", json!({})).await;
                mcp.stop().await;
                std::panic::resume_unwind(panic);
            }
            eprintln!(
                "Collected debugger {engine}/{mode}: Pause/Next waiting-caller identity, strict readiness, pause identity and one resumed click"
            );
        }
    }
    fixture.cli(&["disconnect"]).await;
    mcp.tool("disconnect", json!({})).await;
    mcp.stop().await;
    assert!(
        failures.is_empty(),
        "Original cleanup failures: {failures:?}"
    );
}
