//! One device actor's durable lifecycle record: its connection, epoch-fenced
//! stop requests and cleanup progress. The supervisor holds its lease and
//! native resources.
use super::{Connection, Error, Result, Value, json, now, storage};
#[cfg(unix)]
use std::time::Duration;

pub(crate) struct Owner {
    pub(super) connection: Connection,
    /// Set when the manager exits: the actor finishes with exit cleanup.
    #[cfg_attr(
        windows,
        allow(dead_code, reason = "Read by the Unix retained actor and its fixtures")
    )]
    pub(super) terminate: tokio::sync::watch::Receiver<bool>,
}
impl Owner {
    #[cfg(unix)]
    pub(crate) fn application_records(&self) -> std::path::PathBuf {
        self.connection
            .directory
            .parent()
            .unwrap()
            .join("applications")
    }
    pub(crate) fn epoch(&self) -> &str {
        &self.connection.epoch
    }
    pub(crate) fn cleanup(&self, complete: bool) -> Result<()> {
        storage::write(
            &self.connection.directory.join("cleanup.json"),
            &json!({"complete":complete}),
        )
    }
    pub(crate) fn cleanup_progress(
        &self,
        progress: Value,
        error: Option<&Error>,
        request: Option<&str>,
    ) -> Result<()> {
        let path = self.connection.directory.join("cleanup.json");
        let previous = storage::read::<Value>(&path)?.unwrap_or(json!({}));
        let attempts = previous["attempts"].as_u64().unwrap_or(0) + 1;
        let mut errors = previous["errors"].as_array().cloned().unwrap_or_default();
        if let Some(error) = error {
            errors.push(json!({"attempt":attempts,"error":error}));
        }
        storage::write(
            &path,
            &json!({"complete":error.is_none(),"attempts":attempts,"sessions":progress,"errors":errors,"cleanupRequestId":request}),
        )
    }
    pub(crate) fn state(&self, state: &str, extra: Value) -> Result<()> {
        storage::write(
            &self.connection.directory.join("state.json"),
            &json!({"state":state,"updated":now(),"targetId":extra["targetId"],"androidCapture":extra["androidCapture"],"observation":extra["observation"],"error":extra["error"],"capabilities":extra["capabilities"],"controlPath":extra["controlPath"],"observationProfile":extra["observationProfile"],"runnerSetup":extra["runnerSetup"]}),
        )
    }
    /// Resolves when this epoch's stop marker is written.
    #[cfg(unix)]
    pub(crate) async fn stopped(&self) -> Result<Value> {
        loop {
            if self.stop_request()?.is_some() {
                return Ok(json!({"closed":true,"reason":"disconnect"}));
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
    /// Resolves when the manager asks its actors to exit.
    #[cfg(unix)]
    pub(crate) async fn terminated(&self) -> Result<()> {
        let mut terminate = self.terminate.clone();
        terminate
            .wait_for(|value| *value)
            .await
            .map(|_| ())
            .map_err(|_| Error::new("INTERNAL", "The manager stopped supervising"))
    }
    #[cfg(unix)]
    fn stop_request(&self) -> Result<Option<Value>> {
        Ok(
            storage::read::<Value>(&self.connection.directory.join("stop.json"))?
                .filter(|s| s["epoch"] == self.connection.epoch),
        )
    }
    #[cfg(unix)]
    pub(crate) fn cleanup_request(&self) -> Result<Option<String>> {
        Ok(self
            .stop_request()?
            .and_then(|s| s["requestId"].as_str().map(str::to_owned)))
    }
    #[cfg(unix)]
    pub(crate) async fn cleanup_requested(&self, previous: Option<&str>) -> Result<()> {
        loop {
            if self
                .cleanup_request()?
                .as_deref()
                .is_some_and(|id| Some(id) != previous)
            {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
}

/// Serve one device actor until its stop request or the manager's exit.
pub(super) async fn run(owner: &Owner, server: &mut crate::qa::Server) -> Result<Value> {
    let connection = &owner.connection;
    let input = crate::cli::Mobile {
        target: Some(connection.target.clone()),
        command: crate::cli::Action::Session {
            android_capture: connection.android_capture.map(|mode| {
                match mode {
                    crate::android::streaming::Mode::Stream => "stream",
                    _ => "screenshot",
                }
                .into()
            }),
            runner_team_id: connection.runner_team_id.clone(),
            control_path: connection.control_path.clone(),
            directory: Some(connection.directory.join("s")),
            app_id: connection.bundle_id.clone(),
            setup: connection.setup,
            boot: connection.boot,
            window: connection.window,
            idle_timeout_ms: 600000,
            observation_profile: Some("fast".into()),
        },
    };
    owner.state("opening", json!({}))?;
    owner.cleanup(true)?;
    let result = crate::interactive::serve_managed(&input, owner, server).await;
    let finalized = result.is_ok()
        || result
            .as_ref()
            .is_err_and(|error| error.code == "SESSION_ENDPOINT_LOST");
    owner.state(
        if finalized { "closed" } else { "failed" },
        json!({"error":result.as_ref().err()}),
    )?;
    result
}

/// A refused start allocated nothing. Preserve any existing epoch's cleanup,
/// including a duplicate start while that epoch is still serving requests.
pub(super) fn startup_failure(connection: &Connection, error: &Error) -> Result<()> {
    if connection.held()? {
        return Ok(());
    }
    let cleanup = connection.directory.join("cleanup.json");
    if storage::read::<Value>(&cleanup)?.is_none() {
        storage::write(&cleanup, &json!({"complete":true}))?;
    }
    storage::write(
        &connection.directory.join("state.json"),
        &json!({"state":"failed","error":error}),
    )
}
