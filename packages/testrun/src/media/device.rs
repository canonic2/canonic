//! Physical continuous-video subscriptions and preview worker ownership.
use super::{Health, Media, Preview, encode_packet};
use crate::error::{Error, Result};
use serde_json::{Value, json};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::{Semaphore, watch};
use uuid::Uuid;

impl Media {
    #[cfg(target_os = "macos")]
    pub async fn device_preview(
        &self,
        source: Arc<crate::device_media::Source>,
        target: &Value,
    ) -> Result<Value> {
        let stream_id = Uuid::new_v4().to_string();
        let mut lifecycle = source.subscribe_latest();
        let initial = lifecycle
            .borrow()
            .clone()
            .ok_or_else(|| Error::new("FIRST_FRAME_TIMEOUT", "Device source has no frame"))?;
        let (frames, _) = watch::channel(Some(Arc::new(initial.media.clone())));
        let (delivery, _) = tokio::sync::broadcast::channel(64);
        let mut incoming = source.subscribe_frames();
        let output = delivery.clone();
        let latest = frames.clone();
        let (requests, mut commands) = tokio::sync::mpsc::channel::<()>(8);
        let keyframes = source.request_sender();
        let health = Arc::new(Mutex::new(Health {
            last_seen: std::time::Instant::now(),
            encoded: 0,
            activity: "0".into(),
            alive: true,
        }));
        let worker_health = health.clone();
        let worker_id = stream_id.clone();
        let worker = tokio::spawn(async move {
            loop {
                tokio::select! {
                    message=incoming.recv()=>{match message{
                        Ok(value)=>{
                            let mut frame=value.media.clone();frame.metadata["streamId"]=json!(worker_id);
                            let offset=24+u32::from_be_bytes(frame.packet[12..16].try_into().unwrap_or([0;4]))as usize;
                            match encode_packet(&frame.metadata,&frame.packet[offset..],1,value.key){Ok(packet)=>frame.packet=packet,Err(_)=>break}
                        match encode_packet(&frame.metadata,&value.format.description,2,true){Ok(packet)=>frame.configuration=Some(packet),Err(_)=>break}
                            let frame=Arc::new(frame);latest.send_replace(Some(frame.clone()));let _=output.send(frame);
                            if let Ok(mut h)=worker_health.lock(){h.last_seen=std::time::Instant::now();h.encoded+=1;}
                        },
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_))=>{let _=keyframes.try_send(());},
                        Err(_)=>break,
                    }},
                    command=commands.recv()=>{if command.is_none(){break;}let _=keyframes.try_send(());},
                changed=lifecycle.changed()=>{if changed.is_err()||lifecycle.borrow().is_none(){break;}},
                    _=tokio::time::sleep(Duration::from_secs(10))=>break,
                }
            }
            latest.send_replace(None);
            if let Ok(mut h) = worker_health.lock() {
                h.alive = false;
            }
        });
        let profile = json!({"codec":"hevc","codecString":initial.media.metadata["codecString"],"payloadFraming":"length-prefixed-4","sourceMode":"continuous_video","fpsCeiling":60,"width":initial.format.width,"height":initial.format.height,"hardwareEncoder":"device"});
        self.shared
            .previews
            .lock()
            .map_err(|_| Error::new("INTERNAL", "Media registry unavailable"))?
            .insert(
                stream_id.clone(),
                Preview {
                    browser: false,
                    _simulator: None,
                    grants: Arc::default(),
                    frames,
                    slots: Arc::new(Semaphore::new(8)),
                    worker,
                    requests,
                    health,
                    _source: Some(source),
                    delivery: Some(delivery),
                },
            );
        Ok(
            json!({"streamId":stream_id,"mediaUrl":format!("{}/v1/ios/streams/{stream_id}/media",self.address),"grantBrokerPath":self.broker,"state":"live","profile":profile,"target":target}),
        )
    }
}
