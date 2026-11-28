//! Supervised browser JPEG sources, geometry barriers and private viewer feeds.

use super::*;
use crate::browser::{
    frames::{Frames, RawFrame},
    preview::{Options, Source},
};
use base64::Engine as _;
use std::sync::atomic::Ordering as MemoryOrder;
use tokio::sync::watch;

impl Host {
    pub(super) async fn start_frames(self: &Arc<Self>, mut frames: Frames) {
        let weak = Arc::downgrade(self);
        let task = tokio::spawn(async move {
            while let Some(raw) = frames.next().await {
                let Some(host) = weak.upgrade() else {
                    return;
                };
                let page = raw.page.clone();
                let epoch = raw.epoch;
                if let Err(error) = host.preview_frame(raw).await {
                    // Media errors are scoped to sources; do not replay input
                    // or turn a failed decoder into browser reconnection.
                    let mut sources = host.previews.lock().expect("Browser previews");
                    for source in sources.entries.values_mut().filter(|source| {
                        source.wire == page && source.epoch == epoch && source.state != "stopped"
                    }) {
                        source.failure = Some(error.code);
                        source.state = "failed";
                        source.frames.send_replace(None);
                        source.alive.store(false, MemoryOrder::Release);
                    }
                }
                host.changed.notify_waiters();
            }
            if let Some(host) = weak.upgrade() {
                let mut sources = host.previews.lock().expect("Browser previews");
                for source in sources
                    .entries
                    .values_mut()
                    .filter(|source| source.state != "stopped")
                {
                    source.state = "failed";
                    source.failure = Some("MEDIA_SOURCE_LOST");
                    source.alive.store(false, MemoryOrder::Release);
                    source.frames.send_replace(None);
                }
                host.changed.notify_waiters();
            }
        });
        *self.frames_task.lock().await = Some(task);
    }

    pub(super) async fn preview_start(
        &self,
        page: Reference,
        options: Options,
        deadline: Instant,
    ) -> Result<Value, Error> {
        self.source_start(page, options, true, deadline).await
    }

