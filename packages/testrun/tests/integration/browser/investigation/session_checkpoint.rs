//! Installed 5.5 checkpoint. All application traffic and donor endpoints are local.
use super::*;
use std::{
    io::Write,
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::{Path, PathBuf},
};

fn file(path: &Path, bytes: &[u8]) {
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .unwrap()
        .write_all(bytes)
        .unwrap();
}

fn certificates() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src/browser/http/fixtures")
}

pub(super) async fn wait(mcp: &mut Mcp, target: &str, id: &Value) -> Value {
    assert!(id.is_string(), "Expected a retained operation ID");
    let deadline = tokio::time::Instant::now() + Duration::from_secs(600);
    loop {
        let state = mcp
            .tool(
                "browser_status",
                json!({"targetId":target,"operationId":id,"waitMs":30000}),
            )
            .await;
        if state["operation"]["settled"] == true {
            return state;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "Operation did not settle: {state}"
        );
    }
}

pub(super) async fn ready(mcp: &mut Mcp, connected: &Value) -> Value {
    let target = connected["targetId"].as_str().expect("Browser target");
    let preparation = wait(mcp, target, &connected["preparationOperationId"]).await;
    assert_eq!(
        preparation["operation"]["outcome"], "succeeded",
        "{preparation}"
    );
    let status = mcp.tool("browser_status", json!({"targetId":target})).await;
    let started = wait(mcp, target, &status["startupOperationId"]).await;
    assert_eq!(started["operation"]["outcome"], "succeeded", "{started}");
    let status = mcp.tool("browser_status", json!({"targetId":target})).await;
    assert_eq!(status["browser"]["phase"], "ready", "{status}");
    status["browser"].clone()
}

pub(super) async fn disconnect(mcp: &mut Mcp, target: &str) {
    mcp.tool("browser_disconnect", json!({"targetId":target}))
        .await;
}

async fn refused_attachment(mcp: &mut Mcp, engine: &str, attachment: Value, expected: &str) {
    let opened = mcp
        .tool("connect", json!({"engine":engine,"attachment":attachment}))
        .await;
    let target = opened["targetId"].as_str().unwrap();
    let preparation = wait(mcp, target, &opened["preparationOperationId"]).await;
    assert_eq!(
        preparation["operation"]["outcome"], "succeeded",
        "{preparation}"
    );
    let pending = mcp.tool("browser_status", json!({"targetId":target})).await;
    let refused = wait(mcp, target, &pending["startupOperationId"]).await;
    assert_eq!(refused["operation"]["code"], expected, "{refused}");
    disconnect(mcp, target).await;
}

pub(super) async fn scope_empty(mcp: &mut Mcp) {
    let state = mcp.tool("browser_status", json!({})).await;
    assert_eq!(state["browsers"], json!([]), "{state}");
}

pub(super) async fn finish_cell(
    mcp: &mut Mcp,
    name: &str,
    outcome: Result<(), Box<dyn std::any::Any + Send>>,
    failures: &mut Vec<String>,
) {
    if let Err(panic) = outcome {
        let message = panic
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| panic.downcast_ref::<&str>().copied())
            .unwrap_or("non-text assertion");
        failures.push(format!("{name}: {message}"));
    }
    let state = mcp.tool("browser_status", json!({})).await;
    let witnesses = state["browsers"]
        .as_array()
        .expect("Current fixture browsers");
    for browser in witnesses {
        let response = mcp
            .request(
                "tools/call",
                json!({"name":"browser_disconnect","arguments":{"targetId":browser["targetId"]}}),
            )
            .await;
        if response["result"]["structuredContent"]["ok"] != true {
            failures.push(format!("{name}: retained disconnect failed: {response}"));
        }
    }
    scope_empty(mcp).await;
    if !failures.iter().any(|failure| failure.starts_with(name)) {
        eprintln!("{name} passed");
    }
}

async fn cookies(mcp: &mut Mcp, target: &str, status: &Value, url: &str) -> Value {
    operation(
        mcp,
        target,
        json!({"operation":"context_cookies_read","context":status["selectedContext"],"urls":url}),
    )
    .await
}

