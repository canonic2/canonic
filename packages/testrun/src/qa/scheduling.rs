//! Independent target runtime ownership, ordered bounded admission and run barriers.
use super::{ReadContext, Run, Server, journal::Archive};
use crate::error::{Error, Result};
use futures_util::future::join_all;
#[cfg(test)]
mod tests;
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

#[derive(Default)]
struct Queue {
    entries: Mutex<(u64, VecDeque<u64>)>,
    changed: tokio::sync::Notify,
}
pub(crate) struct Ticket {
    queue: Arc<Queue>,
    id: u64,
}
impl Queue {
    fn reserve(self: &Arc<Self>, limit: usize) -> Result<Ticket> {
        let mut entries = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        if entries.1.len() >= limit {
            return Err(Error::new("OVERLOADED", "Target request queue is full")
                .details(json!({"admitted":false,"dispatched":false})));
        }
        entries.0 += 1;
        let id = entries.0;
        entries.1.push_back(id);
        Ok(Ticket {
            queue: self.clone(),
            id,
        })
    }
}
impl Ticket {
    pub(crate) async fn ready(&self) {
        loop {
            let changed = self.queue.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if self
                .queue
                .entries
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .1
                .front()
                == Some(&self.id)
            {
                return;
            }
            changed.await;
        }
    }
}
impl Drop for Ticket {
    fn drop(&mut self) {
        self.queue
            .entries
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .1
            .retain(|id| *id != self.id);
        self.queue.changed.notify_waiters();
    }
}
struct Lane {
    server: Arc<tokio::sync::Mutex<Server>>,
    queue: Arc<Queue>,
    idle: AtomicBool,
}
struct SessionRecord {
    target: String,
    run: String,
    goal: String,
    read: Option<ReadContext>,
    status: Value,
    archive: Archive,
}
#[derive(Default)]
struct State {
    lanes: HashMap<String, Arc<Lane>>,
    sessions: HashMap<String, SessionRecord>,
    runs: HashMap<String, Run>,
    closing: HashSet<String>,
    /// Runs whose cleanup barrier succeeded and await metadata finalization.
    settled: HashSet<String>,
    membership: HashMap<String, HashSet<String>>,
    failed_targets: HashSet<String>,
    failures: HashMap<String, Error>,
    resources: HashMap<(String, Resource, String), tokio::sync::OwnedSemaphorePermit>,
}
impl State {
    fn target_ready(&self, target: &str, name: &str) -> Result<()> {
        if self.failed_targets.contains(target)
            && !matches!(name, "session_close" | "session_status" | "command_status")
        {
            return Err(Error::new(
                "SESSION_CLOSING",
                "Target cleanup remains unfinished; finish the run or close the session",
            ));
        }
        Ok(())
    }
}
pub(crate) struct Runtimes {
    notifications: Mutex<Option<tokio::sync::mpsc::Sender<Value>>>,
    processing: super::reads::Processing,
    state: Mutex<State>,
    discovery: Arc<super::discovery::Registry>,
    sessions: Arc<tokio::sync::Semaphore>,
    previews: Arc<tokio::sync::Semaphore>,
    recordings: Arc<tokio::sync::Semaphore>,
}
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Resource {
    Preview,
    Recording,
}
pub(crate) struct Prepared {
    pub(crate) session_slot: Option<tokio::sync::OwnedSemaphorePermit>,
    pub(crate) resource_slot: Option<(Resource, tokio::sync::OwnedSemaphorePermit)>,
    pub(crate) server: Arc<tokio::sync::Mutex<Server>>,
    pub(crate) ticket: Ticket,
    pub(crate) target: String,
}
impl Runtimes {
    pub(crate) fn handles(&self, name: &str, args: &Value) -> bool {
        name == "session_open"
            || matches!(name, "developer_mode_reveal" | "developer_support_install")
            || super::Server::managed_operation(name, args)
            || args["sessionId"].as_str().is_some_and(|id| {
                self.state
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .sessions
                    .contains_key(id)
            })
    }
    pub(super) fn new(
        discovery: Arc<super::discovery::Registry>,
        processing: super::reads::Processing,
    ) -> Self {
        Self {
            notifications: Mutex::default(),
            processing,
            state: Mutex::default(),
            discovery,
            sessions: Arc::new(tokio::sync::Semaphore::new(64)),
            previews: Arc::new(tokio::sync::Semaphore::new(4)),
            recordings: Arc::new(tokio::sync::Semaphore::new(2)),
        }
    }
    pub(super) fn register(&self, id: &str, run: &Run) {
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .runs
            .insert(id.into(), run.clone());
    }
    pub(crate) fn prepare(
        &self,
        name: &str,
        args: &Value,
        client: &crate::managed::Client,
    ) -> Option<Result<Prepared>> {
        self.retire_idle();
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let target = if name == "session_open" {
            args["target"]["targetId"].as_str()?.to_owned()
        } else if matches!(name, "developer_mode_reveal" | "developer_support_install") {
            args["targetId"].as_str()?.to_owned()
        } else if let Some(session) = args["sessionId"].as_str() {
            state.sessions.get(session)?.target.clone()
        } else if matches!(name, "app_install" | "app_uninstall") && args["targetId"].is_string() {
            format!("application:{}", args["targetId"].as_str().unwrap())
        } else if super::Server::managed_operation(name, args) {
            // Each named device has its own lane, so calls for different
            // devices run concurrently; the owner resolves an omitted target.
            args["targetId"].as_str().map_or_else(
                || "managed-selection".into(),
                |target| format!("managed:{target}"),
            )
        } else {
            return None;
        };
        Some((|| {
            state.target_ready(&target, name)?;
            if name == "session_open"
                && !args["runId"]
                    .as_str()
                    .is_some_and(|run| state.runs.contains_key(run))
            {
                return Err(Error::new("RUN_NOT_FOUND", "Run is not active"));
            }
            if let Some(record) = args["sessionId"]
                .as_str()
                .and_then(|id| state.sessions.get(id))
            {
                Self::authorize(record, args)?;
            }
            if args["runId"]
                .as_str()
                .is_some_and(|run| state.closing.contains(run))
                && !matches!(name, "session_close" | "session_status" | "command_status")
            {
                return Err(Error::new(
                    "SESSION_CLOSING",
                    "The run is finishing; only status and cleanup are available",
                ));
            }
            if !state.lanes.contains_key(&target) {
                if state.lanes.len() >= 16 {
                    return Err(Error::new(
                        "RESOURCE_LIMIT",
                        "Connection already owns 16 target runtimes",
                    ));
                }
                state.lanes.insert(target.clone(), self.new_lane(client));
            }
            let lane = &state.lanes[&target];
            let ticket = lane.queue.reserve(8)?;
            let session_slot = if name == "session_open" {
                Some(self.sessions.clone().try_acquire_owned().map_err(|_| {
                    Error::new(
                        "RESOURCE_LIMIT",
                        "Connection already owns 64 admitted or active sessions",
                    )
                })?)
            } else {
                None
            };
            let server = lane.server.clone();
            let resource_slot = self.reserve_resource(name, args)?;
            if let Some(run) = args["runId"]
                .as_str()
                .filter(|run| state.runs.contains_key(*run))
            {
                state
                    .membership
                    .entry(run.into())
                    .or_default()
                    .insert(target.clone());
            }
            Ok(Prepared {
                session_slot,
                resource_slot,
                server,
                ticket,
                target,
            })
        })())
    }
    fn reserve_resource(
        &self,
        name: &str,
        args: &Value,
    ) -> Result<Option<(Resource, tokio::sync::OwnedSemaphorePermit)>> {
        let resource_slot = if args["sessionId"].is_string() && name == "preview_start" {
            Some((
                Resource::Preview,
                self.previews.clone().try_acquire_owned().map_err(|_| {
                    Error::new("RESOURCE_LIMIT", "Connection preview capacity is four")
                })?,
            ))
        } else if args["sessionId"].is_string() && name == "recording_start" {
            Some((
                Resource::Recording,
                self.recordings.clone().try_acquire_owned().map_err(|_| {
                    Error::new("RESOURCE_LIMIT", "Connection recording capacity is two")
                })?,
            ))
        } else {
            None
        };

        Ok(resource_slot)
    }
    fn new_lane(&self, client: &crate::managed::Client) -> Arc<Lane> {
        let mut server = Server::new();
        server.scoped_archive = true;
        if let Some(sender) = self
            .notifications
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
        {
            server.notifications(sender);
        }
        server.processing = self.processing.clone();
        server.discovery = self.discovery.clone();
        server.managed = client.clone();
        Arc::new(Lane {
            server: Arc::new(tokio::sync::Mutex::new(server)),
            queue: Arc::default(),
            idle: AtomicBool::new(true),
        })
    }
    pub(crate) fn set_idle(&self, target: &str, idle: bool) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(lane) = state.lanes.get(target) {
            lane.idle.store(idle, Ordering::Release);
        }
        if idle {
            state.failed_targets.remove(target);
        }
    }
    /// Called only at admission/completion boundaries. A verified empty owner
    /// can retire after its last ticket drops; failed and queued owners stay.
    pub(crate) fn retire_idle(&self) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let retired = state
            .lanes
            .iter()
            .filter(|(target, lane)| {
                lane.idle.load(Ordering::Acquire)
                    && lane
                        .queue
                        .entries
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .1
                        .is_empty()
                    && !state.failed_targets.contains(*target)
                    && !state.resources.keys().any(|(owner, _, _)| owner == *target)
                    && !state
                        .sessions
                        .values()
                        .any(|record| record.target == **target && record.status["closed"] != true)
            })
            .map(|(target, _)| target.clone())
            .collect::<Vec<_>>();
        for target in retired {
            state.lanes.remove(&target);
            for members in state.membership.values_mut() {
                members.remove(&target);
            }
            state.failures.remove(&target);
        }
    }
    pub(crate) fn populate(&self, server: &mut Server) {
        server
            .runs
            .clone_from(&self.state.lock().unwrap_or_else(|e| e.into_inner()).runs);
    }
    /// Revokes a session's reads when its close is dispatched. A close that is
    /// cancelled or expires while queued leaves the session unchanged.
    pub(crate) fn fence_close(&self, name: &str, args: &Value) {
        if name != "session_close" {
            return;
        }
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(record) = args["sessionId"]
            .as_str()
            .and_then(|id| state.sessions.get_mut(id))
        {
            record.read.take();
            record.status = json!({"state":"closing","closed":false});
        }
    }
    pub(crate) fn target_ready(&self, target: &str, name: &str) -> Result<()> {
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .target_ready(target, name)
    }
    pub(super) fn notifications(&self, sender: tokio::sync::mpsc::Sender<Value>) {
        *self.notifications.lock().unwrap_or_else(|e| e.into_inner()) = Some(sender);
    }
    pub(crate) fn publish(&self, target: &str, server: &Server) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let previews = server.resource_ids(Resource::Preview);
        let recordings = server.resource_ids(Resource::Recording);
        state.resources.retain(|(owner, kind, id), _| {
            owner != target
                || match kind {
                    Resource::Preview => previews.contains(id),
                    Resource::Recording => recordings.contains(id),
                }
        });
        let failure = state.failures.get(target).cloned();
        for (id, record) in state
            .sessions
            .iter_mut()
            .filter(|(_, r)| r.target == target && r.status["closed"] != true)
        {
            if !server.sessions.contains_key(id) {
                record.read.take();
                record.status = json!({"state":"closed","closed":true});
                if let Some(error) = &failure {
                    record.status["schedulingError"] = json!(error);
                }
            }
        }
        for (id, session) in &server.sessions {
            let args = json!({"runId":session.run_id,"goalId":session.goal_id,"sessionId":id});
            let fenced = state.closing.contains(&session.run_id)
                || state
                    .sessions
                    .get(id)
                    .is_some_and(|record| record.status["state"] == "failed");
            let read = if fenced {
                None
            } else {
                server.read_context(&args).ok()
            };
            let status = json!({"state":if session.cleanup_error.is_some(){"failed"}else if session.closing||fenced {"closing"}else{"ready"},"closed":false,"target":session.target,"controlPath":session.control_path,"cleanup":session.cleanup,"cleanupError":session.cleanup_error});
            state.sessions.insert(
                id.clone(),
                SessionRecord {
                    target: target.into(),
                    run: session.run_id.clone(),
                    goal: session.goal_id.clone(),
                    read,
                    status,
                    archive: server
                        .journal_archive(id)
                        .expect("Runtime has a durable journal"),
                },
            );
        }
        while state.sessions.len() > 192 {
            let Some(id) = state
                .sessions
                .iter()
                .find(|(_, record)| record.status["closed"] == true)
                .map(|(id, _)| id.clone())
            else {
                break;
            };
            state.sessions.remove(&id);
        }
    }
    pub(crate) fn retain_resource(
        &self,
        target: &str,
        server: &Server,
        before: &HashSet<String>,
        slot: Option<(Resource, tokio::sync::OwnedSemaphorePermit)>,
    ) {
        if let Some((kind, slot)) = slot
            && let Some(id) = server
                .resource_ids(kind)
                .into_iter()
                .find(|id| !before.contains(id))
        {
            self.state
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .resources
                .insert((target.into(), kind, id), slot);
        }
    }
    pub(crate) fn read(&self, args: &Value) -> Option<Result<ReadContext>> {
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let record = state.sessions.get(args["sessionId"].as_str()?)?;
        Some(Self::authorize(record, args).and_then(|()| {
            record
                .read
                .clone()
                .ok_or_else(|| Error::new("SESSION_CLOSING", "Session reads have been revoked"))
        }))
    }
    fn authorize(record: &SessionRecord, args: &Value) -> Result<()> {
        if args["runId"] != record.run || args["goalId"] != record.goal {
            return Err(Error::new(
                "SESSION_NOT_OWNED",
                "Session belongs to another run or goal",
            ));
        }
        Ok(())
    }
    pub(crate) fn status(&self, args: &Value) -> Option<Result<Value>> {
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let record = state.sessions.get(args["sessionId"].as_str()?)?;
        Some(Self::authorize(record, args).and_then(|()| {
            if let Some(command) = args["commandId"].as_str() {
                record.archive.status(command)
            } else {
                let mut status = record.status.clone();
                if record.status["closed"] != true
                    && let Some(error) = state.failures.get(&record.target)
                {
                    status["schedulingError"] = json!(error);
                }
                Ok(status)
            }
        }))
    }
    /// Whether `run.end` for this run still needs its target cleanup barrier.
    pub(crate) fn barrier_pending(&self, run: &str) -> bool {
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.runs.contains_key(run) && !state.settled.contains(run)
    }
    /// Fences the run and closes its targets concurrently. Independent of the
    /// root server, so one run's slow cleanup never delays another run's end.
    pub(crate) async fn finish_run(&self, run: &str) -> Result<()> {
        let (work, first_attempt) = {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            if state.settled.contains(run) {
                return Ok(());
            }
            let first_attempt = state.closing.insert(run.into());
            for record in state
                .sessions
                .values_mut()
                .filter(|r| r.run == run && r.status["closed"] != true)
            {
                record.read.take();
                record.status = json!({"state":"closing","closed":false});
            }
            // Include admitted startup even before it publishes a session.
            let work = state
                .lanes
                .iter()
                .filter(|(target, _)| {
                    state
                        .membership
                        .get(run)
                        .is_some_and(|members| members.contains(*target))
                })
                // Barriers bypass the eight-operation cap; the connection's
                // job limit bounds concurrent barriers on one target.
                .map(|(target, lane)| {
                    Ok((
                        target.clone(),
                        lane.server.clone(),
                        lane.queue.reserve(usize::MAX)?,
                    ))
                })
                .collect::<Result<Vec<_>>>()?;
            (work, first_attempt)
        };
        let results = join_all(work.into_iter().map(|(target, server, ticket)| async move {
            ticket.ready().await;
            let mut server = server.lock().await;
            let ids = server
                .sessions
                .iter()
                .filter(|(_, s)| s.run_id == run)
                .map(|(id, _)| id.clone())
                .collect::<Vec<_>>();
            let mut failures = Vec::new();
            for id in ids {
                // A run barrier must report a prior failed close even when
                // this attempt finishes releasing its remaining resources.
                if first_attempt && let Some(error) = server.sessions[&id].cleanup_error.clone() {
                    failures.push(error);
                }
                if let Err(error) = server.close_session(&id).await {
                    failures.push(error);
                }
            }
            if let Err(error) = server.cancel_startup().await {
                failures.push(error);
            }
            let idle = match server.retire_runtime().await {
                Ok(idle) => idle,
                Err(error) => {
                    failures.push(error);
                    false
                }
            };
            self.publish(&target, &server);
            self.released(&target, failures.is_empty());
            self.set_idle(&target, idle && failures.is_empty());
            if failures.is_empty() {
                server.runs.remove(run);
                Ok(())
            } else {
                Err(
                    Error::new("CLEANUP_FAILED", "Target cleanup remains unfinished")
                        .details(json!({"target":target,"failures":failures})),
                )
            }
        }))
        .await;
        self.retire_idle();
        let result = aggregate(results);
        if result.is_ok() {
            self.state
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .settled
                .insert(run.into());
        }
        result
    }
    pub(super) fn retire_run(&self, run: &str) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.runs.remove(run);
        state.settled.remove(run);
        state.membership.remove(run);
        state.closing.remove(run);
    }
    pub(super) fn pending_run(&self, run: &str) -> bool {
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.membership.get(run).is_some_and(|members| {
            members
                .iter()
                .any(|target| state.failed_targets.contains(target))
        }) || state
            .sessions
            .values()
            .any(|r| r.run == run && r.status["closed"] != true)
    }
    pub(super) async fn shutdown(&self) -> Result<()> {
        let work = {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            for record in state.sessions.values_mut() {
                record.read.take();
            }
            state
                .lanes
                .iter()
                .map(|(target, lane)| (target.clone(), lane.server.clone()))
                .collect::<Vec<_>>()
        };
        let results = join_all(work.into_iter().map(|(target, server)| async move {
            let mut server = server.lock().await;
            server.runs.clear();
            let result = Box::pin(server.shutdown()).await;
            self.publish(&target, &server);
            self.released(&target, result.is_ok());
            self.set_idle(&target, result.is_ok());
            result
        }))
        .await;
        self.retire_idle();
        aggregate(results)
    }
    fn released(&self, target: &str, complete: bool) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if complete {
            state.failed_targets.remove(target);
        } else {
            state.failed_targets.insert(target.into());
        }
    }
    pub(crate) fn failed(&self, target: &str, error: Error) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.failed_targets.insert(target.into());
        state.failures.insert(target.into(), error.clone());
        for record in state
            .sessions
            .values_mut()
            .filter(|record| record.target == target && record.status["closed"] != true)
        {
            record.read.take();
            record.status = json!({"state":"failed","closed":false,"cleanupError":error});
        }
    }
}
fn aggregate(results: Vec<Result<()>>) -> Result<()> {
    let failures = results
        .into_iter()
        .filter_map(Result::err)
        .collect::<Vec<_>>();
    if failures.is_empty() {
        Ok(())
    } else {
        Err(
            Error::new("CLEANUP_FAILED", "Independent target cleanup is unfinished")
                .details(json!({"failures":failures})),
        )
    }
}
impl Server {
    pub(crate) fn resource_ids(&self, kind: Resource) -> HashSet<String> {
        self.sessions
            .values()
            .flat_map(|session| match kind {
                Resource::Preview => session.streams.iter(),
                Resource::Recording => session.recordings.iter(),
            })
            .filter(|id| kind == Resource::Preview || self.recordings.active(id))
            .cloned()
            .collect()
    }
    pub(crate) fn session_ids(&self) -> HashSet<String> {
        self.sessions.keys().cloned().collect()
    }
    pub(crate) fn retain_session_slot(
        &mut self,
        existing: &HashSet<String>,
        slot: Option<tokio::sync::OwnedSemaphorePermit>,
    ) {
        if let Some((_, session)) = self
            .sessions
            .iter_mut()
            .find(|(id, _)| !existing.contains(*id))
        {
            session.scheduler_slot = slot;
        }
    }
    pub(crate) fn enable_scheduling(&mut self) {
        self.scheduling = Some(Arc::new(Runtimes::new(
            self.discovery.clone(),
            self.processing.clone(),
        )));
    }
    pub(crate) fn scheduling(&self) -> Option<Arc<Runtimes>> {
        self.scheduling.clone()
    }
    fn managed_operation(name: &str, args: &Value) -> bool {
        args.get("sessionId").is_none()
            && (crate::managed::selected_operation(name)
                || crate::playback::operation(name)
                || crate::network::operation(name)
                || matches!(
                    name,
                    "connect" | "act" | "observe" | "disconnect" | "app_install" | "app_uninstall"
                ))
    }
}
