//! Retained Playwright context tracing, real archive chunks and scoped groups.
use super::*;
use crate::browser::{artifacts::Format, objects::ObjectRef, tracing::Options};
use std::{io::Read, path::PathBuf};
use tokio::sync::{mpsc, oneshot};

const ARCHIVE_BYTES: usize = 32 * 1024 * 1024;
const SOURCE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_CHUNKS: u64 = 4;
struct Command {
    id: Uuid,
    request: Request,
    reply: oneshot::Sender<Result<Value, Error>>,
}
pub(super) struct Trace {
    context: Reference,
    info: Mutex<Value>,
    commands: mpsc::Sender<Command>,
    pub(super) busy: AtomicBool,
    stop: AtomicBool,
    pub(super) changed: Notify,
    task: AsyncMutex<Option<JoinHandle<()>>>,
}
impl Trace {
    fn snapshot(&self) -> Value {
        let mut info = self.info.lock().expect("Trace owner").clone();
        info["commandPending"] = json!(self.busy.load(Ordering::SeqCst));
        info["stopRequested"] = json!(self.stop.load(Ordering::SeqCst));
        info
    }
    fn signal(&self) {
        self.stop.store(true, Ordering::SeqCst);
        self.changed.notify_waiters();
    }
}
struct Runner {
    host: Arc<Host>,
    owner: Arc<Trace>,
    wire: ObjectRef,
    options: Options,
    sources: Arc<crate::browser::tracing::sources::Sources>,
    chunk: Option<Uuid>,
    chunk_stop_sent: bool,
    groups: usize,
    chunks: u64,
    status_capture: Uuid,
}
impl Host {
    pub(super) fn trace_status(&self) -> Value {
        self.trace.lock().expect("Trace owner").as_ref().map_or(
            json!({"active":false,"format":"playwright-trace-zip"}),
            |trace| trace.snapshot(),
        )
    }
    pub(super) async fn trace_start(
        self: &Arc<Self>,
        context: Reference,
        options: Options,
        deadline: Instant,
    ) -> Result<Value, Error> {
        options.validate(false)?;
        if self
            .trace
            .lock()
            .expect("Trace owner")
            .as_ref()
            .is_some_and(|trace| {
                trace.snapshot()["active"] == true || trace.snapshot()["cleanupPending"] == true
            })
        {
            return Err("TRACE_ALREADY_ACTIVE".into());
        }
        let context_wire = self.wire(context, Kind::Context)?;
        let connection = self.connection()?;
        let initializer = connection
            .initializer(&context_wire)
            .await?
            .ok_or("MISSING_INITIALIZER")?;
        let wire = connection
            .resolve(
                initializer["tracing"]["guid"]
                    .as_str()
                    .ok_or("TRACE_UNAVAILABLE")?,
                "Tracing",
            )
            .await?;
        let sources = Arc::new(
            crate::browser::tracing::sources::Sources::prepare(&options, deadline)
                .await
                .map_err(Error::from)?,
        );
        self.wire(context, Kind::Context)?;
        let (tx, rx) = mpsc::channel(1);
        let status_capture = Uuid::new_v4();
        let owner = Arc::new(Trace {
            context,
            info: Mutex::new(
                json!({"captureId":status_capture,"context":context,"active":true,"state":"starting","format":"playwright-trace-zip","scope":"owned-context","options":options.summary(),"sourceTextsIncluded":sources.count()>0,"sourceFileCount":sources.count(),"sourceSelection":"explicit-snapshot-at-start","liveRecording":options.live,"liveReaderAvailable":false,"maxArchiveBytes":ARCHIVE_BYTES,"maxSourceBytes":SOURCE_BYTES,"sourceBudget":"polled-250ms","maxChunks":MAX_CHUNKS,"chunks":0,"groupDepth":0,"reports":[]}),
            ),
            commands: tx,
            busy: AtomicBool::new(true),
            stop: AtomicBool::new(false),
            changed: Notify::new(),
            task: AsyncMutex::new(None),
        });
        {
            let mut slot = self.trace.lock().expect("Trace owner");
            if slot.as_ref().is_some_and(|trace| {
                trace.snapshot()["active"] == true || trace.snapshot()["cleanupPending"] == true
            }) {
                return Err("TRACE_ALREADY_ACTIVE".into());
            }
            self.trace_evidence
                .lock()
                .unwrap()
                .reserve(status_capture, 1, 8192)?;
            *slot = Some(owner.clone());
        }
        let root = self.journal.directory().join("traces");
        let directory = if root.exists() {
            crate::artifacts::check_private_directory(&root)
        } else {
            crate::artifacts::directory(&root)
        };
        if let Err(error) = directory {
            owner.info.lock().unwrap()["active"] = json!(false);
            self.trace_evidence.lock().unwrap().release(status_capture);
            return Err(Error::caused("TRACE_IO", error));
        }
        let runner = Runner {
            host: self.clone(),
            owner: owner.clone(),
            wire,
            options,
            sources,
            chunk: None,
            chunk_stop_sent: false,
            groups: 0,
            chunks: 0,
            status_capture,
        };
        owner.info.lock().unwrap()["setupSettled"] = json!(false);
        *owner.task.lock().await = Some(tokio::spawn(runner.run(rx)));
        self.trace_wait(&owner, deadline, false).await
    }
    async fn trace_wait(
        &self,
        owner: &Arc<Trace>,
        deadline: Instant,
        terminal: bool,
    ) -> Result<Value, Error> {
        loop {
            let changed = owner.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            let status = owner.snapshot();
            if (terminal && status["active"] == false)
                || (!terminal && !owner.busy.load(Ordering::SeqCst))
            {
                if let Some(code) = status["error"].as_str() {
                    let mut error = Error::from(if code == "TRACE_SOURCE_BOUNDS" {
                        "TRACE_SOURCE_BOUNDS"
                    } else {
                        "TRACE_FAILED"
                    });
                    error.partial = Some(status);
                    return Err(error);
                }
                return Ok(status);
            }
            if tokio::time::timeout_at(deadline, changed).await.is_err() {
                let mut error = Error::from("TRACE_COMMAND_PENDING");
                error.partial = Some(status);
                return Err(error);
            }
        }
    }
    pub(super) async fn trace_command(
        &self,
        request: Request,
        deadline: Instant,
    ) -> Result<Value, Error> {
        let owner = self
            .trace
            .lock()
            .expect("Trace owner")
            .clone()
            .ok_or("TRACE_NOT_STARTED")?;
        if matches!(
            request,
            Request::TraceStop {
                file_path: Some(_),
                ..
            } | Request::TraceStop {
                analysis_page: Some(_),
                ..
            } | Request::TraceStop {
                analysis_developer: Some(_),
                ..
            }
        ) {
            return Err("TRACE_NATIVE_OPTIONS_UNSUPPORTED".into());
        }
        if matches!(request, Request::TraceStop { .. }) {
            owner.signal();
            return self.trace_wait(&owner, deadline, true).await;
        }
        if owner.snapshot()["active"] != true {
            return Err("TRACE_NOT_ACTIVE".into());
        }
        if owner.stop.load(Ordering::SeqCst) {
            return Err("TRACE_STOPPING".into());
        }
        if owner.busy.swap(true, Ordering::SeqCst) {
            return Err("TRACE_BUSY".into());
        }
        let (reply, result) = oneshot::channel();
        let id = Uuid::new_v4();
        owner.info.lock().unwrap()["pendingCommand"] =
            json!({"id":id,"operation":command_name(&request)});
        if owner
            .commands
            .try_send(Command { id, request, reply })
            .is_err()
        {
            owner.info.lock().unwrap()["pendingCommand"] = Value::Null;
            owner.busy.store(false, Ordering::SeqCst);
            return Err("TRACE_BUSY".into());
        }
        match tokio::time::timeout_at(deadline, result).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err("TRACE_OWNER_FAILED".into()),
            Err(_) => {
                let mut error = Error::from("TRACE_COMMAND_PENDING");
                error.partial = Some(owner.snapshot());
                Err(error)
            }
        }
    }
    pub(super) async fn close_trace(
        &self,
        context: Option<Reference>,
        deadline: Instant,
    ) -> Result<(), Error> {
        let owner = recovered(&self.trace).clone();
        let Some(owner) =
            owner.filter(|trace| context.is_none_or(|context| context == trace.context))
        else {
            return Ok(());
        };
        owner.signal();
        let mut task = owner.task.lock().await;
        if let Some(job) = task.as_mut() {
            tokio::time::timeout_at(deadline, job)
                .await
                .map_err(|_| Error::from("TRACE_CLEANUP_PENDING"))?
                .map_err(|_| Error::from("TRACE_OWNER_FAILED"))?;
            task.take();
        }
        if owner.snapshot()["cleanupPending"] == true {
            return Err("TRACE_CLEANUP_PENDING".into());
        }
        recovered(&self.trace_evidence).finalize_all(self.journal.directory())?;
        Ok(())
    }
    pub(super) async fn trace_after_driver_exit(&self, deadline: Instant) -> Result<(), Error> {
        let owner = recovered(&self.trace).clone();
        if let Some(owner) = owner {
            let mut task = owner.task.lock().await;
            if let Some(job) = task.as_mut() {
                tokio::time::timeout_at(deadline, job)
                    .await
                    .map_err(|_| Error::from("TRACE_CLEANUP_PENDING"))?
                    .map_err(|_| Error::from("TRACE_OWNER_FAILED"))?;
                task.take();
            }
            let root = self.journal.directory().join("traces");
            if root.exists() {
                std::fs::remove_dir_all(root).map_err(|error| Error::caused("TRACE_IO", error))?;
            }
            recovered(&owner.info)["cleanupPending"] = json!(false);
            if let Ok(capture) =
                serde_json::from_value::<Uuid>(owner.snapshot()["pendingCaptureId"].clone())
            {
                recovered(&self.trace_evidence).release(capture);
            }
        }
        recovered(&self.trace_evidence).finalize_all(self.journal.directory())?;
        Ok(())
    }
    pub(super) fn trace_evidence_operation(&self, request: Request) -> Result<Value, Error> {
        let mut store = self.trace_evidence.lock().expect("Trace evidence");
        match request {
            Request::DeveloperEvidenceList {} => Ok(
                json!({"evidence":store.list(),"pending":store.pending_summary(),"capacity":store.capacity_summary()}),
            ),
            Request::DeveloperEvidenceFinalize { capture_id } => {
                store.finalize(self.journal.directory(), capture_id)
            }
            Request::DeveloperEvidenceRemove { evidence_id } => {
                store.get(evidence_id)?.remove()?;
                store.forget(evidence_id);
                Ok(json!({"evidenceId":evidence_id,"removed":true}))
            }
            _ => Err("CAPABILITY_UNAVAILABLE".into()),
        }
    }
}
impl Runner {
    async fn call(&self, method: &str, params: Value) -> Result<Value, Error> {
        let responsive = matches!(
            method,
            "tracingStop" | "tracingStopChunk" | "tracingGroupEnd"
        );
        Ok(self
            .host
            .connection()?
            .retained(
                &self.wire,
                method,
                params,
                Instant::now() + Duration::from_secs(30),
                responsive,
            )
            .await?
            .unwrap_or(Value::Null))
    }
    fn publish(&self, state: &str) {
        let mut info = self.owner.info.lock().expect("Trace owner");
        info["state"] = json!(state);
        info["chunks"] = json!(self.chunks);
        info["groupDepth"] = json!(self.groups);
        info["chunkActive"] = json!(self.chunk.is_some());
        drop(info);
        self.owner.changed.notify_waiters();
    }
    async fn start_chunk(&mut self, title: &str, name: Option<&str>) -> Result<Value, Error> {
        crate::browser::tracing::label(title)?;
        if let Some(name) = name {
            crate::browser::tracing::prefix(name)?;
        }
        if self.chunk.is_some() {
            return Err("TRACE_CHUNK_ACTIVE".into());
        }
        if self.chunks >= MAX_CHUNKS {
            return Err("TRACE_CHUNK_CAPACITY".into());
        }
        let capture = Uuid::new_v4();
        self.host
            .trace_evidence
            .lock()
            .unwrap()
            .reserve(capture, 1, ARCHIVE_BYTES)?;
        self.chunk = Some(capture); // Retain the reservation before dispatch, including uncertain setup.
        self.publish("starting-chunk");
        let mut params = json!({"title":title});
        if let Some(name) = name {
            params["name"] = json!(name);
        }
        self.call("tracingStartChunk", params).await?;
        self.chunks += 1;
        self.publish("recording");
        Ok(self.owner.snapshot())
    }
    async fn stop_chunk(&mut self, discard: bool) -> Result<Value, Error> {
        let capture = self.chunk.ok_or("TRACE_CHUNK_NOT_ACTIVE")?;
        if self.chunk_stop_sent {
            return Err("TRACE_CLEANUP_PENDING".into());
        }
        self.chunk_stop_sent = true;
        self.publish("stopping-chunk");
        let reply = self
            .call(
                "tracingStopChunk",
                json!({"mode":if discard{"discard"}else{"archive"}}),
            )
            .await?;
        self.chunk = None;
        self.chunk_stop_sent = false;
        self.groups = 0;
        self.publish("between-chunks");
        if discard {
            self.host.trace_evidence.lock().unwrap().release(capture);
            return Ok(self.owner.snapshot());
        }
        let connection = self.host.connection()?;
        let artifact = connection
            .resolve(
                reply["artifact"]["guid"]
                    .as_str()
                    .ok_or("TRACE_ARTIFACT_MISSING")?,
                "Artifact",
            )
            .await?;
        let init = connection
            .initializer(&artifact)
            .await?
            .ok_or("MISSING_INITIALIZER")?;
        let path = PathBuf::from(
            init["absolutePath"]
                .as_str()
                .ok_or("TRACE_ARTIFACT_MISSING")?,
        );
        let root = self.host.journal.directory().join("traces");
        let sources = self.sources.clone();
        let bytes = tokio::task::spawn_blocking(move || {
            let bytes = read_archive(&root, &path)?;
            // Export never reopens caller files; selected bytes belong to this trace owner.
            let bytes = sources.append(bytes, ARCHIVE_BYTES).map_err(Error::from)?;
            if sources.count() > 0 {
                validate_archive(&bytes)?;
            }
            Ok::<_, Error>(bytes)
        })
        .await
        .map_err(|_| Error::from("TRACE_ARCHIVE_WORKER_FAILED"))?;
        // The original immutable driver artifact remains owned until copy/staging
        // finishes. A write failure below retains bytes in the common store.
        let bytes = match bytes {
            Ok(bytes) => bytes,
            Err(error) => {
                self.host.trace_evidence.lock().unwrap().release(capture);
                return Err(error);
            }
        };
        {
            self.host.trace_evidence.lock().unwrap().stage(capture,Format{prefix:"trace",extension:"zip",mime:"application/zip",name:"playwright-trace-zip",max_bytes:ARCHIVE_BYTES},bytes,json!({"context":self.owner.context,"complete":true,"scope":"owned-context","privacy":"private-application-network-and-selected-source-content","sourceTextsIncluded":self.sources.count()>0,"sourceFileCount":self.sources.count(),"sourceSelection":"explicit-snapshot-at-start","liveRecording":self.options.live,"options":self.options.summary(),"chunk":self.chunks}))?;
        }
        connection
            .retained(
                &artifact,
                "delete",
                json!({}),
                Instant::now() + Duration::from_secs(5),
                true,
            )
            .await?;
        let result = self
            .host
            .trace_evidence
            .lock()
            .unwrap()
            .finalize(self.host.journal.directory(), capture)?;
        self.owner.info.lock().unwrap()["reports"]
            .as_array_mut()
            .unwrap()
            .extend(result["reports"].as_array().unwrap().iter().cloned());
        Ok(result)
    }
    async fn command(&mut self, request: Request) -> Result<Value, Error> {
        match request {
            Request::TraceChunkStart { title, name } => {
                self.start_chunk(&title, name.as_deref()).await
            }
            Request::TraceChunkStop {} => self.stop_chunk(false).await,
            Request::TraceGroupStart { name, location } => {
                if self.chunk.is_none() {
                    return Err("TRACE_CHUNK_NOT_ACTIVE".into());
                }
                if self.groups >= 32 {
                    return Err("TRACE_GROUP_CAPACITY".into());
                }
                let mut params = json!({"name":name});
                if let Some(location) = location {
                    location.validate()?;
                    params["location"] = json!(location);
                }
                self.call("tracingGroup", params).await?;
                self.groups += 1;
                self.publish("recording");
                Ok(self.owner.snapshot())
            }
            Request::TraceGroupEnd {} => {
                if self.groups == 0 {
                    return Err("TRACE_GROUP_NOT_ACTIVE".into());
                }
                self.call("tracingGroupEnd", json!({})).await?;
                self.groups -= 1;
                self.publish("recording");
                Ok(self.owner.snapshot())
            }
            _ => Err("CAPABILITY_UNAVAILABLE".into()),
        }
    }
    async fn handle_command(&mut self, command: Command) -> Option<Error> {
        let capture = self.chunk;
        let operation = command_name(&command.request);
        let stopping_chunk =
            capture.is_some() && matches!(command.request, Request::TraceChunkStop {});
        let mut result = self.command(command.request).await;
        if result.is_err()
            && self.chunk.is_none()
            && let Some(capture) = capture
        {
            self.host.trace_evidence.lock().unwrap().release(capture);
        }
        let failure = if stopping_chunk {
            result.as_ref().err().map(|error| Error::from(error.code))
        } else {
            None
        };
        {
            let mut info = self.owner.info.lock().unwrap();
            info["pendingCommand"] = Value::Null;
            info["lastCommand"] = json!({"id":command.id,"operation":operation,"settled":true,"outcome":if result.is_ok(){"succeeded"}else{"failed"},"error":result.as_ref().err().map(|error|error.code),"captureId":result.as_ref().ok().map(|value|&value["captureId"]),"reports":result.as_ref().ok().and_then(|value|value["reports"].as_array())});
        }
        if let Ok(value) = &mut result {
            value["commandId"] = json!(command.id);
            value["commandPending"] = json!(false);
            if value.get("pendingCommand").is_some() {
                value["pendingCommand"] = Value::Null;
            }
            if value.get("state").is_some() {
                value["lastCommand"] = self.owner.snapshot()["lastCommand"].clone();
            }
        }
        self.owner.busy.store(false, Ordering::SeqCst);
        self.owner.changed.notify_waiters();
        let _ = command.reply.send(result);
        failure
    }
    async fn run(mut self, mut commands: mpsc::Receiver<Command>) {
        let until =
            Instant::now() + Duration::from_millis(self.options.max_duration_ms.unwrap_or(30000));
        let monitor = tokio::spawn({
            let owner = self.owner.clone();
            let root = self.host.journal.directory().join("traces");
            async move {
                loop {
                    if owner.stop.load(Ordering::SeqCst) {
                        break;
                    }
                    let expired = Instant::now() >= until;
                    let failure = source_size(&root).err();
                    if expired || failure.is_some() {
                        {
                            let mut info = owner.info.lock().unwrap();
                            info["stopRequested"] = json!(true);
                            info["stopReason"] = json!(if expired {
                                "duration-limit"
                            } else {
                                "source-budget"
                            });
                            if let Some(error) = failure {
                                info["error"] = json!(error.code);
                            }
                        }
                        owner.signal();
                        break;
                    }
                    tokio::select! {()=owner.changed.notified()=>{},()=tokio::time::sleep_until(until.min(Instant::now()+Duration::from_millis(250)))=>{}}
                }
            }
        });
        let startup=async{
            let snapshots=self.options.snapshots.selected();
            let mut params=json!({"name":self.options.name.clone().unwrap_or_else(||format!("testrun-{}",Uuid::new_v4())),"snapshotDom":snapshots.dom,"snapshotAria":snapshots.aria,"snapshotScreen":snapshots.screen,"screencast":self.options.screenshots});
            if self.options.live { params["live"]=json!(true); }
            self.call("tracingStart",params).await?;
            let title=self.options.title.clone().unwrap_or_else(crate::browser::tracing::default_title);
            self.start_chunk(&title,None).await?;Ok::<_,Error>(())
        }.await;
        let mut failure = startup.err();
        self.owner.info.lock().unwrap()["setupSettled"] = json!(true);
        if let Some(error) = &failure {
            self.owner.info.lock().unwrap()["error"] = json!(error.code);
        }
        self.owner.busy.store(false, Ordering::SeqCst);
        self.owner.changed.notify_waiters();
        while failure.is_none() && !self.owner.stop.load(Ordering::SeqCst) {
            if Instant::now() >= until {
                self.owner.info.lock().unwrap()["stopReason"] = json!("duration-limit");
                break;
            }
            if let Err(error) = source_size(&self.host.journal.directory().join("traces")) {
                failure = Some(error);
                break;
            }
            tokio::select! {
                command=commands.recv()=>{
                    let Some(command)=command else{break;};
                    failure=self.handle_command(command).await;
                },
                ()=self.owner.changed.notified()=>{},
                ()=tokio::time::sleep(Duration::from_millis(250))=>{}
            }
        }
        if failure.is_none()
            && let Some(code) = self.owner.snapshot()["error"].as_str()
        {
            failure = Some(Error::from(if code == "TRACE_SOURCE_BOUNDS" {
                "TRACE_SOURCE_BOUNDS"
            } else {
                "TRACE_IO"
            }));
        }
        self.owner.signal();
        commands.close();
        let _ = monitor.await;
        while let Ok(command) = commands.try_recv() {
            let _ = command.reply.send(Err("TRACE_STOPPING".into()));
            self.owner.info.lock().unwrap()["lastCommand"] = json!({"id":command.id,"operation":command_name(&command.request),"settled":true,"outcome":"not-dispatched","error":"TRACE_STOPPING"});
        }
        self.owner.busy.store(false, Ordering::SeqCst);
        self.owner.info.lock().unwrap()["pendingCommand"] = Value::Null;
        self.finish_capture(failure).await;
    }
    async fn finish_capture(&mut self, mut failure: Option<Error>) {
        self.publish("stopping");
        let capture = self.chunk;
        if self.chunk.is_some()
            && !self.chunk_stop_sent
            && let Err(error) = self.stop_chunk(failure.is_some()).await
        {
            failure.get_or_insert(error);
        }
        let stopped = self.call("tracingStop", json!({})).await;
        let mut cleanup_pending = stopped.is_err();
        if let Err(error) = stopped {
            failure.get_or_insert(error);
        }
        if !cleanup_pending && let Some(capture) = capture {
            self.host.trace_evidence.lock().unwrap().release(capture);
        }
        // Only this capture writes into the private launch traces directory.
        if !cleanup_pending
            && let Err(error) =
                std::fs::remove_dir_all(self.host.journal.directory().join("traces"))
            && error.kind() != std::io::ErrorKind::NotFound
        {
            cleanup_pending = true;
            failure.get_or_insert_with(|| Error::caused("TRACE_IO", error));
        }
        if let Some(error) = &failure {
            let metadata = json!({"context":self.owner.context,"complete":false,"error":error.code,"chunks":self.chunks,"cleanupPending":cleanup_pending,"privacy":"private-capture-status","archiveAvailable":false});
            let bytes = serde_json::to_vec(&metadata).expect("Trace status JSON");
            let mut store = self.host.trace_evidence.lock().unwrap();
            if store
                .stage(
                    self.status_capture,
                    Format {
                        prefix: "trace-status",
                        extension: "json",
                        mime: "application/json",
                        name: "playwright-trace-capture-status-json",
                        max_bytes: 8192,
                    },
                    bytes,
                    metadata,
                )
                .is_ok()
                && let Ok(result) =
                    store.finalize(self.host.journal.directory(), self.status_capture)
            {
                self.owner.info.lock().unwrap()["reports"]
                    .as_array_mut()
                    .unwrap()
                    .extend(result["reports"].as_array().unwrap().iter().cloned());
            }
        } else {
            self.host
                .trace_evidence
                .lock()
                .unwrap()
                .release(self.status_capture);
        }
        {
            let mut info = self.owner.info.lock().unwrap();
            info["active"] = json!(false);
            info["chunkActive"] = json!(cleanup_pending && self.chunk.is_some());
            if !cleanup_pending {
                info["groupDepth"] = json!(0);
            }
            info["state"] = json!(if failure.is_some() {
                "failed"
            } else {
                "stopped"
            });
            info["error"] = json!(failure.as_ref().map(|error| error.code));
            info["cleanupPending"] = json!(cleanup_pending);
            info["pendingCaptureId"] = if cleanup_pending {
                json!(capture)
            } else {
                Value::Null
            };
        }
        self.owner.changed.notify_waiters();
    }
}
fn command_name(request: &Request) -> &'static str {
    match request {
        Request::TraceChunkStart { .. } => "trace_chunk_start",
        Request::TraceChunkStop {} => "trace_chunk_stop",
        Request::TraceGroupStart { .. } => "trace_group_start",
        Request::TraceGroupEnd {} => "trace_group_end",
        _ => "trace_stop",
    }
}
pub(super) fn source_size(root: &Path) -> Result<(), Error> {
    let mut directories = vec![root.to_path_buf()];
    let mut bytes = 0u64;
    let mut count = 0;
    while let Some(directory) = directories.pop() {
        for item in
            std::fs::read_dir(directory).map_err(|error| Error::caused("TRACE_IO", error))?
        {
            let item = item.map_err(|error| Error::caused("TRACE_IO", error))?;
            let metadata = item
                .path()
                .symlink_metadata()
                .map_err(|error| Error::caused("TRACE_IO", error))?;
            count += 1;
            if count > 4096 || metadata.is_symlink() {
                return Err("TRACE_SOURCE_BOUNDS".into());
            }
            if metadata.is_dir() {
                directories.push(item.path());
            } else if metadata.is_file() {
                bytes = bytes.saturating_add(metadata.len());
            } else {
                return Err("TRACE_SOURCE_BOUNDS".into());
            }
            if bytes > SOURCE_BYTES {
                return Err("TRACE_SOURCE_BOUNDS".into());
            }
        }
    }
    Ok(())
}
pub(super) fn read_archive(root: &Path, path: &Path) -> Result<Vec<u8>, Error> {
    let root = std::fs::canonicalize(root).map_err(|error| Error::caused("TRACE_IO", error))?;
    if path
        .parent()
        .and_then(|parent| std::fs::canonicalize(parent).ok())
        .as_ref()
        != Some(&root)
    {
        return Err("TRACE_ARTIFACT_SCOPE".into());
    }
    let metadata = path
        .symlink_metadata()
        .map_err(|error| Error::caused("TRACE_IO", error))?;
    if !metadata.is_file() || metadata.len() > ARCHIVE_BYTES as u64 {
        let mut error = Error::from("TRACE_ARCHIVE_BOUNDS");
        error.partial = Some(
            json!({"archiveBytes":metadata.len(),"regularFile":metadata.is_file(),"maxBytes":ARCHIVE_BYTES}),
        );
        return Err(error);
    }
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let file = options
        .open(path)
        .map_err(|error| Error::caused("TRACE_IO", error))?;
    let mut bytes = Vec::new();
    file.take(ARCHIVE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| Error::caused("TRACE_IO", error))?;
    if bytes.len() > ARCHIVE_BYTES {
        return Err("TRACE_ARCHIVE_BOUNDS".into());
    }
    validate_archive(&bytes)?;
    Ok(bytes)
}
fn validate_archive(bytes: &[u8]) -> Result<(), Error> {
    {
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))
            .map_err(|_| Error::from("TRACE_ARCHIVE_PROTOCOL"))?;
        if archive.len() > 4096 {
            return Err("TRACE_ARCHIVE_BOUNDS".into());
        }
        // Streamed ZIP data descriptors can make aggregate size unavailable.
        // Count actual decoded bytes, including CRC validation, without keeping
        // application payloads or trusting declared compression ratios.
        let mut expanded = 0u64;
        for index in 0..archive.len() {
            let file = archive
                .by_index(index)
                .map_err(|_| Error::from("TRACE_ARCHIVE_PROTOCOL"))?;
            let decoded = std::io::copy(
                &mut file.take(SOURCE_BYTES - expanded + 1),
                &mut std::io::sink(),
            )
            .map_err(|_| Error::from("TRACE_ARCHIVE_PROTOCOL"))?;
            expanded += decoded;
            if expanded > SOURCE_BYTES {
                return Err("TRACE_ARCHIVE_BOUNDS".into());
            }
        }
        if archive.by_name("trace.trace").is_err() || archive.by_name("trace.network").is_err() {
            return Err("TRACE_ARCHIVE_PROTOCOL".into());
        }
    }
    Ok(())
}
