//! One simulator framebuffer owner serving independent capture consumers.
use super::Capture;
use crate::{
    error::{Error, Result},
    simulator::{CaptureCommand as Command, ipc},
};
use serde_json::{Value, json};
use std::{
    io::{BufRead, Read},
    path::PathBuf,
    time::{Duration, Instant},
};

struct Recording {
    id: String,
    path: PathBuf,
    started: Instant,
    duration: Duration,
    frames: u64,
    size_limit: bool,
    size: (u32, u32),
}
struct Worker {
    capture: Capture,
    identity: crate::simulator::incarnation::Incarnation,
    epoch: String,
    started: Instant,
    sequences: [u64; 2],
    configurations: [Configuration; 2],
    profiles: [bool; 2],
    recording: Option<Recording>,
    finalizer: Option<(String, std::thread::JoinHandle<Result<Value>>)>,
}
#[derive(Default)]
struct Configuration {
    size: [u32; 4],
    sequence: u64,
}

#[expect(
    clippy::too_many_lines,
    reason = "The bounded command and frame loop owns the single native capture handle"
)]
pub(crate) fn run(developer: &str, uuid: &str) -> Result<()> {
    let capture = super::startup(Capture::open(developer, uuid))?;
    let identity = super::startup(capture.incarnation(uuid))?;
    let mut worker = Worker {
        capture,
        identity,
        epoch: uuid::Uuid::new_v4().to_string(),
        started: Instant::now(),
        sequences: [0; 2],
        configurations: [Configuration::default(), Configuration::default()],
        profiles: [false; 2],
        recording: None,
        finalizer: None,
    };
    // Readiness proves the surface is usable, without retaining a stale screenshot.
    super::startup(worker.capture.frame(false))?;
    let mut output = std::io::stdout().lock();
    ipc::write(
        &mut output,
        &json!({"event":"ready","epoch":worker.epoch,"identity":worker.identity}),
        &[],
    )?;
    let (commands, requests) = std::sync::mpsc::sync_channel(32);
    std::thread::spawn(move || {
        let mut input = std::io::stdin().lock();
        loop {
            let mut bytes = Vec::new();
            let read = (&mut input).take(16385).read_until(b'\n', &mut bytes);
            let command = match read {
                Ok(1..=16384) => serde_json::from_slice(&bytes).unwrap_or(Command::Close),
                _ => Command::Close,
            };
            let closing = matches!(command, Command::Close);
            if commands.send(command).is_err() || closing {
                break;
            }
        }
    });
    let mut activity = 0;
    let mut heartbeat = Instant::now();
    loop {
        worker.identity.validate()?;
        worker.capture.check_connection()?;
        let tick = Instant::now();
        worker.finalized(&mut output, false)?;
        let mut force = false;
        // Bound control work per turn so a busy screenshot client cannot starve video.
        for _ in 0..32 {
            let Ok(command) = requests.try_recv() else {
                break;
            };
            match command {
                Command::Close => {
                    worker.finish(&mut output)?;
                    worker.finalized(&mut output, true)?;
                    return Ok(());
                }
                Command::Profiles { jpeg, h264 } => {
                    force |= [jpeg, h264] != worker.profiles;
                    worker.profiles = [jpeg, h264];
                }
                Command::Keyframe => force = true,
                command @ (Command::Screenshot { .. }
                | Command::Compact { .. }
                | Command::Geometry { .. }) => {
                    let (id, codec) = match command {
                        Command::Screenshot { id } => (id, 0),
                        Command::Compact { id } => (id, 3),
                        Command::Geometry { id } => (id, -1),
                        _ => unreachable!(),
                    };
                    let result = worker.identity.validate().and_then(|()| {
                        let image = if codec == -1 {
                            worker.capture.geometry().map(|(w, h)| (Vec::new(), w, h))
                        } else {
                            worker.capture.frame_codec(codec)
                        }?;
                        worker.identity.validate()?;
                        Ok(image)
                    });
                    match result {
                        Ok((bytes, width, height)) => {
                            let (sw, sh) = if codec == -1 {
                                (width, height)
                            } else {
                                worker.capture.source_size()
                            };
                            ipc::write(
                                &mut output,
                                &json!({"event":"reply","id":id,"sourceWidth":sw,"sourceHeight":sh}),
                                &bytes,
                            )?
                        }
                        Err(error) => ipc::write(
                            &mut output,
                            &json!({"event":"reply","id":id,"error":error}),
                            &[],
                        )?,
                    }
                }
                Command::Record {
                    id,
                    path,
                    duration_ms,
                } => {
                    if worker.recording.is_some()
                        || worker.finalizer.is_some()
                        || !(100..=600000).contains(&duration_ms)
                    {
                        ipc::write(
                            &mut output,
                            &json!({"event":"reply","id":id,"error":Error::new("RESOURCE_LIMIT","Capture source already has a recording or duration is invalid")}),
                            &[],
                        )?;
                    } else if let Err(error) = worker.capture.recording_start(&path) {
                        ipc::write(
                            &mut output,
                            &json!({"event":"reply","id":id,"error":error}),
                            &[],
                        )?;
                    } else {
                        worker.recording = Some(Recording {
                            id,
                            path,
                            started: Instant::now(),
                            duration: Duration::from_millis(duration_ms),
                            frames: 0,
                            size_limit: false,
                            size: (0, 0),
                        });
                        force = true;
                    }
                }
                Command::StopRecording { id } => {
                    if worker.recording.as_ref().is_some_and(|r| r.id == id) {
                        worker.finish(&mut output)?;
                    }
                }
            }
        }
        if force {
            worker.capture.force_keyframe();
        }
        let observed = worker.capture.activity();
        let changed = force || !worker.capture.callbacks() || observed != activity;
        if worker.profiles[0] && changed {
            worker.frame(&mut output, 0, observed)?;
        }
        if (worker.profiles[1] && changed) || worker.recording.is_some() {
            worker.frame(&mut output, 1, observed)?;
        }
        if worker.recording.as_mut().is_some_and(|r| {
            r.size_limit = r
                .path
                .metadata()
                .is_ok_and(|m| m.len() >= 512 * 1024 * 1024);
            r.size_limit || r.started.elapsed() >= r.duration
        }) {
            worker.finish(&mut output)?;
        }
        activity = observed;
        if heartbeat.elapsed() >= Duration::from_secs(1) {
            ipc::write(
                &mut output,
                &json!({"event":"heartbeat","activityCounter":observed.to_string()}),
                &[],
            )?;
            heartbeat = Instant::now();
        }
        std::thread::sleep(Duration::from_millis(33).saturating_sub(tick.elapsed()));
    }
}

