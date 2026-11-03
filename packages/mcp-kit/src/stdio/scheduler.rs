//! Lane queues, worker threads, deadlines, cancellation, confirmation waits
//! and disconnection for one driver: a stdio connection or the HTTP server.
//!
//! The dispatcher thread owns all scheduling state. Workers only run
//! requests and send back an encoded reply or a confirmation step.

use super::observation::{FrameKind, ObservationKind, Settlement, Span, Stage, Trace};
use super::{
    Admission, Confirm, Context, Elicit, ElicitResume, ElicitationPolicy, Handler, Options,
    Outcome, Outgoing, PROGRESS_BUFFER, Progress, Request, Resume, ServeError,
};
use crate::{
    control::Control,
    json::{self, EncodeError},
    rpc::{self, RpcError},
    session::{Decision, Session},
};
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

#[cfg(test)]
mod tests;

/// What a queued or running request is, independent of its work.
#[derive(Clone)]
struct Meta {
    trace: Trace,
    reply_kind: ReplyKind,
    result_schema: Option<crate::ResultSchema>,
    admitted: Instant,
    rich_rounds: u8,
    rich_deadline: Option<Instant>,
    request_context: Option<Arc<crate::RequestContext>>,
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
    ElicitResume(ElicitResume, crate::elicitation::Answer),
}

struct Job {
    queued: Span,
    meta: Meta,
    work: Work,
    control: Arc<Control>,
    /// A continuation already owns its key from the step that asked to confirm.
    holds_key: bool,
}

#[derive(Clone, Copy)]
enum ReplyKind {
    Ordinary,
    Prompt,
    Completion,
}

struct Assignment {
    trace: Trace,
    reply_kind: ReplyKind,
    result_schema: Option<crate::ResultSchema>,
    request_context: Option<Arc<crate::RequestContext>>,
    sequence: u64,
    id: Option<Value>,
    work: Work,
    control: Arc<Control>,
    progress: Option<Progress>,
}

enum Finish {
    Elicit(Elicit),
    Bytes(Vec<u8>),
    Reply(Value),
    Confirm(Confirm),
    Done,
}

struct Running {
    execution: Span,
    meta: Meta,
    worker: usize,
    control: Arc<Control>,
    /// The reply was settled by a deadline or cancellation; discard the outcome.
    answered: bool,
    grace: Option<Instant>,
}

struct Waiting {
    interaction: Span,
    meta: Meta,
    resume: Continuation,
    elicitation: Value,
    control: Control,
}

enum Continuation {
    Confirm(Resume),
    Elicit(ElicitResume),
}

impl Continuation {
    fn closed_message(&self) -> &'static str {
        match self {
            Self::Confirm(_) => "Connection closed before confirmation",
            Self::Elicit(_) => "Connection closed before elicitation",
        }
    }

    fn deadline_message(&self) -> &'static str {
        match self {
            Self::Confirm(_) => "Confirmation deadline exceeded",
            Self::Elicit(_) => "Elicitation deadline exceeded",
        }
    }
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

