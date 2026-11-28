//! Actual sockets witness identity selection, upstream trust and joined teardown.
use super::*;
use crate::http_fixtures::certificates as fixture;
use rustls::pki_types::{CertificateDer, pem::PemObject};
use serde_json::json;

fn deadline() -> Instant {
    Instant::now() + std::time::Duration::from_secs(5)
}
async fn socks(tunnel: &Tunnel, host: &str, port: u16) -> TcpStream {
    let mut stream = TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, tunnel.port))
        .await
        .unwrap();
    stream.write_all(&[5, 1, 0]).await.unwrap();
    let mut method = [0; 2];
    stream.read_exact(&mut method).await.unwrap();
    assert_eq!(method, [5, 0]);
    let mut request = vec![5, 1, 0, 3, host.len().try_into().unwrap()];
    request.extend_from_slice(host.as_bytes());
    request.extend(port.to_be_bytes());
    // A byte at a time also verifies fragmented negotiation.
    for byte in request {
        stream.write_all(&[byte]).await.unwrap();
    }
    let mut connected = [0; 10];
    stream.read_exact(&mut connected).await.unwrap();
    assert_eq!(connected, [5, 0, 0, 1, 127, 0, 0, 1, 0, 0]);
    stream
}
fn roots() -> rustls::RootCertStore {
    let mut roots = rustls::RootCertStore::empty();
    roots
        .add(CertificateDer::from_pem_slice(include_bytes!("../http/fixtures/ca.pem")).unwrap())
        .unwrap();
    roots
}
async fn fetch(tunnel: &Tunnel, url: &str) -> String {
    let url = url::Url::parse(url).unwrap();
    let stream = socks(tunnel, "localhost", url.port().unwrap()).await;
    let mut trust = roots();
    trust.add(tunnel.browser_certificate.clone()).unwrap();
    let mut config = rustls::ClientConfig::builder()
        .with_root_certificates(trust)
        .with_no_client_auth();
    config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    let mut stream = tokio_rustls::TlsConnector::from(Arc::new(config))
        .connect(tls::name("localhost").unwrap(), stream)
        .await
        .unwrap();
    stream
        .write_all(b"GET /identity HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut head = Vec::new();
    while !head.ends_with(b"\r\n\r\n") {
        head.push(stream.read_u8().await.unwrap());
        assert!(head.len() < 4096);
    }
    let head = String::from_utf8(head).unwrap();
    let length: usize = head
        .lines()
        .find_map(|line| line.strip_prefix("Content-Length: "))
        .unwrap()
        .parse()
        .unwrap();
    let mut body = vec![0; length];
    stream.read_exact(&mut body).await.unwrap();
    format!("{head}{}", String::from_utf8(body).unwrap())
}
async fn identities(keys: &fixture::Identities, inputs: serde_json::Value) -> Vec<Prepared> {
    let _ = keys;
    let mut options: super::super::http::Options =
        serde_json::from_value(json!({"clientCertificates":inputs})).unwrap();
    options = super::super::http::client_certificates::prepare_options(options, deadline())
        .await
        .unwrap();
    super::super::http::client_certificates::take_prepared(&mut options).unwrap()
}
#[tokio::test]
async fn certificate_tunnel_plaintext_is_exact_and_close_joins_stalled_sockets() {
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let port = listener.local_addr().unwrap().port();
    let mut tunnel = Tunnel::start(vec![], None, deadline()).await.unwrap();
    let mut browser = socks(&tunnel, "127.0.0.1", port).await;
    let (mut upstream, _) = listener.accept().await.unwrap();
    browser.write_all(&[0, 255, 128, 65]).await.unwrap();
    let mut bytes = [0; 4];
    upstream.read_exact(&mut bytes).await.unwrap();
    assert_eq!(bytes, [0, 255, 128, 65]);
    upstream.write_all(b"Acme reply").await.unwrap();
    let mut reply = [0; 10];
    browser.read_exact(&mut reply).await.unwrap();
    assert_eq!(&reply, b"Acme reply");
    let mut negotiating = TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, tunnel.port))
        .await
        .unwrap();
    negotiating.write_all(&[5]).await.unwrap();
    tunnel.close(deadline()).await.unwrap();
    tunnel.close(deadline()).await.unwrap();
    assert_eq!(browser.read(&mut bytes).await.unwrap(), 0);
    assert_eq!(upstream.read(&mut bytes).await.unwrap(), 0);
    match negotiating.read(&mut bytes).await {
        Ok(0) => {}
        Err(error) => assert_eq!(error.kind(), io::ErrorKind::ConnectionReset),
        result => panic!("Stalled socket remains readable after joined close: {result:?}"),
    }
    assert!(
        TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, tunnel.port))
            .await
            .is_err()
    );
}
#[tokio::test]
async fn certificate_tunnel_distinct_origins_and_duplicate_formats_select_exact_identities() {
    let keys = fixture::Identities::new().await;
    let a = fixture::Site::start(&keys).await;
    let b = fixture::Site::start(&keys).await;
    // Same key type listed PFX-first: pinned grouping keeps PFX after PEM.
    let c = fixture::Site::start(&keys).await;
    let fixture_config = rustls::ClientConfig::builder()
        .with_root_certificates(roots())
        .with_no_client_auth();
    let direct = TcpStream::connect((
        "localhost",
        url::Url::parse(&a.url).unwrap().port().unwrap(),
    ))
    .await
    .unwrap();
    let fixture_peer = tokio_rustls::TlsConnector::from(Arc::new(fixture_config))
        .connect(tls::name("localhost").unwrap(), direct)
        .await
        .expect("Independent fixture trust and hostname validation");
    drop(fixture_peer);
    let inputs = json!([
        keys.input(&a.url, 0),
        keys.input(&a.url, 1),
        keys.encrypted_input(&a.url, 0, true),
        keys.input(&b.url, 1),
        keys.encrypted_input(&c.url, 0, true),
        keys.input(&c.url, 1)
    ]);
    let mut tunnel = Tunnel::with_roots(identities(&keys, inputs).await, None, roots(), deadline())
        .await
        .unwrap();
    let first = fetch(&tunnel, &a.url).await;
    assert!(first.ends_with("\r\n\r\nA"), "{first}");
    let second = fetch(&tunnel, &b.url).await;
    assert!(second.ends_with("\r\n\r\nB"), "{second}");
    let third = fetch(&tunnel, &c.url).await;
    assert!(third.ends_with("\r\n\r\nA"), "{third}");
    assert_eq!(a.requests.lock().unwrap().len(), 1);
    assert_eq!(b.requests.lock().unwrap().len(), 1);
    assert_eq!(format!("{tunnel:?}"), "PrivateClientCertificateTunnel");
    tunnel.close(deadline()).await.unwrap();
}
#[tokio::test]
async fn certificate_tunnel_default_roots_refuse_private_upstream_without_http_or_anonymous_retry()
{
    let keys = fixture::Identities::new().await;
    let site = fixture::Site::start(&keys).await;
    let mut tunnel = Tunnel::start(
        identities(&keys, json!([keys.input(&site.url, 0)])).await,
        None,
        deadline(),
    )
    .await
    .unwrap();
    let response = fetch(&tunnel, &site.url).await;
    assert!(
        response.starts_with("HTTP/1.1 503 Internal Server Error\r\n"),
        "{response}"
    );
    assert!(site.requests.lock().unwrap().is_empty());
    tunnel.close(deadline()).await.unwrap();
}

