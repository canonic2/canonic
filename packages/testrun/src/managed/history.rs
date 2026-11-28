//! Durable action lookup and target status without restoring input authority.
use super::{Caller, Receipt, hash, presentation, storage};
use crate::error::{Error, Result};
use serde_json::{Value, json};
use std::{path::Path, time::Duration};

impl Caller {
    pub(super) fn last_action(&self, directory: &Path) -> Result<Value> {
        let Some(receipt) = storage::read::<Receipt>(&directory.join("last-action.json"))? else {
            return Ok(Value::Null);
        };
        Ok(self.receipt_status(&receipt))
    }
    /// The last action without its images, for replies whose own fresh
    /// observation is the current screen. Status keeps the full record.
    pub(super) fn last_action_summary(&self, directory: &Path) -> Result<Value> {
        let mut action = self.last_action(directory)?;
        without_images(&mut action);
        Ok(action)
    }
    pub(super) fn receipt_status(&self, receipt: &Receipt) -> Value {
        let archive = crate::qa::journal::Archive::at(&receipt.connection.directory.join("s"));
        let mut status=archive.status(&receipt.command).unwrap_or_else(|_|json!({"status":"outcome_unknown","reason":"No complete durable outcome is available"}));
        status["actionId"] = json!(receipt.command);
        status
    }
    pub(super) fn history(&self, directory: &Path) -> Result<Vec<Value>> {
        let root = directory.join("receipts");
        if !root.exists() {
            return Ok(Vec::new());
        }
        crate::artifacts::check_private_directory(&root)?;
        let mut receipts = Vec::new();
        for entry in std::fs::read_dir(&root)
            .map_err(|_| Error::new("STATE_IO", "Cannot read action history"))?
            .take(10000)
        {
            let entry = entry.map_err(|_| Error::new("STATE_IO", "Cannot read action history"))?;
            if entry.path().extension().is_some_and(|s| s == "json")
                && let Some(receipt) = storage::read::<Receipt>(&entry.path())?
            {
                receipts.push(receipt);
            }
        }
        receipts.sort_by_key(|r| std::cmp::Reverse(r.created));
        Ok(receipts
            .iter()
            .take(8)
            .map(|receipt| self.receipt_status(receipt))
            .collect())
    }
    /// Status of this request's target or, without one, of every connected
    /// target: the only one's status, or each one's beside the others.
    pub(super) async fn status(&self, directory: &Path, args: Value) -> Result<Value> {
        if self.target.is_some() || args["actionId"].is_string() {
            let page = canonic_mcp_kit::page::Request::from_mcp(
                &args,
                canonic_mcp_kit::page::Limits::STANDARD,
            )?;
            if page.cursor.is_some() {
                return Err(canonic_mcp_kit::page::Error::InvalidCursor.into());
            }
        }
        if self.target.is_some() {
            return self.target_status(directory, args).await;
        }
        if args["actionId"].is_string() {
            // Action IDs are unique: find the record in whichever target's
            // records have it, open or closed.
            for records in super::selection::target_directories(&self.pool_records()?) {
                if let Ok(found) = self.target_status(&records, args.clone()).await {
                    return Ok(found);
                }
            }
            return self.target_status(directory, args).await;
        }
        self.status_page(directory, &args).await
    }
    async fn status_page(&self, directory: &Path, args: &Value) -> Result<Value> {
        use canonic_mcp_kit::page::{Builder, Limits, Position};
        let scope = crate::pagination::scope("status", &(directory, &self.pool))?;
        let (request, after) = crate::pagination::request(args, &scope)?;
        let mut targets = self.pool_targets()?;
        targets.sort_by_cached_key(|target| crate::pagination::identity_key(target));
        let total = targets.len();
        let mut rows = targets.into_iter().filter(|id| {
            after.as_ref().is_none_or(|after| {
                crate::pagination::identity_key(id).as_slice() > after.as_slice()
            })
        });
        let mut builder = Builder::new(
            &scope,
            request.limit,
            Limits::STANDARD,
            crate::protocol::MAX_REPLY,
            "status with targetId",
        )?;
        let mut ended = false;
        while !builder.is_full() {
            let Some(target) = rows.next() else {
                ended = true;
                break;
            };
            let caller = self.with_target(&target);
            let status = caller
                .target_status(&caller.directory()?, json!({}))
                .await
                .unwrap_or_else(|error| json!({"targetId":target,"error":error}));
            if !builder.push(
                status,
                &Position::Keyset(crate::pagination::identity_key(&target)),
            )? {
                break;
            }
        }
        let page = builder.finish(!ended && rows.size_hint().1 != Some(0));
        Ok(
            json!({"state":if total==0{"not_connected"}else{"connected"},"targets":page.items,"total":total,"nextCursor":page.next_cursor,"nextAction":"observe"}),
        )
    }
    async fn target_status(&self, directory: &Path, args: Value) -> Result<Value> {
        if let Some(reference) = args["actionId"].as_str() {
            if reference.starts_with("app-") {
                return Ok(
                    json!({"appOperation":crate::applications::inspect(&directory.join("applications"),reference).await?}),
                );
            }
            let receipt: Receipt = storage::read(
                &directory
                    .join("receipts")
                    .join(format!("{}.json", hash(reference))),
            )?
            .ok_or_else(|| {
                Error::new(
                    "OUTCOME_UNKNOWN",
                    "This action record is unavailable; no replay is safe",
                )
            })?;
            return Ok(json!({"action":self.receipt_status(&receipt)}));
        }
        let connection = match self.selected(directory) {
            Ok(c) => c,
            Err(error) if error.code == "NOT_CONNECTED" => {
                return Ok(
                    json!({"state":"disconnected","nextAction":"connect","lastAppOperation":crate::applications::last(&directory.join("applications"))?,"lastMediaOperation":self.last_media(directory)?,"lastNetworkOperation":self.last_network(directory)?}),
                );
            }
            Err(error) => return Err(error),
        };
        let state = if connection.held()? {
            match tokio::time::timeout(
                Duration::from_secs(2),
                Self::send(&connection, json!({"kind":"status"})),
            )
            .await
            {
                Ok(Ok(status)) => status,
                _ => {
                    let state = storage::read::<Value>(&connection.directory.join("state.json"))?;
                    match state {
                        Some(state)
                            if state["state"] == "failed"
                                && state["error"]["code"] == "CLEANUP_FAILED" =>
                        {
                            json!({"state":"failed","closed":false,"cleanupError":state["error"]})
                        }
                        state => {
                            json!({"state":"recovering","phase":state.as_ref().map(|s|&s["state"]),"message":"Testrun is restoring the connection; no input was repeated"})
                        }
                    }
                }
            }
        } else {
            stopped_owner_state(&connection.directory)?
        };
        let mut result = json!({"state":state["state"],"targetId":connection.target,"health":state,"capabilities":state["target"]["capabilities"],"controlPath":connection.control_path.as_deref().unwrap_or("native"),"profile":"fast","lastAction":self.last_action(directory)?,"lastAppOperation":crate::applications::last(&directory.join("applications"))?,"history":self.history(directory)?,"nextAction":if connection.held()? {"observe"}else{"connect"}});
        result["lastMediaOperation"] = self.last_media(directory)?;
        result["lastNetworkOperation"] = self.last_network(directory)?;
        result["cleanup"] = json!(storage::read::<Value>(
            &connection.directory.join("cleanup.json")
        )?);
        if connection.require_build().is_err() && connection.held()? {
            result["upgradeRequired"] = json!(true);
            result["nextAction"] = json!("connect");
        }
        if result["health"]["reason"] == "RUNNER_EXITED"
            || result["state"] == "closing"
            || result["state"] == "failed" && result["cleanup"]["complete"] == false
        {
            result["nextAction"] = json!("disconnect");
        }
        Ok(result)
    }
    /// The pool a status describes: live in the manager, or the durable
    /// record read offline, which is never reported as live. A target whose
    /// failed release the manager is retrying says so.
    pub(super) fn pool_status(&self, result: &mut Value) {
        match &self.service {
            Some(service) => {
                if let Some(target) = result["targetId"].as_str()
                    && let Some(release) = service.pending_release(target)
                {
                    result["state"] = json!("releasing");
                    result["release"] = json!({"attempts":release.attempts,"running":release.running,"error":release.error,"retrying":true});
                    result["nextAction"] = json!("connect");
                }
                if let Some(pool) = service.pool_summary() {
                    result["manager"] = pool;
                }
            }
            None => {
                result["owner"] = json!("absent");
                if let Ok(directory) = self.pool_records()
                    && let Ok(Some(mut record)) =
                        storage::read::<Value>(&directory.join("pool.json"))
                {
                    record["live"] = json!(false);
                    result["manager"] = record;
                }
                if matches!(
                    result["state"].as_str(),
                    Some("ready" | "recovering" | "connected")
                ) {
                    result["state"] = json!("lost");
                    result["nextAction"] = json!("connect");
                }
            }
        }
    }
}

