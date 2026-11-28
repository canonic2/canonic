//! Observes and reaps only the XCTest child spawned by this runner owner.
use crate::error::{Error, Result};
use std::{process::ExitStatus, sync::Mutex, time::Duration};
use tokio::process::Child;

pub(super) struct Host(Mutex<Child>);

impl Host {
    pub(super) fn new(child: Child) -> Self {
        Self(Mutex::new(child))
    }

    pub(super) fn exited(&self) -> Result<Option<ExitStatus>> {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .try_wait()
            .map_err(|_| Error::new("HOST_IO", "Cannot inspect the owned XCTest host"))
    }

    pub(super) async fn wait(&self) -> Result<ExitStatus> {
        loop {
            if let Some(status) = self.exited()? {
                return Ok(status);
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    /// Used only after authenticated stop acknowledgement or verified native
    /// process absence, never as a response to a busy runner.
    pub(super) async fn finish(&self) -> Result<()> {
        if let Ok(result) = tokio::time::timeout(Duration::from_secs(5), self.wait()).await {
            return result.map(|_| ());
        }
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .start_kill()
            .map_err(|_| Error::new("CLEANUP_FAILED", "Cannot stop the owned XCTest host"))?;
        tokio::time::timeout(Duration::from_secs(5), self.wait())
            .await
            .map_err(|_| Error::new("CLEANUP_FAILED", "The owned XCTest host did not exit"))??;
        Ok(())
    }

    #[cfg(all(test, unix))]
    pub(super) fn kill_fixture(&self) {
        self.0.lock().unwrap().start_kill().unwrap();
    }
}
