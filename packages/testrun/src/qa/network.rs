//! Network control workflow: lease-guarded switch changes with recorded
//! outcomes. Changes persist until the caller requests the opposite mode.
use super::{Server, arguments::session_ref};
use crate::{
    error::{Error, Result},
    network::{self, Request},
};
use serde_json::{Value, json};

impl Server {
    /// Refuse before any adb command when the lease or verified capability
    /// does not admit this request. Returns the verified adb link.
    fn admit_network(
        &self,
        reference: &super::arguments::SessionRef,
        lease: Option<&str>,
        target: &crate::targets::Target,
        request: Request,
        command: &str,
        signature: &Value,
    ) -> Result<std::result::Result<String, Value>> {
        let session = self.session(reference)?;
        session
            .authority()
            .admit(lease.unwrap_or(""), std::time::Instant::now())
            .map_err(not_admitted)?;
        if !session.control {
            return Err(not_admitted(Error::new(
                "CONTROL_NOT_OWNED",
                "Network control requires the selected controller's current lease",
            )));
        }
        if let Some((previous, status)) = session.journal.get(command) {
            if previous != signature {
                return Err(Error::new(
                    "COMMAND_ID_CONFLICT",
                    "Network command already has different arguments",
                ));
            }
            return Ok(Err(status.clone()));
        }
        if session.journal.len() >= 10000 {
            return Err(not_admitted(Error::new(
                "RESOURCE_LIMIT",
                "Session command journal is full",
            )));
        }
        if session
            .journal
            .values()
            .any(|(_, v)| matches!(v["state"].as_str(), Some("dispatched" | "outcome_unknown")))
        {
            return Err(not_admitted(Error::new(
                "SESSION_UNCERTAIN",
                "A prior command is unresolved; inspect status and disconnect before more mutations",
            )));
        }
        let Some((methods, link)) =
            crate::targets::capabilities::android_network_methods(&json!(target.capabilities))
        else {
            let reason = target.capabilities["network"]["reason"]
                .as_str()
                .unwrap_or("Network control is not verified for this target");
            return Err(not_admitted(
                Error::new("CAPABILITY_UNAVAILABLE", reason)
                    .details(json!({"network":target.capabilities["network"]})),
            ));
        };
        if !methods.iter().any(|m| m == request.method.name()) {
            return Err(not_admitted(
                Error::new(
                    "CAPABILITY_UNAVAILABLE",
                    format!(
                        "The {} method needs Android API {} or later; this target offers {}",
                        request.method.name(),
                        request.method.minimum_api(),
                        methods.join(", ")
                    ),
                )
                .details(json!({"network":target.capabilities["network"]})),
            ));
        }
        Ok(Ok(link))
    }

    pub(crate) async fn network_request(
        &mut self,
        reference: Value,
        name: &str,
        args: Value,
        command: Option<&str>,
    ) -> Result<Value> {
        network::validate(name, &args).map_err(not_admitted)?;
        let request = Request::parse(&args).map_err(not_admitted)?;
        let lease = reference["leaseId"].as_str().map(String::from);
        let reference = session_ref(&reference)?;
        let target = self.live_target(&reference).await.map_err(not_admitted)?;
        if target.kind != "android" {
            return Err(not_admitted(Error::new(
                "CAPABILITY_UNAVAILABLE",
                "Network control is available on Android targets only",
            )));
        }
        let command = command.ok_or_else(|| {
            Error::invalid("Network control requires an internal command identity")
        })?;
        let signature = json!({"tool":name,"arguments":args});
        let link = match self.admit_network(
            &reference,
            lease.as_deref(),
            &target,
            request,
            command,
            &signature,
        )? {
            Ok(link) => link,
            Err(recorded) => return terminal(recorded),
        };
        self.record_command(
            &reference.session_id,
            command.into(),
            signature.clone(),
            json!({"commandId":command,"state":"dispatched","tool":name}),
        )?;
        let adapter = crate::android::Android::default();
        // Nothing has been sent if the first readback fails; that is a plain
        // refusal with `dispatched: false`.
        let before = match adapter.network_readback(&target.platform_id).await {
            Ok(before) => before,
            Err(error) => {
                let status = json!({"commandId":command,"state":"failed","tool":name,"error":{"code":error.code,"message":error.message,"dispatched":false,"cause":error},"actionId":command});
                self.record_command(
                    &reference.session_id,
                    command.into(),
                    signature,
                    status.clone(),
                )?;
                return terminal(status);
            }
        };
        let commands = if before.agrees(request) {
            Vec::new()
        } else {
            network::plan(request, &before)
        };
        let dispatched = !commands.is_empty();
        let result = if before.agrees(request) {
            Ok((before.clone(), true, 0))
        } else {
            network::android::dispatch(&adapter, &target.platform_id, &commands, request).await
        };
        let status = match result {
            Ok((after, agreed, settle_ms)) => {
                let outcome = network::android::Outcome {
                    before,
                    after,
                    changed: dispatched,
                    agreed,
                    settle_ms,
                };
                network_status(command, name, &target, request, &link, &outcome)
            }
            Err(error) => {
                json!({"commandId":command,"state":if dispatched {"outcome_unknown"} else {"failed"},"tool":name,"error":{"code":if dispatched {"NETWORK_OUTCOME_UNKNOWN"} else {error.code.as_str()},"message":error.message,"dispatched":dispatched,"cause":error},"actionId":command})
            }
        };
        self.record_command(
            &reference.session_id,
            command.into(),
            signature,
            status.clone(),
        )?;
        terminal(status)
    }
}

/// The journal status for an applied request: completed when the readback
/// agreed, otherwise a failed status whose error carries the unsettled readback.
fn network_status(
    command: &str,
    name: &str,
    target: &crate::targets::Target,
    request: Request,
    link: &str,
    outcome: &network::android::Outcome,
) -> Value {
    let mut result = json!({
        "mode":request.mode,
        "method":request.method,
        "changed":outcome.changed,
        "agreed":outcome.agreed,
        "before":outcome.before.to_json(),
        "after":outcome.after.to_json(),
        "settleMs":outcome.settle_ms,
        "timeoutMs":request.timeout_ms,
        "sideEffects":network::side_effects(&outcome.before, &outcome.after),
        "link":link,
        "targetId":target.identity.target_id,
        "generation":target.identity.generation,
        "observedAt":network::utc(),
        "actionId":command,
    });
    if outcome.agreed {
        return json!({"commandId":command,"state":"completed","tool":name,"result":result});
    }
    result["uncertain"] = json!(true);
    let error = Error::new(
        "NETWORK_UNSETTLED",
        format!(
            "The readback did not reach {} within {} ms; inspect the readback and request again",
            match request.mode {
                network::Mode::Offline => "offline",
                network::Mode::Online => "online",
            },
            request.timeout_ms
        ),
    );
    json!({"commandId":command,"state":"failed","tool":name,"error":{"code":error.code,"message":error.message,"dispatched":outcome.changed,"result":result},"actionId":command})
}

fn not_admitted(error: Error) -> Error {
    let cause = error.details.clone();
    error.details(json!({"admitted":false,"dispatched":false,"cause":cause}))
}
fn terminal(status: Value) -> Result<Value> {
    if status["state"] == "completed" {
        return Ok(status["result"].clone());
    }
    Err(Error::new(
        status["error"]["code"]
            .as_str()
            .unwrap_or("NETWORK_OUTCOME_UNKNOWN"),
        status["error"]["message"]
            .as_str()
            .unwrap_or("Network operation has no terminal outcome; inspect status"),
    )
    .details(status))
}
