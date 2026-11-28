//! Bounded observation decoding on an owned native worker, independent of media reception.
use super::{apple, source::VideoFrame};
use crate::error::{Error, Result};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc,
    },
    time::Duration,
};
use tokio::sync::oneshot;

trait Decode: Send + 'static {
    fn append(&mut self, frame: &VideoFrame) -> Result<()>;
    fn latest_pts(&self) -> Option<u64>;
    fn snapshot(&self) -> Result<Option<apple::Snapshot>>;
    fn reset(&mut self);
}
struct Native {
    decoder: Option<apple::Decoder>,
    configuration: u64,
    geometry: Result<(u32, u32)>,
}
impl Decode for Native {
    fn append(&mut self, frame: &VideoFrame) -> Result<()> {
        if self.configuration != frame.configuration_sequence {
            self.reset();
            self.configuration = frame.configuration_sequence;
        }
        if self.decoder.is_none() && frame.key {
            self.decoder = Some(apple::Decoder::new(&frame.format, self.geometry.clone()?)?);
        }
        if let Some(decoder) = &mut self.decoder {
            decoder.append(&frame.format, &frame.payload, frame.pts)?;
        }
        Ok(())
    }
    fn latest_pts(&self) -> Option<u64> {
        self.decoder.as_ref().and_then(apple::Decoder::latest_pts)
    }
    fn snapshot(&self) -> Result<Option<apple::Snapshot>> {
        self.decoder
            .as_ref()
            .map(apple::Decoder::snapshot)
            .transpose()
            .map(Option::flatten)
    }
    fn reset(&mut self) {
        self.decoder = None;
    }
}
// Decoder construction/JPEG encoding can take hundreds of milliseconds while
// reception continues. Preserve those predictive frames within both limits.
const QUEUED_FRAMES: usize = 64;
const QUEUED_BYTES: usize = 8 * 1024 * 1024;
struct QueuedFrame {
    frame: Arc<VideoFrame>,
    budget: Arc<AtomicUsize>,
    bytes: usize,
}
impl Drop for QueuedFrame {
    fn drop(&mut self) {
        self.budget.fetch_sub(self.bytes, Ordering::AcqRel);
    }
}
enum Command {
    Frame(QueuedFrame),
    Snapshot(oneshot::Sender<Result<Option<apple::Snapshot>>>),
}
#[derive(Default)]
struct State {
    pts: Option<u64>,
    error: Option<Error>,
}
struct Worker {
    job: Option<std::thread::JoinHandle<()>>,
    result: Option<Result<()>>,
}
pub(super) struct Observation {
    sender: mpsc::SyncSender<Command>,
    enabled: AtomicBool,
    stopped: Arc<AtomicBool>,
    reset: Arc<AtomicBool>,
    queued_bytes: Arc<AtomicUsize>,
    state: Arc<Mutex<State>>,
    worker: Mutex<Worker>,
}
impl Observation {
    pub(super) fn start(
        geometry: Result<(u32, u32)>,
        requests: tokio::sync::mpsc::Sender<()>,
    ) -> Result<Arc<Self>> {
        Self::spawn(
            Native {
                decoder: None,
                configuration: 0,
                geometry,
            },
            requests,
        )
    }
    #[cfg(test)]
    pub(super) fn with_decoder(
        decoder: apple::Decoder,
        geometry: (u32, u32),
        requests: tokio::sync::mpsc::Sender<()>,
    ) -> Result<Arc<Self>> {
        Self::spawn(
            Native {
                decoder: Some(decoder),
                configuration: 1,
                geometry: Ok(geometry),
            },
            requests,
        )
    }
    fn spawn(driver: impl Decode, requests: tokio::sync::mpsc::Sender<()>) -> Result<Arc<Self>> {
        let (sender, receiver) = mpsc::sync_channel(QUEUED_FRAMES);
        let state = Arc::new(Mutex::new(State::default()));
        let stopped = Arc::new(AtomicBool::new(false));
        let reset = Arc::new(AtomicBool::new(false));
        let (worker_state, worker_stop, worker_reset) =
            (state.clone(), stopped.clone(), reset.clone());
        let worker = std::thread::Builder::new()
            .name("testrun-observation".into())
            .spawn(move || {
                run(
                    driver,
                    receiver,
                    worker_state,
                    worker_stop,
                    worker_reset,
                    requests,
                )
            })
            .map_err(|_| Error::new("CAPTURE_STALLED", "Cannot start observation worker"))?;
        Ok(Arc::new(Self {
            sender,
            enabled: AtomicBool::new(false),
            stopped,
            reset,
            queued_bytes: Arc::new(AtomicUsize::new(0)),
            state,
            worker: Mutex::new(Worker {
                job: Some(worker),
                result: None,
            }),
        }))
    }
    pub(super) fn enabled(&self) -> bool {
        self.enabled.load(Ordering::Acquire)
    }
    pub(super) fn enable(&self) {
        self.enabled.store(true, Ordering::Release);
    }
    pub(super) fn health(&self) -> (Option<u64>, Option<Error>) {
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let pts = if self.reset.load(Ordering::Acquire) || self.stopped.load(Ordering::Acquire) {
            None
        } else {
            state.pts
        };
        (pts, state.error.clone())
    }
    /// Never waits for native decoding or queue capacity on the receiver task.
    pub(super) fn submit(&self, frame: Arc<VideoFrame>) {
        if !self.enabled() || self.stopped.load(Ordering::Acquire) {
            return;
        }
        let bytes = frame.payload.len()
            + frame.media.packet.len()
            + frame.media.configuration.as_ref().map_or(0, Vec::len);
        let reserved = self
            .queued_bytes
            .try_update(Ordering::AcqRel, Ordering::Acquire, |used| {
                used.checked_add(bytes)
                    .filter(|total| *total <= QUEUED_BYTES)
            })
            .is_ok();
        let admitted = reserved
            && self
                .sender
                .try_send(Command::Frame(QueuedFrame {
                    frame,
                    budget: self.queued_bytes.clone(),
                    bytes,
                }))
                .is_ok();
        if !admitted {
            self.reset.store(true, Ordering::Release);
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            state.pts = None;
            state.error = Some(Error::new(
                "CAPTURE_STALLED",
                "Observation queue overrun; awaiting a new keyframe",
            ));
        }
    }
    pub(super) async fn snapshot(&self) -> Result<Option<apple::Snapshot>> {
        let (reply, response) = oneshot::channel();
        match self.sender.try_send(Command::Snapshot(reply)) {
            Ok(()) => {}
            Err(mpsc::TrySendError::Full(_)) => return Ok(None),
            Err(mpsc::TrySendError::Disconnected(_)) => {
                return Err(Error::new("CAPTURE_STALLED", "Observation worker ended"));
            }
        }
        let image = response
            .await
            .map_err(|_| Error::new("CAPTURE_STALLED", "Observation request ended"))??;
        if self.reset.load(Ordering::Acquire) || self.stopped.load(Ordering::Acquire) {
            return Ok(None);
        }
        Ok(image)
    }
    pub(super) fn request_stop(&self) {
        self.stopped.store(true, Ordering::Release);
    }
    pub(super) async fn close(&self) -> Result<()> {
        self.request_stop();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        loop {
            {
                let mut worker = self.worker.lock().unwrap_or_else(|e| e.into_inner());
                if let Some(result) = &worker.result {
                    return result.clone();
                }
                if worker
                    .job
                    .as_ref()
                    .is_none_or(std::thread::JoinHandle::is_finished)
                {
                    let result = worker.job.take().map_or(Ok(()), |worker| {
                        worker
                            .join()
                            .map_err(|_| Error::new("CLEANUP_FAILED", "Observation worker failed"))
                    });
                    worker.result = Some(result.clone());
                    return result;
                }
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(Error::new(
                    "CLEANUP_FAILED",
                    "Observation worker is still releasing native resources",
                ));
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
}
impl Drop for Observation {
    fn drop(&mut self) {
        self.request_stop();
    }
}
fn run(
    mut driver: impl Decode,
    receiver: mpsc::Receiver<Command>,
    state: Arc<Mutex<State>>,
    stopped: Arc<AtomicBool>,
    reset: Arc<AtomicBool>,
    requests: tokio::sync::mpsc::Sender<()>,
) {
    while !stopped.load(Ordering::Acquire) {
        if reset.swap(false, Ordering::AcqRel) {
            driver.reset();
            // Frames queued before a dropped predictive frame cannot repair it.
            while let Ok(command) = receiver.try_recv() {
                if let Command::Snapshot(reply) = command {
                    let _ = reply.send(Ok(None));
                }
            }
            let _ = requests.try_send(());
        }
        match receiver.recv_timeout(Duration::from_millis(10)) {
            Ok(Command::Frame(frame)) => {
                if let Err(error) = driver.append(&frame.frame) {
                    driver.reset();
                    state.lock().unwrap_or_else(|e| e.into_inner()).error = Some(error);
                    let _ = requests.try_send(());
                }
            }
            Ok(Command::Snapshot(reply)) => {
                if !reply.is_closed() {
                    let _ = reply.send(driver.snapshot());
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        let pts = driver.latest_pts();
        let mut state = state.lock().unwrap_or_else(|e| e.into_inner());
        state.pts = if reset.load(Ordering::Acquire) {
            None
        } else {
            pts
        };
        if state.pts.is_some() {
            state.error = None;
        }
    }
}

#[cfg(test)]
mod tests;
