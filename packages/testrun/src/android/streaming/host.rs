//! One owned capture worker, remote helper and native decoder per Android controller.
use super::{
    Image, Mode,
    decoder::Decoder,
    protocol::{self, Fence, Packet},
};
use crate::error::{Error, Result};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    os::fd::AsRawFd,
    path::{Path, PathBuf},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};
use tokio::sync::oneshot;

const DEX: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/capture.dex"));
enum Work {
    Capture((u32, u32), oneshot::Sender<Result<Option<Image>>>),
    Close(&'static str, oneshot::Sender<Result<()>>),
    RestartAdmission(f64, bool),
}
pub(super) struct Host {
    sender: mpsc::SyncSender<Work>,
    health: Arc<Mutex<Value>>,
    admission: tokio::sync::Mutex<()>,
    failed: AtomicBool,
    closing: Mutex<Option<oneshot::Receiver<Result<()>>>>,
}
impl Host {
    pub(super) fn new(serial: String, mode: Mode) -> Self {
        let (sender, receiver) = mpsc::sync_channel(4);
        let health = Arc::new(Mutex::new(json!({"requested":mode,"state":"starting"})));
        let shared = health.clone();
        std::thread::spawn(move || Worker::new(serial, shared).run(receiver));
        Self {
            sender,
            health,
            admission: tokio::sync::Mutex::new(()),
            failed: AtomicBool::new(false),
            closing: Mutex::new(None),
        }
    }
    pub(super) fn status(&self) -> Value {
        let pending = self.close_progress().unwrap_or(false);
        let mut status = self
            .health
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        if pending {
            status["releasePending"] = json!(true);
            if status["state"] != "cleanup_pending" {
                status["state"] = json!("recovering");
                status["reason"] = json!("worker_release_pending");
            }
        }
        if self.failed.load(Ordering::Acquire) {
            status["state"] = json!("cleanup_pending");
            status["reason"] = json!("worker_channel_or_release_failed");
        }
        status
    }
    pub(super) async fn snapshot(&self, size: (u32, u32)) -> Result<Option<Image>> {
        self.snapshot_with_budget(size, Duration::from_secs(5))
            .await
    }
    async fn snapshot_with_budget(
        &self,
        size: (u32, u32),
        budget: Duration,
    ) -> Result<Option<Image>> {
        let _slot = self.admission.lock().await;
        if self.close_progress()? {
            return Err(pending());
        }
        let (reply, result) = oneshot::channel();
        self.sender
            .try_send(Work::Capture(size, reply))
            .map_err(|_| {
                self.failed.store(true, Ordering::Release);
                cleanup()
            })?;
        match tokio::time::timeout(budget, result).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => {
                self.failed.store(true, Ordering::Release);
                Err(cleanup())
            }
            Err(_) => {
                // The worker still owns its retry and producer. Queue release
                // behind that work and retain its acknowledgement across callers.
                self.queue_close("host_deadline")?;
                Err(deadline())
            }
        }
    }
    fn queue_close(&self, reason: &'static str) -> Result<()> {
        let mut closing = self.closing.lock().unwrap_or_else(|e| e.into_inner());
        if closing.is_some() {
            return Ok(());
        }
        let (reply, result) = oneshot::channel();
        if self.sender.try_send(Work::Close(reason, reply)).is_err() {
            self.failed.store(true, Ordering::Release);
            return Err(cleanup());
        }
        *closing = Some(result);
        Ok(())
    }
    /// Poll owned release without turning a caller deadline into release failure.
    fn close_progress(&self) -> Result<bool> {
        let mut closing = self.closing.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(result) = closing.as_mut() {
            match result.try_recv() {
                Ok(Ok(())) => {
                    closing.take();
                    self.failed.store(false, Ordering::Release);
                }
                Ok(Err(_)) | Err(oneshot::error::TryRecvError::Closed) => {
                    closing.take();
                    self.failed.store(true, Ordering::Release);
                }
                Err(oneshot::error::TryRecvError::Empty) => return Ok(true),
            }
        }
        if self.failed.load(Ordering::Acquire) {
            Err(cleanup())
        } else {
            Ok(false)
        }
    }
    pub(super) async fn invalidate(&self, reason: &'static str) -> Result<()> {
        self.invalidate_with_budget(reason, Duration::from_secs(5))
            .await
    }
    async fn invalidate_with_budget(&self, reason: &'static str, budget: Duration) -> Result<()> {
        self.queue_close(reason)?;
        let until = tokio::time::Instant::now() + budget;
        while self.close_progress()? {
            if tokio::time::Instant::now() >= until {
                return Err(deadline());
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        Ok(())
    }
    pub(super) fn report_restart(&self, ms: f64, admitted: bool) -> Result<()> {
        self.sender
            .try_send(Work::RestartAdmission(ms, admitted))
            .map_err(|_| {
                self.failed.store(true, Ordering::Release);
                cleanup()
            })
    }
}

struct Worker {
    serial: String,
    adb: PathBuf,
    health: Arc<Mutex<Value>>,
    producer: Option<Producer>,
    disabled: bool,
    generation: u64,
    request: u64,
    restarts: u8,
    startup_ms: f64,
    last_error: Option<Error>,
    restart_ms: Option<f64>,
    restart_available: Option<bool>,
    restart_admitted: Option<bool>,
}
impl Worker {
    fn new(serial: String, health: Arc<Mutex<Value>>) -> Self {
        Self {
            serial,
            adb: super::super::Android::default().adb,
            health,
            producer: None,
            disabled: false,
            generation: 0,
            request: 0,
            restarts: 0,
            startup_ms: 0.0,
            last_error: None,
            restart_ms: None,
            restart_available: None,
            restart_admitted: None,
        }
    }
    fn state(&self, state: &str, reason: Option<&str>) {
        *self.health.lock().unwrap_or_else(|e| e.into_inner()) = json!({"requested":"stream","state":state,"reason":reason,"generation":self.generation,"restartAttempts":self.restarts,"lastError":self.last_error,"lastRestartMs":self.restart_ms,"lastRestartDecodedWithinTarget":self.restart_available,"lastRestartAvailableWithinTarget":self.restart_admitted,"restartTargetMs":2000,"captureAgeKnown":false});
    }
    fn run(mut self, receiver: mpsc::Receiver<Work>) {
        loop {
            match receiver.recv_timeout(Duration::from_millis(5)) {
                Ok(Work::Capture(size, reply)) => {
                    if !reply.is_closed() {
                        let result = self.capture(size);
                        let _ = reply.send(result);
                    }
                }
                Ok(Work::Close(reason, reply)) => {
                    self.disabled = true;
                    let result = self.stop();
                    self.state(
                        if result.is_err() {
                            "cleanup_pending"
                        } else if reason == "protected_content" {
                            "blocked"
                        } else {
                            "raw_fallback"
                        },
                        Some(reason),
                    );
                    let _ = reply.send(result);
                }
                Ok(Work::RestartAdmission(ms, admitted)) => {
                    self.restart_ms = Some(ms);
                    self.restart_admitted = Some(admitted);
                    let mut health = self.health.lock().unwrap_or_else(|e| e.into_inner());
                    health["lastRestartMs"] = json!(ms);
                    health["lastRestartAvailableWithinTarget"] = json!(admitted);
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    // Normal teardown calls Close and waits before dropping the source.
                    // If ownership disappears unexpectedly, retain/retry until verified release.
                    let mut delay = Duration::from_secs(1);
                    while self.stop().is_err() {
                        std::thread::sleep(delay);
                        delay = (delay * 2).min(Duration::from_secs(30));
                    }
                    break;
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if let Some(producer) = &mut self.producer
                        && let Err(error) = producer.pump(Duration::from_millis(1))
                    {
                        self.state("recovering", Some(&error.code));
                        // Capture performs one bounded recovery; idle does not restart encoders.
                    }
                }
            }
        }
    }
    fn stop(&mut self) -> Result<()> {
        if let Some(producer) = &mut self.producer {
            producer.stop()?;
        }
        self.producer.take();
        Ok(())
    }
    fn launch(&mut self, size: (u32, u32)) -> Result<()> {
        let started = Instant::now();
        self.generation = self.generation.checked_add(1).ok_or_else(cleanup)?;
        self.state("starting", None);
        self.producer = Some(Producer::launch(
            &self.adb,
            &self.serial,
            size,
            self.generation,
        )?);
        let producer = self.producer.as_mut().unwrap();
        producer.provision()?;
        let deadline = Instant::now() + Duration::from_secs(2);
        while !producer.hello && Instant::now() < deadline {
            producer.pump(Duration::from_millis(20))?;
        }
        if !producer.hello {
            return Err(Error::new(
                "FIRST_FRAME_TIMEOUT",
                "Android video startup timed out",
            ));
        }
        self.startup_ms = started.elapsed().as_secs_f64() * 1000.0;
        self.state("healthy", None);
        Ok(())
    }
    fn capture(&mut self, size: (u32, u32)) -> Result<Option<Image>> {
        let call_started = Instant::now();
        self.startup_ms = 0.0;
        if self.disabled {
            return Ok(None);
        }
        if !valid_size(size) {
            self.disabled = true;
            self.stop()
                .inspect_err(|_| self.state("cleanup_pending", Some("release_failed")))?;
            self.state("raw_fallback", Some("unsupported_geometry"));
            return Ok(None);
        }
        if self.producer.as_ref().is_some_and(|p| p.size != size) {
            self.disabled = true;
            self.stop()
                .inspect_err(|_| self.state("cleanup_pending", Some("release_failed")))?;
            self.state("raw_fallback", Some("geometry_changed"));
            return Ok(None);
        }
        let result = self.acquire(size);
        match result {
            Ok(image) => Ok(image.map(|mut image| {
                image.timings["startupMs"] = json!(self.startup_ms);
                image.timings["totalMs"] = json!(call_started.elapsed().as_secs_f64() * 1000.0);
                image
            })),
            Err(cause) => {
                self.last_error = Some(cause.clone());
                let started = Instant::now();
                self.state("recovering", Some(&cause.code));
                self.stop()
                    .inspect_err(|_| self.state("cleanup_pending", Some("release_failed")))?;
                if self.restarts == 0 {
                    self.restarts = 1;
                    let recovered = self.acquire(size);
                    let elapsed = started.elapsed();
                    self.restart_ms = Some(elapsed.as_secs_f64() * 1000.0);
                    self.restart_available = Some(
                        matches!(&recovered, Ok(Some(_))) && elapsed <= Duration::from_secs(2),
                    );
                    if self.restart_available != Some(true) {
                        self.restart_admitted = Some(false);
                    }
                    if let Ok(Some(mut image)) = recovered {
                        image.timings["restartMs"] = json!(self.restart_ms);
                        image.timings["startupMs"] = json!(self.startup_ms);
                        image.timings["totalMs"] =
                            json!(call_started.elapsed().as_secs_f64() * 1000.0);
                        // A late recovery remains a failure of the 2 s availability target.
                        if self.restart_available == Some(true) {
                            self.state("healthy", None);
                            return Ok(Some(image));
                        }
                    }
                    self.stop()
                        .inspect_err(|_| self.state("cleanup_pending", Some("release_failed")))?;
                }
                self.disabled = true;
                self.state("raw_fallback", Some(&cause.code));
                Ok(None)
            }
        }
    }
    fn acquire(&mut self, size: (u32, u32)) -> Result<Option<Image>> {
        if self.producer.is_none() {
            self.launch(size)?;
        }
        self.request = self
            .request
            .checked_add(1)
            .ok_or_else(|| Error::new("CAPTURE_STALLED", "Capture request limit"))?;
        let producer = self.producer.as_mut().unwrap();
        let request = self.request;
        producer.send(&format!("B {request}\n"))?;
        let started = Instant::now();
        let deadline = started + Duration::from_millis(500);
        while Instant::now() < deadline {
            producer.pump(Duration::from_millis(5))?;
            if let Some(fence) = producer.fence.filter(|f| f.request == request)
                && let Some(decoder) = &producer.decoder
                && let Some(sequence) = decoder.sequence()?
                && producer
                    .frames
                    .get(&sequence)
                    .is_some_and(|packet| fence.admits(producer.generation, packet))
            {
                let (bytes, sequence) = decoder.snapshot()?;
                if !producer
                    .frames
                    .get(&sequence)
                    .is_some_and(|p| fence.admits(producer.generation, p))
                {
                    continue;
                }
                self.state("healthy", None);
                return Ok(Some(Image {
                    bytes,
                    size,
                    timings: json!({"backend":"adb_mediacodec_h264","sourceGeneration":self.generation,"sourceSequence":sequence,"requestBarrier":request,"freshnessBasis":"surface_acquisition","captureAgeKnown":false,"observationMs":started.elapsed().as_secs_f64()*1000.0}),
                }));
            }
        }
        self.state("healthy", Some("source_freshness_unavailable"));
        Ok(None)
    }
}

struct Producer {
    adb: PathBuf,
    child: Child,
    input: Option<ChildStdin>,
    output: ChildStdout,
    buffer: Vec<u8>,
    decoder: Option<Decoder>,
    config: Vec<u8>,
    frames: BTreeMap<u64, Packet>,
    fence: Option<Fence>,
    generation: u64,
    size: (u32, u32),
    nonce: String,
    serial: String,
    heartbeat: Instant,
    hello: bool,
    error: Option<Error>,
}
impl Producer {
    fn launch(adb: &Path, serial: &str, size: (u32, u32), generation: u64) -> Result<Self> {
        super::super::adb::validate_serial(serial)?;
        let nonce = format!("testrun-capture-{}", uuid::Uuid::new_v4().simple());
        let script = script(&nonce, size);
        let mut child = Command::new(adb)
            .args(["-s", serial, "shell", "-T", &script])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| {
                Error::new("PLATFORM_FAILED", "Cannot start owned Android video helper")
            })?;
        let input = child.stdin.take().unwrap();
        let output = child.stdout.take().unwrap();
        // SAFETY: this worker owns stdout; nonblocking reads make stop/heartbeat bounded.
        let mut error = None;
        for fd in [input.as_raw_fd(), output.as_raw_fd()] {
            let configured = unsafe {
                let flags = libc::fcntl(fd, libc::F_GETFL);
                flags >= 0 && libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) >= 0
            };
            if !configured {
                error = Some(Error::new(
                    "PLATFORM_FAILED",
                    "Cannot bound capture transport I/O",
                ));
            }
        }
        Ok(Self {
            adb: adb.to_owned(),
            child,
            input: Some(input),
            output,
            buffer: Vec::new(),
            decoder: None,
            config: Vec::new(),
            frames: BTreeMap::new(),
            fence: None,
            generation,
            size,
            nonce,
            serial: serial.into(),
            heartbeat: Instant::now(),
            hello: false,
            error,
        })
    }
    fn provision(&mut self) -> Result<()> {
        if let Some(error) = &self.error {
            return Err(error.clone());
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut offset = 0;
        while offset < DEX.len() && Instant::now() < deadline {
            match self
                .input
                .as_mut()
                .ok_or_else(cleanup)?
                .write(&DEX[offset..])
            {
                Ok(0) => {
                    return Err(Error::new(
                        "CAPTURE_STALLED",
                        "Capture provisioning channel ended",
                    ));
                }
                Ok(count) => offset += count,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(5))
                }
                Err(_) => return Err(Error::new("CAPTURE_STALLED", "Capture provisioning failed")),
            }
        }
        if offset != DEX.len() {
            return Err(Error::new(
                "CAPTURE_STALLED",
                "Capture provisioning deadline",
            ));
        }
        Ok(())
    }
    fn send(&mut self, bytes: &str) -> Result<()> {
        self.input
            .as_mut()
            .ok_or_else(cleanup)?
            .write_all(bytes.as_bytes())
            .map_err(|_| Error::new("CAPTURE_STALLED", "Android capture control channel ended"))
    }
    fn pump(&mut self, timeout: Duration) -> Result<()> {
        if let Some(error) = &self.error {
            return Err(error.clone());
        }
        let result = self.read(timeout);
        if let Err(error) = &result {
            self.error = Some(error.clone());
        }
        result
    }
    fn read(&mut self, timeout: Duration) -> Result<()> {
        if self.heartbeat.elapsed() >= Duration::from_secs(1) {
            self.send("P\n")?;
            self.heartbeat = Instant::now();
        }
        let mut poll = libc::pollfd {
            fd: self.output.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: valid owned descriptor, one writable pollfd, bounded timeout.
        unsafe {
            libc::poll(&mut poll, 1, timeout.as_millis().min(100) as i32);
        }
        let mut bytes = [0; 65536];
        match self.output.read(&mut bytes) {
            Ok(0) => {
                return Err(Error::new(
                    "CAPTURE_STALLED",
                    "Android capture source ended",
                ));
            }
            Ok(n) => self.buffer.extend_from_slice(&bytes[..n]),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(_) => {
                return Err(Error::new(
                    "CAPTURE_STALLED",
                    "Android capture transport failed",
                ));
            }
        }
        if self.buffer.len() > protocol::MAX + protocol::HEADER + 65536 {
            return Err(Error::new(
                "PLATFORM_PROTOCOL",
                "Android video buffer limit",
            ));
        }
        while let Some(packet) = protocol::packet(&mut self.buffer)? {
            match packet.kind {
                4 => {
                    let hello: Value = serde_json::from_slice(&packet.payload).map_err(|_| {
                        Error::new("PLATFORM_PROTOCOL", "Invalid capture handshake")
                    })?;
                    if hello["nonce"] != self.nonce
                        || hello["version"] != 1
                        || hello["width"] != self.size.0
                        || hello["height"] != self.size.1
                        || hello["sourceFence"] != "surface_acquisition"
                    {
                        return Err(Error::new(
                            "PLATFORM_PROTOCOL",
                            "Capture handshake identity changed",
                        ));
                    }
                    self.hello = true;
                }
                1 => {
                    if self.decoder.is_some() && packet.payload != self.config {
                        return Err(Error::new(
                            "STALE_DISPLAY",
                            "Android video configuration changed",
                        ));
                    }
                    if self.decoder.is_none() {
                        self.decoder = Some(Decoder::new(&packet.payload, self.size)?);
                        self.config = packet.payload;
                    }
                }
                3 => {
                    self.fence = Some(Fence {
                        generation: self.generation,
                        request: packet.request,
                        sequence: packet.sequence,
                    })
                }
                2 => {
                    let decoder = self.decoder.as_mut().ok_or_else(|| {
                        Error::new(
                            "PLATFORM_PROTOCOL",
                            "Capture frame preceded codec configuration",
                        )
                    })?;
                    decoder.append(&packet.payload, packet.sequence)?;
                    self.frames.insert(
                        packet.sequence,
                        Packet {
                            payload: Vec::new(),
                            ..packet
                        },
                    );
                    while self.frames.len() > 16 {
                        self.frames.pop_first();
                    }
                }
                _ => unreachable!(),
            }
        }
        Ok(())
    }
    fn stop(&mut self) -> Result<()> {
        if let Some(mut input) = self.input.take() {
            let _ = input.write_all(b"S\n");
        }
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            let mut bytes = [0; 65536];
            for _ in 0..16 {
                let mut poll = libc::pollfd {
                    fd: self.output.as_raw_fd(),
                    events: libc::POLLIN,
                    revents: 0,
                };
                if unsafe { libc::poll(&mut poll, 1, 0) } <= 0 {
                    break;
                }
                if !matches!(self.output.read(&mut bytes),Ok(n) if n>0) {
                    break;
                }
            }
            if self.child.try_wait().map_err(|_| cleanup())?.is_some() {
                break;
            }
            if Instant::now() >= deadline {
                // A stuck local adb may be reaped only after remote ownership is gone.
                verify_release(&self.adb, &self.serial, &self.nonce)?;
                self.child.kill().map_err(|_| cleanup())?;
                self.child.wait().map_err(|_| cleanup())?;
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        // The remote EXIT trap's absence witness is stronger than a local process exit.
        verify_release(&self.adb, &self.serial, &self.nonce)?;
        self.decoder.take();
        self.frames.clear();
        self.buffer.clear();
        Ok(())
    }
}
fn valid_size(size: (u32, u32)) -> bool {
    (2..=8192).contains(&size.0)
        && (2..=8192).contains(&size.1)
        && size.0.is_multiple_of(2)
        && size.1.is_multiple_of(2)
        && u64::from(size.0) * u64::from(size.1) <= 16_777_216
}
fn cleanup() -> Error {
    Error::new(
        "CLEANUP_FAILED",
        "Android video release is unfinished; retry disconnect or owner cleanup",
    )
}
fn pending() -> Error {
    Error::new(
        "CAPTURE_PENDING",
        "Android video release is still running; retry observation after releasePending clears",
    )
}
fn deadline() -> Error {
    Error::new("CAPTURE_TIMEOUT", "Android video host wait expired; release remains owned. Inspect status and retry observation after releasePending clears")
        .details(json!({"phase":"host_wait"}))
}

fn verify_release(adb: &Path, serial: &str, nonce: &str) -> Result<()> {
    let script = format!(
        r#"umask 077
nonce='{nonce}'
dir=/data/local/tmp/{nonce}
lock=/data/local/tmp/testrun-capture.owner
created=
child=
{release}
if [ -d "$dir" ]; then
  [ "$(cat "$dir/lease" 2>/dev/null)" = "$nonce" ] || [ "$(cat "$dir.released" 2>/dev/null)" = "$nonce" ] || exit 125
  created=1
  child=$(cat "$dir/pid" 2>/dev/null)
  [ -n "$child" ] || [ "$(cat "$dir.released" 2>/dev/null)" = "$nonce" ] || exit 125
fi
cleanup || exit 125
test ! -e "$dir" && test ! -e "$dir.released" && test "$(readlink "$lock")" != "$dir"
"#,
        release = RELEASE
    );
    let mut child = Command::new(adb)
        .args(["-s", serial, "shell", "-T", &script])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| cleanup())?;
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if let Some(status) = child.try_wait().map_err(|_| cleanup())? {
            return if status.success() {
                Ok(())
            } else {
                Err(cleanup())
            };
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(cleanup());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
fn script(nonce: &str, size: (u32, u32)) -> String {
    let digest = format!("{:x}", Sha256::digest(DEX));
    format!(
        r#"umask 077
nonce='{nonce}'
dir=/data/local/tmp/{nonce}
lock=/data/local/tmp/testrun-capture.owner
child=
created=
{release}
trap 'cleanup || exit 125' EXIT
trap 'exit 143' HUP INT TERM
mkdir "$dir" || exit 1
created=1
printf '%s' "$nonce" > "$dir/lease" || exit 1
# Older helpers used a symlink. Admit only a provably dead legacy owner;
# never remove or signal another invocation's resources.
if [ -L "$lock" ]; then
  legacy=$(readlink "$lock")
  case "$legacy" in /data/local/tmp/testrun-capture-*) ;; *) exit 75 ;; esac
  if [ -d "$legacy" ]; then
    legacy_pid=$(cat "$legacy/pid" 2>/dev/null)
    case "$legacy_pid" in ''|*[!0-9]*) exit 75 ;; esac
    if [ -d /proc/$legacy_pid ]; then
      legacy_start=$(cat "$legacy/start" 2>/dev/null)
      current_start=$(awk '{{print $22}}' /proc/$legacy_pid/stat 2>/dev/null)
      [ -n "$legacy_start" ] && [ -n "$current_start" ] && [ "$legacy_start" != "$current_start" ] || exit 75
    fi
  fi
fi
dd bs=1 count={length} of="$dir/capture.dex" 2>/dev/null || exit 1
[ "$(sha256sum "$dir/capture.dex" | cut -d ' ' -f 1)" = '{digest}' ] || exit 1
chmod 400 "$dir/capture.dex" || exit 1
export CLASSPATH="$dir/capture.dex"
exec 3<&0
stopping=
trap 'stopping=1' HUP INT TERM
app_process / --nice-name={nonce} io.canonic.testrun.capture.Main {nonce} {width} {height} <&3 &
exec 3<&-
child=$!
printf '%s' "$child" > "$dir/pid" || exit 1
awk '{{print $22}}' /proc/$child/stat > "$dir/start" 2>/dev/null
trap 'exit 143' HUP INT TERM
[ -z "$stopping" ] || exit 143
wait "$child"
status=$?
child=
exit "$status"
"#,
        length = DEX.len(),
        width = size.0,
        height = size.1,
        release = RELEASE
    )
}