pub(crate) struct Scheduler {
    elicitation_policy: ElicitationPolicy,
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

/// One submission binds work, protocol identity and diagnostic ownership.
pub(crate) struct Submission {
    pub(crate) trace: Trace,
    pub(crate) id: Option<Value>,
    pub(crate) request: Request,
    pub(crate) admission: Admission,
    pub(crate) read_only: bool,
    pub(crate) progress_token: Option<Value>,
}

impl Scheduler {
    pub(crate) fn start<H: Handler>(
        handler: &Arc<H>,
        options: &Options,
        dispatcher: Thread,
    ) -> Result<Self, ServeError> {
        let (finished_sender, finished) = mpsc::channel();
        let (progress_sender, progress) = mpsc::sync_channel(PROGRESS_BUFFER);
        let mut scheduler = Self {
            elicitation_policy: options.elicitation.clone(),
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
    pub(crate) fn admit(&mut self, session: &mut Session, submission: Submission) -> Vec<Outgoing> {
        let Submission {
            mut trace,
            id,
            request,
            admission,
            read_only,
            progress_token,
        } = submission;
        let mut out = Vec::new();
        trace.lane(admission.lane);
        let Some(lane) = self.lanes.get_mut(admission.lane) else {
            refuse(
                session,
                id.as_ref(),
                rpc::INTERNAL_ERROR,
                "Request lane is not configured",
                &mut out,
                &trace,
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
                &trace,
            );
            return out;
        }
        let timeout = admission.timeout.unwrap_or(self.timeout);
        lane.queue.push_back(Job {
            queued: trace.span(Stage::Queue),
            meta: Meta {
                trace,
                reply_kind: match &request {
                    Request::Prompt(_) => ReplyKind::Prompt,
                    Request::Completion(_) => ReplyKind::Completion,
                    _ => ReplyKind::Ordinary,
                },
                result_schema: match &request {
                    Request::Tool(call) => session.catalog().result_schema(&call.name),
                    _ => None,
                },
                admitted: Instant::now(),
                rich_rounds: 0,
                rich_deadline: None,
                request_context: id.as_ref().and_then(|id| session.request_context(id)),
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
    pub(crate) fn settle(&mut self, session: &mut Session) -> Vec<Outgoing> {
        let mut out = Vec::new();
        // Progress first, but cap the batch even if workers refill the channel.
        // Otherwise both the dispatcher turn and its outgoing Vec can grow
        // without bound before input, cancellation or deadlines are handled.
        // Progress left behind needs no extra wake-up. The channel holds at most
        // one drain's worth, so any item left after a full drain was enqueued
        // during it; Context::progress unparks after every enqueue, leaving a
        // token that makes the next park return immediately.
        for _ in 0..PROGRESS_BUFFER {
            let Ok((sequence, notification)) = self.progress.try_recv() else {
                break;
            };
            if let Some(running) = self.running.get(&sequence)
                && !running.answered
                && running.control.check().is_ok()
            {
                let frame = Outgoing::Value(notification)
                    .observed(&running.meta.trace, FrameKind::Progress);
                out.push(match &running.meta.id {
                    Some(id) => Outgoing::Routed(id.clone(), Box::new(frame)),
                    None => frame,
                });
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
    pub(crate) fn cancel(
        &mut self,
        session: &mut Session,
        id: &Value,
        source: &Trace,
    ) -> Vec<Outgoing> {
        let mut out = Vec::new();
        for lane in &mut self.lanes {
            if let Some(position) = lane
                .queue
                .iter()
                .position(|job| job.meta.id.as_ref() == Some(id))
                && let Some(job) = lane.queue.remove(position)
            {
                source.relate(&job.meta.trace);
                job.meta.trace.settle(Settlement::Cancelled);
                if job.holds_key {
                    lane.release(job.meta.key.as_ref());
                }
            }
        }
        for running in self.running.values_mut() {
            if running.meta.id.as_ref() == Some(id) && !running.answered {
                source.relate(&running.meta.trace);
                running.meta.trace.settle(Settlement::Cancelled);
                running.answered = true;
                running.control.cancel();
            }
        }
        if self
            .waiting
            .as_ref()
            .is_some_and(|waiting| waiting.meta.id.as_ref() == Some(id))
            && let Some(waiting) = self.take_waiting()
        {
            source.relate(&waiting.meta.trace);
            waiting.meta.trace.settle(Settlement::Cancelled);
            if let Some(notice) = session.cancel_confirmation(&waiting.elicitation) {
                out.push(
                    Outgoing::Value(notice).observed(&waiting.meta.trace, FrameKind::Cancellation),
                );
            }
        }
        out
    }

    /// Continues the request whose confirmation `elicitation` was answered.
    pub(crate) fn confirmed(
        &mut self,
        session: &mut Session,
        elicitation: &Value,
        decision: Decision,
        source: &Trace,
    ) -> Vec<Outgoing> {
        let mut out = Vec::new();
        if !self
            .waiting
            .as_ref()
            .is_some_and(|waiting| &waiting.elicitation == elicitation)
        {
            return out;
        }
        if let Some(wait) = &self.waiting {
            source.relate(&wait.meta.trace);
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
                    &waiting.meta.trace,
                );
            }
            return out;
        }
        if let Some(waiting) = self.waiting.take() {
            drop(waiting.interaction);
            self.lanes[waiting.meta.lane].waiting -= 1;
            if let Continuation::Confirm(resume) = waiting.resume {
                self.queue_resume(waiting.meta, resume, decision);
            }
        }
        out
    }

    pub(crate) fn elicited(
        &mut self,
        session: &mut Session,
        elicitation: &Value,
        answer: crate::elicitation::Answer,
        source: &Trace,
    ) -> Vec<Outgoing> {
        let mut out = Vec::new();
        if !self
            .waiting
            .as_ref()
            .is_some_and(|wait| &wait.elicitation == elicitation)
        {
            return out;
        }
        if let Some(wait) = &self.waiting {
            source.relate(&wait.meta.trace);
        }
        if self
            .waiting
            .as_ref()
            .is_some_and(|wait| wait.control.expired())
        {
            if let Some(wait) = self.take_waiting() {
                refuse(
                    session,
                    wait.meta.id.as_ref(),
                    rpc::DEADLINE_EXCEEDED,
                    "Elicitation deadline exceeded",
                    &mut out,
                    &wait.meta.trace,
                );
            }
        } else if let Some(wait) = self.waiting.take() {
            drop(wait.interaction);
            self.lanes[wait.meta.lane].waiting -= 1;
            if let Continuation::Elicit(resume) = wait.resume {
                self.queue_elicit_resume(wait.meta, resume, answer);
            }
        }
        out
    }

    /// Applies the end-of-input rules.
    pub(crate) fn disconnect(&mut self, session: &mut Session) -> Vec<Outgoing> {
        let mut out = Vec::new();
        self.disconnected = true;
        if let Some(notice) = session.close() {
            let trace = self
                .waiting
                .as_ref()
                .map(|wait| wait.meta.trace.clone())
                .unwrap_or_default();
            out.push(Outgoing::Value(notice).observed(&trace, FrameKind::Cancellation));
        }
        if let Some(waiting) = self.take_waiting() {
            refuse(
                session,
                waiting.meta.id.as_ref(),
                rpc::REQUEST_CANCELLED,
                waiting.resume.closed_message(),
                &mut out,
                &waiting.meta.trace,
            );
        }
        for lane in &mut self.lanes {
            let mut kept = VecDeque::new();
            for job in lane.queue.drain(..) {
                if job.meta.read_only && !matches!(job.work, Work::ElicitResume(..)) {
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
                    &job.meta.trace,
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
    pub(crate) fn idle(&self) -> bool {
        self.running.is_empty()
            && self.waiting.is_none()
            && self.lanes.iter().all(|lane| lane.queue.is_empty())
    }

    /// How long the dispatcher may sleep before a deadline needs attention.
    pub(crate) fn next_wake(&self) -> Option<Duration> {
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
    pub(crate) fn shutdown(&mut self) {
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
        while let Ok((sequence, _)) = self.finished.try_recv() {
            if let Some(running) = self.running.get(&sequence) {
                running
                    .meta
                    .trace
                    .event(ObservationKind::End(Stage::OutputQueue));
                running.meta.trace.event(ObservationKind::Discarded);
            }
        }
        self.running.clear();
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
        drop(running.execution);
        running
            .meta
            .trace
            .event(ObservationKind::End(Stage::OutputQueue));
        // Work that returns after its deadline, even before the next deadline
        // sweep, is answered with the deadline error; its outcome is discarded.
        let expired = !running.answered && running.control.expired();
        let confirming = matches!(finish, Finish::Confirm(_) | Finish::Elicit(_))
            && !running.answered
            && !expired;
        let lane = &mut self.lanes[running.meta.lane];
        lane.running -= 1;
        lane.idle.push(running.worker);
        if !confirming {
            lane.release(running.meta.key.as_ref());
        }
        if running.answered {
            running.meta.trace.event(ObservationKind::Discarded);
            return;
        }
        if expired {
            running.meta.trace.event(ObservationKind::Discarded);
            refuse(
                session,
                running.meta.id.as_ref(),
                rpc::DEADLINE_EXCEEDED,
                "Request deadline exceeded",
                out,
                &running.meta.trace,
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
                running.meta.trace.settle(Settlement::Reply);
                if deliver(session) {
                    out.push(
                        Outgoing::Bytes(bytes).observed(&running.meta.trace, FrameKind::Reply),
                    );
                }
            }
            Finish::Reply(reply) => {
                running.meta.trace.settle(Settlement::Reply);
                if deliver(session) {
                    out.push(
                        Outgoing::Value(reply).observed(&running.meta.trace, FrameKind::Reply),
                    );
                }
            }
            Finish::Done => {
                running.meta.trace.settle(Settlement::NoReply);
                deliver(session);
            }
            Finish::Confirm(confirm) => {
                self.begin_confirmation(session, running.meta, confirm, out);
            }
            Finish::Elicit(elicit) => self.begin_elicitation(session, running.meta, elicit, out),
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
                &meta.trace,
            );
            return;
        }
        match session.request_confirmation(&confirm.message) {
            Ok((elicitation, request)) => {
                let encoded = {
                    let _encode = meta.trace.span(Stage::Encode);
                    json::encode(&request, self.max_output_bytes)
                };
                let Ok(bytes) = encoded else {
                    meta.trace.event(ObservationKind::EncodeFailed);
                    // The host never saw this form, so withdraw it locally only.
                    session.cancel_confirmation(&elicitation);
                    self.queue_resume(meta, confirm.resume, Decision::Unavailable);
                    return;
                };
                let interaction = meta.trace.span(Stage::Interaction);
                out.push(Outgoing::Bytes(bytes).observed(&meta.trace, FrameKind::Interaction));
                self.lanes[meta.lane].waiting += 1;
                self.waiting = Some(Waiting {
                    interaction,
                    meta,
                    resume: Continuation::Confirm(confirm.resume),
                    elicitation,
                    control: Control::new(self.confirmation_timeout),
                });
            }
            Err(decision) => self.queue_resume(meta, confirm.resume, decision),
        }
    }

    fn queue_resume(&mut self, meta: Meta, resume: Resume, decision: Decision) {
        let control = resume_control(&meta);
        let holds_key = meta.key.is_some();
        self.lanes[meta.lane].queue.push_front(Job {
            queued: meta.trace.span(Stage::Queue),
            meta,
            work: Work::Resume(resume, decision),
            control,
            holds_key,
        });
    }

    fn begin_elicitation(
        &mut self,
        session: &mut Session,
        mut meta: Meta,
        step: Elicit,
        out: &mut Vec<Outgoing>,
    ) {
        meta.rich_deadline = Some(meta.admitted + self.elicitation_policy.timeout);
        let deadline = meta.rich_deadline.unwrap_or(meta.admitted);
        if self.disconnected
            || Instant::now() >= deadline
            || meta.rich_rounds >= self.elicitation_policy.max_rounds
        {
            self.lanes[meta.lane].release(meta.key.as_ref());
            let (code, message) = if self.disconnected {
                (
                    rpc::REQUEST_CANCELLED,
                    "Connection closed before elicitation",
                )
            } else if Instant::now() >= deadline {
                (
                    rpc::DEADLINE_EXCEEDED,
                    "Elicitation workflow deadline exceeded",
                )
            } else {
                (rpc::INTERNAL_ERROR, "Elicitation round limit exceeded")
            };
            refuse(session, meta.id.as_ref(), code, message, out, &meta.trace);
            return;
        }
        meta.rich_rounds += 1;
        match session.request_elicitation(step.question) {
            Ok((elicitation, request)) => {
                let encoded = {
                    let _encode = meta.trace.span(Stage::Encode);
                    json::encode(&request, self.max_output_bytes)
                };
                let Ok(bytes) = encoded else {
                    meta.trace.event(ObservationKind::EncodeFailed);
                    session.cancel_confirmation(&elicitation);
                    self.queue_elicit_resume(
                        meta,
                        step.resume,
                        crate::elicitation::Answer::Unavailable,
                    );
                    return;
                };
                let interaction = meta.trace.span(Stage::Interaction);
                out.push(Outgoing::Bytes(bytes).observed(&meta.trace, FrameKind::Interaction));
                self.lanes[meta.lane].waiting += 1;
                self.waiting = Some(Waiting {
                    interaction,
                    meta,
                    resume: Continuation::Elicit(step.resume),
                    elicitation,
                    control: Control::until(deadline.min(
                        Instant::now() + self.confirmation_timeout.min(crate::control::MAX_TIMEOUT),
                    )),
                });
            }
            Err(answer) => self.queue_elicit_resume(meta, step.resume, answer),
        }
    }

    fn queue_elicit_resume(
        &mut self,
        meta: Meta,
        resume: ElicitResume,
        answer: crate::elicitation::Answer,
    ) {
        let control = resume_control(&meta);
        let holds_key = meta.key.is_some();
        self.lanes[meta.lane].queue.push_front(Job {
            queued: meta.trace.span(Stage::Queue),
            meta,
            work: Work::ElicitResume(resume, answer),
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
                    &job.meta.trace,
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
                    &running.meta.trace,
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
                out.push(
                    Outgoing::Value(notice).observed(&waiting.meta.trace, FrameKind::Cancellation),
                );
            }
            refuse(
                session,
                waiting.meta.id.as_ref(),
                rpc::DEADLINE_EXCEEDED,
                waiting.resume.deadline_message(),
                out,
                &waiting.meta.trace,
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
                let Some(mut job) = lane.queue.remove(position) else {
                    break;
                };
                drop(job.queued);
                let Some(sequence) = self.sequence.checked_add(1) else {
                    if job.holds_key {
                        lane.release(job.meta.key.as_ref());
                    }
                    refuse(
                        session,
                        job.meta.id.as_ref(),
                        rpc::INTERNAL_ERROR,
                        "Request assignment limit reached",
                        out,
                        &job.meta.trace,
                    );
                    continue;
                };
                self.sequence = sequence;
                job.meta.trace = job.meta.trace.assigned(sequence, worker);
                let execution = job.meta.trace.span(Stage::Worker);
                let progress = job.meta.progress_token.clone().map(|token| Progress {
                    token,
                    sequence,
                    sender: self.progress_sender.clone(),
                    dispatcher: self.dispatcher.clone(),
                });
                let assignment = Assignment {
                    trace: job.meta.trace.clone(),
                    reply_kind: job.meta.reply_kind,
                    result_schema: job.meta.result_schema.clone(),
                    request_context: job.meta.request_context.clone(),
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
                        &job.meta.trace,
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
                        execution,
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
    trace: &Trace,
) {
    trace.settle(match code {
        rpc::DEADLINE_EXCEEDED => Settlement::Deadline,
        rpc::REQUEST_CANCELLED => Settlement::Disconnected,
        _ => Settlement::Refused,
    });
    if let Some(id) = id
        && session.finish(id)
    {
        out.push(
            Outgoing::Value(rpc::failure(id.clone(), &RpcError::new(code, message)))
                .observed(trace, FrameKind::Reply),
        );
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
            result_schema: assignment.result_schema,
            protocol: assignment.request_context,
            id: assignment.id,
            control: assignment.control,
            progress: assignment.progress,
        };
        let rich_resumed = matches!(assignment.work, Work::ElicitResume(..));
        let resumed = rich_resumed || matches!(assignment.work, Work::Resume(..));
        let outcome = panic::catch_unwind(AssertUnwindSafe(|| {
            let outcome = {
                let _handler = (!matches!(&assignment.work, Work::Call(Request::Resource(_))))
                    .then(|| {
                        assignment
                            .trace
                            .span_with_parent(Stage::Handler, Stage::Worker)
                    });
                match assignment.work {
                    Work::Call(Request::Resource(read)) => Outcome::Bundled(read),
                    Work::Call(request) => handler.call(request, &context),
                    Work::Resume(resume, decision) => resume(decision, &context),
                    Work::ElicitResume(resume, answer) => resume(answer, &context),
                }
            };
            let outcome = checked_outcome(assignment.reply_kind, outcome);
            let outcome = match outcome {
                Outcome::Confirm(confirm) if context.protocol.is_some() => {
                    let _handler = assignment
                        .trace
                        .span_with_parent(Stage::Handler, Stage::Worker);
                    (confirm.resume)(Decision::Unavailable, &context)
                }
                outcome => outcome,
            };
            let _validation = assignment
                .trace
                .span_with_parent(Stage::Validation, Stage::Worker);
            let outcome = checked_outcome(assignment.reply_kind, outcome);
            match (&context.result_schema, outcome) {
                (Some(schema), Outcome::Tool(result)) if context.protocol.is_none() => match schema
                    .validate_checked(&result, false, &|| context.check())
                {
                    Ok(()) => Outcome::Tool(result),
                    Err(error) if error.kind == crate::schema::ArgumentErrorKind::Interrupted => {
                        Outcome::Done
                    }
                    Err(error) => Outcome::Method(Err(RpcError::new(
                        rpc::INTERNAL_ERROR,
                        if error.kind == crate::schema::ArgumentErrorKind::Limit {
                            "Tool result exceeds schema validation limits"
                        } else {
                            "Tool result does not match its output schema"
                        },
                    ))),
                },
                (
                    Some(_),
                    Outcome::Method(Ok(_))
                    | Outcome::Resource(_)
                    | Outcome::ResourceWithCache(..)
                    | Outcome::Prompt(_)
                    | Outcome::Completion(_)
                    | Outcome::Bundled(_),
                ) => Outcome::Method(Err(RpcError::new(
                    rpc::INTERNAL_ERROR,
                    "Tool call requires a typed tool result",
                ))),
                (_, outcome) => outcome,
            }
        }));
        if outcome.is_err() {
            assignment.trace.event(ObservationKind::Panicked);
        }
        let encoding = assignment
            .trace
            .span_with_parent(Stage::Encode, Stage::Worker);
        let finish = settle(
            context.id.as_ref(),
            outcome,
            resumed || context.protocol.is_some(),
            limit,
            context.protocol.as_deref(),
            rich_resumed,
            &assignment.trace,
        );
        drop(encoding);
        assignment
            .trace
            .event(ObservationKind::Start(Stage::OutputQueue));
        if finished.send((assignment.sequence, finish)).is_err() {
            return;
        }
        dispatcher.unpark();
    }
}

fn checked_outcome(kind: ReplyKind, outcome: Outcome) -> Outcome {
    let valid = matches!(
        (&kind, &outcome),
        (_, Outcome::Method(Err(_)) | Outcome::Task(Err(_)))
            | (ReplyKind::Ordinary, _)
            | (
                ReplyKind::Prompt,
                Outcome::Prompt(_)
                    | Outcome::Confirm(_)
                    | Outcome::Elicit(_)
                    | Outcome::InputRequired(_),
            )
            | (ReplyKind::Completion, Outcome::Completion(_))
    );
    if valid {
        return outcome;
    }
    Outcome::Method(Err(RpcError::new(
        rpc::INTERNAL_ERROR,
        match kind {
            ReplyKind::Completion => "Completion requires a typed complete result",
            _ => "Prompt retrieval requires a typed prompt result",
        },
    )))
}

fn settle(
    id: Option<&Value>,
    outcome: thread::Result<Outcome>,
    resumed: bool,
    limit: usize,
    context: Option<&crate::RequestContext>,
    rich_resumed: bool,
    trace: &Trace,
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
        Ok(Outcome::Task(Ok(result))) => match context {
            Some(context) => match context.validate_task_result(&result).and_then(|()| {
                context
                    .encode_task_payload(id, &result, limit, &|| Ok(()))
                    .map_err(|_| {
                        trace.event(ObservationKind::EncodeFailed);
                        RpcError::new(
                            rpc::INTERNAL_ERROR,
                            "Cannot encode task response within its output limit",
                        )
                    })
            }) {
                Ok(bytes) => Finish::Bytes(bytes),
                Err(error) => Finish::Reply(rpc::failure(id.clone(), &error)),
            },
            None => internal("Tasks require modern protocol metadata"),
        },
        Ok(Outcome::Tool(result)) => encoded(id, &result, limit, context, trace),
        Ok(Outcome::Completion(result)) => encoded(id, &result, limit, context, trace),
        Ok(Outcome::Prompt(result)) => match context {
            Some(context) => encoded(id, &result, limit, Some(context), trace),
            None => {
                if let Ok(bytes) = result.encode_legacy(id, limit, &|| Ok(())) {
                    Finish::Bytes(bytes)
                } else {
                    trace.event(ObservationKind::EncodeFailed);
                    internal("Cannot encode prompt result for legacy request")
                }
            }
        },
        Ok(Outcome::Resource(result)) => encoded(id, &result, limit, context, trace),
        Ok(Outcome::Bundled(read)) => encoded(id, read.result(), limit, context, trace),
        Ok(Outcome::ResourceWithCache(result, policy)) => {
            encoded_with_cache(id, &result, limit, context, Some(policy), trace)
        }
        Ok(Outcome::ResourceNotFound(uri)) => {
            let code = if context.is_some() {
                rpc::INVALID_PARAMS
            } else {
                rpc::RESOURCE_NOT_FOUND
            };
            Finish::Reply(rpc::failure(
                id.clone(),
                &RpcError::new(code, "Resource not found")
                    .with_data(serde_json::json!({"uri":uri})),
            ))
        }
        Ok(Outcome::InputRequired(result)) => match context {
            Some(context) => match context.interim(&result) {
                Ok(value) => encoded(id, &value, limit, None, trace),
                Err(error) => Finish::Reply(rpc::failure(id.clone(), &error)),
            },
            None => internal("Input-required results require modern protocol metadata"),
        },
        Ok(Outcome::Method(Ok(value))) => {
            if context.is_some()
                && ["resultType", "ttlMs", "cacheScope"]
                    .iter()
                    .any(|key| value.get(*key).is_some())
            {
                return internal("Method result overrides protocol-owned fields");
            }
            encoded(id, &value, limit, context, trace)
        }
        Ok(Outcome::Task(Err(error)) | Outcome::Method(Err(error))) => match context {
            Some(context) => {
                if let Ok(bytes) = context.encode_error(id, &error, limit, &|| Ok(())) {
                    Finish::Bytes(bytes)
                } else {
                    trace.event(ObservationKind::EncodeFailed);
                    internal("Cannot encode error response within its output limit")
                }
            }
            None => Finish::Reply(rpc::failure(id.clone(), &error)),
        },
        Ok(Outcome::Done) => internal("Request produced no reply"),
        Ok(Outcome::Elicit(_)) if context.is_some() || (resumed && !rich_resumed) => internal(
            "Use typed interim input for modern calls; confirmation cannot resume into rich input",
        ),
        Ok(Outcome::Elicit(step)) => Finish::Elicit(step),
        Ok(Outcome::Confirm(_)) if rich_resumed => {
            internal("Rich elicitation cannot resume into legacy confirmation")
        }
        Ok(Outcome::Confirm(_)) if resumed => internal("Request asked for confirmation twice"),
        Ok(Outcome::Confirm(confirm)) => Finish::Confirm(confirm),
    }
}

fn resume_control(meta: &Meta) -> Arc<Control> {
    let deadline = Instant::now() + meta.timeout.min(crate::control::MAX_TIMEOUT);
    Arc::new(Control::until(
        meta.rich_deadline
            .map_or(deadline, |total| total.min(deadline)),
    ))
}

fn encoded(
    id: &Value,
    result: &(impl crate::modern::CompletePayload + ?Sized),
    limit: usize,
    context: Option<&crate::RequestContext>,
    trace: &Trace,
) -> Finish {
    encoded_with_cache(id, result, limit, context, None, trace)
}

fn encoded_with_cache(
    id: &Value,
    result: &(impl crate::modern::CompletePayload + ?Sized),
    limit: usize,
    context: Option<&crate::RequestContext>,
    cache: Option<crate::CachePolicy>,
    trace: &Trace,
) -> Finish {
    let encoded = context.map_or_else(
        || rpc::encode_success(id, result, limit, &|| Ok(())),
        |context| context.encode_with_cache(id, result, cache, limit, &|| Ok(())),
    );
    if encoded.is_err() {
        trace.event(ObservationKind::EncodeFailed);
    }
    match encoded {
        Err(EncodeError::ValidationLimit) => Finish::Reply(rpc::failure(
            id.clone(),
            &RpcError::new(
                rpc::INTERNAL_ERROR,
                "Tool result exceeds schema validation limits",
            ),
        )),
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