fn peer_config(keys: &fixture::Identities, ask_identity: bool) -> rustls::ServerConfig {
    use rustls::pki_types::PrivateKeyDer;
    let builder = rustls::ServerConfig::builder();
    let builder = if ask_identity {
        let mut clients = rustls::RootCertStore::empty();
        for pem in &keys.certs {
            clients
                .add(CertificateDer::from_pem_slice(pem.as_bytes()).unwrap())
                .unwrap();
        }
        builder.with_client_cert_verifier(
            rustls::server::WebPkiClientVerifier::builder(Arc::new(clients))
                .allow_unauthenticated()
                .build()
                .unwrap(),
        )
    } else {
        builder.with_no_client_auth()
    };
    builder
        .with_single_cert(
            vec![
                CertificateDer::from_pem_slice(include_bytes!("../http/fixtures/server.pem"))
                    .unwrap(),
            ],
            PrivateKeyDer::from_pem_slice(include_bytes!("../http/fixtures/server-key.pem"))
                .unwrap(),
        )
        .unwrap()
}

#[tokio::test]
async fn certificate_tunnel_upstream_proxies_isolate_identity_and_connect_credentials() {
    let keys = fixture::Identities::new().await;
    for scheme in ["http", "https", "socks4", "socks5"] {
        let a = fixture::Site::start(&keys).await;
        let b = fixture::Site::start(&keys).await;
        let a_port = url::Url::parse(&a.url).unwrap().port().unwrap();
        let b_port = url::Url::parse(&b.url).unwrap().port().unwrap();
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let proxy_port = listener.local_addr().unwrap().port();
        let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(peer_config(&keys, true)));
        let is_https = scheme == "https";
        let is_http = matches!(scheme, "http" | "https");
        let task = tokio::spawn(async move {
            let mut connections = JoinSet::new();
            for destination_port in [a_port, b_port] {
                let (stream, _) = listener.accept().await.unwrap();
                let acceptor = acceptor.clone();
                connections.spawn(async move {
                    let mut socket: Socket = if is_https {
                        let stream = acceptor.accept(stream).await.unwrap();
                        assert!(
                            stream
                                .get_ref()
                                .1
                                .peer_certificates()
                                .is_none_or(|certs| certs.is_empty()),
                            "Destination identity reached HTTPS proxy"
                        );
                        Box::new(stream)
                    } else {
                        Box::new(stream)
                    };
                    proxy_handshake(&mut socket, scheme, destination_port).await;
                    let mut endpoint = TcpStream::connect(("localhost", destination_port))
                        .await
                        .unwrap();
                    // The browser closes each completed response connection.
                    let _ = tokio::io::copy_bidirectional(&mut socket, &mut endpoint).await;
                });
            }
            while let Some(result) = connections.join_next().await {
                result.unwrap();
            }
        });
        let proxy = ContextProxy {
            server: format!("{scheme}://localhost:{proxy_port}"),
            bypass: None,
            username: is_http.then(|| "Acme-proxy".into()),
            password: is_http.then(|| "Acme-private".into()),
        };
        let mut tunnel = Tunnel::with_roots(
            identities(&keys, json!([keys.input(&a.url, 0)])).await,
            Some(proxy),
            roots(),
            deadline(),
        )
        .await
        .unwrap();
        assert!(
            fetch(&tunnel, &a.url).await.ends_with("\r\n\r\nA"),
            "{scheme}"
        );
        // The other origin is a byte relay, without the configured identity.
        assert!(
            fetch(&tunnel, &b.url).await.ends_with("\r\n\r\nnone"),
            "{scheme}"
        );
        assert_eq!(a.requests.lock().unwrap()[0]["identity"], "A");
        assert_eq!(b.requests.lock().unwrap()[0]["identity"], "none");
        tunnel.close(deadline()).await.unwrap();
        tokio::time::timeout_at(deadline(), task)
            .await
            .unwrap()
            .unwrap();
    }
}

