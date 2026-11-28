//! Bounded plaintext response-head observation above the unchanged TLS/proxy chain.
//! A Client's existing lease admits one request at a time, including redirects.
use super::*;
use std::{fmt, sync::atomic::AtomicUsize};
use ureq::unversioned::transport::{Buffers, ConnectionDetails, Connector, NextTimeout, Transport};
pub(in crate::browser::http) mod connection;

const MAX_HEAD_BYTES: usize = 16384;
const MAX_INFORMATIONAL: u8 = 8;

#[derive(Default)]
struct State {
    bytes: Vec<u8>,
    head: Option<Head>,
    informational: u8,
    failure: Option<&'static str>,
}
pub(super) struct Head {
    status: u16,
    reason: Vec<u8>,
    headers: Vec<(String, Vec<u8>)>,
}
#[derive(Clone, Default)]
pub(super) struct Observer(Arc<Observed>);
#[derive(Default)]
struct Observed {
    state: Mutex<State>,
    active: AtomicBool,
    refusal: super::proxy::refusal::Capture,
    cancelled: Mutex<Option<Arc<AtomicBool>>>,
    transport: super::connection_details::Trace,
}
impl fmt::Debug for Observer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ResponseHeadObserver")
    }
}
impl Observer {
    pub(super) fn cancel(&self, owner: &Arc<AtomicBool>) {
        self.0.transport.cancel(owner);
    }
    pub(super) fn release(&self, owner: &Arc<AtomicBool>) {
        self.0.transport.release(owner);
    }
    pub(super) fn start(&self, cancelled: Option<Arc<AtomicBool>>) {
        self.0.transport.begin();
        *self.0.state.lock().expect("HTTP wire head") = State::default();
        self.0.refusal.clear();
        *self.0.cancelled.lock().expect("HTTP wire admission") = cancelled;
        self.0.active.store(true, Ordering::Release);
    }
    pub(super) fn start_deadline(
        &self,
        cancelled: Option<Arc<AtomicBool>>,
        deadline: std::time::Instant,
    ) {
        self.start(cancelled.clone());
        self.0.transport.bind(deadline, cancelled);
    }
    pub(super) fn transport_details(&self, complete: bool) -> Value {
        self.0.transport.project(complete)
    }
    pub(super) fn refusal_response(
        &self,
        result: Result<ureq::http::Response<ureq::Body>, ureq::Error>,
        method: &ureq::http::Method,
        deadline: std::time::Instant,
    ) -> Result<ureq::http::Response<ureq::Body>, ureq::Error> {
        match result {
            Err(error) if matches!(&error, ureq::Error::ConnectProxyFailed(_)) => {
                let Some(refusal) = self.0.refusal.take() else {
                    return Err(error);
                };
                self.0.transport.synthetic_refusal();
                self.observe(refusal.bytes());
                refusal.response(method, deadline)
            }
            other => other,
        }
    }
    fn observe(&self, bytes: &[u8]) {
        if !self.0.active.load(Ordering::Acquire) {
            return;
        }
        let mut state = self.0.state.lock().expect("HTTP wire head");
        for byte in bytes {
            if state.bytes.len() == MAX_HEAD_BYTES {
                state.failure = Some("wire-head-byte-bound");
                self.0.active.store(false, Ordering::Release);
                break;
            }
            state.bytes.push(*byte);
            if !head_ended(&state.bytes) {
                continue;
            }
            let Some(head) = parse(&state.bytes) else {
                state.failure = Some("wire-head-parse");
                self.0.active.store(false, Ordering::Release);
                break;
            };
            let informational = (100..200).contains(&head.status) && head.status != 101;
            state.head = Some(head);
            state.bytes.clear();
            if !informational {
                self.0.transport.response();
                self.0.active.store(false, Ordering::Release);
                break;
            }
            state.informational += 1;
            if state.informational > MAX_INFORMATIONAL {
                state.failure = Some("wire-informational-bound");
                self.0.active.store(false, Ordering::Release);
                break;
            }
        }
    }
    pub(super) fn take(
        &self,
        response: &ureq::http::Response<ureq::Body>,
    ) -> Result<Head, &'static str> {
        self.0.active.store(false, Ordering::Release);
        self.0.transport.response();
        let mut state = std::mem::take(&mut *self.0.state.lock().expect("HTTP wire head"));
        if let Some(failure) = state.failure {
            return Err(failure);
        }
        let head = state.head.take().ok_or("wire-head-unavailable")?;
        if head.status != response.status().as_u16() {
            return Err("wire-status-mismatch");
        }
        let mut wire: BTreeMap<String, Vec<&[u8]>> = BTreeMap::new();
        for (name, value) in &head.headers {
            wire.entry(name.to_ascii_lowercase())
                .or_default()
                .push(value);
        }
        let mut parsed: BTreeMap<String, Vec<&[u8]>> = BTreeMap::new();
        for (name, value) in response.headers() {
            parsed
                .entry(name.as_str().into())
                .or_default()
                .push(value.as_bytes());
        }
        if wire != parsed {
            return Err("wire-headers-mismatch");
        }
        Ok(head)
    }
}
fn head_ended(bytes: &[u8]) -> bool {
    bytes.ends_with(b"\n\n") || bytes.ends_with(b"\n\r\n")
}
pub(super) fn is_connect_refusal(bytes: &[u8]) -> bool {
    parse(bytes).is_some_and(|head| head.status != 200)
}
fn parse(bytes: &[u8]) -> Option<Head> {
    if !head_ended(bytes) {
        return None;
    }
    let mut lines = bytes.split(|byte| *byte == b'\n');
    let line = lines.next()?;
    let status = line.strip_suffix(b"\r").unwrap_or(line);
    let mut parts = status.splitn(3, |byte| *byte == b' ');
    if !matches!(parts.next()?, b"HTTP/1.0" | b"HTTP/1.1") {
        return None;
    }
    let code = parts.next()?;
    if code.len() != 3 || !code.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let status = std::str::from_utf8(code).ok()?.parse().ok()?;
    let reason = parts.next().unwrap_or_default().to_vec();
    let mut headers = Vec::new();
    for line in lines {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if line.is_empty() {
            break;
        }
        let colon = line.iter().position(|byte| *byte == b':')?;
        let name = &line[..colon];
        ureq::http::HeaderName::from_bytes(name).ok()?;
        let value = line[colon + 1..].trim_ascii().to_vec();
        headers.push((std::str::from_utf8(name).ok()?.into(), value));
    }
    Some(Head {
        status,
        reason,
        headers,
    })
}
fn latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| char::from(*byte)).collect()
}
impl Head {
    pub(super) fn project(self) -> (String, Value) {
        use base64::Engine as _;
        let headers = self
            .headers
            .into_iter()
            .map(|(name, value)| {
                let mut field = json!({"name":name,"value":latin1(&value)});
                if !value.is_ascii() {
                    field["valueBase64"] =
                        json!(base64::engine::general_purpose::STANDARD.encode(value));
                }
                field
            })
            .collect::<Vec<_>>();
        (latin1(&self.reason), json!(headers))
    }
}
#[derive(Debug)]
struct WireConnector {
    inner: connection::ConnectionConnector,
    observer: Observer,
    depth: AtomicUsize,
    authorization: Option<super::proxy::Authorization>,
    proxy_tls: ureq::config::Config,
    refusal: super::proxy::refusal::Capture,
    socks: Option<super::proxy::socks::Socks>,
    certificates: Option<Arc<super::client_certificates::Resolver>>,
}
struct Connecting<'a>(&'a AtomicUsize);
impl Drop for Connecting<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}
impl Connector for WireConnector {
    type Out = Box<dyn Transport>;
    fn connect(
        &self,
        details: &ConnectionDetails<'_>,
        chained: Option<()>,
    ) -> Result<Option<Self::Out>, ureq::Error> {
        // The existing per-client lease serializes connection attempts. Nested
        // CONNECT-proxy connections must stay below this plaintext observer.
        let outer = self.depth.fetch_add(1, Ordering::SeqCst) == 0;
        let _connecting = Connecting(&self.depth);
        if outer {
            self.refusal.clear();
            self.observer
                .0
                .transport
                .route(details.config.proxy().is_some());
        }
        // Endpoint TLS exceptions never disable an HTTPS proxy's own identity
        // check, and endpoint client certificates must not reach that proxy.
        let proxy_details;
        let details = if !outer && details.uri.scheme_str() == Some("https") {
            proxy_details = ConnectionDetails {
                uri: details.uri,
                addrs: details.addrs.clone(),
                config: &self.proxy_tls,
                request_level: details.request_level,
                resolver: details.resolver,
                now: details.now,
                timeout: details.timeout,
                current_time: details.current_time.clone(),
                run_connector: details.run_connector.clone(),
            };
            &proxy_details
        } else {
            details
        };
        let mut inner = match (outer, &self.socks) {
            (true, Some(socks)) => {
                let cancelled = self
                    .observer
                    .0
                    .cancelled
                    .lock()
                    .expect("HTTP wire admission")
                    .clone();
                socks.connect(
                    details,
                    cancelled,
                    self.observer.0.transport.clone(),
                    self.certificates.clone(),
                )?
            }
            _ => match self.inner.connect_scoped(details, chained, outer)? {
                Some(inner) => inner,
                None => return Ok(None),
            },
        };
        if outer {
            self.observer.observe(inner.buffers().input());
            Ok(Some(Box::new(WireTransport {
                inner,
                observer: self.observer.clone(),
                request: None,
                facts: None,
            })))
        } else {
            Ok(Some(Box::new(super::proxy::transport::Connect::new(
                inner,
                self.authorization.clone(),
                self.refusal.clone(),
            ))))
        }
    }
}
struct WireTransport {
    inner: Box<dyn Transport>,
    observer: Observer,
    request: Option<Uuid>,
    facts: Option<super::connection_details::Facts>,
}
impl WireTransport {
    fn begin_request(&mut self) {
        let ticket = self.observer.0.transport.ticket();
        if self.request != Some(ticket) {
            self.request = Some(ticket);
            if let Some(facts) = &self.facts {
                self.observer.0.transport.reused(facts.clone());
            }
        }
    }
    fn retain_facts(&mut self) {
        if self.facts.is_none() {
            self.facts = Some(self.observer.0.transport.facts());
        }
    }
}
impl fmt::Debug for WireTransport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ObservedHttpTransport")
    }
}
impl Transport for WireTransport {
    fn buffers(&mut self) -> &mut dyn Buffers {
        self.inner.buffers()
    }
    fn transmit_output(&mut self, amount: usize, timeout: NextTimeout) -> Result<(), ureq::Error> {
        self.begin_request();
        self.inner.transmit_output(amount, timeout)?;
        self.retain_facts();
        Ok(())
    }
    fn await_input(&mut self, timeout: NextTimeout) -> Result<bool, ureq::Error> {
        self.begin_request();
        let before = self.inner.buffers().input().len();
        let result = self.inner.await_input(timeout)?;
        let input = self.inner.buffers().input();
        if let Some(appended) = input.get(before..) {
            self.observer.observe(appended);
        }
        Ok(result)
    }
    fn is_open(&mut self) -> bool {
        self.inner.is_open()
    }
    fn is_tls(&self) -> bool {
        self.inner.is_tls()
    }
}
pub(super) fn agent(
    config: ureq::config::Config,
    observer: Observer,
    authorization: Option<super::proxy::Authorization>,
    socks: Option<super::proxy::socks::Socks>,
    certificates: Option<Arc<super::client_certificates::Resolver>>,
) -> ureq::Agent {
    let transport = observer.0.transport.clone();
    ureq::Agent::with_parts(
        config,
        WireConnector {
            inner: connection::ConnectionConnector::new(transport.clone())
                .with_client_certificates(certificates.clone()),
            refusal: observer.0.refusal.clone(),
            observer,
            depth: AtomicUsize::new(0),
            authorization,
            socks,
            certificates,
            proxy_tls: ureq::Agent::config_builder()
                .proxy(None)
                .tls_config(
                    ureq::tls::TlsConfig::builder()
                        .disable_verification(false)
                        .build(),
                )
                .build(),
        },
        connection::ObservedResolver::new(transport),
    )
}

