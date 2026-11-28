//! Combined 5.4 installed CLI/MCP checkpoint: virtual passkeys, credential
//! snapshots and broker sign-in against an owned loopback site and a
//! synthetic broker. Requires explicit live acceptance.
use super::*;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

const PASSWORD: &str = "acme-synthetic-password-54-c41e";
const USERNAME: &str = "staff@example.com";

const SIGN_IN: &str = r#"<!doctype html><title>Acme</title>
<h1 id="sign-in">Sign in to Acme</h1>
<form id="form" onsubmit="return false"><label>Email <input id="username" autocomplete="username"></label>
<label>Password <input id="password" type="password" autocomplete="current-password"></label>
<button id="submit" type="button">Sign in</button></form>
<div id="refusal" role="alert" hidden>Incorrect sign-in</div>
<nav id="account" hidden>Staff user · Acme</nav>
<script>document.getElementById('submit').onclick=()=>{
const ok=[...document.getElementById('password').value].reverse().join('')===REVERSED&&document.getElementById('username').value==='USERNAME';
document.getElementById('refusal').hidden=ok;
if(ok){document.getElementById('form').hidden=true;document.getElementById('sign-in').hidden=true;document.getElementById('account').hidden=false;}};</script>"#;

/// An owned `localhost` site: `/sign-in` and a blank passkey page.
struct AcmeSite {
    url: String,
    port: u16,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for AcmeSite {
    fn drop(&mut self) {
        self.task.abort();
    }
}
async fn site() -> AcmeSite {
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let port = listener.local_addr().unwrap().port();
    // The page never carries the password itself, so retained page copies
    // cannot satisfy or defeat the state scan.
    let page = SIGN_IN
        .replace(
            "REVERSED",
            &json!(PASSWORD.chars().rev().collect::<String>()).to_string(),
        )
        .replace("USERNAME", USERNAME);
    let task = tokio::spawn(async move {
        loop {
            let (mut stream, _) = listener.accept().await.unwrap();
            let page = page.clone();
            tokio::spawn(async move {
                let mut bytes = Vec::new();
                let mut chunk = [0; 1024];
                while !bytes.windows(4).any(|part| part == b"\r\n\r\n") && bytes.len() < 8192 {
                    match stream.read(&mut chunk).await {
                        Ok(0) | Err(_) => return,
                        Ok(count) => bytes.extend_from_slice(&chunk[..count]),
                    }
                }
                let body = if bytes.starts_with(b"GET /sign-in ") {
                    page
                } else {
                    "<!doctype html><title>Acme passkeys</title><p>Acme passkeys</p>".into()
                };
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes()).await;
            });
        }
    });
    AcmeSite {
        url: format!("http://localhost:{port}"),
        port,
        task,
    }
}

