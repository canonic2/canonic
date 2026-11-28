//! Authenticated bounded runner RPC and transport timing; no runner installation policy.
use crate::error::{Error, Result};
use serde_json::Value;
#[cfg(unix)]
use serde_json::json;
#[cfg(unix)]
use std::path::Path;
use std::time::Duration;
#[cfg(unix)]
use tokio::io::{AsyncReadExt, AsyncWriteExt};
#[cfg(unix)]
use uuid::Uuid;

pub struct Reply {
    pub data: Value,
    pub payload: Vec<u8>,
}
#[derive(Clone)]
#[cfg_attr(
    windows,
    allow(dead_code, reason = "Physical iOS runner transport requires Unix")
)]
pub struct Runner {
    pub(super) device_id: u32,
    pub(super) session_id: String,
    pub(super) token: String,
    pub(super) port: u16,
    pub(super) host: Option<std::sync::Weak<super::host::Host>>,
}
impl Runner {
    #[cfg(test)]
    pub(crate) fn fixture() -> Self {
        Self {
            device_id: 0,
            session_id: "Acme".into(),
            token: "fixture".into(),
            port: 1,
            host: None,
        }
    }
    #[cfg(unix)]
    pub async fn call(
        &self,
        action: &str,
        arguments: Value,
        command_id: Option<&str>,
        timeout: Duration,
    ) -> Result<Reply> {
        let remote = self.call_remote(action, arguments, command_id, timeout);
        self.call_with_host(action, remote).await
    }

    #[cfg(unix)]
    async fn call_with_host(
        &self,
        action: &str,
        remote: impl std::future::Future<Output = Result<Reply>>,
    ) -> Result<Reply> {
        let Some(host) = &self.host else {
            return remote.await;
        };
        let host = host.upgrade().ok_or_else(host_exited)?;
        if host.exited()?.is_some() {
            return Err(host_exited());
        }
        tokio::select! {
            result = remote => {
                if result.is_err() && host.exited()?.is_some() {
                    return Err(host_request_exited(action));
                }
                result
            },
            status = host.wait() => {
                status?;
                Err(host_request_exited(action))
            }
        }
    }

    /// Cleanup can still contact the authenticated native runner after its
    /// host exits; host process loss alone is not native release proof.
    #[cfg(unix)]
    pub(super) async fn call_remote(
        &self,
        action: &str,
        arguments: Value,
        command_id: Option<&str>,
        timeout: Duration,
    ) -> Result<Reply> {
        self.call_connected(
            action,
            arguments,
            command_id,
            timeout,
            crate::usbmux::connect(Path::new("/var/run/usbmuxd"), self.device_id, self.port),
        )
        .await
    }

