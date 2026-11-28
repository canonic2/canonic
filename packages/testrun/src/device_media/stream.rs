//! CoreDevice stream acquisition, RTP feedback, frame assembly and remote release.
use super::{
    ConnectionOwner, apple, hevc,
    lifecycle::{JoinState, Lifecycle, Release},
    negotiation,
    observation::Observation,
    owner_id,
    source::{Source, VideoFrame},
};
use crate::error::{Error, Result};
use idevice::core_device::{
    ReportBlock, RtpPacket, build_frame_ack, build_keyframe_request, build_receiver_report,
    build_sdes, build_start_video_parameters, is_rtcp,
};
use idevice::tcp::handle::UdpSocketHandle;
use serde_json::{Value, json};
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::{broadcast, mpsc, oneshot, watch};
use uuid::Uuid;

/// The device keeps a closed stream until it has had no feedback for this long,
/// and offers no stop for one session, so disconnect waits about this long.
/// Feedback goes out every second, so a host stall shorter than this keeps the
/// stream alive.
const RTCP_TIMEOUT_SECS: u64 = 5;

struct RemoteRelease {
    serial: String,
    session: Uuid,
    probe: tokio::sync::Mutex<Option<ConnectionOwner>>,
}
impl RemoteRelease {
    async fn release_stream(
        &self,
        socket: UdpSocketHandle,
        connection: ConnectionOwner,
    ) -> Result<()> {
        drop(socket);
        // Keep the stream connection in the verifier's owned slot before the
        // bounded cleanup starts. A retry must witness its actual closure too.
        *self.probe.lock().await = Some(connection);
        self.verify().await
    }

