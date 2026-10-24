//! Lane queues, worker threads, deadlines, cancellation, confirmation waits
//! and disconnection for one connection.
//!
//! The dispatcher thread owns all scheduling state. Workers only run
//! requests and send back an encoded reply or a confirmation step.

use super::{
    Admission, Confirm, Context, Handler, Options, Outcome, Outgoing, PROGRESS_BUFFER, Progress,
    Request, Resume, ServeError,
};
use crate::{
    control::Control,
    json::{self, EncodeError},
    rpc::{self, RpcError},
    session::{Decision, Session},
};
use serde::Serialize;
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet, VecDeque},
    panic::{self, AssertUnwindSafe},
    sync::{
        Arc,
        mpsc::{self, Receiver, Sender, SyncSender},
    },
    thread::{self, JoinHandle, Thread},
    time::{Duration, Instant},
};

/// What a queued or running request is, independent of its work.
#[derive(Clone)]
struct Meta {
    id: Option<Value>,
    lane: usize,
    key: Option<String>,
    timeout: Duration,
    read_only: bool,
    progress_token: Option<Value>,
}

enum Work {
    Call(Request),
    Resume(Resume, Decision),
}

struct Job {
    meta: Meta,
    work: Work,
    control: Arc<Control>,
    /// A continuation already owns its key from the step that asked to confirm.
    holds_key: bool,
}

struct Assignment {
    sequence: u64,
    id: Option<Value>,
    work: Work,
    control: Arc<Control>,
    progress: Option<Progress>,
}

enum Finish {
    Bytes(Vec<u8>),
    Reply(Value),
    Confirm(Confirm),
    Done,
}

struct Running {
    meta: Meta,
    worker: usize,
    control: Arc<Control>,
    /// The reply was settled by a deadline or cancellation; discard the outcome.
    answered: bool,
    grace: Option<Instant>,
}

struct Waiting {
    meta: Meta,
    resume: Resume,
    elicitation: Value,
    control: Control,
}

struct LaneState {
    capacity: usize,
    queue: VecDeque<Job>,
    idle: Vec<usize>,
    keys: HashSet<String>,
    running: usize,
    waiting: usize,
}

impl LaneState {
    fn used(&self) -> usize {
        self.queue.len() + self.running + self.waiting
    }

    fn release(&mut self, key: Option<&String>) {
        if let Some(key) = key {
            self.keys.remove(key);
        }
    }
}

struct Worker {
    jobs: Option<SyncSender<Assignment>>,
    thread: Option<JoinHandle<()>>,
}

pub(super) struct Scheduler {
    lanes: Vec<LaneState>,
    workers: Vec<Worker>,
    running: HashMap<u64, Running>,
    waiting: Option<Waiting>,
    finished: Receiver<(u64, Finish)>,
    progress: Receiver<(u64, Value)>,
    progress_sender: SyncSender<(u64, Value)>,
    dispatcher: Thread,
    sequence: u64,
    timeout: Duration,
    confirmation_timeout: Duration,
    disconnect_grace: Duration,
    max_output_bytes: usize,
    disconnected: bool,
}

impl Scheduler {
    pub(super) fn start<H: Handler>(
        handler: &Arc<H>,
        options: &Options,
        dispatcher: Thread,
    ) -> Result<Self, ServeError> {
        let (finished_sender, finished) = mpsc::channel();
        let (progress_sender, progress) = mpsc::sync_channel(PROGRESS_BUFFER);
        let mut scheduler = Self {
            lanes: Vec::new(),
            workers: Vec::new(),
            running: HashMap::new(),
            waiting: None,
            finished,
            progress,
            progress_sender,
            dispatcher,
            sequence: 0,
            timeout: options.timeout,
            confirmation_timeout: options.confirmation_timeout,
            disconnect_grace: options.disconnect_grace,
            max_output_bytes: options.max_output_bytes,
            disconnected: false,
        };
        for lane in &options.lanes {
            let mut idle = Vec::new();
            for _ in 0..lane.workers {
                let (jobs, incoming) = mpsc::sync_channel::<Assignment>(1);
                let handler = Arc::clone(handler);
                let finished = finished_sender.clone();
                let dispatcher = scheduler.dispatcher.clone();
                let limit = options.max_output_bytes;
                let thread = thread::Builder::new()
                    .name(format!("mcp-{}", lane.name))
                    .spawn(move || work(&*handler, &incoming, &finished, &dispatcher, limit))
                    .map_err(ServeError::Spawn)?;
                idle.push(scheduler.workers.len());
                scheduler.workers.push(Worker {
                    jobs: Some(jobs),
                    thread: Some(thread),
                });
            }
            scheduler.lanes.push(LaneState {
                capacity: lane.capacity,
                queue: VecDeque::new(),
                idle,
                keys: HashSet::new(),
                running: 0,
                waiting: 0,
            });
        }
        Ok(scheduler)
    }

