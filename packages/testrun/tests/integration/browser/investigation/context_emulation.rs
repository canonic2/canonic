//! Context creation options, environment setters, page media and a pinned
//! device descriptor against the unchanged pinned SDK in every engine/mode:
//! a cross-site child frame and a page-opened popup inherit the options before
//! their first script, a plain context stays
//! unaffected (including while the other is offline) and media changes keep a
//! live preview. Chrome-only page throttling and its Firefox/WebKit refusal.
use super::*;

/// The page blocks inline script unless `bypassCSP` is set.
const PAGE: &str = "<!doctype html><meta name=viewport content='width=device-width'><title>Acme emulation</title><script>globalThis.acmeInline=true</script><p>Acme</p>";
const PROBE: &str = "async()=>{const media=query=>matchMedia(query).matches;let position;try{position=await new Promise((resolve,reject)=>navigator.geolocation.getCurrentPosition(found=>resolve([found.coords.latitude,found.coords.longitude]),reject,{timeout:5000}));}catch(error){position='error:'+error.code;}let registration;try{registration=String(await navigator.serviceWorker.register('/sw.js'));}catch(error){registration='refused';}return JSON.stringify({userAgent:navigator.userAgent,language:navigator.language,timeZone:Intl.DateTimeFormat().resolvedOptions().timeZone,offset:new Date(Date.UTC(2026,0,1)).getTimezoneOffset(),dark:media('(prefers-color-scheme: dark)'),reducedMotion:media('(prefers-reduced-motion: reduce)'),inner:[innerWidth,innerHeight],screen:[screen.width,screen.height],dpr:devicePixelRatio,touch:navigator.maxTouchPoints>0,permission:(await navigator.permissions.query({name:'geolocation'})).state,position,registration,inline:globalThis.acmeInline===true});}";
const ONLINE: &str = "async()=>JSON.stringify({online:navigator.onLine,fetch:await fetch('/data',{cache:'no-store'}).then(()=>'ok',()=>'failed')})";
const GEOLOCATION: &str = "async()=>JSON.stringify(await new Promise((resolve,reject)=>navigator.geolocation.getCurrentPosition(found=>resolve([found.coords.latitude,found.coords.longitude]),reject,{timeout:5000})))";
const PERMISSION: &str =
    "async()=>JSON.stringify((await navigator.permissions.query({name:'geolocation'})).state)";
const MEDIA: &str = "()=>JSON.stringify({print:matchMedia('print').matches,dark:matchMedia('(prefers-color-scheme: dark)').matches,light:matchMedia('(prefers-color-scheme: light)').matches})";
/// `localhost` is another site than `127.0.0.1`, so Chrome isolates the frame.
/// Bounded: a frame that never reports returns `"timeout"` instead of hanging.
const FRAME: &str = "async()=>{const message=new Promise(resolve=>addEventListener('message',event=>resolve(event.data),{once:true}));const frame=document.createElement('iframe');frame.src=location.protocol+'//localhost:'+location.port+'/frame';document.body.append(frame);return await Promise.race([message,new Promise(resolve=>setTimeout(()=>resolve(JSON.stringify('timeout')),10000))]);}";
const FRAME_PAGE: &str = "<!doctype html><script>parent.postMessage(JSON.stringify({userAgent:navigator.userAgent,language:navigator.language,timeZone:Intl.DateTimeFormat().resolvedOptions().timeZone,dark:matchMedia('(prefers-color-scheme: dark)').matches,reducedMotion:matchMedia('(prefers-reduced-motion: reduce)').matches}),'*')</script>";
/// The popup records its environment while parsing, before any later script.
/// Bounded: a refused popup returns `"blocked"`, a silent one `"timeout"`.
const POPUP: &str = "async()=>{const popup=window.open('/popup');if(!popup)return JSON.stringify('blocked');const end=Date.now()+10000;while(!popup.acmeParsed){if(Date.now()>end){popup.close();return JSON.stringify('timeout');}await new Promise(resolve=>setTimeout(resolve,20));}const values=popup.acmeParsed;popup.close();return values;}";
const POPUP_PAGE: &str = "<!doctype html><script>globalThis.acmeParsed=JSON.stringify({userAgent:navigator.userAgent,language:navigator.language,timeZone:Intl.DateTimeFormat().resolvedOptions().timeZone,dark:matchMedia('(prefers-color-scheme: dark)').matches,reducedMotion:matchMedia('(prefers-reduced-motion: reduce)').matches,inner:[innerWidth,innerHeight]})</script>";
/// Only what a descriptor sets. A geolocation read in a context without the
/// permission waits on an unanswered prompt in Firefox, where the
/// `getCurrentPosition` timeout starts only after the permission decision.
const DEVICE: &str = "()=>JSON.stringify({userAgent:navigator.userAgent,inner:[innerWidth,innerHeight],screen:[screen.width,screen.height],dpr:devicePixelRatio,touch:navigator.maxTouchPoints>0})";
const PLAIN: &str = "()=>JSON.stringify({acme:navigator.userAgent.includes('Acme'),timeZone:Intl.DateTimeFormat().resolvedOptions().timeZone,dark:matchMedia('(prefers-color-scheme: dark)').matches,inline:globalThis.acmeInline===true})";

