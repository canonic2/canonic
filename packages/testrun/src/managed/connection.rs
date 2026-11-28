//! Device actor acquisition, reuse, bounded recovery and disconnection workflows.
use super::{Caller, Connection, Receipt, hash, now, storage};
use crate::{
    error::{Error, Result},
    lease::ControlLease,
};
use serde_json::{Value, json};
use std::{path::Path, time::Duration};

impl Caller {
    pub(super) async fn send(connection: &Connection, mut input: Value) -> Result<Value> {
        if input["kind"] != "status" && input["kind"] != "close" {
            connection.require_build()?;
        }
        input["ownerEpoch"] = json!(connection.epoch);
        let response = crate::interactive::request(&connection.socket(), &input).await?;
        if response["ok"] == true {
            Ok(response["data"].clone())
        } else {
            Err(Error::new(
                response["error"]["code"]
                    .as_str()
                    .unwrap_or("SESSION_UNAVAILABLE"),
                response["error"]["message"]
                    .as_str()
                    .unwrap_or("Session is unavailable"),
            )
            .details(response["error"]["details"].clone()))
        }
    }
    /// Connect, retrying a recoverable startup once. A failure that leaves no
    /// device actor holding the device takes it out of the pool, so other
    /// targets stay unambiguous; missing developer support keeps it, since
    /// preparation runs on the pooled target. A connect that finds an earlier
    /// failed release retries it first; when that succeeds the release is no
    /// longer pending, and when it fails the error names the blocker.
    pub(super) async fn connect_retry(&self, directory: &Path, args: Value) -> Result<Value> {
        let result = self.connect_attempts(directory, args).await;
        let (Some(service), Some(_), Some(target)) = (&self.service, &self.pool, &self.target)
        else {
            return result;
        };
        let held = self
            .selected(directory)
            .is_ok_and(|connection| connection.held().unwrap_or(false));
        match &result {
            Ok(_) => service.release_settled(target),
            Err(error) if error.code == "DEVELOPER_SUPPORT_REQUIRED" => {}
            Err(_) if !held => service.release_target(target),
            Err(_) => {}
        }
        result
    }
    async fn connect_attempts(&self, directory: &Path, args: Value) -> Result<Value> {
        match self.connect(directory, args.clone()).await {
            Err(error) if error.code == "DEVELOPER_SUPPORT_REQUIRED" => {
                if let Ok(connection) = self.selected(directory) {
                    self.wait_release(&connection).await?;
                }
                Err(error)
            }
            Err(error)
                if matches!(
                    error.code.as_str(),
                    "CAPTURE_STALLED" | "SESSION_ENDPOINT_CONFLICT"
                ) || error.code == "DEVICE_MEDIA_UNAVAILABLE" && error.retryable =>
            {
                self.connect(directory, args).await
            }
            result => result,
        }
    }
    pub(super) async fn connect(&self, directory: &Path, args: Value) -> Result<Value> {
        let mut desired = self.desired(directory, &args)?;
        let target = desired.target.clone();
        // The target joins the pool before any device work; other targets stay.
        if let (Some(service), Some(_)) = (&self.service, &self.pool) {
            service.claim(&target)?;
        }
        let previous = match self.selected(directory) {
            Ok(connection) => Some(connection),
            Err(error) if error.code == "NOT_CONNECTED" => None,
            Err(error) => return Err(error),
        };
        if let Some(previous) = &previous {
            desired.resolve_capture(
                previous,
                args.get("androidCapture")
                    .is_some_and(|value| !value.is_null()),
                previous.held()?,
            )?;
            if previous.task != desired.task {
                return Err(Error::new(
                    "STATE_IO",
                    "Saved ownership does not belong to this task",
                ));
            }
            if !previous.compatible(&desired) && previous.held()? {
                self.close_owner(previous).await?;
            }
            if previous.compatible(&desired) && previous.held()? {
                let previous_state =
                    storage::read::<Value>(&previous.directory.join("state.json"))?;
                if previous_state
                    .as_ref()
                    .is_some_and(|state| state["state"] == "opening")
                {
                    // A slow start, such as an emulator boot that outlasts one
                    // connect deadline, continues in its owner; await it rather
                    // than replacing it.
                    return self.await_startup(directory, previous).await;
                }
                let status = Self::send(previous, json!({"kind":"status"})).await;
                let interrupted = storage::read::<Receipt>(&directory.join("last-action.json"))?
                    .is_some_and(|receipt| {
                        receipt.connection.epoch == previous.epoch
                            && self.receipt_status(&receipt)["status"] == "outcome_unknown"
                    });
                if !interrupted
                    && status.as_ref().is_ok_and(|s| {
                        s["state"] == "ready"
                            && s["inputHealth"]["uncertain"] != true
                            && s["inputHealth"]["videoFresh"] != false
                    })
                {
                    // Startup may finish after the first caller's deadline.
                    // Reconcile its alias when reusing an already-ready actor.
                    if let Some(state) = previous_state.as_ref() {
                        self.record_transition(previous, state)?;
                    }
                    // An app that blocks screenshots leaves the connection usable.
                    let observation = match Self::send(previous, json!({"kind":"observe"})).await {
                        Ok(observation) => observation,
                        Err(error) if error.code == "CAPTURE_BLOCKED" => {
                            json!({"observationError":error})
                        }
                        Err(error) => return Err(error),
                    };
                    let resolved = previous_state
                        .as_ref()
                        .and_then(|s| s["targetId"].as_str())
                        .unwrap_or(&target);
                    let mut ready = json!({"state":"ready","targetId":resolved,"reused":true,"observation":observation,"capabilities":status.as_ref().ok().map(|s| &s["target"]["capabilities"]),"controlPath":status.as_ref().ok().map(|s| &s["controlPath"]),"profile":"fast","lastAction":self.last_action_summary(directory)?});
                    ready["androidCapture"] = status
                        .as_ref()
                        .ok()
                        .map_or(Value::Null, |status| status["androidCapture"].clone());
                    return Ok(ready);
                }
                self.close_owner(previous).await?;
            } else if !previous.held()? {
                self.wait_release(previous).await?;
            }
        }
        if ControlLease::inspect(&desired.authority())?.is_some_and(|(held, _)| held) {
            // Held outside this manager: another user's or data root's
            // manager, or one from an earlier protocol.
            let mut error = Error::new(
                "DEVICE_IN_USE",
                "Another Testrun manager holds this device; testrun manager blockers reports it",
            )
            .details(json!({"admitted":false,"dispatched":false,"blockingState":"external_owner"}));
            error.retryable = true;
            return Err(error);
        }
        // The manager may have begun exiting while this waited: the actor
        // starts only while it is open, with its records written first so
        // the exit finds it.
        let service = self.service()?;
        service.start_if_open(|| {
            storage::directory(&desired.directory)?;
            storage::write(&desired.directory.join("owner.json"), &desired)?;
            storage::write(&directory.join("selected.json"), &desired)?;
            service.supervisor.start(desired.clone())
        })?;
        self.await_startup(directory, &desired).await
    }
    /// Wait for `owner` to report ready or failed.
    async fn await_startup(&self, directory: &Path, owner: &Connection) -> Result<Value> {
        let target = &owner.target;
        // A startup failure is recorded after the owner's cleanup; wait a bounded
        // time for it so the caller sees the cause rather than the cleanup phase.
        let mut settling: Option<tokio::time::Instant> = None;
        loop {
            if let Some(state) = storage::read::<Value>(&owner.directory.join("state.json"))? {
                if state["state"] == "closing" && owner.held()? {
                    let since = *settling.get_or_insert_with(tokio::time::Instant::now);
                    if since.elapsed() < Duration::from_secs(30) {
                        tokio::time::sleep(Duration::from_millis(25)).await;
                        continue;
                    }
                }
                if state["state"] == "closing" || state["state"] == "closed" {
                    Self::never_connected(owner)?;
                    return Err(Error::new(
                        "SESSION_CLOSING",
                        "The device actor stopped during connection startup; query status for cleanup",
                    ));
                }
                if state["state"] == "ready" {
                    self.record_transition(owner, &state)?;
                    let mut ready = json!({"state":"ready","targetId":state["targetId"].as_str().unwrap_or(target),"reused":false,"observation":state["observation"],"capabilities":state["capabilities"],"controlPath":state["controlPath"],"profile":state["observationProfile"],"lastAction":self.last_action_summary(directory)?});
                    ready["androidCapture"] = state["androidCapture"].clone();
                    if !state["runnerSetup"].is_null() {
                        ready["runnerSetup"] = state["runnerSetup"].clone();
                    }
                    return Ok(ready);
                }
                if state["state"] == "failed" {
                    Self::never_connected(owner)?;
                    let mut error = Error::new(
                        state["error"]["code"]
                            .as_str()
                            .unwrap_or("CONNECTION_FAILED"),
                        state["error"]["message"]
                            .as_str()
                            .unwrap_or("Connection failed"),
                    )
                    .details(state["error"]["details"].clone());
                    error.retryable = state["error"]["retryable"].as_bool().unwrap_or(false);
                    return Err(error);
                }
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    }
    /// Mark `owner`'s connection disconnected after its startup failed, so a
    /// later call without `targetId` is refused with `NOT_CONNECTED` instead
    /// of treating it as interrupted and reconnecting it.
    fn never_connected(owner: &Connection) -> Result<()> {
        storage::write(
            &owner.directory.join("disconnected.json"),
            &json!({"epoch":owner.epoch}),
        )
    }
    /// A started AVD becomes its emulator's serial target. Record the
    /// transition; both names reach the same pooled device.
    fn record_transition(&self, owner: &Connection, state: &Value) -> Result<()> {
        let Some(to) = state["targetId"].as_str().filter(|to| *to != owner.target) else {
            return Ok(());
        };
        let path = owner.directory.join("transition.json");
        if storage::read::<Value>(&path)?.is_none() {
            storage::write(
                &path,
                &json!({"from":owner.target,"to":to,"epoch":owner.epoch,"at":now()}),
            )?;
        }
        match (&self.service, &self.pool) {
            (Some(service), Some(_)) => service.transition(&owner.target, to),
            _ => Ok(()),
        }
    }
    pub(super) async fn wait_release(&self, connection: &Connection) -> Result<()> {
        while connection.held()? {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        if connection.boot
            && (connection.target.starts_with("simulator:")
                || connection
                    .target
                    .starts_with(crate::android::emulator::AVD_PREFIX))
        {
            Box::pin(self.reconcile_lost(connection)).await?;
        }
        // A managed process can die while its device media task is still expiring remotely.
        if let Some((held, record)) = ControlLease::inspect(&connection.authority())?
            && !held
            && record["epoch"] == connection.epoch
            && storage::read::<Value>(&connection.directory.join("state.json"))?
                .is_none_or(|s| s["state"] != "closed")
            && storage::read::<Value>(&connection.directory.join("cleanup.json"))?
                .is_none_or(|s| s["complete"] != true)
            && connection.target.starts_with("device:")
        {
            let marker = connection.directory.join("recovery.json");
            let detected = match storage::read::<u64>(&marker)? {
                Some(time) => time,
                None => {
                    let time = now();
                    storage::write(&marker, &time)?;
                    time
                }
            };
            let remaining = 22_u64.saturating_sub(now().saturating_sub(detected));
            if remaining > 0 {
                tokio::time::sleep(Duration::from_secs(remaining)).await;
            }
            let serial = connection.target.strip_prefix("device:").unwrap();
            let mut remote = crate::device_media::ConnectionOwner::connect(serial).await?;
            let status = remote.status().await?;
            if status["running"] != false
                || status["sessions"]
                    .as_array()
                    .is_none_or(|sessions| !sessions.is_empty())
            {
                return Err(Error::new(
                    "RECOVERING",
                    "Device video is still in use; Testrun will recheck without stopping foreign capture",
                ));
            }
            let path = connection.directory.join("cleanup.json");
            let mut cleanup = storage::read::<Value>(&path)?.unwrap_or(json!({}));
            cleanup["complete"] = json!(true);
            storage::write(&path, &cleanup)?;
        }
        Ok(())
    }
    pub(super) async fn close_owner(&self, connection: &Connection) -> Result<()> {
        self.close_owner_requested(connection, &uuid::Uuid::new_v4().to_string())
            .await
    }
    pub(super) async fn close_owner_requested(
        &self,
        connection: &Connection,
        request: &str,
    ) -> Result<()> {
        // An unreachable endpoint cannot acknowledge admission. Wait for this
        // request's attempt instead of returning the previous failed attempt.
        let previous_attempts = storage::read::<Value>(&connection.directory.join("cleanup.json"))?
            .and_then(|cleanup| cleanup["attempts"].as_u64());
        if connection.held()? {
            // The epoch fence prevents a stale disconnect from closing a replacement owner.
            storage::write(
                &connection.directory.join("stop.json"),
                &json!({"epoch":connection.epoch,"requestId":request}),
            )?;
            // The owner watches this private marker during service and after
            // failure. Never send a close to a possibly replaced socket.
            // A quarantined actor retries cleanup under its own supervisor.
            if let Some(service) = &self.service {
                service.supervisor.retry(&connection.epoch);
            }
        }
        while connection.held()? {
            let cleanup = storage::read::<Value>(&connection.directory.join("cleanup.json"))?;
            let requested_attempt = cleanup.as_ref().is_some_and(|cleanup| {
                cleanup["cleanupRequestId"] == request
                    || cleanup["cleanupRequestId"].is_null()
                        && cleanup["attempts"].as_u64() != previous_attempts
            });
            if let Some(state) = storage::read::<Value>(&connection.directory.join("state.json"))?
                && state["state"] == "failed"
                && matches!(
                    state["error"]["code"].as_str(),
                    Some("CLEANUP_FAILED" | "ACTOR_QUARANTINED")
                )
                && requested_attempt
                && cleanup
                    .as_ref()
                    .is_some_and(|cleanup| cleanup["complete"] == false)
            {
                return Err(Error::new("CLEANUP_FAILED", "Owned cleanup is unfinished; retry disconnect")
                    .details(json!({"cause":state["error"],"cleanup":storage::read::<Value>(&connection.directory.join("cleanup.json"))?})));
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        self.wait_release(connection).await?;
        // A later attempt can complete cleanup after the owner recorded a
        // failure; only unfinished cleanup keeps the connection open.
        let complete = storage::read::<Value>(&connection.directory.join("cleanup.json"))?
            .is_some_and(|cleanup| cleanup["complete"] == true);
        if let Some(state) = storage::read::<Value>(&connection.directory.join("state.json"))?
            && state["state"] == "failed"
            && state["error"]["code"] == "CLEANUP_FAILED"
            && !complete
        {
            return Err(Error::new(
                "CLEANUP_FAILED",
                "Owned cleanup failed; status retains the diagnostic and interrupted action",
            ));
        }
        Ok(())
    }
    pub(super) async fn disconnect(
        &self,
        directory: &Path,
        request: Option<&str>,
    ) -> Result<Value> {
        let mut connection = match self.selected(directory) {
            Ok(connection) => connection,
            Err(error) if error.code == "NOT_CONNECTED" => {
                return Ok(json!({"state":"closed","closed":true}));
            }
            Err(error) => return Err(error),
        };
        if let Some(request) = request {
            let requests = directory.join("disconnects");
            storage::directory(&requests)?;
            let path = requests.join(format!("{}.json", hash(request)));
            if let Some(captured) = storage::read::<Connection>(&path)? {
                connection = captured;
            } else {
                storage::write(&path, &connection)?;
            }
        }
        // Reads waiting on this owner must not reconnect what the task disconnected.
        storage::write(
            &connection.directory.join("disconnected.json"),
            &json!({"epoch":connection.epoch}),
        )?;
        self.close_owner(&connection).await?;
        let current = self.selected(directory)?;
        if current.epoch != connection.epoch && current.held()? {
            return Ok(
                json!({"state":"ready","closed":false,"requestClosed":true,"message":"The requested connection is closed; its replacement was preserved"}),
            );
        }
        let mut closed =
            json!({"state":"closed","closed":true,"lastAction":self.last_action(directory)?});
        closed["cleanup"] = json!(storage::read::<Value>(
            &connection.directory.join("cleanup.json")
        )?);
        Ok(closed)
    }
    /// Disconnect from durable records while no manager describes them:
    /// this request's target, or every target the pool recorded.
    pub(super) async fn disconnect_recorded(&self) -> Result<Value> {
        let request = self.request_id.as_deref();
        if self.target.is_some() {
            return self.disconnect(&self.directory()?, request).await;
        }
        let mut closed = Vec::new();
        // One target's failure does not skip the others; the first is reported.
        let mut failed = None;
        for directory in super::selection::target_directories(&self.pool_records()?) {
            let mut result = match self.disconnect(&directory, request).await {
                Ok(result) => result,
                Err(error) => {
                    failed.get_or_insert(error);
                    continue;
                }
            };
            if let Ok(Some(connection)) =
                storage::read::<Connection>(&directory.join("selected.json"))
            {
                result["targetId"] = json!(connection.target);
            }
            closed.push(result);
        }
        if let Some(error) = failed {
            return Err(error);
        }
        Ok(match closed.len() {
            0 => json!({"state":"closed","closed":true}),
            1 => closed.remove(0),
            _ => json!({
                "state":if closed.iter().all(|result| result["closed"] == true) {"closed"} else {"closing"},
                "closed":closed.iter().all(|result| result["closed"] == true),
                "targets":closed,
            }),
        })
    }
    pub(super) async fn reconnect(
        &self,
        directory: &Path,
        connection: &Connection,
    ) -> Result<Value> {
        self.connect_retry(directory,json!({"targetId":connection.target,"androidCapture":connection.android_capture,"runnerTeamId":connection.runner_team_id,"controlPath":connection.control_path,"appId":connection.bundle_id,"setup":connection.setup,"boot":connection.boot,"window":connection.window})).await
    }
}
