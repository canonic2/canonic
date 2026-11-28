//! Private transport facts for the final HTTP hop, independent of body evidence.
use openssl::{asn1::Asn1Time, nid::Nid, x509::X509};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    net::{Shutdown, SocketAddr, TcpStream},
    sync::{Arc, Mutex},
    time::{Instant, SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;

#[derive(Clone, Default)]
pub(super) struct Facts {
    proxied: bool,
    pub peer: Option<SocketAddr>,
    pub security: Option<Value>,
    pub security_failure: Option<&'static str>,
}
struct Stages {
    ticket: Uuid,
    start: Instant,
    wall_ms: f64,
    dns_end: Option<Instant>,
    tcp_end: Option<Instant>,
    tls_end: Option<Instant>,
    request_start: Option<Instant>,
    response_start: Option<Instant>,
    facts: Facts,
    deadline: Option<Instant>,
    cancelled: Option<Arc<std::sync::atomic::AtomicBool>>,
    sockets: BTreeMap<Uuid, TcpStream>,
}
impl Default for Stages {
    fn default() -> Self {
        let wall_ms = match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(value) => value.as_millis() as f64,
            Err(value) => -(value.duration().as_millis() as f64),
        };
        Self {
            ticket: Uuid::new_v4(),
            start: Instant::now(),
            wall_ms,
            dns_end: None,
            tcp_end: None,
            tls_end: None,
            request_start: None,
            response_start: None,
            facts: Facts::default(),
            deadline: None,
            cancelled: None,
            sockets: BTreeMap::new(),
        }
    }
}
#[derive(Clone, Default)]
pub(super) struct Trace(Arc<Mutex<Stages>>);
impl std::fmt::Debug for Trace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PrivateHttpConnectionTrace")
    }
}
impl Trace {
    pub fn cancel(&self, owner: &Arc<std::sync::atomic::AtomicBool>) {
        let state = self.0.lock().expect("HTTP transport facts");
        if state
            .cancelled
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(current, owner))
        {
            for stream in state.sockets.values() {
                let _ = stream.shutdown(Shutdown::Both);
            }
        }
    }
    pub fn release(&self, owner: &Arc<std::sync::atomic::AtomicBool>) {
        let mut state = self.0.lock().expect("HTTP transport facts");
        if state
            .cancelled
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(current, owner))
        {
            state.sockets.clear();
            state.cancelled = None;
            state.deadline = None;
        }
    }
    pub fn socket(&self, id: Uuid, stream: &TcpStream) -> Result<(), ureq::Error> {
        let mut state = self.0.lock().expect("HTTP transport facts");
        if state
            .cancelled
            .as_ref()
            .is_some_and(|owner| owner.load(std::sync::atomic::Ordering::Acquire))
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Interrupted,
                "Cancelled HTTP transport",
            )
            .into());
        }
        if state.cancelled.is_some() && !state.sockets.contains_key(&id) {
            state.sockets.insert(id, stream.try_clone()?);
        }
        Ok(())
    }
    pub fn forget_socket(&self, id: Uuid) {
        self.0
            .lock()
            .expect("HTTP transport facts")
            .sockets
            .remove(&id);
    }
    pub fn synthetic_refusal(&self) {
        let mut state = self.0.lock().expect("HTTP transport facts");
        state.facts = Facts::default();
        state.dns_end = None;
        state.tcp_end = None;
        state.tls_end = None;
        state.request_start = None;
    }
    pub fn begin(&self) {
        *self.0.lock().expect("HTTP transport facts") = Stages::default();
    }
    pub fn bind(&self, deadline: Instant, cancelled: Option<Arc<std::sync::atomic::AtomicBool>>) {
        let mut state = self.0.lock().expect("HTTP transport facts");
        state.deadline = Some(deadline);
        state.cancelled = cancelled;
    }
    pub fn limit(
        &self,
        mut timeout: ureq::unversioned::transport::NextTimeout,
    ) -> Result<ureq::unversioned::transport::NextTimeout, ureq::Error> {
        let state = self.0.lock().expect("HTTP transport facts");
        if state
            .cancelled
            .as_ref()
            .is_some_and(|flag| flag.load(std::sync::atomic::Ordering::Acquire))
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Interrupted,
                "Cancelled HTTP transport",
            )
            .into());
        }
        if let Some(deadline) = state.deadline {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(ureq::Error::Timeout(timeout.reason));
            }
            let maximum = timeout
                .not_zero()
                .map_or(remaining, |value| (*value).min(remaining));
            timeout.after = ureq::unversioned::transport::time::Duration::Exact(maximum);
        }
        Ok(timeout)
    }
    pub fn ticket(&self) -> Uuid {
        self.0.lock().expect("HTTP transport facts").ticket
    }
    pub fn resolved(&self) {
        let mut state = self.0.lock().expect("HTTP transport facts");
        if !state.facts.proxied {
            state.dns_end = Some(Instant::now());
        }
    }
    pub fn route(&self, proxied: bool) {
        let mut state = self.0.lock().expect("HTTP transport facts");
        state.facts.proxied = proxied;
        if proxied {
            state.dns_end = None;
        }
    }
    pub fn connected(&self, peer: Option<SocketAddr>, at: Instant) {
        let mut state = self.0.lock().expect("HTTP transport facts");
        state.facts.peer = peer;
        // The pinned proxy agent hands over an already connected socket. Its
        // TCP/DNS events are not events for the destination request.
        if !state.facts.proxied {
            state.tcp_end = Some(at);
            state.request_start = Some(at);
        }
    }
    pub fn secured(&self, details: Result<Value, &'static str>, at: Instant) {
        let mut state = self.0.lock().expect("HTTP transport facts");
        state.tls_end = Some(at);
        state.request_start = Some(at);
        match details {
            Ok(value) => {
                state.facts.security = Some(value);
                state.facts.security_failure = None;
            }
            Err(code) => {
                state.facts.security = None;
                state.facts.security_failure = Some(code);
            }
        }
    }
    pub fn facts(&self) -> Facts {
        self.0.lock().expect("HTTP transport facts").facts.clone()
    }
    pub fn reused(&self, facts: Facts) {
        let mut state = self.0.lock().expect("HTTP transport facts");
        state.facts = facts;
        state.dns_end = None;
        state.tcp_end = None;
        state.tls_end = None;
        state.request_start = Some(Instant::now());
    }
    pub fn response(&self) {
        self.0
            .lock()
            .expect("HTTP transport facts")
            .response_start
            .get_or_insert_with(Instant::now);
    }
    pub fn project(&self, complete: bool) -> Value {
        let state = self.0.lock().expect("HTTP transport facts");
        let offset = |value: Option<Instant>| {
            value.map_or(-1.0, |at| {
                at.saturating_duration_since(state.start).as_secs_f64() * 1000.0
            })
        };
        let connect_end = state.tls_end.or(state.tcp_end);
        let mut value = json!({
            "serverAddr":state.facts.peer.map(|peer|json!({"ipAddress":peer.ip().to_string(),"port":peer.port()})),
            "securityDetails":state.facts.security,
            "timing":{"startTime":state.wall_ms,
                "domainLookupStart":if state.dns_end.is_some(){0.0}else{-1.0},
                "domainLookupEnd":offset(state.dns_end),
                "connectStart":if state.tcp_end.is_some(){offset(state.dns_end.or(Some(state.start)))}else{-1.0},
                "secureConnectionStart":if state.tls_end.is_some(){offset(state.tcp_end)}else{-1.0},
                "connectEnd":offset(connect_end),"requestStart":offset(state.request_start),
                "responseStart":offset(state.response_start),
                "responseEnd":if complete{offset(Some(Instant::now()))}else{-1.0}}
        });
        if let Some(code) = state.facts.security_failure {
            value["securityDetailsFailure"] = json!(code);
        }
        value
    }
}

