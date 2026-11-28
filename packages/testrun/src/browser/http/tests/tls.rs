//! Opposite TLS policies must never reuse the same keep-alive connection.
use super::*;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject};
use tokio_rustls::{TlsAcceptor, rustls};

struct Site {
    url: String,
    requests: Arc<AtomicUsize>,
    connections: Arc<AtomicUsize>,
    task: JoinHandle<()>,
}
impl Drop for Site {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Site {
    async fn start() -> Self {
        let cert =
            CertificateDer::from_pem_slice(include_bytes!("../fixtures/server.pem")).unwrap();
        let key =
            PrivateKeyDer::from_pem_slice(include_bytes!("../fixtures/server-key.pem")).unwrap();
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
        let requests = Arc::new(AtomicUsize::new(0));
        let connections = Arc::new(AtomicUsize::new(0));
        let accepted_connections = connections.clone();
        let count = requests.clone();
        let task = tokio::spawn(async move {
            let mut calls = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    accepted=listener.accept()=>{
                        accepted_connections.fetch_add(1,Ordering::SeqCst);
                        let (stream,_)=accepted.unwrap();let acceptor=acceptor.clone();let count=count.clone();
                        calls.spawn(async move {
                            let Ok(mut stream)=acceptor.accept(stream).await else{return;};
                            loop {
                                let mut bytes=Vec::new();let mut chunk=[0;1024];
                                while !bytes.windows(4).any(|part|part==b"\r\n\r\n") {
                                    let Ok(n)=stream.read(&mut chunk).await else{return;};
                                    if n==0||bytes.len()+n>16384{return;}
                                    bytes.extend_from_slice(&chunk[..n]);
                                }
                                count.fetch_add(1,Ordering::SeqCst);
                                if stream.write_all(b"HTTP/1.1 200 Acme TLS phrase\r\nX-TLS: one\r\nx-tls: two\r\nContent-Length: 4\r\nConnection: keep-alive\r\n\r\nAcme").await.is_err(){return;}
                            }
                        });
                    },
                    _=calls.join_next(),if !calls.is_empty()=>{}
                }
            }
        });
        Self {
            url,
            requests,
            connections,
            task,
        }
    }
}

#[tokio::test]
async fn http_connection_details_keep_tls_peer_and_reuse_without_repeating_connection_phases() {
    let mut site = Site::start().await;
    let mut contexts = Contexts::default();
    let context: Reference = serde_json::from_value(
        contexts
            .create(
                Uuid::new_v4(),
                Options {
                    base_url: Some(site.url.clone()),
                    ignore_https_errors: Some(true),
                    ..Default::default()
                },
            )
            .unwrap()["httpContext"]
            .clone(),
    )
    .unwrap();
    let mut retained: Vec<(Uuid, Value)> = Vec::new();
    for index in 0..2 {
        let id = contexts
            .start(context, fetch("/"), Instant::now() + Duration::from_secs(5))
            .unwrap();
        let Ready::Reply(reply) = ready(&mut contexts, id).await.unwrap() else {
            panic!("Actual TLS reply");
        };
        assert_eq!(reply.body.as_deref(), Some(b"Acme".as_slice()));
        let details = contexts.response_read(id, 65536).unwrap();
        let origin = Url::parse(&site.url).unwrap();
        assert_eq!(
            details["serverAddr"],
            json!({"ipAddress":"127.0.0.1","port":origin.port().unwrap()})
        );
        assert_eq!(details["securityDetails"]["protocol"], "TLSv1.3");
        assert!(
            details["securityDetails"]["subjectName"]
                .as_str()
                .is_some_and(|value| !value.is_empty())
        );
        let timing = &details["timing"];
        assert!(timing["startTime"].as_f64().unwrap() > 1_000_000_000_000.0);
        assert!(timing["requestStart"].as_f64().unwrap() >= 0.0);
        assert!(
            timing["responseStart"].as_f64().unwrap() >= timing["requestStart"].as_f64().unwrap()
        );
        assert!(
            timing["responseEnd"].as_f64().unwrap() >= timing["responseStart"].as_f64().unwrap()
        );
        if index == 0 {
            assert_eq!(timing["domainLookupStart"], 0.0);
            assert!(
                timing["connectStart"].as_f64().unwrap()
                    >= timing["domainLookupEnd"].as_f64().unwrap()
            );
            assert!(
                timing["secureConnectionStart"].as_f64().unwrap()
                    >= timing["connectStart"].as_f64().unwrap()
            );
            assert!(
                timing["connectEnd"].as_f64().unwrap()
                    >= timing["secureConnectionStart"].as_f64().unwrap()
            );
        } else {
            for field in [
                "domainLookupStart",
                "domainLookupEnd",
                "connectStart",
                "secureConnectionStart",
                "connectEnd",
            ] {
                assert_eq!(
                    timing[field], -1.0,
                    "A reused socket has no new {field} event"
                );
            }
            assert_eq!(details["securityDetails"], retained[0].1["securityDetails"]);
        }
        contexts.complete(id, reply.metadata, None).unwrap();
        retained.push((id, details));
    }
    assert_eq!(site.connections.load(Ordering::SeqCst), 1);
    assert_eq!(site.requests.load(Ordering::SeqCst), 2);
    contexts.remove(context).unwrap();
    for (id, details) in retained {
        assert_eq!(contexts.response_read(id, 65536).unwrap(), details);
        assert_eq!(contexts.response_remove(id)["removed"], true);
        assert_eq!(
            contexts.response_read(id, 65536).unwrap_err().code,
            "UNKNOWN_HTTP_RESPONSE_DETAILS"
        );
    }
    contexts.close().unwrap();
    site.task.abort();
    let _ = (&mut site.task).await;
}
async fn request(
    contexts: &mut Contexts,
    context: Reference,
    ignore: Option<bool>,
) -> Result<Value, Error> {
    let id = contexts.start(
        context,
        Fetch {
            ignore_https_errors: ignore,
            ..fetch("/")
        },
        Instant::now() + Duration::from_secs(5),
    )?;
    match ready(contexts, id).await? {
        Ready::Reply(reply) => {
            assert_eq!(reply.body.as_deref(), Some(b"Acme".as_slice()));
            let details = contexts.response_read(id, 65536).unwrap();
            assert_eq!(details["statusText"], "Acme TLS phrase");
            assert_eq!(details["wireMetadataAvailable"], true);
            assert_eq!(details["headers"][0]["name"], "X-TLS");
            assert_eq!(details["headers"][1]["name"], "x-tls");
            let metadata = reply.metadata.clone();
            contexts.restore(id, reply);
            contexts.discard(id)?;
            Ok(metadata)
        }
        Ready::Complete(value) => panic!("Unexpected response witness: {value}"),
    }
}