/// A synthetic `testrun-broker-v1` broker in the fixture's own data root.
/// `staff` resolves the site password; `intruder` a wrong one.
struct Broker {
    resolved: Arc<Mutex<Vec<Value>>>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Broker {
    fn drop(&mut self) {
        self.task.abort();
    }
}
fn broker(fixture: &Fixture, origin: String) -> Broker {
    use std::os::unix::fs::PermissionsExt;
    // The manager requires its data root private too; the fixture may be first.
    let root = fixture.workspace.data_root();
    let directory = root.join("brokers");
    std::fs::create_dir_all(&directory).unwrap();
    for path in [&root, &directory] {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let listener = tokio::net::UnixListener::bind(directory.join("acme.sock")).unwrap();
    let resolved = Arc::new(Mutex::new(Vec::new()));
    let seen = resolved.clone();
    let task = tokio::spawn(async move {
        loop {
            let (stream, _) = listener.accept().await.unwrap();
            let (read, mut write) = stream.into_split();
            let mut line = String::new();
            if tokio::io::BufReader::new(read)
                .read_line(&mut line)
                .await
                .is_err()
            {
                continue;
            }
            let Ok(request) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            let protocol = "testrun-broker-v1";
            let reply = match request["method"].as_str() {
                Some("describe") => {
                    json!({"protocol":protocol,"accounts":[{"account":"staff","origin":origin,"label":"Staff user"},{"account":"intruder","origin":origin,"label":"Wrong password"}]})
                }
                Some("resolve") => {
                    seen.lock().unwrap().push(request.clone());
                    match request["account"].as_str() {
                        Some("staff") => {
                            json!({"protocol":protocol,"origin":origin,"values":{"username":USERNAME,"password":PASSWORD}})
                        }
                        Some("intruder") => {
                            json!({"protocol":protocol,"values":{"username":USERNAME,"password":"acme-wrong"}})
                        }
                        _ => json!({"protocol":protocol,"error":{"code":"ACCOUNT_UNKNOWN"}}),
                    }
                }
                _ => json!({"protocol":protocol,"error":{"code":"UNAVAILABLE"}}),
            };
            let _ = write.write_all(format!("{reply}\n").as_bytes()).await;
        }
    });
    Broker { resolved, task }
}

/// The settled operation, succeeded or not.
async fn settled(mcp: &mut Mcp, target: &str, request: Value) -> Value {
    let submitted = mcp
        .tool("browser_send", json!({"targetId":target,"request":request}))
        .await;
    // A sign-in may wait 30 s for its outcome, the longest single status wait.
    for _ in 0..4 {
        let status = mcp
            .tool(
                "browser_status",
                json!({"targetId":target,"operationId":submitted["operation"]["id"],"waitMs":30000}),
            )
            .await;
        if status["operation"]["settled"] == true {
            return status["operation"].clone();
        }
    }
    panic!("Operation did not settle: {submitted}");
}
async fn refused(mcp: &mut Mcp, target: &str, request: Value, code: &str) -> Value {
    let operation = settled(mcp, target, request).await;
    assert_eq!(operation["outcome"], "failed", "{operation}");
    assert_eq!(operation["code"], code, "{operation}");
    operation
}
async fn evaluate(mcp: &mut Mcp, target: &str, page: &Value, expression: &str) -> Value {
    let value = operation(
        mcp,
        target,
        json!({"operation":"evaluate","scope":page,"expression":format!("async () => JSON.stringify(await ({expression})())"),"isFunction":true}),
    )
    .await;
    serde_json::from_str(value["value"]["value"].as_str().unwrap()).unwrap()
}
async fn navigate(mcp: &mut Mcp, target: &str, page: &Value, url: &str) {
    operation(
        mcp,
        target,
        json!({"operation":"page_navigate","page":page,"navigation":{"kind":"goto","url":url,"waitUntil":"load"}}),
    )
    .await;
}
async fn credentials(mcp: &mut Mcp, target: &str, context: &Value, action: Value) -> Value {
    let result = operation(
        mcp,
        target,
        json!({"operation":"context_credentials","context":context,"action":action}),
    )
    .await;
    assert_eq!(result["privateKeysReturned"], false, "{result}");
    assert!(!result.to_string().contains("\"privateKey\""), "{result}");
    result
}
fn ids(list: &Value) -> Vec<String> {
    list["credentials"]
        .as_array()
        .unwrap()
        .iter()
        .map(|credential| credential["id"].as_str().unwrap().to_owned())
        .collect()
}
const REGISTER: &str = "async () => { const credential = await navigator.credentials.create({publicKey:{rp:{id:'localhost',name:'Acme'},user:{id:new Uint8Array([7,7,7,7]),name:'staff',displayName:'Staff'},challenge:new Uint8Array(32),pubKeyCredParams:[{type:'public-key',alg:-7}],authenticatorSelection:{residentKey:'required',userVerification:'preferred'}}}); return credential.id; }";
fn assertion(id: Option<&str>) -> String {
    let allow = id.map_or(String::new(), |id| {
        format!(",allowCredentials:[{{type:'public-key',id:Uint8Array.from(atob('{id}'.replace(/-/g,'+').replace(/_/g,'/').padEnd(Math.ceil('{id}'.length/4)*4,'=')),c=>c.charCodeAt(0))}}]")
    });
    format!(
        "async () => {{ const credential = await navigator.credentials.get({{publicKey:{{rpId:'localhost',challenge:new Uint8Array(32),userVerification:'preferred'{allow}}}}}); return credential.id; }}"
    )
}

#[expect(
    clippy::too_many_lines,
    reason = "One ordered passkey checkpoint keeps its registration, propagation and snapshot restore together"
)]
async fn passkeys(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    context: &Value,
    page: &Value,
    site: &AcmeSite,
) -> Value {
    navigate(mcp, target, page, &site.url).await;
    // The first action crosses the CLI, the rest MCP.
    let install =
        json!({"operation":"context_credentials","context":context,"action":{"kind":"install"}});
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &install.to_string()])
        .await;
    assert_eq!(completed(mcp, target, submitted).await["installed"], true);
    let registered = evaluate(mcp, target, page, REGISTER).await;
    let registered = registered.as_str().unwrap().to_owned();
    let listed = credentials(
        mcp,
        target,
        context,
        json!({"kind":"list","rpId":"localhost"}),
    )
    .await;
    assert_eq!(ids(&listed), [registered.clone()], "{listed}");
    let seeded = credentials(
        mcp,
        target,
        context,
        json!({"kind":"create","rpId":"localhost","signCount":5}),
    )
    .await;
    assert_eq!(seeded["pageVisible"], true);
    let seeded = seeded["credential"]["id"].as_str().unwrap().to_owned();
    let duplicate = refused(
        mcp,
        target,
        json!({"operation":"context_credentials","context":context,"action":{"kind":"create","rpId":"localhost","id":registered}}),
        "DUPLICATE_CREDENTIAL_ID",
    )
    .await;
    assert!(!duplicate.to_string().contains("\"privateKey\""));
    // A page opened later receives both passkeys.
    let sibling = operation(
        mcp,
        target,
        json!({"operation":"page_create","context":context}),
    )
    .await["page"]
        .clone();
    navigate(mcp, target, &sibling, &site.url).await;
    assert_eq!(
        evaluate(mcp, target, &sibling, &assertion(Some(&seeded))).await,
        json!(seeded)
    );
    assert_eq!(
        evaluate(mcp, target, &sibling, &assertion(Some(&registered))).await,
        json!(registered)
    );
    let counted = credentials(mcp, target, context, json!({"kind":"list","id":seeded})).await;
    assert!(
        counted["credentials"][0]["signCount"].as_u64().unwrap() >= 6,
        "{counted}"
    );
    let first = credentials(mcp, target, context, json!({"kind":"list","limit":1})).await;
    assert!(first["nextCursor"].is_string(), "{first}");
    // Passkeys round-trip through a private snapshot into a new context.
    let exported = operation(
        mcp,
        target,
        json!({"operation":"storage_export","context":context,"options":{"credentials":true}}),
    )
    .await;
    assert_eq!(exported["included"]["virtualCredentials"], true);
    assert_eq!(exported["snapshot"]["credentialCount"], 2, "{exported}");
    assert!(!exported.to_string().contains("\"privateKey\""));
    let snapshot = exported["snapshot"]["reference"].clone();
    let fresh =
        operation(mcp, target, json!({"operation":"context_create"})).await["context"].clone();
    refused(
        mcp,
        target,
        json!({"operation":"storage_apply","context":fresh,"snapshot":snapshot,"mode":"replace"}),
        "AUTHENTICATOR_INSTALL_REQUIRED",
    )
    .await;
    let applied = operation(
        mcp,
        target,
        json!({"operation":"storage_apply","context":fresh,"snapshot":snapshot,"mode":"replace","installAuthenticator":true}),
    )
    .await;
    let restored_page = operation(
        mcp,
        target,
        json!({"operation":"page_create","context":fresh}),
    )
    .await["page"]
        .clone();
    navigate(mcp, target, &restored_page, &site.url).await;
    assert_eq!(
        evaluate(mcp, target, &restored_page, &assertion(Some(&registered))).await,
        json!(registered)
    );
    let restored = credentials(mcp, target, &fresh, json!({"kind":"list"})).await;
    let mut expected = vec![registered.clone(), seeded.clone()];
    expected.sort();
    assert_eq!(ids(&restored), expected, "{restored}");
    operation(
        mcp,
        target,
        json!({"operation":"context_close","context":fresh}),
    )
    .await;
    operation(
        mcp,
        target,
        json!({"operation":"storage_forget","snapshot":snapshot}),
    )
    .await;
    let deleted = credentials(mcp, target, context, json!({"kind":"delete","id":seeded})).await;
    assert_eq!(deleted["deleted"], true);
    let remaining = credentials(mcp, target, context, json!({"kind":"list"})).await;
    assert_eq!(ids(&remaining), [registered.clone()]);
    operation(
        mcp,
        target,
        json!({"operation":"page_close","page":sibling}),
    )
    .await;
    json!({"registered":registered,"seeded":seeded,"applied":applied["credentials"]})
}

