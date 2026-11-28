//! Installed trace export uses retained evidence, including after destination failure.
use super::*;
use sha2::{Digest, Sha256};
use std::io::Read;

pub(super) fn exported(receipt: &Value, path: &std::path::Path) {
    assert_eq!(receipt["report"]["complete"], true, "{receipt}");
    assert_eq!(
        receipt["export"]["path"],
        std::fs::canonicalize(path).unwrap().to_str().unwrap()
    );
    assert_eq!(
        receipt["export"]["evidenceId"],
        receipt["report"]["evidenceId"]
    );
    assert!(receipt["exportPending"].is_null(), "{receipt}");
    let original =
        std::fs::read(receipt["report"]["attachment"]["path"].as_str().unwrap()).unwrap();
    let stored = std::fs::read(path).unwrap();
    assert_eq!(receipt["export"]["bytes"], stored.len());
    assert_eq!(
        receipt["export"]["sha256"],
        format!("{:x}", Sha256::digest(&stored))
    );
    let decoded = if path.extension().unwrap() == "gz" {
        let mut decoded = Vec::new();
        flate2::read::GzDecoder::new(stored.as_slice())
            .read_to_end(&mut decoded)
            .unwrap();
        decoded
    } else {
        stored
    };
    assert_eq!(decoded, original);
    let trace: Value = serde_json::from_slice(&decoded).unwrap();
    assert!(!trace["traceEvents"].as_array().unwrap().is_empty());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}
