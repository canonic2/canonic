//! The process that started this server: the editor extension, or the
//! standalone command. It owns what is the editor's or the capture
//! helper's, and this server asks it over standard streams:
//!
//! - questions (`host` messages answered by `reply`): the Node runtime for
//!   project code, a capture operation, opening a file, a handoff, the spaces
//!   it shows and opening one, native window streams and their socket;
//! - notices (no answer): a saved screenshot, a diagnostic record, and that
//!   the docs catalog changed.
//!
//! Captured images come back in a file the host wrote, which this server
//! reads and removes, so large images never travel as JSON text.
use crate::serve::Output;
use anyhow::{Context as _, Result, anyhow};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use tokio::sync::OnceCell;

/// A capture operation's result: the image or PDF, with what the helper
/// reported about it.
#[derive(Debug, Default)]
pub(crate) struct Shot {
    pub(crate) body: Vec<u8>,
    pub(crate) targets: Option<Value>,
    pub(crate) details: Option<Value>,
}

pub(crate) struct Host {
    output: Output,
    node: OnceCell<PathBuf>,
}

impl Host {
    pub(crate) fn new(output: Output) -> Self {
        Self {
            output,
            node: OnceCell::new(),
        }
    }

    async fn ask(&self, question: &str, mut fields: Value) -> Result<Value> {
        fields["type"] = json!("host");
        fields["question"] = json!(question);
        self.output.ask(fields).await
    }

    async fn notify(&self, notice: &str, mut fields: Value) {
        fields["type"] = json!(notice);
        // A closed output means the host is gone; the server stops with it.
        let _sent = self.output.send(&fields).await;
    }

    /// The Node runtime project code runs on, asked once.
    pub(crate) async fn node(&self) -> Result<PathBuf> {
        self.node
            .get_or_try_init(|| async {
                let answer = self.ask("node", json!({})).await?;
                answer
                    .as_str()
                    .filter(|path| !path.is_empty())
                    .map(PathBuf::from)
                    .context("the host gave no Node runtime")
            })
            .await
            .cloned()
    }

    /// One capture operation; `worker` names an export pool's worker.
    pub(crate) async fn capture(
        &self,
        operation: &str,
        base: &str,
        payload: &Value,
        worker: Option<&str>,
    ) -> Result<Shot> {
        let answer = self
            .ask("capture", json!({ "operation": operation, "base": base, "payload": payload, "worker": worker }))
            .await?;
        let body = match answer["file"].as_str() {
            Some(file) => {
                let file = Path::new(file);
                let body = tokio::fs::read(file).await.with_context(|| {
                    format!("couldn’t read the captured image at {}", file.display())
                })?;
                let _removed = tokio::fs::remove_file(file).await;
                body
            }
            None => Vec::new(),
        };
        Ok(Shot {
            body,
            targets: answer
                .get("targets")
                .filter(|value| !value.is_null())
                .cloned(),
            details: answer
                .get("details")
                .filter(|value| !value.is_null())
                .cloned(),
        })
    }

    /// Workers for an export: as many as the host gives, up to `size`.
    pub(crate) async fn capture_pool(&self, size: usize) -> Result<Vec<String>> {
        let answer = self.ask("capture-pool", json!({ "size": size })).await?;
        let workers = answer["workers"]
            .as_array()
            .map(|workers| {
                workers
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default();
        Ok(workers)
    }

    pub(crate) async fn close_capture_pool(&self, workers: &[String]) -> Result<()> {
        self.ask("capture-pool-close", json!({ "workers": workers }))
            .await
            .map(drop)
    }

    /// Puts `file` in front of the user.
    pub(crate) async fn open(&self, file: &Path) -> Result<()> {
        self.ask("open", json!({ "file": file })).await.map(drop)
    }

    /// Hands the prompt, and the canvas it describes, to the editor; fails
    /// when nothing receives handoffs.
    pub(crate) async fn handoff(&self, prompt: &str, canvas: &Value) -> Result<()> {
        self.ask("handoff", json!({ "prompt": prompt, "canvas": canvas }))
            .await
            .map(drop)
    }

    /// The spaces the host shows, or `None` when this is its only one.
    pub(crate) async fn spaces(&self) -> Result<Option<Value>> {
        let answer = self.ask("spaces", json!({})).await?;
        Ok((!answer.is_null()).then_some(answer))
    }

    /// Opens a listed space, starting its server first, and answers its address.
    pub(crate) async fn open_space(&self, id: &str) -> Result<Value> {
        self.ask("space-open", json!({ "space": id })).await
    }

    /// Starts or stops a native window stream.
    pub(crate) async fn stream(&self, operation: &str, request: &Value) -> Result<Value> {
        self.ask(
            "stream",
            json!({ "operation": operation, "request": request }),
        )
        .await
    }

    /// The local socket the host serves window streams on.
    pub(crate) async fn stream_socket(&self) -> Result<PathBuf> {
        let answer = self.ask("stream-socket", json!({})).await?;
        answer
            .as_str()
            .map(PathBuf::from)
            .ok_or_else(|| anyhow!("the host serves no window streams"))
    }

    pub(crate) async fn shot(&self, file: &str) {
        self.notify("shot", json!({ "file": file })).await;
    }

    pub(crate) async fn catalog_changed(&self) {
        self.notify("catalog", json!({})).await;
    }

    pub(crate) async fn log(&self, record: Value) {
        self.notify("log", json!({ "record": record })).await;
    }
}
