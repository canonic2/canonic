//! Owned input child, readiness handshake and bounded correlated request/reply I/O.
use super::{
    cleanup,
    protocol::{Action, Command},
    unavailable, unknown,
};
use crate::{
    error::{Error, Result},
    simulator::ipc,
};
use std::time::Duration;
use tokio::{io::AsyncWriteExt, process::Child};

pub(super) struct Factory {
    pub(super) program: std::path::PathBuf,
    pub(super) args: Vec<String>,
}
pub(super) struct Transport {
    child: Child,
    input: tokio::process::ChildStdin,
    output: tokio::process::ChildStdout,
}
impl Factory {
    pub(super) async fn open(&self) -> Result<Transport> {
        let mut command = tokio::process::Command::new(&self.program);
        crate::simulator::device_set::DeviceSet::current()?.apply_command(&mut command);
        let mut child = command
            .args(&self.args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|_| unavailable())?;
        let input = child.stdin.take().ok_or_else(unavailable)?;
        let output = child.stdout.take().ok_or_else(unavailable)?;
        let mut transport = Transport {
            child,
            input,
            output,
        };
        let ready =
            tokio::time::timeout(Duration::from_secs(5), ipc::read(&mut transport.output)).await;
        if !matches!(ready,Ok(Ok(ref m)) if m.metadata["event"]=="ready" && m.payload.is_empty()) {
            let error = match &ready {
                Ok(Ok(message)) if message.metadata["event"] == "error" => {
                    ipc::failure(&message.metadata["error"])
                }
                _ => unavailable(),
            };
            transport.terminate().await?;
            return Err(error);
        }
        Ok(transport)
    }
}
impl Transport {
    pub(super) async fn request(
        &mut self,
        action: Action,
        geometry: Option<[u32; 2]>,
    ) -> Result<()> {
        let id = uuid::Uuid::new_v4().to_string();
        let mut bytes = serde_json::to_vec(&Command {
            id: id.clone(),
            geometry,
            input: action,
        })
        .map_err(|_| unavailable())?;
        if bytes.len() > 4 * 1024 * 1024 {
            return Err(Error::invalid("Native input exceeds limit"));
        }
        bytes.push(b'\n');
        self.input.write_all(&bytes).await.map_err(|_| unknown())?;
        let reply = ipc::read(&mut self.output).await.map_err(|_| unknown())?;
        if reply.metadata["event"] != "reply"
            || reply.metadata["id"] != id
            || !reply.payload.is_empty()
        {
            return Err(unknown());
        }
        if let Some(error) = reply.metadata.get("error").filter(|e| !e.is_null()) {
            return Err(Error::new(
                error["code"].as_str().unwrap_or("OUTCOME_UNKNOWN"),
                error["message"].as_str().unwrap_or("Native input failed"),
            )
            .details(error.clone()));
        }
        Ok(())
    }
    pub(super) async fn terminate(&mut self) -> Result<()> {
        if self.child.try_wait().map_err(|_| cleanup())?.is_none() {
            self.child.kill().await.map_err(|_| cleanup())?;
        }
        self.child.wait().await.map_err(|_| cleanup())?;
        Ok(())
    }
    pub(super) async fn close(&mut self) -> Result<()> {
        let result = tokio::time::timeout(Duration::from_secs(5), async {
            self.request(Action::Close, None).await?;
            let status = self.child.wait().await.map_err(|_| cleanup())?;
            if !status.success() {
                return Err(cleanup());
            }
            Ok(())
        })
        .await;
        match result {
            Ok(Ok(())) => Ok(()),
            _ => {
                self.terminate().await?;
                Err(cleanup())
            }
        }
    }
}
