//! Browser contracts and supervised native Chrome and Firefox/WebKit lifetimes.
//!
//! Library sessions receive trusted owner incarnations from composition. These
//! Preparation acquires private pinned dependencies, and the public surface
//! composes library sessions inside the shared CLI/MCP named-session owner.
//! Native Chrome owns its CDP connection and process independently of Playwright;
//! its public scope includes lifecycle, navigation and document-scoped reads.

pub mod activity;
pub(crate) mod api;
mod artifacts;
mod attachment;
#[cfg(feature = "acceptance-browser-public")]
#[doc(hidden)]
pub use preparation::checkpoint_artifacts;
pub mod audits;
pub mod authentication;
pub mod automation_debugger;
mod base_url;
mod bindings;
mod capture;
pub mod cdp;
mod client_certificate_tunnel;
pub mod clock;
pub mod connection;
mod context_proxy;
pub mod contract;
pub mod credentials;
pub mod debugging;
pub mod developer_tools;
mod devices;
pub mod diagnostic_arguments;
pub mod diagnostics;
pub mod directory;
pub mod dispatcher;
pub mod driver;
pub mod evaluation;
pub mod events;
pub mod evidence;
pub mod extensions;
mod frames;
pub mod framework;
mod har;
pub mod heap;
pub mod http;
pub mod input;
mod investigation;
mod journal;
#[cfg(all(test, feature = "acceptance-browser-lifecycle"))]
mod journeys_native;
mod keyboard;
pub mod lifecycle;
pub mod network;
pub mod objects;
mod owned;
pub mod page;
pub mod performance;
pub(crate) mod preparation;
pub mod preview;
mod processes;
mod profiles;
pub mod profiling;
pub mod pwa;
mod reading;
pub mod recording;
mod routing;
pub(crate) mod runtime;
pub mod schema;
mod scripts;
pub mod session;
pub mod settings;
mod socket_routes;
#[cfg(test)]
#[path = "../tests/fixtures/browser_http_socks.rs"]
pub(crate) mod socks_fixture;
pub mod storage;
pub(crate) mod surface;
pub mod tracing;
pub mod transfer;
pub mod transport;
mod values;
pub mod webmcp_debug;

/// Integration baseline for the unchanged Playwright driver, not a release bundle.
pub const DRIVER_VERSION: &str = "1.64.1";
pub mod clipboard;
