//! Testrun composition: mobile adapters, owned QA state and MCP transport.
#![deny(rustdoc::broken_intra_doc_links)]

mod agent_setup;
pub mod android;
mod app_lifecycle;
mod applications;
mod artifacts;
pub mod browser;
mod builds;
mod capture;
mod catalog;
pub mod cli;
mod configuration;
mod control;
mod device;
pub mod device_media;
mod device_setup;
mod diagnostics;

/// Runs the private bounded diagnostic decoder entry point, when selected.
#[doc(hidden)]
pub fn diagnostic_parser_entry() -> Option<std::process::ExitCode> {
    diagnostics::analysis::worker::entry()
}
mod documentation;
pub mod error;
mod grants;
#[cfg(target_os = "macos")]
mod hid;
mod host_setup;
#[cfg(test)]
#[path = "../tests/fixtures/browser_http.rs"]
mod http_fixtures;
mod interactive;
mod keyboard;
mod lease;
pub(crate) mod managed;
pub mod media;
mod mobile;
mod native;
#[cfg_attr(
    windows,
    allow(
        dead_code,
        reason = "Android network mutations use the Unix retained controller"
    )
)]
mod network;
mod pagination;
#[cfg_attr(
    windows,
    allow(
        dead_code,
        reason = "Mobile playback mutations use the Unix retained controller"
    )
)]
mod playback;
pub mod process;
mod project;
pub mod protocol;
pub mod qa;
mod recording;
mod schema;
mod simulator;
mod skills;
mod targets;
pub mod ui;
pub mod usbmux;
#[cfg(windows)]
mod windows_ipc;
