//! Bounded HTTPS transport: fixed origin and path, TLS validation, no
//! redirects, and absolute connection-setup and per-request deadlines enforced
//! on every socket operation.

use super::AcquisitionError;
use crate::{names, source};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use ureq::unversioned::{
    resolver::DefaultResolver,
    transport::{
        Buffers, ConnectionDetails, Connector, NextTimeout, RustlsConnector, TcpConnector,
        Transport, time,
    },
};

/// Longest a connection may take from the start of the TCP connection
/// attempt to the TLS handshake completing.
const SETUP: Duration = Duration::from_secs(3);

/// HTTPS configuration: the per-request deadline and, for test support only,
/// replacement trust roots. Each request gets its own agent and deadline.
#[derive(Clone, Debug)]
pub(crate) struct Client {
    timeout: Duration,
    roots: Option<ureq::tls::RootCerts>,
}

impl Client {
    /// A client whose requests each end by `timeout`, capped at
    /// [`MAX_DEADLINE`](super::MAX_DEADLINE): the HTTP client adds it to the
    /// current instant without overflow checks.
    pub(crate) fn new(timeout: Duration, roots: Option<ureq::tls::RootCerts>) -> Self {
        Self {
            timeout: timeout.min(super::MAX_DEADLINE),
            roots,
        }
    }

    /// Reads `base` + `path` within one absolute deadline that starts now and
    /// covers resolution, connection, TLS, headers and body alike.
    pub(super) fn read(
        &self,
        base: &str,
        path: &str,
        max_bytes: u64,
    ) -> Result<Vec<u8>, AcquisitionError> {
        let deadline = Instant::now() + self.timeout;
        read(&self.agent(deadline), base, path, max_bytes)
    }

    /// Builds an HTTPS-only agent bound to `deadline`.
    ///
    /// The HTTP client checks its overall deadline only before sending; while
    /// a body is read, an elapsed deadline becomes a fresh one-second wait for
    /// every read, and TLS reuses one relative timeout for every socket read it
    /// makes while assembling a record, during the handshake too. A host that
    /// never pauses, or that drips a TLS record, could therefore hold a request
    /// indefinitely or stretch the handshake past the connection timeout.
    ///
    /// The connector chain is TCP, then [`DeadlineConnector`], then TLS, then
    /// [`EstablishedConnector`]. Every socket read and write, during the
    /// handshake and after it, passes the deadline layer, which waits at most
    /// for the time left before the overall deadline and, until the connection
    /// is established, before the setup deadline. The deadline is fixed before
    /// the request starts, so time spent resolving and connecting is never
    /// granted again. Connections are never pooled, so each agent makes one
    /// connection and its deadlines and establishment belong to one request.
    /// Proxies are disabled, so the chain needs no proxy connectors.
    fn agent(&self, deadline: Instant) -> ureq::Agent {
        let mut tls = ureq::tls::TlsConfig::builder();
        if let Some(roots) = self.roots.clone() {
            tls = tls.root_certs(roots);
        }
        let config = ureq::Agent::config_builder()
            .https_only(true)
            .proxy(None)
            .max_redirects(0)
            // Every non-200 status is a response refusal, not a transport failure.
            .http_status_as_error(false)
            .max_response_header_size(16_384)
            .max_idle_connections(0)
            .max_idle_connections_per_host(0)
            .timeout_global(Some(self.timeout))
            .timeout_connect(Some(SETUP))
            .tls_config(tls.build())
            .build();
        let established = Arc::new(AtomicBool::new(false));
        let connector = TcpConnector::default()
            .chain(DeadlineConnector {
                deadline,
                established: established.clone(),
            })
            .chain(RustlsConnector::default())
            .chain(EstablishedConnector { established });
        ureq::Agent::with_parts(config, connector, DefaultResolver::default())
    }
}