fn options(engine: &str) -> Value {
    let mut options = json!({"viewport":{"width":390,"height":664},"screen":{"width":390,"height":844},"deviceScaleFactor":2,
        "userAgent":"Mozilla/5.0 (Linux; Android 14; Acme) Mobile","locale":"de-DE","timezoneId":"Europe/Berlin",
        "colorScheme":"dark","reducedMotion":"reduce","geolocation":{"latitude":52.5,"longitude":13.4},
        "permissions":["geolocation"],"bypassCSP":true,"serviceWorkers":"block"});
    // Firefox refuses mobile emulation, as the pinned SDK does.
    if engine != "firefox" {
        options["isMobile"] = json!(true);
    }
    options
}

fn device(engine: &str) -> &'static str {
    match engine {
        "chrome" => "Pixel 7",
        "webkit" => "iPhone 15",
        _ => "Desktop Firefox",
    }
}

/// Serves the fixture on IPv4 loopback and, at the same port, IPv6 loopback, so
/// `localhost` reaches it whichever address a browser tries first.
async fn server() -> (String, tokio::task::JoinHandle<()>) {
    let http = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let port = http.local_addr().unwrap().port();
    let origin = format!("http://{}", http.local_addr().unwrap());
    let ipv6 = tokio::net::TcpListener::bind((std::net::Ipv6Addr::LOCALHOST, port))
        .await
        .ok();
    let task = tokio::spawn(async move {
        let ipv6 = ipv6.map(|listener| tokio::spawn(serve(listener)));
        serve(http).await;
        if let Some(ipv6) = ipv6 {
            ipv6.abort();
        }
    });
    (origin, task)
}

async fn serve(http: tokio::net::TcpListener) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    {
        loop {
            let (mut stream, _) = http.accept().await.unwrap();
            tokio::spawn(async move {
                let mut head = Vec::new();
                let mut buffer = [0; 1024];
                while !head.windows(4).any(|part| part == b"\r\n\r\n") {
                    let Ok(read) = stream.read(&mut buffer).await else {
                        return;
                    };
                    if read == 0 {
                        return;
                    }
                    head.extend_from_slice(&buffer[..read]);
                }
                let text = String::from_utf8_lossy(&head).into_owned();
                let response = match text.split_whitespace().nth(1).unwrap_or("/") {
                    "/page" => format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: text/html\r\ncontent-security-policy: script-src 'none'\r\ncontent-length: {}\r\ncache-control: no-store\r\nconnection: close\r\n\r\n{PAGE}",
                        PAGE.len()
                    ),
                    "/data" => "HTTP/1.1 200 OK\r\ncontent-type: text/plain\r\ncontent-length: 4\r\ncache-control: no-store\r\nconnection: close\r\n\r\nAcme".to_owned(),
                    "/popup" => format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: text/html\r\ncontent-length: {}\r\ncache-control: no-store\r\nconnection: close\r\n\r\n{POPUP_PAGE}",
                        POPUP_PAGE.len()
                    ),
                    "/frame" => format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: text/html\r\ncontent-length: {}\r\ncache-control: no-store\r\nconnection: close\r\n\r\n{FRAME_PAGE}",
                        FRAME_PAGE.len()
                    ),
                    _ => "HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
                        .to_owned(),
                };
                let _ = stream.write_all(response.as_bytes()).await;
            });
        }
    }
}

