//! Queues optional frame diagnostics without doing file I/O on the dispatcher.
use super::{Direction, stdio};
use std::{
    sync::{Arc, mpsc},
    thread::{self, JoinHandle},
};

const CAPACITY: usize = 8;
type Frame = (&'static str, Vec<u8>);

struct FrameLog {
    sender: Option<mpsc::SyncSender<Frame>>,
    worker: Option<JoinHandle<()>>,
}

impl FrameLog {
    fn start(log: Arc<crate::diagnostics::Log>) -> std::io::Result<Self> {
        let (sender, frames) = mpsc::sync_channel::<Frame>(CAPACITY);
        let worker = thread::Builder::new()
            .name("shield-frame-log".into())
            .spawn(move || {
                while let Ok((event, bytes)) = frames.recv() {
                    log.mcp_frame(event, &bytes);
                }
            })?;
        Ok(Self {
            sender: Some(sender),
            worker: Some(worker),
        })
    }

    fn submit(&self, direction: Direction, bytes: &[u8]) -> bool {
        let event = match direction {
            Direction::Input => "mcp_input",
            Direction::Output => "mcp_output",
        };
        self.sender
            .as_ref()
            .is_some_and(|sender| sender.try_send((event, bytes.to_vec())).is_ok())
    }
}

impl Drop for FrameLog {
    fn drop(&mut self) {
        self.sender.take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
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
    fn a_busy_log_drops_excess_frames_and_drains_accepted_metadata_on_shutdown() {
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
        drop(frames);
        let text = std::fs::read_to_string(log.path().unwrap()).unwrap();
        let records: Vec<serde_json::Value> = text
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
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
}