/// Wraps every TCP connection in a [`DeadlineTransport`] bound to the
/// request's absolute deadline, before TLS is layered on top.
#[derive(Debug)]
struct DeadlineConnector {
    deadline: Instant,
    established: Arc<AtomicBool>,
}

impl<In: Transport> Connector<In> for DeadlineConnector {
    type Out = DeadlineTransport<In>;

    fn connect(
        &self,
        details: &ConnectionDetails,
        chained: Option<In>,
    ) -> Result<Option<Self::Out>, ureq::Error> {
        // `details.now` is when connection setup began, before the TCP
        // connect, so TCP and the TLS handshake share one setup budget.
        let setup_started = match details.now {
            time::Instant::Exact(started) => started,
            _ => Instant::now(),
        };
        Ok(chained.map(|inner| self.with_inner(inner, setup_started)))
    }
}

impl DeadlineConnector {
    /// Wraps `inner` with the request's overall deadline, never a fresh budget
    /// from the time the connection opened, and a setup deadline measured from
    /// `setup_started`, when connection setup began.
    fn with_inner<T>(&self, inner: T, setup_started: Instant) -> DeadlineTransport<T> {
        DeadlineTransport {
            inner,
            deadline: self.deadline,
            setup_deadline: setup_started + SETUP,
            established: self.established.clone(),
        }
    }
}

/// A socket transport that refuses to send or wait once a deadline passed,
/// and never lets one send or wait outlast it.
///
/// It sits directly on TCP, so every socket operation passes through it: those
/// of the TLS handshake, each read TLS makes while assembling a record, and the
/// HTTP client's reads, including those that discard chunked trailers. Until
/// the connection is established, the earlier setup deadline applies too.
#[derive(Debug)]
struct DeadlineTransport<T> {
    inner: T,
    deadline: Instant,
    setup_deadline: Instant,
    established: Arc<AtomicBool>,
}

impl<T> DeadlineTransport<T> {
    /// `timeout`, shortened to the time left before the deadline in force;
    /// refused once none is left.
    fn bounded(&self, timeout: NextTimeout) -> Result<NextTimeout, ureq::Error> {
        let (limit, reason) =
            if !self.established.load(Ordering::Acquire) && self.setup_deadline < self.deadline {
                (self.setup_deadline, ureq::Timeout::Connect)
            } else {
                (self.deadline, ureq::Timeout::Global)
            };
        let remaining = limit
            .checked_duration_since(Instant::now())
            .filter(|remaining| !remaining.is_zero())
            .ok_or(ureq::Error::Timeout(reason))?;
        if timeout.after.is_not_happening() || *timeout.after > remaining {
            Ok(NextTimeout {
                after: time::Duration::Exact(remaining),
                reason,
            })
        } else {
            Ok(timeout)
        }
    }
}

impl<T: Transport> Transport for DeadlineTransport<T> {
    fn buffers(&mut self) -> &mut dyn Buffers {
        self.inner.buffers()
    }

    fn transmit_output(&mut self, amount: usize, timeout: NextTimeout) -> Result<(), ureq::Error> {
        let timeout = self.bounded(timeout)?;
        self.inner.transmit_output(amount, timeout)
    }

    fn await_input(&mut self, timeout: NextTimeout) -> Result<bool, ureq::Error> {
        let timeout = self.bounded(timeout)?;
        self.inner.await_input(timeout)
    }

    fn is_open(&mut self) -> bool {
        self.inner.is_open()
    }

    fn is_tls(&self) -> bool {
        self.inner.is_tls()
    }
}

/// Marks the connection established once TLS is set up, lifting the setup
/// deadline. It sits above TLS, which handshakes lazily during the first
/// write, so the first write that succeeds is the end of the handshake.
#[derive(Debug)]
struct EstablishedConnector {
    established: Arc<AtomicBool>,
}

impl<In: Transport> Connector<In> for EstablishedConnector {
    type Out = EstablishedTransport<In>;

