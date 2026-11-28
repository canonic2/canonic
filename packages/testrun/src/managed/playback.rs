//! Managed playback receipts bind native commands to the existing controller archive.
use super::{Caller, Receipt, hash, storage};
use crate::error::{Error, Result};
use serde_json::{Value, json};
use std::path::Path;

impl Caller {
    pub(super) fn last_media(&self, directory: &Path) -> Result<Value> {
        Ok(
            storage::read::<Receipt>(&directory.join("last-media.json"))?
                .map(|r| self.receipt_status(&r))
                .unwrap_or(Value::Null),
        )
    }
    fn recorded_media(&self, previous: Receipt, name: &str, args: &Value) -> Result<Value> {
        let signature = hash(
            json!({"target":previous.connection.target,"tool":name,"arguments":args}).to_string(),
        );
        if previous.signature != signature {
            return Err(Error::new(
                "COMMAND_ID_CONFLICT",
                "This request already has different media arguments",
            ));
        }
        let status = self.receipt_status(&previous);
        if status["state"] == "completed" {
            return Ok(status["result"].clone());
        }
        if status["state"] == "failed" {
            return Err(Error::new(
                status["error"]["code"]
                    .as_str()
                    .unwrap_or("MEDIA_HELPER_FAILED"),
                status["error"]["message"]
                    .as_str()
                    .unwrap_or("The original media request failed; it was not repeated"),
            )
            .details(status));
        }
        Err(Error::new(
            "MEDIA_OUTCOME_UNKNOWN",
            "This media request was already submitted; it was not repeated",
        )
        .details(status))
    }

    pub(super) async fn playback(
        &mut self,
        directory: &Path,
        name: &str,
        args: Value,
        deadline: tokio::time::Instant,
    ) -> Result<Value> {
        crate::playback::validate(name, &args)?;
        let command = format!(
            "media-{}",
            self.request_id
                .take()
                .map_or_else(|| uuid::Uuid::new_v4().simple().to_string(), hash)
        );
        let receipts = directory.join("receipts");
        if crate::playback::mutation(name) {
            storage::directory(&receipts)?;
        }
        let path = receipts.join(format!("{}.json", hash(&command)));
        if crate::playback::mutation(name)
            && let Some(previous) = storage::read::<Receipt>(&path)?
        {
            return self.recorded_media(previous, name, &args);
        }
        let connection = self.selected(directory).map_err(|e| {
            if e.code == "NOT_CONNECTED" {
                Error::new(
                    "SESSION_NOT_FOUND",
                    "Connect to an explicit target before media operations",
                )
            } else {
                e
            }
        })?;
        crate::playback::validate_platform(name, &args, connection.target.starts_with("android:"))?;
        if !connection.held()? {
            return Err(Error::new(
                "SESSION_NOT_FOUND",
                "The selected controller is no longer active; reconnect before media operations",
            ));
        }
        connection.admit_work()?;
        let deadline_unix_ms = (std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            + deadline.saturating_duration_since(tokio::time::Instant::now()))
        .as_millis() as u64;
        if !crate::playback::mutation(name) {
            return Self::send(
                &connection,
                json!({"kind":"playback","tool":name,"arguments":args,"deadlineUnixMs":deadline_unix_ms}),
            )
            .await;
        }
        let signature =
            hash(json!({"target":connection.target,"tool":name,"arguments":args}).to_string());
        storage::directory(&receipts)?;
        let receipt = Receipt {
            connection: connection.clone(),
            command: command.clone(),
            created: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos() as u64,
            signature,
        };
        storage::write(&path, &receipt)?;
        let previous_last = storage::read::<Receipt>(&directory.join("last-media.json"))?;
        storage::write(&directory.join("last-media.json"), &receipt)?;
        let request = json!({"kind":"playback","requestedLeaseId":self.input_lease,"tool":name,"arguments":args,"commandId":command,"deadlineUnixMs":deadline_unix_ms});
        let result = Self::send(&connection, request).await;
        if result
            .as_ref()
            .is_err_and(|e| e.details.as_ref().is_some_and(|d| d["admitted"] == false))
        {
            std::fs::remove_file(path)
                .map_err(|_| Error::new("STATE_IO", "Cannot discard refused media receipt"))?;
            if let Some(previous) = previous_last {
                storage::write(&directory.join("last-media.json"), &previous)?;
            } else {
                std::fs::remove_file(directory.join("last-media.json"))
                    .map_err(|_| Error::new("STATE_IO", "Cannot discard refused media summary"))?;
            }
            return result;
        }
        match result {
            Ok(value)=>Ok(value),
            Err(error)=>Err(Error::new(if error.code=="TRANSPORT_IO" || error.code=="SESSION_ENDPOINT_LOST" {"MEDIA_OUTCOME_UNKNOWN"} else {&error.code},error.message.clone())
                .details(json!({"actionId":command,"cause":error,"mediaOperation":self.receipt_status(&receipt)}))),
        }
    }
}
