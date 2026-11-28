//! Typed capture requests; shared native framing lives in simulator::ipc.
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Command {
    Screenshot {
        id: String,
    },
    Compact {
        id: String,
    },
    Geometry {
        id: String,
    },
    Profiles {
        jpeg: bool,
        h264: bool,
    },
    Keyframe,
    Record {
        id: String,
        path: PathBuf,
        duration_ms: u64,
    },
    StopRecording {
        id: String,
    },
    Close,
}
