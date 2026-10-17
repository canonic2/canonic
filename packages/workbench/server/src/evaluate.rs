//! Batched JavaScript evaluation and picomatch adapter; owns its child process
//! and its deadline.
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    path::Path,
    process::{Child, Command, Stdio},
    sync::mpsc,
    thread,
    time::Duration,
};

#[derive(Deserialize)]
pub(crate) struct Evaluated {
    pub(crate) file: String,
    pub(crate) entry: Option<serde_json::Value>,
    pub(crate) error: Option<String>,
    pub(crate) docs: bool,
    pub(crate) files: Vec<String>,
    #[serde(default)]
    pub(crate) cacheable: bool,
}

#[derive(Deserialize)]
pub(crate) struct Batch {
    pub(crate) entries: Vec<Evaluated>,
    pub(crate) settings: Vec<String>,
}

pub(crate) struct Evaluator<'a> {
    root: &'a Path,
    executable: &'a Path,
    script: &'a Path,
    options: &'a Value,
    /// How long one evaluation may run before it is killed.
    deadline: Duration,
}
impl<'a> Evaluator<'a> {
    pub(crate) fn new(
        root: &'a Path,
        executable: &'a Path,
        script: &'a Path,
        options: &'a Value,
        deadline: Duration,
    ) -> Self {
        Self {
            root,
            executable,
            script,
            options,
            deadline,
        }
    }
    pub(crate) fn available(&self) -> bool {
        !self.executable.as_os_str().is_empty()
    }
    pub(crate) fn select(&self, files: &[String]) -> Result<Vec<String>> {
        self.call("select", files)
    }
    pub(crate) fn evaluate(&self, files: &[String]) -> Result<Batch> {
        self.call("evaluate", files)
    }

    fn call<T: DeserializeOwned>(&self, action: &str, files: &[String]) -> Result<T> {
        // Serialize before spawning, so every fallible post-spawn operation is
        // covered by the owned child's cleanup.
        let input = serde_json::to_vec(
            &json!({"root": self.root, "options": self.options, "action": action, "files": files}),
        )?;
        let child = Command::new(self.executable)
            .arg(self.script)
            .env("ELECTRON_RUN_AS_NODE", "1")
            .env_remove("NODE_OPTIONS")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .context("start definition evaluator")?;
        let mut owned = EvaluationProcess(child);
        let mut stdin = owned.0.stdin.take().context("evaluator stdin")?;
        let mut stdout = owned.0.stdout.take().context("evaluator stdout")?;
        // Both ends run on their own threads, so an evaluator that stops
        // reading or never answers cannot outlast the deadline. Dropping
        // `owned` on the way out kills it, which ends both threads.
        let writer = thread::spawn(move || stdin.write_all(&input));
        let (send, receive) = mpsc::channel();
        thread::spawn(move || {
            let mut output = Vec::new();
            let read = stdout.read_to_end(&mut output).map(|_| output);
            let _sent = send.send(read);
        });
        let output = match receive.recv_timeout(self.deadline) {
            Ok(read) => read.context("definition evaluator output")?,
            Err(_) => bail!(
                "definition evaluator did not finish within {} seconds",
                self.deadline.as_secs_f64()
            ),
        };
        writer
            .join()
            .map_err(|_| anyhow::anyhow!("evaluator input writer panicked"))?
            .context("evaluator input")?;
        let status = owned.0.wait().context("definition evaluator exit")?;
        ensure!(status.success(), "definition evaluator failed: {status}");
        serde_json::from_slice(&output).context("invalid evaluator JSON")
    }
}

/// Every early return, the deadline included, kills and reaps this adapter's
/// evaluator. The host owns the enclosing process group, which holds the
/// evaluator's own descendants.
struct EvaluationProcess(Child);
impl Drop for EvaluationProcess {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
        }
        let _ = self.0.wait();
    }
}
