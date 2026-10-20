//! Implementation proxies: a lens frames a page another server owns, such as
//! a Storybook or a dev server, through a loopback proxy on a port of its
//! own. Every request and WebSocket passes through unchanged, except that
//! headers that would stop the page loading inside the workbench frame are
//! dropped, cookies are rewritten to belong to the proxy's origin, and HTML
//! documents gain one script, the preview bridge, which streams the live DOM
//! back to the workbench for capture. A port of its own, not a path, because
//! dev servers ask for absolute paths a prefix would break.
//!
//! An owned implementation, such as a Storybook the server runs, listens on
//! a local socket instead of a port; the proxy asks for that socket on the
//! first request, passing its own port, which the process is told it serves.
use anyhow::{Context as _, Result, anyhow};
use axum::{
    Router,
    body::Body,
    extract::{Request, State},
    http::{HeaderMap, HeaderValue, StatusCode, Uri, header},
    response::{IntoResponse, Response},
};
use futures::future::BoxFuture;
use http_body_util::BodyExt as _;
use hyper_util::rt::TokioIo;
use std::collections::HashMap;
use std::net::Ipv4Addr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::{TcpListener, TcpStream};

/// The largest document the proxy rewrites.
const MAX_DOCUMENT: usize = 32 * 1024 * 1024;
const HOP: [&str; 5] = [
    "connection",
    "keep-alive",
    "proxy-connection",
    "transfer-encoding",
    "upgrade",
];
const DROPPED: [&str; 6] = [
    "content-security-policy",
    "content-security-policy-report-only",
    "x-frame-options",
    "cross-origin-opener-policy",
    "cross-origin-embedder-policy",
    "cross-origin-resource-policy",
];

/// Where an owned implementation listens, given the proxy's port.
pub(crate) type Sockets =
    Arc<dyn Fn(String, u16) -> BoxFuture<'static, Result<PathBuf>> + Send + Sync>;

trait Stream: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Stream for T {}

#[derive(Clone)]
enum Upstream {
    Address {
        host: String,
        port: u16,
        secure: bool,
    },
    Owned {
        key: String,
    },
}

struct Proxy {
    origin: String,
    /// The implementation's origin as the browser would address it.
    target: String,
    upstream: Upstream,
    port: u16,
}

#[derive(Clone)]
struct Shared {
    proxy: Arc<Proxy>,
    bridge: Arc<Mutex<String>>,
    sockets: Sockets,
}

/// The bridge goes first in `<head>` so it listens before the page's own
/// scripts run; a document with no `<head>` or `<html>` gets it at the top.
pub(crate) fn inject(html: &str, tag: &str) -> String {
    static HEAD: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"(?i)<head(\s[^>]*)?>").expect("valid pattern")
    });
    static HTML: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"(?i)<html(\s[^>]*)?>").expect("valid pattern")
    });
    match HEAD.find(html).or_else(|| HTML.find(html)) {
        Some(found) => format!("{}{tag}{}", &html[..found.end()], &html[found.end()..]),
        None => format!("{tag}{html}"),
    }
}

/// The page lives at the proxy's origin over plain HTTP; `Domain`, `Secure`
/// and `SameSite` would keep the browser from storing the cookie there.
pub(crate) fn cookie(value: &str) -> String {
    value
        .split(';')
        .enumerate()
        .filter(|(index, part)| {
            let name = part.trim_start().to_ascii_lowercase();
            *index == 0
                || !(name.starts_with("domain")
                    || name.starts_with("secure")
                    || name.starts_with("samesite"))
        })
        .map(|(_, part)| part)
        .collect::<Vec<_>>()
        .join(";")
}

fn swap_origin(value: &str, from: &str, to: &str) -> String {
    value
        .strip_prefix(from)
        .map_or_else(|| value.to_owned(), |rest| format!("{to}{rest}"))
}

/// Only a navigation gets the bridge: a fetch that answers with HTML is data.
fn is_document(method: &axum::http::Method, request: &HeaderMap, response: &HeaderMap) -> bool {
    let html = response
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.to_ascii_lowercase().starts_with("text/html"));
    if !html || method == axum::http::Method::HEAD {
        return false;
    }
    match request
        .get("sec-fetch-dest")
        .and_then(|value| value.to_str().ok())
    {
        None => true,
        Some(dest) => matches!(dest, "document" | "iframe" | "frame"),
    }
}