    #[expect(
        clippy::too_many_lines,
        reason = "Source reservation, retained native start and first-frame publication form one transaction"
    )]
    pub(super) async fn source_start(
        &self,
        page: Reference,
        options: Options,
        present: bool,
        deadline: Instant,
    ) -> Result<Value, Error> {
        let lane = tokio::time::timeout_at(deadline, self.preview_lane.lock())
            .await
            .map_err(|_| Error::from("MEDIA_BUSY"))?;
        options.validate()?;
        let existing = {
            let sources = self.previews.lock().expect("Browser previews");
            sources
                .entries
                .values()
                .find(|source| source.page == page && source.state != "stopped")
                .map(|source| {
                    (
                        source.reference,
                        source.options.clone(),
                        source.preview_active,
                        source.state,
                    )
                })
        };
        if let Some((reference, current, preview_active, state)) = existing {
            if current != options {
                return Err("MEDIA_OPTIONS_CONFLICT".into());
            }
            if !matches!(state, "live" | "starting") {
                return Err("MEDIA_SOURCE_UNAVAILABLE".into());
            }
            if present && preview_active {
                return Err("PREVIEW_ALREADY_ACTIVE".into());
            }
            if present {
                self.attach_viewer(reference).await?;
            }
            return self.preview_status(reference);
        }
        let scope = self.page_scope(page, None, deadline).await?;
        let wire = self.wire(page, Kind::Page)?;
        self.evaluate(&scope.wire, super::page::PAINT, Value::Null, deadline)
            .await?;
        let geometry = self
            .evaluate(&scope.wire, super::page::GEOMETRY, Value::Null, deadline)
            .await?;
        let width = geometry["viewport"]["width"]
            .as_u64()
            .ok_or("GEOMETRY_UNAVAILABLE")?;
        let height = geometry["viewport"]["height"]
            .as_u64()
            .ok_or("GEOMETRY_UNAVAILABLE")?;
        if width * height > 8_000_000 || width == 0 || height == 0 {
            return Err("MEDIA_GEOMETRY_BOUNDS".into());
        }
        let size = crate::browser::preview::source_size(width, height)?;
        let connection = self.connection()?;
        let epoch = Uuid::new_v4();
        let reference = Reference {
            connection_id: page.connection_id,
            id: Uuid::new_v4(),
            generation: Uuid::new_v4(),
        };
        {
            let mut sources = self.previews.lock().expect("Browser previews");
            if sources
                .entries
                .values()
                .any(|source| source.page == page && source.state != "stopped")
            {
                return Err("PREVIEW_ALREADY_ACTIVE".into());
            }
            if sources.entries.len() >= 32 {
                let removable = sources
                    .entries
                    .iter()
                    .find(|(_, source)| source.state == "stopped")
                    .map(|(id, _)| *id)
                    .ok_or("PREVIEW_CAPACITY")?;
                sources.entries.remove(&removable);
            }
            connection.frame_lane().attach(wire.clone(), epoch)?;
            let (frames, _) = watch::channel(None);
            sources.entries.insert(
                reference.id,
                Source {
                    reference,
                    page,
                    wire: wire.clone(),
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
                    frame: Some(scope.reference),
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
            );
        }
        let result=connection.retained(&wire,"screencastStart",json!({"size":size,"quality":options.quality,"fps":options.fps,"sendFrames":true,"record":false}),deadline,false).await;
        if let Err(error) = result {
            let mut sources = self.previews.lock().expect("Browser previews");
            let source = sources.get_mut(reference)?;
            source.state = "cleanup_pending";
            source.failure = Some("MEDIA_START_FAILED");
            return Err(error.into());
        }
        drop(lane);
        let mut receiver = self
            .previews
            .lock()
            .expect("Browser previews")
            .get(reference)?
            .frames
            .subscribe();
        loop {
            let notified = self.changed.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if receiver.borrow_and_update().is_some() {
                break;
            }
            if self
                .previews
                .lock()
                .expect("Browser previews")
                .get(reference)?
                .failure
                .is_some()
            {
                return Err("MEDIA_SOURCE_FAILED".into());
            }
            if tokio::time::timeout_at(deadline, notified).await.is_err() {
                return Err(Error {
                    partial: Some(json!({"preview":reference,"state":"starting","retained":true})),
                    ..Error::from("FIRST_FRAME_TIMEOUT")
                });
            }
        }
        if present {
            self.attach_viewer(reference).await?;
        }
        self.preview_status(reference)
    }

    async fn attach_viewer(&self, reference: Reference) -> Result<(), Error> {
        let receiver = self
            .previews
            .lock()
            .expect("Browser previews")
            .get(reference)?
            .frames
            .subscribe();
        {
            let mut media = self.media.lock().await;
            if media.is_none() {
                *media = Some(
                    crate::media::Media::start()
                        .await
                        .map_err(|cause| Error::caused("MEDIA_UNAVAILABLE", cause))?,
                );
            }
            let alive = self
                .previews
                .lock()
                .expect("Browser previews")
                .get(reference)?
                .alive
                .clone();
            let viewer = media
                .as_ref()
                .expect("Media service")
                .browser_preview(&reference.id.to_string(), receiver, alive)
                .map_err(|cause| Error::caused("MEDIA_UNAVAILABLE", cause))?;
            self.previews
                .lock()
                .expect("Browser previews")
                .get_mut(reference)?
                .viewer = Some(viewer);
            self.previews
                .lock()
                .expect("Browser previews")
                .get_mut(reference)?
                .preview_active = true;
        }
        Ok(())
    }

    pub(super) fn preview_status(&self, reference: Reference) -> Result<Value, Error> {
        let mut result = self
            .previews
            .lock()
            .expect("Browser previews")
            .get(reference)?
            .snapshot();
        if let Ok(connection) = self.connection() {
            result["laneDroppedFrames"] = json!(connection.frame_lane().dropped());
            result["laneFailure"] = json!(connection.frame_lane().failure());
        }
        Ok(result)
    }

    pub(super) async fn preview_stop(
        &self,
        reference: Reference,
        deadline: Instant,
    ) -> Result<Value, Error> {
        self.source_stop(reference, false, deadline).await
    }

    pub(super) async fn source_stop(
        &self,
        reference: Reference,
        force: bool,
        deadline: Instant,
    ) -> Result<Value, Error> {
        let _lane = tokio::time::timeout_at(deadline, self.preview_lane.lock())
            .await
            .map_err(|_| Error::from("MEDIA_CLEANUP_PENDING"))?;
        self.source_stop_locked(reference, force, deadline).await
    }

    pub(super) async fn release_source(
        &self,
        reference: Reference,
        deadline: Instant,
    ) -> Result<(), Error> {
        let _lane = tokio::time::timeout_at(deadline, self.preview_lane.lock())
            .await
            .map_err(|_| Error::from("MEDIA_CLEANUP_PENDING"))?;
        let unused = {
            let sources = self.previews.lock().expect("Browser previews");
            let source = sources.get(reference)?;
            !source.preview_active && source.recordings.is_empty() && source.state != "stopped"
        };
        if unused {
            self.source_stop_locked(reference, true, deadline).await?;
        }
        Ok(())
    }

    async fn source_stop_locked(
        &self,
        reference: Reference,
        force: bool,
        deadline: Instant,
    ) -> Result<Value, Error> {
        let wire = {
            let mut sources = self.previews.lock().expect("Browser previews");
            let source = sources.get_mut(reference)?;
            source.preview_active = false;
            source.viewer = None;
            if source.state == "stopped" {
                return Ok(source.snapshot());
            }
            if !force && !source.recordings.is_empty() {
                None
            } else {
                source.state = "stopping";
                source.alive.store(false, MemoryOrder::Release);
                source.frames.send_replace(None);
                Some(source.wire.clone())
            }
        };
        if let Some(media) = self.media.lock().await.as_ref() {
            media
                .stop(&reference.id.to_string())
                .map_err(|cause| Error::caused("MEDIA_CLEANUP_PENDING", cause))?;
        }
        let Some(wire) = wire else {
            return self.preview_status(reference);
        };
        let connection = self.connection()?;
        if connection.status().loss.is_none()
            && let Err(error) = connection
                .retained(&wire, "screencastStop", json!({}), deadline, true)
                .await
        {
            self.previews
                .lock()
                .expect("Browser previews")
                .get_mut(reference)?
                .state = "cleanup_pending";
            return Err(error.into());
        }
        connection.frame_lane().detach(&wire);
        let mut sources = self.previews.lock().expect("Browser previews");
        let source = sources.get_mut(reference)?;
        source.state = "stopped";
        source.viewer = None;
        Ok(source.snapshot())
    }

    pub(super) async fn close_previews(&self, deadline: Instant) -> Result<(), Error> {
        let references = recovered(&self.previews)
            .entries
            .values()
            .filter(|source| source.state != "stopped")
            .map(|source| source.reference)
            .collect::<Vec<_>>();
        let mut failure = None;
        for reference in references {
            if let Err(error) = self.source_stop(reference, true, deadline).await {
                failure = Some(error);
            }
        }
        let mut media = self.media.lock().await;
        if let Some(server) = media.as_mut()
            && let Err(cause) = server.close()
        {
            return Err(Error::caused("MEDIA_CLEANUP_PENDING", cause));
        }
        *media = None;
        if let Some(error) = failure {
            return Err(error);
        }
        Ok(())
    }

    pub(super) async fn end_page_previews(
        &self,
        page: Reference,
        crashed: bool,
    ) -> Result<(), Error> {
        self.inputs.lock().expect("Browser input").end(page);
        let ended = {
            let mut sources = self.previews.lock().expect("Browser previews");
            sources
                .entries
                .values_mut()
                .filter(|source| source.page == page && source.state != "stopped")
                .map(|source| {
                    source.state = if crashed {
                        "cleanup_pending"
                    } else {
                        "stopped"
                    };
                    source.failure = Some(if crashed {
                        "PAGE_CRASHED"
                    } else {
                        "PAGE_CLOSED"
                    });
                    source.alive.store(false, MemoryOrder::Release);
                    source.frames.send_replace(None);
                    source.viewer = None;
                    (source.reference, source.wire.clone())
                })
                .collect::<Vec<_>>()
        };
        let media = self.media.lock().await;
        for (reference, wire) in ended {
            if !crashed {
                self.connection()?.frame_lane().detach(&wire);
            }
            if let Some(media) = media.as_ref() {
                media
                    .stop(&reference.id.to_string())
                    .map_err(|cause| Error::caused("MEDIA_CLEANUP_PENDING", cause))?;
            }
        }
        self.changed.notify_waiters();
        Ok(())
    }

    #[expect(
        clippy::too_many_lines,
        reason = "One frame publication validates its source epoch, native geometry and decoded bytes before publishing metadata and pixels together"
    )]
    async fn preview_frame(&self, raw: RawFrame) -> Result<(), Error> {
        let reference = {
            let sources = self.previews.lock().expect("Browser previews");
            let Some(source) = sources.entries.values().find(|source| {
                source.wire == raw.page
                    && source.epoch == raw.epoch
                    && matches!(source.state, "starting" | "live")
            }) else {
                return Ok(());
            };
            source.reference
        };
        let deadline = Instant::now() + Duration::from_secs(5);
        let page = self
            .previews
            .lock()
            .expect("Browser previews")
            .get(reference)?
            .page;
        let restart = self
            .previews
            .lock()
            .expect("Browser previews")
            .get(reference)?
            .frame
            .is_none();
        if restart {
            self.restart_previews(page, deadline).await?;
            return Ok(());
        }
        let scope = self.page_scope(page, None, deadline).await?;
        let geometry = self
            .evaluate(&scope.wire, super::page::GEOMETRY, Value::Null, deadline)
            .await?;
        if self.frame_current(&scope.wire, deadline).await? != scope.reference {
            return Ok(());
        }
        let bounds = self
            .previews
            .lock()
            .expect("Browser previews")
            .get(reference)?
            .bounds;
        if geometry["viewport"] != json!(bounds) {
            self.restart_previews(page, deadline).await?;
            return Ok(());
        }
        let encoded = raw.params["data"].as_str().ok_or("INVALID_MEDIA_FRAME")?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|_| Error::from("INVALID_MEDIA_FRAME"))?;
        let decoded = tokio::task::spawn_blocking(move || {
            let mut reader = image::ImageReader::with_format(
                std::io::Cursor::new(&bytes),
                image::ImageFormat::Jpeg,
            );
            let mut limits = image::Limits::default();
            limits.max_image_width = Some(8192);
            limits.max_image_height = Some(8192);
            limits.max_alloc = Some(64 * 1024 * 1024);
            reader.limits(limits);
            let image = reader
                .decode()
                .map_err(|cause| Error::caused("INVALID_MEDIA_FRAME", cause))?;
            if u64::from(image.width()) * u64::from(image.height()) > 8_000_000 {
                return Err(Error::from("MEDIA_GEOMETRY_BOUNDS"));
            }
            Ok((bytes, image.width(), image.height()))
        })
        .await
        .map_err(|_| Error::from("MEDIA_DECODE_FAILED"))??;
        let identity = self.state.lock().expect("Browser owner").identity.clone();
        let engine = self.state.lock().expect("Browser owner").engine;
        let mut sources = self.previews.lock().expect("Browser previews");
        let source = sources.get_mut(reference)?;
        if self.connection()?.frame_lane().failure().is_some() {
            return Err("MEDIA_SOURCE_LOST".into());
        }
        if source.epoch != raw.epoch {
            source.discarded += 1;
            return Ok(());
        }
        source.source_dropped += raw
            .source_sequence
            .saturating_sub(source.source_sequence + 1);
        source.source_sequence = raw.source_sequence;
        if !matches!(source.state, "starting" | "live") || raw.ordinal < source.document_floor {
            source.discarded += 1;
            return Ok(());
        }
        if decoded.1.abs_diff(source.bounds.width) > 1
            || decoded.2.abs_diff(source.bounds.height) > 1
        {
            source.discarded += 1;
            return Ok(());
        }
        if source.geometry.as_ref() != Some(&geometry) || source.frame != Some(scope.reference) {
            source.viewport_epoch = Uuid::new_v4();
        }
        source.geometry = Some(geometry.clone());
        source.frame = Some(scope.reference);
        source.sequence += 1;
        // The pinned driver publishes CSS pixels. Firefox rounds odd canvases
        // up to even JPEG dimensions; only this bounded right/bottom padding is mapped.
        let metadata = json!({"streamId":reference.id,"platform":"browser","page":page,"frame":scope.reference,"sourceEpoch":source.epoch,"viewportEpoch":source.viewport_epoch,"seq":source.sequence.to_string(),"codec":"jpeg","mimeType":"image/jpeg","width":decoded.1,"height":decoded.2,"logicalViewport":geometry["viewport"],"dpr":geometry["dpr"],"driverViewport":{"width":raw.params["viewportWidth"],"height":raw.params["viewportHeight"]},"sourceTimestamp":raw.params["timestamp"],"clockDomain":"driver-reported-unverified","arrivalUnixMs":raw.arrival_unix_ms,"publicationUnixMs":super::page::now_ms(),"arrivalToPublicationMs":raw.arrived.elapsed().as_millis(),"healthScope":"owner-connection-only","contentGeometryVerified":false,"transform":null,"coordinateInputAvailable":false,"privacy":"local-private"});
        let mut metadata = metadata;
        let exact = decoded.1 >= source.bounds.width
            && decoded.2 >= source.bounds.height
            && (engine == contract::Engine::Firefox
                || (decoded.1 == source.bounds.width && decoded.2 == source.bounds.height))
            && geometry["visualViewport"]["scale"] == 1
            && geometry["pointerLocked"] != true;
        if exact {
            let observation = self.inputs.lock().expect("Browser input").observe(
                page,
                scope.reference,
                geometry.clone(),
                raw.arrived,
                self.input_epoch.load(Ordering::SeqCst),
            );
            metadata["transform"] = json!({"cssToPixels":1,"documentOrigin":geometry["scroll"],"padding":{"right":decoded.1-source.bounds.width,"bottom":decoded.2-source.bounds.height}});
            metadata["contentGeometryVerified"] = json!(true);
            metadata["coordinateInputAvailable"] = json!(true);
            metadata["inputObservation"] = json!(observation);
            metadata["inputCoordinateSpace"] = json!("css-viewport");
            metadata["inputObservationTtlMs"] = json!(10000);
        }
        metadata["sourceDroppedFrames"] = json!(source.source_dropped + source.discarded);
        metadata["visibilityLimit"] = if engine == contract::Engine::Firefox
            && identity
                .as_ref()
                .is_some_and(|identity| identity["mode"] == "headed")
        {
            json!({"kind":"native-window-corner-mask","corners":["bottom_left","bottom_right"],"extentVerified":false})
        } else {
            Value::Null
        };
        let packet = crate::media::encode(&metadata, &decoded.0)
            .map_err(|cause| Error::caused("INVALID_MEDIA_FRAME", cause))?;
        source
            .frames
            .send_replace(Some(Arc::new(crate::media::Frame {
                packet,
                metadata,
                configuration: None,
                received: raw.arrived,
            })));
        source.state = "live";
        self.changed.notify_waiters();
        Ok(())
    }

    pub(super) fn suspend_previews(&self, page: Reference) {
        let mut sources = self.previews.lock().expect("Browser previews");
        for source in sources
            .entries
            .values_mut()
            .filter(|source| source.page == page && matches!(source.state, "starting" | "live"))
        {
            source.interruptions.fetch_add(1, Ordering::SeqCst);
            source.state = "reconfiguring";
            source.frames.send_replace(None);
        }
    }

    pub(super) async fn restart_previews(
        &self,
        page: Reference,
        deadline: Instant,
    ) -> Result<(), Error> {
        let _lane = tokio::time::timeout_at(deadline, self.preview_lane.lock())
            .await
            .map_err(|_| Error::from("MEDIA_BUSY"))?;
        let reference = self
            .previews
            .lock()
            .expect("Browser previews")
            .entries
            .values()
            .find(|source| {
                source.page == page && matches!(source.state, "starting" | "live" | "reconfiguring")
            })
            .map(|source| source.reference);
        let Some(reference) = reference else {
            return Ok(());
        };
        let result = self.restart_preview(reference, page, deadline).await;
        if let Err(error) = &result {
            let mut sources = self.previews.lock().expect("Browser previews");
            let source = sources.get_mut(reference)?;
            source.failure = Some(error.code);
            source.state = "cleanup_pending";
            source.frames.send_replace(None);
            source.alive.store(false, MemoryOrder::Release);
        }
        result
    }

    async fn restart_preview(
        &self,
        reference: Reference,
        page: Reference,
        deadline: Instant,
    ) -> Result<(), Error> {
        let (wire, options) = {
            let mut sources = self.previews.lock().expect("Browser previews");
            let source = sources.get_mut(reference)?;
            if source.state != "reconfiguring" {
                source.interruptions.fetch_add(1, Ordering::SeqCst);
            }
            source.state = "reconfiguring";
            source.frames.send_replace(None);
            (source.wire.clone(), source.options.clone())
        };
        let connection = self.connection()?;
        connection
            .retained(&wire, "screencastStop", json!({}), deadline, true)
            .await?;
        connection.frame_lane().detach(&wire);
        let scope = self.page_scope(page, None, deadline).await?;
        // setViewportSize acknowledges layout before Firefox has painted the
        // resized surface. Do not restart capture against its previous paint.
        self.evaluate(&scope.wire, super::page::PAINT, Value::Null, deadline)
            .await?;
        let geometry = self
            .evaluate(&scope.wire, super::page::GEOMETRY, Value::Null, deadline)
            .await?;
        let width = geometry["viewport"]["width"]
            .as_u64()
            .ok_or("GEOMETRY_UNAVAILABLE")?;
        let height = geometry["viewport"]["height"]
            .as_u64()
            .ok_or("GEOMETRY_UNAVAILABLE")?;
        if width == 0 || height == 0 || width * height > 8_000_000 {
            return Err("MEDIA_GEOMETRY_BOUNDS".into());
        }
        let size = crate::browser::preview::source_size(width, height)?;
        let epoch = Uuid::new_v4();
        connection.frame_lane().attach(wire.clone(), epoch)?;
        {
            let mut sources = self.previews.lock().expect("Browser previews");
            let source = sources.get_mut(reference)?;
            source.epoch = epoch;
            source.viewport_epoch = Uuid::new_v4();
            source.geometry = Some(geometry);
            source.bounds = contract::Viewport {
                width: width as u32,
                height: height as u32,
            };
            source.frame = Some(scope.reference);
            source.document_floor = 0;
            source.source_sequence = 0;
            source.state = "starting";
        }
        connection.retained(&wire,"screencastStart",json!({"size":size,"quality":options.quality,"fps":options.fps,"sendFrames":true,"record":false}),deadline,false).await?;
        Ok(())
    }
}
