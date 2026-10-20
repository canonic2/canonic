//! Standard-stream adapter between the host and the server: one JSON
//! message per line each way.
//!
//! In: `{"type":"index","id":N,"request":{...}}`, one page-index request.
//! In: `{"type":"config","id":N,"dir":"...","key":null}`, resolved manifests.
//! In: `{"type":"catalog","id":N}`, the page tree with what this machine adds.
//! In: `{"type":"announce","id":N}`, this server's address written for agents again.
//! In: `{"type":"reply","id":N,"ok":true,"value":...}`, the host's answer to a question.
//! Out: `{"type":"listening","port":N}` once, `{"type":"answer","id":N,...}`
//! per request, `{"type":"host","id":N,"question":"..."}` for the host,
//! `{"type":"changed"}` when the page tree changed on disk, and
//! `{"type":"log"|"diagnostic"|"shot"|"catalog",...}` notices.
use super::resident::Resident;
use crate::space::Space;
use crate::{config, model::Request};
use anyhow::Result;
use serde::Deserialize;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    sync::Mutex,
};

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
enum Message {
    Index {
        id: u64,
        request: Request,
    },
    Config {
        id: u64,
        dir: PathBuf,
        key: Option<String>,
    },
    Catalog {
        id: u64,
    },
    Announce {
        id: u64,
    },
    /// The host's answer to one of this process's questions.
    Reply {
        id: u64,
        ok: bool,
        value: Option<Value>,
        error: Option<String>,
    },
}

type Waiting = std::sync::Mutex<
    std::collections::HashMap<u64, tokio::sync::oneshot::Sender<Result<Value, String>>>,
>;

/// How long the host has to answer a question; a capture may start the
/// capture helper first.
const ASK_TIMEOUT: std::time::Duration = std::time::Duration::from_mins(5);

/// Serialized writes to standard output, shared by every task, and the
/// questions this process is waiting for the host to answer.
#[derive(Clone)]
pub(crate) struct Output(
    Arc<Mutex<tokio::io::Stdout>>,
    Arc<Waiting>,
    Arc<std::sync::atomic::AtomicU64>,
);

impl Output {
    pub(crate) fn new() -> Self {
        Self(
            Arc::new(Mutex::new(tokio::io::stdout())),
            Arc::default(),
            Arc::new(std::sync::atomic::AtomicU64::new(1)),
        )
    }

    /// Asks the host; its `reply` message answers.
    pub(crate) async fn ask(&self, mut message: Value) -> Result<Value> {
        let id = self.2.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        message["id"] = json!(id);
        let (send, receive) = tokio::sync::oneshot::channel();
        self.1.lock().expect("waiting lock").insert(id, send);
        self.send(&message).await?;
        match tokio::time::timeout(ASK_TIMEOUT, receive).await {
            Ok(Ok(Ok(value))) => Ok(value),
            Ok(Ok(Err(error))) => Err(anyhow::anyhow!(error)),
            _ => {
                self.1.lock().expect("waiting lock").remove(&id);
                Err(anyhow::anyhow!("the Workbench host did not answer"))
            }
        }
    }

    fn reply(&self, id: u64, result: Result<Value, String>) {
        if let Some(waiting) = self.1.lock().expect("waiting lock").remove(&id) {
            let _sent = waiting.send(result);
        }
    }

    pub(crate) async fn listening(&self, port: u16) -> Result<()> {
        self.send(&json!({ "type": "listening", "port": port }))
            .await
    }

    /// A diagnostic for the host's log, such as a request's timing.
    pub(crate) async fn diagnostic(&self, event: &str, details: Value) -> Result<()> {
        self.send(&json!({ "type": "diagnostic", "event": event, "details": details }))
            .await
    }

    pub(crate) async fn changed(&self) -> Result<()> {
        self.send(&json!({ "type": "changed" })).await
    }

    pub(crate) async fn send(&self, message: &Value) -> Result<()> {
        let mut line = serde_json::to_vec(message)?;
        line.push(b'\n');
        let mut stdout = self.0.lock().await;
        stdout.write_all(&line).await?;
        stdout.flush().await?;
        Ok(())
    }
}

/// The same bound as the one-process index's output.
const MAX_ANSWER: usize = 128 * 1024 * 1024;

fn answer(id: u64, response: &crate::protocol::Response) -> Value {
    let message = json!({ "type": "answer", "id": id, "ok": true, "response": response });
    if message.to_string().len() > MAX_ANSWER {
        failure(id, "the page index answer exceeds 128 MiB")
    } else {
        message
    }
}

fn failure(id: u64, error: &str) -> Value {
    json!({ "type": "answer", "id": id, "ok": false, "error": error })
}

fn answered(id: u64, result: Result<Value>) -> Value {
    match result {
        Ok(response) => json!({ "type": "answer", "id": id, "ok": true, "response": response }),
        Err(error) => failure(id, &format!("{error:#}")),
    }
}

/// Answers messages until standard input closes.
pub(crate) async fn read(resident: Arc<Resident>, space: Arc<Space>, output: Output) -> Result<()> {
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    while let Some(line) = lines.next_line().await? {
        let message = match serde_json::from_str::<Message>(&line) {
            Ok(message) => message,
            Err(error) => {
                output
                    .send(&json!({ "type": "error", "error": format!("invalid message: {error}") }))
                    .await?;
                continue;
            }
        };
        let output = output.clone();
        match message {
            Message::Reply {
                id,
                ok,
                value,
                error,
            } => {
                output.reply(
                    id,
                    if ok {
                        Ok(value.unwrap_or(Value::Null))
                    } else {
                        Err(error.unwrap_or_else(|| "the Workbench host failed".into()))
                    },
                );
            }
            Message::Catalog { id } => {
                let space = space.clone();
                tokio::spawn(async move {
                    let result = space.view().await.map(|view| view.unwrap_or(Value::Null));
                    let _sent = output.send(&answered(id, result)).await;
                });
            }
            Message::Announce { id } => {
                let result = space
                    .agent
                    .announce(&format!("http://127.0.0.1:{}/", space.port))
                    .map(|()| Value::Null);
                let _sent = output.send(&answered(id, result)).await;
            }
            Message::Config { id, dir, key } => {
                tokio::spawn(async move {
                    let result =
                        tokio::task::spawn_blocking(move || config::answer(&dir, key.as_deref()))
                            .await;
                    let message = match result {
                        Ok(result) => answered(id, result),
                        Err(error) => failure(id, &format!("configuration task failed: {error}")),
                    };
                    let _sent = output.send(&message).await;
                });
            }
            Message::Index { id, request } => {
                let resident = resident.clone();
                tokio::spawn(async move {
                    let message = match resident.index(request).await {
                        Ok(response) => answer(id, &response),
                        Err(error) => failure(id, &format!("{error:#}")),
                    };
                    // A closed output means the host is gone; the read loop
                    // ends with it.
                    let _sent = output.send(&message).await;
                });
            }
        }
    }
    Ok(())
}
