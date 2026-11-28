//! Classic PCAP packet-header import; no capture, reassembly, payload decoding or decryption.
use super::*;
use std::net::{Ipv4Addr, Ipv6Addr};
fn be16(bytes: &[u8], at: usize) -> Result<u16> {
    Ok(u16::from_be_bytes(
        bytes
            .get(at..at + 2)
            .ok_or_else(invalid)?
            .try_into()
            .unwrap(),
    ))
}
pub(super) fn decode(bytes: &[u8], index: &mut Index) -> Result<()> {
    let (little, nanos) = match bytes.get(..4) {
        Some([0xd4, 0xc3, 0xb2, 0xa1]) => (true, false),
        Some([0xa1, 0xb2, 0xc3, 0xd4]) => (false, false),
        Some([0x4d, 0x3c, 0xb2, 0xa1]) => (true, true),
        Some([0xa1, 0xb2, 0x3c, 0x4d]) => (false, true),
        _ => {
            return Err(Error::new(
                "UNSUPPORTED_PACKET_FORMAT",
                "Select classic PCAP; pcapng and proprietary captures are not decoded",
            ));
        }
    };
    let word = |at| -> Result<u32> {
        let b: [u8; 4] = bytes
            .get(at..at + 4)
            .ok_or_else(invalid)?
            .try_into()
            .unwrap();
        Ok(if little {
            u32::from_le_bytes(b)
        } else {
            u32::from_be_bytes(b)
        })
    };
    let short = |at| -> Result<u16> {
        let b: [u8; 2] = bytes
            .get(at..at + 2)
            .ok_or_else(invalid)?
            .try_into()
            .unwrap();
        Ok(if little {
            u16::from_le_bytes(b)
        } else {
            u16::from_be_bytes(b)
        })
    };
    if short(4)? != 2 || short(6)? != 4 || word(16)? == 0 || word(16)? > 1048576 {
        return Err(invalid());
    }
    let link = word(20)?;
    if !matches!(link, 1 | 101 | 228 | 229) {
        return Err(Error::new(
            "UNSUPPORTED_PACKET_LINK",
            "Only Ethernet/raw IPv4/IPv6 classic PCAP without extended link flags is supported",
        ));
    }
    let mut at = 24usize;
    let mut ordinal = 0;
    while at < bytes.len() {
        if ordinal >= 4096 {
            return Err(Error::new(
                "ANALYSIS_BOUNDS",
                "PCAP exceeds 4096 packet records",
            ));
        }
        let start = at;
        let seconds = word(at)?;
        let fraction = word(at + 4)?;
        let captured = word(at + 8)?;
        let original = word(at + 12)?;
        if captured > word(16)?
            || captured > original
            || fraction >= if nanos { 1_000_000_000 } else { 1_000_000 }
        {
            return Err(invalid());
        }
        at = at.checked_add(16).ok_or_else(invalid)?;
        let end = at.checked_add(captured as usize).ok_or_else(invalid)?;
        let packet = bytes.get(at..end).ok_or_else(invalid)?;
        let mut n = headers(link, packet)?;
        n["category"] = json!("packets");
        n["id"] = json!(format!("packet:{ordinal}"));
        n["timestamp"] = json!({"seconds":seconds.to_string(),"fraction":fraction.to_string(),"fractionUnit":if nanos{"nanoseconds"}else{"microseconds"},"clock":"capture producer epoch; not aligned to application clocks"});
        n["capturedBytes"] = json!(captured);
        n["originalBytes"] = json!(original);
        n["captureTruncated"] = json!(captured < original);
        n["payloadMeaning"] = json!(
            "raw evidence only; no application protocol, body, TLS/QUIC decryption, TCP reassembly, checksum validation, delivery, DNS latency or retry inference"
        );
        emit(
            index,
            "event",
            "Captured packet headers",
            format!("binary:offset:{start}"),
            n,
        )?;
        at = end;
        ordinal += 1;
    }
    index.unsupported.push("PCAP headers are capture observations, not app/process attribution, decrypted requests or wire delivery proof; pcapng, non-IP links and IPv6 extension transport parsing are unavailable".into());
    Ok(())
}
fn headers(link: u32, packet: &[u8]) -> Result<Value> {
    let (ip, ether) = if link == 1 {
        if packet.len() < 14 {
            return Ok(json!({"unavailable":"truncated Ethernet header"}));
        }
        let mut kind = be16(packet, 12)?;
        let mut offset = 14;
        for _ in 0..2 {
            if !matches!(kind, 0x8100 | 0x88a8) {
                break;
            }
            if packet.len() < offset + 4 {
                return Ok(json!({"unavailable":"truncated VLAN header"}));
            }
            kind = be16(packet, offset + 2)?;
            offset += 4;
        }
        if !matches!(kind, 0x0800 | 0x86dd) {
            return Ok(json!({"etherType":kind,"unavailable":"unregistered link payload"}));
        }
        (&packet[offset..], Some(kind))
    } else {
        (packet, None)
    };
    let Some(first) = ip.first() else {
        return Ok(json!({"unavailable":"empty/truncated IP packet"}));
    };
    let (mut n, transport, protocol, fragment) = match first >> 4 {
        4 => {
            if link == 229 || ether == Some(0x86dd) {
                return Err(invalid());
            }
            if ip.len() < 20 {
                return Ok(json!({"ipVersion":4,"unavailable":"truncated IPv4 header"}));
            }
            let header = ((first & 15) as usize) * 4;
            let total = be16(ip, 2)? as usize;
            if header < 20 || total < header {
                return Err(invalid());
            }
            if ip.len() < header {
                return Ok(json!({"ipVersion":4,"unavailable":"truncated IPv4 options"}));
            }
            let frag = be16(ip, 6)?;
            let n = json!({"ipVersion":4,"source":Ipv4Addr::new(ip[12],ip[13],ip[14],ip[15]).to_string(),"destination":Ipv4Addr::new(ip[16],ip[17],ip[18],ip[19]).to_string(),"ipBytes":total,"ipTruncated":ip.len()<total,"fragmented":frag&0x3fff!=0});
            (
                n,
                &ip[header..total.min(ip.len())],
                ip[9],
                frag & 0x3fff != 0,
            )
        }
        6 => {
            if link == 228 || ether == Some(0x0800) {
                return Err(invalid());
            }
            if ip.len() < 40 {
                return Ok(json!({"ipVersion":6,"unavailable":"truncated IPv6 header"}));
            }
            let total = 40 + be16(ip, 4)? as usize;
            let src: [u8; 16] = ip[8..24].try_into().unwrap();
            let dst: [u8; 16] = ip[24..40].try_into().unwrap();
            let n = json!({"ipVersion":6,"source":Ipv6Addr::from(src).to_string(),"destination":Ipv6Addr::from(dst).to_string(),"ipBytes":total,"ipTruncated":ip.len()<total});
            (n, &ip[40..total.min(ip.len())], ip[6], total == 40)
        }
        _ => return Ok(json!({"unavailable":"unregistered IP version"})),
    };
    n["ipProtocol"] = json!(protocol);
    if fragment {
        n["transportUnavailable"] =
            json!("fragmented packet or IPv6 zero payload/jumbogram; no transport guess");
        return Ok(n);
    }
    transport_headers(n, transport, protocol)
}

fn transport_headers(mut n: Value, transport: &[u8], protocol: u8) -> Result<Value> {
    let minimum = match protocol {
        6 => 20,
        17 => 8,
        _ => {
            n["transportUnavailable"] = json!("non TCP/UDP or IPv6 extension header; retained raw");
            return Ok(n);
        }
    };
    if transport.len() < minimum {
        n["transportUnavailable"] = json!("truncated transport header");
        return Ok(n);
    }
    if protocol == 6 {
        let header = ((transport[12] >> 4) as usize) * 4;
        if header < 20 {
            return Err(invalid());
        }
        if header > transport.len() {
            n["transportUnavailable"] = json!("truncated TCP options");
            return Ok(n);
        }
        n["tcpFlags"] = json!(transport[13]);
    }
    if protocol == 17 && be16(transport, 4)? < 8 {
        return Err(invalid());
    }
    n["transport"] = json!(if protocol == 6 { "tcp" } else { "udp" });
    n["sourcePort"] = json!(be16(transport, 0)?);
    n["destinationPort"] = json!(be16(transport, 2)?);
    Ok(n)
}
