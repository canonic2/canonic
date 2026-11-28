//! Bounded CDP events: diagnostic loss is explicit, critical overflow is terminal.

use super::{Loss, MAX_CRITICAL_EVENTS, MAX_EVENT_BYTES, MAX_EVENTS, MAX_EVENTS_BYTES};
use serde_json::Value;
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};
use tokio::sync::Notify;

/// Private protocol data. Consumers must resolve ownership before acting on it.
#[derive(Debug)]
pub struct Event {
    pub method: String,
    pub session_id: Option<String>,
    pub params: Value,
}

impl Event {
    fn critical(&self) -> bool {
        self.method.starts_with("Target.")
            || self.method.starts_with("Page.frame")
            || self.method.starts_with("Runtime.executionContext")
            || matches!(
                self.method.as_str(),
                "Page.javascriptDialogOpening"
                    | "Tracing.tracingComplete"
                    | "HeapProfiler.resetProfiles"
                    | "Debugger.paused"
                    | "Debugger.resumed"
                    | "Page.javascriptDialogClosed"
                    | "Page.fileChooserOpened"
                    | "Page.screencastFrame"
                    | "Page.lifecycleEvent"
                    | "Page.navigatedWithinDocument"
                    | "Page.documentOpened"
                    | "Runtime.bindingCalled"
                    | "Browser.downloadWillBegin"
                    | "Browser.downloadProgress"
                    | "Fetch.requestPaused"
                    | "Fetch.authRequired"
                    | "Input.dragIntercepted"
                    | "Inspector.detached"
                    | "Inspector.targetCrashed"
                    | "Inspector.targetReloadedAfterCrash"
                    // A dropped record would lose a page-registered passkey.
                    | "WebAuthn.credentialAdded"
                    | "WebAuthn.credentialAsserted"
                    | "WebAuthn.credentialUpdated"
                    | "WebAuthn.credentialDeleted"
            )
    }
}

#[derive(Debug)]
pub enum EventItem {
    Event(Event),
    /// Only noncritical diagnostics were evicted; lifecycle events remain ordered.
    Gap {
        dropped: u64,
    },
}

#[derive(Default)]
struct Queue {
    items: VecDeque<(Event, usize)>,
    bytes: usize,
    critical: usize,
    dropped: u64,
    closed: bool,
    heap: Option<(String, Arc<Mutex<HeapData>>)>,
}

struct HeapData {
    bytes: Vec<u8>,
    max: usize,
    truncated: bool,
}
/// An explicitly registered, bounded snapshot stream bypasses ordinary diagnostic eviction.
/// Its bytes survive reader loss and remain private to the registering owner.
pub(crate) struct HeapStream {
    data: Arc<Mutex<HeapData>>,
    owner: Buffer,
}
impl HeapStream {
    pub(crate) fn summary(&self) -> (usize, bool) {
        let data = self.data.lock().expect("Heap stream");
        (data.bytes.len(), data.truncated)
    }
    pub(crate) fn finish(self) -> (Vec<u8>, bool) {
        let mut data = self.data.lock().expect("Heap stream");
        (std::mem::take(&mut data.bytes), data.truncated)
    }
}
impl Drop for HeapStream {
    fn drop(&mut self) {
        let mut queue = self.owner.queue.lock().expect("CDP event owner");
        if queue
            .heap
            .as_ref()
            .is_some_and(|(_, data)| Arc::ptr_eq(data, &self.data))
        {
            queue.heap = None;
        }
    }
}

#[derive(Clone, Default)]
pub(super) struct Buffer {
    queue: Arc<Mutex<Queue>>,
    notify: Arc<Notify>,
}

/// One protocol consumer; lifecycle/control events must be handled before public
/// references are exposed. An overflowing critical queue loses the connection.
pub struct Events(pub(super) Buffer);