    /// Queues a request, or refuses it when its lane is missing or full.
    pub(super) fn admit(
        &mut self,
        session: &mut Session,
        id: Option<Value>,
        request: Request,
        admission: Admission,
        read_only: bool,
        progress_token: Option<Value>,
    ) -> Vec<Outgoing> {
        let mut out = Vec::new();
        let Some(lane) = self.lanes.get_mut(admission.lane) else {
            refuse(
                session,
                id.as_ref(),
                rpc::INTERNAL_ERROR,
                "Request lane is not configured",
                &mut out,
            );
            return out;
        };
        if lane.used() >= lane.capacity {
            refuse(
                session,
                id.as_ref(),
                rpc::SERVER_ERROR,
                "Server request capacity reached",
                &mut out,
            );
            return out;
        }
        let timeout = admission.timeout.unwrap_or(self.timeout);
        lane.queue.push_back(Job {
            meta: Meta {
                id,
                lane: admission.lane,
                key: admission.key,
                timeout,
                read_only,
                progress_token,
            },
            work: Work::Call(request),
            control: Arc::new(Control::new(timeout)),
            holds_key: false,
        });
        out
    }

    /// Collects finished work and progress, expires deadlines and dispatches queued work.
    pub(super) fn settle(&mut self, session: &mut Session) -> Vec<Outgoing> {
        let mut out = Vec::new();
        // Progress first: a notification sent before its request finished
        // precedes the reply.
        while let Ok((sequence, notification)) = self.progress.try_recv() {
            if self
                .running
                .get(&sequence)
                .is_some_and(|running| !running.answered)
            {
                out.push(Outgoing::Value(notification));
            }
        }
        while let Ok((sequence, finish)) = self.finished.try_recv() {
            self.complete(session, sequence, finish, &mut out);
        }
        self.expire(session, &mut out);
        self.pump(session, &mut out);
        out
    }

    /// Stops a request the client cancelled. The session has already released its ID.
    pub(super) fn cancel(&mut self, session: &mut Session, id: &Value) -> Vec<Outgoing> {
        let mut out = Vec::new();
        for lane in &mut self.lanes {
            if let Some(position) = lane
                .queue
                .iter()
                .position(|job| job.meta.id.as_ref() == Some(id))
                && let Some(job) = lane.queue.remove(position)
                && job.holds_key
            {
                lane.release(job.meta.key.as_ref());
            }
        }
        for running in self.running.values_mut() {
            if running.meta.id.as_ref() == Some(id) && !running.answered {
                running.answered = true;
                running.control.cancel();
            }
        }
        if self
            .waiting
            .as_ref()
            .is_some_and(|waiting| waiting.meta.id.as_ref() == Some(id))
            && let Some(waiting) = self.take_waiting()
            && let Some(notice) = session.cancel_confirmation(&waiting.elicitation)
        {
            out.push(Outgoing::Value(notice));
        }
        out
    }

    /// Continues the request whose confirmation `elicitation` was answered.
    pub(super) fn confirmed(
        &mut self,
        session: &mut Session,
        elicitation: &Value,
        decision: Decision,
    ) -> Vec<Outgoing> {
        let mut out = Vec::new();
        if !self
            .waiting
            .as_ref()
            .is_some_and(|waiting| &waiting.elicitation == elicitation)
        {
            return out;
        }
        // An answer read after the deadline, even before the next deadline
        // sweep, does not resume; the session has already ended the form.
        if self
            .waiting
            .as_ref()
            .is_some_and(|waiting| waiting.control.expired())
        {
            if let Some(waiting) = self.take_waiting() {
                refuse(
                    session,
                    waiting.meta.id.as_ref(),
                    rpc::DEADLINE_EXCEEDED,
                    "Confirmation deadline exceeded",
                    &mut out,
                );
            }
            return out;
        }
        if let Some(waiting) = self.waiting.take() {
            self.lanes[waiting.meta.lane].waiting -= 1;
            self.queue_resume(waiting.meta, waiting.resume, decision);
        }
        out
    }

