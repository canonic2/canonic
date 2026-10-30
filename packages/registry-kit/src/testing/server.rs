//! An owned localhost HTTPS server with the public fixture certificate, and
//! fetchers that trust only its CA.

use crate::acquisition::Fetcher;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject};
use std::{
    collections::BTreeMap,
    io::{self, Read, Write},
    net::TcpListener,
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

/// A response the test server returns for one path.
#[derive(Clone, Debug)]
pub struct Response {
    /// HTTP status code; 302 adds a `Location` header.
    pub status: u16,
    /// Response body.
    pub body: Vec<u8>,
    /// Adds `Content-Encoding: gzip` without encoding the body.
    pub encoding: bool,
    /// Delay before responding.
    pub delay: Duration,
    /// `Content-Length` to announce instead of the body's length; announcing
    /// more than is sent makes the connection close mid-body.
    pub declared_length: Option<usize>,
    /// Sends the body one byte at a time with this pause between bytes.
    pub trickle: Option<Duration>,
    /// Sends an empty chunked body followed by trailer fields, without pauses,
    /// until the client disconnects or the server stops.
    pub endless_trailers: bool,
}

impl Response {
    /// An unencoded HTTP 200 response.
    #[must_use]
    pub fn ok(body: impl Into<Vec<u8>>) -> Self {
        Self {
            status: 200,
            body: body.into(),
            encoding: false,
            delay: Duration::ZERO,
            declared_length: None,
            trickle: None,
            endless_trailers: false,
        }
    }

    fn not_found() -> Self {
        Self {
            status: 404,
            ..Self::ok(Vec::new())
        }
    }
}

type Responses = Arc<Mutex<BTreeMap<String, Response>>>;

/// An owned HTTPS server on `localhost` serving configured responses under `/registry/`.
///
/// The worker thread stops and is joined on drop.
pub struct Server {
    url: String,
    responses: Responses,
    requests: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl Server {
    /// Starts a server on an ephemeral loopback port.
    ///
    /// # Panics
    /// Panics if the listener or fixture TLS configuration cannot be created.
    #[must_use]
    pub fn new() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("owned TLS listener");
        let port = listener.local_addr().expect("TLS address").port();
        listener
            .set_nonblocking(true)
            .expect("nonblocking listener");
        let config = Arc::new(tls_config());
        let responses: Responses = Arc::default();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let worker = {
            let (responses, requests, stop) = (responses.clone(), requests.clone(), stop.clone());
            thread::spawn(move || serve(&listener, &config, &responses, &requests, &stop))
        };
        Self {
            url: format!("https://localhost:{port}/registry/"),
            responses,
            requests,
            stop,
            worker: Some(worker),
        }
    }

    /// The registry base URL, ending in `/`.
    #[must_use]
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Sets the response for a path relative to the base URL.
    pub fn set(&self, path: &str, response: Response) {
        self.responses
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(format!("/registry/{path}"), response);
    }

    /// Request paths received so far, in order.
    #[must_use]
    pub fn requests(&self) -> Vec<String> {
        self.requests
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// A fetcher that trusts only the fixture CA.
    ///
    /// # Panics
    /// Panics if the bundled fixture CA cannot be parsed.
    #[must_use]
    pub fn fetcher(timeout: Duration) -> Fetcher {
        let ca: Vec<_> = ureq::tls::parse_pem(include_bytes!("fixtures/ca.pem"))
            .filter_map(|item| match item.expect("fixture CA") {
                ureq::tls::PemItem::Certificate(cert) => Some(cert),
                _ => None,
            })
            .collect();
        Fetcher::with_test_roots(timeout, ureq::tls::RootCerts::from(ca))
    }
}

impl Default for Server {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            // A panicking worker already failed the request under test.
            let _ = worker.join();
        }
    }
}

fn tls_config() -> rustls::ServerConfig {
    let cert = CertificateDer::from_pem_slice(include_bytes!("fixtures/server.pem"))
        .expect("fixture certificate");
    let key = PrivateKeyDer::from_pem_slice(include_bytes!("fixtures/server-key.pem"))
        .expect("fixture key");
    rustls::ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()
        .expect("fixture TLS versions")
        .with_no_client_auth()
        .with_single_cert(vec![cert], key)
        .expect("fixture TLS config")
}

fn serve(
    listener: &TcpListener,
    config: &Arc<rustls::ServerConfig>,
    responses: &Responses,
    requests: &Mutex<Vec<String>>,
    stop: &AtomicBool,
) {
    while !stop.load(Ordering::Relaxed) {
        let Ok((socket, _)) = listener.accept() else {
            thread::sleep(Duration::from_millis(5));
            continue;
        };
        if socket.set_nonblocking(false).is_err()
            || socket
                .set_read_timeout(Some(Duration::from_millis(500)))
                .is_err()
            || socket
                .set_write_timeout(Some(Duration::from_millis(500)))
                .is_err()
        {
            continue;
        }
        let Ok(connection) = rustls::ServerConnection::new(config.clone()) else {
            continue;
        };
        let mut stream = rustls::StreamOwned::new(connection, socket);
        let Some(path) = request_path(&mut stream) else {
            continue;
        };
        requests
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(path.clone());
        let response = responses
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&path)
            .cloned()
            .unwrap_or_else(Response::not_found);
        thread::sleep(response.delay);
        // The client may have given up already; that is the behavior under test.
        let _ = respond(&mut stream, &response, stop);
    }
}

/// Writes `response`, pacing or extending it as configured, until the client
/// disconnects or the server stops.
fn respond(stream: &mut impl Write, response: &Response, stop: &AtomicBool) -> io::Result<()> {
    if response.endless_trailers {
        let head = format!(
            "HTTP/1.1 {} Fixture\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n0\r\n",
            response.status
        );
        stream.write_all(head.as_bytes())?;
        // Without pauses, so the client never waits long enough for a read to time out.
        let fill = b"X-Fill: a\r\n".repeat(64);
        while !stop.load(Ordering::Relaxed) {
            stream.write_all(&fill)?;
        }
        return Ok(());
    }
    stream.write_all(&head(response))?;
    match response.trickle {
        Some(pause) => {
            for byte in &response.body {
                if stop.load(Ordering::Relaxed) {
                    break;
                }
                stream.write_all(&[*byte])?;
                stream.flush()?;
                thread::sleep(pause);
            }
        }
        None => stream.write_all(&response.body)?,
    }
    stream.flush()
}

/// Reads request headers up to 16 KiB and returns the request path.
fn request_path(stream: &mut impl Read) -> Option<String> {
    let mut request = Vec::new();
    let mut buffer = [0; 1024];
    while request.len() < 16_384 && !request.windows(4).any(|w| w == b"\r\n\r\n") {
        match stream.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(n) => request.extend_from_slice(&buffer[..n]),
        }
    }
    String::from_utf8_lossy(&request)
        .split_whitespace()
        .nth(1)
        .map(str::to_owned)
}

/// The status line and headers of a non-chunked response.
fn head(response: &Response) -> Vec<u8> {
    let encoding = if response.encoding {
        "Content-Encoding: gzip\r\n"
    } else {
        ""
    };
    let location = if response.status == 302 {
        "Location: /registry/redirected.json\r\n"
    } else {
        ""
    };
    format!(
        "HTTP/1.1 {} Fixture\r\nContent-Length: {}\r\n{encoding}{location}Connection: close\r\n\r\n",
        response.status,
        response.declared_length.unwrap_or(response.body.len())
    )
    .into_bytes()
}