async fn reference(fixture: &Fixture, engine: &str, mode: &str, origin: &str) -> Value {
    let output = tokio::process::Command::new(
        fixture
            .cache
            .join("node-24.12.0/node-v24.12.0-darwin-x64/bin/node"),
    )
    .arg(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/browser-context-emulation-baseline.cjs"
    ))
    .arg(fixture.cache.join("playwright-core-1.64.1/package"))
    .args([engine, mode])
    .arg(fixture.cache.join("browsers"))
    .args([
        origin,
        &options(engine).to_string(),
        device(engine),
        PROBE,
        ONLINE,
        GEOLOCATION,
        PERMISSION,
        MEDIA,
        FRAME,
        PLAIN,
        POPUP,
        DEVICE,
    ])
    .current_dir(fixture.workspace.path())
    .env_clear()
    .env("PATH", "/usr/bin:/bin")
    .env("PLAYWRIGHT_BROWSERS_PATH", fixture.cache.join("browsers"))
    .stdin(std::process::Stdio::null())
    .kill_on_drop(true)
    .output();
    let output = tokio::time::timeout(std::time::Duration::from_secs(300), output)
        .await
        .expect("Pinned SDK reference exceeded 300 s")
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

async fn read(mcp: &mut Mcp, target: &str, page: &Value, source: &str) -> Value {
    let value = operation(
        mcp,
        target,
        json!({"operation":"evaluate","scope":page,"expression":source,"isFunction":true}),
    )
    .await;
    serde_json::from_str(value["value"]["value"].as_str().unwrap()).unwrap()
}

/// Creates a context through the CLI and opens a page on the fixture origin.
async fn context_page(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    options: &Value,
    origin: &str,
) -> (Value, Value) {
    let request = json!({"operation":"context_create","options":options}).to_string();
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &request])
        .await;
    let context = completed(mcp, target, submitted).await["context"].clone();
    let page = operation(
        mcp,
        target,
        json!({"operation":"page_create","context":context}),
    )
    .await["page"]
        .clone();
    operation(mcp,target,json!({"operation":"page_navigate","page":page,"navigation":{"kind":"goto","url":format!("{origin}/page"),"waitUntil":"load"}})).await;
    (context, page)
}

/// Chrome throttles the page's network; Firefox and WebKit refuse before dispatch.
async fn throttling(mcp: &mut Mcp, target: &str, page: &Value, engine: &str) -> Value {
    let request = json!({"operation":"page_emulate_throttling","page":page,"options":{"network":"offline","cpuRate":2}});
    if engine != "chrome" {
        // The engine refusal settles the queued operation before dispatch.
        collectors::refused(mcp, target, request, "THROTTLING_UNSUPPORTED", true).await;
        return json!({"refused":"THROTTLING_UNSUPPORTED"});
    }
    let applied = operation(mcp, target, request).await;
    assert_eq!(
        applied["effective"],
        json!({"cpuRate":2.0,"network":"offline"}),
        "{applied}"
    );
    let offline = read(mcp, target, page, ONLINE).await;
    operation(mcp,target,json!({"operation":"page_emulate_throttling","page":page,"options":{"network":"none","cpuRate":1}})).await;
    let online = read(mcp, target, page, ONLINE).await;
    assert_eq!(offline, json!({"online":false,"fetch":"failed"}));
    assert_eq!(online, json!({"online":true,"fetch":"ok"}));
    json!({"offline":offline,"online":online})
}

