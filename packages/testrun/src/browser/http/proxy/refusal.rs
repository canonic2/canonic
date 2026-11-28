//! A refused CONNECT returns only its already-buffered response, without a socket.
use std::{
    fmt,
    io::{Cursor, Read},
    sync::{Arc, Mutex},
};
use ureq::unversioned::{
    resolver::DefaultResolver,
    transport::{Buffers, ConnectionDetails, Connector, LazyBuffers, NextTimeout, Transport},
};

#[derive(Clone, Default)]
pub(in crate::browser::http) struct Capture(Arc<Mutex<Option<Vec<u8>>>>);
impl fmt::Debug for Capture {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PrivateProxyRefusalCapture")
    }
}
impl Capture {
    pub(in crate::browser::http) fn clear(&self) {
        self.0.lock().expect("Proxy refusal").take();
    }
    pub(in crate::browser::http) fn observe(&self, bytes: &[u8]) {
        let Some(end) = bytes
            .windows(4)
            .position(|part| part == b"\r\n\r\n")
            .map(|at| at + 4)
        else {
            return;
        };
        // Preserve the existing response-head bound and at most one transport
        // buffer of early body bytes. Never read more from a refusing peer.
        if end > 16384
            || bytes.len() > 32768
            || !super::super::wire::is_connect_refusal(&bytes[..end])
        {
            return;
        }
        let mut captured = self.0.lock().expect("Proxy refusal");
        if captured.is_none() {
            *captured = Some(bytes.to_vec());
        }
    }
    pub(in crate::browser::http) fn take(&self) -> Option<Refusal> {
        self.0
            .lock()
            .expect("Proxy refusal")
            .take()
            .map(|bytes| Refusal {
                response: Cursor::new(bytes),
                buffers: LazyBuffers::new(16384, 16384),
            })
    }
}
pub(in crate::browser::http) struct Refusal {
    response: Cursor<Vec<u8>>,
    buffers: LazyBuffers,
}
impl Refusal {
    pub(in crate::browser::http) fn bytes(&self) -> &[u8] {
        self.response.get_ref()
    }
    pub(in crate::browser::http) fn response(
        self,
        method: &ureq::http::Method,
        deadline: std::time::Instant,
    ) -> Result<ureq::http::Response<ureq::Body>, ureq::Error> {
        // Parse the captured response on a separate, memory-only HTTP channel.
        // The original HTTPS connector keeps its TlsRequired guard. This agent
        // has no network connector and never receives the origin's headers/body.
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            return Err(ureq::Error::Timeout(ureq::Timeout::Global));
        }
        if method == ureq::http::Method::HEAD
            && self
                .bytes()
                .windows(4)
                .position(|part| part == b"\r\n\r\n")
                .is_some_and(|at| self.bytes().len() > at + 4)
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Unexpected body on refused HEAD request",
            )
            .into());
        }
        let agent = ureq::Agent::with_parts(
            ureq::Agent::config_builder()
                .proxy(None)
                .max_redirects(0)
                .max_idle_connections(0)
                .http_status_as_error(false)
                .timeout_global(Some(remaining))
                .build(),
            ResponseConnector(Mutex::new(Some(self))),
            DefaultResolver::default(),
        );
        let request = ureq::http::Request::builder()
            .method(method.clone())
            .uri("http://127.0.0.1:1/")
            .body(())?;
        agent.run(request)
    }
}
struct ResponseConnector(Mutex<Option<Refusal>>);
impl fmt::Debug for ResponseConnector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PrivateProxyResponseConnector")
    }
}
impl Connector for ResponseConnector {
    type Out = Refusal;
    fn connect(
        &self,
        _details: &ConnectionDetails<'_>,
        _chained: Option<()>,
    ) -> Result<Option<Refusal>, ureq::Error> {
        self.0
            .lock()
            .expect("Single proxy refusal response")
            .take()
            .map(Some)
            .ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "Proxy response already consumed",
                )
                .into()
            })
    }
}
impl fmt::Debug for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PrivateProxyRefusal")
    }
}
impl Transport for Refusal {
    fn buffers(&mut self) -> &mut dyn Buffers {
        &mut self.buffers
    }
    fn transmit_output(&mut self, amount: usize, _timeout: NextTimeout) -> Result<(), ureq::Error> {
        if amount > self.buffers.output().len() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Invalid refused request buffer",
            )
            .into());
        }
        // The proxy socket has already been dropped. The HTTP parser may form
        // its origin request, but no transport can send those private bytes.
        Ok(())
    }
    fn await_input(&mut self, _timeout: NextTimeout) -> Result<bool, ureq::Error> {
        let count = self.response.read(self.buffers.input_append_buf())?;
        self.buffers.input_appended(count);
        Ok(count > 0)
    }
    fn is_open(&mut self) -> bool {
        self.response.position() < (self.response.get_ref().len() as u64)
            || !self.buffers.input().is_empty()
    }
    fn is_tls(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests;
