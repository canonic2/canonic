//! A trusted test proxy must not satisfy the destination's independent TLS layer.
use super::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[tokio::test]
async fn http_proxy_refusal_successful_tunnel_never_captures_later_origin_error() {
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        for expected in ["CONNECT 127.0.0.1:9 HTTP/1.1\r\n", "GET /Acme HTTP/1.1\r\n"] {
            let mut head = Vec::new();
            while !head.ends_with(b"\r\n\r\n") {
                assert!(head.len() < 16384);
                head.push(stream.read_u8().await.unwrap());
            }
            assert!(head.starts_with(expected.as_bytes()));
            let response = if expected.starts_with("CONNECT") {
                b"HTTP/1.1 200 Acme tunnel\r\n\r\n".as_slice()
            } else {
                b"HTTP/1.1 403 Acme origin refusal\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            };
            stream.write_all(response).await.unwrap();
        }
    });
    tokio::task::spawn_blocking(move || {
        let observer = Observer::default();
        observer.start(None);
        let config = ureq::Agent::config_builder()
            .proxy(Some(
                ureq::Proxy::new(&format!("http://127.0.0.1:{port}")).unwrap(),
            ))
            .timeout_global(Some(std::time::Duration::from_secs(5)))
            .http_status_as_error(false)
            .build();
        let response = agent(config, observer.clone(), None, None, None)
            .get("http://127.0.0.1:9/Acme")
            .call()
            .unwrap();
        assert_eq!(response.status().as_u16(), 403);
        assert!(
            observer.0.refusal.take().is_none(),
            "Origin response bytes must not become CONNECT refusal evidence"
        );
        assert_eq!(observer.take(&response).unwrap().status, 403);
    })
    .await
    .unwrap();
    server.await.unwrap();
}

#[tokio::test]
async fn http_proxy_https_tunnel_starts_independent_destination_tls() {
    use rustls::pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject};
    use tokio_rustls::{TlsAcceptor, rustls};
    let cert = CertificateDer::from_pem_slice(include_bytes!("../fixtures/server.pem")).unwrap();
    let key = PrivateKeyDer::from_pem_slice(include_bytes!("../fixtures/server-key.pem")).unwrap();
    let config = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_no_client_auth()
    .with_single_cert(vec![cert], key)
    .unwrap();
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut stream = TlsAcceptor::from(Arc::new(config))
            .accept(stream)
            .await
            .unwrap();
        let mut head = Vec::new();
        while !head.ends_with(b"\r\n\r\n") {
            assert!(head.len() < 16384);
            head.push(stream.read_u8().await.unwrap());
        }
        assert!(head.starts_with(b"CONNECT 127.0.0.1:9 HTTP/1.1\r\n"));
        stream
            .write_all(b"HTTP/1.1 200 Acme tunnel\r\n\r\n")
            .await
            .unwrap();
        let mut prefix = [0; 5];
        stream.read_exact(&mut prefix).await.unwrap();
        if prefix.starts_with(b"GET ") {
            stream.write_all(b"HTTP/1.1 200 Acme plaintext leak\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await.unwrap();
        }
        prefix
    });
    let result = tokio::task::spawn_blocking(move || {
        let config = ureq::Agent::config_builder()
            .proxy(Some(
                ureq::Proxy::new(&format!("https://localhost:{port}")).unwrap(),
            ))
            .timeout_global(Some(std::time::Duration::from_secs(5)))
            .http_status_as_error(false)
            .build();
        // Only the test proxy trusts this public fixture CA. Destination roots
        // remain unchanged; production never accepts a caller-controlled CA.
        let proxy_tls = ureq::Agent::config_builder()
            .proxy(None)
            .tls_config(
                ureq::tls::TlsConfig::builder()
                    .root_certs(ureq::tls::RootCerts::Specific(Arc::new(vec![
                        ureq::tls::Certificate::from_pem(include_bytes!("../fixtures/ca.pem"))
                            .unwrap(),
                    ])))
                    .build(),
            )
            .build();
        let observer = Observer::default();
        let transport = observer.0.transport.clone();
        let agent = ureq::Agent::with_parts(
            config,
            WireConnector {
                inner: connection::ConnectionConnector::new(transport.clone()),
                observer,
                depth: AtomicUsize::new(0),
                authorization: None,
                refusal: Default::default(),
                socks: None,
                certificates: None,
                proxy_tls,
            },
            connection::ObservedResolver::new(transport),
        );
        agent
            .get("https://127.0.0.1:9/Acme-private")
            .call()
            .map(|response| response.status().as_u16())
    })
    .await
    .unwrap();
    let prefix = tokio::time::timeout(std::time::Duration::from_secs(5), server)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        prefix[0], 22,
        "Destination must start TLS, never send plaintext HTTPS: {prefix:?}"
    );
    assert_eq!(prefix[1], 3);
    assert!(
        result.is_err(),
        "The fixture closes during the independent destination handshake"
    );
}
