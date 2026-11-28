//! Installed roadmap 5.2 storage against the pinned SDK in every engine and
//! mode: creation state, cookies, WebStorage, export/restore and replacement
//! of a visited origin; then the native Chrome merge, conversion, Cache Storage
//! and interrupted-apply contracts, and the Firefox/WebKit refusals.
use super::*;

/// Page-observed storage, so differing snapshot formats compare by effect.
const READ: &str = "async()=>{let idb=null;try{idb=await new Promise(resolve=>{const request=indexedDB.open('acme');request.onupgradeneeded=()=>request.transaction.abort();request.onerror=()=>resolve(null);request.onsuccess=()=>{const db=request.result;if(!db.objectStoreNames.contains('state')){db.close();resolve(null);return;}const get=db.transaction('state').objectStore('state').get('key');get.onsuccess=()=>{db.close();resolve(get.result??null);};get.onerror=()=>{db.close();resolve('error');};};});}catch(error){idb='error';}let opfs=null;try{const root=await navigator.storage.getDirectory();opfs=await(await(await root.getFileHandle('acme.txt')).getFile()).text();}catch(error){opfs=null;}const items=storage=>Object.keys(storage).sort().map(key=>[key,storage.getItem(key)]);return JSON.stringify({cookie:document.cookie.split('; ').filter(Boolean).sort(),local:items(localStorage),session:items(sessionStorage),idb,opfs});}";
const WRITE: &str = "async()=>{localStorage.setItem('acme','written');sessionStorage.setItem('acme','session');document.cookie='written=1; path=/';await new Promise((resolve,reject)=>{const request=indexedDB.open('acme',1);request.onupgradeneeded=()=>request.result.createObjectStore('state');request.onerror=()=>reject(request.error);request.onsuccess=()=>{const db=request.result;const write=db.transaction('state','readwrite');write.objectStore('state').put('idb-value','key');write.oncomplete=()=>{db.close();resolve();};write.onerror=()=>reject(write.error);};});try{const root=await navigator.storage.getDirectory();const writable=await(await root.getFileHandle('acme.txt',{create:true})).createWritable();await writable.write('opfs-value');await writable.close();}catch(error){}return JSON.stringify(true);}";
const VISIT: &str =
    "async()=>{localStorage.setItem('visited','other');return JSON.stringify(true);}";
const CACHE: &str = "async()=>{const cache=await caches.open('acme-v1');await cache.put('/data',new Response('Acme',{headers:{'content-type':'text/plain'}}));await cache.put('/other',new Response('Other'));return JSON.stringify((await caches.keys()));}";

fn state(origin: &str) -> Value {
    json!({"cookies":[{"name":"acme","value":"initial","domain":"127.0.0.1","path":"/","expires":-1,"httpOnly":false,"secure":false,"sameSite":"Lax"}],
        "origins":[{"origin":origin,"localStorage":[{"name":"acme","value":"initial"}]}]})
}

async fn server() -> (String, String, tokio::task::JoinHandle<()>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let port = listener.local_addr().unwrap().port();
    let task = tokio::spawn(async move {
        loop {
            let (mut stream, _) = listener.accept().await.unwrap();
            tokio::spawn(async move {
                let mut head = Vec::new();
                let mut buffer = [0; 1024];
                while !head.windows(4).any(|part| part == b"\r\n\r\n") {
                    match stream.read(&mut buffer).await {
                        Ok(read) if read > 0 => head.extend_from_slice(&buffer[..read]),
                        _ => return,
                    }
                }
                let body = "<!doctype html><title>Acme storage</title><p>Acme</p>";
                let response = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: text/html\r\ncontent-length: {}\r\ncache-control: no-store\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes()).await;
            });
        }
    });
    // The other origin is the same listener under another host name.
    (
        format!("http://127.0.0.1:{port}"),
        format!("http://localhost:{port}"),
        task,
    )
}

