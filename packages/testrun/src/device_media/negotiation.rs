//! Typed video-only negotiation with independent call/session identities.
use crate::error::{Error, Result};
use std::io::Write;
fn varint(mut n: u64) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let b = (n & 127) as u8;
        n >>= 7;
        out.push(b | if n > 0 { 128 } else { 0 });
        if n == 0 {
            return out;
        }
    }
}
fn integer(out: &mut Vec<u8>, field: u64, n: u64) {
    out.extend(varint(field << 3));
    out.extend(varint(n));
}
fn bytes(out: &mut Vec<u8>, field: u64, value: &[u8]) {
    out.extend(varint(field << 3 | 2));
    out.extend(varint(value.len() as u64));
    out.extend(value);
}
fn bank(pt: u64, features: &str, kind: u64, count: u64) -> Vec<u8> {
    let mut out = Vec::new();
    integer(&mut out, 1, pt);
    for index in 0..count {
        let mut res = Vec::new();
        for (f, v) in [(1, 1), (2, 1 + index % 2), (3, 50115), (4, 0)] {
            integer(&mut res, f, v);
        }
        bytes(&mut out, 2, &res);
    }
    bytes(&mut out, 3, features.as_bytes());
    integer(&mut out, 4, kind);
    out
}
pub fn offer(ssrc: u32) -> Result<Vec<u8>> {
    let mut settings = Vec::new();
    integer(&mut settings, 1, u64::from(ssrc));
    integer(&mut settings, 2, 0);
    bytes(&mut settings, 3, &bank(123, "FLS;SW:1;", 1, 4));
    bytes(&mut settings, 3, &bank(100, "FLS;SW:1;", 14, 2));
    for (f, v) in [(7, 0), (8, 63), (10, 1), (12, 1)] {
        integer(&mut settings, f, v);
    }
    let mut blob = Vec::new();
    integer(&mut blob, 1, 1);
    integer(&mut blob, 2, 1);
    bytes(&mut blob, 5, &settings);
    bytes(&mut blob, 6, b"Viceroy 1.7.0");
    integer(&mut blob, 8, 0);
    for (a, b, c) in [
        (4074, 0, Some(16384)),
        (0, 75000000, Some(524288)),
        (0, 40000000, Some(12288)),
        (16, 4100, None),
        (0, 20000000, Some(98304)),
        (4, 6500, None),
        (0, 6000000, Some(131072)),
        (0, 100000000, Some(1048576)),
        (0, 60000000, Some(262144)),
        (1, 299, None),
    ] {
        let mut tier = Vec::new();
        integer(&mut tier, 1, a);
        integer(&mut tier, 2, b);
        if let Some(n) = c {
            integer(&mut tier, 3, n);
        }
        bytes(&mut blob, 9, &tier);
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| Error::new("INTERNAL", "Invalid host time"))?;
    let ntp = ((now.as_secs() + 2_208_988_800) << 32)
        | ((u64::from(now.subsec_nanos()) << 32) / 1_000_000_000);
    for (f, v) in [(13, ntp), (14, 2), (16, 0), (18, 1)] {
        integer(&mut blob, f, v);
    }
    let mut compressed = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
    compressed
        .write_all(&blob)
        .map_err(|_| Error::new("INTERNAL", "Cannot encode video offer"))?;
    let compressed = compressed
        .finish()
        .map_err(|_| Error::new("INTERNAL", "Cannot finalize video offer"))?;
    let mut endpoint = Vec::new();
    integer(&mut endpoint, 1, 0);
    integer(&mut endpoint, 2, 1);
    // Decoder compatibility identity; actual host identity is reported separately.
    for (f, text) in [(3, "Mac15,9"), (4, "2205.3.1"), (5, "25F80")] {
        bytes(&mut endpoint, f, text.as_bytes());
    }
    let mut plist = plist::Dictionary::new();
    plist.insert(
        "avcMediaStreamOptionRemoteEndpointInfo".into(),
        plist::Value::Data(endpoint),
    );
    plist.insert(
        "avcMediaStreamNegotiatorMode".into(),
        plist::Value::Integer(5.into()),
    );
    plist.insert(
        "avcMediaStreamNegotiatorMediaBlob".into(),
        plist::Value::Data(compressed),
    );
    plist.insert(
        "avcMediaStreamOptionCallID".into(),
        plist::Value::String(uuid::Uuid::new_v4().to_string().to_uppercase()),
    );
    let mut out = Vec::new();
    plist::Value::Dictionary(plist)
        .to_writer_binary(&mut out)
        .map_err(|_| Error::new("INTERNAL", "Cannot serialize video offer"))?;
    Ok(out)
}
