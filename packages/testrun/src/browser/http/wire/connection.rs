//! HTTP transports with private connection witnesses and the existing TLS policy.
use super::super::connection_details::{self, Trace};
use std::{
    fmt,
    io::{Read, Write},
    net::TcpStream,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use ureq::{
    Error,
    unversioned::{
        resolver::{DefaultResolver, ResolvedSocketAddrs, Resolver},
        transport::{
            Buffers, ConnectProxyConnector, ConnectionDetails, Connector, LazyBuffers, NextTimeout,
            Transport, TransportAdapter,
        },
    },
};
use uuid::Uuid;
#[cfg(test)]
mod certificate_scope_tests;

#[derive(Debug)]
pub(in crate::browser::http) struct ConnectionConnector {
    proxy: ConnectProxyConnector,
    tcp: Tcp,
    tls: Tls,
}
impl ConnectionConnector {
    pub fn new(trace: Trace) -> Self {
        Self {
            proxy: ConnectProxyConnector::default(),
            tcp: Tcp(trace.clone()),
            tls: Tls::new(trace),
        }
    }
    pub fn with_client_certificates(
        mut self,
        certificates: Option<Arc<super::super::client_certificates::Resolver>>,
    ) -> Self {
        self.tls = self.tls.with_client_certificates(certificates);
        self
    }
    pub fn connect_scoped(
        &self,
        details: &ConnectionDetails<'_>,
        chained: Option<()>,
        endpoint: bool,
    ) -> Result<Option<Box<dyn Transport>>, Error> {
        let transport = self.proxy.connect(details, chained)?;
        let transport = self.tcp.connect(details, transport)?;
        self.tls.connect_endpoint(details, transport, endpoint)
    }
}
impl Connector for ConnectionConnector {
    type Out = Box<dyn Transport>;
    fn connect(
        &self,
        details: &ConnectionDetails<'_>,
        chained: Option<()>,
    ) -> Result<Option<Self::Out>, Error> {
        self.connect_scoped(details, chained, true)
    }
}
#[derive(Debug)]
pub(in crate::browser::http) struct ObservedResolver {
    inner: DefaultResolver,
    trace: Trace,
}
impl ObservedResolver {
    pub fn new(trace: Trace) -> Self {
        Self {
            inner: DefaultResolver::default(),
            trace,
        }
    }
}
impl Resolver for ObservedResolver {
    fn resolve(
        &self,
        uri: &ureq::http::Uri,
        config: &ureq::config::Config,
        timeout: NextTimeout,
    ) -> Result<ResolvedSocketAddrs, Error> {
        let result = self.inner.resolve(uri, config, timeout);
        let host = uri
            .host()
            .unwrap_or_default()
            .trim_start_matches('[')
            .trim_end_matches(']');
        if result.is_ok() && host.parse::<std::net::IpAddr>().is_err() {
            self.trace.resolved();
        }
        result
    }
}
#[derive(Debug)]
struct Tcp(Trace);
impl<In: Transport> Connector<In> for Tcp {
    type Out = Box<dyn Transport>;
    fn connect(
        &self,
        details: &ConnectionDetails<'_>,
        chained: Option<In>,
    ) -> Result<Option<Self::Out>, Error> {
        if let Some(transport) = chained {
            return Ok(Some(transport.boxed()));
        }
        let started = Instant::now();
        let budget = self
            .0
            .limit(details.timeout)?
            .not_zero()
            .map(|value| *value);
        let weight_total = 2.0 * (1.0 - 0.5_f64.powi(details.addrs.len() as i32));
        let mut weight = 1.0;
        let mut last = Error::ConnectionFailed;
        for address in &details.addrs {
            let timeout = match budget {
                Some(budget) => {
                    let remaining = budget.saturating_sub(started.elapsed());
                    if remaining.is_zero() {
                        return Err(Error::Timeout(details.timeout.reason));
                    }
                    Some(
                        Duration::from_secs_f64(budget.as_secs_f64() * weight / weight_total)
                            .max(Duration::from_millis(10))
                            .min(remaining),
                    )
                }
                None => None,
            };
            let connected = match timeout {
                Some(timeout) => TcpStream::connect_timeout(address, timeout),
                None => TcpStream::connect(address),
            };
            match connected {
                Ok(stream) => {
                    let connected_at = Instant::now();
                    if details.config.no_delay() {
                        stream.set_nodelay(true)?;
                    }
                    self.0.connected(stream.peer_addr().ok(), connected_at);
                    return Ok(Some(Box::new(Socket {
                        id: Uuid::new_v4(),
                        stream,
                        buffers: LazyBuffers::new(
                            details.config.input_buffer_size(),
                            details.config.output_buffer_size(),
                        ),
                        read_timeout: None,
                        write_timeout: None,
                        trace: self.0.clone(),
                    })));
                }
                Err(error) if error.kind() == std::io::ErrorKind::ConnectionRefused => {
                    last = error.into();
                    continue;
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                    ) =>
                {
                    last = Error::Timeout(details.timeout.reason);
                }
                Err(error) => return Err(error.into()),
            }
            weight *= 0.5;
        }
        Err(last)
    }
}
struct Socket {
    id: Uuid,
    stream: TcpStream,
    buffers: LazyBuffers,
    read_timeout: Option<Duration>,
    write_timeout: Option<Duration>,
    trace: Trace,
}
impl Drop for Socket {
    fn drop(&mut self) {
        self.trace.forget_socket(self.id);
    }
}
impl fmt::Debug for Socket {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PrivateHttpSocket")
    }
}
fn io_error(error: std::io::Error, timeout: NextTimeout) -> Error {
    if matches!(
        error.kind(),
        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
    ) {
        Error::Timeout(timeout.reason)
    } else {
        error.into()
    }
}
impl Transport for Socket {
    fn buffers(&mut self) -> &mut dyn Buffers {
        &mut self.buffers
    }
    fn transmit_output(&mut self, amount: usize, timeout: NextTimeout) -> Result<(), Error> {
        self.trace.socket(self.id, &self.stream)?;
        let timeout = self.trace.limit(timeout)?;
        let next = timeout.not_zero().map(|value| *value);
        if self.write_timeout != next {
            self.stream.set_write_timeout(next)?;
            self.write_timeout = next;
        }
        self.stream
            .write_all(&self.buffers.output()[..amount])
            .map_err(|error| io_error(error, timeout))
    }
    fn await_input(&mut self, timeout: NextTimeout) -> Result<bool, Error> {
        self.trace.socket(self.id, &self.stream)?;
        let timeout = self.trace.limit(timeout)?;
        let next = timeout.not_zero().map(|value| *value);
        if self.read_timeout != next {
            self.stream.set_read_timeout(next)?;
            self.read_timeout = next;
        }
        let amount = self
            .stream
            .read(self.buffers.input_append_buf())
            .map_err(|error| io_error(error, timeout))?;
        self.buffers.input_appended(amount);
        Ok(amount > 0)
    }
    fn is_open(&mut self) -> bool {
        if self.stream.set_nonblocking(true).is_err() {
            return false;
        }
        let open = matches!(self.stream.peek(&mut [0]),Err(error) if error.kind()==std::io::ErrorKind::WouldBlock);
        self.stream.set_nonblocking(false).is_ok() && open
    }
}
pub(in crate::browser::http) struct Tls {
    trace: Trace,
    certificates: Option<Arc<super::super::client_certificates::Resolver>>,
    configurations: Mutex<Vec<(ureq::tls::TlsConfig, bool, Arc<rustls::ClientConfig>)>>,
}
impl fmt::Debug for Tls {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PrivateHttpTlsConnector")
    }
}
impl Tls {
    pub fn new(trace: Trace) -> Self {
        Self {
            trace,
            certificates: None,
            configurations: Mutex::new(Vec::new()),
        }
    }
    pub fn with_client_certificates(
        mut self,
        certificates: Option<Arc<super::super::client_certificates::Resolver>>,
    ) -> Self {
        self.certificates = certificates;
        self
    }
    fn configuration(
        &self,
        tls: &ureq::tls::TlsConfig,
        endpoint: bool,
    ) -> Result<Arc<rustls::ClientConfig>, Error> {
        if let Some((_, _, config)) = self
            .configurations
            .lock()
            .expect("HTTP TLS configurations")
            .iter()
            .find(|(known, scope, _)| *scope == endpoint && same_tls(known, tls))
        {
            return Ok(config.clone());
        }
        let provider = tls
            .unversioned_rustls_crypto_provider()
            .clone()
            .unwrap_or_else(|| Arc::new(rustls::crypto::ring::default_provider()));
        let builder = rustls::ClientConfig::builder_with_provider(provider.clone())
            .with_protocol_versions(rustls::ALL_VERSIONS)?;
        let builder = if tls.disable_verification() {
            builder
                .dangerous()
                .with_custom_certificate_verifier(Arc::new(UntrustedPeer(provider)))
        } else {
            let roots = match tls.root_certs() {
                ureq::tls::RootCerts::WebPki => rustls::RootCertStore {
                    roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
                },
                ureq::tls::RootCerts::Specific(certificates) => {
                    let mut roots = rustls::RootCertStore::empty();
                    roots.add_parsable_certificates(certificates.iter().map(|cert| {
                        rustls::pki_types::CertificateDer::from(cert.der()).into_owned()
                    }));
                    roots
                }
                _ => return Err(Error::Tls("Unsupported HTTP TLS roots")),
            };
            builder.with_root_certificates(roots)
        };
        let mut config = if endpoint && let Some(certificates) = &self.certificates {
            builder.with_client_cert_resolver(certificates.clone())
        } else if let Some(identity) = tls.client_cert() {
            let certificates = identity
                .certs()
                .iter()
                .map(|cert| rustls::pki_types::CertificateDer::from(cert.der()).into_owned())
                .collect();
            let key =
                rustls::pki_types::PrivateKeyDer::try_from(identity.private_key().der().to_vec())
                    .map_err(|_| Error::Tls("Invalid HTTP client key"))?;
            builder.with_client_auth_cert(certificates, key)?
        } else {
            builder.with_no_client_auth()
        };
        config.enable_sni = tls.use_sni();
        let config = Arc::new(config);
        let mut cache = self.configurations.lock().expect("HTTP TLS configurations");
        // One endpoint policy and the independently verified HTTPS proxy policy.
        if cache.len() < 2 {
            cache.push((tls.clone(), endpoint, config.clone()));
        }
        Ok(config)
    }
}
fn same_tls(left: &ureq::tls::TlsConfig, right: &ureq::tls::TlsConfig) -> bool {
    if left.provider() != right.provider()
        || left.use_sni() != right.use_sni()
        || left.disable_verification() != right.disable_verification()
    {
        return false;
    }
    let crypto = match (
        left.unversioned_rustls_crypto_provider(),
        right.unversioned_rustls_crypto_provider(),
    ) {
        (None, None) => true,
        (Some(left), Some(right)) => Arc::ptr_eq(left, right),
        _ => false,
    };
    if !crypto {
        return false;
    }
    let certificates_equal = |left: &[ureq::tls::Certificate<'_>],
                              right: &[ureq::tls::Certificate<'_>]| {
        left.len() == right.len()
            && left
                .iter()
                .zip(right)
                .all(|(left, right)| left.der() == right.der())
    };
    let roots = match (left.root_certs(), right.root_certs()) {
        (ureq::tls::RootCerts::WebPki, ureq::tls::RootCerts::WebPki) => true,
        (ureq::tls::RootCerts::Specific(left), ureq::tls::RootCerts::Specific(right)) => {
            certificates_equal(left, right)
        }
        _ => false,
    };
    roots
        && match (left.client_cert(), right.client_cert()) {
            (None, None) => true,
            (Some(left), Some(right)) => {
                certificates_equal(left.certs(), right.certs())
                    && left.private_key().kind() == right.private_key().kind()
                    && left.private_key().der() == right.private_key().der()
            }
            _ => false,
        }
}
impl Tls {
    fn connect_endpoint<In: Transport>(
        &self,
        details: &ConnectionDetails<'_>,
        chained: Option<In>,
        endpoint: bool,
    ) -> Result<Option<Box<dyn Transport>>, Error> {
        let transport = chained.ok_or(Error::ConnectionFailed)?;
        if !details.needs_tls() || transport.is_tls() {
            return Ok(Some(transport.boxed()));
        }
        if details.config.tls_config().provider() != ureq::tls::TlsProvider::Rustls {
            return Err(Error::Tls("Unsupported HTTP TLS provider"));
        }
        let host = details
            .uri
            .host()
            .ok_or(Error::Tls("Missing HTTP TLS host"))?
            .trim_start_matches('[')
            .trim_end_matches(']');
        let name = rustls::pki_types::ServerName::try_from(host.to_owned())
            .map_err(|_| Error::Tls("Invalid HTTP TLS host"))?;
        let conn = rustls::ClientConnection::new(
            self.configuration(details.config.tls_config(), endpoint)?,
            name,
        )?;
        Ok(Some(Box::new(SecureSocket {
            stream: rustls::StreamOwned {
                conn,
                sock: TransportAdapter::new(transport.boxed()),
            },
            buffers: LazyBuffers::new(
                details.config.input_buffer_size(),
                details.config.output_buffer_size(),
            ),
            trace: self.trace.clone(),
            announced: false,
            endpoint,
        })))
    }
}
impl<In: Transport> Connector<In> for Tls {
    type Out = Box<dyn Transport>;
    fn connect(
        &self,
        details: &ConnectionDetails<'_>,
        chained: Option<In>,
    ) -> Result<Option<Self::Out>, Error> {
        self.connect_endpoint(details, chained, true)
    }
}
struct SecureSocket {
    stream: rustls::StreamOwned<rustls::ClientConnection, TransportAdapter>,
    buffers: LazyBuffers,
    trace: Trace,
    announced: bool,
    endpoint: bool,
}
impl fmt::Debug for SecureSocket {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PrivateHttpTlsSocket")
    }
}
impl SecureSocket {
    fn handshake(&mut self, timeout: NextTimeout) -> Result<(), Error> {
        self.stream.sock.set_timeout(timeout);
        if self.announced {
            return Ok(());
        }
        while self.stream.conn.is_handshaking() {
            self.stream.conn.complete_io(&mut self.stream.sock)?;
        }
        let completed_at = Instant::now();
        if self.endpoint {
            let protocol = match self.stream.conn.protocol_version() {
                Some(rustls::ProtocolVersion::TLSv1_3) => Some("TLSv1.3"),
                Some(rustls::ProtocolVersion::TLSv1_2) => Some("TLSv1.2"),
                _ => None,
            };
            let security = self
                .stream
                .conn
                .peer_certificates()
                .and_then(|certificates| certificates.first())
                .ok_or("HTTP_TLS_METADATA_UNAVAILABLE")
                .and_then(|leaf| connection_details::security(protocol, leaf.as_ref()));
            self.trace.secured(security, completed_at);
        }
        self.announced = true;
        Ok(())
    }
}
impl Transport for SecureSocket {
    fn buffers(&mut self) -> &mut dyn Buffers {
        &mut self.buffers
    }
    fn transmit_output(&mut self, amount: usize, timeout: NextTimeout) -> Result<(), Error> {
        self.handshake(timeout)?;
        self.stream.write_all(&self.buffers.output()[..amount])?;
        Ok(())
    }
    fn await_input(&mut self, timeout: NextTimeout) -> Result<bool, Error> {
        self.handshake(timeout)?;
        let amount = self.stream.read(self.buffers.input_append_buf())?;
        self.buffers.input_appended(amount);
        Ok(amount > 0)
    }
    fn is_open(&mut self) -> bool {
        self.stream.sock.get_mut().is_open()
    }
    fn is_tls(&self) -> bool {
        true
    }
}
#[derive(Debug)]
struct UntrustedPeer(Arc<rustls::crypto::CryptoProvider>);
impl rustls::client::danger::ServerCertVerifier for UntrustedPeer {
    fn verify_server_cert(
        &self,
        _: &rustls::pki_types::CertificateDer<'_>,
        _: &[rustls::pki_types::CertificateDer<'_>],
        _: &rustls::pki_types::ServerName<'_>,
        _: &[u8],
        _: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }
    fn verify_tls12_signature(
        &self,
        message: &[u8],
        certificate: &rustls::pki_types::CertificateDer<'_>,
        signature: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            certificate,
            signature,
            &self.0.signature_verification_algorithms,
        )
    }
    fn verify_tls13_signature(
        &self,
        message: &[u8],
        certificate: &rustls::pki_types::CertificateDer<'_>,
        signature: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            certificate,
            signature,
            &self.0.signature_verification_algorithms,
        )
    }
    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}
