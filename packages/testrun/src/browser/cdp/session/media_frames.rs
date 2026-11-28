//! Bounded native JPEG decoding and document-fenced publication, independent of viewer ACKs.
use super::*;
use base64::Engine as _;
use std::sync::atomic::Ordering;

#[expect(
    clippy::too_many_lines,
    reason = "One publication fences source/document epochs before publishing decoded pixels and their coordinate observation together"
)]
pub(super) async fn run(
    connection: Connection,
    mut frames: crate::browser::frames::Frames<String>,
    sources: Arc<Mutex<BTreeMap<Reference, super::media::NativeSource>>>,
    observations: Arc<Mutex<crate::browser::input::State>>,
) {
    while let Some(raw) = frames.next().await {
        let selected = {
            let mut entries = sources.lock().expect("Native sources");
            entries
                .iter_mut()
                .find(|(_, entry)| {
                    entry.source.wire == raw.page
                        && matches!(entry.source.state, "starting" | "live")
                })
                .and_then(|(reference, entry)| {
                    if entry.source.epoch != raw.epoch {
                        entry.source.state = "reconfiguring";
                        entry.source.interruptions.fetch_add(1, Ordering::SeqCst);
                        entry.source.frames.send_replace(None);
                        return None;
                    }
                    if entry.source.frames.borrow().as_ref().is_some_and(|frame| {
                        frame.received.elapsed()
                            < Duration::from_secs_f64(1.0 / f64::from(entry.source.options.fps))
                    }) {
                        entry.source.discarded += 1;
                        return None;
                    }
                    Some((*reference, entry.scope.clone()))
                })
        };
        let Some((reference, scope)) = selected else {
            continue;
        };
        let result = decode_frame(&connection, &scope, raw).await;
        let mut entries = sources.lock().expect("Native sources");
        let Some(entry) = entries.get_mut(&reference) else {
            continue;
        };
        let (raw, bytes, width, height, geometry) = match result {
            Ok(frame) => frame,
            Err(_) => {
                entry.decode_failures = entry.decode_failures.saturating_add(1);
                entry.source.state = "reconfiguring";
                entry.source.frames.send_replace(None);
                entry.source.interruptions.fetch_add(1, Ordering::SeqCst);
                if entry.decode_failures >= 3 {
                    entry.source.state = "failed";
                    entry.source.failure = Some("MEDIA_DECODE_FAILED");
                    entry.source.alive.store(false, Ordering::Release);
                }
                continue;
            }
        };
        let source = &mut entry.source;
        entry.decode_failures = 0;
        if source.epoch != raw.epoch
            || connection.frame_lane().epoch(&scope.session) != Some(raw.epoch)
            || !matches!(source.state, "starting" | "live")
        {
            source.discarded += 1;
            source.frames.send_replace(None);
            continue;
        }
        if source.geometry.as_ref() != Some(&geometry) {
            source.viewport_epoch = Uuid::new_v4();
        }
        source.geometry = Some(geometry.clone());
        source.sequence += 1;
        source.source_dropped += raw
            .source_sequence
            .saturating_sub(source.source_sequence + 1);
        source.source_sequence = raw.source_sequence;
        let viewport = &geometry["viewport"];
        let exact = viewport["width"] == width
            && viewport["height"] == height
            && raw.params["metadata"]["scrollOffsetX"] == geometry["scroll"]["x"]
            && raw.params["metadata"]["scrollOffsetY"] == geometry["scroll"]["y"]
            && raw.params["metadata"]["offsetTop"] == 0
            && geometry["visualViewport"]["scale"] == 1
            && geometry["pointerLocked"] != true;
        let metadata = json!({"streamId":reference.id,"platform":"browser","backend":"chrome_cdp","page":source.page,"frame":scope.frame,"sourceEpoch":source.epoch,"viewportEpoch":source.viewport_epoch,"seq":source.sequence.to_string(),"codec":"jpeg","mimeType":"image/jpeg","width":width,"height":height,"logicalViewport":viewport,"dpr":geometry["dpr"],"sourceTimestamp":raw.params["metadata"]["timestamp"],"clockDomain":"cdp-monotonic-unverified","arrivalUnixMs":raw.arrival_unix_ms,"publicationUnixMs":crate::browser::capture::now_ms(),"sourceDroppedFrames":source.source_dropped+source.discarded,"contentGeometryVerified":exact,"transform":if exact{Some(json!({"cssToPixels":1,"padding":{"right":0,"bottom":0},"documentOrigin":geometry["scroll"]}))}else{None},"coordinateInputAvailable":false,"privacy":"local-private"});
        let mut metadata = metadata;
        if exact {
            let observation = observations.lock().expect("Media observations").observe(
                source.page,
                scope.frame,
                geometry,
                raw.arrived,
                0,
            );
            metadata["inputObservation"] = json!(observation);
            metadata["coordinateInputAvailable"] = json!(true);
            metadata["inputCoordinateSpace"] = json!("css-viewport");
            metadata["inputObservationTtlMs"] = json!(10000);
        }
        let packet = match crate::media::encode(&metadata, &bytes) {
            Ok(packet) => packet,
            Err(_) => {
                source.failure = Some("INVALID_MEDIA_FRAME");
                source.alive.store(false, Ordering::Release);
                continue;
            }
        };
        source
            .frames
            .send_replace(Some(Arc::new(crate::media::Frame {
                packet,
                metadata,
                configuration: None,
                received: raw.arrived,
            })));
        source.state = "live";
    }
    let failure = connection.frame_lane().failure();
    for entry in sources.lock().expect("Native sources").values_mut() {
        entry.source.alive.store(false, Ordering::Release);
        entry.source.frames.send_replace(None);
        if entry.source.state != "stopped" {
            entry.source.state = "failed";
            entry.source.failure = Some(failure.unwrap_or("MEDIA_SOURCE_LOST"));
        }
    }
}
async fn decode_frame(
    connection: &Connection,
    scope: &page::Scope,
    raw: crate::browser::frames::RawFrame<String>,
) -> Result<
    (
        crate::browser::frames::RawFrame<String>,
        Vec<u8>,
        u32,
        u32,
        Value,
    ),
    Error,
> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(raw.params["data"].as_str().ok_or("INVALID_MEDIA_FRAME")?)
        .map_err(|_| Error::from("INVALID_MEDIA_FRAME"))?;
    let (bytes, width, height) = tokio::task::spawn_blocking(move || {
        let mut reader =
            image::ImageReader::with_format(std::io::Cursor::new(&bytes), image::ImageFormat::Jpeg);
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(8192);
        limits.max_image_height = Some(8192);
        limits.max_alloc = Some(64 * 1024 * 1024);
        reader.limits(limits);
        let image = reader
            .decode()
            .map_err(|_| Error::from("INVALID_MEDIA_FRAME"))?;
        if u64::from(image.width()) * u64::from(image.height()) > 8_000_000 {
            return Err(Error::from("MEDIA_GEOMETRY_BOUNDS"));
        }
        Ok((bytes, image.width(), image.height()))
    })
    .await
    .map_err(|_| Error::from("MEDIA_DECODE_FAILED"))??;
    let result=connection.call(Some(&scope.session),"Runtime.evaluate",json!({"expression":format!("({})()",include_str!("../../reading/geometry.js")),"uniqueContextId":scope.unique,"returnByValue":true}),Lane::Ordinary,Instant::now()+Duration::from_secs(3)).await.map_err(protocol_error)?;
    if result.get("exceptionDetails").is_some() {
        return Err("DOCUMENT_CHANGED".into());
    }
    Ok((
        raw,
        bytes,
        width,
        height,
        page::query_result(result["result"]["value"].clone())?,
    ))
}