async fn connect(shared: &Shared) -> Result<Box<dyn Stream>> {
    match &shared.proxy.upstream {
        Upstream::Address { host, port, secure } => {
            let stream = TcpStream::connect((host.as_str(), *port)).await?;
            if *secure {
                return Ok(Box::new(tls(host, stream).await?));
            }
            Ok(Box::new(stream))
        }
        Upstream::Owned { key } => {
            let socket = (shared.sockets)(key.clone(), shared.proxy.port).await?;
            local_socket(socket).await
        }
    }
}

#[cfg(unix)]
async fn local_socket(path: PathBuf) -> Result<Box<dyn Stream>> {
    Ok(Box::new(
        tokio::net::UnixStream::connect(&path)
            .await
            .with_context(|| format!("connect to {}", path.display()))?,
    ))
}

#[cfg(windows)]
async fn local_socket(path: PathBuf) -> Result<Box<dyn Stream>> {
    let pipe = tokio::net::windows::named_pipe::ClientOptions::new()
        .open(&path)
        .with_context(|| format!("connect to {}", path.display()))?;
    Ok(Box::new(pipe))
}

/// A TLS connection that accepts the implementation's certificate whatever
/// it is: local dev servers use self-signed ones.
pub(crate) async fn tls(
    host: &str,
    stream: TcpStream,
) -> Result<tokio_rustls::client::TlsStream<TcpStream>> {
    use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
    use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
    #[derive(Debug)]
    struct Any(Arc<rustls::crypto::CryptoProvider>);
    impl ServerCertVerifier for Any {
        fn verify_server_cert(
            &self,
            _: &CertificateDer<'_>,
            _: &[CertificateDer<'_>],
            _: &ServerName<'_>,
            _: &[u8],
            _: UnixTime,
        ) -> Result<ServerCertVerified, rustls::Error> {
            Ok(ServerCertVerified::assertion())
        }
        fn verify_tls12_signature(
            &self,
            message: &[u8],
            cert: &CertificateDer<'_>,
            signature: &rustls::DigitallySignedStruct,
        ) -> Result<HandshakeSignatureValid, rustls::Error> {
            rustls::crypto::verify_tls12_signature(
                message,
                cert,
                signature,
                &self.0.signature_verification_algorithms,
            )
        }
        fn verify_tls13_signature(
            &self,
            message: &[u8],
            cert: &CertificateDer<'_>,
            signature: &rustls::DigitallySignedStruct,
        ) -> Result<HandshakeSignatureValid, rustls::Error> {
            rustls::crypto::verify_tls13_signature(
                message,
                cert,
                signature,
                &self.0.signature_verification_algorithms,
            )
        }
        fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
            self.0.signature_verification_algorithms.supported_schemes()
        }
    }
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = rustls::ClientConfig::builder_with_provider(provider.clone())
        .with_safe_default_protocol_versions()?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(Any(provider)))
        .with_no_client_auth();
    let name = ServerName::try_from(host.to_owned())?;
    Ok(tokio_rustls::TlsConnector::from(Arc::new(config))
        .connect(name, stream)
        .await?)
}

fn failed(shared: &Shared, error: &anyhow::Error) -> Response {
    // An owned process's own error says why it isn't there.
    let message = match shared.proxy.upstream {
        Upstream::Owned { .. } => format!("{error:#}"),
        Upstream::Address { .. } => format!(
            "{} isn’t answering — is it running? ({error:#})",
            shared.proxy.target
        ),
    };
    let mut response = (StatusCode::BAD_GATEWAY, message).into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    response
}

fn host_of(target: &str) -> String {
    target
        .split_once("://")
        .map_or(target, |(_, rest)| rest)
        .trim_end_matches('/')
        .to_owned()
}

/// Addresses the implementation as itself: its host, origin and referrer.
fn readdress(headers: &mut HeaderMap, proxy: &Proxy, upgrade: bool) {
    if !upgrade {
        for name in HOP {
            headers.remove(name);
        }
        // Documents are rewritten, so ask for them uncompressed.
        headers.remove(header::ACCEPT_ENCODING);
    }
    if let Ok(host) = HeaderValue::from_str(&host_of(&proxy.target)) {
        headers.insert(header::HOST, host);
    }
    for name in [header::ORIGIN, header::REFERER] {
        if let Some(value) = headers
            .get(&name)
            .and_then(|value| value.to_str().ok())
            .map(|value| swap_origin(value, &proxy.origin, &proxy.target))
            && let Ok(value) = HeaderValue::from_str(&value)
        {
            headers.insert(name, value);
        }
    }
}