#[tokio::test]
async fn wire_heads_are_observed_after_a_connect_tunnel_and_tls() {
    let site = Site::start().await;
    let origin = Url::parse(&site.url).unwrap();
    let origin_port = origin.port().unwrap();
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let proxy = format!("http://{}", listener.local_addr().unwrap());
    let tunnel = tokio::spawn(async move {
        let (mut client, _) = listener.accept().await.unwrap();
        let mut header = Vec::new();
        while !header.ends_with(b"\r\n\r\n") {
            header.push(client.read_u8().await.unwrap());
        }
        assert!(header.starts_with(b"CONNECT "));
        let mut upstream =
            tokio::net::TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, origin_port))
                .await
                .unwrap();
        client
            .write_all(b"HTTP/1.1 200 Acme tunnel only\r\n\r\n")
            .await
            .unwrap();
        tokio::io::copy_bidirectional(&mut client, &mut upstream)
            .await
            .unwrap();
    });
    let mut contexts = Contexts::default();
    let created = contexts
        .create(
            Uuid::new_v4(),
            Options {
                base_url: Some(site.url.clone()),
                proxy: Some(proxy.into()),
                ignore_https_errors: Some(true),
                ..Default::default()
            },
        )
        .unwrap();
    let context = serde_json::from_value(created["httpContext"].clone()).unwrap();
    request(&mut contexts, context, None).await.unwrap();
    assert_eq!(site.requests.load(Ordering::SeqCst), 1);
    contexts.remove(context).unwrap();
    tokio::time::timeout(Duration::from_secs(5), tunnel)
        .await
        .unwrap()
        .unwrap();
}
#[tokio::test]
async fn request_tls_overrides_isolate_pools_and_leave_both_context_defaults_unchanged() {
    let mut site = Site::start().await;
    let mut contexts = Contexts::default();
    let connection = Uuid::new_v4();
    for default in [true, false] {
        let created = contexts
            .create(
                connection,
                Options {
                    base_url: Some(site.url.clone()),
                    ignore_https_errors: Some(default),
                    ..Default::default()
                },
            )
            .unwrap();
        let context = serde_json::from_value(created["httpContext"].clone()).unwrap();
        if default {
            assert_eq!(
                request(&mut contexts, context, None).await.unwrap()["tlsVerification"],
                false
            );
        }
        let strict = request(&mut contexts, context, Some(false))
            .await
            .expect_err("The private test CA is not trusted");
        assert_eq!(strict.code, "HTTP_REQUEST_OUTCOME_UNKNOWN");
        assert_eq!(strict.partial.as_ref().unwrap()["failureKind"], "tls");
        let relaxed = request(&mut contexts, context, Some(true)).await.unwrap();
        assert_eq!(relaxed["tlsVerification"], false);
        assert_eq!(relaxed["tlsPolicyOverridden"], true);
        if default {
            assert!(request(&mut contexts, context, None).await.is_ok());
        } else {
            assert!(request(&mut contexts, context, None).await.is_err());
        }
    }
    assert_eq!(site.requests.load(Ordering::SeqCst), 4);
    contexts.close().unwrap();
    site.task.abort();
    let _ = (&mut site.task).await;
}

#[tokio::test]
async fn http_proxy_tls_identity_is_verified_before_credentials_under_every_endpoint_policy() {
    let proxy = super::proxy::tunnel::UntrustedTls::start().await;
    let mut contexts = Contexts::default();
    for default in [false, true] {
        let options:Options=serde_json::from_value(json!({"baseUrl":"http://127.0.0.1:9","ignoreHttpsErrors":default,"proxy":{"server":proxy.url,"username":"Acme-proxy-private","password":"Acme-proxy-secret"}})).unwrap();
        let context = serde_json::from_value(
            contexts
                .create(contexts.connection.unwrap_or_else(Uuid::new_v4), options)
                .unwrap()["httpContext"]
                .clone(),
        )
        .unwrap();
        for selected in [None, Some(false), Some(true)] {
            let error = request(&mut contexts, context, selected)
                .await
                .expect_err("The private proxy CA is not trusted");
            assert_eq!(error.partial.as_ref().unwrap()["failureKind"], "tls");
            assert!(!format!("{error:?}").contains("Acme-proxy-private"));
        }
        contexts.remove(context).unwrap();
    }
    assert_eq!(
        proxy.requests.load(Ordering::SeqCst),
        0,
        "No CONNECT header or origin request may cross the untrusted proxy TLS link"
    );
    assert_eq!(proxy.connections.load(Ordering::SeqCst), 6);
    contexts.close().unwrap();
}
