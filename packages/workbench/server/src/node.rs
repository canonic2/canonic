//! The Node evaluator: one long-lived Node process per space, the only place
//! project JavaScript runs (`workbench.config.ts`, computed definitions,
//! project loaders, Vue's compiler and Astro rendering). Requests and answers
//! are JSON lines on its standard streams, matched by ID, so requests run
//! concurrently. It starts on first need, after the Node server has supplied
//! a runtime for a trusted workspace, and starts again on the next need after
//! it exits. It is killed with this process.
use anyhow::{Context as _, Result, anyhow, bail};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex, RwLock};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::{oneshot, watch};

/// How the evaluator is started: a Node runtime and the evaluator script.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Program {
    pub(crate) node: PathBuf,
    pub(crate) script: PathBuf,
}

type Pending = Arc<Mutex<HashMap<u64, oneshot::Sender<Result<Value, String>>>>>;

struct Running {
    stdin: ChildStdin,
    pending: Pending,
    _child: Child,
    alive: watch::Receiver<bool>,
}

/// Where a runtime comes from when the evaluator first needs one.
pub(crate) type Runtime =
    Arc<dyn Fn() -> futures::future::BoxFuture<'static, Result<PathBuf>> + Send + Sync>;

pub(crate) struct Evaluator {
    root: PathBuf,
    program: watch::Sender<Option<Program>>,
    /// Asked for a runtime on first need, when none was supplied.
    runtime: Mutex<Option<(Runtime, PathBuf)>>,
    running: tokio::sync::Mutex<Option<Running>>,
    next: AtomicU64,
}

/// How long one request may take before it fails.
const DEADLINE: Duration = Duration::from_mins(2);
/// How long a request waits for the Node server to supply a runtime.
const RUNTIME_WAIT: Duration = Duration::from_mins(1);

impl Evaluator {
    pub(crate) fn new(root: &Path) -> Arc<Self> {
        Arc::new(Self {
            root: root.to_path_buf(),
            program: watch::channel(None).0,
            runtime: Mutex::new(None),
            running: tokio::sync::Mutex::new(None),
            next: AtomicU64::new(1),
        })
    }

    /// Supplies the runtime; a different one replaces a running evaluator.
    pub(crate) async fn supply(&self, program: Program) {
        let changed = self.program.borrow().as_ref() != Some(&program);
        if changed {
            self.program.send_replace(Some(program));
            self.stop().await;
        }
    }

    /// Where to ask for a runtime, with the evaluator script, when one is
    /// first needed and none was supplied.
    pub(crate) fn provide(&self, runtime: Runtime, script: PathBuf) {
        *self.runtime.lock().expect("runtime lock") = Some((runtime, script));
    }

    /// Stops the running evaluator; the next request starts a new one, which
    /// evaluates the project's configuration again.
    pub(crate) async fn stop(&self) {
        self.running.lock().await.take();
    }

    async fn program(&self) -> Result<Program> {
        let provider = self.runtime.lock().expect("runtime lock").clone();
        if self.program.borrow().is_none()
            && let Some((runtime, script)) = provider
        {
            let node = runtime().await?;
            self.program.send_if_modified(|current| {
                if current.is_some() {
                    return false;
                }
                *current = Some(Program { node, script });
                true
            });
        }
        let mut program = self.program.subscribe();
        let wait = program.wait_for(Option::is_some);
        match tokio::time::timeout(RUNTIME_WAIT, wait).await {
            Ok(Ok(found)) => found.clone().context("evaluator runtime"),
            _ => bail!(
                "the Node evaluator has no runtime: the workspace is untrusted or previews are off"
            ),
        }
    }

    async fn spawn(&self) -> Result<Running> {
        let program = self.program().await?;
        let mut child = Command::new(&program.node)
            .arg(&program.script)
            .current_dir(&self.root)
            .env("ELECTRON_RUN_AS_NODE", "1")
            .env_remove("NODE_OPTIONS")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .with_context(|| format!("start the Node evaluator with {}", program.node.display()))?;
        let stdin = child.stdin.take().context("evaluator stdin")?;
        let stdout = child.stdout.take().context("evaluator stdout")?;
        let pending: Pending = Arc::default();
        let (alive_send, alive) = watch::channel(true);
        let answers = pending.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let Ok(message) = serde_json::from_str::<Value>(&line) else {
                    continue;
                };
                let Some(id) = message.get("id").and_then(Value::as_u64) else {
                    continue;
                };
                let Some(sender) = answers.lock().expect("pending lock").remove(&id) else {
                    continue;
                };
                let result = if message.get("ok").and_then(Value::as_bool) == Some(true) {
                    Ok(message.get("value").cloned().unwrap_or(Value::Null))
                } else {
                    Err(message
                        .get("error")
                        .and_then(Value::as_str)
                        .unwrap_or("the Node evaluator failed")
                        .to_owned())
                };
                let _sent = sender.send(result);
            }
            // Every request still waiting fails with the process.
            for (_, sender) in answers.lock().expect("pending lock").drain() {
                let _sent = sender.send(Err("the Node evaluator stopped".to_owned()));
            }
            let _sent = alive_send.send(false);
        });
        Ok(Running {
            stdin,
            pending,
            _child: child,
            alive,
        })
    }

    /// One operation, answered by the evaluator within the deadline.
    pub(crate) async fn call(&self, op: &str, mut request: Value) -> Result<Value> {
        let id = self.next.fetch_add(1, Ordering::SeqCst);
        if let Some(object) = request.as_object_mut() {
            object.insert("id".into(), json!(id));
            object.insert("op".into(), json!(op));
            object
                .entry("root")
                .or_insert_with(|| json!(self.root.to_string_lossy()));
        }
        let mut line = serde_json::to_vec(&request)?;
        line.push(b'\n');
        let (send, receive) = oneshot::channel();
        {
            let mut running = self.running.lock().await;
            if running
                .as_ref()
                .is_none_or(|current| !*current.alive.borrow())
            {
                *running = Some(self.spawn().await?);
            }
            let current = running.as_mut().context("evaluator")?;
            current
                .pending
                .lock()
                .expect("pending lock")
                .insert(id, send);
            if let Err(error) = current.stdin.write_all(&line).await {
                running.take();
                return Err(anyhow!(error).context("write to the Node evaluator"));
            }
        }
        match tokio::time::timeout(DEADLINE, receive).await {
            Ok(Ok(Ok(value))) => Ok(value),
            Ok(Ok(Err(message))) => Err(anyhow!(message)),
            Ok(Err(_)) => bail!("the Node evaluator stopped"),
            Err(_) => {
                // A request that never answers may hold the process; start over.
                self.stop().await;
                bail!(
                    "the Node evaluator did not answer {op} within {} seconds",
                    DEADLINE.as_secs()
                )
            }
        }
    }
}

static REGISTRY: LazyLock<RwLock<HashMap<PathBuf, Arc<Evaluator>>>> =
    LazyLock::new(RwLock::default);

/// The evaluator for a project root, shared by everything that runs project
/// JavaScript for it, including loaders the compiler restores from its cache.
pub(crate) fn for_root(root: &Path) -> Arc<Evaluator> {
    if let Some(found) = REGISTRY.read().expect("evaluator registry").get(root) {
        return found.clone();
    }
    REGISTRY
        .write()
        .expect("evaluator registry")
        .entry(root.to_path_buf())
        .or_insert_with(|| Evaluator::new(root))
        .clone()
}