async fn profile_cell(
    fixture: &Fixture,
    mcp: &mut Mcp,
    site: &Site,
    engine: &str,
    mode: &str,
    name: &str,
) {
    let profile = json!({"kind":"retained","name":name});
    let opened = fixture
        .cli(&[
            "connect",
            "--engine",
            engine,
            "--mode",
            mode,
            "--profile",
            name,
        ])
        .await;
    let target = opened["targetId"].as_str().expect("CLI connect target");
    let status = ready(mcp, &opened).await;
    assert_eq!(status["identity"]["profile"], profile);
    operation(mcp,target,json!({"operation":"context_cookies_add","context":status["selectedContext"],"cookies":[{"name":"acme-retained","value":"fixture","url":site.url,"expires":2000000000}]})).await;
    assert!(
        cookies(mcp, target, &status, &site.url)
            .await
            .to_string()
            .contains("acme-retained")
    );

    let contended = mcp
        .tool(
            "connect",
            json!({"engine":engine,"mode":mode,"profile":profile}),
        )
        .await;
    let busy = contended["targetId"].as_str().unwrap();
    let failure = wait(mcp, busy, &contended["preparationOperationId"]).await;
    assert_eq!(failure["operation"]["code"], "PROFILE_BUSY", "{failure}");
    disconnect(mcp, busy).await;
    disconnect(mcp, target).await;

    let reopened = mcp
        .tool(
            "browser_connect",
            json!({"engine":engine,"mode":mode,"profile":profile}),
        )
        .await;
    let target = reopened["targetId"].as_str().unwrap();
    let status = ready(mcp, &reopened).await;
    assert!(
        cookies(mcp, target, &status, &site.url)
            .await
            .to_string()
            .contains("acme-retained")
    );
    disconnect(mcp, target).await;

    let borrowed_path = fixture.workspace.path().join(name);
    std::fs::rename(fixture.cache.join("profiles").join(name), &borrowed_path).unwrap();
    file(&borrowed_path.join("caller-owned"), b"Acme keeps this");
    let borrowed = mcp
        .tool(
            "connect",
            json!({"engine":engine,"mode":mode,"profile":{"kind":"borrowed","path":borrowed_path}}),
        )
        .await;
    let target = borrowed["targetId"].as_str().unwrap();
    let status = ready(mcp, &borrowed).await;
    assert_eq!(status["identity"]["profile"], json!({"kind":"borrowed"}));
    assert!(
        cookies(mcp, target, &status, &site.url)
            .await
            .to_string()
            .contains("acme-retained")
    );
    disconnect(mcp, target).await;
    assert_eq!(
        std::fs::read(borrowed_path.join("caller-owned")).unwrap(),
        b"Acme keeps this"
    );
}

struct Donor {
    child: Child,
    input: tokio::process::ChildStdin,
    output: tokio::io::Lines<BufReader<tokio::process::ChildStdout>>,
}

impl Donor {
    async fn start(fixture: &Fixture, engine: &str, mode: &str) -> Self {
        let root = fixture
            .workspace
            .path()
            .join(format!("donor-{engine}-{mode}"));
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&root)
            .unwrap();
        let [node, package, chrome] = testrun::browser::checkpoint_artifacts(&fixture.cache);
        let mut child = Command::new(node)
            .arg(
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("tests/fixtures/browser-session-donor.cjs"),
            )
            .args([&root, &package, &chrome])
            .args([engine, mode])
            .arg(certificates())
            .current_dir(&root)
            .env_clear()
            .env("HOME", &root)
            .env("PATH", "/usr/bin:/bin")
            .env("PLAYWRIGHT_BROWSERS_PATH", fixture.cache.join("browsers"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let input = child.stdin.take().unwrap();
        let output = BufReader::new(child.stdout.take().unwrap()).lines();
        Self {
            child,
            input,
            output,
        }
    }
    async fn report(&mut self) -> Value {
        let line = tokio::time::timeout(Duration::from_secs(90), self.output.next_line())
            .await
            .expect("Donor must respond before deadline")
            .unwrap()
            .expect("Donor exited");
        serde_json::from_str(&line).unwrap()
    }
    async fn check(&mut self) {
        self.input.write_all(b"check\n").await.unwrap();
        assert_eq!(self.report().await, json!({"intact":true}));
    }
    async fn check_release(&mut self, name: &str) {
        self.input
            .write_all(format!("check {name}\n").as_bytes())
            .await
            .unwrap();
        assert_eq!(
            self.report().await,
            json!({"intact":true,"ownedPageReleased":name})
        );
    }
    async fn stop(mut self) {
        // EOF also releases a donor after a failed fixture assertion.
        let _ = self.input.write_all(b"stop\n").await;
        drop(self.input);
        let status = tokio::time::timeout(Duration::from_secs(60), self.child.wait())
            .await
            .expect("Owned donor must release before deadline")
            .unwrap();
        assert!(status.success(), "Donor teardown exit: {status}");
    }
}

async fn open_page_witness(mcp: &mut Mcp, target: &str, page: &Value, origin: &str, name: &str) {
    operation(
        mcp,
        target,
        json!({"operation":"page_navigate","page":page,"navigation":{
            "kind":"goto","url":origin,"waitUntil":"load"
        }}),
    )
    .await;
    let url = format!("{}/{name}", origin.replacen("http://", "ws://", 1));
    let expression = format!(
        r#"() => new Promise((resolve, reject) => {{
            const witness = globalThis.AcmeOwnedPageSocket = new WebSocket({});
            witness.onmessage = event => resolve(event.data);
            witness.onerror = () => reject(new Error("Acme witness failed to open"));
        }})"#,
        json!(url)
    );
    let value = operation(
        mcp,
        target,
        json!({"operation":"evaluate","scope":page,"expression":expression,"isFunction":true}),
    )
    .await;
    assert_eq!(value["value"]["value"], "Acme witness open", "{value}");
}

