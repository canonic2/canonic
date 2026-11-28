//! Pinned filename helper versus installed native exports, with distinct caller cwd.
use super::*;
use std::path::{Path, PathBuf};

fn private(path: &Path) {
    let mut builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path).unwrap();
}
fn cases() -> Value {
    json!([
        {"name":"nested","path":"missing/deep/Acme.record"},
        {"name":"text","path":"Acme.txt"},
        {"name":"upper","path":"Acme.backup.GZ"},
        {"name":"dot","path":"Acme."},
        {"name":"hidden","path":".Acme"},
        {"name":"gz-hidden","path":".gz"},
        {"name":"parent","path":"level/../Acme.data.gz"},
        {"name":"unicode","path":"Acme-é.trace.gz"},
        {"name":"readable","path":"Acme.record","parentMode":0o755},
        {"name":"readable-nested","path":"missing/deep/Acme.gz","parentMode":0o755},
        {"name":"sticky","path":"Acme.record","parentMode":0o1777},
        {"name":"sticky-nested","path":"missing/deep/Acme.gz","parentMode":0o1777}
    ])
}
async fn baseline(fixture: &Fixture) -> Value {
    let root = std::env::var_os("TESTRUN_CHROME_MCP_REFERENCE_ROOT")
        .map(PathBuf::from)
        .expect("Select the pinned Chrome MCP source fixture");
    let output = Command::new("node")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/browser-trace-filename-baseline.cjs"
        ))
        .arg(root)
        .arg(fixture.workspace.path().join("filename-reference"))
        .arg(cases().to_string())
        .output()
        .await
        .unwrap();
    std::fs::write(
        fixture.workspace.path().join("filename-reference.json"),
        &output.stdout,
    )
    .unwrap();
    std::fs::write(
        fixture.workspace.path().join("filename-reference.stderr"),
        &output.stderr,
    )
    .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}
