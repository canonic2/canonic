//! Separate bounded latest-frame lane; lifecycle events never compete with JPEGs.

use super::objects::ObjectRef;
use serde_json::Value;
use std::{
    collections::{HashMap, VecDeque},
    hash::Hash,
    sync::{Arc, Mutex},
    time::Instant,
};
use tokio::sync::Notify;
use uuid::Uuid;

pub(crate) const MAX_SOURCES: usize = 8;
const MAX_ENCODED: usize = 6 * 1024 * 1024;
const MAX_QUEUE: usize = 16 * 1024 * 1024;

pub(crate) struct RawFrame<K = ObjectRef> {
    pub page: K,
    pub epoch: Uuid,
    pub ordinal: u64,
    pub source_sequence: u64,
    pub params: Value,
    pub arrived: Instant,
    pub arrival_unix_ms: u128,
}

struct State<K> {
    sources: HashMap<K, (Uuid, u64)>,
    frames: VecDeque<(RawFrame<K>, usize)>,
    bytes: usize,
    dropped: u64,
    closed: bool,
    claimed: bool,
    failure: Option<&'static str>,
}
#[derive(Clone)]
pub(crate) struct Lane<K = ObjectRef> {
    state: Arc<Mutex<State<K>>>,
    changed: Arc<Notify>,
}
pub(crate) struct Frames<K = ObjectRef>(Lane<K>);

impl<K> Default for Lane<K> {
    fn default() -> Self {
        Self {
            state: Arc::new(Mutex::new(State {
                sources: HashMap::new(),
                frames: VecDeque::new(),
                bytes: 0,
                dropped: 0,
                closed: false,
                claimed: false,
                failure: None,
            })),
            changed: Arc::new(Notify::new()),
        }
    }
}

impl<K: Clone + Eq + Hash> Lane<K> {
    pub fn claim(&self) -> Result<Frames<K>, &'static str> {
        let mut state = self.state.lock().expect("Browser frame lane");
        if state.claimed {
            return Err("MEDIA_OWNER_EXISTS");
        }
        state.claimed = true;
        Ok(Frames(self.clone()))
    }
    pub fn attach(&self, page: K, epoch: Uuid) -> Result<(), &'static str> {
        let mut state = self.state.lock().expect("Browser frame lane");
        if state.closed || state.failure.is_some() {
            return Err("MEDIA_SOURCE_LOST");
        }
        if state.sources.contains_key(&page) {
            return Err("MEDIA_SOURCE_EXISTS");
        }
        if state.sources.len() >= MAX_SOURCES {
            return Err("MEDIA_SOURCE_CAPACITY");
        }
        state.sources.insert(page, (epoch, 0));
        Ok(())
    }
    pub fn detach(&self, page: &K) {
        let mut state = self.state.lock().expect("Browser frame lane");
        state.sources.remove(page);
        Self::remove(&mut state, page);
    }
    pub fn fence(&self, page: &K) {
        let mut state = self.state.lock().expect("Browser frame lane");
        if let Some((epoch, _)) = state.sources.get_mut(page) {
            *epoch = Uuid::new_v4();
        }
        Self::remove(&mut state, page);
    }
    fn remove(state: &mut State<K>, page: &K) {
        if let Some(index) = state
            .frames
            .iter()
            .position(|(frame, _)| &frame.page == page)
        {
            let (_, bytes) = state.frames.remove(index).expect("Queued frame");
            state.bytes -= bytes;
            state.dropped += 1;
        }
    }
    pub fn push(&self, page: K, ordinal: u64, params: Value) {
        let mut state = self.state.lock().expect("Browser frame lane");
        if state.closed || state.failure.is_some() {
            return;
        }
        let Some((epoch, sequence)) = state.sources.get_mut(&page) else {
            return;
        };
        *sequence += 1;
        let (epoch, source_sequence) = (*epoch, *sequence);
        let bytes = params["data"].as_str().map_or(usize::MAX, str::len);
        if bytes > MAX_ENCODED {
            state.dropped += 1;
            return;
        }
        Self::remove(&mut state, &page);
        while state.bytes + bytes > MAX_QUEUE {
            let Some((_, bytes)) = state.frames.pop_front() else {
                break;
            };
            state.bytes -= bytes;
            state.dropped += 1;
        }
        state.bytes += bytes;
        state.frames.push_back((
            RawFrame {
                page,
                epoch,
                ordinal,
                source_sequence,
                params,
                arrived: Instant::now(),
                arrival_unix_ms: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis(),
            },
            bytes,
        ));
        drop(state);
        self.changed.notify_one();
    }
    pub fn fail(&self, failure: &'static str) {
        let mut state = self.state.lock().expect("Browser frame lane");
        state.failure = Some(failure);
        let queued = state.frames.len() as u64;
        state.dropped += queued;
        state.frames.clear();
        state.bytes = 0;
        drop(state);
        self.changed.notify_one();
    }
    pub fn dropped(&self) -> u64 {
        self.state.lock().expect("Browser frame lane").dropped
    }
    pub fn failure(&self) -> Option<&'static str> {
        self.state.lock().expect("Browser frame lane").failure
    }
    pub fn epoch(&self, page: &K) -> Option<Uuid> {
        self.state
            .lock()
            .expect("Browser frame lane")
            .sources
            .get(page)
            .map(|(epoch, _)| *epoch)
    }
    pub fn close(&self) {
        self.state.lock().expect("Browser frame lane").closed = true;
        self.changed.notify_one();
    }
}

