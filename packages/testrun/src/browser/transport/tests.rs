//! Deterministic pipe-boundary regressions without a driver process or browser.

use super::*;
use serde_json::json;
use std::{
    pin::Pin,
    task::{Context, Poll},
};
use tokio::io::{AsyncRead, ReadBuf};

fn frame(payload: &[u8]) -> Vec<u8> {
    let mut bytes = (payload.len() as u32).to_le_bytes().to_vec();
    bytes.extend_from_slice(payload);
    bytes
}

/// Exercise every header/body boundary even when all data is already available.
struct OneByte<'a>(&'a [u8]);

impl AsyncRead for OneByte<'_> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        _: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if !self.0.is_empty() && buffer.remaining() > 0 {
            buffer.put_slice(&self.0[..1]);
            self.0 = &self.0[1..];
        }
        Poll::Ready(Ok(()))
    }
}

#[tokio::test]
async fn split_and_coalesced_unicode_frames_preserve_message_boundaries() {
    let messages = [
        json!({"id":2,"result":{"text":"Acme 🦀"}}),
        json!({"guid":"page","method":"close","params":{}}),
    ];
    let bytes: Vec<u8> = messages
        .iter()
        .flat_map(|value| frame(&serde_json::to_vec(value).unwrap()))
        .collect();
    for split in [false, true] {
        let stream: Box<dyn AsyncRead + Unpin> = if split {
            Box::new(OneByte(&bytes))
        } else {
            Box::new(bytes.as_slice())
        };
        let mut reader = Reader::new(stream);
        for message in &messages {
            assert_eq!(reader.read().await.unwrap().as_ref(), Some(message));
        }
        assert!(reader.read().await.unwrap().is_none());
        assert!(matches!(reader.read().await, Err(Error::Closed)));
    }
}

#[tokio::test]
async fn every_truncated_header_and_body_is_terminal() {
    let bytes = frame(br#"{"id":1}"#);
    for end in 1..bytes.len() {
        let mut reader = Reader::new(&bytes[..end]);
        assert!(
            matches!(reader.read().await, Err(Error::Truncated)),
            "end={end}"
        );
        assert!(matches!(reader.read().await, Err(Error::Closed)));
    }
}

#[tokio::test]
async fn zero_oversized_and_big_endian_lengths_fail_before_body_allocation() {
    for (header, empty) in [
        (0_u32.to_le_bytes(), true),
        (17_u32.to_le_bytes(), false),
        (u32::MAX.to_le_bytes(), false),
        (2_u32.to_be_bytes(), false),
    ] {
        let mut reader = Reader::with_limit(header.as_slice(), 16).unwrap();
        let result = reader.read().await;
        if empty {
            assert!(matches!(result, Err(Error::Empty)));
        } else {
            assert!(matches!(result, Err(Error::Oversized { limit: 16 })));
        }
        assert!(reader.payload.is_empty());
        assert!(matches!(reader.read().await, Err(Error::Closed)));
    }
}

#[tokio::test]
async fn malformed_json_utf8_and_non_objects_fail_without_payload_in_errors() {
    for payload in [
        b"{secret-token".as_slice(),
        b"{\"x\":\"\xff\"}",
        b"{}{}",
        b"null",
        b"[]",
        b"1",
        b"\"secret-token\"",
    ] {
        let bytes = frame(payload);
        let mut reader = Reader::new(bytes.as_slice());
        let error = reader.read().await.unwrap_err();
        assert!(matches!(
            error,
            Error::InvalidJson { .. } | Error::NotObject
        ));
        assert!(!error.to_string().contains("secret-token"));
        assert!(matches!(reader.read().await, Err(Error::Closed)));
    }
}

#[tokio::test]
async fn exact_limit_round_trips_and_local_refusal_does_not_dispatch() {
    let message = json!({"text":"é"});
    let payload = serde_json::to_vec(&message).unwrap();
    let mut writer = Writer::with_limit(Vec::new(), payload.len()).unwrap();
    assert!(matches!(
        writer.write(&json!({"text":"é!"})).await,
        Err(Error::Oversized { .. })
    ));
    assert!(matches!(
        writer.write(&json!([])).await,
        Err(Error::NotObject)
    ));
    assert!(writer.io.is_empty());
    writer.write(&message).await.unwrap();
    assert_eq!(writer.io, frame(&payload));
    let mut reader = Reader::with_limit(writer.io.as_slice(), payload.len()).unwrap();
    assert_eq!(reader.read().await.unwrap(), Some(message));
}

#[test]
fn limits_cannot_disable_or_increase_the_hard_cap() {
    for limit in [0, MAX_FRAME_BYTES + 1, usize::MAX] {
        assert!(matches!(
            Reader::with_limit(&b""[..], limit),
            Err(Error::InvalidLimit)
        ));
        assert!(matches!(
            Writer::with_limit(Vec::new(), limit),
            Err(Error::InvalidLimit)
        ));
    }
    assert!(Reader::with_limit(&b""[..], MAX_FRAME_BYTES).is_ok());
}

#[tokio::test]
async fn cancelled_reads_resume_at_every_partial_frame_boundary() {
    let message = json!({"id":7,"result":{"value":"Acme"}});
    let bytes = frame(&serde_json::to_vec(&message).unwrap());
    for split in 1..bytes.len() {
        let (host, mut peer) = tokio::io::duplex(bytes.len());
        peer.write_all(&bytes[..split]).await.unwrap();
        let mut reader = Reader::new(host);
        // Poll to pending deterministically, then drop the read future.
        let mut reading = Box::pin(reader.read());
        assert!(futures_util::poll!(reading.as_mut()).is_pending());
        drop(reading);
        peer.write_all(&bytes[split..]).await.unwrap();
        assert_eq!(reader.read().await.unwrap().as_ref(), Some(&message));
    }
}

#[tokio::test]
async fn cancelled_write_is_poisoned_and_never_replays_a_partial_frame() {
    let (host, mut peer) = tokio::io::duplex(2);
    let mut writer = Writer::new(host);
    let message = json!({"id":1,"method":"click"});
    let mut writing = Box::pin(writer.write(&message));
    assert!(futures_util::poll!(writing.as_mut()).is_pending());
    drop(writing);
    assert!(matches!(writer.write(&message).await, Err(Error::Closed)));
    drop(writer);
    let mut sent = Vec::new();
    peer.read_to_end(&mut sent).await.unwrap();
    assert_eq!(sent.len(), 2);
    assert_eq!(sent, frame(&serde_json::to_vec(&message).unwrap())[..2]);
}

#[tokio::test]
async fn io_errors_poison_both_lanes_without_replay() {
    let (host, peer) = tokio::io::duplex(16);
    drop(peer);
    let mut writer = Writer::new(host);
    assert!(matches!(writer.write(&json!({})).await, Err(Error::Io(_))));
    assert!(matches!(writer.write(&json!({})).await, Err(Error::Closed)));
    struct Failing;
    impl AsyncRead for Failing {
        fn poll_read(
            self: Pin<&mut Self>,
            _: &mut Context<'_>,
            _: &mut ReadBuf<'_>,
        ) -> Poll<io::Result<()>> {
            Poll::Ready(Err(io::Error::other("owned fixture failure")))
        }
    }
    let mut reader = Reader::new(Failing);
    assert!(matches!(reader.read().await, Err(Error::Io(_))));
    assert!(matches!(reader.read().await, Err(Error::Closed)));
}