async fn cell(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    engine: &str,
    origin: &str,
    sdk: &Value,
) -> Value {
    let (context, page) = context_page(fixture, mcp, target, &options(engine), origin).await;
    let mut report = json!({"created":read(mcp, target, &page, PROBE).await});
    report["frame"] = read(mcp, target, &page, FRAME).await;
    report["popup"] = read(mcp, target, &page, POPUP).await;
    let plain_options = json!({"viewport":{"width":1280,"height":720},"deviceScaleFactor":1});
    let (plain, plain_page) = context_page(fixture, mcp, target, &plain_options, origin).await;
    report["plain"] = read(mcp, target, &plain_page, PLAIN).await;
    let configure = |setting: Value| json!({"operation":"context_configure","context":context,"setting":setting});
    operation(
        mcp,
        target,
        configure(json!({"kind":"offline","offline":true})),
    )
    .await;
    report["offline"] = read(mcp, target, &page, ONLINE).await;
    // Context offline is browser emulation; other contexts keep the network.
    report["plainOnline"] = read(mcp, target, &plain_page, ONLINE).await;
    operation(
        mcp,
        target,
        json!({"operation":"context_close","context":plain}),
    )
    .await;
    operation(
        mcp,
        target,
        configure(json!({"kind":"offline","offline":false})),
    )
    .await;
    report["online"] = read(mcp, target, &page, ONLINE).await;
    operation(
        mcp,
        target,
        configure(json!({"kind":"geolocation","geolocation":{"latitude":48.85,"longitude":2.35}})),
    )
    .await;
    report["geolocation"] = read(mcp, target, &page, GEOLOCATION).await;
    operation(mcp, target, configure(json!({"kind":"permissions_clear"}))).await;
    report["cleared"] = read(mcp, target, &page, PERMISSION).await;
    // Media changes keep a live preview: Firefox/WebKit reconfigure it and
    // native Chrome's screencast continues.
    let preview = operation(
        mcp,
        target,
        json!({"operation":"preview_start","page":page}),
    )
    .await["preview"]
        .clone();
    let media = operation(mcp,target,json!({"operation":"page_emulate_media","page":page,"options":{"media":"print","colorScheme":"light"}})).await;
    assert_eq!(media["dispatchCompleted"], true, "{media}");
    if engine == "chrome" {
        assert_eq!(
            media["previewRecovery"],
            json!({"required":false}),
            "{media}"
        );
    } else {
        assert_eq!(media["previewRecovery"]["succeeded"], true, "{media}");
    }
    report["media"] = read(mcp, target, &page, MEDIA).await;
    let stopped = operation(
        mcp,
        target,
        json!({"operation":"preview_stop","preview":preview}),
    )
    .await;
    assert_eq!(stopped["previewActive"], false, "{stopped}");
    let throttled = throttling(mcp, target, &page, engine).await;
    operation(
        mcp,
        target,
        json!({"operation":"context_close","context":context}),
    )
    .await;
    let (described, described_page) = context_page(
        fixture,
        mcp,
        target,
        &json!({"device":device(engine)}),
        origin,
    )
    .await;
    report["device"] = read(mcp, target, &described_page, DEVICE).await;
    operation(
        mcp,
        target,
        json!({"operation":"context_close","context":described}),
    )
    .await;
    assert_eq!(&report, sdk, "Testrun {report} SDK {sdk}");
    report["throttling"] = throttled;
    report
}

#[tokio::test]
async fn installed_step_five_context_emulation_checkpoint() {
    let fixture = installed_fixture();
    let (origin, server) = server().await;
    let mut mcp = Mcp::start(&fixture).await;
    let mut failures = Vec::new();
    for mode in ["headless", "headed"] {
        for engine in ["chrome", "firefox", "webkit"] {
            let connected = mcp
                .tool("browser_connect", json!({"engine":engine,"mode":mode}))
                .await;
            let target = connected["targetId"].as_str().unwrap().to_owned();
            let ready = fixture.ready_target(&target).await;
            let sdk = reference(&fixture, engine, mode, &origin).await;
            let result = std::panic::AssertUnwindSafe(cell(
                &fixture, &mut mcp, &target, engine, &origin, &sdk,
            ))
            .catch_unwind()
            .await;
            let closed = performance_lifecycle::disconnect_observed(
                &fixture,
                &mut mcp,
                &target,
                engine,
                mode,
                &ready["browser"]["selectedContext"],
            )
            .await;
            if closed["result"]["structuredContent"]["ok"] != true {
                failures.push(format!("{engine}/{mode}: {closed}"));
            }
            match result {
                Ok(report) => eprintln!("Context emulation {engine} {mode}: {report}"),
                Err(panic) => {
                    mcp.stop().await;
                    server.abort();
                    std::panic::resume_unwind(panic);
                }
            }
        }
    }
    mcp.stop().await;
    server.abort();
    assert!(
        failures.is_empty(),
        "Original cleanup failures: {failures:?}"
    );
}