// Durable nonce + process-start evidence permits retry after the original remote
// shell exits. A reused PID never authorizes signaling the replacement process.
const RELEASE: &str = r#"cleanup() {
  case "$child" in ''|*[!0-9]*) [ -z "$child" ] || return 1 ;; esac
  if [ -n "$child" ] && [ -d /proc/$child ] && [ "$(cat "$dir.released" 2>/dev/null)" != "$nonce" ]; then
    expected=$(cat "$dir/start" 2>/dev/null)
    actual=$(awk '{print $22}' /proc/$child/stat 2>/dev/null)
    [ -n "$expected" ] || return 1
    [ -n "$actual" ] || [ ! -d /proc/$child ] || return 1
    if [ "$actual" = "$expected" ]; then
      name=$(tr '\000' '\n' < /proc/$child/cmdline | head -n 1)
      if [ "$name" != "$nonce" ]; then
        tr '\000' '\n' < /proc/$child/cmdline | grep -F -x -- "--nice-name=$nonce" >/dev/null || return 1
      fi
      kill -TERM "$child" 2>/dev/null || [ ! -d /proc/$child ] || return 1
      count=0
      while [ "$(awk '{print $22}' /proc/$child/stat 2>/dev/null)" = "$expected" ]; do
        [ "$(awk '{print $3}' /proc/$child/stat 2>/dev/null)" != Z ] || break
        [ "$count" -lt 20 ] || return 1
        count=$((count+1))
        sleep 0.05
      done
      actual=$(awk '{print $22}' /proc/$child/stat 2>/dev/null)
      [ -n "$actual" ] || [ ! -d /proc/$child ] || return 1
      if [ "$actual" = "$expected" ]; then
        [ "$(awk '{print $3}' /proc/$child/stat 2>/dev/null)" = Z ] || return 1
      fi
      wait "$child" 2>/dev/null || :
    fi
  fi
  if [ -n "$created" ]; then printf '%s' "$nonce" > "$dir.released" || return 1; fi
  if [ -n "$created" ] && [ -d "$dir" ]; then
    rm -f "$dir/capture.dex" "$dir/pid" "$dir/start" "$dir/lease" || return 1
    rmdir "$dir" || [ ! -d "$dir" ] || return 1
  fi
  if [ "$(readlink "$lock")" = "$dir" ]; then rm "$lock" || [ ! -L "$lock" ] || return 1; fi
  if [ "$(cat "$dir.released" 2>/dev/null)" = "$nonce" ]; then rm "$dir.released" || return 1; fi
  return 0
}
"#;

#[cfg(test)]
mod tests;
