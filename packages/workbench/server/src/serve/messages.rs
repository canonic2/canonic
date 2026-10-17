//! Standard-stream adapter between the Node server and the front door: one JSON
//! message per line each way.
//!
//! In: `{"type":"index","id":N,"request":{...}}`, the page index's request.
//! Out: `{"type":"listening","port":N}` once, `{"type":"answer","id":N,...}`
//! per request, and `{"type":"changed"}` when the page tree changed on disk.
use super::resident::Resident;
use crate::model::Request;
use anyhow::Result;
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    sync::Mutex,
};

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
enum Message {
    Index { id: u64, request: Request },
}

/// Serialized writes to standard output, shared by every task.
#[derive(Clone)]
pub(crate) struct Output(Arc<Mutex<tokio::io::Stdout>>);

impl Output {
    pub(crate) fn new() -> Self {
        Self(Arc::new(Mutex::new(tokio::io::stdout())))
    }

    pub(crate) async fn listening(&self, port: u16) -> Result<()> {
        self.send(&json!({ "type": "listening", "port": port }))
            .await
    }

    pub(crate) async fn changed(&self) -> Result<()> {
        self.send(&json!({ "type": "changed" })).await
    }

    async fn send(&self, message: &Value) -> Result<()> {
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

/// Answers messages until standard input closes.
pub(crate) async fn read(resident: Arc<Resident>, output: Output) -> Result<()> {
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
        match message {
            Message::Index { id, request } => {
                let (resident, output) = (resident.clone(), output.clone());
                tokio::spawn(async move {
                    let message = match resident.index(request).await {
                        Ok(response) => answer(id, &response),
                        Err(error) => failure(id, &format!("{error:#}")),
                    };
                    // A closed output means the Node server is gone; the read
                    // loop ends with it.
                    let _sent = output.send(&message).await;
                });
            }
        }
    }
    Ok(())
}
