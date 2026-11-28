//! Bounded reply/event framing shared by simulator input, identity and capture workers.
use crate::error::{Error, Result};
use serde_json::Value;
#[cfg(target_os = "macos")]
use std::io::Write;
use tokio::io::{AsyncRead, AsyncReadExt};

pub(crate) struct Message {
    pub metadata: Value,
    pub payload: Vec<u8>,
}
const METADATA_LIMIT: usize = 65536;
const PAYLOAD_LIMIT: usize = 64 * 1024 * 1024;

#[cfg(target_os = "macos")]
pub(crate) fn write(output: &mut impl Write, metadata: &Value, payload: &[u8]) -> Result<()> {
    let metadata = serde_json::to_vec(metadata)
        .map_err(|_| Error::new("INTERNAL", "Cannot encode native worker message"))?;
    if metadata.len() > METADATA_LIMIT || payload.len() > PAYLOAD_LIMIT {
        return Err(Error::new(
            "OUTPUT_LIMIT",
            "Native worker message exceeds its limit",
        ));
    }
    output
        .write_all(&(metadata.len() as u32).to_be_bytes())
        .and_then(|()| output.write_all(&(payload.len() as u32).to_be_bytes()))
        .and_then(|()| output.write_all(&metadata))
        .and_then(|()| output.write_all(payload))
        .and_then(|()| output.flush())
        .map_err(|_| Error::new("TRANSPORT_IO", "Native worker consumer disconnected"))
}

pub(crate) async fn read(input: &mut (impl AsyncRead + Unpin)) -> Result<Message> {
    let mut header = [0; 8];
    input.read_exact(&mut header).await.map_err(|_| ended())?;
    let metadata = u32::from_be_bytes(header[..4].try_into().unwrap()) as usize;
    let payload = u32::from_be_bytes(header[4..].try_into().unwrap()) as usize;
    if metadata > METADATA_LIMIT || payload > PAYLOAD_LIMIT {
        return Err(Error::new(
            "OUTPUT_LIMIT",
            "Native worker message exceeds its limit",
        ));
    }
    let mut bytes = vec![0; metadata];
    input.read_exact(&mut bytes).await.map_err(|_| ended())?;
    let metadata = serde_json::from_slice(&bytes)
        .map_err(|_| Error::new("PLATFORM_PROTOCOL", "Invalid native worker message"))?;
    let mut payload = vec![0; payload];
    input.read_exact(&mut payload).await.map_err(|_| ended())?;
    Ok(Message { metadata, payload })
}

pub(super) fn ended() -> Error {
    Error::new("NATIVE_STREAM_ENDED", "Simulator native worker stopped")
}

pub(crate) fn failure(value: &Value) -> Error {
    let error = Error::new(
        value["code"].as_str().unwrap_or("PLATFORM_PROTOCOL"),
        value["message"].as_str().unwrap_or("Native worker failed"),
    );
    match value.get("details") {
        Some(details) => error.details(details.clone()),
        None => error,
    }
}
