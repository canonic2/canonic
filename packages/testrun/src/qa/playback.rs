//! Playback workflow shares target leases and durable command outcomes with UI operations.
use super::{Server, arguments::session_ref};
use crate::error::{Error, Result};
use serde_json::{Value, json};

impl Server {
    async fn prepare_playback(
        &mut self,
        reference: &super::arguments::SessionRef,
        target: &crate::targets::Target,
        name: &str,
        args: &Value,
    ) -> Result<()> {
        #[cfg(target_os = "macos")]
        let preparation = if target.kind == "device" {
            let source = self.session(reference)?.video.clone();
            if name.ends_with("audio_probe") {
                match self
                    .device_media
                    .existing(&target.platform_id, &json!(target.identity))
                {
                    Ok(existing) => self.playback.prepare_audio(
                        &reference.session_id,
                        &target.platform_id,
                        existing.or(source),
                    ),
                    Err(error) => Err(error),
                }
            } else {
                self.playback
                    .prepare_ios(&reference.session_id, &target.platform_id, source)
                    .await
            }
        } else {
            self.playback
                .prepare(&reference.session_id, target, name, args)
                .await
        };
        #[cfg(not(target_os = "macos"))]
        let preparation = self
            .playback
            .prepare(&reference.session_id, target, name, args)
            .await;
        preparation.map_err(not_admitted)?;
        let session = self.sessions.get_mut(&reference.session_id).unwrap();
        if target.kind == "android" {
            for (key, details) in [
                ("media_status", json!({"backend":"shell_media_helper"})),
                (
                    "media_send",
                    json!({"backend":"shell_media_helper","destinations":["media_session"],"commands":["play","pause","play_pause","next","previous","stop"],"sessionActionsRequired":true}),
                ),
                (
                    "volume",
                    json!({"backend":"shell_media_helper","destinations":["music_stream","media_session"]}),
                ),
            ] {
                crate::targets::capabilities::support(&mut session.target, key, details);
            }
        } else if !name.ends_with("audio_probe") {
            crate::targets::capabilities::support(
                &mut session.target,
                "media_send",
                json!({"backend":"coredevice_indigo","destinations":["system_player"],"commands":["play","pause","play_pause","next","previous","volume_up","volume_down"]}),
            );
        }
        Ok(())
    }

    pub(crate) async fn playback_request(
        &mut self,
        reference: Value,
        name: &str,
        args: Value,
        command: Option<&str>,
        deadline: tokio::time::Instant,
    ) -> Result<Value> {
        crate::playback::validate(name, &args).map_err(not_admitted)?;
        let lease = reference["leaseId"].as_str().map(String::from);
        let reference = session_ref(&reference)?;
        let target = self.live_target(&reference).await.map_err(not_admitted)?;
        crate::playback::validate_platform(name, &args, target.kind == "android")
            .map_err(not_admitted)?;
        let write = crate::playback::mutation(name);
        let session = self.session(&reference)?;
        if write {
            session
                .authority()
                .admit(lease.as_deref().unwrap_or(""), std::time::Instant::now())
                .map_err(not_admitted)?;
        }
        if !session.control {
            return Err(not_admitted(Error::new(
                "CONTROL_NOT_OWNED",
                "Media operations require the selected controller",
            )));
        }
        let signature = json!({"tool":name,"arguments":args});
        if write {
            let command = command.ok_or_else(|| {
                Error::invalid("Media mutation requires an internal command identity")
            })?;
            if let Some((previous, status)) = session.journal.get(command) {
                if previous != &signature {
                    return Err(Error::new(
                        "COMMAND_ID_CONFLICT",
                        "Media command already has different arguments",
                    ));
                }
                return terminal(status.clone());
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
        }
        self.prepare_playback(&reference, &target, name, &args)
            .await?;
        if !write {
            let mut result = self
                .playback
                .call(&reference.session_id, name, &args)
                .await?;
            crate::playback::stamp(&mut result, &target);
            return Ok(result);
        }
        let command = command.unwrap();
        self.record_command(
            &reference.session_id,
            command.into(),
            signature.clone(),
            json!({"commandId":command,"state":"dispatched","tool":name}),
        )?;
        let result = if name.ends_with("audio_probe") {
            self.playback
                .probe(
                    &reference.session_id,
                    &args,
                    crate::playback::probe::Context {
                        target: target.clone(),
                        command: command.into(),
                        deadline,
                    },
                )
                .await
        } else {
            self.playback.call(&reference.session_id, name, &args).await
        };
        let status = outcome(command, name, &target, result);
        self.record_command(
            &reference.session_id,
            command.into(),
            signature,
            status.clone(),
        )?;
        terminal(status)
    }
}
fn outcome(
    command: &str,
    name: &str,
    target: &crate::targets::Target,
    result: Result<Value>,
) -> Value {
    match result {
        Ok(mut result) => {
            crate::playback::stamp(&mut result, target);
            result["actionId"] = json!(command);
            json!({"commandId":command,"state":"completed","tool":name,"observed":result["observed"],"result":result})
        }
        Err(error) => {
            let refused = error
                .details
                .as_ref()
                .is_some_and(|v| v["dispatched"] == false);
            let probe = name.ends_with("audio_probe");
            let code = if refused || probe {
                error.code.as_str()
            } else {
                "MEDIA_OUTCOME_UNKNOWN"
            };
            json!({"commandId":command,"state":if refused||probe{"failed"}else{"outcome_unknown"},"tool":name,"error":{"code":code,"message":error.message,"cause":error},"actionId":command})
        }
    }
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
            .unwrap_or("MEDIA_OUTCOME_UNKNOWN"),
        status["error"]["message"]
            .as_str()
            .unwrap_or("Media operation has no terminal outcome; inspect status"),
    )
    .details(status))
}
