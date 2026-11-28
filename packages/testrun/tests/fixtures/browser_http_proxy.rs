//! Owned CONNECT/HTTP endpoints: only selected loopback origin ports are reachable.
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub struct Proxy {
    pub url: String,
    requests: Arc<Mutex<Vec<Value>>>,
    task: tokio::task::JoinHandle<()>,
}
impl Proxy {
    pub async fn start(ports: Vec<u16>, required_authorization: Option<String>) -> Self {
        Self::start_mode(ports, required_authorization, false).await
    }
    #[allow(
        dead_code,
        reason = "This shared fixture is also imported by CONNECT-only tests"
    )]
    pub async fn start_forwarding(ports: Vec<u16>, required_authorization: Option<String>) -> Self {
        Self::start_mode(ports, required_authorization, true).await
    }
    async fn start_mode(
        ports: Vec<u16>,
        required_authorization: Option<String>,
        forwarding: bool,
    ) -> Self {
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let captured = requests.clone();
        let task = tokio::spawn(async move {
            let mut tasks = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    accepted=listener.accept(),if tasks.len()<64=>{
                        let (stream,_)=accepted.unwrap();
                        tasks.spawn(connect(stream,ports.clone(),required_authorization.clone(),captured.clone(),forwarding));
                    },
                    Some(result)=tasks.join_next(),if !tasks.is_empty()=>{result.unwrap();},
                }
            }
        });
        Self {
            url,
            requests,
            task,
        }
    }
    pub fn requests(&self) -> Vec<Value> {
        self.requests.lock().unwrap().clone()
    }
}
impl Drop for Proxy {
    fn drop(&mut self) {
        self.task.abort();
    }
}