impl Buffer {
    pub(super) fn push(&self, event: Event, bytes: usize) -> Result<(), Loss> {
        let mut queue = self.queue.lock().expect("CDP event owner");
        if queue.closed {
            return Err(Loss::Closed);
        }
        if event.critical() && queue.critical == MAX_CRITICAL_EVENTS {
            return Err(Loss::EventCapacity);
        }
        if event.method == "HeapProfiler.addHeapSnapshotChunk"
            && let Some((session, data)) = &queue.heap
            && event.session_id.as_deref() == Some(session.as_str())
        {
            let chunk = event.params["chunk"].as_str().ok_or(Loss::Protocol)?;
            let mut data = data.lock().expect("Heap stream");
            let remaining = data.max.saturating_sub(data.bytes.len());
            data.truncated |= chunk.len() > remaining;
            data.bytes
                .extend_from_slice(&chunk.as_bytes()[..chunk.len().min(remaining)]);
            return Ok(());
        }
        if bytes > MAX_EVENT_BYTES {
            if event.critical() {
                return Err(Loss::EventCapacity);
            }
            queue.dropped = queue.dropped.saturating_add(1);
        } else {
            while queue.items.len() >= MAX_EVENTS || queue.bytes + bytes > MAX_EVENTS_BYTES {
                let Some(index) = queue.items.iter().position(|(event, _)| !event.critical())
                else {
                    if event.critical() {
                        return Err(Loss::EventCapacity);
                    }
                    queue.dropped = queue.dropped.saturating_add(1);
                    drop(queue);
                    self.notify.notify_one();
                    return Ok(());
                };
                let (_, removed) = queue.items.remove(index).expect("Known diagnostic event");
                queue.bytes -= removed;
                queue.dropped = queue.dropped.saturating_add(1);
            }
            queue.bytes += bytes;
            queue.critical += usize::from(event.critical());
            queue.items.push_back((event, bytes));
        }
        drop(queue);
        self.notify.notify_one();
        Ok(())
    }

    pub(super) fn close(&self) {
        self.queue.lock().expect("CDP event owner").closed = true;
        self.notify.notify_one();
    }
}

impl Events {
    pub(crate) fn heap_stream(&mut self, session: String, max: usize) -> Option<HeapStream> {
        if !(1024..=32 * 1024 * 1024).contains(&max) {
            return None;
        }
        let mut queue = self.0.queue.lock().expect("CDP event owner");
        if queue.closed || queue.heap.is_some() {
            return None;
        }
        let data = Arc::new(Mutex::new(HeapData {
            bytes: Vec::new(),
            max,
            truncated: false,
        }));
        queue.heap = Some((session, data.clone()));
        Some(HeapStream {
            data,
            owner: self.0.clone(),
        })
    }
    /// Drain events already ordered before a completed command reply. The
    /// single lifecycle consumer applies these before exposing new references.
    pub(crate) fn try_next(&mut self) -> Option<EventItem> {
        let mut queue = self.0.queue.lock().expect("CDP event owner");
        if queue.dropped != 0 {
            return Some(EventItem::Gap {
                dropped: std::mem::take(&mut queue.dropped),
            });
        }
        let (event, bytes) = queue.items.pop_front()?;
        queue.bytes -= bytes;
        queue.critical -= usize::from(event.critical());
        Some(EventItem::Event(event))
    }