fn attachment_token(ca: &Path, endpoint: &str) -> tempfile::NamedTempFile {
    let origin = url::Url::parse(endpoint)
        .unwrap()
        .origin()
        .ascii_serialization();
    let mut token = tempfile::NamedTempFile::new_in(ca.parent().unwrap()).unwrap();
    serde_json::to_writer(
        &mut token,
        &json!({"origin":origin,"token":"acme-session-token"}),
    )
    .unwrap();
    token
}

async fn attached_cell(fixture: &Fixture, mcp: &mut Mcp, engine: &str, mode: &str, ca: &Path) {
    let mut donor = Donor::start(fixture, engine, mode).await;
    let result = std::panic::AssertUnwindSafe(async {
        let endpoints = donor.report().await;
        let token = attachment_token(ca, endpoints["tlsEndpoint"].as_str().unwrap());
        if mode == "headless" {
            refused_attachment(
                mcp,
                engine,
                json!({"endpoint":endpoints["tlsEndpoint"]}),
                "ATTACHMENT_TLS_FAILED",
            )
            .await;
            refused_attachment(
                mcp,
                engine,
                json!({"endpoint":endpoints["tlsEndpoint"],"caPath":ca}),
                "ATTACHMENT_AUTHORIZATION_FAILED",
            )
            .await;
            donor.check().await;
        }
        for (selector, tls) in [("endpoint", false), ("tlsEndpoint", true)] {
            let mut attachment = json!({"endpoint":endpoints[selector]});
            if tls {
                attachment["caPath"] = json!(ca);
                attachment["credentialPath"] = json!(token.path());
            }
            let opened = mcp
                .tool("connect", json!({"engine":engine,"attachment":attachment}))
                .await;
            let target = opened["targetId"].as_str().unwrap();
            let status = ready(mcp, &opened).await;
            assert_eq!(status["identity"]["launch"], "attached", "{status}");
            assert_eq!(
                operation(mcp, target, json!({"operation":"context_list"})).await["items"]
                    .as_array()
                    .unwrap()
                    .len(),
                1
            );
            assert_eq!(
                operation(mcp, target, json!({"operation":"page_list"})).await["items"]
                    .as_array()
                    .unwrap()
                    .len(),
                1
            );
            assert!(
                !status
                    .to_string()
                    .contains(endpoints[selector].as_str().unwrap())
            );
            let name = if tls { "tls" } else { "loopback" };
            open_page_witness(
                mcp,
                target,
                &status["selectedPage"],
                endpoints["witnessOrigin"].as_str().unwrap(),
                name,
            )
            .await;
            disconnect(mcp, target).await;
            donor.check_release(name).await;
        }
        if engine == "chrome" {
            let context = endpoints["contextId"].as_str().unwrap();
            let endpoint = endpoints["endpoint"].as_str().unwrap();
            let opened = fixture
                .cli(&[
                    "browser",
                    "connect",
                    "--engine",
                    "chrome",
                    "--attach",
                    endpoint,
                    "--adopt-context",
                    context,
                ])
                .await;
            let target = opened["targetId"].as_str().unwrap();
            let status = ready(mcp, &opened).await;
            assert!(status["selectedPage"].is_object());
            let pages = operation(mcp, target, json!({"operation":"page_list"})).await;
            assert_eq!(pages["items"].as_array().unwrap().len(), 1, "{pages}");
            disconnect(mcp, target).await;
            donor.check().await;
            let rejected = endpoints["defaultContextId"].as_str().unwrap();
            refused_attachment(
                mcp,
                engine,
                json!({"endpoint":endpoint,"contextId":rejected}),
                "ATTACHMENT_CONTEXT_UNAVAILABLE",
            )
            .await;
            donor.check().await;
        }
    })
    .catch_unwind()
    .await;
    donor.stop().await;
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}

