//! Pure versioned frame encoding, validation and dependent-frame recovery.
use crate::error::{Error, Result};
use serde_json::Value;

pub(super) const HEADER: usize = 24;
const MAX_PAYLOAD: usize = 16 * 1024 * 1024;
pub fn h264_configuration(access_unit: &[u8]) -> Result<Vec<u8>> {
    let starts: Vec<_> = access_unit
        .windows(4)
        .enumerate()
        .filter_map(|(i, bytes)| (bytes == [0, 0, 0, 1]).then_some(i))
        .collect();
    let mut configuration = Vec::new();
    let mut sps = false;
    let mut pps = false;
    for (index, start) in starts.iter().copied().enumerate() {
        let end = starts.get(index + 1).copied().unwrap_or(access_unit.len());
        if start + 4 >= end {
            continue;
        }
        match access_unit[start + 4] & 0x1f {
            7 => sps = true,
            8 => pps = true,
            _ => continue,
        };
        configuration.extend_from_slice(&access_unit[start..end]);
    }
    if !sps || !pps {
        return Err(Error::new(
            "PLATFORM_PROTOCOL",
            "H.264 keyframe is missing parameter sets",
        ));
    }
    Ok(configuration)
}
#[derive(Clone)]
pub struct Frame {
    pub packet: Vec<u8>,
    pub metadata: Value,
    pub configuration: Option<Vec<u8>>,
    pub received: std::time::Instant,
}
#[derive(Default)]
pub(super) struct Decoder {
    last: Option<u64>,
    waiting_for_keyframe: bool,
    dropped: u64,
}
impl Decoder {
    pub(super) fn waiting_for_keyframe(&self) -> bool {
        self.waiting_for_keyframe
    }
    pub(super) fn require_keyframe(&mut self) {
        self.waiting_for_keyframe = true;
    }
    pub(super) fn dropped_frames(&self) -> u64 {
        self.dropped
    }
    pub(super) fn accept(&mut self, frame: &Frame) -> bool {
        let Some(seq) = frame.metadata["seq"]
            .as_str()
            .and_then(|s| s.parse::<u64>().ok())
        else {
            return false;
        };
        if self.last.is_some_and(|last| seq <= last) {
            return false;
        }
        let dependent = matches!(frame.metadata["codec"].as_str(), Some("h264" | "hevc"));
        if let Some(last) = self.last {
            if seq > last + 1 {
                self.dropped += seq - last - 1;
                if dependent {
                    self.waiting_for_keyframe = true;
                }
            }
        } else if dependent {
            self.waiting_for_keyframe = true;
        }
        self.last = Some(seq);
        if dependent && self.waiting_for_keyframe {
            if frame.packet[7] & 1 == 0 {
                return false;
            }
            self.waiting_for_keyframe = false;
        }
        true
    }
}

pub fn encode(metadata: &Value, payload: &[u8]) -> Result<Vec<u8>> {
    encode_packet(metadata, payload, 1, true)
}
pub fn encode_packet(
    metadata: &Value,
    payload: &[u8],
    kind: u8,
    keyframe: bool,
) -> Result<Vec<u8>> {
    let meta = serde_json::to_vec(metadata)
        .map_err(|_| Error::new("INTERNAL", "Cannot encode frame metadata"))?;
    if meta.len() > 4096 || payload.len() > MAX_PAYLOAD {
        return Err(Error::new(
            "OUTPUT_LIMIT",
            "Frame exceeds media protocol bounds",
        ));
    }
    let mut bytes = Vec::with_capacity(HEADER + meta.len() + payload.len());
    bytes.extend_from_slice(b"SBVF");
    bytes.extend_from_slice(&1u16.to_be_bytes());
    bytes.extend_from_slice(&[kind, u8::from(keyframe)]);
    bytes.extend_from_slice(&24u16.to_be_bytes());
    bytes.extend_from_slice(&0u16.to_be_bytes());
    bytes.extend_from_slice(&(meta.len() as u32).to_be_bytes());
    bytes.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    bytes.extend_from_slice(&0u32.to_be_bytes());
    bytes.extend(meta);
    bytes.extend(payload);
    Ok(bytes)
}
pub fn lengths(header: &[u8; HEADER]) -> Result<(usize, usize)> {
    if &header[..4] != b"SBVF"
        || header[4..6] != 1u16.to_be_bytes()
        || !matches!(header[6], 1 | 2)
        || header[7] & !1 != 0
        || header[8..10] != 24u16.to_be_bytes()
        || header[10..12] != [0, 0]
        || header[20..24] != [0, 0, 0, 0]
    {
        return Err(Error::new(
            "PLATFORM_PROTOCOL",
            "Invalid native frame header",
        ));
    }
    let meta = u32::from_be_bytes(
        header[12..16]
            .try_into()
            .map_err(|_| Error::new("PLATFORM_PROTOCOL", "Invalid metadata length"))?,
    ) as usize;
    let payload = u32::from_be_bytes(
        header[16..20]
            .try_into()
            .map_err(|_| Error::new("PLATFORM_PROTOCOL", "Invalid payload length"))?,
    ) as usize;
    if meta > 4096 || payload > MAX_PAYLOAD {
        return Err(Error::new(
            "OUTPUT_LIMIT",
            "Native frame exceeds protocol bounds",
        ));
    }
    Ok((meta, payload))
}
