//! Bounded CoreDevice audio acquisition alongside the source's video, with independent release.
use super::{ConnectionOwner, Source, owner_id};
use crate::{
    error::{Error, Result},
    playback::probe::{Samples, clip, failed, limitations},
};
use idevice::{
    core_device::{
        CallInfoBlob, ReportBlock, RtpPacket, build_receiver_report, build_screen_audio_offer,
        build_sdes, build_start_audio_parameters, is_rtcp,
    },
    tcp::handle::{AdapterHandle, UdpSocketHandle},
};
use serde_json::{Value, json};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use uuid::Uuid;
mod decoder;

#[derive(Clone)]
pub(super) struct Context {
    pub(super) tunnel: AdapterHandle,
    pub(super) port: u16,
    pub(super) version: String,
    pub(super) session: Uuid,
}
struct Transport {
    socket: UdpSocketHandle,
    port: u16,
    ours: u32,
    theirs: u32,
    pt: u8,
}

pub(crate) struct Owner {
    serial: String,
    state: tokio::sync::Mutex<State>,
}
struct State {
    source: Option<std::sync::Weak<Source>>,
    pending: Option<Pending>,
}
struct Pending {
    _connection: ConnectionOwner,
    _lease: Option<crate::lease::ControlLease>,
    _source: Option<Arc<Source>>,
    session: Uuid,
}
impl Owner {
    pub(crate) fn new(serial: &str, source: Option<Arc<Source>>) -> Arc<Self> {
        Arc::new(Self {
            serial: serial.into(),
            state: tokio::sync::Mutex::new(State {
                source: source.as_ref().map(Arc::downgrade),
                pending: None,
            }),
        })
    }
    pub(crate) fn refresh_source(&self, source: Option<Arc<Source>>) -> Result<()> {
        let mut state = self
            .state
            .try_lock()
            .map_err(|_| Error::new("CAPTURE_CONFLICT", "An audio probe is already running"))?;
        if state.pending.is_some() {
            return Err(
                failed("previous_cleanup_pending").details(json!({"cleanupVerified":false}))
            );
        }
        state.source = source.as_ref().map(Arc::downgrade);
        Ok(())
    }
    pub(crate) async fn close(&self) -> Result<()> {
        let mut state = self.state.lock().await;
        if let Some(owner) = state.pending.as_mut() {
            // A failed release may have lost its status transport. Reconnect
            // only the read-only monitor, preserving the original stream UUID,
            // lease and borrowed source until audio absence is verified.
            let mut monitor = ConnectionOwner::connect(&self.serial).await?;
            release(&mut monitor, owner.session).await?;
        }
        state.pending.take();
        Ok(())
    }
    #[expect(
        clippy::too_many_lines,
        reason = "Audio acquisition and partial evidence must retain cleanup ownership across every failure"
    )]
    pub(crate) async fn probe(
        &self,
        args: Value,
        request: crate::playback::probe::Context,
    ) -> Result<Value> {
        let mut state = self.state.lock().await;
        if state.pending.is_some() {
            return Err(
                failed("previous_cleanup_pending").details(json!({"cleanupVerified":false}))
            );
        }
        let now = tokio::time::Instant::now();
        if request.deadline.saturating_duration_since(now) <= Duration::from_secs(24) {
            return Err(failed("insufficient_acquisition_and_cleanup_budget"));
        }
        let source = state.source.as_ref().and_then(std::sync::Weak::upgrade);
        let serial = &self.serial;
        // The spawned playback job retains this source until audio cleanup finishes.
        let context = source.as_ref().and_then(|s| s.audio.clone());
        let session = context.as_ref().map_or_else(Uuid::new_v4, |c| c.session);
        let mut lease = None;
        let mut connection = None;
        let mut transport = None;
        let mut attempted = false;
        let mut samples = Samples::new(args["retainClip"] == true);
        let mut health = Health::default();
        let started = Instant::now();
        let observed_start = utc();
        let measurement = tokio::time::timeout_at(
            (now + Duration::from_secs(48)).min(request.deadline - Duration::from_secs(24)),
            async {
                let owner = if let Some(context) = &context {
                    ConnectionOwner::shared(context).await?
                } else {
                    lease = Some(crate::lease::ControlLease::acquire(&format!(
                        "device-media:{serial}"
                    ))?);
                    ConnectionOwner::connect(serial).await?
                };
                connection = Some(owner);
                let owner = connection.as_mut().unwrap();
                readiness(&owner.status().await?, session)?;
                let socket = owner
                    .tunnel
                    .bind_udp(0)
                    .await
                    .map_err(|e| super::platform(e.into()))?;
                let info = CallInfoBlob {
                    call_id: 0,
                    client_version: 1,
                    device_type: "Mac15,9".into(),
                    framework_version: "2205.3.1".into(),
                    os_version: "25F80".into(),
                    device_name: None,
                    audio_device_uid: None,
                };
                let offer =
                    build_screen_audio_offer(&Uuid::new_v4().to_string().to_uppercase(), &info)
                        .map_err(super::platform)?;
                let mut params = build_start_audio_parameters(
                    &owner.tunnel.host_ip().to_string(),
                    socket.local_port(),
                    &owner.tunnel.peer_ip().to_string(),
                    0,
                    offer,
                    140,
                    session,
                );
                // The device keeps a stopped stream until this RTCP timeout
                // (seconds) expires, so release takes about this long.
                params.insert("timeout".into(), idevice::xpc::XPCObject::UInt64(3));
                attempted = true;
                let answer = owner
                    .invoke(
                        "com.apple.coredevice.feature.startmediastream",
                        "com.apple.coredevice.action.mediastreamstart",
                        params,
                    )
                    .await?;
                let negotiated = negotiate(socket, &answer)?;
                transport = Some(negotiated);
                receive(
                    transport.as_ref().unwrap(),
                    args["durationMs"].as_u64().unwrap_or(2000),
                    &mut samples,
                    &mut health,
                )
                .await
            },
        )
        .await
        .unwrap_or_else(|_| Err(failed("measurement_deadline")));
        let cleanup = if attempted {
            tokio::time::timeout(Duration::from_secs(24), async {
                if let Some(transport) = transport.take() {
                    let mut bye = vec![0x81, 203, 0, 1];
                    bye.extend(transport.ours.to_be_bytes());
                    transport
                        .socket
                        .send_to(transport.port, bye)
                        .await
                        .map_err(|e| super::platform(e.into()))?;
                    drop(transport);
                }
                let owner = connection
                    .as_mut()
                    .ok_or_else(|| failed("cleanup_connection_missing"))?;
                release(owner, session).await
            })
            .await
            .unwrap_or_else(|_| Err(failed("remote_release_deadline")))
        } else {
            Ok(())
        };
        // A borrowed video tunnel is never closed by this audio consumer.
        if cleanup.is_err()
            && let Some(connection) = connection.take()
        {
            state.pending = Some(Pending {
                _connection: connection,
                _lease: lease.take(),
                _source: source.clone(),
                session,
            });
        }
        drop(connection);
        drop(lease);
        drop(source);
        let mut result = samples.result();
        result["backend"] = json!("coredevice_system_audio");
        result["sourceEpoch"] = json!(session);
        result["observedStart"] = json!(observed_start);
        result["observedEnd"] = json!(utc());
        result["wallDurationMs"] = json!(started.elapsed().as_millis());
        result["health"] = health.result();
        result["format"] = json!({"codec":"aac_eld","profile":"coredevice_mode_8_48khz_stereo_480","audioSpecificConfig":"F8E65000"});
        result["routes"] = json!({"before":null,"during":null,"after":null,"unavailableReason":"CoreDevice exposes no generic output-route query; AirPlay may return zeros"});
        result["cleanupVerified"] = json!(cleanup.is_ok());
        result["observed"] = json!(measurement.is_ok());
        limitations(&mut result, false);
        crate::playback::stamp(&mut result, &request.target);
        result["actionId"] = json!(request.command);
        if measurement.is_err() || cleanup.is_err() {
            let code = if cleanup.is_ok() {
                measurement
                    .as_ref()
                    .err()
                    .filter(|e| {
                        matches!(
                            e.code.as_str(),
                            "CAPABILITY_UNAVAILABLE" | "CAPTURE_CONFLICT" | "TARGET_BUSY"
                        )
                    })
                    .map(|e| e.code.as_str())
                    .unwrap_or("AUDIO_PROBE_FAILED")
            } else {
                "AUDIO_PROBE_FAILED"
            };
            return Err(Error::new(code,"Audio probe acquisition, measurement or cleanup did not complete").details(json!({"phase":"capture_or_cleanup","cause":measurement.err(),"cleanup":cleanup.err(),"cleanupVerified":result["cleanupVerified"],"partial":result})));
        }
        if let Some(pcm) = samples.pcm() {
            clip(&mut result, pcm)?;
        }
        Ok(result)
    }
}
async fn release(owner: &mut ConnectionOwner, session: Uuid) -> Result<()> {
    tokio::time::timeout(Duration::from_secs(24), async {
        loop {
            if !audio_present(&owner.status().await?, session)? {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    })
    .await
    .unwrap_or_else(|_| {
        Err(failed("remote_release_deadline").details(json!({"cleanupVerified":false})))
    })
}
fn utc() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}
fn readiness(status: &Value, session: Uuid) -> Result<()> {
    let sessions = status["sessions"]
        .as_array()
        .ok_or_else(|| failed("status_protocol"))?;
    for entry in sessions {
        if owner_id(entry).is_none_or(|s| !s.eq_ignore_ascii_case(&session.to_string()))
            || entry["connection"]["type"] != "video"
        {
            return Err(Error::new(
                "CAPTURE_CONFLICT",
                "An existing media session is not this source's video",
            ));
        }
    }
    if sessions.is_empty() && status["running"] != false {
        return Err(Error::new(
            "CAPTURE_CONFLICT",
            "Media server has unknown ownership",
        ));
    }
    Ok(())
}
fn audio_present(status: &Value, session: Uuid) -> Result<bool> {
    let sessions = status["sessions"]
        .as_array()
        .ok_or_else(|| failed("release_status_protocol"))?;
    if sessions.iter().any(|s| owner_id(s).is_none()) {
        return Err(failed("release_unknown_ownership"));
    }
    if sessions.iter().any(|s| {
        owner_id(s).is_some_and(|id| id.eq_ignore_ascii_case(&session.to_string()))
            && !matches!(s["connection"]["type"].as_str(), Some("audio" | "video"))
    }) {
        return Err(failed("release_unknown_stream_type"));
    }
    Ok(sessions.iter().any(|s| {
        owner_id(s).is_some_and(|id| id.eq_ignore_ascii_case(&session.to_string()))
            && s["connection"]["type"] == "audio"
    }))
}
fn negotiate(socket: UdpSocketHandle, answer: &Value) -> Result<Transport> {
    let config = &answer["connection"]["streamConfig"];
    if answer["connection"]["type"] != "audio"
        || config["AudioStreamMode"] != 8
        || config["SRTPCipherSuite"] != 0
        || config["SRTCPCipherSuite"] != 0
    {
        return Err(Error::new(
            "CAPABILITY_UNAVAILABLE",
            "Only unencrypted CoreDevice audio mode 8 is supported",
        ));
    }
    let field = |key: &str| {
        config[key]
            .as_u64()
            .ok_or_else(|| failed("audio_negotiation"))
    };
    let port = u16::try_from(field("SourcePort")?).map_err(|_| failed("audio_port"))?;
    if port == 0 {
        return Err(failed("audio_port"));
    }
    Ok(Transport {
        socket,
        port,
        ours: u32::try_from(field("RemoteSSRC")?).map_err(|_| failed("audio_ssrc"))?,
        theirs: u32::try_from(field("LocalSSRC")?).map_err(|_| failed("audio_ssrc"))?,
        pt: u8::try_from(field("RxPayloadType")?).map_err(|_| failed("audio_payload_type"))?,
    })
}
#[derive(Default)]
struct Health {
    packets: u64,
    highest: u32,
    previous: Option<(u16, u32)>,
    lost: u64,
    decode_errors: u64,
    first: Option<Instant>,
    last: Option<Instant>,
}
impl Health {
    fn result(&self) -> Value {
        json!({"packets":self.packets,"packetLoss":self.lost,"decodeErrors":self.decode_errors,
        "firstPacketAgeMs":self.first.map(|v|v.elapsed().as_millis()),"lastPacketAgeMs":self.last.map(|v|v.elapsed().as_millis())})
    }
    fn accept(&mut self, sequence: u16, timestamp: u32) -> Result<()> {
        if let Some((seq, ts)) = self.previous
            && (sequence != seq.wrapping_add(1) || timestamp != ts.wrapping_add(480))
        {
            self.lost += u64::from(sequence.wrapping_sub(seq).saturating_sub(1));
            return Err(failed("rtp_discontinuity"));
        }
        self.highest = if self.previous.is_none() {
            u32::from(sequence)
        } else {
            self.highest.wrapping_add(1)
        };
        self.previous = Some((sequence, timestamp));
        self.packets += 1;
        self.first.get_or_insert_with(Instant::now);
        self.last = Some(Instant::now());
        Ok(())
    }
}
async fn receive(
    t: &Transport,
    duration: u64,
    samples: &mut Samples,
    health: &mut Health,
) -> Result<()> {
    let mut decoder = decoder::Decoder::new()?;
    let mut feedback = tokio::time::interval(Duration::from_millis(500));
    let mut watchdog = tokio::time::interval(Duration::from_millis(50));
    let started = Instant::now();
    while samples.frames() < duration * 48 {
        tokio::select! {
            _=feedback.tick()=>{
                let mut rr=build_receiver_report(t.ours,&[ReportBlock{source_ssrc:t.theirs,highest_seq:health.highest,..ReportBlock::default()}]);
                rr.extend(build_sdes(t.ours,"Testrun"));
                t.socket.send_to(t.port,rr).await.map_err(|e|super::platform(e.into()))?;
            },
            _=watchdog.tick()=>{
                if health.last.unwrap_or(started).elapsed()>Duration::from_secs(1){return Err(failed("packet_watchdog"));}
                if started.elapsed()>Duration::from_millis(duration+2000){return Err(failed("sample_interval_incomplete"));}
            },
            packet=t.socket.recv()=>{
                let packet=packet.map_err(|e|super::platform(e.into()))?;
                if packet.source_port!=t.port || is_rtcp(&packet.data){continue;}
                let p=RtpPacket::parse(&packet.data).ok_or_else(||failed("rtp_parse"))?;
                if p.ssrc!=t.theirs || p.payload_type!=t.pt {continue;}
                health.accept(p.sequence_number,p.timestamp)?;
                let pcm=decoder.decode(p.payload).inspect_err(|_|health.decode_errors+=1)?;
                let count=((duration*48-samples.frames())*2).min(pcm.len() as u64) as usize;
                samples.push(&pcm[..count])?;
            }
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn refreshing_a_source_refuses_while_a_probe_owns_the_state() {
        let owner = Owner::new("Acme", None);
        let state = owner.state.lock().await;
        assert_eq!(
            owner.refresh_source(None).unwrap_err().code,
            "CAPTURE_CONFLICT"
        );
        drop(state);
        owner.refresh_source(None).unwrap();
        owner.close().await.unwrap();
    }
    #[tokio::test]
    async fn idle_audio_owner_does_not_keep_a_stopped_preview_alive() {
        let source = super::super::source::recording_fixture();
        let owner = Owner::new("Acme", None);
        owner.refresh_source(Some(source.clone())).unwrap();
        {
            let state = owner.state.lock().await;
            assert!(Arc::ptr_eq(
                &state.source.as_ref().unwrap().upgrade().unwrap(),
                &source
            ));
        }
        drop(source);
        assert!(
            owner
                .state
                .lock()
                .await
                .source
                .as_ref()
                .unwrap()
                .upgrade()
                .is_none()
        );
        owner.close().await.unwrap();
    }
    #[test]
    fn readiness_accepts_only_own_video_and_release_ignores_video() {
        let session = Uuid::new_v4();
        let entry = |kind: &str| json!({"connection":{"type":kind,"options":{"avcMediaStreamOptionClientSessionID":{"uuid":session}}}});
        let status = json!({"running":true,"sessions":[entry("video")]});
        readiness(&status, session).unwrap();
        assert!(!audio_present(&status, session).unwrap());
        assert!(readiness(&status, Uuid::new_v4()).is_err());
        let status = json!({"running":true,"sessions":[entry("video"),entry("audio")]});
        assert!(readiness(&status, session).is_err());
        assert!(audio_present(&status, session).unwrap());
        let foreign = json!({"connection":{"type":"audio","options":{"avcMediaStreamOptionClientSessionID":{"uuid":Uuid::new_v4()}}}});
        let status = json!({"running":true,"sessions":[entry("audio"),foreign.clone()]});
        assert!(audio_present(&status, session).unwrap());
        let status = json!({"running":true,"sessions":[foreign.clone()]});
        assert!(!audio_present(&status, session).unwrap());
        assert_eq!(status["sessions"][0], foreign);
        assert!(
            audio_present(
                &json!({"sessions":[{"connection":{"type":"audio"}}]}),
                session
            )
            .is_err()
        );
    }
    #[test]
    fn loss_and_reordering_are_failures_while_wrap_is_valid() {
        let mut health = Health::default();
        health.accept(u16::MAX, u32::MAX - 479).unwrap();
        health.accept(0, 0).unwrap();
        assert_eq!(health.highest, 65536);
        assert!(health.accept(2, 960).is_err());
        assert_eq!(health.lost, 1);
    }
}
