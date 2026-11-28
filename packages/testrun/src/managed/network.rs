//! Managed network control: receipts for `network_set` and the device's last
//! network operation.
use super::{Caller, Receipt, hash, storage};
use crate::error::{Error, Result};
use serde_json::{Value, json};
use std::path::Path;

impl Caller {
    pub(super) fn last_network(&self, directory: &Path) -> Result<Value> {
        Ok(
            storage::read::<Receipt>(&directory.join("last-network.json"))?
                .map(|r| self.receipt_status(&r))
                .unwrap_or(Value::Null),
        )
    }
    fn recorded_network(&self, previous: Receipt, name: &str, args: &Value) -> Result<Value> {
        let signature = hash(
            json!({"target":previous.connection.target,"tool":name,"arguments":args}).to_string(),
        );
        if previous.signature != signature {
            return Err(Error::new(
                "COMMAND_ID_CONFLICT",
                "This request already has different network arguments",
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
                    .unwrap_or("NETWORK_OUTCOME_UNKNOWN"),
                status["error"]["message"]
                    .as_str()
                    .unwrap_or("The original network request failed; it was not repeated"),
            )
            .details(status));
        }
        Err(Error::new(
            "NETWORK_OUTCOME_UNKNOWN",
            "This network request was already submitted; it was not repeated",
        )
        .details(status))
    }

    pub(super) async fn network(
        &mut self,
        directory: &Path,
        name: &str,
        args: Value,
        deadline: tokio::time::Instant,
    ) -> Result<Value> {
        crate::network::validate(name, &args)?;
        let command = format!(
            "net-{}",
            self.request_id
                .take()
                .map_or_else(|| uuid::Uuid::new_v4().simple().to_string(), hash)
        );
        let receipts = directory.join("receipts");
        storage::directory(&receipts)?;
        let path = receipts.join(format!("{}.json", hash(&command)));
        if let Some(previous) = storage::read::<Receipt>(&path)? {
            return self.recorded_network(previous, name, &args);
        }
        let connection = self.selected(directory).map_err(|e| {
            if e.code == "NOT_CONNECTED" {
                Error::new(
                    "SESSION_NOT_FOUND",
                    "Connect to an explicit Android target before network control",
                )
            } else {
                e
            }
        })?;
        if !connection.target.starts_with("android:") {
            return Err(Error::new(
                "CAPABILITY_UNAVAILABLE",
                "Network control is available on Android targets only",
            ));
        }
        if !connection.held()? {
            return Err(Error::new(
                "SESSION_NOT_FOUND",
                "The selected controller is no longer active; reconnect before network control",
            ));
        }
        connection.admit_work()?;
        let deadline_unix_ms = (std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            + deadline.saturating_duration_since(tokio::time::Instant::now()))
        .as_millis() as u64;
        let signature =
            hash(json!({"target":connection.target,"tool":name,"arguments":args}).to_string());
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
        let previous_last = storage::read::<Receipt>(&directory.join("last-network.json"))?;
        storage::write(&directory.join("last-network.json"), &receipt)?;
        let request = json!({"kind":"network","requestedLeaseId":self.input_lease,"tool":name,"arguments":args,"commandId":command,"deadlineUnixMs":deadline_unix_ms});
        let result = Self::send(&connection, request).await;
        if result
            .as_ref()
            .is_err_and(|e| e.details.as_ref().is_some_and(|d| d["admitted"] == false))
        {
            std::fs::remove_file(path)
                .map_err(|_| Error::new("STATE_IO", "Cannot discard refused network receipt"))?;
            if let Some(previous) = previous_last {
                storage::write(&directory.join("last-network.json"), &previous)?;
            } else {
                std::fs::remove_file(directory.join("last-network.json")).map_err(|_| {
                    Error::new("STATE_IO", "Cannot discard refused network summary")
                })?;
            }
            return result;
        }
        match result {
            Ok(value) => Ok(value),
            Err(error) => Err(Error::new(
                if error.code == "TRANSPORT_IO" || error.code == "SESSION_ENDPOINT_LOST" {
                    "NETWORK_OUTCOME_UNKNOWN"
                } else {
                    &error.code
                },
                error.message.clone(),
            )
            .details(
                json!({"actionId":command,"cause":error,"networkOperation":self.receipt_status(&receipt)}),
            )),
        }
    }
}
