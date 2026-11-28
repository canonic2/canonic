//! Simulator preview subscriptions and viewer packet presentation.
use super::{Frame, Health, Media, Preview, encode_packet};
use crate::{
    error::{Error, Result},
    simulator::Source,
};
use serde_json::{Value, json};
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::{Semaphore, mpsc, watch};
use uuid::Uuid;

impl Media {
    #[expect(
        clippy::too_many_lines,
        reason = "Preview admission and its subscription pump share one consumer lifetime"
    )]
    pub(crate) async fn preview(
        &self,
        source: Arc<Source>,
        target: &Value,
        format: &str,
    ) -> Result<Value> {
        let stream_id = Uuid::new_v4().to_string();
        let subscription = source.subscribe(format);
        subscription.keyframe();
        let mut subscribed_frames = subscription.frames.clone();
        let mut source_health = subscription.health.clone();
        let source = Arc::downgrade(&source);
        let worker_stream = stream_id.clone();
        let worker_target = target.clone();
        let (frames, mut readiness) = watch::channel(None);
        let (requests, mut commands) = mpsc::channel::<()>(16);
        let health = Arc::new(Mutex::new(Health {
            last_seen: Instant::now(),
            encoded: 0,
            activity: "0".into(),
            alive: true,
        }));
        let worker_health = health.clone();
        let sender = frames.clone();
        let worker = tokio::spawn(async move {
            let mut configuration = None;
            loop {
                tokio::select! {
                    result = source_health.changed() => {
                        if result.is_err() { break; }
                        let Some(seen) = *source_health.borrow_and_update() else { break; };
                        if let Ok(mut health) = worker_health.lock() { health.last_seen = seen; }
                        continue;
                    }
                    command = commands.recv() => {
                        if command.is_none() { break; }
                        if let Some(source) = source.upgrade() { source.keyframe(); }
                        continue;
                    }
                    result = subscribed_frames.changed() => if result.is_err() { break; },
                }
                let Some(message) = subscribed_frames.borrow_and_update().clone() else {
                    break;
                };
                let mut metadata = message.metadata.clone();
                metadata["streamId"] = json!(worker_stream);
                metadata["target"] = worker_target.clone();
                let key = metadata["keyframe"] == true;
                if metadata["codec"] == "h264" && key {
                    let Ok(bytes) = crate::media::h264_configuration(&message.payload) else {
                        break;
                    };
                    let Ok(packet) = encode_packet(&metadata, &bytes, 2, true) else {
                        break;
                    };
                    configuration = Some(packet);
                }
                if metadata["codec"] == "h264" && configuration.is_none() {
                    continue;
                }
                let Ok(packet) = encode_packet(&metadata, &message.payload, 1, key) else {
                    break;
                };
                if let Ok(mut health) = worker_health.lock() {
                    health.last_seen = Instant::now();
                    health.encoded += 1;
                    health.activity = metadata["activityCounter"].as_str().unwrap_or("0").into();
                }
                sender.send_replace(Some(Arc::new(Frame {
                    packet,
                    metadata,
                    configuration: configuration.clone(),
                    received: Instant::now(),
                })));
            }
            sender.send_replace(None);
            if let Ok(mut health) = worker_health.lock() {
                health.alive = false;
            }
        });
        let preview = Preview {
            browser: false,
            _simulator: Some(subscription),
            grants: Arc::default(),
            frames,
            slots: Arc::new(Semaphore::new(8)),
            worker,
            requests,
            health,
            #[cfg(target_os = "macos")]
            _source: None,
            delivery: None,
        };
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if readiness.borrow().is_some() {
                    return Ok::<(), Error>(());
                }
                readiness.changed().await.map_err(|_| {
                    Error::new(
                        "NATIVE_STREAM_ENDED",
                        "Simulator preview ended before its first frame",
                    )
                })?;
            }
        })
        .await
        .map_err(|_| {
            Error::new(
                "NATIVE_STREAM_UNAVAILABLE",
                "Simulator preview did not deliver a frame within five seconds",
            )
        })??;
        let frame = readiness
            .borrow()
            .clone()
            .ok_or_else(|| Error::new("NATIVE_STREAM_ENDED", "Simulator preview ended"))?;
        self.shared
            .previews
            .lock()
            .map_err(|_| Error::new("INTERNAL", "Media registry unavailable"))?
            .insert(stream_id.clone(), preview);
        Ok(
            json!({"streamId":stream_id,"mediaUrl":format!("{}/v1/ios/streams/{stream_id}/media",self.address),"grantBrokerPath":self.broker,"state":"live","profile":{"codec":format,"sourceMode":frame.metadata["sourceMode"],"fpsCeiling":30,"width":frame.metadata["width"],"height":frame.metadata["height"],"hardwareEncoder":frame.metadata["hardwareEncoder"]},"target":target}),
        )
    }
}
