//! Queues optional frame diagnostics without doing file I/O on the dispatcher.
use super::{Direction, stdio};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const CAPACITY: usize = 8;
type Frame = (&'static str, Vec<u8>, Instant);

struct FrameLog {
    sender: Option<mpsc::SyncSender<Frame>>,
    worker: Option<JoinHandle<()>>,
    done: Mutex<mpsc::Receiver<()>>,
    dropped: AtomicU64,
}

impl FrameLog {
    fn start(log: Arc<crate::diagnostics::Log>) -> std::io::Result<Self> {
        let (sender, frames) = mpsc::sync_channel::<Frame>(CAPACITY);
        let (finished, done) = mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("shield-frame-log".into())
            .spawn(move || {
                while let Ok((event, bytes, observed)) = frames.recv() {
                    log.mcp_frame_at(event, &bytes, observed);
                }
                let _ = finished.send(());
            })?;
        Ok(Self {
            sender: Some(sender),
            worker: Some(worker),
            done: Mutex::new(done),
            dropped: AtomicU64::new(0),
        })
    }

    fn submit(&self, direction: Direction, bytes: &[u8]) -> bool {
        let event = match direction {
            Direction::Input => "mcp_input",
            Direction::Output => "mcp_output",
        };
        let accepted = self.sender.as_ref().is_some_and(|sender| {
            sender
                .try_send((event, bytes.to_vec(), Instant::now()))
                .is_ok()
        });
        if !accepted {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
        accepted
    }
}

impl Drop for FrameLog {
    fn drop(&mut self) {
        self.sender.take();
        if self
            .done
            .get_mut()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .recv_timeout(Duration::from_millis(5))
            .is_ok()
        {
            if let Some(worker) = self.worker.take() {
                let _ = worker.join();
            }
        } else {
            eprintln!("Shield: frame trace drain unfinished; diagnostic frames may be missing.");
        }
        let dropped = self.dropped.load(Ordering::Relaxed);
        if dropped != 0 {
            eprintln!("Shield: {dropped} diagnostic frames dropped.");
        }
    }
}

pub(super) fn observer(log: Arc<crate::diagnostics::Log>) -> Option<stdio::Observer> {
    log.path()?;
    let Ok(frames) = FrameLog::start(log) else {
        eprintln!("Shield: frame diagnostics unavailable; continuing without frame logging.");
        return None;
    };
    Some(Arc::new(move |direction, bytes| {
        frames.submit(direction, bytes);
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser as _;

    #[test]
    fn a_busy_log_drops_excess_frames_and_delivers_accepted_metadata_while_running() {
        let root = assert_fs::TempDir::new().unwrap();
        let cli = crate::cli::Cli::parse_from([
            "shield",
            "--debug-log-dir",
            root.path().to_str().unwrap(),
            "status",
        ]);
        let log = Arc::new(crate::diagnostics::Log::open(&cli));
        let frames = FrameLog::start(log.clone()).unwrap();
        // The writer cannot progress until released, so the bounded queue must
        // overflow without waiting for the writer.
        let guard = log.hold_writer();
        let mut accepted = 0;
        for _ in 0..CAPACITY + 2 {
            accepted += usize::from(frames.submit(
                Direction::Input,
                br#"{"id":7,"method":"tools/call","params":{"name":"status","secret":"private-payload"}}"#,
            ));
        }
        assert!(accepted <= CAPACITY + 1);
        drop(guard);
        // Bounded shutdown may detach a delayed writer. Establish live delivery
        // before closing the observer; the stalled-writer test owns shutdown.
        let deadline = Instant::now() + Duration::from_secs(1);
        let (text, records) = loop {
            let text = std::fs::read_to_string(log.path().unwrap()).unwrap();
            let records: Vec<serde_json::Value> = text
                .lines()
                .filter_map(|line| serde_json::from_str(line).ok())
                .collect();
            if records
                .iter()
                .filter(|record| record["event"] == "mcp_input")
                .count()
                == accepted
            {
                break (text, records);
            }
            assert!(
                Instant::now() < deadline,
                "accepted frame metadata was not delivered"
            );
            thread::yield_now();
        };
        drop(frames);
        assert_eq!(
            records
                .iter()
                .filter(|record| record["event"] == "mcp_input")
                .count(),
            accepted
        );
        assert!(text.contains(r#""tool":"status""#), "{text}");
        assert!(!text.contains("private-payload"));
    }

    #[test]
    fn disabled_logging_does_not_create_a_frame_worker() {
        assert!(observer(Arc::new(crate::diagnostics::Log::disabled())).is_none());
    }

    #[test]
    fn stalled_frame_writer_cannot_keep_shutdown_waiting() {
        let root = assert_fs::TempDir::new().unwrap();
        let cli = crate::cli::Cli::parse_from([
            "shield",
            "--debug-log-dir",
            root.path().to_str().unwrap(),
            "status",
        ]);
        let log = Arc::new(crate::diagnostics::Log::open(&cli));
        let frames = FrameLog::start(log.clone()).unwrap();
        let held = log.hold_writer();
        assert!(frames.submit(Direction::Input, br#"{"id":1,"method":"ping"}"#));
        let started = Instant::now();
        drop(frames);
        assert!(started.elapsed() < Duration::from_secs(1));
        drop(held);
    }
}
