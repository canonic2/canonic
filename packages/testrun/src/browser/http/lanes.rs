//! Per-request transport lanes preserve wire facts and cancellation ownership.
use super::*;

#[derive(Default)]
pub(super) struct Activity {
    leases: BTreeMap<Uuid, Active>,
    pub(super) disposed: Option<super::lifetime::Disposed>,
    pub(super) parent_closed: bool,
}
struct Active {
    filter: bool,
    cancelled: Arc<AtomicBool>,
}
pub(super) struct Agents {
    pub(super) default: ureq::Agent,
    pub(super) other_tls_policy: ureq::Agent,
    pub(super) direct: Option<(ureq::Agent, ureq::Agent)>,
}
pub(super) struct TransportLane {
    busy: AtomicBool,
    pub(super) wire: wire::Observer,
    pub(super) agents: Agents,
    pub(super) certificate_agents: Vec<(String, Agents)>,
}
impl TransportLane {
    pub(super) fn new(
        options: &Options,
        proxy: Option<&proxy::Prepared>,
        certificates: &[client_certificates::Prepared],
    ) -> Self {
        let wire = wire::Observer::default();
        let agents = |certificates: Option<Arc<client_certificates::Resolver>>| {
            let agent = |ignore_https_errors, idle, proxied| {
                let config = ureq::Agent::config_builder()
                    .proxy(if proxied {
                        proxy.map(|value| value.proxy.clone())
                    } else {
                        None
                    })
                    .http_status_as_error(false)
                    .max_redirects(0)
                    .max_response_header_size(16384)
                    .max_idle_connections(idle)
                    .tls_config(
                        ureq::tls::TlsConfig::builder()
                            .disable_verification(ignore_https_errors)
                            .build(),
                    )
                    .build();
                wire::agent(
                    config,
                    wire.clone(),
                    if proxied {
                        proxy.and_then(|value| value.authorization.clone())
                    } else {
                        None
                    },
                    if proxied {
                        proxy.and_then(|value| value.socks.clone())
                    } else {
                        None
                    },
                    certificates.clone(),
                )
            };
            let default_agent = agent(
                options.ignore_https_errors.unwrap_or(false),
                if proxy.is_some() { 0 } else { 4 },
                true,
            );
            let override_agent = agent(!options.ignore_https_errors.unwrap_or(false), 0, true);
            let direct_agents = proxy.filter(|value| value.has_bypass()).map(|_| {
                (
                    agent(options.ignore_https_errors.unwrap_or(false), 4, false),
                    agent(!options.ignore_https_errors.unwrap_or(false), 0, false),
                )
            });
            Agents {
                default: default_agent,
                other_tls_policy: override_agent,
                direct: direct_agents,
            }
        };
        let default_agents = agents(None);
        let certificate_agents = certificates
            .iter()
            .map(|certificate| {
                (
                    certificate.origin.clone(),
                    agents(Some(certificate.resolver.clone())),
                )
            })
            .collect();
        Self {
            busy: AtomicBool::new(false),
            wire,
            agents: default_agents,
            certificate_agents,
        }
    }
}
enum Kind {
    Request(Arc<TransportLane>),
    Filter,
}
pub(super) struct Lease {
    client: Arc<Client>,
    pub(super) cancelled: Arc<AtomicBool>,
    id: Uuid,
    kind: Kind,
}
impl Lease {
    pub(super) fn transport(&self) -> &TransportLane {
        match &self.kind {
            Kind::Request(lane) => lane,
            Kind::Filter => unreachable!("Filter leases cannot dispatch HTTP"),
        }
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        let mut activity = self.client.activity.lock().expect("HTTP activity");
        if let Kind::Request(lane) = &self.kind {
            lane.wire.release(&self.cancelled);
            lane.busy.store(false, Ordering::Release);
            // Preserve one idle request lane, rather than retaining every
            // historical concurrency peak and its connection pools.
            let mut lanes = self.client.lanes.lock().expect("HTTP transport lanes");
            let mut kept_idle = false;
            let keep_idle = !(activity.parent_closed
                || activity.disposed.is_some() && self.client.browser.is_none());
            lanes.retain(|lane| {
                if lane.busy.load(Ordering::Acquire) {
                    true
                } else if keep_idle && !kept_idle {
                    kept_idle = true;
                    true
                } else {
                    false
                }
            });
        }
        activity.leases.remove(&self.id);
    }
}
impl Client {
    pub(super) fn is_active(&self) -> bool {
        !self
            .activity
            .lock()
            .expect("HTTP activity")
            .leases
            .is_empty()
    }
    pub(super) fn request_count(&self) -> usize {
        self.activity
            .lock()
            .expect("HTTP activity")
            .leases
            .values()
            .filter(|active| !active.filter)
            .count()
    }
    pub(super) fn filter_lease(self: &Arc<Self>) -> Result<Lease, Error> {
        let mut activity = self.activity.lock().expect("HTTP activity");
        self.require_open(&activity)?;
        if !activity.leases.is_empty() {
            return Err("HTTP_CONTEXT_BUSY".into());
        }
        let id = Uuid::new_v4();
        let cancelled = Arc::new(AtomicBool::new(false));
        activity.leases.insert(
            id,
            Active {
                filter: true,
                cancelled: cancelled.clone(),
            },
        );
        Ok(Lease {
            client: self.clone(),
            cancelled,
            id,
            kind: Kind::Filter,
        })
    }
    pub(super) fn request_lease(self: &Arc<Self>) -> Result<Lease, Error> {
        let mut activity = self.activity.lock().expect("HTTP activity");
        self.require_open(&activity)?;
        if activity.leases.values().any(|active| active.filter) {
            return Err("HTTP_CONTEXT_BUSY".into());
        }
        if activity.leases.len() >= 32 {
            return Err("HTTP_RESPONSE_CAPACITY".into());
        }
        let mut lanes = self.lanes.lock().expect("HTTP transport lanes");
        let lane = if let Some(lane) = lanes.iter().find(|lane| !lane.busy.load(Ordering::Acquire))
        {
            lane.clone()
        } else {
            let lane = Arc::new(TransportLane::new(
                &self.options,
                self.proxy.as_ref(),
                &self.prepared_certificates,
            ));
            lanes.push(lane.clone());
            lane
        };
        lane.busy.store(true, Ordering::Release);
        let id = Uuid::new_v4();
        let cancelled = Arc::new(AtomicBool::new(false));
        activity.leases.insert(
            id,
            Active {
                filter: false,
                cancelled: cancelled.clone(),
            },
        );
        Ok(Lease {
            client: self.clone(),
            cancelled,
            id,
            kind: Kind::Request(lane),
        })
    }
    pub(super) fn dispose_leases(&self, reason: Option<String>, parent_closed: bool) -> bool {
        let mut activity = self.activity.lock().expect("HTTP activity");
        let repeated = activity.disposed.is_some();
        activity.parent_closed |= parent_closed;
        match &mut activity.disposed {
            Some(disposed) if !parent_closed => disposed.fetch_reason = reason,
            Some(_) => {}
            None => {
                activity.disposed = Some(super::lifetime::Disposed {
                    initial_reason: reason.clone(),
                    fetch_reason: reason,
                })
            }
        }
        if self.browser.is_none() || parent_closed {
            for active in activity.leases.values() {
                active.cancelled.store(true, Ordering::SeqCst);
            }
            self.lanes
                .lock()
                .expect("HTTP transport lanes")
                .retain(|lane| lane.busy.load(Ordering::Acquire));
        }
        repeated
    }
}