async fn channel_cell(fixture: &Fixture, mcp: &mut Mcp, channel: &str, mode: &str) {
    let opened = fixture
        .cli(&[
            "connect",
            "--engine",
            "chrome",
            "--channel",
            channel,
            "--mode",
            mode,
        ])
        .await;
    let target = opened["targetId"].as_str().unwrap();
    let status = ready(mcp, &opened).await;
    assert_eq!(status["identity"]["build"]["channel"], channel, "{status}");
    assert_eq!(status["identity"]["mode"], mode, "{status}");
    assert!(
        status["identity"]["browser"]["product"].is_string(),
        "{status}"
    );
    eprintln!("Stable channel {channel}/{mode}: {}", status["identity"]);
    disconnect(mcp, target).await;
}

async fn edge_cells(fixture: &Fixture, mcp: &mut Mcp, failures: &mut Vec<String>) {
    let doctor = fixture
        .cli(&["browser", "doctor", "--engine", "chrome"])
        .await;
    let edge = doctor["installedChannels"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["channel"] == "msedge")
        .unwrap();
    for mode in ["headless", "headed"] {
        if edge["state"] == "installed_unverified" {
            let result = std::panic::AssertUnwindSafe(channel_cell(fixture, mcp, "msedge", mode))
                .catch_unwind()
                .await;
            finish_cell(mcp, &format!("Edge Stable {mode}"), result, failures).await;
            continue;
        }
        let expected = match edge["state"].as_str() {
            Some("build_unsupported") => "BROWSER_CHANNEL_BUILD_UNSUPPORTED",
            Some(code @ ("BROWSER_CHANNEL_UNAVAILABLE" | "BROWSER_CHANNEL_INVALID")) => code,
            _ => panic!("Unexpected Edge channel status: {edge}"),
        };
        let opened = mcp
            .tool(
                "connect",
                json!({"engine":"chrome","channel":"msedge","mode":mode}),
            )
            .await;
        let target = opened["targetId"].as_str().unwrap();
        let refused = wait(mcp, target, &opened["preparationOperationId"]).await;
        assert_eq!(refused["operation"]["code"], expected, "{refused}");
        disconnect(mcp, target).await;
        eprintln!("Edge Stable {mode} refusal passed: {edge}");
    }
    if edge["state"] != "installed_unverified" {
        failures
            .push("The two positive Edge Stable cells require a compatible installed build".into());
    }
}

#[tokio::test]
async fn installed_browser_session_checkpoint() {
    let fixture = installed_fixture();
    let site = Site::start().await;
    let attachment_root = fixture.cache.join("attachments");
    match std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&attachment_root)
    {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => panic!("Cannot create the private attachment directory: {error}"),
    }
    let mut ca = tempfile::NamedTempFile::new_in(&attachment_root).unwrap();
    ca.write_all(&std::fs::read(certificates().join("ca.pem")).unwrap())
        .unwrap();
    let mut mcp = Mcp::start(&fixture).await;
    let mut failures = Vec::new();
    for mode in ["headless", "headed"] {
        for engine in ["chrome", "firefox", "webkit"] {
            let name = format!("acme-{}-{engine}-{mode}", uuid::Uuid::new_v4().simple());
            let profile = std::panic::AssertUnwindSafe(profile_cell(
                &fixture, &mut mcp, &site, engine, mode, &name,
            ))
            .catch_unwind()
            .await;
            finish_cell(
                &mut mcp,
                &format!("Profile {engine}/{mode}"),
                profile,
                &mut failures,
            )
            .await;
            match std::fs::remove_dir_all(fixture.cache.join("profiles").join(name)) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    failures.push(format!("Profile {engine}/{mode} cleanup failed: {error}"))
                }
            }
            let attachment = std::panic::AssertUnwindSafe(attached_cell(
                &fixture,
                &mut mcp,
                engine,
                mode,
                ca.path(),
            ))
            .catch_unwind()
            .await;
            finish_cell(
                &mut mcp,
                &format!("Attachment {engine}/{mode}"),
                attachment,
                &mut failures,
            )
            .await;
        }
        let chrome = std::panic::AssertUnwindSafe(channel_cell(&fixture, &mut mcp, "chrome", mode))
            .catch_unwind()
            .await;
        finish_cell(
            &mut mcp,
            &format!("Chrome Stable {mode}"),
            chrome,
            &mut failures,
        )
        .await;
    }
    edge_cells(&fixture, &mut mcp, &mut failures).await;
    scope_empty(&mut mcp).await;
    mcp.stop().await;
    assert!(
        failures.is_empty(),
        "Session acceptance failures: {failures:#?}"
    );
}