    /// Applies the end-of-input rules.
    pub(super) fn disconnect(&mut self, session: &mut Session) -> Vec<Outgoing> {
        let mut out = Vec::new();
        self.disconnected = true;
        if let Some(notice) = session.close() {
            out.push(Outgoing::Value(notice));
        }
        if let Some(waiting) = self.take_waiting() {
            refuse(
                session,
                waiting.meta.id.as_ref(),
                rpc::REQUEST_CANCELLED,
                "Connection closed before confirmation",
                &mut out,
            );
        }
        for lane in &mut self.lanes {
            let mut kept = VecDeque::new();
            for job in lane.queue.drain(..) {
                if job.meta.read_only {
                    kept.push_back(job);
                    continue;
                }
                if job.holds_key
                    && let Some(key) = &job.meta.key
                {
                    lane.keys.remove(key);
                }
                refuse(
                    session,
                    job.meta.id.as_ref(),
                    rpc::REQUEST_CANCELLED,
                    "Connection closed before dispatch",
                    &mut out,
                );
            }
            lane.queue = kept;
        }
        let grace = Instant::now() + self.disconnect_grace;
        for running in self.running.values_mut() {
            if !running.meta.read_only && !running.answered {
                running.grace = Some(grace);
            }
        }
        out
    }

    /// Whether no request is queued, running or awaiting confirmation.
    pub(super) fn idle(&self) -> bool {
        self.running.is_empty()
            && self.waiting.is_none()
            && self.lanes.iter().all(|lane| lane.queue.is_empty())
    }

    /// How long the dispatcher may sleep before a deadline needs attention.
    pub(super) fn next_wake(&self) -> Option<Duration> {
        let queued = self
            .lanes
            .iter()
            .flat_map(|lane| lane.queue.iter().map(|job| job.control.remaining()));
        let running = self.running.values().flat_map(|running| {
            let deadline = (!running.answered).then(|| running.control.remaining());
            let grace = running
                .grace
                .map(|grace| grace.saturating_duration_since(Instant::now()));
            deadline.into_iter().chain(grace)
        });
        let waiting = self
            .waiting
            .iter()
            .map(|waiting| waiting.control.remaining());
        queued.chain(running).chain(waiting).min()
    }

    /// Cancels all work and joins the workers.
    pub(super) fn shutdown(&mut self) {
        for running in self.running.values() {
            running.control.cancel();
        }
        for lane in &mut self.lanes {
            lane.queue.clear();
        }
        self.waiting = None;
        for worker in &mut self.workers {
            worker.jobs = None;
        }
        for worker in &mut self.workers {
            if let Some(thread) = worker.thread.take() {
                let _ = thread.join();
            }
        }
    }

    fn complete(
        &mut self,
        session: &mut Session,
        sequence: u64,
        finish: Finish,
        out: &mut Vec<Outgoing>,
    ) {
        let Some(running) = self.running.remove(&sequence) else {
            return;
        };
        // Work that returns after its deadline, even before the next deadline
        // sweep, is answered with the deadline error; its outcome is discarded.
        let expired = !running.answered && running.control.expired();
        let confirming = matches!(finish, Finish::Confirm(_)) && !running.answered && !expired;
        let lane = &mut self.lanes[running.meta.lane];
        lane.running -= 1;
        lane.idle.push(running.worker);
        if !confirming {
            lane.release(running.meta.key.as_ref());
        }
        if running.answered {
            return;
        }
        if expired {
            refuse(
                session,
                running.meta.id.as_ref(),
                rpc::DEADLINE_EXCEEDED,
                "Request deadline exceeded",
                out,
            );
            return;
        }
        let deliver = |session: &mut Session| {
            running
                .meta
                .id
                .as_ref()
                .is_some_and(|id| session.finish(id))
        };
        match finish {
            Finish::Bytes(bytes) => {
                if deliver(session) {
                    out.push(Outgoing::Bytes(bytes));
                }
            }
            Finish::Reply(reply) => {
                if deliver(session) {
                    out.push(Outgoing::Value(reply));
                }
            }
            Finish::Done => {
                deliver(session);
            }
            Finish::Confirm(confirm) => {
                self.begin_confirmation(session, running.meta, confirm, out);
            }
        }
    }

