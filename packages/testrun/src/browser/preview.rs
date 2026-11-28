//! Browser preview settings and owned source state, independent of protocol GUIDs.

use super::{contract::Reference, objects::ObjectRef};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};
use tokio::sync::watch;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Options {
    #[serde(default = "quality")]
    pub quality: u8,
    #[serde(default = "fps")]
    pub fps: u8,
}
fn quality() -> u8 {
    75
}
fn fps() -> u8 {
    20
}

/// The pinned driver floors source bounds to even dimensions. Round upward
/// first, preserving the logical viewport and avoiding a second aspect shrink.
pub(crate) fn source_size(width: u64, height: u64) -> Result<Value, &'static str> {
    let width = width.checked_add(1).ok_or("MEDIA_GEOMETRY_BOUNDS")? & !1;
    let height = height.checked_add(1).ok_or("MEDIA_GEOMETRY_BOUNDS")? & !1;
    if width == 0 || height == 0 || width > 8192 || height > 8192 || width * height > 8_000_000 {
        return Err("MEDIA_GEOMETRY_BOUNDS");
    }
    Ok(json!({"width":width,"height":height}))
}
impl Default for Options {
    fn default() -> Self {
        Self {
            quality: quality(),
            fps: fps(),
        }
    }
}
impl Options {
    pub(crate) fn validate(&self) -> Result<(), &'static str> {
        if !(1..=100).contains(&self.quality) || !(1..=30).contains(&self.fps) {
            return Err("INVALID_PREVIEW_OPTIONS");
        }
        Ok(())
    }
}

pub(crate) struct Source<K = ObjectRef> {
    pub reference: Reference,
    pub page: Reference,
    pub wire: K,
    pub epoch: Uuid,
    pub options: Options,
    pub bounds: super::contract::Viewport,
    pub frames: watch::Sender<Option<Arc<crate::media::Frame>>>,
    pub alive: Arc<AtomicBool>,
    pub interruptions: Arc<AtomicU64>,
    pub state: &'static str,
    pub sequence: u64,
    pub document_floor: u64,
    pub frame: Option<Reference>,
    pub geometry: Option<Value>,
    pub viewport_epoch: Uuid,
    pub failure: Option<&'static str>,
    pub viewer: Option<Value>,
    pub preview_active: bool,
    pub recordings: std::collections::BTreeSet<Uuid>,
    pub discarded: u64,
    pub source_sequence: u64,
    pub source_dropped: u64,
}

impl<K> Source<K> {
    pub fn snapshot(&self) -> Value {
        let latest = self.frames.borrow().clone();
        json!({"preview":self.reference,"page":self.page,"sourceEpoch":self.epoch,"viewportEpoch":self.viewport_epoch,"frame":self.frame,"state":self.state,"previewActive":self.preview_active,"recordingConsumers":self.recordings.len(),"options":self.options,"sourceBounds":self.bounds,"sequence":self.sequence,"geometry":self.geometry,"failure":self.failure,"viewer":self.viewer,"lastFrameAgeMs":latest.as_ref().map(|frame|frame.received.elapsed().as_millis()),"metadata":latest.as_ref().map(|frame|&frame.metadata),"discardedFrames":self.discarded,"audio":false,"coordinateInputAvailable":latest.as_ref().is_some_and(|frame|frame.metadata["coordinateInputAvailable"]==true)})
    }
}

#[derive(Default)]
pub(crate) struct Sources {
    pub entries: BTreeMap<Uuid, Source>,
}
impl Sources {
    pub fn get(&self, reference: Reference) -> Result<&Source, &'static str> {
        self.entries
            .get(&reference.id)
            .filter(|source| source.reference == reference)
            .ok_or("STALE_PREVIEW")
    }
    pub fn get_mut(&mut self, reference: Reference) -> Result<&mut Source, &'static str> {
        self.entries
            .get_mut(&reference.id)
            .filter(|source| source.reference == reference)
            .ok_or("STALE_PREVIEW")
    }
    pub fn snapshot(&self) -> Value {
        json!(
            self.entries
                .values()
                .map(Source::snapshot)
                .collect::<Vec<_>>()
        )
    }
    pub fn document_changed(&mut self, page: Reference, ordinal: u64) {
        for source in self
            .entries
            .values_mut()
            .filter(|source| source.page == page && matches!(source.state, "starting" | "live"))
        {
            source.document_floor = ordinal;
            source.interruptions.fetch_add(1, Ordering::SeqCst);
            source.frames.send_replace(None);
            source.geometry = None;
            source.frame = None;
            source.viewport_epoch = Uuid::new_v4();
            if source.failure.is_none() {
                source.state = "starting";
            }
        }
    }
}

pub(crate) fn schemas(reference: &Value) -> Vec<Value> {
    let object = |properties, required: &[&str]| json!({"type":"object","properties":properties,"required":required,"additionalProperties":false});
    let options = object(
        json!({"quality":{"type":"integer","minimum":1,"maximum":100,"default":75},"fps":{"type":"integer","minimum":1,"maximum":30,"default":20}}),
        &[],
    );
    let mut result = vec![object(
        json!({"operation":{"const":"preview_start"},"page":reference,"options":options}),
        &["operation", "page"],
    )];
    for operation in ["preview_status", "preview_stop"] {
        result.push(object(
            json!({"operation":{"const":operation},"preview":reference}),
            &["operation", "preview"],
        ));
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn odd_logical_viewports_do_not_shrink_twice_through_driver_rounding() {
        assert_eq!(
            source_size(959, 599).unwrap(),
            json!({"width":960,"height":600})
        );
        assert_eq!(
            source_size(1280, 720).unwrap(),
            json!({"width":1280,"height":720})
        );
        assert!(source_size(8192, 8192).is_err());
        assert!(source_size(u64::MAX, 720).is_err());
    }
}
