//! Drain diagnostics continuously, retaining only live inputs and one requested sample.
use canonic_mcp_kit::stdio::{FrameKind, Observation, ObservationKind as Kind, Settlement, Stage};
use serde_json::Value;
use std::{collections::HashMap, sync::mpsc, thread};

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(super) struct Shape {
    pub(super) handlers: usize,
    encodes: usize,
    publications: usize,
    pub(super) interactions: usize,
    pub(super) frames: usize,
    pub(super) settlement: Option<Settlement>,
}

#[derive(Default)]
struct Live {
    id: Option<Value>,
    shape: Shape,
}

pub(super) struct Summary {
    pub(super) released: usize,
    pub(super) peak: usize,
}

pub(super) fn start(
    records: mpsc::Receiver<Observation>,
) -> (mpsc::Receiver<Shape>, thread::JoinHandle<Summary>) {
    let (sender, samples) = mpsc::sync_channel(1);
    let collector = thread::spawn(move || {
        let mut live = HashMap::<u64, Live>::new();
        let mut summary = Summary {
            released: 0,
            peak: 0,
        };
        for record in records {
            if record.request == 0 {
                continue;
            }
            let input = live.entry(record.request).or_default();
            if let Some(id) = record.rpc_id {
                input.id = Some((*id).clone());
            }
            match record.kind {
                Kind::Start(Stage::Handler) => input.shape.handlers += 1,
                Kind::Start(Stage::Encode) => input.shape.encodes += 1,
                Kind::Start(Stage::Publication) => input.shape.publications += 1,
                Kind::Start(Stage::Interaction) => input.shape.interactions += 1,
                Kind::Frame { kind, .. } if kind != FrameKind::Input => input.shape.frames += 1,
                Kind::Settled(why) => {
                    assert!(
                        input.shape.settlement.replace(why).is_none(),
                        "duplicate settlement"
                    );
                }
                Kind::Released => {
                    let input = live
                        .remove(&record.request)
                        .expect("live observation owner");
                    assert!(
                        input.shape.settlement.is_some(),
                        "released without settlement"
                    );
                    summary.released += 1;
                    if input
                        .id
                        .as_ref()
                        .and_then(Value::as_str)
                        .is_some_and(|id| id.starts_with("sample-"))
                    {
                        sender.send(input.shape).expect("sample consumer");
                    }
                }
                _ => {}
            }
            summary.peak = summary.peak.max(live.len());
            // Default lane capacity, input buffer, dispatcher and reader handoff.
            assert!(live.len() <= 8 + canonic_mcp_kit::stdio::INPUT_BUFFER + 2);
        }
        assert!(
            live.is_empty(),
            "completed inputs retained after driver drain"
        );
        summary
    });
    (samples, collector)
}