    fn begin_confirmation(
        &mut self,
        session: &mut Session,
        meta: Meta,
        confirm: Confirm,
        out: &mut Vec<Outgoing>,
    ) {
        // After the input ends no one can answer, and the continuation never runs.
        if self.disconnected {
            self.lanes[meta.lane].release(meta.key.as_ref());
            refuse(
                session,
                meta.id.as_ref(),
                rpc::REQUEST_CANCELLED,
                "Connection closed before confirmation",
                out,
            );
            return;
        }
        match session.request_confirmation(&confirm.message) {
            Ok((elicitation, request)) => {
                let Ok(bytes) = json::encode(&request, self.max_output_bytes) else {
                    // The host never saw this form, so withdraw it locally only.
                    session.cancel_confirmation(&elicitation);
                    self.queue_resume(meta, confirm.resume, Decision::Unavailable);
                    return;
                };
                out.push(Outgoing::Bytes(bytes));
                self.lanes[meta.lane].waiting += 1;
                self.waiting = Some(Waiting {
                    meta,
                    resume: confirm.resume,
                    elicitation,
                    control: Control::new(self.confirmation_timeout),
                });
            }
            Err(decision) => self.queue_resume(meta, confirm.resume, decision),
        }
    }

    fn queue_resume(&mut self, meta: Meta, resume: Resume, decision: Decision) {
        let control = Arc::new(Control::new(meta.timeout));
        let holds_key = meta.key.is_some();
        self.lanes[meta.lane].queue.push_front(Job {
            meta,
            work: Work::Resume(resume, decision),
            control,
            holds_key,
        });
    }

    fn take_waiting(&mut self) -> Option<Waiting> {
        let waiting = self.waiting.take()?;
        let lane = &mut self.lanes[waiting.meta.lane];
        lane.waiting -= 1;
        lane.release(waiting.meta.key.as_ref());
        Some(waiting)
    }

    fn expire(&mut self, session: &mut Session, out: &mut Vec<Outgoing>) {
        for lane in &mut self.lanes {
            let mut kept = VecDeque::new();
            for job in lane.queue.drain(..) {
                if !job.control.expired() {
                    kept.push_back(job);
                    continue;
                }
                if job.holds_key
                    && let Some(key) = &job.meta.key
                {
                    lane.keys.remove(key);
                }
                refuse(
                    session,
                    job.meta.id.as_ref(),
                    rpc::DEADLINE_EXCEEDED,
                    "Request deadline exceeded before dispatch",
                    out,
                );
            }
            lane.queue = kept;
        }
        let now = Instant::now();
        for running in self.running.values_mut() {
            if !running.answered && running.control.expired() {
                running.answered = true;
                running.control.cancel();
                refuse(
                    session,
                    running.meta.id.as_ref(),
                    rpc::DEADLINE_EXCEEDED,
                    "Request deadline exceeded",
                    out,
                );
            }
            if running.grace.is_some_and(|grace| now >= grace) {
                running.grace = None;
                running.control.cancel();
            }
        }
        if self
            .waiting
            .as_ref()
            .is_some_and(|waiting| waiting.control.expired())
            && let Some(waiting) = self.take_waiting()
        {
            if let Some(notice) = session.cancel_confirmation(&waiting.elicitation) {
                out.push(Outgoing::Value(notice));
            }
            refuse(
                session,
                waiting.meta.id.as_ref(),
                rpc::DEADLINE_EXCEEDED,
                "Confirmation deadline exceeded",
                out,
            );
        }
    }

