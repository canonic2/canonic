//! Returned owner-helper Promises and transient activation match the pinned SDK.
use super::*;

const INSTALL: &str = r#"(node,options)=>{
const {mode,gateUrl,readyUrl}=options;
globalThis.acmeOwnerReads=0;globalThis.acmeOwnerGesture=null;
Object.defineProperty(node,'ownerDocument',{configurable:true,get(){
globalThis.acmeOwnerReads++;globalThis.acmeOwnerGesture=navigator.userActivation.isActive;
return {documentElement:mode==='reject'&&acmeOwnerReads>1?Promise.reject(new Error('Acme owner rejection')):mode==='pending'&&acmeOwnerReads>1?new Promise(()=>{}):Promise.resolve(document.documentElement)};
}});
fetch(gateUrl).then(()=>{const before=navigator.userActivation.isActive;const popup=window.open('about:blank');if(popup)popup.close();fetch(readyUrl+'?before='+Number(before)+'&opened='+Number(!!popup)+'&active='+Number(navigator.userActivation.isActive));});
}"#;

struct SignalSite {
    url: String,
    ready: tokio::sync::mpsc::UnboundedReceiver<(String, Option<bool>, String)>,
    gates: tokio::sync::mpsc::UnboundedReceiver<(String, tokio::sync::oneshot::Sender<()>)>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for SignalSite {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl SignalSite {
    async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let (sender, ready) = tokio::sync::mpsc::unbounded_channel();
        let (gate_sender, gates) = tokio::sync::mpsc::unbounded_channel();
        let task = tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let sender = sender.clone();
                let gate_sender = gate_sender.clone();
                tokio::spawn(async move {
                    let mut request = Vec::new();
                    let mut bytes = [0; 2048];
                    while request.len() < 16384
                        && !request.windows(4).any(|bytes| bytes == b"\r\n\r\n")
                    {
                        let Ok(count) = stream.read(&mut bytes).await else {
                            return;
                        };
                        if count == 0 {
                            return;
                        }
                        request.extend_from_slice(&bytes[..count]);
                    }
                    let request = String::from_utf8_lossy(&request);
                    let path = request.split_whitespace().nth(1).unwrap_or("");
                    let (status, body) = if let Some(ready) = path.strip_prefix("/ready/") {
                        let (mode, query) = ready.split_once('?').unwrap_or((ready, ""));
                        let active = match query
                            .split('&')
                            .find_map(|field| field.strip_prefix("active="))
                        {
                            Some("0") => Some(false),
                            Some("1") => Some(true),
                            _ => None,
                        };
                        let _ = sender.send((mode.to_owned(), active, query.to_owned()));
                        ("204 No Content", "")
                    } else if let Some(mode) = path.strip_prefix("/gate/") {
                        let (release, waiting) = tokio::sync::oneshot::channel();
                        if gate_sender.send((mode.to_owned(), release)).is_err() {
                            return;
                        }
                        if !matches!(
                            tokio::time::timeout(Duration::from_secs(5), waiting).await,
                            Ok(Ok(()))
                        ) {
                            return;
                        }
                        ("204 No Content", "")
                    } else {
                        ("200 OK", "<!doctype html><button id=Acme>Acme</button>")
                    };
                    let response = format!(
                        "HTTP/1.1 {status}\r\nContent-Type: text/html\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = stream.write_all(response.as_bytes()).await;
                });
            }
        });
        Self {
            url,
            ready,
            gates,
            task,
        }
    }
}
async fn install(site: &mut SignalSite, mcp: &mut Mcp, target: &str, handle: &Value, mode: &str) {
    operation(mcp,target,json!({"operation":"evaluate","scope":handle,"expression":INSTALL,"isFunction":true,"argument":{"kind":"json","value":{"mode":mode,"gateUrl":format!("{}/gate/{mode}",site.url),"readyUrl":format!("{}/ready/{mode}",site.url)}}})).await;
    let until = tokio::time::Instant::now() + Duration::from_secs(5);
    let (gate_mode, release) = tokio::time::timeout_at(until, site.gates.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(gate_mode, mode);
    release.send(()).unwrap();
    let signal = tokio::time::timeout_at(until, site.ready.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (signal.0, signal.1),
        (mode.to_owned(), Some(false)),
        "A page task after the acknowledged installation must measure consumed activation before the owner query: {}",
        signal.2
    );
}
async fn stats(mcp: &mut Mcp, target: &str, page: &Value) {
    let value=operation(mcp,target,json!({"operation":"evaluate","scope":page,"expression":"()=>({reads:acmeOwnerReads,gesture:acmeOwnerGesture})","isFunction":true})).await;
    assert_eq!(value["value"]["kind"], "object");
    let observed = value["value"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| {
            (
                entry["key"].as_str().unwrap().to_owned(),
                entry["value"]["value"].clone(),
            )
        })
        .collect::<serde_json::Map<String, Value>>();
    assert_eq!(Value::Object(observed), json!({"reads":2,"gesture":true}));
}
async fn exercise(fixture: &Fixture, mcp: &mut Mcp, target: &str, context: &Value, mode: &str) {
    let mut site = SignalSite::start().await;
    let page = operation(
        mcp,
        target,
        json!({"operation":"page_create","context":context,"select":true}),
    )
    .await["page"]
        .clone();
    operation(mcp,target,json!({"operation":"page_navigate","page":page,"navigation":{"kind":"goto","url":site.url,"waitUntil":"load"}})).await;
    let handle=operation(mcp,target,json!({"operation":"evaluate","scope":page,"expression":"()=>document.querySelector('#Acme')","isFunction":true,"returnHandle":true})).await["handle"].clone();
    install(&mut site, mcp, target, &handle, "fulfill").await;
    let owner = operation(
        mcp,
        target,
        json!({"operation":"handle_owner_frame","handle":handle}),
    )
    .await;
    assert_eq!(owner["page"], page);
    assert!(!owner["frame"].is_null());
    assert_eq!(owner["potentiallyMutating"], true);
    assert_eq!(owner["relationshipCleanupPending"], false);
    stats(mcp, target, &page).await;
    install(&mut site, mcp, target, &handle, "reject").await;
    let submitted = mcp
        .tool(
            "browser_send",
            json!({"targetId":target,"request":{"operation":"handle_owner_frame","handle":handle}}),
        )
        .await;
    let failed = mcp
        .tool(
            "browser_status",
            json!({"targetId":target,"operationId":submitted["operation"]["id"],"waitMs":30000}),
        )
        .await;
    assert_eq!(
        failed["operation"]["code"], "JAVASCRIPT_EXCEPTION",
        "{failed}"
    );
    assert_eq!(
        failed["operation"]["result"]["relationshipCleanupPending"],
        false
    );
    assert_eq!(
        failed["operation"]["result"]["cause"]["dispatchCompleted"],
        true
    );
    stats(mcp, target, &page).await;
    install(&mut site, mcp, target, &handle, "pending").await;
    let submitted=mcp.tool("browser_send",json!({"targetId":target,"timeoutMs":250,"request":{"operation":"handle_owner_frame","handle":handle}})).await;
    let unknown = mcp
        .tool(
            "browser_status",
            json!({"targetId":target,"operationId":submitted["operation"]["id"],"waitMs":30000}),
        )
        .await;
    std::fs::write(
        fixture
            .workspace
            .path()
            .join(format!("owner-getters-unknown-{mode}.json")),
        serde_json::to_vec_pretty(&unknown).unwrap(),
    )
    .unwrap();
    assert_eq!(unknown["operation"]["outcome"], "unknown", "{unknown}");
    assert_eq!(unknown["operation"]["code"], "OUTCOME_UNKNOWN");
    assert_eq!(
        unknown["operation"]["result"]["relationshipCleanupPending"],
        true
    );
    assert_eq!(
        unknown["browser"]["handleCapacity"],
        json!({"limit":256,"retained":1,"pending":0,"uncertain":0})
    );
    stats(mcp, target, &page).await;
    std::fs::write(
        fixture
            .workspace
            .path()
            .join(format!("owner-getters-{mode}.json")),
        serde_json::to_vec_pretty(
            &json!({"owner":owner,"rejected":failed["operation"],"unknown":unknown["operation"]}),
        )
        .unwrap(),
    )
    .unwrap();
    operation(mcp,target,json!({"operation":"evaluate","scope":handle,"expression":"node=>delete node.ownerDocument","isFunction":true})).await;
    operation(
        mcp,
        target,
        json!({"operation":"handle_dispose","handle":handle}),
    )
    .await;
    operation(mcp, target, json!({"operation":"page_close","page":page})).await;
}

#[tokio::test]
async fn installed_step_four_owner_getters_checkpoint() {
    let fixture = installed_fixture();
    let root = std::path::PathBuf::from(std::env::var_os("TESTRUN_BROWSER_ROOT").unwrap());
    let mut mcp = Mcp::start(&fixture).await;
    let mut failures = Vec::new();
    for mode in ["headless", "headed"] {
        let baseline=Command::new(root.join("node-24.12.0/node-v24.12.0-darwin-x64/bin/node")).arg("tests/fixtures/browser-owner-getter-baseline.cjs").arg(root.join("playwright-core-1.64.1/package")).arg(mode).arg(root.join("browsers/chrome-156.0.8078.47/chrome-mac-x64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing")).output().await.unwrap();
        assert!(
            baseline.status.success(),
            "Pinned getter {mode}: {}",
            String::from_utf8_lossy(&baseline.stderr)
        );
        assert_eq!(
            serde_json::from_slice::<Value>(&baseline.stdout).unwrap(),
            json!({"fulfilled":{"reads":2,"gesture":true},"rejected":{"reads":2,"gesture":true},"activationConsumed":true})
        );
        let connected = fixture
            .cli(&["browser", "connect", "--engine", "chrome", "--mode", mode])
            .await;
        let target = connected["targetId"].as_str().unwrap();
        let ready = fixture.ready_target(target).await;
        let context = &ready["browser"]["selectedContext"];
        let result =
            std::panic::AssertUnwindSafe(exercise(&fixture, &mut mcp, target, context, mode))
                .catch_unwind()
                .await;
        let before_disconnect = mcp.tool("browser_status", json!({"targetId":target})).await;
        std::fs::write(
            fixture
                .workspace
                .path()
                .join(format!("owner-getters-before-disconnect-{mode}.json")),
            serde_json::to_vec_pretty(&before_disconnect).unwrap(),
        )
        .unwrap();
        let closed = performance_lifecycle::disconnect_observed(
            &fixture, &mut mcp, target, "chrome", mode, context,
        )
        .await;
        if closed["result"]["structuredContent"]["ok"] != true {
            failures.push(format!("chrome/{mode}: {closed}"));
        }
        if let Err(panic) = result {
            mcp.stop().await;
            std::panic::resume_unwind(panic);
        }
        eprintln!(
            "Owner getters chrome/{mode} passed; workspace {}",
            fixture.workspace.path().display()
        );
    }
    mcp.stop().await;
    assert!(
        failures.is_empty(),
        "Original cleanup failures: {failures:?}"
    );
}