#[tokio::test]
async fn certificate_tunnel_alpn_and_non_http_bytes_survive_the_verified_tls_relay() {
    let keys = fixture::Identities::new().await;
    let mut server = peer_config(&keys, false);
    server.alpn_protocols = vec![b"h2".to_vec()];
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let port = listener.local_addr().unwrap().port();
    let endpoint = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut stream = tokio_rustls::TlsAcceptor::from(Arc::new(server))
            .accept(stream)
            .await
            .unwrap();
        assert_eq!(stream.get_ref().1.alpn_protocol(), Some(b"h2".as_slice()));
        let mut bytes = [0; 4];
        stream.read_exact(&mut bytes).await.unwrap();
        assert_eq!(bytes, [0, 255, 128, 65]);
        stream.write_all(&[255, 0, 1]).await.unwrap();
        stream.flush().await.unwrap();
    });
    let origin = format!("https://localhost:{port}");
    let mut tunnel = Tunnel::with_roots(
        identities(&keys, json!([keys.input(&origin, 0)])).await,
        None,
        roots(),
        deadline(),
    )
    .await
    .unwrap();
    let raw = socks(&tunnel, "localhost", port).await;
    let mut trust = roots();
    trust.add(tunnel.browser_certificate.clone()).unwrap();
    let mut client = rustls::ClientConfig::builder()
        .with_root_certificates(trust)
        .with_no_client_auth();
    client.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    let mut browser = tokio_rustls::TlsConnector::from(Arc::new(client))
        .connect(tls::name("localhost").unwrap(), raw)
        .await
        .unwrap();
    assert_eq!(browser.get_ref().1.alpn_protocol(), Some(b"h2".as_slice()));
    browser.write_all(&[0, 255, 128, 65]).await.unwrap();
    let mut bytes = [0; 3];
    browser.read_exact(&mut bytes).await.unwrap();
    assert_eq!(bytes, [255, 0, 1]);
    endpoint.await.unwrap();
    tunnel.close(deadline()).await.unwrap();
}

