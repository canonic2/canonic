//! Browser-side ephemeral TLS and independently verified origin-side TLS.
use super::*;
use openssl::{
    asn1::Asn1Time,
    bn::BigNum,
    ec::{EcGroup, EcKey},
    hash::MessageDigest,
    nid::Nid,
    pkey::PKey,
    x509::{X509, X509NameBuilder},
};

pub(super) fn server() -> io::Result<(
    rustls::ServerConfig,
    rustls::pki_types::CertificateDer<'static>,
)> {
    fn material() -> Result<(Vec<u8>, Vec<u8>), openssl::error::ErrorStack> {
        let group = EcGroup::from_curve_name(Nid::X9_62_PRIME256V1)?;
        let key = PKey::from_ec_key(EcKey::generate(&group)?)?;
        let mut name = X509NameBuilder::new()?;
        name.append_entry_by_text("CN", "Testrun private certificate tunnel")?;
        let name = name.build();
        let mut certificate = X509::builder()?;
        certificate.set_version(2)?;
        let mut serial = BigNum::new()?;
        serial.rand(128, openssl::bn::MsbOption::MAYBE_ZERO, false)?;
        let serial = serial.to_asn1_integer()?;
        certificate.set_serial_number(&serial)?;
        certificate.set_subject_name(&name)?;
        certificate.set_issuer_name(&name)?;
        certificate.set_pubkey(&key)?;
        let before = Asn1Time::days_from_now(0)?;
        let after = Asn1Time::days_from_now(1)?;
        certificate.set_not_before(&before)?;
        certificate.set_not_after(&after)?;
        certificate.append_extension(
            openssl::x509::extension::BasicConstraints::new()
                .critical()
                .build()?,
        )?;
        certificate.append_extension(
            openssl::x509::extension::ExtendedKeyUsage::new()
                .server_auth()
                .build()?,
        )?;
        let names = openssl::x509::extension::SubjectAlternativeName::new()
            .dns("localhost")
            .build(&certificate.x509v3_context(None, None))?;
        certificate.append_extension(names)?;
        certificate.sign(&key, MessageDigest::sha256())?;
        Ok((certificate.build().to_der()?, key.private_key_to_pkcs8()?))
    }
    let (certificate, key) = material().map_err(|_| invalid())?;
    let certificate = rustls::pki_types::CertificateDer::from(certificate);
    let config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(
            vec![certificate.clone()],
            rustls::pki_types::PrivatePkcs8KeyDer::from(key).into(),
        )
        .map_err(|_| invalid())?;
    Ok((config, certificate))
}
pub(super) fn name(host: &str) -> io::Result<rustls::pki_types::ServerName<'static>> {
    rustls::pki_types::ServerName::try_from(host.to_owned()).map_err(|_| invalid())
}
pub(super) async fn relay(
    browser: TcpStream,
    upstream: Socket,
    host: &str,
    identity: Arc<super::super::http::client_certificates::Resolver>,
    settings: Settings,
) -> io::Result<()> {
    let hello =
        tokio_rustls::LazyConfigAcceptor::new(rustls::server::Acceptor::default(), browser).await?;
    let protocols: Vec<Vec<u8>> = hello
        .client_hello()
        .alpn()
        .map(|values| values.map(Vec::from).collect())
        .unwrap_or_else(|| vec![b"http/1.1".to_vec()]);
    let mut client = rustls::ClientConfig::builder()
        .with_root_certificates(settings.roots.clone())
        .with_client_cert_resolver(identity);
    client.alpn_protocols = protocols;
    let connected = tokio_rustls::TlsConnector::from(Arc::new(client))
        .connect(name(host)?, upstream)
        .await;
    let mut server = (*settings.server).clone();
    match connected {
        Ok(mut upstream) => {
            server.alpn_protocols = upstream
                .get_ref()
                .1
                .alpn_protocol()
                .map(|protocol| vec![protocol.to_vec()])
                .unwrap_or_default();
            let mut browser = hello.into_stream(Arc::new(server)).await?;
            tokio::io::copy_bidirectional(&mut browser, &mut upstream).await?;
        }
        Err(_) => {
            // As in the pinned proxy, an upstream TLS refusal becomes a local
            // error response, never a retry without the configured identity.
            server.alpn_protocols = vec![b"http/1.1".to_vec()];
            let mut browser = hello.into_stream(Arc::new(server)).await?;
            let body = b"Testrun client-certificate error: upstream TLS verification or negotiation failed";
            browser.write_all(format!("HTTP/1.1 503 Internal Server Error\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).as_bytes()).await?;
            browser.write_all(body).await?;
            browser.shutdown().await?;
        }
    }
    Ok(())
}
