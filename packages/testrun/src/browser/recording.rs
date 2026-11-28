//! Independent browser recording jobs with bounded frame admission and final proof.

use super::contract::Reference;
use crate::{
    artifacts,
    error::{Error, Result},
    media::Frame,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::{
    sync::{Mutex as AsyncMutex, oneshot, watch},
    task::JoinHandle,
};

mod native;
#[cfg(any(windows, test))]
mod pixels;

#[cfg(windows)]
pub(super) use native::available;

pub(super) fn private_directory(parent: &Path) -> Result<PathBuf> {
    let directory = parent.join("recordings");
    #[cfg(windows)]
    return canonic_process_kit::endpoint::prepare_directory(&directory)
        .map_err(|_| Error::new("EVIDENCE_IO", "Cannot prepare private recording directory"));
    #[cfg(not(windows))]
    {
        if !directory.exists() {
            artifacts::directory(&directory)?;
        }
        artifacts::check_private_directory(&directory)?;
        Ok(directory)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Options {
    #[serde(default = "duration")]
    pub max_duration_ms: u64,
}
fn duration() -> u64 {
    60000
}
impl Default for Options {
    fn default() -> Self {
        Self {
            max_duration_ms: duration(),
        }
    }
}
impl Options {
    pub(crate) fn validate(&self) -> std::result::Result<(), &'static str> {
        if !(100..=600000).contains(&self.max_duration_ms) {
            return Err("INVALID_RECORDING_DURATION");
        }
        Ok(())
    }
}

pub(crate) struct Job {
    pub worker: Option<JoinHandle<Result<Value>>>,
    pub result: Option<Value>,
}
pub(crate) struct Recording {
    pub reference: Reference,
    pub source: Reference,
    pub page: Reference,
    pub started: Instant,
    pub options: Options,
    pub signal: Mutex<Option<oneshot::Sender<Instant>>>,
    pub job: AsyncMutex<Job>,
    pub terminal: Arc<Mutex<Option<Value>>>,
    pub finalizing: Arc<AtomicBool>,
    pub ready: Arc<AtomicBool>,
}
impl Recording {
    pub fn terminal(&self) -> bool {
        self.terminal
            .lock()
            .expect("Recording terminal result")
            .is_some()
    }
    pub fn stop(&self) {
        self.finalizing.store(true, Ordering::Release);
        if let Some(signal) = self.signal.lock().expect("Browser recorder stop").take() {
            let _ = signal.send(Instant::now());
        }
    }
    pub fn snapshot(&self) -> Value {
        if let Some(result) = self
            .terminal
            .lock()
            .expect("Recording terminal result")
            .as_ref()
        {
            return result.clone();
        }
        if let Ok(job) = self.job.try_lock()
            && let Some(result) = &job.result
        {
            return result.clone();
        }
        json!({"recording":self.reference,"page":self.page,"source":self.source,"status":if self.finalizing.load(Ordering::Acquire)||self.started.elapsed().as_millis()>=u128::from(self.options.max_duration_ms){"finalizing"}else if self.ready.load(Ordering::Acquire){"recording"}else{"initializing"},"elapsedMs":self.started.elapsed().as_millis(),"maxDurationMs":self.options.max_duration_ms,"codec":"h264","container":"mp4","audio":false,"privacy":"local-private"})
    }
}

pub(crate) struct Input {
    pub recording: Reference,
    pub page: Reference,
    pub options: Options,
    pub directory: PathBuf,
    pub frames: watch::Receiver<Option<Arc<Frame>>>,
    pub first: Arc<Frame>,
    pub started: Instant,
    pub interruptions: Arc<AtomicU64>,
    pub initial_interruptions: u64,
    pub ready: Arc<AtomicBool>,
    pub alive: Arc<AtomicBool>,
    pub finalizing: Arc<AtomicBool>,
}

struct Movie {
    writer: native::Writer,
    width: u32,
    height: u32,
    frames: u64,
    dropped: u64,
    discontinuities: u64,
    sequence: Option<u64>,
    epoch: Option<Value>,
    last_frame: Option<Arc<Frame>>,
    last_pts: u64,
    started: Instant,
    initial_age_ms: Option<u128>,
    source_dropped: Option<u64>,
    segments: Vec<Value>,
    omitted_segments: u64,
}

impl Movie {
    fn open(path: &std::path::Path, frame: &Frame, started: Instant) -> Result<Self> {
        let width = frame.metadata["width"]
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .ok_or_else(|| Error::new("INVALID_MEDIA_FRAME", "Missing browser frame width"))?
            .div_ceil(2)
            * 2;
        let height = frame.metadata["height"]
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .ok_or_else(|| Error::new("INVALID_MEDIA_FRAME", "Missing browser frame height"))?
            .div_ceil(2)
            * 2;
        Ok(Self {
            writer: native::Writer::open(path, width, height)?,
            width,
            height,
            frames: 0,
            dropped: 0,
            discontinuities: 0,
            sequence: None,
            epoch: None,
            last_frame: None,
            last_pts: 0,
            started,
            initial_age_ms: Some(
                started
                    .saturating_duration_since(frame.received)
                    .as_millis(),
            ),
            source_dropped: None,
            segments: Vec::new(),
            omitted_segments: 0,
        })
    }
    fn append(&mut self, frame: Arc<Frame>, terminal: Option<Instant>) -> Result<()> {
        let sequence = frame.metadata["seq"]
            .as_str()
            .and_then(|sequence| sequence.parse::<u64>().ok())
            .ok_or_else(|| Error::new("INVALID_MEDIA_FRAME", "Missing browser frame sequence"))?;
        if terminal.is_none() && self.sequence.is_some_and(|previous| sequence <= previous) {
            return Ok(());
        }
        if terminal.is_none() {
            let dropped = frame.metadata["sourceDroppedFrames"].as_u64().unwrap_or(0);
            if let Some(previous) = self.source_dropped {
                self.dropped += dropped.saturating_sub(previous);
            }
            self.source_dropped = Some(dropped);
            if let Some(previous) = self.sequence {
                self.dropped += sequence.saturating_sub(previous + 1);
            }
            let epoch = frame.metadata["sourceEpoch"].clone();
            if self.epoch.as_ref() != Some(&epoch) {
                if self.segments.len() < 64 {
                    self.segments.push(json!({"sourceEpoch":epoch,"firstSequence":sequence,"width":frame.metadata["width"],"height":frame.metadata["height"],"logicalViewport":frame.metadata["logicalViewport"],"dpr":frame.metadata["dpr"],"frame":frame.metadata["frame"],"viewportEpoch":frame.metadata["viewportEpoch"],"contentGeometryVerified":false,"visibilityLimit":frame.metadata["visibilityLimit"]}));
                } else {
                    self.omitted_segments += 1;
                }
            }
            if self
                .epoch
                .as_ref()
                .is_some_and(|previous| *previous != epoch)
            {
                self.discontinuities += 1;
            }
            self.epoch = Some(epoch);
            self.sequence = Some(sequence);
        }
        let header = frame
            .packet
            .get(..24)
            .and_then(|bytes| bytes.try_into().ok())
            .ok_or_else(|| Error::new("INVALID_MEDIA_FRAME", "Truncated browser media packet"))?;
        let (metadata, payload) = crate::media::lengths(&header)?;
        if frame.packet.len() != 24 + metadata + payload {
            return Err(Error::new(
                "INVALID_MEDIA_FRAME",
                "Invalid browser media packet length",
            ));
        }
        let pts = if let Some(stopped) = terminal {
            stopped
                .saturating_duration_since(self.started)
                .as_nanos()
                .min(u128::from(u64::MAX)) as u64
        } else {
            frame
                .received
                .saturating_duration_since(self.started)
                .as_nanos()
                .min(u128::from(u64::MAX)) as u64
        };
        let pts = if self.frames == 0 {
            0
        } else {
            pts.max(self.last_pts + 1)
        };
        if self.writer.append(&frame.packet[24 + metadata..], pts)? {
            self.frames += 1;
            self.last_pts = pts;
            self.last_frame = Some(frame);
        } else {
            self.dropped += 1;
        }
        Ok(())
    }
}

pub(crate) async fn run(input: Input, stop: oneshot::Receiver<Instant>) -> Result<Value> {
    let runtime = tokio::runtime::Handle::current();
    // A recording retains one worker so native apartment-bound writers are
    // opened, appended, finalized and dropped on the same thread.
    tokio::task::spawn_blocking(move || runtime.block_on(record(input, stop)))
        .await
        .map_err(|_| Error::new("RECORDING_FAILED", "Browser recording worker stopped"))?
}

async fn record(mut input: Input, mut stop: oneshot::Receiver<Instant>) -> Result<Value> {
    let temporary = input
        .directory
        .join(format!(".{}.partial.mp4", input.recording.id));
    let first = input.first.clone();
    let started = input.started;
    let mut movie = Movie::open(&temporary, &first, started)?;
    movie.append(first, None)?;
    if movie.frames > 0 {
        input.ready.store(true, Ordering::Release);
    }
    let duration_end = started + Duration::from_millis(input.options.max_duration_ms);
    let deadline = tokio::time::sleep_until(tokio::time::Instant::from_std(duration_end));
    tokio::pin!(deadline);
    let mut reason = None;
    let mut failure = None;
    let stopped = loop {
        tokio::select! {biased;
            stopped=&mut stop=>break stopped.unwrap_or_else(|_|Instant::now()),
            _=&mut deadline=>break duration_end,
            changed=input.frames.changed()=>{
                if changed.is_err() {reason=Some("SOURCE_LOST");break Instant::now();}
                let Some(frame)=input.frames.borrow_and_update().clone() else {
                    if !input.alive.load(Ordering::Acquire) {reason=Some("SOURCE_LOST");break Instant::now();}
                    movie.discontinuities+=1;
                    continue;
                };
                let result=movie.append(frame,None);
                if movie.frames>0 {input.ready.store(true,Ordering::Release);}
                if let Err(error)=result {reason=Some("ENCODER_FRAME_FAILED");failure=Some(error);break Instant::now();}
                if temporary.metadata().is_ok_and(|metadata|metadata.len()>=256*1024*1024) {reason=Some("SIZE_LIMIT");break Instant::now();}
            }
        }
    };
    if reason.is_none() && !input.alive.load(Ordering::Acquire) {
        reason = Some("SOURCE_LOST");
    }
    finalize(movie, input, stopped, reason, failure)
}

fn finalize(
    mut movie: Movie,
    mut input: Input,
    stopped: Instant,
    reason: Option<&str>,
    failure: Option<Error>,
) -> Result<Value> {
    let source_interruptions = input
        .interruptions
        .load(Ordering::SeqCst)
        .saturating_sub(input.initial_interruptions);
    let latest = input
        .frames
        .borrow_and_update()
        .clone()
        .filter(|frame| frame.received <= stopped);
    input.finalizing.store(true, Ordering::Release);
    let recording = input.recording;
    let page = input.page;
    let directory = input.directory;
    let temporary = directory.join(format!(".{}.partial.mp4", recording.id));
    let final_path = directory.join(format!("{}.mp4", recording.id));
    if let Some(latest) = latest.filter(|_| failure.is_none()) {
        movie.append(latest, None)?;
    }
    let before_terminal = movie.frames;
    if let Some(last) = movie.last_frame.clone() {
        movie.append(last, Some(stopped))?;
    }
    let terminal_duplicate = movie.frames > before_terminal;
    let frames = movie.frames;
    let width = movie.width;
    let height = movie.height;
    let end = movie.last_pts.saturating_add(1_000_000);
    let elapsed = stopped.saturating_duration_since(movie.started).as_millis();
    let dropped = movie.dropped;
    let discontinuities = movie.discontinuities.max(source_interruptions);
    let initial_age = movie.initial_age_ms;
    let segments = movie.segments;
    let omitted_segments = movie.omitted_segments;
    movie.writer.finish(end)?;
    let verification = native::verify(&temporary, width, height, frames)?;
    if verification["decodedFrames"] != frames || frames == 0 {
        return Err(Error::new(
            "INVALID_ARTIFACT",
            "Decoded browser recording frames do not match admitted encoder frames",
        )
        .details(
            json!({"encodedFrames":frames,"verification":verification,"temporaryPath":temporary}),
        ));
    }
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(cfg!(windows))
        .open(&temporary)
        .map_err(|_| Error::new("EVIDENCE_IO", "Cannot open finalized browser movie"))?;
    artifacts::sync(&file)
        .map_err(|_| Error::new("EVIDENCE_IO", "Cannot flush finalized browser movie"))?;
    drop(file);
    let artifact = artifacts::register(&temporary, &final_path, "video/mp4")?;
    let result = json!({"recording":recording,"page":page,"status":if reason.is_some()||dropped>0||discontinuities>0 {"partial"}else{"complete"},"artifact":artifact,"verification":verification,"manifest":{"path":directory.join(format!("{}.json",recording.id)),"mimeType":"application/json"},
            "codec":"h264","container":"mp4","audio":false,"masks":[],"privacy":"local-private","encoding":"testrun-native-h264-from-jpeg","geometry":{"width":width,"height":height,"fit":"aspect-fit-letterbox"},"segments":segments,"omittedSegments":omitted_segments,
            "statistics":{"encodedFrames":frames,"droppedFrames":dropped,"discontinuities":discontinuities,"sourceInterruptions":source_interruptions,"durationMs":elapsed,"initialFrameAgeMs":initial_age,"reason":reason,"failure":failure,"timingDomain":"host-arrival","terminalDuplicateFrame":terminal_duplicate}});
    let persist =
        || -> Result<()> {
            artifacts::write(
                &directory.join(format!("{}.json", recording.id)),
                &serde_json::to_vec_pretty(&result).map_err(|_| {
                    Error::new("EVIDENCE_IO", "Cannot encode browser recording manifest")
                })?,
            )?;
            #[cfg(unix)]
            artifacts::sync(&std::fs::File::open(&directory).map_err(|_| {
                Error::new("EVIDENCE_IO", "Cannot open browser recording directory")
            })?)
            .map_err(|_| Error::new("EVIDENCE_IO", "Cannot flush browser recording directory"))?;
            Ok(())
        };
    persist().map_err(|error| {
        error.details(
            json!({"publishedArtifact":result["artifact"],"verification":result["verification"]}),
        )
    })?;
    Ok(result)
}

pub(crate) fn schemas(reference: &Value) -> Vec<Value> {
    let object = |properties, required: &[&str]| json!({"type":"object","properties":properties,"required":required,"additionalProperties":false});
    let options = object(
        json!({"maxDurationMs":{"type":"integer","minimum":100,"maximum":600000,"default":60000}}),
        &[],
    );
    let mut result = vec![object(
        json!({"operation":{"const":"recording_start"},"page":reference,"options":options}),
        &["operation", "page"],
    )];
    for operation in ["recording_status", "recording_stop"] {
        result.push(object(
            json!({"operation":{"const":operation},"recording":reference}),
            &["operation", "recording"],
        ));
    }
    result
}

#[cfg(all(test, any(windows, target_os = "macos")))]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn reference() -> Reference {
        Reference {
            connection_id: Uuid::new_v4(),
            id: Uuid::new_v4(),
            generation: Uuid::new_v4(),
        }
    }
    fn frame() -> Arc<Frame> {
        let image = image::RgbImage::from_fn(320, 240, |x, y| {
            image::Rgb(if x < 160 {
                if y < 120 { [255, 0, 0] } else { [0, 255, 0] }
            } else if y < 120 {
                [0, 0, 255]
            } else {
                [255, 255, 0]
            })
        });
        let mut jpeg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 90)
            .encode_image(&image)
            .unwrap();
        let metadata = json!({"seq":"1","sourceEpoch":Uuid::new_v4(),"width":320,"height":240,"sourceDroppedFrames":0});
        Arc::new(Frame {
            packet: crate::media::encode(&metadata, &jpeg).unwrap(),
            metadata,
            configuration: None,
            received: Instant::now(),
        })
    }

    #[tokio::test]
    async fn admission_frame_and_source_interruptions_survive_a_stop_before_encoder_start() {
        let directory = tempfile::tempdir().unwrap();
        let path = private_directory(directory.path()).unwrap();
        let (sender, frames) = watch::channel(Some(frame()));
        let first = frames.borrow().clone().unwrap();
        let started = Instant::now();
        let interruptions = Arc::new(AtomicU64::new(0));
        let input = Input {
            recording: reference(),
            page: reference(),
            options: Options::default(),
            directory: path,
            frames,
            first,
            started,
            interruptions: interruptions.clone(),
            initial_interruptions: 0,
            ready: Arc::new(AtomicBool::new(false)),
            alive: Arc::new(AtomicBool::new(true)),
            finalizing: Arc::new(AtomicBool::new(false)),
        };
        sender.send_replace(None);
        interruptions.fetch_add(1, Ordering::SeqCst);
        let (stop, stopped) = oneshot::channel();
        let stop_time = Instant::now();
        stop.send(stop_time).unwrap();
        let result = run(input, stopped).await.unwrap();
        assert_eq!(result["status"], "partial", "{result}");
        assert_eq!(result["statistics"]["sourceInterruptions"], 1);
        assert_eq!(
            result["statistics"]["durationMs"].as_u64(),
            Some(stop_time.duration_since(started).as_millis() as u64)
        );
        assert_eq!(result["verification"]["decodable"], true);
        assert_eq!(
            result["verification"]["decodedFrames"],
            result["statistics"]["encodedFrames"]
        );
    }

    #[tokio::test]
    async fn publication_failure_preserves_the_verified_movie_and_existing_manifest() {
        let directory = tempfile::tempdir().unwrap();
        let path = private_directory(directory.path()).unwrap();
        let recording = reference();
        let manifest = path.join(format!("{}.json", recording.id));
        artifacts::write(&manifest, b"Existing evidence").unwrap();
        let (_sender, frames) = watch::channel(Some(frame()));
        let first = frames.borrow().clone().unwrap();
        let (_stop, stopped) = oneshot::channel();
        let error = run(
            Input {
                recording,
                page: reference(),
                options: Options {
                    max_duration_ms: 100,
                },
                directory: path,
                first,
                started: Instant::now(),
                interruptions: Arc::new(AtomicU64::new(0)),
                initial_interruptions: 0,
                ready: Arc::new(AtomicBool::new(false)),
                frames,
                alive: Arc::new(AtomicBool::new(true)),
                finalizing: Arc::new(AtomicBool::new(false)),
            },
            stopped,
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, "EVIDENCE_IO");
        let proof = error.details.unwrap();
        assert_eq!(proof["verification"]["decodable"], true, "{proof}");
        assert!(
            std::path::Path::new(proof["publishedArtifact"]["path"].as_str().unwrap()).is_file()
        );
        assert_eq!(std::fs::read(manifest).unwrap(), b"Existing evidence");
    }

    #[tokio::test]
    async fn native_recording_decodes_the_entire_static_track_and_preserves_a_partial_on_source_loss()
     {
        for lost in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let path = private_directory(directory.path()).unwrap();
            let recording = reference();
            let alive = Arc::new(AtomicBool::new(true));
            let (sender, frames) = watch::channel(Some(frame()));
            let first = frames.borrow().clone().unwrap();
            let (_stop, stopped) = oneshot::channel();
            let worker = tokio::spawn(run(
                Input {
                    recording,
                    page: reference(),
                    options: Options {
                        max_duration_ms: 100,
                    },
                    directory: path.clone(),
                    first,
                    started: Instant::now(),
                    interruptions: Arc::new(AtomicU64::new(0)),
                    initial_interruptions: 0,
                    ready: Arc::new(AtomicBool::new(false)),
                    frames,
                    alive: alive.clone(),
                    finalizing: Arc::new(AtomicBool::new(false)),
                },
                stopped,
            ));
            if lost {
                tokio::time::sleep(Duration::from_millis(30)).await;
                alive.store(false, Ordering::Release);
                sender.send_replace(None);
            }
            let result = worker.await.unwrap().unwrap();
            assert_eq!(result["verification"]["decodable"], true, "{result}");
            assert_eq!(
                result["verification"]["decodedFrames"],
                result["statistics"]["encodedFrames"]
            );
            assert!(result["statistics"]["encodedFrames"].as_u64().unwrap() > 0);
            assert_eq!(result["artifact"]["mimeType"], "video/mp4");
            #[cfg(target_os = "macos")]
            let colors = native::corners(std::path::Path::new(
                result["artifact"]["path"].as_str().unwrap(),
            ));
            #[cfg(target_os = "macos")]
            for (pixel, expected) in colors.as_chunks::<3>().0.iter().zip([
                [255u8, 0, 0],
                [0, 0, 255],
                [0, 255, 0],
                [255, 255, 0],
            ]) {
                assert!(
                    pixel
                        .iter()
                        .zip(expected)
                        .all(|(actual, expected)| actual.abs_diff(expected) < 50),
                    "Decoded recording quadrant {pixel:?} differs from {expected:?}"
                );
            }
            assert!(path.join(format!("{}.json", recording.id)).is_file());
            if lost {
                assert_eq!(result["status"], "partial");
                assert_eq!(result["statistics"]["reason"], "SOURCE_LOST");
            } else {
                assert_eq!(result["status"], "complete", "{result}");
                assert!(result["statistics"]["durationMs"].as_u64().unwrap() >= 100);
            }
        }
    }
}