async fn proxy_handshake(socket: &mut Socket, scheme: &str, destination_port: u16) {
    use base64::Engine as _;
    if matches!(scheme, "http" | "https") {
        let mut head = Vec::new();
        while !head.ends_with(b"\r\n\r\n") {
            head.push(socket.read_u8().await.unwrap());
            assert!(head.len() < 4096);
        }
        let head = String::from_utf8(head).unwrap();
        assert!(
            head.starts_with(&format!(
                "CONNECT localhost:{destination_port} HTTP/1.1\r\n"
            )),
            "{head}"
        );
        let token = base64::engine::general_purpose::STANDARD.encode("Acme-proxy:Acme-private");
        assert!(head.contains(&format!("Proxy-Authorization: Basic {token}\r\n")));
        socket
            .write_all(b"HTTP/1.1 200 Connection established\r\nX-Acme: \xff\r\n\r\n")
            .await
            .unwrap();
    } else if scheme == "socks5" {
        let mut greeting = [0; 3];
        socket.read_exact(&mut greeting).await.unwrap();
        assert_eq!(greeting, [5, 1, 0]);
        socket.write_all(&[5, 0]).await.unwrap();
        let mut prefix = [0; 5];
        socket.read_exact(&mut prefix).await.unwrap();
        assert_eq!(prefix, [5, 1, 0, 3, 9]);
        let mut name = [0; 9];
        socket.read_exact(&mut name).await.unwrap();
        assert_eq!(&name, b"localhost");
        assert_eq!(socket.read_u16().await.unwrap(), destination_port);
        socket
            .write_all(&[5, 0, 0, 1, 127, 0, 0, 1, 0, 0])
            .await
            .unwrap();
    } else {
        let mut prefix = [0; 8];
        socket.read_exact(&mut prefix).await.unwrap();
        let [hi, lo] = destination_port.to_be_bytes();
        assert_eq!(prefix, [4, 1, hi, lo, 0, 0, 0, 1]);
        assert_eq!(socket.read_u8().await.unwrap(), 0);
        let mut name = [0; 10];
        socket.read_exact(&mut name).await.unwrap();
        assert_eq!(&name, b"localhost\0");
        socket.write_all(&[0, 90, 0, 0, 0, 0, 0, 0]).await.unwrap();
    }
}
