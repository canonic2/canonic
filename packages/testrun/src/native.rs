//! Simulator adapter composition; process commands, native FFI and workers have separate roles.
#[cfg(target_os = "macos")]
mod bridge;
mod commands;
#[cfg(target_os = "macos")]
mod input;
#[cfg(target_os = "macos")]
mod source;

#[cfg(target_os = "macos")]
pub(crate) use bridge::peer_alive;
#[cfg(all(test, target_os = "macos"))]
pub(crate) use bridge::process_peer;
#[cfg(target_os = "macos")]
pub(crate) use bridge::verify_video_track;
#[cfg(target_os = "macos")]
pub use bridge::{Capture, accessibility};
pub(crate) use commands::incarnation;
pub use commands::{developer_directory, snapshot};
#[cfg(target_os = "macos")]
pub(crate) use input::run as worker_input;
#[cfg(target_os = "macos")]
pub(crate) use source::run as worker_source;

#[cfg(target_os = "macos")]
pub(crate) fn worker_identity(developer: &str, uuid: &str) -> crate::error::Result<()> {
    let result = bridge::identity_connection(developer, uuid);
    let message = match result {
        Ok(identity) => serde_json::json!({"event":"identity","identity":identity}),
        Err(error) => serde_json::json!({"event":"error","error":error}),
    };
    crate::simulator::ipc::write(&mut std::io::stdout().lock(), &message, &[])
}

#[cfg(target_os = "macos")]
fn startup<T>(result: crate::error::Result<T>) -> crate::error::Result<T> {
    result.map_err(|error| {
        let report=crate::simulator::ipc::write(&mut std::io::stdout().lock(),&serde_json::json!({"event":"error","error":error}),&[]);
        match report {
            Ok(())=>error,
            Err(report_error)=>{let details=serde_json::json!({"cause":error.details,"startupReportError":report_error.code});error.details(details)}
        }
    })
}