impl Worker {
    fn finish(&mut self, output: &mut impl std::io::Write) -> Result<()> {
        let Some(recording) = self.recording.take() else {
            return Ok(());
        };
        let (finalizer, drops) = match self.capture.recording_detach() {
            Ok(detached) => detached,
            Err(error) => {
                return ipc::write(
                    output,
                    &json!({"event":"reply","id":recording.id,"error":error}),
                    &[],
                );
            }
        };
        let statistics = json!({"status":if drops>0||recording.size_limit{"partial"}else{"complete"},"sourceEpoch":self.epoch,"durationMs":recording.started.elapsed().as_millis(),"encodedFrames":recording.frames,"droppedFrames":drops,"sizeLimitReached":recording.size_limit,"width":recording.size.0,"height":recording.size.1,"hardwareEncoder":self.capture.hardware_encoder()});
        let job = std::thread::Builder::new()
            .name("testrun-mp4-finalize".into())
            .spawn(move || {
                finalizer.finish()?;
                Ok(statistics)
            });
        match job {
            Ok(job) => {
                self.finalizer = Some((recording.id, job));
                Ok(())
            }
            Err(_) => ipc::write(
                output,
                &json!({"event":"reply","id":recording.id,"error":Error::new("RECORDING_FAILED", "Cannot start recording finalization")}),
                &[],
            ),
        }
    }
    fn finalized(&mut self, output: &mut impl std::io::Write, wait: bool) -> Result<()> {
        if !self
            .finalizer
            .as_ref()
            .is_some_and(|(_, job)| wait || job.is_finished())
        {
            return Ok(());
        }
        let (id, job) = self.finalizer.take().unwrap();
        let result = job
            .join()
            .map_err(|_| Error::new("RECORDING_FAILED", "Recording finalizer stopped"))
            .and_then(|result| result);
        let reply = match result {
            Ok(statistics) => json!({"event":"reply","id":id,"result":statistics}),
            Err(error) => json!({"event":"reply","id":id,"error":error}),
        };
        ipc::write(output, &reply, &[])
    }
    fn frame(
        &mut self,
        output: &mut impl std::io::Write,
        index: usize,
        activity: u64,
    ) -> Result<()> {
        let codec = if index == 0 { "jpeg" } else { "h264" };
        let pts = self.started.elapsed().as_nanos();
        self.identity.validate()?;
        let (bytes, width, height) = self.capture.frame_codec(if index == 0 { 1 } else { 2 })?;
        self.identity.validate()?;
        let keyframe = index == 0 || self.capture.keyframe();
        let (sw, sh) = self.capture.source_size();
        let configuration = &mut self.configurations[index];
        if configuration.size != [width, height, sw, sh] {
            configuration.size = [width, height, sw, sh];
            configuration.sequence += 1;
        }
        self.sequences[index] += 1;
        let seq = self.sequences[index];
        if index == 1
            && let Some(recording) = &mut self.recording
        {
            recording.frames += 1;
            recording.size = (width, height);
        }
        if !self.profiles[index] {
            return Ok(());
        }
        let captured_at = time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .map_err(|_| Error::new("INTERNAL", "Cannot format capture time"))?;
        let metadata: Value = json!({"event":"frame","epoch":self.epoch,"frameId":format!("{}:{codec}:{seq}",self.epoch),"seq":seq.to_string(),"ptsNs":pts.to_string(),"durationNs":"33333333","capturedAt":captured_at,"displayId":"main","geometryVersion":format!("{sw}x{sh}"),"configurationSeq":configuration.sequence.to_string(),"width":width,"height":height,"sourceWidth":sw,"sourceHeight":sh,"codec":codec,"keyframe":keyframe,"hardwareEncoder":self.capture.hardware_encoder(),"activityCounter":activity.to_string(),"sourceMode":if self.capture.callbacks(){"native_surface_callbacks"}else{"native_surface_poll"},"privacy":"raw_local"});
        ipc::write(output, &metadata, &bytes)
    }
}
