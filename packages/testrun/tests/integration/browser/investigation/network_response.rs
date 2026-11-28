//! Browser connection facts and timing, independent of host HTTP-client semantics.
use super::*;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};

struct MetadataSite {
    http: String,
    https: String,
    finish: Arc<tokio::sync::Notify>,
    requests: Arc<AtomicUsize>,
    tasks: Vec<tokio::task::JoinHandle<()>>,
}
impl Drop for MetadataSite {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}
async fn respond<S: AsyncRead + AsyncWrite + Unpin>(
    mut stream: S,
    finish: Arc<tokio::sync::Notify>,
    requests: Arc<AtomicUsize>,
) {
    let mut head = Vec::new();
    while !head.ends_with(b"\r\n\r\n") {
        if head.len() == 16384 {
            return;
        }
        match stream.read_u8().await {
            Ok(byte) => head.push(byte),
            Err(_) => return,
        }
    }
    if head.starts_with(b"GET /pending?") {
        requests.fetch_add(1, Ordering::SeqCst);
        stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nX-Content-Type-Options: nosniff\r\nContent-Length: 4100\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n").await.unwrap();
        stream.write_all(&[b'A'; 4096]).await.unwrap();
        stream.flush().await.unwrap();
        finish.notified().await;
        let _ = stream.write_all(b"Acme").await;
    } else {
        let body = "<!doctype html><title>Acme network facts</title><p>Acme</p>";
        let _=stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await;
    }
}
impl MetadataSite {
    async fn start() -> Self {
        use rustls::pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject};
        use tokio_rustls::{TlsAcceptor, rustls};
        let cert = CertificateDer::from_pem_slice(include_bytes!(
            "../../../../src/browser/http/fixtures/server.pem"
        ))
        .unwrap();
        let key = PrivateKeyDer::from_pem_slice(include_bytes!(
            "../../../../src/browser/http/fixtures/server-key.pem"
        ))
        .unwrap();
        let config = rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(vec![cert], key)
        .unwrap();
        let acceptor = TlsAcceptor::from(Arc::new(config));
        let finish = Arc::new(tokio::sync::Notify::new());
        let requests = Arc::new(AtomicUsize::new(0));
        let mut site = Self {
            http: String::new(),
            https: String::new(),
            finish,
            requests,
            tasks: vec![],
        };
        for tls in [false, true] {
            let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
                .await
                .unwrap();
            let port = listener.local_addr().unwrap().port();
            let url = format!("{}://localhost:{port}", if tls { "https" } else { "http" });
            if tls {
                site.https = url;
            } else {
                site.http = url;
            }
            let finish = site.finish.clone();
            let requests = site.requests.clone();
            let acceptor = acceptor.clone();
            site.tasks.push(tokio::spawn(async move {
                let mut calls=tokio::task::JoinSet::new();
                loop {
                    tokio::select! {
                        accepted=listener.accept()=>{
                            let (stream,_)=accepted.unwrap();let finish=finish.clone();let requests=requests.clone();let acceptor=acceptor.clone();
                            calls.spawn(async move {if tls {if let Ok(stream)=acceptor.accept(stream).await {respond(stream,finish,requests).await;}}else{respond(stream,finish,requests).await;}});
                        },
                        _=calls.join_next(),if !calls.is_empty()=>{},
                    }
                }
            }));
        }
        site
    }
}

