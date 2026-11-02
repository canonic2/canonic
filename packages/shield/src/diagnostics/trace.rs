//! Optional request-owned spans. Explicit links cross threads; scopes only attach a link.
use serde_json::{Value, json};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    fs::File,
    io::Write,
    marker::PhantomData,
    rc::Rc,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const QUEUE: usize = 512;
const SPANS: u64 = 256;
type Writer = Arc<Mutex<Option<(File, u64)>>>;

pub(crate) struct Recorder {
    sender: Option<mpsc::SyncSender<Value>>,
    done: Mutex<mpsc::Receiver<()>>,
    worker: Option<thread::JoinHandle<()>>,
    sequence: AtomicU64,
    dropped: Arc<AtomicU64>,
    epoch: Instant,
}

impl Recorder {
    pub(super) fn new(writer: Writer, epoch: Instant) -> Self {
        let (sender, records) = mpsc::sync_channel::<Value>(QUEUE);
        let (finished, done) = mpsc::sync_channel(1);
        let dropped = Arc::new(AtomicU64::new(0));
        let loss = dropped.clone();
        let worker = thread::Builder::new()
            .name("shield-request-trace".into())
            .spawn(move || {
                while let Ok(mut record) = records.recv() {
                    record["dropped_records"] = json!(loss.swap(0, Ordering::Relaxed));
                    let Ok(mut writer) = writer.lock() else { break };
                    let Some((file, used)) = writer.as_mut() else {
                        break;
                    };
                    let Ok(mut bytes) = serde_json::to_vec(&record) else {
                        continue;
                    };
                    bytes.push(b'\n');
                    if used.saturating_add(bytes.len() as u64).saturating_add(64) > super::LIMIT {
                        let _ = file.write_all(b"{\"event\":\"log_limit_reached\"}\n");
                        *writer = None;
                        eprintln!("Shield: trace capacity exhausted; diagnostics stopped.");
                        break;
                    }
                    if file.write_all(&bytes).is_err() {
                        *writer = None;
                        eprintln!("Shield: trace sink unavailable; diagnostics stopped.");
                        break;
                    }
                    *used += bytes.len() as u64;
                }
                let _ = finished.send(());
            })
            .ok();
        Self {
            sender: worker.as_ref().map(|_| sender),
            done: Mutex::new(done),
            worker,
            sequence: AtomicU64::new(0),
            dropped,
            epoch,
        }
    }

