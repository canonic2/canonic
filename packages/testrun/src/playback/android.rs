//! One persistent shell helper per controller, with cancellation-independent device cleanup.
use crate::{
    error::{Error, Result},
    process,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    path::PathBuf,
    process::Stdio,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, ChildStdout, Command},
    sync::{mpsc, oneshot, watch},
};

pub(crate) const DEX: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/classes.dex"));
type Completion = Option<Result<()>>;
enum Message {
    Call(Value, oneshot::Sender<Result<Value>>),
    Close(oneshot::Sender<Result<()>>),
}
pub(super) struct Helper {
    commands: mpsc::Sender<Message>,
    ready: watch::Receiver<Completion>,
    closed: watch::Receiver<Completion>,
    closing: AtomicBool,
}
impl Helper {
    pub(super) fn start(serial: &str) -> Arc<Self> {
        let (commands, mut requests) = mpsc::channel(8);
        let (ready_tx, ready) = watch::channel(None);
        let (closed_tx, closed) = watch::channel(None);
        let helper = Arc::new(Self {
            commands,
            ready,
            closed,
            closing: AtomicBool::new(false),
        });
        let serial = serial.to_owned();
        tokio::spawn(async move {
            let mut remote = Remote::new(serial);
            let started = tokio::time::timeout(Duration::from_secs(10), remote.start())
                .await
                .unwrap_or_else(|_| Err(failed("startup deadline exceeded")))
                .map_err(|error| {
                    if matches!(
                        error.code.as_str(),
                        "CAPABILITY_UNAVAILABLE" | "HOST_ACCESS_REQUIRED"
                    ) {
                        error
                    } else {
                        failed("startup did not complete").details(
                            json!({"phase":"startup","cause":error,"stderr":remote.diagnostic()}),
                        )
                    }
                });
            ready_tx.send_replace(Some(started.clone()));
            let mut reply = if started.is_ok() {
                remote.serve(&mut requests).await
            } else {
                None
            };
            loop {
                let cleanup = remote.cleanup().await;
                closed_tx.send_replace(Some(cleanup.clone()));
                if let Some(reply) = reply.take() {
                    let _ = reply.send(cleanup.clone());
                }
                if cleanup.is_ok() {
                    break;
                }
                // Keep the exact nonce, serial and unfinished remote/filesystem
                // ownership. No helper request is replayed during a retry.
                loop {
                    match requests.recv().await {
                        Some(Message::Close(next)) => {
                            reply = Some(next);
                            break;
                        }
                        Some(Message::Call(_, response)) => {
                            let _ = response.send(Err(failed("helper is closing")));
                        }
                        None => return,
                    }
                }
            }
        });
        helper
    }
    pub(super) async fn ready(&self) -> Result<()> {
        if self.closing.load(Ordering::SeqCst) {
            return Err(failed("helper is closing"));
        }
        wait(self.ready.clone()).await
    }
    /// The helper names a media session `sessionRef` and a volume source
    /// `volumeTarget`; callers use one `destination` with `kind` and `id`.
    /// Its operations keep their own names: `media_state` for `media_status`
    /// and `media_control` for `media_send`.
    pub(super) async fn call(&self, name: &str, args: &Value, prepare: bool) -> Result<Value> {
        let destination = &args["destination"];
        let mut request = args.clone();
        if let Some(fields) = request.as_object_mut() {
            fields.remove("destination");
        }
        let name = match name {
            "media_status" => "media_state",
            "media_send" => "media_control",
            name => name,
        };
        if name == "media_control" {
            request["sessionRef"] = destination["id"].clone();
        } else if destination.is_object() {
            request["volumeTarget"] = json!({"type":destination["kind"]});
            if destination["id"].is_string() {
                request["volumeTarget"]["sessionRef"] = destination["id"].clone();
            }
        }
        let mut result = self.enqueue(name, &request, prepare, None).await?;
        if let Some(sessions) = result["sessions"].as_array_mut() {
            for session in sessions {
                if let Some(fields) = session.as_object_mut()
                    && let Some(id) = fields.remove("sessionRef")
                {
                    fields.insert("id".into(), id);
                }
            }
        }
        if destination.is_object() {
            if let Some(fields) = result.as_object_mut() {
                fields.remove("volumeTarget");
            }
            if result.get("dispatch").is_some() {
                result["dispatch"]["destination"] = destination.clone();
            } else {
                result["destination"] = destination.clone();
            }
        }
        Ok(result)
    }
    pub(super) async fn probe(
        &self,
        args: &Value,
        deadline: tokio::time::Instant,
    ) -> Result<Value> {
        self.enqueue("audio_probe", args, false, Some(deadline))
            .await
    }
    async fn enqueue(
        &self,
        name: &str,
        args: &Value,
        prepare: bool,
        deadline: Option<tokio::time::Instant>,
    ) -> Result<Value> {
        self.ready().await?;
        let (reply, receiver) = oneshot::channel();
        let mut request = json!({"version":1,"id":uuid::Uuid::new_v4().to_string(),"operation":name,"arguments":args,"prepare":prepare});
        if let Some(deadline) = deadline {
            request["deadlineUnixMs"] = json!(
                (std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    + deadline.saturating_duration_since(tokio::time::Instant::now()))
                .as_millis() as u64
            );
        }
        self.commands
            .send(Message::Call(request, reply))
            .await
            .map_err(|_| failed("helper exited"))?;
        receiver
            .await
            .map_err(|_| failed("helper reply was interrupted"))?
    }
    pub(super) async fn close(&self) -> Result<()> {
        self.closing.store(true, Ordering::SeqCst);
        if self.closed.borrow().as_ref().is_some_and(Result::is_ok) {
            return Ok(());
        }
        let (reply, result) = oneshot::channel();
        if self.commands.send(Message::Close(reply)).await.is_err() {
            return wait(self.closed.clone()).await;
        }
        result.await.unwrap_or_else(|_| {
            self.closed
                .borrow()
                .clone()
                .unwrap_or_else(|| Err(failed("cleanup reply lost")))
        })
    }
}
async fn wait(mut state: watch::Receiver<Completion>) -> Result<()> {
    loop {
        if let Some(result) = state.borrow().clone() {
            return result;
        }
        state
            .changed()
            .await
            .map_err(|_| failed("helper lifecycle ended without a result"))?;
    }
}
pub(super) fn cleanup_command(directory: &str, name: &str) -> String {
    format!(
        "if [ ! -d {0} ]; then exit 0; fi; [ \"$(cat {0}/owner)\" = '{1}' ] || exit 1; if [ -f {0}/pid ]; then pid=$(cat {0}/pid); case \"$pid\" in ''|*[!0-9]*) exit 1;; esac; if [ -d /proc/$pid ]; then name=$(tr '\\000' '\\n' < /proc/$pid/cmdline | head -n 1); [ \"$name\" = '{1}' ] || exit 1; kill -TERM \"$pid\" || exit 1; i=0; while [ -d /proc/$pid ] && [ $i -lt 10 ]; do sleep 0.1; i=$((i+1)); done; [ ! -d /proc/$pid ] || exit 1; fi; fi; rm -rf {0}",
        directory, name
    )
}
fn failed(message: &str) -> Error {
    Error::new(
        "MEDIA_HELPER_FAILED",
        format!("Android media helper: {message}"),
    )
}
struct Remote {
    adb: PathBuf,
    serial: String,
    directory: String,
    name: String,
    owned: bool,
    local: Option<PathBuf>,
    child: Option<Child>,
    input: Option<ChildStdin>,
    output: Option<BufReader<ChildStdout>>,
    diagnostics: Arc<Mutex<Vec<u8>>>,
}
impl Remote {
    fn new(serial: String) -> Self {
        let name = format!("testrun-media-{}", uuid::Uuid::new_v4().simple());
        Self {
            adb: crate::android::Android::default().adb,
            serial,
            directory: format!("/data/local/tmp/{name}"),
            name,
            owned: false,
            local: None,
            child: None,
            input: None,
            output: None,
            diagnostics: Arc::new(Mutex::new(Vec::new())),
        }
    }
    async fn shell(&self, command: &str) -> Result<Vec<u8>> {
        Ok(process::run_diagnostic(
            &self.adb,
            &["-s", &self.serial, "shell", command],
            Duration::from_secs(3),
            16384,
        )
        .await?
        .bytes)
    }
    async fn start(&mut self) -> Result<()> {
        // Track allocation before awaiting adb: cancellation can lose a successful mkdir reply.
        // Cleanup requires the nonce marker, so a pre-existing directory is never adopted.
        self.owned = true;
        self.shell(&format!(
            "umask 077; mkdir {0} && printf '%s' '{1}' > {0}/owner",
            self.directory, self.name
        ))
        .await?;
        let local = std::env::temp_dir().join(&self.name);
        crate::artifacts::directory(&local)?;
        self.local = Some(local.clone());
        let artifact = local.join("media-helper.dex");
        crate::artifacts::write(&artifact, DEX)?;
        process::run_diagnostic(
            &self.adb,
            &[
                "-s",
                &self.serial,
                "push",
                artifact
                    .to_str()
                    .ok_or_else(|| failed("invalid artifact path"))?,
                &format!("{}/media-helper.jar", self.directory),
            ],
            Duration::from_secs(3),
            16384,
        )
        .await?;
        let expected = format!("{:x}", Sha256::digest(DEX));
        let digest = self
            .shell(&format!("sha256sum {}/media-helper.jar", self.directory))
            .await?;
        if String::from_utf8_lossy(&digest).split_whitespace().next() != Some(expected.as_str()) {
            return Err(failed(
                "pushed helper digest does not match the embedded artifact",
            ));
        }
        // Generated identities contain only fixed prefixes and UUID hex, never caller shell text.
        let script = format!(
            "umask 077; echo $$ > {0}/pid; export CLASSPATH={0}/media-helper.jar; exec app_process / --nice-name={1} io.canonic.testrun.media.Main",
            self.directory, self.name
        );
        let mut child = Command::new(&self.adb)
            .args(["-s", &self.serial, "shell", "-T", &script])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|_| failed("could not launch app_process"))?;
        self.input = child.stdin.take();
        self.output = child.stdout.take().map(BufReader::new);
        if let Some(mut errors) = child.stderr.take() {
            let diagnostics = self.diagnostics.clone();
            tokio::spawn(async move {
                // Keep a bounded cause for startup/protocol failures and keep draining.
                let mut buffer = [0; 1024];
                while let Ok(n) = errors.read(&mut buffer).await {
                    if n == 0 {
                        break;
                    }
                    let mut kept = diagnostics.lock().unwrap_or_else(|e| e.into_inner());
                    let count = n.min(4096_usize.saturating_sub(kept.len()));
                    kept.extend_from_slice(&buffer[..count]);
                }
            });
        }
        self.child = Some(child);
        let hello = self.read().await?;
        if hello["version"] == 1 && hello["ready"] == false && hello["code"] == "MEDIA_UNSUPPORTED"
        {
            return Err(Error::new(
                "CAPABILITY_UNAVAILABLE",
                hello["error"]
                    .as_str()
                    .unwrap_or("Android media prerequisites are unavailable"),
            ));
        }
        if hello["version"] != 1
            || hello["ready"] != true
            || hello["uid"] != 2000
            || hello["pid"].as_u64().is_none()
        {
            return Err(
                failed("version, shell identity or service initialization did not pass")
                    .details(json!({"hello":hello})),
            );
        }
        let pid = self.shell(&format!("cat {}/pid", self.directory)).await?;
        if String::from_utf8_lossy(&pid).trim().parse::<u64>().ok() != hello["pid"].as_u64() {
            return Err(failed("remote process identity mismatch"));
        }
        Ok(())
    }
    async fn read(&mut self) -> Result<Value> {
        let output = self
            .output
            .as_mut()
            .ok_or_else(|| failed("missing protocol pipe"))?;
        let mut bytes = Vec::new();
        loop {
            let available = output
                .fill_buf()
                .await
                .map_err(|_| failed("protocol read failed"))?;
            if available.is_empty() {
                return Err(failed("unexpected protocol EOF"));
            }
            let end = available.iter().position(|b| *b == b'\n').map(|n| n + 1);
            let size = end.unwrap_or(available.len());
            if bytes.len() + size > 4 * 1024 * 1024 {
                return Err(failed("protocol frame exceeds 4 MiB"));
            }
            bytes.extend_from_slice(&available[..size]);
            output.consume(size);
            if end.is_some() {
                break;
            }
        }
        serde_json::from_slice(&bytes).map_err(|_| failed("malformed protocol frame"))
    }
    async fn request(&mut self, request: &Value) -> Result<Value> {
        let mut bytes =
            serde_json::to_vec(request).map_err(|_| failed("request encoding failed"))?;
        bytes.push(b'\n');
        self.input
            .as_mut()
            .ok_or_else(|| failed("missing request pipe"))?
            .write_all(&bytes)
            .await
            .map_err(|_| failed("request write failed"))?;
        let response = self.read().await?;
        if response["version"] != 1
            || response["id"] != request["id"]
            || !response["ok"].is_boolean()
        {
            return Err(failed("unexpected protocol response"));
        }
        if response["ok"] == true {
            if !response["data"].is_object() {
                return Err(failed("response data is not an object"));
            }
            return Ok(response["data"].clone());
        }
        let e = &response["error"];
        let capture = if request["operation"] == "audio_probe" {
            super::probe::failure_diagnostics(&e["details"])
        } else {
            e["details"].clone()
        };
        let cleanup = if request["operation"] == "audio_probe" {
            json!(capture["cleanupVerified"] == true)
        } else {
            e["details"]["cleanupVerified"].clone()
        };
        let dispatched = if request["operation"] == "audio_probe" {
            e["dispatched"].as_bool().map_or(Value::Null, Value::Bool)
        } else {
            e["dispatched"].clone()
        };
        let code = match e["code"].as_str() {
            Some("MEDIA_SESSION_STALE") => "MEDIA_SESSION_STALE",
            Some("MEDIA_UNSUPPORTED") => "CAPABILITY_UNAVAILABLE",
            Some("INVALID_ARGUMENT") => "INVALID_ARGUMENT",
            Some("MEDIA_OUTCOME_UNKNOWN") => "MEDIA_OUTCOME_UNKNOWN",
            Some("AUDIO_PROBE_FAILED") => "AUDIO_PROBE_FAILED",
            _ => "MEDIA_HELPER_FAILED",
        };
        Err(Error::new(
            code,
            e["message"]
                .as_str()
                .unwrap_or("Android media operation failed")
                .chars()
                .take(512)
                .collect::<String>(),
        )
        .details(json!({"dispatched":dispatched,"capture":capture,"cleanupVerified":cleanup})))
    }
    fn diagnostic(&self) -> String {
        String::from_utf8_lossy(&self.diagnostics.lock().unwrap_or_else(|e| e.into_inner()))
            .into_owned()
    }
    async fn serve(
        &mut self,
        requests: &mut mpsc::Receiver<Message>,
    ) -> Option<oneshot::Sender<Result<()>>> {
        while let Some(message) = requests.recv().await {
            match message {
                Message::Close(reply) => return Some(reply),
                Message::Call(request, reply) => {
                    // A cancelled read that was never dequeued has no native work to perform.
                    if reply.is_closed() {
                        continue;
                    }
                    let budget = if let Some(deadline) = request["deadlineUnixMs"].as_u64() {
                        let now = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_millis() as u64;
                        if deadline <= now {
                            let _ = reply.send(Err(super::probe::failed("helper_queue_deadline")));
                            continue;
                        }
                        Duration::from_millis((deadline - now).min(25000))
                    } else {
                        Duration::from_secs(25)
                    };
                    let result = tokio::time::timeout(budget, self.request(&request))
                        .await
                        .unwrap_or_else(|_| Err(failed("native request deadline exceeded")))
                        .map_err(|error| {
                            if error.code == "MEDIA_HELPER_FAILED" {
                                failed("protocol operation failed")
                                    .details(json!({"cause":error,"stderr":self.diagnostic()}))
                            } else {
                                error
                            }
                        });
                    let fatal = result
                        .as_ref()
                        .is_err_and(|e| e.code == "MEDIA_HELPER_FAILED");
                    let _ = reply.send(result);
                    if fatal {
                        break;
                    }
                }
            }
        }
        None
    }
    async fn cleanup(&mut self) -> Result<()> {
        let cleanup = async {
            if self.child.is_some() {
                let _ = tokio::time::timeout(
                    Duration::from_secs(1),
                    self.request(&json!({"version":1,"id":"close","operation":"quit"})),
                )
                .await;
                self.input.take();
                if let Some(child) = &mut self.child {
                    match tokio::time::timeout(Duration::from_secs(1), child.wait()).await {
                        Ok(result) => {
                            result.map_err(|_| failed("cannot reap owned adb child"))?;
                        }
                        Err(_) => child
                            .kill()
                            .await
                            .map_err(|_| failed("cannot stop owned adb child"))?,
                    }
                }
                self.child.take();
                self.output.take();
            }
            if self.owned {
                let command = cleanup_command(&self.directory, &self.name);
                self.shell(&command).await?;
                self.owned = false;
            }
            Ok(())
        };
        let result = tokio::time::timeout(Duration::from_secs(10), cleanup)
            .await
            .unwrap_or_else(|_| Err(failed("device cleanup deadline exceeded")));
        if let Some(local) = self.local.as_ref() {
            match std::fs::remove_dir_all(local) {
                Ok(()) => {
                    self.local.take();
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    self.local.take();
                }
                Err(_) => {
                    return Err(failed("host artifact cleanup failed")
                        .details(json!({"remoteCleanupError":result.err()})));
                }
            }
        }
        result.map_err(|e| {
            Error::new(
                "CLEANUP_FAILED",
                "Android media helper cleanup is unverified",
            )
            .details(json!({"cause":e}))
        })
    }
}
