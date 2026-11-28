//! Selected-session capability admission with non-mutating adapter checks.
use super::{Server, Session, arguments::SessionRef};
use crate::{
    error::{Error, Result},
    targets::capabilities,
};
use serde_json::{Value, json};

impl Server {
    pub(super) fn capability_verified(
        &mut self,
        reference: &SessionRef,
        name: &str,
        details: Value,
    ) {
        capabilities::support(
            &mut self.sessions.get_mut(&reference.session_id).unwrap().target,
            name,
            details,
        );
    }

    pub(super) fn simulator_input_verified(&mut self, reference: &SessionRef, operation: &str) {
        if matches!(
            operation,
            "tap" | "long_press" | "swipe" | "key" | "type_text"
        ) {
            capabilities::input(
                &mut self.sessions.get_mut(&reference.session_id).unwrap().target,
                "simulator_hid",
            );
        }
    }
    pub(super) async fn require_capabilities(
        &self,
        session: &mut Session,
        required: &[String],
    ) -> Result<()> {
        for required in required {
            let name = required.as_str();
            if session
                .target
                .capabilities
                .get(name)
                .is_some_and(|v| v["state"] == "supported")
            {
                continue;
            }
            self.probe_capability(session, name).await?;
            if session
                .target
                .capabilities
                .get(name)
                .and_then(|v| v.get("state"))
                .and_then(Value::as_str)
                != Some("supported")
            {
                return Err(Error::new(
                    "CAPABILITY_UNAVAILABLE",
                    format!("Required capability is unavailable or unverified: {required}"),
                ));
            }
        }
        Ok(())
    }

    async fn probe_capability(&self, session: &mut Session, name: &str) -> Result<()> {
        let target = session.target.clone();
        match name {
            "input" | "tap" | "swipe" | "long_press" | "key" | "type_text"
                if target.kind == "simulator" && session.control =>
            {
                let identity = admission_identity(session).await?;
                if session.simulator_input.get().is_none() {
                    let input = crate::simulator::input::Input::start(
                        &target.platform_id,
                        identity.clone(),
                    )
                    .await?;
                    let _ = session.simulator_input.set(input);
                }
                session.simulator_input.get().unwrap().ready().await?;
                identity.validate()?;
                capabilities::input(&mut session.target, "simulator_hid");
            }
            "screenshots" if target.kind == "device" => {
                self.runners
                    .runner(&target.platform_id)?
                    .call(
                        "screenshot",
                        json!({"format":"png"}),
                        None,
                        std::time::Duration::from_secs(5),
                    )
                    .await?;
                capabilities::support(
                    &mut session.target,
                    "screenshots",
                    json!({"profiles":["evidence"]}),
                );
            }
            "accessibility" => {
                let snapshot = if target.kind == "device" {
                    self.runners
                        .runner(&target.platform_id)?
                        .call(
                            "ui_snapshot",
                            json!({"scope":{"kind":"system"}}),
                            None,
                            std::time::Duration::from_secs(5),
                        )
                        .await?
                        .data
                } else if target.kind == "simulator" {
                    let identity = admission_identity(session).await?;
                    let snapshot = crate::native::snapshot(&target.platform_id).await?;
                    identity.validate()?;
                    snapshot
                } else {
                    return Ok(());
                };
                capabilities::support(
                    &mut session.target,
                    "accessibility",
                    json!({"partial":snapshot["quality"]!="complete"}),
                );
            }
            "recording" if target.kind == "android" => {
                let limit = crate::android::Android::default()
                    .recording_limit(&target.platform_id)
                    .await?;
                capabilities::support(
                    &mut session.target,
                    "recording",
                    json!({"format":"h264","backend":"adb_screenrecord","targetMaxDurationMs":limit}),
                );
            }
            "app_list" if target.kind == "device" => {
                crate::applications::inventory(&target.identity.target_id).await?;
                capabilities::support(&mut session.target, "app_list", json!({}));
            }
            _ => {}
        }
        Ok(())
    }
}

async fn admission_identity(
    session: &Session,
) -> Result<crate::simulator::incarnation::Incarnation> {
    crate::simulator::device_set::validate_target(&session.target)?;
    if let Some(identity) = session.simulator_incarnation.get() {
        identity.validate()?;
        return Ok(identity.clone());
    }
    let identity = crate::native::incarnation(&session.target.platform_id).await?;
    identity.matches_discovery(&session.target.epoch)?;
    let _ = session.simulator_incarnation.set(identity.clone());
    Ok(identity)
}
