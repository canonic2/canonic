//! Owned loopback TLS peers validate actual client identity and absence.
use base64::{Engine as _, engine::general_purpose::STANDARD};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    task::JoinHandle,
};
use tokio_rustls::{TlsAcceptor, rustls};

pub struct Identities {
    pub certs: [String; 2],
    pub key: String,
    second_key: Option<String>,
}
impl Identities {
    pub async fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let key = include_str!("../../src/browser/http/fixtures/server-key.pem").to_string();
        let key_path = directory.path().join("Acme-key.pem");
        tokio::fs::write(&key_path, &key).await.unwrap();
        let configuration = directory.path().join("Acme-client.cnf");
        tokio::fs::write(&configuration, "[req]\ndistinguished_name=dn\nx509_extensions=client\nprompt=no\n[dn]\nCN=Acme\n[client]\nbasicConstraints=critical,CA:FALSE\nkeyUsage=critical,digitalSignature\nextendedKeyUsage=clientAuth\n").await.unwrap();
        let mut certs = Vec::new();
        for name in ["Acme-client-A", "Acme-client-B"] {
            let path = directory.path().join(format!("{name}.pem"));
            let output = tokio::process::Command::new("/usr/bin/openssl")
                .env_clear()
                .current_dir(directory.path())
                .args(["req", "-new", "-x509", "-sha256", "-days", "2", "-subj"])
                .arg(format!("/CN={name}"))
                .arg("-config")
                .arg(&configuration)
                .arg("-key")
                .arg(&key_path)
                .arg("-out")
                .arg(&path)
                .output()
                .await
                .unwrap();
            assert!(
                output.status.success(),
                "Owned client fixture generation: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            certs.push(tokio::fs::read_to_string(path).await.unwrap());
        }
        Self {
            certs: certs.try_into().unwrap(),
            key,
            second_key: None,
        }
    }
    pub async fn different_algorithms() -> Self {
        use openssl::{
            asn1::Asn1Time,
            bn::BigNum,
            ec::{EcGroup, EcKey},
            hash::MessageDigest,
            nid::Nid,
            pkey::PKey,
            x509::{
                X509, X509NameBuilder,
                extension::{BasicConstraints, ExtendedKeyUsage, KeyUsage},
            },
        };
        let mut identities = Self::new().await;
        let group = EcGroup::from_curve_name(Nid::X9_62_PRIME256V1).unwrap();
        let key = PKey::from_ec_key(EcKey::generate(&group).unwrap()).unwrap();
        let mut name = X509NameBuilder::new().unwrap();
        name.append_entry_by_text("CN", "Acme-client-B").unwrap();
        let name = name.build();
        let mut cert = X509::builder().unwrap();
        cert.set_version(2).unwrap();
        cert.set_serial_number(&BigNum::from_u32(2).unwrap().to_asn1_integer().unwrap())
            .unwrap();
        cert.set_subject_name(&name).unwrap();
        cert.set_issuer_name(&name).unwrap();
        cert.set_pubkey(&key).unwrap();
        cert.set_not_before(&Asn1Time::days_from_now(0).unwrap())
            .unwrap();
        cert.set_not_after(&Asn1Time::days_from_now(2).unwrap())
            .unwrap();
        cert.append_extension(BasicConstraints::new().critical().build().unwrap())
            .unwrap();
        cert.append_extension(
            KeyUsage::new()
                .critical()
                .digital_signature()
                .build()
                .unwrap(),
        )
        .unwrap();
        cert.append_extension(ExtendedKeyUsage::new().client_auth().build().unwrap())
            .unwrap();
        cert.sign(&key, MessageDigest::sha256()).unwrap();
        identities.certs[1] = String::from_utf8(cert.build().to_pem().unwrap()).unwrap();
        identities.second_key =
            Some(String::from_utf8(key.private_key_to_pem_pkcs8().unwrap()).unwrap());
        identities
    }
    fn key_for(&self, identity: usize) -> &str {
        if identity == 1 {
            self.second_key.as_deref().unwrap_or(&self.key)
        } else {
            &self.key
        }
    }
    pub fn input(&self, origin: &str, identity: usize) -> Value {
        json!({"origin":origin,"cert":STANDARD.encode(self.certs[identity].as_bytes()),"key":STANDARD.encode(self.key_for(identity).as_bytes())})
    }
    pub fn encrypted_input(&self, origin: &str, identity: usize, pfx: bool) -> Value {
        use openssl::{pkcs12::Pkcs12, pkey::PKey, symm::Cipher, x509::X509};
        let private = PKey::private_key_from_pem(self.key_for(identity).as_bytes()).unwrap();
        let leaf = X509::from_pem(self.certs[identity].as_bytes()).unwrap();
        let passphrase = "Acme fixture passphrase";
        if pfx {
            let mut builder = Pkcs12::builder();
            builder.name("Acme client").pkey(&private).cert(&leaf);
            let bytes = builder.build2(passphrase).unwrap().to_der().unwrap();
            json!({"origin":origin,"pfx":STANDARD.encode(bytes),"passphrase":passphrase})
        } else {
            let key = private
                .private_key_to_pem_pkcs8_passphrase(Cipher::aes_256_cbc(), passphrase.as_bytes())
                .unwrap();
            json!({"origin":origin,"cert":STANDARD.encode(self.certs[identity].as_bytes()),"key":STANDARD.encode(key),"passphrase":passphrase})
        }
    }
}
pub struct Site {
    pub url: String,
    pub requests: Arc<Mutex<Vec<Value>>>,
    redirect: Arc<Mutex<Option<String>>>,
    task: JoinHandle<()>,
}
#[derive(Debug)]
struct SignatureChoices(
    Arc<dyn rustls::server::danger::ClientCertVerifier>,
    Vec<rustls::SignatureScheme>,
);
impl rustls::server::danger::ClientCertVerifier for SignatureChoices {
    fn client_auth_mandatory(&self) -> bool {
        self.0.client_auth_mandatory()
    }
    fn root_hint_subjects(&self) -> &[rustls::DistinguishedName] {
        self.0.root_hint_subjects()
    }
    fn verify_client_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::server::danger::ClientCertVerified, rustls::Error> {
        self.0.verify_client_cert(end_entity, intermediates, now)
    }
    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        self.0.verify_tls12_signature(message, cert, dss)
    }
    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        self.0.verify_tls13_signature(message, cert, dss)
    }
    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.1.clone()
    }
}
impl Drop for Site {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Site {
    pub async fn start(identities: &Identities) -> Self {
        Self::start_with_hint(identities, None).await
    }
    pub async fn start_with_hint(identities: &Identities, hint: Option<usize>) -> Self {
        Self::start_configured(identities, hint, None).await
    }
    pub async fn start_rsa_only(identities: &Identities) -> Self {
        Self::start_configured(
            identities,
            None,
            Some(vec![rustls::SignatureScheme::RSA_PSS_SHA256]),
        )
        .await
    }
    pub async fn start_signature_order(identities: &Identities, rsa_first: bool) -> Self {
        let mut schemes = vec![
            rustls::SignatureScheme::RSA_PSS_SHA256,
            rustls::SignatureScheme::ECDSA_NISTP256_SHA256,
        ];
        if !rsa_first {
            schemes.reverse();
        }
        Self::start_configured(identities, None, Some(schemes)).await
    }
    async fn start_configured(
        identities: &Identities,
        hint: Option<usize>,
        schemes: Option<Vec<rustls::SignatureScheme>>,
    ) -> Self {
        let clients = identities
            .certs
            .iter()
            .map(|pem| CertificateDer::from_pem_slice(pem.as_bytes()).unwrap())
            .collect::<Vec<_>>();
        let mut roots = rustls::RootCertStore::empty();
        for cert in &clients {
            roots.add(cert.clone()).unwrap();
        }
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let mut verifier = rustls::server::WebPkiClientVerifier::builder_with_provider(
            Arc::new(roots),
            provider.clone(),
        )
        .allow_unauthenticated();
        if let Some(index) = hint {
            let cert = openssl::x509::X509::from_pem(identities.certs[index].as_bytes()).unwrap();
            verifier = verifier.clear_root_hint_subjects().add_root_hint_subjects([
                rustls::DistinguishedName::from(cert.subject_name().to_der().unwrap()),
            ]);
        }
        let verifier = verifier.build().unwrap();
        let verifier: Arc<dyn rustls::server::danger::ClientCertVerifier> =
            if let Some(schemes) = schemes {
                Arc::new(SignatureChoices(verifier, schemes))
            } else {
                verifier
            };
        let config = rustls::ServerConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_client_cert_verifier(verifier)
            .with_single_cert(
                vec![
                    CertificateDer::from_pem_slice(include_bytes!(
                        "../../src/browser/http/fixtures/server.pem"
                    ))
                    .unwrap(),
                ],
                PrivateKeyDer::from_pem_slice(include_bytes!(
                    "../../src/browser/http/fixtures/server-key.pem"
                ))
                .unwrap(),
            )
            .unwrap();
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let url = format!(
            "https://localhost:{}",
            listener.local_addr().unwrap().port()
        );
        let acceptor = TlsAcceptor::from(Arc::new(config));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let redirect = Arc::new(Mutex::new(None::<String>));
        let observed = requests.clone();
        let destination = redirect.clone();
        let task = tokio::spawn(async move {
            let mut calls = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    accepted=listener.accept()=>{
                        let (stream,_)=accepted.unwrap();let acceptor=acceptor.clone();
                        let clients=clients.clone();let observed=observed.clone();let destination=destination.clone();
                        calls.spawn(async move {
                            let Ok(mut stream)=acceptor.accept(stream).await else{return;};
                            let peer=stream.get_ref().1.peer_certificates().and_then(|chain|chain.first());
                            let identity=peer.and_then(|peer|clients.iter().position(|cert|cert==peer)).map_or("none",|index|if index==0{"A"}else{"B"});
                            loop {
                                let mut head=Vec::new();let mut chunk=[0;1024];
                                while !head.windows(4).any(|part|part==b"\r\n\r\n") {
                                    let Ok(count)=stream.read(&mut chunk).await else{return;};
                                    if count==0||head.len()+count>16384{return;}head.extend_from_slice(&chunk[..count]);
                                }
                                let path=String::from_utf8_lossy(&head).split_whitespace().nth(1).unwrap_or("").to_string();
                                observed.lock().unwrap().push(json!({"path":path,"identity":identity}));
                                let location=destination.lock().unwrap().clone().filter(|_|path=="/redirect");
                                let response=if let Some(location)=location {
                                    format!("HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: keep-alive\r\n\r\n")
                                }else{format!("HTTP/1.1 200 Acme identity\r\nContent-Length: {}\r\nConnection: keep-alive\r\n\r\n{identity}",identity.len())};
                                if stream.write_all(response.as_bytes()).await.is_err(){return;}
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
            redirect,
            task,
        }
    }
    pub fn redirect_to(&self, url: &str) {
        *self.redirect.lock().unwrap() = Some(url.into());
    }
}