/// Keeps redirects and cookies on the proxy and drops what blocks framing.
fn relay_headers(headers: &mut HeaderMap, proxy: &Proxy) {
    for name in HOP.iter().chain(DROPPED.iter()) {
        headers.remove(*name);
    }
    if let Some(location) = headers
        .get(header::LOCATION)
        .and_then(|value| value.to_str().ok())
        .map(|value| swap_origin(value, &proxy.target, &proxy.origin))
        && let Ok(location) = HeaderValue::from_str(&location)
    {
        headers.insert(header::LOCATION, location);
    }
    let cookies: Vec<HeaderValue> = headers
        .get_all(header::SET_COOKIE)
        .iter()
        .filter_map(|value| {
            value
                .to_str()
                .ok()
                .and_then(|value| HeaderValue::from_str(&cookie(value)).ok())
        })
        .collect();
    headers.remove(header::SET_COOKIE);
    for value in cookies {
        headers.append(header::SET_COOKIE, value);
    }
}

/// The document with the bridge first in its head, never cached.
async fn with_bridge(
    shared: &Shared,
    mut parts: hyper::http::response::Parts,
    body: hyper::body::Incoming,
) -> Response {
    let collected = match http_body_util::Limited::new(body, MAX_DOCUMENT)
        .collect()
        .await
    {
        Ok(collected) => collected.to_bytes(),
        Err(error) => {
            return failed(
                shared,
                &anyhow!("document too large to rewrite, or interrupted: {error}"),
            );
        }
    };
    let tag = format!(
        "<script src=\"{}\"></script>",
        shared.bridge.lock().expect("bridge lock")
    );
    let html = inject(&String::from_utf8_lossy(&collected), &tag);
    // The rewritten document names this session's workbench port; a cached
    // copy from another session would name a stale one.
    for name in [
        header::ETAG,
        header::LAST_MODIFIED,
        header::CONTENT_ENCODING,
    ] {
        parts.headers.remove(name);
    }
    parts
        .headers
        .insert(header::CONTENT_LENGTH, HeaderValue::from(html.len()));
    parts
        .headers
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Response::from_parts(parts, Body::from(html))
}

async fn forward(State(shared): State<Shared>, mut request: Request) -> Response {
    let upgrade = request.headers().contains_key(header::UPGRADE);
    let method = request.method().clone();
    let request_headers = request.headers().clone();
    let client_upgrade = upgrade.then(|| hyper::upgrade::on(&mut request));
    readdress(request.headers_mut(), &shared.proxy, upgrade);
    let target = request
        .uri()
        .path_and_query()
        .map_or("/", |target| target.as_str())
        .to_owned();
    match target.parse::<Uri>() {
        Ok(uri) => *request.uri_mut() = uri,
        Err(_) => return failed(&shared, &anyhow!("the request target is not a valid path")),
    }
    let io = match connect(&shared).await {
        Ok(io) => io,
        Err(error) => return failed(&shared, &error),
    };
    let (mut sender, connection) =
        match hyper::client::conn::http1::handshake(TokioIo::new(io)).await {
            Ok(pair) => pair,
            Err(error) => return failed(&shared, &anyhow!(error)),
        };
    tokio::spawn(async move {
        let _done = connection.with_upgrades().await;
    });
    let mut response = match sender.send_request(request).await {
        Ok(response) => response,
        Err(error) => return failed(&shared, &anyhow!(error)),
    };
    if response.status() == StatusCode::SWITCHING_PROTOCOLS
        && let Some(client) = client_upgrade
    {
        let server = hyper::upgrade::on(&mut response);
        tokio::spawn(async move {
            if let (Ok(client), Ok(server)) = tokio::join!(client, server) {
                let mut client = TokioIo::new(client);
                let mut server = TokioIo::new(server);
                let _closed = tokio::io::copy_bidirectional(&mut client, &mut server).await;
            }
        });
        return response.map(|_| Body::empty());
    }
    let document = is_document(&method, &request_headers, response.headers());
    let (mut parts, body) = response.into_parts();
    relay_headers(&mut parts.headers, &shared.proxy);
    if document {
        with_bridge(&shared, parts, body).await
    } else {
        Response::from_parts(parts, Body::new(body))
    }
}