    async fn verify_on_probe(&self) -> Result<()> {
        let mut probe = self.probe.lock().await;
        // A timed-out stream or verifier connection stays owned for retry.
        if let Some(connection) = probe.as_mut() {
            connection.close().await;
        }
        let connection =
            when_attached(USB_RETURN_GRACE, || ConnectionOwner::connect(&self.serial)).await?;
        let connection = probe.insert(connection);
        let result = wait_released(connection, self.session).await;
        connection.close().await;
        probe.take();
        result
    }
}
impl Release for RemoteRelease {
    fn verify(
        &self,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
        Box::pin(async move {
            tokio::time::timeout(Duration::from_secs(30), self.verify_on_probe())
                .await
                .unwrap_or_else(|_| {
                    Err(Error::new(
                        "CLEANUP_FAILED",
                        "Owned remote release verification timed out",
                    )
                    .details(json!({"remoteReleasePending":true})))
                })
        })
    }
}
async fn invoke(
    connection: &mut ConnectionOwner,
    feature: &str,
    input: idevice::xpc::Dictionary,
) -> Result<Value> {
    // Generic CoreDevice client retains typed UUIDs instead of converting them to strings.
    // The service supplies its action identifier for startmediastream internally.
    connection
        .invoke(
            feature,
            "com.apple.coredevice.action.mediastreamstart",
            input,
        )
        .await
}
#[expect(
    clippy::too_many_lines,
    reason = "Remote acquisition, readiness and rollback are one source transaction"
)]
pub(super) async fn start(
    serial: &str,
    target: &Value,
    ownership: Arc<super::startup::Ownership>,
) -> Result<Source> {
    let mut connection = ConnectionOwner::connect(serial).await?;
    let status = connection.status().await?;
    if status["running"] != false || status["sessions"].as_array().is_none_or(|s| !s.is_empty()) {
        let mut error = Error::new(
            "CAPTURE_CONFLICT",
            "Device already has a CoreDevice media session; Testrun will not replace it",
        ).details(json!({"blockingState":"remote_media_session",
            "running":status["running"],"sessionCount":status["sessions"].as_array().map(Vec::len),
            "retryAfterMs":20000,
            "recoveryAction":"Close capture through its owning tool. If that owner terminated, allow CoreDevice feedback expiry before retrying; Testrun does not stop foreign sessions."
        }));
        error.retryable = true;
        return Err(error);
    }
    let socket = connection.tunnel.bind_udp(0).await.map_err(|_| {
        Error::new(
            "DEVICE_MEDIA_UNAVAILABLE",
            "Cannot bind device media receiver",
        )
    })?;
    let session = Uuid::new_v4();
    let remote = Arc::new(RemoteRelease {
        serial: serial.into(),
        session,
        probe: tokio::sync::Mutex::new(None),
    });
    // Invocation may allocate remotely even if its reply is lost.
    ownership.allocated(remote.clone());
    let ssrc = Uuid::new_v4().as_u128() as u32;
    let negotiated = async {
        let mut params = build_start_video_parameters(
            &connection.tunnel.host_ip().to_string(),
            socket.local_port(),
            &connection.tunnel.peer_ip().to_string(),
            0,
            negotiation::offer(ssrc)?,
            140,
            1,
            session,
        );
        params.insert(
            "timeout".into(),
            idevice::xpc::XPCObject::UInt64(RTCP_TIMEOUT_SECS),
        );
        let answer = invoke(
            &mut connection,
            "com.apple.coredevice.feature.startmediastream",
            params,
        )
        .await?;
        let cfg = &answer["connection"]["streamConfig"];
        if cfg["SRTPCipherSuite"].as_u64() != Some(0) {
            return Err(Error::new(
                "CAPABILITY_UNAVAILABLE",
                "Encrypted media is not supported by this adapter",
            ));
        }
        let port = cfg["SourcePort"]
            .as_u64()
            .and_then(|p| u16::try_from(p).ok())
            .ok_or_else(|| Error::new("PLATFORM_PROTOCOL", "Device omitted its media port"))?;
        let ours = cfg["RemoteSSRC"]
            .as_u64()
            .and_then(|p| u32::try_from(p).ok())
            .ok_or_else(|| Error::new("PLATFORM_PROTOCOL", "Missing receiver SSRC"))?;
        let theirs = cfg["LocalSSRC"]
            .as_u64()
            .and_then(|p| u32::try_from(p).ok())
            .ok_or_else(|| Error::new("PLATFORM_PROTOCOL", "Missing sender SSRC"))?;
        let pt = cfg["RxPayloadType"].as_u64().unwrap_or(100) as u8;
        Ok::<_, Error>((port, ours, theirs, pt))
    }
    .await;
    let (port, ours, theirs, pt) = match negotiated {
        Ok(values) => values,
        Err(error) => {
            let cleanup = remote.release_stream(socket, connection).await;
            if cleanup.is_ok() {
                ownership.released();
            }
            return Err(error.details(json!({"cleanup":cleanup.err()})));
        }
    };
    let (frames, _) = broadcast::channel(64);
    let (sender, latest) = watch::channel(None);
    let (commands, requests) = mpsc::channel(8);
    let (stop, signal) = oneshot::channel();
    let target = target.clone();
    let source_target = target.clone();
    let output = frames.clone();
    let hid = connection.hid_transport();
    let consumer = connection.consumer_transport();
    let device_info = connection
        .device_info_port
        .map(|port| (connection.tunnel.clone(), port));
    let audio = Some(connection.audio_context(session));
    let display_geometry = connection.display_geometry().await;
    let observation = match Observation::start(display_geometry.clone(), commands.clone()) {
        Ok(observation) => observation,
        Err(error) => {
            let cleanup = remote.release_stream(socket, connection).await;
            if cleanup.is_ok() {
                ownership.released();
            }
            return Err(error.details(json!({"cleanup":cleanup.err()})));
        }
    };
    let job_observation = observation.clone();
    let stream_error = Arc::new(Mutex::new(None));
    let job_error = stream_error.clone();
    let job_ownership = ownership.clone();
    let job_remote = remote.clone();
    let job = tokio::spawn(async move {
        let result = receive(
            &socket,
            port,
            ours,
            theirs,
            pt,
            &target,
            session,
            &output,
            &sender,
            requests,
            signal,
            job_observation,
        )
        .await;
        sender.send_replace(None);
        // BYE identifies only our RTP participant; never use stopAll.
        let mut bye = vec![0x81, 203, 0, 1];
        bye.extend_from_slice(&ours.to_be_bytes());
        let _ = socket.send_to(port, bye).await;
        let cleanup = job_remote.release_stream(socket, connection).await;
        let result = finish_release(result, cleanup, &job_error);
        if result.is_ok() {
            job_ownership.released();
        }
        result
    });
    let lifecycle = Arc::new(Lifecycle {
        stream_error,
        remote: Some(remote),
        observation: Some(observation.clone()),
        stop: Mutex::new(Some(stop)),
        join: tokio::sync::Mutex::new(JoinState {
            job: Some(job),
            result: None,
        }),
    });
    ownership.streaming(lifecycle.clone());
    let mut readiness = latest.clone();
    let ready = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if readiness.borrow().is_some() {
                return Ok(());
            }
            readiness.changed().await.map_err(|_| {
                Error::new(
                    "DEVICE_MEDIA_UNAVAILABLE",
                    "Device video ended before first frame",
                )
            })?;
        }
    })
    .await;
    let source = Source {
        audio,
        device_info,
        consumer,
        display_geometry,
        hid,
        observation: observation.clone(),
        target: source_target,
        frames,
        latest,
        commands,
        lifecycle,
    };
    match ready {
        Ok(Ok(())) => Ok(source),
        other => {
            let cleanup = source.close().await;
            let error = match other {
                Ok(Err(e)) => e,
                _ => Error::new(
                    "FIRST_FRAME_TIMEOUT",
                    "Device did not deliver a decodable frame within ten seconds",
                ),
            };
            Err(error.details(
                json!({"cleanup":cleanup.err(),"streamError":source.lifecycle.stream_error()}),
            ))
        }
    }
}
pub(super) fn finish_release(
    stream: Result<()>,
    release: Result<()>,
    diagnostic: &Mutex<Option<Error>>,
) -> Result<()> {
    let stream_error = stream.err();
    *diagnostic.lock().unwrap_or_else(|e| e.into_inner()) = stream_error.clone();
    release.map_err(|error| {
        Error::new("CLEANUP_FAILED", "Owned media release is unverified")
            .details(json!({"cause":error,"streamError":stream_error,"remoteReleasePending":true}))
    })
}