fn journey(origin: &str) -> Value {
    json!({"origin":origin,
        "username":{"kind":"label","text":"Email"},
        "password":{"kind":"label","text":"Password"},
        "submit":{"kind":"role","role":"button","name":"Sign in"},
        "signedIn":{"kind":"css","selector":"#account"},
        "signedOut":{"kind":"role","role":"heading","name":"Sign in to Acme"},
        "failure":{"kind":"role","role":"alert"},
        "identity":{"kind":"css","selector":"#account"}})
}

async fn sign_in(
    mcp: &mut Mcp,
    target: &str,
    page: &Value,
    site: &AcmeSite,
    broker: &Broker,
) -> Value {
    let journey = journey(&site.url);
    navigate(mcp, target, page, &format!("{}/sign-in", site.url)).await;
    let listed = operation(mcp, target, json!({"operation":"auth_broker_list"})).await;
    assert_eq!(listed["brokers"][0]["status"], "available", "{listed}");
    let status = json!({"operation":"auth_status","page":page,"journey":journey,"expectedIdentity":"Staff user"});
    assert_eq!(
        operation(mcp, target, status.clone()).await["state"],
        "signed_out"
    );
    let request = json!({"operation":"auth_sign_in","page":page,"broker":"acme","account":"staff","journey":journey,"expectedIdentity":"Staff user"});
    let signed = operation(mcp, target, request.clone()).await;
    assert_eq!(signed["action"], "signed_in", "{signed}");
    assert_eq!(signed["identity"]["matchesExpected"], true, "{signed}");
    let observed = operation(mcp, target, status.clone()).await;
    assert_eq!(observed["state"], "signed_in", "{observed}");
    assert_eq!(
        operation(mcp, target, request.clone()).await["action"],
        "none"
    );
    assert_eq!(broker.resolved.lock().unwrap().len(), 1);
    // A full load drops the page's session; detection sees the sign-in screen.
    navigate(mcp, target, page, &format!("{}/sign-in", site.url)).await;
    assert_eq!(operation(mcp, target, status).await["state"], "signed_out");
    let mut wrong = request.clone();
    wrong["account"] = json!("intruder");
    let refusal = refused(mcp, target, wrong, "SIGN_IN_REFUSED").await;
    assert!(!refusal.to_string().contains("acme-wrong"), "{refusal}");
    // Another origin of the same server never receives values.
    navigate(
        mcp,
        target,
        page,
        &format!("http://127.0.0.1:{}/sign-in", site.port),
    )
    .await;
    refused(mcp, target, request, "ORIGIN_MISMATCH").await;
    let resolved = broker.resolved.lock().unwrap().clone();
    assert_eq!(resolved.len(), 2, "{resolved:?}");
    assert!(resolved.iter().all(|request| request["origin"] == site.url));
    for result in [&signed, &observed, &refusal] {
        assert!(!result.to_string().contains(PASSWORD));
    }
    json!({"signedIn":signed,"refusal":refusal["code"]})
}

