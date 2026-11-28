//! Native Chrome source ownership feeding the shared viewer transport and H.264 recorder.
use super::*;
use crate::browser::{
    preview::{Options, Source},
    recording::Recording,
};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use tokio::{sync::watch, task::JoinHandle};

pub(super) struct NativeSource {
    pub source: Source<String>,
    pub scope: page::Scope,
    pub decode_failures: u8,
}
#[derive(Default)]
pub(super) struct MediaState {
    pub sources: Arc<Mutex<BTreeMap<Reference, NativeSource>>>,
    pub recordings: BTreeMap<Reference, Arc<Recording>>,
    pub transport: Option<crate::media::Media>,
    pub observations: Arc<Mutex<crate::browser::input::State>>,
    worker: Option<JoinHandle<()>>,
}
impl MediaState {
    pub fn snapshot(&self) -> Value {
        json!(
            self.sources
                .lock()
                .expect("Native sources")
                .values()
                .map(|entry| entry.source.snapshot())
                .collect::<Vec<_>>()
        )
    }
}
impl Worker {
    pub(super) fn fence_native_media(&mut self, event: &Event) {
        if !matches!(
            event.method.as_str(),
            "Page.frameNavigated" | "Page.documentOpened" | "Runtime.executionContextsCleared"
        ) {
            return;
        }
        if let Some(session) = &event.session_id {
            for entry in self
                .media
                .sources
                .lock()
                .expect("Native sources")
                .values_mut()
            {
                if entry.source.wire == *session
                    && matches!(entry.source.state, "starting" | "live")
                {
                    entry.source.state = "reconfiguring";
                    entry.source.frames.send_replace(None);
                    entry.source.interruptions.fetch_add(1, Ordering::SeqCst);
                    self.media
                        .observations
                        .lock()
                        .expect("Media observations")
                        .invalidate(entry.source.page);
                }
            }
        }
    }
    pub(super) fn media_active(&self) -> bool {
        self.media
            .sources
            .lock()
            .expect("Native sources")
            .values()
            .any(|entry| entry.source.state != "stopped")
            || self
                .media
                .recordings
                .values()
                .any(|recording| !recording.terminal())
    }
    #[expect(
        clippy::too_many_lines,
        reason = "Source allocation retains its lane registration, decoder owner and teardown across screencast admission"
    )]
    pub(super) async fn preview_start(
        &mut self,
        id: Uuid,
        page: Reference,
        options: Options,
        preview: bool,
        deadline: Instant,
    ) -> Result<Value, Error> {
        options.validate()?;
        let existing = {
            self.media
                .sources
                .lock()
                .expect("Native sources")
                .iter()
                .find(|(_, entry)| entry.source.page == page && entry.source.state != "stopped")
                .map(|(reference, entry)| (*reference, entry.source.options.clone()))
        };
        if let Some((reference, current)) = existing {
            if current != options {
                return Err("MEDIA_SOURCE_CONFLICT".into());
            }
            if preview {
                self.attach_native_viewer(reference).await?;
            }
            return self.wait_native_frame(reference, deadline).await;
        }
        let scope = self.page_scope(id, page, None, deadline).await?;
        let geometry = self
            .page_query(
                id,
                &scope,
                json!({"selector":"css=body","read":"geometry","maxBytes":8192}),
                deadline,
            )
            .await?;
        let width = geometry["viewport"]["width"]
            .as_u64()
            .ok_or("GEOMETRY_UNAVAILABLE")?;
        let height = geometry["viewport"]["height"]
            .as_u64()
            .ok_or("GEOMETRY_UNAVAILABLE")?;
        crate::browser::preview::source_size(width, height)?;
        let reference = self.transfer_reference();
        let epoch = Uuid::new_v4();
        let connection = self.connection.clone().ok_or("BACKEND_LOST")?;
        let lane = connection.frame_lane();
        {
            let mut entries = self.media.sources.lock().expect("Native sources");
            if entries
                .values()
                .filter(|entry| entry.source.state != "stopped")
                .count()
                >= 8
            {
                return Err("MEDIA_SOURCE_CAPACITY".into());
            }
            if entries.len() >= 32 {
                let retired = entries
                    .iter()
                    .find(|(_, entry)| entry.source.state == "stopped")
                    .map(|(reference, _)| *reference)
                    .ok_or("MEDIA_SOURCE_CAPACITY")?;
                entries.remove(&retired);
            }
            lane.attach(scope.session.clone(), epoch)?;
            let (frames, _) = watch::channel(None);
            entries.insert(
                reference,
                NativeSource {
                    scope: scope.clone(),
                    decode_failures: 0,
                    source: Source {
                        reference,
                        page,
                        wire: scope.session.clone(),
                        epoch,
                        options: options.clone(),
                        bounds: contract::Viewport {
                            width: width as u32,
                            height: height as u32,
                        },
                        frames,
                        alive: Arc::new(AtomicBool::new(true)),
                        interruptions: Arc::new(AtomicU64::new(0)),
                        state: "starting",
                        sequence: 0,
                        document_floor: 0,
                        frame: Some(scope.frame),
                        geometry: Some(geometry),
                        viewport_epoch: Uuid::new_v4(),
                        failure: None,
                        viewer: None,
                        preview_active: false,
                        recordings: BTreeSet::new(),
                        discarded: 0,
                        source_sequence: 0,
                        source_dropped: 0,
                    },
                },
            );
        }
        if self.media.worker.is_none() {
            let frames = lane.claim()?;
            let sources = self.media.sources.clone();
            let connection = connection.clone();
            let observations = self.media.observations.clone();
            self.media.worker = Some(tokio::spawn(async move {
                super::media_frames::run(connection, frames, sources, observations).await;
            }));
        }
        let result=self.call(id,Some(&scope.session),"Page.startScreencast",json!({"format":"jpeg","quality":options.quality,"maxWidth":width,"maxHeight":height,"everyNthFrame":1}),deadline).await;
        if let Err(error) = result {
            let _ = self
                .stop_native_source(reference, Instant::now() + Duration::from_secs(3))
                .await;
            return Err(error);
        }
        if preview {
            self.attach_native_viewer(reference).await?;
        }
        self.wait_native_frame(reference, deadline).await
    }
    async fn attach_native_viewer(&mut self, reference: Reference) -> Result<(), Error> {
        let (frames, alive, already) = {
            let entries = self.media.sources.lock().expect("Native sources");
            let source = &entries.get(&reference).ok_or("STALE_PREVIEW")?.source;
            (
                source.frames.subscribe(),
                source.alive.clone(),
                source.preview_active,
            )
        };
        if already {
            return Ok(());
        }
        if self.media.transport.is_none() {
            self.media.transport = Some(
                crate::media::Media::start()
                    .await
                    .map_err(|error| Error::caused("MEDIA_UNAVAILABLE", error))?,
            );
        }
        let viewer = self
            .media
            .transport
            .as_ref()
            .expect("Media transport")
            .browser_preview(&reference.id.to_string(), frames, alive)
            .map_err(|error| Error::caused("MEDIA_UNAVAILABLE", error))?;
        let mut entries = self.media.sources.lock().expect("Native sources");
        let source = &mut entries.get_mut(&reference).ok_or("STALE_PREVIEW")?.source;
        source.preview_active = true;
        source.viewer = Some(viewer);
        Ok(())
    }
    async fn wait_native_frame(
        &mut self,
        reference: Reference,
        deadline: Instant,
    ) -> Result<Value, Error> {
        loop {
            let snapshot = self.preview_status(reference)?;
            if snapshot["state"] == "live" {
                return Ok(snapshot);
            }
            if matches!(snapshot["state"].as_str(), Some("failed" | "stopped")) {
                return Err("MEDIA_SOURCE_LOST".into());
            }
            if Instant::now() >= deadline {
                return Err("FIRST_FRAME_TIMEOUT".into());
            }
            self.observation_tick(deadline).await?;
        }
    }
    pub(super) fn preview_status(&self, reference: Reference) -> Result<Value, Error> {
        self.transfer_scope(reference)?;
        let entries = self.media.sources.lock().expect("Native sources");
        Ok(entries
            .get(&reference)
            .ok_or("STALE_PREVIEW")?
            .source
            .snapshot())
    }
    pub(super) async fn preview_stop(
        &mut self,
        reference: Reference,
        deadline: Instant,
    ) -> Result<Value, Error> {
        self.transfer_scope(reference)?;
        let (release, was_active) = {
            let mut entries = self.media.sources.lock().expect("Native sources");
            let source = &mut entries.get_mut(&reference).ok_or("STALE_PREVIEW")?.source;
            let was_active = source.preview_active;
            source.preview_active = false;
            source.viewer = None;
            (source.recordings.is_empty(), was_active)
        };
        if let Some(transport) = &self.media.transport
            && was_active
        {
            transport
                .stop(&reference.id.to_string())
                .map_err(|error| Error::caused("MEDIA_CLEANUP_PENDING", error))?;
        }
        if release {
            self.stop_native_source(reference, deadline).await?;
        }
        self.preview_status(reference)
    }
    pub(super) async fn stop_native_source(
        &mut self,
        reference: Reference,
        deadline: Instant,
    ) -> Result<(), Error> {
        let (session, page, viewer_active) = {
            let mut entries = self.media.sources.lock().expect("Native sources");
            let entry = entries.get_mut(&reference).ok_or("STALE_PREVIEW")?;
            if entry.source.state == "stopped" {
                return Ok(());
            }
            entry.source.state = "stopping";
            entry.source.frames.send_replace(None);
            (
                entry.source.wire.clone(),
                entry.source.page,
                entry.source.preview_active,
            )
        };
        if self.index.route(page, Some(Kind::Page)).is_ok()
            && self
                .connection
                .as_ref()
                .is_some_and(|connection| connection.status().open)
        {
            self.cleanup_call(Some(&session), "Page.stopScreencast", json!({}), deadline)
                .await?;
        }
        if let Some(connection) = &self.connection {
            connection.frame_lane().detach(&session);
        }
        if viewer_active && let Some(transport) = &self.media.transport {
            transport
                .stop(&reference.id.to_string())
                .map_err(|error| Error::caused("MEDIA_CLEANUP_PENDING", error))?;
        }
        let mut entries = self.media.sources.lock().expect("Native sources");
        let source = &mut entries.get_mut(&reference).ok_or("STALE_PREVIEW")?.source;
        source.state = "stopped";
        source.preview_active = false;
        source.viewer = None;
        source.alive.store(false, Ordering::Release);
        Ok(())
    }
    pub(super) async fn poll_media(&mut self, id: Uuid, deadline: Instant) -> Result<(), Error> {
        let failed = self
            .media
            .sources
            .lock()
            .expect("Native sources")
            .iter()
            .filter(|(_, entry)| entry.source.state == "failed")
            .map(|(reference, _)| *reference)
            .collect::<Vec<_>>();
        for reference in failed {
            self.stop_native_source(reference, deadline).await?;
        }
        let terminal = self
            .media
            .recordings
            .values()
            .filter(|job| job.terminal())
            .map(|job| (job.source, job.reference.id))
            .collect::<Vec<_>>();
        for (reference, id) in terminal {
            let release = {
                let mut entries = self.media.sources.lock().expect("Native sources");
                entries.get_mut(&reference).is_some_and(|entry| {
                    entry.source.recordings.remove(&id);
                    !entry.source.preview_active
                        && entry.source.recordings.is_empty()
                        && entry.source.state != "stopped"
                })
            };
            if release {
                self.stop_native_source(reference, deadline).await?;
            }
        }
        let restart = self
            .media
            .sources
            .lock()
            .expect("Native sources")
            .iter()
            .filter(|(_, entry)| entry.source.state == "reconfiguring")
            .map(|(reference, entry)| (*reference, entry.source.page, entry.source.options.clone()))
            .collect::<Vec<_>>();
        for (reference, page, options) in restart {
            self.restart_native_source(id, reference, page, options, deadline)
                .await?;
        }
        Ok(())
    }
    async fn restart_native_source(
        &mut self,
        id: Uuid,
        reference: Reference,
        page: Reference,
        options: Options,
        deadline: Instant,
    ) -> Result<(), Error> {
        let scope = match self.page_scope(id, page, None, deadline).await {
            Ok(scope) => scope,
            Err(_) => {
                self.stop_native_source(reference, deadline).await?;
                return Ok(());
            }
        };
        let connection = self.connection.clone().ok_or("BACKEND_LOST")?;
        let geometry = self
            .page_query(
                id,
                &scope,
                json!({"selector":"css=body","read":"geometry","maxBytes":8192}),
                deadline,
            )
            .await?;
        let width = geometry["viewport"]["width"]
            .as_u64()
            .ok_or("GEOMETRY_UNAVAILABLE")?;
        let height = geometry["viewport"]["height"]
            .as_u64()
            .ok_or("GEOMETRY_UNAVAILABLE")?;
        crate::browser::preview::source_size(width, height)?;
        self.cleanup_call(
            Some(&scope.session),
            "Page.stopScreencast",
            json!({}),
            deadline,
        )
        .await?;
        connection.frame_lane().detach(&scope.session);
        let epoch = Uuid::new_v4();
        connection
            .frame_lane()
            .attach(scope.session.clone(), epoch)?;
        {
            let mut entries = self.media.sources.lock().expect("Native sources");
            let entry = entries.get_mut(&reference).ok_or("STALE_PREVIEW")?;
            entry.scope = scope.clone();
            entry.source.epoch = epoch;
            entry.source.frame = Some(scope.frame);
            entry.source.state = "starting";
            entry.source.source_sequence = 0;
            entry.source.bounds = contract::Viewport {
                width: width as u32,
                height: height as u32,
            };
        }
        self.call(
                id,
                Some(&scope.session),
                "Page.startScreencast",
                json!({"format":"jpeg","quality":options.quality,"maxWidth":width,"maxHeight":height,"everyNthFrame":1}),
                deadline,
            )
            .await?;
        Ok(())
    }
    pub(super) async fn close_media(&mut self, deadline: Instant) -> Result<(), Error> {
        for recording in self.media.recordings.values() {
            recording.stop();
        }
        let recordings = self.media.recordings.keys().copied().collect::<Vec<_>>();
        for reference in recordings {
            self.native_recording_result(reference, true, deadline)
                .await?;
            if !self.media.recordings[&reference].terminal() {
                return Err("MEDIA_CLEANUP_PENDING".into());
            }
        }
        let sources = self
            .media
            .sources
            .lock()
            .expect("Native sources")
            .keys()
            .copied()
            .collect::<Vec<_>>();
        for reference in sources {
            self.stop_native_source(reference, deadline).await?;
        }
        if let Some(transport) = self.media.transport.as_mut() {
            transport
                .close()
                .map_err(|error| Error::caused("MEDIA_CLEANUP_PENDING", error))?;
        }
        self.media.transport = None;
        if let Some(connection) = &self.connection {
            connection.frame_lane().close();
        }
        if let Some(worker) = self.media.worker.as_mut() {
            tokio::time::timeout_at(deadline, worker)
                .await
                .map_err(|_| Error::from("MEDIA_CLEANUP_PENDING"))?
                .map_err(|_| Error::from("MEDIA_CLEANUP_PENDING"))?;
            self.media.worker = None;
        }
        Ok(())
    }
    pub(super) fn fail_native_media(&self) {
        for entry in self
            .media
            .sources
            .lock()
            .expect("Native sources")
            .values_mut()
        {
            if entry.source.state != "stopped" {
                entry.source.failure = Some("MEDIA_RECOVERY_FAILED");
                entry.source.state = "failed";
                entry.source.alive.store(false, Ordering::Release);
                entry.source.frames.send_replace(None);
            }
        }
    }
}