    fn connect(
        &self,
        _: &ConnectionDetails,
        chained: Option<In>,
    ) -> Result<Option<Self::Out>, ureq::Error> {
        Ok(chained.map(|inner| EstablishedTransport {
            inner,
            established: self.established.clone(),
        }))
    }
}

/// The application-level transport, which reports establishment after its
/// first successful write.
#[derive(Debug)]
struct EstablishedTransport<T> {
    inner: T,
    established: Arc<AtomicBool>,
}

impl<T: Transport> Transport for EstablishedTransport<T> {
    fn buffers(&mut self) -> &mut dyn Buffers {
        self.inner.buffers()
    }

    fn transmit_output(&mut self, amount: usize, timeout: NextTimeout) -> Result<(), ureq::Error> {
        self.inner.transmit_output(amount, timeout)?;
        self.established.store(true, Ordering::Release);
        Ok(())
    }

    fn await_input(&mut self, timeout: NextTimeout) -> Result<bool, ureq::Error> {
        self.inner.await_input(timeout)
    }

    fn is_open(&mut self) -> bool {
        self.inner.is_open()
    }

    fn is_tls(&self) -> bool {
        self.inner.is_tls()
    }
}

fn read(
    agent: &ureq::Agent,
    base: &str,
    path: &str,
    max_bytes: u64,
) -> Result<Vec<u8>, AcquisitionError> {
    if !source::is_https_location(base) || !names::is_artifact_path(path) {
        return Err(AcquisitionError::Location);
    }
    let mut response = agent
        .get(&format!("{base}{path}"))
        .call()
        .map_err(|_| AcquisitionError::Transport)?;
    if response.status() != 200 || response.headers().contains_key("content-encoding") {
        return Err(AcquisitionError::Response);
    }
    let bytes = response
        .body_mut()
        .with_config()
        // ureq's limit errors at exact exhaustion; reserve one byte to observe EOF.
        .limit(max_bytes.saturating_add(1))
        .read_to_vec()
        .map_err(|error| match error {
            ureq::Error::BodyExceedsLimit(_) => AcquisitionError::TooLarge,
            // A connection that closes, passes the deadline or breaks mid-body.
            _ => AcquisitionError::Transport,
        })?;
    if bytes.len() as u64 > max_bytes {
        return Err(AcquisitionError::TooLarge);
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ureq::unversioned::transport::LazyBuffers;

    /// A transport that records the timeouts it is handed.
    #[derive(Debug)]
    struct Recording {
        buffers: LazyBuffers,
        waits: Vec<Duration>,
    }

    impl Transport for Recording {
        fn buffers(&mut self) -> &mut dyn Buffers {
            &mut self.buffers
        }

        fn transmit_output(&mut self, _: usize, timeout: NextTimeout) -> Result<(), ureq::Error> {
            self.waits.push(*timeout.after);
            Ok(())
        }

        fn await_input(&mut self, timeout: NextTimeout) -> Result<bool, ureq::Error> {
            self.waits.push(*timeout.after);
            Ok(false)
        }

        fn is_open(&mut self) -> bool {
            true
        }
    }

    fn recording() -> Recording {
        Recording {
            buffers: LazyBuffers::new(1024, 1024),
            waits: Vec::new(),
        }
    }

    /// An established connection, where only the overall deadline applies.
    fn transport(deadline: Instant) -> DeadlineTransport<Recording> {
        DeadlineTransport {
            inner: recording(),
            deadline,
            setup_deadline: deadline,
            established: Arc::new(AtomicBool::new(true)),
        }
    }

    fn generous() -> NextTimeout {
        NextTimeout {
            after: time::Duration::Exact(Duration::from_secs(3600)),
            reason: ureq::Timeout::RecvBody,
        }
    }

    #[test]
    fn every_wait_is_capped_at_the_time_left_and_refused_after_it() {
        let mut open = transport(Instant::now() + Duration::from_secs(5));
        open.await_input(generous()).unwrap();
        open.transmit_output(0, generous()).unwrap();
        let unbounded = NextTimeout {
            after: time::Duration::NotHappening,
            reason: ureq::Timeout::RecvBody,
        };
        open.await_input(unbounded).unwrap();
        assert!(
            open.inner
                .waits
                .iter()
                .all(|wait| *wait <= Duration::from_secs(5))
        );

        let mut expired = transport(Instant::now());
        assert!(matches!(
            expired.await_input(generous()),
            Err(ureq::Error::Timeout(ureq::Timeout::Global))
        ));
        assert!(matches!(
            expired.transmit_output(0, generous()),
            Err(ureq::Error::Timeout(ureq::Timeout::Global))
        ));
        assert!(expired.inner.waits.is_empty(), "nothing reaches the socket");
    }

    #[test]
    fn connections_inherit_the_request_deadline_instead_of_a_fresh_budget() {
        let deadline = Instant::now();
        let connector = DeadlineConnector {
            deadline,
            established: Arc::new(AtomicBool::new(false)),
        };
        // However late the connection opens, it carries the deadline fixed at request start.
        let transport = connector.with_inner(recording(), deadline + Duration::from_secs(1));
        assert_eq!(transport.deadline, deadline);
    }

    #[test]
    fn connection_setup_is_measured_from_its_start_not_from_the_tcp_connection() {
        let connector = DeadlineConnector {
            deadline: Instant::now() + Duration::from_secs(60),
            established: Arc::new(AtomicBool::new(false)),
        };
        // Setup began two seconds ago, before a slow TCP connect: one second remains.
        let started = Instant::now()
            .checked_sub(Duration::from_secs(2))
            .expect("the clock is past two seconds");
        let transport = connector.with_inner(recording(), started);
        assert_eq!(transport.setup_deadline, started + SETUP);
        assert!(
            transport
                .setup_deadline
                .saturating_duration_since(Instant::now())
                <= Duration::from_secs(1)
        );
    }

    #[test]
    fn the_setup_deadline_applies_until_the_connection_is_established() {
        let established = Arc::new(AtomicBool::new(false));
        let mut setting_up = DeadlineTransport {
            inner: recording(),
            deadline: Instant::now() + Duration::from_secs(60),
            setup_deadline: Instant::now(),
            established: established.clone(),
        };
        // Before establishment the elapsed setup deadline refuses, as a connection timeout.
        assert!(matches!(
            setting_up.await_input(generous()),
            Err(ureq::Error::Timeout(ureq::Timeout::Connect))
        ));
        assert!(matches!(
            setting_up.transmit_output(0, generous()),
            Err(ureq::Error::Timeout(ureq::Timeout::Connect))
        ));
        // Once established, only the overall deadline remains.
        established.store(true, Ordering::Release);
        setting_up.await_input(generous()).unwrap();
        assert!(setting_up.inner.waits[0] > Duration::from_secs(50));

        // A setup deadline later than the overall one never extends it.
        let mut short = DeadlineTransport {
            inner: recording(),
            deadline: Instant::now(),
            setup_deadline: Instant::now() + Duration::from_secs(60),
            established: Arc::new(AtomicBool::new(false)),
        };
        assert!(matches!(
            short.await_input(generous()),
            Err(ureq::Error::Timeout(ureq::Timeout::Global))
        ));
    }

    #[test]
    fn the_first_successful_write_marks_the_connection_established() {
        let established = Arc::new(AtomicBool::new(false));
        let mut application = EstablishedTransport {
            inner: recording(),
            established: established.clone(),
        };
        application.await_input(generous()).unwrap();
        assert!(
            !established.load(Ordering::Acquire),
            "reads do not establish"
        );
        application.transmit_output(0, generous()).unwrap();
        assert!(established.load(Ordering::Acquire));
    }
}