async fn reference(
    fixture: &Fixture,
    engine: &str,
    mode: &str,
    origin: &str,
    other: &str,
) -> Value {
    let file = format!("sdk-state-{engine}-{mode}.json");
    std::fs::write(
        fixture.workspace.path().join(&file),
        state(origin).to_string(),
    )
    .unwrap();
    let output = tokio::process::Command::new(
        fixture
            .cache
            .join("node-24.12.0/node-v24.12.0-darwin-x64/bin/node"),
    )
    .arg(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/browser-storage-closure-baseline.cjs"
    ))
    .arg(fixture.cache.join("playwright-core-1.64.1/package"))
    .args([engine, mode])
    .arg(fixture.cache.join("browsers"))
    .args([
        origin,
        other,
        &state(origin).to_string(),
        &file,
        &format!("sdk-export-{engine}-{mode}.json"),
        READ,
        WRITE,
        VISIT,
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
/// Creates a context through the CLI, so a relative `storageState` binds to
/// the CLI's working directory, and opens a page on `url`.
async fn context_page(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    mut options: Value,
    url: &str,
) -> (Value, Value) {
    // Testrun requires a layout or device; these are the SDK defaults.
    options["viewport"] = json!({"width":1280,"height":720});
    options["deviceScaleFactor"] = json!(1);
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
    goto(mcp, target, &page, url).await;
    (context, page)
}
async fn goto(mcp: &mut Mcp, target: &str, page: &Value, url: &str) {
    operation(mcp,target,json!({"operation":"page_navigate","page":page,"navigation":{"kind":"goto","url":url,"waitUntil":"load"}})).await;
}
/// Cookies as sorted `name=value`, following every page.
async fn cookies(mcp: &mut Mcp, target: &str, context: &Value, urls: Value) -> Value {
    let mut request =
        json!({"operation":"context_cookies_read","context":context,"urls":urls,"limit":1});
    let mut found = Vec::new();
    loop {
        let page = operation(mcp, target, request.clone()).await;
        for cookie in page["cookies"].as_array().unwrap() {
            found.push(format!(
                "{}={}",
                cookie["name"].as_str().unwrap(),
                cookie["value"].as_str().unwrap()
            ));
        }
        match page["nextRequest"].as_object() {
            Some(next) => request = Value::Object(next.clone()),
            None => break,
        }
    }
    found.sort();
    json!(found)
}
async fn web(mcp: &mut Mcp, target: &str, page: &Value, kind: &str, update: Value) {
    operation(
        mcp,
        target,
        json!({"operation":"page_web_storage_update","page":page,"kind":kind,"update":update}),
    )
    .await;
}
async fn items(mcp: &mut Mcp, target: &str, page: &Value, kind: &str) -> Value {
    operation(
        mcp,
        target,
        json!({"operation":"page_web_storage_read","page":page,"kind":kind}),
    )
    .await["items"]
        .clone()
}
async fn close(mcp: &mut Mcp, target: &str, context: &Value) {
    operation(
        mcp,
        target,
        json!({"operation":"context_close","context":context}),
    )
    .await;
}

#[expect(clippy::too_many_lines, reason = "one SDK-mirrored storage sequence")]
async fn cell(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    engine: &str,
    mode: &str,
    origin: &str,
    other: &str,
    sdk: &Value,
) -> Value {
    let page_url = format!("{origin}/page");
    let other_url = format!("{other}/page");
    let mut report = json!({});
    let (created, page) = context_page(
        fixture,
        mcp,
        target,
        json!({"storageState":state(origin)}),
        &page_url,
    )
    .await;
    report["initial"] = read(mcp, target, &page, READ).await;
    let file = format!("testrun-state-{engine}-{mode}.json");
    std::fs::write(
        fixture.workspace.path().join(&file),
        state(origin).to_string(),
    )
    .unwrap();
    let (from_file, file_page) = context_page(
        fixture,
        mcp,
        target,
        json!({"storageState":file}),
        &page_url,
    )
    .await;
    report["file"] = read(mcp, target, &file_page, READ).await;
    close(mcp, target, &from_file).await;
    operation(mcp,target,json!({"operation":"context_cookies_add","context":created,"cookies":[{"name":"extra","value":"1","url":origin},{"name":"other","value":"2","url":other}]})).await;
    report["selected"] = cookies(mcp, target, &created, json!([format!("{origin}/")])).await;
    operation(mcp,target,json!({"operation":"context_cookies_clear","context":created,"options":{"name":{"regex":"^ext"}}})).await;
    report["cleared"] = cookies(mcp, target, &created, json!([])).await;
    web(
        mcp,
        target,
        &page,
        "local",
        json!({"action":"set","name":"k","value":"v"}),
    )
    .await;
    web(
        mcp,
        target,
        &page,
        "session",
        json!({"action":"set","name":"s","value":"1"}),
    )
    .await;
    report["items"] = items(mcp, target, &page, "local").await;
    report["sessionItems"] = items(mcp, target, &page, "session").await;
    let missing = operation(
        mcp,
        target,
        json!({"operation":"page_web_storage_read","page":page,"kind":"local","name":"missing"}),
    )
    .await;
    assert_eq!(missing["found"], false, "{missing}");
    report["missing"] = missing["value"].clone();
    web(
        mcp,
        target,
        &page,
        "local",
        json!({"action":"remove","name":"k"}),
    )
    .await;
    web(mcp, target, &page, "session", json!({"action":"clear"})).await;
    report["updated"] = read(mcp, target, &page, READ).await;
    read(mcp, target, &page, WRITE).await;
    // A CLI-relative export path, as the SDK's storageState({path}).
    let exported = format!("testrun-export-{engine}-{mode}.json");
    let request = json!({"operation":"storage_export","context":created,"options":{"indexedDB":true,"opfs":engine != "webkit","path":exported}}).to_string();
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &request])
        .await;
    let export = completed(mcp, target, submitted).await;
    let snapshot = export["snapshot"]["reference"].clone();
    assert_eq!(
        export["snapshot"]["format"],
        if engine == "chrome" {
            "testrun-chrome-storage-v1"
        } else {
            "playwright-storage-state-v1"
        },
        "{export}"
    );
    let (restored, restored_page) = context_page(
        fixture,
        mcp,
        target,
        json!({"storageState":exported}),
        &page_url,
    )
    .await;
    report["restored"] = read(mcp, target, &restored_page, READ).await;
    close(mcp, target, &restored).await;
    let (visited, visited_page) = context_page(fixture, mcp, target, json!({}), &other_url).await;
    read(mcp, target, &visited_page, VISIT).await;
    let applied = operation(
        mcp,
        target,
        json!({"operation":"storage_apply","context":visited,"snapshot":snapshot,"mode":"replace"}),
    )
    .await;
    assert_eq!(applied["mode"], "replace", "{applied}");
    goto(mcp, target, &visited_page, &other_url).await;
    report["visitedCleared"] = read(mcp, target, &visited_page, READ).await;
    goto(mcp, target, &visited_page, &page_url).await;
    report["visitedRestored"] = read(mcp, target, &visited_page, READ).await;
    report["visitedCookies"] = cookies(mcp, target, &visited, json!([])).await;
    assert_eq!(&report, sdk, "Testrun {report} SDK {sdk}");
    report["native"] = if engine == "chrome" {
        native(
            fixture,
            mcp,
            target,
            &visited,
            &visited_page,
            &snapshot,
            origin,
        )
        .await
    } else {
        refusals(mcp, target, &visited, &snapshot, origin).await
    };
    close(mcp, target, &visited).await;
    close(mcp, target, &created).await;
    if engine == "chrome" {
        report["interrupted"] = interrupted(mcp, target, &snapshot).await;
    } else {
        operation(
            mcp,
            target,
            json!({"operation":"storage_forget","snapshot":snapshot}),
        )
        .await;
    }
    report
}

