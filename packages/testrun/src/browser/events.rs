//! Bounded private event delivery that cannot block driver replies or callbacks.

use super::{dispatcher::Event, schema};
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};
use tokio::sync::Notify;

const MAX_EVENTS: usize = 256;
const MAX_EVENT_BYTES: usize = 1024 * 1024;
const MAX_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug)]
pub enum Item {
    /// Internal ordering fence: processed only after all earlier lifetime events.
    Barrier(tokio::sync::oneshot::Sender<()>),
    Event(Event),
    /// Number of omitted events, including oversized events and dropped oldest
    /// entries. A consumer must resynchronize state after this witness.
    Gap {
        dropped: u64,
    },
    /// A dispatched call settled after its caller stopped waiting. This is a
    /// droppable diagnostic witness, not a durable action journal or a replay
    /// instruction. Workflow owners must retain their own operation lifetime.
    Settled {
        id: u64,
        succeeded: bool,
    },
}

#[derive(Default)]
struct Queue {
    items: VecDeque<(Item, usize, u64)>,
    sequence: u64,
    bytes: usize,
    dropped: u64,
    closed: bool,
}

#[derive(Clone, Default)]
pub(crate) struct Buffer {
    queue: Arc<Mutex<Queue>>,
    notify: Arc<Notify>,
}

/// One private consumer per connection. Viewer fan-out is a later media owner.
pub struct Events(pub(crate) Buffer);

impl Buffer {
    pub(crate) fn next_sequence(&self) -> u64 {
        let mut queue = self.queue.lock().expect("Event buffer owner");
        queue.sequence += 1;
        queue.sequence
    }
    pub(crate) fn push(&self, item: Item) {
        let bytes = match &item {
            Item::Event(Event::Message {
                params: Some(value),
                ..
            }) => schema::encoded_len(value, MAX_EVENT_BYTES).map(|bytes| bytes + 4096),
            _ => Ok(4096),
        };
        let mut queue = self.queue.lock().expect("Event buffer owner");
        queue.sequence += 1;
        let sequence = queue.sequence;
        if queue.closed {
            return;
        }
        let Ok(bytes) = bytes else {
            queue.dropped = queue.dropped.saturating_add(1);
            drop(queue);
            self.notify.notify_one();
            return;
        };
        while queue.items.len() >= MAX_EVENTS || queue.bytes + bytes > MAX_BYTES {
            let (_, bytes, _) = queue.items.pop_front().expect("Nonempty saturated buffer");
            queue.bytes -= bytes;
            queue.dropped = queue.dropped.saturating_add(1);
        }
        queue.bytes += bytes;
        queue.items.push_back((item, bytes, sequence));
        drop(queue);
        self.notify.notify_one();
    }

    pub(crate) fn close(&self) {
        self.queue.lock().expect("Event buffer owner").closed = true;
        self.notify.notify_one();
    }
}

impl Events {
    pub async fn next(&mut self) -> Option<Item> {
        self.next_ordered().await.map(|(_, item)| item)
    }
    pub(crate) async fn next_ordered(&mut self) -> Option<(u64, Item)> {
        loop {
            let notified = self.0.notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            {
                let mut queue = self.0.queue.lock().expect("Event buffer owner");
                if queue.dropped != 0 {
                    let dropped = std::mem::take(&mut queue.dropped);
                    return Some((queue.sequence, Item::Gap { dropped }));
                }
                if let Some((item, bytes, sequence)) = queue.items.pop_front() {
                    queue.bytes -= bytes;
                    return Some((sequence, item));
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

    #[tokio::test]
    async fn slow_consumers_receive_loss_without_blocking_producers_and_can_drain_after_close() {
        let buffer = Buffer::default();
        let mut events = Events(buffer.clone());
        for id in 0..300 {
            buffer.push(Item::Settled {
                id,
                succeeded: true,
            });
        }
        buffer.close();
        assert!(matches!(
            events.next().await,
            Some(Item::Gap { dropped: 44 })
        ));
        assert!(matches!(
            events.next().await,
            Some(Item::Settled { id: 44, .. })
        ));
        for _ in 1..256 {
            assert!(events.next().await.is_some());
        }
        assert!(events.next().await.is_none());
    }

    #[tokio::test]
    async fn payload_byte_budget_and_oversized_events_report_loss() {
        let buffer = Buffer::default();
        let mut events = Events(buffer.clone());
        let objects = crate::browser::objects::ObjectTable::default();
        let object = objects.lookup("", "Root").unwrap();
        buffer.push(Item::Event(Event::Message {
            object: object.clone(),
            method: "fixture".into(),
            params: Some(serde_json::json!("x".repeat(MAX_EVENT_BYTES))),
        }));
        assert!(matches!(
            events.next().await,
            Some(Item::Gap { dropped: 1 })
        ));
        for _ in 0..9 {
            buffer.push(Item::Event(Event::Message {
                object: object.clone(),
                method: "fixture".into(),
                params: Some(serde_json::json!("x".repeat(512 * 1024))),
            }));
        }
        assert!(buffer.queue.lock().unwrap().bytes <= MAX_BYTES);
        let Some(Item::Gap { dropped }) = events.next().await else {
            panic!("Expected explicit byte-budget loss");
        };
        assert!(dropped > 0);
        buffer.close();
        let mut received = 0;
        while events.next().await.is_some() {
            received += 1;
        }
        assert_eq!(received + dropped, 9);
    }
}