/// The saved state of a connection whose device actor is no longer running.
/// Cleanup that recovery reconciled after the actor was lost is closed.
fn stopped_owner_state(directory: &Path) -> Result<Value> {
    let reconciled = storage::read::<Value>(&directory.join("cleanup.json"))?
        .is_some_and(|cleanup| cleanup["complete"] == true && cleanup.get("reconciled").is_some());
    Ok(
        match storage::read::<Value>(&directory.join("state.json"))? {
            Some(state) if state["state"] == "closed" || reconciled => {
                json!({"state":"closed","closed":true})
            }
            Some(state) if state["state"] == "failed" => {
                let error = &state["error"];
                let failure = presentation::error(
                    Error::new(
                        error["code"].as_str().unwrap_or("CONNECTION_FAILED"),
                        error["message"]
                            .as_str()
                            .unwrap_or("The previous connection failed"),
                    )
                    .details(error["details"].clone()),
                );
                json!({"state":"failed","message":failure.message,"failure":failure})
            }
            _ => {
                json!({"state":"lost","message":"The device actor stopped unexpectedly; reconnect for recovery"})
            }
        },
    )
}

/// Removes step and final images, keeping each step's outcome.
fn without_images(value: &mut Value) {
    match value {
        Value::Object(object) => {
            if object
                .get("observation")
                .is_some_and(|value| !value.is_null())
            {
                object.remove("observation");
            }
            for item in object.values_mut() {
                without_images(item);
            }
        }
        Value::Array(items) => items.iter_mut().for_each(without_images),
        _ => {}
    }
}

#[cfg(test)]
mod image_tests {
    #[test]
    fn action_summaries_drop_images_but_keep_outcomes() {
        let mut action = serde_json::json!({"status":"completed","actionId":"a","observation":{"attachment":{"path":"/tmp/old.jpeg"}},"steps":[{"tool":"tap","ok":true,"observation":{"attachment":{"path":"/tmp/step.jpeg"}}}]});
        super::without_images(&mut action);
        assert_eq!(
            action,
            serde_json::json!({"status":"completed","actionId":"a","steps":[{"tool":"tap","ok":true}]})
        );
        let mut unknown =
            serde_json::json!({"status":"outcome_unknown","observation":null,"steps":[]});
        let original = unknown.clone();
        super::without_images(&mut unknown);
        assert_eq!(unknown, original);
    }
}
