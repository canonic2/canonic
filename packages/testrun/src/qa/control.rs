//! Input ownership transitions are serialized after admission has fenced queued work.
use super::Server;
#[cfg(unix)]
use super::arguments::session_ref;
#[cfg(unix)]
use crate::{
    control::{Credential, Owner},
    error::{Error, Result},
};
#[cfg(unix)]
use serde_json::Value;
#[cfg(unix)]
use std::time::Instant;

impl super::Session {
    pub(super) fn authority(&self) -> std::sync::Arc<crate::control::Authority> {
        self.read_gate
            .authority
            .get_or_init(|| {
                std::sync::Arc::new(crate::control::Authority::new(
                    self.lease_id.clone(),
                    self.control,
                ))
            })
            .clone()
    }
}

impl Server {
    #[cfg(unix)]
    pub(crate) async fn change_control(
        &mut self,
        reference: &Value,
        input: &Value,
    ) -> Result<Value> {
        let reference = session_ref(reference)?;
        let session = self.session(&reference)?;
        if session.journal.values().any(|(_, status)| {
            matches!(
                status["state"].as_str(),
                Some("dispatched" | "outcome_unknown")
            )
        }) {
            return Err(Error::new(
                "SESSION_UNCERTAIN",
                "Input must settle before control changes; unknown input requires closing the controller",
            ));
        }
        #[cfg(target_os = "macos")]
        if let Some(hid) = &session.hid {
            let health = hid.health();
            if ["uncertain", "heldContact", "heldKeys"]
                .iter()
                .any(|key| health[key] == true)
            {
                return Err(Error::new(
                    "SESSION_UNCERTAIN",
                    "Release or close uncertain native input before handoff",
                ));
            }
        }
        let desired = desired(input)?;
        let expected = expected(&session.authority(), input)?;
        let proxy = self.control_proxy.clone();
        if matches!(desired, Some(Owner::Viewer(_))) && proxy.is_none() {
            return Err(Error::new(
                "CAPABILITY_UNAVAILABLE",
                "Viewer input requires a retained local controller",
            ));
        }
        let authority = session.authority();
        let credential = authority.apply(&expected, desired.clone(), Instant::now())?;
        let gate = self.sessions[&reference.session_id].read_gate.clone();
        let old = gate.viewer.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(old) = old {
            old.close().await;
        }
        let viewer = if matches!(desired, Some(Owner::Viewer(_))) {
            match crate::control::channel::Channel::start(
                authority.clone(),
                credential.clone(),
                proxy.unwrap(),
            ) {
                Ok(channel) => Some(channel),
                Err(error) => {
                    authority.disconnect(&credential);
                    return Err(error);
                }
            }
        } else {
            None
        };
        let mut status = authority.status(Instant::now());
        if let Some(viewer) = &viewer {
            status["channel"] = viewer.metadata();
        }
        *gate.viewer.lock().unwrap_or_else(|e| e.into_inner()) = viewer;
        Ok(status)
    }
}
#[cfg(unix)]
pub(crate) fn desired(input: &Value) -> Result<Option<Owner>> {
    if input["operation"] == "release" {
        Ok(None)
    } else if input["operation"] == "acquire" {
        crate::control::owner(&input["arguments"]).map(Some)
    } else {
        Err(Error::invalid("Choose acquire or release"))
    }
}
#[cfg(unix)]
pub(crate) fn expected(authority: &crate::control::Authority, input: &Value) -> Result<Credential> {
    let credential = authority.credential(Instant::now());
    if input["arguments"]["expectedVersion"].as_u64() != Some(credential.version) {
        return Err(Error::new(
            "STALE_CONTROL_LEASE",
            "Read control status and use its current controlVersion",
        ));
    }
    Ok(credential)
}
#[cfg(unix)]
impl super::ReadContext {
    pub(crate) fn plan_control(&self, input: &Value) -> Result<()> {
        let expected = expected(&self.authority, input)?;
        self.authority
            .plan(&expected, desired(input)?, Instant::now())
    }
    pub(crate) fn control_status(&self) -> Value {
        let mut status = self.authority.status(Instant::now());
        if let Some(viewer) = self.viewer.as_ref().and_then(std::sync::Weak::upgrade) {
            status["channel"] = viewer.metadata();
        }
        status
    }
    pub(crate) fn control_grant(&self, input: &Value) -> Result<Value> {
        self.viewer
            .as_ref()
            .and_then(std::sync::Weak::upgrade)
            .ok_or_else(|| Error::new("STALE_CONTROL_LEASE", "No current viewer channel"))?
            .grant(input["channelId"].as_str().unwrap_or(""))
    }
}
