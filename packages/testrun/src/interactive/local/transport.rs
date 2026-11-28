//! Same-user bounded IPC framing, requests and archive lookup after endpoint loss.
use crate::{
    error::{Error, Result},
    qa::journal::Archive,
};
use serde_json::{Value, json};
use std::{
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::Path,
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixStream,
};

pub(super) const MAX_BYTES: usize = 1024 * 1024;
pub(super) fn fence(input: &mut Value, epoch: Option<&str>) -> bool {
    if input["kind"] == "control_grant" {
        return true;
    }
    if let Some(epoch) = epoch {
        if input["ownerEpoch"] != epoch {
            return false;
        }
        input.as_object_mut().unwrap().remove("ownerEpoch");
    }
    true
}
pub(crate) fn check_owner(socket: &UnixStream) -> Result<()> {
    // SAFETY: read-only query of this process's effective user identity.
    let uid = unsafe { libc::geteuid() };
    if socket
        .peer_cred()
        .map_err(|_| Error::new("UNAUTHORIZED", "Cannot verify local peer"))?
        .uid()
        != uid
    {
        return Err(Error::new(
            "UNAUTHORIZED",
            "Retained session peer must have the same user identity",
        ));
    }
    Ok(())
}
pub(crate) async fn read(socket: &mut UnixStream) -> Result<Value> {
    let length = socket
        .read_u32()
        .await
        .map_err(|_| Error::new("TRANSPORT_IO", "Local request header interrupted"))?
        as usize;
    if length == 0 || length > MAX_BYTES {
        return Err(Error::invalid("Local message exceeds 1 MiB"));
    }
    let mut bytes = vec![0; length];
    socket
        .read_exact(&mut bytes)
        .await
        .map_err(|_| Error::new("TRANSPORT_IO", "Local message interrupted"))?;
    serde_json::from_slice(&bytes).map_err(|_| Error::invalid("Invalid local message"))
}
pub(crate) async fn write(socket: &mut UnixStream, value: &Value) -> Result<()> {
    let bytes = serde_json::to_vec(value)
        .map_err(|_| Error::new("INTERNAL", "Cannot encode local message"))?;
    if bytes.len() > MAX_BYTES {
        return Err(Error::new("OUTPUT_LIMIT", "Local response exceeds 1 MiB"));
    }
    socket
        .write_u32(bytes.len() as u32)
        .await
        .map_err(|_| Error::new("TRANSPORT_IO", "Local response header interrupted"))?;
    socket
        .write_all(&bytes)
        .await
        .map_err(|_| Error::new("TRANSPORT_IO", "Local response interrupted"))
}
pub(crate) async fn request(path: &Path, input: &Value) -> Result<Value> {
    let parent = path
        .parent()
        .ok_or_else(|| Error::invalid("Session endpoint has no directory"))?;
    let meta = std::fs::symlink_metadata(parent).map_err(|_| {
        Error::new(
            "SESSION_NOT_FOUND",
            "Retained session endpoint is unavailable",
        )
    })?;
    // SAFETY: read-only query of this process's effective user identity.
    if !meta.is_dir()
        || meta.uid() != unsafe { libc::geteuid() }
        || meta.permissions().mode() & 0o077 != 0
    {
        return Err(Error::new(
            "UNAUTHORIZED",
            "Session directory must be private and owned by this user",
        ));
    }
    let result = tokio::time::timeout(Duration::from_secs(if input["kind"]=="application"{240}else{125}), async {
        let mut socket = UnixStream::connect(path).await.map_err(|_| Error::new("SESSION_NOT_FOUND", "Cannot connect to retained session"))?;
        check_owner(&socket)?;
        let result = async { write(&mut socket, input).await?; read(&mut socket).await }.await;
        result.map_err(|error| if input["kind"] == "batch" {
            Error::new("BATCH_OUTCOME_UNKNOWN", format!("Reply interrupted for command {}; inspect status before further input", input["commandId"].as_str().unwrap_or("unknown"))).details(json!({"cause":error.code,"commandId":input["commandId"],"session":path,"recoveryAction":"Query status with the same session path and command ID; archived outcomes survive endpoint loss. Do not replay."}))
        } else { error })
    }).await.unwrap_or_else(|_| Err(Error::new(
        if input["kind"] == "batch" {"BATCH_OUTCOME_UNKNOWN"} else {"TRANSPORT_IO"},
        format!("Local reply deadline expired for command {}; inspect status before another input", input["commandId"].as_str().unwrap_or("unknown")),
    )));
    if result.is_err()
        && input["kind"] == "status"
        && let Some(command) = input["commandId"].as_str()
    {
        return archived_response(&Archive::at(parent), command);
    }
    result
}

pub(super) fn archived_response(archive: &Archive, command: &str) -> Result<Value> {
    Ok(json!({"ok":true,"commandId":command,"data":archive.status(command)?}))
}
