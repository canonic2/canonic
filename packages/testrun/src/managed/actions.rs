//! Managed observation and action workflows with durable receipts and no replay.
use super::{Caller, Connection, Receipt, hash, presentation, storage, validation};
use crate::error::{Error, Result};
use serde_json::{Value, json};
use std::{path::Path, time::Duration};

impl Caller {
    pub(super) async fn observe(
        &self,
        directory: &Path,
        args: Value,
        deadline: tokio::time::Instant,
    ) -> Result<Value> {
        let connection = self.selected(directory)?;
        let mut input = json!({"kind":"observe"});
        if let Some(profile) = args.get("profile") {
            input["profile"] = profile.clone();
        }
        let observation = match Self::send(&connection, input).await {
            Ok(image) => image,
            Err(error)
                if matches!(
                    error.code.as_str(),
                    "SESSION_NOT_FOUND"
                        | "TRANSPORT_IO"
                        | "CAPTURE_STALLED"
                        | "VERSION_MISMATCH"
                        | "SESSION_CLOSING"
                ) =>
            {
                let _queue = self.queue(Some(deadline)).await?;
                if self.selected(directory)?.epoch != connection.epoch {
                    return Err(Error::new(
                        "STALE_OWNER",
                        "Selection changed while observation was interrupted; no recovery or input was replayed",
                    ));
                }
                // A disconnect that stopped this owner must not be undone by a read;
                // other lost endpoints recover as before.
                if disconnected(&connection)? {
                    return Err(Error::new(
                        "SESSION_CLOSING",
                        "The device was disconnected while this observation waited; it was not reconnected",
                    )
                    .details(json!({"admitted":false,"dispatched":false})));
                }
                let recovered = self.reconnect(directory, &connection).await?;
                if let Some(profile) = args.get("profile") {
                    Self::send(
                        &self.selected(directory)?,
                        json!({"kind":"observe","profile":profile}),
                    )
                    .await?
                } else {
                    recovered["observation"].clone()
                }
            }
            Err(error) => return Err(error),
        };
        Ok(
            json!({"state":"ready","observation":observation,"lastAction":self.last_action_summary(directory)?}),
        )
    }
    pub(super) async fn act(&mut self, directory: &Path, args: Value) -> Result<Value> {
        let steps = validation::steps(&args).map_err(presentation::validation_error)?;
        let connection = self.selected(directory)?;
        let command = self
            .request_id
            .take()
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let signature = hash(json!({"target":connection.target,"actions":args}).to_string());
        let receipts = directory.join("receipts");
        storage::directory(&receipts)?;
        let request_path = receipts.join(format!("{}.json", hash(&command)));
        if let Some(previous) = storage::read::<Receipt>(&request_path)? {
            if previous.signature != signature {
                return Err(Error::new(
                    "COMMAND_ID_CONFLICT",
                    "This logical request already has different actions",
                ));
            }
            let status = crate::qa::journal::Archive::at(&previous.connection.directory.join("s"))
                .status(&previous.command)
                .unwrap_or(
                    json!({"status":"outcome_unknown","reason":"No durable terminal outcome"}),
                );
            if status["status"] != "completed" {
                return Err(Error::new(
                    if status["status"] == "failed" {
                        "BATCH_FAILED"
                    } else {
                        "BATCH_OUTCOME_UNKNOWN"
                    },
                    "The previous request remains unresolved or failed; it was not repeated",
                )
                .details(json!({"replayed":false,"lastAction":status})));
            }
            return Ok(json!({"state":"recorded","replayed":false,"result":status}));
        }
        // Resolve existing receipts first: stricter admission rules cannot
        // reclassify a previously admitted request as proven non-dispatch.
        // Unfinished cleanup refuses new input with its blocker; only
        // recovery resumes it, never new work replacing the device actor.
        connection.admit_work()?;
        let kind = if connection.target.starts_with("device:") {
            "device"
        } else if connection.target.starts_with("android:") {
            "android"
        } else {
            "simulator"
        };
        let chords = kind == "android"
            && storage::read::<Value>(&connection.directory.join("state.json"))?.is_some_and(
                |state| crate::targets::capabilities::android_chords(&state["capabilities"]),
            );
        validation::backend_options(&steps, kind, connection.control_path.as_deref(), chords)
            .map_err(presentation::validation_error)?;
        let receipt = Receipt {
            connection: connection.clone(),
            command: command.clone(),
            created: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos() as u64,
            signature,
        };
        storage::write(&request_path, &receipt)?;
        storage::write(&directory.join("last-action.json"), &receipt)?;
        let output=Self::send(&connection,json!({"kind":"batch","requestedLeaseId":self.input_lease,"commandId":command,"steps":steps,"observe":true,"observeAfterEach":args["observeAfterEach"]!=false,"observationProfile":args.get("profile").cloned().unwrap_or(json!("fast")),"settle":args["settle"]!=false})).await;
        match output {
            Ok(result) => Ok(
                json!({"state":"ready","result":result,"lastAction":self.last_action_summary(directory)?}),
            ),
            Err(error) => {
                let mut observation = tokio::time::timeout(
                    Duration::from_secs(6),
                    Self::send(&connection, json!({"kind":"observe"})),
                )
                .await
                .ok()
                .and_then(std::result::Result::ok);
                if matches!(
                    error.code.as_str(),
                    "BATCH_OUTCOME_UNKNOWN"
                        | "SESSION_UNCERTAIN"
                        | "SESSION_NOT_FOUND"
                        | "TRANSPORT_IO"
                        | "BATCH_FAILED"
                ) && !focus_refusal(&error)
                    && let Ok(recovered) = self.reconnect(directory, &connection).await
                {
                    observation = Some(recovered["observation"].clone());
                }
                let partial = error.details.clone();
                Err(error.details(json!({"lastAction":self.last_action(directory)?,"result":partial,"observation":observation})))
            }
        }
    }
}

fn disconnected(connection: &Connection) -> Result<bool> {
    Ok(
        storage::read::<Value>(&connection.directory.join("disconnected.json"))?
            .is_some_and(|stop| stop["epoch"] == connection.epoch),
    )
}

/// A terminal, observed batch with a proven focus refusal needs no new owner.
/// Missing evidence or any uncertain/observation failure retains normal recovery.
fn focus_refusal(error: &Error) -> bool {
    if error.code != "BATCH_FAILED" {
        return false;
    }
    let Some(result) = &error.details else {
        return false;
    };
    let Some(steps) = result["steps"].as_array() else {
        return false;
    };
    let Some((last, previous)) = steps.split_last() else {
        return false;
    };
    let status = &last["error"]["details"];
    result["status"] == "failed"
        && result.get("observationError").is_none()
        && steps
            .iter()
            .all(|step| step.get("observationError").is_none())
        && previous.iter().all(|step| step["ok"] == true)
        && last["ok"] == false
        && status["state"] == "failed"
        && matches!(
            status["error"]["code"].as_str(),
            Some("FOCUS_VERIFICATION_UNAVAILABLE" | "FOCUS_NOT_PROVEN")
        )
        && status["error"]["details"]["stage"] == "focus_verification"
        && status["error"]["details"]["inputDispatched"] == false
}