async fn native(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    context: &Value,
    mode: &str,
    reference: &Value,
) {
    let root = fixture.workspace.path().join(format!("filenames-{mode}"));
    private(&root);
    for item in cases().as_array().unwrap() {
        let directory = root.join(item["name"].as_str().unwrap());
        private(&directory);
        #[cfg(unix)]
        if let Some(mode) = item["parentMode"].as_u64() {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(mode as u32))
                .unwrap();
        }
    }
    let page = operation(
        mcp,
        target,
        json!({"operation":"page_create","context":context,"select":true}),
    )
    .await["page"]
        .clone();
    let cli_root = root.join("nested");
    let began = std::time::Instant::now();
    let submitted = fixture.cli_in(&cli_root, &["browser","send","--target",target,&json!({"operation":"trace_start","context":context,"options":{"filePath":"missing/deep/Acme.record"}}).to_string()]).await;
    let started = completed(mcp, target, submitted).await;
    let fresh_ms = began.elapsed().as_secs_f64() * 1000.0;
    assert!(
        !cli_root.join("missing").exists(),
        "Starting a trace must not create export parents"
    );
    let first = operation(mcp, target, json!({"operation":"trace_stop"})).await;
    assert_eq!(first["report"]["traceId"], started["traceId"]);
    let retained = std::time::Instant::now();
    let mut latest = first.clone();
    for (index, item) in cases().as_array().unwrap().iter().enumerate() {
        let name = item["name"].as_str().unwrap();
        let expected = &reference[name];
        let path = root.join(name).join(expected["relative"].as_str().unwrap());
        let request = format!("filenames-{mode}/{name}/{}", item["path"].as_str().unwrap());
        if index > 0 {
            latest = operation(
                mcp,
                target,
                json!({"operation":"trace_stop","filePath":request}),
            )
            .await;
        }
        assert_eq!(
            latest["report"], first["report"],
            "Retained export allocated new evidence"
        );
        trace_export::exported(&latest, &path);
        assert_eq!(
            latest["export"]["mimeType"],
            if expected["gzip"] == true {
                "application/gzip"
            } else {
                "application/json"
            }
        );
        assert_eq!(expected["mode"], 0o600);
        #[cfg(unix)]
        if let Some(mode) = item["parentMode"].as_u64() {
            use std::os::unix::fs::PermissionsExt;
            let directory = root.join(name);
            assert_eq!(expected["parentMode"], mode);
            assert_eq!(
                std::fs::metadata(&directory).unwrap().permissions().mode() & 0o7777,
                mode as u32
            );
            if name.ends_with("-nested") {
                for child in ["missing", "missing/deep"] {
                    assert_eq!(
                        std::fs::metadata(directory.join(child))
                            .unwrap()
                            .permissions()
                            .mode()
                            & 0o777,
                        0o700
                    );
                }
            }
        }
        assert_eq!(
            operation(
                mcp,
                target,
                json!({"operation":"trace_stop","filePath":request})
            )
            .await,
            latest
        );
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for name in ["missing", "missing/deep"] {
            assert_eq!(
                std::fs::metadata(cli_root.join(name))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o700
            );
        }
    }
    assert_eq!(
        operation(mcp, target, json!({"operation":"developer_evidence_list"})).await["evidence"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    // A different CLI cwd binds the start destination. Shared act overrides it
    // relative to the MCP process cwd, without creating the abandoned parents.
    let submitted = fixture.cli_in(&cli_root, &["browser","send","--target",target,&json!({"operation":"trace_start","context":context,"options":{"filePath":"never-created/deep/Acme.record"}}).to_string()]).await;
    completed(mcp, target, submitted).await;
    assert!(!cli_root.join("never-created").exists());
    let override_root = root.join("override");
    private(&override_root);
    let requested = format!("filenames-{mode}/override/later/Acme.gz");
    let submitted = mcp
        .tool(
            "act",
            json!({"targetId":target,"actions":[{"action":"trace_stop","filePath":requested}]}),
        )
        .await;
    let second = completed(mcp, target, submitted).await["results"][0].clone();
    trace_export::exported(&second, &override_root.join("later/Acme.gz"));
    assert_ne!(
        second["report"]["evidenceId"],
        first["report"]["evidenceId"]
    );
    assert!(!cli_root.join("never-created").exists());
    assert_eq!(
        operation(mcp, target, json!({"operation":"trace_stop"})).await,
        second
    );
    assert_eq!(
        operation(mcp, target, json!({"operation":"developer_evidence_list"})).await["evidence"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    // A parent becoming unsafe after start must retain complete evidence and
    // defer all child creation. Restoring sticky ownership retries publication.
    let recovery_root = root.join("recovery");
    private(&recovery_root);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&recovery_root, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let recovery_path = recovery_root.join("missing/deep/Acme.json");
    operation(
        mcp,
        target,
        json!({"operation":"trace_start","context":context,"options":{"filePath":recovery_path}}),
    )
    .await;
    assert!(!recovery_root.join("missing").exists());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&recovery_root, std::fs::Permissions::from_mode(0o777)).unwrap();
    }
    collectors::refused(
        mcp,
        target,
        json!({"operation":"trace_stop","filePath":recovery_path}),
        "TRACE_EXPORT_PRIVATE_DIRECTORY_REQUIRED",
        true,
    )
    .await;
    assert_eq!(
        operation(mcp, target, json!({"operation":"trace_status"})).await["state"],
        "recording"
    );
    collectors::refused(
        mcp,
        target,
        json!({"operation":"trace_stop"}),
        "TRACE_EXPORT_PRIVATE_DIRECTORY_REQUIRED",
        true,
    )
    .await;
    let pending = operation(mcp, target, json!({"operation":"trace_status"})).await;
    assert_eq!(pending["active"], false);
    assert_eq!(pending["report"]["complete"], true);
    assert!(!recovery_root.join("missing").exists());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&recovery_root, std::fs::Permissions::from_mode(0o1777)).unwrap();
    }
    let submitted = mcp
        .tool(
            "act",
            json!({"targetId":target,"actions":[{"action":"trace_stop"}]}),
        )
        .await;
    let recovered = completed(mcp, target, submitted).await["results"][0].clone();
    assert_eq!(recovered["report"], pending["report"]);
    trace_export::exported(&recovered, &recovery_path);
    assert_eq!(
        operation(mcp, target, json!({"operation":"trace_stop"})).await,
        recovered
    );
    assert_eq!(
        operation(mcp, target, json!({"operation":"developer_evidence_list"})).await["evidence"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    for receipt in [&first, &second, &recovered] {
        operation(mcp,target,json!({"operation":"developer_evidence_remove","evidenceId":receipt["report"]["evidenceId"]})).await;
    }
    operation(mcp, target, json!({"operation":"page_close","page":page})).await;
    eprintln!(
        "Trace filenames {mode}: freshCliStartMs={fresh_ms}; retainedCopiesAndOverrideMs={}",
        retained.elapsed().as_secs_f64() * 1000.0
    );
}

#[tokio::test]
async fn sdk_trace_shared_parent_reference() {
    let fixture = installed_fixture();
    let reference = baseline(&fixture).await;
    assert_eq!(reference.as_object().unwrap().len(), 12);
    for (name, mode) in [
        ("readable", 0o755),
        ("readable-nested", 0o755),
        ("sticky", 0o1777),
        ("sticky-nested", 0o1777),
    ] {
        assert_eq!(reference[name]["parentMode"], mode);
        assert_eq!(reference[name]["mode"], 0o600);
    }
}

#[tokio::test]
async fn installed_step_four_trace_filename_checkpoint() {
    let fixture = installed_fixture();
    let reference = baseline(&fixture).await;
    let mut mcp = Mcp::start(&fixture).await;
    let mut failures = vec![];
    for engine in ["chrome", "firefox", "webkit"] {
        for mode in ["headless", "headed"] {
            let connected = fixture
                .cli(&["browser", "connect", "--engine", engine, "--mode", mode])
                .await;
            let target = connected["targetId"].as_str().unwrap();
            let ready = fixture.ready_target(target).await;
            let context = &ready["browser"]["selectedContext"];
            let result = std::panic::AssertUnwindSafe(async {
                if engine == "chrome" {
                    native(&fixture,&mut mcp,target,context,mode,&reference).await;
                } else {
                    let path=format!("refused-{engine}-{mode}/Acme.record");
                    collectors::refused(&mut mcp,target,json!({"operation":"trace_start","context":context,"options":{"filePath":path}}),"TRACE_NATIVE_OPTIONS_UNSUPPORTED",false).await;
                    collectors::refused(&mut mcp,target,json!({"operation":"trace_stop","filePath":path}),"TRACE_NATIVE_OPTIONS_UNSUPPORTED",false).await;
                    assert!(!fixture.workspace.path().join(format!("refused-{engine}-{mode}")).exists());
                }
            }).catch_unwind().await;
            let closed = performance_lifecycle::disconnect_observed(
                &fixture, &mut mcp, target, engine, mode, context,
            )
            .await;
            if closed["result"]["structuredContent"]["ok"] != true {
                failures.push(format!("{engine}/{mode}: {closed}"));
            }
            if let Err(panic) = result {
                mcp.stop().await;
                std::panic::resume_unwind(panic);
            }
            eprintln!(
                "Trace filename {engine} {mode} passed; workspace {}",
                fixture.workspace.path().display()
            );
        }
    }
    mcp.stop().await;
    assert!(
        failures.is_empty(),
        "Original cleanup failures: {failures:?}"
    );
}