async fn exercise(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    (context, page): (&Value, &Value),
    url: &str,
    site: &MetadataSite,
    engine: &str,
) {
    let tls = url.starts_with("https://");
    operation(
        mcp,
        target,
        json!({"operation":"diagnostics_start","context":context}),
    )
    .await;
    operation(mcp,target,json!({"operation":"page_navigate","page":page,"navigation":{"kind":"goto","url":url,"waitUntil":"load"}})).await;
    let before = operation(
        mcp,
        target,
        json!({"operation":"diagnostics_read","context":context}),
    )
    .await;
    let calls = site.requests.load(Ordering::SeqCst);
    operation(mcp,target,json!({"operation":"evaluate","scope":page,"expression":"()=>{globalThis.AcmeNetworkPending=fetch('/pending?token=Acme-private').then(r=>r.text());return true;}","isFunction":true})).await;
    let response=operation(mcp,target,json!({"operation":"diagnostics_wait","context":context,"after":before["latestSequence"],"conditions":[{"event":"response","page":page,"url":format!("{url}/pending")}]})).await;
    let request = &response["event"]["data"]["requestId"];
    let read = json!({"operation":"network_read","context":context,"requestId":request,"options":{"responseDetails":true}});
    let command = read.to_string();
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &command])
        .await;
    let live = completed(mcp, target, submitted).await;
    let details = &live["responseDetails"];
    assert_eq!(live["complete"], false, "{live}");
    assert_eq!(details["timing"]["responseEnd"], -1.0, "{details}");
    assert!(
        details["timing"]["startTime"].as_f64().unwrap() > 1e12,
        "{details}"
    );
    assert!(!live.to_string().contains("token=Acme-private"));
    assert_eq!(site.requests.load(Ordering::SeqCst), calls + 1);
    if engine == "chrome" {
        assert!(details["serverAddr"].is_object(), "{details}");
    }
    if !details["serverAddr"].is_null() {
        assert!(
            details["serverAddr"]["ipAddress"]
                .as_str()
                .unwrap()
                .trim_matches(['[', ']'])
                .parse::<std::net::IpAddr>()
                .unwrap()
                .is_loopback()
        );
        assert_eq!(
            details["serverAddr"]["port"],
            url::Url::parse(url).unwrap().port().unwrap()
        );
    }
    if tls && engine != "webkit" {
        let security = &details["securityDetails"];
        assert!(
            security["protocol"].as_str().unwrap().starts_with("TLS"),
            "{details}"
        );
        assert!(
            security["issuer"].as_str().unwrap().contains("Acme"),
            "{details}"
        );
        assert!(
            security["subjectName"]
                .as_str()
                .unwrap()
                .contains("localhost"),
            "{details}"
        );
        assert!(security["validTo"].as_f64().unwrap() > security["validFrom"].as_f64().unwrap());
        assert_eq!(details["securityFieldsOmitted"], json!([]));
    } else if !tls {
        assert!(details["securityDetails"].is_null(), "{details}");
    }
    assert_eq!(details["connectionDetailsPending"], engine == "webkit");
    site.finish.notify_one();
    let finished=operation(mcp,target,json!({"operation":"evaluate","scope":page,"expression":"()=>AcmeNetworkPending","isFunction":true})).await;
    assert_eq!(
        finished["value"]["value"],
        format!("{}Acme", "A".repeat(4096))
    );
    operation(mcp,target,json!({"operation":"diagnostics_wait","context":context,"after":response["event"]["sequence"],"conditions":[{"event":"request_finished","requestId":request}]})).await;
    let ended = operation(mcp, target, read.clone()).await;
    if let Some(address) = ended["responseDetails"]["serverAddr"].as_object() {
        assert!(
            address["ipAddress"]
                .as_str()
                .unwrap()
                .trim_matches(['[', ']'])
                .parse::<std::net::IpAddr>()
                .unwrap()
                .is_loopback()
        );
        assert_eq!(
            address["port"],
            url::Url::parse(url).unwrap().port().unwrap()
        );
    }
    assert_eq!(ended["responseDetails"]["connectionDetailsPending"], false);
    if tls {
        let security = &ended["responseDetails"]["securityDetails"];
        assert!(
            security["protocol"].as_str().unwrap().starts_with("TLS"),
            "{ended}"
        );
        assert!(
            security["subjectName"]
                .as_str()
                .unwrap()
                .contains("localhost"),
            "{ended}"
        );
        assert!(security["validTo"].as_f64().unwrap() > security["validFrom"].as_f64().unwrap());
        if engine == "webkit" {
            assert!(
                security.get("issuer").is_none(),
                "The pinned WebKit adapter supplies no issuer: {ended}"
            );
        }
    }
    assert_eq!(ended["complete"], true);
    let submitted=mcp.tool("act",json!({"targetId":target,"actions":[{"action":"network_read","context":context,"requestId":request,"options":{"responseDetails":true}}]})).await;
    let action = completed(mcp, target, submitted).await;
    assert_eq!(action["completedActions"], 1);
    assert_eq!(
        action["results"][0]["responseDetails"],
        ended["responseDetails"]
    );
    assert!(
        ended["responseDetails"]["timing"]["responseEnd"]
            .as_f64()
            .unwrap()
            >= ended["responseDetails"]["timing"]["responseStart"]
                .as_f64()
                .unwrap()
    );
    let metadata = operation(
        mcp,
        target,
        json!({"operation":"network_read","context":context,"requestId":request}),
    )
    .await;
    assert!(metadata.get("responseDetails").is_none());
    let history = operation(
        mcp,
        target,
        json!({"operation":"diagnostics_read","context":context}),
    )
    .await;
    assert!(
        !history.to_string().contains("securityDetails")
            && !history.to_string().contains("Acme Shield")
    );
    let other =
        operation(mcp, target, json!({"operation":"context_create"})).await["context"].clone();
    let submitted=mcp.tool("browser_send",json!({"targetId":target,"request":{"operation":"network_read","context":other,"requestId":request,"options":{"responseDetails":true}}})).await;
    let foreign = mcp
        .tool(
            "browser_status",
            json!({"targetId":target,"operationId":submitted["operation"]["id"],"waitMs":30000}),
        )
        .await;
    assert_eq!(foreign["operation"]["code"], "NETWORK_REQUEST_EXPIRED");
    operation(
        mcp,
        target,
        json!({"operation":"context_close","context":other}),
    )
    .await;
    operation(mcp, target, read).await;
    assert_eq!(
        site.requests.load(Ordering::SeqCst),
        calls + 1,
        "Metadata reads replayed a network request"
    );
    eprintln!(
        "Network response facts {engine} {}: {}",
        if tls { "https" } else { "http" },
        ended["responseDetails"]
    );
}