#[cfg(test)]
mod proxy_tests;
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fragments_informational_heads_and_body_bytes_preserve_only_the_final_head() {
        let observer = Observer::default();
        observer.start(None);
        let bytes=b"HTTP/1.1 103 Early Hints\r\nLink: ignored\r\n\r\nHTTP/1.1 201 Acme custom\r\nX-First: one\r\nx-second: two\r\nx-first: three\r\n\r\nprivate body";
        for byte in bytes {
            observer.observe(&[*byte]);
        }
        let state = observer.0.state.lock().unwrap();
        let head = state.head.as_ref().unwrap();
        assert_eq!(head.status, 201);
        assert_eq!(head.reason, b"Acme custom");
        assert_eq!(
            head.headers
                .iter()
                .map(|(name, _)| name.as_str())
                .collect::<Vec<_>>(),
            vec!["X-First", "x-second", "x-first"]
        );
        assert!(state.bytes.is_empty());
        assert!(!format!("{observer:?}").contains("Acme"));
    }
    #[test]
    fn byte_and_informational_bounds_are_explicit_and_reset_does_not_reuse_a_head() {
        let observer = Observer::default();
        observer.start(None);
        observer.observe(&vec![b'A'; MAX_HEAD_BYTES + 1]);
        assert_eq!(
            observer.0.state.lock().unwrap().failure,
            Some("wire-head-byte-bound")
        );
        observer.start(None);
        for _ in 0..=MAX_INFORMATIONAL {
            observer.observe(b"HTTP/1.1 100 Continue\r\n\r\n");
        }
        assert_eq!(
            observer.0.state.lock().unwrap().failure,
            Some("wire-informational-bound")
        );
        observer.start(None);
        assert!(observer.0.state.lock().unwrap().head.is_none());
    }
    #[test]
    fn empty_and_latin1_reason_phrases_are_wire_values() {
        assert_eq!(parse(b"HTTP/1.0 204\r\n\r\n").unwrap().project().0, "");
        assert_eq!(
            parse(b"HTTP/1.1 200 Acme \xff\r\n\r\n")
                .unwrap()
                .project()
                .0,
            "Acme ÿ"
        );
    }
    #[test]
    fn mismatched_or_missing_wire_metadata_remains_an_explicit_parsed_fallback() {
        let observer = Observer::default();
        observer.start(None);
        observer.observe(b"HTTP/1.1 200 Acme secret reason\r\nX-Acme: wire\r\n\r\n");
        let response = ureq::http::Response::builder()
            .status(200)
            .header("x-acme", "parsed")
            .body(ureq::Body::builder().data(Vec::new()))
            .unwrap();
        let failure = observer.take(&response).err().unwrap();
        assert_eq!(failure, "wire-headers-mismatch");
        let value = details::capture(
            Uuid::new_v4(),
            &Url::parse("https://example.com/acme").unwrap(),
            &response,
            Err(failure),
        );
        assert_eq!(value["wireMetadataAvailable"], false);
        assert_eq!(value["wireMetadataFailure"], failure);
        assert_eq!(value["headerNames"], "lowercase");
        assert!(value["statusText"].is_null());
        assert_eq!(value["statusTextAvailable"], false);
        observer.start(None);
        assert_eq!(
            observer.take(&response).err().unwrap(),
            "wire-head-unavailable"
        );
    }
}
