//! Exclusive logical input leases; device ownership and media lifetimes remain independent.
use crate::error::{Error, Result};
use serde_json::{Value, json};
use std::{
    sync::Mutex,
    time::{Duration, Instant},
};
use tokio::sync::Notify;

pub(crate) mod channel;
#[cfg(test)]
mod tests;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Owner {
    Agent,
    Viewer(String),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Credential {
    pub(crate) id: String,
    pub(crate) version: u64,
}

struct State {
    credential: Credential,
    owner: Option<Owner>,
    pending: Option<Option<Owner>>,
    connected: bool,
    expires: Option<Instant>,
    closed: bool,
    reason: Option<&'static str>,
}
pub(crate) struct Authority {
    enabled: bool,
    state: Mutex<State>,
    changed: Notify,
}
impl Authority {
    pub(crate) fn new(id: String, enabled: bool) -> Self {
        Self {
            enabled,
            state: Mutex::new(State {
                credential: Credential { id, version: 1 },
                owner: enabled.then_some(Owner::Agent),
                pending: None,
                connected: false,
                expires: None,
                closed: false,
                reason: None,
            }),
            changed: Notify::new(),
        }
    }
    fn expire(state: &mut State, now: Instant) {
        if state.expires.is_some_and(|deadline| now >= deadline) {
            Self::release(
                state,
                if state.connected {
                    "heartbeat_expired"
                } else {
                    "viewer_connect_timeout"
                },
            );
        }
    }
    fn release(state: &mut State, reason: &'static str) {
        state.credential = Credential {
            id: uuid::Uuid::new_v4().to_string(),
            version: state.credential.version + 1,
        };
        state.owner = None;
        state.pending = None;
        state.expires = None;
        state.connected = false;
        state.reason = Some(reason);
    }
    pub(crate) fn credential(&self, now: Instant) -> Credential {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        Self::expire(&mut state, now);
        state.credential.clone()
    }
    pub(crate) fn status(&self, now: Instant) -> Value {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        Self::expire(&mut state, now);
        json!({"state":if state.closed {"closed"} else if state.pending.is_some() {"handoff_pending"} else if matches!(state.owner, Some(Owner::Viewer(_))) && !state.connected {"viewer_pending"} else if state.owner.is_some() {"active"} else {"released"},
            "owner":match state.owner {Some(Owner::Agent)=>Some("agent"),Some(Owner::Viewer(_))=>Some("viewer"),None=>None},
            "viewerId":match &state.owner {Some(Owner::Viewer(id))=>Some(id),_=>None},
            "leaseId":state.credential.id,"leaseVersion":state.credential.version,"controlVersion":state.credential.version,"reason":state.reason,
            "heartbeatTimeoutMs":5000,"expiresInMs":state.expires.map(|deadline|deadline.saturating_duration_since(now).as_millis())})
    }
    /// Admission fencing rejects queued old work while an already active command settles.
    pub(crate) fn plan(
        &self,
        expected: &Credential,
        desired: Option<Owner>,
        now: Instant,
    ) -> Result<()> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        Self::expire(&mut state, now);
        if !self.enabled || state.closed {
            return Err(Error::new(
                "CONTROL_NOT_OWNED",
                "An observer or closed session cannot acquire input",
            ));
        }
        if state.credential != *expected {
            return Err(stale());
        }
        if state
            .pending
            .as_ref()
            .is_some_and(|pending| pending != &desired)
        {
            return Err(Error::new(
                "CONTROL_HANDOFF_PENDING",
                "A different handoff is already pending; settle or retry that handoff",
            ));
        }
        state.pending = Some(desired);
        self.changed.notify_waiters();
        Ok(())
    }
    pub(crate) fn apply(
        &self,
        expected: &Credential,
        desired: Option<Owner>,
        now: Instant,
    ) -> Result<Credential> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        Self::expire(&mut state, now);
        if !self.enabled
            || state.closed
            || state.credential != *expected
            || state.pending.as_ref() != Some(&desired)
        {
            return Err(stale());
        }
        state.credential = Credential {
            id: uuid::Uuid::new_v4().to_string(),
            version: state.credential.version + 1,
        };
        state.expires =
            matches!(desired, Some(Owner::Viewer(_))).then_some(now + Duration::from_secs(30));
        state.owner = desired;
        state.pending = None;
        state.connected = false;
        state.reason = None;
        self.changed.notify_waiters();
        Ok(state.credential.clone())
    }
    pub(crate) fn admit(&self, id: &str, now: Instant) -> Result<Credential> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        Self::expire(&mut state, now);
        if state.pending.is_some() {
            return Err(Error::new(
                "CONTROL_HANDOFF_PENDING",
                "Input is fenced while control changes owner",
            ));
        }
        if state.closed
            || state.owner.is_none()
            || state.credential.id != id
            || matches!(state.owner, Some(Owner::Viewer(_))) && !state.connected
        {
            return Err(stale());
        }
        Ok(state.credential.clone())
    }
    pub(crate) fn connect(&self, credential: &Credential, now: Instant) -> Result<()> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        Self::expire(&mut state, now);
        if state.credential != *credential
            || state.closed
            || state.pending.is_some()
            || !matches!(state.owner, Some(Owner::Viewer(_)))
        {
            return Err(stale());
        }
        if state.connected {
            return Err(Error::new(
                "TARGET_BUSY",
                "This viewer lease already has a connected host",
            ));
        }
        state.connected = true;
        state.expires = Some(now + Duration::from_secs(5));
        self.changed.notify_waiters();
        Ok(())
    }
    pub(crate) fn heartbeat(&self, credential: &Credential, now: Instant) -> Result<()> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        Self::expire(&mut state, now);
        if state.credential != *credential
            || state.closed
            || !state.connected
            || !matches!(state.owner, Some(Owner::Viewer(_)))
        {
            return Err(stale());
        }
        state.expires = Some(now + Duration::from_secs(5));
        self.changed.notify_waiters();
        Ok(())
    }
    pub(crate) fn disconnect(&self, credential: &Credential) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.credential == *credential && matches!(state.owner, Some(Owner::Viewer(_))) {
            Self::release(&mut state, "viewer_disconnected");
            self.changed.notify_waiters();
        }
    }
    pub(crate) fn close(&self) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        Self::release(&mut state, "session_closed");
        state.closed = true;
        self.changed.notify_waiters();
    }
    /// Pending handoff does not cancel the active command; expiry/revocation does.
    pub(crate) async fn lost(&self, credential: &Credential) {
        self.until(|state| {
            (state.closed || state.credential != *credential || state.owner.is_none()).then_some(())
        })
        .await;
    }
    /// Returns why a delay must end: a handoff started fencing input, the lease
    /// changed or the session closed. Unlike input, a delay never holds up a handoff.
    pub(crate) async fn interrupted(&self, credential: &Credential) -> Error {
        self.until(|state| {
            if state.closed {
                Some(Error::new("SESSION_CLOSING", "Session cleanup has started"))
            } else if state.pending.is_some() {
                Some(Error::new(
                    "CONTROL_HANDOFF_PENDING",
                    "A control handoff started",
                ))
            } else if state.credential != *credential || state.owner.is_none() {
                Some(stale())
            } else {
                None
            }
        })
        .await
    }
    async fn until<T>(&self, check: impl Fn(&State) -> Option<T>) -> T {
        loop {
            let changed = self.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            let deadline = {
                let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
                Self::expire(&mut state, Instant::now());
                if let Some(found) = check(&state) {
                    return found;
                }
                state.expires
            };
            if let Some(deadline) = deadline {
                tokio::select! { () = &mut changed => {}, () = tokio::time::sleep_until(deadline.into()) => {} }
            } else {
                changed.await;
            }
        }
    }
}
pub(crate) fn owner(args: &Value) -> Result<Owner> {
    match args["owner"].as_str() {
        Some("agent") if args.get("viewerId").is_none() => Ok(Owner::Agent),
        Some("viewer") => {
            let id = args["viewerId"]
                .as_str()
                .filter(|id| {
                    !id.is_empty()
                        && id.len() <= 128
                        && id
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
                })
                .ok_or_else(|| Error::invalid("viewerId must be a bounded opaque host identity"))?;
            Ok(Owner::Viewer(id.into()))
        }
        _ => Err(Error::invalid(
            "Choose agent or a viewer with a trusted host identity",
        )),
    }
}
fn stale() -> Error {
    Error::new(
        "STALE_CONTROL_LEASE",
        "Input lease is no longer current; read control status before acquiring control",
    )
}