/// Native Chrome: merge keeps unnamed state, a Playwright file converts, Cache
/// Storage reads and deletes, and an interrupted apply reports its progress.
async fn native(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    context: &Value,
    page: &Value,
    snapshot: &Value,
    origin: &str,
) -> Value {
    web(
        mcp,
        target,
        page,
        "local",
        json!({"action":"set","name":"kept","value":"1"}),
    )
    .await;
    web(
        mcp,
        target,
        page,
        "local",
        json!({"action":"set","name":"acme","value":"stale"}),
    )
    .await;
    let merged = operation(
        mcp,
        target,
        json!({"operation":"storage_apply","context":context,"snapshot":snapshot,"mode":"merge"}),
    )
    .await;
    assert_eq!(merged["origins"][0]["status"], "merged", "{merged}");
    let after = read(mcp, target, page, READ).await;
    assert_eq!(
        after["local"],
        json!([["acme", "written"], ["kept", "1"]]),
        "{after}"
    );
    // A pinned Playwright file in a private directory registers by conversion.
    let directory = tempfile::Builder::new()
        .prefix("acme-state-")
        .tempdir_in(fixture.workspace.path())
        .unwrap();
    std::fs::set_permissions(
        directory.path(),
        std::os::unix::fs::PermissionsExt::from_mode(0o700),
    )
    .unwrap();
    let path = directory.path().join("playwright.json");
    std::fs::write(&path, state(origin).to_string()).unwrap();
    std::fs::set_permissions(&path, std::os::unix::fs::PermissionsExt::from_mode(0o600)).unwrap();
    let registered = operation(
        mcp,
        target,
        json!({"operation":"storage_register","path":path}),
    )
    .await;
    assert_eq!(registered["converted"], true, "{registered}");
    assert_eq!(
        registered["snapshot"]["format"],
        "testrun-chrome-storage-v1"
    );
    operation(
        mcp,
        target,
        json!({"operation":"storage_forget","snapshot":registered["snapshot"]["reference"]}),
    )
    .await;
    // Cache Storage of the page's origin.
    read(mcp, target, page, CACHE).await;
    let caches = operation(
        mcp,
        target,
        json!({"operation":"context_cache_storage_read","context":context,"origin":origin}),
    )
    .await;
    assert_eq!(
        caches["caches"],
        json!([{"cacheName":"acme-v1"}]),
        "{caches}"
    );
    let entries = operation(mcp,target,json!({"operation":"context_cache_storage_read","context":context,"origin":origin,"cacheName":"acme-v1","limit":1})).await;
    assert_eq!(
        entries["entries"][0]["url"],
        format!("{origin}/data"),
        "{entries}"
    );
    assert!(entries["nextCursor"].is_string(), "{entries}");
    let response = operation(mcp,target,json!({"operation":"context_cache_storage_read","context":context,"origin":origin,"cacheName":"acme-v1","url":format!("{origin}/data")})).await;
    assert_eq!(response["bodyBase64"], "QWNtZQ==", "{response}");
    operation(mcp,target,json!({"operation":"context_cache_storage_delete","context":context,"origin":origin,"cacheName":"acme-v1","url":format!("{origin}/data")})).await;
    operation(mcp,target,json!({"operation":"context_cache_storage_delete","context":context,"origin":origin,"cacheName":"acme-v1"})).await;
    let emptied = operation(
        mcp,
        target,
        json!({"operation":"context_cache_storage_read","context":context,"origin":origin}),
    )
    .await;
    assert_eq!(emptied["caches"], json!([]), "{emptied}");
    json!({"merged":merged["origins"],"converted":true})
}