    fn pump(&mut self, session: &mut Session, out: &mut Vec<Outgoing>) {
        let now = Instant::now();
        for lane in &mut self.lanes {
            while let Some(&worker) = lane.idle.last() {
                let Some(position) = lane.queue.iter().position(|job| {
                    job.holds_key
                        || job
                            .meta
                            .key
                            .as_ref()
                            .is_none_or(|key| !lane.keys.contains(key))
                }) else {
                    break;
                };
                let Some(job) = lane.queue.remove(position) else {
                    break;
                };
                self.sequence += 1;
                let sequence = self.sequence;
                let progress = job.meta.progress_token.clone().map(|token| Progress {
                    token,
                    sequence,
                    sender: self.progress_sender.clone(),
                    dispatcher: self.dispatcher.clone(),
                });
                let assignment = Assignment {
                    sequence,
                    id: job.meta.id.clone(),
                    work: job.work,
                    control: Arc::clone(&job.control),
                    progress,
                };
                lane.idle.pop();
                let sent = self.workers[worker]
                    .jobs
                    .as_ref()
                    .is_some_and(|jobs| jobs.send(assignment).is_ok());
                if !sent {
                    // A stopped worker leaves the idle list for good.
                    if job.holds_key {
                        lane.release(job.meta.key.as_ref());
                    }
                    refuse(
                        session,
                        job.meta.id.as_ref(),
                        rpc::INTERNAL_ERROR,
                        "Request worker is unavailable",
                        out,
                    );
                    continue;
                }
                lane.running += 1;
                if let Some(key) = &job.meta.key {
                    lane.keys.insert(key.clone());
                }
                let grace =
                    (self.disconnected && !job.meta.read_only).then(|| now + self.disconnect_grace);
                self.running.insert(
                    sequence,
                    Running {
                        meta: job.meta,
                        worker,
                        control: job.control,
                        answered: false,
                        grace,
                    },
                );
            }
        }
    }
}

impl Drop for Scheduler {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Sends an error reply for `id` if the session still expects one.
fn refuse(
    session: &mut Session,
    id: Option<&Value>,
    code: i32,
    message: &str,
    out: &mut Vec<Outgoing>,
) {
    if let Some(id) = id
        && session.finish(id)
    {
        out.push(Outgoing::Value(rpc::failure(
            id.clone(),
            &RpcError::new(code, message),
        )));
    }
}

fn work<H: Handler>(
    handler: &H,
    incoming: &Receiver<Assignment>,
    finished: &Sender<(u64, Finish)>,
    dispatcher: &Thread,
    limit: usize,
) {
    while let Ok(assignment) = incoming.recv() {
        let context = Context {
            id: assignment.id,
            control: assignment.control,
            progress: assignment.progress,
        };
        let resumed = matches!(assignment.work, Work::Resume(..));
        let outcome = panic::catch_unwind(AssertUnwindSafe(|| match assignment.work {
            Work::Call(request) => handler.call(request, &context),
            Work::Resume(resume, decision) => resume(decision, &context),
        }));
        let finish = settle(context.id.as_ref(), outcome, resumed, limit);
        if finished.send((assignment.sequence, finish)).is_err() {
            return;
        }
        dispatcher.unpark();
    }
}

fn settle(
    id: Option<&Value>,
    outcome: thread::Result<Outcome>,
    resumed: bool,
    limit: usize,
) -> Finish {
    // A notification's outcome, including a confirmation step, has no reply.
    let Some(id) = id else {
        return Finish::Done;
    };
    let internal = |message: &str| {
        Finish::Reply(rpc::failure(
            id.clone(),
            &RpcError::new(rpc::INTERNAL_ERROR, message),
        ))
    };
    match outcome {
        Err(_) => internal("Request failed"),
        Ok(Outcome::Tool(result)) => encoded(id, &result, limit),
        Ok(Outcome::Method(Ok(value))) => encoded(id, &value, limit),
        Ok(Outcome::Method(Err(error))) => Finish::Reply(rpc::failure(id.clone(), &error)),
        Ok(Outcome::Done) => internal("Request produced no reply"),
        Ok(Outcome::Confirm(_)) if resumed => internal("Request asked for confirmation twice"),
        Ok(Outcome::Confirm(confirm)) => Finish::Confirm(confirm),
    }
}

fn encoded(id: &Value, result: &(impl Serialize + ?Sized), limit: usize) -> Finish {
    match rpc::encode_success(id, result, limit, &|| Ok(())) {
        Ok(bytes) => Finish::Bytes(bytes),
        Err(EncodeError::TooLarge { .. } | EncodeError::Interrupted) => {
            Finish::Reply(rpc::failure(
                id.clone(),
                &RpcError::new(rpc::INTERNAL_ERROR, "Response exceeds output limit"),
            ))
        }
        Err(EncodeError::Invalid) => Finish::Reply(rpc::failure(
            id.clone(),
            &RpcError::new(rpc::INTERNAL_ERROR, "Cannot encode response"),
        )),
    }
}