    #[cfg(unix)]
    async fn call_connected<S>(
        &self,
        action: &str,
        arguments: Value,
        command_id: Option<&str>,
        timeout: Duration,
        connect: impl std::future::Future<Output = Result<S>>,
    ) -> Result<Reply>
    where
        S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
    {
        let started = std::time::Instant::now();
        tokio::time::timeout(timeout, async {
            let mut socket = connect.await?;
            let request_id = Uuid::new_v4().to_string();
            let request = json!({
                "version":1,
                "sessionId":self.session_id,
                "token":self.token,
                "requestId":request_id,
                "action":action,
                "arguments":arguments,
                "commandId":command_id
            });
            let bytes = serde_json::to_vec(&request)
                .map_err(|_| Error::new("INTERNAL", "Cannot encode private runner request"))?;
            if bytes.len() > 1024 * 1024 {
                return Err(Error::invalid("Runner request exceeds 1 MiB"));
            }
            socket
                .write_all(&(bytes.len() as u32).to_be_bytes())
                .await
                .map_err(|_| Error::new("DEVICE_DISCONNECTED", "Device request was interrupted"))?;
            socket
                .write_all(&bytes)
                .await
                .map_err(|_| Error::new("DEVICE_DISCONNECTED", "Device request was interrupted"))?;
            let mut header = [0; 4];
            socket
                .read_exact(&mut header)
                .await
                .map_err(|_| interrupted(action, "Device disconnected after request dispatch"))?;
            let length = u32::from_be_bytes(header) as usize;
            if length > 2 * 1024 * 1024 {
                return Err(Error::new(
                    "OUTPUT_LIMIT",
                    "Device reply metadata exceeds 2 MiB",
                ));
            }
            let mut bytes = vec![0; length];
            socket
                .read_exact(&mut bytes)
                .await
                .map_err(|_| interrupted(action, "Device reply was interrupted"))?;
            let response: Value = serde_json::from_slice(&bytes)
                .map_err(|_| Error::new("PLATFORM_PROTOCOL", "Invalid owned runner response"))?;
            if response["ok"] != true {
                let mut error = Error::new(
                    response["code"].as_str().unwrap_or("PLATFORM_FAILED"),
                    response["message"]
                        .as_str()
                        .unwrap_or("Owned runner operation failed"),
                );
                error.retryable = response["retryable"].as_bool().unwrap_or(false);
                return Err(error);
            }
            if response["requestId"] != request_id {
                return Err(Error::new(
                    "PLATFORM_PROTOCOL",
                    "Runner reply identity does not match its request",
                ));
            }
            let length = response["payloadLength"].as_u64().ok_or_else(|| {
                Error::new("PLATFORM_PROTOCOL", "Runner reply omitted payload length")
            })?;
            if length > 64 * 1024 * 1024 {
                return Err(Error::new("OUTPUT_LIMIT", "Device frame exceeds 64 MiB"));
            }
            let mut payload = vec![0; length as usize];
            socket
                .read_exact(&mut payload)
                .await
                .map_err(|_| Error::new("DEVICE_DISCONNECTED", "Device frame was interrupted"))?;
            let mut data = response["data"].clone();
            if data.is_object() {
                data["rpcTimingsMs"] = rpc_timings(started.elapsed(), &response["runnerElapsedMs"]);
            }
            Ok(Reply { data, payload })
        })
        .await
        .map_err(|_| expired(action))?
    }
    #[cfg(not(unix))]
    pub async fn call(
        &self,
        _action: &str,
        _arguments: Value,
        _command_id: Option<&str>,
        _timeout: Duration,
    ) -> Result<Reply> {
        Err(Error::new(
            "UNSUPPORTED_HOST",
            "Physical device adapter requires a Unix host",
        ))
    }
    #[cfg(not(unix))]
    pub(super) async fn call_remote(
        &self,
        action: &str,
        arguments: Value,
        command_id: Option<&str>,
        timeout: Duration,
    ) -> Result<Reply> {
        self.call(action, arguments, command_id, timeout).await
    }
}

#[cfg(unix)]
fn host_exited() -> Error {
    Error::new(
        "RUNNER_EXITED",
        "The owned XCTest host exited; disconnect and reconnect with runner setup. Input was not replayed",
    )
}

