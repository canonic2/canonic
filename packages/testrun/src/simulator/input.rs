//! Controller-owned bounded input transport and cancellation-safe release/reaping.
use super::{cleanup::Cleanup, incarnation::Incarnation, input::protocol::Action};
use crate::error::{Error, Result};
use serde_json::json;
use std::{sync::Arc, time::Duration};
use tokio::sync::{mpsc, oneshot, watch};

pub(crate) mod protocol;
mod transport;
use transport::Factory;

struct Request {
    action: Action,
    geometry: [u32; 2],
    timeout: Duration,
    reply: oneshot::Sender<Result<()>>,
}
pub(crate) struct Input {
    requests: mpsc::Sender<Request>,
    ready: watch::Receiver<Option<Result<()>>>,
    cleanup: Arc<Cleanup>,
}

impl Input {
    pub(crate) async fn start(uuid: &str, identity: Incarnation) -> Result<Arc<Self>> {
        let developer = crate::native::developer_directory().await?;
        let factory = Factory {
            program: std::env::current_exe().map_err(|_| unavailable())?,
            args: vec![
                "native-input".into(),
                "--developer".into(),
                developer,
                "--id".into(),
                uuid.into(),
                "--identity".into(),
                serde_json::to_string(&identity).map_err(|_| unavailable())?,
            ],
        };
        Ok(Self::launch(factory, identity))
    }
    fn launch(factory: Factory, identity: Incarnation) -> Arc<Self> {
        let (sender, receiver) = mpsc::channel(4);
        let (ready, readiness) = watch::channel(None);
        let (stop, stopped) = watch::channel(false);
        let job = tokio::spawn(run(factory, identity, receiver, ready, stopped));
        Arc::new(Self {
            requests: sender,
            ready: readiness,
            cleanup: Cleanup::new(stop, job),
        })
    }
    pub(crate) async fn ready(&self) -> Result<()> {
        let mut ready = self.ready.clone();
        loop {
            if let Some(result) = ready.borrow().clone() {
                return result;
            }
            ready.changed().await.map_err(|_| unavailable())?;
        }
    }
    pub(crate) async fn perform(
        &self,
        action: Action,
        geometry: [u32; 2],
        timeout: Duration,
    ) -> Result<()> {
        action.validate()?;
        self.ready().await?;
        let (reply, receiver) = oneshot::channel();
        self.requests
            .try_send(Request {
                action,
                geometry,
                timeout,
                reply,
            })
            .map_err(|_| unavailable())?;
        receiver
            .await
            .map_err(|_| Error::new("OUTCOME_UNKNOWN", "Simulator input stopped before replying"))?
    }
    pub(crate) async fn close(&self) -> Result<()> {
        self.cleanup.close().await
    }
}
impl Drop for Input {
    fn drop(&mut self) {
        self.cleanup.stop();
    }
}

async fn recover(factory: &Factory, identity: &Incarnation, release: Action) -> Result<()> {
    // Never release against a replacement incarnation.
    if identity.retired() {
        return Ok(());
    }
    identity.validate().map_err(|_| cleanup())?;
    let mut transport = factory.open().await.map_err(|_| cleanup())?;
    let released =
        tokio::time::timeout(Duration::from_secs(5), transport.request(release, None)).await;
    let closed = transport.close().await;
    if !matches!(released, Ok(Ok(()))) {
        return Err(cleanup());
    }
    closed
}
async fn run(
    factory: Factory,
    identity: Incarnation,
    mut requests: mpsc::Receiver<Request>,
    ready: watch::Sender<Option<Result<()>>>,
    mut stop: watch::Receiver<bool>,
) -> Result<()> {
    let opened = factory.open().await;
    ready.send_replace(Some(opened.as_ref().map(|_| ()).map_err(Clone::clone)));
    let mut transport = match opened {
        Ok(t) => t,
        Err(error) if error.code == "CLEANUP_FAILED" => return Err(error),
        Err(_) => return Ok(()),
    };
    loop {
        let request = tokio::select! {biased; _=stop.changed()=>break, request=requests.recv()=>match request {Some(r)=>r,None=>break}};
        let Request {
            action,
            geometry,
            timeout,
            mut reply,
        } = request;
        if reply.is_closed() {
            continue;
        }
        let result = tokio::select! {
            biased;
            _=stop.changed()=>Err(unknown()),
            _=reply.closed()=>Err(unknown()),
            result=tokio::time::timeout(timeout,transport.request(action.clone(),Some(geometry)))=>result.unwrap_or_else(|_|Err(unknown())),
        };
        if result.as_ref().is_err_and(|e| e.code == "OUTCOME_UNKNOWN") {
            transport.terminate().await?;
            let release = recover(&factory, &identity, action.release()).await;
            let cause = result.expect_err("Unknown input failure is retained before cleanup");
            let error =
                unknown().details(json!({"cause":cause,"cleanupError":release.as_ref().err()}));
            let _ = reply.send(Err(error));
            return release;
        }
        let _ = reply.send(result);
    }
    transport.close().await
}
fn unavailable() -> Error {
    Error::new(
        "INPUT_UNAVAILABLE",
        "Simulator controller input is unavailable",
    )
}
fn unknown() -> Error {
    Error::new(
        "OUTCOME_UNKNOWN",
        "Simulator input may have run; inspect status and reacquire control without replay",
    )
}
fn cleanup() -> Error {
    Error::new(
        "CLEANUP_FAILED",
        "Simulator owned input release or reaping could not be verified",
    )
}

#[cfg(test)]
mod tests;
