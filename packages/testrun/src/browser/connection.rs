//! Concurrent driver connection with independent reads, writes and control lanes.
//!
//! Dropping a call never replays input. Queued cancellation prevents dispatch;
//! dispatched calls keep protocol ownership until a reply or epoch loss.

use super::{
    dispatcher::{self, Bootstrap, Dispatcher, Incoming},
    events::{self, Events, Item},
    objects::ObjectRef,
    schema, transport,
};
use serde_json::{Value, json};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU8, Ordering},
};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    sync::{Notify, Semaphore, mpsc, oneshot},
    time::Instant,
};

const QUEUED: u8 = 0;
const DISPATCHED: u8 = 1;
const CANCELLED: u8 = 2;
const COMPLETED: u8 = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dispatch {
    NotDispatched,
    PossiblyDispatched,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Loss {
    Eof,
    Read,
    Write,
    Protocol,
    Closed,
}

#[derive(Clone, Debug)]
pub struct Failure {
    pub summary: String,
    pub os_code: Option<i32>,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Driver call capacity is exhausted")]
    Busy,
    #[error("Driver version differs from the pinned binding")]
    Version,
    #[error("Driver call deadline expired ({dispatch:?})")]
    Timeout { dispatch: Dispatch },
    #[error("Driver connection was lost ({dispatch:?})")]
    Lost { dispatch: Dispatch },
    #[error("Playwright method failed; details are private diagnostics")]
    Remote(Box<dispatcher::RemoteError>),
    #[error(transparent)]
    Protocol(#[from] dispatcher::Error),
    #[error(transparent)]
    Bounds(#[from] schema::Error),
}

type Reply = Result<Option<Value>, Error>;

#[derive(Clone, Debug, Default)]
pub struct Status {
    pub ready: bool,
    pub pending: usize,
    pub loss: Option<Loss>,
    pub failure: Option<Failure>,
}

struct Shared {
    status: Mutex<Status>,
    bootstrap: Mutex<Option<Bootstrap>>,
    stopped: AtomicBool,
    closed: Notify,
    frames: super::frames::Lane,
}

struct Client {
    calls: mpsc::Sender<Call>,
    control: mpsc::Sender<Control>,
    slots: Arc<Semaphore>,
    shared: Arc<Shared>,
}

#[derive(Clone)]
pub struct Connection(Arc<Client>);

struct Call {
    object: ObjectRef,
    method: String,
    params: Value,
    deadline: Instant,
    state: Arc<AtomicU8>,
    reply: oneshot::Sender<Reply>,
    retain_reply: bool,
    location: Option<super::automation_debugger::Location>,
    internal: bool,
}

#[derive(Default)]
struct CallOptions {
    priority: bool,
    retain_reply: bool,
    location: Option<super::automation_debugger::Location>,
    internal: bool,
}

enum Control {
    OwnedBy(
        ObjectRef,
        ObjectRef,
        oneshot::Sender<Result<bool, dispatcher::Error>>,
    ),
    Children(
        ObjectRef,
        String,
        oneshot::Sender<Result<Vec<ObjectRef>, dispatcher::Error>>,
    ),
    EventBarrier(oneshot::Sender<()>),
    Call(Call),
    Resolve(
        String,
        String,
        oneshot::Sender<Result<ObjectRef, dispatcher::Error>>,
    ),
    Initializer(
        ObjectRef,
        oneshot::Sender<Result<Option<Value>, dispatcher::Error>>,
    ),
    Close(oneshot::Sender<()>),
}

struct Outbound {
    id: u64,
    message: Value,
    deadline: Instant,
    state: Arc<AtomicU8>,
}
enum Written {
    Skipped(u64),
    Failed(Failure),
}
struct Waiting {
    deadline: Instant,
    state: Arc<AtomicU8>,
    reply: Option<oneshot::Sender<Reply>>,
    retain_reply: bool,
    media_ack: bool,
    media_ack_expired: bool,
}

struct FrameAck {
    page: ObjectRef,
    id: Value,
    deadline: Instant,
}
struct Guard(Arc<AtomicU8>);
struct AbortWriter(tokio::task::JoinHandle<()>);
impl Drop for AbortWriter {
    fn drop(&mut self) {
        self.0.abort();
    }
}
impl Drop for Guard {
    fn drop(&mut self) {
        let _ = self
            .0
            .compare_exchange(QUEUED, CANCELLED, Ordering::SeqCst, Ordering::SeqCst);
    }
}

impl Connection {
    pub(crate) fn frame_lane(&self) -> super::frames::Lane {
        self.0.shared.frames.clone()
    }
    pub(crate) async fn event_barrier(&self, deadline: Instant) -> Result<(), Error> {
        let (send, receive) = oneshot::channel();
        tokio::time::timeout_at(deadline, self.0.control.send(Control::EventBarrier(send)))
            .await
            .map_err(|_| Error::Timeout {
                dispatch: Dispatch::NotDispatched,
            })?
            .map_err(|_| Error::Lost {
                dispatch: Dispatch::NotDispatched,
            })?;
        tokio::time::timeout_at(deadline, receive)
            .await
            .map_err(|_| Error::Timeout {
                dispatch: Dispatch::NotDispatched,
            })?
            .map_err(|_| Error::Lost {
                dispatch: Dispatch::NotDispatched,
            })
    }
    /// The version comes from the owned bundle's identity check, not caller
    /// guesswork or a remote protocol negotiation. No process is spawned here.
    pub async fn open<R, W>(
        reader: R,
        writer: W,
        version: &str,
        deadline: Instant,
    ) -> Result<(Self, Events), Error>
    where
        R: AsyncRead + Unpin + Send + 'static,
        W: AsyncWrite + Unpin + Send + 'static,
    {
        if version != super::DRIVER_VERSION {
            return Err(Error::Version);
        }
        Self::open_frames(
            transport::Reader::new(reader),
            transport::Writer::new(writer),
            deadline,
        )
        .await
    }

    pub(super) async fn open_server(
        socket: super::attachment::Socket,
        deadline: Instant,
    ) -> Result<(Self, Events), Error> {
        use futures_util::StreamExt;
        let (writer, reader) = socket.split();
        Self::open_frames(reader, writer, deadline).await
    }

    async fn open_frames(
        reader: impl transport::ReadFrame + Send + 'static,
        writer: impl transport::WriteFrame + Send + 'static,
        deadline: Instant,
    ) -> Result<(Self, Events), Error> {
        let dispatcher = Dispatcher::default();
        let root = dispatcher.root()?;
        let (calls, requests) = mpsc::channel(128);
        let (control, controls) = mpsc::channel(16);
        let shared = Arc::new(Shared {
            status: Mutex::new(Status::default()),
            bootstrap: Mutex::new(None),
            stopped: AtomicBool::new(false),
            closed: Notify::new(),
            frames: super::frames::Lane::default(),
        });
        let buffer = events::Buffer::default();
        let connection = Self(Arc::new(Client {
            calls,
            control,
            slots: Arc::new(Semaphore::new(128)),
            shared: shared.clone(),
        }));
        tokio::spawn(run(
            reader,
            writer,
            dispatcher,
            requests,
            controls,
            shared,
            buffer.clone(),
        ));
        let bootstrap = async {
            connection
                .call(
                    &root,
                    "initialize",
                    json!({"sdkLanguage":"javascript"}),
                    deadline,
                )
                .await?;
            connection.bootstrap()
        }
        .await;
        if let Err(error) = bootstrap {
            let _ = tokio::time::timeout_at(deadline, connection.close()).await;
            return Err(error);
        }
        Ok((connection, Events(buffer)))
    }

    pub fn status(&self) -> Status {
        self.0
            .shared
            .status
            .lock()
            .expect("Connection status")
            .clone()
    }

    pub fn bootstrap(&self) -> Result<Bootstrap, Error> {
        self.0
            .shared
            .bootstrap
            .lock()
            .expect("Bootstrap owner")
            .clone()
            .ok_or(Error::Lost {
                dispatch: Dispatch::NotDispatched,
            })
    }

    pub async fn call(
        &self,
        object: &ObjectRef,
        method: &str,
        params: Value,
        deadline: Instant,
    ) -> Reply {
        self.submit(object, method, params, deadline, CallOptions::default())
            .await
    }

    /// Only the dispatcher's explicit callback/ACK/cleanup allowlist can use the
    /// reserved pending slots and the priority writer lane.
    pub async fn respond(
        &self,
        object: &ObjectRef,
        method: &str,
        params: Value,
        deadline: Instant,
    ) -> Reply {
        self.submit(
            object,
            method,
            params,
            deadline,
            CallOptions {
                priority: true,
                ..Default::default()
            },
        )
        .await
    }

    /// Workflow-owned calls retain the actual terminal result after dispatch,
    /// including allocated objects arriving after the public deadline. The
    /// workflow must keep this future owned and provide its own status/deadline
    /// response; connection loss remains a terminal uncertain outcome.
    pub(crate) async fn retained(
        &self,
        object: &ObjectRef,
        method: &str,
        params: Value,
        deadline: Instant,
        responsive: bool,
    ) -> Reply {
        self.submit(
            object,
            method,
            params,
            deadline,
            CallOptions {
                priority: responsive,
                retain_reply: true,
                ..Default::default()
            },
        )
        .await
    }

    pub(crate) async fn retained_at(
        &self,
        object: &ObjectRef,
        method: &str,
        params: Value,
        deadline: Instant,
        location: super::automation_debugger::Location,
    ) -> Reply {
        location.validate().map_err(|_| schema::Error::Bounds)?;
        self.submit(
            object,
            method,
            params,
            deadline,
            CallOptions {
                retain_reply: true,
                location: Some(location),
                ..Default::default()
            },
        )
        .await
    }

    /// Host-owned probes must not consume the caller's SDK pause/step request.
    /// Deadlines and retained terminal reply ownership are unchanged.
    pub(crate) async fn retained_internal(
        &self,
        object: &ObjectRef,
        method: &str,
        params: Value,
        deadline: Instant,
        responsive: bool,
    ) -> Reply {
        self.submit(
            object,
            method,
            params,
            deadline,
            CallOptions {
                priority: responsive,
                internal: true,
                retain_reply: true,
                ..Default::default()
            },
        )
        .await
    }

    async fn submit(
        &self,
        object: &ObjectRef,
        method: &str,
        params: Value,
        deadline: Instant,
        options: CallOptions,
    ) -> Reply {
        if Instant::now() >= deadline {
            return Err(Error::Timeout {
                dispatch: Dispatch::NotDispatched,
            });
        }
        if deadline.saturating_duration_since(Instant::now()) > std::time::Duration::from_secs(900)
        {
            return Err(schema::Error::Bounds.into());
        }
        if method.len() > 128 {
            return Err(dispatcher::Error::Method.into());
        }
        schema::encoded_len(&params, dispatcher::MAX_ARGUMENT_BYTES)?;
        let _slot = if options.priority {
            None
        } else {
            Some(
                self.0
                    .slots
                    .clone()
                    .try_acquire_owned()
                    .map_err(|_| Error::Busy)?,
            )
        };
        let state = Arc::new(AtomicU8::new(QUEUED));
        let _guard = Guard(state.clone());
        let (reply, result) = oneshot::channel();
        let call = Call {
            object: object.clone(),
            method: method.into(),
            params,
            deadline,
            state: state.clone(),
            reply,
            retain_reply: options.retain_reply,
            location: options.location,
            internal: options.internal,
        };
        let sent = if options.priority {
            self.0
                .control
                .try_send(Control::Call(call))
                .map_err(|error| matches!(error, mpsc::error::TrySendError::Full(_)))
        } else {
            self.0
                .calls
                .try_send(call)
                .map_err(|error| matches!(error, mpsc::error::TrySendError::Full(_)))
        };
        sent.map_err(|full| if full { Error::Busy } else { lost(&state) })?;
        result.await.unwrap_or_else(|_| Err(lost(&state)))
    }

    pub async fn resolve(&self, guid: &str, expected: &str) -> Result<ObjectRef, Error> {
        if guid.len() > 1024 || expected.len() > 128 {
            return Err(dispatcher::Error::Message.into());
        }
        let (reply, result) = oneshot::channel();
        self.0
            .control
            .try_send(Control::Resolve(guid.into(), expected.into(), reply))
            .map_err(control_error)?;
        result
            .await
            .map_err(|_| Error::Lost {
                dispatch: Dispatch::NotDispatched,
            })?
            .map_err(Error::from)
    }

    pub async fn initializer(&self, object: &ObjectRef) -> Result<Option<Value>, Error> {
        let (reply, result) = oneshot::channel();
        self.0
            .control
            .try_send(Control::Initializer(object.clone(), reply))
            .map_err(control_error)?;
        result
            .await
            .map_err(|_| Error::Lost {
                dispatch: Dispatch::NotDispatched,
            })?
            .map_err(Error::from)
    }

    pub(crate) async fn owned_by(
        &self,
        child: &ObjectRef,
        parent: &ObjectRef,
    ) -> Result<bool, Error> {
        let (reply, result) = oneshot::channel();
        self.0
            .control
            .try_send(Control::OwnedBy(child.clone(), parent.clone(), reply))
            .map_err(control_error)?;
        result
            .await
            .map_err(|_| Error::Lost {
                dispatch: Dispatch::NotDispatched,
            })?
            .map_err(Error::from)
    }
    pub(crate) async fn children(
        &self,
        parent: &ObjectRef,
        kind: &str,
    ) -> Result<Vec<ObjectRef>, Error> {
        let (reply, result) = oneshot::channel();
        self.0
            .control
            .try_send(Control::Children(parent.clone(), kind.into(), reply))
            .map_err(control_error)?;
        result
            .await
            .map_err(|_| Error::Lost {
                dispatch: Dispatch::NotDispatched,
            })?
            .map_err(Error::from)
    }

    pub async fn close(&self) -> Result<(), Error> {
        if self.status().loss.is_some() {
            return self.wait_stopped().await;
        }
        let (reply, result) = oneshot::channel();
        if let Err(error) = self.0.control.try_send(Control::Close(reply)) {
            if self.status().loss.is_some() {
                return self.wait_stopped().await;
            }
            return Err(control_error(error));
        }
        match result.await {
            Ok(()) => Ok(()),
            Err(_) if self.status().loss.is_some() => self.wait_stopped().await,
            Err(_) => Err(Error::Lost {
                dispatch: Dispatch::NotDispatched,
            }),
        }
    }

    async fn wait_stopped(&self) -> Result<(), Error> {
        loop {
            let notified = self.0.shared.closed.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if self.0.shared.stopped.load(Ordering::Acquire) {
                return Ok(());
            }
            notified.await;
        }
    }
}

fn knowledge(state: &AtomicU8) -> Dispatch {
    if state.load(Ordering::SeqCst) == DISPATCHED {
        Dispatch::PossiblyDispatched
    } else {
        Dispatch::NotDispatched
    }
}
fn control_error(error: mpsc::error::TrySendError<Control>) -> Error {
    if matches!(error, mpsc::error::TrySendError::Full(_)) {
        Error::Busy
    } else {
        Error::Lost {
            dispatch: Dispatch::NotDispatched,
        }
    }
}
fn lost(state: &AtomicU8) -> Error {
    Error::Lost {
        dispatch: knowledge(state),
    }
}

async fn write(
    mut writer: impl transport::WriteFrame,
    mut normal: mpsc::Receiver<Outbound>,
    mut priority: mpsc::Receiver<Outbound>,
    completed: mpsc::Sender<Written>,
) {
    loop {
        let item = tokio::select! { biased;
            item = priority.recv() => item,
            item = normal.recv() => item,
        };
        let Some(mut item) = item else {
            return;
        };
        if Instant::now() >= item.deadline {
            let _ =
                item.state
                    .compare_exchange(QUEUED, CANCELLED, Ordering::SeqCst, Ordering::SeqCst);
        }
        if item
            .state
            .compare_exchange(QUEUED, DISPATCHED, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            if completed.send(Written::Skipped(item.id)).await.is_err() {
                return;
            }
            continue;
        }
        // Upstream operation timeout is call metadata, not a launch/action
        // parameter. Never send zero: upstream interprets that as no deadline.
        item.message["metadata"]["timeout"] = json!(
            item.deadline
                .saturating_duration_since(Instant::now())
                .as_millis()
                .clamp(1, 900_000) as u64
        );
        // Once a frame starts, its writer owns completion independently of the
        // public waiter. Expiring a private observation must not cut the pipe
        // halfway through a JSON frame and lose unrelated browser operations.
        // A blocked writer still has a bounded transport ceiling.
        let write_deadline = Instant::now() + std::time::Duration::from_secs(5);
        match tokio::time::timeout_at(write_deadline, writer.write_value(&item.message)).await {
            Ok(Ok(())) => {}
            failure => {
                let failure = match failure {
                    Ok(Err(error)) => transport_failure(&error),
                    _ => Failure {
                        summary: "Driver frame write deadline expired".into(),
                        os_code: None,
                    },
                };
                let _ = completed.send(Written::Failed(failure)).await;
                return;
            }
        }
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "One connection owner orders reads, admission, expiration and terminal loss without holding an async lock"
)]
async fn run(
    mut reader: impl transport::ReadFrame,
    writer: impl transport::WriteFrame + Send + 'static,
    mut dispatcher: Dispatcher,
    mut calls: mpsc::Receiver<Call>,
    mut controls: mpsc::Receiver<Control>,
    shared: Arc<Shared>,
    events: events::Buffer,
) {
    let (normal, normal_rx) = mpsc::channel(128);
    let (priority, priority_rx) = mpsc::channel(16);
    let (written, mut writes) = mpsc::channel(129);
    let mut writer = AbortWriter(tokio::spawn(write(writer, normal_rx, priority_rx, written)));
    let mut waiting = std::collections::BTreeMap::<u64, Waiting>::new();
    let mut frame_acks = std::collections::VecDeque::<FrameAck>::new();
    let mut closing = None;
    let mut failure = None;
    let loss = loop {
        if frame_acks
            .front()
            .is_some_and(|ack| Instant::now() >= ack.deadline)
        {
            shared.frames.fail("MEDIA_ACK_TIMEOUT");
            frame_acks.clear();
        }
        while waiting.values().filter(|call| call.media_ack).count() < 4 {
            let Some(ack) = frame_acks.front() else {
                break;
            };
            if Instant::now() >= ack.deadline {
                shared.frames.fail("MEDIA_ACK_TIMEOUT");
                frame_acks.clear();
                break;
            }
            match acknowledge_frame(&mut dispatcher, &priority, &mut waiting, ack) {
                Ok(()) => {
                    frame_acks.pop_front();
                }
                Err(Error::Busy | Error::Protocol(dispatcher::Error::Capacity)) => break,
                Err(Error::Protocol(dispatcher::Error::Object(
                    super::objects::Error::Stale | super::objects::Error::Unknown,
                ))) => {
                    frame_acks.pop_front();
                }
                Err(_) => {
                    shared.frames.fail("MEDIA_ACK_FAILED");
                    frame_acks.clear();
                    break;
                }
            }
        }
        *shared.status.lock().expect("Connection status") = Status {
            ready: dispatcher.bootstrap().is_some(),
            pending: dispatcher.pending_count(),
            loss: None,
            failure: None,
        };
        let deadline = waiting
            .values()
            .filter(|call| {
                (call.media_ack && !call.media_ack_expired)
                    || (call.reply.is_some()
                        && !(call.retain_reply && call.state.load(Ordering::SeqCst) == DISPATCHED))
            })
            .map(|call| call.deadline)
            .min()
            .into_iter()
            .chain(frame_acks.front().map(|ack| ack.deadline))
            .min()
            .unwrap_or_else(|| Instant::now() + std::time::Duration::from_secs(86400));
        let admitted = tokio::select! {
            message = reader.read_value() => {
                let message = match message {
                    Ok(Some(message)) => message, Ok(None) => break Loss::Eof,
                    Err(error) => { failure=Some(transport_failure(&error)); break Loss::Read; }
                };
                match dispatcher.dispatch(message) {
                    Ok(Incoming::Event(dispatcher::Event::Message { object, method, params })) if method == "screencastFrame" => {
                        let params = params.unwrap_or(json!({}));
                        let frame_id = params["frameId"].clone();
                        shared.frames.push(object.clone(), events.next_sequence(), params);
                        if frame_acks.len()>=32 { shared.frames.fail("MEDIA_ACK_CAPACITY"); }
                        else { frame_acks.push_back(FrameAck {page:object,id:frame_id,deadline:Instant::now()+std::time::Duration::from_secs(5)}); }
                    },
                    Ok(Incoming::Event(event)) => events.push(Item::Event(event)),
                    Ok(Incoming::Reply { id, result }) => {
                        let Some(mut call) = waiting.remove(&id) else { break Loss::Protocol; };
                        if call.state.load(Ordering::SeqCst) != DISPATCHED { break Loss::Protocol; }
                        *shared.bootstrap.lock().expect("Bootstrap owner") = dispatcher.bootstrap().cloned();
                        *shared.status.lock().expect("Connection status") = Status {
                            ready:dispatcher.bootstrap().is_some(), pending:dispatcher.pending_count(), loss:None,failure:None };
                        if call.media_ack {
                            if let Err(error)=&result && error.error["error"]["name"]!="TargetClosedError" { shared.frames.fail("MEDIA_ACK_FAILED"); }
                        } else if let Some(reply) = call.reply.take() {
                            let result = result.map_err(|error| Error::Remote(Box::new(error)));
                            if let Err(result) = reply.send(result) { events.push(Item::Settled { id, succeeded:result.is_ok() }); }
                        } else { events.push(Item::Settled { id, succeeded:result.is_ok() }); }
                        call.state.store(COMPLETED, Ordering::SeqCst);
                    }
                    Err(error) => { failure=Some(Failure { summary:error.to_string(),os_code:None }); break Loss::Protocol; }
                }
                None
            },
            control = controls.recv() => match control {
                Some(Control::Call(call)) => Some((call,true)),
                Some(Control::EventBarrier(reply)) => { events.push(Item::Barrier(reply)); None },
                Some(Control::Resolve(guid, kind, reply)) => { let _ = reply.send(dispatcher.resolve(&guid,&kind)); None },
                Some(Control::Initializer(object, reply)) => { let _ = reply.send(dispatcher.initializer(&object)); None },
                Some(Control::Children(object,kind,reply))=>{let _=reply.send(dispatcher.children(&object,&kind));None},
                Some(Control::OwnedBy(child,parent,reply))=>{let _=reply.send(dispatcher.owned_by(&child,&parent));None},
                Some(Control::Close(reply)) => { closing = Some(reply); break Loss::Closed; },
                None => break Loss::Closed,
            },
            call = calls.recv() => match call { Some(call) => Some((call,false)), None => break Loss::Closed },
            completion = writes.recv() => {
                match completion {
                    Some(Written::Skipped(id)) => {
                        dispatcher.retire_unsent(id);
                        if let Some(mut call) = waiting.remove(&id) {
                            if call.media_ack { shared.frames.fail("MEDIA_ACK_NOT_DISPATCHED"); }
                            if let Some(reply) = call.reply.take() { let _ = reply.send(Err(Error::Timeout { dispatch: Dispatch::NotDispatched })); }
                        }
                    }
                    Some(Written::Failed(error)) => { failure=Some(error); break Loss::Write; },
                    None => break Loss::Write,
                }
                None
            },
            () = tokio::time::sleep_until(deadline) => {
                for call in waiting.values_mut().filter(|call|call.media_ack && !call.media_ack_expired && Instant::now()>=call.deadline) {
                    call.media_ack_expired=true;
                    shared.frames.fail("MEDIA_ACK_TIMEOUT");
                }
                for call in waiting.values_mut().filter(|call| call.reply.is_some() && Instant::now() >= call.deadline) {
                    let _ = call.state.compare_exchange(QUEUED, CANCELLED, Ordering::SeqCst, Ordering::SeqCst);
                    if call.retain_reply && call.state.load(Ordering::SeqCst) == DISPATCHED { continue; }
                    if let Some(reply) = call.reply.take() { let _ = reply.send(Err(Error::Timeout { dispatch: knowledge(&call.state) })); }
                }
                None
            },
        };
        if let Some((mut call, responsive)) = admitted {
            if call.state.load(Ordering::SeqCst) == CANCELLED || Instant::now() >= call.deadline {
                let _ = call.reply.send(Err(Error::Timeout {
                    dispatch: Dispatch::NotDispatched,
                }));
                continue;
            }
            let result = if responsive {
                dispatcher.admit_responsive(
                    &call.object,
                    &call.method,
                    std::mem::take(&mut call.params),
                )
            } else {
                dispatcher.admit(&call.object, &call.method, std::mem::take(&mut call.params))
            };
            match result {
                Ok((id, mut message)) => {
                    if call.internal {
                        message["metadata"]["internal"] = json!(true);
                    }
                    if let Some(location) = call.location {
                        message["metadata"]["location"] = json!(location);
                    }
                    let output = Outbound {
                        id,
                        message,
                        deadline: call.deadline,
                        state: call.state.clone(),
                    };
                    let sender = if responsive { &priority } else { &normal };
                    if sender.try_send(output).is_err() {
                        dispatcher.retire_unsent(id);
                        let _ = call.reply.send(Err(Error::Busy));
                    } else {
                        waiting.insert(
                            id,
                            Waiting {
                                deadline: call.deadline,
                                state: call.state,
                                reply: Some(call.reply),
                                retain_reply: call.retain_reply,
                                media_ack: false,
                                media_ack_expired: false,
                            },
                        );
                    }
                }
                Err(error) => {
                    let _ = call.reply.send(Err(error.into()));
                }
            }
        }
    };
    dispatcher.close();
    *shared.bootstrap.lock().expect("Bootstrap owner") = None;
    *shared.status.lock().expect("Connection status") = Status {
        ready: false,
        pending: 0,
        loss: Some(loss),
        failure,
    };
    // Freeze dispatch knowledge before publishing outcomes. Until the writer
    // has stopped, a queued call may still cross the dispatch boundary.
    writer.0.abort();
    let _ = (&mut writer.0).await;
    for (_, mut call) in waiting {
        if let Some(reply) = call.reply.take() {
            let _ = reply.send(Err(lost(&call.state)));
        }
    }
    events.close();
    shared.frames.close();
    shared.stopped.store(true, Ordering::Release);
    shared.closed.notify_waiters();
    if let Some(reply) = closing {
        let _ = reply.send(());
    }
}

/// ACK after bounded frame admission, independent of decoding and viewers.
fn acknowledge_frame(
    dispatcher: &mut Dispatcher,
    writer: &mpsc::Sender<Outbound>,
    waiting: &mut std::collections::BTreeMap<u64, Waiting>,
    ack: &FrameAck,
) -> Result<(), Error> {
    let (id, message) =
        dispatcher.admit_responsive(&ack.page, "screencastFrameAck", json!({"frameId":ack.id}))?;
    let state = Arc::new(AtomicU8::new(QUEUED));
    let deadline = ack.deadline;
    let outbound = Outbound {
        id,
        message,
        deadline,
        state: state.clone(),
    };
    if writer.try_send(outbound).is_err() {
        dispatcher.retire_unsent(id);
        return Err(Error::Busy);
    }
    waiting.insert(
        id,
        Waiting {
            deadline,
            state,
            reply: None,
            retain_reply: false,
            media_ack: true,
            media_ack_expired: false,
        },
    );
    Ok(())
}

fn transport_failure(error: &transport::Error) -> Failure {
    Failure {
        summary: error.to_string(),
        os_code: match error {
            transport::Error::Io(io) => io.raw_os_error(),
            _ => None,
        },
    }
}

#[cfg(test)]
pub(crate) mod tests;
