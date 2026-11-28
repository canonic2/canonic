//! Bounded SOCKS negotiation below destination TLS and HTTP response observation.
use std::{
    fmt,
    net::IpAddr,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};
use ureq::{
    http::Uri,
    unversioned::transport::{
        ConnectionDetails, Connector, NextTimeout, Transport, time::Duration,
    },
};

#[derive(Clone)]
pub(in crate::browser::http) struct Socks(Arc<Settings>);
struct Settings {
    version: u8,
    user: Vec<u8>,
    password: Vec<u8>,
}
impl fmt::Debug for Socks {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PrivateSocksProxy")
    }
}
impl Socks {
    pub(super) fn new(version: u8, user: String, password: String) -> Result<Self, super::Error> {
        if user.len() > 255 || password.len() > 255 || version == 4 && user.contains('\0') {
            return Err("HTTP_PROXY_INVALID".into());
        }
        Ok(Self(Arc::new(Settings {
            version,
            user: user.into_bytes(),
            password: password.into_bytes(),
        })))
    }
    pub(in crate::browser::http) fn connect(
        &self,
        details: &ConnectionDetails<'_>,
        cancelled: Option<Arc<AtomicBool>>,
        trace: super::super::connection_details::Trace,
        certificates: Option<Arc<super::super::client_certificates::Resolver>>,
    ) -> Result<Box<dyn Transport>, ureq::Error> {
        let connector = Handshake {
            settings: self.clone(),
            cancelled,
            trace: trace.clone(),
        }
        .chain(
            super::super::wire::connection::Tls::new(trace).with_client_certificates(certificates),
        );
        let inner = connector
            .connect(details, None)?
            .ok_or(ureq::Error::ConnectionFailed)?;
        Ok(Box::new(inner))
    }
}
#[derive(Debug)]
struct Handshake {
    settings: Socks,
    cancelled: Option<Arc<AtomicBool>>,
    trace: super::super::connection_details::Trace,
}
struct Budget {
    started: Instant,
    timeout: NextTimeout,
    cancelled: Option<Arc<AtomicBool>>,
}
fn invalid() -> ureq::Error {
    std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        "Invalid private SOCKS handshake",
    )
    .into()
}
impl Budget {
    fn next(&self) -> Result<NextTimeout, ureq::Error> {
        if self
            .cancelled
            .as_ref()
            .is_some_and(|flag| flag.load(Ordering::SeqCst))
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Interrupted,
                "Cancelled private SOCKS handshake",
            )
            .into());
        }
        let after = match self.timeout.after {
            Duration::Exact(after) => {
                let left = after.saturating_sub(self.started.elapsed());
                if left.is_zero() {
                    return Err(ureq::Error::Timeout(self.timeout.reason));
                }
                Duration::Exact(left)
            }
            Duration::NotHappening => Duration::NotHappening,
        };
        Ok(NextTimeout {
            after,
            reason: self.timeout.reason,
        })
    }
    fn send(&self, transport: &mut dyn Transport, bytes: &[u8]) -> Result<(), ureq::Error> {
        let mut at = 0;
        while at < bytes.len() {
            let output = transport.buffers().output();
            let count = output.len().min(bytes.len() - at);
            if count == 0 {
                return Err(invalid());
            }
            output[..count].copy_from_slice(&bytes[at..at + count]);
            transport.transmit_output(count, self.next()?)?;
            at += count;
        }
        Ok(())
    }
    fn read(&self, transport: &mut dyn Transport, count: usize) -> Result<Vec<u8>, ureq::Error> {
        let mut bytes = Vec::with_capacity(count);
        while bytes.len() < count {
            self.next()?;
            if transport.buffers().input().is_empty() {
                let mut timeout = self.next()?;
                // A read slice observes cancellation without resetting the
                // original request budget or issuing another handshake write.
                timeout.after =
                    Duration::Exact((*timeout.after).min(std::time::Duration::from_millis(50)));
                match transport.await_input(timeout) {
                    Ok(true) => {}
                    Ok(false) => {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::UnexpectedEof,
                            "Incomplete private SOCKS handshake",
                        )
                        .into());
                    }
                    Err(ureq::Error::Timeout(_)) => {
                        self.next()?;
                        continue;
                    }
                    Err(error) => return Err(error),
                }
            }
            let input = transport.buffers().input();
            let used = input.len().min(count - bytes.len());
            if used == 0 {
                return Err(invalid());
            }
            bytes.extend_from_slice(&input[..used]);
            transport.buffers().input_consume(used);
        }
        Ok(bytes)
    }
}
impl Handshake {
    fn negotiate_v4(
        &self,
        transport: &mut dyn Transport,
        budget: &Budget,
        host: &str,
        port: u16,
    ) -> Result<(), ureq::Error> {
        let settings = &self.settings.0;
        let mut request = vec![4, 1];
        request.extend_from_slice(&port.to_be_bytes());
        let domain = match host.parse::<std::net::Ipv4Addr>() {
            Ok(ip) => {
                request.extend_from_slice(&ip.octets());
                false
            }
            Err(_) => {
                request.extend_from_slice(&[0, 0, 0, 1]);
                true
            }
        };
        request.extend_from_slice(&settings.user);
        request.push(0);
        if domain {
            if host.len() > 255 {
                return Err(invalid());
            }
            request.extend_from_slice(host.as_bytes());
            request.push(0);
        }
        budget.send(transport, &request)?;
        if budget.read(transport, 8)?[1] != 90 {
            return Err(invalid());
        }
        Ok(())
    }