impl<K> Frames<K> {
    pub async fn next(&mut self) -> Option<RawFrame<K>> {
        loop {
            let changed = self.0.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            {
                let mut state = self.0.state.lock().expect("Browser frame lane");
                if let Some((frame, bytes)) = state.frames.pop_front() {
                    state.bytes -= bytes;
                    return Some(frame);
                }
                if state.closed || state.failure.is_some() {
                    return None;
                }
            }
            changed.await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[tokio::test]
    async fn each_source_keeps_its_latest_frame_without_blocking_other_sources() {
        let mut objects = super::super::objects::ObjectTable::default();
        let root = objects.lookup("", "Root").unwrap();
        let first = objects.create(&root, "first", "Page").unwrap();
        let second = objects.create(&root, "second", "Page").unwrap();
        let lane = Lane::default();
        let mut frames = lane.claim().unwrap();
        let epoch = Uuid::new_v4();
        lane.attach(first.clone(), epoch).unwrap();
        lane.attach(second.clone(), Uuid::new_v4()).unwrap();
        lane.push(first.clone(), 1, json!({"data":"YQ==","frameId":1}));
        lane.push(second.clone(), 2, json!({"data":"Yg==","frameId":2}));
        lane.push(first.clone(), 3, json!({"data":"Yw==","frameId":3}));
        assert_eq!(lane.dropped(), 1);
        let frame = frames.next().await.unwrap();
        assert_eq!(frame.page, second);
        let frame = frames.next().await.unwrap();
        assert_eq!(frame.page, first);
        assert_eq!(frame.ordinal, 3);
        assert_eq!(frame.epoch, epoch);
        lane.detach(&first);
        lane.push(first, 4, json!({"data":"ZA==","frameId":4}));
        lane.close();
        assert!(frames.next().await.is_none());
    }

    #[tokio::test]
    async fn byte_limits_and_lane_failure_remain_explicit() {
        let mut objects = super::super::objects::ObjectTable::default();
        let root = objects.lookup("", "Root").unwrap();
        let page = objects.create(&root, "page", "Page").unwrap();
        let lane = Lane::default();
        let mut frames = lane.claim().unwrap();
        assert!(lane.claim().is_err());
        lane.attach(page.clone(), Uuid::new_v4()).unwrap();
        lane.push(page, 1, json!({"data":"A".repeat(MAX_ENCODED+1)}));
        assert_eq!(lane.dropped(), 1);
        assert!(lane.state.lock().unwrap().bytes <= MAX_QUEUE);
        lane.fail("MEDIA_ACK_FAILED");
        assert!(frames.next().await.is_none());
        assert_eq!(lane.failure(), Some("MEDIA_ACK_FAILED"));
    }
}
