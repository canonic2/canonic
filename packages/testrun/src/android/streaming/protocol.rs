//! Bounded versioned video packets and acquisition fences, independent of I/O.
use crate::error::{Error, Result};
pub(crate) const MAX: usize = 8 * 1024 * 1024;
pub(crate) const HEADER: usize = 32;
pub(crate) const MAGIC: u32 = 0x54525631;

#[derive(Debug)]
pub(crate) struct Packet {
    pub(crate) kind: u32,
    pub(crate) request: u64,
    pub(crate) sequence: u64,
    pub(crate) payload: Vec<u8>,
}
pub(crate) fn packet(buffer: &mut Vec<u8>) -> Result<Option<Packet>> {
    if buffer.len() < HEADER {
        return Ok(None);
    }
    let word = |at| u32::from_be_bytes(buffer[at..at + 4].try_into().unwrap());
    if word(0) != MAGIC {
        return Err(Error::new(
            "PLATFORM_PROTOCOL",
            "Invalid Android video protocol",
        ));
    }
    let kind = word(4);
    let length = word(8) as usize;
    if length > MAX || word(12) != 0 || !(1..=4).contains(&kind) {
        return Err(Error::new(
            "PLATFORM_PROTOCOL",
            "Invalid Android video packet bound or type",
        ));
    }
    if buffer.len() < HEADER + length {
        return Ok(None);
    }
    let request = u64::from_be_bytes(buffer[16..24].try_into().unwrap());
    let sequence = u64::from_be_bytes(buffer[24..32].try_into().unwrap());
    let payload = buffer[HEADER..HEADER + length].to_vec();
    buffer.drain(..HEADER + length);
    Ok(Some(Packet {
        kind,
        request,
        sequence,
        payload,
    }))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Fence {
    pub(crate) generation: u64,
    pub(crate) request: u64,
    pub(crate) sequence: u64,
}
impl Fence {
    pub(crate) fn admits(self, generation: u64, packet: &Packet) -> bool {
        generation == self.generation
            && packet.kind == 2
            && packet.request == self.request
            && packet.sequence > self.sequence
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn wire(kind: u32, length: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        for word in [MAGIC, kind, length, 0] {
            bytes.extend(word.to_be_bytes());
        }
        bytes.extend(7u64.to_be_bytes());
        bytes.extend(12u64.to_be_bytes());
        bytes
    }
    #[test]
    fn fragmented_packets_wait_for_their_complete_declared_payload() {
        let mut bytes = wire(2, 3);
        bytes.extend([0, 1]);
        assert!(packet(&mut bytes).unwrap().is_none());
        bytes.push(2);
        let frame = packet(&mut bytes).unwrap().unwrap();
        assert_eq!(frame.payload, [0, 1, 2]);
        assert!(bytes.is_empty());
        assert!(packet(&mut wire(2, MAX as u32 + 1)).is_err());
        assert!(packet(&mut wire(9, 0)).is_err());
    }
    #[test]
    fn only_post_acquisition_frames_of_the_current_generation_and_request_qualify() {
        let fence = Fence {
            generation: 4,
            request: 7,
            sequence: 11,
        };
        let mut frame = packet(&mut wire(2, 0)).unwrap().unwrap();
        assert!(fence.admits(4, &frame));
        assert!(!fence.admits(3, &frame));
        frame.request = 6;
        assert!(!fence.admits(4, &frame));
        frame.request = 7;
        frame.sequence = 11;
        assert!(!fence.admits(4, &frame));
    }
}
