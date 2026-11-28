//! Expiring single-use viewer authorization and private grant-broker I/O.
#[cfg(any(unix, windows))]
use super::Shared;
#[cfg(any(unix, windows))]
use crate::error::{Error, Result};
#[cfg(any(unix, windows))]
use crate::grants::issue_grant;
pub(super) use crate::grants::{Grants, consume_grant};
#[cfg(any(unix, windows))]
use serde_json::{Value, json};
#[cfg(unix)]
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[cfg(windows)]
pub(super) mod windows;

#[cfg(any(unix, windows))]
const REQUEST_LIMIT: usize = 4096;

#[cfg(any(unix, windows))]
fn reply(state: &Shared, bytes: &[u8]) -> Result<Value> {
    let request: Value =
        serde_json::from_slice(bytes).map_err(|_| Error::invalid("Invalid grant JSON"))?;
    let previews = state
        .previews
        .lock()
        .map_err(|_| Error::new("INTERNAL", "Media registry unavailable"))?;
    let preview = previews
        .get(request["streamId"].as_str().unwrap_or(""))
        .ok_or_else(|| Error::new("NOT_FOUND", "Preview is not active"))?;
    Ok(
        json!({"token":issue_grant(&preview.grants)?,"access":"read_only","expiresInMs":30000,"singleUse":true}),
    )
}

#[cfg(any(unix, windows))]
fn framed_reply(state: &Shared, bytes: &[u8]) -> Value {
    reply(state, bytes).unwrap_or_else(|error| json!({"ok":false,"error":error}))
}

#[cfg(unix)]
pub(super) async fn grant(mut socket: tokio::net::UnixStream, state: Shared) -> Result<()> {
    let mut header = [0; 4];
    socket
        .read_exact(&mut header)
        .await
        .map_err(|_| Error::invalid("Invalid grant request"))?;
    let length = u32::from_be_bytes(header) as usize;
    if length > REQUEST_LIMIT {
        return Err(Error::invalid("Grant request too large"));
    }
    let mut bytes = vec![0; length];
    socket
        .read_exact(&mut bytes)
        .await
        .map_err(|_| Error::invalid("Incomplete grant request"))?;
    let response = framed_reply(&state, &bytes);
    let bytes = response.to_string().into_bytes();
    socket
        .write_all(&(bytes.len() as u32).to_be_bytes())
        .await
        .map_err(|_| Error::new("TRANSPORT_IO", "Cannot return media grant"))?;
    socket
        .write_all(&bytes)
        .await
        .map_err(|_| Error::new("TRANSPORT_IO", "Cannot return media grant"))?;
    Ok(())
}
