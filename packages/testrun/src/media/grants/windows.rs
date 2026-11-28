//! Same-user, bounded private media-grant exchanges on Windows.
use super::{REQUEST_LIMIT, Shared, framed_reply};
use crate::{
    error::{Error, Result},
    windows_ipc::{IdleBackoff, ScratchRoot},
};
use canonic_process_kit::{endpoint, frame::Channel, net::Listener};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use tokio::sync::oneshot;

const MAX_PEERS: usize = 8;
const POLL_INTERVAL: Duration = Duration::from_millis(10);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(3);

pub(in crate::media) struct Broker {
    root: ScratchRoot,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<Result<()>>>,
}

impl Broker {
    pub(in crate::media) async fn start(state: Shared) -> Result<Self> {
        let root = ScratchRoot::create().map_err(unavailable)?;
        let lease = root.acquire().map_err(unavailable)?;
        let mut broker = Self {
            root,
            stop: Arc::new(AtomicBool::new(false)),
            worker: None,
        };
        let (ready, bound) = oneshot::channel();
        let stop = broker.stop.clone();
        broker.worker = Some(
            thread::Builder::new()
                .name("testrun-media-grants".into())
                .spawn(move || {
                    let listener = lease.bind(false).map_err(unavailable)?;
                    if ready.send(()).is_ok() {
                        serve(listener.listener(), &state, &stop);
                    }
                    listener.unlink().map_err(cleanup)
                })
                .map_err(unavailable)?,
        );
        bound.await.map_err(unavailable)?;
        Ok(broker)
    }

    pub(in crate::media) fn path(&self) -> PathBuf {
        self.root.path()
    }

    pub(in crate::media) fn close(&mut self) -> Result<()> {
        self.stop.store(true, Ordering::Release);
        let stopped = self.worker.take().map_or(Ok(()), |worker| {
            worker.thread().unpark();
            worker.join().map_err(cleanup)?
        });
        let removed = self.root.close().map_err(cleanup);
        stopped.and(removed)
    }
}

impl Drop for Broker {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

fn serve(listener: &Listener, state: &Shared, stop: &AtomicBool) {
    let mut peers = Vec::with_capacity(MAX_PEERS);
    let mut idle = IdleBackoff::default();
    while !stop.load(Ordering::Acquire) {
        for _ in peers.len()..MAX_PEERS {
            let Ok((socket, _)) = listener.accept() else {
                break;
            };
            if endpoint::check_peer(&socket).is_ok()
                && let Ok(channel) = Channel::new(socket, REQUEST_LIMIT)
            {
                peers.push((channel, Instant::now() + REQUEST_TIMEOUT));
            }
        }
        peers.retain_mut(|(channel, deadline)| {
            if Instant::now() >= *deadline {
                return false;
            }
            match channel.receive() {
                Ok(None) => true,
                Ok(Some(bytes)) => {
                    let response = framed_reply(state, &bytes);
                    let until = (*deadline).min(Instant::now() + POLL_INTERVAL);
                    let _ = channel.send(response.to_string().as_bytes(), until);
                    false
                }
                Err(_) => false,
            }
        });
        if peers.is_empty() {
            thread::park_timeout(idle.next_delay());
        } else {
            idle.reset();
            thread::park_timeout(POLL_INTERVAL);
        }
    }
}

fn unavailable<T>(_: T) -> Error {
    Error::new(
        "MEDIA_UNAVAILABLE",
        "Cannot start the private media grant broker",
    )
}

fn cleanup<T>(_: T) -> Error {
    Error::new(
        "CLEANUP_FAILED",
        "Cannot release the private media grant broker",
    )
}