    fn negotiate_v5(
        &self,
        transport: &mut dyn Transport,
        budget: &Budget,
        host: &str,
        port: u16,
    ) -> Result<(), ureq::Error> {
        let settings = &self.settings.0;
        let authenticate = !settings.user.is_empty() || !settings.password.is_empty();
        budget.send(
            transport,
            if authenticate {
                &[5, 2, 0, 2]
            } else {
                &[5, 1, 0]
            },
        )?;
        let response = budget.read(transport, 2)?;
        if response[0] != 5 {
            return Err(invalid());
        }
        match response[1] {
            0 => {}
            2 => {
                let mut request = vec![1, settings.user.len() as u8];
                request.extend_from_slice(&settings.user);
                request.push(settings.password.len() as u8);
                request.extend_from_slice(&settings.password);
                budget.send(transport, &request)?;
                if budget.read(transport, 2)?[1] != 0 {
                    return Err(invalid());
                }
            }
            _ => return Err(invalid()),
        }
        let mut request = vec![5, 1, 0];
        match host.parse::<IpAddr>() {
            Ok(IpAddr::V4(ip)) => {
                request.push(1);
                request.extend_from_slice(&ip.octets());
            }
            Ok(IpAddr::V6(ip)) => {
                request.push(4);
                request.extend_from_slice(&ip.octets());
            }
            Err(_) => {
                if host.len() > 255 {
                    return Err(invalid());
                }
                request.extend_from_slice(&[3, host.len() as u8]);
                request.extend_from_slice(host.as_bytes());
            }
        }
        request.extend_from_slice(&port.to_be_bytes());
        budget.send(transport, &request)?;
        let response = budget.read(transport, 4)?;
        if response[0] != 5 || response[1] != 0 {
            return Err(invalid());
        }
        let length = match response[3] {
            1 => 4,
            4 => 16,
            3 => budget.read(transport, 1)?[0] as usize,
            _ => return Err(invalid()),
        };
        budget.read(transport, length + 2)?;
        Ok(())
    }
}

impl Connector for Handshake {
    type Out = Box<dyn Transport>;
    fn connect(
        &self,
        details: &ConnectionDetails<'_>,
        _chained: Option<()>,
    ) -> Result<Option<Self::Out>, ureq::Error> {
        let budget = Budget {
            started: Instant::now(),
            timeout: details.timeout,
            cancelled: self.cancelled.clone(),
        };
        budget.next()?;
        let proxy = details.config.proxy().ok_or_else(invalid)?;
        let endpoint: Uri = format!("http://{}:{}", proxy.host(), proxy.port())
            .parse()
            .map_err(|_| invalid())?;
        let config = ureq::Agent::config_builder()
            .proxy(None)
            .no_delay(details.config.no_delay())
            .build();
        let addresses = details
            .resolver
            .resolve(&endpoint, &config, budget.next()?)?;
        let proxy_details = ConnectionDetails {
            uri: &endpoint,
            addrs: addresses,
            config: &config,
            request_level: details.request_level,
            resolver: details.resolver,
            now: (details.current_time)(),
            timeout: budget.next()?,
            current_time: details.current_time.clone(),
            run_connector: details.run_connector.clone(),
        };
        let mut transport =
            super::super::wire::connection::ConnectionConnector::new(self.trace.clone())
                .connect(&proxy_details, None)?
                .ok_or(ureq::Error::ConnectionFailed)?;
        let host = details
            .uri
            .host()
            .ok_or_else(invalid)?
            .trim_start_matches('[')
            .trim_end_matches(']');
        let port = details
            .uri
            .port_u16()
            .unwrap_or(if details.uri.scheme_str() == Some("https") {
                443
            } else {
                80
            });
        if self.settings.0.version == 4 {
            self.negotiate_v4(&mut *transport, &budget, host, port)?;
        } else {
            self.negotiate_v5(&mut *transport, &budget, host, port)?;
        }
        budget.next()?;
        Ok(Some(transport))
    }
}
