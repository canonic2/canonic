//! Installed 5.6 workflows using only synthetic loopback sites and feature files.
use super::{
    session_checkpoint::{disconnect, finish_cell, ready, scope_empty, wait},
    *,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use futures_util::FutureExt;
use image::{ExtendedColorType, ImageEncoder, codecs::png::PngEncoder};
use std::sync::Arc;
use tokio::{
    net::{TcpListener, TcpStream},
    sync::{Notify, Semaphore},
    task::{JoinHandle, JoinSet},
};

struct FeatureSite {
    origin: String,
    name: String,
    task: JoinHandle<()>,
    gate: Arc<Gate>,
}

struct Gate {
    armed: std::sync::atomic::AtomicBool,
    requested: Notify,
    release: Semaphore,
    extension_ready: Semaphore,
}

impl Default for Gate {
    fn default() -> Self {
        Self {
            armed: false.into(),
            requested: Notify::new(),
            release: Semaphore::new(0),
            extension_ready: Semaphore::new(0),
        }
    }
}

impl Drop for FeatureSite {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl FeatureSite {
    async fn start() -> Self {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let name = format!("Acme-Testrun-{}", uuid::Uuid::new_v4().simple());
        let gate = Arc::new(Gate::default());
        let shared = gate.clone();
        let manifest_name = name.clone();
        let task = tokio::spawn(async move {
            let mut requests = JoinSet::new();
            loop {
                tokio::select! {
                    stream = listener.accept() => { requests.spawn(respond(stream.unwrap().0, manifest_name.clone(), shared.clone())); },
                    _ = requests.join_next(), if !requests.is_empty() => {},
                }
            }
        });
        Self {
            origin,
            name,
            task,
            gate,
        }
    }

    async fn wait_for_extension_worker(&self) {
        tokio::time::timeout(Duration::from_secs(10), self.gate.extension_ready.acquire())
            .await
            .expect("Chrome started the synthetic extension worker")
            .unwrap()
            .forget();
    }
}

fn icon(dimension: u32) -> Vec<u8> {
    let mut png = Vec::new();
    let pixels = vec![124; (dimension * dimension * 3) as usize];
    PngEncoder::new(&mut png)
        .write_image(&pixels, dimension, dimension, ExtendedColorType::Rgb8)
        .unwrap();
    png
}

async fn respond(mut stream: TcpStream, name: String, gate: Arc<Gate>) {
    use std::sync::atomic::Ordering;
    let mut headers = Vec::new();
    let mut chunk = [0; 1024];
    while headers.len() < 8192 && !headers.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
        let count = stream.read(&mut chunk).await.unwrap_or(0);
        if count == 0 {
            return;
        }
        headers.extend_from_slice(&chunk[..count]);
    }
    let headers = String::from_utf8_lossy(&headers);
    let path = headers
        .lines()
        .next()
        .and_then(|line| line.split_ascii_whitespace().nth(1))
        .unwrap_or_default();
    if path == "/interrupt/" && gate.armed.swap(false, Ordering::AcqRel) {
        gate.requested.notify_one();
        gate.release.acquire().await.unwrap().forget();
    }
    if path == "/extension/worker-ready" {
        gate.extension_ready.add_permits(1);
    }
    let (content_type, body) = match path {
        "/app/manifest.webmanifest" | "/interrupt/manifest.webmanifest" => {
            let scope = if path.starts_with("/interrupt/") { "/interrupt/" } else { "/app/" };
            ("application/manifest+json", json!({"name":name,"short_name":name,"start_url":scope,"scope":scope,"id":scope,"display":"standalone","icons":[{"src":"/icon-192.png","sizes":"192x192","type":"image/png"},{"src":"/icon-512.png","sizes":"512x512","type":"image/png"}]}).to_string().into_bytes())
        }
        "/icon-192.png" => ("image/png", icon(192)),
        "/icon-512.png" => ("image/png", icon(512)),
        "/app/" | "/app/launch" | "/interrupt/" => ("text/html", b"<!doctype html><link rel=manifest href=manifest.webmanifest><title>Acme PWA</title><body>Acme app</body>".to_vec()),
        "/tools" | "/child" => {
            let document = if path == "/tools" { "main" } else { "child" };
            let child = if path == "/tools" { "<iframe id=child src=/child></iframe>" } else { "" };
            ("text/html", format!(r#"<!doctype html><title>Acme tools</title>{child}<script>
            globalThis.registerAcme = () => {{
              globalThis.acmeAbort?.abort(); globalThis.acmeAbort = new AbortController();
              document.modelContext.registerTool({{name:'acme',description:'{document}',inputSchema:{{type:'object',properties:{{outcome:{{enum:['error','pending']}}}}}},execute:async (input) => {{
                if(input.outcome === 'error') return {{isError:true,content:[{{type:'text',text:'Acme rejected input'}}]}};
                if(input.outcome === 'pending') {{
                  globalThis.acmePendingCount = (globalThis.acmePendingCount ?? 0) + 1;
                  await new Promise(() => {{}});
                }}
                return {{content:[{{type:'text',text:'{document}'}}]}};
              }}}}, {{signal:acmeAbort.signal}});
            }};
            registerAcme();
            document.modelContext.registerTool({{name:'acme-debug',description:'debug',annotations:{{debugging:true}},execute:async () => ({{content:[]}})}});
            </script>"#).into_bytes())
        }
        _ => ("text/html", b"<!doctype html><title>Acme neighbor</title><body>Acme neighbor</body>".to_vec()),
    };
    let reply = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    );
    if stream.write_all(reply.as_bytes()).await.is_ok() {
        let _ = stream.write_all(&body).await;
    }
}

async fn navigate(mcp: &mut Mcp, target: &str, page: &Value, url: &str) {
    operation(mcp, target, json!({"operation":"page_navigate","page":page,"navigation":{"kind":"goto","url":url,"waitUntil":"load"}})).await;
}

async fn evaluate(mcp: &mut Mcp, target: &str, page: &Value, expression: &str) -> Value {
    operation(
        mcp,
        target,
        json!({"operation":"evaluate","scope":page,"expression":expression}),
    )
    .await["value"]["value"]
        .clone()
}

async fn fixture_file(mcp: &mut Mcp, target: &str, name: &str, bytes: &[u8]) -> Value {
    operation(mcp, target, json!({"operation":"fixture_register","fixture":{"name":name,"mimeType":"text/plain","dataBase64":STANDARD.encode(bytes),"provenance":"synthetic_generated","cleared":{"by":"Acme checkpoint","method":"Inline synthetic extension source"}}})).await["fixture"]["reference"].clone()
}

async fn extension_directory(mcp: &mut Mcp, target: &str, manifest: &[u8], script: &[u8]) -> Value {
    let manifest = fixture_file(mcp, target, "manifest.json", manifest).await;
    let script = fixture_file(mcp, target, "script.js", script).await;
    operation(mcp, target, json!({"operation":"directory_register","name":"acme-extension","entries":[{"path":"manifest.json","fixture":manifest},{"path":"script.js","fixture":script}]})).await["fixture"]["reference"].clone()
}

async fn extension_workflow(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    page: &Value,
    neighbor: &Value,
    site: &FeatureSite,
) {
    let ready_url = json!(format!("{}/extension/worker-ready", site.origin));
    let worker = format!(
        r#"let clicks = 0;
        chrome.runtime.onInstalled.addListener(() => {{}});
        chrome.action.onClicked.addListener(async tab => {{
          await chrome.scripting.executeScript({{target:{{tabId:tab.id}},
            func:(n) => document.body.dataset.acmeClick=String(n),args:[++clicks]}});
        }});
        void fetch({ready_url});"#
    );
    let directory = extension_directory(mcp, target,
        br#"{"manifest_version":3,"name":"Acme extension","version":"1.0","permissions":["scripting"],"host_permissions":["http://127.0.0.1/*"],"action":{},"background":{"service_worker":"script.js"}}"#,
        worker.as_bytes(),
    ).await;
    let install =
        json!({"operation":"browser_extension","action":{"kind":"install","directory":directory}});
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &install.to_string()])
        .await;
    let installed = completed(mcp, target, submitted).await;
    let id = &installed["extension"]["extensionId"];
    assert!(id.is_string(), "{installed}");
    let listed = operation(
        mcp,
        target,
        json!({"operation":"browser_extension","action":{"kind":"list"}}),
    )
    .await;
    assert_eq!(listed["extensions"][0]["extensionId"], *id);
    for phase in ["install", "reload"] {
        if phase == "reload" {
            operation(mcp, target, json!({"operation":"browser_extension","action":{"kind":"reload","extensionId":id}})).await;
            evaluate(
                mcp,
                target,
                page,
                "delete document.body.dataset.acmeClick; true",
            )
            .await;
        }
        site.wait_for_extension_worker().await;
        operation(mcp, target, json!({"operation":"browser_extension","action":{"kind":"trigger_action","extensionId":id,"page":page}})).await;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        loop {
            if evaluate(mcp, target, page, "document.body.dataset.acmeClick ?? null").await == "1" {
                break;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "Default action after {phase}"
            );
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        assert_eq!(
            evaluate(
                mcp,
                target,
                neighbor,
                "document.body.dataset.acmeClick ?? null"
            )
            .await,
            Value::Null
        );
    }
    operation(
        mcp,
        target,
        json!({"operation":"browser_extension","action":{"kind":"uninstall","extensionId":id}}),
    )
    .await;
    let listed = operation(
        mcp,
        target,
        json!({"operation":"browser_extension","action":{"kind":"list"}}),
    )
    .await;
    assert_eq!(listed["extensions"], json!([]));
}

async fn pwa_workflow(mcp: &mut Mcp, target: &str, page: &Value, site: &FeatureSite) {
    navigate(mcp, target, page, &format!("{}/app/", site.origin)).await;
    let installed = operation(
        mcp,
        target,
        json!({"operation":"browser_pwa","action":{"kind":"install","page":page}}),
    )
    .await;
    let id = &installed["appId"];
    let status = operation(
        mcp,
        target,
        json!({"operation":"browser_pwa","action":{"kind":"list"}}),
    )
    .await;
    assert_eq!(status["apps"][0]["installed"], true);
    let url = format!("{}/app/launch", site.origin);
    let result = operation(
        mcp,
        target,
        json!({"operation":"browser_pwa","action":{"kind":"launch","appId":id,"url":url}}),
    )
    .await;
    let app = &result["page"];
    assert_ne!(app, page);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        if evaluate(mcp, target, app, "location.href").await == url {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "PWA launch did not commit"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    operation(
        mcp,
        target,
        json!({"operation":"browser_pwa","action":{"kind":"uninstall","appId":id}}),
    )
    .await;
    let pages = operation(mcp, target, json!({"operation":"page_list"})).await;
    assert!(
        !pages["items"]
            .to_string()
            .contains(app["id"].as_str().unwrap()),
        "{pages}"
    );
}

async fn webmcp_result(
    mcp: &mut Mcp,
    target: &str,
    scope: &Value,
    invocation: &Value,
    expected: &str,
) -> Value {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        let result = operation(mcp, target, json!({"operation":"web_mcp","scope":scope,"action":{"kind":"read","invocationId":invocation["invocationId"]}})).await;
        if result["state"] == expected {
            return result;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "Expected {expected}: {result}"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

async fn webmcp_workflow(mcp: &mut Mcp, target: &str, page: &Value, site: &FeatureSite) {
    navigate(mcp, target, page, &format!("{}/tools", site.origin)).await;
    let child = operation(
        mcp,
        target,
        json!({"operation":"frame_enter","page":page,"locator":{"kind":"css","selector":"#child"}}),
    )
    .await["frame"]
        .clone();
    let mut main_tool = Value::Null;
    for (scope, description) in [(page, "main"), (&child, "child")] {
        operation(
            mcp,
            target,
            json!({"operation":"web_mcp","scope":scope,"action":{"kind":"enable"}}),
        )
        .await;
        if description == "child" {
            evaluate(mcp, target, scope, "registerAcme(); true").await;
        }
        let status = operation(
            mcp,
            target,
            json!({"operation":"web_mcp","scope":scope,"action":{"kind":"status"}}),
        )
        .await;
        assert_eq!(
            status["tools"].as_array().unwrap().len(),
            1,
            "{description}: {status}"
        );
        let tool = &status["tools"][0];
        assert_eq!(tool["description"], description);
        if description == "main" {
            main_tool = tool["toolRef"].clone();
        }
        let invocation = operation(mcp, target, json!({"operation":"web_mcp","scope":scope,"action":{"kind":"invoke","toolRef":tool["toolRef"]}})).await;
        let result = webmcp_result(mcp, target, scope, &invocation, "completed").await;
        assert!(
            result["result"]["output"].to_string().contains(description),
            "{result}"
        );
    }
    let debug = operation(
        mcp,
        target,
        json!({"operation":"web_mcp_debug","scope":page,"action":{"kind":"enable"}}),
    )
    .await;
    assert_eq!(debug["tools"][0]["name"], "acme-debug");
    let shared_status = operation(
        mcp,
        target,
        json!({"operation":"web_mcp","scope":page,"action":{"kind":"status"}}),
    )
    .await;
    assert_eq!(shared_status["tools"][0]["toolRef"], main_tool);
    evaluate(mcp, target, page, "registerAcme(); true").await;
    let stale = mcp.tool("browser_send",json!({"targetId":target,"request":{"operation":"web_mcp","scope":page,"action":{"kind":"invoke","toolRef":main_tool}}})).await;
    let stale = wait(mcp, target, &stale["operation"]["id"]).await;
    assert_eq!(stale["operation"]["code"], "WEB_MCP_TOOL_STALE", "{stale}");

    let status = operation(
        mcp,
        target,
        json!({"operation":"web_mcp","scope":page,"action":{"kind":"status"}}),
    )
    .await;
    let current = &status["tools"][0]["toolRef"];
    assert_ne!(current, &main_tool);
    let failed = operation(mcp, target, json!({"operation":"web_mcp","scope":page,"action":{"kind":"invoke","toolRef":current,"parameters":{"outcome":"error"}}})).await;
    let failed = webmcp_result(mcp, target, page, &failed, "completed").await;
    assert_eq!(failed["result"]["output"]["isError"], true, "{failed}");

    let pending = operation(mcp, target, json!({"operation":"web_mcp","scope":page,"action":{"kind":"invoke","toolRef":current,"parameters":{"outcome":"pending"}}})).await;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        if evaluate(mcp, target, page, "globalThis.acmePendingCount ?? 0").await == 1 {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "Fixture invocation did not reach its pending effect"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    operation(mcp, target, json!({"operation":"web_mcp","scope":page,"action":{"kind":"cancel","invocationId":pending["invocationId"]}})).await;
    webmcp_result(mcp, target, page, &pending, "canceled").await;
    assert_eq!(
        evaluate(mcp, target, page, "globalThis.acmePendingCount").await,
        1
    );
    for (scope, operation_name) in [
        (&child, "web_mcp"),
        (page, "web_mcp"),
        (page, "web_mcp_debug"),
    ] {
        operation(
            mcp,
            target,
            json!({"operation":operation_name,"scope":scope,"action":{"kind":"disable"}}),
        )
        .await;
    }
}

async fn interrupted_install(mcp: &mut Mcp, target: &str, page: &Value, site: &FeatureSite) {
    use std::sync::atomic::Ordering;
    navigate(mcp, target, page, &format!("{}/interrupt/", site.origin)).await;
    site.gate.armed.store(true, Ordering::Release);
    let submitted = mcp.tool("browser_send",json!({"targetId":target,"request":{"operation":"browser_pwa","action":{"kind":"install","page":page}}})).await;
    let operation = &submitted["operation"]["id"];
    tokio::time::timeout(Duration::from_secs(15), site.gate.requested.notified())
        .await
        .expect("Chrome fetched the fixture after PWA dispatch");
    mcp.tool(
        "browser_cancel",
        json!({"targetId":target,"operationId":operation}),
    )
    .await;
    let terminated = wait(mcp, target, operation).await;
    site.gate.release.add_permits(1);
    assert_eq!(
        terminated["operation"]["code"], "OUTCOME_UNKNOWN",
        "{terminated}"
    );
}

async fn check_shims(name: &str) -> Result<(), String> {
    let home = std::env::var_os("HOME").expect("Host home");
    let applications = std::path::PathBuf::from(home).join("Applications");
    let shim = format!("{name}.app");
    let deadline = tokio::time::Instant::now() + Duration::from_secs(90);
    loop {
        let registered = tokio::time::timeout_at(deadline, registration_exists(name))
            .await
            .map_err(|_| format!("Launch Services query timed out for {name}"))?
            .map_err(|error| format!("Read macOS application registrations: {error}"))?;
        let installed = [
            "Chromium Apps.localized",
            "Chrome Apps.localized",
            "Chrome for Testing Apps.localized",
        ]
        .iter()
        .any(|folder| applications.join(folder).join(&shim).exists());
        if !registered && !installed {
            return Ok(());
        }
        let wake = tokio::time::Instant::now() + Duration::from_secs(5);
        if wake >= deadline {
            return Err(format!(
                "{name} remains (shim {installed}, registration {registered})"
            ));
        }
        tokio::time::sleep_until(wake).await;
    }
}

async fn registration_exists(name: &str) -> std::io::Result<bool> {
    let mut child = tokio::process::Command::new("/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister")
        .arg("-dump")
        .kill_on_drop(true)
        .stdout(std::process::Stdio::piped())
        .spawn()?;
    let mut stdout = BufReader::new(child.stdout.take().expect("Piped registry output"));
    let mut line = Vec::new();
    let mut found = false;
    while stdout.read_until(b'\n', &mut line).await? > 0 {
        found |= String::from_utf8_lossy(&line).contains(name);
        line.clear();
    }
    let status = child.wait().await?;
    if status.success() {
        Ok(found)
    } else {
        Err(std::io::Error::other(format!(
            "Launch Services returned {status}"
        )))
    }
}

async fn extension_restart(
    mcp: &mut Mcp,
    target: &str,
    page: &Value,
    site: &FeatureSite,
    mode: &str,
    profile: &str,
) {
    let source = extension_directory(mcp, target,
            br#"{"manifest_version":3,"name":"Acme restart extension","version":"1.0","content_scripts":[{"matches":["http://127.0.0.1/*"],"js":["script.js"],"run_at":"document_end"}]}"#,
            b"document.body.dataset.acmeRestart = 'loaded';",
        ).await;
    operation(
        mcp,
        target,
        json!({"operation":"browser_extension","action":{"kind":"install","directory":source}}),
    )
    .await;
    let marker = "document.body.dataset.acmeRestart ?? null";
    let url = format!("{}/extension-restart", site.origin);
    navigate(mcp, target, page, &url).await;
    assert_eq!(evaluate(mcp, target, page, marker).await, "loaded");
    disconnect(mcp, target).await;
    let reopened = mcp
        .tool(
            "connect",
            json!({"engine":"chrome","mode":mode,"profile":{"kind":"retained","name":profile}}),
        )
        .await;
    let reopened_target = reopened["targetId"].as_str().unwrap();
    let state = ready(mcp, &reopened).await;
    let reopened_page = &state["selectedPage"];
    navigate(mcp, reopened_target, reopened_page, &url).await;
    let persisted = evaluate(mcp, reopened_target, reopened_page, marker).await == "loaded";
    eprintln!("Native extension content script survived restart ({mode}): {persisted}");
    disconnect(mcp, reopened_target).await;
}

#[derive(Debug, Clone, Copy)]
enum Workflow {
    Extension,
    Pwa,
    WebMcp,
}

async fn feature_cell(
    fixture: &Fixture,
    mcp: &mut Mcp,
    site: &FeatureSite,
    mode: &str,
    profile: Option<&str>,
    workflow: Workflow,
) {
    let mut args = vec!["connect", "--engine", "chrome", "--mode", mode];
    if let Some(profile) = profile {
        args.extend(["--profile", profile]);
    } else if matches!(workflow, Workflow::Extension) {
        args.push("--disposable-profile");
    }
    let connection = fixture.cli(&args).await;
    let target = connection["targetId"].as_str().unwrap();
    let started = ready(mcp, &connection).await;
    if profile.is_none() && matches!(workflow, Workflow::Extension) {
        assert_eq!(started["identity"]["profile"]["kind"], "disposable");
    }
    let page = &started["selectedPage"];
    eprintln!(
        "Feature identity ({workflow:?}, {mode}, retained {}): {}",
        profile.is_some(),
        started["identity"]
    );
    if matches!(workflow, Workflow::WebMcp) {
        webmcp_workflow(mcp, target, page, site).await;
        disconnect(mcp, target).await;
        return;
    }
    let neighbor = operation(
        mcp,
        target,
        json!({"operation":"page_create","context":started["selectedContext"]}),
    )
    .await["page"]
        .clone();
    navigate(mcp, target, &neighbor, &format!("{}/neighbor", site.origin)).await;
    match workflow {
        Workflow::Extension => {
            navigate(mcp, target, page, &format!("{}/target", site.origin)).await;
            navigate(mcp, target, &neighbor, &format!("{}/target", site.origin)).await;
            extension_workflow(fixture, mcp, target, page, &neighbor, site).await;
            if let Some(profile) = profile {
                extension_restart(mcp, target, page, site, mode, profile).await;
                return;
            }
        }
        Workflow::Pwa => {
            pwa_workflow(mcp, target, page, site).await;
            assert_eq!(
                evaluate(mcp, target, &neighbor, "document.title").await,
                "Acme neighbor"
            );
            interrupted_install(mcp, target, page, site).await;
        }
        Workflow::WebMcp => unreachable!("WebMCP uses its own documents"),
    }
    disconnect(mcp, target).await;
}

#[tokio::test]
async fn installed_browser_feature_checkpoint() {
    let fixture = installed_fixture();
    let mut mcp = Mcp::start(&fixture).await;
    let mut failures = Vec::new();
    for mode in ["headless", "headed"] {
        for retained in [false, true] {
            for workflow in [Workflow::Extension, Workflow::Pwa, Workflow::WebMcp] {
                let profile =
                    retained.then(|| format!("acme-features-{}", uuid::Uuid::new_v4().simple()));
                let site = FeatureSite::start().await;
                let result = std::panic::AssertUnwindSafe(feature_cell(
                    &fixture,
                    &mut mcp,
                    &site,
                    mode,
                    profile.as_deref(),
                    workflow,
                ))
                .catch_unwind()
                .await;
                site.gate.release.add_permits(1);
                let name = format!("{workflow:?} {mode}, retained {retained}");
                finish_cell(&mut mcp, &name, result, &mut failures).await;
                // Wait for the Mac app launch and registration lifecycle to
                // finish before opening another Chrome fixture.
                if matches!(workflow, Workflow::Pwa)
                    && let Err(error) = check_shims(&site.name).await
                {
                    failures.push(format!("{name}: {error}"));
                }
                if let Some(profile) = profile {
                    let path = fixture.cache.join("profiles").join(&profile);
                    if path.exists() {
                        std::fs::rename(path, fixture.workspace.path().join(profile))
                            .expect("Retain only the synthetic profile with the fixture");
                    }
                }
            }
        }
    }
    scope_empty(&mut mcp).await;
    mcp.stop().await;
    assert!(
        failures.is_empty(),
        "Feature acceptance failures: {failures:#?}"
    );
}