/// A replugged phone takes a moment to reappear on USB. Release verification
/// waits this long for it instead of failing on the first absent inventory.
const USB_RETURN_GRACE: Duration = Duration::from_secs(8);

/// Retry `connect` while it reports `USB_UNAVAILABLE`, up to `grace`. Other
/// errors and the last unavailable result are returned unchanged.
async fn when_attached<T, F, Fut>(grace: Duration, mut connect: F) -> Result<T>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T>>,
{
    let deadline = tokio::time::Instant::now() + grace;
    loop {
        match connect().await {
            Err(error)
                if error.code == "USB_UNAVAILABLE" && tokio::time::Instant::now() < deadline =>
            {
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
            result => return result,
        }
    }
}

async fn wait_released(monitor: &mut ConnectionOwner, session: Uuid) -> Result<()> {
    let session = session.to_string();
    for _ in 0..23 {
        let status = monitor.status().await?;
        let sessions = status["sessions"].as_array().ok_or_else(|| {
            Error::new("CLEANUP_FAILED", "Device omitted media session ownership")
        })?;
        let mut exists = false;
        for entry in sessions {
            let id = owner_id(entry).ok_or_else(|| {
                Error::new(
                    "CLEANUP_FAILED",
                    "Cannot verify unknown remote media ownership",
                )
            })?;
            exists |= id.eq_ignore_ascii_case(&session);
        }
        if !exists {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    Err(Error::new(
        "CLEANUP_FAILED",
        "Owned device media session survived transport shutdown; no foreign session was stopped",
    ))
}
#[expect(
    clippy::too_many_arguments,
    reason = "One receive loop owns negotiated media transport and output channels"
)]
async fn receive(
    socket: &UdpSocketHandle,
    port: u16,
    ours: u32,
    theirs: u32,
    pt: u8,
    target: &Value,
    epoch: Uuid,
    frames: &broadcast::Sender<Arc<VideoFrame>>,
    latest: &watch::Sender<Option<Arc<VideoFrame>>>,
    mut requests: mpsc::Receiver<()>,
    mut stop: oneshot::Receiver<()>,
    observation: Arc<Observation>,
) -> Result<()> {
    let mut assembler = hevc::Assembler::new();
    let mut format: Option<Arc<apple::Format>> = None;
    let mut parameters = [Vec::new(), Vec::new(), Vec::new()];
    let mut configuration_sequence = 0;
    let mut seq = 0u64;
    let mut origin = None;
    let mut elapsed_ticks = 0u64;
    let started = Instant::now();
    let mut feedback = tokio::time::interval(Duration::from_secs(1));
    let mut last_pli = Instant::now() - Duration::from_secs(3);
    let mut highest = 0;
    let mut last_frame = Instant::now();
    let mut control_feedback = tokio::time::interval(Duration::from_millis(50));
    let mut last_timestamp = 0u32;
    let mut packets = 0u32;
    let mut frame_packets = 0u32;
    let mut previous_frame_packets = 0u32;
    let mut fir = 0u8;
    loop {
        tokio::select! {
            _=&mut stop=>return Ok(()),
            command=requests.recv()=>{if command.is_some()&&last_pli.elapsed()>Duration::from_secs(1){fir=fir.wrapping_add(1);socket.send_to(port,build_keyframe_request(ours,"Testrun",theirs,&[],fir)).await.map_err(|_|Error::new("DEVICE_DISCONNECTED","Keyframe request failed"))?;last_pli=Instant::now();}},
            _=control_feedback.tick(),if packets>0=>{
                let mut report=vec![0x80,204,0,7];report.extend(ours.to_be_bytes());report.extend(b"RCTL");
                for word in [0x85000004,((last_timestamp>>8)&65535)<<16,previous_frame_packets,((last_timestamp/24)&65535)<<16,((packets&65535)<<16)|0xea61]{report.extend(word.to_be_bytes());}
                report.extend(build_frame_ack(ours,last_timestamp));
                socket.send_to(port,report).await.map_err(|_|Error::new("DEVICE_DISCONNECTED","Receiver feedback failed"))?;
            },
            _=feedback.tick()=>{
                let mut report=build_receiver_report(ours,&[ReportBlock{source_ssrc:theirs,highest_seq:highest,..ReportBlock::default()}]);report.extend(build_sdes(ours,"Testrun"));
                socket.send_to(port,report).await.map_err(|_|Error::new("DEVICE_DISCONNECTED","Media feedback failed"))?;
                if last_pli.elapsed()>Duration::from_secs(2){fir=fir.wrapping_add(1);socket.send_to(port,build_keyframe_request(ours,"Testrun",theirs,&[],fir)).await.map_err(|_|Error::new("DEVICE_DISCONNECTED","Media recovery failed"))?;last_pli=Instant::now();}
                if last_frame.elapsed()>Duration::from_secs(10){return Err(Error::new("CAPTURE_STALLED","Device did not deliver a decodable frame for ten seconds"));}
            },
            datagram=socket.recv()=>{
                let datagram=datagram.map_err(|_|Error::new("DEVICE_DISCONNECTED","Device media transport closed"))?;
                if datagram.source_port!=port||is_rtcp(&datagram.data){continue;}
                let Some(packet)=RtpPacket::parse(&datagram.data)else{continue;};
                if packet.ssrc!=theirs||packet.payload_type!=pt{continue;}
                last_timestamp=packet.timestamp;packets=packets.wrapping_add(1);frame_packets+=1;if packet.marker{previous_frame_packets=frame_packets;frame_packets=0;}
                highest=u32::from(packet.sequence_number);
                let Some(unit)=assembler.push(&packet)else{continue;};
                if parameters!=assembler.parameters{
                    if !unit.key{continue;}
                    parameters=assembler.parameters.clone();format=Some(Arc::new(apple::Format::new(&parameters)?));configuration_sequence+=1;
                }
                let Some(format)=format.clone()else{continue;};
                if let Some(previous)=origin{let delta=unit.timestamp.wrapping_sub(previous);if delta>240000{ return Err(Error::new("PLATFORM_PROTOCOL","Invalid device video timestamp discontinuity"));}elapsed_ticks+=u64::from(delta);}
                origin=Some(unit.timestamp);seq+=1;last_frame=Instant::now();let pts=elapsed_ticks*1_000_000_000/24000;
                let metadata=json!({"seq":seq.to_string(),"frameId":format!("{epoch}:{seq}"),"epoch":epoch,"target":target,"displayId":"main","geometryVersion":format!("{}x{}",format.width,format.height),"configurationSequence":configuration_sequence,"width":format.width,"height":format.height,"codec":"hevc","codecString":format.codec()?,"payloadFraming":"length-prefixed-4","sourceMode":"continuous_video","backend":"coredevice_usb","ptsNs":pts.to_string(),"durationNs":"0","deviceTimestamp":unit.timestamp,"deviceClockRate":24000,"timestampDomain":"device_media_relative","captureAgeKnown":false,"privacy":"local","sourceFrames":seq,"sourceFps":seq as f64/started.elapsed().as_secs_f64().max(0.001),"droppedAccessUnits":assembler.dropped,"hardwareEncoder":"device"});
                let config=crate::media::encode_packet(&metadata,&format.description,2,true)?;
                let media=crate::media::Frame{packet:crate::media::encode_packet(&metadata,&unit.payload,1,unit.key)?,metadata,configuration:Some(config),received:Instant::now()};
                let frame=Arc::new(VideoFrame{media,payload:unit.payload,key:unit.key,pts,format,configuration_sequence});
                observation.submit(frame.clone());
                latest.send_replace(Some(frame.clone()));let _=frames.send(frame);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn release_waits_briefly_for_a_replugged_phone_and_only_for_usb_absence() {
        let unavailable = || Error::new("USB_UNAVAILABLE", "Selected device has no USB connection");
        // The phone reappears on the third inventory.
        let attempts = std::cell::Cell::new(0);
        let attached = when_attached(Duration::from_secs(5), || {
            attempts.set(attempts.get() + 1);
            let attempt = attempts.get();
            async move {
                if attempt < 3 {
                    Err(unavailable())
                } else {
                    Ok(attempt)
                }
            }
        })
        .await;
        assert_eq!(attached.unwrap(), 3);
        // Still absent after the grace: the unavailable error is kept.
        let absent = when_attached(Duration::from_millis(600), || async {
            Err::<(), _>(unavailable())
        })
        .await
        .unwrap_err();
        assert_eq!(absent.code, "USB_UNAVAILABLE");
        // Other failures are not retried.
        let calls = std::cell::Cell::new(0);
        let other = when_attached(Duration::from_secs(5), || {
            calls.set(calls.get() + 1);
            async { Err::<(), _>(Error::new("PLATFORM_FAILED", "Acme transport failure")) }
        })
        .await
        .unwrap_err();
        assert_eq!((other.code.as_str(), calls.get()), ("PLATFORM_FAILED", 1));
    }

    #[test]
    fn confirmed_release_preserves_receive_failure_without_failing_cleanup() {
        let diagnostic = Mutex::new(None);
        let result = finish_release(
            Err(Error::new(
                "DEVICE_DISCONNECTED",
                "Device media transport closed",
            )),
            Ok(()),
            &diagnostic,
        );
        assert!(result.is_ok());
        assert_eq!(
            diagnostic.lock().unwrap().as_ref().unwrap().code,
            "DEVICE_DISCONNECTED"
        );
        let failed = finish_release(
            Err(Error::new("CAPTURE_STALLED", "Fixture stopped receiving")),
            Err(Error::new("CLEANUP_FAILED", "Fixture release pending")),
            &diagnostic,
        )
        .unwrap_err();
        assert_eq!(
            failed.details.as_ref().unwrap()["remoteReleasePending"],
            true
        );
        assert_eq!(
            failed.details.as_ref().unwrap()["streamError"]["code"],
            "CAPTURE_STALLED"
        );
        assert_eq!(
            diagnostic.lock().unwrap().as_ref().unwrap().code,
            "CAPTURE_STALLED"
        );
    }
}
