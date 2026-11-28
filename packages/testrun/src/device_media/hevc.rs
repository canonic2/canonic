//! Bounded RTP access-unit assembly; damaged references recover at a keyframe.
use idevice::core_device::RtpPacket;
const MAX: usize = 1024 * 1024;
pub struct AccessUnit {
    pub payload: Vec<u8>,
    pub key: bool,
    pub timestamp: u32,
}
#[derive(Default)]
pub struct Assembler {
    seq: Option<u16>,
    timestamp: Option<u32>,
    fragment: Vec<u8>,
    nals: Vec<Vec<u8>>,
    size: usize,
    corrupt: bool,
    waiting: bool,
    pub dropped: u64,
    pub parameters: [Vec<u8>; 3],
}
impl Assembler {
    pub fn new() -> Self {
        Self {
            waiting: true,
            ..Self::default()
        }
    }
    #[expect(
        clippy::too_many_lines,
        reason = "Packet validation and access-unit assembly share corruption state"
    )]
    pub fn push(&mut self, packet: &RtpPacket<'_>) -> Option<AccessUnit> {
        if let Some(seq) = self.seq {
            let delta = packet.sequence_number.wrapping_sub(seq);
            if delta == 0 || delta > 32768 {
                return None;
            }
            if delta != 1 {
                self.corrupt = true;
                self.fragment.clear();
                self.waiting = true;
            }
        }
        self.seq = Some(packet.sequence_number);
        if self.timestamp.is_some_and(|ts| ts != packet.timestamp) {
            if !self.nals.is_empty() {
                self.dropped += 1;
                self.waiting = true;
            }
            self.clear();
        }
        self.timestamp = Some(packet.timestamp);
        let p = packet.payload;
        if p.len() < 2 {
            self.corrupt = true;
        } else {
            match (p[0] >> 1) & 63 {
                48 => {
                    let mut offset = 2;
                    while offset + 2 <= p.len() {
                        let length = u16::from_be_bytes([p[offset], p[offset + 1]]) as usize;
                        offset += 2;
                        if length < 2 || offset + length > p.len() {
                            self.corrupt = true;
                            break;
                        }
                        self.nal(&p[offset..offset + length]);
                        offset += length;
                    }
                    if offset != p.len() {
                        self.corrupt = true;
                    }
                }
                49 => {
                    if p.len() < 3 {
                        self.corrupt = true;
                    } else {
                        let start = p[2] & 128 != 0;
                        let end = p[2] & 64 != 0;
                        if start {
                            if !self.fragment.is_empty() {
                                self.corrupt = true;
                            }
                            self.fragment = vec![(p[0] & 129) | ((p[2] & 63) << 1), p[1]];
                        }
                        if self.fragment.is_empty() {
                            self.corrupt = true;
                        } else if self.fragment.len() + p.len() - 3 > MAX {
                            self.corrupt = true;
                            self.fragment.clear();
                        } else {
                            self.fragment.extend_from_slice(&p[3..]);
                            if end {
                                let nal = std::mem::take(&mut self.fragment);
                                self.nal(&nal);
                            }
                        }
                    }
                }
                0..=47 => self.nal(p),
                _ => self.corrupt = true,
            }
        }
        if !packet.marker {
            return None;
        }
        let key = self
            .nals
            .iter()
            .any(|n| (16..=23).contains(&((n[0] >> 1) & 63)));
        let vcl = self.nals.iter().any(|n| ((n[0] >> 1) & 63) <= 31);
        let valid = !self.corrupt
            && self.fragment.is_empty()
            && vcl
            && (!self.waiting || key)
            && self.parameters.iter().all(|p| !p.is_empty());
        let output = if valid {
            self.waiting = false;
            let mut payload = Vec::with_capacity(self.size);
            for nal in &self.nals {
                payload.extend_from_slice(&(nal.len() as u32).to_be_bytes());
                payload.extend(nal);
            }
            Some(AccessUnit {
                payload,
                key,
                timestamp: packet.timestamp,
            })
        } else {
            self.dropped += 1;
            self.waiting = true;
            None
        };
        self.clear();
        output
    }
    fn nal(&mut self, nal: &[u8]) {
        if nal.len() < 2 || self.size + nal.len() + 4 > MAX {
            self.corrupt = true;
            return;
        }
        let kind = (nal[0] >> 1) & 63;
        if (32..=34).contains(&kind) {
            if nal.len() > 65535 {
                self.corrupt = true;
                return;
            }
            self.parameters[(kind - 32) as usize] = nal.to_vec();
        }
        self.size += nal.len() + 4;
        self.nals.push(nal.to_vec());
    }
    fn clear(&mut self) {
        self.nals.clear();
        self.fragment.clear();
        self.size = 0;
        self.corrupt = false;
        self.timestamp = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn packet(seq: u16, ts: u32, marker: bool, payload: &[u8]) -> Vec<u8> {
        let mut bytes = vec![0x80, 100 | if marker { 128 } else { 0 }];
        bytes.extend(seq.to_be_bytes());
        bytes.extend(ts.to_be_bytes());
        bytes.extend(7u32.to_be_bytes());
        bytes.extend(payload);
        bytes
    }
    fn push(
        a: &mut Assembler,
        seq: u16,
        ts: u32,
        marker: bool,
        payload: &[u8],
    ) -> Option<AccessUnit> {
        a.push(&RtpPacket::parse(&packet(seq, ts, marker, payload)).unwrap())
    }
    fn parameters(a: &mut Assembler) {
        for (i, t) in [32, 33, 34].into_iter().enumerate() {
            push(a, i as u16, 0, false, &[t << 1, 1, 2]);
        }
    }
    #[test]
    fn incomplete_fragments_and_lost_references_wait_for_a_keyframe() {
        let mut a = Assembler::new();
        parameters(&mut a);
        let frame = push(&mut a, 3, 0, true, &[19 << 1, 1, 4]).unwrap();
        assert!(frame.key);
        assert_eq!(
            &frame.payload[frame.payload.len() - 7..],
            &[0, 0, 0, 3, 38, 1, 4]
        );
        assert!(push(&mut a, 4, 400, false, &[49 << 1, 1, 128 | 1, 5]).is_none());
        assert!(push(&mut a, 6, 400, true, &[49 << 1, 1, 64 | 1, 6]).is_none());
        assert!(push(&mut a, 7, 800, true, &[2, 1, 7]).is_none());
        assert!(push(&mut a, 8, 1200, true, &[19 << 1, 1, 8]).unwrap().key);
        assert!(push(&mut a, 9, 1600, true, &[2, 1, 9]).is_some());
        assert_eq!(a.dropped, 2);
    }
    #[test]
    fn sequence_wrap_and_malformed_aggregation_do_not_emit_corrupt_frames() {
        let mut a = Assembler::new();
        parameters(&mut a);
        a.seq = Some(65534);
        assert!(push(&mut a, 65535, 0, true, &[38, 1, 4]).is_some());
        assert!(push(&mut a, 0, 400, true, &[2, 1, 9]).is_some());
        assert!(push(&mut a, 1, 800, true, &[96, 1, 0, 20, 2, 1]).is_none());
        assert!(push(&mut a, 2, 1200, true, &[2, 1, 9]).is_none());
        assert!(push(&mut a, 3, 1600, true, &[38, 1, 4]).is_some());
        assert!(push(&mut a, 2, 1200, true, &[2, 1, 9]).is_none());
    }
    #[test]
    fn overlarge_access_units_are_discarded() {
        let mut a = Assembler::new();
        parameters(&mut a);
        assert!(push(&mut a, 3, 0, true, &[38, 1, 4]).is_some());
        let mut large = vec![0; MAX];
        large[0] = 2;
        large[1] = 1;
        assert!(push(&mut a, 4, 400, true, &large).is_none());
        assert!(push(&mut a, 5, 800, true, &[2, 1, 4]).is_none());
    }
}