#[cfg(unix)]
fn host_request_exited(action: &str) -> Error {
    let code = match action {
        "ui_snapshot" => "INSPECTION_FAILED",
        "tap" | "long_press" | "swipe" | "type_text" | "app_launch" | "app_terminate" => {
            "OUTCOME_UNKNOWN"
        }
        _ => "RUNNER_EXITED",
    };
    Error::new(
        code,
        "XCTest host exited during the request; input was not replayed",
    )
    .details(json!({"cause":"RUNNER_EXITED"}))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn runner() -> Runner {
        Runner::fixture()
    }

    async fn request(socket: &mut tokio::io::DuplexStream) -> Value {
        let length = socket.read_u32().await.unwrap();
        assert!(length < 4096);
        let mut bytes = vec![0; length as usize];
        socket.read_exact(&mut bytes).await.unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn inspection_expiry_and_interruption_are_not_uncertain_input() {
        for action in ["ui_snapshot", "tap"] {
            for interrupted in [false, true] {
                let (host, mut peer) = tokio::io::duplex(4096);
                let (admitted, admission) = tokio::sync::oneshot::channel();
                let remote = tokio::spawn(async move {
                    let received = request(&mut peer).await;
                    assert_eq!(received["action"], action);
                    admitted.send(()).unwrap();
                    if !interrupted {
                        std::future::pending::<()>().await;
                    }
                });
                let result = runner()
                    .call_connected(
                        action,
                        json!({}),
                        Some("once"),
                        Duration::from_millis(100),
                        async { Ok(host) },
                    )
                    .await;
                admission.await.unwrap();
                let error = result.err().unwrap();
                assert_eq!(
                    error.code,
                    if action != "ui_snapshot" {
                        "OUTCOME_UNKNOWN"
                    } else if interrupted {
                        "INSPECTION_FAILED"
                    } else {
                        "INSPECTION_TIMEOUT"
                    }
                );
                remote.abort();
                let _ = remote.await;
            }
        }
    }

    #[tokio::test]
    async fn busy_refusal_preserves_retryability_without_a_success_reply() {
        let (host, mut peer) = tokio::io::duplex(4096);
        let remote = tokio::spawn(async move {
            let received = request(&mut peer).await;
            let reply = serde_json::to_vec(&json!({"requestId":received["requestId"],
                "ok":false,"code":"RUNNER_BUSY","message":"No work queued","retryable":true}))
            .unwrap();
            peer.write_u32(reply.len() as u32).await.unwrap();
            peer.write_all(&reply).await.unwrap();
        });
        let error = runner()
            .call_connected(
                "ui_snapshot",
                json!({}),
                None,
                Duration::from_secs(1),
                async { Ok(host) },
            )
            .await
            .err()
            .unwrap();
        assert_eq!(error.code, "RUNNER_BUSY");
        assert!(error.retryable);
        remote.await.unwrap();
    }

    #[tokio::test]
    async fn host_exit_interrupts_waiters_without_replaying_or_killing_on_read_cancellation() {
        use std::sync::Arc;
        for action in ["ui_snapshot", "tap", "hello"] {
            let child = tokio::process::Command::new("/bin/sleep")
                .arg("60")
                .kill_on_drop(true)
                .spawn()
                .unwrap();
            let host = Arc::new(super::super::host::Host::new(child));
            let mut runner = Runner::fixture();
            runner.host = Some(Arc::downgrade(&host));
            assert!(
                tokio::time::timeout(
                    Duration::from_millis(20),
                    runner.call_with_host(action, std::future::pending())
                )
                .await
                .is_err()
            );
            assert!(host.exited().unwrap().is_none());
            let exiting = host.clone();
            let exit = tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(20)).await;
                exiting.kill_fixture();
            });
            let error = tokio::time::timeout(
                Duration::from_secs(1),
                runner.call_with_host(action, std::future::pending()),
            )
            .await
            .unwrap()
            .err()
            .unwrap();
            assert_eq!(
                error.code,
                match action {
                    "ui_snapshot" => "INSPECTION_FAILED",
                    "hello" => "RUNNER_EXITED",
                    _ => "OUTCOME_UNKNOWN",
                }
            );
            assert_eq!(error.details.unwrap()["cause"], "RUNNER_EXITED");
            exit.await.unwrap();
        }
    }
}

#[cfg(unix)]
fn interrupted(action: &str, message: &str) -> Error {
    Error::new(
        if action == "ui_snapshot" {
            "INSPECTION_FAILED"
        } else {
            "OUTCOME_UNKNOWN"
        },
        message,
    )
}

#[cfg(unix)]
fn expired(action: &str) -> Error {
    if action == "ui_snapshot" {
        Error::new(
            "INSPECTION_TIMEOUT",
            "Inspection deadline expired; the runner may still be busy. Observe/status or disconnect without replaying input",
        )
    } else {
        Error::new(
            "OUTCOME_UNKNOWN",
            "Owned runner deadline expired after dispatch",
        )
    }
}
#[cfg(unix)]
pub(super) fn rpc_timings(elapsed: Duration, runner: &Value) -> Value {
    let total = elapsed.as_secs_f64() * 1000.0;
    let runner = runner.as_f64().filter(|ms| ms.is_finite() && *ms >= 0.0);
    json!({
        "roundTrip":total,
        "runner":runner,
        "hostAndTransport":runner.map(|ms|(total-ms).max(0.0))
    })
}