    pub async fn next(&mut self) -> Option<EventItem> {
        loop {
            let notified = self.0.notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            {
                let mut queue = self.0.queue.lock().expect("CDP event owner");
                if queue.dropped != 0 {
                    return Some(EventItem::Gap {
                        dropped: std::mem::take(&mut queue.dropped),
                    });
                }
                if let Some((event, bytes)) = queue.items.pop_front() {
                    queue.bytes -= bytes;
                    queue.critical -= usize::from(event.critical());
                    return Some(EventItem::Event(event));
                }
                if queue.closed {
                    return None;
                }
            }
            notified.await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[tokio::test]
    async fn diagnostic_byte_overflow_reports_loss_and_draining_releases_critical_capacity() {
        let buffer = Buffer::default();
        let mut events = Events(buffer.clone());
        let critical = || Event {
            method: "Target.targetDestroyed".into(),
            session_id: None,
            params: json!({}),
        };
        for _ in 0..MAX_CRITICAL_EVENTS {
            buffer.push(critical(), 64).unwrap();
        }
        assert_eq!(buffer.push(critical(), 64), Err(Loss::EventCapacity));
        assert!(matches!(events.try_next(), Some(EventItem::Event(_))));
        assert!(matches!(events.next().await, Some(EventItem::Event(_))));
        buffer.push(critical(), 64).unwrap();
        buffer.push(critical(), 64).unwrap();
        assert_eq!(buffer.push(critical(), 64), Err(Loss::EventCapacity));
        for _ in 0..MAX_EVENTS_BYTES / MAX_EVENT_BYTES {
            buffer
                .push(
                    Event {
                        method: "Network.dataReceived".into(),
                        session_id: None,
                        params: json!({}),
                    },
                    MAX_EVENT_BYTES,
                )
                .unwrap();
        }
        assert!(matches!(
            events.try_next(),
            Some(EventItem::Gap { dropped: 1 })
        ));
        // The byte bound evicts only diagnostics even when many record slots
        // remain; all critical events survive in their original order.
        for _ in 0..MAX_CRITICAL_EVENTS {
            assert!(
                matches!(events.try_next(), Some(EventItem::Event(event)) if event.method == "Target.targetDestroyed")
            );
        }
        for _ in 0..MAX_EVENTS_BYTES / MAX_EVENT_BYTES - 1 {
            assert!(
                matches!(events.try_next(), Some(EventItem::Event(event)) if event.method == "Network.dataReceived")
            );
        }
        assert!(events.try_next().is_none());
    }

    #[test]
    fn owned_heap_stream_accepts_large_chunks_bounds_retention_and_preserves_control_events() {
        let buffer = Buffer::default();
        let mut events = Events(buffer.clone());
        let stream = events
            .heap_stream("Acme".into(), 2 * MAX_EVENT_BYTES)
            .unwrap();
        assert!(events.heap_stream("other".into(), 1024).is_none());
        let chunk = "x".repeat(MAX_EVENT_BYTES + 1);
        buffer
            .push(
                Event {
                    method: "HeapProfiler.addHeapSnapshotChunk".into(),
                    session_id: Some("Acme".into()),
                    params: json!({"chunk":chunk}),
                },
                chunk.len() + 128,
            )
            .unwrap();
        assert_eq!(stream.summary(), (chunk.len(), false));
        assert!(events.try_next().is_none());
        buffer
            .push(
                Event {
                    method: "Page.javascriptDialogOpening".into(),
                    session_id: Some("Acme".into()),
                    params: json!({}),
                },
                128,
            )
            .unwrap();
        assert!(
            matches!(events.try_next(),Some(EventItem::Event(event)) if event.method=="Page.javascriptDialogOpening")
        );
        buffer
            .push(
                Event {
                    method: "HeapProfiler.addHeapSnapshotChunk".into(),
                    session_id: Some("Acme".into()),
                    params: json!({"chunk":chunk}),
                },
                chunk.len() + 128,
            )
            .unwrap();
        assert_eq!(stream.summary(), (2 * MAX_EVENT_BYTES, true));
        buffer.close();
        let (bytes, truncated) = stream.finish();
        assert_eq!(bytes.len(), 2 * MAX_EVENT_BYTES);
        assert!(truncated);
        assert!(events.heap_stream("Acme".into(), 1024).is_none());
    }
    #[test]
    fn heap_stream_registration_does_not_admit_foreign_chunks_and_drop_releases_its_slot() {
        let buffer = Buffer::default();
        let mut events = Events(buffer.clone());
        let stream = events.heap_stream("Acme".into(), 1024).unwrap();
        buffer
            .push(
                Event {
                    method: "HeapProfiler.addHeapSnapshotChunk".into(),
                    session_id: Some("other".into()),
                    params: json!({"chunk":"Acme foreign"}),
                },
                128,
            )
            .unwrap();
        assert_eq!(stream.summary(), (0, false));
        assert!(matches!(events.try_next(), Some(EventItem::Event(_))));
        drop(stream);
        assert!(events.heap_stream("Acme".into(), 1024).is_some());
    }
}