async fn native(fixture: &Fixture, mcp: &mut Mcp, target: &str, context: &Value, mode: &str) {
    let directory = fixture.workspace.path().join(format!("exports-{mode}"));
    let mut builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(&directory).unwrap();
    let page = operation(
        mcp,
        target,
        json!({"operation":"page_create","context":context,"select":true}),
    )
    .await["page"]
        .clone();
    let path = directory.join("Acme-automatic.json.gz");
    let began = std::time::Instant::now();
    let submitted=fixture.cli(&["browser","send","--target",target,&json!({"operation":"trace_start","context":context,"options":{"filePath":path,"maxDurationMs":1000}}).to_string()]).await;
    let started = completed(mcp, target, submitted).await;
    let fresh_ms = began.elapsed().as_secs_f64() * 1000.0;
    let until = tokio::time::Instant::now() + Duration::from_secs(15);
    loop {
        let status = operation(mcp, target, json!({"operation":"trace_status"})).await;
        if status["active"] == false {
            break;
        }
        assert!(tokio::time::Instant::now() < until, "{status}");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let automatic = operation(mcp, target, json!({"operation":"trace_stop"})).await;
    assert_eq!(automatic["report"]["traceId"], started["traceId"]);
    exported(&automatic, &path);
    assert_eq!(
        operation(
            mcp,
            target,
            json!({"operation":"trace_stop","filePath":path})
        )
        .await,
        automatic
    );
    let retained = std::time::Instant::now();
    let skipped = directory.join("Acme-overridden.json");
    let submitted=mcp.tool("act",json!({"targetId":target,"actions":[{"action":"trace_start","context":context,"options":{"filePath":skipped}}]})).await;
    assert_eq!(
        completed(mcp, target, submitted).await["completedActions"],
        1
    );
    let occupied = directory.join("Acme-occupied.json");
    std::fs::write(&occupied, b"Acme existing file").unwrap();
    collectors::refused(
        mcp,
        target,
        json!({"operation":"trace_stop","filePath":occupied}),
        "TRACE_EXPORT_EXISTS",
        true,
    )
    .await;
    assert_eq!(
        operation(mcp, target, json!({"operation":"trace_status"})).await["state"],
        "recording"
    );
    let manual_path = directory.join("Acme-manual.json");
    let submitted = mcp
        .tool(
            "act",
            json!({"targetId":target,"actions":[{"action":"trace_stop","filePath":manual_path}]}),
        )
        .await;
    let manual = completed(mcp, target, submitted).await["results"][0].clone();
    exported(&manual, &manual_path);
    assert!(!skipped.exists());
    let blocked = directory.join("Acme-blocked.json");
    operation(
        mcp,
        target,
        json!({"operation":"trace_start","context":context,"options":{"filePath":blocked}}),
    )
    .await;
    std::fs::write(&blocked, b"Acme destination race").unwrap();
    collectors::refused(
        mcp,
        target,
        json!({"operation":"trace_stop"}),
        "TRACE_EXPORT_EXISTS",
        true,
    )
    .await;
    let pending = operation(mcp, target, json!({"operation":"trace_status"})).await;
    assert_eq!(pending["active"], false, "{pending}");
    assert_eq!(pending["report"]["complete"], true, "{pending}");
    assert_eq!(
        pending["exportPending"]["code"], "TRACE_EXPORT_EXISTS",
        "{pending}"
    );
    let recovered_path = directory.join("Acme-recovered.json.gz");
    let recovered = operation(
        mcp,
        target,
        json!({"operation":"trace_stop","filePath":recovered_path}),
    )
    .await;
    assert_eq!(recovered["report"], pending["report"]);
    exported(&recovered, &recovered_path);
    assert_eq!(
        operation(mcp, target, json!({"operation":"trace_stop"})).await,
        recovered
    );
    assert_eq!(std::fs::read(&occupied).unwrap(), b"Acme existing file");
    assert_eq!(std::fs::read(&blocked).unwrap(), b"Acme destination race");
    let evidence = operation(mcp, target, json!({"operation":"developer_evidence_list"})).await;
    assert_eq!(
        evidence["evidence"].as_array().unwrap().len(),
        3,
        "{evidence}"
    );
    for receipt in [&automatic, &manual, &recovered] {
        operation(mcp,target,json!({"operation":"developer_evidence_remove","evidenceId":receipt["report"]["evidenceId"]})).await;
    }
    operation(mcp, target, json!({"operation":"page_close","page":page})).await;
    eprintln!(
        "Trace export {mode}: freshCliStartMs={fresh_ms}; retainedTwoCaptureRecoveryMs={}",
        retained.elapsed().as_secs_f64() * 1000.0
    );
}
#[tokio::test]
async fn installed_step_four_trace_export_checkpoint() {
    let fixture = installed_fixture();
    let mut mcp = Mcp::start(&fixture).await;
    for engine in ["chrome", "firefox", "webkit"] {
        for mode in ["headless", "headed"] {
            let connected = fixture
                .cli(&["browser", "connect", "--engine", engine, "--mode", mode])
                .await;
            let target = connected["targetId"].as_str().unwrap();
            let ready = fixture.ready_target(target).await;
            let context = &ready["browser"]["selectedContext"];
            let result=std::panic::AssertUnwindSafe(async {
                if engine=="chrome" {native(&fixture,&mut mcp,target,context,mode).await;}
                else {
                    let path=fixture.workspace.path().join(format!("Acme-{engine}-{mode}.json"));
                    collectors::refused(&mut mcp,target,json!({"operation":"trace_start","context":context,"options":{"filePath":path}}),"TRACE_NATIVE_OPTIONS_UNSUPPORTED",false).await;
                    collectors::refused(&mut mcp,target,json!({"operation":"trace_stop","filePath":path}),"TRACE_NATIVE_OPTIONS_UNSUPPORTED",false).await;
                    assert!(!path.exists());
                }
            }).catch_unwind().await;
            mcp.tool("disconnect", json!({"targetId":target})).await;
            if let Err(panic) = result {
                fixture.cli(&["disconnect"]).await;
                mcp.tool("disconnect", json!({})).await;
                mcp.stop().await;
                std::panic::resume_unwind(panic);
            }
            eprintln!(
                "Trace export {engine} {mode} passed; workspace {}",
                fixture.workspace.path().display()
            );
        }
    }
    fixture.cli(&["disconnect"]).await;
    mcp.tool("disconnect", json!({})).await;
    mcp.stop().await;
}
