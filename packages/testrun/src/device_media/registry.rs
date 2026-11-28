//! Share sources by selected target identity and retain cleanup after the last consumer drops.
use super::startup::{Ownership, Startup};
use super::{lifecycle::Lifecycle, source::Source, stream::start};
use crate::error::{Error, Result};
use serde_json::{Value, json};
use std::{collections::HashMap, sync::Arc};

struct Entry {
    source: std::sync::Weak<Source>,
    lifecycle: Arc<Lifecycle>,
    ownership: Option<Arc<Ownership>>,
}
#[derive(Default)]
pub struct Sources {
    sources: HashMap<String, Entry>,
    starting: HashMap<String, Startup>,
    // Keep the most recent receive failure after its successfully released
    // source leaves the registry. This diagnostic never owns a resource.
    stream_errors: HashMap<String, Error>,
}
impl Sources {
    pub(crate) fn take_stream_error(&mut self, serial: &str) -> Option<Error> {
        self.sources
            .get(serial)
            .and_then(|entry| entry.lifecycle.stream_error())
            .or_else(|| self.stream_errors.remove(serial))
    }
    fn remove_released(&mut self, serial: &str) {
        if let Some(entry) = self.sources.remove(serial)
            && let Some(error) = entry.lifecycle.stream_error()
        {
            self.stream_errors.insert(serial.into(), error);
        }
    }
    /// Find this owner's source without starting video or claiming decoder health.
    pub(crate) fn existing(&self, serial: &str, target: &Value) -> Result<Option<Arc<Source>>> {
        if let Some(entry) = self.sources.get(serial)
            && let Some(source) = entry.source.upgrade()
        {
            if source.target != *target {
                return Err(Error::new(
                    "STALE_TARGET",
                    "Media source belongs to another target generation",
                ));
            }
            return Ok(Some(source));
        }
        Ok(None)
    }
    pub async fn acquire(&mut self, serial: &str, target: &Value) -> Result<Arc<Source>> {
        let device = serial.to_owned();
        let identity = target.clone();
        self.acquire_with(serial, target, move |ownership| async move {
            start(&device, &identity, ownership).await
        })
        .await
    }
    async fn acquire_with<F, Fut>(
        &mut self,
        serial: &str,
        target: &Value,
        start: F,
    ) -> Result<Arc<Source>>
    where
        F: FnOnce(Arc<Ownership>) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = Result<Source>> + Send + 'static,
    {
        if let Some(source) = self.existing(serial, target)? {
            if source.latest.borrow().is_none() {
                return Err(Error::new(
                    "CAPTURE_STALLED",
                    "Existing device media source is not live",
                ));
            }
            return Ok(source);
        }
        if let Some(entry) = self.sources.get(serial) {
            entry.lifecycle.close().await?;
            if let Some(ownership) = &entry.ownership {
                ownership.released();
            }
            self.remove_released(serial);
        }
        if self.starting.get(serial).is_some_and(Startup::failed) {
            self.starting.get_mut(serial).unwrap().close().await?;
            self.starting.remove(serial);
        }
        if !self.starting.contains_key(serial) {
            self.starting
                .insert(serial.into(), Startup::new(serial, start)?);
        }
        let source = self.starting.get_mut(serial).unwrap().wait().await?;
        let startup = self.starting.remove(serial).unwrap();
        self.sources.insert(
            serial.into(),
            Entry {
                source: Arc::downgrade(&source),
                lifecycle: source.lifecycle.clone(),
                ownership: Some(startup.ownership),
            },
        );
        Ok(source)
    }
    /// Release only this selected device, preserving other devices' outcomes.
    pub(crate) async fn release_unused_for(&mut self, serial: &str) -> Result<()> {
        if let Some(startup) = self.starting.get_mut(serial) {
            startup.close().await?;
            self.starting.remove(serial);
        }
        if let Some(entry) = self.sources.get(serial)
            && entry.source.strong_count() == 0
        {
            entry.lifecycle.close().await?;
            if let Some(ownership) = &entry.ownership {
                ownership.released();
            }
            self.remove_released(serial);
        }
        Ok(())
    }
    pub async fn release_unused(&mut self) -> Result<()> {
        let mut ids = self
            .sources
            .iter()
            .filter(|(_, s)| s.source.strong_count() == 0)
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        ids.extend(self.starting.keys().cloned());
        let mut failures = Vec::new();
        for id in ids {
            if let Err(error) = self.release_unused_for(&id).await {
                failures.push(error);
            }
        }
        if failures.is_empty() {
            Ok(())
        } else {
            Err(Error::new("CLEANUP_FAILED", "Device media cleanup failed")
                .details(json!({"failures":failures})))
        }
    }
    pub async fn shutdown(&mut self) -> Result<()> {
        let mut failures = Vec::new();
        let pending: Vec<_> = self.starting.keys().cloned().collect();
        for id in pending {
            match self.starting.get_mut(&id).unwrap().close().await {
                Ok(()) => {
                    self.starting.remove(&id);
                }
                Err(error) => failures.push(error),
            }
        }
        let ids: Vec<_> = self.sources.keys().cloned().collect();
        for id in ids {
            let result = self.sources[&id].lifecycle.close().await;
            match result {
                Ok(()) => {
                    if let Some(ownership) = &self.sources[&id].ownership {
                        ownership.released();
                    }
                    self.remove_released(&id);
                }
                Err(error) => failures.push(error),
            }
        }
        if failures.is_empty() {
            Ok(())
        } else {
            Err(Error::new("CLEANUP_FAILED", "Device media cleanup failed")
                .details(json!({"failures":failures})))
        }
    }
}

#[cfg(test)]
mod tests;