async fn cell(engine: &str, mode: &str) {
    let fixture = installed_fixture();
    let site = MetadataSite::start().await;
    let mut mcp = Mcp::start(&fixture).await;
    let connected = fixture
        .cli(&["browser", "connect", "--engine", engine, "--mode", mode])
        .await;
    let target = connected["targetId"].as_str().unwrap();
    let ready = fixture.ready().await;
    let result = std::panic::AssertUnwindSafe(async {
        exercise(
            &fixture,
            &mut mcp,
            target,
            (&ready["browser"]["selectedContext"],&ready["browser"]["selectedPage"]),
            &site.http,
            &site,
            engine,
        )
        .await;
        if engine=="chrome" {
            let context=&ready["browser"]["selectedContext"];
            let page=&ready["browser"]["selectedPage"];
            let before=operation(&mut mcp,target,json!({"operation":"diagnostics_read","context":context})).await;
            let failed=operation(&mut mcp,target,json!({"operation":"evaluate","scope":page,"isFunction":true,"expression":"async url=>{try{await fetch(url);return false;}catch{return true;}}","argument":{"kind":"json","value":format!("{}/pending?token=Acme-private",site.https)}})).await;
            assert_eq!(failed["value"]["value"],true);
            let event=operation(&mut mcp,target,json!({"operation":"diagnostics_wait","context":context,"after":before["latestSequence"],"conditions":[{"event":"request_failed","page":page,"url":format!("{}/pending",site.https)}]})).await;
            let read=operation(&mut mcp,target,json!({"operation":"network_read","context":context,"requestId":event["event"]["data"]["requestId"],"options":{"responseDetails":true}})).await;
            assert_eq!(read["failed"],true);
            assert!(read["request"]["text"].as_str().is_some_and(|text|text.contains("CERT_")),"{read}");
            assert_eq!(read["responseDetails"]["responseAvailable"],false);
            assert!(read["responseDetails"]["securityDetails"].is_null());
            assert_eq!(read["responseDetails"]["timing"]["responseEnd"],-1.0);
        }
        if engine != "chrome" {
            let context = operation(
                &mut mcp,
                target,
                json!({"operation":"context_create","options":{"viewport":{"width":1280,"height":720},"deviceScaleFactor":1,"ignoreHTTPSErrors":true}}),
            )
            .await["context"]
                .clone();
            let page = operation(
                &mut mcp,
                target,
                json!({"operation":"page_create","context":context,"select":false}),
            )
            .await["page"]
                .clone();
            exercise(
                &fixture,
                &mut mcp,
                target,
                (&context,&page),
                &site.https,
                &site,
                engine,
            )
            .await;
            operation(
                &mut mcp,
                target,
                json!({"operation":"context_close","context":context}),
            )
            .await;
        }
    })
    .catch_unwind()
    .await;
    mcp.tool("disconnect", json!({"targetId":target})).await;
    fixture.cli(&["disconnect"]).await;
    mcp.tool("disconnect", json!({})).await;
    mcp.stop().await;
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}
/// Successful default-trust HTTPS on native Chrome against a public origin.
/// Requires outbound network access; no trust setting or TLS exception is used.
async fn trusted_https_cell(mode: &str) {
    const ORIGIN: &str = "https://example.com/";
    let fixture = installed_fixture();
    let mut mcp = Mcp::start(&fixture).await;
    let connected = fixture
        .cli(&["browser", "connect", "--engine", "chrome", "--mode", mode])
        .await;
    let target = connected["targetId"].as_str().unwrap();
    let ready = fixture.ready().await;
    let result = std::panic::AssertUnwindSafe(async {
        let context = &ready["browser"]["selectedContext"];
        let page = &ready["browser"]["selectedPage"];
        operation(&mut mcp,target,json!({"operation":"diagnostics_start","context":context})).await;
        let before = operation(&mut mcp,target,json!({"operation":"diagnostics_read","context":context})).await;
        let navigated = operation(&mut mcp,target,json!({"operation":"page_navigate","page":page,"navigation":{"kind":"goto","url":ORIGIN,"waitUntil":"load"}})).await;
        assert_eq!(navigated["response"]["status"], 200, "{navigated}");
        let response = operation(&mut mcp,target,json!({"operation":"diagnostics_wait","context":context,"after":before["latestSequence"],"conditions":[{"event":"response","page":page,"url":ORIGIN}]})).await;
        let request = &response["event"]["data"]["requestId"];
        operation(&mut mcp,target,json!({"operation":"diagnostics_wait","context":context,"after":before["latestSequence"],"conditions":[{"event":"request_finished","requestId":request}]})).await;
        let read = json!({"operation":"network_read","context":context,"requestId":request,"options":{"responseDetails":true}});
        let ended = operation(&mut mcp, target, read).await;
        let details = &ended["responseDetails"];
        assert_eq!(ended["complete"], true, "{ended}");
        assert_eq!(details["connectionDetailsPending"], false, "{details}");
        let security = &details["securityDetails"];
        // Chrome reports its CDP protocol name: "TLS 1.x", or "QUIC" over HTTP/3.
        assert!(security["protocol"].as_str().is_some_and(|value| value.starts_with("TLS") || value == "QUIC"), "{details}");
        assert!(security["subjectName"].as_str().is_some_and(|value| !value.is_empty()), "{details}");
        assert!(security["issuer"].as_str().is_some_and(|value| !value.is_empty()), "{details}");
        assert!(security["validTo"].as_f64().unwrap() > security["validFrom"].as_f64().unwrap(), "{details}");
        let address = &details["serverAddr"];
        let ip = address["ipAddress"].as_str().unwrap().trim_matches(['[', ']']).parse::<std::net::IpAddr>().unwrap();
        assert!(!ip.is_loopback(), "{details}");
        assert_eq!(address["port"], 443, "{details}");
        let timing = &details["timing"];
        assert!(timing["startTime"].as_f64().unwrap() > 1e12, "{details}");
        assert!(timing["responseEnd"].as_f64().unwrap() >= timing["responseStart"].as_f64().unwrap(), "{details}");
        let submitted = mcp.tool("act",json!({"targetId":target,"actions":[{"action":"network_read","context":context,"requestId":request,"options":{"responseDetails":true}}]})).await;
        let action = completed(&mut mcp, target, submitted).await;
        assert_eq!(action["results"][0]["responseDetails"], *details);
        eprintln!("Trusted HTTPS facts chrome {mode}: {details}");
    })
    .catch_unwind()
    .await;
    let closed = super::performance_lifecycle::disconnect_observed(
        &fixture,
        &mut mcp,
        target,
        "chrome",
        mode,
        &ready["browser"]["selectedContext"],
    )
    .await;
    fixture.cli(&["disconnect"]).await;
    mcp.tool("disconnect", json!({})).await;
    mcp.stop().await;
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
    assert_eq!(
        closed["result"]["structuredContent"]["ok"], true,
        "{closed}"
    );
}
#[tokio::test]
async fn installed_step_four_trusted_https_chrome_headless() {
    trusted_https_cell("headless").await;
}
#[tokio::test]
async fn installed_step_four_trusted_https_chrome_headed() {
    trusted_https_cell("headed").await;
}
#[tokio::test]
async fn installed_step_four_network_response_chrome_headless() {
    cell("chrome", "headless").await;
}
#[tokio::test]
async fn installed_step_four_network_response_chrome_headed() {
    cell("chrome", "headed").await;
}
#[tokio::test]
async fn installed_step_four_network_response_firefox_headless() {
    cell("firefox", "headless").await;
}
#[tokio::test]
async fn installed_step_four_network_response_firefox_headed() {
    cell("firefox", "headed").await;
}
#[tokio::test]
async fn installed_step_four_network_response_webkit_headless() {
    cell("webkit", "headless").await;
}
#[tokio::test]
async fn installed_step_four_network_response_webkit_headed() {
    cell("webkit", "headed").await;
}