    fn emit(&self, event: &'static str, fields: Value) {
        let mut record =
            json!({"event":event,"schema_version":1,"elapsed_us":micros(self.epoch.elapsed())});
        record["fields"] = fields;
        if self
            .sender
            .as_ref()
            .is_none_or(|sender| sender.try_send(record).is_err())
        {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub(super) fn request(
        self: &Arc<Self>,
        surface: &'static str,
        operation: &'static str,
        id: Option<&Value>,
        connection: Option<&str>,
    ) -> Request {
        let trace_id = format!(
            "{}-{}",
            std::process::id(),
            self.sequence.fetch_add(1, Ordering::Relaxed)
        );
        let bounded_id = id.filter(|id| id.is_i64() || id.as_str().is_some_and(|s| s.len() <= 128));
        self.emit("request_start", json!({"trace_id":trace_id,"surface":surface,"operation":operation,"request_id":bounded_id,
            "connection_id":connection.filter(|s| s.len() <= 128),"wall_time_us":SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, micros)}));
        Request(Arc::new(Shared {
            recorder: self.clone(),
            trace_id,
            started: Instant::now(),
            state: Mutex::new(State::default()),
        }))
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        self.sender.take();
        if self
            .done
            .get_mut()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .recv_timeout(Duration::from_millis(5))
            .is_ok()
        {
            if let Some(worker) = self.worker.take() {
                let _ = worker.join();
            }
        } else {
            eprintln!("Shield: trace drain unfinished; diagnostic records may be missing.");
        }
        let dropped = self.dropped.load(Ordering::Relaxed);
        if dropped != 0 {
            eprintln!("Shield: {dropped} trace records dropped.");
        }
    }
}

struct State {
    next: u64,
    counts: BTreeMap<&'static str, u64>,
    finished: bool,
}
impl Default for State {
    fn default() -> Self {
        Self {
            next: 0,
            finished: false,
            counts: BTreeMap::from([
                ("policy.preparations", 0),
                ("source.hydrated", 0),
                ("source.read_bytes", 0),
                ("state.read_bytes", 0),
                ("storage.sync_calls", 0),
                ("projection.files_written", 0),
            ]),
        }
    }
}
struct Shared {
    recorder: Arc<Recorder>,
    trace_id: String,
    started: Instant,
    state: Mutex<State>,
}
impl Drop for Shared {
    fn drop(&mut self) {
        if !self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .finished
        {
            self.recorder.emit(
                "request_incomplete",
                json!({"trace_id":self.trace_id,"duration_us":micros(self.started.elapsed())}),
            );
        }
    }
}

/// A request link is explicitly cloned into a worker or continuation, never inferred from a thread.
#[derive(Clone)]
pub(crate) struct Request(Arc<Shared>);
#[derive(Clone)]
struct Attached {
    request: Request,
    parent: Option<u64>,
}
thread_local! { static CURRENT: RefCell<Option<Attached>> = const { RefCell::new(None) }; }

pub(crate) struct Scope {
    previous: Option<Attached>,
    _thread: PhantomData<Rc<()>>,
}
impl Drop for Scope {
    fn drop(&mut self) {
        CURRENT.with(|current| *current.borrow_mut() = self.previous.take());
    }
}
impl Request {
    /// An interval may cross a continuation boundary without attaching thread state.
    pub(crate) fn wait(&self, stage: &'static str, counter: &'static str) -> Wait {
        let mut request_state = self
            .0
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        request_state.next = request_state.next.saturating_add(1);
        Wait {
            inner: (request_state.next <= SPANS).then(|| {
                (
                    self.clone(),
                    request_state.next,
                    stage,
                    counter,
                    Instant::now(),
                )
            }),
        }
    }
    pub(crate) fn enter(&self) -> Scope {
        Scope {
            previous: CURRENT.replace(Some(Attached {
                request: self.clone(),
                parent: None,
            })),
            _thread: PhantomData,
        }
    }
    pub(crate) fn finish(&self, outcome: &'static str) {
        let mut state = self
            .0
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.finished {
            return;
        }
        state.finished = true;
        self.0.recorder.emit("request_end", json!({"trace_id":self.0.trace_id,"outcome":outcome,
            "duration_us":micros(self.0.started.elapsed()),"span_count":state.next.min(SPANS),"dropped_spans":state.next.saturating_sub(SPANS),"counts":state.counts}));
    }
}
pub(crate) struct Wait {
    inner: Option<(Request, u64, &'static str, &'static str, Instant)>,
}
impl Drop for Wait {
    fn drop(&mut self) {
        if let Some((request, id, stage, counter, started)) = self.inner.take() {
            let elapsed = micros(started.elapsed());
            let mut state = request
                .0
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if state.counts.contains_key(counter) || state.counts.len() < 64 {
                let entry = state.counts.entry(counter).or_default();
                *entry = entry.saturating_add(elapsed);
            }
            request.0.recorder.emit("request_span", json!({"trace_id":request.0.trace_id,"span_id":id,"parent_span_id":null,
                "stage":stage,"start_us":micros(started.duration_since(request.0.recorder.epoch)),"inclusive_us":elapsed}));
        }
    }
}
pub(crate) fn current() -> Option<Request> {
    CURRENT.with_borrow(|current| current.as_ref().map(|c| c.request.clone()))
}
pub(crate) fn operation(name: &str) -> &'static str {
    super::tool_names()
        .iter()
        .find(|tool| tool.as_str() == name)
        .map_or("unknown", String::as_str)
}

pub(crate) struct Span {
    inner: Option<(Attached, u64, &'static str, Instant, Scope)>,
}
pub(crate) fn span(name: &'static str) -> Span {
    let inner = CURRENT.with_borrow(Clone::clone).and_then(|attached| {
        let id = {
            let mut state = attached
                .request
                .0
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            state.next += 1;
            if state.next > SPANS {
                return None;
            }
            state.next
        };
        let previous = CURRENT.replace(Some(Attached {
            request: attached.request.clone(),
            parent: Some(id),
        }));
        Some((
            attached,
            id,
            name,
            Instant::now(),
            Scope {
                previous,
                _thread: PhantomData,
            },
        ))
    });
    Span { inner }
}
impl Drop for Span {
    fn drop(&mut self) {
        if let Some((attached, id, name, started, _scope)) = self.inner.take() {
            attached.request.0.recorder.emit("request_span", json!({"trace_id":attached.request.0.trace_id,"span_id":id,"parent_span_id":attached.parent,
                "stage":name,"start_us":micros(started.duration_since(attached.request.0.recorder.epoch)),"inclusive_us":micros(started.elapsed())}));
        }
    }
}
pub(crate) fn count(name: &'static str, amount: u64) {
    CURRENT.with_borrow(|current| {
        if let Some(attached) = current {
            let mut state = attached
                .request
                .0
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if state.counts.contains_key(name) || state.counts.len() < 64 {
                let entry = state.counts.entry(name).or_default();
                *entry = entry.saturating_add(amount);
            }
        }
    });
}
fn micros(duration: Duration) -> u64 {
    u64::try_from(duration.as_micros()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests;
