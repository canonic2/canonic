//! Bounded transport observations published to Shield's private event sink.
use canonic_mcp_kit::stdio::{
    Observation, ObservationKind, ObservationSender, observation_channel,
};
use serde_json::{Value, json};
use std::{
    sync::{Arc, mpsc},
    thread,
    time::Duration,
};

pub(crate) struct Log {
    sender: Option<ObservationSender>,
    worker: Option<thread::JoinHandle<()>>,
    done: mpsc::Receiver<()>,
}

impl Log {
    pub(crate) fn start(log: &Arc<crate::diagnostics::Log>) -> Option<Self> {
        log.path()?;
        let (sender, observations) =
            observation_channel(512).expect("bounded observation capacity");
        let (finished, done) = mpsc::sync_channel(1);
        let log = log.clone();
        let worker = thread::Builder::new()
            .name("shield-mcp-observations".into())
            .spawn(move || {
                for observation in observations {
                    log.event("mcp_transport", || fields(&log, &observation));
                }
                let _ = finished.send(());
            });
        if let Ok(worker) = worker {
            Some(Self {
                sender: Some(sender),
                worker: Some(worker),
                done,
            })
        } else {
            eprintln!("Shield: MCP timing diagnostics unavailable.");
            None
        }
    }

    pub(crate) fn sender(&self) -> ObservationSender {
        self.sender
            .as_ref()
            .expect("live observation sender")
            .clone()
    }
}

impl Drop for Log {
    fn drop(&mut self) {
        let dropped = self.sender.take().map_or(0, |sender| sender.dropped());
        if self.done.recv_timeout(Duration::from_millis(5)).is_ok() {
            if let Some(worker) = self.worker.take() {
                let _ = worker.join();
            }
        } else {
            eprintln!("Shield: MCP timing drain unfinished; observations may be missing.");
        }
        if dropped != 0 {
            eprintln!("Shield: {dropped} MCP timing observations dropped.");
        }
    }
}

fn fields(log: &crate::diagnostics::Log, observation: &Observation) -> Value {
    let mut fields = json!({
        "connection": observation.connection,
        "request": observation.request,
        "assignment": observation.assignment,
        "lane": observation.lane,
        "worker": observation.worker,
        "parent_stage": observation.parent.map(|stage| format!("{stage:?}")),
        "observed_us": log.observed_us(observation.at),
    });
    let transition = match observation.kind {
        ObservationKind::Start(stage) => {
            fields["stage"] = json!(format!("{stage:?}"));
            "start"
        }
        ObservationKind::End(stage) => {
            fields["stage"] = json!(format!("{stage:?}"));
            "end"
        }
        ObservationKind::Frame { kind, bytes } => {
            fields["frame_kind"] = json!(format!("{kind:?}"));
            fields["bytes"] = json!(bytes);
            "frame"
        }
        ObservationKind::Settled(outcome) => {
            fields["settlement"] = json!(format!("{outcome:?}"));
            "settled"
        }
        ObservationKind::Related(request) => {
            fields["related_request"] = json!(request);
            "related"
        }
        ObservationKind::ProtocolId => "protocol_id",
        ObservationKind::Released => "released",
        ObservationKind::Discarded => "discarded",
        ObservationKind::Panicked => "panicked",
        ObservationKind::EncodeFailed => "encode_failed",
        ObservationKind::PublicationFailed => "publication_failed",
        ObservationKind::StreamOpened => "stream_opened",
        ObservationKind::StreamClosed => "stream_closed",
        ObservationKind::ReaderDetached => "reader_detached",
    };
    fields["transition"] = json!(transition);
    fields
}