/// Every proxy the server has opened, by implementation origin or owner key.
pub(crate) struct Proxies {
    bridge: Arc<Mutex<String>>,
    sockets: Sockets,
    opened: tokio::sync::Mutex<HashMap<String, Arc<Proxy>>>,
    origins: Mutex<Vec<String>>,
}

impl Proxies {
    pub(crate) fn new(sockets: Sockets) -> Arc<Self> {
        Arc::new(Self {
            bridge: Arc::new(Mutex::new(String::new())),
            sockets,
            opened: tokio::sync::Mutex::new(HashMap::new()),
            origins: Mutex::new(vec![]),
        })
    }

    /// The bridge script's address, once the front door listens.
    pub(crate) fn set_bridge(&self, port: u16) {
        *self.bridge.lock().expect("bridge lock") =
            format!("http://127.0.0.1:{port}/_workbench/preview-bridge.js");
    }

    async fn open(&self, key: String, target: Option<String>) -> Result<Arc<Proxy>> {
        let mut opened = self.opened.lock().await;
        if let Some(proxy) = opened.get(&key) {
            return Ok(proxy.clone());
        }
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
        let port = listener.local_addr()?.port();
        let origin = format!("http://127.0.0.1:{port}");
        let (target, upstream) = match target {
            Some(target) => {
                let url = target.parse::<Uri>().context("implementation address")?;
                let secure = url.scheme_str() == Some("https");
                let host = url
                    .host()
                    .context("implementation host")?
                    .trim_matches(['[', ']'])
                    .to_owned();
                let port = url.port_u16().unwrap_or(if secure { 443 } else { 80 });
                (
                    target.trim_end_matches('/').to_owned(),
                    Upstream::Address { host, port, secure },
                )
            }
            // An owned process believes it serves this port on localhost.
            None => (
                format!("http://localhost:{port}"),
                Upstream::Owned {
                    key: key.trim_start_matches("owned:").to_owned(),
                },
            ),
        };
        let proxy = Arc::new(Proxy {
            origin: origin.clone(),
            target,
            upstream,
            port,
        });
        let shared = Shared {
            proxy: proxy.clone(),
            bridge: self.bridge.clone(),
            sockets: self.sockets.clone(),
        };
        let app = Router::new().fallback(forward).with_state(shared);
        tokio::spawn(async move {
            let _served = axum::serve(listener, app).await;
        });
        opened.insert(key, proxy.clone());
        self.origins.lock().expect("origins lock").push(origin);
        Ok(proxy)
    }

    /// The same address at its implementation origin's proxy.
    pub(crate) async fn address(&self, address: &str) -> Result<String> {
        let uri = address.parse::<Uri>().context("implementation address")?;
        let scheme = uri
            .scheme_str()
            .context("implementation address needs http or https")?;
        let authority = uri
            .authority()
            .context("implementation address needs a host")?;
        let origin = format!("{scheme}://{authority}");
        let proxy = self.open(origin.clone(), Some(origin.clone())).await?;
        Ok(swap_origin(address, &proxy.target, &proxy.origin))
    }

    /// The origin of an owned implementation's proxy.
    pub(crate) async fn owned(&self, key: &str) -> Result<String> {
        Ok(self
            .open(format!("owned:{key}"), None)
            .await?
            .origin
            .clone())
    }