/// No retained Testrun state outside private snapshots repeats the password.
fn scan(directory: &std::path::Path) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            scan(&path);
        } else if kind.is_file()
            && let Ok(bytes) = std::fs::read(&path)
        {
            assert!(
                !bytes
                    .windows(PASSWORD.len())
                    .any(|part| part == PASSWORD.as_bytes()),
                "{} repeats the synthetic password",
                path.display()
            );
        }
    }
}

async fn cell(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    ready: &Value,
    site: &AcmeSite,
    broker: &Broker,
) -> Value {
    let context = &ready["browser"]["selectedContext"];
    let page = &ready["browser"]["selectedPage"];
    let passkeys = passkeys(fixture, mcp, target, context, page, site).await;
    let sign_in = sign_in(mcp, target, page, site, broker).await;
    json!({"passkeys":passkeys,"signIn":sign_in})
}

#[tokio::test]
async fn installed_step_five_credentials_checkpoint() {
    let fixture = installed_fixture();
    let site = site().await;
    let mut mcp = Mcp::start(&fixture).await;
    let mut failures = Vec::new();
    for mode in ["headless", "headed"] {
        for engine in ["chrome", "firefox", "webkit"] {
            let broker = broker(&fixture, site.url.clone());
            let connected = mcp
                .tool("browser_connect", json!({"engine":engine,"mode":mode}))
                .await;
            let target = connected["targetId"].as_str().unwrap().to_owned();
            let ready = fixture.ready_target(&target).await;
            let result = std::panic::AssertUnwindSafe(cell(
                &fixture, &mut mcp, &target, &ready, &site, &broker,
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
            assert_eq!(
                closed["result"]["structuredContent"]["ok"], true,
                "{closed}"
            );
            drop(broker);
            let _ = std::fs::remove_file(fixture.workspace.data_root().join("brokers/acme.sock"));
            match result {
                Ok(report) => {
                    std::fs::write(
                        fixture
                            .workspace
                            .path()
                            .join(format!("credentials-{engine}-{mode}.json")),
                        serde_json::to_vec_pretty(&report).unwrap(),
                    )
                    .unwrap();
                    eprintln!(
                        "Credential acceptance {engine}/{mode} passed; workspace {}",
                        fixture.workspace.path().display()
                    );
                }
                Err(panic) => {
                    let message = panic
                        .downcast_ref::<String>()
                        .cloned()
                        .or_else(|| panic.downcast_ref::<&str>().map(|text| (*text).to_owned()))
                        .unwrap_or_else(|| "non-text assertion".into());
                    failures.push(format!("{engine}/{mode}: {message}"));
                }
            }
        }
    }
    mcp.stop().await;
    scan(&fixture.workspace.data_root());
    assert!(
        failures.is_empty(),
        "Credential acceptance failures: {failures:?}"
    );
}