pub(super) fn security(protocol: Option<&str>, leaf: &[u8]) -> Result<Value, &'static str> {
    let certificate = X509::from_der(leaf).map_err(|_| "HTTP_TLS_METADATA_INVALID")?;
    let epoch = Asn1Time::from_unix(0).map_err(|_| "HTTP_TLS_METADATA_INVALID")?;
    let seconds = |time: &openssl::asn1::Asn1TimeRef| {
        let delta = epoch.diff(time).map_err(|_| "HTTP_TLS_METADATA_INVALID")?;
        Ok::<i64, &'static str>(i64::from(delta.days) * 86400 + i64::from(delta.secs))
    };
    let mut result = json!({
        "validFrom":seconds(certificate.not_before())?,
        "validTo":seconds(certificate.not_after())?
    });
    if let Some(protocol) = protocol {
        result["protocol"] = json!(protocol);
    }
    // Node's TLS projection uses the first CN when an issuer/subject has several.
    for (field, name) in [
        ("issuer", certificate.issuer_name()),
        ("subjectName", certificate.subject_name()),
    ] {
        if let Some(entry) = name.entries_by_nid(Nid::COMMONNAME).next() {
            result[field] = json!(
                entry
                    .data()
                    .to_string()
                    .map_err(|_| "HTTP_TLS_METADATA_INVALID")?
            );
        }
    }
    crate::browser::schema::encoded_len(&result, 16384).map_err(|_| "HTTP_TLS_METADATA_BOUNDS")?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use openssl::{hash::MessageDigest, pkey::PKey, x509::X509Name};

    fn certificate(common_names: &[&str]) -> Vec<u8> {
        let key = PKey::private_key_from_pem(include_bytes!("fixtures/server-key.pem")).unwrap();
        let mut name = X509Name::builder().unwrap();
        name.append_entry_by_nid(Nid::ORGANIZATIONNAME, "Acme")
            .unwrap();
        for common_name in common_names {
            name.append_entry_by_nid(Nid::COMMONNAME, common_name)
                .unwrap();
        }
        let name = name.build();
        let mut builder = X509::builder().unwrap();
        builder.set_version(2).unwrap();
        builder.set_subject_name(&name).unwrap();
        builder.set_issuer_name(&name).unwrap();
        builder.set_pubkey(&key).unwrap();
        builder
            .set_not_before(&Asn1Time::from_unix(1_700_000_000).unwrap())
            .unwrap();
        builder
            .set_not_after(&Asn1Time::from_unix(1_700_086_400).unwrap())
            .unwrap();
        builder.sign(&key, MessageDigest::sha256()).unwrap();
        builder.build().to_der().unwrap()
    }
    #[test]
    fn http_connection_security_keeps_first_common_name_protocol_and_epoch_seconds() {
        let der = certificate(&["Acme first", "Acme second"]);
        assert_eq!(
            security(Some("TLSv1.3"), &der).unwrap(),
            json!({
                "issuer":"Acme first", "subjectName":"Acme first", "protocol":"TLSv1.3",
                "validFrom":1_700_000_000, "validTo":1_700_086_400
            })
        );
    }
    #[test]
    fn http_connection_security_omits_absent_names_and_refuses_invalid_der() {
        assert_eq!(
            security(None, &certificate(&[])).unwrap(),
            json!({
                "validFrom":1_700_000_000, "validTo":1_700_086_400
            })
        );
        assert_eq!(
            security(Some("TLSv1.3"), b"Acme invalid certificate").unwrap_err(),
            "HTTP_TLS_METADATA_INVALID"
        );
    }
}