/// Last in the Chrome cell: cancel a replacing apply in a fresh context. It
/// completes, settles before any dispatch, or reports its progress without
/// replay and fences the session as every uncertain native write does.
async fn interrupted(mcp: &mut Mcp, target: &str, snapshot: &Value) -> Value {
    let context = operation(mcp,target,json!({"operation":"context_create","options":{"viewport":{"width":1280,"height":720},"deviceScaleFactor":1}})).await["context"].clone();
    let submitted = mcp
        .tool("browser_send", json!({"targetId":target,"request":{"operation":"storage_apply","context":context,"snapshot":snapshot,"mode":"replace"}}))
        .await;
    mcp.tool(
        "browser_cancel",
        json!({"targetId":target,"operationId":submitted["operation"]["id"]}),
    )
    .await;
    let status = mcp
        .tool(
            "browser_status",
            json!({"targetId":target,"operationId":submitted["operation"]["id"],"waitMs":30000}),
        )
        .await;
    let settled = &status["operation"];
    assert_eq!(settled["settled"], true, "{status}");
    if settled["outcome"] == "unknown" {
        assert_eq!(settled["result"]["replayed"], false, "{status}");
        assert!(settled["result"]["origins"].is_array(), "{status}");
        let browser = mcp.tool("browser_status", json!({"targetId":target})).await;
        assert_eq!(browser["browser"]["phase"], "lost", "{browser}");
    } else {
        // Completed, or cancelled before dispatch: the session stays usable.
        operation(
            mcp,
            target,
            json!({"operation":"storage_forget","snapshot":snapshot}),
        )
        .await;
        close(mcp, target, &context).await;
    }
    json!({"interrupted":settled["outcome"]})
}

/// Firefox/WebKit refuse merge before admission and Cache Storage as a queued
/// engine refusal.
async fn refusals(
    mcp: &mut Mcp,
    target: &str,
    context: &Value,
    snapshot: &Value,
    origin: &str,
) -> Value {
    collectors::refused(
        mcp,
        target,
        json!({"operation":"storage_apply","context":context,"snapshot":snapshot,"mode":"merge"}),
        "STORAGE_MERGE_UNSUPPORTED",
        false,
    )
    .await;
    collectors::refused(
        mcp,
        target,
        json!({"operation":"context_cache_storage_read","context":context,"origin":origin}),
        "CACHE_STORAGE_UNSUPPORTED",
        true,
    )
    .await;
    json!({"merge":"STORAGE_MERGE_UNSUPPORTED","cacheStorage":"CACHE_STORAGE_UNSUPPORTED"})
}

#[tokio::test]
async fn installed_step_five_storage_closure_checkpoint() {
    let fixture = installed_fixture();
    let (origin, other, server) = server().await;
    let mut mcp = Mcp::start(&fixture).await;
    let mut failures = Vec::new();
    for mode in ["headless", "headed"] {
        for engine in ["chrome", "firefox", "webkit"] {
            let connected = mcp
                .tool("browser_connect", json!({"engine":engine,"mode":mode}))
                .await;
            let target = connected["targetId"].as_str().unwrap().to_owned();
            let ready = fixture.ready_target(&target).await;
            let sdk = reference(&fixture, engine, mode, &origin, &other).await;
            let result = std::panic::AssertUnwindSafe(cell(
                &fixture, &mut mcp, &target, engine, mode, &origin, &other, &sdk,
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
                Ok(report) => eprintln!("Storage closure {engine} {mode}: {report}"),
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