pub struct UntrustedTls {
    pub url: String,
    pub connections: Arc<std::sync::atomic::AtomicUsize>,
    pub requests: Arc<std::sync::atomic::AtomicUsize>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for UntrustedTls {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl UntrustedTls {
    pub async fn start() -> Self {
        use rustls::pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject};
        use std::sync::atomic::{AtomicUsize, Ordering};
        use tokio_rustls::{TlsAcceptor, rustls};
        let cert = CertificateDer::from_pem_slice(include_bytes!(
            "../../src/browser/http/fixtures/server.pem"
        ))
        .unwrap();
        let key = PrivateKeyDer::from_pem_slice(include_bytes!(
            "../../src/browser/http/fixtures/server-key.pem"
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
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let url = format!(
            "https://localhost:{}",
            listener.local_addr().unwrap().port()
        );
        let connections = Arc::new(AtomicUsize::new(0));
        let requests = Arc::new(AtomicUsize::new(0));
        let (connected, received) = (connections.clone(), requests.clone());
        let task = tokio::spawn(async move {
            let mut tasks = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    accepted=listener.accept(),if tasks.len()<64=>{
                        let (stream,_)=accepted.unwrap();
                        connected.fetch_add(1,Ordering::SeqCst);
                        let (acceptor,received)=(acceptor.clone(),received.clone());
                        tasks.spawn(async move {
                            let Ok(mut stream)=acceptor.accept(stream).await else{return;};
                            let mut bytes=[0;16384];
                            if stream.read(&mut bytes).await.is_ok_and(|count|count>0) {
                                received.fetch_add(1,Ordering::SeqCst);
                                let _=stream.write_all(b"HTTP/1.1 502 Acme fixture\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
                            }
                        });
                    },
                    Some(result)=tasks.join_next(),if !tasks.is_empty()=>{result.unwrap();},
                }
            }
        });
        Self {
            url,
            connections,
            requests,
            task,
        }
    }
}
async fn connect(
    mut client: tokio::net::TcpStream,
    ports: Vec<u16>,
    required: Option<String>,
    requests: Arc<Mutex<Vec<Value>>>,
    forwarding: bool,
) {
    let Some((text, early_payload)) = proxy_header(&mut client, forwarding).await else {
        return;
    };
    let mut fields = text.split_whitespace();
    let method = fields.next().unwrap();
    if !forwarding {
        assert_eq!(method, "CONNECT");
    }
    let target = fields.next().unwrap();
    let absolute = if method == "CONNECT" {
        None
    } else {
        url::Url::parse(target).ok()
    };
    let destination = if method == "CONNECT" {
        target
            .rsplit_once(':')
            .and_then(|(host, port)| Some((host, port.parse::<u16>().ok()?)))
    } else {
        absolute
            .as_ref()
            .and_then(|url| Some((url.host_str()?, url.port_or_known_default()?)))
    };
    let selected = destination.is_some_and(|(host, port)| {
        matches!(host, "127.0.0.1" | "localhost") && ports.contains(&port)
    });
    if forwarding
        && (!selected
            || method != "CONNECT"
                && (!matches!(method, "GET" | "HEAD") || !early_payload.is_empty()))
    {
        let _ = client
            .write_all(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            .await;
        return;
    }
    let (host, port) = destination.unwrap();
    assert!(matches!(host, "127.0.0.1" | "localhost"));
    assert!(ports.contains(&port));
    let authorization = text
        .lines()
        .filter_map(|line| line.split_once(':'))
        .find(|(name, _)| name.eq_ignore_ascii_case("proxy-authorization"))
        .map(|(_, value)| value.trim().to_string());
    {
        let mut records = requests.lock().unwrap();
        assert!(records.len() < 1024);
        let headers = text
            .lines()
            .skip(1)
            .filter_map(|line| line.split_once(':'))
            .map(|(name, value)| (name.to_ascii_lowercase(), json!(value.trim())))
            .collect::<serde_json::Map<_, _>>();
        let mut record = json!({"target":target,"authorization":authorization,"headers":headers});
        if forwarding {
            record["method"] = json!(method);
            if let Some(url) = &absolute {
                record["path"] = json!(url.path());
            }
        }
        records.push(record);
    }
    if required
        .as_ref()
        .is_some_and(|required| Some(required) != authorization.as_ref())
    {
        let _=client.write_all(b"HTTP/1.1 407 Acme proxy authentication\r\nProxy-Authenticate: Basic realm=Acme\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
        return;
    }
    let mut upstream = tokio::net::TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port))
        .await
        .unwrap();
    if method == "CONNECT" {
        client
            .write_all(b"HTTP/1.1 200 Acme tunnel\r\n\r\n")
            .await
            .unwrap();
        upstream.write_all(&early_payload).await.unwrap();
    } else {
        let url = absolute.as_ref().unwrap();
        let mut request = format!(
            "{method} {} HTTP/1.1\r\n",
            &url[url::Position::BeforePath..url::Position::AfterQuery]
        );
        for line in text.lines().skip(1).filter(|line| !line.is_empty()) {
            let (name, _) = line.split_once(':').unwrap();
            if !["proxy-authorization", "proxy-connection", "connection"]
                .iter()
                .any(|excluded| name.eq_ignore_ascii_case(excluded))
            {
                request.push_str(line);
                request.push_str("\r\n");
            }
        }
        request.push_str("Connection: close\r\n\r\n");
        upstream.write_all(request.as_bytes()).await.unwrap();
    }
    let _ = tokio::io::copy_bidirectional(&mut client, &mut upstream).await;
}

async fn proxy_header(
    client: &mut tokio::net::TcpStream,
    forwarding: bool,
) -> Option<(String, Vec<u8>)> {
    let mut bytes = Vec::new();
    let mut buffer = [0; 1024];
    while bytes.len() <= 32768 && !bytes.windows(4).any(|part| part == b"\r\n\r\n") {
        let count = match client.read(&mut buffer).await {
            Ok(count) => count,
            Err(_) if forwarding => return None,
            Err(error) => panic!("Owned CONNECT fixture read failed: {error}"),
        };
        if count == 0 {
            return None;
        }
        bytes.extend_from_slice(&buffer[..count]);
    }
    let end = bytes
        .windows(4)
        .position(|part| part == b"\r\n\r\n")
        .expect("Owned proxy request must have a complete header")
        + 4;
    assert!(end <= 32768);
    if !forwarding {
        assert_eq!(
            end,
            bytes.len(),
            "CONNECT-only fixture has no early payload"
        );
    }
    // Browser-level proxies also receive background requests. Refuse those
    // destinations even when their body shares a read with the bounded header.
    let early_payload = bytes.split_off(end);
    let text = String::from_utf8(bytes).unwrap();
    Some((text, early_payload))
}
