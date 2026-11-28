//! Scripted SOCKS proxy and selected loopback HTTP/TLS origin, with owned sockets.
use serde_json::{Value, json};
use std::{
    io,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Forward,
    BrowserForward,
    ForceAuth,
    HoldReply,
    StallGreeting,
    RefuseAuth,
    RefuseConnect,
}
#[derive(Clone, Default)]
struct Records {
    connections: Arc<Mutex<Vec<Value>>>,
    accepted: Arc<AtomicUsize>,
    closed: Arc<AtomicUsize>,
    permit: Arc<tokio::sync::Notify>,
    protocol_errors: Arc<Mutex<Vec<u8>>>,
    protocol_prefixes: Arc<Mutex<Vec<Vec<u8>>>>,
    refused: Arc<AtomicUsize>,
}
pub struct Site {
    pub port: u16,
    pub proxy: String,
    origin: Arc<Mutex<Vec<Value>>>,
    prefixes: Arc<Mutex<Vec<u8>>>,
    records: Records,
    mode: Mode,
    tasks: Vec<tokio::task::JoinHandle<()>>,
}
impl Drop for Site {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}
impl Site {
    pub async fn start(mode: Mode, tls: bool) -> Self {
        let origin_listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let port = origin_listener.local_addr().unwrap().port();
        let origin = Arc::<Mutex<Vec<Value>>>::default();
        let prefixes = Arc::<Mutex<Vec<u8>>>::default();
        let captured_origin = origin.clone();
        let captured_prefixes = prefixes.clone();
        let origin_task = tokio::spawn(async move {
            let acceptor = if tls { Some(tls_acceptor()) } else { None };
            let mut tasks = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    accepted=origin_listener.accept(),if tasks.len()<64=>{
                        let (stream,_)=accepted.unwrap();let records=captured_origin.clone();let prefixes=captured_prefixes.clone();let acceptor=acceptor.clone();
                        tasks.spawn(async move {
                            let mut byte=[0;1];if stream.peek(&mut byte).await.is_ok_and(|n|n>0){prefixes.lock().unwrap().push(byte[0]);}
                            if let Some(acceptor)=acceptor {if let Ok(stream)=acceptor.accept(stream).await {respond(stream,port,records).await;}}
                            else {respond(stream,port,records).await;}
                        });
                    },
                    Some(result)=tasks.join_next(),if !tasks.is_empty()=>{result.unwrap();},
                }
            }
        });
        let records = Records::default();
        let (proxy, proxy_task) = start_proxy(port, mode, records.clone()).await;
        Self {
            port,
            proxy,
            origin,
            prefixes,
            records,
            mode,
            tasks: vec![origin_task, proxy_task],
        }
    }
    /// The caller keeps this site's selected origin alive through the fork's lifetime.
    pub async fn fork_proxy(&self) -> Self {
        let records = Records::default();
        let (proxy, task) = start_proxy(self.port, self.mode, records.clone()).await;
        Self {
            port: self.port,
            proxy,
            origin: self.origin.clone(),
            prefixes: self.prefixes.clone(),
            records,
            mode: self.mode,
            tasks: vec![task],
        }
    }
    pub fn server(&self, scheme: &str, userinfo: &str) -> String {
        format!(
            "{scheme}://{userinfo}{}",
            self.proxy.strip_prefix("http://").unwrap()
        )
    }
    pub fn url(&self, scheme: &str, host: &str, path: &str) -> String {
        format!("{scheme}://{host}:{}{path}", self.port)
    }
    pub fn facts(&self) -> Value {
        json!({"connections":self.records.connections.lock().unwrap().clone(),"origin":self.origin.lock().unwrap().clone()})
    }
    pub fn prefixes(&self) -> Vec<u8> {
        self.prefixes.lock().unwrap().clone()
    }
    pub fn protocol_errors(&self) -> Vec<u8> {
        self.records.protocol_errors.lock().unwrap().clone()
    }
    pub fn protocol_prefixes(&self) -> Vec<Vec<u8>> {
        self.records.protocol_prefixes.lock().unwrap().clone()
    }
    pub fn refused(&self) -> usize {
        self.records.refused.load(Ordering::SeqCst)
    }
    pub async fn handshake(&self) {
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while self.records.connections.lock().unwrap().is_empty() {
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
    }
    pub fn grant(&self) {
        self.records.permit.notify_one();
    }
    pub async fn settled(&self, count: usize) -> Value {
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                let closed = self.records.closed.load(Ordering::SeqCst);
                let complete = if self.mode == Mode::BrowserForward {
                    closed == self.records.accepted.load(Ordering::SeqCst)
                        && self.records.connections.lock().unwrap().len() == count
                } else {
                    closed == count
                };
                if complete {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("Every owned proxy stream must settle");
        self.facts()
    }
}
async fn start_proxy(
    port: u16,
    mode: Mode,
    records: Records,
) -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let proxy = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        let mut tasks = tokio::task::JoinSet::new();
        loop {
            tokio::select! {
                accepted=listener.accept(),if tasks.len()<64=>{
                    let (stream,_)=accepted.unwrap();let observed=records.clone();
                    observed.accepted.fetch_add(1,Ordering::SeqCst);
                    tasks.spawn(async move {
                        let result=negotiate(stream,port,mode,observed.clone()).await;
                        observed.closed.fetch_add(1,Ordering::SeqCst);
                        match result {
                            Err(error) if mode==Mode::BrowserForward && matches!(error.kind(),io::ErrorKind::UnexpectedEof|io::ErrorKind::ConnectionReset|io::ErrorKind::ConnectionAborted|io::ErrorKind::BrokenPipe)=>{},
                            other=>other.unwrap(),
                        }
                    });
                },
                Some(result)=tasks.join_next(),if !tasks.is_empty()=>{result.unwrap();},
            }
        }
    });
    (proxy, task)
}
fn tls_acceptor() -> tokio_rustls::TlsAcceptor {
    use rustls::pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject};
    let certificate = CertificateDer::from_pem_slice(include_bytes!(
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
    .with_single_cert(vec![certificate], key)
    .unwrap();
    tokio_rustls::TlsAcceptor::from(Arc::new(config))
}
async fn respond<S: AsyncRead + AsyncWrite + Unpin>(
    mut stream: S,
    port: u16,
    records: Arc<Mutex<Vec<Value>>>,
) {
    let mut bytes = Vec::new();
    let mut buffer = [0; 1024];
    while bytes.len() <= 32768 && !bytes.windows(4).any(|part| part == b"\r\n\r\n") {
        let Ok(n) = stream.read(&mut buffer).await else {
            return;
        };
        if n == 0 {
            return;
        }
        bytes.extend_from_slice(&buffer[..n]);
    }
    let end = bytes
        .windows(4)
        .position(|part| part == b"\r\n\r\n")
        .unwrap()
        + 4;
    assert!(end <= 32768);
    let head = std::str::from_utf8(&bytes[..end]).unwrap();
    let path = head.split_whitespace().nth(1).unwrap();
    let header = |name: &str| {
        head.lines()
            .skip(1)
            .filter_map(|line| line.split_once(':'))
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.trim())
            .unwrap_or("")
    };
    records.lock().unwrap().push(json!({"path":path,"host":header("host").replace(&port.to_string(),"PORT"),"authorization":header("authorization"),"proxyAuthorization":header("proxy-authorization"),"custom":header("x-acme")}));
    let reply = if matches!(path, "/forward" | "/reverse") {
        format!(
            "HTTP/1.1 302 Acme redirect\r\nLocation: http://{}:{port}/echo\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            if path == "/forward" {
                "127.0.0.1"
            } else {
                "localhost"
            }
        )
    } else if path == "/Acme-page" {
        let body = "<!doctype html><link rel=\"icon\" href=\"data:,\"><title>Acme SOCKS</title><p>Acme</p>";
        format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: text/html\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
    } else {
        "HTTP/1.1 200 OK\r\nContent-Length: 4\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\nAcme".into()
    };
    let _ = stream.write_all(reply.as_bytes()).await;
}
async fn zero(stream: &mut tokio::net::TcpStream) -> io::Result<String> {
    let mut bytes = Vec::new();
    loop {
        let byte = stream.read_u8().await?;
        if byte == 0 {
            return Ok(String::from_utf8(bytes).unwrap());
        }
        assert!(bytes.len() < 1024);
        bytes.push(byte);
    }
}
async fn negotiate(
    mut stream: tokio::net::TcpStream,
    port: u16,
    mode: Mode,
    records: Records,
) -> io::Result<()> {
    let version = stream.read_u8().await?;
    if mode == Mode::BrowserForward && !matches!(version, 4 | 5) {
        let mut remaining = [0; 3];
        stream.read_exact(&mut remaining).await?;
        let mut prefix = vec![version];
        prefix.extend_from_slice(&remaining);
        let mut errors = records.protocol_errors.lock().unwrap();
        assert!(errors.len() < 64);
        errors.push(version);
        records.protocol_prefixes.lock().unwrap().push(prefix);
        return Ok(());
    }
    let mut record = json!({"version":version});
    let (host, destination_port) = if version == 4 {
        negotiate_v4(&mut stream, &mut record).await?
    } else {
        assert_eq!(version, 5);
        let Some(destination) = negotiate_v5(&mut stream, mode, &records, &mut record).await?
        else {
            return Ok(());
        };
        destination
    };
    let selected = destination_port == port
        && (matches!(host.as_str(), "localhost" | "127.0.0.1" | "::1")
            || mode == Mode::BrowserForward
                && matches!(
                    host.as_str(),
                    "acme.invalid" | "::ffff:127.0.0.1" | "::ffff:7f00:1"
                ));
    if mode == Mode::BrowserForward && !selected {
        records.refused.fetch_add(1, Ordering::SeqCst);
        stream
            .write_all(if version == 4 {
                &[0, 91, 0, 0, 0, 0, 0, 0]
            } else {
                &[5, 2, 0, 1, 0, 0, 0, 0, 0, 0]
            })
            .await?;
        return Ok(());
    }
    assert!(
        selected,
        "The strict fixture must never forward an unselected destination"
    );
    record["destination"]["port"] = json!("PORT");
    if mode == Mode::RefuseConnect {
        record["rejection"] = json!("connect");
    }
    let index = {
        let mut connections = records.connections.lock().unwrap();
        let index = connections.len();
        if mode == Mode::BrowserForward {
            record["applicationData"] = json!(false);
        }
        connections.push(record);
        index
    };
    if mode == Mode::HoldReply {
        records.permit.notified().await;
        let mut byte = [0; 1];
        match stream.try_read(&mut byte) {
            Ok(0) => return Ok(()),
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::ConnectionReset | io::ErrorKind::ConnectionAborted
                ) =>
            {
                return Ok(());
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
            other => panic!("No origin bytes may precede the held reply: {other:?}"),
        }
    }
    if mode == Mode::RefuseConnect {
        stream
            .write_all(if version == 4 {
                &[0, 91, 0, 0, 0, 0, 0, 0]
            } else {
                &[5, 5, 0, 1, 0, 0, 0, 0, 0, 0]
            })
            .await?;
        return Ok(());
    }
    relay_selected(&mut stream, port, mode, version, &records, index).await
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

async fn negotiate_v4(
    stream: &mut tokio::net::TcpStream,
    record: &mut Value,
) -> io::Result<(String, u16)> {
    let host;
    let mut head = [0; 7];
    stream.read_exact(&mut head).await?;
    assert_eq!(head[0], 1);
    let destination_port = u16::from_be_bytes([head[1], head[2]]);
    record["userId"] = json!(zero(stream).await?);
    if head[3..6] == [0, 0, 0] && head[6] != 0 {
        host = zero(stream).await?;
        record["destination"] = json!({"type":"domain","host":host});
    } else {
        let ip = std::net::Ipv4Addr::from([head[3], head[4], head[5], head[6]]);
        host = ip.to_string();
        record["destination"] = json!({"type":"ipv4","addressHex":hex(&head[3..])});
    }
    Ok((host, destination_port))
}
async fn negotiate_v5(
    stream: &mut tokio::net::TcpStream,
    mode: Mode,
    records: &Records,
    record: &mut Value,
) -> io::Result<Option<(String, u16)>> {
    let host;
    let count = stream.read_u8().await? as usize;
    let mut methods = vec![0; count];
    stream.read_exact(&mut methods).await?;
    record["methods"] = json!(methods);
    record["authentication"] = Value::Null;
    if mode == Mode::StallGreeting {
        records
            .connections
            .lock()
            .unwrap()
            .push(std::mem::take(record));
        let mut byte = [0; 1];
        assert!(matches!(stream.read(&mut byte).await, Ok(0) | Err(_)));
        return Ok(None);
    }
    let authentication =
        mode == Mode::ForceAuth || mode == Mode::RefuseAuth || methods.contains(&2);
    stream
        .write_all(&[5, if authentication { 2 } else { 0 }])
        .await?;
    if authentication {
        assert_eq!(stream.read_u8().await?, 1);
        let count = stream.read_u8().await? as usize;
        let mut user = vec![0; count];
        stream.read_exact(&mut user).await?;
        let count = stream.read_u8().await? as usize;
        let mut password = vec![0; count];
        stream.read_exact(&mut password).await?;
        record["authentication"] = json!({"userId":String::from_utf8(user).unwrap(),"password":String::from_utf8(password).unwrap()});
        stream
            .write_all(&[1, if mode == Mode::RefuseAuth { 1 } else { 0 }])
            .await?;
        if mode == Mode::RefuseAuth {
            record["rejection"] = json!("authentication");
            records
                .connections
                .lock()
                .unwrap()
                .push(std::mem::take(record));
            return Ok(None);
        }
    }
    let mut head = [0; 4];
    stream.read_exact(&mut head).await?;
    assert_eq!(&head[..3], &[5, 1, 0]);
    match head[3] {
        1 => {
            let mut address = [0; 4];
            stream.read_exact(&mut address).await?;
            host = std::net::Ipv4Addr::from(address).to_string();
            record["destination"] = json!({"type":"ipv4","addressHex":hex(&address)});
        }
        3 => {
            let n = stream.read_u8().await? as usize;
            let mut address = vec![0; n];
            stream.read_exact(&mut address).await?;
            host = String::from_utf8(address).unwrap();
            record["destination"] = json!({"type":"domain","host":host});
        }
        4 => {
            let mut address = [0; 16];
            stream.read_exact(&mut address).await?;
            host = std::net::Ipv6Addr::from(address).to_string();
            record["destination"] = json!({"type":"ipv6","addressHex":hex(&address)});
        }
        _ => panic!("Unexpected owned address type"),
    }
    let destination_port = stream.read_u16().await?;
    Ok(Some((host, destination_port)))
}

async fn relay_selected(
    stream: &mut tokio::net::TcpStream,
    port: u16,
    mode: Mode,
    version: u8,
    records: &Records,
    index: usize,
) -> io::Result<()> {
    let mut upstream =
        tokio::net::TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port)).await?;
    let sent = stream
        .write_all(if version == 4 {
            &[0, 90, 0, 0, 127, 0, 0, 1]
        } else {
            &[5, 0, 0, 1, 127, 0, 0, 1, 0, 0]
        })
        .await;
    if let Err(error) = sent {
        if mode == Mode::HoldReply
            && matches!(
                error.kind(),
                io::ErrorKind::BrokenPipe
                    | io::ErrorKind::ConnectionReset
                    | io::ErrorKind::ConnectionAborted
            )
        {
            return Ok(());
        }
        return Err(error);
    }
    if mode == Mode::BrowserForward {
        let mut first = [0; 1];
        if stream.peek(&mut first).await? > 0 {
            records.connections.lock().unwrap()[index]["applicationData"] = json!(true);
        }
    }
    let _ = tokio::io::copy_bidirectional(stream, &mut upstream).await;
    Ok(())
}