    /// Every proxy origin open so far, for checking a capture's address.
    pub(crate) fn origins(&self) -> Vec<String> {
        self.origins.lock().expect("origins lock").clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One GET over loopback, answering the status, headers and body.
    async fn get(origin: &str, path: &str) -> (u16, String) {
        let stream = TcpStream::connect(origin.trim_start_matches("http://"))
            .await
            .unwrap();
        let (mut sender, connection) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
            .await
            .unwrap();
        tokio::spawn(connection);
        let request = hyper::Request::get(path)
            .header(header::HOST, origin.trim_start_matches("http://"))
            .header("sec-fetch-dest", "iframe")
            .body(http_body_util::Empty::<axum::body::Bytes>::new())
            .unwrap();
        let response = sender.send_request(request).await.unwrap();
        let status = response.status().as_u16();
        let body = response.into_body().collect().await.unwrap().to_bytes();
        (status, String::from_utf8_lossy(&body).into_owned())
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn an_owned_implementation_starts_on_its_first_request_and_is_reached_over_its_socket() {
        let folder = std::env::temp_dir().join(format!("wb-owned-{}", std::process::id()));
        let _clean = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        let socket = folder.join("owned.sock");
        let started: Arc<Mutex<Vec<u16>>> = Arc::default();
        let seen: Arc<Mutex<Vec<String>>> = Arc::default();
        let (record, path, hosts) = (started.clone(), socket.clone(), seen.clone());
        let sockets: Sockets = Arc::new(move |key: String, port: u16| {
            let (record, path, hosts) = (record.clone(), path.clone(), hosts.clone());
            Box::pin(async move {
                if key != "storybook" {
                    return Err(anyhow!("Storybook isn’t installed in /acme."));
                }
                let first = {
                    let mut record = record.lock().unwrap();
                    record.push(port);
                    record.len() == 1
                };
                if first {
                    let listener = tokio::net::UnixListener::bind(&path)?;
                    let app = Router::new().fallback(move |request: Request| {
                        let hosts = hosts.clone();
                        async move {
                            let host = request
                                .headers()
                                .get(header::HOST)
                                .and_then(|host| host.to_str().ok())
                                .unwrap_or_default()
                                .to_owned();
                            hosts.lock().unwrap().push(host);
                            (
                                [(header::CONTENT_TYPE, "text/html")],
                                "<html><head></head><body>Acme</body></html>",
                            )
                        }
                    });
                    tokio::spawn(async move { axum::serve(listener, app).await });
                }
                Ok(path)
            })
        });
        let proxies = Proxies::new(sockets);
        proxies.set_bridge(3579);
        let origin = proxies.owned("storybook").await.unwrap();
        let port: u16 = origin.rsplit(':').next().unwrap().parse().unwrap();
        assert!(
            started.lock().unwrap().is_empty(),
            "nothing starts until a request needs it"
        );
        let (status, body) = get(&origin, "/iframe.html").await;
        assert_eq!(status, 200);
        assert!(
            body.contains(
                "<head><script src=\"http://127.0.0.1:3579/_workbench/preview-bridge.js\"></script>"
            ),
            "{body}"
        );
        assert_eq!(
            started.lock().unwrap()[0],
            port,
            "the process is told the proxy’s port"
        );
        assert_eq!(seen.lock().unwrap()[0], format!("localhost:{port}"));
        assert_eq!(
            proxies.owned("storybook").await.unwrap(),
            origin,
            "one proxy per owned process"
        );
        assert!(proxies.origins().contains(&origin));
        let failing = proxies.owned("other").await.unwrap();
        let (status, body) = get(&failing, "/iframe.html").await;
        assert_eq!(status, 502);
        assert!(
            body.contains("Storybook isn’t installed in /acme."),
            "{body}"
        );
        std::fs::remove_dir_all(&folder).unwrap();
    }

    #[test]
    fn documents_get_the_bridge_first_and_cookies_stay_on_the_proxy() {
        let tag = "<script src=\"/bridge.js\"></script>";
        assert_eq!(
            inject(
                "<!doctype html><html><head><title>A</title></head></html>",
                tag
            ),
            format!("<!doctype html><html><head>{tag}<title>A</title></head></html>")
        );
        assert_eq!(
            inject("<html lang=\"en\"><body></body></html>", tag),
            format!("<html lang=\"en\">{tag}<body></body></html>")
        );
        assert_eq!(
            inject("<p>Fragment</p>", tag),
            format!("{tag}<p>Fragment</p>")
        );
        assert_eq!(
            cookie("session=1; Domain=example.com; Path=/; Secure; SameSite=None; HttpOnly"),
            "session=1; Path=/; HttpOnly"
        );
        assert_eq!(
            swap_origin(
                "http://localhost:6006/a",
                "http://localhost:6006",
                "http://127.0.0.1:9"
            ),
            "http://127.0.0.1:9/a"
        );
        assert_eq!(
            swap_origin("https://other.example.com/", "http://localhost:6006", "x"),
            "https://other.example.com/"
        );
    }
}
