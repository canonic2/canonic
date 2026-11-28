//! Private endpoint identity, bind diagnostics and ownership-safe removal.
use crate::error::{Error, Result};
use serde_json::{Value, json};
use std::{os::unix::fs::MetadataExt, path::PathBuf, time::Duration};

pub(super) fn endpoint_bind_error(error: std::io::Error) -> Error {
    let (code, state, message) = match error.kind() {
        std::io::ErrorKind::PermissionDenied => (
            "HOST_ACCESS_REQUIRED",
            "host_access_denied",
            "The QA host must allow Testrun to create local control sockets",
        ),
        std::io::ErrorKind::AddrInUse => (
            "SESSION_ENDPOINT_CONFLICT",
            "endpoint_in_use",
            "The new controller endpoint is already in use; its existing owner was preserved",
        ),
        _ => (
            "HOST_IO",
            "endpoint_unavailable",
            "Cannot bind owned CLI endpoint",
        ),
    };
    Error::new(code, message).details(json!({
        "blockingState":state,"phase":"control_endpoint_bind",
        "osError":error.raw_os_error(),"ioKind":format!("{:?}",error.kind()),
        "deviceState":"not_checked","mediaAllocated":false,
        "requiresHostAccess":code == "HOST_ACCESS_REQUIRED"
    }))
}
pub(crate) struct Endpoint {
    pub(crate) directory: PathBuf,
    pub(crate) socket: PathBuf,
    pub(crate) identity: Option<(u64, u64)>,
}
impl Endpoint {
    pub(super) fn owned(&self) -> bool {
        self.identity.is_some_and(|identity| {
            std::fs::symlink_metadata(&self.socket).is_ok_and(|m| (m.dev(), m.ino()) == identity)
        })
    }
    pub(crate) async fn lost(&self) -> Result<Value> {
        let mut interval = tokio::time::interval(Duration::from_secs(1));
        loop {
            interval.tick().await;
            if !self.owned() {
                return Err(Error::new(
                    "SESSION_ENDPOINT_LOST",
                    "Retained endpoint was removed or replaced; closing its owned resources",
                ));
            }
        }
    }
}
impl Drop for Endpoint {
    fn drop(&mut self) {
        if self.owned() {
            let _ = std::fs::remove_file(&self.socket);
        }
        let _ = std::fs::remove_dir(&self.directory);
    }
}
