//! Installed isolated request state is compared with the unchanged pinned SDK.
use super::*;
use sha2::{Digest, Sha256};
#[path = "../../../fixtures/browser_http_defaults.rs"]
mod wire;

fn cookie(name: &str, domain: &str, value: &str, secure: bool) -> Value {
    json!({"name":name,"value":value,"domain":domain,"path":"/","expires":-1,"httpOnly":true,"secure":secure,"sameSite":"Lax"})
}
fn seed() -> Value {
    json!({"cookies":[cookie("first","127.0.0.1","old",false),cookie("second","127.0.0.1","Acme-two",false),cookie("first",".127.0.0.1","Acme-domain",false),cookie("first","127.0.0.1","Acme-replaced",false),cookie("secure","127.0.0.1","Acme-private-secure",true)],"origins":[{"origin":"https://example.com","localStorage":[{"name":"Acme-account","value":"Acme-private-local"}],"indexedDB":[{"name":"Acme-private-db","version":1,"stores":[]}],"opfs":[{"path":"Acme-private.txt","type":"file","base64":"QWNtZQ=="}]}]})
}
fn private_file(path: &std::path::Path, bytes: &[u8]) {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path).unwrap().write_all(bytes).unwrap();
}
pub(super) struct BlockPublication(std::path::PathBuf);
impl Drop for BlockPublication {
    fn drop(&mut self) {
        if std::fs::read(&self.0).is_ok_and(|bytes| bytes == b"Acme publication blocker") {
            std::fs::remove_file(&self.0).unwrap();
        }
    }
}
pub(super) fn block_publication(fixture: &Fixture, context: &Value) -> BlockPublication {
    let connection = context["connectionId"].as_str().unwrap();
    let journal = std::fs::read_dir(fixture.workspace.data_root())
        .unwrap()
        .map(|entry| {
            entry
                .unwrap()
                .path()
                .join("browser")
                .join(format!("browser-{connection}"))
        })
        .find(|path| path.is_dir())
        .expect("Exact owned browser journal");
    let path = journal.join("http-state");
    private_file(&path, b"Acme publication blocker");
    BlockPublication(path)
}
async fn baseline(fixture: &Fixture, site: &wire::Site) -> Value {
    let result = Command::new(
        fixture
            .cache
            .join("node-24.12.0/node-v24.12.0-darwin-x64/bin/node"),
    )
    .arg(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/browser-http-state-baseline.cjs"),
    )
    .arg(fixture.cache.join("playwright-core-1.64.1/package"))
    .arg(&site.url)
    .arg(seed().to_string())
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
            .join("http-state-sdk-reference.json"),
        &result.stdout,
    )
    .unwrap();
    std::fs::write(
        fixture
            .workspace
            .path()
            .join("http-state-sdk-reference.stderr"),
        &result.stderr,
    )
    .unwrap();
    assert!(
        result.status.success(),
        "Independent SDK reference: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    let report: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(report["challengeStatus"], 401);
    report
}
async fn shared(mcp: &mut Mcp, target: &str, action: Value) -> Value {
    let submitted = mcp
        .tool("act", json!({"targetId":target,"actions":[action]}))
        .await;
    completed(mcp, target, submitted).await["results"][0].clone()
}
pub(super) async fn state(mcp: &mut Mcp, target: &str, receipt: &Value) -> Value {
    let report = &receipt["reports"][0];
    assert_eq!(report["secretBearing"], true);
    assert_eq!(report["replayed"], false);
    assert!(!receipt.to_string().contains("Acme-private"));
    let path = report["attachment"]["path"].as_str().unwrap();
    let bytes = std::fs::read(path).unwrap();
    assert_eq!(report["attachment"]["bytes"], bytes.len());
    assert_eq!(
        report["attachment"]["sha256"],
        format!("{:x}", Sha256::digest(&bytes))
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    let read = operation(
        mcp,
        target,
        json!({"operation":"http_evidence_read","evidenceId":report["evidenceId"],"format":"json"}),
    )
    .await;
    assert_eq!(read["private"], true);
    let parsed: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(read["value"], parsed);
    parsed
}
async fn fetch(mcp: &mut Mcp, target: &str, context: &Value, url: &str, status: u64) -> Value {
    let response = operation(mcp,target,json!({"operation":"http_fetch","httpContext":context,"request":{"url":url,"readBody":true}})).await;
    assert_eq!(response["status"], status);
    let body = &response["body"]["evidenceId"];
    let read = operation(
        mcp,
        target,
        json!({"operation":"http_evidence_read","evidenceId":body,"format":"json"}),
    )
    .await;
    operation(
        mcp,
        target,
        json!({"operation":"http_response_remove","requestId":response["requestId"]}),
    )
    .await;
    operation(
        mcp,
        target,
        json!({"operation":"http_evidence_remove","evidenceId":body}),
    )
    .await;
    read["value"].clone()
}
async fn run(
    fixture: &Fixture,
    mcp: &mut Mcp,
    site: &wire::Site,
    reference: &Value,
    target: &str,
    ready: &Value,
    engine: &str,
    mode: &str,
) {
    let input_directory = fixture
        .workspace
        .path()
        .join(format!("http-state-input-{engine}-{mode}"));
    let mut builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(&input_directory).unwrap();
    let input = input_directory.join("auth-state.json");
    private_file(&input, &serde_json::to_vec(&seed()).unwrap());
    let submitted = fixture
        .cli(&[
            "browser",
            "send",
            "--target",
            target,
            &json!({"operation":"http_context_create","options":{"storageState":seed()}})
                .to_string(),
        ])
        .await;
    let inline = completed(mcp, target, submitted).await["httpContext"].clone();
    let file = shared(
        mcp,
        target,
        json!({"action":"http_context_create","options":{"storageState":input}}),
    )
    .await["httpContext"]
        .clone();
    std::fs::write(&input, b"changed after acknowledgement").unwrap();
    let blocker = block_publication(fixture, &inline);
    let submitted = mcp.tool("browser_send",json!({"targetId":target,"request":{"operation":"http_context_storage_state","httpContext":inline,"options":{"indexedDB":true,"opfs":true}}})).await;
    let failed = mcp
        .tool(
            "browser_status",
            json!({"targetId":target,"operationId":submitted["operation"]["id"],"waitMs":30000}),
        )
        .await;
    assert_eq!(failed["operation"]["settled"], true, "{failed}");
    assert_eq!(failed["operation"]["outcome"], "failed", "{failed}");
    assert_eq!(failed["operation"]["code"], "EVIDENCE_IO", "{failed}");
    let retained = &failed["operation"]["result"];
    assert_eq!(retained["stateRetained"], true, "{failed}");
    let capture = retained["captureId"].clone();
    let pending = operation(mcp, target, json!({"operation":"http_evidence_list"})).await;
    assert_eq!(pending["pendingArtifacts"].as_array().unwrap().len(), 1);
    assert_eq!(pending["pendingArtifacts"][0]["captureId"], capture);
    operation(
        mcp,
        target,
        json!({"operation":"http_context_cookies_clear","httpContext":inline}),
    )
    .await;
    operation(
        mcp,
        target,
        json!({"operation":"http_context_remove","httpContext":inline}),
    )
    .await;
    drop(blocker);
    let submitted = fixture
        .cli(&[
            "browser",
            "send",
            "--target",
            target,
            &json!({"operation":"http_evidence_finalize","captureId":capture}).to_string(),
        ])
        .await;
    let recovered = completed(mcp, target, submitted).await;
    assert_eq!(state(mcp, target, &recovered).await, reference["initial"]);
    let mut evidence = vec![recovered["reports"][0]["evidenceId"].clone()];
    let echo = fetch(mcp, target, &file, &format!("{}/echo", site.url), 200).await;
    assert_eq!(
        echo["headers"]["cookie"],
        reference["echo"]["headers"]["cookie"]
    );
    fetch(mcp, target, &file, &format!("{}/auth/basic", site.url), 401).await;
    for (indexed_db, opfs, expected) in [
        (false, false, "defaults"),
        (true, false, "indexedDB"),
        (false, true, "opfs"),
        (true, true, "all"),
    ] {
        let captured = if indexed_db && opfs {
            shared(mcp,target,json!({"action":"http_context_storage_state","httpContext":file,"options":{"indexedDB":indexed_db,"opfs":opfs}})).await
        } else {
            operation(mcp,target,json!({"operation":"http_context_storage_state","httpContext":file,"options":{"indexedDB":indexed_db,"opfs":opfs}})).await
        };
        assert_eq!(
            state(mcp, target, &captured).await,
            reference[expected],
            "{engine}/{mode}/{expected}"
        );
        evidence.push(captured["reports"][0]["evidenceId"].clone());
    }
    // Pinned readFile/writeFile semantics: relative caller paths, ordinary
    // permissions, created parents and replacement.
    let relative = format!("http-state-relative-{engine}-{mode}");
    std::fs::create_dir(fixture.workspace.path().join(&relative)).unwrap();
    let shared_input = fixture
        .workspace
        .path()
        .join(&relative)
        .join("auth-state.json");
    std::fs::write(&shared_input, serde_json::to_vec(&seed()).unwrap()).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&shared_input, std::fs::Permissions::from_mode(0o644)).unwrap();
    }
    let submitted = fixture
        .cli(&[
            "browser",
            "send",
            "--target",
            target,
            &json!({"operation":"http_context_create","options":{"storageState":format!("{relative}/auth-state.json")}})
                .to_string(),
        ])
        .await;
    let relative_client = completed(mcp, target, submitted).await["httpContext"].clone();
    let echo = fetch(
        mcp,
        target,
        &relative_client,
        &format!("{}/echo", site.url),
        200,
    )
    .await;
    assert_eq!(
        echo["headers"]["cookie"],
        reference["echo"]["headers"]["cookie"]
    );
    let exported = format!("{relative}/created/parents/state.json");
    for replaced in [false, true] {
        let submitted = fixture
            .cli(&[
                "browser",
                "send",
                "--target",
                target,
                &json!({"operation":"http_context_storage_state","httpContext":relative_client,"options":{"path":exported}})
                    .to_string(),
            ])
            .await;
        let written = completed(mcp, target, submitted).await;
        let absolute = fixture.workspace.path().join(&exported);
        assert_eq!(
            std::fs::canonicalize(written["file"]["path"].as_str().unwrap()).unwrap(),
            std::fs::canonicalize(&absolute).unwrap(),
            "{written}"
        );
        assert!(!written.to_string().contains("Acme-private"));
        let copied: Value = serde_json::from_slice(&std::fs::read(&absolute).unwrap()).unwrap();
        assert_eq!(
            copied,
            state(mcp, target, &written).await,
            "replaced={replaced}"
        );
        assert_eq!(copied["cookies"].as_array().unwrap().len(), 4, "{copied}");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&absolute).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        evidence.push(written["reports"][0]["evidenceId"].clone());
    }
    operation(
        mcp,
        target,
        json!({"operation":"http_context_remove","httpContext":relative_client}),
    )
    .await;
    operation(
        mcp,
        target,
        json!({"operation":"http_context_cookies_clear","httpContext":file}),
    )
    .await;
    let cleared = operation(mcp,target,json!({"operation":"http_context_storage_state","httpContext":file,"options":{"indexedDB":true,"opfs":true}})).await;
    assert_eq!(state(mcp, target, &cleared).await, reference["cleared"]);
    evidence.push(cleared["reports"][0]["evidenceId"].clone());
    operation(
        mcp,
        target,
        json!({"operation":"http_context_remove","httpContext":file}),
    )
    .await;
    collectors::refused(mcp,target,json!({"operation":"http_context_create","options":{"browserContext":ready["browser"]["selectedContext"],"storageState":seed(),"allowUnknownCookiePartitions":engine=="firefox"}}),"HTTP_BROWSER_STORAGE_STATE_UNAVAILABLE",false).await;
    for id in evidence {
        operation(
            mcp,
            target,
            json!({"operation":"http_evidence_remove","evidenceId":id}),
        )
        .await;
    }
    let remaining = operation(mcp, target, json!({"operation":"http_evidence_list"})).await;
    assert_eq!(remaining["pendingArtifacts"], json!([]));
    assert_eq!(remaining["evidence"], json!([]));
}
#[tokio::test]
async fn installed_step_four_http_storage_state_checkpoint() {
    let fixture = installed_fixture();
    let mut mcp = Mcp::start(&fixture).await;
    let site = wire::Site::start(None).await;
    let reference = baseline(&fixture, &site).await;
    let mut failures = vec![];
    for mode in ["headless", "headed"] {
        for engine in ["chrome", "firefox", "webkit"] {
            let connected = mcp
                .tool("browser_connect", json!({"engine":engine,"mode":mode}))
                .await;
            let target = connected["targetId"].as_str().unwrap();
            let ready = fixture.ready_target(target).await;
            let result = std::panic::AssertUnwindSafe(run(
                &fixture, &mut mcp, &site, &reference, target, &ready, engine, mode,
            ))
            .catch_unwind()
            .await;
            std::fs::write(
                fixture
                    .workspace
                    .path()
                    .join(format!("http-state-wire-{engine}-{mode}.json")),
                serde_json::to_vec_pretty(&site.records()).unwrap(),
            )
            .unwrap();
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
