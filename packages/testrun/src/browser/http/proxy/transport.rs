//! Own one bounded CONNECT head below origin/TLS observation.
use super::Authorization;
use std::{fmt, time::Instant};
use ureq::unversioned::transport::{Buffers, NextTimeout, Transport, time::Duration};

pub(in crate::browser::http) struct Connect {
    inner: Box<dyn Transport>,
    authorization: Option<Authorization>,
    head: Vec<u8>,
    started: Instant,
    sent: bool,
    refusal: super::refusal::Capture,
    response_head_seen: bool,
}
impl Connect {
    pub(in crate::browser::http) fn new(
        inner: Box<dyn Transport>,
        authorization: Option<Authorization>,
        refusal: super::refusal::Capture,
    ) -> Self {
        Self {
            inner,
            authorization,
            head: Vec::new(),
            started: Instant::now(),
            sent: false,
            refusal,
            response_head_seen: false,
        }
    }
}
impl fmt::Debug for Connect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PrivateConnectTransport")
    }
}
fn invalid() -> ureq::Error {
    std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        "Invalid private CONNECT request",
    )
    .into()
}
impl Transport for Connect {
    fn buffers(&mut self) -> &mut dyn Buffers {
        self.inner.buffers()
    }
    fn transmit_output(&mut self, amount: usize, timeout: NextTimeout) -> Result<(), ureq::Error> {
        if self.sent {
            return self.inner.transmit_output(amount, timeout);
        }
        let output = self.inner.buffers().output();
        if amount > output.len() || self.head.len().saturating_add(amount) > 16384 {
            return Err(invalid());
        }
        self.head.extend_from_slice(&output[..amount]);
        let Some(end) = self.head.windows(4).position(|bytes| bytes == b"\r\n\r\n") else {
            return Ok(());
        };
        if end + 4 != self.head.len() || !self.head.starts_with(b"CONNECT ") {
            return Err(invalid());
        }
        if self.head.split(|byte| *byte == b'\n').any(|line| {
            line.get(..20)
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case(b"proxy-authorization:"))
        }) {
            return Err(invalid());
        }
        let original = std::mem::take(&mut self.head);
        for line in original[..end + 2].split_inclusive(|byte| *byte == b'\n') {
            // The pinned agent sends its own close policy and no origin agent.
            if line
                .get(..11)
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case(b"user-agent:"))
                || line
                    .get(..17)
                    .is_some_and(|prefix| prefix.eq_ignore_ascii_case(b"proxy-connection:"))
            {
                continue;
            }
            self.head.extend_from_slice(line);
        }
        self.head.extend_from_slice(b"Proxy-Connection: close\r\n");
        if let Some(authorization) = &self.authorization {
            self.head.extend_from_slice(authorization.bytes());
        }
        self.head.extend_from_slice(b"\r\n");
        if self.head.len() > 32768 {
            return Err(invalid());
        }
        let mut at = 0;
        while at < self.head.len() {
            let after = match timeout.after {
                Duration::Exact(after) => {
                    let remaining = after.saturating_sub(self.started.elapsed());
                    if remaining.is_zero() {
                        return Err(ureq::Error::Timeout(timeout.reason));
                    }
                    Duration::Exact(remaining)
                }
                Duration::NotHappening => Duration::NotHappening,
            };
            let output = self.inner.buffers().output();
            let count = output.len().min(self.head.len() - at);
            if count == 0 {
                return Err(invalid());
            }
            output[..count].copy_from_slice(&self.head[at..at + count]);
            self.inner
                .transmit_output(count, NextTimeout { after, ..timeout })?;
            at += count;
        }
        self.head.clear();
        self.sent = true;
        Ok(())
    }
    fn await_input(&mut self, timeout: NextTimeout) -> Result<bool, ureq::Error> {
        if !self.sent {
            return Err(invalid());
        }
        let result = self.inner.await_input(timeout)?;
        if !self.response_head_seen {
            let input = self.inner.buffers().input();
            self.refusal.observe(input);
            self.response_head_seen = input.len() > 16384
                || input.windows(2).any(|part| part == b"\n\n")
                || input.windows(3).any(|part| part == b"\n\r\n");
        }
        Ok(result)
    }
    fn is_open(&mut self) -> bool {
        self.inner.is_open()
    }
    fn is_tls(&self) -> bool {
        // A CONNECT tunnel is a fresh destination transport. Encryption to the
        // proxy never satisfies the destination's own TLS handshake.
        false
    }
}
